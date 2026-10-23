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
    let uv = gw / grid;                       // 扭曲后的 uv（供 attr 采样）

    // 采样位置 `pos`：**格坐标（边界空间）**——第 i 格占 `[i, i+1)`，格心在
    // `i + 0.5`。三张纹理**统一用这一个位置**，不再各自 ±0.5。
    //
    // ⚠️ 这里修掉一处静默错位：原本 `coast` / `height` 走的是 `gw − 0.5`，
    // 而 `attr` 走 `gw / grid`，两者差**半格**。半格意味着 SDF 的符号
    // 与水陆掩码的边界不在同一位置——岸线墨线会整体偏移，且偏移量随缩放
    // 线性放大（32 px/格 时是 16 px）。fbm 地形平滑，肉眼难判，属静默缺陷。
    let pos = gw;

    let px_per_cell = max(p.shape.x, 1e-5);
    // 一个屏幕像素覆盖多少个格（缩小时 > 1）。
    let footprint = clamp(1.0 / px_per_cell, 1.0, 8.0);
    // T32/T33：`1 = 纯面积平均（远观）`，`0 = 纯双三次（近观）`。
    let lod_t = 1.0 - smoothstep(1.0, 3.0, px_per_cell);

    // ── 亚格高度 + 明暗梯度：一次 4×4 采样出三者（P5，T13/T14–T17）──────
    // `hg.x` 是高度（与旧双三次逐位同值），`hg.yz` 是解析偏导（米/格）。
    let hg = sample_height_bicubic_grad(pos);
    let h = mix(hg.x, box_avg_height(pos, footprint), lod_t);

    // ── 明暗：解析梯度 × 光源（T14–T17）──────────────────────────────────
    // 旧写法为中心差分 `(h(p+e) − h(p−e)) / 2e`，为 dx 与 dy 各重取两次
    // 双三次 = 64 tap；解析偏导与 `h` 共享同一次16 tap，省去全部重采样。
    let zx = hg.y * H_RANGE_M * p.shape.z;
    let zy = hg.z * H_RANGE_M * p.shape.z;
    let n = normalize(vec3<f32>(-zx, -zy, 1.0));
    let lambert = clamp(dot(n, normalize(p.light_dir.xyz)), 0.0, 1.0);

    // T17：明暗乘子钳制 —— 陡崖不压黑也不过曝。
    // 远观把明暗向 1.0 收敛：梯度必须靠双三次算（插值要连续性），但那个
    // 空间频率在小于 1 px/格 时已超出屏幕能表现的范围，留着只会闪成噪点。
    // 这与 T33「远观只留色域」一致——山体仍靠三阶色阶读出层次。
    let relief = mix(mix(p.relief.x, p.relief.y, lambert), 1.0, lod_t);

    // ── 海岸距离场（T3）──────────────────────────────────────────────────
    // 取**原始归一化值**，解码留给下面按编码模式做（`p.pad1.y`）。
    let coast_raw = mix(sample_coast_raw(pos), box_avg_coast(pos, footprint), lod_t);

    // ── 底色 ────────────────────────────────────────────────────────────
    let attr = textureSampleLevel(attr_tex, attr_smp, uv, 0.0);
    let is_land = attr.r > 0.5;         // attr.r = 湿判定结果（pack.rs 口径）
    let biome_id = u32(round(attr.g * 255.0));

    // ── 有符号距离（SUB-CELL 关键）──────────────────────────────────────
    //
    // 两种编码模式（`p.pad1.y`，须与 `pack::CoastEncoding` 一致）：
    //
    // **Unsigned（默认，T3 原值）**：纹理存「到最近异类格的距离」，0 = 岸上。
    // 符号只能取自 `attr.r`（Nearest），于是 `sd` 在格边界处**从 +0.5 跳到
    // −0.5**——`land_t` 一帧内完成 0→1，即**逐格硬切**，放大见台阶。
    //
    // **Signed（实验）**：纹理存有符号距离，岸线两侧格心分别是 `+1` / `−1`，
    // 双线性插值穿过 0 的位置正好是交界 → `sd` 是**连续函数**，`land_t` 在
    // 一个格内平滑过渡。这是唯一能让水陆交界在亚格尺度连续的办法（不改数据
    // 分辨率的前提下）。
    var sd: f32; // 正 = 陆，负 = 水，单位 = 格
    var d: f32;  // 到最近异类格的**无符号**距离（色带用它，两模式都成立）
    if (p.pad1.y > 0.5) {
        sd = (coast_raw - 0.5) * 64.0; // pack 编码 (sd + 32) / 64
        d = abs(sd);
    } else {
        d = coast_raw * 32.0; // pack 编码 d / 32
        sd = select(-(d - 0.5), d - 0.5, is_land);
    }
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
    // `px_per_cell` 已在函数开头声明（LOD 也要用）。

    // T31–T34 缩放自适应：墨线强度随每格像素数收敛。
    // - 近观（≥3 px/格）：全开
    // - 过渡（1~3）：线性淡出（避免小尺幅时墨线糊成一片灰）
    // - 远观（<1）：全关，只留色域（否则彩色噪点会闪）
    let ink_gate = smoothstep(1.0, 3.0, px_per_cell);

    // ── 岸线墨线（T23 / T11 = 1.2 屏幕 px）────────────────────────────────
    //
    // **用「水陆掩码的屏幕空间不连续」定位，不用距离场。**
    //
    // 旧写法是 `dd = |d − 0.5|`，配合阈值 `1.2 / px_per_cell`——它**一个像素
    // 都画不出来**，实测全图零墨线。原因在数据结构：`coast_tex` 存「到最近
    // 异类格的距离」，在岸线**两侧的两个格心处都是 1**，中间平顶，所以
    // `|d − 0.5|` 的最小值就是 0.5，永远大于任何合理线宽。
    // 换成 `|d − 1|` 也不行：平顶使墨带宽度达到 1.15 格，不是线是带。
    // 根因是**无符号距离场在边界处是平顶的**（T5 的直接后果），拿它定不住
    // 一条亚格宽度的线。
    //
    // 可行做法：`attr.r` 是 Nearest 采样——格内恒定、跨界跳变，故
    // `fwidth(attr.r)` 只在格边界那个像素为正。用它定位，线宽天然是
    // **1 个屏幕像素**（`fwidth` 的分辨率下限），且随缩放恒定。
    //
    // 已知偏差：T23 写 1.2 px，实际约 1 px。1.2 与 1 的差别小于一像素，
    // 且 `fwidth` 无法表达小数线宽——要精确到 1.2 需有符号 SDF（乙案，
    // 已被裁定不采纳）。此处按「1 px 就是本数据模型能画的最细线」登记。
    //
    // **只在水侧落墨**：`fwidth` 会把边界两侧的像素都标成「有跳变」，
    // 两侧都画就是 2 px（实测）。手绘地图的岸线惯例也是画在水侧那一侧，
    // 且这样线宽正好收到 1 px。
    var coast_ink: f32;
    if (p.pad1.y > 0.5) {
        // **Signed**：距离场在岸线处穿过 0（不再平顶），可以直接用它定线宽。
        // 线宽以屏幕像素给：`half_w`（格）= 半个线宽 / 每格像素数。
        let dd = abs(sd);
        let half_w = 0.5 * COAST_LINE_PX / px_per_cell;
        let aa = max(fwidth(dd), 1e-6);
        coast_ink = 1.0 - smoothstep(half_w - aa, half_w + aa, dd);
    } else {
        // **Unsigned**：距离场平顶，定不住亚格线宽，只能靠掩码的屏幕空间
        // 跳变定位（详见上方 `mask_edge` 的说明）。
        let mask_edge = fwidth(attr.r);
        coast_ink = smoothstep(0.05, 0.45, mask_edge) * select(0.0, 1.0, !is_land);
    }
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

