//! `ui_shell` 单测（原单文件尾部 `mod tests` 整段平移）。

use std::path::PathBuf;

use bevy::tasks::AsyncComputeTaskPool;

use super::*;

use crate::state::{PanelState, ViewState, WorldProjection};
use crate::theme::ui_tokens::UiTheme;
use crate::ui_command_palette::CommandId;
use wv_render::terrain_view::TerrainTexture;

#[test]
fn immersive_toggle_flips_state() {
    // §6.4：沉浸模式只隐藏面板，不隐藏时间轴与日期。
    let mut p = PanelState::default();
    assert!(!p.immersive);
    p.immersive = !p.immersive;
    assert!(p.immersive);
}

#[test]
fn theme_toggle_round_trips() {
    let mut c = UiShellConfig::default();
    assert_eq!(c.theme, UiTheme::Dark);
    c.theme = c.theme.toggled();
    assert_eq!(c.theme, UiTheme::Light);
    c.theme = c.theme.toggled();
    assert_eq!(c.theme, UiTheme::Dark);
}

#[test]
fn open_project_task_is_idle_by_default() {
    // 没有 `spawn()` 过的任务，轮询必须给 `None`，否则「打开工程」会被
    // 默认状态误触发。
    let mut t = OpenProjectTask::default();
    assert!(t.task.is_none());
    assert!(t.take_if_finished().is_none());
}

#[test]
fn open_project_task_does_not_block_when_dialog_is_still_open() {
    // 模拟「对话框还开着」——用一个 pending 的 future 装进 task 槽位。
    // `take_if_finished` 必须立刻返回 `None`，**不能**阻塞等它完成。
    // `get_or_init`：裸单测没有 TaskPoolPlugin，`get()` 会 panic；
    // 应用运行时 Bevy 已初始化，这里 init 是幂等的。
    let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let pending = pool.spawn(async move { std::future::pending::<Option<PathBuf>>().await });
    let mut t = OpenProjectTask {
        task: Some(pending),
        purpose: DialogPurpose::default(),
        wanted: None,
    };
    // 此刻 future 永远 pending，is_finished 一定为 false。
    assert!(
        t.take_if_finished().is_none(),
        "未关闭的对话框不应让 UI 线程阻塞"
    );
}

/// 给分发器测试搭一个最小 `World`：与真实装配同名的资源 + 一条待执行命令
/// （落在 `CommandPalette::pending`，即快捷键 / 命令面板的入队口）。
///
/// `AsyncComputeTaskPool::get_or_init`：分发器走对话框路径时会 `get()`，
/// 裸单测没有 TaskPoolPlugin 未初始化会 panic；应用运行时 Bevy 已初始化，
/// 这里 init 是幂等的。
///
/// UX-001（2026-10-29）：dirty 的 CloseProject 在守卫处被拦——命令不入
/// 分发、确认框接管（`CloseConfirm::pending` 置位），工程原样保留。
#[test]
fn dirty_close_project_is_intercepted_into_confirm_dialog() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(Some("D:/old/project"), true, CommandId::CloseProject);
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");

    let confirm = world.resource::<crate::ui_shell::close_confirm::CloseConfirm>();
    assert_eq!(
        confirm.pending,
        Some(crate::ui_shell::close_confirm::CloseAction::CloseProject),
        "dirty 关闭应转入确认框"
    );
    let projection = world.resource::<WorldProjection>();
    assert!(
        projection.project_dir.is_some() && projection.dirty,
        "守卫拦截后工程必须原样保留（不得静默丢弃）"
    );
}

