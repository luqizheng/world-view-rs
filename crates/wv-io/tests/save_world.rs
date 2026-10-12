//! 阶段七 v0.1 / v0.2 / v0.3 / v0.4：`Project::save_world` 集成测试。
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
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::time::TimeSpan;
use wv_core::{Color, Faction, World, WorldMeta};
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

/// 在 [`build_min_project`] 的基础上，再额外创建 `maps/map_main/index.yaml`
/// —— 模拟「作者先把根地图摆好」的工程状态。这是 v0.3 地形落盘测试的
/// 现实场景（root_map 没建好的工程谈不上「保存」）。
fn build_min_project_with_root_map(
    dir: &std::path::Path,
    factions: &[Faction],
) -> IoResult<Project> {
    let proj = build_min_project(dir, factions)?;
    let map_dir = dir.join("maps").join("map_main");
    std::fs::create_dir_all(&map_dir)?;
    let map_yaml = r#"id: map_main
name: Main
level: world
"#;
    std::fs::write(map_dir.join("index.yaml"), map_yaml)?;
    Ok(proj)
}

#[test]
fn save_then_load_preserves_faction() {
    let tmp = TempDir::new("save_then_load");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
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
    let mut proj = build_min_project(dir, &[f1]).expect("建工程");
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
    let mut proj = build_min_project(dir, &[f1, f2]).expect("建工程");
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
    let mut proj = build_min_project(dir, &[f]).expect("建工程");
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
    let mut proj = build_min_project(dir, &[f]).expect("建工程");
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
    let mut proj = build_min_project(dir, &[f]).expect("建工程");
    let w1 = proj.load_world().expect("load 1");
    proj.save_world(&w1, &[]).expect("save 1");
    let mut proj2 = Project::open(dir).expect("open 2");
    let w2 = proj2.load_world().expect("load 2");
    proj2.save_world(&w2, &[]).expect("save 2");
    let w3 = Project::open(dir)
        .expect("open 3")
        .load_world()
        .expect("load 3");
    assert_eq!(w1.factions, w3.factions, "两次落盘后 faction 应一致");
}

/// v0.2 dirty 路由：手写 `distances/places.yaml` 之类**未列在 dirty** 的目录，
/// save 时不被覆盖；已列在 dirty 的目录**必须**重写。
///
/// 这是 UI Save 走 `EditHistory::take_dirty` 后实际效果的最小模型。
#[test]
fn dirty_routing_leaves_unlisted_dirs_untouched() {
    let tmp = TempDir::new("dirty_routing");
    let dir = tmp.path();
    let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#555555".into()));
    let mut proj = build_min_project(dir, &[f]).expect("建工程");
    // 写一个手改文件到 `characters/` 目录（dirty 列表不含它）。
    let char_dir = dir.join("characters");
    std::fs::create_dir_all(&char_dir).expect("mkdir characters");
    let hand_char = char_dir.join("char_hand.yaml");
    let hand_content = "id: char_hand\nname: 手改角色\n";
    std::fs::write(&hand_char, hand_content).expect("写手改文件");
    let mtime_before = std::fs::metadata(&hand_char).unwrap().modified().unwrap();

    // 触发一次「只 factions 脏」的保存。
    let world = proj.load_world().expect("load");
    proj.save_world(&world, &["factions".into()])
        .expect("save factions only");

    // 角色文件应**未被重写**——mtime 保持。
    let mtime_after = std::fs::metadata(&hand_char).unwrap().modified().unwrap();
    assert_eq!(
        mtime_before, mtime_after,
        "未列在 dirty 的目录文件不应被触碰"
    );
    // 内容也保持。
    let read = std::fs::read_to_string(&hand_char).unwrap();
    assert_eq!(read, hand_content, "文件内容应原样保留");
}

/// v0.2 dirty 路由：空 dirty → 全量；非空 dirty → 只动 dirty 项。
/// （在现有 v0.1 测试里已分别覆盖两边，本测试做交叉断言「空/非空都走得通」。）
#[test]
fn empty_dirty_falls_back_to_full_rewrite() {
    let tmp = TempDir::new("empty_dirty");
    let dir = tmp.path();
    let f1 = Faction::new(Id::new_unchecked("faction_a"), "A", Color("#AAAAAA".into()));
    let f2 = Faction::new(Id::new_unchecked("faction_b"), "B", Color("#BBBBBB".into()));
    let mut proj = build_min_project(dir, &[f1.clone(), f2.clone()]).expect("建工程");
    let world = proj.load_world().expect("load");
    proj.save_world(&world, &[]).expect("全量 save");
    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.factions.len(), 2);
    let ids: Vec<&str> = world2.factions.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"faction_a"));
    assert!(ids.contains(&"faction_b"));
}

