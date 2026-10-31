//! 点选落笔 + hover 追踪（ADR-0057：点选进统一选区，EditCell 只画不选）。
//!
//! [ADR-0074]（经 [ADR-0094] 修订，2026-10-31 用户裁定）：CellMap
//! （Topographic）工作区里点选器是**「POI 命中优先，其余一律选群系」**——
//! 标签 > 图钉（点状 POI）> 群系区（Nature Region 实体，自然层开着且命中
//! 时）> **同群系连通域泛洪兜底**（群系区没创建也照样有得选）。精细的
//! 格子选择（单格 / 泛洪判据 / 选区加减法）仍在圈选「单格」形状
//! （[`commit_cell_shape_click`]，本文件下方）。
//! 浏览族（其余模式）的命中在 `wv_render::drill::drill_pick`（追记一）：
//! 那边标签命中之后接图钉，两边互不重叠。图钉命中纯函数
//! [`wv_render::drill::pick_place_pin`] 两边共用；标签命中
//! [`wv_render::label_pick::hit_test`] 亦同源。

use super::*;
use wv_core::model::region::RegionKind;
use wv_core::{EntityKind, Id, SelectedEntity};
use wv_render::layer_frame::LayerFrame;
use wv_render::state::ViewMode;

use wv_render::drill::pick_place_pin;

/// 点在平铺多边形环内（射线法）。`poly` 为 `[x0, y0, x1, y1, …]` 环坐标
/// （图内局部 wu，与 `Region::polygon` / `Place::envelope` 同一存储形态）。
/// 少于 3 个顶点恒 false——退化环不产生命中。
pub fn point_in_flat_polygon(px: f32, py: f32, poly: &[f32]) -> bool {
    let n = poly.len() / 2;
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i * 2], poly[i * 2 + 1]);
        let (xj, yj) = (poly[j * 2], poly[j * 2 + 1]);
        // 边 (j→i) 跨过光标所在水平线，且交点在光标左侧 → 奇偶翻转。
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 命中包含该点的群系区（[ADR-0074] 裁定第二段）。
///
/// 候选 = 本图 + Nature + 已命名（无名区不画，选不到）。`local` 为图内
/// 局部坐标（`cursor_world − grid.map_origin`，与 `map_to_view` 同一原点）。
/// 重叠时**后注册者胜**——与标签命中「画在上层的先被点到」同一约定；
/// 群系区来自分区算法，正常不重叠，此约定只兜退化数据。
pub fn pick_nature_region(local: (f32, f32), world: &wv_core::World, map_id: &Id) -> Option<Id> {
    let mut hit = None;
    for r in &world.regions {
        if r.map != *map_id || r.kind != RegionKind::Nature || r.is_unnamed() {
            continue;
        }
        if point_in_flat_polygon(local.0, local.1, &r.polygon) {
            hit = Some(r.id.clone());
        }
    }
    hit
}

/// 点选系统的双击追迹（上次左键时间 + 世界坐标）。
///
/// `pub` 是必须的：它出现在 pub 系统的 `Local<>` 参数里（同 `RightDragWatch`
/// 的教训——`SystemParam` 类型比系统更私有会直接编译失败）。
#[derive(Default)]
pub struct DoubleClickWatch(
    pub Option<(f64, Vec2)>,
    /// 双击序列内的「选区被改写」累计（ADR-0097）：第一击泛洪出新群系、
    /// 第二击落在泛洪出的选区里（contains 守卫不再置位）——页签裁决要的
    /// 是**整个序列**是否改写了选区，非单次点击。
    pub bool,
);

