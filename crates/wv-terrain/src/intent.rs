//! 意图笔刷：作者给「意图 + 大致区域」，程序长出自然地形（2026-10-15，意图笔刷 plan v0.1）。
//!
//! ## 与 [`edit`] 的区别（为什么是两个模块）
//!
//! [`edit`] 是**直接笔刷**：作者涂到哪格改哪格（硬圆 + falloff）——精确，但形状
//! 是几何的（正圆、平边），一眼「程序味」。本模块是**意图生成**：作者只圈定
//! 「这里要一条山脉 / 一座火山 / 一片高原」，山脊走向、沿线起伏、两头收尖、
//! 跨海自动成岛等自然细节由噪声与拓扑规则补全。
//!
//! 思路移植自 civ-atlas 的 terrainEdits / carveLakes / carveRivers
//! （`.ref/civ-atlas`，AGPL-3.0 —— **只参考算法结构与数值比例，代码全部重写**，
//! 未复制任何源码）。
//!
//! ## 职责边界
//!
//! - **纯函数 + 可变网格**：不感知 Undo/Redo（wv-edit 记 before/after diff）、
//!   不感知笔刷光标（那是 wv-app 的事）。
//! - **零第三方依赖**：噪声复用 [`crate::generate::Perlin`]。
//! - **确定性**：所有随机性 = `Perlin::new(seed_mix(seed, 用途))`；同 seed + 同
//!   intent → 逐格相同。这是 undo 后 redo 重放不漂移的前提（seed 由命令在构造时
//!   派生并随命令保存，见 [`Intent::derive_seed`]）。
//! - **不做侵蚀**（plan 裁决，MVP 无流水侵蚀）：火山没有放射状水系、山脉没有
//!   V 形谷。函数结构上「先算场、后落格」，第二刀加侵蚀时插在中间即可。
//!
//! ## 数值口径
//!
//! 高度单位 = 米（[`Height`]），海平面 = 0；强度 `strength` 夹在 [0.2, 2.0]。
//! 峰值量级：山脉/火山中心 ≈ 2400 m × strength，高原顶面 ≈ 1200 m × strength，
//! 丘陵起伏 ≤ 300 m × strength。这些是初始默认值，实机校准后可调。

use std::collections::HashSet;

use crate::edit::is_water_cell;
use crate::generate::Perlin;
use crate::{
    Height, TerrainCell, TerrainGrid, BIOME_GRASSLAND, BIOME_HILLS, BIOME_MOUNTAINS, BIOME_NONE,
    SURFACE_LAND, SURFACE_WATER,
};

// ---------------------------------------------------------------------------
// 数值常量（各意图）
// ---------------------------------------------------------------------------

/// 强度夹取范围（与 civ-atlas 的 TERRAIN_S 同口径）。
const STRENGTH_MIN: f32 = 0.2;
const STRENGTH_MAX: f32 = 2.0;

/// 山脉脊线中心高度（米 × strength）。
const RANGE_PEAK_M: f32 = 2400.0;
/// 山脉影响半径 = `radius × 2.0`（`exp(-1.4·2²) ≈ 0.004`，之外可忽略）。
const RANGE_REACH: f32 = 2.0;
/// 山脊走向噪声的特征波长（世界单位）。
const RANGE_RIDGE_SCALE: f32 = 60.0;
/// 山脊噪声的调幅范围中心与半幅：`v = 0.78 + 0.32·n`（n ∈ [-1,1]）。
const RANGE_VAR_BASE: f32 = 0.78;
const RANGE_VAR_AMP: f32 = 0.32;
/// 高度增量超过此值（米）才写山地 kind——低肩保持原类型。
const RANGE_KIND_ADD_M: f32 = 500.0;

/// 高原顶面高度（米 × strength），顶面 85% 平坦 + 15% 起伏。
const PLATEAU_TOP_M: f32 = 1200.0;
const PLATEAU_FLAT: f32 = 0.85;

/// 丘陵最大起伏（米 × strength）。
const HILLS_AMP_M: f32 = 300.0;

/// 火山峰高（米 × strength，锥剖面 `(1-t)^1.6`）。
const VOLCANO_PEAK_M: f32 = 2400.0;
/// 火山口半径（× 笔刷半径）：口内从环沿往中心回挖。
const VOLCANO_CRATER_T: f32 = 0.18;
/// 火山口底 = 环沿高度 × (1 - 0.45)。
const VOLCANO_CRATER_DEPTH: f32 = 0.45;
/// 深海底往火山方向抬升的目标深度（米，海山效果）。
const VOLCANO_SEAMOUNT_M: f32 = -60.0;
/// 海域非核心区的深度上限（不许擦着海平面露出，保持浅滩）。
const VOLCANO_SHALLOW_CAP_M: f32 = -20.0;

/// 湖：碗沿比碗外最低邻居（溢出口）再低的保护量（米）。
const LAKE_LIP_M: f32 = 6.0;
/// 湖中心最大水深（米 × strength，边缘按 (1-t²) 收浅）。
const LAKE_DEPTH_M: f32 = 50.0;

/// 河：河床比两岸最低格再低多少（米 × strength）。
///
/// `pub(crate)`：走廊下切（[`crate::line_path::carve_corridor`]）复用同一常量
/// ——河谷与道路走廊的下切深度必须同源，否则同一处地形在两条路径上会被
/// 切出不同的深度。
pub(crate) const RIVER_CUT_M: f32 = 12.0;
/// 河：沿链向下游每格至少再降多少（米）。
///
/// `pub(crate)`：同 [`RIVER_CUT_M`]——走廊下切复用，不重定义。
pub(crate) const RIVER_DROP_M: f32 = 0.4;

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 一次地形意图。坐标与半径均为**图内局部坐标 / 世界单位**（与 [`crate::edit::Brush`]
/// 同口径），`apply_intent` 内部按 `cell_size` 换算成格。
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    /// 山脉：沿 `pts` 折线长一道山脊。单点 = 一座圆山。
    /// 山脊走向与起伏由 ridged 噪声给出；跨海的段自动冒出岛链 / 半岛。
    MountainRange {
        /// 脊线折点（世界单位）。
        pts: Vec<(f32, f32)>,
        /// 山体半宽（世界单位）。
        radius: f32,
        /// 强度 [0.2, 2.0]：中心高度 ≈ 2400 m × strength。
        strength: f32,
    },
    /// 高原：`pts` 周围抬成一片**顶面平坦**的高地（85% 平 + 15% 起伏），
    /// 边缘平滑收落；不压低已有的更高地形。
    Plateau {
        pts: Vec<(f32, f32)>,
        radius: f32,
        /// 强度：顶面 ≈ 1200 m × strength。
        strength: f32,
    },
    /// 丘陵：`pts` 周围加一片低矮起伏（≤ 300 m × strength）。只在陆上生效。
    Hills {
        pts: Vec<(f32, f32)>,
        radius: f32,
        strength: f32,
    },
    /// 火山：点一下放一座锥形山，顶部带火山口凹陷；点在海里自动成岛，
    /// 周边深海抬成海山、近岸留浅滩。
    Volcano {
        center: (f32, f32),
        radius: f32,
        /// 强度：峰高（环沿）≈ 2400 m × strength。
        strength: f32,
    },
    /// 湖：点一下挖一个碗形洼地，**碗底比碗外最低邻居（溢出口）还低**，
    /// 水自然积在碗里。湖心必须落在陆地上（在海上点湖 = 无操作）。
    Lake {
        center: (f32, f32),
        radius: f32,
        /// 强度：中心水深 ≈ 50 m × strength。
        strength: f32,
    },
    /// 河：从源头拖到河口的一条线。程序密采点落成格链，沿链**拓扑挖深**
    /// （每格比两岸最低格低、比上游低），水就顺着链流。两端都在陆上时，
    /// 高的一端当源头；一头在水里则那头是河口。河宽固定一格。
    River { pts: Vec<(f32, f32)>, strength: f32 },

    // ---- 区域意图（2026-10-17 选择工具 spec §4；多边形 = 选区，图内局部 wu）----
    /// 高山群：选区内沿主轴派生脊线跑 [`Intent::MountainRange`] 同款 ridged
    /// 噪声（半径 = 短轴之半）；跨海段自动冒岛链 / 半岛（继承语义，特性）。
    MountainMassif { polygon: Vec<(f32, f32)> },
    /// 丘陵：区域内铺 [`Intent::Hills`] 低矮起伏；只在陆上生效（纯海 = 无操作）。
    HillsRegion { polygon: Vec<(f32, f32)> },
    /// 平原：区域内**主动压平**到 [`PLAINS_TARGET_M`] ± 噪声（与
    /// [`Intent::Plateau`]「不压低更高地形」语义相反——生成平原就是要把
    /// 区域内高地抹平，spec §4 裁定）；只在陆上生效。
    PlainsRegion { polygon: Vec<(f32, f32)> },
    /// 湖泊：区域定湖界的碗形洼地（中心 ≈ [`LAKE_DEPTH_M`]，边缘 ≈ -[`LAKE_LIP_M`]）；
    /// 多边形质心在海上 = 无操作（沿 [`Intent::Lake`]「湖心须陆上」口径）。
    LakeRegion { polygon: Vec<(f32, f32)> },
}

