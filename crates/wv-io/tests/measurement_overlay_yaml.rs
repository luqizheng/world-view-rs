//! 测量叠加文件（`maps/<map_id>/measurements.yaml`）的读写验证（2026-10-14）。
//!
//! 与 `street_layer_yaml.rs` 同型：文档模型在 `wv-core::measurement`，
//! 落盘路径在 `Project::write_measurements_layers`（无公开 API，
//! 由 `save_world` 内部走虚拟脏路径触发）。

use wv_core::{
    map::{MapLevel, MapNode},
    Measurement, MeasurementKind, MeasurementOverlay, Point,
};

/// 造一张最小地图节点——`Project::open` 不会凭空造图，必须手工给 `world.maps` 塞一份。
fn stub_map_node(id: &str) -> MapNode {
    MapNode {
        id: wv_core::Id::new_unchecked(id),
        name: format!("图{id}"),
        level: MapLevel::World,
        parent: None,
        children: Vec::new(),
        icons: Vec::new(),
        layers: Vec::new(),
        extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
        trashed: false,
        trashed_at: None,
    }
}

#[test]
fn measurement_overlay_round_trips_through_yaml() {
    let mut ov = MeasurementOverlay::new();
    ov.add(
        Measurement::polyline(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)])
            .with_id(wv_core::Id::new_unchecked("m1")),
    )
    .unwrap();
    ov.add(
        Measurement::rectangle(Point::new(0.0, 0.0), Point::new(20.0, 30.0))
            .with_id(wv_core::Id::new_unchecked("m2"))
            .with_color("#ff8800".into()),
    )
    .unwrap();

    let text = serde_yaml::to_string(&ov).expect("序列化");
    let back: MeasurementOverlay = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, ov, "YAML round-trip 应无损");
    assert_eq!(back.measurements[0].kind, MeasurementKind::Polyline);
    assert_eq!(back.measurements[1].kind, MeasurementKind::Rectangle);
}

#[test]
fn empty_overlay_round_trips_to_empty() {
    let text = serde_yaml::to_string(&MeasurementOverlay::new()).expect("序列化");
    let back: MeasurementOverlay = serde_yaml::from_str(&text).expect("反序列化");
    assert!(back.is_empty());
}

#[test]
fn point_field_uses_explicit_mapping_not_tuple() {
    // 与 street 同款：Point YAML 形态是 `{x, y}` 映射，不是元组数组。
    let ov: MeasurementOverlay = serde_yaml::from_str(
        "measurements:\n  - id: m1\n    kind: rectangle\n    points:\n      - {x: 1, y: 2}\n      - {x: 3, y: 4}\n",
    )
    .expect("解析");
    assert_eq!(ov.len(), 1);
    assert_eq!(ov.measurements[0].points[0], Point::new(1.0, 2.0));
    assert_eq!(ov.measurements[0].points[1], Point::new(3.0, 4.0));
    assert_eq!(ov.measurements[0].kind, MeasurementKind::Rectangle);
}

#[test]
fn color_field_is_optional_and_skipped_when_absent() {
    let ov: MeasurementOverlay = serde_yaml::from_str(
        "measurements:\n  - id: m1\n    kind: polyline\n    points:\n      - {x: 0, y: 0}\n      - {x: 1, y: 1}\n",
    )
    .expect("解析");
    assert!(ov.measurements[0].color.is_none());
}

// ---------------------------------------------------------------------------
// 极简 tempdir + mini 工程：覆盖 save/load + 删除传播（write 走 `save_world`）。
// ---------------------------------------------------------------------------

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
        let dir = std::env::temp_dir().join(format!("wv_io_measure_test_{tag}_{pid}_{nanos}_{n}"));
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
    let meta_yaml = "name: 测量测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_main\n";
    std::fs::write(dir.join("world.yaml"), meta_yaml).expect("写 world.yaml");
    wv_io::Project::open(dir).expect("打开工程")
}

#[test]
fn save_world_writes_per_map_measurements_yaml_and_load_round_trips() {
    let tmp = TempDir::new("save_load");
    let mut project = open_min_project(&tmp.0);
    let mut world = project.load_world().expect("load 空工程");
    world.maps.push(stub_map_node("map_main"));

    // 内存里塞一份测量 → save_world 把它写到 maps/map_main/measurements.yaml。
    let mut ov = MeasurementOverlay::new();
    ov.add(
        Measurement::rectangle(Point::new(0.0, 0.0), Point::new(50.0, 20.0))
            .with_id(wv_core::Id::new_unchecked("rect_1")),
    )
    .unwrap();
    ov.add(
        Measurement::polyline(vec![
            Point::new(10.0, 10.0),
            Point::new(40.0, 10.0),
            Point::new(40.0, 50.0),
        ])
        .with_id(wv_core::Id::new_unchecked("line_1")),
    )
    .unwrap();
    world.set_measurement(wv_core::Id::new_unchecked("map_main"), ov.clone());

    project.save_world(&world, &[]).expect("save");
    let path = tmp.0.join("maps/map_main/measurements.yaml");
    assert!(path.is_file(), "应写出 {path:?}");

    // 重 load → overlay 应一致。
    let project2 = open_min_project(&tmp.0);
    let world2 = project2.load_world().expect("重 load");
    let back = world2
        .measurement_of(&wv_core::Id::new_unchecked("map_main"))
        .expect("应有 overlay");
    assert_eq!(back.measurements, ov.measurements);
    assert_eq!(back.measurements.len(), 2);
}

#[test]
fn save_world_skips_writing_when_overlay_is_empty() {
    let tmp = TempDir::new("empty_skip");
    let mut project = open_min_project(&tmp.0);
    let mut world = project.load_world().expect("load");
    world.maps.push(stub_map_node("map_main"));
    // 显式空 overlay。
    world.set_measurement(
        wv_core::Id::new_unchecked("map_main"),
        MeasurementOverlay::new(),
    );
    project.save_world(&world, &[]).expect("save");
    let path = tmp.0.join("maps/map_main/measurements.yaml");
    assert!(!path.is_file(), "空 overlay 不写盘（V1：不替作者造东西）");
}

#[test]
fn save_world_deletes_stale_measurement_file_when_overlay_becomes_empty() {
    let tmp = TempDir::new("delete_propagation");
    let mut project = open_min_project(&tmp.0);

    // 阶段 1：写一份非空 overlay。
    let mut world = project.load_world().expect("load");
    world.maps.push(stub_map_node("map_main"));
    let mut ov = MeasurementOverlay::new();
    ov.add(
        Measurement::rectangle(Point::new(0.0, 0.0), Point::new(1.0, 1.0))
            .with_id(wv_core::Id::new_unchecked("rect_1")),
    )
    .unwrap();
    world.set_measurement(wv_core::Id::new_unchecked("map_main"), ov);
    project.save_world(&world, &[]).expect("save");
    let path = tmp.0.join("maps/map_main/measurements.yaml");
    assert!(path.is_file());

    // 阶段 2：清空 overlay 并重新保存 → 文件应被删除。
    world.set_measurement(
        wv_core::Id::new_unchecked("map_main"),
        MeasurementOverlay::new(),
    );
    project.save_world(&world, &[]).expect("save 2");
    assert!(!path.is_file(), "空 overlay 应触发删除传播");
}