/// 点选工具的鼠标点击处理器（ADR-0057 补记 6；原 `commit_edit_cell_on_click`）。
///
/// 双击参数束（ADR-0089 D2）用 **Option 口径**：部分单测夹具不装 Time /
/// 请求资源（只测裁定本身），缺席即跳过双击判定；生产 app 两资源恒注册
/// （`ui_shell::register`）。
///
/// 只在 [`PointSelectState::tool_active`] 为 true 时生效——
/// 由 `apply_pending_commands` 的 `SelectMapTool(PointSelect)` 分支翻 on。
///
/// 行为（ADR-0057 D2「先名词后动词」）：
/// 1. 收 `MouseButton::Left` **按下**而非松开（点击即入列,不需要拖拽）；
/// 2. 拿 `view.cursor_world` 与 `drill.current()` 的图地形；
/// 3. 用 [`cell_pick::pick_at`] 反查 (gx, gy)，按修饰键写入
///    [`CellSelection`]：**无修饰 = 替换、Shift = 并、Alt = 差**；
/// 4. 点在地图外（无修饰）= 清空选区（「点空白 = 取消当前选」）。
///
/// **CellMap 裁定**（[ADR-0090] D2 修订 [ADR-0074] 追记四，仅 `Topographic`
/// 档生效）：**图钉**（选 Place）＞ **群系区**（`nature_overlay` 开时选
/// Region）。格子两档（Alt 单格 / 默认泛洪）已**移交圈选「单格」形状**
/// （[`commit_cell_shape_click`]，下方）——点选不再碰格子，点空地=不动选区。
/// 单焦点模型不变：选中实体即清格选区（`resolve_form` 里 Cell 表单优先，
/// 不清 = 悬挂选中）。
///
/// **非 Topographic（浏览族）**：本系统不处理——浏览族的图钉命中在
/// `wv_render::drill::drill_pick`（追记一）；格子选择按 ADR-0090 D2 随
/// 浏览族只读语义退役，实体仲裁待 M2 接管。
///
/// 修饰键走 Bevy `ButtonInput<KeyCode>`（ShiftLeft/Right、AltLeft/Right）——
/// **不读 egui 键位链**（2026-10-24 真机 panic 前科：egui 上下文在系统里
/// 的可用性依赖装配顺序，Bevy 输入资源是硬保证）。
///
/// 播放中只读（基线 §1.8 / V37）：直接跳过,不写选区。
/// PointerOverUi：面板点击不吃成选格（与 drill / brush_cursor 同口径）。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn commit_point_select_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    mut projection: ResMut<WorldProjection>,
    layers: Res<LayerFrame>,
    ui: Res<wv_render::camera::PointerOverUi>,
    point_select: Res<crate::state::PointSelectState>,
    mut sel: ResMut<crate::state::CellSelection>,
    screen: Res<crate::app_screen::AppScreen>,
    mut link: ResMut<super::region::LassoLiveLink>,
    // 标签命中表（2026-10-31 修复）：CellMap 里标签可见（ADR-0051 §7
    // Topographic 开档）且命中表照常注册，但消费者只有浏览族 `drill_pick`
    // （那边 Topographic 早退）——点标签文字毫无反应（用户实测）。
    // Option 口径同下方双击束：生产 app 在 `drill::register` 恒注册，
    // 部分单测夹具不装（只测裁定本身），缺席即跳过标签段。
    label_hits: Option<Res<wv_render::label_pick::LabelHits>>,
    // 双击跟踪 + 属性窗体请求（ADR-0089 D2：CellMap 双击 = 照常裁定 +
    // 拉起 Inspector）。time/watch/请求折进元组——系统已顶 16 参上限；
    // Option 口径见函数文档。
    (time, (mut dbl, mut inspector_req, mut cell_dlg)): (
        Option<Res<Time>>,
        (
            Local<DoubleClickWatch>,
            Option<ResMut<crate::ui_shell::property_window::OpenInspectorRequest>>,
            // ADR-0097：双击群系/选区 → 居中编辑 dialog。Option 口径同上：
            // 生产恒注册，夹具缺席即跳过。
            Option<ResMut<crate::ui::cell_edit_dialog::CellEditDialogState>>,
        ),
    ),
) {
    // 入口：左键单击（含修饰）。右键不归本系统（追记二的 Ctrl+右键已被
    // Alt+左键取代，见追记四）。
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    // 加载期间不选格：选中的格来自旧世界，对不上新世界。
    if screen.shows_loading_overlay() {
        return;
    }
    if !point_select.tool_active {
        return;
    }
    // 指针压在 egui 面板/顶栏上：这是面板交互（拖数值、按按钮、拖窗口），
    // 不是「选格」——吃了它会把面板底下那格误设进选区（2026-10-20 实机
    // bug：点面板即丢焦点）。与 drill / brush_cursor 的 PointerOverUi
    // gate 同口径。
    if ui.wants_pointer {
        return;
    }
    if playback.playing {
        // 时间游标在动 → 当前 world 是投影 → 不要写。
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(target_map) = drill.current() else {
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        // 子图继承父图栅格时 `terrain_of` 会返 None——无地形可选。
        return;
    };
    // 双击跟踪（ADR-0089 D2；ADR-0094 追记：后果按裁定结果分流，见函数尾
    // ——实体拉 Inspector，群系格开笔刷面板）。两次单击各自幂等。阈值单点
    // [`wv_render::input`]，位移按屏幕像素口径乘 `view.scale`（UX-024 同口径）。
    let mut is_double = false;
    // 序列累计读入：双击的第二击继承第一击的「选区被改写」位（ADR-0097）。
    let mut seq_changed = false;
    if let Some(time) = time.as_deref() {
        let now = time.elapsed_secs_f64();
        let double_slop_world = wv_render::input::DOUBLE_CLICK_SLOP_PX * view.scale;
        is_double = dbl
            .0
            .map(|(t, p)| {
                now - t <= wv_render::input::DOUBLE_CLICK_SECS
                    && p.distance(cursor) <= double_slop_world
            })
            .unwrap_or(false);
        seq_changed = if is_double { dbl.1 } else { false };
        dbl.0 = if is_double { None } else { Some((now, cursor)) };
    }
    // 图绑定兜底：正常每帧由 `sync_cell_region` 幂等绑定；本系统跑在渲染
    // 之前时（首帧 / 测试）也得保证写进的是当前图的位图。
    sel.bind_map(target_map, terrain.width, terrain.height);
    // 新选择动作 → 断开套索活链接（ADR-0057 补记 5）。
    link.deactivate();

    bevy::log::debug!(
        "[synth] 裁定入口: mode={:?} cursor={:?} tool={}",
        view.mode,
        view.cursor_world,
        point_select.tool_active
    );
    // 双击侧消费（ADR-0097）：本次点击是否泛洪改写了选区 + 锚定格。
    // 在 Topographic 块内赋值；块外（双击后果段）只读。
    let mut sel_changed = false;
    let mut anchor_cell: Option<(i32, i32)> = None;

    // ── CellMap 裁定（[ADR-0090] D2 修订 [ADR-0074] 追记四）：标签 > 图钉 > 群系区 ──
    // 仅 Topographic 档。格子两档（Alt 单格 / 默认泛洪）已移交圈选「单格」
    // 形状（下方 [`commit_cell_shape_click`]）——点选不再碰格子。
    if view.mode == ViewMode::Topographic {
        // 标签命中优先（ADR-0017 §2.7，与浏览族 `drill_pick` 同序）：标签是
        // 更精确的靶子，且 CellMap 的 rank≥2 地标「有 pin 必有名字」——
        // 作者看到的「东港」三个字理应可点。命中表是上一帧注册的（本系统
        // 在标签系统之前跑），按下那一刻相机尚未移动，包围盒仍然有效。
        if layers.label.visible {
            if let Some(hits) = label_hits.as_deref() {
                if let Some(target) = wv_render::label_pick::hit_test(cursor, &hits.items) {
                    sel.clear();
                    projection.selection = Some(target);
                    return;
                }
            }
        }
        if let Some(id) = pick_place_pin(
            cursor,
            &projection.world,
            target_map,
            &grid,
            layers.place.visible,
            view.scale,
            wv_render::state::lod_tier(view.scale),
        ) {
            sel.clear();
            projection.selection = Some(SelectedEntity::new(EntityKind::Place, id));
            return;
        }
        if view.nature_overlay {
            let origin = grid.map_origin;
            if let Some(id) = pick_nature_region(
                (cursor.x - origin.x, cursor.y - origin.y),
                &projection.world,
                target_map,
            ) {
                sel.clear();
                projection.selection = Some(SelectedEntity::new(EntityKind::Region, id));
                return;
            }
        }
        // ── 群系兜底（ADR-0094，用户裁定 2026-10-31「除了点中 POI，否则都
        // 应该是选择群系，哪怕它没有创建」）──
        // 非 POI 命中的点击一律选中脚下格的**同群系连通域**（wv-terrain
        // 泛洪，Biome 判据；与圈选「单格」的泛洪同一套机器）。群系区实体
        // 建了就选实体（上一段，可改属性），没建也照样有得选（本段兜底）。
        // 单焦点：写格让出实体选中。
        // ADR-0097 追记：点在**已选中**的格上不泛洪——保护既有选区（否则
        // 圈选 N 格后一碰就散，「选完再双击批量编辑」永远不成立）。
        if let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) {
            anchor_cell = Some((pick.gx, pick.gy));
            if !sel.contains(pick.gx, pick.gy) {
                let grid_terrain = crate::edit::flood_select::to_terrain_grid(terrain);
                let cells = wv_terrain::cellops::flood_region(
                    &grid_terrain,
                    pick.gx,
                    pick.gy,
                    wv_terrain::cellops::FloodCriterion::Biome,
                );
                if !cells.is_empty() {
                    sel.replace(cells);
                    projection.selection = None;
                    sel_changed = true;
                    seq_changed = true;
                }
            }
        }
    }

    // ── 双击后果（裁定之后分流，ADR-0089 D2 + ADR-0094/0096 追记）──
    // 实体选中 → Inspector（既有语义）；群系格/选区 → **居中编辑 dialog**
    // （ADR-0097：四页签 群系/POI/单格/批量，页签按 decide_tab 裁决——泛洪
    // 改写选区 = 命名流；点在已选格 = 单格/批量）。
    if is_double {
        if projection.selection.is_some() {
            if let Some(req) = inspector_req.as_deref_mut() {
                req.request();
            }
        } else if view.mode == ViewMode::Topographic && !sel.is_empty() {
            if let Some(dlg) = cell_dlg.as_deref_mut() {
                let tab = crate::ui::cell_edit_dialog::decide_tab(
                    sel_changed || seq_changed,
                    sel.len() as usize,
                    dlg.tab,
                );
                if let Some(anchor) = anchor_cell {
                    dlg.open_at(tab, anchor);
                }
            }
        }
    }
    // 序列累计写回（双击已消费/新序列起点均由此归位）。
    dbl.1 = seq_changed;
}

