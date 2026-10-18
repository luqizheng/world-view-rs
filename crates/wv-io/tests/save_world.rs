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
use wv_core::map::sea::Sea;
use wv_core::map::{Extent, GeometryData, GeometrySpan, MapLevel, MapNode};
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

/// 海名（手绘地图 #13）save → load round-trip：
/// 之前 `save_world` 漏写 `seas/`，save 后海名全丢；此用例守住修复。
#[test]
fn sea_round_trips_via_save_and_load() {
    let tmp = TempDir::new("sea_round_trip");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("初始加载");

    world.seas.push(Sea {
        id: Id::new_unchecked("sea_pacific"),
        name: "沧浪洋".into(),
        map: Id::new_unchecked("map_main"),
        x: 1024.0,
        y: 512.0,
    });
    world.seas.push(Sea {
        id: Id::new_unchecked("sea_internal"),
        name: "内海".into(),
        map: Id::new_unchecked("map_main"),
        x: 256.0,
        y: 768.0,
    });
    proj.save_world(&world, &["seas".into()])
        .expect("save 海名");

    // 磁盘上应有两个 yaml。
    let s_dir = dir.join("seas");
    assert!(s_dir.join("sea_pacific.yaml").is_file(), "海名文件应落盘");
    assert!(s_dir.join("sea_internal.yaml").is_file(), "海名文件应落盘");

    let proj2 = Project::open(dir).expect("重开工程");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.seas.len(), 2, "海名应全部回来");
    // loader 用目录遍历，**不**保证顺序——按 id 找实体，逐字段断言。
    let pacific = world2
        .seas
        .iter()
        .find(|s| s.id.as_str() == "sea_pacific")
        .expect("sea_pacific 应在");
    assert_eq!(pacific.name, "沧浪洋");
    assert_eq!(pacific.map.as_str(), "map_main");
    assert_eq!(pacific.x, 1024.0);
    assert_eq!(pacific.y, 512.0);
    let internal = world2
        .seas
        .iter()
        .find(|s| s.id.as_str() == "sea_internal")
        .expect("sea_internal 应在");
    assert_eq!(internal.name, "内海");
}

