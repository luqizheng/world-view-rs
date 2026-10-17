//! 大陆配方内核：意图包络 + 八参数 + 种子 → 双通道格子写入。
//!
//! 裁决出处：`docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §8
//! 「2026-10-17 · 大陆生成工作流」+ README §12-40。坐标为**图内世界单位**
//! （与 [`crate::edit::Brush`] 同口径），不感知 Bevy / IO / Undo。
//!
//! ## 分层职责
//!
//! - 纯函数 + 可变 [`crate::TerrainGrid`]；撤销由 `wv-edit` 的 before 缓存负责。
//! - 零第三方依赖——坐标一律 `(f32, f32)`，不用 glam。
//! - 确定性（D6）：同 `(envelopes, params, seed)` → 逐位一致的网格。

use crate::generate::Perlin;
use crate::intent::{KIND_GRASS, KIND_HILL, KIND_MOUNTAIN, KIND_WATER_SHALLOW};
use crate::{TerrainCell, TerrainKind};

// ---------------------------------------------------------------------------
// 八参数（大陆配方属性面板 v1）
// ---------------------------------------------------------------------------

/// 起伏强度档：面板「起伏强度」低 / 中 / 高 → 中心最高点量级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliefLevel {
    /// 丘陵——中心约 600 m。
    Hills,
    /// 山地——中心约 2000 m（裁决缺省）。
    Mountains,
    /// 高山——中心约 4000 m。
    HighMountains,
}

impl ReliefLevel {
    /// 中心最高点高度（米）。值即面板档位的量级承诺，实机校准只调这条。
    pub fn meters(self) -> i16 {
        match self {
            Self::Hills => 600,
            Self::Mountains => 2000,
            Self::HighMountains => 4000,
        }
    }
}

/// 离岸小岛密度：海岸外侧低频噪声超过阈值才撒岛。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandDensity {
    /// 不撒岛（裁决缺省）。
    Off,
    /// 稀疏。
    Low,
    /// 适中。
    Medium,
    /// 密集。
    High,
}

impl IslandDensity {
    /// 撒岛噪声阈值。`fbm01 > threshold` 才成岛——阈值越高岛越少。
    pub fn threshold(self) -> f64 {
        match self {
            Self::Off => 0.0,
            Self::Low => 0.60,
            Self::Medium => 0.70,
            Self::High => 0.78,
        }
    }
}

/// 大陆配方八参数（面板 v1 参数表；裁决缺省见 [`Default`]）。
///
/// 参数归属是**全局单一面板**（用户裁定「1 同意」）：每个意图包络不单独带
/// 参数，同一批 envelope 共用一个 `ContinentParams`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContinentParams {
    /// 海岸蜿蜒度 0–100%（fBm 对距离场的扰动幅度）。0 = 贴多边形边。
    pub coast_wiggle: f32,
    /// 海岸细节 = fBm octaves：低 2 / 中 4 / 高 6。
    pub coast_octaves: u32,
    /// 起伏强度档。
    pub relief: ReliefLevel,
    /// 森林覆盖 0–100%：内部森林子配方撒布比例。
    pub forest_cover: f32,
    /// 荒漠覆盖 0–100%：内部沙漠子配方撒布比例。
    pub desert_cover: f32,
    /// 大陆架宽度（格）：岸外浅水带厚度。
    pub shelf_cells: u32,
    /// 离岸小岛档。
    pub islands: IslandDensity,
}

impl Default for ContinentParams {
    /// 裁决缺省（plan v0.1 §8 参数表）。
    fn default() -> Self {
        Self {
            coast_wiggle: 0.40,
            coast_octaves: 4,
            relief: ReliefLevel::Mountains,
            forest_cover: 0.30,
            desert_cover: 0.15,
            shelf_cells: 8,
            islands: IslandDensity::Off,
        }
    }
}