/// 圈选「单格」形状的点击处理器（[ADR-0090] D3；2026-10-30 用户裁定：单格
/// 为圈选第四形状变体，与多边形/矩形/椭圆同级——单击直选一格，不产草稿、
/// 不成环；拖拽手势在 wv-render 侧已被 `SelectionShape::Cell` 排除）。
///
/// 判据分工（泛洪裁量，沿 [ADR-0088] D4「执行侧裁量、零能力损失、待实机
/// 验收」先例）：
/// - [`wv_world_cell_map::state::FloodCriterion::Off`]（**新默认**）→ 点击 =
///   直选单格，修饰键走「选区加减法」：无修饰=替换、Shift=并、Alt=差；
///   点图外不动选区；
/// - 判据 ≠ Off → 点击 = 泛洪（[ADR-0074] 追记四 / [ADR-0088] 的泛洪手感
///   由判据档位承载，「泛洪判据」UI 段随本工具显示）。
///
/// 单焦点模型同点选器：写格即让出实体选中。仅 Topographic 生效；工具态 =
/// `SelectionState.enabled ∧ shape == Cell`——wv-render 只声明形状，格子
/// 写入在 wv-app（ADR-0054 反向依赖红线）。PointerOverUi / 播放中 / 加载中
/// 守卫与 [`commit_point_select_on_click`] 同口径。
#[allow(clippy::too_many_arguments)]
pub fn commit_cell_shape_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    mut projection: ResMut<WorldProjection>,
    select_env: Res<wv_render::state::SelectionState>,
    flood_select: Res<crate::state::FloodSelectState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut sel: ResMut<crate::state::CellSelection>,
    screen: Res<crate::app_screen::AppScreen>,
    mut link: ResMut<super::region::LassoLiveLink>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if screen.shows_loading_overlay() {
        return;
    }
    // 工具态：圈选激活且形状 = 单格（ADR-0090 D3）。
    if !(select_env.enabled && select_env.shape == wv_render::state::SelectionShape::Cell) {
        return;
    }
    if view.mode != ViewMode::Topographic {
        return;
    }
    if ui.wants_pointer {
        return;
    }
    if playback.playing {
        // 时间游标在动 → 当前 world 是投影 → 不要写。
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(target_map) = drill.current() else {
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        // 子图继承父图栅格时 `terrain_of` 会返 None——无地形可选。
        return;
    };
    // 图绑定兜底：正常每帧由 `sync_cell_region` 幂等绑定；本系统跑在渲染
    // 之前时（首帧 / 测试）也得保证写进的是当前图的位图。
    sel.bind_map(target_map, terrain.width, terrain.height);
    // 新选择动作 → 断开套索活链接（ADR-0057 补记 5）。
    link.deactivate();

    let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) else {
        return; // 点在图外不动选区（误触不该清空）
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);

    // 判据翻译：Off = 直选单格；其余映射到 wv-terrain 的泛洪判据。
    let flood_criterion = match flood_select.criterion {
        wv_world_cell_map::state::FloodCriterion::Off => None,
        wv_world_cell_map::state::FloodCriterion::Surface => {
            Some(wv_terrain::cellops::FloodCriterion::Surface)
        }
        wv_world_cell_map::state::FloodCriterion::Biome => {
            Some(wv_terrain::cellops::FloodCriterion::Biome)
        }
        wv_world_cell_map::state::FloodCriterion::HeightTolerance(tol) => {
            Some(wv_terrain::cellops::FloodCriterion::HeightTolerance(tol))
        }
    };

    let Some(criterion) = flood_criterion else {
        // 直选单格（替换 / Shift=并 / Alt=差，「选区加减法」口径）。
        let cell = [(pick.gx, pick.gy)];
        if shift {
            sel.union(cell);
        } else if alt {
            sel.subtract(cell);
        } else {
            sel.replace(cell);
        }
        // 单焦点：写格即让出实体焦点。
        projection.selection = None;
        return;
    };

    // 泛洪：点谁淹谁（同判据连通区进格选区；Shift = 并入）。
    let grid_terrain = crate::edit::flood_select::to_terrain_grid(terrain);
    let cells = wv_terrain::cellops::flood_region(&grid_terrain, pick.gx, pick.gy, criterion);
    if cells.is_empty() {
        return;
    }
    if shift {
        sel.union(cells);
    } else {
        sel.replace(cells);
    }
    // 单焦点：写格即让出实体焦点（同格裁定口径）。
    projection.selection = None;
}

