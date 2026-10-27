//! 一次性工具：生成 `examples/small-demo` —— 「内容轴 × 模式矩阵」的最小夹具。
//!
//! 用法：`cargo run -p wv-app --example gen_small_demo`
//!
//! ## 为什么不复用既有示例
//!
//! 现有 8 个示例没有一个能在「小体量、开得快、每条内容轴独立」的条件下做
//! 目视验收：
//!
//! - `layer-showcase` 是**图层体系**验收专用（1024² 世界 + 叙事数据）；
//! - `lod-demo` 只服务分档验收；
//! - `guangdong` 75 MB 真实数据，问题淹没在规模里。
//!
//! 本工程把「地形 / 道路 / 河流 / POI」四条内容轴各切成一张 256² 专用图，
//! 外加一张综合图：**一张图只强测一件事**，否则「有问题」时分不清是谁的锅。
//! 验收判据见 `examples/small-demo/README.md`。
//!
//! ## 两个刻意的口径（决定数据长什么样）
//!
//! 1. **幅面 4096²、栅格 256²**（cell = 16 wu）。LOD 阈值
//!    （`LOD_NATION_SCALE = 3.0` / `LOD_REGION_SCALE = 1.0`）是绝对 scale，
//!    小幅面图 fit 后天然落在 Edit 档，`rank` 门槛根本测不出来；幅面拉到
//!    4096 才能让「拉远看主干、拉近看支线」在真实档位上发生。
//! 2. **`template: None`**。派生河（ADR-0037 D5）的种子是**工程级**字段，
//!    一旦声明，每张图都会长出河；本工程只要**作者手绘河**（ADR-0046），
//!    所以不声明模板，河全部走 `Place.line`，纯 YAML、零二进制。
//!
//! ## 生成方式
//!
//! 与 `gen_layer_showcase` 同款：用 Rust 构建 `wv_core::World`，走
//! `wv_io::Project::save_world` 标准链路落盘（每实体一文件 §4.5、
//! terrain.bin 由 `write_terrain_if_any` 写出并回写 RasterRef）——
//! 生成物与加载器 schema 天然一致（round-trip 同源）。

use wv_core::distance::{Place, PlaceEdge, PoiKind, Vehicle};
use wv_core::id::Id;
use wv_core::map::{
    Extent, GeometryData, GeometrySpan, IconShape, IconSpec, Layer, LayerKind, MapLevel, MapNode,
};
use wv_core::model::faction::Faction;
use wv_core::time::{Anchor, Calendar, TimeSpan, Timeline};
use wv_core::{Color, EntityKind, LabelLayerConfig, World, WorldMeta};
use wv_terrain::generate::Perlin;

/// 图幅边长（世界单位）。与既有世界图同口径，LOD 阈值才有意义。
const EXTENT: f32 = 4096.0;
/// 地形栅格边长（格）。**实测**：terrain.bin 单图 54–75 KB（均值 ≈ 62 KB，落盘
/// 走压缩），六张图合计 370 430 B ≈ 0.36 MB——不是「256² × 4 B = 256 KB」。
const GRID: u32 = 256;

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/small-demo".into());
    std::fs::create_dir_all(&root).expect("create project dir");

    let meta = WorldMeta {
        name: "小样张（内容轴 × 模式矩阵）".into(),
        version: "0.1".into(),
        schema_version: 2,
        root_map: Id::new("map_index").unwrap(),
        timeline_range: TimeSpan {
            from: 1,
            to: Some(101),
        },
        // 刻意不声明模板：派生河是工程级种子，会污染「只测手绘河」的口径。
        template: None,
    };

    // 先写 world.yaml（Project::open 的准入凭证），再走标准保存链路。
    let world_yaml = serde_yaml::to_string(&meta).expect("serialize meta");
    std::fs::write(
        std::path::Path::new(&root).join("world.yaml"),
        format!("# 小样张工程（gen_small_demo 生成）· schema v2\n{world_yaml}"),
    )
    .expect("write world.yaml");

    let world = build_world(meta);
    let mut project = wv_io::Project::open(&root).expect("open project");
    // 空 dirty = 全量写（见 project.rs save_world 的匹配语义）。
    project.save_world(&world, &[]).expect("save world");

    // 标签层配置：只投影 Place（本工程没有势力 / 区域标签需求）。
    let labels = LabelLayerConfig {
        entity_kinds: vec![EntityKind::Place],
        ..LabelLayerConfig::default()
    };
    for map in ["map_index", "t_road", "t_poi", "t_combo"] {
        project
            .write_labels(&format!("maps/{map}/labels.yaml"), &labels)
            .expect("write labels");
    }

    // 空目录占位：本工程不测人物 / 关系，但校验器会提示「缺少预期目录」——
    // 补最小合法文件消掉提示（V1：提示不阻止，收干净而已）。
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
        "written {root}: maps={} places={} edges={} rivers(line)={} icons={}",
        world.maps.len(),
        world.distances.places.len(),
        world.distances.edges.len(),
        world
            .distances
            .places
            .iter()
            .filter(|p| p.line.is_some())
            .count(),
        world.maps.iter().map(|m| m.icons.len()).sum::<usize>(),
    );
    verify(&world);
    println!("验收：`wv gui -m {root}`，演示点逐项见 examples/small-demo/README.md。");
}

