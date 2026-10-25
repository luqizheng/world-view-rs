//! 掩码 → 符号距离场（群系 / 地类矢量面的输入场）。
//!
//! ## 为什么要有这个模块
//!
//! 专业制图（`ViewMode::GoogleMap`）的绿地必须是**平滑多边形**，而群系是
//! 栅格——直接按格平涂就是格子边界。既有 `kind_segments` 只把格缘阶梯连成
//! 线（Chaikin 磨角后仍带梯田感），拿不到可三角化的面。本模块把「某类格」
//! 变成连续标量场，交给 [`wv_terrain::isoline::height_segments`] 提取等值线
//! ——那条管线本来对**任意标量场**通用，只是此前只喂了高度场。
//!
//! ## 为什么是「距离 − 半格」
//!
//! [`distance_field`] 的种子是**格心**（异类格距离 0）。若直接按 `±d` 取号：
//!
//! - 相邻两格（内侧 d=1 / 外侧 d=1）零交点落在格心连线中点——恰好是格边界；
//! - 但内侧深处（d=5）与相邻外侧（d=1）插值时，`+5 / -1` 的零交点会滑向
//!   外侧格心一侧，边界整体偏移。
//!
//! 减去半格后，内侧浅格 `+0.5`、外侧 `-0.5`，零交点恒在格边界；深处的
//! `+4.5` 不参与该段插值（等值线只穿最近的一对）。这与 `coast_sdf` 的
//! 「海岸 = 距离场 0 等值」不同——那是 T3 的另一种编码，不要混用。
//!
//! ## 边界（不做什么）
//!
//! - **零第三方依赖**（与 crate 同规）：复用同目录的 Felzenszwalb EDT；
//! - **不感知 Bevy / 渲染**：输出逐格 `f32`，等值线提取与三角化由下游做；
//! - **不缓存**：缓存键与失效策略属渲染层（`wv-render::biome_fill`）。

use super::coast_sdf::distance_field;