#[allow(dead_code)] // helper used by tests; warnings emitted in non-test lib build
fn dispatcher_world(project_dir: Option<&str>, dirty: bool, pending: CommandId) -> World {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let mut world = World::new();
    world.init_resource::<CommandPalette>();
    world.init_resource::<crate::ui::ProjectMenuState>();
    world.init_resource::<PanelState>();
    world.init_resource::<UiShellConfig>();
    world.init_resource::<ViewState>();
    world.init_resource::<crate::state::PlaybackState>();
    world.init_resource::<crate::state::BrushState>();
    world.init_resource::<DrillState>();
    world.init_resource::<RecentProjects>();
    world.init_resource::<ShellNotice>();
    world.init_resource::<crate::edit::editing::EditHistory>();
    world.init_resource::<OpenProjectTask>();
    // UX-001：dispatch 的 CloseProject 守卫读它——测试世界同样要装配。
    world.init_resource::<crate::ui_shell::close_confirm::CloseConfirm>();
    world.init_resource::<crate::state::ProjectDiagnostics>();
    world.init_resource::<crate::ui::NewMapDialog>();
    world.init_resource::<crate::ui::NewProjectDialog>();
    world.init_resource::<crate::state::SelectionState>();
    // 2026-10-19 地图工具条：分发器互斥工具需要测量 / 街道会话态。
    world.init_resource::<crate::state::MeasureState>();
    world.init_resource::<wv_render::streets::StreetEditorState>();
    // 2026-10-19 单格编辑工具（[ADR-0040]）:分发器需要 EditCellState 才能
    // 运行 `apply_pending_commands` 的 SelectMapTool(EditCell) 分支。
    world.init_resource::<crate::state::EditCellState>();
    world.init_resource::<wv_render::state::BiomeBoundaryState>();
    world.init_resource::<crate::ui::ContinentDialog>();
    // 「地图属性」对话框（2026-10-27）：分发器签名新增依赖，fixture 同款补齐。
    world.init_resource::<crate::ui::MapPropertiesDialog>();
    // ADR-0018：UI 文案语言；默认 `Locale::Zh`（与 GUI 启动默认值一致）。
    world.init_resource::<crate::i18n::CurrentLocale>();
    world.init_resource::<LayoutFile>();
    use bevy::asset::Assets;
    use bevy::image::Image;
    world.init_resource::<Assets<Image>>();
    world.insert_resource(WorldProjection {
        world: wv_core::World::default(),
        project_dir: project_dir.map(str::to_owned),
        dirty,
        load_error: None,
        selection: None,
    });
    // 应用屏状态与加载排队（2026-10-22）：分发器签名新增的依赖。
    // 初值按 `project_dir` 推导，与生产端 `current_screen` 同口径——
    // 测试装的是「稳定世界」，永远不处于加载中。
    world.insert_resource(crate::app_screen::AppScreen {
        has_project: project_dir.is_some(),
        loading: false,
        load_error: false,
    });
    world.init_resource::<crate::project_loader::PendingProjectLoad>();
    world.init_resource::<crate::project_loader::ProjectLoadTask>();
    world.insert_resource(TerrainTexture {
        handle: Default::default(),
        size: 1,
        height: 1,
        res: 1,
        dirty: None,
        chunks_dirty: None,
        field_dirty: None,
        last_mode: None,
        last_soften: false,
        last_theme_id: wv_render::theme::theme_pack::builtin::DEFAULT.id,
        last_hypsometric: 0.0,
        last_map: None,
    });
    world.insert_resource(Time::<()>::default());
    // CellMap 工具会话态（[ADR-0052] D8-A）：`apply_pending_commands`
    // 的撤销/重做白闪分支要读写它。生产端在 `app_bootstrap` 注册；测试
    // fixture 必须同款补齐，否则 `SystemParamValidationError { Resource
    // does not exist }`。（泛洪工具态已随 ADR-0086 退役，不再进分发器。）
    world.init_resource::<wv_world_cell_map::cell_flash::CellFlashState>();
    // ADR-0057：点选工具会话态 + 统一选区位图——分发器互斥分支与
    // 选区保留测试都要读写它们（漏 init 即 SystemParamValidationError）。
    world.init_resource::<crate::state::PointSelectState>();
    world.init_resource::<crate::state::CellSelection>();
    // 线段 POI 工具会话态（ADR-0070 D4）：分发器 SelectMapTool(LinePoi)
    // 分支读写（漏 init 同上）。
    world.init_resource::<crate::state::LinePoiState>();
    // ADR-0071：地标放置会话态——分发器 SelectMapTool(Landmark) 分支要写。
    world.init_resource::<crate::state::LandmarkToolState>();
    world.resource_mut::<CommandPalette>().pending = Some(pending);
    world
}

