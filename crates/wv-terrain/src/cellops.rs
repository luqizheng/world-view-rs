//! 单格选择与派生运算（[ADR-0052] D6 · §5 M2）。
//!
//! ## 定位
//!
//! CellMap 工作台的三个**纯函数**——不碰 UI、不碰撤销栈、不写世界：
//!
//! - [`flood_region`] —— **泛洪选择**：从起点扩散出「同值连通区」，
//!   返回格集合（**不写世界**——写不写是上层的决定）。
//! - [`smooth_cells`] —— **平滑**：对给定格集合做一次邻域均值。
//! - [`quantize_heights`] —— **量化**：把海拔吸附到步长网格。
//!
//! ## 为什么是纯函数
//!
//! 三者都要能被**测试锚定**（§9.8 测试门槛：`flood_region` 4/8 连通与
//! 越界 5 条、`smooth_cells` 保边界与不漂移 3 条），且必须保证「世界写入
//! 只经 `EditCommand → History`」（[ADR-0021] §3.3）。纯函数天然满足二者。

use crate::{Height, TerrainCell, TerrainGrid};

/// **泛洪判据**：什么算「同值」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloodCriterion {
    /// 按**地表**判同（水 / 陆）。
    Surface,
    /// 按**群系**判同。
    Biome,
    /// 按**海拔容差**判同（`|h − h₀| ≤ tol`）。
    HeightTolerance(i16),
}

/// 泛洪判据的取值函数（纯）。
fn flood_match(criterion: FloodCriterion, cell: TerrainCell, seed: TerrainCell) -> bool {
    match criterion {
        FloodCriterion::Surface => cell.surface == seed.surface,
        FloodCriterion::Biome => cell.biome == seed.biome,
        FloodCriterion::HeightTolerance(tol) => {
            (cell.height as i32 - seed.height as i32).abs() <= tol as i32
        }
    }
}

/// **泛洪选择**：从 `(sx, sy)` 出发，扩散出所有「与种子同值且四向连通」的格。
///
/// **四向连通**（上下左右），不是八向——八向会在对角相邻处渗漏，
/// 让「一片森林」沿着对角线爬上另一片不相邻的草地。
///
/// **不写世界**：只返回格集合。写不写、写成什么，由上层决定
/// （[ADR-0052] D6-A + [ADR-0057]：泛洪结果入统一选区 `CellSelection`，
/// 无需预览确认）。
///
/// **越界安全**：起点不在网格内 → 返回空集（不 panic）。
pub fn flood_region(
    grid: &TerrainGrid,
    sx: i32,
    sy: i32,
    criterion: FloodCriterion,
) -> Vec<(i32, i32)> {
    let Some(seed) = grid.get(sx, sy) else {
        return Vec::new();
    };
    let w = grid.width() as i32;
    let h = grid.height() as i32;
    if sx < 0 || sy < 0 || sx >= w || sy >= h {
        return Vec::new();
    }
    let mut visited = vec![false; (w as usize) * (h as usize)];
    let mut stack = vec![(sx, sy)];
    let mut out = Vec::new();
    while let Some((x, y)) = stack.pop() {
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        let idx = (y as usize) * (w as usize) + (x as usize);
        if visited[idx] {
            continue;
        }
        visited[idx] = true;
        let Some(cell) = grid.get(x, y) else {
            continue;
        };
        if !flood_match(criterion, cell, seed) {
            continue;
        }
        out.push((x, y));
        // 四向扩展。
        stack.push((x + 1, y));
        stack.push((x - 1, y));
        stack.push((x, y + 1));
        stack.push((x, y - 1));
    }
    // 稳定序（行优先）——测试可断言，且上层渲染的格序不影响视觉。
    out.sort_unstable();
    out
}

