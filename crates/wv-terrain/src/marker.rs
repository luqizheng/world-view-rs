//! 生物群系标记内核（ADR-0029）：包络 → 识别或生成群系 → 边界环。
//!
//! ## 识别或生成（ADR-0029 D3）
//!
//! - **识别**：包络内已有足量目标群系格（≥8% 且 ≥4 格）→ 不写格子，
//!   边界取这些格子的连通外环（最大主块 + 其洞）；
//! - **生成**：否则把包络内格子写成目标群系，边界取写入后掩膜的环。
//!
//! ## 湖泊（D14 语义派）
//!
//! 目标群系 `LAKE`：包络内**陆格** → `surface=WATER`、`biome=LAKE`、
//! `height=包络内中位海拔`（湖面平，不挖碗）。既有水格保留原高度、补上 LAKE 群系。
//!
//! ## 坐标口径
//!
//! 入参包络是**图内局部 wu**（与选择工具/Place 一致）；本模块按 `cell_size`
//! 换成格坐标后全部用格坐标（与外层调用方零约定歧义——C1 的教训）。

use crate::biome::surface_for_biome;
use crate::divide::{mask_boundary_rings, rasterize_ring, split_outers_and_holes};
use crate::{TerrainGrid, SURFACE_LAND, SURFACE_WATER};

/// 识别分支最少命中格数。
pub const IDENTIFY_MIN_CELLS: usize = 4;
/// 识别分支最少命中占比（相对包络格数）。
pub const IDENTIFY_MIN_RATIO: f32 = 0.08;

/// 识别或生成——标记的两面（ADR-0029 D3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerOutcome {
    /// 包络内已有足量同类群系：只圈出，不写格子。
    Identified,
    /// 包络内不足：写入目标群系后再圈出。
    Generated,
}

/// 标记区域（格坐标）。
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerRegion {
    /// 最大外环（v1 多块时只取主块）。
    pub outer: Vec<(f32, f32)>,
    /// 主块的洞（如环湖国中间的湖）。
    pub holes: Vec<Vec<(f32, f32)>>,
    /// 识别还是生成。
    pub outcome: MarkerOutcome,
    /// 实际改动的格数（识别分支恒 0）。
    pub cells_changed: usize,
}

/// 标记失败原因（V1：调用方给提示，不 panic）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerError {
    /// 包络不足 3 点 / 非正格尺寸。
    TooFewPoints,
    /// 包络内没有任何可用格（全是水且目标为陆群系 / 包络过小）。
    EmptyRegion,
}

/// 在包络内标记群系（就地把 `grid` 改成生成后的状态）。
///
/// `envelope` 为图内局部 wu（首尾不重复）；`biome` 为冻结表编号。
pub fn mark_region(
    grid: &mut TerrainGrid,
    envelope: &[(f32, f32)],
    biome: u8,
    cell_size: f32,
) -> Result<MarkerRegion, MarkerError> {
    let (w, h) = (grid.width(), grid.height());
    if envelope.len() < 3 || cell_size <= 0.0 || !cell_size.is_finite() {
        return Err(MarkerError::TooFewPoints);
    }
    let envelope_cells: Vec<(f32, f32)> = envelope
        .iter()
        .map(|&(x, y)| (x / cell_size, y / cell_size))
        .collect();
    let mask = rasterize_ring(w, h, &envelope_cells);
    let total = mask.iter().filter(|&&b| b).count();
    if total < 3 {
        return Err(MarkerError::EmptyRegion);
    }
    let target_surface = surface_for_biome(biome);

    // ---- 识别：包络内已有目标群系 ----
    let mut matched = vec![false; mask.len()];
    let mut matched_count = 0usize;
    for (i, &inside) in mask.iter().enumerate() {
        if !inside {
            continue;
        }
        let (x, y) = ((i % w as usize) as i32, (i / w as usize) as i32);
        if let Some(cell) = grid.get(x, y) {
            // 与生成分支同一湿判定口径（surface 水 ‖ 海拔 < 0），避免
            // 「识别把沉海陆格算命中、生成却跳过」的定义漂移（终审 M10）。
            let wet = crate::edit::is_water_cell(cell);
            let same_domain = if target_surface == SURFACE_WATER {
                wet
            } else {
                !wet
            };
            if cell.biome == biome && same_domain {
                matched[i] = true;
                matched_count += 1;
            }
        }
    }
    let threshold = ((total as f32 * IDENTIFY_MIN_RATIO).ceil() as usize).max(IDENTIFY_MIN_CELLS);
    if matched_count >= threshold {
        let (outer, holes) = rings_of(&matched, w, h)?;
        return Ok(MarkerRegion {
            outer,
            holes,
            outcome: MarkerOutcome::Identified,
            cells_changed: 0,
        });
    }

    // ---- 生成 ----
    let median_height = if target_surface == SURFACE_WATER {
        median_land_height(grid, &mask, w)
    } else {
        0
    };
    let mut written = vec![false; mask.len()];
    let mut changed = 0usize;
    for (i, &inside) in mask.iter().enumerate() {
        if !inside {
            continue;
        }
        let (x, y) = ((i % w as usize) as i32, (i / w as usize) as i32);
        let Some(cell) = grid.get(x, y) else {
            continue;
        };
        let is_water = crate::edit::is_water_cell(cell);
        let new_cell = if target_surface == SURFACE_WATER {
            // 水群系（湖）：陆格压成水面且**高度取中位**（D14 湖面平）；
            // 已有水格只补群系、保留原高度。
            let height = if is_water { cell.height } else { median_height };
            crate::TerrainCell {
                height,
                surface: SURFACE_WATER,
                biome,
            }
        } else {
            // 陆群系：水格保持水（Q3 精神，不把湖变成森林）。
            if is_water {
                continue;
            }
            crate::TerrainCell {
                height: cell.height,
                surface: SURFACE_LAND,
                biome,
            }
        };
        if new_cell != cell {
            grid.set(x, y, new_cell);
            changed += 1;
        }
        written[i] = true;
    }
    if !written.iter().any(|&b| b) {
        return Err(MarkerError::EmptyRegion);
    }
    let (outer, holes) = rings_of(&written, w, h)?;
    Ok(MarkerRegion {
        outer,
        holes,
        outcome: MarkerOutcome::Generated,
        cells_changed: changed,
    })
}