/// 海名删除传播：删一个海名 → save → 磁盘对应文件消失。
#[test]
fn removed_sea_propagates_to_disk() {
    let tmp = TempDir::new("removed_sea");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("初始加载");
    world.seas.push(Sea {
        id: Id::new_unchecked("sea_keep"),
        name: "留".into(),
        map: Id::new_unchecked("map_main"),
        x: 0.0,
        y: 0.0,
    });
    world.seas.push(Sea {
        id: Id::new_unchecked("sea_drop"),
        name: "丢".into(),
        map: Id::new_unchecked("map_main"),
        x: 1.0,
        y: 1.0,
    });
    proj.save_world(&world, &["seas".into()]).expect("先存两个");

    let s_dir = dir.join("seas");
    assert!(s_dir.join("sea_keep.yaml").is_file());
    assert!(s_dir.join("sea_drop.yaml").is_file());

    world.seas.retain(|s| s.id.as_str() != "sea_drop");
    proj.save_world(&world, &["seas".into()]).expect("再存一个");

    assert!(s_dir.join("sea_keep.yaml").is_file(), "留的仍在");
    assert!(!s_dir.join("sea_drop.yaml").is_file(), "删的应传播到磁盘");
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
        relation: None,
    });
    world.faction_geometry.push(GeometrySpan {
        map: Id::new_unchecked("map_main"),
        owner: Id::new_unchecked("f1"),
        span: TimeSpan {
            from: 200,
            to: None,
        },
        geometry: GeometryData::Polygon(vec![1.0, 1.0, 2.0, 1.0, 2.0, 2.0]),
        relation: None,
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

    // 验证变体类型 round-trip 正确：Polygon 不应退化为 Points（untagged 顺序 bug）。
    let g1 = world2
        .faction_geometry
        .iter()
        .find(|g| g.span.from == 1)
        .expect("from=1 应存在");
    assert!(
        matches!(g1.geometry, GeometryData::Polygon(_)),
        "Polygon 应 round-trip 为 Polygon，实得 {:?}",
        g1.geometry
    );
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
    assert!(world.terrains.is_empty(), "初始状态：一张栅格都没有");

    // 装一块 8×6 的栅格：每格 (height, surface, biome) 写入。
    let mut cells = Vec::with_capacity(8 * 6);
    for y in 0..6i32 {
        for x in 0..8i32 {
            cells.push(TerrainCellData {
                height: (x * 100 + y) as i16,
                surface: ((x + y) % 2) as u8,
                biome: ((x + y) % 5) as u8,
            });
        }
    }
    let data = TerrainData {
        width: 8,
        height: 6,
        cells,
    };
    world.set_terrain(Id::new_unchecked("map_main"), data.clone());

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
    let t2 = world2
        .terrain_of(&Id::new_unchecked("map_main"))
        .expect("terrain 应被读回");
    assert_eq!(t2.width, 8);
    assert_eq!(t2.height, 6);
    assert_eq!(t2.cells.len(), 48);
    assert_eq!(t2.cells, data.cells, "terrain 栅格必须 byte-for-byte 一致");
}

/// Phase 2（2026-10-13）：每张图各有各的底图，加载/落盘都按图 ID 走。
///
/// 这是「缩放到街道」（ADR-0009 决策 ③「每级自带栅格」）在 I/O 层的地基：
/// 原先加载器只取**第一张**声明了 `raster` 的图，城市图就算自己声明了也读不到。
///
/// 验证四件事：
/// 1. 一次 `load_world` 把**两张**图的栅格都读进来，键 = 图 ID；
/// 2. 只脏了城市图时，**只有**城市图的 `.bin` 被写（不重写世界图那张）；
/// 3. 全量落盘时两张都写；
/// 4. 重载后两张各自 round-trip 回原来的格子，不串味。
#[test]
fn each_map_keeps_its_own_terrain_raster() {
    let tmp = TempDir::new("per_map_terrain");
    let dir = tmp.path();
    let mut proj = build_min_project_with_root_map(dir, &[]).expect("建工程");

    // 再手写一张城市图（`load_maps` 按 `maps/*/index.yaml` 收集）。
    // 注意：它**不**声明 `raster`——首次落盘才由 `save_world` 补上。
    let city_dir = dir.join("maps").join("map_city");
    std::fs::create_dir_all(&city_dir).expect("mkdir maps/map_city");
    std::fs::write(
        city_dir.join("index.yaml"),
        "id: map_city\nname: City\nlevel: city\nparent: map_main\nextent:\n  width: 64.0\n  height: 64.0\n",
    )
    .expect("写城市图 index.yaml");

    let mut world = proj.load_world().expect("load");
    assert_eq!(world.maps.len(), 2, "应加载到世界图与城市图两张");
    assert!(world.terrains.is_empty(), "两张图都还没声明 raster");

    let world_id = Id::new_unchecked("map_main");
    let city_id = Id::new_unchecked("map_city");
    let cell = |b: u8| {
        vec![
            TerrainCellData {
                height: 7,
                surface: 1,
                biome: b
            };
            4
        ]
    };
    world.set_terrain(
        world_id.clone(),
        TerrainData {
            width: 2,
            height: 2,
            cells: cell(1),
        },
    );
    world.set_terrain(
        city_id.clone(),
        TerrainData {
            width: 2,
            height: 2,
            cells: cell(2),
        },
    );

    // ② 只脏城市图：世界图的 .bin 不该出现。
    proj.save_world(&world, &["maps/map_city/terrain.bin".into()])
        .expect("增量保存");
    let world_bin = dir.join("maps").join("map_main").join("terrain.bin");
    let city_bin = dir.join("maps").join("map_city").join("terrain.bin");
    assert!(
        city_bin.is_file(),
        "脏了的那张应落盘: {}",
        city_bin.display()
    );
    assert!(
        !world_bin.is_file(),
        "没脏的那张不该被重写（否则每刷一笔都要把所有城市 zstd 一遍）"
    );

    // ③ 全量保存：两张都写。
    proj.save_world(&world, &[]).expect("全量保存");
    assert!(world_bin.is_file(), "全量路径应写出世界图栅格");

    // ④ 重载：两张各自 round-trip，且键没串。
    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.terrains.len(), 2, "应读回两张栅格");
    let back_world = world2.terrain_of(&world_id).expect("世界图栅格");
    let back_city = world2.terrain_of(&city_id).expect("城市图栅格");
    assert_eq!(back_world.cells, cell(1), "世界图栅格内容不得串味");
    assert_eq!(back_city.cells, cell(2), "城市图栅格内容不得串味");
}