/// **平滑**：对给定格集合做一次**邻域均值**。
///
/// **只改集合内的格**（集合外的邻格参与均值计算但不被改写）——
/// 这是「平滑选区」的语义：作者圈了要平滑的那片，边界外的地形不动。
///
/// **强度** `strength ∈ [0, 1]`：新值 = `old + (avg − old) × strength`。
/// `strength = 0` → 不变；`1` → 完全变成邻域均值。
///
/// **保边界**（§9.8 测试门槛）：集合外的格**一律不改**；集合内的格
/// 也不会被平滑成邻域均值之外的值。
///
/// **不漂移**（§9.8）：平滑是**均值**（不是中值）——对平坦区域，
/// 均值 = 原值，**不进位漂移**（§5 M2 验收 ④：均值在中值 ±1 内）。
///
/// 返回 `(变更前, 变更后)` 的逐格对照（仅含**真变化**的格）。
pub fn smooth_cells(
    grid: &TerrainGrid,
    cells: &[(i32, i32)],
    strength: f32,
) -> Vec<((i32, i32), TerrainCell, TerrainCell)> {
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.0 || cells.is_empty() {
        return Vec::new();
    }
    let w = grid.width() as i32;
    let h = grid.height() as i32;
    let mut out = Vec::new();
    for &(x, y) in cells {
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        let Some(before) = grid.get(x, y) else {
            continue;
        };
        // 邻域（含自身）均值——四邻 + 中心，共最多 5 个样本。
        let mut sum = 0i32;
        let mut n = 0i32;
        for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
            if let Some(c) = grid.get(x + dx, y + dy) {
                sum += c.height as i32;
                n += 1;
            }
        }
        if n == 0 {
            continue;
        }
        let avg = sum as f32 / n as f32;
        let new_h = before.height as f32 + (avg - before.height as f32) * strength;
        // 四舍五入到整数米（Height 是 i16）。
        let new_h = new_h.round().clamp(i16::MIN as f32, i16::MAX as f32) as Height;
        let after = TerrainCell {
            height: new_h,
            surface: before.surface,
            biome: before.biome,
        };
        if after != before {
            out.push(((x, y), before, after));
        }
    }
    out
}

/// **量化**：把给定格集合的海拔吸附到步长网格。
///
/// `step` 为量化步长（米）：新海拔 = `round(h / step) × step`。
///
/// `step ≤ 0` → 原样返回（不量化）。
///
/// **只改集合内的格**（集合外不动）。
///
/// 返回 `(变更前, 变更后)` 的逐格对照（仅含**真变化**的格）。
pub fn quantize_heights(
    grid: &TerrainGrid,
    cells: &[(i32, i32)],
    step: i32,
) -> Vec<((i32, i32), TerrainCell, TerrainCell)> {
    if step <= 0 || cells.is_empty() {
        return Vec::new();
    }
    let w = grid.width() as i32;
    let h = grid.height() as i32;
    let mut out = Vec::new();
    for &(x, y) in cells {
        if x < 0 || y < 0 || x >= w || y >= h {
            continue;
        }
        let Some(before) = grid.get(x, y) else {
            continue;
        };
        let q = ((before.height as f32 / step as f32).round() * step as f32)
            .clamp(i16::MIN as f32, i16::MAX as f32) as Height;
        let after = TerrainCell {
            height: q,
            surface: before.surface,
            biome: before.biome,
        };
        if after != before {
            out.push(((x, y), before, after));
        }
    }
    out
}

/// 构建一个测试用网格（指定尺寸 + 全部同值）。
#[cfg(test)]
fn flat_grid(w: u32, h: u32, cell: TerrainCell) -> TerrainGrid {
    let mut g = TerrainGrid::new(w, h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            g.set(x, y, cell);
        }
    }
    g
}

/// 构建一个测试用网格（每格按 `(x + y) % 2` 交替两种值）。
#[cfg(test)]
fn checker_grid(w: u32, h: u32, a: TerrainCell, b: TerrainCell) -> TerrainGrid {
    let mut g = TerrainGrid::new(w, h);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            g.set(x, y, if (x + y) % 2 == 0 { a } else { b });
        }
    }
    g
}