/// Catmull-Rom 权重对 `t` 的**解析导数**（P5）。
///
/// 对上面四条逐项求导（dt3 = 3t², dt2 = 2t）：
///   w0' = -1.5t² + 2t − 0.5
///   w1' =  4.5t² − 5t
///   w2' = −4.5t² + 4t + 0.5
///   w3' =  1.5t² − t
///
/// 四项和恒为 0（权重和 = 1 是常数 → 导数为 0），这是解析解的自检项。
fn cubic_weights_deriv(t: f32) -> vec4<f32> {
    let t2 = t * t;
    return vec4<f32>(
        -1.5 * t2 + 2.0 * t - 0.5,
         4.5 * t2 - 5.0 * t,
        -4.5 * t2 + 4.0 * t + 0.5,
         1.5 * t2 - t,
    );
}

/// 按下标取 `vec4` 分量。`vec4` 的动态下标在部分驱动上退化，
/// 统一走展开分支——与权重取分量同一套写法。
fn pick4(v: vec4<f32>, i: i32) -> f32 {
    if (i == 0) { return v.x; }
    if (i == 1) { return v.y; }
    if (i == 2) { return v.z; }
    return v.w;
}

/// Catmull-Rom 双三次（T13）的值与**解析偏导**，一次 4×4 采样同时给出（P5）。
///
/// 返回 `vec3(h, dh/dx, dh/dy)`，导数单位是「归一化高度 / 格」。
///
/// ## 坐标约定（**唯一权威，改前先读**）
///
/// 入参 `pos` 是**格坐标（边界空间）**：第 i 格占 `[i, i+1)`，格心在 `i + 0.5`。
///
/// 换算到纹素下标：`tc = pos − 0.5`，于是格心 `i+0.5` 映射到整数 `i`，
/// `f = fract(tc)` 是格内的亚格位置。四点下标取 `floor(tc) − 1 … floor(tc) + 2`
/// （Catmull-Rom 需要左右各一个邻点），权重 `[w0..w3]` 依次乘上去。
///
/// 校验：`pos = i + 0.5` → `tc = i` → `f = 0` → 只有 `w1 = 1` → 取
/// `floor(tc) + 0 = i` → 得到第 i 格自己的纹素。**无偏移**。
///
/// `tc = pos − 0.5` 是**平移**，对 x、y 的导数因子都是 1，故
/// `d/dpos = d/dtc`，无需链式系数。
///
/// ⚠️ 此前写成 `base = floor(tc) − 1` 且下标取 `base + (i − 1)`，等于把四点
/// 窗口整体左移一格：`f = 0` 时取到的是 `i − 1` 号纹素，**整条高度场偏一个
/// 格**。再叠加调用方传的 `gw − 0.5`，height 实际落在 `gw − 1.5`、coast 落在
/// `gw − 0.5`、attr 落在 `gw`——三套不同配准。fbm 地形平滑，肉眼难判，
/// 属静默缺陷；现统一为全部在 `gw` 采样。
///
/// ## 为什么能一次采样出三个量
///
/// 值与两个偏导共享同一个 4×4 窗口与同一次 `height_texel` 取样：
///   h    = Σᵢⱼ texel(i,j) · wxᵢ(f.x) · wyⱼ(f.y)
///   ∂h/∂x = Σᵢⱼ texel(i,j) · wx′ᵢ(f.x) · wyⱼ(f.y)
///   ∂h/∂y = Σᵢⱼ texel(i,j) · wxᵢ(f.x) · wy′ⱼ(f.y)
/// 旧写法为 h 取 16 tap、再为 dh/dx 中心差分取 2×16、再为 dh/dy 取 2×16，
/// 共 **80 tap**；现在一次 **16 tap** 同时给出三者。
///
/// ## 与旧中心差分的差异（**不是等价替换**）
///
/// 旧式 `(h(p+e) − h(p−e)) / 2e` 是**数值近似**，误差 O(e²)，且在 `f`
/// 靠近 0 或 1（格边界）时精度最差：`fract()` 的不连续让 e 附近的
/// 两点可能落在不同四点窗口，差分实际跨了窗口边界。解析导数没有这个坑——
/// 它在窗口内处处成立，这正是 T13 买 C1 连续性时想要的东西。
///
/// 代价：梯度不再受 `GRAD_STEP`（T14）控制。T14 的作用是让差分步长
/// 可调以滤高频噪声；解析导数对 16 tap 窗口内的全频段一视同仁。
/// 远观档的噪声抑制仍由 T33 的 `lod_t → 1` 把 relief 收敛到 1.0 承担，
/// 故 T14 的意图未被绕过，`grad_step` 槽位保留但不再参与梯度计算。
fn sample_height_bicubic_grad(pos: vec2<f32>) -> vec3<f32> {
    let tc = pos - vec2<f32>(0.5);
    let f = fract(tc);
    let base = floor(tc);
    let wx = cubic_weights(f.x);
    let wy = cubic_weights(f.y);
    let dxw = cubic_weights_deriv(f.x);
    let dyw = cubic_weights_deriv(f.y);

    var h = 0.0;
    var gx = 0.0;
    var gy = 0.0;
    for (var j = 0; j < 4; j = j + 1) {
        let offy = f32(j) - 1.0;
        let wyj = pick4(wy, j);
        let dywj = pick4(dyw, j);
        for (var i = 0; i < 4; i = i + 1) {
            let wxi = pick4(wx, i);
            let dxwi = pick4(dxw, i);
            let t = height_texel(base + vec2<f32>(f32(i) - 1.0, offy));
            h = h + t * (wxi * wyj);
            gx = gx + t * (dxwi * wyj);
            gy = gy + t * (wxi * dywj);
        }
    }
    // Catmull-Rom 四项权重和恒为 1（负权重由正权重补偿），值通道无需归一化；
    // 导数通道的权重和恒为 0（常数 1 的导数），无需去偏。
    return vec3<f32>(h, gx, gy);
}