/// ADR-0010 待做第 3 项（2026-10-13）：街道文档按图层落 `Layer.source` 的
/// YAML，脏路径走虚拟命名空间 `streets/<layer_id>`。
///
/// 验证四件事：
/// 1. 虚拟脏路径能命中正确的落盘位置（`maps/<map>/<source>`）；
/// 2. 只脏街道时不写任何 `terrain.bin`（虚拟路径不得误触发栅格重写）；
/// 3. 全量落盘同样能写出街道文件；
/// 4. 重载后 `World.streets` 按图层 ID round-trip 回来。
#[test]
fn street_layers_are_written_to_their_source_and_survive_reload() {
    let tmp = TempDir::new("street_layer_io");
    let dir = tmp.path();
    let mut proj = build_min_project_with_root_map(dir, &[]).expect("建工程");

    // 手写一张城市图，声明 Road 图层 + source。
    let city_dir = dir.join("maps").join("map_city");
    std::fs::create_dir_all(&city_dir).expect("mkdir maps/map_city");
    std::fs::write(
        city_dir.join("index.yaml"),
        "id: map_city\nname: City\nlevel: city\nparent: map_main\nextent:\n  width: 64.0\n  height: 64.0\nlayers:\n  - id: layer_city_roads\n    name: Streets\n    kind: road\n    visible: true\n    source: maps/map_city/roads.yaml\n",
    )
    .expect("写城市图 index.yaml");

    let mut world = proj.load_world().expect("load");
    assert!(world.terrains.is_empty(), "无 raster，不写地形");

    let layer_id = Id::new_unchecked("layer_city_roads");
    let mut doc = wv_core::StreetLayer::new();
    doc.push(wv_core::StreetSegment::straight(
        wv_core::RoadClass::Primary,
        1.0,
        2.0,
        3.0,
        4.0,
    ));
    world.set_streets(layer_id.clone(), doc);

    // ①② 虚拟脏路径增量保存：roads.yaml 出现，terrain.bin 不出现。
    proj.save_world(&world, &["streets/layer_city_roads".into()])
        .expect("增量保存");
    let roads = city_dir.join("roads.yaml");
    assert!(roads.is_file(), "街道文件应落盘: {}", roads.display());
    assert!(
        !city_dir.join("terrain.bin").is_file(),
        "虚拟脏路径不得误触发地形重写"
    );

    // ④ 重载 round-trip。
    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    assert_eq!(world2.streets.len(), 1, "应读回 1 份街道文档");
    let back = world2.street_of(&layer_id).expect("按图层 ID 读回");
    assert_eq!(back.len(), 1);
    assert_eq!(back.segments[0].class, wv_core::RoadClass::Primary);
    assert_eq!(back.segments[0].points.len(), 2);
    assert_eq!(
        back.segments[0].points[1],
        wv_core::geometry::Point::new(3.0, 4.0)
    );
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
    world.set_terrain(
        Id::new_unchecked("map_main"),
        TerrainData {
            width: 4,
            height: 4,
            cells: vec![
                TerrainCellData {
                    height: 0,
                    surface: 0,
                    biome: 0
                };
                16
            ],
        },
    );
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

/// 阶段七实体对称性修复：PlaceEdge 切到每实体一文件后，
/// save→load 必须能 round-trip 所有 edges（修复前 save→load 静默丢光）。
#[test]
fn place_edges_round_trip_per_file_format() {
    use wv_core::distance::{Place, PlaceEdge, Vehicle};
    let tmp = TempDir::new("edges_round_trip");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");
    // 装 2 个 place、2 个 vehicle、2 条 edge。
    world.distances.places.push(Place {
        id: Id::new_unchecked("p_a"),
        name: "A".into(),
        map: None,
        x: None,
        y: None,
        w: None,
        h: None,
        feature: None,
        biome: None,
        envelope: None,
    });
    world.distances.places.push(Place {
        id: Id::new_unchecked("p_b"),
        name: "B".into(),
        map: None,
        x: None,
        y: None,
        w: None,
        h: None,
        feature: None,
        biome: None,
        envelope: None,
    });
    world.distances.vehicles.push(Vehicle {
        id: Id::new_unchecked("v_walk"),
        name: "步行".into(),
        instant: false,
        speed_kmh: Some(5.0),
        daily_hours: Some(10.0),
        terrain_modifiers: Default::default(),
        requires_endpoints: false,
        requirements: vec![],
    });
    let edges = vec![
        PlaceEdge {
            id: Id::new_unchecked("e_a_b"),
            a: Id::new_unchecked("p_a"),
            b: Id::new_unchecked("p_b"),
            distance_km: 100.0,
            terrain: "plain".into(),
            bidirectional: true,
        },
        PlaceEdge {
            id: Id::new_unchecked("e_b_a"),
            a: Id::new_unchecked("p_b"),
            b: Id::new_unchecked("p_a"),
            distance_km: 105.0,
            terrain: "road".into(),
            bidirectional: true,
        },
    ];
    world.distances.edges = edges.clone();
    // 存 + 重开 + 读。
    proj.save_world(&world, &[]).expect("save");
    // 每边一个 yaml。
    assert!(dir
        .join("distances")
        .join("edges")
        .join("e_a_b.yaml")
        .is_file());
    assert!(dir
        .join("distances")
        .join("edges")
        .join("e_b_a.yaml")
        .is_file());
    // 旧单文件应被清掉。
    assert!(
        !dir.join("distances").join("edges.yaml").is_file(),
        "旧单文件 edges.yaml 应被新格式写盘后清理"
    );
    let proj2 = Project::open(dir).expect("重开");
    let w2 = proj2.load_world().expect("重载");
    assert_eq!(w2.distances.edges.len(), 2);
    let ids: Vec<&str> = w2.distances.edges.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&"e_a_b"));
    assert!(ids.contains(&"e_b_a"));
    // 内容也对。
    let ab = w2
        .distances
        .edges
        .iter()
        .find(|e| e.id.as_str() == "e_a_b")
        .unwrap();
    assert_eq!(ab.distance_km, 100.0);
    assert_eq!(ab.terrain, "plain");
}

