//! `Project::create`：新建最小工程的集成测试（v0.3 §B——「新建 → 首次保存」断链修复）。
//!
//! 覆盖：
//! 1. `create_then_open_round_trips_meta` —— create 后 open 能读回元数据
//! 2. `create_then_save_world_then_load_round_trips` —— create + 全量保存 + 重载一致
//! 3. `create_refuses_to_clobber_existing_project` —— 已有工程拒绝覆盖

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use wv_core::id::Id;
use wv_core::{Color, Faction, World};
use wv_io::error::IoError;
use wv_io::Project;

/// 极简 tempdir——与 `save_world.rs` 同款，避免引 `tempfile` 依赖。
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
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wv_io_create_test_{tag}_{pid}_{nanos}_{n}"));
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

#[test]
fn create_then_open_round_trips_meta() {
    let tmp = TempDir::new("meta_round_trip");
    let proj = Project::create(tmp.path(), "新工程").expect("create");
    assert_eq!(proj.meta().name, "新工程");
    assert_eq!(proj.meta().schema_version, 2, "schema 应取当前版本");

    let proj2 = Project::open(tmp.path()).expect("create 产出的目录必须能被 open");
    assert_eq!(proj2.meta().name, "新工程");
}

#[test]
fn create_then_save_world_then_load_round_trips() {
    let tmp = TempDir::new("save_round_trip");
    let mut proj = Project::create(tmp.path(), "round trip").expect("create");
    let mut world = World::default();
    world.factions.push(Faction::new(
        Id::new_unchecked("faction_a"),
        "A",
        Color("#112233".into()),
    ));
    // 空 dirty 表 = 全量路径（编辑器首次保存走的就是它）。
    proj.save_world(&world, &[]).expect("首次全量保存");

    let world2 = Project::open(tmp.path())
        .and_then(|p| p.load_world())
        .expect("重载");
    assert_eq!(world2.factions.len(), 1);
    assert_eq!(world2.factions[0].id.as_str(), "faction_a");
}

#[test]
fn create_refuses_to_clobber_existing_project() {
    let tmp = TempDir::new("no_clobber");
    Project::create(tmp.path(), "第一份").expect("首次 create 应成功");
    let err = Project::create(tmp.path(), "第二份").expect_err("已有工程必须拒绝覆盖");
    assert!(
        matches!(err, IoError::AlreadyAProject(_)),
        "应报 AlreadyAProject，实际 {err}"
    );
}
