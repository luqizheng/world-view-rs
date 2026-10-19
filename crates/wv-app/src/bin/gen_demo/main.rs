//! `wv-gen-demo` —— 独立 CLI：为 `examples/work-view-test` 生成「地中海 + 北非」底座。
//!
//! 用法：
//!   cargo build -p wv-app --bin wv-gen-demo
//!   ./target/debug/wv-gen-demo.exe <工程目录>        # 缺省 examples/work-view-test
//!
//! 行为分支（自动探测 `<工程>/data/` 下产物）：
//!
//! 有 `coastlines.geojson` + `rivers.geojson` 时：读 GeoJSON（自带零依赖解析器），
//! 栅格化 surface / 高度 / 群系，河流 → BIOME_RIVER。
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
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEMO_DIR));
    println!("工程目录：{}", root.display());
    let data_dir = root.join(DATA_SUB);
    let map_dir = root.join("maps").join(MAP_ID);
    std::fs::create_dir_all(&map_dir).expect("create maps/world_main");

    let (data, basis) = build_grid(&data_dir);
    let seed = seed_now();
    let land = data.cells.iter().any(|c| c.surface == 1);
    let kind = if land { "cold_north" } else { "ocean" };

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
fn build_grid(data_dir: &Path) -> (TerrainData, &'static str) {
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
    let rivers = geo::read(&rivers_path).ok();
    let seas = seas_path
        .exists()
        .then(|| geo::read(&seas_path))
        .and_then(|r| r.ok());
    (
        rasterize(&coast, rivers.as_deref(), seas.as_deref()),
        "natural-earth",
    )
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
/// 4. 陆格按「距最近海岸线」距离分高度带，群系按纬度带（0=北）；
/// 5. 河流线刷 BIOME_RIVER + 河床 -2m。
fn rasterize(coast: &[F], rivers: Option<&[F]>, seas: Option<&[F]>) -> TerrainData {
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
                _ => {}
            }
        }
        out
    };
    let lines = |fs: &[F]| -> Vec<Vec<[i32; 2]>> {
        let mut out = Vec::new();
        for f in fs {
            match &f.geom {
                G::LineString(ls) => {
                    out.push(ls.iter().map(|p| [to_x(p[0]), to_y(p[1])]).collect());
                }
                G::MultiLineString(mlls) => {
                    for l in mlls {
                        out.push(l.iter().map(|p| [to_x(p[0]), to_y(p[1])]).collect());
                    }
                }
                _ => {}
            }
        }
        out
    };

    let coast_rings = rings(coast);
    let sea_rings = seas.map(rings).unwrap_or_default();
    let river_lines = rivers.map(lines).unwrap_or_default();

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

    for y in 0..HEIGHT as i32 {
        for x in 0..WIDTH as i32 {
            let idx = (y as u32 * WIDTH + x as u32) as usize;
            let lat = y as f32 / HEIGHT as f32; // 0=北
            if surface[idx] == 1 {
                let d = geo::min_dist_to_rings(x, y, &coast_rings);
                height[idx] = match d {
                    0..=1 => 30,
                    2..=5 => 150,
                    6..=13 => 700,
                    14..=22 => 1500,
                    _ => 2300,
                };
                biome[idx] = match (lat, height[idx]) {
                    (_, h) if h >= 1250 => 13, // MOUNTAINS
                    (_, h) if h >= 250 => 12,  // HILLS
                    (l, _) if l < 0.18 => 9,   // SNOWFIELD
                    (l, _) if l < 0.28 => 1,   // FOREST
                    (l, _) if l > 0.75 => 5,   // DESERT
                    (l, _) if l > 0.55 => 3,   // GRASSLAND
                    _ => 2,                    // PLAINS
                };
            } else {
                let d = geo::min_dist_to_rings(x, y, &coast_rings);
                height[idx] = match d {
                    0..=1 => -10,
                    2..=6 => -60,
                    7..=20 => -400,
                    _ => -1500,
                };
            }
        }
    }

    for line in &river_lines {
        for seg in line.windows(2) {
            geo::draw_line_i16(&mut height, -2, seg[0], seg[1], WIDTH as i32, HEIGHT as i32);
            geo::draw_line_u8(&mut biome, 16, seg[0], seg[1], WIDTH as i32, HEIGHT as i32);
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