/// 生成即验证：本夹具的每条断言都是 README 里写给人看的那几条的机器版。
///
/// **期望值独立给出**（不引用渲染层常量处的同一来源）：河长按折线独立量、
/// 档位阈值按 ADR-0047 的 0.40 / 0.15 手写、前缀切片的段数按 rank 现数。
fn verify(world: &World) {
    // 1. 所有子图必须能从根沿 children 走通（drill_to 是 BFS，孤图进不去）。
    let root = world.meta.as_ref().expect("必须有 meta").root_map.clone();
    let mut seen = vec![root.clone()];
    let mut i = 0;
    while i < seen.len() {
        let cur = seen[i].clone();
        i += 1;
        let node = world
            .maps
            .iter()
            .find(|m| m.id == cur)
            .unwrap_or_else(|| panic!("children 指向不存在的图: {cur:?}"));
        for c in &node.children {
            if !seen.contains(c) {
                seen.push(c.clone());
            }
        }
    }
    assert_eq!(
        seen.len(),
        world.maps.len(),
        "有图不可从根到达（drill 进不去）：{:?}",
        world
            .maps
            .iter()
            .filter(|m| !seen.contains(&m.id))
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>()
    );

    // 2. 每张声明了地形图层的图，都必须真有栅格（validate 不查这一项）。
    for m in &world.maps {
        if m.layers.iter().any(|l| l.kind == LayerKind::Terrain) {
            assert!(
                world.terrain_of(&m.id).is_some(),
                "{} 声明了地形图层但没有栅格",
                m.id.as_str()
            );
        }
    }

    // 3. 手绘河 rank = 长度 ÷ 图幅宽，按 ADR-0047 D5 的三档折算。
    let expect = [
        ("rv_main", 3),
        ("rv_flush", 3),
        ("rv_short", 3),
        ("rv_trib", 2),
        ("rv_brook", 1),
        ("rv_delta_a", 1),
        ("rv_delta_b", 1),
        ("rv_delta_c", 1),
    ];
    for (id, want) in expect {
        let p = world
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == id)
            .unwrap_or_else(|| panic!("缺少手绘河 {id}"));
        let line = p.line.as_ref().expect("手绘河必须有 line");
        let pts: Vec<(f32, f32)> = line.chunks(2).map(|c| (c[0], c[1])).collect();
        let len: f32 = pts
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum();
        let frac = len / EXTENT;
        let got = if frac >= 0.40 {
            3
        } else if frac >= 0.15 {
            2
        } else {
            1
        };
        assert_eq!(
            got, want,
            "{id} 长度占比 {frac:.3} → rank {got}，期望 {want}"
        );
        // 逐条打印实测长度：河口坐标一挪，长度就变，得能一眼看出还剩多少余量。
        println!("  {id}: len={len:.0} wu frac={frac:.4} rank={got}");
        // 手绘河必须显式带锚点（PlaceIndex::build 只认 x/y）。
        assert!(
            p.x.is_some() && p.y.is_some(),
            "{id} 缺 x/y，渲染层会丢整条"
        );
    }

    // 4. 路网三档前缀切片：Edit 全量 / Region ≥2 / Nation ≥3。
    let road_ranks: Vec<u8> = world
        .distances
        .edges
        .iter()
        .filter(|e| {
            world
                .distances
                .places
                .iter()
                .find(|p| p.id == e.a)
                .map(|p| p.map.as_ref().map(|m| m.as_str()) == Some("t_road"))
                .unwrap_or(false)
        })
        .map(|e| e.rank.expect("t_road 的边必须显式给 rank"))
        .collect();
    let edit = road_ranks.len();
    let region = road_ranks.iter().filter(|r| **r >= 2).count();
    let nation = road_ranks.iter().filter(|r| **r >= 3).count();
    assert_eq!((edit, region, nation), (8, 6, 3), "t_road 档位切片段数不对");

    // 5. 河口三态（**cell 级**，不靠注释自证）：越岸 / 压线 / 留缝。
    //    判据直接读 `cells[i].surface`（0 = 水，**含浅水**；1 = 陆），不推断高度：
    //    `channels_of` 把 kind 0|1 都记 surface=0，水面西界因此在 gx≈0.51，
    //    按「深水 t<0.30」估的 0.58 靠东约 300 wu——D1 就是这么漏过去的。
    //    噪声让岸线逐行波动 ±1 格，故断言点一律选在实测过的行上。
    let td = world
        .terrain_of(&Id::new("t_river").unwrap())
        .expect("t_river 必须有栅格");
    let (tw, th) = (td.width as usize, td.height as usize);
    let surface_at = |x: f32, y: f32| -> u8 {
        let cx = ((x / EXTENT) * tw as f32) as usize;
        let cy = ((y / EXTENT) * th as f32) as usize;
        td.cells[cy * tw + cx].surface
    };
    let river_end = |id: &str| -> (f32, f32) {
        let p = world
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == id)
            .unwrap_or_else(|| panic!("缺少手绘河 {id}"));
        let l = p.line.as_ref().expect("手绘河必须有 line");
        (l[l.len() - 2], l[l.len() - 1])
    };

    // 越岸：末点在深水，向东 600 wu 仍是水。
    let (mx, my) = river_end("rv_main");
    assert_eq!(
        surface_at(mx, my),
        0,
        "rv_main 末点 ({mx}, {my}) 应为水，实测 surface={}（单元格 ({}, {})）",
        surface_at(mx, my),
        ((mx / EXTENT) * tw as f32) as usize,
        ((my / EXTENT) * th as f32) as usize
    );
    assert_eq!(
        surface_at(mx + 600.0, my),
        0,
        "rv_main 末点向东 600 wu（x={}）应为水（越岸），实测 surface={}",
        mx + 600.0,
        surface_at(mx + 600.0, my)
    );

    // 压线：末点在水面格里，向西 200 wu 已回到陆地。
    let (fx, fy) = river_end("rv_flush");
    assert_eq!(
        surface_at(fx, fy),
        0,
        "rv_flush 末点 ({fx}, {fy}) 应压在水面格内，实测 surface={}（水面西界靠东了）",
        surface_at(fx, fy)
    );
    assert_eq!(
        surface_at(fx - 200.0, fy),
        1,
        "rv_flush 末点向西 200 wu（x={}）应为陆，实测 surface={}（河口陷进水里了）",
        fx - 200.0,
        surface_at(fx - 200.0, fy)
    );

    // 留缝：末点在陆地，向东 250 wu 已是水面。
    let (sx, sy) = river_end("rv_short");
    assert_eq!(
        surface_at(sx, sy),
        1,
        "rv_short 末点 ({sx}, {sy}) 应为陆，实测 surface={}（末点落水里了）",
        surface_at(sx, sy)
    );
    assert_eq!(
        surface_at(sx + 250.0, sy),
        0,
        "rv_short 末点向东 250 wu（x={}）应为水，实测 surface={}（缝开太大，够不到水）",
        sx + 250.0,
        surface_at(sx + 250.0, sy)
    );

    println!(
        "verify: 图 {seen_len} 张可达 / 手绘河 8 条 rank 达标 / 路网切片 8-6-3 / 河口三态 越岸·压线·留缝",
        seen_len = seen.len()
    );
}

