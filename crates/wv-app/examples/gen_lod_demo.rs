//! 一次性工具：生成 `examples/lod-demo` 测试工程（LOD 分档渲染的验收数据）。
//!
//! 用法：`cargo run -p wv-app --example gen_lod_demo`
//!
//! ## 为什么手搓一个新示例而不是改 minimal-world
//!
//! LOD 验收（v0.3 优化项 A）需要**世界级视野下「城市 / 点标」混排**的数据：
//! 4 座有范围的城市（`w/h` 齐备 → `is_city`）+ 6 个无范围的点标（关隘、渡口…）。
//! minimal-world 只有城市，xinhai-1911 的世界级地点全是城市——都筛不出差异。
//!
//! ## 生成方式
//!
//! 不手写 YAML：用 Rust 构建 `wv_core::World`，走 `wv_io::Project::save_world`
//! 的标准保存链路落盘（每实体一文件 §4.5、terrain.bin 经 `write_terrain_if_any`
//! 写出并回写 RasterRef）——生成物天然与加载器 schema 一致（round-trip 同源）。

use wv_core::distance::{Place, PlaceEdge, Vehicle};
use wv_core::event::{Event, EventLocation};
use wv_core::id::Id;
use wv_core::map::{Extent, Layer, LayerKind, MapLevel, MapNode};
use wv_core::time::{Anchor, Calendar, TimeSpan, Timeline};
use wv_core::{World, WorldMeta};
use wv_terrain::generate::Perlin;

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/lod-demo".into());
    std::fs::create_dir_all(&root).expect("create project dir");

    let meta = WorldMeta {
        name: "LOD 演示世界".into(),
        version: "0.1".into(),
        schema_version: 2,
        root_map: Id::new("map_world").unwrap(),
        timeline_range: TimeSpan {
            from: 1,
            to: Some(180_001),
        },
        template: None,
    };

    // 先写 world.yaml（Project::open 的准入凭证），再走标准保存链路。
    let world_yaml = serde_yaml::to_string(&meta).expect("serialize meta");
    std::fs::write(
        std::path::Path::new(&root).join("world.yaml"),
        format!("# LOD 演示工程（gen_lod_demo 生成）· schema v2\n{world_yaml}"),
    )
    .expect("write world.yaml");

    let world = build_world(meta);
    let mut project = wv_io::Project::open(&root).expect("open project");
    // 空 dirty = 全量写（见 project.rs save_world 的匹配语义）。
    project.save_world(&world, &[]).expect("save world");

    let n_city = world
        .distances
        .places
        .iter()
        .filter(|p| p.w.is_some() && p.h.is_some())
        .count();
    println!(
        "written {root}: maps={} places={} (cities={n_city}) edges={} vehicles={} events={}",
        world.maps.len(),
        world.distances.places.len(),
        world.distances.edges.len(),
        world.distances.vehicles.len(),
        world.events.len(),
    );
    println!("验收：`wv gui -m {root}`，拉远到国家级视野应只剩 4 座城市点标。");
}

