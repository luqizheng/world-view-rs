//! 一次性工具：为 `examples/xinhai-1911` 生成真实地形数据（`terrain.bin`）。
//!
//! 用法：`cargo run -p wv-app --example gen_xinhai_terrain -- examples/xinhai-1911`
//!
//! 生成器与 `terrain_view::placeholder_terrain` 同源（wv-terrain 的种子 Perlin），
//! 输出走 `wv_io::terrain_bin::write_terrain`（AGENTS §5 的唯一二进制例外）。
//! 写完后把返回的 `RasterRef` 打印成 YAML 片段，手工贴回 `maps/world/index.yaml`
//! 的 `raster:` 字段（示例工程不自动改索引，避免工具越权写 YAML）。

use wv_io::terrain_bin::write_terrain;
use wv_terrain::generate::Perlin;

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/xinhai-1911".into());
    let (w, h) = (512u32, 512u32);

    // 种子取工程名哈希风格的固定值，保证可重现（原则：确定性生成）。
    let perlin = Perlin::new(1911_1010);
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = perlin.fbm(x as f64 * 0.045, y as f64 * 0.045, 5, 2.0, 0.5);
            let t = ((n + 1.0) / 2.0).clamp(0.0, 1.0);
            // 华中地形：西山地（川），中东部平原水网（两湖），配水域。
            // 用 x 坐标做东西梯度，叠加噪声分档。
            let west = 1.0 - (x as f64 / w as f64) * 1.2; // 西高东低
            let t = (t * 0.55 + west * 0.45).clamp(0.0, 1.0);
            let kind = if t < 0.30 {
                0 // 深水
            } else if t < 0.38 {
                1 // 浅水
            } else if t < 0.46 {
                2 // 沙
            } else if t < 0.66 {
                3 // 草（平原）
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
    let data = wv_core::terrain::TerrainData {
        width: w,
        height: h,
        cells,
    };
    let raster = write_terrain(&root, "map_world", &data).expect("write terrain.bin");
    println!(
        "written {} ({}x{})",
        raster.path, raster.width, raster.height
    );
    println!("checksum: {:?}", raster.checksum);
    println!("--- 贴回 maps/world/index.yaml 的 MapNode ---");
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