/// 按**纹素下标**取单点高度（`height_texel(i)` = 第 i 格）。钳在图内，
/// 避免边缘越界采样导致的黑边。
fn height_texel(idx: vec2<f32>) -> f32 {
    let g = p.grid.xy;
    let c = clamp(idx, vec2<f32>(0.0), g - vec2<f32>(1.0));
    let uv = (c + vec2<f32>(0.5)) / g;
    return textureSampleLevel(height_tex, height_smp, uv, 0.0).r;
}

/// 按**格坐标（边界空间）**取高度，双线性。`height_at(i + 0.5)` = 第 i 格。
fn height_at(pos: vec2<f32>) -> f32 {
    let g = p.grid.xy;
    return textureSampleLevel(height_tex, height_smp, clamp(pos, vec2<f32>(0.0), g) / g, 0.0).r;
}

// ── T32/T33 缩小采样：手工 8 tap 盒式平均 ─────────────────────────────────
//
// **为什么必须手工做**：T4 规定零 mip 链。没有 mip，`px_per_cell < 1` 时
// `textureSampleLevel(.., 0.0)` 只在 1 个纹素邻域做双线性——那是**欠采样**
// 而不是平均，高频地形（碎岛、笔刷画的噪点）在远观档会闪。
//
// **为什么不能用硬件 mip**：`attr` 是 ID 纹理，mip 会把相邻群系平均成
// `biome 5.3` 这种不存在的编号（T2 的翻车点）。故只对**连续场**
// （height / coast）手工求面积平均，ID 一律保持最近邻。

