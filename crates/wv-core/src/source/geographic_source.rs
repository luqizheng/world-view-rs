//! 真实地理底图引用：Overture Maps 元数据。
//!
//! 本模块实现 ADR-0007 的核心数据结构：
//! - [`GeographicSourceV1`]：地图引用真实地理资源时的元数据声明；
//! - [`LatLonBbox`]：EPSG:4326 经纬度 bbox。
//!
//! 设计纪律：
//! - **领域模型不存几何**：仅存 `theme` / `bbox` / `data_path` 引用，
//!   Parquet 文件本身位于工程 `data/` 目录，由 `wv-overture` crate 读取。
//! - **真假地图派生**：`Map.geographic_source: Option<GeographicSourceV1>`，
//!   `None` 即虚构地图，`Some(...)` 即引用真实资源。
//!   不另设 `geographic_kind` 枚举字段（避免 ADR-0001 D2「数据承载开放集」违反）。
//! - **结构体为数据**：`theme` 用 `String`（开放集），不枚举化。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 真实地理底图引用元数据。YAML 落盘形式：
///
/// ```yaml
/// geographic_source:
///   theme: places
///   bbox:
///     west: -122.50
///     south: 47.50
///     east: -122.20
///     north: 47.75
///   data_path: maps/seattle_modern/data/places.parquet
///   release: 2026-09-23.0   # 可选；缺省 = Overture latest
/// ```
///
/// 与 `Map.geographic_source: Option<GeographicSourceV1>` 配套：
/// `None` 即虚构地图，`Some(...)` 即引用真实资源。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GeographicSourceV1 {
    /// Overture 主题名（`places` / `transportation` / `buildings` /
    /// `divisions` / `addresses` / `base` 等）。
    ///
    /// 自由字符串，由 Overture schema 定义有效值。
    /// **不**枚举化以遵守 ADR-0001 D2「数据承载开放集」（Overture 主题集会扩张）。
    #[serde(default)]
    pub theme: String,

    /// 切片 bbox（EPSG:4326 经纬度，WGS84）。
    #[serde(default)]
    pub bbox: LatLonBbox,

    /// 工程内 Parquet 文件路径（相对工程根）。
    ///
    /// 与 `terrain.bin` 路径声明风格相同（AGENTS §5 二进制分块纪律）。
    #[serde(default)]
    pub data_path: PathBuf,

    /// Overture release tag（如 `2026-09-23.0`）。
    ///
    /// 可选；缺省由 `wv-overture` 默认选 latest。
    /// **写出来 = 钉版**，**省略 = 浮动**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<String>,
}

