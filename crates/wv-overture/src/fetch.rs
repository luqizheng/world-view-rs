//! CLI `wv-overture fetch <map-id>` —— 下载并转换 Overture 切片（ADR-0007 §决策 (5)）。
//!
//! ## 当前实现策略（2026-10-13）
//!
//! **GeoJSON 中间格式**——避免 Parquet 依赖：
//! 1. `fetch` 调用 `overturemaps-py`（项目已有 Python 工具）下载 GeoJSON；
//! 2. `parse_geojson_segments` 把 GeoJSON FeatureCollection 转成 `PlaceEdgeCandidate` 列表；
//! 3. 调用方负责把 `PlaceEdgeCandidate` 写回领域 YAML。
//!
//! 为什么不直接读 Parquet（README §17 §18 待裁）：
//! - `parquet` 53.x 兼容 MSRV 1.80（AGENTS §8），但 GeoParquet 需 WKB 解码；
//! - `duckdb ~1.20000.0`：体积 ~30MB、编译时间 >5min；
//! - 短期 `serde_json` 体积小、GeoJSON 与 Overture 文档示例对齐。
//!
//! ## 边界
//!
//! - **只处理 `transportation` theme**（第一版）；其他主题（places / buildings）按本框架扩展。
//! - **每个 segment 走 `class_to_terrain()` + `should_keep_subtype()`**（见
//!   [`crate::transportation`]）；被过滤的 segment 不进入结果集（第一版默认）。
//! - **不写回领域模型**——本模块只返回 `Vec<PlaceEdgeCandidate>`，调用方写盘。

use serde::Deserialize;

use crate::geometry::{haversine_m, GeoCoord};
use crate::transportation::{class_to_terrain, should_keep_subtype, PlaceEdgeCandidate};
use wv_core::OvertureId;

/// GeoJSON FeatureCollection —— Overture 输出格式之一（实测 geojson / geojsonseq）。
///
/// 仅声明本模块**用到的字段**，其余由 `serde_json::Value` 中转（如 `sources` / `properties.confidence` 等暂不消费）。
#[derive(Debug, Deserialize)]
pub struct GeoJSONFeatureCollection {
    pub r#type: String,
    pub features: Vec<GeoJSONFeature>,
}