impl Intent {
    /// 从图 ID 与意图内容派生种子（FNV-1a over Debug 表示）。
    ///
    /// wv-app 在构造命令时调用一次、把结果存进命令——命令重放（redo）用
    /// 同一个 seed，结果逐格一致。History 只存在于内存（命令不落盘），
    /// Debug 格式的跨版本稳定性无关紧要。
    pub fn derive_seed(&self, map_key: &str) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in map_key.bytes().chain(format!("{self:?}").bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        h
    }
}

/// 在网格上施加一次地形意图（就地修改）。返回**实际被改动**的格数。
///
/// 这是 `wv-edit::IntentTerrainCommand::apply` 的落点（意图笔刷 plan v0.1）。
/// 越界 / 退化输入（空折线、非正半径、湖心在海里）安全返回 0，不 panic。
pub fn apply_intent(grid: &mut TerrainGrid, intent: &Intent, seed: u64, cell_size: f32) -> usize {
    match intent {
        Intent::MountainRange {
            pts,
            radius,
            strength,
        } => apply_mountain_range(grid, pts, *radius, *strength, seed, cell_size),
        Intent::Plateau {
            pts,
            radius,
            strength,
        } => apply_plateau(grid, pts, *radius, *strength, seed, cell_size),
        Intent::Hills {
            pts,
            radius,
            strength,
        } => apply_hills(grid, pts, *radius, *strength, seed, cell_size),
        Intent::Volcano {
            center,
            radius,
            strength,
        } => apply_volcano(grid, *center, *radius, *strength, seed, cell_size),
        Intent::Lake {
            center,
            radius,
            strength,
        } => apply_lake(grid, *center, *radius, *strength, seed, cell_size),
        Intent::River { pts, strength } => apply_river(grid, pts, *strength, cell_size),
        Intent::MountainMassif { polygon } => {
            let Some((spine, radius)) = region_spine(polygon) else {
                return 0;
            };
            apply_region_masked(grid, polygon, seed, cell_size, |sc| {
                apply_mountain_range(sc, &spine, radius, 1.0, seed, cell_size)
            })
        }
        Intent::HillsRegion { polygon } => {
            let Some(c) = polygon_centroid(polygon) else {
                return 0;
            };
            let r_max = polygon
                .iter()
                .map(|&(x, y)| ((x - c.0).powi(2) + (y - c.1).powi(2)).sqrt())
                .fold(0.0f32, f32::max);
            apply_region_masked(grid, polygon, seed, cell_size, |sc| {
                apply_hills(sc, &[c], r_max, 1.0, seed, cell_size)
            })
        }
        Intent::PlainsRegion { polygon } => {
            apply_region_masked(grid, polygon, seed, cell_size, |sc| {
                apply_plains_region(sc, polygon, seed, cell_size)
            })
        }
        Intent::LakeRegion { polygon } => {
            apply_region_masked(grid, polygon, seed, cell_size, |sc| {
                apply_lake_region(sc, polygon, cell_size)
            })
        }
    }
}

// ---------------------------------------------------------------------------
// 地基：seed 派生 / 噪声采样 / 域扭曲 / 折线最近点 / 工具函数
// ---------------------------------------------------------------------------

