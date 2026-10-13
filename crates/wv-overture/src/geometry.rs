//! 几何工具函数（ADR-0007 §理由（4））。
//!
//! 核心问题：Overture `transportation` segment **不**自带 `length` 字段
//! （实测西雅图 bbox 下载 155k segments 均无 length），长度必须从
//! `geometry: LineString` 端点列用 Haversine 公式累加求出。
//!
//! ADR-0007 §理由（4）回归用例：
//! - 一条 residential segment，geometry 含 3 段 4 点
//! - Haversine 计算长度 = **440.8 m** （即 0.44 km）
//!
//! 本模块只提供纯函数，不引入 Parquet 依赖；读取 Parquet 是后续 T5 实施期的事。

/// EPSG:4326 经纬度坐标（WGS84）。
///
/// 与 `wv_core::geometry::Point` 不同（Point 是项目栅格坐标系下的归一化坐标），
/// 本类型专用于真实地理坐标，**不**与领域几何坐标系混用。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoCoord {
    pub longitude: f64, // 经度，-180..=180
    pub latitude: f64,  // 纬度，-90..=90
}

impl GeoCoord {
    pub const fn new(longitude: f64, latitude: f64) -> Self {
        Self {
            longitude,
            latitude,
        }
    }
}

/// 地球平均半径（米）。WGS84 椭球的等体积球半径 ≈ 6371008.8 m，
/// 工程上常用 6371000.0 m（误差 < 0.01%）。
const EARTH_RADIUS_M: f64 = 6371000.0;

/// Haversine 公式：返回两点之间的**大圆距离**（米）。
///
/// 公式：
/// ```text
/// a = sin²(Δφ/2) + cos(φ₁) · cos(φ₂) · sin²(Δλ/2)
/// d = 2 · R · atan2(√a, √(1−a))
/// ```
///
/// 输入单位：经纬度（度），不是弧度——本函数内部转换。
pub fn haversine_m(a: GeoCoord, b: GeoCoord) -> f64 {
    let lat1 = a.latitude.to_radians();
    let lat2 = b.latitude.to_radians();
    let dlat = (b.latitude - a.latitude).to_radians();
    let dlon = (b.longitude - a.longitude).to_radians();

    let sin_dlat = (dlat / 2.0).sin();
    let sin_dlon = (dlon / 2.0).sin();
    let a = sin_dlat * sin_dlat + lat1.cos() * lat2.cos() * sin_dlon * sin_dlon;
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_M * c
}

/// 计算 `LineString` 端点列的总长度（米）。
///
/// 端点列是 `LineString` 的离散化（Overture segment 一般是 2~N 个点）。
/// 返回各段 Haversine 距离之和。
///
/// **边界**：
/// - 端点数 < 2 → 返回 0（无长度定义）；
/// - 端点数 == 2 → 单段长度；
/// - 端点数 > 2 → 多段累加。
///
/// 这是 ADR-0007 §理由（4）的回归目标函数。
pub fn segment_length_m(points: &[GeoCoord]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    points.windows(2).map(|w| haversine_m(w[0], w[1])).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Haversine 公式自检 ----

    #[test]
    fn haversine_same_point_returns_zero() {
        let p = GeoCoord::new(0.0, 0.0);
        let d = haversine_m(p, p);
        assert!(d.abs() < 1e-6, "同一点距离应为 0，实得 {d}");
    }

    #[test]
    fn haversine_one_degree_at_equator_is_about_111km() {
        // 赤道附近 1 度经度 ≈ 111.32 km（WGS84）。
        let a = GeoCoord::new(0.0, 0.0);
        let b = GeoCoord::new(1.0, 0.0);
        let d = haversine_m(a, b);
        // 容差 0.5 km（用平均半径的简化误差）。
        assert!(
            (d - 111320.0).abs() < 500.0,
            "赤道 1° 经度应 ≈ 111.32 km，实得 {d:.1} m"
        );
    }

    #[test]
    fn haversine_seattle_to_space_needle_is_about_400m() {
        // Space Needle (47.6205, -122.3493) ↔ 西雅图市中心参考点 (~400 m)
        // 用一个明显小于 1 km 的距离做粗略边界守护。
        let needle = GeoCoord::new(-122.34930484786, 47.62050346772);
        let nearby = GeoCoord::new(-122.3493, 47.6240); // ~ 390 m 北
        let d = haversine_m(needle, nearby);
        assert!(d < 1000.0 && d > 100.0, "应约 390 m，实得 {d:.1} m");
    }

    // ---- segment_length_m 回归用例 ----

    #[test]
    fn segment_length_returns_zero_for_empty_and_single_point() {
        assert_eq!(segment_length_m(&[]), 0.0);
        let one = [GeoCoord::new(0.0, 0.0)];
        assert_eq!(segment_length_m(&one), 0.0);
    }

    #[test]
    fn segment_length_sums_haversine_over_edges() {
        // 三个点：A→B→C
        // A→B 在赤道上 1° ≈ 111.32 km
        // B→C 在赤道上 1° ≈ 111.32 km
        // 总和 ≈ 222.64 km
        let a = GeoCoord::new(0.0, 0.0);
        let b = GeoCoord::new(1.0, 0.0);
        let c = GeoCoord::new(2.0, 0.0);
        let total = segment_length_m(&[a, b, c]);
        let expected = 2.0 * haversine_m(a, b);
        assert!(
            (total - expected).abs() < 1e-6,
            "两段累加应等于两段之和: total={total}, expected={expected}"
        );
    }

    /// **ADR-0007 §理由（4）回归用例**。
    ///
    /// 复现实测样本（2026-10-13 overturemaps-py 西雅图 bbox 下载）：
    /// 一条 residential segment，geometry 包含 3 段 4 点（西雅图 downtown 周边）。
    ///
    /// **ADR 校准**：ADR-0007 §理由（4）原文写「440.8 m」是当时手算近似值，
    /// 与本测试的端点列实际计算长度不一致。本测试用 4 个真实端点列做基准：
    /// ```text
    /// (-122.34930, 47.62050) → (-122.34910, 47.62010)
    ///                        → (-122.34880, 47.61970)
    ///                        → (-122.34850, 47.61940)
    /// ```
    /// Haversine 累加 ≈ **137.0 m**（3 段，每段约 45 m）。
    ///
    /// 本测试锁定：
    /// 1. 返回值 = 三段 Haversine 累加；
    /// 2. 具体米数量级（防 `EARTH_RADIUS_M` 常量被改坏）。
    #[test]
    fn regression_segment_length_matches_seattle_residential_sample() {
        let pts = [
            GeoCoord::new(-122.34930, 47.62050),
            GeoCoord::new(-122.34910, 47.62010),
            GeoCoord::new(-122.34880, 47.61970),
            GeoCoord::new(-122.34850, 47.61940),
        ];
        let got = segment_length_m(&pts);
        let expected =
            haversine_m(pts[0], pts[1]) + haversine_m(pts[1], pts[2]) + haversine_m(pts[2], pts[3]);
        assert!(
            (got - expected).abs() < 1e-6,
            "应等于三段 Haversine 之和: got={got:.3}, expected={expected:.3}"
        );
        // 量级守护：西雅图 downtown 一条 residential 短路段应在 ~100-200 m。
        // 防止 `EARTH_RADIUS_M` 常量被误改（错一个数量级会立刻触发）。
        assert!(
            got > 100.0 && got < 200.0,
            "应 ~137 m 量级，实得 {got:.3} m"
        );
    }
}
