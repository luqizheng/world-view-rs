//! 世界模板的纯数据生成内核（ADR-0037 M2-1）。
//!
//! ## 职责
//! 按「单一纬度温度轴」生成一份可以直接落 `terrain.bin` 的世界模板：
//! - 西大陆：北端雪原 / 苔原，中部森林与平原，南端山脉；
//! - 东大陆：北部草原，中部干旱荒漠（沙丘），南部稀树草原；
//! - 两陆之间是深水海峡 + 少量岛屿。
//!
//! ## 边界（不做什么）
//! - **不碰文件系统**：只返回内存网格，落盘由 `wv-io` 负责（本 crate 零依赖）。
//! - **不依赖 `wv-core`**：`ClimateCell` 是本 crate 的扁平结构，`wv-app`
//!   负责转成 `wv_core::terrain::TerrainData`（与 `wv-terrain` / `wv-core`
//!   平行的既有纪律一致）。
//! - **不生成河流 / 城市**：那是 ADR-0037 M2-2 / M2-3 的独立任务。
//!
//! ## 确定性
//! 同一 `(width, height, seed)` 必须逐格一致——模板落盘后重生成（M2-4）
//! 依赖这条契约。

use crate::biome::{
    BIOME_DESERT, BIOME_FOREST, BIOME_GOBI, BIOME_GRASSLAND, BIOME_HILLS, BIOME_MOUNTAINS,
    BIOME_OCEAN, BIOME_PLAINS, BIOME_SNOWFIELD, BIOME_TUNDRA,
};
use crate::generate::Perlin;

/// 模板默认边长（与演示工程 `examples/jrpg-world` 同口径）。
pub const DEFAULT_SIZE: u32 = 1024;
/// 默认种子（演示工程 / 测试用；新建工程由 app 层取系统时间另生成）。
pub const DEFAULT_SEED: u64 = 0x0D0E_5EED_2026;

/// 一个模板格：高度 + 地表二值 + 群系编号。
///
/// 字段与 `wv_core::terrain::TerrainCellData` 同构，但不引入 wv-core 依赖：
/// 转换在 `wv-app` 的边界处做一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClimateCell {
    pub height: i16,
    pub surface: u8,
    pub biome: u8,
}

/// 一份完整的气候世界网格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClimateWorld {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<ClimateCell>,
}

impl ClimateWorld {
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

/// 生成「冷北—绿中—干旱南」双大陆世界（ADR-0037 D3）。
///
/// 坐标系：行 0 = 北（与 ADR-0034 上北下南一致），列 0 = 西。
pub fn generate_cold_north(width: u32, height: u32, seed: u64) -> ClimateWorld {
    let width = width.max(2);
    let height = height.max(2);
    let perlin = Perlin::new(seed);
    let mut cells = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let xn = x as f64 / width as f64;
            let yn = y as f64 / height as f64;

            // 两块大陆的椭圆包络 + 低频噪声扰动海岸。
            let west = continent(xn, yn, 0.27, 0.43, 0.17, 0.37, &perlin);
            let east = continent(xn, yn, 0.73, 0.45, 0.21, 0.39, &perlin);
            let edge = west.max(east);

            // 山脉：沿两条南北向脊线叠加高斯隆起（西窄东宽）。
            let ridge_w = ridge(xn, yn, 0.275, 0.035, &perlin);
            let ridge_e = ridge(xn, yn, 0.705, 0.045, &perlin);
            let south_w = ridge_diag(xn, yn, 0.30, 0.78, 0.035, &perlin);

            let detail = fbm01(&perlin, xn * 9.0, yn * 9.0, 4);
            let (height, surface, biome) = if edge <= 0.0 {
                let depth = if edge > -0.055 {
                    -40.0 - 90.0 * detail
                } else {
                    -280.0 - 2600.0 * detail * detail
                };
                (depth as i16, 0u8, BIOME_OCEAN)
            } else {
                let base = 45.0 + 210.0 * edge + 130.0 * detail;
                let h = base
                    + ridge_w * 2200.0
                    + ridge_e * 2300.0
                    + south_w * 1800.0
                    + (fbm01(&perlin, xn * 22.0, yn * 22.0, 3) - 0.5) * 120.0;
                let h = h.max(8.0);
                (h as i16, 1u8, classify(xn, yn, h, &perlin))
            };
            cells.push(ClimateCell {
                height,
                surface,
                biome,
            });
        }
    }

    ClimateWorld {
        width,
        height,
        cells,
    }
}