/// EPSG:4326 经纬度 bbox（西-南-东-北，按 WGS84）。
///
/// 坐标系与 `wv-terrain` 的栅格 bbox 不同（栅格用栅格坐标系），
/// 因此**不**抽公共 `Bbox` 类型，独立维护。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LatLonBbox {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl LatLonBbox {
    /// 经度范围（东 - 西）。
    pub fn width(&self) -> f64 {
        self.east - self.west
    }

    /// 纬度范围（北 - 南）。
    pub fn height(&self) -> f64 {
        self.north - self.south
    }

    /// 标准化坐标顺序：保证 west ≤ east、south ≤ north。
    /// 若用户反传（north < south 或 east < west），自动归位。
    pub fn normalized(&self) -> Self {
        Self {
            west: self.west.min(self.east),
            east: self.west.max(self.east),
            south: self.south.min(self.north),
            north: self.south.max(self.north),
        }
    }

    /// 把 `bbox` 覆盖面积折算成 `wu → km` 的系数。
    ///
    /// 返回 `(km_per_wu_x, km_per_wu_y)`。计算口径：
    ///
    /// - 1° 纬度 ≈ 111.132 km（地球平均，不分南北）；
    /// - 1° 经度 ≈ 111.132 · cos(中心纬度) km（Mercator 真实经度缩放）；
    /// - 折算：把 bbox 的经纬度范围当成本图 extent 的世界尺度覆盖，
    ///   系数 = `(bbox_km) / (extent_wu)`。
    ///
    /// ## 这是**近似**，不是测绘
    ///
    /// - bbox 与 extent 中心**不必对齐**（子图继承父图底图时，bbox 写
    ///   的是父图那一块）；
    /// - 真实地图的米制尺度由 ADR-0007 与地理信息本身决定**，本方法
    ///   不重新实现**，只给一个基于 bbox 的粗估；
    /// - 子图（City 级）应使用**父图**的 bbox 换算（调用方负责传入）。
    ///
    /// `extent.width` 或 `extent.height ≤ 0` 时返回 `None`——避免除零。
    pub fn km_per_wu(&self, extent: &crate::map::Extent) -> Option<(f64, f64)> {
        let w = extent.width as f64;
        let h = extent.height as f64;
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let n = self.normalized();
        // 中心纬度（用于经度 cos 修正）。
        let center_lat_rad = ((n.north + n.south) * 0.5).to_radians();
        let lat_km = n.height() * 111.132;
        let lon_km = n.width() * 111.132 * center_lat_rad.cos();
        Some((lon_km / w, lat_km / h))
    }

    /// 图内归一化坐标（UV）→ 经纬度（WGS84，单位度）。
    ///
    /// UV 口径与 [`crate::map::IconSpec::anchor`] / [`crate::map::coord::MapFrame::world_to_uv`]
    /// 一致：`[0,1]²`、**左下角（西南角）为原点、y 向上**——`[0,0]` 是本图
    /// **西南角**，`[1,1]` 是东北角（ADR-0034「上北下南」）。调用链是
    /// `世界坐标 → (coord 模块) UV → 本方法`，本方法只负责最后一段线性映射，
    /// 不碰世界坐标。
    ///
    /// 返回 `(纬度 lat, 经度 lon)`。越界输入按线性外推（不夹取）——
    /// 「光标是否在图幅内、要不要显示经纬度」是**可见性**判断，归调用方。
    pub fn uv_to_lat_lon(&self, uv: [f64; 2]) -> (f64, f64) {
        let n = self.normalized();
        let lat = n.south + uv[1] * n.height();
        let lon = n.west + uv[0] * n.width();
        (lat, lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bbox_width_and_height_are_non_negative_after_normalization() {
        // 反传坐标：north < south、east < west。
        let raw = LatLonBbox {
            west: 10.0,
            south: -5.0,
            east: 5.0,
            north: -10.0,
        };
        let n = raw.normalized();
        assert_eq!(n.west, 5.0);
        assert_eq!(n.east, 10.0);
        assert_eq!(n.south, -10.0);
        assert_eq!(n.north, -5.0);
        assert!((n.width() - 5.0).abs() < 1e-9);
        assert!((n.height() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn geographic_source_default_is_empty() {
        let src = GeographicSourceV1::default();
        assert_eq!(src.theme, "");
        assert_eq!(src.bbox.west, 0.0);
        assert_eq!(src.data_path.to_str(), Some(""));
        assert!(src.release.is_none());
    }

    /// 西雅图 ~0.25° × 0.25° 的 bbox，extent = 1024 × 1024 wu。
    /// 纬度 1° ≈ 111.132 km，所以 0.25° ≈ 27.78 km。
    /// 中心纬度 ~47.625°，cos(47.625°) ≈ 0.6732；经度 0.25° ≈ 18.70 km。
    /// 因此 km/wu_x ≈ 0.0183，km/wu_y ≈ 0.0271。
    #[test]
    fn km_per_wu_seattle_like_bbox() {
        let bbox = LatLonBbox {
            west: -122.50,
            south: 47.50,
            east: -122.25,
            north: 47.75,
        };
        let ext = crate::map::Extent::sized(1024.0, 1024.0);
        let (kx, ky) = bbox.km_per_wu(&ext).unwrap();
        assert!(
            (ky - 27.78 / 1024.0).abs() < 1e-3,
            "ky ≈ {} got {}",
            27.78 / 1024.0,
            ky
        );
        // 经度系数受 cos 修正，预期 ~ 0.01825
        assert!(
            (kx - 18.70 / 1024.0).abs() < 1e-3,
            "kx ≈ {} got {}",
            18.70 / 1024.0,
            kx
        );
        // 比例关系：纬度系数 > 经度系数（Seattle 偏北）
        assert!(ky > kx, "Seattle 纬度 1° 的 km 数应大于经度 1°");
    }

    /// 赤道地区（中心纬度 0）：经度 cos 修正 = 1。
    /// bbox = (-0.25..0.25, -0.25..0.25)，中心 0°，各 0.5° 跨度。
    /// 0.5° ≈ 55.566 km，cos(0°) = 1，所以 kx = ky。
    #[test]
    fn km_per_wu_at_equator_is_isotropic() {
        let bbox = LatLonBbox {
            west: -0.25,
            south: -0.25,
            east: 0.25,
            north: 0.25,
        };
        let ext = crate::map::Extent::sized(1000.0, 1000.0);
        let (kx, ky) = bbox.km_per_wu(&ext).unwrap();
        // 赤道 cos(0) = 1，故经度 km/° 与纬度一致
        assert!(
            (kx - ky).abs() < 1e-6,
            "赤道处 x/y 系数应相等：{kx} vs {ky}"
        );
    }

    /// 极地：中心纬度 ±90°，cos → 0。经度 km/wu 应趋于 0。
    #[test]
    fn km_per_wu_near_pole_collapses_lon_factor() {
        let bbox = LatLonBbox {
            west: 0.0,
            south: 89.0,
            east: 1.0,
            north: 89.5,
        };
        let ext = crate::map::Extent::sized(1000.0, 1000.0);
        let (kx, ky) = bbox.km_per_wu(&ext).unwrap();
        assert!(kx < ky, "洛地 x 系数应小于 y，实际 kx={kx} ky={ky}");
        assert!(kx < 0.02, "极地 1° 经度换算 km 应接近 0，实际 {kx}");
    }

    /// extent = 0：返回 None（避免除零）。
    #[test]
    fn km_per_wu_returns_none_when_extent_is_zero() {
        let bbox = LatLonBbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        let ext = crate::map::Extent::ZERO;
        assert!(bbox.km_per_wu(&ext).is_none());

        let ext = crate::map::Extent::sized(0.0, 1024.0);
        assert!(bbox.km_per_wu(&ext).is_none());
    }

    /// UV 四角与中心的手算对位（西雅图式 bbox）：
    /// `[0,0]` = 西南角 (47.50, -122.50)，`[1,1]` = 东北角 (47.75, -122.25)，
    /// `[0.5,0.5]` = 中心 (47.625, -122.375)。UV 左下原点、y 向上（ADR-0034），
    /// 因此 **v 越大纬度越大**（v=1 落在北边界）。
    #[test]
    fn uv_to_lat_lon_maps_corners_and_center() {
        let bbox = LatLonBbox {
            west: -122.50,
            south: 47.50,
            east: -122.25,
            north: 47.75,
        };
        let (lat, lon) = bbox.uv_to_lat_lon([0.0, 0.0]);
        assert!((lat - 47.50).abs() < 1e-9, "西南角纬度，got {lat}");
        assert!((lon - (-122.50)).abs() < 1e-9, "西南角经度，got {lon}");

        let (lat, lon) = bbox.uv_to_lat_lon([1.0, 1.0]);
        assert!((lat - 47.75).abs() < 1e-9, "东北角纬度，got {lat}");
        assert!((lon - (-122.25)).abs() < 1e-9, "东北角经度，got {lon}");

        let (lat, lon) = bbox.uv_to_lat_lon([0.5, 0.5]);
        assert!((lat - 47.625).abs() < 1e-9, "中心纬度，got {lat}");
        assert!((lon - (-122.375)).abs() < 1e-9, "中心经度，got {lon}");
    }

    /// 反传 bbox（north < south）经 `normalized` 归位后结果一致：
    /// 本方法吃的是「作者可能写反」的真实数据。
    #[test]
    fn uv_to_lat_lon_normalizes_reversed_bbox() {
        let raw = LatLonBbox {
            west: -122.25,
            south: 47.75,
            east: -122.50,
            north: 47.50,
        };
        let (lat, lon) = raw.uv_to_lat_lon([0.0, 0.0]);
        assert!((lat - 47.50).abs() < 1e-9, "归位后西南角纬度，got {lat}");
        assert!(
            (lon - (-122.50)).abs() < 1e-9,
            "归位后西南角经度，got {lon}"
        );
    }

    /// 越界输入线性外推、不夹取——夹不夹是调用方的可见性策略。
    #[test]
    fn uv_to_lat_lon_extrapolates_out_of_range_input() {
        let bbox = LatLonBbox {
            west: 0.0,
            south: 0.0,
            east: 1.0,
            north: 1.0,
        };
        // v = 2：越过北边界一格 → lat = 0 + 2 = 2（y 向上，ADR-0034）。
        let (lat, lon) = bbox.uv_to_lat_lon([0.0, 2.0]);
        assert!((lat - 2.0).abs() < 1e-9, "北向外推纬度，got {lat}");
        assert!((lon - 0.0).abs() < 1e-9, "经度不外推，got {lon}");
    }
}
