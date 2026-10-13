//! Overture `transportation` 主题 → `PlaceEdge` 字段映射（ADR-0007 §理由（4））。
//!
//! ## 职责
//!
//! 把 Overture segment 的字段（`subtype` / `class` / `geometry`）转成本项目
//! 领域模型的字段（`PlaceEdge.distance_km` / `PlaceEdge.terrain` / `OvertureId`）。
//!
//! ## 关键纪律
//!
//! - **单向注入**（Overture → 领域字段），反方向回灌会绕过 AGENTS §4.2 落地判据；
//! - `subtype` / `class` 都是开放集字符串**，**不**用枚举化；
//! - `terrain` 字段是 `String`，由作者在 `Vehicle.terrain_modifiers` 中自定义；
//! - **footway 默认过滤**：步行专用（待裁 F，第一版不生成 `PlaceEdge`）。
//!
//! ## 待裁
//!
//! - **F** `class: footway` 是否独立分支（西雅图 77604 行 footway 占 50%）。
//!   第一版默认过滤。若「步行」成为独立 `Vehicle`，需独立 `PlaceEdge`
//!   （含 `footway_only: bool`），需检索器处理。

use wv_core::OvertureId;

/// Overture segment 转换出的领域字段候选（不含坐标 / geometry）。
///
/// **不**是 `PlaceEdge` 本身——`PlaceEdge` 由调用方组装（本函数不返回完整
/// `PlaceEdge`，因为本 crate 不依赖 `wv-core::distance` 之外的全部领域类型）。
/// 本结构只承载**转换层的中间结果**，调用方负责拼装完整 `PlaceEdge`。
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceEdgeCandidate {
    pub distance_km: f32,
    pub terrain: String,
    pub overture_id: Option<OvertureId>,
}

/// Overture `class` → `terrain` 字符串映射。
///
/// 映射表锚定到示例工程已使用的字符串池（`road` / `plain` / `mountain` / `forest`），
/// 详见 `examples/minimal-world/distances/vehicles.yaml`。
///
/// **不**接受枚举参数——`class` 实测是字符串（OSM 风格），未来 OSM 升级会新增类。
/// 返回 `None` 表示该 `class` 应被过滤掉（不生成 `PlaceEdge`）。
///
/// 第一版（2026-10-13）的映射策略：
/// - `motorway` / `trunk` / `primary` / `secondary` / `tertiary` → `road`
/// - `service` / `residential` / `unclassified` → `road`
/// - `footway` / `path` / `cycleway` / `pedestrian` / `track` / `steps` → `None`（步行专用，F 待裁）
/// - `unknown` → `Some("road")`（兜底，调用方建议发 hint）
///
/// 返回 `Some("road")` 即使值相同，**仍**返回 `Some`（避免 `Option` 嵌套）。
pub fn class_to_terrain(class: &str) -> Option<&'static str> {
    match class {
        // 等级公路：OSM highway 等级体系映射到项目 `road` 字符串池
        "motorway" | "trunk" | "primary" | "secondary" | "tertiary" => Some("road"),
        // 城内路 / 服务路 / 未分类路：仍属「可通行的路」
        "service" | "residential" | "unclassified" => Some("road"),
        // 步行专用：第一版默认不生成 PlaceEdge（待裁 F）
        "footway" | "path" | "cycleway" | "pedestrian" | "track" | "steps" => None,
        // OSM 偶发缺标签：兜底为 road（建议校验面板提示）
        "unknown" => Some("road"),
        // 任何 Overture 新增的 class：兜底为 road（不阻塞导入）
        // 这是 §4.3「数据承载开放集」的体现——我们的映射表是开放的。
        _ => Some("road"),
    }
}

