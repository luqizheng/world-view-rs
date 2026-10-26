//! `wv-gen-demo` —— 独立 CLI：为 `examples/work-view-test` 生成「地中海 + 北非」底座。
//!
//! 用法：
//!   cargo build -p wv-app --bin wv-gen-demo
//!   ./target/debug/wv-gen-demo.exe <工程目录>        # 缺省 examples/work-view-test
//!
//! 行为分支（自动探测 `<工程>/data/` 下产物）：
//!
//! 有 `coastlines.geojson` + `rivers.geojson` 时：读 GeoJSON（自带零依赖解析器），
//! 栅格化 surface / 高度 / 群系。**河流不再涂格**（ADR-0070 D3 · 计划 Task 12：
//! 河走实体——工程带 `template.rivers_seed`，加载期迁移把种子河转为河线
//! POI；涂格河群系 16 已退役，`rivers.geojson` 只留作 NE 数据在场的触发器）。
//!
//! 没有时：退到 `wv_terrain::climate::generate_cold_north` 冷北程序生成（与
//! `examples/jrpg-world` 同源，先让 demo 可开，NE 数据落盘后自动切真底座）。
//!
//! 写入：
//!   `maps/world_main/terrain.bin`（wv-io，AGENTS §5 唯一二进制例外）
//!   `maps/world_main/index.yaml`（MapNode + raster 引用）
//!   `world.yaml` 的 `template:` 段重写（不动内容层字段）
//!
//! 数据准备（一次性，见 examples/work-view-test/README.md）：
//!   Natural Earth 110m shp → examples/work-view-test/data/ne/convert.py → GeoJSON
//!
//! 边界（不做什么）：
//!   - 零网络（GeoJSON 由 convert.py 离线产出）
//!   - 不碰 distances/ factions/ —— 内容层是作者手作的活

use std::path::{Path, PathBuf};

use wv_core::map::RasterRef;
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_io::terrain_bin::write_terrain;
use wv_terrain::climate::{generate_cold_north, DEFAULT_SEED};

mod geo;

use geo::{F, G};

const DEMO_DIR: &str = "examples/work-view-test";
const DATA_SUB: &str = "data";
const MAP_ID: &str = "world_main";
const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const EXTENT_W: f32 = 4096.0;
const EXTENT_H: f32 = 3072.0;

/// 地中海 + 北非 bbox（经度西→东，纬度南→北）。
const LON: (f64, f64) = (-12.0, 37.0);
const LAT: (f64, f64) = (30.0, 48.0);

fn main() {
    let mut args = std::env::args().peekable();
    let _bin = args.next();
    let mut root = PathBuf::from(DEMO_DIR);
    let mut dump: Option<PathBuf> = None;
    let mut peaks: f64 = 3400.0; // 峰顶高度（米）；RPG 幻想山可拉到 10000
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dump-png" => dump = args.next().map(PathBuf::from),
            "--peaks" => {
                peaks = args
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| *v >= 500.0 && *v <= 30000.0)
                    .unwrap_or(3400.0);
            }
            _ => root = PathBuf::from(a),
        }
    }
    println!("工程目录：{}", root.display());
    let data_dir = root.join(DATA_SUB);
    let map_dir = root.join("maps").join(MAP_ID);
    std::fs::create_dir_all(&map_dir).expect("create maps/world_main");

    let (data, basis) = build_grid(&data_dir, peaks);
    let seed = seed_now();
    let land = data.cells.iter().any(|c| c.surface == 1);
    let kind = if land { "cold_north" } else { "ocean" };

    if let Some(png_path) = &dump {
        dump_debug_png(&data, png_path);
        println!("[debug] biome map -> {}", png_path.display());
    }

    let raster = write_terrain(&root, MAP_ID, &data).expect("write terrain.bin");
    println!(
        "[{basis}] terrain.bin ({}x{}) checksum {:?}",
        raster.width, raster.height, raster.checksum
    );

    write_index(&map_dir, &raster);
    if let Err(e) = patch_template(&root, kind, seed) {
        eprintln!("[warn] world.yaml template 段重写失败：{e}");
    }
    println!("完成。用 `wv gui -m {}` 查看效果。", root.display());
}

fn seed_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(DEFAULT_SEED)
}

