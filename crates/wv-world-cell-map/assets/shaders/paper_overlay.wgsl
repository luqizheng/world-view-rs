// 纸面与后处理（P4 · T45 纤维 + T46 暗角）
//
// 一次混合输出两层常色半透明。合并的依据是「连续 over 对背景是仿射的」——
// 推导与单测见 `field/paper.rs::composite_weights`。这里只做转写。
//
// 顺序（T48）：纤维在内、暗角在外 = 先纤维后暗角。

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: PaperParams;

struct PaperParams {
    view: vec4<f32>,      // xy = 视口世界尺寸, z = 纤维 alpha, w = 暗角 alpha
    tune: vec4<f32>,      // xy = 视口像素尺寸, z = 纤维频率, w = 暗角起始半径
    grain_rgb: vec4<f32>, // 纤维色（线性）
    vig_rgb: vec4<f32>,   // 暗角色（线性）
};

/// 二维哈希 → [0,1)。确定、无随机数（SPEC §3.8.6 守护项 8）。
fn hash21(q: vec2<f32>, seed: f32) -> f32 {
    var v = fract(q * vec2<f32>(127.1, 311.7) + vec2<f32>(seed, seed * 1.37));
    v = v + vec2<f32>(dot(v, v + 45.32));
    return fract(v.x * v.y * 95.4307);
}

/// 值噪声：整数格点哈希 + smooth 双线性。
fn vnoise(q: vec2<f32>, seed: f32) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i, seed);
    let b = hash21(i + vec2<f32>(1.0, 0.0), seed);
    let c = hash21(i + vec2<f32>(0.0, 1.0), seed);
    let d = hash21(i + vec2<f32>(1.0, 1.0), seed);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

/// T45：**屏幕空间** 2 octave fbm。
///
/// 频率锚屏幕空间（不是世界空间）：锚世界会让缩放时纤维变粗变细，
/// 像镜头在推拉纸面——纸是**静止介质**，不该有这种知觉。
fn grain_fbm(px: vec2<f32>) -> f32 {
    let q = px * p.tune.z; // 像素 → 噪声空间
    let n0 = vnoise(q, 3.7);
    let n1 = vnoise(q * 2.0, 11.3);
    // 2 octave：振幅 0.5 / 0.25，归一化到 [0,1]
    return (n0 * 0.5 + n1 * 0.25) / 0.75;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // ── T45 纤维 ──────────────────────────────────────────────────────────
    // 以中灰为中心的双向微扰：纤维既轻微提亮也轻微压暗，不是一层灰纱。
    let px = uv * p.tune.xy;
    let n = grain_fbm(px);
    let ag = p.view.z;

    // ── T46 暗角 ──────────────────────────────────────────────────────────
    // 归一化半径：中心 0、角上 1（`length` 到角是 √2/2，乘 √2 归一化）。
    let r = length(uv - vec2<f32>(0.5)) * 1.41421356;
    let t = clamp((r - p.tune.w) / max(1.0 - p.tune.w, 1e-5), 0.0, 1.0);
    let av = p.view.w * t * t * (3.0 - 2.0 * t);

    // ── 合并（推导见 paper.rs::composite_weights）─────────────────────────
    let a = ag + av * (1.0 - ag);
    if (a <= 1e-6) {
        discard;
    }
    let wg = ag * (1.0 - av) / a;
    let wv = av / a;
    // 纤维色的灰度按噪声在 [0,1] 内双向微扰：0.5 处为中性。
    let g = p.grain_rgb.rgb * (0.5 + n);
    let c = g * wg + p.vig_rgb.rgb * wv;
    return vec4<f32>(c, a);
}