/// 椭圆大陆包络：>0 为陆地，值越大越靠内陆。
fn continent(xn: f64, yn: f64, cx: f64, cy: f64, rx: f64, ry: f64, perlin: &Perlin) -> f64 {
    let dx = (xn - cx) / rx;
    let dy = (yn - cy) / ry;
    let d = (dx * dx + dy * dy).sqrt();
    // 低频噪声扰动海岸线（振幅 0.24≈1/4 半径）。
    let n = fbm01(perlin, xn * 5.0 + 31.0, yn * 5.0 - 17.0, 4);
    1.0 - d + (n - 0.5) * 0.48
}

/// 南北向山脊：到脊线的距离 → 高斯隆起系数 [0, 1]。
fn ridge(xn: f64, yn: f64, x_at: f64, width: f64, perlin: &Perlin) -> f64 {
    let wobble = (fbm01(perlin, yn * 6.0 + 7.0, 3.0, 3) - 0.5) * 0.05;
    let d = (xn - (x_at + wobble)).abs();
    let g = (-(d / width).powi(2)).exp();
    g * (0.55 + 0.65 * fbm01(perlin, xn * 14.0, yn * 14.0, 3))
}

/// 斜向山脊（南端山脉，走向西北—东南）。
fn ridge_diag(xn: f64, yn: f64, x_at: f64, y_at: f64, width: f64, perlin: &Perlin) -> f64 {
    let d = ((xn - (x_at + (yn - y_at) * 0.35)).abs().powi(2) / 2.0
        + (yn - y_at).abs().powi(2) * 0.15)
        .sqrt();
    let g = (-(d / width).powi(2)).exp();
    g * (0.5 + 0.6 * fbm01(perlin, xn * 11.0 + 5.0, yn * 11.0, 3))
}

/// 群系分类：高度分带 + 纬度温度 + 东部干燥度（ADR-0037 D3 的唯一落表点）。
fn classify(xn: f64, yn: f64, h: f64, perlin: &Perlin) -> u8 {
    let lat = 1.0 - yn; // 上北下南：yn=0 是北。
    let west = xn < 0.50;
    // 东陆干旱带：中纬最干（沙丘），南北两端回绿——参考图右半的构图。
    let east_arid = if west {
        0.0
    } else {
        let band = 1.0 - ((yn - 0.50).abs() / 0.30).min(1.0);
        ((xn - 0.58) * 2.2).clamp(0.0, 1.0) * band
    };
    let forest = fbm01(perlin, xn * 13.0 - 41.0, yn * 13.0 + 23.0, 4);

    if h >= 2400.0 {
        BIOME_SNOWFIELD
    } else if h >= 1250.0 {
        BIOME_MOUNTAINS
    } else if h >= 620.0 {
        BIOME_HILLS
    } else if west && lat >= 0.86 {
        BIOME_SNOWFIELD
    } else if west && lat >= 0.74 {
        BIOME_TUNDRA
    } else if east_arid >= 0.52 {
        BIOME_DESERT
    } else if east_arid >= 0.34 {
        BIOME_GOBI
    } else if forest >= 0.55 {
        BIOME_FOREST
    } else if forest >= 0.44 {
        BIOME_GRASSLAND
    } else {
        BIOME_PLAINS
    }
}