/// **Esc 清空统一选区**（ADR-0057 补记 3：全新增入口，非沿用）。
///
/// 会话态生命周期（D1 / D5）：Esc 清空、切图清空、**切工具保留**。
/// [ADR-0074] 起 Esc 同时清实体选中（单焦点模型的两面一并归零）。
///
/// 优先级（补记 3）：`低于「进行中手势的取消」`——
/// - 拖绘进行中（`CellPaintSession.stroke.active`）→ Esc 归笔触丢弃；
/// - 套索 / 框选进行中（草稿非空 / 已闭合 / 正拖锚）→ Esc 归手势；
/// - 两者都空闲时，Esc 才清空选区。
///
/// **排序纪律**：本系统必须排在 `wv_render::selection::commit_selection_click`
/// 与 `paint_edit_cell_on_drag` **之前**。它们在同帧处理 Esc 后会把手势态清掉，
/// 排在其后会让本系统看到「无手势」而顺便清选区——一次按键两个效果。
pub fn clear_cell_selection_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    selection: Res<crate::state::SelectionState>,
    session: Res<wv_world_cell_map::cell_paint::CellPaintSession>,
    mut sel: ResMut<crate::state::CellSelection>,
    mut projection: ResMut<WorldProjection>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    // 手势优先：Esc 先取消进行中的笔触 / 圈选，不动已提交的选区。
    if session.stroke.active {
        return;
    }
    if selection.closed.is_some() || !selection.draft.is_empty() || selection.drag.is_some() {
        return;
    }
    sel.clear();
    projection.selection = None;
}