/// 探测 NE GeoJSON 产物；缺文件 / 解析失败 → 程序冷北兜底。
fn build_grid(data_dir: &Path, peaks: f64) -> (TerrainData, &'static str) {
    let coast_path = data_dir.join("coastlines.geojson");
    let rivers_path = data_dir.join("rivers.geojson");
    let seas_path = data_dir.join("seas.geojson");
    if !coast_path.exists() || !rivers_path.exists() {
        println!(
            "[fallback] 未找到 {} / {}，走程序冷北生成",
            coast_path.display(),
            rivers_path.display()
        );
        return (program_north(), "program");
    }
    let coast = match geo::read(&coast_path) {
        Ok(f) => f,
        Err(e) => {
            println!("[fallback] {e}；走程序冷北生成");
            return (program_north(), "program");
        }
    };
    // 河流不再栅格化（ADR-0070 D3：河走实体）——rivers.geojson 只在上面的
    // NE 数据在场判定里点名；种子河由加载期迁移（template.rivers_seed →
    // 河线 POI）落实体，涂格河群系 16 已退役。
    let seas = seas_path
        .exists()
        .then(|| geo::read(&seas_path))
        .and_then(|r| r.ok());
    (rasterize(&coast, seas.as_deref(), peaks), "natural-earth")
}

