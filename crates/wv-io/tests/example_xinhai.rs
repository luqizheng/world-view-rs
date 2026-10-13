//! 辛亥示例工程（examples/xinhai-1911）的集成测试。
//!
//! 覆盖：工程加载、公历历法换算、旁白加载与引用、诊断无错误、
//! 多地图层级（P4）与轨迹插值。

use std::path::PathBuf;

use wv_io::Project;

fn xinhai_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/xinhai-1911")
}

#[test]
fn xinhai_world_loads_with_all_eight_events() {
    let project = Project::open(xinhai_dir()).expect("辛亥示例工程应可打开");
    assert_eq!(project.meta().name, "辛亥一九一一");
    assert_eq!(project.meta().schema_version, 2);

    let world = project.load_world().expect("加载应成功");
    assert_eq!(
        world.events.len(),
        8,
        "应载入全部 8 个事件, got {}",
        world.events.len()
    );
    assert_eq!(
        world.characters.len(),
        3,
        "应载入 3 个角色, got {}",
        world.characters.len()
    );
    assert_eq!(
        world.units.len(),
        3,
        "应载入 3 个单位, got {}",
        world.units.len()
    );
    assert_eq!(
        world.tracks.len(),
        3,
        "应载入 3 条轨迹, got {}",
        world.tracks.len()
    );
    assert_eq!(
        world.situations.len(),
        3,
        "应载入 3 条事态, got {}",
        world.situations.len()
    );
    // 10 世界城 + 3 武昌要地 + 2 成都要地 = 15（任务清单逐一枚举数）。
    assert_eq!(
        world.distances.places.len(),
        15,
        "应载入 15 个地点, got {}",
        world.distances.places.len()
    );
}

#[test]
fn xinhai_calendar_renders_wuchang_uprising_as_gregorian() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();
    let timeline = world.timeline.as_ref().expect("axis.yaml 应已载入");

    assert_eq!(timeline.calendar.to_ymd(283), (1911, 10, 10));
    assert_eq!(
        timeline.calendar.format(283),
        "公元 1911 年 10 月 10 日",
        "武昌起义的公历显示串"
    );
}

#[test]
fn xinhai_narrations_are_loaded_and_referenced() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    assert!(
        !world.narrations.is_empty(),
        "timeline/narrations/ 应载入旁白, got {}",
        world.narrations.len()
    );

    let narration_ids: Vec<&str> = world.narrations.iter().map(|n| n.id.as_str()).collect();
    for event in &world.events {
        for nid in &event.narrations {
            assert!(
                narration_ids.contains(&nid.as_str()),
                "事件 {} 引用的旁白 {} 必须存在; 实得 {narration_ids:?}",
                event.id,
                nid
            );
        }
    }
    // 全部 8 段旁白都应被至少一个事件引用。
    let referenced: Vec<&str> = world
        .events
        .iter()
        .flat_map(|e| e.narrations.iter().map(|i| i.as_str()))
        .collect();
    assert_eq!(
        referenced.len(),
        8,
        "8 个事件各引用 1 段旁白, got {referenced:?}"
    );
}

#[test]
fn xinhai_diagnose_reports_no_errors() {
    let project = Project::open(xinhai_dir()).unwrap();
    let diags = project.diagnose().expect("diagnose 永不因提示而失败");
    assert!(
        !diags.has_errors(),
        "示例工程不应有错误级诊断: {:?}",
        diags.items
    );
}

#[test]
fn xinhai_city_submaps_are_children_of_world() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    let world_map = world
        .maps
        .iter()
        .find(|m| m.id.as_str() == "map_world")
        .expect("世界图应载入");
    let children: Vec<&str> = world_map.children.iter().map(|i| i.as_str()).collect();
    assert!(
        children.contains(&"map_city_wuchang") && children.contains(&"map_city_chengdu"),
        "父图 children 只存子图 Id（P4）, got {children:?}"
    );
    // 子图自身存在，且经 parent 反向指回父图——只存引用，不复制内容。
    for cid in ["map_city_wuchang", "map_city_chengdu"] {
        let city = world
            .maps
            .iter()
            .find(|m| m.id.as_str() == cid)
            .unwrap_or_else(|| panic!("{cid} 应作为独立地图载入"));
        assert_eq!(
            city.parent.as_ref().map(|p| p.as_str()),
            Some("map_world"),
            "{cid} 的 parent 应指回 map_world"
        );
    }
}