// ---------------------------------------------------------------------------
// 图层与节点
// ---------------------------------------------------------------------------

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

fn terrain_layer(map: &str) -> Layer {
    let mut l = layer(&format!("l_{map}_terrain"), "地形", LayerKind::Terrain);
    l.source = Some(format!("maps/{map}/terrain.bin"));
    l
}

fn label_layer(map: &str) -> Layer {
    let mut l = layer(&format!("l_{map}_labels"), "标注", LayerKind::Label);
    l.source = Some(format!("maps/{map}/labels.yaml"));
    l
}

fn node(id: &str, name: &str, level: MapLevel, parent: Option<&str>) -> MapNode {
    MapNode {
        id: Id::new(id).unwrap(),
        name: name.into(),
        level,
        parent: parent.map(|p| Id::new(p).unwrap()),
        children: vec![],
        icons: vec![],
        layers: vec![],
        extent: Some(Extent::sized(EXTENT, EXTENT)),
        raster: None, // save_world 的 write_terrain_if_any 会生成并回写
        geographic_source: None,
        time_span: None,
        unit_per_km: None,
        orientation: 0.0,
        base: None,
        outline_polygon: None,
    }
}

/// 索引页图标：`anchor` 归一化 [0,1]²；`drill_into = Some(id)` 即钻入入口。
fn icon_at(
    id: &str,
    x: f32,
    y: f32,
    shape: IconShape,
    size: Option<f32>,
    drill_into: Option<&str>,
) -> IconSpec {
    IconSpec {
        id: Id::new(id).unwrap(),
        anchor: Some([x / EXTENT, y / EXTENT]),
        shape,
        size,
        color: None,
        drill_into: drill_into.map(|m| Id::new(m).unwrap()),
    }
}

// ---------------------------------------------------------------------------
// 世界装配
// ---------------------------------------------------------------------------

