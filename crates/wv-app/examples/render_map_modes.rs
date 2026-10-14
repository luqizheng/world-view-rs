//! 离线渲染各 ViewMode 的地形底图为 PNG（用于视觉验证与风格对比）。
//!
//! 用法（工作区根目录）：
//! ```text
//! cargo run -p wv-app --features app --example render_map_modes -- [terrain.bin 路径] [输出目录]
//! ```
//! 默认读 `examples/xinhai-1911/maps/map_world/terrain.bin`，输出到 `target/map_preview/`。
//!
//! PNG 编码内置实现（filter 0 + zlib stored block + adler32/crc32），
//! 不引入 `image` 依赖（AGENTS.md §9：默认拒绝新增依赖）。

use std::path::PathBuf;

use wv_app::render::terrain_view::image_from_terrain;
use wv_app::state::ViewMode;
use wv_io::terrain_bin::decode_terrain;

fn main() {
    let root = std::env::current_dir().expect("cwd");
    let input: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("examples/xinhai-1911/maps/map_world/terrain.bin"));
    let out_dir: PathBuf = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/map_preview"));
    std::fs::create_dir_all(&out_dir).unwrap();

    let bytes = std::fs::read(&input).unwrap_or_else(|e| panic!("读 {input:?} 失败: {e}"));
    let data = decode_terrain(&bytes).expect("terrain.bin 解码失败");
    println!(
        "terrain: {}x{} 格, {} cells",
        data.width,
        data.height,
        data.cells.len()
    );

    for mode in ViewMode::ALL {
        let img = image_from_terrain(&data, mode, None, true);
        // atlas=None 时多数模式 res=1（每格 1 像素）；GoogleMap / VectorFlat
        // 走亚像素（plan v0.1 M2）——尺寸以 Image 实际边长为准。
        let (w, h) = (img.width() as usize, img.height() as usize);
        debug_assert_eq!(
            img.data.as_ref().expect("地形图必须有像素数据").len(),
            w * h * 4
        );
        let name = format!("{mode:?}");
        let path = out_dir.join(format!("{}.png", snake(&name)));
        write_png(
            &path,
            w,
            h,
            img.data.as_ref().expect("地形图必须有像素数据"),
        )
        .expect("写 PNG 失败");
        println!("已写出 {} ({}x{})", path.display(), w, h);
    }
    println!("完成，共 {} 个模式", ViewMode::ALL.len());
}

/// `RealTerrain` → `real_terrain`（文件名用）。
fn snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// ---- 最小 PNG 编码器（RGBA8，无压缩 stored blocks）----
#[allow(clippy::empty_line_after_doc_comments)] // 分节注释，非紧挂某个项的文档
fn write_png(path: &std::path::Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
    // 每行前加 filter 字节 0。
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
