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

use crate::state::SelectionState;

/// 锚点 / 边命中容差（屏幕像素）——调用方按 `view.scale` 反比换算成 wu。
pub const PICK_TOLERANCE_PX: f32 = 8.0;
/// 首锚闭合吸附半径（屏幕像素）。
pub const CLOSE_SNAP_PX: f32 = 10.0;
/// 双击判定窗口（秒）与位置容差（wu，≈1.5 格）——与 `continent_sketch` 同口径。
pub const DOUBLE_CLICK_SECS: f64 = 0.4;
pub const DOUBLE_CLICK_WU: f32 = 12.0;

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
            pick_anchor(ring, local, tol).is_some() || pick_edge_insertion(ring, local, tol).is_some()
        }
        None => false,
    }
}

/// 左键单击的核心语义（系统只做守卫与坐标转换）。
///
/// - Closed 态：锚点命中 → 单击选中（记 `last_click` 供双击判定）、双击删除；
///   边命中 → 插入并选中新锚点；空白 → 取消整个选区。
/// - Drawing 态：≥3 点且点首锚吸附内 → 闭合；双击 → 收笔；否则落点。
pub fn handle_click(sel: &mut SelectionState, p: (f32, f32), now: f64, tol: f32, snap: f32) {
    let is_double = match sel.last_click {
        Some(((lx, ly), t)) => {
            (p.0 - lx).hypot(p.1 - ly) <= DOUBLE_CLICK_WU
                && now - t <= DOUBLE_CLICK_SECS
        }
        None => false,
    };

    if let Some(ring) = sel.closed.clone() {
        // ====== Closed 态 ======
        if let Some(i) = pick_anchor(&ring, p, tol) {
            if is_double {
                sel.delete_anchor(i);
                sel.selected_anchor = None;
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
        handle_click(&mut s, (0.0, 0.0), 10.0, 0.5, 1.0);
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
        handle_click(&mut s, (10.0, 10.0), 10.0, 0.5, 1.0);
        handle_click(&mut s, (10.0, 10.0), 10.2, 0.5, 1.0);
        assert!(s.is_closed(), "双击应收笔闭合");
    }

    /// 单击锚点 = 选中；紧接着的双击 = 删除该锚点（4 环删 1 剩 3，保持闭合）。
    #[test]
    fn clicking_anchor_selects_and_double_click_deletes_it() {
        let mut s = closed_square();
        handle_click(&mut s, (10.0, 0.0), 10.0, 0.5, 1.0);
        assert_eq!(s.selected_anchor, Some(1), "单击应选中 1 号锚");
        handle_click(&mut s, (10.0, 0.0), 10.2, 0.5, 1.0);
        assert_eq!(
            s.closed.as_ref().map(Vec::len),
            Some(3),
            "双击删除后剩 3 点仍闭合"
        );
        assert_eq!(s.selected_anchor, None, "删除后选中复位");
    }

    /// 点击边线 = 该处插入锚点并选中（随后可拖）。
    #[test]
    fn clicking_edge_inserts_and_selects_anchor() {
        let mut s = closed_square();
        handle_click(&mut s, (5.0, 10.0), 10.0, 0.5, 1.0);
        assert_eq!(s.closed.as_ref().map(Vec::len), Some(5), "插入后 5 点");
        assert_eq!(s.selected_anchor, Some(3), "新锚点应被选中");
        assert_eq!(s.last_click, None, "插入不走双击判定");
    }

    /// Closed 态点空白 = 取消整个选区（spec D9）。
    #[test]
    fn clicking_empty_cancels_closed_selection() {
        let mut s = closed_square();
        handle_click(&mut s, (50.0, 50.0), 10.0, 0.5, 1.0);
        assert!(!s.is_closed(), "点空白应取消选区");
    }

    /// drill 守卫（spec §3 命中优先级）：画中全吃；Closed 态锚点/边命中吃，
    /// 远处放行给下钻/标签。
    #[test]
    fn hit_tests_claim_click_in_drawing_and_on_anchors() {
        let mut drawing = SelectionState::default();
        drawing.add_point((0.0, 0.0));
        assert!(hit_tests_claim_click(&drawing, (100.0, 100.0), 0.5), "画中全吃");

        assert!(hit_tests_claim_click(&closed_square(), (10.1, 0.0), 0.5), "锚点命中");
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