fn build_world(meta: WorldMeta) -> World {
    let mut world = World {
        meta: Some(meta),
        ..World::default()
    };

    let maps: [(&str, &str); 5] = [
        ("t_terrain", "地貌五带"),
        ("t_road", "路网与分支"),
        ("t_river", "水系与出海口"),
        ("t_poi", "POI 与图标"),
        ("t_combo", "综合小样张"),
    ];

    // ---- 0. 索引页：只承担「一次点进任一内容轴」----------------------------
    // 图标 ≠ POI：五个钻入图标各有同名 Place（走 POI + 标签），第 6 个图标
    // 刻意没有任何 Place——它只在图标层出现，用来钉住「两套点不是一套」。
    let idx_pos = [
        (900.0, 1100.0),
        (2900.0, 1100.0),
        (900.0, 2900.0),
        (2900.0, 2900.0),
        (1950.0, 2000.0),
    ];
    let idx_shapes = [
        IconShape::Diamond,
        IconShape::Circle,
        IconShape::Square,
        IconShape::Star,
        IconShape::Diamond,
    ];
    let mut index = node("map_index", "小样张索引", MapLevel::World, None);
    index.children = maps.iter().map(|(id, _)| Id::new(*id).unwrap()).collect();
    index.layers = vec![terrain_layer("map_index"), label_layer("map_index")];
    index.icons = maps
        .iter()
        .enumerate()
        .map(|(i, (id, _))| {
            let (x, y) = idx_pos[i];
            icon_at(&format!("icon_{id}"), x, y, idx_shapes[i], None, Some(id))
        })
        .chain(std::iter::once(icon_at(
            "icon_decoy_no_place",
            1500.0,
            3600.0,
            IconShape::Cross,
            Some(10.0),
            None,
        )))
        .collect();
    world.maps.push(index);

    // ---- 1..5. 五张内容轴子图 --------------------------------------------
    for (id, name) in maps {
        let mut m = node(id, name, MapLevel::Region, Some("map_index"));
        m.layers = layers_for(id);
        world.maps.push(m);
    }

    // ---- 索引页 POI：与钻入图标同锚点，rank 3 + w/h（有范围 → 地标）--------
    let idx_places = [
        ("place_idx_terrain", "地貌五带"),
        ("place_idx_road", "路网与分支"),
        ("place_idx_river", "水系与出海口"),
        ("place_idx_poi", "POI 与图标"),
        ("place_idx_combo", "综合小样张"),
    ];
    for (i, (pid, pname)) in idx_places.iter().enumerate() {
        let (x, y) = idx_pos[i];
        world.distances.places.push(Place {
            kind: Some(PoiKind::Settlement),
            line: None,
            id: Id::new(*pid).unwrap(),
            name: (*pname).into(),
            map: Some(Id::new("map_index").unwrap()),
            x: Some(x),
            y: Some(y),
            w: Some(240.0),
            h: Some(150.0),
            feature: None,
            biome: None,
            envelope: None,
            rank: Some(3),
            line_kind: None,
            line_width_m: None,
        });
    }

    build_terrain_map(&mut world);
    build_road_map(&mut world);
    build_river_map(&mut world);
    build_poi_map(&mut world);
    build_combo_map(&mut world);

    // 索引页自己的底图：低起伏平原。**必须写**——索引页的 `Layer.source`
    // 已经声明了 `maps/map_index/terrain.bin`，而 `validate` 不检查该文件是否存在
    // （只在 raster 字段上做校验），漏写也能「校验通过」，打开却是空底。
    write_terrain(&mut world, &Id::new("map_index").unwrap(), |_gx, _gy, n| {
        (0.56 + n * 0.06).clamp(0.0, 1.0)
    });

    // ---- 时间轴：只放两个路标，够拖就行（本工程不测事件窗口）--------------
    world.timeline = Some(Timeline {
        calendar: Calendar {
            kind: wv_core::time::CalendarKind::Simple,
            prefix: Some("样张".into()),
            start_year: 1,
            days_per_year: 360,
            days_per_month: 30,
        },
        timeline_range: TimeSpan {
            from: 1,
            to: Some(101),
        },
        anchors: vec![
            Anchor {
                id: Id::new("anchor_baseline").unwrap(),
                name: "基线".into(),
                day: 1,
            },
            Anchor {
                id: Id::new("anchor_later").unwrap(),
                name: "后续".into(),
                day: 60,
            },
        ],
    });

    // 距离图没有载具时行程换算无处落，补一只（与既有示例同款）。
    world.distances.vehicles = vec![Vehicle {
        id: Id::new("veh_horse").unwrap(),
        name: "驿马".into(),
        instant: false,
        speed_kmh: Some(10.0),
        daily_hours: Some(8.0),
        terrain_modifiers: [("plain".to_string(), 1.2), ("forest".to_string(), 0.6)]
            .into_iter()
            .collect(),
        requires_endpoints: false,
        requirements: vec![],
    }];

    world
}

fn layers_for(map: &str) -> Vec<Layer> {
    let mut out = vec![terrain_layer(map)];
    match map {
        "t_terrain" => {}
        "t_road" => {
            out.push(layer(&format!("l_{map}_road"), "道路", LayerKind::Road));
            out.push(layer(&format!("l_{map}_place"), "节点", LayerKind::Place));
            out.push(label_layer(map));
        }
        "t_river" => {}
        "t_poi" => {
            out.push(layer(&format!("l_{map}_place"), "地点", LayerKind::Place));
            out.push(label_layer(map));
        }
        "t_combo" => {
            out.push(layer(&format!("l_{map}_road"), "道路", LayerKind::Road));
            out.push(layer(&format!("l_{map}_place"), "地点", LayerKind::Place));
            out.push(layer(
                &format!("l_{map}_influence"),
                "疆域",
                LayerKind::Influence,
            ));
            out.push(label_layer(map));
        }
        _ => {}
    }
    out
}

// ---------------------------------------------------------------------------
// 图 1：地貌五带 —— 只测地形着色 / 等高线 / 海岸，路与点一律不放
// ---------------------------------------------------------------------------

fn build_terrain_map(world: &mut World) {
    let map = Id::new("t_terrain").unwrap();
    let (w, h) = (GRID, GRID);
    let perlin = Perlin::new(2026_1023);
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let gx = x as f64 / w as f64;
            let gy = y as f64 / h as f64;
            let n = perlin.fbm(x as f64 * 0.04, y as f64 * 0.04, 4, 2.0, 0.5) * 0.5;
            // 西 → 东的五带：深水 / 沙 / 草 / 林 / 丘 / 山；东侧 0.82 处一条
            // 南北向**山脊**（再抬 0.20，上到雪线），西南 0.42/0.72 处一处**湖盆**。
            let base = 0.24 + gx * 0.70;
            let ridge = 0.20 * gauss(gx, 0.82, 0.05);
            let lake_w = gauss2(gx, gy, 0.42, 0.72, 0.07, 0.06);
            let lake = 0.34 * lake_w;
            let t = (base + ridge - lake + n * 0.05).clamp(0.0, 1.0);
            let kind = terrain_kind(t);
            // 湖盆里的水面记 Biome::Lake(15)，与海（14）分开——cell 检视器
            // 里能读出来这是湖，不是海侵。
            let (surface, biome) = if kind <= 1 && lake_w > 0.45 {
                (0u8, 15u8)
            } else {
                channels_of(kind)
            };
            cells.push(wv_core::terrain::TerrainCellData {
                height: (t * 2000.0) as i16,
                surface,
                biome,
            });
        }
    }
    world.set_terrain(
        map,
        wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        },
    );
}