/// 无工程闸门（2026-10-14）：没有打开任何工程时，「保存」不再排队「选保存
/// 位置」对话框（那是旧的「内存空白工程」链路），而是挡下并给出下一步
/// 指引——欢迎页是唯一入口。有工程在手时才走原有的 dirty 保存路径。
#[test]
fn save_without_project_shows_guidance_instead_of_a_dialog() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(None, true, CommandId::Save);
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    let task = world.resource::<OpenProjectTask>();
    assert!(
        task.wanted.is_none() && task.task.is_none(),
        "无工程时不得发起 / 排队任何目录对话框"
    );
    let notice = world.resource::<ShellNotice>();
    assert!(
        notice.is_error
            && notice
                .text
                .as_deref()
                .unwrap_or("")
                .contains("新建或打开工程"),
        "应给出「先新建或打开工程」的指引，实得 {:?}",
        notice.text
    );
}

#[test]
fn open_project_command_queues_open_dialog() {
    // 「打开工程…」同样只排队意图：发起收口在轮询系统（见
    // `OpenProjectTask::request` 的文档）。
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(None, false, CommandId::OpenProject);
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    let task = world.resource::<OpenProjectTask>();
    assert_eq!(
        task.wanted,
        Some(DialogPurpose::Open),
        "打开工程应排队「打开」对话框意图"
    );
    assert!(task.task.is_none(), "分发器不得直接发起 rfd 对话框");
}

#[test]
fn select_brush_tool_command_picks_tool_and_activates_brush() {
    // 命令面板 / 快捷键选工具的唯一路径：索引对 `BrushTool::ALL`，
    // 选中即激活笔刷。越界索引不 panic。
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(1));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    let brush = world.resource::<crate::state::BrushState>();
    assert_eq!(
        brush.tool,
        crate::state::BrushTool::MountainRange,
        "索引 1 应选中山脉"
    );
    assert!(brush.enabled, "选工具 = 激活笔刷");

    // 越界索引：忽略而不改状态（不 panic 也不误切）。
    let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(99));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    let brush = world.resource::<crate::state::BrushState>();
    assert_eq!(
        brush.tool,
        crate::state::BrushTool::PaintKind,
        "越界不改工具"
    );
    assert!(!brush.enabled, "越界不激活笔刷");
}

/// 地图工具条互斥（2026-10-19）：开抓手 = 关笔刷 / 选区 / 测量 / 街道；
/// 再点一次同一工具 = 关闭。左键永远只有一个消费者。
#[test]
fn map_tool_selection_is_mutually_exclusive_and_toggles_off() {
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;

    // 先激活笔刷（与工具条 B 同语义的既有命令）。
    let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(0));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(world.resource::<crate::state::BrushState>().enabled);

    // 点 H：开抓手，笔刷让位。
    world.resource_mut::<CommandPalette>().pending = Some(CommandId::SelectMapTool(MapToolId::Pan));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(world.resource::<ViewState>().pan_tool, "H 应开启抓手工具");
    assert!(
        !world.resource::<crate::state::BrushState>().enabled,
        "开抓手应关掉笔刷（互斥）"
    );
    assert!(
        !world.resource::<crate::state::SelectionState>().enabled,
        "开抓手应关掉选区"
    );

    // 再点 H：关闭抓手。
    world.resource_mut::<CommandPalette>().pending = Some(CommandId::SelectMapTool(MapToolId::Pan));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world.resource::<ViewState>().pan_tool,
        "再次点击同一工具应关闭"
    );

    // 点 L：折线测量，且选区仍关闭。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::MeasureLine));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert_eq!(
        world.resource::<crate::state::MeasureState>().mode,
        crate::state::MeasureMode::Line,
        "L 应进入折线测量"
    );
    assert!(!world.resource::<crate::state::SelectionState>().enabled);
}