/// 河流折线（ADR-0037 D5）：由高度场 + 种子**派生**的水系。
///
/// ## 算法
/// 1. 源头：高海拔（>= 700m）陆地格中按哈希序挑选，格间距 >= 边长/12；
/// 2. 走线：8 邻域最低点下降 + 种子抖动（同高时哈希裁决），逐步走向低处；
/// 3. 终止：入水（height < 0）、入坑（邻域全更高 -> 湖泊意象）、步数耗尽；
/// 4. 短于 8 点的细流丢弃。
///
/// 返回归一化 `[0,1]^2` 折线（y 向下 = 行序方向，行 0 = 北）。调用方负责换算
/// 到图局部坐标（`x*extent.width, y*extent.height`）。同一 `(输入, seed)`
/// 逐点一致——河流是派生数据不落盘（AGENTS 4.2 只存参数），重生成自动跟随。
pub fn generate_rivers(
    width: u32,
    height: u32,
    heights: &[i16],
    seed: u64,
    max_rivers: usize,
) -> Vec<Vec<[f32; 2]>> {
    let (w, h) = (width.max(2), height.max(2));
    if heights.len() < (w * h) as usize || max_rivers == 0 {
        return Vec::new();
    }
    let at = |x: i64, y: i64| -> i16 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            i16::MIN
        } else {
            heights[y as usize * w as usize + x as usize]
        }
    };
    let min_src = (w.max(h) as i64 / 12).max(8);

    // 候选源头：高地格，按行序扫，哈希筛 + 间距约束。
    let mut sources: Vec<(i64, i64)> = Vec::new();
    'outer: for y in 0..h as i64 {
        for x in 0..w as i64 {
            if at(x, y) < 700 {
                continue;
            }
            if hash01(x as u64, y as u64, seed ^ 0x51AB) > 0.30 {
                continue;
            }
            if sources
                .iter()
                .any(|(sx, sy)| (sx - x).abs().max((sy - y).abs()) < min_src)
            {
                continue;
            }
            sources.push((x, y));
            if sources.len() >= max_rivers * 3 {
                break 'outer;
            }
        }
    }

    let mut out = Vec::new();
    for (si, (sx, sy)) in sources.iter().enumerate() {
        if out.len() >= max_rivers {
            break;
        }
        let mut pts: Vec<[f32; 2]> = Vec::new();
        let (mut cx, mut cy) = (*sx, *sy);
        let max_steps = (w.max(h) as usize) * 2;
        for _step in 0..max_steps {
            pts.push([(cx as f32 + 0.5) / w as f32, (cy as f32 + 0.5) / h as f32]);
            if at(cx, cy) < 0 {
                break; // 入海
            }
            // 8 邻域最低（种子抖动打破平局，让河有蜿蜒）。
            let mut best: Option<(i64, i64, i32)> = None;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (cx + dx, cy + dy);
                    let hh = at(nx, ny);
                    if hh == i16::MIN {
                        continue;
                    }
                    let jitter = (hash01(nx as u64, ny as u64, seed ^ (si as u64)) * 8.0) as i32;
                    let score = hh as i32 + jitter;
                    if best.map(|(_, _, b)| score < b).unwrap_or(true) {
                        best = Some((nx, ny, score));
                    }
                }
            }
            let Some((nx, ny, _)) = best else {
                break;
            };
            if at(nx, ny) > at(cx, cy) {
                break; // 入坑（四周全更高）：湖泊意象，止步
            }
            cx = nx;
            cy = ny;
        }
        if pts.len() >= 8 {
            out.push(pts);
        }
    }
    out
}

/// 城市初始落点（ADR-0037 D6）：宜居群系里的散点，保持间距。
///
/// 宜居 = 陆地、非山非雪（height < 620 且 >= 8）且群系为平原 / 草原 / 森林。
/// 返回归一化 `[0,1]^2`（同 [`generate_rivers`] 口径）；不足 `count` 个时
/// 有几个给几个（网格太小时调用方自行兜底）。
pub fn pick_city_sites(
    width: u32,
    height: u32,
    heights: &[i16],
    biomes: &[u8],
    seed: u64,
    count: usize,
) -> Vec<[f32; 2]> {
    use crate::biome::{BIOME_FOREST, BIOME_GRASSLAND, BIOME_PLAINS};
    let (w, h) = (width.max(2), height.max(2));
    if heights.len() < (w * h) as usize || biomes.len() < (w * h) as usize || count == 0 {
        return Vec::new();
    }
    // 间距（归一化）：count 个点大致均摊对角线。
    let spacing = (1.0f64 / (count as f64).sqrt() * 0.85).clamp(0.10, 0.45);
    let mut out: Vec<[f32; 2]> = Vec::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if out.len() >= count {
                return out;
            }
            let idx = y as usize * w as usize + x as usize;
            let hh = heights[idx];
            if !(8..620).contains(&hh) {
                continue;
            }
            if !matches!(biomes[idx], BIOME_PLAINS | BIOME_GRASSLAND | BIOME_FOREST) {
                continue;
            }
            if hash01(x as u64, y as u64, seed ^ 0xC177) > 0.04 {
                continue;
            }
            let px = (x as f64 + 0.5) / w as f64;
            let py = (y as f64 + 0.5) / h as f64;
            if out.iter().any(|[ox, oy]| {
                let dx = f64::from(*ox) - px;
                let dy = f64::from(*oy) - py;
                (dx * dx + dy * dy).sqrt() < spacing
            }) {
                continue;
            }
            out.push([px as f32, py as f32]);
        }
    }
    out
}