// ---------------------------------------------------------------------------
// 图 2：路网 —— 分支 / 等级 / 穿林，三个问题一屏可见
// ---------------------------------------------------------------------------

fn build_road_map(world: &mut World) {
    let map = Id::new("t_road").unwrap();
    write_terrain(
        world,
        &map,
        // 西半平原、东侧一条南北向密林带（穿林段由此经过）。
        // 林带用高斯而不是硬区间：硬 `if` 会在色块模式里留下矩形接缝，
        // 看上去像渲染错误，掩盖真正要验的东西。
        |gx, _gy, n| (0.55 + gauss(gx, 0.70, 0.09) * 0.20 + n * 0.04).clamp(0.0, 1.0),
    );

    // 节点：两个路口（rank 0——GLOSSARY 明写「路网交点」，当前无任何示例覆盖）、
    // 两座城（rank 3，带范围）、两座镇（rank 2）、一个村（rank 1）。
    let p = |id: &str, name: &str, x: f32, y: f32, rank: u8, city: bool| Place {
        kind: city.then_some(PoiKind::Settlement),
        line: None,
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(map.clone()),
        x: Some(x),
        y: Some(y),
        w: city.then_some(180.0),
        h: city.then_some(120.0),
        feature: None,
        biome: None,
        envelope: None,
        rank: Some(rank),
        line_kind: None,
        line_width_m: None,
    };
    world.distances.places.extend(vec![
        p("rd_west_city", "西口城", 550.0, 1900.0, 3, true),
        p("rd_east_city", "东林城", 3450.0, 1850.0, 3, true),
        p("rd_cross", "十字路口", 1700.0, 1900.0, 0, false),
        p("rd_tee", "丁字路口", 2800.0, 2650.0, 0, false),
        p("rd_north_town", "北岗镇", 1700.0, 700.0, 2, false),
        p("rd_south_town", "南铺镇", 1650.0, 3200.0, 2, false),
        p("rd_village", "林边村", 2800.0, 3620.0, 1, false),
    ]);

    // 边：rank 显式声明（不靠 distance_km 折算），三档可见性一屏对照。
    // 穿林段 = rd_cross → rd_east_city，横穿 0.58–0.82 的密林带。
    let e = |id: &str, a: &str, b: &str, km: f32, terrain: &str, rank: u8| PlaceEdge {
        rank: Some(rank),
        id: Id::new(id).unwrap(),
        a: Id::new(a).unwrap(),
        b: Id::new(b).unwrap(),
        distance_km: km,
        terrain: terrain.into(),
        bidirectional: true,
        derived: false,
    };
    world.distances.edges.extend(vec![
        e(
            "rd_e_trunk_w",
            "rd_west_city",
            "rd_cross",
            240.0,
            "plain",
            3,
        ),
        e(
            "rd_e_trunk_e",
            "rd_cross",
            "rd_east_city",
            320.0,
            "forest",
            3,
        ),
        e("rd_e_north", "rd_cross", "rd_north_town", 180.0, "plain", 2),
        e("rd_e_south", "rd_cross", "rd_south_town", 190.0, "plain", 2),
        e("rd_e_diag", "rd_cross", "rd_tee", 180.0, "forest", 3),
        e(
            "rd_e_tee_east",
            "rd_tee",
            "rd_east_city",
            120.0,
            "forest",
            2,
        ),
        e("rd_e_tee_south", "rd_tee", "rd_village", 160.0, "forest", 1),
        e(
            "rd_e_local",
            "rd_south_town",
            "rd_village",
            220.0,
            "plain",
            1,
        ),
    ]);
}

// ---------------------------------------------------------------------------
// 图 3：水系 —— 手绘河（ADR-0046）+ 河口三态 + rank 门槛
// ---------------------------------------------------------------------------

/// 主河走线（图内局部 wu）。地形按它**刻谷**——河要看得出「在山谷里流」，
/// 而不是「一根线压在山上」。刻谷权重是 `exp(-(d/σ)²)`，σ ≈ 260 wu、
/// 半深全宽 ≈ 430 wu（见 `build_river_map` 的闭包注释）。
const RIVER_MAIN: &[(f32, f32)] = &[
    (400.0, 900.0),
    (1200.0, 1200.0),
    (1800.0, 1600.0),
    (2300.0, 2000.0),
    (2700.0, 2300.0),
    (3100.0, 2650.0),
];
/// 即岸河走线（河口**压在岸线上**）。
///
/// 末点 (2070, 2336) 落在水面格里（该行水面西界 x=2064），向西一格即回陆地，
/// 向西 200 wu（x=1870）实测 `surface=1`。中段左右摆 3 次是为长度：端点直线
/// 只有 1395 wu（占比 0.34 → rank 2），摆完后 1774 wu（占比 0.433，安全过 0.40）。
const RIVER_FLUSH: &[(f32, f32)] = &[
    (700.0, 2600.0),
    (1080.0, 2705.0),
    (1355.0, 2285.0),
    (1770.0, 2565.0),
    (2070.0, 2336.0),
];
/// 未及岸河走线（河口**离岸留缝**）。
///
/// 末点 (1950, 3050) 落在陆地格（该行水面西界 x=2096），向东 250 wu（x=2200）
/// 实测 `surface=0`——缝约 146 wu。长度 1747 wu（占比 0.426），同样靠中段摆弯
/// 补足（端点直线只有 1395 wu，占比 0.34 会掉到 rank 2）。
const RIVER_SHORT: &[(f32, f32)] = &[
    (600.0, 3400.0),
    (980.0, 3475.0),
    (1230.0, 3060.0),
    (1655.0, 3300.0),
    (1950.0, 3050.0),
];

