//! ADR-0088 D5 预算核查探针：编辑单帧「分块指纹增量 + FieldPack 补丁 + 材质行段
//! 拷贝」CPU 全链实测（headless，无窗口、无 GPU 提交）。
//!
//! ## 口径
//!
//! 三段与 `wv_render::cellmap_bridge::field::sync_terrain_field` 编辑帧消费的
//! **同三个生产函数、同序**：
//! 1. `BlockFingerprint::update_region`——分块指纹增量重算（ADR-0088 D3）；
//! 2. `FieldPack::patch_region`——脏矩形重打包；
//! 3. `patch_material_images`——数据纹理行段拷贝（`last_patch_ms` 的主体）。
//!
//! **GPU 整张重上传不含在内**：`Assets::<Image>::get_mut` 把资产标脏后由
//! RenderAssetPlugin 整张重抽取上传，Bevy 没有「局部纹理更新」的公开路径——
//! 那部分属 ADR-0088 D5 预登记的「超标 → GPU 子资源上传专项（另 ADR）」管辖，
//! 本探针只钉 CPU EDT 侧。纹理字节账在输出里单列，供 VRAM 折算。
//!
//! ## 用法
//!
//! ```text
//! cargo run -p wv-app --features app --example perf_field_patch_chain -- --size 4096
//! cargo run -p wv-app --features app --example perf_field_patch_chain -- --project examples/jrpg-world
//! ```
//!
//! - `--size N`：合成 N×N 图，生成器 `wv_terrain::climate::generate_cold_north(N, N,
//!   DEFAULT_SEED)`（与 examples/jrpg-world / work-view-test 同源程序化生成）；
//! - `--project <dir>`：读真实工程的栅格（RasterRef 按 examples/jrpg-world 的
//!   `maps/world_main/index.yaml` 口径：`maps/world/terrain.bin` i16 1024² chunk 256）。
//!
//! 笔刷脏区缺省 **24×24 格**（默认笔刷半径 1–2 格连拖一笔的典型矩形，T3 探针
//! 12×10 口径的邻域），`--rect WxH` 可改（`256x256` = 单个指纹块整块的最坏情况）。

use std::time::Instant;

use bevy::asset::Assets;
use bevy::image::Image;
use wv_core::map::{RasterEncoding, RasterRef};
use wv_core::terrain::TerrainData;
use wv_io::terrain_bin::read_terrain;
use wv_terrain::climate::{generate_cold_north, DEFAULT_SEED};
use wv_world_cell_map::terrain_view::field::{
    build_material, default_params, patch_material_images, BlockFingerprint, CoastEncoding,
    FieldPack, FieldPalette, ViewBranch,
};

/// 实测轮数：首 3 轮预热（页分配 / 缓存驻留），后 [`RUNS`] 轮计入统计。
const RUNS: usize = 30;

