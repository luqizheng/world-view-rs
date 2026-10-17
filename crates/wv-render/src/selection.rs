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
//! 共用同一条早退链（UI 指针 / 播放 / 缩放下限 / 无工程锁 / `map_origin` 同源链）。
//!
//! ## 边界
//! - 不生成地形、不碰 `History`、不落盘——生成类意图由 wv-app 经
//!   `SelectionState::consume_closed` 取走后自行提交（AGENTS §2.1 红线）。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{
    DrillState, GridConfig, PlaybackState, SelectionState, ViewState, WorldProjection,
};

/// 锚点 / 边命中容差（屏幕像素）——调用方按 `view.scale` 反比换算成 wu。
pub const PICK_TOLERANCE_PX: f32 = 8.0;
/// 首锚闭合吸附半径（屏幕像素）。
pub const CLOSE_SNAP_PX: f32 = 10.0;
/// 双击判定窗口（秒）；位移容差见 [`DOUBLE_CLICK_PX`]（按屏幕像素计）。
pub const DOUBLE_CLICK_SECS: f64 = 0.4;
/// 双击位移容差（屏幕像素）——调用方按 `view.scale` 换算成 wu 再传入
/// [`handle_click`]。原 12 wu 世界定长在深缩放下小于像素抖动（M1 终审 minor，
/// M2 清偿）。
pub const DOUBLE_CLICK_PX: f32 = 12.0;

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
pub fn hit_tests_claim_click(sel: &SelectionState, local: (f32, f32), tol: f32) -> bool {
    if sel.is_drawing() {
        return true;
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
/// `commit_continent_sketch_click`（UI 指针 / 播放 / 缩放下限 / 无工程锁）。
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
        }
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
    sel.move_anchor(i, (local.x, local.y));
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
        for (i, sp) in pts.iter().enumerate() {
            let r = if sel.selected_anchor == Some(i) {
                vertex_r * 1.8
            } else {
                vertex_r
            };
            gizmos.circle_2d(Isometry2d::from_translation(*sp), r, closed);
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
}