fn build_river_map(world: &mut World) {
    let map = Id::new("t_river").unwrap();
    write_terrain(world, &map, |gx, gy, n| {
        // 西高（山/雪）→ 东低（海）。**水面口径按 surface 实测，不按深水估**：
        // `channels_of` 把 kind 0|1 都记 `surface=0`（浅水也算水面，t < 0.38），
        // 所以水面西界在 gx ≈ 0.51（x ≈ 2080 wu），不是按「深水 t<0.30」推出的
        // 0.58 —— 那个值靠东约 300 wu，拿它摆河口会把末点整个埋进水里。
        // 主河沿线刻一道谷：权重 exp(-(d/σ)²)，σ ≈ 260 wu（半深全宽 ≈ 430 wu），
        // 最深处 -0.10；再叠加 ±0.04 的噪声（噪声让岸线逐行波动 ±1 格）。
        let (x, y) = (gx as f32 * EXTENT, gy as f32 * EXTENT);
        let valley = 0.10 * (-(dist_to_poly(RIVER_MAIN, x, y) / 260.0).powi(2)).exp();
        (0.98 - gx * 1.18 - valley as f64 - n * 0.04).clamp(0.0, 1.0)
    });

    // 每条河一个 Place：`line` = 扁平点列（图内局部 wu）。
    // **x/y 必须显式给**——`PlaceIndex::build` 只认 `p.x/p.y`，不走
    // `Place::effective_anchor()`；省掉 x/y 的手绘河在渲染层会被整条丢掉。
    // `rank = 0` 现在只影响**标签档位**（河名只在 Edit 档出现）：点标那条路
    // 已由 G2 修复堵掉（`place_dot_for` 见 `line.is_some()` 直接返回 None），
    // 不必再靠压级别来躲圆点。
    let river = |id: &str, name: &str, pts: &[(f32, f32)]| {
        let flat: Vec<f32> = pts.iter().flat_map(|(x, y)| [*x, *y]).collect();
        let (mx, my) = midpoint(pts);
        Place {
            kind: None,
            line: Some(flat),
            id: Id::new(id).unwrap(),
            name: name.into(),
            map: Some(map.clone()),
            x: Some(mx),
            y: Some(my),
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: Some(0),
            line_kind: None,
            line_width_m: None,
        }
    };

    world.distances.places.extend(vec![
        // 主河：长 ≈ 3200 wu（占比 0.78 → rank 3），**越过水面西界**约 1000 wu
        // （末点 x=3100 已在深水区，向东 600 wu 仍是水）。
        river("rv_main", "澜江", RIVER_MAIN),
        // 即岸：长 ≈ 1774（占比 0.433 → rank 3），河口**压在岸线上**（末点为水、
        // 向西 200 wu 即陆）。
        river("rv_flush", "平口河", RIVER_FLUSH),
        // 未及岸：长 ≈ 1747（占比 0.426 → rank 3），河口**离岸留缝** ≈ 146 wu。
        river("rv_short", "断口河", RIVER_SHORT),
        // 区域河：长 ≈ 1350（占比 0.33 → rank 2），Region 档可见、Nation 档消失。
        river(
            "rv_trib",
            "白水支",
            &[(1400.0, 1000.0), (1700.0, 1400.0), (2300.0, 2000.0)],
        ),
        // 溪流：长 ≈ 390（占比 0.10 → rank 1），只有 Edit 档可见。
        river(
            "rv_brook",
            "细流",
            &[(1100.0, 1700.0), (1300.0, 1780.0), (1450.0, 1850.0)],
        ),
        // 三角洲三叉（无分叉模型：用三条独立短线近似）：
        // 每条 ≈ 160–190 wu（rank 1）——**拉远到 Nation / Region 档会整组消失**。
        river(
            "rv_delta_a",
            "澜江·东汊",
            &[(2700.0, 2300.0), (2820.0, 2400.0)],
        ),
        river(
            "rv_delta_b",
            "澜江·中汊",
            &[(2700.0, 2300.0), (2760.0, 2480.0)],
        ),
        river(
            "rv_delta_c",
            "澜江·西汊",
            &[(2700.0, 2300.0), (2880.0, 2320.0)],
        ),
    ]);
}

// ---------------------------------------------------------------------------
// 图 4：POI 与图标 —— 两套点分开摆，别互相冒充
// ---------------------------------------------------------------------------