/// FNV-1a：seed 与用途标签混合成新 seed——同一意图的不同用途（走向噪声、
/// 扰动、海岸噪声）各自独立取场。
fn seed_mix(seed: u64, tag: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325 ^ seed;
    for b in tag.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 固定频率与倍频数的 fbm 采样器。`scale` 是特征波长（世界单位），越大越平缓。
struct N2 {
    perlin: Perlin,
    scale: f64,
    octaves: u32,
}

impl N2 {
    fn new(seed: u64, tag: &str, scale: f32, octaves: u32) -> Self {
        Self {
            perlin: Perlin::new(seed_mix(seed, tag)),
            scale: scale as f64,
            octaves,
        }
    }

    /// 采样 ∈ [-1, 1]。
    fn at(&self, x: f32, y: f32) -> f32 {
        self.perlin.fbm(
            x as f64 / self.scale,
            y as f64 / self.scale,
            self.octaves,
            2.0,
            0.5,
        ) as f32
    }

    /// 采样映射到 [0, 1]。
    fn at01(&self, x: f32, y: f32) -> f32 {
        self.at(x, y) * 0.5 + 0.5
    }
}

/// 域扭曲：把查询点先挪一下再取场值——所有形状的边界都因此不再有
/// 正圆 / 直边（civ-atlas warper 的同款思路）。
struct Warper {
    wx: N2,
    wy: N2,
}

impl Warper {
    fn new(seed: u64) -> Self {
        Self {
            wx: N2::new(seed, "warp-x", 130.0, 3),
            wy: N2::new(seed, "warp-y", 130.0, 3),
        }
    }

    /// 返回扭曲后的查询点。
    fn apply(&self, x: f32, y: f32, amp: f32) -> (f32, f32) {
        (x + amp * self.wx.at(x, y), y + amp * self.wy.at(x, y))
    }
}

/// 点到折线的最近点信息。
struct PolyNearest {
    /// 最近距离（世界单位）。
    d: f32,
    /// 最近点的沿弧长位置（世界单位）。
    a: f32,
    /// 折线总长（世界单位）。
    len: f32,
    /// 最近点坐标（供「按位置取噪声」——同一处的两道山脉起伏一致）。
    qx: f32,
    qy: f32,
}

/// 折线最近点（逐段投影取最小）。`pts` 必须非空；单点折线 = 圆（d = 到该点距离）。
fn polyline_nearest(pts: &[(f32, f32)], x: f32, y: f32) -> PolyNearest {
    debug_assert!(!pts.is_empty());
    let mut best = PolyNearest {
        d: f32::INFINITY,
        a: 0.0,
        len: 0.0,
        qx: pts[0].0,
        qy: pts[0].1,
    };
    let mut cum = 0.0f32;
    for w in pts.windows(2) {
        let (ax, ay) = w[0];
        let (bx, by) = w[1];
        let dx = bx - ax;
        let dy = by - ay;
        let l2 = dx * dx + dy * dy;
        let seg_len = l2.sqrt();
        let t = if l2 <= f32::EPSILON {
            0.0
        } else {
            (((x - ax) * dx + (y - ay) * dy) / l2).clamp(0.0, 1.0)
        };
        let qx = ax + t * dx;
        let qy = ay + t * dy;
        let d = ((x - qx) * (x - qx) + (y - qy) * (y - qy)).sqrt();
        if d < best.d {
            best = PolyNearest {
                d,
                a: cum + t * seg_len,
                len: 0.0,
                qx,
                qy,
            };
        }
        cum += seg_len;
    }
    if pts.len() == 1 {
        let (px, py) = pts[0];
        best = PolyNearest {
            d: ((x - px) * (x - px) + (y - py) * (y - py)).sqrt(),
            a: 0.0,
            len: 0.0,
            qx: px,
            qy: py,
        };
    }
    best.len = cum;
    best
}

/// 折线 + 外扩距离覆盖的格范围（钳到网格内）。空折线 / 完全越界 → `None`。
fn pts_bbox(
    grid: &TerrainGrid,
    pts: &[(f32, f32)],
    reach: f32,
    cell_size: f32,
) -> Option<(i32, i32, i32, i32)> {
    if pts.is_empty() || cell_size <= 0.0 {
        return None;
    }
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for &(px, py) in pts {
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    let g = |v: f32| (v / cell_size).floor() as i32;
    let x0 = g(min_x - reach).max(0);
    let y0 = g(min_y - reach).max(0);
    let x1 = g(max_x + reach).min(grid.width() as i32 - 1);
    let y1 = g(max_y + reach).min(grid.height() as i32 - 1);
    if x0 > x1 || y0 > y1 {
        None
    } else {
        Some((x0, y0, x1, y1))
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 强度夹取（`pub(crate)`：走廊下切复用同款夹取口径）。
pub(crate) fn clamp_strength(s: f32) -> f32 {
    s.clamp(STRENGTH_MIN, STRENGTH_MAX)
}

/// 浮点高度 → 整米高度（`pub(crate)`：走廊下切复用同款取整口径）。
pub(crate) fn to_height(v: f32) -> Height {
    v.round().clamp(i16::MIN as f32, i16::MAX as f32) as Height
}

// ---------------------------------------------------------------------------
// 山脉
// ---------------------------------------------------------------------------

/// 山脊高度增量（米）：`peak · taper · v · exp(-1.4·t²)`。
///
/// 拆成纯函数是为了手算断言（AGENTS §7.2）：`v=1, taper=1` 时
/// t=0 → 2400、t=0.5 → 2400·e^-0.35 ≈ 1691、t=1 → 2400·e^-1.4 ≈ 592。
fn range_add(t: f32, taper: f32, variation: f32, strength: f32) -> f32 {
    RANGE_PEAK_M * strength * taper * variation * (-1.4 * t * t).exp()
}

fn apply_mountain_range(
    grid: &mut TerrainGrid,
    pts: &[(f32, f32)],
    radius: f32,
    strength: f32,
    seed: u64,
    cell_size: f32,
) -> usize {
    if pts.is_empty() || radius <= 0.0 || cell_size <= 0.0 {
        return 0;
    }
    let s = clamp_strength(strength);
    let reach = radius * RANGE_REACH;
    let warp_amp = 0.45 * radius;
    let ridge = N2::new(seed, "range-ridge", RANGE_RIDGE_SCALE, 4);
    let warp = Warper::new(seed);
    let coast_a = N2::new(seed, "coarse-coast", 22.0, 3);
    let coast_b = N2::new(seed, "fine-coast", 9.0, 3);

    let plen = polyline_nearest(pts, f32::MIN, f32::MIN).len;
    // 两头收尖长度；太短的（点一下）不收，就是一座圆山。
    let taper_len = (0.35 * plen).min(1.8 * radius);

    let Some((x0, y0, x1, y1)) = pts_bbox(grid, pts, reach, cell_size) else {
        return 0;
    };
    let mut touched = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let (qx, qy) = warp.apply(wx, wy, warp_amp);
            let pn = polyline_nearest(pts, qx, qy);
            let t = pn.d / radius;
            if t >= RANGE_REACH {
                continue;
            }
            let taper = if taper_len > 0.5 * radius {
                0.15 + 0.85
                    * smoothstep(0.0, taper_len, pn.a)
                    * smoothstep(0.0, taper_len, plen - pn.a)
            } else {
                1.0
            };
            // 山脊走向：ridged 噪声（1-|fbm| 的双极映射）按最近点位置取。
            let n = ridge.at(pn.qx, pn.qy) * -2.0;
            let variation = RANGE_VAR_BASE + RANGE_VAR_AMP * n.clamp(-1.0, 1.0);
            let add = range_add(t, taper, variation, s);
            let shape = add / (RANGE_PEAK_M * s);
            // 海岸细碎：两种尺度的同族噪声，只影响「成陆判定」的边界。
            let coast = 0.6 * coast_a.at(wx, wy) + 0.4 * coast_b.at(wx, wy);
            let core = shape + 0.14 * coast > 0.55;

            let Some(cell0) = grid.get(gx, gy) else {
                continue;
            };
            let water0 = is_water_cell(cell0);
            let base = if core && water0 {
                0.0
            } else {
                cell0.height as f32
            };
            let mut new_h = base + add;
            // 非核心的水域不许擦着海平面露出：一律压回海平面以下（近岸浅滩）。
            if water0 && !core {
                new_h = new_h.min(-1.0);
            }
            if add < 0.5 && !core {
                continue; // 量太小，不动这格
            }
            let mut surface = cell0.surface;
            let mut biome = cell0.biome;
            if new_h >= 0.0 {
                if add >= RANGE_KIND_ADD_M {
                    surface = SURFACE_LAND;
                    biome = BIOME_MOUNTAINS;
                } else if water0 {
                    surface = SURFACE_LAND;
                    biome = BIOME_GRASSLAND; // 新露出水面的低肩是草原
                }
            }
            let new_cell = TerrainCell {
                height: to_height(new_h),
                surface,
                biome,
            };
            if new_cell != cell0 {
                grid.set(gx, gy, new_cell);
                touched += 1;
            }
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 高原
// ---------------------------------------------------------------------------

fn apply_plateau(
    grid: &mut TerrainGrid,
    pts: &[(f32, f32)],
    radius: f32,
    strength: f32,
    seed: u64,
    cell_size: f32,
) -> usize {
    if pts.is_empty() || radius <= 0.0 || cell_size <= 0.0 {
        return 0;
    }
    let s = clamp_strength(strength);
    let warp_amp = 0.3 * radius;
    let top = N2::new(seed, "plateau-top", 90.0, 4);
    let warp = Warper::new(seed);

    let Some((x0, y0, x1, y1)) = pts_bbox(grid, pts, radius * 1.4, cell_size) else {
        return 0;
    };
    let mut touched = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let (qx, qy) = warp.apply(wx, wy, warp_amp);
            let pn = polyline_nearest(pts, qx, qy);
            let t = pn.d / radius;
            if t >= 1.4 {
                continue;
            }
            let field = 1.0 - smoothstep(0.8, 1.4, t);
            if field <= 0.02 {
                continue;
            }
            // 顶面 85% 平坦 + 15% 起伏。
            let target = PLATEAU_TOP_M
                * s
                * (PLATEAU_FLAT + (1.0 - PLATEAU_FLAT) * top.at01(pn.qx, pn.qy))
                * field;

            let Some(cell0) = grid.get(gx, gy) else {
                continue;
            };
            let new_cell = if is_water_cell(cell0) {
                // 涂到海里：核心区（field ≥ 0.3）长出高原陆地，边缘仍是海。
                if field < 0.3 {
                    continue;
                }
                TerrainCell {
                    height: to_height(target.max(40.0)),
                    surface: SURFACE_LAND,
                    biome: BIOME_HILLS,
                }
            } else {
                let h = (cell0.height as f32).max(target);
                // 已有更高地形（如山脊穿过高原）不降、也不改群系。
                let biome = if field >= 0.55 && target > cell0.height as f32 {
                    BIOME_HILLS
                } else {
                    cell0.biome
                };
                TerrainCell {
                    height: to_height(h),
                    surface: SURFACE_LAND,
                    biome,
                }
            };
            if new_cell != cell0 {
                grid.set(gx, gy, new_cell);
                touched += 1;
            }
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 丘陵
// ---------------------------------------------------------------------------

fn apply_hills(
    grid: &mut TerrainGrid,
    pts: &[(f32, f32)],
    radius: f32,
    strength: f32,
    seed: u64,
    cell_size: f32,
) -> usize {
    if pts.is_empty() || radius <= 0.0 || cell_size <= 0.0 {
        return 0;
    }
    let s = clamp_strength(strength);
    let warp_amp = 0.35 * radius;
    let bumps = N2::new(seed, "hill-bumps", 50.0, 4);
    let warp = Warper::new(seed);

    let Some((x0, y0, x1, y1)) = pts_bbox(grid, pts, radius * 1.3, cell_size) else {
        return 0;
    };
    let mut touched = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let (qx, qy) = warp.apply(wx, wy, warp_amp);
            let pn = polyline_nearest(pts, qx, qy);
            let t = pn.d / radius;
            if t >= 1.2 {
                continue;
            }
            let field = 1.0 - smoothstep(0.6, 1.2, t);
            if field <= 0.02 {
                continue;
            }
            let Some(cell0) = grid.get(gx, gy) else {
                continue;
            };
            if is_water_cell(cell0) {
                continue; // 丘陵只在陆上长（MVP 裁决）
            }
            let bump = HILLS_AMP_M * s * (0.6 + 0.4 * bumps.at01(pn.qx, pn.qy)) * field;
            let biome = if field >= 0.65 {
                BIOME_HILLS
            } else {
                cell0.biome
            };
            let new_cell = TerrainCell {
                height: to_height(cell0.height as f32 + bump),
                surface: SURFACE_LAND,
                biome,
            };
            if new_cell != cell0 {
                grid.set(gx, gy, new_cell);
                touched += 1;
            }
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 火山
// ---------------------------------------------------------------------------

/// 火山锥剖面（米，不含域扭曲）：`t < 1` 为锥 `(1-t)^1.6`；
/// `t < VOLCANO_CRATER_T` 内从环沿往中心线性回挖成火山口。
///
/// 纯函数供手算断言：t=0.3, peak=2400 → 2400·0.7^1.6 ≈ 1356；
/// 环沿（t=0.18）= 2400·0.82^1.6 ≈ 1747；口底（t=0）≈ 1747·0.55 ≈ 961。
fn volcano_profile(t: f32, peak: f32) -> f32 {
    if t >= 1.0 {
        return 0.0;
    }
    let cone = peak * (1.0 - t).powf(1.6);
    if t < VOLCANO_CRATER_T {
        let rim = peak * (1.0 - VOLCANO_CRATER_T).powf(1.6);
        rim * (1.0 - VOLCANO_CRATER_DEPTH * (1.0 - t / VOLCANO_CRATER_T))
    } else {
        cone
    }
}

fn apply_volcano(
    grid: &mut TerrainGrid,
    center: (f32, f32),
    radius: f32,
    strength: f32,
    seed: u64,
    cell_size: f32,
) -> usize {
    if radius <= 0.0 || cell_size <= 0.0 {
        return 0;
    }
    let s = clamp_strength(strength);
    let peak = VOLCANO_PEAK_M * s;
    let warp_amp = 0.2 * radius;
    let warp = Warper::new(seed);
    let coast_a = N2::new(seed, "volcano-coast-a", 22.0, 3);
    let coast_b = N2::new(seed, "volcano-coast-b", 9.0, 3);

    let Some((x0, y0, x1, y1)) = pts_bbox(grid, &[center], radius * 2.6, cell_size) else {
        return 0;
    };
    let mut touched = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let (qx, qy) = warp.apply(wx, wy, warp_amp);
            let t = ((qx - center.0) * (qx - center.0) + (qy - center.1) * (qy - center.1)).sqrt()
                / radius;
            if t >= 2.6 {
                continue;
            }
            let cone_h = if t < 1.0 {
                volcano_profile(t, peak)
            } else {
                0.0
            };
            let coast = 0.6 * coast_a.at(wx, wy) + 0.4 * coast_b.at(wx, wy);
            let core = t + 0.16 * coast < 0.72;
            // 深海底抬升程度（海山）：t∈[0.6, 2.6] 从 1 衰减到 0。
            let floor = 1.0 - smoothstep(0.6, 2.6, t);

            let Some(cell0) = grid.get(gx, gy) else {
                continue;
            };
            let water0 = is_water_cell(cell0);
            let h0 = cell0.height as f32;
            let new_h = if water0 {
                if core {
                    cone_h // 海里成岛：从海平面起锥
                } else {
                    // 非核心海域：锥体余量垫高海底 + 深水向海山顶抬；
                    // 但不许擦着海平面露出——封顶为浅滩。
                    let mut h = h0 + cone_h;
                    if h0 < VOLCANO_SEAMOUNT_M {
                        h = h.max(h0 + (VOLCANO_SEAMOUNT_M - h0) * floor);
                    }
                    h.min(VOLCANO_SHALLOW_CAP_M).max(h0)
                }
            } else {
                h0 + cone_h
            };
            if (new_h - h0).abs() < 0.5 {
                continue;
            }
            let mut surface = cell0.surface;
            let mut biome = cell0.biome;
            if new_h >= 0.0 {
                if cone_h >= 400.0 {
                    surface = SURFACE_LAND;
                    biome = BIOME_MOUNTAINS;
                } else if water0 {
                    surface = SURFACE_LAND;
                    biome = BIOME_GRASSLAND;
                }
            }
            let new_cell = TerrainCell {
                height: to_height(new_h),
                surface,
                biome,
            };
            if new_cell != cell0 {
                grid.set(gx, gy, new_cell);
                touched += 1;
            }
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 湖
// ---------------------------------------------------------------------------

fn apply_lake(
    grid: &mut TerrainGrid,
    center: (f32, f32),
    radius: f32,
    strength: f32,
    seed: u64,
    cell_size: f32,
) -> usize {
    if radius <= 0.0 || cell_size <= 0.0 {
        return 0;
    }
    // 湖心必须落在陆地上：点在海里不挖（civ-atlas 同款裁决）。
    let cxg = (center.0 / cell_size).floor() as i32;
    let cyg = (center.1 / cell_size).floor() as i32;
    let Some(center_cell) = grid.get(cxg, cyg) else {
        return 0;
    };
    if is_water_cell(center_cell) {
        return 0;
    }
    let s = clamp_strength(strength);
    let warp_amp = 0.3 * radius; // 碗沿扭一扭，不是正圆
    let warp = Warper::new(seed);

    // 碗成员可能因扭曲偏移多出 0.3·radius 的余量。
    let Some((x0, y0, x1, y1)) = pts_bbox(grid, &[center], radius * 1.3 + cell_size, cell_size)
    else {
        return 0;
    };
    let bw = (x1 - x0 + 1) as usize;
    let bh = (y1 - y0 + 1) as usize;
    let mut in_bowl = vec![false; bw * bh];
    let idx = |gx: i32, gy: i32| (gy - y0) as usize * bw + (gx - x0) as usize;

    // 第一遍：标碗（扭曲后的距离 < radius 的陆格）。
    let mut bowl: Vec<(i32, i32)> = Vec::new();
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            let (qx, qy) = warp.apply(wx, wy, warp_amp);
            let d = ((qx - center.0) * (qx - center.0) + (qy - center.1) * (qy - center.1)).sqrt();
            if d >= radius {
                continue;
            }
            if let Some(c) = grid.get(gx, gy) {
                if !is_water_cell(c) {
                    in_bowl[idx(gx, gy)] = true;
                    bowl.push((gx, gy));
                }
            }
        }
    }
    if bowl.is_empty() {
        return 0;
    }

    // 碗沿高度 = 碗外一圈邻居里最低的（水邻居按海平面 0 计）——即溢出口。
    let mut rim = f32::INFINITY;
    for &(gx, gy) in &bowl {
        for (nx, ny) in [(gx + 1, gy), (gx - 1, gy), (gx, gy + 1), (gx, gy - 1)] {
            if nx >= x0 && nx <= x1 && ny >= y0 && ny <= y1 && in_bowl[idx(nx, ny)] {
                continue;
            }
            match grid.get(nx, ny) {
                Some(c) => {
                    let v = if is_water_cell(c) {
                        0.0
                    } else {
                        c.height as f32
                    };
                    rim = rim.min(v);
                }
                None => continue, // 越界邻居不算（湖贴地图边缘时口径从紧）
            }
        }
    }
    if !rim.is_finite() {
        rim = center_cell.height as f32;
    }

    // 第二遍：挖碗。碗底 = rim - 唇高 - 深度·(1-t²)，只降不升。
    let mut touched = 0;
    for &(gx, gy) in &bowl {
        let wx = (gx as f32 + 0.5) * cell_size;
        let wy = (gy as f32 + 0.5) * cell_size;
        let (qx, qy) = warp.apply(wx, wy, warp_amp);
        let d = ((qx - center.0) * (qx - center.0) + (qy - center.1) * (qy - center.1)).sqrt();
        let tt = (d / radius).min(1.0);
        let target = rim - LAKE_LIP_M - LAKE_DEPTH_M * s * (1.0 - tt * tt);
        let Some(cell0) = grid.get(gx, gy) else {
            continue;
        };
        let new_cell = TerrainCell {
            height: to_height((cell0.height as f32).min(target)),
            surface: SURFACE_WATER,
            biome: BIOME_NONE,
        };
        if new_cell != cell0 {
            grid.set(gx, gy, new_cell);
            touched += 1;
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 河
// ---------------------------------------------------------------------------

/// 挖一条一格宽的河（既有「涂格河」实现）。
///
/// `pub(crate)`：走廊下切 [`crate::line_path::carve_corridor`] 在
/// 「一格宽 + 写水面」时**直接委托**本函数，保证与既有涂格河行为**逐位
/// 相同**（测试 `width_one_river_matches_legacy_apply_river` 钉死）。
pub(crate) fn apply_river(
    grid: &mut TerrainGrid,
    pts: &[(f32, f32)],
    strength: f32,
    cell_size: f32,
) -> usize {
    if pts.is_empty() || cell_size <= 0.0 {
        return 0;
    }
    let s = clamp_strength(strength);

    // 密采点 → 格链（相邻去重）。步长 < 格径，折线绝不跳格。
    let step = (cell_size / 3.0).max(0.25);
    let mut chain: Vec<(i32, i32)> = Vec::new();
    let push_cell = |x: f32, y: f32, chain: &mut Vec<(i32, i32)>| {
        let gx = (x / cell_size).floor() as i32;
        let gy = (y / cell_size).floor() as i32;
        if gx < 0 || gy < 0 || gx >= grid.width() as i32 || gy >= grid.height() as i32 {
            return;
        }
        if chain.last() != Some(&(gx, gy)) {
            chain.push((gx, gy));
        }
    };
    if pts.len() == 1 {
        return 0; // 单点成不了河
    }
    for w in pts.windows(2) {
        let (ax, ay) = w[0];
        let (bx, by) = w[1];
        let len = ((bx - ax) * (bx - ax) + (by - ay) * (by - ay)).sqrt();
        let steps = (len / step).ceil().max(1.0) as u32;
        for k in 0..=steps {
            let t = k as f32 / steps as f32;
            push_cell(ax + (bx - ax) * t, ay + (by - ay) * t, &mut chain);
        }
    }
    if chain.len() < 2 {
        return 0;
    }

    // 定源头：一头在水 → 另一头是源头；都在陆上 → 高的一头是源头。
    let wet = |c: (i32, i32)| grid.get(c.0, c.1).map(is_water_cell).unwrap_or(true);
    let head = chain[0];
    let tail = *chain.last().expect("链至少 2 格");
    let h_head = grid.get(head.0, head.1).map(|c| c.height).unwrap_or(0);
    let h_tail = grid.get(tail.0, tail.1).map(|c| c.height).unwrap_or(0);
    if (wet(head) && !wet(tail)) || (!wet(head) && !wet(tail) && h_head < h_tail) {
        chain.reverse();
    }
    // 截掉源头端的水段；走到水（河口）就停。
    let mut s0 = 0;
    while s0 < chain.len() && wet(chain[s0]) {
        s0 += 1;
    }
    let mut s1 = s0;
    while s1 < chain.len() && !wet(chain[s1]) {
        s1 += 1;
    }
    let run = &chain[s0..s1];
    if run.len() < 2 {
        return 0;
    }

    // 沿链挖深：每格比两岸最低格低 CUT·s、比上游低 DROP，不低于 1 m。
    let on_chain: HashSet<(i32, i32)> = run.iter().copied().collect();
    let mut prev = f32::INFINITY;
    let mut touched = 0;
    for &(cx, cy) in run {
        let Some(cell0) = grid.get(cx, cy) else {
            continue;
        };
        let mut low = f32::INFINITY;
        for (nx, ny) in [(cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)] {
            if on_chain.contains(&(nx, ny)) {
                continue;
            }
            if let Some(c) = grid.get(nx, ny) {
                let v = if is_water_cell(c) {
                    0.0
                } else {
                    c.height as f32
                };
                low = low.min(v);
            }
        }
        let mut e = cell0.height as f32;
        if low.is_finite() {
            e = e.min(low - RIVER_CUT_M * s);
        }
        e = e.min(prev - RIVER_DROP_M).max(1.0);
        let new_cell = TerrainCell {
            height: to_height(e),
            surface: SURFACE_WATER,
            biome: BIOME_NONE,
        };
        if new_cell != cell0 {
            grid.set(cx, cy, new_cell);
            touched += 1;
        }
        // 传未取整的坡降给下游，保证长河累计下切。
        prev = e;
    }
    touched
}

// ---------------------------------------------------------------------------
// 区域意图（2026-10-17 选择工具 spec §4 / §12）
// ---------------------------------------------------------------------------

/// 平原压平目标高度（米，海平面以上低海拔带中心，spec §4 裁定初值）。
const PLAINS_TARGET_M: f32 = 60.0;
/// 平原目标高度的噪声半幅（米）——压平不等于抹成常数。
const PLAINS_NOISE_M: f32 = 15.0;

/// 多边形质心（鞋带公式；零面积 / 空环回退顶点平均，`None` 仅空环）。
fn polygon_centroid(polygon: &[(f32, f32)]) -> Option<(f32, f32)> {
    let n = polygon.len();
    if n == 0 {
        return None;
    }
    let mut a2 = 0.0f32;
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    for i in 0..n {
        let (x0, y0) = polygon[i];
        let (x1, y1) = polygon[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        a2 += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if a2.abs() < 1e-6 {
        let sx: f32 = polygon.iter().map(|p| p.0).sum();
        let sy: f32 = polygon.iter().map(|p| p.1).sum();
        return Some((sx / n as f32, sy / n as f32));
    }
    Some((cx / (3.0 * a2), cy / (3.0 * a2)))
}

/// 区域主轴脊线（spec §4「高山」行）：协方差主方向为脊走向，脊线端点 =
/// 顶点沿主轴投影的极值向质心收缩 20%（山脊不出掩膜）；返回 `(脊线, 半径)`，
/// 半径 = 垂直半幅之半（spec 裁定「半径 = 短轴之半」）。
pub fn region_spine(polygon: &[(f32, f32)]) -> Option<(Vec<(f32, f32)>, f32)> {
    if polygon.len() < 3 {
        return None;
    }
    let c = polygon_centroid(polygon)?;
    let n = polygon.len() as f32;
    let mut sxx = 0.0f32;
    let mut sxy = 0.0f32;
    let mut syy = 0.0f32;
    for &(x, y) in polygon {
        let (dx, dy) = (x - c.0, y - c.1);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    sxx /= n;
    sxy /= n;
    syy /= n;
    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (dir_x, dir_y) = (theta.cos(), theta.sin());
    let (perp_x, perp_y) = (-dir_y, dir_x);
    let mut t_min = f32::INFINITY;
    let mut t_max = f32::NEG_INFINITY;
    let mut r_max = 0.0f32;
    for &(x, y) in polygon {
        let (dx, dy) = (x - c.0, y - c.1);
        let t = dx * dir_x + dy * dir_y;
        t_min = t_min.min(t);
        t_max = t_max.max(t);
        r_max = r_max.max((dx * perp_x + dy * perp_y).abs());
    }
    if !t_min.is_finite() || t_max <= t_min {
        return None;
    }
    const SPINE_SHRINK: f32 = 0.8;
    let a = (
        c.0 + dir_x * t_min * SPINE_SHRINK,
        c.1 + dir_y * t_min * SPINE_SHRINK,
    );
    let b = (
        c.0 + dir_x * t_max * SPINE_SHRINK,
        c.1 + dir_y * t_max * SPINE_SHRINK,
    );
    Some((vec![a, b], r_max * 0.5))
}

/// even-odd 射线法点包含测试（自交环按栅格化口径处理，spec §12 裁定）。
pub(crate) fn point_in_polygon(p: (f32, f32), polygon: &[(f32, f32)]) -> bool {
    let (x, y) = p;
    let n = polygon.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        if ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 栅格化掩膜（spec §12 裁定的「按栅格化掩膜处理」）：
///
/// 1. `scratch = grid.clone()`；2. 闭包只改 `scratch`；
/// 3. 只扫多边形 bbox，`多边形内`（扰动查询点，边缘有机化、偏离有界
///    ≤ 2 格）的格子把 `scratch` 值拷回 `grid`——**掩膜外逐位不变**由构造保证。
///
/// 内层返回 0（无操作语义：纯海丘陵 / 湖心在海等）→ 整体返回 0，
/// wv-app 据此发通知。
fn apply_region_masked(
    grid: &mut TerrainGrid,
    polygon: &[(f32, f32)],
    seed: u64,
    cell_size: f32,
    inner: impl FnOnce(&mut TerrainGrid) -> usize,
) -> usize {
    if polygon.len() < 3 || cell_size <= 0.0 {
        return 0;
    }
    let mut scratch = grid.clone();
    if inner(&mut scratch) == 0 {
        return 0;
    }
    let wiggle = N2::new(seed, "mask-wiggle", 8.0, 3);
    let Some((x0, y0, x1, y1)) = pts_bbox(grid, polygon, 3.0 * cell_size, cell_size) else {
        return 0;
    };
    let mut copied = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            // 扰动查询点：边缘格按噪声进出，直边变有机边（偏离有界 ≤ 2 格）。
            let qx = wx + wiggle.at(wx, wy) * 2.0 * cell_size;
            let qy = wy + wiggle.at(wy, wx) * 2.0 * cell_size;
            if !point_in_polygon((qx, qy), polygon) {
                continue;
            }
            match (grid.get(gx, gy), scratch.get(gx, gy)) {
                (Some(a), Some(b)) if a != b => {
                    grid.set(gx, gy, b);
                    copied += 1;
                }
                _ => {}
            }
        }
    }
    copied
}

/// 平原：掩膜内**陆格**压平到 [`PLAINS_TARGET_M`] ± [`PLAINS_NOISE_M`]，
/// 类型写草原；海格不动（纯海选区 = 0 改动 → 上层通知）。
fn apply_plains_region(
    grid: &mut TerrainGrid,
    polygon: &[(f32, f32)],
    seed: u64,
    cell_size: f32,
) -> usize {
    if cell_size <= 0.0 {
        return 0;
    }
    let relief = N2::new(seed, "plains-relief", 40.0, 3);
    let Some((x0, y0, x1, y1)) = pts_bbox(grid, polygon, cell_size, cell_size) else {
        return 0;
    };
    let mut touched = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            if !point_in_polygon((wx, wy), polygon) {
                continue;
            }
            let Some(cell0) = grid.get(gx, gy) else {
                continue;
            };
            if is_water_cell(cell0) {
                continue; // 只在陆上生效（spec §4 裁定）
            }
            let r = relief.at01(wx, wy) * 2.0 - 1.0;
            let new_cell = TerrainCell {
                height: to_height(PLAINS_TARGET_M + PLAINS_NOISE_M * r),
                surface: SURFACE_LAND,
                biome: BIOME_GRASSLAND,
            };
            if new_cell != cell0 {
                grid.set(gx, gy, new_cell);
                touched += 1;
            }
        }
    }
    touched
}

/// 湖泊：区域定湖界——质心须在陆上（沿 [`Intent::Lake`] 口径），掩膜内
/// `h = min(原高, 碗剖面)`：中心 ≈ -[`LAKE_DEPTH_M`]、边缘 ≈ -[`LAKE_LIP_M`]，
/// 类型写浅水；水自然积在碗里（h < 0）。
fn apply_lake_region(grid: &mut TerrainGrid, polygon: &[(f32, f32)], cell_size: f32) -> usize {
    if cell_size <= 0.0 {
        return 0;
    }
    let Some(c) = polygon_centroid(polygon) else {
        return 0;
    };
    // 湖心（质心）必须落在陆地上：点在海里不挖（`apply_lake` 同款裁决）。
    let cgx = (c.0 / cell_size).floor() as i32;
    let cgy = (c.1 / cell_size).floor() as i32;
    match grid.get(cgx, cgy) {
        Some(cell) if !is_water_cell(cell) => {}
        _ => return 0,
    }
    let r_max = polygon
        .iter()
        .map(|&(x, y)| ((x - c.0).powi(2) + (y - c.1).powi(2)).sqrt())
        .fold(0.0f32, f32::max);
    if r_max <= 0.0 {
        return 0;
    }
    let Some((x0, y0, x1, y1)) = pts_bbox(grid, polygon, cell_size, cell_size) else {
        return 0;
    };
    let mut touched = 0;
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let wx = (gx as f32 + 0.5) * cell_size;
            let wy = (gy as f32 + 0.5) * cell_size;
            if !point_in_polygon((wx, wy), polygon) {
                continue;
            }
            let Some(cell0) = grid.get(gx, gy) else {
                continue;
            };
            let d = ((wx - c.0).powi(2) + (wy - c.1).powi(2)).sqrt();
            let tt = (d / r_max).min(1.0);
            let target = -LAKE_DEPTH_M * (1.0 - tt * tt) - LAKE_LIP_M;
            let new_cell = TerrainCell {
                height: to_height((cell0.height as f32).min(target)),
                surface: SURFACE_WATER,
                biome: BIOME_NONE,
            };
            if new_cell != cell0 {
                grid.set(gx, gy, new_cell);
                touched += 1;
            }
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 测试（AGENTS §7.2：核心公式用手算期望值，集成行为用性质断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BIOME_NONE, SURFACE_LAND, SURFACE_WATER};

    /// 平坦陆地网格：高度 `h`、陆面草原。
    fn flat_land(w: u32, h: u32, height: i16) -> TerrainGrid {
        let mut g = TerrainGrid::new(w, h);
        for gy in 0..h as i32 {
            for gx in 0..w as i32 {
                g.set(
                    gx,
                    gy,
                    TerrainCell {
                        height,
                        surface: SURFACE_LAND,
                        biome: BIOME_GRASSLAND,
                    },
                );
            }
        }
        g
    }

    /// 平坦海面网格：深度 `d`、水面。
    fn flat_sea(w: u32, h: u32, depth: i16) -> TerrainGrid {
        let mut g = TerrainGrid::new(w, h);
        for gy in 0..h as i32 {
            for gx in 0..w as i32 {
                g.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: depth,
                        surface: SURFACE_WATER,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        g
    }

    // ---- 纯公式：手算期望值 ----

    /// 山脊剖面：v=1, taper=1, s=1 时 t=0 → 2400、t=0.5 → 2400·e^-0.35 ≈ 1691、
    /// t=1 → 2400·e^-1.4 ≈ 592。
    #[test]
    fn range_add_matches_hand_computed_exp_profile() {
        let v = range_add(0.0, 1.0, 1.0, 1.0);
        assert!((v - 2400.0).abs() < 0.5, "t=0 应为 2400，实得 {v}");
        let v = range_add(0.5, 1.0, 1.0, 1.0);
        // 手算：2400 × e^(-1.4×0.25) = 2400 × e^(-0.35) = 2400 × 0.70469 = 1691.3
        assert!((v - 1691.3).abs() < 1.0, "t=0.5 应 ≈1691，实得 {v}");
        let v = range_add(1.0, 1.0, 1.0, 1.0);
        // 手算：2400 × e^(-1.4) = 2400 × 0.24660 = 591.8
        assert!((v - 591.8).abs() < 1.0, "t=1 应 ≈592，实得 {v}");
        // taper 只做线性缩放。
        let v = range_add(0.0, 0.15, 1.0, 1.0);
        assert!((v - 360.0).abs() < 0.5, "taper=0.15 应为 360，实得 {v}");
    }

    /// 火山锥剖面：t=0.3 → 2400·0.7^1.6 ≈ 1356；环沿 t=0.18 ≈ 1747；
    /// 口底 t=0 ≈ 961（环沿 × 0.55）。
    #[test]
    fn volcano_profile_matches_hand_computed_cone_and_crater() {
        let v = volcano_profile(0.3, 2400.0);
        // 手算：0.7^1.6 = e^(1.6·ln0.7) = e^(-0.57068) = 0.56514 → 2400×0.56514 = 1356.3
        assert!((v - 1356.3).abs() < 1.5, "t=0.3 应 ≈1356，实得 {v}");
        let rim = volcano_profile(VOLCANO_CRATER_T, 2400.0);
        // 手算：0.82^1.6 = e^(1.6×(-0.19845)) = e^(-0.31752) = 0.72797 → 1747.1
        assert!((rim - 1747.1).abs() < 1.5, "环沿应 ≈1747，实得 {rim}");
        let floor = volcano_profile(0.0, 2400.0);
        // 手算：1747.1 × (1-0.45) = 960.9
        assert!((floor - 960.9).abs() < 1.5, "口底应 ≈961，实得 {floor}");
        assert!(floor < rim, "火山口必须低于环沿");
        assert_eq!(volcano_profile(1.0, 2400.0), 0.0, "锥外应为 0");
    }

    // ---- 集成行为：性质断言（固定 seed，确定性） ----

    /// 平地上拖一道山脉：中段高于两端（taper 收尖），远处不动。
    #[test]
    fn mountain_range_is_higher_in_middle_than_at_ends() {
        let mut g = flat_land(40, 40, 0);
        let intent = Intent::MountainRange {
            pts: vec![(8.5, 20.5), (32.5, 20.5)],
            radius: 5.0,
            strength: 1.0,
        };
        let touched = apply_intent(&mut g, &intent, 7, 1.0);
        assert!(touched > 50, "山脉应触及大量格，实得 {touched}");
        let mid = g.get(20, 20).unwrap().height;
        let end = g.get(8, 20).unwrap().height;
        assert!(
            mid > end + 1000,
            "中段 {mid} 应远高于端点 {end}（taper 收尖）"
        );
        assert_eq!(g.get(20, 39).unwrap().height, 0, "远处格不应被动");
        assert_eq!(g.get(20, 20).unwrap().biome, BIOME_MOUNTAINS, "脊上是山地");
    }

    /// 海上拖一道山脉：沿线冒出岛 / 半岛（高度 > 0），远处海不动。
    #[test]
    fn mountain_range_crossing_sea_emerges_islands() {
        let mut g = flat_sea(40, 40, -200);
        let intent = Intent::MountainRange {
            pts: vec![(5.5, 20.5), (35.5, 20.5)],
            radius: 6.0,
            strength: 1.0,
        };
        apply_intent(&mut g, &intent, 11, 1.0);
        let emerged = (5..=35)
            .filter(|&x| g.get(x, 20).map(|c| c.height > 0).unwrap_or(false))
            .count();
        assert!(emerged >= 5, "沿线应至少冒出 5 格陆地，实得 {emerged}");
        assert_eq!(g.get(20, 39).unwrap().height, -200, "远海不应被动");
        // 成岛处必须是陆面，不能是水面挂陆地高度。
        for x in 5..=35 {
            if let Some(c) = g.get(x, 20) {
                if c.height > 0 {
                    assert!(
                        c.surface == SURFACE_LAND,
                        "露出的格 ({x},20) surface={} 应为陆面",
                        c.surface
                    );
                }
            }
        }
    }

    /// 高原：顶面落在 [1200×0.85, 1200] 区间且高于边缘；不压低已有更高地形。
    #[test]
    fn plateau_has_flat_top_within_expected_band() {
        let mut g = flat_land(30, 30, 0);
        let intent = Intent::Plateau {
            pts: vec![(15.5, 15.5)],
            radius: 8.0,
            strength: 1.0,
        };
        apply_intent(&mut g, &intent, 3, 1.0);
        let top = g.get(15, 15).unwrap().height;
        // 顶面 = 1200×(0.85..1.0)，允许取整误差。
        assert!(
            (1015..=1205).contains(&top),
            "顶面应在 [1015,1205]，实得 {top}"
        );
        assert_eq!(g.get(15, 15).unwrap().biome, BIOME_HILLS, "顶面群系是丘陵");
        let shoulder = g.get(15, 23).unwrap().height; // t≈1.0 的边缘
        assert!(shoulder < top, "边缘 {shoulder} 应低于顶面 {top}");
        assert_eq!(g.get(2, 2).unwrap().height, 0, "远处不动");
    }

    /// 丘陵：内陆起伏 ≤ 300×s 且确实有起伏；类型成丘陵。
    #[test]
    fn hills_add_rolling_bumps_within_amplitude() {
        let mut g = flat_land(30, 30, 0);
        let intent = Intent::Hills {
            pts: vec![(15.5, 15.5)],
            radius: 8.0,
            strength: 1.0,
        };
        apply_intent(&mut g, &intent, 5, 1.0);
        let mut heights = Vec::new();
        for gy in 11..=20 {
            for gx in 11..=20 {
                heights.push(g.get(gx, gy).unwrap().height);
            }
        }
        assert!(
            heights.iter().all(|&h| (0..=302).contains(&h)),
            "起伏应 ≤ 302，实得 max={}",
            heights.iter().max().unwrap()
        );
        let spread = heights.iter().max().unwrap() - heights.iter().min().unwrap();
        assert!(spread > 30, "丘陵应有起伏（max-min={spread} > 30）");
        assert_eq!(g.get(15, 15).unwrap().biome, BIOME_HILLS, "中心群系是丘陵");
    }

    /// 海上放火山：成岛（中心 > 0）、有火山口（中心低于环沿）、远处海不动。
    #[test]
    fn volcano_over_sea_emerges_island_with_crater() {
        let mut g = flat_sea(50, 50, -200);
        let intent = Intent::Volcano {
            center: (25.5, 25.5),
            radius: 10.0,
            strength: 1.0,
        };
        apply_intent(&mut g, &intent, 9, 1.0);
        let center = g.get(25, 25).unwrap();
        assert!(center.height > 0, "中心应成岛，实得 {}", center.height);
        assert_eq!(center.biome, BIOME_MOUNTAINS, "锥体群系是山地");
        // 环沿附近应存在比中心（口底 ≈ 961）高得多的格（环沿 ≈ 1747）。
        let mut rim_max = 0;
        for gy in 22..=28 {
            for gx in 22..=28 {
                if (gx, gy) != (25, 25) {
                    rim_max = rim_max.max(g.get(gx, gy).unwrap().height);
                }
            }
        }
        assert!(
            rim_max > center.height + 200,
            "火山口 {} 应显著低于环沿 {rim_max}",
            center.height
        );
        assert_eq!(g.get(49, 49).unwrap().height, -200, "远海不动");
    }

    /// 湖：碗底比碗沿（= 周边平地 100）低 [6+50×0.91, 6+50] 米；
    /// 类型变浅水；湖心在海上时无操作。
    #[test]
    fn lake_carves_bowl_below_rim_minus_lip() {
        let mut g = flat_land(40, 40, 100);
        let intent = Intent::Lake {
            center: (20.5, 20.5),
            radius: 8.0,
            strength: 1.0,
        };
        let touched = apply_intent(&mut g, &intent, 13, 1.0);
        assert!(touched > 20, "碗应触及大量格，实得 {touched}");
        let bottom = g.get(20, 20).unwrap();
        // target = 100 - 6 - 50×(1-t²)，扭曲使 t ∈ [0, 0.3] → target ∈ [44, 48.5]。
        assert!(
            (42..=50).contains(&bottom.height),
            "碗底应在 [42,50]，实得 {}",
            bottom.height
        );
        assert_eq!(bottom.surface, SURFACE_WATER, "碗内是水面");
        assert_eq!(g.get(2, 2).unwrap().height, 100, "碗外不动");

        // 湖心在海上：无操作。
        let mut sea = flat_sea(40, 40, -50);
        let n = apply_intent(
            &mut sea,
            &Intent::Lake {
                center: (20.5, 20.5),
                radius: 8.0,
                strength: 1.0,
            },
            13,
            1.0,
        );
        assert_eq!(n, 0, "海上点湖应无操作");
    }

    /// 河：链上高度非增（向下游）、首格比两岸低 12×s、不低于 1 m、类型浅水。
    #[test]
    fn river_chain_descends_and_cuts_below_banks() {
        let mut g = flat_land(40, 40, 100);
        let intent = Intent::River {
            pts: vec![(5.5, 5.5), (20.5, 20.5)],
            strength: 1.0,
        };
        let touched = apply_intent(&mut g, &intent, 1, 1.0);
        assert!(touched >= 14, "对角链应触及 ≥14 格，实得 {touched}");
        // 收集链上格（地表被改成水面的格）。
        let mut chain = Vec::new();
        for gy in 4..=21 {
            for gx in 4..=21 {
                if g.get(gx, gy).unwrap().surface == SURFACE_WATER {
                    chain.push((gx, gy, g.get(gx, gy).unwrap().height));
                }
            }
        }
        assert!(chain.len() >= 14, "链长 {} 应 ≥14", chain.len());
        // 非增（源→下游按 a 排序即对角序）。
        for w in chain.windows(2) {
            assert!(
                w[1].2 <= w[0].2,
                "高度应沿链非增：{:?} → {:?}",
                (w[0].0, w[0].1, w[0].2),
                (w[1].0, w[1].1, w[1].2)
            );
        }
        // 每格比两岸最低格低 ≥ 11（12 - 取整容差）。
        for &(cx, cy, h) in &chain {
            let mut low = i16::MAX;
            for (nx, ny) in [(cx + 1, cy), (cx - 1, cy), (cx, cy + 1), (cx, cy - 1)] {
                if let Some(c) = g.get(nx, ny) {
                    if c.surface != SURFACE_WATER {
                        low = low.min(c.height);
                    }
                }
            }
            if low < i16::MAX {
                assert!(
                    h <= low - 11,
                    "河床 ({cx},{cy}) h={h} 应 ≤ 两岸最低 {low} - 11"
                );
            }
        }
        assert!(chain.iter().all(|&(_, _, h)| h >= 1), "河床不低于 1 m");
        assert_eq!(g.get(5, 6).unwrap().height, 100, "链外格不动");
    }

    /// 确定性：同 seed 重放逐格一致；异 seed 结果不同。
    #[test]
    fn same_seed_replays_identically_and_different_seed_differs() {
        let intent = Intent::MountainRange {
            pts: vec![(10.0, 10.0), (30.0, 25.0)],
            radius: 6.0,
            strength: 1.0,
        };
        let mut a = flat_land(40, 40, 0);
        let mut b = flat_land(40, 40, 0);
        apply_intent(&mut a, &intent, 42, 1.0);
        apply_intent(&mut b, &intent, 42, 1.0);
        assert_eq!(a, b, "同 seed 同 intent 应逐格一致");
        let mut c = flat_land(40, 40, 0);
        apply_intent(&mut c, &intent, 43, 1.0);
        assert_ne!(a, c, "异 seed 应产生不同地形");
    }

    /// 越界 / 退化输入安全：空折线、完全越界、海上丘陵、单点河 → 0，不 panic。
    #[test]
    fn degenerate_and_out_of_bounds_intents_are_safe() {
        let mut g = flat_land(10, 10, 0);
        assert_eq!(
            apply_intent(
                &mut g,
                &Intent::MountainRange {
                    pts: vec![],
                    radius: 5.0,
                    strength: 1.0
                },
                1,
                1.0
            ),
            0,
            "空折线应无操作"
        );
        assert_eq!(
            apply_intent(
                &mut g,
                &Intent::MountainRange {
                    pts: vec![(-50.0, -50.0), (-45.0, -45.0)],
                    radius: 5.0,
                    strength: 1.0
                },
                1,
                1.0
            ),
            0,
            "完全越界应无操作"
        );
        let mut sea = flat_sea(10, 10, -50);
        assert_eq!(
            apply_intent(
                &mut sea,
                &Intent::Hills {
                    pts: vec![(5.0, 5.0)],
                    radius: 4.0,
                    strength: 1.0
                },
                1,
                1.0
            ),
            0,
            "海上丘陵应无操作"
        );
        let mut g2 = flat_land(10, 10, 0);
        assert_eq!(
            apply_intent(
                &mut g2,
                &Intent::River {
                    pts: vec![(5.0, 5.0)],
                    strength: 1.0
                },
                1,
                1.0
            ),
            0,
            "单点河应无操作"
        );
    }

    /// 单点山脉 = 一座圆山（点一下的用法）。
    #[test]
    fn single_point_range_makes_a_round_mountain() {
        let mut g = flat_land(30, 30, 0);
        let intent = Intent::MountainRange {
            pts: vec![(15.5, 15.5)],
            radius: 5.0,
            strength: 1.0,
        };
        let touched = apply_intent(&mut g, &intent, 21, 1.0);
        assert!(touched > 10, "圆山应触及格，实得 {touched}");
        assert!(g.get(15, 15).unwrap().height > 1000, "山顶应 >1000 m");
    }

    /// seed 派生：同意图同图同 seed；图不同或意图不同则 seed 不同。
    #[test]
    fn derive_seed_is_stable_and_discriminating() {
        let a = Intent::Volcano {
            center: (10.0, 10.0),
            radius: 5.0,
            strength: 1.0,
        };
        assert_eq!(a.derive_seed("map_x"), a.derive_seed("map_x"));
        assert_ne!(a.derive_seed("map_x"), a.derive_seed("map_y"));
        let b = Intent::Volcano {
            center: (10.0, 10.0),
            radius: 5.0,
            strength: 1.1,
        };
        assert_ne!(a.derive_seed("map_x"), b.derive_seed("map_x"));
    }

    // ---- 区域意图（M3，spec §4 / §13）----

    /// 2:1 长方形 → 脊线沿长轴（x 向），端点向质心收缩 20%；半径 = 短轴之半。
    #[test]
    fn massif_ridge_runs_along_long_axis() {
        let rect = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)];
        let (spine, radius) = region_spine(&rect).expect("正常多边形应有脊线");
        assert_eq!(spine.len(), 2, "脊线是两段点");
        let a = spine[0];
        let b = spine[1];
        assert!(
            (a.1 - 5.0).abs() < 1e-4 && (b.1 - 5.0).abs() < 1e-4,
            "y 应恒等于 5：{spine:?}"
        );
        assert!(
            (a.0 - 2.0).abs() < 1e-4,
            "t_min=-10 收缩 20% → x=2：got {a:?}"
        );
        assert!(
            (b.0 - 18.0).abs() < 1e-4,
            "t_max=+10 收缩 20% → x=18：got {b:?}"
        );
        assert!(
            (radius - 2.5).abs() < 1e-4,
            "短轴 10 之半 → 半径 2.5：got {radius}"
        );
    }

    /// 高山群：掩膜内显著隆起（有的格 +500 m 以上、出山地类型），
    /// 掩膜外**逐位不变**（spec §13 断言口径）。
    #[test]
    fn massif_raises_inside_and_keeps_outside_bitwise() {
        let before = flat_land(64, 64, 100);
        let polygon = vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)];
        let mut g = before.clone();
        let touched = apply_intent(
            &mut g,
            &Intent::MountainMassif {
                polygon: polygon.clone(),
            },
            42,
            1.0,
        );
        assert!(touched > 20, "掩膜内应大面积隆起：touched={touched}");
        let mut raised = 0;
        for gy in 16..48 {
            for gx in 16..48 {
                let c = g.get(gx, gy).expect("在界内");
                if c.height >= 600 {
                    raised += 1;
                }
            }
        }
        assert!(
            raised > 0,
            "掩膜内应有 +500 m 以上的山体格（100 m 底 + 500 m 增量）"
        );
        // 掩膜外（bbox 外三圈以外）逐位不变。
        for gy in 0..64i32 {
            for gx in 0..64i32 {
                if (12..52).contains(&gx) && (12..52).contains(&gy) {
                    continue;
                }
                assert_eq!(
                    g.get(gx, gy),
                    before.get(gx, gy),
                    "掩膜外 ({gx},{gy}) 逐位不变"
                );
            }
        }
    }

    /// 丘陵继承「只在陆上长」语义：纯海选区 = 0 改动；陆上选区有改动。
    #[test]
    fn hills_region_only_changes_land_cells() {
        let polygon = vec![(20.0, 20.0), (44.0, 20.0), (44.0, 44.0), (20.0, 44.0)];
        let mut sea = flat_sea(64, 64, -80);
        let touched_sea = apply_intent(
            &mut sea,
            &Intent::HillsRegion {
                polygon: polygon.clone(),
            },
            7,
            1.0,
        );
        assert_eq!(touched_sea, 0, "纯海选区应无操作");
        let mut land = flat_land(64, 64, 40);
        let touched_land = apply_intent(&mut land, &Intent::HillsRegion { polygon }, 7, 1.0);
        assert!(
            touched_land > 10,
            "陆上选区应有丘陵起伏：touched={touched_land}"
        );
    }

    /// 平原：掩膜内 200 m 高地压平到 60±15 m 且出草原；海格与掩膜外不动。
    #[test]
    fn plains_region_flattens_land_to_target_band() {
        let before = flat_land(64, 64, 200);
        let polygon = vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)];
        let mut g = before.clone();
        let touched = apply_intent(
            &mut g,
            &Intent::PlainsRegion {
                polygon: polygon.clone(),
            },
            9,
            1.0,
        );
        assert!(touched > 100, "掩膜内应大面积压平：touched={touched}");
        for gy in 16..48 {
            for gx in 16..48 {
                let c = g.get(gx, gy).expect("在界内");
                let on_edge = !(20..=43).contains(&gx) || !(20..=43).contains(&gy);
                if on_edge {
                    // 边缘有机化（掩膜扰动）：贴边格允许保留原状或已压平。
                    assert!(
                        (45.0..=75.0).contains(&(c.height as f32)) || c.height == 200,
                        "({gx},{gy}) 边缘格应为压平值或原值：got {}",
                        c.height
                    );
                    continue;
                }
                assert!(
                    (45.0..=75.0).contains(&(c.height as f32)),
                    "({gx},{gy}) 应在 60±15 m：got {}",
                    c.height
                );
                assert_eq!(c.biome, BIOME_GRASSLAND, "平原出草原");
            }
        }
        for gy in 0..64i32 {
            for gx in 0..64i32 {
                if (14..50).contains(&gx) && (14..50).contains(&gy) {
                    continue;
                }
                assert_eq!(
                    g.get(gx, gy),
                    before.get(gx, gy),
                    "掩膜外 ({gx},{gy}) 逐位不变"
                );
            }
        }
    }

    /// 湖泊：掩膜内挖成水面（中心最深、h<0 出浅水），质心在海 = 0 改动；
    /// 掩膜外逐位不变。
    #[test]
    fn lake_region_digs_water_bowl_below_sea() {
        let before = flat_land(64, 64, 100);
        let polygon = vec![(20.0, 20.0), (44.0, 20.0), (44.0, 44.0), (20.0, 44.0)];
        let mut g = before.clone();
        let touched = apply_intent(
            &mut g,
            &Intent::LakeRegion {
                polygon: polygon.clone(),
            },
            11,
            1.0,
        );
        assert!(touched > 100, "掩膜内应成湖：touched={touched}");
        let center = g.get(32, 32).expect("湖心在界内");
        assert!(center.height < 0, "湖心应在水面以下：got {}", center.height);
        assert_eq!(center.surface, SURFACE_WATER, "湖面出水面");
        let rim = g.get(21, 32).expect("近边缘格在界内");
        assert!(rim.height < 0, "近边缘仍是水面：got {}", rim.height);
        assert!(
            f32::from(rim.height) > f32::from(center.height),
            "边缘应浅于中心：rim={} center={}",
            rim.height,
            center.height
        );
        // 质心在海 → 0 改动。
        let mut sea = flat_sea(64, 64, -80);
        let touched_sea = apply_intent(&mut sea, &Intent::LakeRegion { polygon }, 11, 1.0);
        assert_eq!(touched_sea, 0, "湖心在海里应无操作");
        for gy in 0..64i32 {
            for gx in 0..64i32 {
                if (18..46).contains(&gx) && (18..46).contains(&gy) {
                    continue;
                }
                assert_eq!(
                    g.get(gx, gy),
                    before.get(gx, gy),
                    "掩膜外 ({gx},{gy}) 逐位不变"
                );
            }
        }
    }

    /// 退化环（空 / 两点）全部安全返回 0，不 panic（Review Focus 2）。
    #[test]
    fn degenerate_region_returns_zero_changes() {
        let mut g = flat_land(32, 32, 50);
        for polygon in [Vec::new(), vec![(1.0, 1.0), (2.0, 2.0)]] {
            for intent in [
                Intent::MountainMassif {
                    polygon: polygon.clone(),
                },
                Intent::HillsRegion {
                    polygon: polygon.clone(),
                },
                Intent::PlainsRegion {
                    polygon: polygon.clone(),
                },
                Intent::LakeRegion {
                    polygon: polygon.clone(),
                },
            ] {
                assert_eq!(
                    apply_intent(&mut g, &intent, 3, 1.0),
                    0,
                    "退化环应无操作：{intent:?}"
                );
            }
        }
    }

    /// 山上挖湖：碗底海拔为正，surface 仍为 WATER（设计稿 D14——水不再必然
    /// 低于海平面，authoring 真值由 surface 通道承载）。
    #[test]
    fn lake_surface_water_survives_positive_height() {
        let mut grid = TerrainGrid::new(64, 64);
        // 先铺一块 200 m 高的陆地（surface LAND）。
        for gy in 0..64 {
            for gx in 0..64 {
                grid.set(
                    gx,
                    gy,
                    TerrainCell {
                        height: 200,
                        surface: SURFACE_LAND,
                        biome: BIOME_NONE,
                    },
                );
            }
        }
        apply_intent(
            &mut grid,
            &Intent::Lake {
                center: (32.0, 32.0),
                radius: 8.0,
                strength: 1.0,
            },
            7,
            1.0,
        );
        let c = grid.get(32, 32).unwrap();
        assert_eq!(c.surface, SURFACE_WATER, "got surface {}", c.surface);
        assert!(c.height > 0, "got {}", c.height);
    }
}
