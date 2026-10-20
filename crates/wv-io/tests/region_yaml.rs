//! 命名区域实体（`Region`）的读写验证（ADR-0017 §2.2）。
//!
//! 存储 = `regions/<id>.yaml`（§4.5 每实体一文件），几何内联本图局部
//! 坐标多边形。验证：
//! 1. YAML round-trip 无损（含多边形顶点）；
//! 2. Project 级 save（`wants("regions")`）→ load round-trip；
//! 3. 零迁移：无 `regions/` 目录的既有工程加载得空 Vec。

use wv_core::{
    model::region::{Region, RegionKind},
    Id, World,
};

fn sample_region() -> Region {
    Region {
        id: Id::new_unchecked("region_guanzhong"),
        name: "关中平原".into(),
        map: Id::new_unchecked("map_world"),
        polygon: vec![0.0, 0.0, 40.0, 0.0, 40.0, 30.0, 0.0, 30.0],
        holes: Vec::new(),
        kind: RegionKind::Area,
        parent: None,
        overlay: None,
    }
}

/// 自然区域（ADR-0045）：带洞 + 类别 + 父区 + 覆盖物键的全字段样本。
fn sample_nature_region() -> Region {
    Region {
        id: Id::new_unchecked("region_miyansenlin"),
        name: "迷幻深林".into(),
        map: Id::new_unchecked("map_world"),
        polygon: vec![0.0, 0.0, 40.0, 0.0, 40.0, 30.0, 0.0, 30.0],
        holes: vec![vec![10.0, 10.0, 20.0, 10.0, 20.0, 20.0, 10.0, 20.0]],
        kind: RegionKind::Nature,
        parent: Some(Id::new_unchecked("region_mountains")),
        overlay: Some("conifer".into()),
    }
}

#[test]
fn region_round_trips_through_yaml() {
    let region = sample_region();
    let text = serde_yaml::to_string(&region).expect("序列化");
    let back: Region = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, region, "Region YAML round-trip 应无损");
}

/// 自然区域（ADR-0045）：洞环 / 类别 / 父区 / 覆盖物键全字段 round-trip。
#[test]
fn nature_region_round_trips_with_holes_kind_parent_and_overlay() {
    let region = sample_nature_region();
    let text = serde_yaml::to_string(&region).expect("序列化");
    let back: Region = serde_yaml::from_str(&text).expect("反序列化");
    assert_eq!(back, region, "自然区域 YAML round-trip 应无损");
    assert_eq!(back.kind, RegionKind::Nature);
    assert_eq!(back.holes.len(), 1, "洞环应保留");
    assert_eq!(
        back.parent.as_ref().map(|p| p.as_str()),
        Some("region_mountains")
    );
    assert_eq!(back.overlay.as_deref(), Some("conifer"));
    assert_eq!(back.declared_overlay(), Some("conifer"));
}

/// 旧 YAML（无新字段）零迁移：缺省即 `Area` 语义。
#[test]
fn legacy_region_yaml_without_new_fields_loads_as_area() {
    let legacy = "id: region_legacy\nname: 关中\nmap: map_world\npolygon: [0, 0, 4, 0, 4, 4]\n";
    let region: Region = serde_yaml::from_str(legacy).expect("旧 YAML 应可读");
    assert_eq!(region.kind, RegionKind::Area);
    assert!(region.holes.is_empty());
    assert!(region.parent.is_none());
    assert!(region.overlay.is_none());
    assert!(region.declared_overlay().is_none());
}

/// 极简 tempdir（与 save_world.rs 同法）。
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
        let dir = std::env::temp_dir().join(format!("wv_io_region_test_{tag}_{pid}_{nanos}_{n}"));
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
        "name: 区域测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_main\n";
    std::fs::write(dir.join("world.yaml"), meta_yaml).expect("写 world.yaml");
    wv_io::Project::open(dir).expect("打开工程")
}

#[test]
fn project_saves_then_loads_regions_round_trip() {
    let tmp = TempDir::new("roundtrip");
    let mut proj = open_min_project(&tmp.0);

    let mut world = World::default();
    world.regions.push(sample_region());

    proj.save_world(&world, &["regions".into()])
        .expect("保存区域");
    assert!(
        tmp.0.join("regions/region_guanzhong.yaml").is_file(),
        "区域应落在 regions/<id>.yaml"
    );

    let back = proj.load_world().expect("读回");
    assert_eq!(back.regions.len(), 1, "应加载出 1 个区域");
    assert_eq!(back.regions[0], sample_region(), "round-trip 应无损");
}

#[test]
fn project_without_regions_dir_loads_empty_vec() {
    // 零迁移：既有工程没有 regions/ 目录——加载不报错、得空 Vec。
    let tmp = TempDir::new("legacy");
    let proj = open_min_project(&tmp.0);
    let world = proj.load_world().expect("无 regions 目录应能加载");
    assert!(world.regions.is_empty(), "无 regions 目录的工程应得空 Vec");
}