#[cfg(test)]
mod flood_tests {
    use super::*;
    use crate::{BIOME_FOREST, BIOME_PLAINS, SURFACE_LAND, SURFACE_WATER};

    fn land_forest(h: Height) -> TerrainCell {
        TerrainCell {
            height: h,
            surface: SURFACE_LAND,
            biome: BIOME_FOREST,
        }
    }

    /// **四向连通**：8×8 全森林 → 泛洪出 64 格。
    #[test]
    fn flood_region_finds_whole_uniform_area() {
        let g = flat_grid(8, 8, land_forest(100));
        let cells = flood_region(&g, 0, 0, FloodCriterion::Biome);
        assert_eq!(cells.len(), 64, "全同值区应全部命中");
    }

    /// **判据区分**：按群系泛洪只取森林，不取平原。
    #[test]
    fn flood_region_respects_criterion() {
        let g = checker_grid(
            8,
            8,
            land_forest(100),
            TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome: BIOME_PLAINS,
            },
        );
        let cells = flood_region(&g, 0, 0, FloodCriterion::Biome);
        assert!(
            cells.iter().all(|(x, y)| (x + y) % 2 == 0),
            "只应命中森林格"
        );
        assert!(!cells.is_empty());
    }

    /// **地表判据**：水陆分开。
    #[test]
    fn flood_region_by_surface_separates_water_and_land() {
        let mut g = flat_grid(
            4,
            4,
            TerrainCell {
                height: 0,
                surface: SURFACE_WATER,
                biome: BIOME_FOREST,
            },
        );
        // 左半陆地。
        for y in 0..4 {
            for x in 0..2 {
                g.set(x, y, land_forest(50));
            }
        }
        let cells = flood_region(&g, 0, 0, FloodCriterion::Surface);
        assert_eq!(cells.len(), 8, "左半 2×4 = 8 格陆地");
    }

    /// **海拔容差**：容差内的格算同值。
    #[test]
    fn flood_region_by_height_tolerance() {
        let mut g = flat_grid(5, 1, land_forest(100));
        g.set(2, 0, land_forest(103));
        g.set(3, 0, land_forest(200));
        // 容差 5 → 100/103 同值（格 (0,0)=100 (1,0)=100 (2,0)=103），200 不同。
        let cells = flood_region(&g, 0, 0, FloodCriterion::HeightTolerance(5));
        assert_eq!(cells.len(), 3, "100/100/103 应命中 3 格");
        // 容差 0 → 只与种子**完全同高**的格（(0,0) 与 (1,0) 都是 100）。
        let cells = flood_region(&g, 0, 0, FloodCriterion::HeightTolerance(0));
        assert_eq!(cells.len(), 2, "容差 0 → 只有等高格");
    }

    /// **越界起点** → 空集，不 panic。
    #[test]
    fn flood_region_handles_out_of_bounds_seed() {
        let g = flat_grid(4, 4, land_forest(10));
        assert!(flood_region(&g, -1, 0, FloodCriterion::Biome).is_empty());
        assert!(flood_region(&g, 0, -1, FloodCriterion::Biome).is_empty());
        assert!(flood_region(&g, 99, 0, FloodCriterion::Biome).is_empty());
    }

    /// **四向而非八向**：对角相邻但四向不连通的格不应被吸入。
    #[test]
    fn flood_region_is_four_connected_not_eight() {
        let mut g = flat_grid(3, 3, land_forest(10));
        // 只把 (2,2) 改成平原——它与 (1,1) 对角相邻，但四向不连通。
        g.set(
            2,
            2,
            TerrainCell {
                height: 10,
                surface: SURFACE_LAND,
                biome: BIOME_PLAINS,
            },
        );
        let cells = flood_region(&g, 0, 0, FloodCriterion::Biome);
        assert!(!cells.contains(&(2, 2)), "对角格不应被八向泛洪吸入");
        assert_eq!(cells.len(), 8);
    }
}

#[cfg(test)]
mod smooth_tests {
    use super::*;
    use crate::{BIOME_FOREST, SURFACE_LAND};

