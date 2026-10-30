//! 选择工具（pen 方法）的交互层（2026-10-17 选择工具 spec §3；M1 Task 2/3）。
//!
//! ## 职责
//! - 纯命中判定：[`pick_anchor`]（锚点）/ [`pick_edge_insertion`]（点边插入位）。
//! - 单击核心语义 [`handle_click`]：Closed 态的 选锚/双击删锚/点边插锚/点空白取消，
//!   Drawing 态的 落点/点首锚闭合/双击收笔。
//! - [`hit_tests_claim_click`]：`drill_pick` 的让位守卫（spec §3 命中优先级：
//!   锚点/边 > 图标下钻 > 空白）。
//! - （Task 3）输入/绘制系统：`commit_selection_click` / `drag_selected_anchor` /
//!   `sync_selection_map_change` / `draw_selection`。
//!
//! ## 与 `continent_sketch` / `measurement` 的对照
//!
//! 共用同一条早退链（UI 指针 / 播放 / 缩放下限 / 无工程闸门 / `map_origin` 同源链）。
//!
//! ## 边界
//! - 不生成地形、不碰 `History`、不落盘——生成类意图由 wv-app 只读克隆环后
//!   自行提交（ADR-0033：不再消费选区；AGENTS §2.1 红线）。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{
    DrillState, GridConfig, PlaybackState, SelectionShape, SelectionState, ShapeDrag, ViewState,
    WorldProjection,
};

/// 锚点 / 边命中容差（屏幕像素）——调用方按 `view.scale` 反比换算成 wu。
pub const PICK_TOLERANCE_PX: f32 = 8.0;
/// 首锚闭合吸附半径（屏幕像素）。
pub const CLOSE_SNAP_PX: f32 = 10.0;
/// 右键点击的最大位移（屏幕像素）——松手时按住期间移动不超过它才算
/// 「点击」，超过即右键拖拽平移（平移按键口径：右键 / 中键始终保留给
/// 平移，2026-10-14 裁定；撤锚不得吃掉平移，2026-10-24）。单点定义在
/// [`crate::input::CLICK_MAX_PX`]（ADR-0089 D1），旧名保留为交互语义别名。
pub use crate::input::CLICK_MAX_PX as RIGHT_CLICK_MAX_PX;
/// 双击判定窗口（秒）与位移容差（屏幕像素）——单点定义在 [`crate::input`]
/// （ADR-0089 D1）；位移容差调用方按 `view.scale` 换算成 wu 再传入
/// [`handle_click`]。原 12 wu 世界定长在深缩放下小于像素抖动（M1 终审 minor，
/// M2 清偿）。
pub use crate::input::{DOUBLE_CLICK_SECS, DOUBLE_CLICK_SLOP_PX as DOUBLE_CLICK_PX};

/// 椭圆选环的分段数（ADR-0043）。
///
/// 48 段在 400px 直径下每段约 26px，肉眼看不出棱角；再密只是白费顶点。
/// 定值而非自适应：选区环要参与后续的点在多边形内判定与 CellMap 批量改，
/// **点数必须可预测**，否则同一块地在不同缩放下命中结果会漂。
pub const ELLIPSE_SEGMENTS: usize = 48;

/// 框选「拖出」判为有效的最小跨度（屏幕像素）。
///
/// 小于这个尺寸的拖拽视为误触（手抖一下），不产生一个退化选区。
/// 调用方按 `view.scale` 换算成 wu。
pub const MARQUEE_MIN_SPAN_PX: f32 = 6.0;

// ── 框选形状的几何（ADR-0043） ──────────────────────────────────────────

/// 两角点 → 矩形环（逆时针 4 角）。
///
/// **两角任意顺序**：`(10,20)` 与 `(20,10)` 必须给出同一个矩形，所以这里先取
/// min/max 再排角。返回的点序恒为 左下 → 右下 → 右上 → 左上，与 Y 向上坐标系
/// 一致（winding 只影响有向面积，而多边形求面积的调用方自己定符号）。
pub fn rect_ring(a: Vec2, b: Vec2) -> Vec<(f32, f32)> {
    let (x0, x1) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
    let (y0, y1) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// 两角点 → 内接椭圆环（[`ELLIPSE_SEGMENTS`] 段）。
///
/// 圆心取两角中点，半轴取半宽 / 半高——所以 `a`、`b` 是椭圆**外接矩形**的两
/// 对角，与「拖出框就是那个框」的直觉一致（不是内切）。
///
/// 从角度 0（正右）起、逆时针等分；首点恰好落在 `cx + rx`，末点落在其下方一步，
/// 闭边由渲染端连回首点。
pub fn ellipse_ring(a: Vec2, b: Vec2) -> Vec<(f32, f32)> {
    let c = (a + b) * 0.5;
    let rx = (b.x - a.x).abs() * 0.5;
    let ry = (b.y - a.y).abs() * 0.5;
    let n = ELLIPSE_SEGMENTS;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            (c.x + rx * t.cos(), c.y + ry * t.sin())
        })
        .collect()
}

