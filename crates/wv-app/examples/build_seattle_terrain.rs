//! 由 OSM 反推的 Seattle kind 网格生成 `examples/seattle` 工程。
//!
//! 输入：`target/map_preview/seattle_kinds.bin`（由 build_seattle_kinds.py 生成，
//! 头 2 字节小端 u16 网格宽，随后每格 1 字节 kind）。
//!
//! 输出：
//! 1. `examples/seattle/maps/map_world/terrain.bin`（wv-io terrain.bin 格式）
//! 2. `target/map_preview/seattle_google.png`（用本项目渲染器的谷歌风渲染）
//!
//! 目的：用真实世界地理数据验证渲染器——如果谷歌风渲染出可辨认的西雅图
//! （Puget Sound 海湾 + Lake Washington），说明渲染没问题，此前"不像地图"
//! 出在 Perlin 生成数据上。
//!
//! 运行（工作区根目录，需先生成 kinds 文件）：
//! ```text
//! cargo run -p wv-app --features app --example build_seattle_terrain
//! ```

use std::path::Path;

use wv_app::state::ViewMode;
use wv_app::terrain_view::image_from_terrain;
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_io::terrain_bin::write_terrain;

fn main() {
    let root = std::env::current_dir().expect("cwd");
    let kinds_path = root.join("target/map_preview/seattle_kinds.bin");
    let bytes = std::fs::read(&kinds_path)
        .unwrap_or_else(|e| panic!("读 {kinds_path:?} 失败: {e}（先跑 build_seattle_kinds.py）"));
    let grid_w = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
    let grid_h = (bytes.len() - 2) as u32 / grid_w;
    assert_eq!(
        bytes.len(),
        2 + (grid_w * grid_h) as usize,
        "kinds 文件长度与网格宽不符"
    );
    println!("seattle kinds: {grid_w}x{grid_h}");

    // kind → 高度（仅影响真实地形模式；谷歌风不看高度）。
    let height_of = |kind: u8| match kind {
        1 => 0,   // 水面
        4 => 110, // 绿地
        _ => 90,  // 陆地
    };
    let cells: Vec<TerrainCellData> = bytes[2..]
        .iter()
        .map(|&kind| TerrainCellData {
            height: height_of(kind),
            kind,
        })
        .collect();
    let data = TerrainData {
        width: grid_w,
        height: grid_h,
        cells,
    };

    // 写入 examples/seattle 工程（P2：目录即工程）。
    let seattle_root = root.join("examples/seattle");
    let raster = write_terrain(&seattle_root, "map_world", &data).expect("写 terrain.bin 失败");
    println!(
        "已写 {}",
        seattle_root.join("maps/map_world/terrain.bin").display()
    );

    // 最小工程骨架：world.yaml + maps/world/index.yaml（缺省目录按空集加载）。
    std::fs::write(
        seattle_root.join("world.yaml"),
        "# 示例工程：西雅图（由真实 OpenStreetMap 数据反推的 kind 网格）\n\
         # 用途：验证谷歌风渲染器——数据是真实地理，若渲染不像地图则是渲染问题。\n\
         name: 西雅图 OSM\nversion: \"0.1\"\nschema_version: 2\nroot_map: map_world\ntimeline_range:\n  from: 1\n  to: 2\n",
    ).expect("写 world.yaml 失败");
    let raster_yaml = serde_yaml::to_string(&raster).expect("序列化 RasterRef");
    let raster_block: String = raster_yaml.lines().map(|l| format!("  {l}\n")).collect();
    std::fs::create_dir_all(seattle_root.join("maps/world")).expect("建 maps/world 失败");
    std::fs::write(
        seattle_root.join("maps/world/index.yaml"),
        format!(
            "# 西雅图世界地图（OSM z11 瓦片反推，256² 网格）\n\
             id: map_world\nname: 西雅图\nlevel: world\nchildren: []\nextent:\n  width: 1280.0\n  height: 1280.0\nraster:\n{raster_block}\
             layers:\n  - id: layer_terrain\n    name: 地形\n    kind: terrain\n    visible: true\n    locked: false\n    opacity: 1.0\n    z: 0\n    source: terrain.bin\n"
        ),
    ).expect("写 maps/world/index.yaml 失败");

    // 验证工程可加载（terrain round-trip + Project::load_world）。
    let back = wv_io::terrain_bin::read_terrain(&seattle_root, &raster).expect("回读失败");
    assert_eq!(back, data, "terrain.bin round-trip 必须无损");
    let project = wv_io::project::Project::open(&seattle_root).expect("打开工程失败");
    let world = project.load_world().expect("加载世界失败");
    // Phase 2：地形按图存，这里取那张声明的 raster 所属图（`map_world`）。
    assert_eq!(
        world.terrain_of(&wv_core::Id::new_unchecked("map_world")),
        Some(&data),
        "load_world 应带出同一份地形"
    );
    println!(
        "工程加载验证通过：{}（terrain {}x{}）",
        project.root().display(),
        grid_w,
        grid_h
    );

    // 渲染谷歌风 PNG。
    let img = image_from_terrain(&data, ViewMode::GoogleMap, None, true);
    let out = root.join("target/map_preview/seattle_google.png");
    write_png(
        &out,
        data.width as usize,
        data.height as usize,
        img.data.as_ref().expect("地形图必须有像素数据"),
    )
    .expect("写 PNG 失败");
    println!("已写 {}", out.display());

    // 也渲染真实地形模式，供对照。
    let img_rt = image_from_terrain(&data, ViewMode::RealTerrain, None, true);
    let out_rt = root.join("target/map_preview/seattle_real.png");
    write_png(
        &out_rt,
        data.width as usize,
        data.height as usize,
        img_rt.data.as_ref().expect("地形图必须有像素数据"),
    )
    .expect("写 PNG 失败");
    println!("已写 {}", out_rt.display());
}

/// ---- 与 render_map_modes 相同的最小 PNG 编码器 ----
#[allow(clippy::empty_line_after_doc_comments)] // 分节注释，非紧挂某个项的文档
fn write_png(path: &Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(h * (w * 4 + 1));
    for y in 0..h {
        raw.push(0u8);
        let start = y * w * 4;
        raw.extend_from_slice(&rgba[start..start + w * 4]);
    }
    let mut z = vec![0x78, 0x01];
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
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
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
