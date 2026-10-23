//! 临时诊断：量化全图候选枚举的规模（T11 性能验收）。
use wv_terrain::{TerrainCell, TerrainGrid};

fn main() {
    let path = "examples/work-view-test/maps/world_main/terrain.bin";
    let bytes = std::fs::read(path).expect("read terrain.bin");
    let data: wv_core::terrain::TerrainData =
        wv_io::terrain_bin::decode_terrain(&bytes).expect("decode");
    println!(
        "decoded {}x{} cells={}",
        data.width,
        data.height,
        data.cells.len()
    );
    let cells: Vec<TerrainCell> = data
        .cells
        .iter()
        .map(|c| TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let grid = TerrainGrid::from_cells(data.width, data.height, cells).expect("grid");
    let t0 = std::time::Instant::now();
    let regions = wv_terrain::region::partition_by_biome(&grid);
    let dt = t0.elapsed();
    let total_segs: usize = regions.iter().map(|r| r.outer.len().max(1)).sum();
    let big = regions.iter().filter(|r| r.cells >= 16).count();
    let dashed_segs: usize = regions.iter().map(|r| r.outer.len().max(1) / 2).sum();
    println!(
        "分区数={}（≥16格的大区={big}），总环段={total_segs}，dashed 线段≈{dashed_segs}，partition 耗时={dt:?}",
        regions.len()
    );
}
