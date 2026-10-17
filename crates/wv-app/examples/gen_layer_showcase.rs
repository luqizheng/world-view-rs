//! 一次性工具：生成 `examples/layer-showcase` 测试工程（图层体系的验收数据）。
//!
//! 用法：`cargo run -p wv-app --example gen_layer_showcase`
//!
//! ## 为什么手搓一个新示例
//!
//! 图层体系（ADR-0013 LayerFrame + ADR-0017 标签层）的每个特性散落在
//! 既有示例里，没有一个工程能「打开一个、看全」：
//!
//! - `minimal-world` 只声明了 3 层，且 `z` 全缺省；
//! - `lod-demo` 服务于 LOD 分档验收，无势力 / 子图 / 街道；
//! - `xinhai-1911` 最全，但图层声明保守（`z` 全 0 / 无双 kind / 无隐藏层），
//!   特性演示点淹没在历史叙事数据里。
//!
//! 本示例**故意**把每个图层特性做成一个肉眼可见的演示点（`z` 抬压 / 半透明 /
//! 声明隐藏 / 同 kind 双声明 / 越界 `z` 不拒收 / 子图声明全量接管……），
//! 逐项说明见 `examples/layer-showcase/README.md`。
//!
//! ## 生成方式
//!
//! 与 `gen_lod_demo` 同款：用 Rust 构建 `wv_core::World`，走
//! `wv_io::Project::save_world` 标准链路落盘（每实体一文件 §4.5、
//! terrain.bin 经 `write_terrain_if_any` 写出并回写 RasterRef）——
//! 生成物与加载器 schema 天然一致（round-trip 同源）。标签层配置不在
//! `World` 上（只存配置不存文本，ADR-0017 §2.1），另走 `write_labels`。

use wv_core::distance::{Place, PlaceEdge, Vehicle};
use wv_core::event::{Event, EventLocation};
use wv_core::id::Id;
use wv_core::map::{
    Extent, GeometryData, GeometrySpan, IconShape, IconSpec, Layer, LayerKind, MapLevel, MapNode,
};
use wv_core::model::faction::Faction;
use wv_core::source::measurement::{Measurement, MeasurementKind, MeasurementOverlay};
use wv_core::source::street::{RoadClass, StreetLayer, StreetSegment};
use wv_core::time::{Anchor, Calendar, TimeSpan, Timeline};
use wv_core::{
    Building, BuildingLayer, Color, EntityKind, LabelLayerConfig, Point, Region, Track, Unit,
    Waypoint, World, WorldMeta,
};
use wv_terrain::generate::Perlin;

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/layer-showcase".into());
    std::fs::create_dir_all(&root).expect("create project dir");

    let meta = WorldMeta {
        name: "图层演示世界".into(),
        version: "0.1".into(),
        schema_version: 2,
        root_map: Id::new("map_world").unwrap(),
        timeline_range: TimeSpan {
            from: 1,
            to: Some(401),
        },
    };

    // 先写 world.yaml（Project::open 的准入凭证），再走标准保存链路。
    let world_yaml = serde_yaml::to_string(&meta).expect("serialize meta");
    std::fs::write(
        std::path::Path::new(&root).join("world.yaml"),
        format!("# 图层演示工程（gen_layer_showcase 生成）· schema v2\n{world_yaml}"),
    )
    .expect("write world.yaml");

    let world = build_world(meta);
    let mut project = wv_io::Project::open(&root).expect("open project");
    // 空 dirty = 全量写（见 project.rs save_world 的匹配语义）。
    project.save_world(&world, &[]).expect("save world");

    // 标签层配置（世界 / 城市各一份）：不在 World 上，单独落盘。
    let world_labels = LabelLayerConfig {
        entity_kinds: vec![EntityKind::Place, EntityKind::Faction, EntityKind::Region],
        ..LabelLayerConfig::default()
    };
    project
        .write_labels("maps/map_world/labels.yaml", &world_labels)
        .expect("write world labels");
    let city_labels = LabelLayerConfig {
        entity_kinds: vec![EntityKind::Place, EntityKind::StreetSegment],
        ..LabelLayerConfig::default()
    };
    project
        .write_labels("maps/map_city_dongang/labels.yaml", &city_labels)
        .expect("write city labels");

    // 空目录占位：characters / relations 不在本演示范围，但校验器会提示
    // 「缺少预期目录」——补最小合法文件消掉提示（V1：提示不阻止，收干净而已）。
    std::fs::create_dir_all(std::path::Path::new(&root).join("characters"))
        .expect("mkdir characters");
    std::fs::write(
        std::path::Path::new(&root).join("characters/index.yaml"),
        "characters: []\n",
    )
    .expect("write characters index");
    std::fs::create_dir_all(std::path::Path::new(&root).join("relations"))
        .expect("mkdir relations");
    std::fs::write(
        std::path::Path::new(&root).join("relations/graph.yaml"),
        "relations: []\n",
    )
    .expect("write relations graph");

    println!(
        "written {root}: maps={} layers(world)={} places={} factions={} spans={} events={}",
        world.maps.len(),
        world.maps[0].layers.len(),
        world.distances.places.len(),
        world.factions.len(),
        world.faction_geometry.len(),
        world.events.len(),
    );
    println!("验收：`wv gui -m {root}`，演示点逐项见 examples/layer-showcase/README.md。");
}