/// 单条 GeoJSON Feature。
#[derive(Debug, Deserialize)]
pub struct GeoJSONFeature {
    /// Overture GeoJSON 里 Feature.id = GERS UUID（实测 Space Needle 的 GERS）。
    pub id: String,
    /// GeoJSON 几何对象。**仅支持 `LineString` 与 `Point`**；
    /// `Polygon` / `MultiPolygon` 不在本 crate 范畴（应走 `wv-terrain` 的高度场）。
    pub geometry: GeoJSONGeometry,
    #[serde(default)]
    pub properties: GeoJSONProperties,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum GeoJSONGeometry {
    Point { coordinates: [f64; 2] },
    LineString { coordinates: Vec<[f64; 2]> },
    #[serde(other)]
    Other,
}

/// Overture `transportation` segment 的 properties 字段（仅消费用到的）。
#[derive(Debug, Default, Deserialize)]
pub struct GeoJSONProperties {
    #[serde(default)]
    pub subtype: String,
    #[serde(default)]
    pub class: String,
}

/// GeoJSON FeatureCollection → `PlaceEdgeCandidate` 列表。
///
/// **过滤策略**（见 [`crate::transportation`]）：
/// 1. 几何不是 `LineString` → 跳过；
/// 2. `subtype != "road"` → 跳过；
/// 3. `class` 在步行专用集中（footway / path / cycleway / pedestrian / track / steps）→ 跳过；
/// 4. 端点数 < 2 → 跳过（无长度定义）；
/// 5. 通过过滤的 segment 转成 `PlaceEdgeCandidate`。
///
/// 端点数 = 2 的 segment 已是 `LineString` 的最小形态，**直接**算 Haversine；
/// 端点数 > 2 的多段用 [`crate::geometry::segment_length_m`] 累加。
///
/// **OvertureId 解析**：从 `Feature.id`（hex UUID）尝试构造；失败时降级为 `None`。
/// 这意味着非 UUID 形式的 ID 仍能进入 `PlaceEdgeCandidate`（只是丢失引用追踪）。
pub fn parse_geojson_segments(gj: &GeoJSONFeatureCollection) -> Vec<PlaceEdgeCandidate> {
    let mut out = Vec::with_capacity(gj.features.len());
    for feat in &gj.features {
        let coords: Vec<GeoCoord> = match &feat.geometry {
            GeoJSONGeometry::LineString { coordinates } => coordinates
                .iter()
                .map(|c| GeoCoord::new(c[0], c[1]))
                .collect(),
            _ => continue, // 非 LineString 跳过
        };

        if !should_keep_subtype(&feat.properties.subtype) {
            continue;
        }
        let terrain = match class_to_terrain(&feat.properties.class) {
            Some(t) => t,
            None => continue, // 被过滤
        };

        let length_m = sum_haversine(&coords);
        if length_m <= 0.0 {
            continue;
        }

        out.push(PlaceEdgeCandidate {
            distance_km: (length_m / 1000.0) as f32,
            terrain: terrain.to_string(),
            overture_id: OvertureId::new(&feat.id).ok(),
        });
    }
    out
}

fn sum_haversine(coords: &[GeoCoord]) -> f64 {
    if coords.len() < 2 {
        return 0.0;
    }
    coords
        .windows(2)
        .map(|w| haversine_m(w[0], w[1]))
        .sum()
}

/// 从字符串解析 GeoJSON FeatureCollection。
pub fn parse_geojson_str(s: &str) -> Result<GeoJSONFeatureCollection, FetchError> {
    serde_json::from_str(s).map_err(|e| FetchError::ParseFailed(e.to_string()))
}

/// 本模块错误类型（thiserror，与 AGENTS §8 一致）。
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("GeoJSON parse failed: {0}")]
    ParseFailed(String),
    /// CLI 子命令 `wv-overture fetch` 调用 `overturemaps-py` 的过程错误（第一版未实现）。
    #[error("overturemaps-py invocation failed: {0}")]
    PyCliFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SEGMENT_GEOJSON: &str = r#"{
        "type": "FeatureCollection",
        "features": [
            {
                "id": "50e76fae-ac7e-448a-b34d-fcb4db17b5ee",
                "type": "Feature",
                "geometry": {
                    "type": "LineString",
                    "coordinates": [
                        [-122.34930, 47.62050],
                        [-122.34910, 47.62010],
                        [-122.34880, 47.61970],
                        [-122.34850, 47.61940]
                    ]
                },
                "properties": {
                    "subtype": "road",
                    "class": "residential"
                }
            }
        ]
    }"#;

    #[test]
    fn parses_residential_segment_into_place_edge_candidate() {
        let fc = parse_geojson_str(SAMPLE_SEGMENT_GEOJSON).unwrap();
        let cands = parse_geojson_segments(&fc);
        assert_eq!(cands.len(), 1, "residential road 应保留");

        let cand = &cands[0];
        assert_eq!(cand.terrain, "road");
        // 137 m 端到端回归（与 geometry::regression 同根）。
        assert!(
            (cand.distance_km - 0.137).abs() < 0.005,
            "应 ~0.137 km，实得 {} km",
            cand.distance_km
        );
        assert_eq!(
            cand.overture_id.as_ref().unwrap().as_str(),
            "50e76fae-ac7e-448a-b34d-fcb4db17b5ee"
        );
    }

    #[test]
    fn filters_footway_segments() {
        let gj = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "id": "11111111-2222-3333-4444-555555555555",
                    "type": "Feature",
                    "geometry": {"type": "LineString", "coordinates": [[0,0],[1,1]]},
                    "properties": {"subtype": "road", "class": "footway"}
                }
            ]
        }"#;
        let fc = parse_geojson_str(gj).unwrap();
        let cands = parse_geojson_segments(&fc);
        assert!(cands.is_empty(), "footway 默认过滤");
    }

    #[test]
    fn filters_non_road_subtype() {
        let gj = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "id": "22222222-3333-4444-5555-666666666666",
                    "type": "Feature",
                    "geometry": {"type": "LineString", "coordinates": [[0,0],[1,1]]},
                    "properties": {"subtype": "rail", "class": "rail"}
                }
            ]
        }"#;
        let fc = parse_geojson_str(gj).unwrap();
        assert!(parse_geojson_segments(&fc).is_empty());
    }

    #[test]
    fn skips_point_geometry_silently() {
        // Point 不在本转换路径范畴（PlaceEdge 需要 LineString）。
        let gj = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "id": "33333333-4444-5555-6666-777777777777",
                    "type": "Feature",
                    "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
                    "properties": {"subtype": "road", "class": "motorway"}
                }
            ]
        }"#;
        let fc = parse_geojson_str(gj).unwrap();
        assert!(parse_geojson_segments(&fc).is_empty());
    }

    #[test]
    fn invalid_uuid_does_not_block_other_features() {
        // 坏 UUID 不能「牵连」整批转换 —— 仅 Feature 自身降级为 None。
        let gj = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "id": "not-a-uuid",
                    "type": "Feature",
                    "geometry": {"type": "LineString", "coordinates": [[0,0],[0.001,0]]},
                    "properties": {"subtype": "road", "class": "primary"}
                },
                {
                    "id": "44444444-5555-6666-7777-888888888888",
                    "type": "Feature",
                    "geometry": {"type": "LineString", "coordinates": [[0,0],[0.002,0]]},
                    "properties": {"subtype": "road", "class": "primary"}
                }
            ]
        }"#;
        let fc = parse_geojson_str(gj).unwrap();
        let cands = parse_geojson_segments(&fc);
        assert_eq!(cands.len(), 2);
        assert!(cands[0].overture_id.is_none(), "坏 UUID 应降级为 None");
        assert!(cands[1].overture_id.is_some());
    }

    #[test]
    fn empty_feature_collection_yields_empty_candidates() {
        let gj = r#"{"type":"FeatureCollection","features":[]}"#;
        let fc = parse_geojson_str(gj).unwrap();
        assert!(parse_geojson_segments(&fc).is_empty());
    }

    #[test]
    fn parse_error_propagates_cleanly() {
        let bad = "{ this is not json }";
        let err = parse_geojson_str(bad).unwrap_err();
        // 不让错误冒到 caller —— 强制 FetchError 包装。
        assert!(matches!(err, FetchError::ParseFailed(_)));
    }
}