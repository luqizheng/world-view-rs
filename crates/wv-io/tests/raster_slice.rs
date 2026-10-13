//! 底图切片（README §12-24 落地项，`wv_io::slice_terrain`）。
//!
//! 场景：城市图还没自己的栅格，视图靠「底图继承」看父图那块（ADR-0009 §7）；
//! 继承的精度就是父图精度，街道级会糊。切片 = 把父图被覆盖的那块采样成
//! 子图自己的 `terrain.bin`——继承退场、精度升级的时刻。

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::{Id, World};

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
        let dir = std::env::temp_dir().join(format!("wv_io_slice_test_{tag}_{pid}_{nanos}_{n}"));
        std::fs::create_dir_all(&dir).expect("tempdir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 两图工程：世界图 4096²（原点 0,0），城市图 1024² 显式落点 (1024, 512)。
/// 父图栅格 8×8 → 每格覆盖 512 世界单位。
fn two_level_world() -> World {
    let mut world = World::default();
    world.maps.push(parse_map(
        "id: map_main\nname: Main\nlevel: world\nextent:\n  width: 4096.0\n  height: 4096.0\n",
    ));
    world.maps.push(parse_map(
        "id: map_city\nname: City\nlevel: city\nparent: map_main\nextent:\n  width: 1024.0\n  height: 1024.0\n  origin:\n    x: 1024.0\n    y: 512.0\n",
    ));
    world
}

fn parse_map(yaml: &str) -> wv_core::map::MapNode {
    serde_yaml::from_str(yaml).expect("解析 MapNode")
}

/// 父图 8×8 栅格：height = x*10 + y（可手算），kind = x。
fn parent_terrain() -> TerrainData {
    let mut cells = Vec::with_capacity(64);
    for y in 0..8i32 {
        for x in 0..8i32 {
            cells.push(TerrainCellData {
                height: (x * 10 + y) as i16,
                kind: x as u8,
            });
        }
    }
    TerrainData {
        width: 8,
        height: 8,
        cells,
    }
}

#[test]
fn sliced_child_raster_samples_the_parent_region_it_covers() {
    let mut world = two_level_world();
    world.set_terrain(Id::new_unchecked("map_main"), parent_terrain());

    // 城市覆盖世界区域 [1024,2048)×[512,1536)；4×4 切片 → 每格 256 世界单位
    // = 父图半格。格心落在父图格 (2 + gx/2, 1 + gy/2)。
    let sliced = wv_io::slice_terrain(&world, &Id::new_unchecked("map_city"), 4).expect("切片");
    assert_eq!((sliced.width, sliced.height), (4, 4));

    // 手算：子图格 (0,0) 格心 = 本图 (128,128) → 世界 (1152,640) → 父图格 (2,1)
    // → height = 2*10+1 = 21, kind = 2。
    let c = sliced.cells[0];
    assert_eq!(c.height, 21, "格(0,0) 应取父图 (2,1)，实际 {:?}", c);
    assert_eq!(c.kind, 2, "格(0,0) 类别应继承父图列号，实际 {:?}", c);

    // 子图格 (3,3) 格心 = 本图 (896,896) → 世界 (1920,1408) → 父图格 (3,2)
    // → height = 32, kind = 3。
    let c = sliced.cells[3 * 4 + 3];
    assert_eq!(c.height, 32, "格(3,3) 应取父图 (3,2)，实际 {:?}", c);
    assert_eq!(c.kind, 3, "格(3,3) 类别应继承父图列号，实际 {:?}", c);
}

#[test]
fn cells_outside_the_source_raster_fall_back_to_sea_level() {
    let mut world = two_level_world(); // 把城市挪到世界图东北角外沿：覆盖 [3584,4608)×[3584,4608)，右上一角出界。
    world.maps[1].extent.as_mut().unwrap().origin = Some(wv_core::Point::new(3584.0, 3584.0));
    world.set_terrain(Id::new_unchecked("map_main"), parent_terrain());

    let sliced = wv_io::slice_terrain(&world, &Id::new_unchecked("map_city"), 4).expect("切片");
    // 格 (0,0) 格心世界 = (3712,3712) → 父图格 (7,7)：界内。
    assert_eq!(sliced.cells[0].height, 77, "界内格应正常采样");
    // 格 (3,3) 格心世界 = (4480,4480) > 4096 → 出界 → 海平面。
    assert_eq!(
        sliced.cells[3 * 4 + 3],
        TerrainCellData::default(),
        "出界格应取海平面（高 0 / 类 0），不镜像对侧地形"
    );
}

#[test]
fn slicing_rejects_root_maps_unknown_maps_and_rasterless_chains() {
    let world = two_level_world();
    let root_err = wv_io::slice_terrain(&world, &Id::new_unchecked("map_main"), 4)
        .expect_err("根图不该被切片");
    assert!(root_err.to_string().contains("根图"), "{root_err}");

    let unknown_err =
        wv_io::slice_terrain(&world, &Id::new_unchecked("map_no"), 4).expect_err("未知图该报错");
    assert!(unknown_err.to_string().contains("不存在"), "{unknown_err}");

    // 父链上没有任何栅格 → 报错而不是给一张全海平面。
    let rasterless =
        wv_io::slice_terrain(&world, &Id::new_unchecked("map_city"), 4).expect_err("无栅格");
    assert!(rasterless.to_string().contains("栅格"), "{rasterless}");
}

#[test]
fn sliced_raster_is_written_and_reloads_as_the_childs_own_base() {
    let tmp = TempDir::new("write_reload");
    let dir = &tmp.0;
    std::fs::write(
        dir.join("world.yaml"),
        "name: 切片测试\nversion: \"0.1.0\"\nschema_version: 2\ntimeline_range:\n  from: 1\nroot_map: map_main\n",
    )
    .expect("写 world.yaml");
    let main_dir = dir.join("maps").join("map_main");
    std::fs::create_dir_all(&main_dir).expect("mkdir");
    std::fs::write(
        main_dir.join("index.yaml"),
        "id: map_main\nname: Main\nlevel: world\nextent:\n  width: 4096.0\n  height: 4096.0\n",
    )
    .expect("写世界图 index.yaml");
    let city_dir = dir.join("maps").join("map_city");
    std::fs::create_dir_all(&city_dir).expect("mkdir");
    std::fs::write(
        city_dir.join("index.yaml"),
        "id: map_city\nname: City\nlevel: city\nparent: map_main\nextent:\n  width: 1024.0\n  height: 1024.0\n  origin:\n    x: 1024.0\n    y: 512.0\n",
    )
    .expect("写城市图 index.yaml");

    let mut proj = wv_io::Project::open(dir).expect("打开工程");
    let mut world = proj.load_world().expect("load");
    world.set_terrain(Id::new_unchecked("map_main"), parent_terrain());

    // CLI 的完整链路：slice → set_terrain → save（脏路径同 AddStreetSegment 的虚拟口径）。
    let sliced = wv_io::slice_terrain(&world, &Id::new_unchecked("map_city"), 4).expect("切片");
    world.set_terrain(Id::new_unchecked("map_city"), sliced);
    proj.save_world(&world, &["maps/map_city/terrain.bin".into()])
        .expect("保存");

    // 城市图 index.yaml 应出现 raster 引用；世界图 raster 不受牵连。
    let idx = std::fs::read_to_string(city_dir.join("index.yaml")).expect("读 index.yaml");
    assert!(
        idx.contains("maps/map_city/terrain.bin"),
        "index.yaml 应引用城市栅格，实际：{idx}"
    );

    let reloaded = proj.load_world().expect("重载");
    let city = reloaded
        .terrain_of(&Id::new_unchecked("map_city"))
        .expect("城市栅格应落盘并被读回");
    assert_eq!((city.width, city.height), (4, 4), "尺寸应保留");
    assert_eq!(city.cells[0].height, 21, "内容应与切片一致（同上手算）");
}
