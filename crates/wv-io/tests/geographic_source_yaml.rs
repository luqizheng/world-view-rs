//! ADR-0007 新增结构的 YAML round-trip 测试。
//!
//! 覆盖 `OvertureId`（UUID 例外条款）与 `GeographicSourceV1`
//! （真实地理底图引用元数据）的 YAML 序列化稳定性。
//!
//! 为什么放 `wv-io` 而不是 `wv-core`：`wv-core` 不引 `serde_yaml`（AGENTS §9
//! 「依赖默认拒绝」），集成测试在 `wv-io` 完成，符合 `AGENTS.md` §7.2
//! 「YAML round-trip 必须稳定」的强制要求。

use wv_core::geographic_source::{GeographicSourceV1, LatLonBbox};
use wv_core::overture_id::OvertureId;

// =============================================================================
// OvertureId
// =============================================================================

#[test]
fn overture_id_yaml_round_trips_for_real_space_needle_sample() {
    // Space Needle 实测 GERS UUID（ADR-0007 §理由（5））。
    let raw = "2f099609-47e9-4c3e-bfeb-072ccd609a03";
    let id = OvertureId::new(raw).expect("实测样本必须通过格式校验");

    let yaml = serde_yaml::to_string(&id).expect("序列化应成功");
    assert_eq!(yaml.trim(), "2f099609-47e9-4c3e-bfeb-072ccd609a03");

    let back: OvertureId = serde_yaml::from_str(&yaml).expect("反序列化应成功");
    assert_eq!(id, back);
}

#[test]
fn overture_id_yaml_rejects_invalid_uuid() {
    // 长度错
    let bad = serde_yaml::from_str::<OvertureId>("\"2f099609-47e9-4c3e-bfeb\"");
    assert!(bad.is_err(), "长度不足应被拒绝");

    // 位置 8 不是 '-'
    let bad = serde_yaml::from_str::<OvertureId>("\"2f099609_47e9-4c3e-bfeb-072ccd609a03\"");
    assert!(bad.is_err(), "连字符位错应被拒绝");

    // 非 hex 字符
    let bad = serde_yaml::from_str::<OvertureId>("\"gf099609-47e9-4c3e-bfeb-072ccd609a03\"");
    assert!(bad.is_err(), "非 hex 字符应被拒绝");

    // 大括号形式
    let bad = serde_yaml::from_str::<OvertureId>("\"\\{2f099609-47e9-4c3e-bfeb-072ccd609a03\\}\"");
    assert!(bad.is_err(), "大括号形式应被拒绝");
}

#[test]
fn overture_id_yaml_distinguishes_from_human_readable_id() {
    // ADR-0007 UUID 例外条款 §后果：YAML 里 overture_id 带 UUID 形式引号，
    // 与 PlaceId: city_seattle 视觉显著区分。
    use wv_core::Id;

    let yaml =
        "place:\n  id: city_seattle\n  overture_id: \"2f099609-47e9-4c3e-bfeb-072ccd609a03\"\n";

    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct Place {
        id: Id,
        overture_id: OvertureId,
    }

    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct Doc {
        place: Place,
    }

    let doc: Doc = serde_yaml::from_str(yaml).expect("并排解析应成功");
    assert_eq!(doc.place.id, Id::new_unchecked("city_seattle"));
    assert_eq!(
        doc.place.overture_id.as_str(),
        "2f099609-47e9-4c3e-bfeb-072ccd609a03"
    );
}

// =============================================================================
// GeographicSourceV1
// =============================================================================

#[test]
fn geographic_source_yaml_round_trips_with_all_fields() {
    let src = GeographicSourceV1 {
        theme: "places".into(),
        bbox: LatLonBbox {
            west: -122.50,
            south: 47.50,
            east: -122.20,
            north: 47.75,
        },
        data_path: "maps/seattle_modern/data/places.parquet".into(),
        release: Some("2026-09-23.0".into()),
    };

    let yaml = serde_yaml::to_string(&src).expect("序列化应成功");
    let back: GeographicSourceV1 = serde_yaml::from_str(&yaml).expect("反序列化应成功");
    assert_eq!(src, back, "全字段 YAML round-trip 必须稳定");

    // release 显式存在时应被序列化进 YAML。
    assert!(yaml.contains("release"), "Some(release) 必须出现: {yaml}");
    assert!(
        yaml.contains("2026-09-23.0"),
        "release tag 必须出现: {yaml}"
    );
}

#[test]
fn geographic_source_yaml_omits_release_when_none() {
    let src = GeographicSourceV1 {
        theme: "transportation".into(),
        bbox: LatLonBbox {
            west: -122.45,
            south: 47.62,
            east: -122.30,
            north: 47.65,
        },
        data_path: "data/segments.parquet".into(),
        release: None,
    };

    let yaml = serde_yaml::to_string(&src).expect("序列化应成功");
    // None 字段按 #[serde(skip_serializing_if = ...)] 不写入。
    assert!(
        !yaml.contains("release"),
        "None 的 release 不应出现: {yaml}"
    );

    let back: GeographicSourceV1 = serde_yaml::from_str(&yaml).expect("反序列化应成功");
    assert_eq!(src, back);
    assert!(back.release.is_none());
}