fn main() {
    let mut size: u32 = 4096;
    let mut project: Option<String> = None;
    let mut rect = (24u32, 24u32);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--size" => size = args.next().and_then(|v| v.parse().ok()).unwrap_or(4096),
            "--project" => project = args.next(),
            "--rect" => {
                let raw = args.next().unwrap_or_default();
                let mut it = raw.splitn(2, 'x');
                let w = it.next().and_then(|v| v.parse().ok()).unwrap_or(24);
                let h = it.next().and_then(|v| v.parse().ok()).unwrap_or(w);
                rect = (w, h);
            }
            other => {
                eprintln!("未知参数 {other}（支持 --size N / --project dir / --rect WxH）");
                std::process::exit(2);
            }
        }
    }

    let (mut data, source) = match &project {
        Some(dir) => {
            let raster = RasterRef {
                path: "maps/world/terrain.bin".into(),
                encoding: RasterEncoding::I16,
                width: 1024,
                height: 1024,
                chunk: Some(256),
                format_version: 2,
                checksum: None,
            };
            (
                read_terrain(dir, &raster).expect("读工程栅格"),
                format!("{dir}（真实工程栅格）"),
            )
        }
        None => (
            synthetic(size),
            format!("generate_cold_north({size}, {size}, DEFAULT_SEED)（合成图）"),
        ),
    };
    let (gw, gh) = (data.width, data.height);
    println!(
        "数据源：{source}\n幅面：{gw}×{gh}（{} 格）；笔刷脏区：{}×{} 格",
        gw as u64 * gh as u64,
        rect.0,
        rect.1
    );

    // ── 一次性建场（编辑帧之外的一次性成本，作对照） ──
    let t = Instant::now();
    let mut pack = FieldPack::from_terrain_with(&data, CoastEncoding::Signed);
    let full_pack_ms = ms(t);
    let t = Instant::now();
    let mut fp = BlockFingerprint::new_full(&data);
    let fp_full_ms = ms(t);

    let mut images = Assets::<Image>::default();
    let t = Instant::now();
    let material = build_material(
        &mut images,
        &pack,
        default_params(
            [gw as f32, gh as f32],
            8.0,
            ViewBranch::Themed,
            FieldPalette::default(),
        ),
    );
    let build_mat_ms = ms(t);
    println!(
        "一次性建场：FieldPack 全量 = {full_pack_ms:.2} ms；指纹全量 = {fp_full_ms:.2} ms；build_material = {build_mat_ms:.2} ms"
    );
    println!(
        "纹理字节账（CPU 侧 = GPU 上传量的下界）：height R16 {} + coast R16 {} + attr RGBA8 {} + biome RGBA8 ×2 张 {} + line_dash RGBA8 {} = 共 {:.1} MB",
        pack.height_tex.len(),
        pack.coast_tex.len(),
        pack.attr_tex.len(),
        pack.biome_tex.len() * 2,
        gw as usize * gh as usize * 4,
        (pack.height_tex.len() + pack.coast_tex.len() + pack.attr_tex.len() + pack.biome_tex.len() * 2 + gw as usize * gh as usize * 4) as f64 / (1024.0 * 1024.0)
    );

    // ── 编辑帧全链：改格 → 指纹增量 → 补丁 → 行段拷贝 ──
    let mut samples: Vec<f64> = Vec::with_capacity(RUNS);
    let mut split = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..(RUNS + 3) {
        // 半径 1–2 格的一笔：脏区内逐格抬高度（正负交替，保证字节必变）。
        let x0 = (gw / 2).saturating_sub(rect.0 / 2);
        let y0 = (gh / 2).saturating_sub(rect.1 / 2);
        for y in y0..(y0 + rect.1).min(gh) {
            for x in x0..(x0 + rect.0).min(gw) {
                let cell = &mut data.cells[(y * gw + x) as usize];
                cell.height = if i % 2 == 0 {
                    cell.height.saturating_add(5)
                } else {
                    cell.height.saturating_sub(5)
                };
            }
        }
        let bounds = [x0, y0, (x0 + rect.0).min(gw), (y0 + rect.1).min(gh)];
        let rect_c = wv_world_cell_map::terrain_view::field::CellRect::from_bounds(bounds);

        let t0 = Instant::now();
        fp.update_region(&data, bounds);
        let t1 = Instant::now();
        let patched = pack.patch_region(&data, rect_c);
        let t2 = Instant::now();
        let ok = patch_material_images(&mut images, &material, &pack, rect_c);
        let t3 = Instant::now();
        assert!(patched && ok, "补丁链必须走通（探针夹具错误）");

        let total = t3.duration_since(t0).as_nanos() as f64 / 1e6;
        if i < 3 {
            continue; // 预热
        }
        samples.push(total);
        split = (
            t1.duration_since(t0).as_nanos() as f64 / 1e6,
            t2.duration_since(t1).as_nanos() as f64 / 1e6,
            t3.duration_since(t2).as_nanos() as f64 / 1e6,
        );
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let min = samples[0];
    let med = samples[samples.len() / 2];
    let max = samples[samples.len() - 1];
    println!(
        "编辑帧全链（指纹增量 + 补丁 + 行段拷贝）×{}：min {min:.3} / median {med:.3} / max {max:.3} ms",
        samples.len()
    );
    println!(
        "末轮分段：指纹增量 {:.3} + pack 补丁 {:.3} + 行段拷贝 {:.3} ms",
        split.0, split.1, split.2
    );
    println!(
        "预算对照（ADR-0088 D5）：编辑单帧 ≤8 ms @1024² / 4096² 单笔 patch 全链 ≤16 ms——本轮幅面 {gw}×{gh}，CPU 侧中位 {med:.3} ms"
    );
}

fn synthetic(size: u32) -> TerrainData {
    let w = generate_cold_north(size, size, DEFAULT_SEED);
    TerrainData {
        width: w.width,
        height: w.height,
        cells: w
            .cells
            .into_iter()
            .map(|c| wv_core::terrain::TerrainCellData {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
            .collect(),
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_nanos() as f64 / 1e6
}