/// **每帧**追踪 hover 格（[ADR-0052] D2 写入侧；ADR-0057 加宽 gate）。
///
/// hover 是**持续**态——光标在格上移动就得跟着变，故需一个每帧跑的系统。
///
/// 写入 [`EditCellState::hover`]，供渲染侧 `sync_cell_hover` 画双色描边
/// + 准星（D2）。**EditCell 与 PointSelect 共用这一路准星**（补记 6：
///   点选也要「光标压哪格」的瞬时反馈）。
///
/// gate 与点选同口径：
/// - EditCell 与 PointSelect 都关 → 不写（工具没开就别画准星）；
/// - 指针压在 egui 面板上 → 不写（面板交互不是选格）；
/// - 时间轴播放中 → 不写（投影态不写世界）；
/// - 光标不在图上 / 无图 / 无地形 → 写 `None`（清掉旧准星）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn track_edit_cell_hover(
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    ui: Res<wv_render::camera::PointerOverUi>,
    playback: Res<PlaybackState>,
    point_select: Res<crate::state::PointSelectState>,
    mut edit_cell: ResMut<EditCellState>,
) {
    if !(edit_cell.tool_active || point_select.tool_active) {
        if edit_cell.hover.is_some() {
            edit_cell.hover = None;
        }
        return;
    }
    if ui.wants_pointer || playback.playing {
        if edit_cell.hover.is_some() {
            edit_cell.hover = None;
        }
        return;
    }
    let Some(cursor) = view.cursor_world else {
        edit_cell.hover = None;
        return;
    };
    let Some(target_map) = drill.current() else {
        edit_cell.hover = None;
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        edit_cell.hover = None;
        return;
    };
    edit_cell.hover =
        cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain).map(|pick| (pick.gx, pick.gy));
}
