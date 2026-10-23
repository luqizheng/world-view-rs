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
/// Topographic 三阶切点（与 topographic::pick 的 p_low / p_high 同口径）。
const CUT_LOW: f32 = 0.33;
const CUT_HIGH: f32 = 0.66;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let grid = p.grid.xy;
    if (grid.x < 1.0 || grid.y < 1.0) {
        return p.paper;
    }

    // 格坐标（texel 空间）。uv 来自 mesh 的 0..1；数据 y 自上而下，故翻转。
    let uv = vec2<f32>(in.uv.x, 1.0 - in.uv.y);
    let center = uv * grid - vec2<f32>(0.5); // 采样点落在格心

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

    var base: vec3<f32>;
    if (p.shape.y < 0.5) {
        // CellMap / Topographic：群系固定色块，**不随主题**（ADR-0051 §5）。
        base = biome_raw_approx(biome_id);
    } else {
        base = themed_land(is_land, h);
    }

    var rgb = base;
    if (is_land) {
        // 明暗只作用于陆面。水面用深度量程表达 —— 海面跟着地形一起明暗是
        // 现行烘焙路径最刺眼的 bug 之一。
        rgb = base * relief;
    } else {
        // ── 海岸三段色带（SPEC §3.8.4：滩 0–1 / 浅 1–3 / 深 3–9 格）──────
        // 由**距离场**连续驱动，不得逐格改色表达（SPEC §3.8.2）。
        let t_shore = smoothstep(0.0, p.shore_edges.x, d);
        let t_shallow = smoothstep(p.shore_edges.x, p.shore_edges.y, d);
        let t_deep = smoothstep(p.shore_edges.y, p.shore_edges.z, d);

        // 滩（近岸）→ 浅水 → 深水，三段连续过渡。
        let shore_c = p.theme[0].rgb;
        let shallow_c = mix(p.theme[0].rgb, p.theme[1].rgb, 0.55);
        let deep_c = p.theme[0].rgb * 0.82;
        let water = mix(mix(shore_c, shallow_c, t_shallow), deep_c, t_deep);
        // 近岸白滩（t_shore 最高处向纸色偏一点，给水线一个视觉落点）
        rgb = mix(mix(water, p.paper.rgb, 0.18 * t_shore * (1.0 - t_shallow)), water, t_shallow);
    }

    // ── 墨线（屏幕像素口径 + fwidth 抗锯齿）───────────────────────────────
    // 禁裸 smoothstep：边缘宽会随缩放变化（近观粗、远观糊）。
    // 用屏幕空间导数把距离折算成像素，线宽才恒定。
    let px_per_cell = max(p.shape.x, 1e-5);

    // 岸线：d 的等值线 d≈0.5 处即水陆中线（两侧最近异类格距离都是 1，
    // 中点 0.5）。dd=0 即岸线本身。
    let dd = abs(d - 0.5);
    let coast_grad = max(fwidth(dd), 1e-6);
    let coast_half = COAST_LINE_PX / px_per_cell;               // 格单位半宽
    let coast_ink = 1.0 - smoothstep(coast_half - coast_grad, coast_half + coast_grad, dd);
    rgb = mix(rgb, p.ink.rgb, clamp(coast_ink, 0.0, 1.0) * COAST_INK_ALPHA);

    // biome 边界（T27：4 邻 + 中心比较，ID 只比较不插值）。
    // 已知限制：斜边线宽最多 1.4× —— 0.8px 低 α 线视觉是「晕开」非「画歪」
    // （Q3 已裁定 P4 复评，精确化需 biome SDF）。
    // 仅主题分支画：CellMap 是数据视图，色块间不加墨线。
    if (p.shape.y >= 0.5 && is_land) {
        let diff = biome_boundary(uv, grid, biome_id);
        let lw = BIOME_LINE_PX / px_per_cell;
        let ink = 1.0 - smoothstep(0.0, lw, diff);
        rgb = mix(rgb, p.ink.rgb, clamp(ink, 0.0, 1.0) * BIOME_INK_ALPHA);
    }

    return vec4<f32>(rgb, 1.0);
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

/// biome 边界强度（T27）：中心与 4 邻比较。
/// 返回 0 = 紧邻异类格（在线上），1 = 四邻同类（无线）。
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
    // 线上返回 0（最靠近边界），无线返回 1
    return 1.0 - is_edge;
}

/// 主题分支陆面底色（Topographic 四阶）。
fn themed_land(is_land: bool, h: f32) -> vec3<f32> {
    if (!is_land) {
        return p.theme[0].rgb;
    }
    if (h > CUT_HIGH) { return p.theme[3].rgb; }
    if (h > CUT_LOW) { return p.theme[2].rgb; }
    return p.theme[1].rgb;
}

/// CellMap 分支：群系固定色块的 P1 占位色板。
///
/// **不是**完整的 BIOME_RAW_COLORS[21]——那需要 21 个 vec4 进 uniform，
/// 会改 uniform 布局。P1 先按族给 5 档近似，保证 ADR-0051 §5 的核心约束
/// ——「CellMap 看到的颜色与主题无关」——在语义上成立（精确色值随 P2 一起搬）。
fn biome_raw_approx(biome_id: u32) -> vec3<f32> {
    let id = f32(biome_id);
    if (id <= 0.0) {
        return vec3<f32>(0.72, 0.70, 0.64); // NONE 占位
    }
    if (id >= 14.0 && id <= 16.0) {
        return vec3<f32>(0.18, 0.38, 0.58); // 水域族：海洋/湖泊/河流
    }
    if (id == 9.0 || id == 19.0) {
        return vec3<f32>(0.88, 0.90, 0.92); // 雪原/盐碱地
    }
    if (id == 4.0 || id == 13.0 || id == 8.0) {
        return vec3<f32>(0.55, 0.50, 0.43); // 戈壁/山地/苔原
    }
    if (id == 5.0 || id == 18.0 || id == 2.0) {
        return vec3<f32>(0.85, 0.74, 0.44); // 沙漠/稀树草原/平原
    }
    return vec3<f32>(0.32, 0.55, 0.32); // 其余归绿族
}