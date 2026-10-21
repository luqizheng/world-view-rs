//! 海洋洗白归因探针（2026-10-20 S0 · 临时诊断工具）。
//!
//! 对指定工程的 terrain.bin 直接离线烘焙 VectorFlat 底图（不经 GUI、
//! 不叠 veil / mottle / vignette 覆盖层），输出：
//! 1. 水深分布统计（分位数 + 分带占比）——给深度量程调参用；
//! 2. soften=on / soften=off 两版烘焙 PNG——把「色板 → 烘焙」的色移
//!    全部归因到烘焙期（soften + bleed + grain）；
//!    「烘焙 → GUI 截图」的残差才是覆盖层（mottle / vignette / grain）。
//! 3. 每版前 8 名颜色直方图（供与 GUI 截图直方图对表）。
//!
//! 用法（工作区根目录）：
//! ```text
//! cargo run -p wv-app --features app --example diag_sea_bake -- [terrain.bin] [输出目录]
//! ```

use std::path::PathBuf;

use wv_io::terrain_bin::decode_terrain;
use wv_render::state::ViewMode;
use wv_render::terrain_view::image_from_terrain;
use wv_render::theme::palette::{colors, MapPalette};
use wv_render::theme::theme_pack::builtin;

fn main() {
    let root = std::env::current_dir().expect("cwd");
    let input: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("examples/work-view-test/maps/world_main/terrain.bin"));
    let out_dir: PathBuf = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/diag_sea"));
    std::fs::create_dir_all(&out_dir).unwrap();

    let bytes = std::fs::read(&input).unwrap_or_else(|e| panic!("读 {input:?} 失败: {e}"));
    let data = decode_terrain(&bytes).expect("terrain.bin 解码失败");
    println!(
        "terrain: {}x{} 格, {} cells",
        data.width,
        data.height,
        data.cells.len()
    );

    print_depth_stats(&data);

    let pal = colors(MapPalette::Daylight);
    let pack = builtin::for_palette(MapPalette::Daylight);
    let shallow = pal.core_terrain(1);
    let deep = pal.core_terrain(0);
    println!("\n色板锚点: water_shallow {shallow:?}  water_deep {deep:?}");

    for soften in [true, false] {
        let img = image_from_terrain(&data, ViewMode::VectorFlat, None, soften, pack, 0.0);
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.data.as_ref().expect("地形图必须有像素数据");
        let name = if soften {
            "bake_soften_on.png"
        } else {
            "bake_soften_off.png"
        };
        let path = out_dir.join(name);
        write_png(&path, w, h, px).expect("写 PNG 失败");
        println!("\n== {} ({}x{}) ==", name, w, h);
        print_histogram(px);
    }
}

/// 水深分布：湿判定与渲染同口径（`wv_terrain::edit::is_water_cell`）。
fn print_depth_stats(data: &wv_core::terrain::TerrainData) {
    let mut depths: Vec<i32> = data
        .cells
        .iter()
        .filter(|c| {
            wv_terrain::edit::is_water_cell(wv_terrain::TerrainCell {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
        })
        .map(|c| -(c.height as i32))
        .collect();
    let water_n = depths.len();
    let total = data.cells.len();
    if water_n == 0 {
        println!("无水格");
        return;
    }
    depths.sort_unstable();
    let pct = |p: f32| depths[((depths.len() as f32 - 1.0) * p).round() as usize];
    println!(
        "\n水格 {water_n}/{total}（{:.1}%）",
        100.0 * water_n as f32 / total as f32
    );
    println!(
        "深度分位: p05={} p25={} p50={} p75={} p90={} p98={} max={}",
        pct(0.05),
        pct(0.25),
        pct(0.50),
        pct(0.75),
        pct(0.90),
        pct(0.98),
        depths[depths.len() - 1]
    );
    for band in [200, 500, 1000, 2000] {
        let n = depths.iter().filter(|d| **d <= band).count();
        println!(
            "深度 ≤ {band}m（近岸带，sqrt t ≥ {:.2}）: {:.1}%",
            ((band as f32) / 3500.0).sqrt(),
            100.0 * n as f32 / water_n as f32
        );
    }
}

/// 前 8 名颜色直方图（隔 2 像素采样，足够代表）。
fn print_histogram(px: &[u8]) {
    let mut counter = std::collections::HashMap::<[u8; 3], u64>::new();
    let stride = 8usize; // 每 2×2 像素块取 1
    for i in (0..px.len() / 4).step_by(stride) {
        *counter
            .entry([px[i * 4], px[i * 4 + 1], px[i * 4 + 2]])
            .or_insert(0) += 1;
    }
    let mut ranked: Vec<_> = counter.into_iter().collect();
    ranked.sort_unstable_by_key(|(_, score)| std::cmp::Reverse(*score));
    let total: u64 = ranked.iter().map(|(_, n)| n).sum();
    for (c, n) in ranked.iter().take(8) {
        println!(
            "  #{:02X}{:02X}{:02X}  {:6.2}%",
            c[0],
            c[1],
            c[2],
            100.0 * *n as f64 / total as f64
        );
    }
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
