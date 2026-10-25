//! **唯一**命令分发器：快捷键 / 命令面板 / 工程菜单的排队命令都在这里执行。
//!
//! 每帧最多消费一条（菜单队列优先），先试工程 / 导航子派发，再走视图与工具命令。

use bevy::prelude::*;

use crate::app_screen::AppScreen;
use crate::i18n::CurrentLocale;
use crate::project_loader::PendingProjectLoad;
use crate::state::{
    DrillState, PanelState, PlaybackState, RecentProjects, ViewState, WorldProjection,
};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_command_palette::{CommandId, CommandPalette};
use wv_render::terrain_view::{TerrainTexture, TileAtlas};

use super::notice::ShellNotice;
use super::project_io::{backup_terrain_before_regen, DialogPurpose, OpenProjectTask};
use super::project_load::adopt_world_snapshot;

/// 分发工程 / 导航类命令（v0.2 §A §B），返回是否已消费该命令。
///
/// 唯一命令分发器的工程 / 导航子派发。
///
/// 保存那条不是偷懒：数据落盘要处理「每实体一文件 + 引用完整性 + 二进制分块」
/// 三件事（AGENTS §5/§4.5），不是一次 `serde_yaml::to_string` 能收尾的。
///
/// 能力边界（如实回执，不假装成功）：
///
/// - 打开工程… → ✅ 原生目录选择对话框（`rfd`），加载走 `wv_io::Project::open`
/// - 打开最近工程 → ✅ 真加载 + 视图接管（排队 → 后台线程 → `apply_loaded_world` 内含 adopt）
/// - 新建工程 → ✅ 弹「新建工程」对话框（名称 + 位置 + 路径预览）；确认后在
///   `位置\名称` 落盘（含首张世界地图）并打开（2026-10-14 改版，取代旧的
///   「内存空白工程 + 首次保存再选目录」链路）
/// - 关闭工程 → ✅ 清空工作副本并归位视图（底图 / 相机 / 播放）
/// - 另存为… → ✅ 目录对话框 → `Project::create`/`open` + 全量保存
/// - 导出… → ⛔ 导出格式未定（见 v0.2 §G）
/// - 项目设置 → ⛔ 面板未实现（v0.3）
/// - 保存 → ✅ 无目录时排队「选保存位置」对话框；有目录走
///   `wv_io::Project::save_world` 全量写，dirty 路由 v0.2
#[allow(clippy::too_many_arguments)]
fn dispatch_project_command(
    id: &CommandId,
    projection: &mut WorldProjection,
    new_project_dialog: &mut crate::ui::NewProjectDialog,
    drill: &mut DrillState,
    recents: &mut RecentProjects,
    notice: &mut ShellNotice,
    open_task: &mut OpenProjectTask,
    diagnostics: &mut crate::state::ProjectDiagnostics,
    edit: &mut crate::edit::editing::EditHistory,
    playback: &mut PlaybackState,
    view: &mut ViewState,
    tex: &mut TerrainTexture,
    atlas: Option<&TileAtlas>,
    images: &mut Assets<Image>,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    pending: &mut PendingProjectLoad,
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
) -> bool {
    match id {
        CommandId::OpenRecentProject(i) => {
            let list = recents.visible(projection);
            match list.get(*i as usize) {
                Some(entry) => {
                    let dir = entry.dir.clone();
                    // 只排队——加载由 `drive_project_load` 在后台线程派发
                    //（2026-10-22 C 档）。
                    pending.0 = Some(dir);
                }
                None => notice.error(
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeRecentsChanged),
                    frame,
                ),
            }
            true
        }
        CommandId::NewProject => {
            // 2026-10-14 改版（用户需求：参考 VS 的新建项目对话框）：不再默默
            // 造一个内存空白世界——弹对话框收「名称 + 位置」，确认后在
            // `位置\名称` 落盘（含首张世界地图）并打开。创建逻辑与落盘细节
            // 全在 `ui::new_project_dialog`；这里只负责把框打开。
            new_project_dialog.open();
            true
        }
        CommandId::CloseProject => {
            projection.world = wv_core::World::default();
            projection.project_dir = None;
            projection.dirty = false;
            projection.load_error = None;
            drill.reset_to_world();
            // 关闭工程：与新建同样清诊断。
            diagnostics.diagnostics = wv_io::diagnostics::Diagnostics::default();
            diagnostics.last_count = 0;
            diagnostics.needs_diagnose = true;
            edit.history = Default::default();
            edit.clear_dirty();
            // 关闭 = 换成空世界：底图 / 相机 / 播放必须跟着归位，否则旧工程的
            // 地图永远挂在屏上（回归 2026-10-14「关了像没关」，见 adopt 文档）。
            adopt_world_snapshot(
                projection, drill, playback, view, tex, atlas, images, cameras,
            );
            notice.info(
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeProjectClosed),
                frame,
            );
            true
        }
        CommandId::OpenProject => {
            // 排队「打开工程」对话框意图（不阻塞）；下一帧由
            // `poll_open_project_dialog` 发起、收口，复用 `OpenRecentProject`
            // 的同一条加载路径（排队 → 后台线程）——结果就只是「多了一条最近工程」。
            open_task.request(DialogPurpose::Open);
            true
        }
        CommandId::SaveProjectAs => {
            // v0.3 §B：排队「选保存位置」意图——下一帧发起，选完由
            // `poll_open_project_dialog` 的 SaveAs 分支收口（create-or-open + 全量保存）。
            open_task.request(DialogPurpose::SaveAs);
            true
        }
        CommandId::ExportProject => {
            notice.info(
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeExportPending),
                frame,
            );
            true
        }
        CommandId::ProjectSettings => {
            notice.info(
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeSettingsPendingV03),
                frame,
            );
            true
        }
        CommandId::DrillUp => {
            if !drill.up() {
                notice.info(
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeAlreadyWorldMap),
                    frame,
                );
            }
            true
        }
        CommandId::DrillToRoot => {
            drill.jump_to(0);
            true
        }
        CommandId::DrillToSegment(i) => {
            // 面包屑点第 i 段（0 = 根）。
            if !drill.jump_to(*i as usize) {
                notice.info(
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeAlreadyAtLevel),
                    frame,
                );
            }
            true
        }
        _ => false,
    }
}