fn build_poi_map(world: &mut World) {
    let map = Id::new("t_poi").unwrap();
    write_terrain(world, &map, |_gx, _gy, n| (0.52 + n * 0.03).clamp(0.0, 1.0));

    // 四排 = 四个呈现级别；拉远拉近逐排消失（Nation ≥3 / Region ≥2 / Edit 全见）。
    let rows = [
        (3u8, 900.0, "国家级"),
        (2, 1700.0, "市镇级"),
        (1, 2600.0, "普通点"),
        (0, 3400.0, "网络节点"),
    ];
    for (rank, y, tag) in rows {
        for (i, x) in [900.0_f32, 2000.0, 3100.0].iter().enumerate() {
            world.distances.places.push(Place {
                kind: None,
                line: None,
                id: Id::new(format!("poi_r{rank}_{i}")).unwrap(),
                name: format!("{tag}·{}", i + 1),
                map: Some(map.clone()),
                x: Some(*x),
                y: Some(y),
                w: (rank >= 2).then_some(160.0),
                h: (rank >= 2).then_some(110.0),
                feature: None,
                biome: None,
                envelope: None,
                rank: Some(rank),
                line_kind: None,
                line_width_m: None,
            });
        }
    }

    // 五种 IconShape 各一枚（地图级声明，与上面的 POI 完全无关）。
    let icons = [
        ("icon_diamond", 500.0, 500.0, IconShape::Diamond),
        ("icon_circle", 1500.0, 500.0, IconShape::Circle),
        ("icon_cross", 2500.0, 500.0, IconShape::Cross),
        ("icon_square", 3500.0, 500.0, IconShape::Square),
        ("icon_star", 2000.0, 3800.0, IconShape::Star),
    ];
    let idx = world
        .maps
        .iter()
        .position(|m| m.id == map)
        .expect("t_poi 必须已入表");
    world.maps[idx].icons = icons
        .iter()
        .map(|(id, x, y, shape)| icon_at(id, *x, *y, *shape, Some(14.0), None))
        .collect();
}

// ---------------------------------------------------------------------------
// 图 5：综合 —— 地形 + 路 + 河 + POI + 疆域叠加，8 模式横扫用
// ---------------------------------------------------------------------------

fn build_combo_map(world: &mut World) {
    let map = Id::new("t_combo").unwrap();
    write_terrain(
        world,
        &map,
        // 西侧入海、中东林带、东北丘陵：三种底子叠在一起看叠加是否还成立。
        // 全部用高斯场，避免硬区间在色块模式里留下矩形接缝。
        |gx, gy, n| {
            // 只在 gx < 0.18 入海（`.max(0.0)` 不可省：线性项若全程生效，
            // 东半幅会整片沉到水下，叠加层就无从看了）。
            let sea = -(0.16 - gx).max(0.0) * 3.2;
            let forest = gauss(gx, 0.56, 0.09) * 0.16;
            let hill = gauss2(gx, gy, 0.86, 0.28, 0.11, 0.16) * 0.30;
            (0.58 + sea + forest + hill + n * 0.05).clamp(0.0, 1.0)
        },
    );

    let p = |id: &str, name: &str, x: f32, y: f32, rank: u8, city: bool| Place {
        kind: city.then_some(PoiKind::Settlement),
        line: None,
        id: Id::new(id).unwrap(),
        name: name.into(),
        map: Some(map.clone()),
        x: Some(x),
        y: Some(y),
        w: city.then_some(180.0),
        h: city.then_some(120.0),
        feature: None,
        biome: None,
        envelope: None,
        rank: Some(rank),
        line_kind: None,
        line_width_m: None,
    };
    world.distances.places.extend(vec![
        p("cb_port", "潮口港", 700.0, 2200.0, 3, true),
        p("cb_keep", "南岭关", 3200.0, 1000.0, 3, true),
        p("cb_junction", "三岔口", 2000.0, 1900.0, 0, false),
        p("cb_mill", "水磨镇", 1600.0, 2900.0, 2, false),
        p("cb_hamlet", "林间屯", 2700.0, 3000.0, 1, false),
    ]);

    let e = |id: &str, a: &str, b: &str, km: f32, terrain: &str, rank: u8| PlaceEdge {
        rank: Some(rank),
        id: Id::new(id).unwrap(),
        a: Id::new(a).unwrap(),
        b: Id::new(b).unwrap(),
        distance_km: km,
        terrain: terrain.into(),
        bidirectional: true,
        derived: false,
    };
    world.distances.edges.extend(vec![
        e("cb_e_trunk", "cb_port", "cb_junction", 210.0, "plain", 3),
        e("cb_e_keep", "cb_junction", "cb_keep", 260.0, "forest", 3),
        e("cb_e_mill", "cb_junction", "cb_mill", 120.0, "plain", 2),
        e("cb_e_hamlet", "cb_mill", "cb_hamlet", 90.0, "forest", 1),
    ]);

    // 两条手绘河：一条从丘陵穿林入海，一条支流。
    let river = |id: &str, name: &str, pts: &[(f32, f32)]| {
        let flat: Vec<f32> = pts.iter().flat_map(|(x, y)| [*x, *y]).collect();
        let (mx, my) = midpoint(pts);
        Place {
            kind: None,
            line: Some(flat),
            id: Id::new(id).unwrap(),
            name: name.into(),
            map: Some(map.clone()),
            x: Some(mx),
            y: Some(my),
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: Some(0),
            line_kind: None,
            line_width_m: None,
        }
    };
    world.distances.places.extend(vec![
        river(
            "cb_rv_main",
            "回澜水",
            &[
                (3300.0, 800.0),
                (2800.0, 1300.0),
                (2300.0, 1700.0),
                (1700.0, 2100.0),
                (1100.0, 2300.0),
                (600.0, 2400.0),
            ],
        ),
        river(
            "cb_rv_trib",
            "石涧",
            &[(2500.0, 600.0), (2450.0, 1100.0), (2300.0, 1700.0)],
        ),
    ]);

    // 势力：区间化疆域（P6/P7 口径，与演示主题无关，只为验证叠加层）。
    world.factions = vec![Faction::new(
        Id::new("cb_faction_river").unwrap(),
        "澜江守",
        Color("#4E6EC4".into()),
    )];
    world.faction_geometry = vec![GeometrySpan {
        map: map.clone(),
        owner: Id::new("cb_faction_river").unwrap(),
        span: TimeSpan {
            from: 1,
            to: Some(101),
        },
        geometry: GeometryData::Polygon(vec![
            1400.0, 1200.0, 3000.0, 1200.0, 3000.0, 2500.0, 1400.0, 2500.0,
        ]),
        relation: None,
    }];
}