#[test]
fn toggle_grid_command_flips_the_view_flag() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(None, false, CommandId::ToggleGrid);
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        world.resource::<ViewState>().grid_lines,
        "默认关 → 执行一次应为开"
    );
}

/// 「清除全部测量」（2026-10-23 用户需求）：工具条的清除钮 / 命令面板
/// 都排 `CommandId::ClearMeasurements`，分发器必须真的把当前图的 overlay
/// 清空并给出回执——否则按钮点了没反应（与 `toggle_grid` 同级的接线回归）。
#[test]
fn clear_measurements_command_empties_the_current_map_overlay() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(Some("D:/some/project"), false, CommandId::ClearMeasurements);
    let map = wv_core::Id::new_unchecked("map_root");
    // 预置「一张图 + 两条测量」，并把 drill 的当前图指向它。
    world.resource_scope::<WorldProjection, _>(|w, mut projection| {
        projection.world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            name: "root".into(),
            ..Default::default()
        });
        let overlay = projection.world.measurement_of_mut(&map);
        for (i, x) in [0.0_f32, 10.0].into_iter().enumerate() {
            overlay
                .add(
                    wv_core::Measurement::polyline(vec![
                        wv_core::Point::new(x, 0.0),
                        wv_core::Point::new(x, 5.0),
                    ])
                    .with_id(wv_core::Id::new_unchecked(format!("measure_{i}"))),
                )
                .expect("预置测量");
        }
        w.resource_mut::<DrillState>()
            .ensure_root(&projection.world);
        let cur = w
            .resource::<DrillState>()
            .current_node(&projection.world)
            .map(|n| n.id.clone());
        assert_eq!(cur, Some(map.clone()), "drill 应停在预置图");
    });

    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");

    let projection = world.resource::<WorldProjection>();
    let left = projection
        .world
        .measurement_of(&map)
        .map(|o| o.measurements.len())
        .unwrap_or(0);
    assert_eq!(left, 0, "ClearMeasurements 应清空当前图全部测量");
    let notice = world.resource::<ShellNotice>();
    assert!(
        !notice.is_error
            && notice
                .text
                .as_deref()
                .unwrap_or("")
                .contains("已清除 2 条测量"),
        "应给出清除条数回执，实得 {:?}",
        notice.text
    );
}