fn layer(id: &str, name: &str, kind: LayerKind) -> Layer {
    Layer {
        id: Id::new(id).unwrap(),
        name: name.into(),
        kind,
        visible: true,
        locked: false,
        opacity: 1.0,
        z: 0,
        source: None,
    }
}

fn build_world(meta: WorldMeta) -> World {
    let mut world = World {
        meta: Some(meta),
        ..World::default()
    };
    let map_world = Id::new("map_world").unwrap();
    let map_city = Id::new("map_city_dongang").unwrap();

    // ---- 世界图：9 种 LayerKind 全声明，每层一个演示点 --------------------
    // 演示点口径（与 README 一一对应）：
    // - z 是「槽位偏移」（ADR-0013）：+4 格 = +0.2 世界 Z；渲染层钳 ±4 格。
    // - 同 kind 双声明：第一个生效（layer_roads 的 opacity/z 赢，
    //   layer_roads_backup 全字段被忽略）。
    // - 越界 z=10（border）：数据层不拒收（V1），渲染层钳回 +4。
    let mut l = layer("layer_terrain", "地形", LayerKind::Terrain);
    l.source = Some("maps/map_world/terrain.bin".into());
    world.maps.push(MapNode {
        id: map_world.clone(),
        name: "澜野九州".into(),
        level: MapLevel::World,
        parent: None,
        children: vec![map_city.clone()],
        icons: vec![
            icon(
                "icon_xiling",
                "place_xiling",
                IconShape::Circle,
                Some(12.0),
                None,
            ),
            icon(
                "icon_beiyuan",
                "place_beiyuan",
                IconShape::Circle,
                Some(12.0),
                None,
            ),
            // 钻入入口（缺省菱形）：双击进入东港城子图。
            icon(
                "icon_dongang",
                "place_dongang",
                IconShape::Diamond,
                None,
                Some("map_city_dongang"),
            ),
            icon(
                "icon_nanze",
                "place_nanze",
                IconShape::Circle,
                Some(12.0),
                None,
            ),
        ],
        layers: vec![
            l,
            {
                let mut b = layer("layer_border_reserve", "疆界（预留）", LayerKind::Border);
                b.opacity = 0.8;
                // 越界演示：|z|>4 由渲染层钳回，数据层照单全收（AGENTS §7.5 V1）。
                b.z = 10;
                b
            },
            {
                let mut r = layer("layer_roads", "官道", LayerKind::Road);
                // 抬升演示：+4 格把城际连线（内建 0.09/0.1）抬到 POI（0.21/0.22）
                // 与势力面（0.3）之上——默认 z=0 时连线在 POI 之下。
                // 半透明演示：0.65。
                r.opacity = 0.65;
                r.z = 4;
                r
            },
            {
                // 同 kind 第二声明：全字段被「first declared wins」忽略——
                // 改它的 opacity/z 不会引起任何渲染变化。
                let mut r = layer(
                    "layer_roads_backup",
                    "官道（第二声明·不生效）",
                    LayerKind::Road,
                );
                r.opacity = 1.0;
                r.z = -4;
                r
            },
            layer("layer_places", "城邑", LayerKind::Place),
            layer("layer_events", "事件", LayerKind::EventPoint),
            {
                let mut i = layer("layer_influence", "势力疆域", LayerKind::Influence);
                // 下沉演示：-4 格把势力多边形（内建 0.3）压到道路（0.1）一带，
                // 不再盖住 POI 与地名。
                i.opacity = 0.75;
                i.z = -4;
                i
            },
            {
                let mut lab = layer("layer_labels", "标注", LayerKind::Label);
                lab.source = Some("maps/map_world/labels.yaml".into());
                lab
            },
            {
                // 声明隐藏 + 无消费者：Custom 既不渲染也不报错，留作占位。
                let mut c = layer("layer_notes", "私注（隐藏）", LayerKind::Custom);
                c.visible = false;
                c.opacity = 0.9;
                c
            },
        ],
        extent: Some(Extent::sized(4096.0, 4096.0)),
        raster: None, // save_world 的 write_terrain_if_any 会生成并回写
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    });

    // ---- 城市子图：声明全量接管 ------------------------------------------
    // 世界图声明了 9 层；钻入子图后**全部失效**，生效的是子图自己的 5 层。
    // 故意不声明 EventPoint / Influence / Custom：未声明的 kind 回落会话开关
    // 缺省（neutral 绑定）——不是「继承父图」。
    let mut city_terrain = layer("layer_city_terrain", "城郭底图", LayerKind::Terrain);
    city_terrain.source = Some("maps/map_city_dongang/terrain.bin".into());
    let mut city_streets = layer("layer_city_streets", "街巷", LayerKind::Road);
    city_streets.source = Some("maps/map_city_dongang/roads.yaml".into());
    city_streets.z = 1;
    let mut city_buildings = layer("layer_city_buildings", "建筑", LayerKind::Building);
    city_buildings.source = Some("maps/map_city_dongang/buildings.yaml".into());
    let mut city_labels = layer("layer_city_labels", "标注", LayerKind::Label);
    city_labels.source = Some("maps/map_city_dongang/labels.yaml".into());
    world.maps.push(MapNode {
        id: map_city.clone(),
        name: "东港城".into(),
        level: MapLevel::City,
        parent: Some(map_world.clone()),
        children: vec![],
        icons: vec![],
        layers: vec![
            city_terrain,
            city_streets,
            city_buildings,
            layer("layer_city_places", "城内要地", LayerKind::Place),
            city_labels,
        ],
        extent: Some(Extent::sized(1024.0, 1024.0)),
        raster: None,
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    });

    // ---- 世界图地形：1024² 确定性 Perlin（西陆东海）-----------------------
    write_world_terrain(&mut world, &map_world);
    // ---- 子图地形：256² 滨海平原 + 东侧入海（Phase 2 按图栅格）------------
    write_city_terrain(&mut world, &map_city);

    // ---- 距离图：4 城（is_city）+ 4 点标 ---------------------------------
    let city = |id: &str, name: &str, x: f32, y: f32| Place {
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(map_world.clone()),
        x: Some(x),
        y: Some(y),
        w: Some(80.0),
        h: Some(60.0),
        feature: None,
    };
    let dot = |id: &str, name: &str, x: f32, y: f32| Place {
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(map_world.clone()),
        x: Some(x),
        y: Some(y),
        w: None,
        h: None,
        feature: None,
    };
    world.distances.places = vec![
        city("place_xiling", "西陵城", 500.0, 900.0),
        city("place_beiyuan", "北原城", 2400.0, 800.0),
        city("place_dongang", "东港城", 3300.0, 2300.0),
        city("place_nanze", "南泽城", 1300.0, 3100.0),
        dot("place_qingshi", "青石渡", 1600.0, 1200.0),
        dot("place_luoyan", "落雁隘", 2100.0, 1600.0),
        dot("place_fenghuo", "烽火台", 1800.0, 2600.0),
        dot("place_gucheng", "古城遗迹", 2900.0, 2900.0),
        // 城内要地（子图坐标）：只挂子图，无 w/h。
        dot_inner("place_dock", "东港码头", 200.0, 650.0, &map_city),
        dot_inner("place_market", "市集", 480.0, 430.0, &map_city),
        dot_inner("place_yamen", "知港府", 600.0, 300.0, &map_city),
    ];

    let edge = |id: &str, a: &str, b: &str, km: f32, terrain: &str| PlaceEdge {
        id: Id::new(id).unwrap(),
        a: Id::new(a).unwrap(),
        b: Id::new(b).unwrap(),
        distance_km: km,
        terrain: terrain.into(),
        bidirectional: true,
    };
    world.distances.edges = vec![
        edge(
            "edge_xiling_beiyuan",
            "place_xiling",
            "place_beiyuan",
            520.0,
            "plain",
        ),
        edge(
            "edge_beiyuan_dongang",
            "place_beiyuan",
            "place_dongang",
            610.0,
            "hill",
        ),
        edge(
            "edge_dongang_nanze",
            "place_dongang",
            "place_nanze",
            780.0,
            "plain",
        ),
        edge(
            "edge_nanze_xiling",
            "place_nanze",
            "place_xiling",
            430.0,
            "plain",
        ),
        edge(
            "edge_qingshi",
            "place_xiling",
            "place_qingshi",
            120.0,
            "plain",
        ),
        edge("edge_luoyan", "place_beiyuan", "place_luoyan", 90.0, "hill"),
        edge(
            "edge_fenghuo",
            "place_nanze",
            "place_fenghuo",
            80.0,
            "plain",
        ),
        edge(
            "edge_gucheng",
            "place_dongang",
            "place_gucheng",
            110.0,
            "plain",
        ),
    ];

    world.distances.vehicles = vec![Vehicle {
        id: Id::new("veh_horse").unwrap(),
        name: "驿马".into(),
        instant: false,
        speed_kmh: Some(10.0),
        daily_hours: Some(8.0),
        terrain_modifiers: [("plain".to_string(), 1.2), ("hill".to_string(), 0.7)]
            .into_iter()
            .collect(),
        requires_endpoints: false,
        requirements: vec![],
    }];

    // ---- 势力：区间化疆域（P6/P7——拖时间轴看版图变化）-------------------
    world.factions = vec![
        faction("faction_beichao", "北府", "#4E6EC4"),
        faction("faction_nanchao", "南盟", "#C4644E"),
    ];
    let span = |owner: &str, from: i64, to: i64, poly: &[f32]| GeometrySpan {
        map: map_world.clone(),
        owner: Id::new(owner).unwrap(),
        span: TimeSpan { from, to: Some(to) },
        geometry: GeometryData::Polygon(poly.to_vec()),
    };
    world.faction_geometry = vec![
        // 北府早期版图（day < 120）：北境大块。
        span(
            "faction_beichao",
            1,
            120,
            &[300.0, 300.0, 2600.0, 300.0, 2600.0, 1500.0, 300.0, 1500.0],
        ),
        // 北府收缩（day ≥ 120）：落雁隘一役后让出东境。
        span(
            "faction_beichao",
            120,
            400,
            &[300.0, 300.0, 1600.0, 300.0, 1600.0, 1100.0, 300.0, 1100.0],
        ),
        // 南盟（day ≥ 80 才出现）。
        span(
            "faction_nanchao",
            80,
            400,
            &[700.0, 1800.0, 3000.0, 1800.0, 3000.0, 3400.0, 700.0, 3400.0],
        ),
    ];

    // ---- 事件：只有时间窗内可见（拖时间轴）-------------------------------
    let event = |id: &str, name: &str, from: i64, to: i64, place: &str, outcome: &str| Event {
        id: Id::new(id).unwrap(),
        name: name.into(),
        span: TimeSpan { from, to: Some(to) },
        location: Some(EventLocation {
            place: Id::new(place).unwrap(),
            x: None,
            y: None,
            w: None,
            h: None,
        }),
        participants: vec![],
        narrative: None,
        outcome: Some(outcome.into()),
        effects: vec![],
        narrations: vec![],
    };
    world.events = vec![
        event(
            "evt_founding",
            "西陵建城",
            1,
            15,
            "place_xiling",
            "新历元年，西陵城依山而立。",
        ),
        event(
            "evt_pass_war",
            "落雁隘之战",
            120,
            128,
            "place_luoyan",
            "北府败退，让出东境四郡。",
        ),
        event(
            "evt_port_open",
            "东港开埠",
            200,
            215,
            "place_dongang",
            "东港开海禁，商船云集。",
        ),
    ];

    // ---- 单位 + 轨迹（Trail：会话开关，不在 LayerKind——ADR-0013 待裁）----
    world.units = vec![Unit {
        id: Id::new("unit_courier").unwrap(),
        name: "驿使队".into(),
        owner: None,
        members: vec![],
    }];
    world.tracks = vec![Track {
        id: Id::new("track_courier_west").unwrap(),
        unit: Id::new("unit_courier").unwrap(),
        map: map_world.clone(),
        span: TimeSpan {
            from: 100,
            to: Some(112),
        },
        waypoints: vec![
            Waypoint {
                day: Some(100),
                pos: Some(Point { x: 500.0, y: 900.0 }),
                place: Some(Id::new("place_xiling").unwrap()),
            },
            Waypoint {
                day: Some(105),
                pos: Some(Point {
                    x: 1600.0,
                    y: 1200.0,
                }),
                place: Some(Id::new("place_qingshi").unwrap()),
            },
            Waypoint {
                day: Some(110),
                pos: Some(Point {
                    x: 2400.0,
                    y: 800.0,
                }),
                place: Some(Id::new("place_beiyuan").unwrap()),
            },
        ],
        path: None,
    }];

    // ---- 命名区域（标签投影源，ADR-0017 EntityKind::Region）--------------
    // 海名（Sea）数据层已就位但渲染未接线，本演示只收「肉眼可见」的特性，
    // 故不放入。
    world.regions = vec![Region {
        id: Id::new("region_woye").unwrap(),
        name: "沃野郡".into(),
        map: map_world.clone(),
        polygon: vec![400.0, 2000.0, 1400.0, 2000.0, 1400.0, 2900.0, 400.0, 2900.0],
    }];

    // ---- 测量（工具产物：不走 LayerKind，每图一份 YAML）------------------
    world.measurements.insert(
        map_world.clone(),
        MeasurementOverlay {
            measurements: vec![
                Measurement {
                    id: Id::new("meas_west_road").unwrap(),
                    kind: MeasurementKind::Polyline,
                    points: vec![
                        Point { x: 500.0, y: 900.0 },
                        Point {
                            x: 1600.0,
                            y: 1200.0,
                        },
                        Point {
                            x: 2400.0,
                            y: 800.0,
                        },
                    ],
                    color: None,
                },
                Measurement {
                    id: Id::new("meas_se_block").unwrap(),
                    kind: MeasurementKind::Rectangle,
                    points: vec![
                        Point {
                            x: 3000.0,
                            y: 2600.0,
                        },
                        Point {
                            x: 3900.0,
                            y: 3300.0,
                        },
                    ],
                    color: Some("#C4A24E".into()),
                },
            ],
        },
    );

    // ---- 城市街巷（含段级 time_span：ADR-0015）---------------------------
    let seg = |class: RoadClass, points: &[(f32, f32)]| StreetSegment {
        class,
        points: points.iter().map(|(x, y)| Point { x: *x, y: *y }).collect(),
        name: None,
        time_span: None,
    };
    world.set_streets(
        Id::new("layer_city_streets").unwrap(),
        StreetLayer {
            segments: vec![
                seg(
                    RoadClass::Primary,
                    &[
                        (60.0, 420.0),
                        (340.0, 416.0),
                        (640.0, 390.0),
                        (964.0, 404.0),
                    ],
                ),
                seg(
                    RoadClass::Primary,
                    &[
                        (300.0, 860.0),
                        (322.0, 560.0),
                        (340.0, 380.0),
                        (352.0, 220.0),
                    ],
                ),
                // 段级时间窗演示：环城老墙街只在 day < 150 存在——
                // 拖过 150（老墙拆除锚点）整条街消失。
                StreetSegment {
                    class: RoadClass::Secondary,
                    points: vec![
                        Point { x: 140.0, y: 180.0 },
                        Point { x: 880.0, y: 180.0 },
                        Point { x: 880.0, y: 880.0 },
                        Point { x: 140.0, y: 880.0 },
                        Point { x: 140.0, y: 180.0 },
                    ],
                    name: Some("环城老墙街".into()),
                    time_span: Some(TimeSpan {
                        from: 1,
                        to: Some(150),
                    }),
                },
                seg(RoadClass::Residential, &[(160.0, 560.0), (480.0, 566.0)]),
                seg(RoadClass::Residential, &[(560.0, 480.0), (880.0, 486.0)]),
                seg(RoadClass::Residential, &[(620.0, 560.0), (626.0, 820.0)]),
                seg(RoadClass::Service, &[(200.0, 120.0), (360.0, 118.0)]),
            ],
        },
    );

    // ---- 城市建筑（2.5D 挤出，ADR-0016 §6）------------------------------
    let mut buildings = BuildingLayer::new();
    buildings.push(named_rect("知港府", 560.0, 240.0, 120.0, 80.0, 18.0));
    buildings.push(named_rect("望海塔", 700.0, 300.0, 36.0, 36.0, 42.0));
    buildings.push(named_rect("市仓", 380.0, 520.0, 150.0, 70.0, 6.0));
    buildings.push(Building::rect(180.0, 560.0, 60.0, 60.0, 9.0));
    buildings.push(Building::rect(250.0, 560.0, 60.0, 60.0, 9.0));
    buildings.push(Building::rect(320.0, 560.0, 60.0, 60.0, 9.0));
    buildings.push(Building::rect(430.0, 620.0, 60.0, 60.0, 9.0));
    buildings.push(named_rect("码头货栈", 120.0, 680.0, 140.0, 60.0, 8.0));
    world
        .buildings
        .insert(Id::new("layer_city_buildings").unwrap(), buildings);

    // ---- 时间轴：锚点即「演示点的路标」-----------------------------------
    world.timeline = Some(Timeline {
        calendar: Calendar {
            kind: wv_core::time::CalendarKind::Simple,
            prefix: Some("新历".into()),
            start_year: 1,
            days_per_year: 360,
            days_per_month: 30,
        },
        timeline_range: TimeSpan {
            from: 1,
            to: Some(401),
        },
        anchors: vec![
            Anchor {
                id: Id::new("anchor_founding").unwrap(),
                name: "西陵建城".into(),
                day: 1,
            },
            Anchor {
                id: Id::new("anchor_courier").unwrap(),
                name: "驿使出发".into(),
                day: 100,
            },
            Anchor {
                id: Id::new("anchor_nanchao").unwrap(),
                name: "南盟立旗".into(),
                day: 80,
            },
            Anchor {
                id: Id::new("anchor_pass_war").unwrap(),
                name: "落雁隘之战".into(),
                day: 120,
            },
            Anchor {
                id: Id::new("anchor_wall_demolish").unwrap(),
                name: "老墙拆除".into(),
                day: 150,
            },
            Anchor {
                id: Id::new("anchor_port_open").unwrap(),
                name: "东港开埠".into(),
                day: 200,
            },
        ],
    });

    world
}

