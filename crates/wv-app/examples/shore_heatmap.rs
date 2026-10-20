//! 离岸距离场热力图（2026-10-20 S1 · 调试视图）。
//!
//! 对指定工程的 terrain.bin 计算 [`wv_terrain::shore`] 距离场并落 PNG：
//! 陆格 = 纸色；水格按离岸距离从近岸浅蓝渐变到远海深蓝，截断带（≥
//! max_dist）压暗。肉眼核对「距岸 X 格」是否正确，给 S2 浅滩带 / S3
//! 浪花带的带宽参数定标。
//!
//! 用法（工作区根目录）：
//! ```text
//! cargo run -p wv-app --example shore_heatmap -- [terrain.bin] [输出 PNG] [max_dist]
//! ```

use std::path::PathBuf;

use wv_io::terrain_bin::decode_terrain;
use wv_terrain::shore::{shore_distance_for_cells, DEFAULT_MAX_DIST};
use wv_terrain::TerrainCell;

fn main() {
    let root = std::env::current_dir().expect("cwd");
    let input: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("examples/work-view-test/maps/world_main/terrain.bin"));
    let out: PathBuf = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/diag_sea/shore_heatmap.png"));
    let max_dist: u16 = std::env::args()
        .nth(3)
        .map(|s| s.parse().expect("max_dist 应为整数"))
        .unwrap_or(DEFAULT_MAX_DIST);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }

    let bytes = std::fs::read(&input).unwrap_or_else(|e| panic!("读 {input:?} 失败: {e}"));
    let data = decode_terrain(&bytes).expect("terrain.bin 解码失败");
    let cells: Vec<TerrainCell> = data
        .cells
        .iter()
        .map(|c| TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let (w, h) = (data.width, data.height);
    let field = shore_distance_for_cells(&cells, w, h, max_dist);

    // 带宽统计：渲染侧定标用。
    let water_total = cells
        .iter()
        .filter(|c| wv_terrain::edit::is_water_cell(**c))
        .count() as f32;
    let mut bands = [0u32; 5]; // 0 / 1 / 2-3 / 4-7 / 8+
    for (i, d) in field.iter().enumerate() {
        if cells[i].surface != wv_terrain::SURFACE_WATER && cells[i].height >= 0 {
            continue;
        }
        let band = match d {
            0 => 0,
            1 => 1,
            2..=3 => 2,
            4..=7 => 3,
            _ => 4,
        };
        bands[band] += 1;
    }
    println!("max_dist={max_dist}  水格 {water_total:.0}");
    for (label, n) in [
        ("陆", bands[0]),
        ("距岸 1", bands[1]),
        ("2-3", bands[2]),
        ("4-7", bands[3]),
        ("8+", bands[4]),
    ] {
        println!("  {label:>6}: {:6.2}%", 100.0 * n as f32 / water_total);
    }

    // 热力图：1 格 = 1 像素（1024x768 足够肉眼判读）。
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, d) in field.iter().enumerate() {
        let t = (*d as f32 / max_dist as f32).clamp(0.0, 1.0);
        let (r, g, b) = if t <= 0.0 {
            (0xE8, 0xDC, 0xC0) // 陆：纸色
        } else if *d >= max_dist {
            (0x14, 0x2A, 0x45) // 截断带：压暗远海
        } else {
            // 近岸 #9FC8E8 → 远海 #1E4E7A 线性渐变。
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
            (lerp(0x9F, 0x1E), lerp(0xC8, 0x4E), lerp(0xE8, 0x7A))
        };
        rgba[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, 255]);
    }
    write_png(&out, w as usize, h as usize, &rgba).expect("写 PNG 失败");
    println!("已写出 {}", out.display());
}

/// ---- 最小 PNG 编码器（RGBA8，无压缩 stored blocks；与 render_map_modes 同款）----
fn write_png(path: &std::path::Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(h * (w * 4 + 1));
    for y in 0..h {
        raw.push(0u8);
        let start = y * w * 4;
        raw.extend_from_slice(&rgba[start..start + w * 4]);
    }

    let mut z = vec![0x78, 0x01]; // zlib header
    for chunk in raw.chunks(65_535) {
        let last = chunk.as_ptr() as usize + chunk.len() == raw.as_ptr() as usize + raw.len();
        z.push(if last { 1 } else { 0 });
        z.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut png: Vec<u8> = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8bit, RGBA
    chunk_out(&mut png, b"IHDR", &ihdr);
    chunk_out(&mut png, b"IDAT", &z);
    chunk_out(&mut png, b"IEND", &[]);
    std::fs::write(path, png)
}

fn chunk_out(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    let mut crc_input = tag.to_vec();
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}