/// 点是否在环内（射线法）。
///
/// 用于两处：框选的「环内按下 = 整体平移」判定，以及对外暴露的
/// 「这一点被选中了吗」。环**无需**是凸的——射线法对任意简单多边形都成立，
/// 所以作者用多边形画出的凹形选区也判得对。
///
/// 跨扫描线的判定用 `(ay > py) != (by > py)`（半开区间）：顶点恰好落在扫描线上
/// 时只数一次，否则一条穿过顶点的水平线会把一条边算两遍、得出错误结果。
///
/// **恰好落在水平边上的点归属不确定**（取决于边的朝向）。选区交互不依赖这条
/// 边界——环内判定只用于「整体拖动 vs 取消」，而沿边拖动时手柄命中（见
/// [`near_ring`]）会先一步接住。
pub fn point_in_ring(ring: &[(f32, f32)], p: (f32, f32)) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let (px, py) = p;
    let mut inside = false;
    for (i, &a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        let (ax, ay) = a;
        let (bx, by) = b;
        if (ay > py) != (by > py) {
            // 边跨越扫描线 → 算一次交点，看它是否在点的右边。
            let t = (py - ay) / (by - ay);
            let x = ax + t * (bx - ax);
            if px < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// 环的轴对齐包围盒（左下, 右上）。空环返回 `None`。
pub fn ring_bounds(ring: &[(f32, f32)]) -> Option<(Vec2, Vec2)> {
    if ring.is_empty() {
        return None;
    }
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for &(x, y) in ring {
        min.x = min.x.min(x);
        min.y = min.y.min(y);
        max.x = max.x.max(x);
        max.y = max.y.max(y);
    }
    Some((min, max))
}

/// 环上距 `p` 最近的点是否在容差内（框选形状的「拖手柄缩放」命中）。
pub fn near_ring(ring: &[(f32, f32)], p: (f32, f32), tol: f32) -> bool {
    ring.iter().any(|&q| dist2(p, q) <= tol * tol)
}

/// 框选环的**缩放手柄**——矩形与椭圆都取包围盒的 4 个角。
///
/// 椭圆也用包围盒角而不是四个极点，理由是**一致**：用户拖的是「框的角」，
/// 不是「椭圆上的某个极值点」。手柄落在包围盒角上（椭圆本体之外）是业界选框
/// 的常规画法；拖角之后椭圆重新内接进新框，形状与手柄始终对得上。
///
/// 多边形没有手柄——它的编辑模型是逐锚点（拖 / 删 / 插），见
/// [`pick_anchor`] / [`pick_edge_insertion`]。
pub fn shape_handles(shape: SelectionShape, ring: &[(f32, f32)]) -> Vec<(f32, f32)> {
    if shape == SelectionShape::Polygon || ring.len() < 3 {
        return Vec::new();
    }
    let Some((min, max)) = ring_bounds(ring) else {
        return Vec::new();
    };
    vec![
        (min.x, min.y),
        (max.x, min.y),
        (max.x, max.y),
        (min.x, max.y),
    ]
}

/// 命中最近锚点（容差内取距离最小者）。
pub fn pick_anchor(pts: &[(f32, f32)], p: (f32, f32), tol: f32) -> Option<usize> {
    let (best, _) = pts.iter().enumerate().min_by_key(|&(_, &q)| {
        (dist2(p, q) * 1e6) as u64 // 放大取整，避免浮点序不稳定
    })?;
    (dist2(p, pts[best]) <= tol * tol).then_some(best)
}

/// 点边插入位：返回 `splice ∈ 1..=len`（新点落在 `pts[splice-1]` 与
/// `pts[splice % len]` 之间）；容差内取距离最小的边。
pub fn pick_edge_insertion(pts: &[(f32, f32)], p: (f32, f32), tol: f32) -> Option<usize> {
    let len = pts.len();
    if len < 2 {
        return None;
    }
    let mut best: Option<(f32, usize)> = None;
    for i in 0..len {
        let d = dist_point_segment(p, pts[i], pts[(i + 1) % len]);
        if d <= tol && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i + 1));
        }
    }
    best.map(|(_, splice)| splice)
}

/// `drill_pick` 的让位守卫（spec §3 命中优先级）：画中全吃；Closed 态
/// 锚点 / 边命中吃，其余放行给标签与下钻。
///
/// **框选形状（ADR-0043）整块吃掉**：成形后的矩形 / 椭圆是个整体，没有逐锚点
/// 可让位，于是「落在形状内的任何一点」都归选择工具。这正是 Figma 的行为——
/// 点选区内部不该穿透下去触发下钻。多边形保持原口径（只有锚点 / 边才吃），
/// 因为它有逐锚点编辑，语义是「点空白 = 取消选区」。
pub fn hit_tests_claim_click(sel: &SelectionState, local: (f32, f32), tol: f32) -> bool {
    if sel.is_drawing() {
        return true;
    }
    if !sel.shape.is_polygon() {
        // 成形中（正按住）已由上面的 `is_drawing` 接住——注意框选成形走的是
        // `drag` 而不是 `draft`，所以这里只判闭合态。
        return sel
            .closed
            .as_deref()
            .is_some_and(|ring| point_in_ring(ring, local) || near_ring(ring, local, tol));
    }
    match &sel.closed {
        Some(ring) => {
            pick_anchor(ring, local, tol).is_some()
                || pick_edge_insertion(ring, local, tol).is_some()
        }
        None => false,
    }
}

/// 左键单击的核心语义（系统只做守卫与坐标转换）。
///
/// - Closed 态：锚点命中 → 单击选中（记 `last_click` 供双击判定）、双击删除；
///   边命中 → 插入并选中新锚点；空白 → 取消整个选区。
/// - Drawing 态：≥3 点且点首锚吸附内 → 闭合；双击 → 收笔；否则落点。
pub fn handle_click(
    sel: &mut SelectionState,
    p: (f32, f32),
    now: f64,
    tol: f32,
    snap: f32,
    dbl_tol: f32,
) {
    let is_double = match sel.last_click {
        Some(((lx, ly), t)) => {
            (p.0 - lx).hypot(p.1 - ly) <= dbl_tol && now - t <= DOUBLE_CLICK_SECS
        }
        None => false,
    };

    if let Some(ring) = sel.closed.clone() {
        // ====== Closed 态 ======
        if let Some(i) = pick_anchor(&ring, p, tol) {
            if is_double {
                sel.delete_anchor(i);
                sel.selected_anchor = None;
                // 复位双击判定——否则 0.4s 内的第三击再判成双击，连删第二个
                // 锚点（终审 Important 1）。
                sel.last_click = None;
            } else {
                sel.selected_anchor = Some(i);
                sel.last_click = Some((ring[i], now));
            }
        } else if let Some(splice) = pick_edge_insertion(&ring, p, tol) {
            sel.insert_anchor(splice, p);
            sel.selected_anchor = Some(splice);
            sel.last_click = None;
        } else {
            sel.cancel();
        }
        return;
    }

    // ====== Drawing 态 ======
    if sel.draft.len() >= 3 && dist2(p, sel.draft[0]) <= snap * snap {
        sel.try_close();
        sel.last_click = None;
    } else if is_double {
        // 双击收笔：<3 点的失败收笔会丢弃草稿（`try_close` 语义）。
        sel.try_close();
        sel.last_click = None;
    } else {
        sel.add_point(p);
        sel.last_click = Some((p, now));
    }
}