/// 3×3 去中心 = 8 tap（T33 指定的 tap 数），覆盖 `footprint` 格见方。
fn box_avg_height(pos: vec2<f32>, footprint: f32) -> f32 {
    let h2 = footprint * 0.5;
    var sum = 0.0;
    for (var j = 0; j < 3; j = j + 1) {
        for (var i = 0; i < 3; i = i + 1) {
            if (i == 1 && j == 1) { continue; }
            sum = sum + height_at(pos + vec2<f32>(f32(i) - 1.0, f32(j) - 1.0) * h2);
        }
    }
    return sum / 8.0;
}

/// 同 [`box_avg_height`]，作用于海岸距离场。
fn box_avg_coast(pos: vec2<f32>, footprint: f32) -> f32 {
    let h2 = footprint * 0.5;
    var sum = 0.0;
    for (var j = 0; j < 3; j = j + 1) {
        for (var i = 0; i < 3; i = i + 1) {
            if (i == 1 && j == 1) { continue; }
            sum = sum + sample_coast_raw(pos + vec2<f32>(f32(i) - 1.0, f32(j) - 1.0) * h2);
        }
    }
    return sum / 8.0;
}

/// 海岸距离（T3）。Linear 采样保亚格连续 —— 整数截断场插值会出平顶带。
/// 海岸距离纹理的**原始归一化值**（0..1）。解码由调用方按编码模式做。
///
/// 之所以返回原始值而不直接返回格数：两种编码（`d/32` 与 `(sd+32)/64`）
/// 共用同一张纹理，解码方式必须跟 `pack::CoastEncoding` 一起切换。
/// 在这里解码就等于把模式判断藏进采样函数，容易出现「pack 换了、shader 没换」
/// 的静默错位。
///
/// 入参 `pos` 是**格坐标（边界空间）**，与其他采样函数同一套约定。
fn sample_coast_raw(pos: vec2<f32>) -> f32 {
    let g = p.grid.xy;
    let uv = clamp(pos, vec2<f32>(0.0), g) / g;
    return textureSampleLevel(coast_tex, coast_smp, uv, 0.0).r;
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