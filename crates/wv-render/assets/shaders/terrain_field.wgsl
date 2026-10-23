// 地形场逐像素求值（ADR-0053 D1 · P0 规格锁 T1–T3 + T7–T18 + T23/T24/T27）
//
// 三张数据纹理进，逐像素出一张颜色。**shader 只决定「在哪个像素求值」，
// 不决定「求什么值」**——色板与 SeaRecipe 等已定数值原样搬进 uniform
// （T35 / ADR-0053 D5）。
//
// P1 落地：T1–T3（数据面）+ T7–T18（求值面）+ T23/T24/T27（墨线）。
// P2 才做（此处刻意不实现，参数位已预留）：T19–T22 域扭曲、T28–T30 水彩、
// T31–T34 缩放自适应三档。
//
// import 段路径来自 R1 spike 实测（同版本 bevy_sprite_render-0.19.1 crate 源码
// + 编译运行验证），不凭记忆。

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

// ── 绑定（编号须与 material.rs 的 AsBindGroup 一致）────────────────────────
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: FieldParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var height_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var height_smp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var coast_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var coast_smp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var attr_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var attr_smp: sampler;

// vec3 在 std140 下补到 16 字节；这里用 vec4 分组避免隐式 padding 错位。
struct FieldParams {
    grid: vec4<f32>,        // xy = 格宽/格高
    shape: vec4<f32>,       // x = px_per_cell, y = branch, z = z_exag, w = grad_step
    pad0: vec4<f32>,
    light_dir: vec4<f32>,   // xyz = 光源方向, w = 预留
    pad1: vec4<f32>,
    shore_edges: vec4<f32>, // x = 1（滩）, y = 3（浅）, z = 9（深）, w = wash_amt
    relief: vec4<f32>,      // xy = 明暗钳制, z = lod_blend, w = 预留
    // 主题四阶色：[0]=water [1]=land_low [2]=land_mid [3]=land_high
    theme: array<vec4<f32>, 4>,
    ink: vec4<f32>,
    paper: vec4<f32>,
};

// ── 常量（与 material.rs 同源）───────────────────────────────────────────

/// T11：岸线墨线宽（**屏幕像素**，SPEC §3.8.1 口径）。
const COAST_LINE_PX: f32 = 1.2;
/// T24：biome 边界线宽（屏幕像素）。地貌交界是「暗示」不是「划线」。
const BIOME_LINE_PX: f32 = 0.8;
/// 岸线墨线不透明度（SPEC §1.5「描线 −45% 明度」的落地强度）。
const COAST_INK_ALPHA: f32 = 0.55;
/// biome 界墨线不透明度（比岸线浅一档）。
const BIOME_INK_ALPHA: f32 = 0.38;
/// 高度量程（米）= H_MAX(6000) − H_MIN(-4000)，与 pack.rs 同源。
const H_RANGE_M: f32 = 10000.0;

