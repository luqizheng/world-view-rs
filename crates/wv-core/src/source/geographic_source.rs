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
}