/// 右键点击 = 撤回画中上一锚点（2026-10-24 多边形手感改造）。
///
/// 平移占着右键**按住拖拽**（2026-10-14 平移按键裁定），所以「点击」与
/// 「拖拽」必须分开结算：按下时系统把光标世界坐标记进
/// [`SelectionState::right_press`]，松手时交给本函数——按住期间位移
/// ≤ `max_move`（世界单位）判为点击，弹出最后一枚锚点；超过即拖拽平移，
/// 什么都不动。闭合态 / 非多边形 / 空草稿一律不吃。
///
/// 手势记录无论结算结果如何都清掉（`take`）——半途手势不留到下一次。
/// 返回是否真弹了锚点（测试锚；系统不消费返回值）。
pub fn right_click_pop_anchor(
    sel: &mut SelectionState,
    cursor: Option<Vec2>,
    max_move: f32,
) -> bool {
    let Some(press) = sel.right_press.take() else {
        return false;
    };
    let Some(cursor) = cursor else {
        return false;
    };
    if (cursor - press).length() > max_move {
        return false;
    }
    if !sel.shape.is_polygon() || sel.is_closed() || sel.draft.is_empty() {
        return false;
    }
    sel.draft.pop();
    // 撤锚后 0.4s 内在同点重按左键不得被判成「双击收笔」——那会把整份
    // 草稿丢掉（`try_close` 的 <3 点失败语义），一次手滑毁一次圈选。
    sel.last_click = None;
    true
}

/// 距离平方（比较用，免开方）。
fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    dx * dx + dy * dy
}

/// 点到线段的最短距离（向量投影夹取）。
fn dist_point_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let abx = b.0 - a.0;
    let aby = b.1 - a.1;
    let apx = p.0 - a.0;
    let apy = p.1 - a.1;
    let ab2 = abx * abx + aby * aby;
    if ab2 <= f32::EPSILON {
        return apx.hypot(apy);
    }
    let t = ((apx * abx + apy * aby) / ab2).clamp(0.0, 1.0);
    let dx = apx - t * abx;
    let dy = apy - t * aby;
    dx.hypot(dy)
}

// ====== Bevy 系统（薄壳：守卫链与坐标转换，逻辑都在上面纯函数里）======

/// 顶点半径（屏幕像素）——绘制按 `view.scale` 换算成世界单位。
const VERTEX_PX: f32 = 3.0;

/// 每帧以当前图重申选区绑定——变图即清（spec D9：切图 = 取消选择）。
pub fn sync_selection_map_change(drill: Res<DrillState>, mut sel: ResMut<SelectionState>) {
    sel.on_map_change(drill.current());
}

/// 左键 + 键盘 → [`handle_click`]。守卫链逐字照抄
/// `commit_continent_sketch_click`（UI 指针 / 播放 / 缩放下限 / 无工程闸门）。
#[allow(clippy::too_many_arguments)]
pub fn commit_selection_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    time: Res<Time>,
    mut sel: ResMut<SelectionState>,
) {
    if !sel.enabled {
        return;
    }
    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }

    // Esc：Closed → 取消整个选区；画中 → 只丢草稿（spec §3）。
    if keys.just_pressed(KeyCode::Escape) {
        if sel.is_closed() {
            sel.cancel();
        } else {
            sel.discard_draft();
            sel.drag = None;
        }
        return;
    }

    // 框选形状（ADR-0043）不走本系统——它的交互是「按住拖拽」，由
    // `commit_selection_shape_drag` 独占。若继续往下走，每次按下都会被当成
    // 一个草稿锚点，于是拖一下框出来 40 个点的多边形。
    if !sel.shape.is_polygon() {
        return;
    }
    // Backspace：画中删上一锚点。
    if keys.just_pressed(KeyCode::Backspace) {
        sel.draft.pop();
        return;
    }
    // Enter：收笔（≥3 点成环；<3 点丢弃）。
    if keys.just_pressed(KeyCode::Enter) {
        sel.try_close();
        sel.last_click = None;
        return;
    }
    // 右键：按下记录位置，松手结算「点击 = 撤锚 vs 拖拽 = 平移」。
    // 平移本身照常在 `camera_pan` 里跑（按住即平移），两边读同一份
    // `ButtonInput` 互不相犯；结算规则见 [`right_click_pop_anchor`]。
    if buttons.just_pressed(MouseButton::Right) {
        sel.right_press = view.cursor_world;
        return;
    }
    if buttons.just_released(MouseButton::Right) {
        right_click_pop_anchor(&mut sel, view.cursor_world, RIGHT_CLICK_MAX_PX * view.scale);
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor_world) = view.cursor_world else {
        return;
    };
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let origin = crate::drill::map_origin(&projection.world, node, &grid);
    let local = cursor_world - origin;
    let tol = PICK_TOLERANCE_PX * view.scale;
    let snap = CLOSE_SNAP_PX * view.scale;
    handle_click(
        &mut sel,
        (local.x, local.y),
        time.elapsed_secs_f64(),
        tol,
        snap,
        DOUBLE_CLICK_PX * view.scale,
    );
}