/// T28：水彩幅度（±7% 明度）。< 0.05 看不出，> 0.12 破坏「色块可辨」。
const WASH_AMT: f32 = 0.07;
/// T29：水彩噪声频率锚（格空间）。波长约 2.2 格——是**水彩晕染**不是噪声纹理。
const WASH_FREQ: f32 = 0.45;
/// T30：沙漠/雪原等大地色系的减免系数。
const WASH_SPECIAL_K: f32 = 0.5;
/// Topographic 三阶切点（与 topographic::pick 的 p_low / p_high 同口径）。
const CUT_LOW: f32 = 0.33;
const CUT_HIGH: f32 = 0.66;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let grid = p.grid.xy;
    if (grid.x < 1.0 || grid.y < 1.0) {
        return p.paper;
    }

    // 格坐标（texel 空间）。
    //
    // ⚠️ **u 与 v 都要翻**：Bevy 的 `Rectangle` mesh 顶点序为
    // `[1,0] [0,0] [0,1] [1,1]`——**u 轴是反的**（世界左边 u=1、右边 u=0），
    // v 轴顶部为 1。数据纹理是「左上原点、x 向右、y 向下」，
    // 故 `u = 1 − in.uv.x`、`v = 1 − in.uv.y`。
    // 只翻 v 会让整幅图**左右镜像**（实测踩过：相机对准的海岸画到了镜像位置）。
    let uv0 = vec2<f32>(1.0 - in.uv.x, 1.0 - in.uv.y);

    // T19–T22 域扭曲：在**格坐标**里扰动采样位置（不是屏幕坐标——否则
    // 缩放时岸线会「游动」，作者会以为数据变了）。幅度由 Rust 侧按
    // `px_per_cell` 算好写进 `p.pad0.x`（远观大 / 近观收敛）。
    let g = uv0 * grid;
    let gw = g + warp_offset(g);
    let uv = gw / grid;                       // 扭曲后的 uv（供三张纹理采样）
    let center = gw - vec2<f32>(0.5);         // texel 中心空间（供双三次）

    // ── 亚格高度：Catmull-Rom 双三次 16 tap（T13）────────────────────────
    let h = sample_height_bicubic(center);

    // ── 明暗：中心差分梯度 × 光源（T14–T17）────────────────────────────
    let e = p.shape.w;
    let dx = (sample_height_bicubic(center + vec2<f32>(e, 0.0))
            - sample_height_bicubic(center - vec2<f32>(e, 0.0))) / (2.0 * e);
    let dy = (sample_height_bicubic(center + vec2<f32>(0.0, e))
            - sample_height_bicubic(center - vec2<f32>(0.0, e))) / (2.0 * e);

    // 归一化高度 → 世界斜率（米/格），乘 T15 高程夸张。
    let zx = dx * H_RANGE_M * p.shape.z;
    let zy = dy * H_RANGE_M * p.shape.z;
    let n = normalize(vec3<f32>(-zx, -zy, 1.0));
    let lambert = clamp(dot(n, normalize(p.light_dir.xyz)), 0.0, 1.0);

    // T17：明暗乘子钳制 —— 陡崖不压黑也不过曝。
    let relief = mix(p.relief.x, p.relief.y, lambert);

    // ── 海岸距离场（T3 → T7–T9）────────────────────────────────────────
    let d = sample_coast(center);

    // ── 底色 ────────────────────────────────────────────────────────────
    let attr = textureSampleLevel(attr_tex, attr_smp, uv, 0.0);
    let is_land = attr.r > 0.5;         // attr.r = 湿判定结果（pack.rs 口径）
    let biome_id = u32(round(attr.g * 255.0));

    // ── 有符号距离（SUB-CELL 关键）──────────────────────────────────────
    //
    // `coast_tex` 存的是「到最近**异类格**的距离」，相邻异类格的格心距离为 1。
    // 直接按 `is_land` 二值切换 → 水陆边界**逐格硬切**，放大即见阶梯
    // （违反 SPEC §3.8.2「边界不得逐格改色表达」）。
    //
    // 改成有符号距离：格心处 `sd = ±0.5`，于是**相邻两格的格心之间线性插值
    // 恰好穿过 0**——0 的位置就是两格的交界线。这样水陆过渡是连续函数，
    // 任意放大倍数下岸线都是平滑曲线。
    let sd = select(-(d - 0.5), d - 0.5, is_land); // 正 = 陆，负 = 水
    let land_t = smoothstep(-0.5, 0.5, sd);

    var base: vec3<f32>;
    if (p.shape.y < 0.5) {
        // CellMap / Topographic：群系固定色块，**不随主题**（ADR-0051 §5）。
        base = biome_raw_approx(biome_id);
    } else {
        base = themed_land(is_land, h);
    }
    // 明暗只作用于陆面。水面用深度量程表达 —— 海面跟着地形一起明暗是
    // 现行烘焙路径最刺眼的 bug 之一。
    let land_col = base * relief;

    // ── 海岸三段色带（SPEC §3.8.4：滩 0–1 / 浅 1–3 / 深 3–9 格）──────────
    // 用「入水深度」`wd = max(-sd, 0)` 驱动，全程连续量。
    let wd = max(-sd, 0.0);
    let t_shore = smoothstep(0.0, p.shore_edges.x, wd);
    let t_shallow = smoothstep(p.shore_edges.x, p.shore_edges.y, wd);
    let t_deep = smoothstep(p.shore_edges.y, p.shore_edges.z, wd);

    let shallow_c = mix(p.theme[0].rgb, p.theme[1].rgb, 0.55);
    let deep_c = p.theme[0].rgb * 0.82;
    var water_col = mix(mix(p.theme[0].rgb, shallow_c, t_shallow), deep_c, t_deep);
    // 近岸白滩（t_shore 最高处向纸色偏一点，给水线一个视觉落点）
    water_col = mix(water_col, p.paper.rgb, 0.18 * t_shore * (1.0 - t_shallow));

    // 亚格混合：一格宽的软过渡替代逐格硬切。
    var rgb = mix(water_col, land_col, land_t);

    // ── 水彩颗粒（T28–T30）──────────────────────────────────────────────
    // 只调**明度**，不动色相——SPEC §1.6 禁令的边界：是水彩晕染，
    // 不是往画面上铺噪声纹理。
    // 水面**不叠**（T30）：海的呼吸感走 SeaRecipe 的连续量，叠两层会脏。
    if (land_t > 0.5 && p.shore_edges.w > 0.0) {
        // 频率锚在格空间（随内容走，不随屏幕走——否则缩放时颗粒会「沸腾」）。
        let wash_n = fbm3(g, 733.0, WASH_FREQ);
        // 特殊面减免：沙漠(5)/雪原(9)/盐碱(19) 是大地色系，呼吸位小一些。
        var k = 1.0;
        if (biome_id == 5u || biome_id == 9u || biome_id == 19u) {
            k = WASH_SPECIAL_K;
        }
        let wash = (wash_n - 0.5) * 2.0 * WASH_AMT * k;
        rgb = rgb * (1.0 + wash);
    }

    // ── 墨线（屏幕像素口径 + fwidth 抗锯齿）───────────────────────────────
    // 禁裸 smoothstep：边缘宽会随缩放变化（近观粗、远观糊）。
    // 用屏幕空间导数把距离折算成像素，线宽才恒定。
    let px_per_cell = max(p.shape.x, 1e-5);

    // T31–T34 缩放自适应：墨线强度随每格像素数收敛。
    // - 近观（≥3 px/格）：全开
    // - 过渡（1~3）：线性淡出（避免小尺幅时墨线糊成一片灰）
    // - 远观（<1）：全关，只留色域（否则彩色噪点会闪）
    let ink_gate = smoothstep(1.0, 3.0, px_per_cell);

    // 岸线：`sd = 0` 即岸线本身（相邻两格格心之间的交界）。
    let dd = abs(sd);
    let coast_grad = max(fwidth(dd), 1e-6);
    let coast_half = COAST_LINE_PX / px_per_cell;               // 格单位半宽
    let coast_ink = 1.0 - smoothstep(coast_half - coast_grad, coast_half + coast_grad, dd);
    rgb = mix(rgb, p.ink.rgb, clamp(coast_ink, 0.0, 1.0) * COAST_INK_ALPHA * ink_gate);

    // biome 边界（T27：4 邻 + 中心比较，ID 只比较不插值）。
    //
    // **已知限制（Q3 裁定 A：不做 biome SDF，P4 复评）**：
    // 4 邻比较只能给出「整格」级的判定，天然渲成**一格宽**的带。
    // 一格在屏幕上的宽度是 `px_per_cell`，故：
    // - 远观（px_per_cell < 1）：一格 ≈ 1px，带子接近 T24 的 0.8px 目标值 ✓
    // - 近观（px_per_cell 大）：带子会涨到几十 px，读起来是「色块」而非「线」
    //
    // 因此按 T24 的精神（边界是暗示不是划线）做**淡出**：
    // 当一格已明显宽于目标线宽若干倍时把线收掉——那时作者看的是 biome 色块本身，
    // 一条 1 格宽的假线只会误导。精确的亚格边界需要 biome SDF，属 P4。
    if (p.shape.y >= 0.5 && is_land) {
        let band_px = px_per_cell; // 4 邻检测的天然带子宽度（1 格 = px_per_cell px）
        let fade = 1.0 - smoothstep(BIOME_LINE_PX * 3.0, BIOME_LINE_PX * 8.0, band_px);
        // 再乘缩放门控：T33 要求**远观墨线全关只留色域**——否则 1 px/格
        // 以下时 biome 界会变成彩色噪点并随缩放闪烁。
        let total = fade * ink_gate;
        if (total > 0.01) {
            let is_edge = biome_boundary(uv, grid, biome_id);
            rgb = mix(rgb, p.ink.rgb, is_edge * BIOME_INK_ALPHA * total);
        }
    }

    return vec4<f32>(rgb, 1.0);
}