// ---------------------------------------------------------------------------
// 公共：地形、坐标
// ---------------------------------------------------------------------------

/// 统一的地形写入：闭包给 (归一化 x, 归一化 y, 噪声)，返回 [0,1] 高度占比。
fn write_terrain<F>(world: &mut World, map: &Id, f: F)
where
    F: Fn(f64, f64, f64) -> f64,
{
    let (w, h) = (GRID, GRID);
    let perlin = Perlin::new(2026_1023);
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let gx = x as f64 / w as f64;
            let gy = y as f64 / h as f64;
            let n = perlin.fbm(x as f64 * 0.04, y as f64 * 0.04, 4, 2.0, 0.5) * 0.5;
            let t = f(gx, gy, n).clamp(0.0, 1.0);
            cells.push(channels(terrain_kind(t), (t * 2000.0) as i16));
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

/// 折线中点（与 `Place::anchor_from_line` 同口径：弧长中点，非顶点平均）。
fn midpoint(pts: &[(f32, f32)]) -> (f32, f32) {
    if pts.is_empty() {
        return (EXTENT * 0.5, EXTENT * 0.5);
    }
    if pts.len() == 1 {
        return pts[0];
    }
    let seg: Vec<f32> = pts
        .windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .collect();
    let total: f32 = seg.iter().sum();
    if total <= 0.0 {
        return pts[0];
    }
    let half = total * 0.5;
    let mut acc = 0.0;
    for (i, len) in seg.iter().enumerate() {
        if acc + *len >= half {
            let t = if *len > 0.0 { (half - acc) / *len } else { 0.0 };
            return (
                pts[i].0 + (pts[i + 1].0 - pts[i].0) * t,
                pts[i].1 + (pts[i + 1].1 - pts[i].1) * t,
            );
        }
        acc += *len;
    }
    *pts.last().unwrap()
}

/// 一维高斯（中心 `c`、宽度 `s`）：地貌脊线 / 洼地的权重场。
fn gauss(x: f64, c: f64, s: f64) -> f64 {
    (-((x - c) / s).powi(2)).exp()
}

/// 二维高斯（椭圆洼地）。
fn gauss2(x: f64, y: f64, cx: f64, cy: f64, sx: f64, sy: f64) -> f64 {
    (-(((x - cx) / sx).powi(2) + ((y - cy) / sy).powi(2))).exp()
}

/// 点到折线的最短距离（wu）——用它沿河刻谷。
fn dist_to_poly(pts: &[(f32, f32)], x: f32, y: f32) -> f32 {
    let mut best = f32::MAX;
    for w in pts.windows(2) {
        let (ax, ay) = w[0];
        let (bx, by) = w[1];
        let (dx, dy) = (bx - ax, by - ay);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= f32::EPSILON {
            0.0
        } else {
            (((x - ax) * dx + (y - ay) * dy) / len2).clamp(0.0, 1.0)
        };
        let (px, py) = (ax + dx * t, ay + dy * t);
        best = best.min(((x - px).powi(2) + (y - py).powi(2)).sqrt());
    }
    best
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

/// 旧 kind 槽位 → 三通道。
///
/// **群系编号用冻结表**（`wv_terrain::biome`）：`migrate_v1_kind` 把 5..=7
/// 一律折成 `BIOME_NONE(0)`，原因是「高度派生带无群系」；但那样一来
/// 丘陵 / 山地 / 雪原在色块模式里同色，地貌展示就退化成一条褐色带。
/// 本工程是**展示夹具**，故显式给 12 / 13 / 9，让色块模式能逐带分辨。
fn channels_of(kind: u8) -> (u8, u8) {
    match kind {
        0 | 1 => (0, 14), // 水 → 海洋
        2 => (1, 5),      // 沙 → 沙漠
        3 => (1, 3),      // 草 → 草原
        4 => (1, 1),      // 林 → 森林
        5 => (1, 12),     // 丘 → 丘陵
        6 => (1, 13),     // 山 → 山地
        7 => (1, 9),      // 雪 → 雪原
        _ => (1, 2),      // 兜底：平原
    }
}

/// 旧 kind 槽位 → 三通道（与 `wv_io::terrain_bin::migrate_v1_kind` 同表）。
fn channels(kind: u8, height: i16) -> wv_core::terrain::TerrainCellData {
    let (surface, biome) = channels_of(kind);
    wv_core::terrain::TerrainCellData {
        height,
        surface,
        biome,
    }
}
