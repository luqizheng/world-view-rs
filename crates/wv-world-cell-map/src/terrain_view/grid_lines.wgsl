// 逐格编辑网格的屏幕空间求值（2026-10-23，迭代档缺陷修复）
//
// ## 为什么用 shader
//
// 旧实现把「1px 屏幕线宽」换算成世界宽度（`px × view.scale`）再展开成 CPU
// 三角网：scale 一变，世界宽度变，mesh 必须整层重建（1024² 栅格 2050 条链）。
// 滚轮每推一档都重建，这是用户报的「个位数 FPS」的一条来源。
//
// 本 shader 把「线画在哪」搬到 fragment：
// - `fwidth(cell)` 直接给出**每格占多少屏幕像素**（世界坐标对像素的导数），
//   于是线宽天然是屏幕常量，缩放**不需要**任何 uniform 更新；
// - 双档：格距 ≥ 4px 画细档（每格一条，随密度淡出）；更密时按整数倍
//   （2 的幂）降采样画粗档，保证屏上定位格 ≥ 8px。
//   旧实现在 2px 处整层退场，世界级 fit（0.78px 格距）下就完全看不见。
//
// ## 单一数字载体
//
// 四个阈值（线宽 / 细档两端 / 粗档下限）与粗档 alpha 系数**全部来自 uniform**
// （`p.tune` / `p.mix`），本文件不出现任何阈值字面量——Rust 侧的
// `GRID_*` 常量是唯一真源（`grid_lines.rs`），两侧不可能各写一份而漂移。
//
// import 段路径来自 R1 spike 实测（bevy_sprite_render-0.19.1 的
// `mesh2d_vertex_output.wgsl`），不凭记忆。

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

// 绑定编号须与 grid_lines.rs 的 AsBindGroup 一致（uniform 只有一个块）。
// 全部字段用 vec4：encase 对 `[f32; 4]` 按 array stride 4 布局，
// WGSL uniform 要求 16 → 运行期 panic（field/material.rs 的血书）。
struct GridParams {
    // xy = 每格世界边长；zw = 栅格原点（世界坐标）
    geo: vec4<f32>,
    // rgb = 墨色（线性空间）；a = 基色 alpha（已含图层 opacity）
    ink: vec4<f32>,
    // x = 线宽 px；y = 细档退出 px；z = 细档全显 px；w = 粗档最小 px
    tune: vec4<f32>,
    // x = 粗档 alpha 系数；yzw 预留
    mix: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: GridParams;

/// 一条格线的覆盖率（0..1）：`coord` 是**格坐标**（每整数位是一条格线）。
///
/// `fract(coord - 0.5) - 0.5` 在格线处为 0；除以 `fwidth(coord)`（格坐标对
/// 像素的导数 = 1/格距像素）后即为「到最近格线的屏幕像素距离」。
/// 覆盖率 = `半线宽 + 0.5 - 距离` 的线性斜坡：中心满、边缘淡——1px 线宽
/// 落地时正好覆盖约一个像素，且任意缩放下不闪。
fn line_coverage(coord: f32) -> f32 {
    let d = abs(fract(coord - 0.5) - 0.5) / max(fwidth(coord), 1e-6);
    let half_px = p.tune.x * 0.5;
    return clamp(half_px + 0.5 - d, 0.0, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // 格坐标：世界坐标减去栅格原点再除以格径（与 CPU 的 grid_chains 同口径）。
    let cell = (in.world_position.xy - p.geo.zw) / p.geo.xy;
    // 每格屏上边长（px）。fwidth 在 quad 上恒为常数（线性插值），逐像素求值
    // 也不会有块状差异。
    let cell_px = 1.0 / max(fwidth(cell.x), 1e-6);

    // 粗档倍率：最小 2 的幂（exp2(ceil(log2(...)))），使粗档格距 ≥ tune.w。
    // 必须 2 的幂：粗线要落在格缘（k·m 格）上，否则与细档错拍。
    // 上限来自 uniform `mix.y`（CPU 侧 GRID_COARSE_MAX_MULT）——两端必须同值，
    // 否则极远观时 CPU 说「倍率 32」而 GPU 画「倍率 64」，粗线落在半档处。
    let need = max(p.tune.w / max(cell_px, 1e-6), 1.0);
    let stride = min(exp2(ceil(log2(need))), p.mix.y);

    // 细档强度：≥ tune.z 全显，≤ tune.y 退出（细档退出≠整层退场）。
    let fine = clamp((cell_px - p.tune.y) / (p.tune.z - p.tune.y), 0.0, 1.0);
    let m_fine = max(line_coverage(cell.x), line_coverage(cell.y)) * fine;

    // 粗档：把格坐标压到 stride 格一条。粗线恒在（系数来自 uniform）。
    let m_coarse = max(
        line_coverage(cell.x / stride),
        line_coverage(cell.y / stride),
    ) * p.mix.x;

    // 粗线是细线的子集，取较强者即可，不必相加（相加会在重合处过曝）。
    let a = max(m_fine, m_coarse) * p.ink.a;
    if (a <= 0.002) {
        discard;
    }
    return vec4<f32>(p.ink.rgb, a);
}