/// 按住左键拖动选中锚点（平移是右 / 中键的 `camera_pan`，左键拖不冲突）。
#[allow(clippy::too_many_arguments)]
pub fn drag_selected_anchor(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    mut sel: ResMut<SelectionState>,
) {
    let Some(i) = sel.selected_anchor else {
        return;
    };
    if !sel.enabled || ui.wants_pointer || playback.playing {
        return;
    }
    if !buttons.pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor_world) = view.cursor_world else {
        return;
    };
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let origin = crate::drill::map_origin(&projection.world, node, &grid);
    let local = cursor_world - origin;
    // 多边形才有逐锚点拖动；框选形状的手柄拖动由 shape_drag 独占。
    if sel.shape.is_polygon() {
        sel.move_anchor(i, (local.x, local.y));
    }
}

/// 框选形状的「按住拖拽」交互（ADR-0043 · 矩形 / 椭圆）。
///
/// 三种手势，命中优先级自高而低：
///
/// | 手势 | 触发 | 松手后 |
/// |---|---|---|
/// | [`ShapeDrag::Resize`] | 已有选区 + 按在手柄（包围盒角）附近 | 按位移重算包围盒并重造环 |
/// | [`ShapeDrag::Move`] | 已有选区 + 按在环内 | 整个环平移 |
/// | [`ShapeDrag::Draw`] | 没有选区（或选区已被点空白取消） | 按两角点造出新环 |
///
/// 点在选区**外** = 取消选区（Figma 的点空白取消），不做「从空白处拖出新框」——
/// 后者会让「点空白」这个动作有两种含义，作者每次点空白都得先猜一下上次到底
/// 取消没有。
///
/// 守卫链与 [`commit_selection_click`] 逐条一致（UI 指针 / 播放 / 缩放下限 /
/// 无工程闸门），两套输入因此不会在同一个左键上打架。
#[allow(clippy::too_many_arguments)]
pub fn commit_selection_shape_drag(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    mut sel: ResMut<SelectionState>,
) {
    if !sel.enabled || sel.shape.is_polygon() || sel.shape == SelectionShape::Cell {
        // 多边形归 `commit_selection_click`；单格（ADR-0090 D3）无拖拽手势
        // ——单击直选一格在 wv-app 侧 `commit_cell_shape_click`，这里绝不能
        // 把按下起成框选手势（否则松手会按旧手势结算出一个环）。
        // 换回多边形时若残留半个框选手势，必须丢弃——否则松手时会按旧手势结算。
        sel.drag = None;
        return;
    }
    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }
    // 没工程 / 没当前图 / 光标不在窗内：正在进行的拖拽**必须收尾**，否则松手
    // 时状态会卡在一个永远等不到 `just_released` 的手势里。
    let Some(node) = drill.current_node(&projection.world) else {
        sel.drag = None;
        return;
    };
    let Some(cursor_world) = view.cursor_world else {
        return;
    };
    let origin = crate::drill::map_origin(&projection.world, node, &grid);
    let local = cursor_world - origin;
    let tol = PICK_TOLERANCE_PX * view.scale;

    // ── 松手：结算 ────────────────────────────────────────────────────
    if buttons.just_released(MouseButton::Left) {
        let Some(drag) = sel.drag.take() else {
            return;
        };
        match drag {
            ShapeDrag::Draw { from, to } => {
                // 跨度小于 6px 视为手抖：什么都不留。
                let span = (to - from).abs();
                if span.x < MARQUEE_MIN_SPAN_PX * view.scale
                    || span.y < MARQUEE_MIN_SPAN_PX * view.scale
                {
                    return;
                }
                sel.closed = None;
                sel.rebuild_closed(from, to);
            }
            ShapeDrag::Move { from, to } => {
                sel.translate_closed(to - from);
            }
            ShapeDrag::Resize { from, to } => {
                let Some(ring) = sel.closed.clone() else {
                    return;
                };
                let Some((min, max)) = ring_bounds(&ring) else {
                    return;
                };
                let d = to - from;
                sel.rebuild_closed(min + d, max + d);
            }
        }
        return;
    }

    // ── 按住：更新 ────────────────────────────────────────────────────
    if buttons.pressed(MouseButton::Left) {
        if let Some(drag) = sel.drag.as_mut() {
            drag.set_to(local);
        }
        return;
    }

    // ── 按下：定手势 ──────────────────────────────────────────────────
    if buttons.just_pressed(MouseButton::Left) {
        match sel.closed.clone() {
            None => {
                sel.drag = Some(ShapeDrag::Draw {
                    from: local,
                    to: local,
                });
            }
            Some(ring) => {
                let on_handle = shape_handles(sel.shape, &ring)
                    .iter()
                    .any(|&h| dist2((local.x, local.y), h) <= tol * tol);
                if on_handle {
                    sel.drag = Some(ShapeDrag::Resize {
                        from: local,
                        to: local,
                    });
                } else if point_in_ring(&ring, (local.x, local.y)) {
                    sel.drag = Some(ShapeDrag::Move {
                        from: local,
                        to: local,
                    });
                } else {
                    // 点空白 = 取消。取消后这一帧**不**顺手开一个新框——
                    // 见函数头「点在选区外」的说明。
                    sel.cancel();
                }
            }
        }
    }
}