#[test]
fn project_menu_pending_command_reaches_the_dispatcher() {
    // 回归 1：工程菜单的点击 / 回车写的是 `ProjectMenuState::pending`，而唯一
    // 分发器曾只消费 `CommandPalette::pending`——菜单里的「打开工程 / 新建 /
    // 关闭」全部悬空（点击无反应）。
    // 回归 2：换世界后必须走 `adopt_world_snapshot` 接管纹理 / 相机 / 播放，
    // 否则「关闭像没点」——旧底图原封不动挂在屏上。
    // 本测试用最小 World 跑真实分发器：菜单队列里的 CloseProject 必须被执行
    //（世界被清空 + 状态栏回执 + 纹理整体重建标记）。
    //（2026-10-14 起用 CloseProject 验证 adopt：NewProject 改为只弹对话框、
    // 不再直接换世界，adopt 的菜单级回归覆盖转移到 CloseProject 上。）
    use crate::ui::ProjectMenuState;
    use bevy::asset::Assets;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::image::Image;
    use wv_render::terrain_view::TerrainTexture;

    let mut world = World::new();
    world.init_resource::<CommandPalette>();
    world.init_resource::<ProjectMenuState>();
    world.init_resource::<PanelState>();
    world.init_resource::<UiShellConfig>();
    world.init_resource::<ViewState>();
    world.init_resource::<crate::state::PlaybackState>();
    world.init_resource::<crate::state::BrushState>();
    world.init_resource::<DrillState>();
    world.init_resource::<RecentProjects>();
    world.init_resource::<ShellNotice>();
    world.init_resource::<crate::edit::editing::EditHistory>();
    world.init_resource::<OpenProjectTask>();
    // UX-001：dispatch 的 CloseProject 守卫读它——测试世界同样要装配。
    world.init_resource::<crate::ui_shell::close_confirm::CloseConfirm>();
    world.init_resource::<crate::state::ProjectDiagnostics>();
    world.init_resource::<crate::ui::NewMapDialog>();
    world.init_resource::<crate::ui::NewProjectDialog>();
    world.init_resource::<crate::state::SelectionState>();
    // 2026-10-19 地图工具条：分发器互斥工具需要测量 / 街道会话态。
    world.init_resource::<crate::state::MeasureState>();
    world.init_resource::<wv_render::streets::StreetEditorState>();
    // 2026-10-19 单格编辑工具（[ADR-0040]）:分发器需要 EditCellState 才能
    // 运行 `apply_pending_commands` 的 SelectMapTool(EditCell) 分支。
    world.init_resource::<crate::state::EditCellState>();
    world.init_resource::<wv_render::state::BiomeBoundaryState>();
    world.init_resource::<crate::ui::ContinentDialog>();
    // 「地图属性」对话框（2026-10-27）：分发器签名新增依赖，fixture 同款补齐。
    world.init_resource::<crate::ui::MapPropertiesDialog>();
    // ADR-0018：UI 文案语言；默认 `Locale::Zh`（与 GUI 启动默认值一致）。
    world.init_resource::<crate::i18n::CurrentLocale>();
    world.init_resource::<LayoutFile>();
    world.init_resource::<Assets<Image>>();
    world.insert_resource(WorldProjection {
        world: wv_core::World::default(),
        project_dir: Some("D:/some/project".into()),
        dirty: true,
        load_error: None,
        selection: None,
    });
    // UX-001：dirty 的 CloseProject 会被确认框守卫拦下——本测试的靶子是
    // 「菜单队列命令到达分发器并执行 adopt」，故预置「不保存」的单次放行。
    world
        .resource_mut::<crate::ui_shell::close_confirm::CloseConfirm>()
        .force_close = true;
    // 应用屏状态与加载排队（2026-10-22）：分发器签名新增的依赖。
    world.insert_resource(crate::app_screen::AppScreen {
        has_project: true,
        loading: false,
        load_error: false,
    });
    world.init_resource::<crate::project_loader::PendingProjectLoad>();
    world.init_resource::<crate::project_loader::ProjectLoadTask>();
    world.insert_resource(TerrainTexture {
        handle: Default::default(),
        size: 1,
        height: 1,
        res: 1,
        dirty: None,
        chunks_dirty: None,
        field_dirty: None,
        last_mode: None,
        last_soften: false,
        last_theme_id: wv_render::theme::theme_pack::builtin::DEFAULT.id,
        last_hypsometric: 0.0,
        last_map: None,
    });
    world.insert_resource(Time::<()>::default());
    // CellMap 工具会话态（[ADR-0052] D8-A）：与 `dispatcher_world`
    // 同款补齐——`apply_pending_commands` 现在读写这些资源。
    world.init_resource::<wv_world_cell_map::cell_flash::CellFlashState>();
    // ADR-0057：点选工具会话态 + 统一选区位图（分发器互斥分支要读写）。
    world.init_resource::<crate::state::PointSelectState>();
    world.init_resource::<crate::state::CellSelection>();
    // 线段 POI 工具会话态（ADR-0070 D4）+ 地标放置会话态（ADR-0071）：
    // 分发器 SelectMapTool 两分支各写各的（漏 init 即 SystemParamValidationError）。
    world.init_resource::<crate::state::LinePoiState>();
    world.init_resource::<crate::state::LandmarkToolState>();
    world.resource_mut::<ProjectMenuState>().pending = Some(CommandId::CloseProject);

    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");

    let projection = world.resource::<WorldProjection>();
    assert!(
        projection.project_dir.is_none() && !projection.dirty,
        "菜单 pending 的 CloseProject 未被执行（菜单命令悬空回归）"
    );
    assert!(
        world.resource::<ShellNotice>().text.is_some(),
        "命令执行后状态栏应有回执"
    );
    let tex = world.resource::<TerrainTexture>();
    assert!(
        tex.dirty.is_some(),
        "CloseProject 后纹理未整体重建（adopt_world_snapshot 未被触发：地图停在旧世界）"
    );
    // 初始手工构造的纹理 `last_mode` 为 None；adopt 内的
    // `replace_terrain_texture` 必然写入 `Some(mode)`。
    assert!(
        tex.last_mode.is_some(),
        "adopt 未按新世界重建纹理（last_mode 仍为 None）"
    );
}

