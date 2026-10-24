//! `ui_shell` 单测（原单文件尾部 `mod tests` 整段平移）。

use std::path::PathBuf;

use bevy::prelude::*;
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
#[allow(dead_code)] // helper used by tests; warnings emitted in non-test lib build
fn dispatcher_world(project_dir: Option<&str>, dirty: bool, pending: CommandId) -> World {
    AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let mut world = World::new();
    world.init_resource::<CommandPalette>();
    // 主题包注册表（ADR-0023 M2）：命令分发器要遍历它做主题包循环切换。
    world.insert_resource(wv_render::theme::switch::ThemeRegistry(
        wv_theme::registry::MergedThemeRegistry::merge(&[
            &wv_theme::registry::BuiltinRegistry,
            &wv_theme_chinese_ink::ChineseInkRegistry,
        ]),
    ));
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
    // CellMap 工具会话态（[ADR-0052] D6/D8-A）：`apply_pending_commands`
    // 的 `SelectMapTool(FloodSelect)` 与撤销/重做白闪分支都要读写它们。
    // 生产端在 `app_bootstrap` 注册；测试 fixture 必须同款补齐，否则
    // `SystemParamValidationError { Resource does not exist }`。
    world.init_resource::<crate::state::FloodSelectState>();
    world.init_resource::<wv_world_cell_map::cell_flash::CellFlashState>();
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
    // 命令面板 / 未来快捷键选工具的唯一路径：索引对 `BrushTool::ALL`，
    // 选中即激活笔刷（与面板瓷砖点击同语义）。同时 sync `tool_group`
    // 让段控件高亮跟随。越界索引不 panic。
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
    assert_eq!(
        brush.tool_group,
        crate::state::BrushToolGroup::Shape,
        "选 MountainRange 后段应切到塑"
    );
    assert!(brush.enabled, "选工具 = 激活笔刷");

    // 选 Place 组工具（Volcano=索引 4）→ 段切到放。
    let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(4));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    let brush = world.resource::<crate::state::BrushState>();
    assert_eq!(brush.tool_group, crate::state::BrushToolGroup::Place);

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
    assert_eq!(
        brush.tool_group,
        crate::state::BrushToolGroup::Paint,
        "越界不改组"
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
    // 主题包注册表（ADR-0023 M2）：命令分发器要遍历它做主题包循环切换。
    world.insert_resource(wv_render::theme::switch::ThemeRegistry(
        wv_theme::registry::MergedThemeRegistry::merge(&[
            &wv_theme::registry::BuiltinRegistry,
            &wv_theme_chinese_ink::ChineseInkRegistry,
        ]),
    ));
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
    // CellMap 工具会话态（[ADR-0052] D6/D8-A）：与 `dispatcher_world`
    // 同款补齐——`apply_pending_commands` 现在读写这两个资源。
    world.init_resource::<crate::state::FloodSelectState>();
    world.init_resource::<wv_world_cell_map::cell_flash::CellFlashState>();
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
    let mut world = dispatcher_world(Some("D:/old/project"), true, CommandId::CloseProject);
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

// ── 单选模式（2026-10-24 用户需求）─────────────────────────────

/// **单选模式与单格编辑共用 `tool_active`，只由 `allow_drag` 区分形态。**
///
/// 这条测的是分发器的三件事：
/// 1. `CellSelect` 开起来 = `tool_active && !allow_drag`；
/// 2. 切到 `EditCell` = 同一 `tool_active` 但 `allow_drag` 翻回 true
///    （否则「点了单选再点单格编辑」会两个瓷砖都不亮）；
/// 3. 对**当前形态**再点一次 = 关闭（不是切换）。
#[test]
fn cell_select_and_edit_cell_share_tool_active_but_differ_in_allow_drag() {
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;

    let mut world = dispatcher_world(None, false, CommandId::SelectMapTool(MapToolId::CellSelect));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let c = world.resource::<wv_render::state::EditCellState>();
        assert!(c.tool_active, "单选模式应激活工具");
        assert!(!c.allow_drag, "单选模式必须关掉拖绘");
    }

    // 切到 EditCell：同一 tool_active，形态翻回「可拖绘」。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let c = world.resource::<wv_render::state::EditCellState>();
        assert!(c.tool_active, "切到 EditCell 后工具仍激活");
        assert!(c.allow_drag, "EditCell 形态必须允许拖绘");
    }

    // 对当前形态再点一次 → 关闭。
    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    assert!(
        !world
            .resource::<wv_render::state::EditCellState>()
            .tool_active,
        "再次点击当前形态应关闭工具"
    );
}

/// 从单选模式切到 EditCell 时，**选区不丢**（S0-1 口径对两种形态一致）。
#[test]
fn switching_from_cell_select_to_edit_cell_keeps_selection() {
    use crate::ui_command_palette::MapToolId;
    use bevy::ecs::system::RunSystemOnce;

    let mut world = dispatcher_world(None, false, CommandId::SelectMapTool(MapToolId::CellSelect));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");
    {
        let mut c = world.resource_mut::<wv_render::state::EditCellState>();
        c.selected = Some((7, 9));
        c.region_cells = vec![(0, 0), (1, 0)];
    }

    world.resource_mut::<CommandPalette>().pending =
        Some(CommandId::SelectMapTool(MapToolId::EditCell));
    world
        .run_system_once(apply_pending_commands)
        .expect("系统运行失败");

    let c = world.resource::<wv_render::state::EditCellState>();
    assert_eq!(c.selected, Some((7, 9)), "切形态不得清掉单格选中");
    assert_eq!(c.region_cells.len(), 2, "切形态不得清掉区域选区");
}