#[test]
fn xinhai_track_interpolates_new_army_route() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    let track = world
        .tracks
        .iter()
        .find(|t| t.id.as_str() == "track_new_army_west")
        .expect("西调轨迹应载入");

    // 锚点日精确命中：day 258 = 武汉。
    let start = track.sample_at(258).unwrap();
    assert_eq!((start.x, start.y), (1650.0, 1700.0), "起点应为武汉坐标");
    // day 260 位于武汉(258) → 宜昌(263) 之间：u = 2/5。
    // x = 1650 + (1500-1650)*0.4 = 1590; y = 1700 + (1850-1700)*0.4 = 1760。
    let mid = track.sample_at(260).unwrap();
    assert!(
        (mid.x - 1590.0).abs() < 1e-3 && (mid.y - 1760.0).abs() < 1e-3,
        "day 260 应为武汉→宜昌之间的线性插值点 (1590, 1760), got ({}, {})",
        mid.x,
        mid.y
    );
}

#[test]
fn xinhai_city_frame_is_derived_from_the_world_icon_anchor() {
    // 「缩放到街道」的落点派生（ADR-0009 §6）：城市图**没有**声明 `origin`，
    // 它在世界里的位置由世界图上那个入口图标反推——
    // `icon_wuhan` 的 `anchor: [0.403, 0.415]`、`drill_into: map_city_wuchang`。
    //
    // 图标标记的是城市**中心**，故（手算，不跑出来就算数）：
    //   中心 = (0.403×4096, 0.415×4096) = (1650.688, 1699.84)
    //   左上 = 中心 − 1024/2       = (1138.688, 1187.84)
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    let frame = wv_core::resolve_frame_in(
        &world.maps,
        &wv_core::id::Id::new_unchecked("map_city_wuchang"),
    )
    .expect("武昌城图应能还原出世界坐标帧");

    assert!(
        (frame.world_origin.x - 1138.688).abs() < 0.1
            && (frame.world_origin.y - 1187.84).abs() < 0.1,
        "got ({}, {})，期望 (1138.688, 1187.84) = 图标点 (1650.688, 1699.84) − 半幅 512",
        frame.world_origin.x,
        frame.world_origin.y
    );

    // 语义校验：城市中心必须落在 `distances/places.yaml` 里武汉的坐标 (1650, 1700) 上。
    // 两者的差只有锚点「三位小数」的取整误差（< 1 世界单位 / 4096）。
    let center = frame.local_to_world(wv_core::geometry::Point::new(512.0, 512.0));
    assert!(
        (center.x - 1650.0).abs() < 1.0 && (center.y - 1700.0).abs() < 1.0,
        "城市中心 ({}, {}) 应落在武汉 Place 坐标 (1650, 1700) 上",
        center.x,
        center.y
    );

    // 反证：旧实现把「未声明 origin」当 `(0,0)`，城市图整体落在世界帧左上四分之一。
    assert!(
        frame.world_origin != wv_core::geometry::Point::ORIGIN,
        "落点若为 (0,0)，说明派生没生效（旧 bug 回归了）"
    );
}

#[test]
fn xinhai_loads_one_terrain_per_declared_raster() {
    // Phase 2（ADR-0009 §7）：栅格从「全世界一张」改为「每图各自一张」。
    // 本测试守住两条口径——
    //   ① 每张**声明了 `raster`** 的图，都必须有一条按它自己的 Id 存的地形；
    //   ② 没有声明 `raster` 的图（本示例的武昌/成都城图）不带地形，
    //      城市视图因此走**底图继承**（向上取最近一张有栅格的图）而不是自己的。
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    let declared: Vec<&str> = world
        .maps
        .iter()
        .filter(|m| m.raster.is_some())
        .map(|m| m.id.as_str())
        .collect();
    assert!(
        declared.contains(&"map_world"),
        "世界图声明了 raster，应在声明集合里, got {declared:?}"
    );
    assert_eq!(
        world.terrains.len(),
        declared.len(),
        "地形条数必须与「声明了 raster 的图」数一致（按 Id 一一对应）, got terrains={:?} declared={declared:?}",
        world.terrains.keys().map(|i| i.as_str()).collect::<Vec<_>>()
    );
    for id in &declared {
        assert!(
            world
                .terrain_of(&wv_core::id::Id::new_unchecked(*id))
                .is_some(),
            "{id} 声明了 raster，必须真的载入了一条地形"
        );
    }

    // 城市图当前没有自己的 terrain.bin（未声明 raster）→ 城市视图继承世界图底图。
    // 作者给城市刷出地形之后，这条断言会自然失效——那正是继承退场、精度升级的时刻。
    assert!(
        world
            .terrain_of(&wv_core::id::Id::new_unchecked("map_city_wuchang"))
            .is_none(),
        "武昌城图尚未自带栅格，不应凭空多出一条地形"
    );
}
