//! 渲染热点探针（2026-10-23，迭代档缺陷修复 A 项：先测后改）。
//!
//! ## 为什么存在
//!
//! 用户报「cell-map 里 zoom in/out 只有个位数 FPS」。制作人查得构建档位是纯
//! debug（`[profile.dev]` 缺省 = opt-level 0，连 bevy/wgpu 都未优化）。要不要动
//! 构建档位、以及动哪几个 crate，必须先用**同一份数据**量出 debug 与 release
//! 的倍数差，不许拍墙钟猜。本文件就是那把尺：三项 CPU 热点各测一次，
//! 数字抄进 `docs/PERF_GRID_AND_PROFILE_v0.1.md`。
//!
//! ## 三项对应的真实现场
//!
//! | 探针 | 现场调用点 |
//! |---|---|
//! | `grid_chains + build_stroke_mesh` | `sync_grid_lines`（缩放每档整层重建） |
//! | `assemble_strokes`（DP 全链） | `sync_isoline_overlay`（缩放跨档重跑 DP） |
//! | `terrain_pixels`（整图合成） | `rebuild_texture_on_mode_change` / 换色 |
//!
//! ## 怎么跑（复跑必读）
//!
//! ```text
//! # debug（未改档位时 / 改档位后各跑一次，两次都记账）
//! cargo test -p wv-world-cell-map --lib -- --ignored --nocapture perf_probe
//! # release
//! cargo test -p wv-world-cell-map --lib --release -- --ignored --nocapture perf_probe
//! ```
//!
//! 三个 `#[ignore]` 测试**不进常规闸门**——它们是尺子不是断言：常规
//! `cargo test -p wv-world-cell-map` 不跑，避免 CI 里多花十几秒。
//!
//! ## 口径纪律
//!
//! - 每项先暖机 1 次（分配器爬升 / 分支预测冷启动会让首跑偏大 2–3 倍），
//!   再跑 [`RUNS`] 次取**最小值**。取最小而非平均：我们要量的是「最好的机器
//!   状态下这个操作要多久」，抖动全是外来噪声（其它进程、调度）。
//! - 输入完全确定（正弦起伏地形，无随机、无磁盘、无 GPU）：两台机器上
//!   跑出的数字可直接比。

use std::time::Instant;

use bevy::prelude::Vec2;

use crate::palette::CellPack;
use crate::shade::ShadeMode;
use crate::terrain_view::cells::terrain_pixels;
use crate::terrain_view::grid_lines::grid_chains;
use crate::terrain_view::isoline_overlay::{
    assemble_strokes, build_stroke_mesh, IsolineChain, STROKE_PX,
};
use wv_terrain::isoline::{assemble_chains, chaikin, height_segments, kind_segments};

/// 计时重复次数（取最小值）。3 次足够区分 debug / release 的量级差，
/// 又不至于让单次探针跑成秒级。
const RUNS: usize = 3;

/// 地形边长（格）。取 1024 是因为：lod-demo 的 raster 就是 1024，
/// 而用户抱怨的 CellMap 工作台跑的是 `extent 4096 / raster 1024`。
const SIDE: u32 = 1024;

/// 造一张确定性的 1024² 起伏地形（正弦叠加）。
///
/// 幅度取 ±100（i16 域内），波长 64 / 37 格——两个**互质**波长让海岸线
/// 不是直线、也不是方格，因而海岸链的段数与 DP 的输入点数都接近真实地图
/// 的量级（探针若用几何图形会低估 DP 成本）。
fn terrain_1024() -> wv_core::terrain::TerrainData {
    let w = SIDE;
    let h = SIDE;
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32;
            let fy = y as f32;
            let v = (fx / 64.0).sin() * 60.0 + (fy / 37.0).cos() * 40.0;
            let height = v.round() as i16;
            cells.push(wv_core::terrain::TerrainCellData {
                height,
                surface: u8::from(height > 0),
                // 三条群系带（横切），给类型边界链一点活干。
                biome: (fy / 300.0) as u8 % 4,
            });
        }
    }
    wv_core::terrain::TerrainData {
        width: w,
        height: h,
        cells,
    }
}

