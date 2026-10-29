//! 场景分区验收程序（2026-10-29 · 场景编辑器 M1 人工验收辅助）。
//!
//! **不是产品代码**——它把「作者实际会走的路径」跑一遍并落 PNG，供人眼验收：
//!
//! 1. 建一张**城市图**（City 级，无栅格——与新对话框非 World 行为一致）；
//! 2. 走**真实的编辑命令**（`CreateSceneBaseGridCommand` + `PaintTerrainCommand`）
//!    刷出贫民区 / 商业区 / 工业区；
//! 3. 走**真实的渲染管线**（`image_from_terrain`）烘出 PNG。
//!
//! 用真实命令而不是直接改数据，是为了连「首刷自动铺基底」这条引导路径一起验。
//!
//! 跑法：
//! ```bash
//! cargo run -p wv-app --features app --example scene_district_acceptance
//! ```
//! 产物：`target/scene-acceptance/scene.png`（VectorFlat 视图）
//! 与 `scene-topographic.png`（CellMap 方格视图的固定色）。

use std::path::PathBuf;

use wv_core::map::{Extent, MapLevel, MapNode};
use wv_core::{Id, World};
use wv_edit::command::{CreateSceneBaseGridCommand, PaintTerrainCommand};
use wv_edit::history::Transaction;
use wv_terrain::biome::{DISTRICT_FIRST, DISTRICT_NAMES_ZH};
use wv_terrain::edit::{Brush, PaintTarget, Stroke};

/// 城市图边长（格）。
const GRID: u32 = 64;
/// 每格世界边长。
const CELL_SIZE: f32 = 16.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from("target/scene-acceptance");
    std::fs::create_dir_all(&out_dir)?;

    // ── ① 城市图节点（无栅格：与新对话框非 World 行为一致）─────────
    let city_id = Id::new_unchecked("city_test");
    let mut world = World::default();
    world.maps.push(MapNode {
        id: city_id.clone(),
        name: "验收城".into(),
        level: MapLevel::City,
        parent: None,
        extent: Some(Extent::sized(
            GRID as f32 * CELL_SIZE,
            GRID as f32 * CELL_SIZE,
        )),
        ..MapNode::default()
    });

    // ── ② 走真实命令：铺基底 + 刷分区 ────────────────────────────
    let mut history = wv_edit::History::default();

    // 引导：铺基底网格（这一步正是 M1 的首刷自动引导）。
    let mut tx = Transaction::new();
    tx.push(Box::new(CreateSceneBaseGridCommand::new(
        city_id.clone(),
        CELL_SIZE,
    )));
    history.commit(&mut world, tx)?;
    let grid = world.terrain_of(&city_id).expect("基底网格应已写入");
    println!(
        "[验收] 基底网格：{}×{} 格，全格 biome={}（{}）",
        grid.width, grid.height, grid.cells[0].biome, DISTRICT_NAMES_ZH[0]
    );

    // 刷三个分区：贫民区（左上）/ 商业区（中）/ 工业区（右下）。
    // **坐标是世界单位**（笔刷中心 / 半径都按 wu 算，格号 = wu / cell_size）。
    let blocks: [(usize, f32, f32); 3] = [
        (1, 16.0 * CELL_SIZE, 8.0 * CELL_SIZE), // 贫民区
        (4, 32.0 * CELL_SIZE, 7.0 * CELL_SIZE), // 商业区
        (5, 48.0 * CELL_SIZE, 6.0 * CELL_SIZE), // 工业区
    ];
    for (idx, center, radius) in blocks {
        let mut stroke = Stroke::new();
        stroke.push(Brush::paint(
            (center, center),
            radius,
            PaintTarget {
                surface: wv_terrain::SURFACE_LAND,
                biome: DISTRICT_FIRST + idx as u8,
            },
        ));
        let mut tx = Transaction::new();
        tx.push(Box::new(PaintTerrainCommand::new(
            city_id.clone(),
            stroke,
            CELL_SIZE,
        )));
        history.commit(&mut world, tx)?;
        println!(
            "[验收] 刷分区：{}（biome={}）",
            DISTRICT_NAMES_ZH[idx],
            DISTRICT_FIRST + idx as u8
        );
    }

    // ── ③ 校验：高度必须全是 0（分区不改高度）─────────────────────
    let grid = world.terrain_of(&city_id).expect("栅格").clone();
    let bad: Vec<i16> = grid
        .cells
        .iter()
        .filter(|c| c.height != 0)
        .map(|c| c.height)
        .collect();
    assert!(bad.is_empty(), "分区刷色不得改高度，实得 {bad:?}");
    let count = |biome: u8| grid.cells.iter().filter(|c| c.biome == biome).count();
    println!(
        "[验收] 高度全为 0 ✓；分区格数：贫民 {} / 商业 {} / 工业 {}",
        count(DISTRICT_FIRST + 1),
        count(DISTRICT_FIRST + 4),
        count(DISTRICT_FIRST + 5)
    );

    // ── ④ 落盘工程（真实写盘路径，供 GUI 打开）────────────────────
    let project_dir = out_dir.join("project");
    std::fs::create_dir_all(project_dir.join("maps/city_test"))?;
    std::fs::write(
        project_dir.join("world.yaml"),
        "name: 场景分区验收\nversion: \"0.1\"\nschema_version: 2\nroot_map: city_test\ntimeline_range:\n  from: 1\n  to: 180001\n",
    )?;
    wv_io::terrain_bin::write_terrain(&project_dir, "city_test", &grid)?;
    std::fs::write(
        project_dir.join("maps/city_test/index.yaml"),
        format!(
            "id: city_test\nname: 验收城\nlevel: city\nchildren: []\nextent:\n  width: {}\n  height: {}\n",
            GRID as f32 * CELL_SIZE,
            GRID as f32 * CELL_SIZE
        ),
    )?;

    // ── ⑤ 真实渲染管线烘 PNG ─────────────────────────────────────
    let pack = wv_world_cell_map::palette::CellPack::fixture();
    for (name, mode) in [
        ("scene.png", wv_world_cell_map::shade::ShadeMode::VectorFlat),
        (
            "scene-topographic.png",
            wv_world_cell_map::shade::ShadeMode::Topographic,
        ),
    ] {
        let (bytes, res) =
            wv_world_cell_map::terrain_view::cells::terrain_pixels(&grid, mode, None, false, &pack);
        let w = res as usize * GRID as usize;
        let path = out_dir.join(name);
        write_png(&path, w, w, &bytes)?;
        println!("[验收] {mode:?} 视图 → {}（{w}×{w}）", path.display());
    }
    println!("[验收] 工程目录：{}", project_dir.display());
    Ok(())
}

// ── 最小 PNG 编码器（RGBA8，zlib stored block）────────────────────
fn write_png(path: &std::path::Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
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
