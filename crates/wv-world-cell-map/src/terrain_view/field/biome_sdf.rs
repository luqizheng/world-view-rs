//! biome 距离场——群系填色与亚格平滑边界的数据面（ADR-0072 T1）。
//!
//! ## 语义（与海岸无符号距离同族）
//!
//! 每格两个量：
//! - `d`：格心到最近**异类群系**格心的距离（格，cap 饱和）；
//! - `nb`：该最近异类格的群系 ID（argmin 传播；cap 外无解 → [`NO_FOREIGN`]）。
//!
//! shader 侧消费（T4）：`t = smoothstep(0.5, 0.5 + half, d)`——d ≤ 0.5 的
//! 格带内向最近异类色过渡，直段零平移、直角由屏幕封顶的过渡带自然圆角
//! （海岸追记十五同语法）；d ≥ 0.5 + half 完全 own 色。群系色块边界由此
//! 亚格连续，4 邻逐格硬切（T27）退役。
//!
//! ## 为什么是逐类 Dijkstra 而不是 EDT
//!
//! 海岸只有水/陆两类，一趟 Felzenszwalb 就够；群系约 20 类，逐类 EDT 是
//! 20×O(N)。但平滑带只有 ~1.5 格宽：距最近异类超过 cap（缺省 8 格）的
//! 格在视觉上与「无边界」无异，故按类做多源 Dijkstra、距离超过 cap 即
//! 早退——实际代价 ∝ 边界带面积 × 类数，与 N 解耦。后半格内部饱和为
//! cap，编码后即 own 色满档。

use wv_core::terrain::TerrainData;

/// 距离饱和值（格）：超过它的格在 shader 侧完全 own 色。
/// 与 line_sdf 的 `DASH_MAX_CELLS` 同族取值，改动须两处对评。
pub const BIOME_FIELD_CAP: f32 = 8.0;

/// cap 外无最近异类时的 `nb` 哨兵——避开真实群系 ID（0..20 区间）。
pub const NO_FOREIGN: u8 = 255;

/// 定点距离：1/256 格为单位，直边 256、斜边 362（≈√2×256）。
/// 用整数堆保证确定性（f32 不可全序）；cap=8 时最大值 2048，宽裕。
const FIXED_ONE: u32 = 256;
const FIXED_DIAG: u32 = 362;

/// 一个格的 biome 距离场：`d`（格，≤ cap）与 `nb`（异类群系 ID）。
#[derive(Debug, Clone, PartialEq)]
pub struct BiomeField {
    /// 每格到最近异类群系格心的距离（格，cap 饱和；行主序）。
    pub d: Vec<f32>,
    /// 每格的最近异类群系 ID（cap 外 = [`NO_FOREIGN`]；行主序）。
    pub nb: Vec<u8>,
}

/// 全图 biome 距离场（cap 取 [`BIOME_FIELD_CAP`]）。
pub fn biome_distance_field(data: &TerrainData) -> BiomeField {
    with_cap(
        data.width.max(1) as usize,
        data.height.max(1) as usize,
        |x, y| {
            data.cells
                .get(y * data.width.max(1) as usize + x)
                .map(|c| c.biome)
                .unwrap_or(NO_FOREIGN)
        },
        BIOME_FIELD_CAP,
    )
}

/// 纯函数变体：biome 由闭包给出（单测手摆网格用，与 coast_sdf 同风格）。
pub fn with_cap(w: usize, h: usize, biome_of: impl Fn(usize, usize) -> u8, cap: f32) -> BiomeField {
    let n = w * h;
    if n == 0 {
        return BiomeField {
            d: Vec::new(),
            nb: Vec::new(),
        };
    }
    let cap_fixed = (cap * FIXED_ONE as f32) as u32;
    let mut best = vec![u32::MAX; n];
    let mut nb = vec![NO_FOREIGN; n];

    // 按类收集种子（BTreeMap → 类 ID 升序，确定性）。
    let mut classes: std::collections::BTreeMap<u8, Vec<usize>> = Default::default();
    for y in 0..h {
        for x in 0..w {
            classes.entry(biome_of(x, y)).or_default().push(y * w + x);
        }
    }

    // 逐类多源 Dijkstra（惰性删除堆）。expansion 用**本类**距离数组驱动
    // （它是真实的「到该类距离」），全局 best 只记录答案——各类之间的
    // argmin 竞争经 `best` 的比较自然完成。
    let mut class_dist = vec![u32::MAX; n];
    for (&class, seeds) in classes.iter() {
        if seeds.is_empty() {
            continue;
        }
        class_dist.fill(u32::MAX);
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u32, usize)>> =
            Default::default();
        for &s in seeds {
            class_dist[s] = 0;
            heap.push(std::cmp::Reverse((0, s)));
        }
        while let Some(std::cmp::Reverse((dist, idx))) = heap.pop() {
            if dist > class_dist[idx] {
                continue; // 惰性删除：过期堆项
            }
            if dist > cap_fixed {
                break; // 堆有序 → 其余全部超 cap，本类收工
            }
            let x = (idx % w) as i64;
            let y = (idx / w) as i64;
            for (dx, dy, wgt) in NEIGHBORS {
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let m = (ny as usize) * w + (nx as usize);
                if biome_of(nx as usize, ny as usize) == class {
                    continue; // 只对外类格记录/传播答案
                }
                let nd = dist + wgt;
                if nd < class_dist[m] {
                    class_dist[m] = nd;
                    heap.push(std::cmp::Reverse((nd, m)));
                }
                if nd < best[m] || (nd == best[m] && class < nb[m]) {
                    // argmin；平距取小类 ID（确定性）
                    best[m] = nd;
                    nb[m] = class;
                }
            }
        }
    }

    let d = best
        .into_iter()
        .map(|b| {
            if b == u32::MAX {
                cap
            } else {
                (b as f32 / FIXED_ONE as f32).min(cap)
            }
        })
        .collect();
    BiomeField { d, nb }
}