// ---------------------------------------------------------------------------
// 多边形几何原语
// ---------------------------------------------------------------------------

/// 点到线段的最近距离（投影参数钳到 `[0, 1]`）。
fn point_segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    let (qx, qy) = if len_sq <= f32::EPSILON {
        // 退化边（重合点）：最近点即端点本身。
        (ax, ay)
    } else {
        let t = (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0);
        (ax + t * dx, ay + t * dy)
    };
    let ex = px - qx;
    let ey = py - qy;
    (ex * ex + ey * ey).sqrt()
}

/// 射线法内点判定；**边界算在内**。
///
/// 自交叉多边形（作者随手圈出的蝴蝶结形）按奇偶规则给出稳定结果，
/// 不 panic——形状是否「合理」不是内核的职责（V1 只提示不阻止）。
pub(crate) fn point_in_polygon(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    // 边界先判：距离为 0 的点直接算在内，避免射线恰好擦边时的奇偶抖动。
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        if point_segment_distance(p, a, b) <= 1e-6 {
            return true;
        }
    }
    let (px, py) = p;
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        // 只统计跨越 py 且交点在 p 右侧的边（半开区间避免顶点重复计数）。
        if (yi > py) != (yj > py) {
            let x_at = xi + (py - yi) * (xj - xi) / (yj - yi);
            if x_at > px {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// 有符号距离：内正外负，绝对值 = 到最近边的距离。
///
/// 距离场是海岸线的唯一几何底座——fBm 只在其上做扰动，
/// 因此这里必须是**精确**的点到边距离，不能用 bbox 近似。
pub(crate) fn signed_distance(p: (f32, f32), poly: &[(f32, f32)]) -> f32 {
    if poly.len() < 3 {
        return 0.0;
    }
    let mut nearest = f32::INFINITY;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let d = point_segment_distance(p, a, b);
        if d < nearest {
            nearest = d;
        }
    }
    if point_in_polygon(p, poly) {
        nearest
    } else {
        -nearest
    }
}

/// 全部意图包络的联合 bbox（min 角点, max 角点）；无有效输入 → `None`。
///
/// 遍历范围以此为基础外扩；空输入返回 `None` 让调用方零改动早退（V1 精神）。
pub(crate) fn polygon_bbox(polys: &[Vec<(f32, f32)>]) -> Option<((f32, f32), (f32, f32))> {
    let mut min = (f32::INFINITY, f32::INFINITY);
    let mut max = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    for poly in polys {
        for &(x, y) in poly {
            any = true;
            if x < min.0 {
                min.0 = x;
            }
            if y < min.1 {
                min.1 = y;
            }
            if x > max.0 {
                max.0 = x;
            }
            if y > max.1 {
                max.1 = y;
            }
        }
    }
    any.then_some((min, max))
}

// ---------------------------------------------------------------------------
// 海岸距离场
// ---------------------------------------------------------------------------

/// 海岸有符号距离场：多包络取「最近的包络」（各包络 `signed_distance` 的 max），
/// 再叠 fBm 扰动。
///
/// `amp = coast_wiggle × 0.10 × diag`：蜿蜒度 0 时扰动为零，海岸严格贴
/// 多边形边（Task 5 的贴边性质测试依赖这一点）。fBm 采样坐标按 `diag × 0.25`
/// 归一，使不同规模地图的「海岬尺度」观感一致。
pub(crate) fn coast_field(
    p: (f32, f32),
    polys: &[Vec<(f32, f32)>],
    params: &ContinentParams,
    noise: &Perlin,
    diag: f32,
) -> f32 {
    let mut d = f32::NEG_INFINITY;
    for poly in polys {
        if poly.len() < 3 {
            continue;
        }
        let sd = signed_distance(p, poly);
        if sd > d {
            d = sd;
        }
    }
    if !d.is_finite() {
        return 0.0;
    }
    let amp = params.coast_wiggle * 0.10 * diag;
    if amp == 0.0 {
        return d;
    }
    let scale = (diag * 0.25).max(f32::EPSILON);
    let n = noise.fbm(
        (p.0 / scale) as f64,
        (p.1 / scale) as f64,
        params.coast_octaves,
        2.0,
        0.5,
    );
    let fbm01 = ((n * 0.5 + 1.0) as f32).clamp(0.0, 1.0);
    d + amp * fbm01
}

// ---------------------------------------------------------------------------
// 高度剖面 / 分带 / 融合
// ---------------------------------------------------------------------------

/// 深水（kind 0）与雪原（kind 7）/ 荒漠沙地（kind 2）/ 森林（kind 4）编号：
/// `intent.rs` 只锚定了水/草/丘/山四类，其余沿用默认主题
/// `wv-render/theme/colors.rs` 的 `CORE_TERRAIN` 索引（同上文注释口径）。
pub(crate) const KIND_WATER_DEEP: TerrainKind = 0;
pub(crate) const KIND_SAND: TerrainKind = 2;
pub(crate) const KIND_FOREST: TerrainKind = 4;
pub(crate) const KIND_SNOW: TerrainKind = 7;

/// 雪线（米）。取山地档缺省峰高（2000 m）之上、高山档峰高（4000 m）之下：
/// 2000 m 峰顶不盖雪、4000 m 峰顶盖雪，与面板「起伏强度」三档观感自洽。
const SNOW_LINE_M: i16 = 2600;
/// 山地分带下沿（米）——对齐 `intent::PLATEAU_TOP_M`（高原顶面 1200 m）。
const MOUNTAIN_MIN_M: i16 = 1200;
/// 丘陵分带下沿（米）——对齐 `intent::HILLS_AMP_M`（丘陵起伏 300 m）。
const HILL_MIN_M: i16 = 300;
/// 浅水与深水的高度分界（米）；仅用于「类型随高度走」的兜底判定，
/// 真实浅水由大陆架距离带给出（Q5 优先序第一位）。
const SHALLOW_WATER_MAX_DEPTH_M: i16 = -200;

/// 径向高度剖面：海岸 0 → 中心 `relief.meters()`，内部叠 fBm 起伏。
///
/// `t = d / (0.6 × inradius)`：距海岸 60% 内半径处到达满高，再往里有平台期，
/// 避免圆锥顶点感；`inradius` 由调用方按包络面积开方估算（Task 5）。
/// 调制系数 `0.7 + 0.6·fbm01` ∈ [0.7, 1.3]：中心高度不会被噪声甩出档位承诺。
pub(crate) fn land_height(
    d: f32,
    inradius: f32,
    params: &ContinentParams,
    noise: &Perlin,
    p: (f32, f32),
) -> i16 {
    let peak = params.relief.meters() as f32;
    let denom = (0.6 * inradius).max(f32::EPSILON);
    let t = (d / denom).clamp(0.0, 1.0);
    let base = peak * t;
    let scale = (inradius * 0.25).max(1.0);
    let n = noise.fbm(
        (p.0 / scale) as f64,
        (p.1 / scale) as f64,
        4,
        2.0,
        0.5,
    );
    let fbm01 = ((n * 0.5 + 1.0) as f32).clamp(0.0, 1.0);
    let h = base * (0.7 + 0.6 * fbm01);
    h.round().clamp(0.0, i16::MAX as f32) as i16
}

/// 按高度给出分带类型（无森林/荒漠掩码）——供 [`blended`] 的海岸带使用。
fn kind_for_height(height_m: i16) -> TerrainKind {
    if height_m >= SNOW_LINE_M {
        KIND_SNOW
    } else if height_m >= MOUNTAIN_MIN_M {
        KIND_MOUNTAIN
    } else if height_m >= HILL_MIN_M {
        KIND_HILL
    } else if height_m >= 0 {
        KIND_GRASS
    } else if height_m > SHALLOW_WATER_MAX_DEPTH_M {
        KIND_WATER_SHALLOW
    } else {
        KIND_WATER_DEEP
    }
}

/// fBm 归一化到 0..1。
fn fbm01(noise: &Perlin, x: f64, y: f64, octaves: u32) -> f32 {
    ((noise.fbm(x, y, octaves, 2.0, 0.5) * 0.5 + 1.0) as f32).clamp(0.0, 1.0)
}

/// Q5 优先序分带：大陆架浅水 → 雪原 → 山地 → 丘陵 → 森林 → 荒漠 → 平原；
/// 水下且不在大陆架带内 → 深水。
///
/// `shelf_wu` = 大陆架宽度（世界单位，= `shelf_cells × cell_size`）；
/// 由调用方换算后传入，保持本函数与格尺寸解耦。
pub(crate) fn band_kind(
    height_m: i16,
    d: f32,
    params: &ContinentParams,
    noise: &Perlin,
    p: (f32, f32),
    diag: f32,
    shelf_wu: f32,
) -> TerrainKind {
    if (-shelf_wu..0.0).contains(&d) {
        return KIND_WATER_SHALLOW;
    }
    if height_m >= SNOW_LINE_M {
        return KIND_SNOW;
    }
    if height_m >= MOUNTAIN_MIN_M {
        return KIND_MOUNTAIN;
    }
    if height_m >= HILL_MIN_M {
        return KIND_HILL;
    }
    if height_m < 0 {
        return KIND_WATER_DEEP;
    }
    let scale = (diag * 0.5).max(1.0);
    let forest = fbm01(noise, (p.0 / scale) as f64, (p.1 / scale) as f64, 3);
    if forest < params.forest_cover {
        return KIND_FOREST;
    }
    // 荒漠用同族噪声的错位采样，避免与森林掩码完全相关。
    let desert = fbm01(
        noise,
        ((p.0 + diag * 0.37) / scale) as f64,
        ((p.1 + diag * 0.71) / scale) as f64,
        3,
    );
    if desert < params.desert_cover {
        return KIND_SAND;
    }
    KIND_GRASS
}

/// 与既有地形融合：海岸带内按 `falloff_weight` 过渡，内部只增不减。
///
/// - `d ∈ [0, 6 格)`：`h = existing + (new − existing) × w`，`w` 从海岸的 1
///   衰减到 6 格处的 0——海岸线交给新配方，内陆交给既有作者地形。
/// - `d ≥ 6 格`：高度取 `max`，**既有陆地不被压低**（海洋世界打底，
///   但图上可能已有作者先画的岛）。
pub(crate) fn blended(
    existing: TerrainCell,
    new_height: i16,
    new_kind: TerrainKind,
    d: f32,
    cell_size: f32,
) -> TerrainCell {
    let band = (6.0 * cell_size).max(f32::EPSILON);
    if d < band {
        let w = if d <= 0.0 {
            1.0
        } else {
            crate::edit::falloff_weight(d, band, 0.5)
        };
        let h = (existing.height as f32 + (new_height as f32 - existing.height as f32) * w)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        let kind = if h == new_height {
            new_kind
        } else {
            kind_for_height(h)
        };
        TerrainCell { height: h, kind }
    } else {
        let height = existing.height.max(new_height);
        TerrainCell {
            height,
            kind: new_kind,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_polygon_point_in_polygon_and_signed_distance() {
        let sq = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        assert!(point_in_polygon((5.0, 5.0), &sq), "中心在内");
        assert!(!point_in_polygon((15.0, 5.0), &sq), "外部");
        let d = signed_distance((5.0, 5.0), &sq);
        assert!((d - 5.0).abs() < 1e-3, "中心到边 5.0，got {d}");
        let d_out = signed_distance((13.0, 5.0), &sq);
        assert!((d_out + 3.0).abs() < 1e-3, "外 3 单位 = -3.0，got {d_out}");
    }

    #[test]
    fn default_params_match_ruling_values() {
        let p = ContinentParams::default();
        assert!((p.coast_wiggle - 0.40).abs() < 1e-6, "coast_wiggle 得 {}", p.coast_wiggle);
        assert_eq!(p.coast_octaves, 4, "coast_octaves 得 {}", p.coast_octaves);
        assert_eq!(p.relief, ReliefLevel::Mountains, "relief 得 {:?}", p.relief);
        assert!((p.forest_cover - 0.30).abs() < 1e-6, "forest_cover 得 {}", p.forest_cover);
        assert!((p.desert_cover - 0.15).abs() < 1e-6, "desert_cover 得 {}", p.desert_cover);
        assert_eq!(p.shelf_cells, 8, "shelf_cells 得 {}", p.shelf_cells);
        assert_eq!(p.islands, IslandDensity::Off, "islands 得 {:?}", p.islands);
    }

    #[test]
    fn zero_wiggle_coast_field_equals_plain_signed_distance() {
        let sq = vec![vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]];
        // 计划原文这里写 `default()` 却称 wiggle=0；缺省是 0.40，实际属性是
        // 「wiggle=0 时扰动为零」，故显式置 0（Ruling 见 ledger）。
        let mut p = ContinentParams::default();
        p.coast_wiggle = 0.0;
        let n = Perlin::new(42);
        let f = coast_field((50.0, 50.0), &sq, &p, &n, 141.42);
        assert!((f - 50.0).abs() < 1e-3, "wiggle=0 应等于裸距离 50，got {f}");
    }

    #[test]
    fn higher_cells_never_have_lower_priority_kind() {
        let mut p = ContinentParams::default();
        p.forest_cover = 0.0;
        p.desert_cover = 0.0;
        let n = Perlin::new(7);
        let rank = |k: TerrainKind| -> u8 {
            match k {
                KIND_WATER_DEEP => 0,
                KIND_WATER_SHALLOW => 1,
                KIND_SAND => 2,
                KIND_GRASS => 3,
                KIND_FOREST => 4,
                KIND_HILL => 5,
                KIND_MOUNTAIN => 6,
                KIND_SNOW => 7,
                _ => 255,
            }
        };
        let mut prev = 0u8;
        for h in [0i16, 300, 800, 2200, 4500] {
            let k = band_kind(h, 500.0, &p, &n, (10.0, 10.0), 200.0, 64.0);
            let r = rank(k);
            assert!(
                r >= prev,
                "高度 {h} m 的类型 {k} 优先序 {r} 低于前档 {prev}"
            );
            prev = r;
        }
    }

    #[test]
    fn blended_interior_never_lowers_existing_land() {
        let existing = TerrainCell {
            height: 800,
            kind: KIND_HILL,
        };
        let cell_size = 8.0;
        let out = blended(existing, 50, KIND_GRASS, 100.0, cell_size);
        assert!(
            out.height >= 800,
            "内部（d=100 ≥ 6 格=48）既有陆地 800 不得被压低，got {}",
            out.height
        );
    }

    #[test]
    fn shelf_band_is_shallow_water() {
        let p = ContinentParams::default();
        let n = Perlin::new(3);
        let cell_size = 8.0;
        let shelf_wu = p.shelf_cells as f32 * cell_size;
        let k = band_kind(-40, -2.0 * cell_size, &p, &n, (10.0, 10.0), 200.0, shelf_wu);
        assert_eq!(k, crate::intent::KIND_WATER_SHALLOW, "大陆架带应为浅水");
    }

    #[test]
    fn self_intersecting_bowtie_polygon_is_handled_without_panic() {
        let bowtie = vec![(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
        let _ = signed_distance((5.0, 5.0), &bowtie); // 只要不 panic 即可
        let _ = point_in_polygon((2.0, 1.0), &bowtie);
    }
}