/// 挂子图的点标（无范围）。
fn dot_inner(id: &str, name: &str, x: f32, y: f32, map: &Id) -> Place {
    Place {
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(map.clone()),
        x: Some(x),
        y: Some(y),
        w: None,
        h: None,
        feature: None,
    }
}

/// 世界图图标：anchor = 同名 Place 的 (x,y) ÷ extent(4096)，归一化 [0,1]²。
///
/// `drill_into = Some(id)` 时该图标是钻入入口（双击进入子图）。
fn icon(
    id: &str,
    place: &str,
    shape: IconShape,
    size: Option<f32>,
    drill_into: Option<&str>,
) -> IconSpec {
    IconSpec {
        id: Id::new(id).unwrap(),
        anchor: Some(anchor_of(place)),
        shape,
        size,
        color: None,
        drill_into: drill_into.map(|m| Id::new(m).unwrap()),
    }
}

/// 8 个演示 Place 在世界图的归一化锚点（x/4096, y/4096）。
fn anchor_of(place: &str) -> [f32; 2] {
    let (x, y) = match place {
        "place_xiling" => (500.0_f32, 900.0_f32),
        "place_beiyuan" => (2400.0, 800.0),
        "place_dongang" => (3300.0, 2300.0),
        "place_nanze" => (1300.0, 3100.0),
        _ => (2048.0, 2048.0),
    };
    [x / 4096.0, y / 4096.0]
}