fn program_north() -> TerrainData {
    let w = generate_cold_north(WIDTH, HEIGHT, DEFAULT_SEED);
    TerrainData {
        width: w.width,
        height: w.height,
        cells: w
            .cells
            .into_iter()
            .map(|c| TerrainCellData {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
            .collect(),
    }
}

/// NE 数据栅格化：
/// 1. 全格初值水（surface=0, OCEAN）；
/// 2. 海域多边形内 → 水；
/// 3. 海岸国界多边形内 → 陆；
/// 4. 陆格按「距最近海岸线」距离分高度带，群系按纬度带（0=北）。
///
/// 河流**不**在此栅格化（ADR-0070 D3 · 计划 Task 12：河走实体——工程带
/// `template.rivers_seed`，加载期迁移把种子河转为河线 POI；涂格河 16 与
/// 河床 -2 m 的旧口径一并退役，河床下切改由线 POI 的下切命令承担）。
fn rasterize(coast: &[F], seas: Option<&[F]>, peaks: f64) -> TerrainData {
    let to_x = |lon: f64| ((lon - LON.0) / (LON.1 - LON.0) * WIDTH as f64).floor() as i32;
    let to_y = |lat: f64| ((LAT.1 - lat) / (LAT.1 - LAT.0) * HEIGHT as f64).floor() as i32;
    let rings = |fs: &[F]| -> Vec<Vec<[i32; 2]>> {
        let mut out = Vec::new();
        for f in fs {
            match &f.geom {
                G::Polygon(rs) => {
                    for r in rs {
                        out.push(r.iter().map(|p| [to_x(p[0]), to_y(p[1])]).collect());
                    }
                }
                G::MultiPolygon(polys) => {
                    for rs in polys {
                        for r in rs {
                            out.push(r.iter().map(|p| [to_x(p[0]), to_y(p[1])]).collect());
                        }
                    }
                }
            }
        }
        out
    };

    let coast_rings = rings(coast);
    let sea_rings = rings(seas.unwrap_or(&[]));

    let n = (WIDTH * HEIGHT) as usize;
    let mut surface = vec![0u8; n]; // 水
    let mut height = vec![0i16; n];
    let mut biome = vec![14u8; n]; // OCEAN

    for ring in &sea_rings {
        geo::fill_polygon(&mut surface, 0, ring, WIDTH as i32, HEIGHT as i32);
    }
    for ring in &coast_rings {
        geo::fill_polygon(&mut surface, 1, ring, WIDTH as i32, HEIGHT as i32);
    }

    // 真实山系脊线（经纬度折线）：阿尔卑斯 / 阿平宁 / 迪纳拉 / 阿特拉斯 /
    // 托罗斯—安纳托利亚 / 高加索。高度主场从「距海岸」改为「距山脊」。
    const RIDGES: &[&[[f64; 2]]] = &[
        // 阿尔卑斯
        &[
            [-6.0, 44.0],
            [0.0, 45.5],
            [7.0, 46.5],
            [13.0, 47.0],
            [16.0, 46.5],
        ],
        // 阿平宁
        &[[9.5, 44.2], [12.5, 43.0], [14.5, 41.8], [16.0, 39.5]],
        // 迪纳拉—品都斯
        &[[14.0, 45.5], [17.0, 43.5], [19.5, 42.0], [21.5, 40.5]],
        // 阿特拉斯
        &[
            [-8.0, 31.0],
            [-4.0, 32.5],
            [0.0, 34.0],
            [5.0, 35.0],
            [9.0, 35.5],
        ],
        // 托罗斯—安纳托利亚—本廷
        &[
            [28.0, 37.0],
            [32.0, 37.5],
            [36.0, 38.5],
            [40.0, 40.0],
            [44.0, 41.0],
        ],
        // 高加索
        &[[39.5, 43.5], [43.0, 42.8], [47.0, 42.0]],
        // 伊比利亚：坎塔布连 + 内华达
        &[[-6.0, 43.0], [-3.0, 43.0], [-1.0, 42.5]],
        &[[-3.5, 37.2], [-1.0, 37.3]],
    ];

    // 脊线换算到格坐标，并预采样点集（算距离用）。
    let to_x = |lon: f64| ((lon - LON.0) / (LON.1 - LON.0) * WIDTH as f64).floor() as i32;
    let to_y = |lat: f64| ((LAT.1 - lat) / (LAT.1 - LAT.0) * HEIGHT as f64).floor() as i32;
    let ridge_pts: Vec<[i32; 2]> = RIDGES
        .iter()
        .flat_map(|line| {
            line.windows(2).flat_map(move |w| {
                // 每段线性插值采样（约每 2 格一个点，够距离场用）
                let (ax, ay) = (to_x(w[0][0]), to_y(w[0][1]));
                let (bx, by) = (to_x(w[1][0]), to_y(w[1][1]));
                let steps = ((bx - ax).abs().max((by - ay).abs()).max(1)) as usize;
                (0..=steps).map(move |s| {
                    let t = s as f32 / steps as f32;
                    [
                        (ax as f32 * (1.0 - t) + bx as f32 * t).round() as i32,
                        (ay as f32 * (1.0 - t) + by as f32 * t).round() as i32,
                    ]
                })
            })
        })
        .collect();

    // Perlin detail（wv-terrain 自带，零依赖）
    let perlin = wv_terrain::generate::Perlin::new(0xC0FFEE);
    let fbm = |x: f64, y: f64| -> f64 {
        let v = perlin.fbm(x, y, 4, 2.0, 0.5);
        ((v + 1.0) * 0.5).clamp(0.0, 1.0)
    };

    for y in 0..HEIGHT as i32 {
        for x in 0..WIDTH as i32 {
            let idx = (y as u32 * WIDTH + x as u32) as usize;
            let fy = y as f64 / HEIGHT as f64; // 0=北
            if surface[idx] != 1 {
                // 海：距海岸浅→深
                let d = geo::min_dist_to_rings(x, y, &coast_rings);
                height[idx] = match d {
                    0..=1 => -10,
                    2..=6 => -60,
                    7..=20 => -400,
                    21..=55 => -1500,
                    // 大洋盆（ADR-0042）：压到 -3200 让渲染端 water_shade_at 的
                    // 深度渐变（t = depth/3500）真正走到深蓝段——旧值 -1500 封顶
                    // t≈0.43，海面永远停在中间蓝（2026-10-20 实机截图确认）。
                    // 55 格外按距离线性下探（约 40 格过渡到全深）——硬档位边界
                    // 会在海面画出锐利的多边形色块（第一轮实机截图确认）。
                    _ => {
                        let dd = ((d - 55) as f64).min(40.0);
                        (-1500.0 - 1700.0 * dd / 40.0) as i16
                    }
                };
                continue;
            }
            // 陆地：
            let d_coast = geo::min_dist_to_rings(x, y, &coast_rings);
            let d_ridge = geo::min_dist_to_rings(x, y, std::slice::from_ref(&ridge_pts));
            // 脊线距离噪声扭曲（ADR-0042 追记 D13）：NE 脊线本体是几段直线
            // 折线，直距离场 + 高度分类会拉出一条笔直的白色雪带——zoom out
            // 时读作「道路」（用户 2026-10-20 实机反馈）。对**距离场**做低频
            // 扰动让山脉走向自然蜿蜒，而不是只调亮度。
            // 振幅必须小于山宽（宽 ~7–10 格）：±3 格只横向挪动脊线；
            // ±7 格会把峰高削掉一半（实测：整条阿尔卑斯退化成丘陵）。
            let warp = (fbm(x as f64 * 0.022, y as f64 * 0.022) - 0.5) * 6.0;
            let d_ridge = (d_ridge as f64 + warp).max(0.0);
            // 基础 = 沿海平原；高度主场 = 山脊高斯（宽 ~10 格）+ perlin 起伏。
            // 峰顶 lift ≈ peaks * 0.9（脊线上 d≈0）。
            // 峰越高衰减越快（width ∝ 1/peaks）：尖峰造型 + 雪带宽度稳定。
            // ADR-0042：22 → 10——旧宽度让每条山脉两侧 ~40 格的山地走廊
            // 把整个半岛吞成雪原条带（实机截图确认），10 格收回「山脉是链
            // 不是高原」的参考图构图。
            // D13：宽度沿线起伏（±35%）——同一条山脊忽宽忽窄，避免等宽缎带。
            let width_base = (10.0 * (3400.0 / peaks)).max(5.0);
            let width = width_base * (0.65 + 0.7 * fbm(x as f64 * 0.05, y as f64 * 0.05));
            // 边缘噪声化：等高线不再是干净平行条带——山体内部疏密起伏，
            // 群系边界跟着碎成有机曲线。
            let ridge_noise = 0.65 + 0.7 * fbm(x as f64 * 0.09, y as f64 * 0.09);
            let ridge_lift = (-(d_ridge / width)).exp() * peaks * 0.9 * ridge_noise;
            let detail = fbm(x as f64 * 0.03, y as f64 * 0.03) * 260.0 - 60.0;
            let h = if d_coast <= 1 {
                20.0
            } else {
                (60.0 + ridge_lift + detail + (d_coast as f64 * 4.0)).clamp(10.0, peaks * 1.05)
            };
            height[idx] = h as i16;
            // 群系：纬度气候为主，山只认高度。
            // 雪线随世界峰值缩放（3400 世界 2250 / 10000 世界 ~6600）——
            // 气候边界噪声化（ADR-0042）：纯纬度阈值画出横贯全图的水平条带
            //（实机截图确认）；低频噪声把纬度线揉成有机曲线，幅度 ±0.07 纬度幅。
            let fy_wob = fy + (fbm(x as f64 * 0.045, y as f64 * 0.045) - 0.5) * 0.14;
            // ADR-0044 D2（学 civ-atlas）：**山不是颜色**。civ-atlas 的 biome
            // 枚举（温度+降水的 Whittaker 图）里没有 MOUNTAINS/SNOWFIELD——
            // 高地的地面色就是气候色，山的身份全部由符号层承担。此前按高度
            // 分带出 MOUNTAINS(岩褐)/SNOWFIELD(冷白)，连成一条高对比缎带，
            // zoom out 读作道路（用户两轮实机反馈）——从根上拆掉：
            // 高地一律落 HILLS（干草黄，SPEC §1.4 既有色），山形与雪帽由
            // 装饰层按**高度**出图标（terrain_decor 高度优先规则，同 ADR）。
            // 作者仍可用笔刷手涂 MOUNTAINS/SNOWFIELD biome（笔刷意图优先，
            // 渲染层 biome 回退规则保留，两条路互不干扰）。
            biome[idx] = if h >= 550.0 {
                12 // HILLS（高地干草黄；山形 = 符号，见 terrain_decor）
            } else if fy_wob < 0.30 {
                1 // FOREST（中欧森林）
            } else if fy_wob < 0.42 {
                3 // GRASSLAND
            } else if fy_wob < 0.72 {
                2 // PLAINS（地中海硬叶/农田）
            } else {
                5 // DESERT（撒哈拉）
            };
        }
    }

    let cells = (0..n)
        .map(|i| TerrainCellData {
            surface: surface[i],
            height: height[i],
            biome: biome[i],
        })
        .collect();
    TerrainData {
        width: WIDTH,
        height: HEIGHT,
        cells,
    }
}

fn write_index(map_dir: &Path, raster: &RasterRef) {
    let mut index = String::new();
    index.push_str(
        "# work-view-test 首图索引（wv-gen-demo 生成）
",
    );
    index.push_str(&format!(
        "id: {MAP_ID}
"
    ));
    index.push_str(
        "name: 策展 JRPG 世界（地中海 + 北非）
",
    );
    index.push_str(
        "level: world
",
    );
    index.push_str(
        "children: []
",
    );
    index.push_str(
        "extent:
",
    );
    index.push_str(&format!(
        "  width: {EXTENT_W}
"
    ));
    index.push_str(&format!(
        "  height: {EXTENT_H}
"
    ));
    index.push_str(
        "raster:
",
    );
    index.push_str(&format!(
        "  path: maps/{MAP_ID}/terrain.bin
"
    ));
    index.push_str(
        "  encoding: i16
",
    );
    index.push_str(&format!(
        "  width: {}
",
        raster.width
    ));
    index.push_str(&format!(
        "  height: {}
",
        raster.height
    ));
    index.push_str(
        "  chunk: 256
",
    );
    index.push_str(
        "  format_version: 2
",
    );
    index.push_str(&format!(
        "  checksum: {}
",
        raster.checksum.as_deref().unwrap_or("")
    ));
    index.push_str(
        "layers:
",
    );
    index.push_str(
        "  - id: layer_terrain
",
    );
    index.push_str(
        "    name: 地形
",
    );
    index.push_str(
        "    kind: terrain
",
    );
    index.push_str(
        "    visible: true
",
    );
    index.push_str(
        "    locked: false
",
    );
    index.push_str(
        "    opacity: 1.0
",
    );
    index.push_str(
        "    z: 0
",
    );
    let path = map_dir.join("index.yaml");
    std::fs::write(&path, index).expect("write index.yaml");
    println!("[ok] {}", path.display());
}

/// 只重写 world.yaml 的 `template:` 段（到文件尾），其余字段（内容层）原样保留。
///
/// `rivers_seed` 的消费点在**加载期迁移**（ADR-0070 D3 生成器统一）：迁移器
/// （`wv-edit::MigrateLegacyLineDataCommand`）见「模板在场而无河线 POI」时，
/// 用它跑 `generate_rivers` 把种子河转为河线 POI 实体——渲染期不再消费种子
/// （T8 已删），生成期唯一定义点在 `wv-app::edit::river_gen`（RegenWorld /
/// 新建工程走它；本 CLI 只落盘种子记录，不造实体——内容层归作者）。
fn patch_template(root: &Path, kind: &str, seed: u64) -> Result<(), String> {
    let p = root.join("world.yaml");
    let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let mut head: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("template:") {
            break;
        }
        head.push(line);
    }
    let rivers_seed = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut out = head.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!(
        "template:\n  kind: {kind}\n  seed: {seed}\n  rivers_seed: {rivers_seed}\n"
    ));
    std::fs::write(&p, out).map_err(|e| e.to_string())
}