/// 包络内**陆格**高度的中位数（全水 → 0）。
fn median_land_height(grid: &TerrainGrid, mask: &[bool], w: u32) -> i16 {
    let mut heights: Vec<i16> = Vec::new();
    for (i, &inside) in mask.iter().enumerate() {
        if !inside {
            continue;
        }
        let (x, y) = ((i % w as usize) as i32, (i / w as usize) as i32);
        if let Some(cell) = grid.get(x, y) {
            if !crate::edit::is_water_cell(cell) {
                heights.push(cell.height);
            }
        }
    }
    if heights.is_empty() {
        return 0;
    }
    heights.sort_unstable();
    heights[heights.len() / 2]
}

/// 环组 =（外环，洞列表）。
type RingsWithHoles = (Vec<(f32, f32)>, Vec<Vec<(f32, f32)>>);

/// 掩膜 →（最大外环，该环的洞）。
fn rings_of(mask: &[bool], w: u32, h: u32) -> Result<RingsWithHoles, MarkerError> {
    let rings = mask_boundary_rings(mask, w, h);
    let (outers, mut grouped) = split_outers_and_holes(rings);
    let Some((idx, _)) = outers.iter().enumerate().max_by(|a, b| {
        area(a.1)
            .partial_cmp(&area(b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return Err(MarkerError::EmptyRegion);
    };
    let holes = if idx < grouped.len() {
        std::mem::take(&mut grouped[idx])
    } else {
        Vec::new()
    };
    Ok((outers[idx].clone(), holes))
}

fn area(ring: &[(f32, f32)]) -> f32 {
    let n = ring.len();
    let mut a = 0.0f32;
    for i in 0..n {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % n];
        a += x0 * y1 - x1 * y0;
    }
    (a / 2.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BIOME_FOREST, BIOME_GRASSLAND, BIOME_LAKE};

    fn flat_land(w: u32, h: u32, height: i16) -> TerrainGrid {
        let mut g = TerrainGrid::new(w, h);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                g.set(
                    x,
                    y,
                    crate::TerrainCell {
                        height,
                        surface: SURFACE_LAND,
                        biome: crate::BIOME_NONE,
                    },
                );
            }
        }
        g
    }

    fn sq(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    #[test]
    fn lake_marker_flattens_to_local_median_height() {
        // 坡地：西高东低（80..120），包络盖住中段 → 湖面 = 中位海拔（≈100），
        // 不是挖碗（不得出现负高度）。
        let mut g = flat_land(64, 64, 100);
        for y in 0..64 {
            for x in 0..64 {
                let mut c = g.get(x, y).unwrap();
                c.height = 80 + (x as i16 / 3);
                g.set(x, y, c);
            }
        }
        let envelope = sq(20.0, 20.0, 40.0, 40.0);
        let r = mark_region(&mut g, &envelope, BIOME_LAKE, 1.0).expect("湖标记");
        assert_eq!(r.outcome, MarkerOutcome::Generated);
        // 包络内全部为水、群系 LAKE、高度统一为同一中位值（湖面平）。
        let mut heights = Vec::new();
        for y in 21..40 {
            for x in 21..40 {
                let c = g.get(x, y).unwrap();
                assert_eq!(c.surface, SURFACE_WATER, "格({x},{y}) 应为水");
                assert_eq!(c.biome, BIOME_LAKE, "格({x},{y}) 应为湖泊群系");
                heights.push(c.height);
            }
        }
        let first = heights[0];
        assert!(
            heights.iter().all(|&h| h == first),
            "湖面必须等高，实得 {heights:?}"
        );
        assert!(first > 0, "湖面在陆地上（正海拔），实得 {first}");
    }

    #[test]
    fn existing_forest_is_identified_without_touching_cells() {
        let mut g = flat_land(64, 64, 100);
        // 包络内 16×16 的森林块。
        for y in 20..36 {
            for x in 20..36 {
                let mut c = g.get(x, y).unwrap();
                c.biome = BIOME_FOREST;
                g.set(x, y, c);
            }
        }
        let before = g.clone();
        let envelope = sq(16.0, 16.0, 40.0, 40.0);
        let r = mark_region(&mut g, &envelope, BIOME_FOREST, 1.0).expect("森林标记");
        assert_eq!(r.outcome, MarkerOutcome::Identified, "已有森林应走识别");
        assert_eq!(r.cells_changed, 0, "识别不得写格子");
        assert_eq!(g, before, "识别分支地形必须逐位不变");
        // 边界应贴着森林块（≈16×16）：周长 ≈64。
        let perim: f32 = (0..r.outer.len())
            .map(|i| {
                let a = r.outer[i];
                let b = r.outer[(i + 1) % r.outer.len()];
                ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
            })
            .sum();
        assert!(
            (perim - 64.0).abs() < 8.0,
            "识别边界应贴 16×16 森林块（周长≈64），实得 {perim}"
        );
    }

    #[test]
    fn lake_median_is_the_middle_land_height_not_the_mean() {
        // 高度 80..120 的斜坡：中位数 = 100（均值也≈100——用右偏分布区分：
        // 90 格给 80 m、10 格给 200 m → 中位 80、均值 92）。
        let mut g = flat_land(64, 64, 80);
        for y in 20..30 {
            for x in 20..30 {
                let mut c = g.get(x, y).unwrap();
                c.height = 200;
                g.set(x, y, c);
            }
        }
        let envelope = sq(20.0, 20.0, 50.0, 50.0);
        mark_region(&mut g, &envelope, BIOME_LAKE, 1.0).expect("湖");
        // 包络内 900 格中 100 格是 200 m，其余 800 格 80 m → 中位 80。
        let c = g.get(40, 40).unwrap();
        assert!(c.surface == SURFACE_WATER);
        assert_eq!(c.height, 80, "湖面应取中位海拔 80（不是均值）");
    }

    #[test]
    fn land_biome_over_all_water_errors_without_writing() {
        let mut g = TerrainGrid::new(32, 32);
        for y in 0..32 {
            for x in 0..32 {
                g.set(
                    x,
                    y,
                    crate::TerrainCell {
                        height: -50,
                        surface: SURFACE_WATER,
                        biome: crate::BIOME_NONE,
                    },
                );
            }
        }
        let before = g.clone();
        assert_eq!(
            mark_region(&mut g, &sq(4.0, 4.0, 20.0, 20.0), BIOME_FOREST, 1.0),
            Err(MarkerError::EmptyRegion)
        );
        assert_eq!(g, before, "失败不得改动地形");
    }

    #[test]
    fn generate_skips_water_cells_for_land_biomes() {
        let mut g = flat_land(64, 64, 100);
        // 包络正中一片湖（水域）。
        for y in 26..38 {
            for x in 26..38 {
                g.set(
                    x,
                    y,
                    crate::TerrainCell {
                        height: -20,
                        surface: SURFACE_WATER,
                        biome: crate::BIOME_NONE,
                    },
                );
            }
        }
        let envelope = sq(16.0, 16.0, 48.0, 48.0);
        let r = mark_region(&mut g, &envelope, BIOME_GRASSLAND, 1.0).expect("草原标记");
        assert_eq!(r.outcome, MarkerOutcome::Generated);
        // 水域保持水与负高度。
        for y in 27..37 {
            for x in 27..37 {
                let c = g.get(x, y).unwrap();
                assert_eq!(c.surface, SURFACE_WATER, "湖格({x},{y}) 不得被陆群系改写");
                assert!(c.height < 0, "湖格高度不得被抬升");
            }
        }
        // 陆格写成草原。
        let c = g.get(20, 20).unwrap();
        assert_eq!(c.biome, BIOME_GRASSLAND);
        assert_eq!(c.surface, SURFACE_LAND);
    }

    #[test]
    fn degenerate_envelope_errors_not_panics() {
        let mut g = flat_land(16, 16, 0);
        assert_eq!(
            mark_region(&mut g, &[(0.0, 0.0), (1.0, 1.0)], BIOME_FOREST, 1.0),
            Err(MarkerError::TooFewPoints)
        );
        assert_eq!(
            mark_region(&mut g, &sq(0.0, 0.0, 1.0, 1.0), BIOME_FOREST, 0.0),
            Err(MarkerError::TooFewPoints)
        );
    }
}
