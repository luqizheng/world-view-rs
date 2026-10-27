//! 点选落笔 + hover 追踪（ADR-0057：点选进统一选区，EditCell 只画不选）。
//!
//! [ADR-0074]：CellMap（Topographic）工作区里点选器升级为**三段裁定**——
//! 图钉（点状 POI）> 群系区（Nature Region）> 格。浏览态不在此列：实体
//! 选中已由 `drill_pick` 的标签命中承担，本模块不改浏览态行为。

use super::*;
use wv_core::model::region::RegionKind;
use wv_core::{EntityKind, Id, SelectedEntity};
use wv_render::layer_frame::LayerFrame;
use wv_render::state::ViewMode;

/// 图钉命中半径（屏幕像素）。比视觉图钉略大一圈——点「图钉旁边一点」也
/// 算点中，降低小目标瞄准成本；换算世界单位乘 `view.scale`（同
/// `selection::PICK_TOLERANCE_PX` 口径）。
const PIN_HIT_PX: f32 = 18.0;

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

/// 在当前图的**可见图钉**里命中离光标最近者（[ADR-0074] 裁定第一段）。
///
/// 「可见图钉」与 `world_projection` 进 `landmark_dots` 的过滤同一口径：
/// 锚定本图 + `rank ≥ 2` + 点状（`line.is_none()`——线段 POI 没有图钉）；
/// 外加 place 图层可见（层关了图钉不画，点「看不见的东西」不该有反应）。
/// 与 `LabelHits` 不同，本命中不走注册表：点击是单发动作，直接遍历当前
/// 图锚定桶即可，无每帧注册成本。
pub fn pick_place_pin(
    cursor_world: Vec2,
    world: &wv_core::World,
    map_id: &Id,
    grid: &GridConfig,
    place_layer_visible: bool,
    scale: f32,
) -> Option<Id> {
    if !place_layer_visible {
        return None;
    }
    let radius = PIN_HIT_PX * scale;
    let r2 = radius * radius;
    // 最近者胜：`best_d2` 单独存距离（`Option<Id>` 配 `f32::INFINITY` 初值，
    // 免元组解构与 map_or）。
    let mut best_d2 = f32::INFINITY;
    let mut best: Option<Id> = None;
    for p in &world.distances.places {
        if p.map.as_ref() != Some(map_id) || p.line.is_some() || p.effective_rank() < 2 {
            continue;
        }
        let Some((x, y)) = p.effective_anchor() else {
            continue;
        };
        let pos = grid.map_to_view(Vec2::new(x, y));
        let d2 = pos.distance_squared(cursor_world);
        if d2 <= r2 && d2 < best_d2 {
            best_d2 = d2;
            best = Some(p.id.clone());
        }
    }
    best
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

/// 点选工具的鼠标点击处理器（ADR-0057 补记 6；原 `commit_edit_cell_on_click`）。
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
/// **CellMap 三段裁定**（[ADR-0074]，仅 `Topographic` 档生效）：格之上
/// 先试实体命中——**图钉 > 群系区 > 格**。命中图钉或群系区 → 写
/// `WorldProjection.selection`（Place / Region）并清空格选区。实体命中
/// **不吃修饰键**：Shift/Alt 的并/差语义只对格有意义，压着修饰点图钉的
/// 作者意图是「选那个钉」，不是「往选区里并一个钉底下的格」。
/// 单焦点模型：实体选中与格选区互斥——`resolve_form` 里 Cell 表单
/// 优先于实体选中（设计稿 §11 Q4），不清格选区 = 表单看不见切换。
///
/// 修饰键走 Bevy `ButtonInput<KeyCode>`（ShiftLeft/Right、AltLeft/Right）——
/// **不读 egui 键位链**（2026-10-24 真机 panic 前科：egui 上下文在系统里
/// 的可用性依赖装配顺序，Bevy 输入资源是硬保证）。
///
/// 播放中只读（基线 §1.8 / V37）：直接跳过,不写选区。
/// PointerOverUi：面板点击不吃成选格（与 drill / brush_cursor 同口径）。
#[allow(clippy::too_many_arguments)]
pub fn commit_point_select_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
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
) {
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
    // 图绑定兜底：正常每帧由 `sync_cell_region` 幂等绑定；本系统跑在渲染
    // 之前时（首帧 / 测试）也得保证写进的是当前图的位图。
    sel.bind_map(target_map, terrain.width, terrain.height);
    // 新选择动作 → 断开套索活链接（ADR-0057 补记 5）。
    link.deactivate();

    // ── CellMap 三段裁定（[ADR-0074]）：图钉 > 群系区 > 格 ──────────
    // 仅 Topographic 档：浏览态的实体选中由 `drill_pick` 标签命中承担，
    // 这里重复做会跟它抢点击。
    if view.mode == ViewMode::Topographic {
        if let Some(id) = pick_place_pin(
            cursor,
            &projection.world,
            target_map,
            &grid,
            layers.place.visible,
            view.scale,
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
    }

    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);

    if let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) {
        let cell = [(pick.gx, pick.gy)];
        if shift {
            sel.union(cell);
        } else if alt {
            sel.subtract(cell);
        } else {
            sel.replace(cell);
        }
        // 单焦点（[ADR-0074]）：格选区非空时 `resolve_form` 直出 Cell 表单，
        // 实体选中会变成看不见的悬挂态——选格即让出实体焦点。
        projection.selection = None;
    } else if !shift && !alt {
        // 点在地图外（无修饰）= 清空选区；带修饰时点空白不动既有选区
        // （Shift/Alt 是「在既有基础上增/减」，空白处无格可增，也不应误清）。
        sel.clear();
        projection.selection = None;
    }
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