#[test]
fn geographic_source_yaml_handles_unsealed_bbox() {
    // 反传坐标（east < west、north < south），序列化后反序列化仍应通过，
    // 但调用方应自行 .normalized()。
    let src = GeographicSourceV1 {
        theme: "buildings".into(),
        bbox: LatLonBbox {
            west: 10.0,
            south: -5.0,
            east: 5.0,
            north: -10.0,
        },
        data_path: "data/buildings.parquet".into(),
        release: None,
    };

    let yaml = serde_yaml::to_string(&src).unwrap();
    let back: GeographicSourceV1 = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(src, back);

    let n = back.bbox.normalized();
    assert!((n.west - 5.0).abs() < 1e-9);
    assert!((n.east - 10.0).abs() < 1e-9);
    assert!((n.south - -10.0).abs() < 1e-9);
    assert!((n.north - -5.0).abs() < 1e-9);
}

// =============================================================================
// MapNode.geographic_source 字段集成
// =============================================================================

#[test]
fn map_node_with_geographic_source_round_trips() {
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::Id;

    let node = MapNode {
        id: Id::new_unchecked("seattle_modern"),
        name: "西雅图（现代）".into(),
        level: MapLevel::City,
        parent: Some(Id::new_unchecked("us_pacific_nw")),
        children: vec![],
        icons: vec![],
        layers: vec![],
        extent: None,
        raster: None,
        geographic_source: Some(GeographicSourceV1 {
            theme: "places".into(),
            bbox: LatLonBbox {
                west: -122.50,
                south: 47.50,
                east: -122.20,
                north: 47.75,
            },
            data_path: "maps/seattle_modern/data/places.parquet".into(),
            release: Some("2026-09-23.0".into()),
        }),
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    };

    let yaml = serde_yaml::to_string(&node).unwrap();
    let back: MapNode = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(node, back, "MapNode.geographic_source round-trip 必须稳定");

    // 视觉验证：geographic_source 块应作为顶层字段出现。
    assert!(
        yaml.contains("geographic_source"),
        "Some(...) 字段必须出现: {yaml}"
    );
    assert!(yaml.contains("theme: places"));
    assert!(yaml.contains("data_path: maps/seattle_modern/data/places.parquet"));
}

#[test]
fn map_node_without_geographic_source_omits_field() {
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::Id;

    let node = MapNode {
        id: Id::new_unchecked("fictional_world"),
        name: "虚构世界".into(),
        level: MapLevel::World,
        parent: None,
        children: vec![],
        icons: vec![],
        layers: vec![],
        extent: None,
        raster: None,
        geographic_source: None, // 虚构地图
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    };

    let yaml = serde_yaml::to_string(&node).unwrap();
    // 符合 #[serde(skip_serializing_if = "Option::is_none")] 约定。
    assert!(
        !yaml.contains("geographic_source"),
        "虚构地图不应出现 geographic_source 字段: {yaml}"
    );

    let back: MapNode = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(node, back);
    assert!(back.geographic_source.is_none());
}

#[test]
fn map_node_round_trip_preserves_presence_signal() {
    // ADR-0007 §决策（2）：真假地图由 Option 的 None/Some 派生。
    // 同一字段「出现 / 缺失」本身就是真假信号——必须有测试守护。
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::Id;

    let real = MapNode {
        id: Id::new_unchecked("real_map"),
        name: "real".into(),
        level: MapLevel::World,
        parent: None,
        children: vec![],
        icons: vec![],
        layers: vec![],
        extent: None,
        raster: None,
        geographic_source: Some(GeographicSourceV1::default()),
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    };
    let fictional = MapNode {
        unit_per_km: None,
        orientation: 0.0,
        id: Id::new_unchecked("fictional_map"),
        geographic_source: None,
        ..real.clone()
    };

    let real_yaml = serde_yaml::to_string(&real).unwrap();
    let fictional_yaml = serde_yaml::to_string(&fictional).unwrap();

    // 真地图有字段，虚构没有——肉眼一眼可分。
    assert!(real_yaml.contains("geographic_source"));
    assert!(!fictional_yaml.contains("geographic_source"));

    // 但二者字段集合差，不破坏 round-trip。
    let real_back: MapNode = serde_yaml::from_str(&real_yaml).unwrap();
    let fictional_back: MapNode = serde_yaml::from_str(&fictional_yaml).unwrap();
    assert!(real_back.geographic_source.is_some());
    assert!(fictional_back.geographic_source.is_none());
}