/// 掩码的符号距离场（格坐标，逐格 `f32`）。
///
/// - 掩码内：`+(d − 0.5)`，`d` = 到最近**掩码外**格心的欧氏距离；
/// - 掩码外：`−(d − 0.5)`，`d` = 到最近**掩码内**格心的欧氏距离；
/// - 全图同质（无跨界）：`distance_field` 返回饱和值，场符号恒定，**零交点
///   不存在**——下游得到空几何，这是合法稳态。
///
/// `is_in` 是纯谓词（`(x, y) → 是否属于该组`），调用方按视觉组传闭包。
pub fn signed_mask_distance(w: usize, h: usize, is_in: impl Fn(usize, usize) -> bool) -> Vec<f32> {
    let mut d = distance_field(w, h, &is_in);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            // 减半格：把「到最近异类**格心**的距离」换成「到最近**格边界**的距离」。
            // 钳到 0 只为防御（distance_field 的种子格距离恒为 0，正常不会负）。
            let v = (d[i] - 0.5).max(0.0);
            d[i] = if is_in(x, y) { v } else { -v };
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::isoline::{assemble_chains, height_segments, Pt};

    fn pts_of(chain: &wv_terrain::isoline::Chain) -> Vec<[f32; 2]> {
        chain.pts.iter().map(|p| [p.x, p.y]).collect()
    }

    /// 单格岛（3×3 中央）：等值线必须是内切菱形——与高度场版
    /// `height_segments` 的招牌案例同形，证明「距离 − 半格」把零交点
    /// 精确放回格边界。
    #[test]
    fn single_cell_mask_yields_inscribed_diamond() {
        let field = signed_mask_distance(3, 3, |x, y| (x, y) == (1, 1));
        let chains = assemble_chains(&height_segments(
            3,
            3,
            |x, y| field[y as usize * 3 + x as usize],
            0.0,
        ));
        assert_eq!(chains.len(), 1, "单格岛应只有一条闭环，got {chains:?}");
        assert!(chains[0].closed, "掩码岛应闭合");
        let mut got = pts_of(&chains[0]);
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let want = [[1.0, 1.5], [1.5, 1.0], [1.5, 2.0], [2.0, 1.5]];
        assert_eq!(got.len(), 4, "菱形四顶点，got {got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g[0] - w[0]).abs() < 1e-6 && (g[1] - w[1]).abs() < 1e-6,
                "顶点 {g:?} ≠ 手算 {w:?}"
            );
        }
    }

    /// 2×2 方块（4×4 网格中央）：等值线包住 `[1,3]²`，四条直边落在
    /// x=1 / x=3 / y=1 / y=3，四个角被 45° 切掉——这是**格心采样 marching
    /// squares 的固有形状**（对角 dual 格只有一对交点，连成斜段），不是缺陷：
    /// 后续 Chaikin 正是要把这圈八边形磨成圆弧角。
    ///
    /// 本测试同时钉住「深处不偏移」：2×2 块内部的格距离是 1（四个方向都是
    /// 异类邻格），若忘了减半格，边界会整体外扩半格，直边就落到 0.5 / 3.5。
    #[test]
    fn two_by_two_block_yields_chamfered_square() {
        let w = 4usize;
        let field = signed_mask_distance(4, 4, |x, y| (1..3).contains(&x) && (1..3).contains(&y));
        let chains = assemble_chains(&height_segments(
            4,
            4,
            |x, y| field[y as usize * w + x as usize],
            0.0,
        ));
        assert_eq!(chains.len(), 1, "单连通方块一条闭环，got {chains:?}");
        let mut got = pts_of(&chains[0]);
        got.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let want = [
            [1.0, 1.5],
            [1.0, 2.5],
            [1.5, 1.0],
            [1.5, 3.0],
            [2.5, 1.0],
            [2.5, 3.0],
            [3.0, 1.5],
            [3.0, 2.5],
        ];
        assert_eq!(got.len(), want.len(), "八边形顶点，got {got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g[0] - w[0]).abs() < 1e-6 && (g[1] - w[1]).abs() < 1e-6,
                "顶点 {g:?} ≠ 手算 {w:?}"
            );
        }
        // 包围盒 = 方块本体，不多不少（防「整体偏移半格」）。
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &got {
            lo[0] = lo[0].min(p[0]);
            lo[1] = lo[1].min(p[1]);
            hi[0] = hi[0].max(p[0]);
            hi[1] = hi[1].max(p[1]);
        }
        assert_eq!(lo, [1.0, 1.0], "包围盒左下，got {lo:?}");
        assert_eq!(hi, [3.0, 3.0], "包围盒右上，got {hi:?}");
    }

    /// 全图同质：场符号恒定、无零交点 → 空几何（合法稳态，不是缺陷）。
    #[test]
    fn uniform_mask_has_no_contour() {
        let field = signed_mask_distance(8, 8, |_, _| true);
        let chains = assemble_chains(&height_segments(
            8,
            8,
            |x, y| field[y as usize * 8 + x as usize],
            0.0,
        ));
        assert!(chains.is_empty(), "全内无边界，got {chains:?}");

        let none = signed_mask_distance(8, 8, |_, _| false);
        let chains = assemble_chains(&height_segments(
            8,
            8,
            |x, y| none[y as usize * 8 + x as usize],
            0.0,
        ));
        assert!(chains.is_empty(), "全外无边界，got {chains:?}");
    }

    /// 符号方向：掩码内为正、掩码外为负（等值线 level=0 的方向语义）。
    #[test]
    fn inside_is_positive_outside_is_negative() {
        let f = signed_mask_distance(3, 3, |x, y| (x, y) == (1, 1));
        assert!(f[4] > 0.0, "中心格应为正，got {}", f[4]);
        assert!(f[0] < 0.0, "角格应为负，got {}", f[0]);
        // 中心格到最近外格心距离 1 → 0.5。
        assert!(
            (f[4] - 0.5).abs() < 1e-6,
            "中心格符号距离应为 0.5，got {}",
            f[4]
        );
    }

    /// 两条水平异类边（3×2：上排在内）：等值线是 y=1 的一条直线。
    #[test]
    fn flat_boundary_yields_single_straight_segment() {
        let field = signed_mask_distance(3, 2, |_, y| y == 0);
        let chains = assemble_chains(&height_segments(
            3,
            2,
            |x, y| field[y as usize * 3 + x as usize],
            0.0,
        ));
        assert_eq!(
            chains.len(),
            1,
            "一整条边界应装配成一条开链，got {chains:?}"
        );
        let chain = &chains[0];
        assert!(!chain.closed, "触到网格边界 → 开链");
        for Pt { y, .. } in &chain.pts {
            assert!((*y - 1.0).abs() < 1e-6, "边界应恰在 y=1，got {y}");
        }
    }
}