/// 2026-10-14 改版：`CommandId::NewProject` 只负责弹「新建工程」对话框，
/// 不再默默清世界 / 标脏（创建与落盘在对话框确认后发生）。无工程闸门下的
/// 门禁不拦它——它就是入口之一。
#[test]
fn new_project_command_opens_the_dialog_not_a_blank_world() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(None, false, CommandId::NewProject);
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        world.resource::<crate::ui::NewProjectDialog>().open,
        "NewProject 应打开「新建工程」对话框"
    );
    let projection = world.resource::<WorldProjection>();
    assert!(
        !projection.dirty && projection.project_dir.is_none(),
        "对话框未确认前不得改工作副本（世界替换 / 落盘都在确认后）"
    );
}

/// 无工程闸门：`CommandId::NewMap`（同样改数据）在无工程时必须被挡下，
/// 对话框不打开、世界不动、回执给指引。
#[test]
fn new_map_is_blocked_until_a_project_is_open() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = dispatcher_world(None, false, CommandId::NewMap);
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world.resource::<crate::ui::NewMapDialog>().open,
        "无工程时不得打开「新建地图」对话框"
    );
    let notice = world.resource::<ShellNotice>();
    assert!(
        notice.is_error
            && notice
                .text
                .as_deref()
                .unwrap_or("")
                .contains("新建或打开工程"),
        "应给出指引，实得 {:?}",
        notice.text
    );
}

/// 2026-10-14 用户反馈「新建工程后笔刷涂大地图没变化」的根本原因（旧版
/// NewProject 直接换世界）：换世界后 `view.current_map` 没同步到 drill 镜像
/// ——笔触按陈旧 id 提交命令，落到不存在的图上。
///
/// NewProject 改版后 adopt 的换世界路径由 CloseProject 代表验证：预置一个
/// 陈旧的 `view.current_map`，断言 adopt 路径把它清回 drill 的镜像；且若
/// drill.current() 指向某张图，那张图必须真实存在。
#[test]
fn adopt_world_syncs_view_current_map_to_drill_current() {
    use bevy::ecs::system::RunSystemOnce;
    // dirty=false：本测试的靶子是 adopt 的镜像同步，不是关闭确认（后者有
    // 专门测试）。dirty=true 的执行路径见 force_close 放行测试。
    let mut world = dispatcher_world(Some("D:/old/project"), false, CommandId::CloseProject);
    // 预置陈旧镜像：关闭工程后世界为空，正确的同步结果是把它清空。
    world.resource_mut::<ViewState>().current_map = "stale_map".into();
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");

    let drill = world.resource::<DrillState>();
    let view = world.resource::<ViewState>();
    // adopt 路径必须把 view.current_map 与 drill.current 对齐。
    assert_eq!(
        view.current_map,
        drill
            .current()
            .map(|i| i.as_str().to_owned())
            .unwrap_or_default(),
        "adopt 路径必须把 view.current_map 同步到 drill.current()，\
         否则笔触会按陈旧 id 提交"
    );
    // 若 drill.current 指向某 id，那张图必须存在。
    if let Some(id) = drill.current() {
        let exists = world
            .resource::<WorldProjection>()
            .world
            .maps
            .iter()
            .any(|m| &m.id == id);
        assert!(exists, "drill.current() 指向的图应真实存在：{id:?}");
    }
}

// ── 点选工具（ADR-0057 补记 6，原「单选模式」）─────────────────