/// 带名矩形建筑。
fn named_rect(name: &str, x: f32, y: f32, w: f32, h: f32, height_m: f32) -> Building {
    let mut b = Building::rect(x, y, w, h, height_m);
    b.name = Some(name.into());
    b
}

/// 北府 / 南盟构造（颜色用 `Color("#RRGGBB")`）。
fn faction(id: &str, name: &str, color: &str) -> Faction {
    Faction::new(Id::new(id).unwrap(), name, Color(color.into()))
}

/// 世界图地形：西陆东海（与 gen_lod_demo 同款梯度，种子不同）。
fn write_world_terrain(world: &mut World, map: &Id) {
    let (w, h) = (1024u32, 1024u32);
    let perlin = Perlin::new(2026_1016);
    let freq = 0.045 * 512.0 / w as f64;
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = perlin.fbm(x as f64 * freq, y as f64 * freq, 5, 2.0, 0.5);
            let t = ((n + 1.0) / 2.0).clamp(0.0, 1.0);
            let east = (x as f64 / w as f64) * 1.2;
            let t = (t * 0.55 + east * 0.45).clamp(0.0, 1.0);
            cells.push(wv_core::terrain::TerrainCellData {
                height: (t * 2000.0) as i16,
                kind: terrain_kind(t),
            });
        }
    }
    world.set_terrain(
        map.clone(),
        wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        },
    );
}

