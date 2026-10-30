//! 给工程的世界地形叠加山脊纹理（ADR-0092 §9 B1 数据侧 CLI）。
//!
//! 平缓的生成地形近看是「涂色带」不是山（晕渲只能照亮高度场里已有的起伏）——
//! 本工具把 [`wv_terrain::intent::apply_ridge_detail`] 应用于
//! `maps/world/terrain.bin`（世界主图），让山地带长出山脊/河谷高频结构。
//! 只动 height（surface / biome 不改），高度域会变但完全可撤销（GUI 里
//! `Ctrl+Z`，或重跑旧数据还原）。
//!
//! ## 用法
//!
//! ```text
//! cargo run -p wv-app --features app --example add_ridges -- <project_dir> [seed]
//! ```
//!
//! 与意图笔刷（BrushTool::MountainRange，作者手绘脊线）互补：笔刷做
//! 「作者知道山在哪」的 authoring，本工具做「已有山地批量长纹理」的
//! 整图加工。
//!
//! ## 为什么是 example 而非命令面板入口
//!
//! 整图加工是**一次性批处理**语义（作者对着结果决定去留——撤销栈/重跑
//! 都兜得住），与面板命令的「当下交互态」不同族；gen_demo 已立 example
//! 先例。GUI 内的等价入口是地形设计器的「山地叠加山脊纹理」按钮
//! （`AddRidgeDetailCommand`，同一函数、同一确定性口径）。

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next().map(PathBuf::from) else {
        eprintln!("用法: add_ridges <project_dir> [seed]");
        std::process::exit(2);
    };
    let seed: u64 = args
        .next()
        .map(|v| v.parse().expect("seed 应为 u64"))
        .unwrap_or(0x5249_4447_4501); // "RIDGE\x01" 的十六进制速记

    let map_id = "world";
    let path = root.join("maps").join(map_id).join("terrain.bin");
    let bytes = std::fs::read(&path).expect("读 terrain.bin");
    let data = wv_io::terrain_bin::decode_terrain(&bytes).expect("解码 terrain.bin");
    println!(
        "[add_ridges] {}x{} 跨度 {}..{}",
        data.width,
        data.height,
        data.cells.iter().map(|c| c.height).min().unwrap_or(0),
        data.cells.iter().map(|c| c.height).max().unwrap_or(0)
    );

    // TerrainData（中立元组）→ TerrainGrid（crate 核心格型）→ 变换 → 回写。
    let w = data.width as usize;
    let mut grid = wv_terrain::TerrainGrid::new(data.width, data.height);
    for (i, c) in data.cells.iter().enumerate() {
        grid.set(
            (i % w) as i32,
            (i / w) as i32,
            wv_terrain::TerrainCell {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            },
        );
    }
    let touched = wv_terrain::intent::apply_ridge_detail(&mut grid, seed);
    println!("[add_ridges] 触碰 {touched} 格（seed = {seed:#x}）");

    let out = wv_core::terrain::TerrainData {
        width: data.width,
        height: data.height,
        cells: grid
            .cells()
            .iter()
            .map(|c| wv_core::terrain::TerrainCellData {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
            .collect(),
    };
    let raster = wv_io::terrain_bin::write_terrain(&root, map_id, &out).expect("写 terrain.bin");
    println!(
        "[add_ridges] checksum {:?} —— 记得同步 maps/*/index.yaml 的 raster.checksum",
        raster.checksum
    );
}
