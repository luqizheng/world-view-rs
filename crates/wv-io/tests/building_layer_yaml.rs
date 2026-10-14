//! 建筑图层文件（`Layer.source` 指向的 YAML）的读写验证（ADR-0016 §6）。
//!
//! 与 `street_layer_yaml.rs` 同型：文档模型在 `wv-core::building`，
//! 读写入口在 `Project`。round-trip 按 AGENTS §7.2 必须无损。

use wv_core::{Building, BuildingLayer, Point};

#[test]
fn building_layer_round_trips_through_yaml() {
    let mut layer = BuildingLayer::new();
    layer.push(Building::rect(10.0, 20.0, 30.0, 40.0, 12.5));
    layer.push(Building {
        name: Some("石库门".into()),
        height_m: 6.0,
        ring: vec![
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 4.0),
            Point::new(4.0, 4.0),
            Point::new(4.0, 10.0),
            Point::new(0.0, 10.0),
        ],
    });

    let text = serde_yaml::to_string(&layer).expect("序列化");
    let back: BuildingLayer = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, layer, "YAML round-trip 应无损");
}

#[test]
fn building_without_height_defaults_to_nine_meters() {
    // Point 的 YAML 形态是映射（与街道一致）；height_m 缺省 9.0（约三层）。
    let layer: BuildingLayer = serde_yaml::from_str(
        "buildings:\n  - ring:\n      - {x: 1, y: 2}\n      - {x: 3, y: 4}\n      - {x: 5, y: 1}",
    )
    .expect("解析");
    assert_eq!(layer.len(), 1, "应解析出 1 栋");
    assert!(
        (layer.buildings[0].height_m - 9.0).abs() < 1e-6,
        "缺 height_m 应取缺省 9 米，实得 {}",
        layer.buildings[0].height_m
    );
    assert_eq!(layer.buildings[0].name, None, "缺 name 应为 None");
}

#[test]
fn building_layer_survives_project_write_and_load() {
    // 经 `Project::write_buildings` → `load_buildings` 的**磁盘** round-trip
    // （serde 层 round-trip 之上再验一层真实文件 IO）。
    let dir = std::env::temp_dir().join(format!(
        "wv-building-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let rel = "maps/city/buildings.yaml";

    let project = wv_io::Project::create(&dir, "building-test").expect("建工程");
    let mut layer = BuildingLayer::new();
    layer.push(Building::rect(0.0, 0.0, 8.0, 8.0, 15.0));
    project.write_buildings(rel, &layer).expect("写盘");
    let back = project.load_buildings(rel).expect("读回");
    assert_eq!(back, layer, "磁盘 round-trip 应无损");

    std::fs::remove_dir_all(&dir).ok();
}
