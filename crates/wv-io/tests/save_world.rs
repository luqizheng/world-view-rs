//! 阶段七 v0.1：`Project::save_world` 集成测试。
//!
//! 策略：在临时目录里手工拼一个最小工程（`world.yaml` + 一个 faction），
//! 加载后修改，save，再加载一次，断言 round-trip 一致；另测增量写与删除传播。
//!
//! 阶段七 v0.1 测试覆盖：
//! 1. `save_then_load_preserves_faction` —— 写入新势力、load、断言相同
//! 2. `save_respects_dirty_paths_skips_other_dirs` —— 只重写脏目录
//! 3. `removed_entity_propagates_to_disk` —— 删除传播：删势力 → 磁盘文件消失
//! 4. `faction_geometry_round_trip_via_synthetic_id` —— 几何合成 id 不冲突
//! 5. `full_dirty_rewrite_unchanged_entities` —— dirty=空 → 全量写
//! 6. `save_load_save_idempotent` —— 两次落盘结果一致

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use wv_core::id::Id;
use wv_core::map::{GeometryData, GeometrySpan};
use wv_core::time::TimeSpan;
use wv_core::{Color, Faction, World};
use wv_io::error::IoResult;
use wv_io::Project;

/// 极简 tempdir 替代 `tempfile` crate——基于 `std::env::temp_dir()` +
/// 进程唯一前缀 + 单调计数器，每个测试目录**全局唯一**。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wv_io_save_test_{tag}_{pid}_{nanos}_{n}"));
        std::fs::create_dir_all(&dir).expect("tempdir");
        Self { path: dir }
    }
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 拼一个最小可加载的工程目录：
/// `world.yaml`（Project::open 必需）+ `factions/<id>.yaml`。
fn build_min_project(dir: &std::path::Path, factions: &[Faction]) -> IoResult<Project> {
    std::fs::create_dir_all(dir)?;
    let meta_yaml = r#"name: 测试工程
version: "0.1.0"
schema_version: 2
timeline_range:
  from: 1
root_map: map_main
"#;
    std::fs::write(dir.join("world.yaml"), meta_yaml)?;
    let f_dir = dir.join("factions");
    std::fs::create_dir_all(&f_dir)?;
    for f in factions {
        let text = serde_yaml::to_string(f).unwrap();
        std::fs::write(f_dir.join(format!("{}.yaml", f.id.as_str())), text)?;
    }
    Project::open(dir)
}

#[test]
fn save_then_load_preserves_faction() {
    let tmp = TempDir::new("save_then_load");
    let dir = tmp.path();
    let proj = build_min_project(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("初始加载");

    world.factions.push(Faction::new(
        Id::new_unchecked("faction_x"),
        "X",
        Color("#FF8800".into()),
    ));
    proj.save_world(&world, &["factions".into()]).expect("保存");

    let proj2 = Project::open(dir).expect("重开工程");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.factions.len(), 1);
    assert_eq!(world2.factions[0].id.as_str(), "faction_x");
    assert_eq!(world2.factions[0].name, "X");
    assert_eq!(world2.factions[0].color.as_str(), "#FF8800");
}

#[test]
fn save_respects_dirty_paths_skips_other_dirs() {
    let tmp = TempDir::new("dirty_paths");
    let dir = tmp.path();
    let f1 = Faction::new(
        Id::new_unchecked("faction_init"),
        "Init",
        Color("#000000".into()),
    );
    let proj = build_min_project(dir, &[f1]).expect("建工程");
    let hand_path = dir.join("factions").join("faction_hand.yaml");
    std::fs::write(
        &hand_path,
        "id: faction_hand\nname: Hand\ncolor: \"#ABCDEF\"\n",
    )
    .expect("写手改文件");

    let world = proj.load_world().expect("load");
    proj.save_world(&world, &["characters".into()])
        .expect("save 字符目录");
    assert!(
        hand_path.is_file(),
        "dirty 不含 factions 时，手改的势力文件应保留"
    );
}

#[test]
fn removed_entity_propagates_to_disk() {
    let tmp = TempDir::new("removed_entity");
    let dir = tmp.path();
    let f1 = Faction::new(
        Id::new_unchecked("faction_keep"),
        "Keep",
        Color("#000000".into()),
    );
    let f2 = Faction::new(
        Id::new_unchecked("faction_drop"),
        "Drop",
        Color("#111111".into()),
    );
    let proj = build_min_project(dir, &[f1, f2]).expect("建工程");
    let f_dir = dir.join("factions");
    assert!(f_dir.join("faction_keep.yaml").is_file());
    assert!(f_dir.join("faction_drop.yaml").is_file());

    let mut world = proj.load_world().expect("load");
    world.factions.retain(|f| f.id.as_str() != "faction_drop");
    proj.save_world(&world, &["factions".into()]).expect("save");

    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.factions.len(), 1, "应只剩 1 个");
    assert_eq!(world2.factions[0].id.as_str(), "faction_keep");
    assert!(
        !f_dir.join("faction_drop.yaml").is_file(),
        "删除应传播到磁盘"
    );
    assert!(f_dir.join("faction_keep.yaml").is_file(), "保留的应仍在");
}

#[test]
fn faction_geometry_round_trip_via_synthetic_id() {
    let tmp = TempDir::new("geo_round_trip");
    let dir = tmp.path();
    let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#222222".into()));
    let proj = build_min_project(dir, &[f]).expect("建工程");
    let mut world = proj.load_world().expect("load");
    world.faction_geometry.push(GeometrySpan {
        map: Id::new_unchecked("map_main"),
        owner: Id::new_unchecked("f1"),
        span: TimeSpan {
            from: 1,
            to: Some(100),
        },
        geometry: GeometryData::Polygon(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0]),
    });
    world.faction_geometry.push(GeometrySpan {
        map: Id::new_unchecked("map_main"),
        owner: Id::new_unchecked("f1"),
        span: TimeSpan {
            from: 200,
            to: None,
        },
        geometry: GeometryData::Polygon(vec![1.0, 1.0, 2.0, 1.0, 2.0, 2.0]),
    });
    proj.save_world(&world, &["geometry/faction".into()])
        .expect("save geometry");

    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.faction_geometry.len(), 2);
    let froms: Vec<i64> = world2
        .faction_geometry
        .iter()
        .map(|g| g.span.from)
        .collect();
    assert!(froms.contains(&1));
    assert!(froms.contains(&200));
}

#[test]
fn full_dirty_rewrite_unchanged_entities() {
    let tmp = TempDir::new("full_dirty");
    let dir = tmp.path();
    let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#333333".into()));
    let proj = build_min_project(dir, &[f]).expect("建工程");
    let world = proj.load_world().expect("load");
    proj.save_world(&world, &[]).expect("全量 save");
    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.factions.len(), 1);
}

#[test]
fn save_load_save_idempotent() {
    let tmp = TempDir::new("idempotent");
    let dir = tmp.path();
    let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#444444".into()));
    let proj = build_min_project(dir, &[f]).expect("建工程");
    let w1 = proj.load_world().expect("load 1");
    proj.save_world(&w1, &[]).expect("save 1");
    let proj2 = Project::open(dir).expect("open 2");
    let w2 = proj2.load_world().expect("load 2");
    proj2.save_world(&w2, &[]).expect("save 2");
    let w3 = Project::open(dir)
        .expect("open 3")
        .load_world()
        .expect("load 3");
    assert_eq!(w1.factions, w3.factions, "两次落盘后 faction 应一致");
}

#[allow(dead_code)]
fn _silence_unused(_w: &World) {}