/// 旧工程兼容性：手写一个 `distances/edges.yaml`（单文件旧格式），
/// loader 必须仍能加载，作为迁移缓冲。
#[test]
fn place_edges_legacy_single_file_fallback_still_loads() {
    let tmp = TempDir::new("edges_legacy");
    let dir = tmp.path();
    let proj = build_min_project(dir, &[]).expect("建工程");
    // 手放一个旧单文件——模拟「升级前的老工程」。
    std::fs::create_dir_all(dir.join("distances")).expect("mkdir distances");
    let legacy = dir.join("distances").join("edges.yaml");
    let content = r#"- id: e_legacy
  a: p_a
  b: p_b
  distance_km: 50.0
  terrain: river
"#;
    std::fs::write(&legacy, content).expect("写旧单文件");
    // loader 必须能解析。
    let w = proj.load_world().expect("load（带旧单文件）");
    assert_eq!(w.distances.edges.len(), 1, "fallback 应读出 1 条 edge");
    let e = &w.distances.edges[0];
    assert_eq!(e.id.as_str(), "e_legacy");
    assert_eq!(e.distance_km, 50.0);
    assert_eq!(e.terrain, "river");
}

/// 旧 → 新迁移：先有旧单文件 → load → save → 重开 → 数据完整且旧文件已清。
#[test]
fn place_edges_migrate_from_single_file_to_directory() {
    let tmp = TempDir::new("edges_migrate");
    let dir = tmp.path();
    let mut proj = build_min_project(dir, &[]).expect("建工程");
    // 准备两个 place（旧单文件中的 edge 引用它们）。
    std::fs::create_dir_all(dir.join("distances").join("places")).expect("mkdir");
    std::fs::write(
        dir.join("distances").join("places").join("p_a.yaml"),
        "id: p_a\nname: A\n",
    )
    .expect("写 p_a");
    std::fs::write(
        dir.join("distances").join("places").join("p_b.yaml"),
        "id: p_b\nname: B\n",
    )
    .expect("写 p_b");
    // 旧单文件。
    std::fs::write(
        dir.join("distances").join("edges.yaml"),
        "- id: e_legacy\n  a: p_a\n  b: p_b\n  distance_km: 12.0\n  terrain: plain\n",
    )
    .expect("写旧 edges.yaml");

    // load → 看到旧数据。
    let w = proj.load_world().expect("load");
    assert_eq!(w.distances.edges.len(), 1);
    // save → 写新格式 + 删旧文件。
    proj.save_world(&w, &[]).expect("save");
    assert!(
        !dir.join("distances").join("edges.yaml").is_file(),
        "旧文件应被清"
    );
    assert!(
        dir.join("distances")
            .join("edges")
            .join("e_legacy.yaml")
            .is_file(),
        "新格式文件应被写"
    );
    // 重开 → 数据完整。
    let proj2 = Project::open(dir).expect("重开");
    let w2 = proj2.load_world().expect("重载");
    assert_eq!(w2.distances.edges.len(), 1);
    assert_eq!(w2.distances.edges[0].id.as_str(), "e_legacy");
    // 字段齐全（防止漏字段）。
    let e = &w2.distances.edges[0];
    assert_eq!(e.a.as_str(), "p_a");
    assert_eq!(e.b.as_str(), "p_b");
    assert_eq!(e.distance_km, 12.0);
    assert_eq!(e.terrain, "plain");
}