/// 8 邻（dx, dy, 定点权重）。
const NEIGHBORS: [(i64, i64, u32); 8] = [
    (1, 0, FIXED_ONE),
    (-1, 0, FIXED_ONE),
    (0, 1, FIXED_ONE),
    (0, -1, FIXED_ONE),
    (1, 1, FIXED_DIAG),
    (1, -1, FIXED_DIAG),
    (-1, 1, FIXED_DIAG),
    (-1, -1, FIXED_DIAG),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn field_of(w: usize, grid: &[u8], cap: f32) -> BiomeField {
        with_cap(w, grid.len() / w, |x, y| grid[y * w + x], cap)
    }

    #[test]
    fn half_plane_boundary_cell_is_one_cell_from_foreign_class() {
        // 4×4，左两列类 1 / 右两列类 2：col1 的最近异类在 col2（距 1）。
        let grid = [1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2];
        let f = field_of(4, &grid, 8.0);
        assert!((f.d[1] - 1.0).abs() < 1e-3, "got {}", f.d[1]);
        assert_eq!(f.nb[1], 2);
        assert!((f.d[2] - 1.0).abs() < 1e-3, "got {}", f.d[2]);
        assert_eq!(f.nb[2], 1);
        // 外侧一格距 2。
        assert!((f.d[0] - 2.0).abs() < 1e-3, "got {}", f.d[0]);
    }

    #[test]
    fn diagonal_foreign_neighbor_costs_sqrt_two() {
        // 2×2，仅 (0,0) 类 1：(1,1) 的最近异类是对角 → √2。
        let grid = [1, 2, 2, 2];
        let f = field_of(2, &grid, 8.0);
        assert!(
            (f.d[3] - std::f32::consts::SQRT_2).abs() < 0.01,
            "got {}",
            f.d[3]
        );
    }

    #[test]
    fn interior_beyond_cap_saturates_with_no_foreign() {
        // 20×20 全类 1，仅 (0,0) 类 2：(19,19) 八方向距 ≈ 26.9 > cap=8。
        let mut grid = vec![1u8; 400];
        grid[0] = 2;
        let f = field_of(20, &grid, 8.0);
        assert!((f.d[399] - 8.0).abs() < 1e-3, "got {}", f.d[399]);
        assert_eq!(f.nb[399], NO_FOREIGN);
    }

    #[test]
    fn uniform_grid_never_sees_a_foreign_class() {
        let grid = vec![7u8; 36];
        let f = field_of(6, &grid, 8.0);
        assert!(f.d.iter().all(|&v| (v - 8.0).abs() < 1e-3));
        assert!(f.nb.iter().all(|&v| v == NO_FOREIGN));
    }

    #[test]
    fn three_way_corner_argmin_prefers_the_nearest_class() {
        // 3×3：col0 类 1 / col1 类 2 / col2 类 3——col1 的最近异类是左右
        // 邻（各距 1），平距取小类 ID → nb = 1。
        let grid = [1, 2, 3, 1, 2, 3, 1, 2, 3];
        let f = field_of(3, &grid, 8.0);
        assert!((f.d[4] - 1.0).abs() < 1e-3, "got {}", f.d[4]);
        assert_eq!(f.nb[4], 1);
    }

    #[test]
    fn real_terrain_field_has_same_shape_as_grid() {
        // 纯函数与 TerrainData 入口同构：4×2 全类 1 → 全饱和。
        let data = TerrainData {
            width: 4,
            height: 2,
            cells: (0..8)
                .map(|_| wv_core::terrain::TerrainCellData {
                    height: 0,
                    surface: 1,
                    biome: 3,
                })
                .collect(),
        };
        let f = biome_distance_field(&data);
        assert_eq!(f.d.len(), 8);
        assert_eq!(f.nb.len(), 8);
        assert!(f.d.iter().all(|&v| (v - 8.0).abs() < 1e-3));
    }
}
