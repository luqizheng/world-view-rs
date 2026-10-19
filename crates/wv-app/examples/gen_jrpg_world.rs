//! 一次性工具：为 `examples/jrpg-world` 写「冷北模板」地形（ADR-0037 M2-1）。
//!
//! 用法：`cargo run -p wv-app --example gen_jrpg_world -- examples/jrpg-world`
//!
//! 生成算法已收口到 [`wv_terrain::climate::generate_cold_north`]；本工具只做
//! `ClimateWorld → TerrainData` 的边界转换与 `terrain.bin` 落盘（AGENTS §5
//! 的唯一二进制例外）。不碰 `world.yaml` / 索引 YAML。

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_io::terrain_bin::write_terrain;
use wv_terrain::climate::{generate_cold_north, DEFAULT_SEED, DEFAULT_SIZE};

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/jrpg-world".into());
    let map_id = std::env::args().nth(2).unwrap_or_else(|| "world".into());
    let seed = std::env::args()
        .nth(3)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_SEED);

    let generated = generate_cold_north(DEFAULT_SIZE, DEFAULT_SIZE, seed);
    let data = TerrainData {
        width: generated.width,
        height: generated.height,
        cells: generated
            .cells
            .into_iter()
            .map(|c| TerrainCellData {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
            .collect(),
    };
    let raster = write_terrain(&root, &map_id, &data).expect("write terrain.bin");
    println!(
        "written {} ({}x{}, seed {seed})",
        raster.path, raster.width, raster.height
    );
    println!("checksum: {:?}", raster.checksum);
    println!("--- 贴回 maps/{map_id}/index.yaml 的 raster 段 ---");
    println!("raster:");
    println!("  path: {}", raster.path);
    println!("  encoding: i16");
    println!("  width: {}", raster.width);
    println!("  height: {}", raster.height);
    println!("  chunk: {}", raster.chunk.unwrap_or(256));
    println!("  format_version: {}", raster.format_version);
    if let Some(c) = &raster.checksum {
        println!("  checksum: {c}");
    }
}