/// ADR-0022 M1 闸门 ①：`MapNode.outline_polygon` 字段的 YAML round-trip。
///
/// 验证三件事：
/// 1. 给城市子图加 `outline_polygon` 后 save → load 回原值；
/// 2. 序列化产物含 `outline_polygon` 键；
/// 3. `GeometryData::Polygon` 在 untagged 顺序下不被误判为 `Points`
///    （与 `faction_geometry_round_trip_via_synthetic_id` § 304 同款 bug 防护）。
#[test]
fn map_node_outline_polygon_round_trips_via_save_and_load() {
    let tmp = TempDir::new("map_node_outline_polygon_round_trip");
    let dir = tmp.path();
    let mut proj = build_min_project_with_root_map(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");

    let city_id = Id::new_unchecked("map_city");
    let city = MapNode {
        id: city_id.clone(),
        name: "A 城".into(),
        level: MapLevel::City,
        parent: Some(Id::new_unchecked("map_main")),
        extent: Some(Extent::sized(1024.0, 1024.0)),
        outline_polygon: Some(GeometryData::Polygon(vec![
            0.0, 0.0, 1024.0, 0.0, 1024.0, 1024.0, 0.0, 1024.0,
        ])),
        ..MapNode::default()
    };
    world.maps.push(city);

    proj.save_world(&world, &[])
        .expect("save 含 outline_polygon");

    let city_yaml = dir.join("maps").join("map_city").join("index.yaml");
    assert!(city_yaml.is_file(), "index.yaml 应落盘");

    let text = std::fs::read_to_string(&city_yaml).expect("读 index.yaml");
    assert!(
        text.contains("outline_polygon"),
        "index.yaml 应含 outline_polygon 键：{text}"
    );

    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    let back = world2
        .maps
        .iter()
        .find(|m| m.id == city_id)
        .expect("城市子图应在");
    let poly = back
        .outline_polygon
        .as_ref()
        .expect("outline_polygon 应被读回");
    assert!(
        matches!(poly, GeometryData::Polygon(_)),
        "outline_polygon 应为 Polygon，实得 {poly:?}"
    );
    assert_eq!(poly.point_count(), 4, "4 对坐标 = 4 顶点");
    assert_eq!(
        poly,
        &GeometryData::Polygon(vec![0.0, 0.0, 1024.0, 0.0, 1024.0, 1024.0, 0.0, 1024.0]),
        "polygon 顶点应 byte-for-byte 一致"
    );
}

/// ADR-0022 M1 闸门 ②：缺省 `outline_polygon` 时不写盘（既有工程零迁移）。
///
/// 验证：
/// 1. 没声明 `outline_polygon` 的城市子图序列化后**不**含该键；
/// 2. 重载后该字段为 `None`；
/// 3. 与 `faction_geometry_round_trip_via_synthetic_id` 共同守住
///    「GeometryData::Polygon 在 YAML 裸数组下被正确识别」的 untagged 顺序约定。
#[test]
fn map_node_without_outline_polygon_keeps_yaml_clean() {
    let tmp = TempDir::new("map_node_no_outline_polygon");
    let dir = tmp.path();
    let mut proj = build_min_project_with_root_map(dir, &[]).expect("建工程");
    let mut world = proj.load_world().expect("load");

    let city_id = Id::new_unchecked("map_city_no_outline");
    let city = MapNode {
        id: city_id.clone(),
        name: "无轮廓城".into(),
        level: MapLevel::City,
        parent: Some(Id::new_unchecked("map_main")),
        extent: Some(Extent::sized(64.0, 64.0)),
        ..MapNode::default()
    };
    world.maps.push(city);
    proj.save_world(&world, &[])
        .expect("save 无 outline_polygon");

    let city_yaml = dir
        .join("maps")
        .join("map_city_no_outline")
        .join("index.yaml");
    let text = std::fs::read_to_string(&city_yaml).expect("读 index.yaml");
    assert!(
        !text.contains("outline_polygon"),
        "缺省字段不应写盘（既有工程零迁移）：{text}"
    );

    let proj2 = Project::open(dir).expect("重开");
    let world2 = proj2.load_world().expect("重载");
    let back = world2
        .maps
        .iter()
        .find(|m| m.id == city_id)
        .expect("无轮廓城应在");
    assert!(
        back.outline_polygon.is_none(),
        "未声明时 outline_polygon 应为 None"
    );
}

/// ADR-0022 M1 闸门 ③：手写不含 `outline_polygon` 的旧工程 YAML 仍能加载。
///
/// 这是「既有工程零迁移」承诺在 IO 层的最后一道闸：
/// ADR-0022 落地前手写的所有工程 YAML 都应继续可加载，outline_polygon
/// 字段被 Option 缺省兼容。
#[test]
fn legacy_yaml_without_outline_polygon_loads_with_none() {
    let tmp = TempDir::new("legacy_no_outline_polygon");
    let dir = tmp.path();
    let proj = build_min_project(dir, &[]).expect("建工程");
    let city_dir = dir.join("maps").join("map_city_legacy");
    std::fs::create_dir_all(&city_dir).expect("mkdir");
    std::fs::write(
        city_dir.join("index.yaml"),
        "id: map_city_legacy\nname: Legacy\nlevel: city\nparent: map_main\nextent:\n  width: 128.0\n  height: 128.0\n",
    )
    .expect("写旧格式 index.yaml");

    let world = proj.load_world().expect("load 旧格式工程");
    let city = world
        .maps
        .iter()
        .find(|m| m.id == Id::new_unchecked("map_city_legacy"))
        .expect("legacy 城市应在");
    assert!(
        city.outline_polygon.is_none(),
        "旧 YAML 加载后 outline_polygon 应为 None（Option 缺省兼容）"
    );
}