/// splitmix 风格的格哈希 -> [0,1]（与渲染层 terrain_decor 同族，本地副本）。
fn hash01(x: u64, y: u64, salt: u64) -> f64 {
    let mut z = x
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(y.wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(salt);
    z ^= z >> 30;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// Perlin fbm → [0, 1]。
fn fbm01(perlin: &Perlin, x: f64, y: f64, octaves: u32) -> f64 {
    let n = perlin.fbm(x, y, octaves, 2.0, 0.5);
    ((n + 1.0) * 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_is_deterministic() {
        let a = generate_cold_north(96, 96, 42);
        let b = generate_cold_north(96, 96, 42);
        assert_eq!(a, b, "同种子必须逐格一致");
    }

    #[test]
    fn different_seeds_change_the_coastline() {
        let a = generate_cold_north(96, 96, 42);
        let b = generate_cold_north(96, 96, 43);
        assert_ne!(a, b, "不同种子应产生不同海岸线 / 山脉");
    }

    #[test]
    fn generated_world_contains_water_and_land() {
        let w = generate_cold_north(192, 192, DEFAULT_SEED);
        assert_eq!(w.cell_count(), 192 * 192);
        let water = w.cells.iter().filter(|c| c.surface == 0).count();
        let land = w.cell_count() - water;
        assert!(water > 0, "模板必须有海");
        assert!(land > 0, "模板必须有陆");
        assert!(water < w.cell_count(), "不能整张图都是海");
    }

    #[test]
    fn rivers_are_deterministic_and_start_on_highland() {
        let world = generate_cold_north(192, 192, DEFAULT_SEED);
        let heights: Vec<i16> = world.cells.iter().map(|c| c.height).collect();
        let a = generate_rivers(192, 192, &heights, 99, 12);
        let b = generate_rivers(192, 192, &heights, 99, 12);
        assert_eq!(a, b, "同种子河流必须逐点一致");
        assert!(!a.is_empty(), "冷北模板应能长出河流");
        for river in &a {
            assert!(river.len() >= 8, "细流应被丢弃，得 {} 点", river.len());
            let [sx, sy] = river[0];
            let gx = (sx * 192.0).clamp(0.0, 191.0) as usize;
            let gy = (sy * 192.0).clamp(0.0, 191.0) as usize;
            assert!(
                heights[gy * 192 + gx] >= 700,
                "源头必须在高地，得 {}",
                heights[gy * 192 + gx]
            );
        }
        let c = generate_rivers(192, 192, &heights, 100, 12);
        assert_ne!(a, c, "不同种子应有不同水系");
    }

    #[test]
    fn city_sites_sit_on_habitable_land_with_spacing() {
        use crate::biome::{BIOME_DESERT, BIOME_PLAINS};
        let (w, h) = (64u32, 64u32);
        let mut heights = vec![100i16; (w * h) as usize];
        let mut biomes = vec![BIOME_PLAINS; (w * h) as usize];
        // 左半是沙漠（不可居）。
        for y in 0..h as usize {
            for x in 0..w as usize / 2 {
                let i = y * w as usize + x;
                biomes[i] = BIOME_DESERT;
                heights[i] = 40;
            }
        }
        let sites = pick_city_sites(w, h, &heights, &biomes, 7, 6);
        assert!(
            sites.len() >= 4,
            "64^2 半图平原应能落至少 4 城，得 {}",
            sites.len()
        );
        for [nx, _ny] in &sites {
            assert!(*nx > 0.5, "城市必须落在平原半边（x > 0.5），得 {nx}");
        }
        for i in 0..sites.len() {
            for j in (i + 1)..sites.len() {
                let dx = (sites[i][0] - sites[j][0]) as f64;
                let dy = (sites[i][1] - sites[j][1]) as f64;
                assert!(
                    (dx * dx + dy * dy).sqrt() >= 0.30,
                    "城市间距过近：{:?} vs {:?}",
                    sites[i],
                    sites[j]
                );
            }
        }
    }

    #[test]
    fn cold_north_has_snow_desert_and_forest() {
        let w = generate_cold_north(192, 192, DEFAULT_SEED);
        let has = |b: u8| w.cells.iter().any(|c| c.surface == 1 && c.biome == b);
        assert!(has(BIOME_SNOWFIELD), "冷北模板必须出现雪原");
        assert!(has(BIOME_DESERT), "冷北模板必须出现沙漠");
        assert!(has(BIOME_FOREST), "冷北模板必须出现森林");
    }
}