/// **PointSelect 与 EditCell 是两件工具、两个独立 Resource（ADR-0057 补记 6）。**
///
/// 这条测的是分发器的三件事：
/// 1. `PointSelect` 开起来 = `point_select.tool_active`，且 EditCell 保持关；
/// 2. 切到 `EditCell` = 关掉 PointSelect、开 `edit_cell.tool_active`；
/// 3. 对**当前工具**再点一次 = 关闭（不是切换）。
#[test]
fn point_select_and_edit_cell_are_independent_tools() {
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;

    let mut world = dispatcher_world(
        None,
        false,
        CommandId::SelectMapTool(MapToolId::PointSelect),
    );
    // 工作区白名单（ADR-0080）：EditCell 是 CellMap 工具，夹具进 Topographic
    // 档（点选三档公共，不受影响）。
    world.resource_mut::<ViewState>().mode = wv_render::state::ViewMode::Topographic;
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let p = world.resource::<crate::state::PointSelectState>();
        assert!(p.tool_active, "点选工具应激活");
        assert!(
            !world
                .resource::<wv_render::state::EditCellState>()
                .tool_active,
            "点选开启时 EditCell 应保持关闭"
        );
    }

    // 切到 EditCell：互斥——PointSelect 关、EditCell 开。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        assert!(
            world
                .resource::<wv_render::state::EditCellState>()
                .tool_active,
            "切到 EditCell 后工具应激活"
        );
        assert!(
            !world
                .resource::<crate::state::PointSelectState>()
                .tool_active,
            "切到 EditCell 后点选应关闭（互斥）"
        );
    }

    // 对当前工具再点一次 → 关闭。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world
            .resource::<wv_render::state::EditCellState>()
            .tool_active,
        "再次点击当前工具应关闭"
    );
}

/// 线段 POI 分裂按钮的换类语义（ADR-0070 D4，与 `Select(shape)` 同口径）：
/// 同类开着再点 = 关；异类开着再点 = 换类**不关工具**；面板随工具开关
/// （开 = 开 `Panel::LinePoi`，关 = 关面板——设置承载在 T11 的面板里）。
#[test]
fn line_poi_split_menu_switches_kind() {
    use crate::state::Panel;
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;
    use wv_core::distance::LineKind;

    // 开「河」：tool_active + kind=River + 面板开。
    let mut world = dispatcher_world(
        None,
        false,
        CommandId::SelectMapTool(MapToolId::LinePoi(LineKind::River)),
    );
    // 工作区白名单（ADR-0080）：LinePoi 只在 Map 档可激活——夹具进 GoogleMap。
    world.resource_mut::<ViewState>().mode = wv_render::state::ViewMode::GoogleMap;
    // 档位前置（T10 评审 minor ② 兜底落地后）：线段 POI 仅 Edit 档可激活，
    // 缺省 ViewState scale=1.0 是 Region 档——本测激活语义，先放大进 Edit 档。
    world.resource_mut::<ViewState>().scale = 0.5;
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let lp = world.resource::<crate::state::LinePoiState>();
        assert!(lp.tool_active, "选河应激活线段 POI 工具");
        assert_eq!(lp.kind, LineKind::River, "默认类 = 河");
    }
    assert!(
        world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "开工具应打开河流和道路面板"
    );

    // 弹层选「道路」：换类 = 换参数，工具保持开启（分发器 already_on 判据
    // 是「同款才关」）。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::LinePoi(LineKind::Road)));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let lp = world.resource::<crate::state::LinePoiState>();
        assert!(lp.tool_active, "河→路换类不得关掉工具");
        assert_eq!(lp.kind, LineKind::Road, "弹层选路应把 kind 切到 Road");
    }
    assert!(
        world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "换类后面板应保持打开"
    );

    // 同款（路）再点：关闭工具，面板随关。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::LinePoi(LineKind::Road)));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world.resource::<crate::state::LinePoiState>().tool_active,
        "同类再点应关闭工具"
    );
    assert!(
        !world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "关工具应关闭河流和道路面板"
    );

    // 互斥：再开河后切抓手，线段 POI 工具与面板一并收掉。
    world.resource_mut::<CommandPalette>().pending = Some(CommandId::SelectMapTool(
        MapToolId::LinePoi(LineKind::River),
    ));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    world.resource_mut::<CommandPalette>().pending = Some(CommandId::SelectMapTool(MapToolId::Pan));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world.resource::<crate::state::LinePoiState>().tool_active,
        "切抓手应关掉线段 POI 工具（互斥）"
    );
    assert!(
        !world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "切走工具应关闭河流和道路面板"
    );
}

