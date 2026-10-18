//! Place 地界（`geometry/place/`）与 `Place.feature` 的读写验证。
//!
//! 存储形态与势力几何（`geometry/faction/`）逐字同构：
//! 每条 [`GeometrySpan`] 一个 YAML，文件名用合成 id
//! `{map}__{owner}__{from}.yaml`（§4.5 每实体一文件；几何无独立 id 的
//! IO 层妥协见 `save_entities` 文档）。`Place.feature` 随
//! `distances/places/<id>.yaml` 自动 round-trip。
//!
//! 验证：
//! 1. `Place.feature` 的 YAML round-trip（`Some(4)`）与缺席语义（`None` 不写键）；
//! 2. Project 级 save（`wants("distances/places")` + `wants("geometry/place")`）
//!    → load round-trip，含一条开放区间地界；
//! 3. 零迁移：无 `geometry/place/` 目录的既有工程加载得空 Vec。

use wv_core::distance::Place;
use wv_core::map::{GeometryData, GeometrySpan};
use wv_core::{Id, TimeSpan, World};

fn sample_place(feature: Option<u8>) -> Place {
    Place {
        id: Id::new_unchecked("place_linan"),
        name: "临安".into(),
        feature,
        map: Some(Id::new_unchecked("map_world")),
        x: Some(10.0),
        y: Some(20.0),
        w: Some(4.0),
        h: Some(2.0),
        rank: None,
    }
}

fn sample_geometry() -> GeometrySpan {
    GeometrySpan {
        map: Id::new_unchecked("map_world"),
        owner: Id::new_unchecked("place_linan"),
        // 开放区间：至今有效（P7 时间轴驱动的常态表达）。
        span: TimeSpan { from: 1, to: None },
        geometry: GeometryData::Polygon(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]),
        relation: None,
    }
}

#[test]
fn place_feature_round_trips_through_yaml() {
    let place = sample_place(Some(4));
    let text = serde_yaml::to_string(&place).expect("序列化");
    assert!(
        text.contains("feature: 4"),
        "Some(4) 应写成 feature: 4，实得:\n{text}"
    );
    let back: Place = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, place, "带 feature 的 Place YAML round-trip 应无损");

    // None = 普通地点：不写键，旧工程与旧读取端都不受影响。
    let plain = sample_place(None);
    let text = serde_yaml::to_string(&plain).expect("序列化");
    assert!(
        !text.contains("feature"),
        "feature: None 不应输出 feature 键，实得:\n{text}"
    );
    let back: Place = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, plain, "无 feature 键的 Place YAML round-trip 应无损");
}

/// 极简 tempdir（与 `region_yaml.rs` 同法）。
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
        let dir = std::env::temp_dir().join(format!(
            "wv_io_place_territory_test_{tag}_{pid}_{nanos}_{n}"
        ));
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
    let meta_yaml =
        "name: 地界测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_main\n";
    std::fs::write(dir.join("world.yaml"), meta_yaml).expect("写 world.yaml");
    wv_io::Project::open(dir).expect("打开工程")
}

#[test]
fn project_saves_then_loads_place_geometry_round_trip() {
    let tmp = TempDir::new("roundtrip");
    let mut proj = open_min_project(&tmp.0);

    let mut world = World::default();
    world.distances.places.push(sample_place(Some(4)));
    world.place_geometry.push(sample_geometry());

    proj.save_world(
        &world,
        &["distances/places".into(), "geometry/place".into()],
    )
    .expect("保存地点与地界");

    let geo_dir = tmp.0.join("geometry/place");
    assert!(geo_dir.is_dir(), "地界应落在 geometry/place/ 目录");
    let files: Vec<_> = std::fs::read_dir(&geo_dir)
        .expect("读 geometry/place/")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".yaml"))
        .collect();
    assert_eq!(
        files.len(),
        1,
        "1 条 GeometrySpan 应落 1 个文件，实得 {files:?}"
    );
    assert!(
        geo_dir.join("map_world__place_linan__1.yaml").is_file(),
        "文件名应为 {{map}}__{{owner}}__{{from}}.yaml，实得 {files:?}"
    );

    let back = proj.load_world().expect("读回");
    assert_eq!(back.distances.places.len(), 1, "应加载出 1 个地点");
    assert_eq!(
        back.distances.places[0],
        sample_place(Some(4)),
        "Place（含 feature=Some(4)）round-trip 应无损"
    );
    assert_eq!(back.place_geometry.len(), 1, "应加载出 1 条地界");
    assert_eq!(
        back.place_geometry[0],
        sample_geometry(),
        "GeometrySpan（开放区间）round-trip 应无损"
    );
}

#[test]
fn project_without_geometry_place_dir_loads_empty_vec() {
    // 零迁移：既有工程没有 geometry/place/ 目录——加载不报错、得空 Vec。
    let tmp = TempDir::new("legacy");
    let mut proj = open_min_project(&tmp.0);

    let mut world = World::default();
    world.distances.places.push(sample_place(Some(4)));
    proj.save_world(&world, &["distances/places".into()])
        .expect("只保存地点");
    assert!(
        !tmp.0.join("geometry/place").exists(),
        "未保存地界时不应凭空创建 geometry/place/"
    );

    let back = proj.load_world().expect("无 geometry/place 目录应能加载");
    assert_eq!(
        back.distances.places.len(),
        1,
        "地点应正常加载，实得 {} 个",
        back.distances.places.len()
    );
    assert!(
        back.place_geometry.is_empty(),
        "无 geometry/place 目录的工程应得空 Vec，实得 {} 条",
        back.place_geometry.len()
    );
}