/// Overture `subtype` 过滤门（ADR-0007 §理由（4）转换映射表）。
///
/// 西雅图样本 154k segments 中：
/// - `subtype: road` 占 99%（含 footway）
/// - `subtype: rail` / `water` / 其他 占 1%
///
/// 第一版**只保留 `subtype: road`**（轨道交通与水路不在 PlaceEdge 范畴——
/// 它们对应的是不同的领域模型扩展，本 ADR 不承担）。
///
/// 返回 `true` 表示应保留并继续处理。
pub fn should_keep_subtype(subtype: &str) -> bool {
    subtype == "road"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motorway_trunk_primary_all_map_to_road() {
        assert_eq!(class_to_terrain("motorway"), Some("road"));
        assert_eq!(class_to_terrain("trunk"), Some("road"));
        assert_eq!(class_to_terrain("primary"), Some("road"));
        assert_eq!(class_to_terrain("secondary"), Some("road"));
        assert_eq!(class_to_terrain("tertiary"), Some("road"));
    }

    #[test]
    fn residential_and_service_stay_as_road() {
        // ADR-0007 §理由（4）：城内/服务路归 road，**不**单独建 plain。
        assert_eq!(class_to_terrain("residential"), Some("road"));
        assert_eq!(class_to_terrain("service"), Some("road"));
        assert_eq!(class_to_terrain("unclassified"), Some("road"));
    }

    #[test]
    fn footway_and_pedestrian_returns_none_for_filtering() {
        // F 待裁：footway 默认过滤（不生成 PlaceEdge）。
        // 西雅图样本 77604 行 footway 占 50%。
        assert_eq!(class_to_terrain("footway"), None);
        assert_eq!(class_to_terrain("path"), None);
        assert_eq!(class_to_terrain("cycleway"), None);
        assert_eq!(class_to_terrain("pedestrian"), None);
        assert_eq!(class_to_terrain("track"), None);
        assert_eq!(class_to_terrain("steps"), None);
    }

    #[test]
    fn unknown_class_falls_back_to_road_with_implicit_hint() {
        assert_eq!(class_to_terrain("unknown"), Some("road"));
        // 兜底：OSM 任何新增 class 都不阻塞导入。
        assert_eq!(class_to_terrain("funicular"), Some("road"));
        assert_eq!(class_to_terrain("alien_road_type"), Some("road"));
    }

    #[test]
    fn subtype_filter_keeps_road_only() {
        assert!(should_keep_subtype("road"));
        assert!(!should_keep_subtype("rail"));
        assert!(!should_keep_subtype("water"));
        assert!(!should_keep_subtype(""));
    }

    #[test]
    fn mapping_table_is_open_set_consistent() {
        // §4.3「数据承载开放集」：映射表不强制穷举。
        // 同一 class 多次调用结果稳定（无内部状态）。
        for _ in 0..3 {
            assert_eq!(class_to_terrain("primary"), Some("road"));
            assert_eq!(class_to_terrain("footway"), None);
        }
    }

    // ---- 端到端：从 Overture segment 字段到 PlaceEdgeCandidate ----

    /// 模拟一条 Overture segment 的字段（DRY 端到端测试用）。
    /// 真实 Overture row 的字段名：id, subtype, class, geometry (LineString), sources。
    fn sample_segment(subtype: &str, class: &str, overture_id: &str) -> SegmentRow {
        SegmentRow {
            subtype: subtype.into(),
            class: class.into(),
            overture_id: OvertureId::new(overture_id).unwrap(),
        }
    }

    /// 模拟「读 Overture Parquet → 字段抽出 → 转换 → PlaceEdgeCandidate」的全链路。
    fn convert(row: &SegmentRow, length_m: f64) -> Option<PlaceEdgeCandidate> {
        if !should_keep_subtype(&row.subtype) {
            return None;
        }
        let terrain = class_to_terrain(&row.class)?;
        Some(PlaceEdgeCandidate {
            distance_km: (length_m / 1000.0) as f32,
            terrain: terrain.to_string(),
            overture_id: Some(row.overture_id.clone()),
        })
    }

    #[derive(Debug)]
    struct SegmentRow {
        subtype: String,
        class: String,
        overture_id: OvertureId,
    }

    #[test]
    fn end_to_end_residential_segment_becomes_road_place_edge() {
        // ADR-0007 §理由（4）回归用例：137 m 西雅图 downtown residential segment
        let row = sample_segment("road", "residential", "50e76fae-ac7e-448a-b34d-fcb4db17b5ee");
        let cand = convert(&row, 137.0).expect("residential road 应被保留");
        assert!((cand.distance_km - 0.137).abs() < 1e-4, "got {}", cand.distance_km);
        assert_eq!(cand.terrain, "road");
        assert_eq!(
            cand.overture_id.as_ref().unwrap().as_str(),
            "50e76fae-ac7e-448a-b34d-fcb4db17b5ee"
        );
    }

    #[test]
    fn end_to_end_motorway_segment_becomes_road_place_edge() {
        let row = sample_segment("road", "motorway", "82975819-f06b-42a7-91e2-3fef62a4b31c");
        let cand = convert(&row, 5000.0).expect("motorway 应被保留");
        assert_eq!(cand.terrain, "road");
        assert!((cand.distance_km - 5.0).abs() < 1e-4);
    }

    #[test]
    fn end_to_end_footway_is_filtered_out() {
        // F 待裁：footway 默认过滤。
        let row = sample_segment("road", "footway", "11111111-2222-3333-4444-555555555555");
        assert_eq!(convert(&row, 50.0), None, "footway 默认不生成 PlaceEdge");
    }

    #[test]
    fn end_to_end_rail_subtype_is_filtered_out() {
        let row = sample_segment("rail", "rail", "22222222-3333-4444-5555-666666666666");
        assert_eq!(convert(&row, 1000.0), None, "rail 子类型不在 PlaceEdge 范畴");
    }

    #[test]
    fn end_to_end_unknown_class_falls_back_to_road() {
        let row = sample_segment("road", "alien_type", "33333333-4444-5555-6666-777777777777");
        let cand = convert(&row, 200.0).expect("未知 class 兜底为 road，不阻塞");
        assert_eq!(cand.terrain, "road");
    }
}