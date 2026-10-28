//! 势力图切蛋糕分区（ADR-0079）的落盘 / 读回验证。
//!
//! 存储形态与势力几何（`geometry/faction/`）同构：每份
//! [`FactionPartition`] 一个 YAML，落 `partitions/<map>__<layer>.yaml`
//! （§4.5 每实体一文件；分区无独立 id，键 = map + layer 的 IO 层合成，
//! 与 `geometry/place` 的合成文件名同一妥协）。dirty 键 = `"partitions"`。
//!
//! 验证：
//! 1. Project 级 save（`wants("partitions")`）→ load round-trip，
//!    含开放区间 span 与无主脸（skip 字段）；
//! 2. 零迁移：无 `partitions/` 目录的既有工程加载得空 Vec（V1 不报错）。

use wv_core::model::faction_partition::{
    FactionPartition, PartitionFace, PartitionSpan, PartitionVertex,
};
use wv_core::{Id, World};

fn sample_partition() -> FactionPartition {
    FactionPartition {
        map: Id::new_unchecked("map_world"),
        layer: Id::new_unchecked("layer_borders"),
        spans: vec![
            PartitionSpan {
                from: 0,
                to: Some(10),
                vertices: vec![
                    PartitionVertex {
                        id: 0,
                        xy: [0.0, 0.0],
                    },
                    PartitionVertex {
                        id: 1,
                        xy: [10.0, 0.0],
                    },
                    PartitionVertex {
                        id: 2,
                        xy: [10.0, 10.0],
                    },
                    PartitionVertex {
                        id: 3,
                        xy: [0.0, 10.0],
                    },
                    PartitionVertex {
                        id: 4,
                        xy: [5.0, 6.0],
                    },
                    PartitionVertex {
                        id: 5,
                        xy: [6.0, 5.0],
                    },
                    PartitionVertex {
                        id: 6,
                        xy: [5.0, 4.0],
                    },
                ],
                faces: vec![
                    // 无主脸带洞（湖）：owner None + holes 走 serde skip。
                    PartitionFace {
                        id: 0,
                        owner: None,
                        kind: None,
                        ring: vec![0, 1, 2, 3],
                        holes: vec![vec![4, 5, 6]],
                    },
                    PartitionFace {
                        id: 1,
                        owner: Some(Id::new_unchecked("faction_a")),
                        kind: None,
                        ring: vec![0, 1, 2],
                        holes: Vec::new(),
                    },
                ],
            },
            // 开放区间段：to == None（skip 序列化）。
            PartitionSpan {
                from: 20,
                to: None,
                vertices: vec![
                    PartitionVertex {
                        id: 0,
                        xy: [0.0, 0.0],
                    },
                    PartitionVertex {
                        id: 1,
                        xy: [8.0, 0.0],
                    },
                    PartitionVertex {
                        id: 2,
                        xy: [8.0, 8.0],
                    },
                    PartitionVertex {
                        id: 3,
                        xy: [0.0, 8.0],
                    },
                ],
                faces: vec![PartitionFace {
                    id: 0,
                    owner: Some(Id::new_unchecked("faction_b")),
                    kind: None,
                    ring: vec![0, 1, 2, 3],
                    holes: Vec::new(),
                }],
            },
        ],
    }
}

/// 极简 tempdir（与 `place_territory_yaml.rs` 同法）。
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
        let dir =
            std::env::temp_dir().join(format!("wv_io_partition_test_{tag}_{pid}_{nanos}_{n}"));
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
        "name: 分区测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_world\n";
    std::fs::write(dir.join("world.yaml"), meta_yaml).expect("写 world.yaml");
    wv_io::Project::open(dir).expect("打开工程")
}

#[test]
fn partition_save_load_round_trips() {
    let tmp = TempDir::new("roundtrip");
    let mut proj = open_min_project(&tmp.0);

    let mut world = World::default();
    world.partitions.push(sample_partition());

    proj.save_world(&world, &["partitions".into()])
        .expect("保存分区");

    let part_dir = tmp.0.join("partitions");
    assert!(part_dir.is_dir(), "分区应落在 partitions/ 目录");
    assert!(
        part_dir.join("map_world__layer_borders.yaml").is_file(),
        "文件名应为 {{map}}__{{layer}}.yaml"
    );

    let back = proj.load_world().expect("读回");
    assert_eq!(back.partitions.len(), 1, "应加载出 1 份分区");
    assert_eq!(
        back.partitions[0],
        sample_partition(),
        "分区 round-trip 应无损（含开放区间、无主脸、洞环）"
    );
}

#[test]
fn missing_partition_files_silently_default() {
    // 零迁移：既有工程没有 partitions/ 目录——加载不报错、得空 Vec（V1）。
    let tmp = TempDir::new("legacy");
    let proj = open_min_project(&tmp.0);

    let back = proj.load_world().expect("无 partitions/ 也应能打开");
    assert!(
        back.partitions.is_empty(),
        "缺目录应得空 Vec，实得 {} 份",
        back.partitions.len()
    );
}

#[test]
fn deleted_partition_is_removed_from_disk_on_save() {
    // 删除传播：磁盘上存在但 World 不含的分区文件被删（save_entities 契约）。
    let tmp = TempDir::new("delete");
    let mut proj = open_min_project(&tmp.0);

    let mut world = World::default();
    world.partitions.push(sample_partition());
    proj.save_world(&world, &["partitions".into()])
        .expect("首存");

    world.partitions.clear();
    proj.save_world(&world, &["partitions".into()])
        .expect("清空后再存");

    let back = proj.load_world().expect("读回");
    assert!(
        back.partitions.is_empty(),
        "删除传播后应读回空 Vec，实得 {} 份",
        back.partitions.len()
    );
}