/// 子图地形：256² 滨海平原，东侧 (x>0.84) 入海——子图有自己的 terrain.bin
/// （World.terrains 按图索引，Phase 2）。
fn write_city_terrain(world: &mut World, map: &Id) {
    let (w, h) = (256u32, 256u32);
    let perlin = Perlin::new(1016);
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = perlin.fbm(x as f64 * 0.03, y as f64 * 0.03, 4, 2.0, 0.5);
            // 平原基调（草 / 林），起伏 ±0.1；东侧滑入海面。
            let mut t = 0.52 + n * 0.10;
            let east = x as f64 / w as f64;
            if east > 0.84 {
                t -= (east - 0.84) * 4.0;
            }
            let t = t.clamp(0.0, 1.0);
            // 低洼海滨：高度上限 600 m（与 2000 m 的世界图拉开层次）。
            cells.push(wv_core::terrain::TerrainCellData {
                height: (t * 600.0) as i16,
                kind: terrain_kind(t),
            });
        }
    }
    world.set_terrain(
        map.clone(),
        wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        },
    );
}

/// 高度 → 地形类型的分带（与既有示例同一套阈值口径）。
fn terrain_kind(t: f64) -> u8 {
    if t < 0.30 {
        0 // 深水
    } else if t < 0.38 {
        1 // 浅水
    } else if t < 0.46 {
        2 // 沙
    } else if t < 0.66 {
        3 // 草
    } else if t < 0.78 {
        4 // 林
    } else if t < 0.87 {
        5 // 丘
    } else if t < 0.94 {
        6 // 山
    } else {
        7 // 雪
    }
}