/// 绘制选区（Gizmos，与 `draw_continent_sketch` 同层口径，不做 z 排序）：
/// Drawing = 白折线 + 顶点 + 橡皮筋（末锚→光标）+ ≥3 点首尾虚线提示 +
/// 吸附圈内首锚闭合圈；Closed = 金描边环 + 顶点（选中锚点加大）。
///
/// 注：bevy Gizmos 没有多边形填充原语——spec §3 的「半透明填充」延后
/// （spec 追记已记偏差），v1 只有描边 + 顶点。
pub fn draw_selection(
    mut gizmos: Gizmos,
    view: Res<ViewState>,
    sel: Res<SelectionState>,
    grid: Res<GridConfig>,
) {
    if !sel.enabled {
        return;
    }
    let stroke = Color::WHITE.with_alpha(0.9);
    let closed = Color::srgb(1.0, 0.78, 0.18).with_alpha(0.95);
    let vertex_r = (VERTEX_PX * view.scale).max(0.5);
    let to_view = |p: (f32, f32)| grid.map_to_view(Vec2::new(p.0, p.1));

    if let Some(ring) = &sel.closed {
        let pts: Vec<Vec2> = ring.iter().map(|&q| to_view(q)).collect();
        for w in 0..pts.len() {
            gizmos.line_2d(pts[w], pts[(w + 1) % pts.len()], closed);
        }
        // 多边形：逐锚点画圆点（可拖 / 可双击删 / 可点边插）。
        // 框选形状：画 4 个包围盒角手柄。**不**画 48 个顶点——48 个一模一样的
        // 圆点既没信息量又把真正要拖的 4 个角淹了。
        if sel.shape.is_polygon() {
            for (i, sp) in pts.iter().enumerate() {
                let r = if sel.selected_anchor == Some(i) {
                    vertex_r * 1.8
                } else {
                    vertex_r
                };
                gizmos.circle_2d(Isometry2d::from_translation(*sp), r, closed);
            }
        } else {
            for h in shape_handles(sel.shape, ring) {
                gizmos.circle_2d(
                    Isometry2d::from_translation(to_view(h)),
                    vertex_r * 1.2,
                    closed,
                );
            }
        }
    }

    // 拖拽中的实时预览（ADR-0043）：把正在成形的框用半透明线画出来。
    // 只画轮廓不画手柄——手柄是成形后才有的东西。
    if let Some(drag @ (ShapeDrag::Draw { .. } | ShapeDrag::Resize { .. })) = sel.drag {
        let a = drag.from();
        let b = drag.to();
        let preview = match sel.shape {
            SelectionShape::Rect => rect_ring(a, b),
            SelectionShape::Ellipse => ellipse_ring(a, b),
            SelectionShape::Polygon | SelectionShape::Cell => Vec::new(),
        };
        let pv: Vec<Vec2> = preview.iter().map(|&q| to_view(q)).collect();
        for w in 0..pv.len() {
            gizmos.line_2d(pv[w], pv[(w + 1) % pv.len()], closed.with_alpha(0.55));
        }
    }

    if !sel.draft.is_empty() {
        let pts: Vec<Vec2> = sel.draft.iter().map(|&q| to_view(q)).collect();
        for w in pts.windows(2) {
            gizmos.line_2d(w[0], w[1], stroke);
        }
        for sp in &pts {
            gizmos.circle_2d(Isometry2d::from_translation(*sp), vertex_r, stroke);
        }
        // 橡皮筋：末锚 → 光标（spec §3 的 Figma pen 手感）。
        if let Some(cursor_world) = view.cursor_world {
            let cursor_view = grid.map_to_view(cursor_world);
            if let Some(last) = pts.last() {
                gizmos.line_2d(*last, cursor_view, stroke.with_alpha(0.6));
            }
        }
        // ≥3 点提示「再点一下即可闭合」+ 光标吸附近首锚画闭合圈。
        if pts.len() >= 3 {
            gizmos.line_2d(*pts.last().expect("非空"), pts[0], stroke.with_alpha(0.35));
            if let Some(cursor_world) = view.cursor_world {
                let snap = CLOSE_SNAP_PX * view.scale;
                if dist2((cursor_world.x, cursor_world.y), sel.draft[0]) <= snap * snap {
                    gizmos.circle_2d(Isometry2d::from_translation(pts[0]), vertex_r * 2.0, closed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 方形环：底 (0,0)-(10,0)、右 (10,0)-(10,10)、顶 (10,10)-(0,10)、左 (0,10)-(0,0)。
    fn square() -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
    }

    fn closed_square() -> SelectionState {
        SelectionState {
            closed: Some(square()),
            ..Default::default()
        }
    }

    #[test]
    fn pick_anchor_returns_index_within_tolerance() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        assert_eq!(pick_anchor(&pts, (10.2, 0.1), 0.5), Some(1), "命中 1 号锚");
        assert_eq!(pick_anchor(&pts, (0.3, 0.2), 0.5), Some(0), "命中 0 号锚");
    }

    #[test]
    fn pick_anchor_misses_beyond_tolerance() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        assert_eq!(pick_anchor(&pts, (5.0, 5.0), 0.5), None, "远离所有锚点");
    }

    #[test]
    fn pick_edge_insertion_returns_splice_index() {
        // 点 (5,10) 贴顶边（pts[2]→pts[3]）→ 插入位 = 3。
        assert_eq!(
            pick_edge_insertion(&square(), (5.0, 10.0), 1.0),
            Some(3),
            "顶边中点应插在 splice=3"
        );
    }

    #[test]
    fn pick_edge_insertion_misses_off_polygon() {
        assert_eq!(pick_edge_insertion(&square(), (20.0, 20.0), 1.0), None);
    }

    /// ≥3 点草稿点首锚（吸附半径内）= 闭合——Figma pen 的收笔手势。
    #[test]
    fn clicking_first_anchor_closes_draft() {
        let mut s = SelectionState::default();
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.add_point((10.0, 10.0));
        handle_click(&mut s, (0.0, 0.0), 10.0, 0.5, 1.0, 12.0);
        assert!(s.is_closed(), "点首锚应闭合");
        assert!(!s.is_drawing(), "闭合后草稿清空");
    }

    /// 双击（同点近时距）收笔——点首锚之外的另一条收笔路径。
    #[test]
    fn double_click_closes_draft() {
        let mut s = SelectionState::default();
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.add_point((10.0, 10.0));
        // 在远离首锚的 (10,10) 附近快速二连击：第一击已入草稿并记 last_click。
        handle_click(&mut s, (10.0, 10.0), 10.0, 0.5, 1.0, 12.0);
        handle_click(&mut s, (10.0, 10.0), 10.2, 0.5, 1.0, 12.0);
        assert!(s.is_closed(), "双击应收笔闭合");
    }

    // ── 右键撤锚（2026-10-24 多边形手感改造） ───────────────────────────

    /// 右键点击（按住期间几乎没动）= 撤回上一锚点。
    #[test]
    fn right_click_without_movement_pops_last_anchor() {
        let mut s = SelectionState::default();
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.add_point((20.0, 5.0));
        // 按下记位（系统职责），松手时几乎原位（亚像素抖动）：判为点击。
        s.right_press = Some(Vec2::new(20.0, 5.0));
        assert!(right_click_pop_anchor(
            &mut s,
            Some(Vec2::new(20.6, 4.8)),
            6.0
        ));
        assert_eq!(s.draft.len(), 2, "末锚应被弹出，实得 {}", s.draft.len());
        assert_eq!(s.draft.last(), Some(&(10.0, 0.0)), "弹出的是最后一枚");
        assert_eq!(s.right_press, None, "手势记录必须清掉");
    }

    /// 右键按住拖远 = 平移，不撤锚（平移按键裁定不被撤锚吃掉）。
    #[test]
    fn right_drag_beyond_tolerance_keeps_the_draft() {
        let mut s = SelectionState::default();
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.right_press = Some(Vec2::new(10.0, 0.0));
        assert!(!right_click_pop_anchor(
            &mut s,
            Some(Vec2::new(110.0, 0.0)),
            6.0
        ));
        assert_eq!(s.draft.len(), 2, "拖拽平移不得吃锚点");
        assert_eq!(s.right_press, None, "手势记录同样要清");
    }

    /// 闭合态 / 空草稿 / 框选形状：右键点击一律不吃。
    #[test]
    fn right_click_ignores_closed_empty_and_non_polygon_states() {
        let mut closed = closed_square();
        closed.right_press = Some(Vec2::ZERO);
        assert!(!right_click_pop_anchor(&mut closed, Some(Vec2::ZERO), 6.0));
        assert!(closed.is_closed(), "闭合选区不受右键影响");

        let mut empty = SelectionState {
            right_press: Some(Vec2::ZERO),
            ..Default::default()
        };
        assert!(!right_click_pop_anchor(&mut empty, Some(Vec2::ZERO), 6.0));

        let mut rect = SelectionState {
            shape: SelectionShape::Rect,
            ..Default::default()
        };
        rect.rebuild_closed(Vec2::ZERO, Vec2::splat(10.0));
        rect.right_press = Some(Vec2::ZERO);
        assert!(
            !right_click_pop_anchor(&mut rect, Some(Vec2::ZERO), 6.0),
            "框选形状没有逐锚点语义"
        );
    }

    /// 撤锚后 `last_click` 复位——0.4s 内同点左键不得被判成双击收笔
    /// 把整份草稿丢掉（`try_close` 的 <3 点失败语义）。
    #[test]
    fn popping_anchor_resets_the_double_click_window() {
        let mut s = SelectionState::default();
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.last_click = Some(((10.0, 0.0), 99.0));
        s.right_press = Some(Vec2::new(10.0, 0.0));
        assert!(right_click_pop_anchor(
            &mut s,
            Some(Vec2::new(10.0, 0.0)),
            6.0
        ));
        assert_eq!(s.last_click, None, "撤锚必须复位双击判定");
    }

    /// 单击锚点 = 选中；紧接着的双击 = 删除该锚点（4 环删 1 剩 3，保持闭合）。
    #[test]
    fn clicking_anchor_selects_and_double_click_deletes_it() {
        let mut s = closed_square();
        handle_click(&mut s, (10.0, 0.0), 10.0, 0.5, 1.0, 12.0);
        assert_eq!(s.selected_anchor, Some(1), "单击应选中 1 号锚");
        handle_click(&mut s, (10.0, 0.0), 10.2, 0.5, 1.0, 12.0);
        assert_eq!(
            s.closed.as_ref().map(Vec::len),
            Some(3),
            "双击删除后剩 3 点仍闭合"
        );
        assert_eq!(s.selected_anchor, None, "删除后选中复位");
    }

    /// 终审 Important 1：双击删除后 `last_click` 必须复位——否则 0.4s 内的
    /// 第三击又判成双击，连删第二个锚点；若第一删已把 3 环降级成 2 点草稿，
    /// 失败的 try_close 还会把剩余草稿整个丢掉（一次手滑丢作品）。
    #[test]
    fn triple_click_after_anchor_delete_does_not_delete_twice() {
        let mut s = closed_square();
        handle_click(&mut s, (10.0, 0.0), 10.0, 0.5, 1.0, 12.0); // 单击选中 1 号锚
        handle_click(&mut s, (10.0, 0.0), 10.15, 0.5, 1.0, 12.0); // 双击 → 删 1 号（剩 3 点）
                                                                  // 第三击落在原 2 号锚（删除后前移为 1）：应只是选中，不得再删。
                                                                  // （时刻取 10.3：距首击 0.3 < 0.4s 窗口；避开 10.4-10.0 的 f64 边界。）
        handle_click(&mut s, (10.0, 10.0), 10.3, 0.5, 1.0, 12.0);
        assert_eq!(
            s.closed.as_ref().map(Vec::len),
            Some(3),
            "三连击不得连删第二个锚点"
        );
        assert_eq!(s.selected_anchor, Some(1), "第三击应按单击语义选中该锚点");
    }

    /// 点击边线 = 该处插入锚点并选中（随后可拖）。
    #[test]
    fn clicking_edge_inserts_and_selects_anchor() {
        let mut s = closed_square();
        handle_click(&mut s, (5.0, 10.0), 10.0, 0.5, 1.0, 12.0);
        assert_eq!(s.closed.as_ref().map(Vec::len), Some(5), "插入后 5 点");
        assert_eq!(s.selected_anchor, Some(3), "新锚点应被选中");
        assert_eq!(s.last_click, None, "插入不走双击判定");
    }

    /// Closed 态点空白 = 取消整个选区（spec D9）。
    #[test]
    fn clicking_empty_cancels_closed_selection() {
        let mut s = closed_square();
        handle_click(&mut s, (50.0, 50.0), 10.0, 0.5, 1.0, 12.0);
        assert!(!s.is_closed(), "点空白应取消选区");
    }

    /// drill 守卫（spec §3 命中优先级）：画中全吃；Closed 态锚点/边命中吃，
    /// 远处放行给下钻/标签。
    #[test]
    fn hit_tests_claim_click_in_drawing_and_on_anchors() {
        let mut drawing = SelectionState::default();
        drawing.add_point((0.0, 0.0));
        assert!(
            hit_tests_claim_click(&drawing, (100.0, 100.0), 0.5),
            "画中全吃"
        );

        assert!(
            hit_tests_claim_click(&closed_square(), (10.1, 0.0), 0.5),
            "锚点命中"
        );
        assert!(
            hit_tests_claim_click(&closed_square(), (5.0, 10.0), 0.5),
            "边线命中"
        );
        assert!(
            !hit_tests_claim_click(&closed_square(), (50.0, 50.0), 0.5),
            "远处放行"
        );
        assert!(
            !hit_tests_claim_click(&SelectionState::default(), (1.0, 1.0), 0.5),
            "空闲态放行"
        );
    }

    // ── 框选形状（ADR-0043） ────────────────────────────────────────────

    /// 矩形环：4 个角、两角顺序无关、包围盒正好是两角的外接矩形。
    #[test]
    fn rect_ring_has_four_axis_aligned_corners_in_either_drag_order() {
        let a = rect_ring(Vec2::new(2.0, 3.0), Vec2::new(12.0, 23.0));
        let b = rect_ring(Vec2::new(12.0, 23.0), Vec2::new(2.0, 3.0));
        assert_eq!(a, b, "反向拖拽必须给出同一个矩形");
        assert_eq!(a.len(), 4, "矩形是 4 个角，实得 {}", a.len());
        assert_eq!(a, vec![(2.0, 3.0), (12.0, 3.0), (12.0, 23.0), (2.0, 23.0)]);
        let (min, max) = ring_bounds(&a).expect("非空环");
        assert_eq!(min, Vec2::new(2.0, 3.0));
        assert_eq!(max, Vec2::new(12.0, 23.0));
    }

    /// 椭圆环：段数固定、半轴 = 两角半宽半高、包围盒与拖拽框重合。
    ///
    /// 段数固定是**契约**不是实现细节——选环要喂给「点在多边形内」判定与
    /// CellMap 批量改，点数可预测才能保证同一块地命中稳定。
    #[test]
    fn ellipse_ring_has_fixed_segments_and_matches_the_drag_box() {
        let a = ellipse_ring(Vec2::new(0.0, 0.0), Vec2::new(20.0, 10.0));
        assert_eq!(a.len(), ELLIPSE_SEGMENTS, "段数应固定为 {ELLIPSE_SEGMENTS}");
        let (min, max) = ring_bounds(&a).expect("非空环");
        assert_eq!(min, Vec2::new(0.0, 0.0), "内接框应与拖拽框左下重合");
        assert_eq!(max, Vec2::new(20.0, 10.0), "内接框应与拖拽框右上重合");
        // 首点在正右极值（圆心 (10,5) + 半轴 x=10）。
        assert!(
            (a[0].0 - 20.0).abs() < 1e-4,
            "首点应落在正右，实得 {:?}",
            a[0]
        );
        assert!(
            (a[0].1 - 5.0).abs() < 1e-4,
            "首点 y 应为圆心，实得 {:?}",
            a[0]
        );
        // 半轴长：最左/最上/最下极值。
        let min_x = a.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
        let min_y = a.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let max_y = a.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
        assert!((min_x - 0.0).abs() < 1e-3, "最左应贴框，实得 {min_x}");
        assert!((min_y - 0.0).abs() < 1e-3, "最下应贴框，实得 {min_y}");
        assert!((max_y - 10.0).abs() < 1e-3, "最上应贴框，实得 {max_y}");
    }

    /// 射线法：正方形内外判定 + 凹多边形（射线法对非凸环同样成立）。
    #[test]
    fn point_in_ring_separates_inside_from_outside_and_handles_concave_rings() {
        let sq = rect_ring(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(point_in_ring(&sq, (5.0, 5.0)), "中心在环内");
        assert!(!point_in_ring(&sq, (11.0, 5.0)), "右侧在环外");
        assert!(!point_in_ring(&sq, (-1.0, -1.0)), "左下在环外");
        // 退化输入不得 panic 也不得判真。
        assert!(!point_in_ring(&[], (0.0, 0.0)), "空环");
        assert!(
            !point_in_ring(&[(0.0, 0.0), (1.0, 1.0)], (0.5, 0.5)),
            "2 点环"
        );

        // L 形凹环 (0,0)(10,0)(10,4)(4,4)(4,10)(0,10)：
        // (8,8) 落在缺口外，(2,2) 在体内。
        let l = vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 4.0),
            (4.0, 4.0),
            (4.0, 10.0),
            (0.0, 10.0),
        ];
        assert!(point_in_ring(&l, (2.0, 2.0)), "L 形体内");
        assert!(!point_in_ring(&l, (8.0, 8.0)), "L 形缺口外");
    }

    /// 框选手柄 = 包围盒 4 角；多边形没有手柄（走逐锚点模型）。
    #[test]
    fn shape_handles_are_bbox_corners_for_shapes_and_none_for_polygon() {
        let sq = rect_ring(Vec2::ZERO, Vec2::new(10.0, 20.0));
        let want = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 20.0), (0.0, 20.0)];
        assert_eq!(shape_handles(SelectionShape::Rect, &sq), want);
        // 椭圆同样用包围盒角——拖的是框，不是椭圆上的极值点。
        let ell = ellipse_ring(Vec2::ZERO, Vec2::new(10.0, 20.0));
        assert_eq!(
            shape_handles(SelectionShape::Ellipse, &ell),
            want,
            "椭圆手柄应与矩形同位置（外接框角）"
        );
        assert!(
            shape_handles(SelectionShape::Polygon, &sq).is_empty(),
            "多边形不画框选手柄"
        );
    }

    /// 拖拽成形的落点：两角 → 闭合环，下游拿到的仍是普通 `closed` 环。
    ///
    /// 这条断言守的是**下游零改动**这条设计约定：意图菜单 / 大陆生成 / 城镇 /
    /// 势力划分读的都是 `closed`，只要这里是 `Some(环)`，它们对矩形 / 椭圆
    /// 与对多边形一视同仁。
    #[test]
    fn rebuild_closed_produces_a_ring_downstream_can_read_unchanged() {
        for shape in [SelectionShape::Rect, SelectionShape::Ellipse] {
            let mut s = SelectionState {
                shape,
                ..Default::default()
            };
            assert!(s.rebuild_closed(Vec2::new(1.0, 2.0), Vec2::new(11.0, 22.0)));
            let ring = s.closed.as_ref().expect("应成形");
            assert!(ring.len() >= 3, "闭合环至少 3 点，实得 {}", ring.len());
            let (min, max) = ring_bounds(ring).expect("非空环");
            assert_eq!(min, Vec2::new(1.0, 2.0), "{shape:?} 包围盒左下");
            assert_eq!(max, Vec2::new(11.0, 22.0), "{shape:?} 包围盒右上");
            // 中心必在环内——这是下游「点在选区内」判定的最低要求。
            assert!(point_in_ring(ring, (6.0, 12.0)), "{shape:?} 中心应在环内");
        }
        // 退化两角（零宽或零高）不产生选区。
        let mut s = SelectionState {
            shape: SelectionShape::Rect,
            ..Default::default()
        };
        assert!(
            !s.rebuild_closed(Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0)),
            "零跨度不该成形"
        );
        assert!(s.closed.is_none());
        // 多边形没有「按两角重算」这回事。
        let mut poly = SelectionState::default();
        assert!(!poly.rebuild_closed(Vec2::ZERO, Vec2::splat(10.0)));
    }

    /// 整体平移：环整体移动，形状与包围盒尺寸不变。
    #[test]
    fn translate_closed_moves_the_ring_without_resizing_it() {
        let mut s = SelectionState {
            shape: SelectionShape::Rect,
            ..Default::default()
        };
        s.rebuild_closed(Vec2::ZERO, Vec2::new(10.0, 20.0));
        let before = ring_bounds(&s.closed.clone().unwrap()).unwrap();
        assert!(s.translate_closed(Vec2::new(5.0, -5.0)));
        let after = ring_bounds(s.closed.as_ref().unwrap()).unwrap();
        assert_eq!(after.1 - after.0, before.1 - before.0, "平移不应改变尺寸");
        assert_eq!(after.0, Vec2::new(5.0, -5.0), "包围盒应整体右移 5、上移 -5");
        // 无选区时返回 false 而不是 panic。
        assert!(!SelectionState::default().translate_closed(Vec2::ONE));
    }

    /// 框选形状整块吃掉点击（不让 `drill_pick` 穿透）；多边形维持原口径。
    #[test]
    fn shaped_selection_claims_clicks_inside_the_whole_ring() {
        let mut s = SelectionState {
            shape: SelectionShape::Rect,
            ..Default::default()
        };
        s.rebuild_closed(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(
            hit_tests_claim_click(&s, (5.0, 5.0), 0.5),
            "形状内部的点击应归选择工具，不该穿透下钻"
        );
        assert!(
            !hit_tests_claim_click(&s, (50.0, 50.0), 0.5),
            "形状外部放行"
        );
        // 多边形内部**不**吃（它有逐锚点模型，点空白 = 取消）。
        let mut poly = SelectionState {
            shape: SelectionShape::Polygon,
            ..Default::default()
        };
        poly.rebuild_closed(Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(!hit_tests_claim_click(&poly, (5.0, 5.0), 0.5));
    }

    /// `ShapeDrag` 的 from/to 读写与拖拽态清理。
    #[test]
    fn shape_drag_tracks_press_point_and_current_point() {
        let mut d = ShapeDrag::Draw {
            from: Vec2::new(1.0, 1.0),
            to: Vec2::new(1.0, 1.0),
        };
        assert_eq!(d.from(), Vec2::new(1.0, 1.0));
        d.set_to(Vec2::new(9.0, 7.0));
        assert_eq!(d.to(), Vec2::new(9.0, 7.0));
        assert_eq!(d.from(), Vec2::new(1.0, 1.0), "from 是按下点，全程不动");

        // 切工具 / 切图时 cancel 必须把拖拽态一并清掉，否则状态会卡在一个
        // 等不到 just_released 的手势里。
        let mut s = SelectionState {
            shape: SelectionShape::Ellipse,
            drag: Some(ShapeDrag::Move {
                from: Vec2::ZERO,
                to: Vec2::ONE,
            }),
            ..Default::default()
        };
        s.cancel();
        assert!(s.drag.is_none(), "cancel 应清掉拖拽态");
    }
}