    /// **不漂移**：平坦区域平滑后**不变**（均值 = 原值）。
    #[test]
    fn smooth_cells_does_not_drift_on_flat_terrain() {
        let g = flat_grid(
            8,
            8,
            TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        let cells: Vec<(i32, i32)> = (0..8).flat_map(|x| (0..8).map(move |y| (x, y))).collect();
        let changes = smooth_cells(&g, &cells, 1.0);
        assert!(changes.is_empty(), "平坦地形平滑后不应有变化（不进位漂移）");
    }

    /// **保边界**：集合外的格**一律不改**。
    #[test]
    fn smooth_cells_keeps_cells_outside_selection_untouched() {
        let mut g = flat_grid(
            8,
            8,
            TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        // 在 (0,0) 造一个尖峰。
        g.set(
            0,
            0,
            TerrainCell {
                height: 500,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        // 只平滑 (1,0) 一格。
        let changes = smooth_cells(&g, &[(1, 0)], 1.0);
        // (1,0) 的邻域含 (0,0) 的 500 → 均值抬高 → (1,0) 变化。
        assert_eq!(changes.len(), 1, "只应改选区内的那一格");
        assert_eq!(changes[0].0, (1, 0));
        // 尖峰本身（集合外）**没被动过**。
        assert_eq!(g.get(0, 0).unwrap().height, 500, "集合外不得改");
    }

    /// **强度 0 → 不变**。
    #[test]
    fn smooth_cells_with_zero_strength_changes_nothing() {
        let mut g = flat_grid(
            4,
            4,
            TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        g.set(
            0,
            0,
            TerrainCell {
                height: 500,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        let cells: Vec<(i32, i32)> = (0..4).flat_map(|x| (0..4).map(move |y| (x, y))).collect();
        assert!(smooth_cells(&g, &cells, 0.0).is_empty());
    }

    /// **越界格跳过**，不 panic。
    #[test]
    fn smooth_cells_skips_out_of_bounds() {
        let g = flat_grid(
            4,
            4,
            TerrainCell {
                height: 100,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        let changes = smooth_cells(&g, &[(-1, 0), (0, 99)], 1.0);
        assert!(changes.is_empty());
    }
}

#[cfg(test)]
mod quantize_tests {
    use super::*;
    use crate::{BIOME_FOREST, SURFACE_LAND};

    /// **量化**：海拔吸附到步长网格。
    #[test]
    fn quantize_heights_snaps_to_step_grid() {
        let mut g = flat_grid(
            4,
            4,
            TerrainCell {
                height: 137,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        g.set(
            1,
            1,
            TerrainCell {
                height: 250,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        let changes = quantize_heights(&g, &[(0, 0), (1, 1)], 100);
        // 137 → 100；250 → 300（.5 向上取整到 300？——round(2.5) = 3 → 300）
        let by_pos: std::collections::HashMap<(i32, i32), Height> =
            changes.iter().map(|(p, _, a)| (*p, a.height)).collect();
        assert_eq!(by_pos.get(&(0, 0)), Some(&100));
        assert_eq!(by_pos.get(&(1, 1)), Some(&300));
    }

    /// **步长 ≤ 0** → 原样返回。
    #[test]
    fn quantize_heights_with_non_positive_step_changes_nothing() {
        let g = flat_grid(
            4,
            4,
            TerrainCell {
                height: 137,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        assert!(quantize_heights(&g, &[(0, 0)], 0).is_empty());
        assert!(quantize_heights(&g, &[(0, 0)], -10).is_empty());
    }

    /// **已在网格上的值不变**（不产生空变更）。
    #[test]
    fn quantize_heights_skips_already_quantized_cells() {
        let g = flat_grid(
            4,
            4,
            TerrainCell {
                height: 200,
                surface: SURFACE_LAND,
                biome: BIOME_FOREST,
            },
        );
        assert!(quantize_heights(&g, &[(0, 0)], 100).is_empty());
    }
}
