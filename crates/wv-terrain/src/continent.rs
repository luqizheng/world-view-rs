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
    fn self_intersecting_bowtie_polygon_is_handled_without_panic() {
        let bowtie = vec![(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
        let _ = signed_distance((5.0, 5.0), &bowtie); // 只要不 panic 即可
        let _ = point_in_polygon((2.0, 1.0), &bowtie);
    }
}