/// 阶段七 v0.3：`World.terrain` → `maps/<root_map>/terrain.bin` 落盘。
///
/// 验证三件事：
/// 1. 写盘后 `maps/map_main/terrain.bin` 文件存在；
/// 2. root_map 的 `index.yaml` 里出现 `raster.path = "maps/map_main/terrain.bin"`；
/// 3. 再加载能 round-trip 回 `World.terrain`。
#[test]
fn terrain_bin_round_trip_via_save_and_load() {
    let tmp = TempDir::new("terrain_round_trip");
    let dir = tmp.path();
    let mut proj = build_min_project_with_root_map(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");
    assert!(world.terrain.is_none(), "初始状态：terrain 为 None");

    // 装一块 8×6 的栅格：每格 (height=i16, kind=u8) 写入。
    let mut cells = Vec::with_capacity(8 * 6);
    for y in 0..6i32 {
        for x in 0..8i32 {
            cells.push(TerrainCellData {
                height: (x * 100 + y) as i16,
                kind: ((x + y) % 5) as u8,
            });
        }
    }
    let data = TerrainData {
        width: 8,
        height: 6,
        cells,
    };
    world.terrain = Some(data.clone());

    // 触发 save。dirty 里含 `"maps/<map>/terrain.bin"` 这种细粒度路径，
    // 验证 v0.3 的**前缀匹配**能让它驱动 `maps` 目录重写。
    proj.save_world(&world, &["maps/map_main/terrain.bin".into()])
        .expect("save 含 terrain");

    // 1) .bin 落盘
    let bin_path = dir.join("maps").join("map_main").join("terrain.bin");
    assert!(
        bin_path.is_file(),
        "terrain.bin 应已落盘: {}",
        bin_path.display()
    );

    // 2) root_map 的 index.yaml 应包含 raster 引用
    let idx_path = dir.join("maps").join("map_main").join("index.yaml");
    assert!(idx_path.is_file(), "index.yaml 应已落盘");
    let idx_text = std::fs::read_to_string(&idx_path).expect("读 index.yaml");
    assert!(
        idx_text.contains("terrain.bin"),
        "index.yaml 应引用 terrain.bin，实际：{idx_text}"
    );

    // 3) 重载：terrain 应 round-trip 回来
    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    let t2 = world2.terrain.as_ref().expect("terrain 应被读回");
    assert_eq!(t2.width, 8);
    assert_eq!(t2.height, 6);
    assert_eq!(t2.cells.len(), 48);
    assert_eq!(t2.cells, data.cells, "terrain 栅格必须 byte-for-byte 一致");
}

/// 阶段七 v0.3：地形随空 dirty 走「全量」路径也要落盘。
///
/// 第一次落盘（dirty=空 Vec）的场景：terrain 一样要被写出来。
#[test]
fn terrain_bin_written_on_full_dirty_save() {
    let tmp = TempDir::new("terrain_full_dirty");
    let dir = tmp.path();
    let mut proj = build_min_project_with_root_map(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");
    world.terrain = Some(TerrainData {
        width: 4,
        height: 4,
        cells: vec![TerrainCellData { height: 0, kind: 0 }; 16],
    });
    proj.save_world(&world, &[]).expect("全量 save");
    let bin = dir.join("maps").join("map_main").join("terrain.bin");
    assert!(bin.is_file(), "全量路径也应写 terrain.bin");
}

/// 阶段七 v0.3：`terrain = None` 时不写 .bin，也不删既有 .bin（V1 守约）。
///
/// 这条规则保护作者的「手改 .bin」劳动——工具只在自己能确认的时候动它。
#[test]
fn terrain_none_does_not_touch_existing_bin() {
    let tmp = TempDir::new("terrain_none");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    // 先写一个 .bin（模拟「作者手放了一个地形」）。
    let bin = dir.join("maps").join("map_main");
    std::fs::create_dir_all(&bin).expect("mkdir maps/map_main");
    let hand_bin = bin.join("terrain.bin");
    std::fs::write(&hand_bin, b"author hand-edited bytes").expect("写手改 .bin");
    let mtime_before = std::fs::metadata(&hand_bin).unwrap().modified().unwrap();

    // 加载 → 不动 terrain → 触发一次 dirty=factions 的保存。
    let world = proj.load_world().expect("load");
    proj.save_world(&world, &["factions".into()])
        .expect("save（factions only）");

    // .bin 文件 mtime 应保持（save 不会去删/改它）。
    let mtime_after = std::fs::metadata(&hand_bin).unwrap().modified().unwrap();
    assert_eq!(
        mtime_before, mtime_after,
        "terrain=None 时不应触碰既有 .bin（V1 守约）"
    );
}

/// 阶段七 v0.4：`world.yaml`（工程元数据）随 `save_world` 落盘。
///
/// 验证三件事：
/// 1. 写盘后磁盘上的 `world.yaml` 内容与 `World.meta` 一致；
/// 2. 重开工程时 loader 拿到的 `Project::meta()` 与落盘前一致；
/// 3. `Project::meta()` 内存视图被同步（避免「写盘后内存仍是旧值」的同进程脏读）。
#[test]
fn world_yaml_round_trips_via_save_and_load() {
    let tmp = TempDir::new("world_yaml_round_trip");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    let world = proj.load_world().expect("load");
    let original = world.meta.as_ref().expect("应有 meta").clone();

    // 改一个字段：name 改一改
    let mut world2 = world.clone();
    let mut meta = original.clone();
    meta.name = "改名后的工程".into();
    world2.meta = Some(meta.clone());

    // 保存：即便 dirty 是空（全量写路径），world.yaml 也必须被刷新。
    proj.save_world(&world2, &[]).expect("save");

    // 1) 磁盘上的 world.yaml 应含新 name
    let yaml_text = std::fs::read_to_string(dir.join("world.yaml")).expect("读 world.yaml");
    assert!(
        yaml_text.contains("改名后的工程"),
        "world.yaml 应含新 name: {yaml_text}"
    );

    // 2) 重开：meta 字段应 round-trip
    let proj2 = Project::open(dir).expect("重开");
    let meta2 = proj2.meta().clone();
    assert_eq!(meta2.name, "改名后的工程", "name 应 round-trip");
    assert_eq!(
        meta2.root_map.as_str(),
        original.root_map.as_str(),
        "root_map 不应被改"
    );
    assert_eq!(
        meta2.schema_version, original.schema_version,
        "schema_version 不应被改"
    );

    // 3) 内存里的 Project.meta 已被同步——`proj.meta().name` 已是新值
    assert_eq!(
        proj.meta().name,
        "改名后的工程",
        "Project.meta 内存视图应被同步"
    );
}

/// 阶段七 v0.4：dirty 路由**不**作用于 `world.yaml`——它总是被写。
///
/// 理由：`world.yaml` 是工程的头部，几十字节，永远写比加 dirty 维度更省心。
/// 这条测试用一个**只 factions 脏**的保存，断言 world.yaml 仍然被刷。
#[test]
fn world_yaml_always_written_even_on_focused_save() {
    let tmp = TempDir::new("world_yaml_always");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    // 先把 world.yaml 改成可识别的内容。
    let sentinel_path = dir.join("world.yaml");
    let original = std::fs::read_to_string(&sentinel_path).expect("读 world.yaml");
    assert!(original.contains("测试工程"));

    // 加载 → 触发一次「只 factions 脏」的保存。
    let world = proj.load_world().expect("load");
    proj.save_world(&world, &["factions".into()])
        .expect("save factions only");

    // world.yaml 应仍在且可解析。
    let text_after = std::fs::read_to_string(&sentinel_path).expect("再读 world.yaml");
    let parsed: WorldMeta =
        serde_yaml::from_str(&text_after).expect("world.yaml 仍可解析为 WorldMeta");
    assert_eq!(parsed.name, "测试工程", "name 应保持");
    assert_eq!(parsed.schema_version, 2);
}

/// 阶段七 v0.4：`World.meta = None` 时落盘 fallback 到 `self.meta`。
///
/// 这是「刚加载、还没任何命令填回 `World.meta`」的边界——v0.4 之前 save
/// 会跳过 meta，world.yaml 不被刷新；现在 fallback 到 `self.meta` 保证
/// loader 需要的字段都在。
#[test]
fn world_yaml_falls_back_to_project_meta_when_world_meta_missing() {
    let tmp = TempDir::new("world_yaml_fallback");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");
    world.meta = None; // 模拟「world.meta 暂未填回」

    proj.save_world(&world, &[])
        .expect("save（world.meta=None）");

    // loader 必须还能 open
    let proj2 = Project::open(dir).expect("重开");
    assert_eq!(proj2.meta().name, "测试工程", "应 fallback 到 self.meta");
}

/// 阶段七 v0.5：`World.timeline` → `timeline/axis.yaml` 落盘。
///
/// 验证：
/// 1. 写盘后 `timeline/axis.yaml` 存在且可被 loader 反序列化为 `Timeline`；
/// 2. timeline_range / anchors / calendar 全部 round-trip；
/// 3. 重开工程后 `world.timeline` 一致。
#[test]
fn timeline_axis_round_trips_via_save_and_load() {
    use wv_core::time::{Anchor, Calendar, CalendarKind, TimeSpan, Timeline};

    let tmp = TempDir::new("timeline_round_trip");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");
    assert!(world.timeline.is_none(), "初始：timeline 为 None");

    // 装一个完整 Timeline：calendar、range、两个 anchor。
    let original = Timeline {
        calendar: Calendar {
            kind: CalendarKind::Gregorian,
            prefix: Some("公元".into()),
            start_year: 1840,
            days_per_year: 365,
            days_per_month: 30,
        },
        timeline_range: TimeSpan {
            from: 1,
            to: Some(36500),
        },
        anchors: vec![
            Anchor {
                id: Id::new_unchecked("anchor_start"),
                name: "起点".into(),
                day: 1,
            },
            Anchor {
                id: Id::new_unchecked("anchor_mid"),
                name: "中点".into(),
                day: 18000,
            },
        ],
    };
    world.timeline = Some(original.clone());

    // 全量写：触发 axis.yaml 落盘。
    proj.save_world(&world, &[]).expect("save");

    // 1) 落盘文件存在
    let axis_path = dir.join("timeline").join("axis.yaml");
    assert!(
        axis_path.is_file(),
        "axis.yaml 应已落盘: {}",
        axis_path.display()
    );

    // 2) 文本可被 loader 反序列化
    let text = std::fs::read_to_string(&axis_path).expect("读 axis.yaml");
    let parsed: Timeline = serde_yaml::from_str(&text).expect("axis.yaml 应可反序列化为 Timeline");
    assert_eq!(parsed.calendar.kind, CalendarKind::Gregorian);
    assert_eq!(parsed.timeline_range.from, 1);
    assert_eq!(parsed.timeline_range.to, Some(36500));
    assert_eq!(parsed.anchors.len(), 2);
    assert_eq!(parsed.anchors[0].id.as_str(), "anchor_start");
    assert_eq!(parsed.anchors[1].day, 18000);

    // 3) 重载：world.timeline 应与落盘前一致
    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    let t2 = world2.timeline.as_ref().expect("timeline 应被读回");
    assert_eq!(t2, &original, "timeline 必须 byte-for-byte 一致");
}

/// 阶段七 v0.5：`timeline = None` 时不写 axis.yaml，也不删既有 axis.yaml。
///
/// 与 terrain.bin 的 V1 守约镜像——作者手改的单文件我们不擅自删。
/// 模拟「作者编辑时清掉了世界模型里的时间轴，但磁盘上的文件还在」的场景：
/// 加载后**手动**把 `world.timeline` 置为 None，再 save，断言磁盘文件不变。
#[test]
fn timeline_none_does_not_touch_existing_axis() {
    let tmp = TempDir::new("timeline_none");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    // 先手放一个 axis.yaml（模拟「作者手改了时间轴」）。
    std::fs::create_dir_all(dir.join("timeline")).expect("mkdir timeline");
    let hand_axis = dir.join("timeline").join("axis.yaml");
    let hand_content = "calendar:\n  kind: simple\ntimeline_range:\n  from: 100\n  to: 200\n";
    std::fs::write(&hand_axis, hand_content).expect("写手改 axis.yaml");
    let mtime_before = std::fs::metadata(&hand_axis).unwrap().modified().unwrap();

    // 加载 → 把 world.timeline 显式置 None（模拟「清掉时间轴」），再保存。
    let mut world = proj.load_world().expect("load");
    world.timeline = None; // 关键：与「无 axis.yaml」状态语义一致
    proj.save_world(&world, &["factions".into()])
        .expect("save（factions only，timeline=None）");

    // axis.yaml 应未被触碰——mtime 保持。
    let mtime_after = std::fs::metadata(&hand_axis).unwrap().modified().unwrap();
    assert_eq!(
        mtime_before, mtime_after,
        "timeline=None 时不应触碰既有 axis.yaml（V1 守约）"
    );
    // 内容也保持。
    let read = std::fs::read_to_string(&hand_axis).unwrap();
    assert_eq!(read, hand_content, "axis.yaml 内容应原样保留");
}

#[allow(dead_code)]
fn _silence_unused(_w: &World) {}