// ── 程序化噪声（T20/T29 共用）────────────────────────────────────────────
//
// 不用纹理噪声：噪声纹理要额外资产与采样，而这里只需 3 octave 的低频
// 抖动，纯算术更省也更确定（同输入必得同输出，守护项 8）。

/// 二维哈希 → [0,1)。确定、无随机数（SPEC §3.8.6）。
fn hash21(p: vec2<f32>, seed: f32) -> f32 {
    var q = fract(p * vec2<f32>(127.1, 311.7) + vec2<f32>(seed, seed * 1.37));
    q = q + vec2<f32>(dot(q, q + 45.32));
    return fract(q.x * q.y * 95.4307);
}

/// 值噪声：整数格点哈希 + smooth 双线性。
fn vnoise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i, seed);
    let b = hash21(i + vec2<f32>(1.0, 0.0), seed);
    let c = hash21(i + vec2<f32>(0.0, 1.0), seed);
    let d = hash21(i + vec2<f32>(1.0, 1.0), seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

/// 3 octave fBm。频率按 `freq` 起、每层 ×2；振幅 0.5 起、每层 ×0.5。
///
/// T20 指定域扭曲用「1/6、1/3、2/3 格」三档 → 起始频率 `1/6`。
fn fbm3(p: vec2<f32>, seed: f32, freq0: f32) -> f32 {
    var sum = 0.0;
    var norm = 0.0;
    var amp = 0.5;
    var freq = freq0;
    for (var i = 0; i < 3; i = i + 1) {
        sum = sum + vnoise(p * freq, seed + f32(i) * 19.0) * amp;
        norm = norm + amp;
        amp = amp * 0.5;
        freq = freq * 2.0;
    }
    return sum / max(norm, 1e-6);
}

/// 域扭曲偏移（T19–T22）。P2 的核心：把格网对齐打散成手绘抖动。
///
/// - **T21 采样坐标系用「格坐标」**而不是屏幕坐标：屏幕坐标会让海岸在
///   缩放时「游动」，作者会以为数据变了。
/// - **T22 两通道种子 +17 / +41** 去相关，否则 x/y 会同步偏移、
///   抖动出斜向拖影。
fn warp_offset(g: vec2<f32>) -> vec2<f32> {
    let amp = p.pad0.x; // T19 幅度，Rust 侧按 px_per_cell 算好（见 material::warp_amp）
    if (amp <= 0.0) {
        return vec2<f32>(0.0, 0.0);
    }
    let nx = fbm3(g, 17.0, 1.0 / 6.0);
    let ny = fbm3(g, 41.0, 1.0 / 6.0);
    // 中心化到 [-1,1] 再乘幅度（格单位）
    return (vec2<f32>(nx, ny) * 2.0 - vec2<f32>(1.0)) * amp;
}

// ── Catmull-Rom 双三次（16 tap，T13）──────────────────────────────────────
//
// 双线性（4 tap）的梯度在格边界 C1 不连续 → hillshade 出规则格纹折痕，
// 「看着像 bug 不是风格」。16 tap 换 C1 连续，是 P1 最核心的一处成本。
//
// 权重要素（t ∈ [0,1]）：
//   w0 = -0.5t³ + t² - 0.5t
//   w1 =  1.5t³ - 2.5t² + 1
//   w2 = -1.5t³ + 2t² + 0.5t
//   w3 =  0.5t³ - 0.5t²

fn cubic_weights(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(
        -0.5 * t3 + t2 - 0.5 * t,
         1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
         0.5 * t3 - 0.5 * t2,
    );
}

fn sample_height_bicubic(uv_cell: vec2<f32>) -> f32 {
    let g = p.grid.xy;
    // texel 中心空间：格心 (i+0.5) 对应整数 i
    let tc = uv_cell - vec2<f32>(0.5);
    let f = fract(tc);
    let base = floor(tc) - vec2<f32>(1.0);
    let wx = cubic_weights(f.x);
    let wy = cubic_weights(f.y);

    var acc = 0.0;
    for (var j = 0; j < 4; j = j + 1) {
        let offy = f32(j) - 1.0;
        var wyj = 0.0;
        if (j == 0) { wyj = wy.x; } else if (j == 1) { wyj = wy.y; }
        else if (j == 2) { wyj = wy.z; } else { wyj = wy.w; }
        var row = 0.0;
        for (var i = 0; i < 4; i = i + 1) {
            var wxi = 0.0;
            if (i == 0) { wxi = wx.x; } else if (i == 1) { wxi = wx.y; }
            else if (i == 2) { wxi = wx.z; } else { wxi = wx.w; }
            row = row + height_texel(base + vec2<f32>(f32(i) - 1.0, offy)) * (wxi * wyj);
        }
        acc = acc + row;
    }
    // Catmull-Rom 四项权重和恒为 1（负权重由正权重补偿），无需再归一化。
    return acc;
}

/// 单点取样（钳在图内，避免边缘越界采样导致的黑边）。
fn height_texel(cell: vec2<f32>) -> f32 {
    let g = p.grid.xy;
    let c = clamp(cell, vec2<f32>(0.0), g - vec2<f32>(1.0));
    let uv = (c + vec2<f32>(0.5)) / g;
    return textureSampleLevel(height_tex, height_smp, uv, 0.0).r;
}

/// 海岸距离（T3）。Linear 采样保亚格连续 —— 整数截断场插值会出平顶带。
fn sample_coast(uv_cell: vec2<f32>) -> f32 {
    let g = p.grid.xy;
    let uv = uv_cell / g;
    let r = textureSampleLevel(coast_tex, coast_smp, uv, 0.0).r;
    return r * 32.0; // pack 编码 d/32 → 还原为格单位
}

/// biome 边界检测（T27）：中心与 4 邻比较，ID 只比较不插值。
/// 返回 1 = 四邻中有异类（本格落在边界上），0 = 四邻同类。
fn biome_boundary(uv: vec2<f32>, grid: vec2<f32>, center_id: u32) -> f32 {
    let t = 1.0 / grid;
    var is_edge = 0.0;
    let s1 = textureSampleLevel(attr_tex, attr_smp, uv + vec2<f32>(t.x, 0.0), 0.0);
    if (u32(round(s1.g * 255.0)) != center_id) { is_edge = 1.0; }
    let s2 = textureSampleLevel(attr_tex, attr_smp, uv - vec2<f32>(t.x, 0.0), 0.0);
    if (u32(round(s2.g * 255.0)) != center_id) { is_edge = 1.0; }
    let s3 = textureSampleLevel(attr_tex, attr_smp, uv + vec2<f32>(0.0, t.y), 0.0);
    if (u32(round(s3.g * 255.0)) != center_id) { is_edge = 1.0; }
    let s4 = textureSampleLevel(attr_tex, attr_smp, uv - vec2<f32>(0.0, t.y), 0.0);
    if (u32(round(s4.g * 255.0)) != center_id) { is_edge = 1.0; }
    // 1 = 本格贴边界，0 = 内部格
    return is_edge;
}

/// 主题分支陆面底色（Topographic 四阶）。
fn themed_land(is_land: bool, h: f32) -> vec3<f32> {
    if (!is_land) {
        return p.theme[0].rgb;
    }
    // 用**窄平滑**代替硬阈值：硬切在远观（1 px/格以下）会在两个色阶之间
    // 产生高频锯齿，读起来是彩色噪点且随缩放闪烁（T33 要避免的正是这个）。
    // 过渡带 ±0.02 归一化高度 ≈ 小几百米，视觉上仍是「三阶色阶」。
    const TIER_SOFT: f32 = 0.02;
    let hi_t = smoothstep(CUT_HIGH - TIER_SOFT, CUT_HIGH + TIER_SOFT, h);
    let lo_t = smoothstep(CUT_LOW - TIER_SOFT, CUT_LOW + TIER_SOFT, h);
    let low_mid = mix(p.theme[1].rgb, p.theme[2].rgb, lo_t);
    return mix(low_mid, p.theme[3].rgb, hi_t);
}

/// sRGB 分量 → 线性。手写的色板常量都是**人眼/取色器口径的 sRGB**
/// （如 `0.32, 0.55, 0.32` 对应 `#52 8C 52`），而片元输出住线性空间，
/// 直接用会整体偏亮发灰（同 `FieldPalette` 里踩过的色彩空间坑）。
fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

/// CellMap 分支：群系固定色块的 P1 占位色板。
///
/// **不是**完整的 BIOME_RAW_COLORS[21]——那需要 21 个 vec4 进 uniform，
/// 会改 uniform 布局。P1 先按族给 5 档近似，保证 ADR-0051 §5 的核心约束
/// ——「CellMap 看到的颜色与主题无关」——在语义上成立（精确色值随 P2 一起搬）。
fn biome_raw_approx(biome_id: u32) -> vec3<f32> {
    let id = f32(biome_id);
    if (id <= 0.0) {
        return srgb_to_linear(vec3<f32>(0.72, 0.70, 0.64)); // NONE 占位
    }
    if (id >= 14.0 && id <= 16.0) {
        return srgb_to_linear(vec3<f32>(0.18, 0.38, 0.58)); // 水域族：海洋/湖泊/河流
    }
    if (id == 9.0 || id == 19.0) {
        return srgb_to_linear(vec3<f32>(0.88, 0.90, 0.92)); // 雪原/盐碱地
    }
    if (id == 4.0 || id == 13.0 || id == 8.0) {
        return srgb_to_linear(vec3<f32>(0.55, 0.50, 0.43)); // 戈壁/山地/苔原
    }
    if (id == 5.0 || id == 18.0 || id == 2.0) {
        return srgb_to_linear(vec3<f32>(0.85, 0.74, 0.44)); // 沙漠/稀树草原/平原
    }
    return srgb_to_linear(vec3<f32>(0.32, 0.55, 0.32)); // 其余归绿族
}