/// 调试输出：biome 伪彩 BMP（无需图像库，手写 BMP 头）。
fn dump_debug_png(data: &TerrainData, path: &Path) {
    const COLORS: [[u8; 3]; 21] = [
        [40, 40, 40],    // 0 NONE 深灰
        [30, 120, 60],   // 1 FOREST 绿
        [140, 200, 90],  // 2 PLAINS 浅绿
        [170, 210, 100], // 3 GRASSLAND 黄绿
        [190, 175, 130], // 4 GOBI 灰黄
        [235, 200, 120], // 5 DESERT 沙
        [90, 130, 110],  // 6 SWAMP
        [110, 150, 120], // 7 WETLAND
        [180, 190, 200], // 8 TUNDRA
        [245, 248, 249], // 9 SNOW 白
        [40, 150, 60],   // 10 JUNGLE
        [50, 170, 70],   // 11 RAINFOREST
        [180, 160, 90],  // 12 HILLS
        [130, 110, 90],  // 13 MOUNTAINS 棕
        [60, 110, 160],  // 14 OCEAN 蓝
        [90, 160, 200],  // 15 LAKE
        [40, 90, 200],   // 16 RIVER 深蓝
        [220, 190, 90],  // 17 OASIS
        [200, 190, 80],  // 18 SAVANNA
        [230, 230, 220], // 19 SALT_FLAT 近白
        [90, 70, 50],    // 20 ROAD
    ];
    let (w, h) = (data.width as usize, data.height as usize);
    let row_pad = (w * 3) % 4;
    let row_size = w * 3 + row_pad;
    let img_size = (row_size * h + 54) as u32;
    let mut out = Vec::with_capacity(54 + row_size * h);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&img_size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as u32).to_le_bytes());
    out.extend_from_slice(&(h as u32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    // BMP 自底向上：从最后一行写起
    for y in (0..h).rev() {
        for x in 0..w {
            let c = data.cells[y * w + x];
            let col = COLORS
                .get(c.biome as usize)
                .copied()
                .unwrap_or([255, 0, 255]);
            // 按高度调一点明度让分带可见
            let lum = if c.height >= 1250 {
                0.8
            } else if c.height < 0 {
                0.85
            } else {
                1.0
            };
            out.extend_from_slice(&[
                (col[2] as f32 * lum) as u8,
                (col[1] as f32 * lum) as u8,
                (col[0] as f32 * lum) as u8,
            ]);
        }
        out.resize(out.len() + row_pad, 0);
    }
    std::fs::write(path, out).expect("write debug bmp");
}
