//! Place 地界（`geometry/place/`）与标记字段的读写验证。
//!
//! 存储形态与势力几何（`geometry/faction/`）逐字同构：
//! 每条 [`GeometrySpan`] 一个 YAML，文件名用合成 id
//! `{map}__{owner}__{from}.yaml`（§4.5 每实体一文件；几何无独立 id 的
//! IO 层妥协见 `save_entities` 文档）。标记字段随
//! `distances/places/<id>.yaml` round-trip。
//!
//! 验证（ADR-0032）：
//! 1. 词汇收口：`biome` 正常写回；旧 `feature` **只读不写**（迁移窗口）；
//! 2. Project 级 save（`wants("distances/places")` + `wants("geometry/place")`）
//!    → load round-trip，含一条开放区间地界；
//! 3. 加载迁移：旧 YAML 的 `feature` 在 load 时迁入 `biome`，证据驻留内存、
//!    首次保存后收敛（`feature` 键消失）；
//! 4. 零迁移：无 `geometry/place/` 目录的既有工程加载得空 Vec。

use wv_core::distance::Place;
use wv_core::map::{GeometryData, GeometrySpan};
use wv_core::{Id, TimeSpan, World};

fn sample_place(biome: Option<u8>) -> Place {
    Place {
        id: Id::new_unchecked("place_linan"),
        name: "临安".into(),
        feature: None,
        biome,
        envelope: None,
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

/// ADR-0032 D1：旧 `feature` 字段**只读不写**——旧 YAML 照常读入（迁移窗口），
/// 序列化一律不再输出 feature 键（写回只写 biome）。
#[test]
fn legacy_feature_field_is_read_but_never_written() {
    let place = sample_place(Some(1));
    let text = serde_yaml::to_string(&place).expect("序列化");
    assert!(text.contains("biome: 1"), "biome 应正常写回，实得:\n{text}");

    let legacy = "id: place_linan\nname: 临安\nfeature: 4\n";
    let back: Place = serde_yaml::from_str(legacy).expect("旧 YAML 必须照常加载");
    assert_eq!(back.feature, Some(4), "读取侧：旧 feature 字段保留");
    assert_eq!(back.biome, None, "纯 serde 层不做迁移（迁移在 io 加载层）");

    let out = serde_yaml::to_string(&back).expect("序列化");
    assert!(
        !out.contains("feature"),
        "写入侧：feature 不得再落盘（ADR-0032 D1），实得:\n{out}"
    );
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
    world.distances.places.push(sample_place(Some(1)));
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
        sample_place(Some(1)),
        "Place（含 biome=Some(1)）round-trip 应无损"
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
    world.distances.places.push(sample_place(Some(1)));
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

/// ADR-0032 D3：旧 YAML 的 `feature: 4` 在 load 时迁移为 `biome=1`（森林），
/// feature 原值驻留内存作为迁移提示的证据（`detect_feature_migration_hints`）。
#[test]
fn legacy_feature_migrates_to_biome_on_load_and_keeps_evidence() {
    let tmp = TempDir::new("migrate");
    let proj = open_min_project(&tmp.0);
    let places_dir = tmp.0.join("distances/places");
    std::fs::create_dir_all(&places_dir).expect("建 places 目录");
    let yaml = "id: place_linan\nname: 临安\nmap: map_world\nx: 10.0\ny: 20.0\nw: 4.0\nh: 2.0\nfeature: 4\n";
    std::fs::write(places_dir.join("place_linan.yaml"), yaml).expect("写旧 YAML");

    let world = proj.load_world().expect("加载");
    assert_eq!(world.distances.places.len(), 1, "应加载出 1 个地点");
    let p = &world.distances.places[0];
    assert_eq!(
        p.biome,
        Some(1),
        "feature=4 应在加载时迁移为 biome=1（森林）"
    );
    assert_eq!(p.feature, Some(4), "内存保留 feature 原值作为迁移提示证据");
}

/// ADR-0032 D3/D8：迁移后的世界首次保存即物理收敛——磁盘只写 biome、不再写
/// feature；重载后旧字段从内存消失（迁移提示随之消散）。
#[test]
fn save_after_migration_writes_biome_and_drops_feature() {
    let tmp = TempDir::new("migrate_save");
    let mut proj = open_min_project(&tmp.0);
    let places_dir = tmp.0.join("distances/places");
    std::fs::create_dir_all(&places_dir).expect("建 places 目录");
    let yaml = "id: place_linan\nname: 临安\nmap: map_world\nfeature: 4\n";
    std::fs::write(places_dir.join("place_linan.yaml"), yaml).expect("写旧 YAML");

    let world = proj.load_world().expect("加载");
    proj.save_world(&world, &["distances/places".into()])
        .expect("保存");

    let text = std::fs::read_to_string(places_dir.join("place_linan.yaml")).expect("读回落盘");
    assert!(
        text.contains("biome: 1"),
        "迁移后的群系应落盘，实得:\n{text}"
    );
    assert!(
        !text.contains("feature"),
        "旧字段不得再写回（ADR-0032 D1），实得:\n{text}"
    );

    let back = proj.load_world().expect("重载");
    let p = &back.distances.places[0];
    assert_eq!(p.biome, Some(1), "群系保持不变");
    assert_eq!(p.feature, None, "首次保存后迁移收敛：旧字段从内存消失");
}