/// 命令面板 / 快捷键路径的档位兜底（T10 评审 minor ②）：非 Edit 档发
/// `SelectMapTool(LinePoi)` **不得**激活工具——瓷砖门控（T10，仅 Edit 档
/// 渲染）之外的命令链第二道闸。Edit 档放行、关工具不受档位限制。
#[test]
fn line_poi_command_rejected_outside_edit_tier() {
    use crate::state::Panel;
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;
    use wv_core::distance::LineKind;

    let cmd = || CommandId::SelectMapTool(MapToolId::LinePoi(LineKind::River));
    let mut world = dispatcher_world(None, false, cmd());
    // 工作区白名单（ADR-0080）：LinePoi 属 Map 档——夹具进 GoogleMap，让
    // 本测继续专测**档位闸**（scale），与工作区闸正交。
    world.resource_mut::<ViewState>().mode = wv_render::state::ViewMode::GoogleMap;
    // 缺省 scale=1.0 = Region 档：激活必须被拒（工具不开、面板不开）。
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world.resource::<crate::state::LinePoiState>().tool_active,
        "Region 档（scale=1.0）不得激活线段 POI 工具"
    );
    assert!(
        !world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "Region 档被拒时河流和道路面板不得打开"
    );

    // 放大到 Edit 档（scale < 1.0）：同一条命令即可激活。
    world.resource_mut::<ViewState>().scale = 0.5;
    world.resource_mut::<CommandPalette>().pending = Some(cmd());
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        world.resource::<crate::state::LinePoiState>().tool_active,
        "Edit 档（scale=0.5）应激活线段 POI 工具"
    );
    assert!(
        world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "Edit 档激活应打开河流和道路面板"
    );

    // 缩回 Region 档再发同款命令：关工具不受档位限制（already_on 走不到
    // 激活分支）。
    world.resource_mut::<ViewState>().scale = 1.0;
    world.resource_mut::<CommandPalette>().pending = Some(cmd());
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world.resource::<crate::state::LinePoiState>().tool_active,
        "非 Edit 档的关闭命令应照常生效"
    );
    assert!(
        !world.resource::<PanelState>().is_visible(Panel::LinePoi),
        "关工具后面板应收起"
    );
}

/// 从点选切到 EditCell 时，**统一选区不丢**（ADR-0057 D1：选区跨工具保留）。
#[test]
fn switching_from_point_select_to_edit_cell_keeps_selection() {
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;

    let mut world = dispatcher_world(
        None,
        false,
        CommandId::SelectMapTool(MapToolId::PointSelect),
    );
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let mut sel = world.resource_mut::<crate::state::CellSelection>();
        let map = wv_core::Id::new_unchecked("map_a");
        // 16×16 图幅：(7,9) 落在图内（8×8 会把它当越界静默丢弃）。
        sel.bind_map(&map, 16, 16);
        sel.replace([(7, 9), (0, 0), (1, 0)]);
    }

    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(sel.len(), 3, "切工具不得清掉统一选区");
    assert!(sel.contains(7, 9) && sel.contains(0, 0) && sel.contains(1, 0));
}

/// 工作区白名单闸（ADR-0080 裁定⑤）：Browse 档发 CellMap 工具（EditCell）
/// 必须被拒——工具不开、发 notice；白名单闸与播放/档位闸正交叠加。
#[test]
fn workspace_whitelist_rejects_cellmap_tool_in_browse() {
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;

    // dispatcher_world 缺省 mode = VectorFlat（Browse 档）。
    let mut world = dispatcher_world(None, false, CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world
            .resource::<wv_render::state::EditCellState>()
            .tool_active,
        "Browse 档不得激活 CellMap 专属工具（白名单闸）"
    );
}