/// 跑 `f` 一次暖机 + [`RUNS`] 次计时，返回最小毫秒。
fn best_ms(mut f: impl FnMut()) -> f64 {
    f();
    let mut best = f64::MAX;
    for _ in 0..RUNS {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    best
}

/// 打印一行结果（`--nocapture` 才可见）。
fn report(label: &str, note: &str, ms: f64) {
    println!("perf_probe\t{label}\t{ms:.2} ms\t{note}");
}

/// 探针 1：网格整层重建（`sync_grid_lines` 的缩放热路径）。
///
/// 规模口径：`grid_chains(SIDE, SIDE, half_w)` = `2 × (SIDE+1)` 条链（2050 条），
/// `half_w` 取 `GRID_LINE_PX × 0.5 × scale`，`scale` 用世界级适配档 5.12
/// （`zoom_level::fit` 的实测值）——即「刚打开 CellMap 时」的线宽。
#[test]
#[ignore = "性能探针：--ignored --nocapture 显式运行"]
fn perf_probe_grid_lines_rebuild() {
    let half_w = 1.0 * 0.5 * 5.12;
    let ms = best_ms(|| {
        let chains = grid_chains(SIDE, SIDE, half_w);
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 4.0);
        assert!(mesh.is_some(), "1024² 网格应能挤出 mesh");
    });
    report(
        "grid_chains+build_stroke_mesh",
        "2050 链 / 4100 段 → 16400 顶点",
        ms,
    );
}

/// 探针 2：等值线 DP 全链（`sync_isoline_overlay` 的跨档热路径）。
///
/// 链缓存（marching squares + 装配 + Chaikin）在计时之外建一次——它在键不变
/// 时不重跑；缩放跨档重跑的只有 `assemble_strokes`（DP + 半宽赋值）。
#[test]
#[ignore = "性能探针：--ignored --nocapture 显式运行"]
fn perf_probe_isoline_dp() {
    let data = terrain_1024();
    let w = data.width;
    let cell = |x: u32, y: u32| &data.cells[(y * w + x) as usize];
    let coast_segs = height_segments(w, data.height, |x, y| cell(x, y).height as f32, 0.0);
    let kinds_segs = kind_segments(w, data.height, |x, y| {
        wv_terrain::biome::display_kind(&wv_terrain::TerrainCell {
            height: cell(x, y).height,
            surface: cell(x, y).surface,
            biome: cell(x, y).biome,
        })
    });
    let coast: Vec<IsolineChain> = assemble_chains(&coast_segs)
        .into_iter()
        .map(|c| (chaikin(&c.pts, c.closed, 2), c.closed))
        .collect();
    let kinds: Vec<IsolineChain> = assemble_chains(&kinds_segs)
        .into_iter()
        .map(|c| (chaikin(&c.pts, c.closed, 2), c.closed))
        .collect();
    let pts: usize = coast.iter().map(|c| c.0.len()).sum::<usize>()
        + kinds.iter().map(|c| c.0.len()).sum::<usize>();
    let treatment = crate::palette::TreatmentFlags {
        coast_halo: true,
        river_centerline: true,
        ink_sublinear: true,
        ..crate::palette::TreatmentFlags::HAND_DRAWN
    };
    // tol = 0.25（scale 0.5 档，中近观；DP 成本随 tol 变小而升，这是偏保守的一档）。
    let ms = best_ms(|| {
        let out = assemble_strokes(&coast, &kinds, 0.25, 0.5, 1.0, treatment);
        assert!(!out.0.is_empty(), "起伏地形应有海岸链");
    });
    report(
        "assemble_strokes(DP 全链)",
        &format!("{} 条链 / {pts} 点 → 四路描边", coast.len() + kinds.len()),
        ms,
    );
}

/// 探针 3：底图整图合成（`terrain_pixels`）。
///
/// `Topographic` 是 CellMap 工作台的着色模式；`atlas = None` 走纯色回退
/// （无图集时的真实路径），`soften = false`（最薄的一档，数字偏乐观但同口径）。
#[test]
#[ignore = "性能探针：--ignored --nocapture 显式运行"]
fn perf_probe_terrain_pixels() {
    let data = terrain_1024();
    let pack = CellPack::fixture();
    let ms = best_ms(|| {
        let (px, res) = terrain_pixels(&data, ShadeMode::Topographic, None, false, &pack);
        assert!(!px.is_empty() && res > 0, "整图合成应产出像素");
    });
    report(
        "terrain_pixels(1024²)",
        "RGBA8 整图，行带并行（核数自适应）",
        ms,
    );
}

/// 一次性跑完三项并打一张对齐的表（报告直接抄这张）。
#[test]
#[ignore = "性能探针：--ignored --nocapture 显式运行"]
fn perf_probe_all() {
    println!("── perf probe (SIDE={SIDE}, RUNS={RUNS}, 取最小值) ──");
    perf_probe_grid_lines_rebuild();
    perf_probe_isoline_dp();
    perf_probe_terrain_pixels();
    // 编译档位自证：同一次构建里 debug 与 release 的断言开关不同，
    // 打出来才能确认「这行数字是哪个档的」。
    println!(
        "perf_probe\tprofile\tdebug_assertions={}\topt-level 由 Cargo.toml 决定",
        cfg!(debug_assertions)
    );
    let _ = STROKE_PX;
}