fn build_world(meta: WorldMeta) -> World {
    let mut world = World {
        meta: Some(meta),
        ..World::default()
    };

    // ---- 地图树：单张世界图 4096² --------------------------------------
    world.maps.push(MapNode {
        id: Id::new("map_world").unwrap(),
        name: "主世界".into(),
        level: MapLevel::World,
        parent: None,
        children: vec![],
        icons: vec![],
        layers: vec![Layer {
            id: Id::new("layer_terrain").unwrap(),
            name: "地形".into(),
            kind: LayerKind::Terrain,
            visible: true,
            locked: false,
            opacity: 1.0,
            z: 0,
            source: Some("maps/map_world/terrain.bin".into()),
        }],
        // 世界层没有父图，落点恒为 (0,0)（见 `Extent::origin` 文档）。
        extent: Some(Extent::sized(4096.0, 4096.0)),
        raster: None, // save_world 的 write_terrain_if_any 会生成并回写
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    });

    // ---- 地形：西陆东海（1024²，确定性 Perlin）---------------------------
    // 方案 B（配瓦片重采样下限放宽到 1）：格数翻倍 → 海岸线台阶减半；
    // 纹理预算不变（res = 4096/1024 = 4px/格，整图仍 4096²）。
    let (w, h) = (1024u32, 1024u32);
    let perlin = Perlin::new(2026_1013);
    let mut cells = Vec::with_capacity((w * h) as usize);
    // 频率按格密度同步减半，保持「大陆尺度」特征不变（否则 1024² 下
    // 同样的 0.045 频率会让海岸线碎成噪点）。
    let freq = 0.045 * 512.0 / w as f64;
    for y in 0..h {
        for x in 0..w {
            let n = perlin.fbm(x as f64 * freq, y as f64 * freq, 5, 2.0, 0.5);
            let t = ((n + 1.0) / 2.0).clamp(0.0, 1.0);
            // 东高西低反转 xinhai 的梯度：西平原（绿），东海（蓝），
            // 让「国家级视野」下东西分带肉眼可辨。
            let east = (x as f64 / w as f64) * 1.2;
            let t = (t * 0.55 + east * 0.45).clamp(0.0, 1.0);
            let kind = if t < 0.30 {
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
            };
            cells.push(channels(kind, (t * 2000.0) as i16));
        }
    }
    world.set_terrain(
        Id::new("map_world").unwrap(),
        wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        },
    );

    // ---- 距离图：4 城（is_city）+ 6 点标（!is_city）--------------------
    // 城市有 w/h；点标只有锚点坐标（AGENTS §「双重身份」：None = 点 icon）。
    let city = |id: &str, name: &str, x: f32, y: f32| Place {
        line: None,
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(Id::new("map_world").unwrap()),
        x: Some(x),
        y: Some(y),
        w: Some(80.0),
        h: Some(60.0),
        feature: None,
        biome: None,
        envelope: None,
        rank: None,
    };
    let dot = |id: &str, name: &str, x: f32, y: f32| Place {
        line: None,
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(Id::new("map_world").unwrap()),
        x: Some(x),
        y: Some(y),
        w: None,
        h: None,
        feature: None,
        biome: None,
        envelope: None,
        rank: None,
    };
    world.distances.places = vec![
        city("place_west_city", "西陵城", 400.0, 600.0),
        city("place_north_city", "北原城", 2400.0, 700.0),
        city("place_east_city", "东港城", 3400.0, 2200.0),
        city("place_south_city", "南泽城", 1200.0, 3200.0),
        dot("place_ford", "青石渡", 1500.0, 1100.0),
        dot("place_pass", "落雁隘", 2000.0, 1500.0),
        dot("place_lighthouse", "灯塔角", 3700.0, 3400.0),
        dot("place_oasis", "月泉绿洲", 800.0, 2200.0),
        dot("place_ruin", "古城遗迹", 2800.0, 2900.0),
        dot("place_beacon", "烽火台", 1700.0, 2500.0),
    ];

    // 边：城市环线 + 各点标挂到最近城市。
    let edge = |id: &str, a: &str, b: &str, km: f32, terrain: &str| PlaceEdge {
        rank: None,
        id: Id::new(id).unwrap(),
        a: Id::new(a).unwrap(),
        b: Id::new(b).unwrap(),
        distance_km: km,
        terrain: terrain.into(),
        bidirectional: true,
    };
    world.distances.edges = vec![
        edge(
            "edge_west_north",
            "place_west_city",
            "place_north_city",
            520.0,
            "plain",
        ),
        edge(
            "edge_north_east",
            "place_north_city",
            "place_east_city",
            610.0,
            "hill",
        ),
        edge(
            "edge_east_south",
            "place_east_city",
            "place_south_city",
            780.0,
            "plain",
        ),
        edge(
            "edge_south_west",
            "place_south_city",
            "place_west_city",
            430.0,
            "plain",
        ),
        edge("edge_ford", "place_west_city", "place_ford", 120.0, "plain"),
        edge("edge_pass", "place_north_city", "place_pass", 90.0, "hill"),
        edge(
            "edge_lighthouse",
            "place_east_city",
            "place_lighthouse",
            150.0,
            "waterway",
        ),
        edge(
            "edge_oasis",
            "place_west_city",
            "place_oasis",
            170.0,
            "plain",
        ),
        edge("edge_ruin", "place_east_city", "place_ruin", 110.0, "plain"),
        edge(
            "edge_beacon",
            "place_south_city",
            "place_beacon",
            80.0,
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

    // ---- 时间轴 + 2 个事件 ---------------------------------------------
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
            to: Some(180_001),
        },
        anchors: vec![
            Anchor {
                id: Id::new("anchor_founding").unwrap(),
                name: "西陵城建城".into(),
                day: 1,
            },
            Anchor {
                id: Id::new("anchor_east_port").unwrap(),
                name: "东港开埠".into(),
                day: 721,
            },
        ],
    });

    world.events = vec![
        Event {
            id: Id::new("evt_west_founding").unwrap(),
            name: "西陵城建城".into(),
            span: TimeSpan {
                from: 1,
                to: Some(10),
            },
            location: Some(EventLocation {
                place: Id::new("place_west_city").unwrap(),
                x: None,
                y: None,
                w: None,
                h: None,
            }),
            participants: vec![],
            narrative: None,
            outcome: Some("新历元年，西陵城依山而立。".into()),
            effects: vec![],
            narrations: vec![],
        },
        Event {
            id: Id::new("evt_east_port").unwrap(),
            name: "东港开埠".into(),
            span: TimeSpan {
                from: 721,
                to: Some(760),
            },
            location: Some(EventLocation {
                place: Id::new("place_east_city").unwrap(),
                x: None,
                y: None,
                w: None,
                h: None,
            }),
            participants: vec![],
            narrative: None,
            outcome: Some("东港城开放海贸，商船云集。".into()),
            effects: vec![],
            narrations: vec![],
        },
    ];

    world
}

/// 旧 kind 槽位 → 三通道（与 `wv_io::terrain_bin::migrate_v1_kind` 同表）。
fn channels(kind: u8, height: i16) -> wv_core::terrain::TerrainCellData {
    let (surface, biome) = match kind {
        0 | 1 => (0u8, 0u8),
        2 => (1, 5),     // 沙 → 沙漠
        3 => (1, 3),     // 草原
        4 => (1, 1),     // 森林
        5..=7 => (1, 0), // 丘/山/雪：高度派生
        _ => (1, 0),
    };
    wv_core::terrain::TerrainCellData {
        height,
        surface,
        biome,
    }
}