// ── 主循环 ────────────────────────────────────────────────────────────────────

/// 执行命令面板 / 快捷键 / 菜单排队的命令。
///
/// **唯一分发器**：快捷键与命令面板写 [`CommandPalette::pending`]；顶栏工程菜单
/// 的绘制函数只拿得到菜单自身状态，命令先落 [`crate::ui::ProjectMenuState::pending`]。
/// 两条队列在这里汇合（菜单优先，每帧最多消费一条）统一执行——拆成多个系统各自
/// `take()` 会在「谁先拿到」上产生时序耦合。菜单的写入发生在
/// `EguiPrimaryContextPass`（晚于本 `Update`），所以菜单命令在下一帧这里被执行，
/// 一帧延迟与快捷键「PreUpdate 写 / 本帧 Update 收」同量级，用户不可感。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn apply_pending_commands(
    // Bevy 单个函数系统的参数上限是 16 个；汇入菜单队列、换世界接管资源后
    // 到这里共 19 个，超限报「not a valid system」。按职责折成嵌套元组——
    // `SystemParam` 对元组递归实现，每层元组算 1 个参数；解构保留原名，函数体不受影响。
    (mut palette, mut menu, mut panels): (
        ResMut<CommandPalette>,
        ResMut<crate::ui::ProjectMenuState>,
        ResMut<PanelState>,
    ),
    (mut cfg, mut view, mut playback): (
        ResMut<UiShellConfig>,
        ResMut<ViewState>,
        ResMut<crate::state::PlaybackState>,
    ),
    (mut brush, mut measure, mut streets, mut projection, mut drill): (
        ResMut<crate::state::BrushState>,
        ResMut<crate::state::MeasureState>,
        ResMut<wv_render::streets::StreetEditorState>,
        ResMut<WorldProjection>,
        ResMut<DrillState>,
    ),
    (mut recents, mut notice, mut edit): (
        ResMut<RecentProjects>,
        ResMut<ShellNotice>,
        ResMut<crate::edit::editing::EditHistory>,
    ),
    (mut tex, mut open_task, mut diagnostics): (
        ResMut<wv_render::terrain_view::TerrainTexture>,
        ResMut<OpenProjectTask>,
        ResMut<crate::state::ProjectDiagnostics>,
    ),
    // 工程加载排队（2026-10-22 C 档）：「打开工程…」对话框 / 「打开最近工程」
    // 只排队，加载由 `drive_project_load` 在后台线程派发（不冻窗口）。
    mut pending: ResMut<PendingProjectLoad>,
    // 「新建地图」对话框的开关（CommandId::NewMap → open_for；真正的创建
    // 在对话框自身的系统里提交，见 `ui::new_map_dialog`）。
    // 「新建工程」对话框的开关同源（CommandId::NewProject / 欢迎页 → open）。
    // 两个 ResMut 折成一层元组：单系统参数上限 16，拆元组后每层元组算 1 个。
    (mut new_map_dialog, mut new_project_dialog, mut continent_dialog): (
        ResMut<crate::ui::NewMapDialog>,
        ResMut<crate::ui::NewProjectDialog>,
        ResMut<crate::ui::ContinentDialog>,
    ),
    // 换世界接管（adopt_world_snapshot）所需：瓦片图集可缺席（纯色平涂回退）。
    (atlas, mut images): (
        Option<Res<wv_render::terrain_view::TileAtlas>>,
        ResMut<Assets<Image>>,
    ),
    // 选择工具会话态：命令面板「生成大陆…」顺手启用选择工具。
    mut selection: ResMut<crate::state::SelectionState>,
    // 单格编辑工具会话态（[ADR-0040]）：跟 selection 同款粒度。
    mut edit_cell: ResMut<crate::state::EditCellState>,
    mut boundary: ResMut<wv_render::state::BiomeBoundaryState>,
    // 泛洪选择工具会话态（[ADR-0052] D6）：与 EditCell 同款粒度的互斥工具。
    // 撤销白闪会话态（[ADR-0052] D8-A）：撤销/重做后对受影响格做 250ms
    // 白色脉冲；点选工具会话态（ADR-0057 补记 6）——三者同属 CellMap
    // 会话域，折成一层元组（元组算 1 个参数，单系统上限 16）。
    (mut flood_select, mut flash, mut point_select): (
        ResMut<crate::state::FloodSelectState>,
        ResMut<wv_world_cell_map::cell_flash::CellFlashState>,
        ResMut<crate::state::PointSelectState>,
    ),
    current_loc: Res<CurrentLocale>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    time: Res<Time>,
    // 应用屏状态（2026-10-22 B 档）：「谁该显示」的唯一裁决点。
    screen: Res<AppScreen>,
) {
    // 菜单队列优先（egui 一帧最多产生一条），命令面板其次。
    let id = menu.pending.take().or_else(|| palette.pending.take());
    let Some(id) = id else {
        return;
    };
    // 帧号：回执过期判断只依赖它，不引入计时器资源。
    let frame = (time.elapsed_secs() * 60.0) as u64;

    // 无工程闸门（2026-10-14 用户需求）：没有打开任何工程时，一切「改数据 /
    // 需要工程上下文」的命令一律挡下并指出下一步——欢迎页是唯一入口。
    // NewProject / OpenProject / CloseProject（本身就是入口）与视图 / 面板
    // 命令不在其列；笔刷与街道的输入侧另有同口径门禁（见 collect_brush_stroke）。
    //
    // 2026-10-22（异步加载）：加载期间**也**挡下同一批命令——世界即将被替换，
    // 此时落盘会写错工程（改的是旧世界、存的是新目录）。但回执话术要分开：
    // 「没有工程」与「正在加载」是两回事，不能用同一句。
    if matches!(
        id,
        CommandId::NewMap
            | CommandId::Save
            | CommandId::SaveProjectAs
            | CommandId::ExportProject
            | CommandId::GenerateContinent
    ) {
        if screen.shows_loading_overlay() {
            notice.error(
                crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeProjectLoading),
                frame,
            );
            return;
        }
        if !screen.has_project {
            notice.error(
                crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeNeedProjectFirst),
                frame,
            );
            return;
        }
    }

    // 工程 / 导航类先试（它们改的是世界与路径，与下面的视图状态互不重叠）。
    if dispatch_project_command(
        &id,
        &mut projection,
        &mut new_project_dialog,
        &mut drill,
        &mut recents,
        &mut notice,
        &mut open_task,
        &mut diagnostics,
        &mut edit,
        &mut playback,
        &mut view,
        &mut tex,
        atlas.as_deref(),
        &mut images,
        &mut cameras,
        &mut pending,
        frame,
        &current_loc,
    ) {
        return;
    }

    match id {
        CommandId::SetViewMode(i) => {
            // 索引对 `BROWSING` 而不是 `ALL`：命令面板 / 快捷键只开放两种浏览模式
            // （2026-10-14 用户裁定），索引语义必须与列表来源一致。
            if let Some(m) = crate::state::ViewMode::BROWSING.get(i as usize) {
                view.mode = *m;
            }
        }
        CommandId::OpenCommandPalette => palette.open(),
        // 新建地图：只负责把对话框打开（默认值按世界现状预填）；创建动作
        // 在对话框自身的系统里经 `AddMapCommand` 提交（见 `ui::new_map_dialog`）。
        CommandId::NewMap => new_map_dialog.open_for(&projection.world),
        CommandId::ToggleTheme => cfg.theme = cfg.theme.toggled(),
        CommandId::ToggleDensity => cfg.density = cfg.density.toggled(),
        CommandId::ToggleImmersive => panels.immersive = !panels.immersive,
        CommandId::TogglePanel(p) => panels.toggle(p),
        CommandId::ResetPanelLayout => panels.reset_requested = true,
        CommandId::FitMap => {
            // FitMap 的语义 = 与启动/加载后相同的取景（`adopt_loaded_world`
            // 同款 fit），而不是字面 ZERO/1.0——旧实现只写 ViewState，
            // 相机从不跟随，语义从未真正生效过。
            let (center, scale) = crate::app_bootstrap::fit_view_for_world(&projection.world);
            view.camera_center = center;
            view.scale = scale;
            wv_render::camera::apply_view_to_camera(center, scale, &mut cameras);
        }
        CommandId::Zoom(d) => {
            // 命令没有指针上下文（锚点固定为视口中心），但**缩放上下限必须按当前级别取**：
            // 固定 `0.1..8.0` 是单级时代的产物，分级后城市/街道级的区间完全不同
            // （ADR-0009 §8 决策①）。窗口尺寸只影响区间宽度，取不到时按默认窗口。
            let window_size = view.window_size;
            let band = wv_render::zoom_level::level_zoom(
                &projection.world,
                drill.current(),
                window_size,
                Vec2::splat(wv_render::zoom_level::FALLBACK_MAP_SIZE),
            );
            view.scale = band.clamp(view.scale * if d > 0 { 1.25 } else { 0.8 });
            // 缩放锚点 = 视口中心（命令无指针上下文，保持中心不动）。
            wv_render::camera::apply_view_to_camera(view.camera_center, view.scale, &mut cameras);
        }
        CommandId::TogglePlay => playback.playing = !playback.playing,
        CommandId::JumpToStart => playback.current_day = playback.range_from,
        CommandId::JumpToEnd => playback.current_day = playback.range_to,
        CommandId::BrushRadius(d) => {
            brush.radius = (brush.radius + if d > 0 { 8.0 } else { -8.0 }).clamp(8.0, 600.0);
        }
        CommandId::SelectBrushTool(i) => {
            // 越界索引静默忽略（与 SetViewMode 同口径）；选中即激活笔刷——
            // 「选工具 = 准备画」。工具分组控件与图标行已移除（2026-10-24），
            // 这里只改 `tool` 并开笔刷。
            if let Some(t) = crate::state::BrushTool::ALL.get(i as usize) {
                brush.tool = *t;
                brush.enabled = true;
            }
        }
        CommandId::ToggleGrid => view.grid_lines = !view.grid_lines,
        // 地形场渲染（ADR-0053 D1，Phase 1 接线）：只翻旗标，接线全在
        // `wv_render::cellmap_bridge::sync_terrain_field`——命令层不认识材质。
        // 当帧生效：场系统排在 Update 主链的 chunk 层之前，同一帧就完成
        // 「建 quad / 拆 quad + 块 spawn / despawn」的交接。
        CommandId::ToggleFieldRender => view.field_render = !view.field_render,
        // 地图工具条单选工具（2026-10-19）：**互斥的唯一实现**。
        // 工具条瓷砖与 V/H/B/E/L/M/P 快捷键都排到这里，先关掉所有工具再开目标，
        // 保证「左键」永远只有一个消费者（相机平移 / 选区 / 笔刷 / 测量 / 街道）。
        CommandId::SelectMapTool(tool) => {
            use crate::state::MeasureMode;
            use crate::ui_command_palette::MapToolId;

            // 目标工具当前是否已启用（再次点击 = 关闭）。
            //
            // `Select(shape)` 的口径是「同形状开着 = 关；不同形状开着 = 换形状」
            // ——三个形状是同一把选择工具的三种笔尖，V 快捷键在多边形 / 矩形 /
            // 椭圆之间走一圈时不应出现「先关再开把选区弄丢」的顿挫。
            let already_on = match tool {
                MapToolId::Select(shape) => selection.enabled && selection.shape == shape,
                MapToolId::Pan => view.pan_tool,
                MapToolId::Brush => brush.enabled && brush.tool != crate::state::BrushTool::Eraser,
                MapToolId::BrushErase => {
                    brush.enabled && brush.tool == crate::state::BrushTool::Eraser
                }
                MapToolId::MeasureLine => measure.mode == MeasureMode::Line,
                MapToolId::MeasureRect => measure.mode == MeasureMode::Rect,
                MapToolId::MeasureEraser => measure.mode == MeasureMode::Eraser,
                MapToolId::Continent => false,
                // EditCell 的「已开」判据 = `tool_active`（[ADR-0052] §9.1 S0-1）。
                // ADR-0057 补记 6：PointSelect 与 EditCell 拆成两件工具两个状态，
                // 不再共用 `tool_active` / `allow_drag`——判据各读各的 Resource。
                MapToolId::EditCell => edit_cell.tool_active,
                MapToolId::PointSelect => point_select.tool_active,
                MapToolId::BiomeBoundary => boundary.line.is_some(),
                MapToolId::FloodSelect => flood_select.tool_active,
            };

            if matches!(tool, MapToolId::Continent) {
                // 「生成大陆」是开门动作，不是互斥工具：沿用旧语义——只开面板 +
                // 顺手启用选择工具（圈出意图包络）。
                continent_dialog.open = true;
                selection.enabled = true;
            } else {
                // ① 先全关：互斥。切走时清掉草稿/半边选区，避免「看不见的手」。
                let had_selection = selection.enabled;
                let had_measure = measure.mode != MeasureMode::Off;
                selection.enabled = false;
                view.pan_tool = false;
                brush.enabled = false;
                streets.enabled = false;
                measure.mode = MeasureMode::Off;
                if had_selection {
                    selection.cancel();
                }
                if had_measure {
                    measure.clear_draft();
                }
                // 关掉 EditCell 时**只关工具，不清选区**（[ADR-0052] §9.1 S0-1 +
                // ADR-0057 D1）：`CellSelection` 是已提交的选区态，切工具后仍保留，
                // 「框选一片 → 切 EditCell」不丢选区（§5 M1 验收 ④）。
                // 切图清空由 `sync_cell_region` 的 `bind_map` 承担；Esc 走清空入口。
                edit_cell.close_tool();
                // 点选工具同款：只关工具，不动选区（ADR-0057 补记 6）。
                point_select.close_tool();
                // 关掉泛洪工具：同样只关工具，不清选区（D6-A）。
                flood_select.close_tool();
                // 切走分界线工具：线是会话工具，一并清（ADR-0051）。
                boundary.clear();

                // ② 再按需打开（已启用 → 保持全关，即切换关闭）。
                if !already_on {
                    match tool {
                        MapToolId::Select(shape) => {
                            selection.shape = shape;
                            selection.enabled = true;
                        }
                        MapToolId::Pan => view.pan_tool = true,
                        MapToolId::Brush => brush.enabled = true,
                        // 橡皮与涂抹共用 `BrushState`，只差 `tool` 指到哪把。
                        MapToolId::BrushErase => {
                            brush.tool = crate::state::BrushTool::Eraser;
                            brush.enabled = true;
                        }
                        MapToolId::MeasureLine => {
                            measure.clear_draft();
                            measure.mode = MeasureMode::Line;
                        }
                        MapToolId::MeasureRect => {
                            measure.clear_draft();
                            measure.mode = MeasureMode::Rect;
                        }
                        MapToolId::MeasureEraser => {
                            measure.clear_draft();
                            measure.mode = MeasureMode::Eraser;
                        }
                        MapToolId::Continent => unreachable!("Continent 已在上方分支处理"),
                        // EditCell 工具激活：ADR-0057 后**只画不选**——点击/拖动
                        // 累积笔触（见 edit::cell_stroke），选区写入全归
                        // PointSelect / 框选 / 套索 / 泛洪四个落笔点。
                        MapToolId::EditCell => {
                            edit_cell.tool_active = true;
                        }
                        // 点选工具（ADR-0057 补记 6）：独立 `PointSelectState`。
                        // 落笔写 `CellSelection`（见 editing::commit_edit_cell_on_click
                        // 的点选分支），hover 准星与 EditCell 共用。
                        MapToolId::PointSelect => {
                            point_select.tool_active = true;
                        }
                        // 分界线工具激活：线要等用户在地图上拖出（见
                        // edit::biome_boundary_input）。
                        MapToolId::BiomeBoundary => {
                            boundary.tool_active = true;
                        }
                        // 泛洪选择激活：点一下即泛洪，结果入统一选区
                        // （见 `edit::flood_select`）。判据在 CellMap 面板的
                        // segmented 里改（[ADR-0052] D6）。
                        MapToolId::FloodSelect => {
                            flood_select.tool_active = true;
                        }
                    }
                }
            }
        }
        // 大陆生成属性面板（面板本身只收参数；生成在 continent_dialog_system）。
        CommandId::GenerateContinent => {
            continent_dialog.open = true;
            // 顺手启用选择工具，作者圈完选区回面板确认（spec D1/D5；
            // 与测量的互斥由 map_controls 的瓷砖在启用路径上处理）。
            selection.enabled = true;
        }
        // 撤销 / 重做走 `editing` 的公开入口（同一份逻辑，不复制）。
        CommandId::Undo | CommandId::Redo => {
            let redo = matches!(id, CommandId::Redo);
            // **D8-A 白闪**：撤销/重做后对受影响格做 250ms 白色脉冲——
            // 撤销时画面差异肉眼不可辨，需要一个「这几格刚刚变了」的定位信号。
            match crate::edit::editing::apply_history_command_detailed(
                redo,
                &mut edit,
                &mut projection,
                &mut tex,
                &playback,
            ) {
                Ok(outcome) => {
                    // 撤销/重做后世界可能改——诊断需要重算。
                    // `Ok(false)`（无可撤销/重做）时不动，避免无谓重算。
                    if outcome.message != "没有可撤销的操作"
                        && outcome.message != "没有可重做的操作"
                    {
                        diagnostics.needs_diagnose = true;
                    }
                    if !outcome.changed_cells.is_empty() {
                        flash.trigger(
                            wv_world_cell_map::cell_flash::CellFlashKind::Undo,
                            outcome.changed_cells,
                            time.elapsed_secs(),
                        );
                    }
                    notice.info(outcome.message, frame);
                }
                Err(msg) => notice.error(msg, frame),
            }
        }
        // 清除当前图的全部测量（2026-10-23 用户需求）：工具条测量组右侧的
        // 清除钮与命令面板同源。一次事务可整体撤销。
        CommandId::ClearMeasurements => {
            let Some(map_id) = drill.current().cloned() else {
                notice.error(
                    crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeClearMeasureNoMap),
                    frame,
                );
                return;
            };
            match crate::edit::editing::apply_clear_measurements(
                &map_id,
                &mut edit,
                &mut projection,
                &playback,
            ) {
                Ok(msg) => notice.info(msg, frame),
                Err(msg) => notice.error(msg, frame),
            }
        }
        // 重置地形到底图（2026-10-16 用户需求）。
        CommandId::ResetTerrainToBase => {
            let Some(map_id) = drill.current().cloned() else {
                notice.error(
                    crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeResetTerrainNoMap),
                    frame,
                );
                return;
            };
            match crate::edit::editing::apply_reset_terrain_to_base(
                &map_id,
                &mut edit,
                &mut projection,
                &mut tex,
                &playback,
            ) {
                Ok(msg) => {
                    // 重置后地形 cells 改了 → 派生诊断（事件位移 / 相遇）需要重算。
                    diagnostics.needs_diagnose = true;
                    notice.info(msg, frame);
                }
                Err(msg) => notice.error(msg, frame),
            }
        }
        // 重新生成世界地形（ADR-0037 D4）：新种子重跑冷北模板，单事务可撤销。
        // 备份文件在提交前落盘——重生成是破坏性的，必须给作者退路。
        CommandId::RegenWorld => {
            let Some(map_id) = drill.current().cloned() else {
                notice.error(
                    crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeRegenNoMap),
                    frame,
                );
                return;
            };
            let (w, h) = projection
                .world
                .terrain_of(&map_id)
                .map(|t| (t.width.max(2), t.height.max(2)))
                .unwrap_or((512, 288));
            let seed = crate::ui::new_project_dialog::seed_from_clock();
            let generated = wv_terrain::climate::generate_cold_north(w, h, seed);
            let terrain = wv_core::terrain::TerrainData {
                width: generated.width,
                height: generated.height,
                cells: generated
                    .cells
                    .into_iter()
                    .map(|c| wv_core::terrain::TerrainCellData {
                        height: c.height,
                        surface: c.surface,
                        biome: c.biome,
                    })
                    .collect(),
            };
            let template = wv_core::WorldTemplate {
                kind: wv_core::WorldTemplateKind::ColdNorth,
                seed,
                rivers_seed: seed ^ 0x9E37_79B9_7F4A_7C15,
            };
            if let Some(dir) = projection.project_dir.clone() {
                backup_terrain_before_regen(&dir, &map_id, &projection.world);
            }
            match crate::edit::editing::apply_regen_world(
                &map_id,
                terrain,
                template,
                &mut edit,
                &mut projection,
                &mut tex,
                &playback,
            ) {
                Ok(msg) => {
                    diagnostics.needs_diagnose = true;
                    notice.info(msg, frame);
                }
                Err(msg) => notice.error(msg, frame),
            }
        }
        // 保存（阶段七 v0.2）：走 `wv_io::Project::save_world`，只重写 dirty
        // 目录。`take_dirty` 原子取空集合；空 Vec 走全量路径（首次保存、
        // 刚加载、刚撤销等场景）。失败不排空——脏标记仍在，下次保存能补上。
        CommandId::Save => {
            let Some(dir) = projection.project_dir.clone() else {
                // 新建工程还没目录：不再报死错，而是排队「选保存位置」对话框
                // 意图（v0.3 §B，下一帧由 `poll_open_project_dialog` 发起）。
                // 这补上了「新建 → 首次保存」的断链。
                open_task.request(DialogPurpose::SaveAs);
                return;
            };
            let dirty = edit.take_dirty();
            let n = dirty.len();
            match wv_io::Project::open(&dir) {
                Ok(mut project) => match project.save_world(&projection.world, &dirty) {
                    Ok(()) => {
                        projection.dirty = false;
                        let scope = if n == 0 {
                            crate::i18n::t_key(
                                &current_loc,
                                crate::i18n::I18nKey::NoticeSavedScopeFull,
                            )
                            .to_owned()
                        } else {
                            format!(
                                "{}{}{}",
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopePartialCountPrefix
                                ),
                                n,
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopePartialCountSuffix
                                )
                            )
                        };
                        notice.info(
                            format!(
                                "{}{}{}{}{}",
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedToPrefix
                                ),
                                dir,
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopePrefix
                                ),
                                scope,
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopeSuffix
                                )
                            ),
                            frame,
                        );
                    }
                    Err(e) => {
                        // 失败时把 dirty 放回，让下次保存能再试。
                        edit.record_dirty(dirty);
                        notice.error(
                            format!(
                                "{}{}",
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSaveFailedPrefix
                                ),
                                e
                            ),
                            frame,
                        );
                    }
                },
                Err(e) => notice.error(
                    format!(
                        "{}{}",
                        crate::i18n::t_key(
                            &current_loc,
                            crate::i18n::I18nKey::NoticeOpenProjectDirFailedPrefix
                        ),
                        e
                    ),
                    frame,
                ),
            }
        }
        // 强制重画地形纹理（2026-10-14 用户需求：怀疑数据已改、画面没跟上时点 ↻）。
        // 不动世界数据；只清 `last_mode/last_map` 让下一帧必然全图重建。
        CommandId::ForceRebuildTexture => {
            wv_render::terrain_view::force_rebuild(&mut tex);
            notice.info(
                crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeForceRedrawn),
                frame,
            );
        }
        // 其余工程 / 导航命令已在上面消费。
        CommandId::OpenRecentProject(_)
        | CommandId::NewProject
        | CommandId::OpenProject
        | CommandId::SaveProjectAs
        | CommandId::CloseProject
        | CommandId::ExportProject
        | CommandId::ProjectSettings
        | CommandId::DrillUp
        | CommandId::DrillToRoot
        | CommandId::DrillToSegment(_) => {}
    }
}
