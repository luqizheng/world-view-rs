//! 街道图层文件（`Layer.source` 指向的 YAML）的读写验证（ADR-0010）。
//!
//! 与 `geographic_source_yaml.rs` 同型：文档模型在 `wv-core::street`，
//! 读写入口在 `Project`。round-trip 按 AGENTS §7.2 必须无损。

use wv_core::{Point, RoadClass, StreetLayer, StreetSegment, TimeSpan};

#[test]
fn street_layer_round_trips_through_yaml() {
    let mut layer = StreetLayer::new();
    layer.push(StreetSegment::straight(
        RoadClass::Primary,
        10.0,
        20.0,
        30.0,
        40.0,
    ));
    layer.push(StreetSegment {
        class: RoadClass::Path,
        points: vec![
            Point::new(0.0, 0.0),
            Point::new(5.0, 5.0),
            Point::new(5.0, 9.0),
        ],
        name: Some("中山路".into()),
        time_span: Some(TimeSpan::new(1450, 1900).unwrap()),
    });

    let text = serde_yaml::to_string(&layer).expect("序列化");
    let back: StreetLayer = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, layer, "YAML round-trip 应无损");
}

#[test]
fn segment_without_class_defaults_to_residential() {
    // Point 的 YAML 形态是映射（与 `extent.origin` 一致），不是二元数组。
    let layer: StreetLayer =
        serde_yaml::from_str("segments:\n  - points:\n      - {x: 1, y: 2}\n      - {x: 3, y: 4}")
            .expect("解析");
    assert_eq!(layer.len(), 1, "应解析出 1 段");
    assert_eq!(
        layer.segments[0].class,
        RoadClass::Residential,
        "缺 class 的段应取缺省类别"
    );
    assert_eq!(
        layer.segments[0].points,
        vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)]
    );
}

#[test]
fn segment_without_time_span_field_is_zero_migration_and_omitted_on_write() {
    // 既有工程 YAML 无 time_span 字段（ADR-0015 零迁移回归线）：
    // 解析得 None，且 None 序列化时不写回字段（保持既有文件干净）。
    let layer: StreetLayer =
        serde_yaml::from_str("segments:\n  - points:\n      - {x: 1, y: 2}\n      - {x: 3, y: 4}")
            .expect("既有 YAML 应兼容解析");
    assert!(
        layer.segments[0].time_span.is_none(),
        "未声明 time_span 的段应解析为 None"
    );

    let text = serde_yaml::to_string(&layer).expect("序列化");
    assert!(
        !text.contains("time_span"),
        "None 不应写出 time_span 字段（既有工程零污染）：\n{text}"
    );
    let back: StreetLayer = serde_yaml::from_str(&text).expect("round-trip");
    assert_eq!(back, layer, "None 形态 round-trip 应无损");
}

#[test]
fn segment_time_span_round_trips_with_open_window() {
    // 开放窗口（to: None）是「至今有效」的常用写法，必须可逆。
    let mut layer = StreetLayer::new();
    layer.push(StreetSegment {
        class: RoadClass::Primary,
        points: vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)],
        name: Some("中山路".into()),
        time_span: Some(TimeSpan::open(1500)),
    });
    let text = serde_yaml::to_string(&layer).expect("序列化");
    let back: StreetLayer = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(
        back.segments[0].time_span,
        Some(TimeSpan::open(1500)),
        "开放窗口 round-trip 应无损"
    );
}

#[test]
fn empty_layer_serializes_to_an_empty_document() {
    let text = serde_yaml::to_string(&StreetLayer::new()).expect("序列化");
    let back: StreetLayer = serde_yaml::from_str(&text).expect("反序列化");
    assert!(back.is_empty(), "空图层 round-trip 后仍为空");
}

/// 极简 tempdir（与 save_world.rs 同法：进程唯一 + 单调计数器）。
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wv_io_street_test_{tag}_{pid}_{nanos}_{n}"));
        std::fs::create_dir_all(&dir).expect("tempdir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_min_project(dir: &std::path::Path) -> wv_io::Project {
    let meta_yaml = "name: 街道测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_main\n";
    std::fs::write(dir.join("world.yaml"), meta_yaml).expect("写 world.yaml");
    wv_io::Project::open(dir).expect("打开工程")
}

#[test]
fn project_writes_then_loads_a_street_layer_in_a_new_directory() {
    let tmp = TempDir::new("roundtrip");
    let project = open_min_project(&tmp.0);

    let mut layer = StreetLayer::new();
    layer.push(StreetSegment::straight(
        RoadClass::Secondary,
        12.5,
        3.0,
        88.25,
        40.0,
    ));
    layer.push(StreetSegment {
        class: RoadClass::Service,
        points: vec![Point::new(0.0, 0.0), Point::new(7.0, 1.0)],
        name: None,
        time_span: None,
    });

    // 目标目录 `maps/map_city/` 尚不存在——首次保存应自建。
    let written = project
        .write_streets("maps/map_city/roads.yaml", &layer)
        .expect("写街道图层");
    assert!(
        tmp.0.join("maps/map_city/roads.yaml").is_file(),
        "文件应落在 {written}"
    );

    let back = project
        .load_streets("maps/map_city/roads.yaml")
        .expect("读回");
    assert_eq!(back, layer, "经工程读写的街道图层应与原数据一致");
}

#[test]
fn project_loading_a_missing_street_file_reports_the_path() {
    let tmp = TempDir::new("missing");
    let project = open_min_project(&tmp.0);
    let err = project
        .load_streets("maps/map_city/roads.yaml")
        .expect_err("文件不存在应报错");
    let msg = err.to_string();
    assert!(msg.contains("roads.yaml"), "错误信息应含路径，实际: {msg}");
}
