//! 纸面与后处理（P4 · T44–T48）。
//!
//! ## 这一层解决什么
//!
//! 地形场再怎么调，画出来仍是「一堆色块」；让它读起来像**一张纸上的地图**，
//! 靠的是三样介质感：远观向纸面退散的雾、纸纤维的微噪、四角的暗角。
//! 三者都不是「内容」，是**承载内容的介质**——所以它们必须压在所有地图内容
//! 之上（T48），包括未来的装饰 Sprite 与标注。
//!
//! ## 层序（T48，常量即断言）
//!
//! ```text
//! Z = 0    纸底          ← ClearColor（T44，地形没画到的地方也是纸不是黑）
//! Z = 0    地形场        ← TerrainFieldMaterial
//! Z = 8    雾霭          ← veil：纯纸色 + 缩放联动 alpha（T47）
//! Z = 9    纸面          ← grain + vignette：程序化，一次混合输出（T45/T46）
//! ```
//!
//! 雾单独一层是因为它是**纯色**（不需要 shader）；纤维与暗角都是程序化的，
//! 合在一个材质里减少一个 quad。两者在 shader 内按 T48 的顺序合成
//! （纤维在内、暗角在外），合成公式见 [`composite_weights`]。
//!
//! ## 为什么雾放在 Z=8、而不是和纸面合成一个材质
//!
//! 雾是**逐帧连续量**（随缩放变化），纸面是**静态量**。分开之后缩放时只改
//! 一个 `ColorMaterial` 的 alpha，不必重传纸面的 uniform。

use bevy::{
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, ColorMaterial, Material2d, Material2dPlugin},
};

// ── Z 序（T48）：常量即断言 ──────────────────────────────────────────────

/// 地形场的 Z（纸底在 ClearColor 里，不占实体）。
pub const TERRAIN_Z: f32 = 0.0;
/// 雾霭 Z（T47）。
pub const VEIL_Z: f32 = 8.0;
/// 纸面（纤维 + 暗角）Z（T45/T46）。
pub const PAPER_Z: f32 = 9.0;

// 编译期钉死层序：纤维/暗角必须在雾之上，雾必须在地形之上。
// 三者在同一 crate 同一模块里，改错会在这里报错，而不是等到出图才发现
// 「暗角被雾压住了」这种看一眼也未必认得出的问题。
const _: () = assert!(VEIL_Z > TERRAIN_Z);
const _: () = assert!(PAPER_Z > VEIL_Z);

// ── 数值（P0 锁）─────────────────────────────────────────────────────────

/// T44：纸底色（sRGB 分量）。
///
/// 与 demo 的 `ClearColor` 同源——「地形没画到的地方也是纸不是黑」。
pub const PAPER_SRGB: [f32; 3] = [0.94, 0.91, 0.84];

/// T46：暗角峰值不透明度。
pub const VIGNETTE_ALPHA: f32 = 0.06;
/// T46：暗角起衰减的归一化半径（角上为 1.0）。
///
/// 0.55 意味着中心 55% 半径内完全干净，只有靠边的部分才压暗——
/// 古舆图的边角压暗是「纸的厚度感」，不是镜头暗角，故不能从中心就开始。
pub const VIGNETTE_START: f32 = 0.55;

/// T45：纤维噪声的屏幕空间频率（每像素几个噪声周期）。
///
/// 0.22 时特征尺度约 4.5 px——是「纸纤维」而不是「电视雪花」。
/// 频率锚**屏幕空间**（T45 明写）：锚世界空间会让缩放时纤维变粗变细，
/// 像镜头在推拉纸面，破坏「纸是静止介质」的知觉。
pub const GRAIN_FREQ: f32 = 0.22;

/// T45：纤维色（sRGB）。以中灰为中心 —— 纤维既轻微提亮也轻微压暗，
/// 不是一层灰纱。与既有 `atmosphere::paper_grain_image` 的 112..=144 同源。
pub const GRAIN_SRGB: [f32; 3] = [0.5, 0.5, 0.5];

/// T46：暗角色（sRGB）。暖褐色而非纯黑 —— 纸上不会有纯黑的角。
pub const VIGNETTE_SRGB: [f32; 3] = [0.18, 0.15, 0.12];

// ── 合成数学（可单测的那部分）───────────────────────────────────────────

/// 把「纤维」与「暗角」两层常色半透明合成为**一次**混合所需的权重与总 alpha。
///
/// 为什么可以合并：连续两次 `over` 对背景是**仿射**的——
/// ```text
/// C1 = Cg·ag + B·(1−ag)
/// C2 = Cv·av + C1·(1−av)
///    = [Cg·ag·(1−av) + Cv·av] + B·(1−ag)(1−av)
/// ```
/// 与单次 `over`（`C·a + B·(1−a)`）同形，故：
/// ```text
/// a = ag + av·(1−ag)                    （= 1 − (1−ag)(1−av)）
/// wg = ag·(1−av) / a                    （纤维的归一化权重）
/// wv = av / a                           （暗角的归一化权重）
/// C = Cg·wg + Cv·wv
/// ```
/// 与背景色无关，所以对任意底图都精确等价——这正是「一个 quad 输出一次混合
/// 就能还原两次叠加」的依据。
///
/// 返回 `(w_grain, w_vignette, alpha)`。`alpha == 0` 时两个权重定义为 0
/// （避免除零；此时输出被完全透明覆盖，权重取什么都不可见）。
pub fn composite_weights(grain_alpha: f32, vignette_alpha: f32) -> (f32, f32, f32) {
    let ag = grain_alpha.clamp(0.0, 1.0);
    let av = vignette_alpha.clamp(0.0, 1.0);
    let a = ag + av * (1.0 - ag);
    if a <= 1e-6 {
        return (0.0, 0.0, 0.0);
    }
    (ag * (1.0 - av) / a, av / a, a)
}

/// T46：某点（归一化半径 `r`，角上为 1）的暗角 alpha。
pub fn vignette_alpha_at(r: f32) -> f32 {
    let t = ((r - VIGNETTE_START) / (1.0 - VIGNETTE_START)).clamp(0.0, 1.0);
    // smoothstep：两端导数为 0，暗角不会出现「一圈可见的起边」。
    VIGNETTE_ALPHA * t * t * (3.0 - 2.0 * t)
}

// ── 材质 ────────────────────────────────────────────────────────────────

/// 纸面材质的 uniform。布局纪律与 `TerrainFieldMaterial` 相同：
/// 一律 `Vec4`，因为 encase 会把 `[f32; 4]` 当作 `array<f32, 4>`（stride 4），
/// 与 WGSL `vec4<f32>`（stride 16）不符，运行期会 panic。
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct PaperParams {
    /// `x,y` = 视口世界尺寸；`z` = 纤维 alpha；`w` = 暗角 alpha。
    pub view: Vec4,
    /// `x,y` = 视口像素尺寸；`z` = 纤维频率；`w` = 暗角起始半径。
    pub tune: Vec4,
    /// 纤维色（线性 rgb，`a` 未用）。
    pub grain_rgb: Vec4,
    /// 暗角色（线性 rgb，`a` 未用）。
    pub vig_rgb: Vec4,
}

impl Default for PaperParams {
    fn default() -> Self {
        Self::new(Vec2::new(1600.0, 900.0), Vec2::new(1600.0, 900.0), 0.0)
    }
}

impl PaperParams {
    /// 按视口尺寸与当前雾 alpha 构造。
    ///
    /// `veil_alpha` 不在这里——雾是独立的一层（纯色 quad），见模块文档。
    pub fn new(view_world: Vec2, view_px: Vec2, vignette_alpha: f32) -> Self {
        let lin = |c: [f32; 3]| {
            let s = Color::srgb(c[0], c[1], c[2]).to_linear();
            Vec4::new(s.red, s.green, s.blue, 1.0)
        };
        Self {
            view: Vec4::new(
                view_world.x,
                view_world.y,
                crate::terrain_view::atmosphere::GRAIN_ALPHA,
                vignette_alpha,
            ),
            tune: Vec4::new(view_px.x, view_px.y, GRAIN_FREQ, VIGNETTE_START),
            grain_rgb: lin(GRAIN_SRGB),
            vig_rgb: lin(VIGNETTE_SRGB),
        }
    }
}

/// 纸面材质：纤维（T45）+ 暗角（T46），一次混合输出。
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PaperOverlayMaterial {
    #[uniform(0)]
    pub params: PaperParams,
}

impl Material2d for PaperOverlayMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/paper_overlay.wgsl".into()
    }
    /// 纸面是叠加层：必须走透明管线，且不能写深度。
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// 注册纸面材质。
pub struct PaperOverlayPlugin;

impl Plugin for PaperOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<PaperOverlayMaterial>::default());
    }
}

/// 构造纸面材质。
pub fn build_paper_material(
    materials: &mut Assets<PaperOverlayMaterial>,
    params: PaperParams,
) -> Handle<PaperOverlayMaterial> {
    materials.add(PaperOverlayMaterial { params })
}

/// 构造雾霭材质（T47）：纯纸色，alpha 由调用方按缩放逐帧改写。
///
/// 用 `ColorMaterial` 而不是自定义材质——雾没有程序化内容，只是一层纯色，
/// 再造一个 `Material2d` 只会多一个管线。
pub fn build_veil_material(
    materials: &mut Assets<ColorMaterial>,
    veil_alpha: f32,
) -> Handle<ColorMaterial> {
    let c = Color::srgb(PAPER_SRGB[0], PAPER_SRGB[1], PAPER_SRGB[2]).with_alpha(veil_alpha);
    materials.add(ColorMaterial::from_color(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成公式的两种写法必须一致：解析式 vs 逐层算。
    #[test]
    fn composite_weights_match_sequential_over() {
        for &ag in &[0.0f32, 0.035, 0.2, 0.5] {
            for &av in &[0.0f32, 0.06, 0.3, 0.7] {
                let (wg, wv, a) = composite_weights(ag, av);
                // 逐层推：C2 = Cg·ag·(1−av) + Cv·av + B·(1−ag)(1−av)
                let expect_a = 1.0 - (1.0 - ag) * (1.0 - av);
                assert!((a - expect_a).abs() < 1e-6, "总 alpha 不符");
                if a > 1e-6 {
                    // 权重和为 1（纯色合成的必要条件）
                    assert!((wg + wv - 1.0).abs() < 1e-6, "权重和应为 1：{wg} + {wv}");
                }
            }
        }
    }

    /// 两层都透明时不得输出「有颜色但不透明」的东西（否则整幅图会被糊成灰）。
    #[test]
    fn zero_alphas_yield_zero_alpha() {
        let (wg, wv, a) = composite_weights(0.0, 0.0);
        assert_eq!((wg, wv, a), (0.0, 0.0, 0.0));
    }

    /// 单层时权重应完全归它——退化情形不能引入对方的颜色。
    #[test]
    fn single_layer_owns_all_weight() {
        let (wg, wv, a) = composite_weights(0.035, 0.0);
        assert!((wg - 1.0).abs() < 1e-6 && wv.abs() < 1e-6);
        assert!((a - 0.035).abs() < 1e-6);

        let (wg, wv, a) = composite_weights(0.0, 0.06);
        assert!(wg.abs() < 1e-6 && (wv - 1.0).abs() < 1e-6);
        assert!((a - 0.06).abs() < 1e-6);
    }

    /// 暗角：中心干净、角上取峰值、单调不减。
    #[test]
    fn vignette_is_clean_at_center_and_peaks_at_corner() {
        assert_eq!(vignette_alpha_at(0.0), 0.0);
        assert_eq!(vignette_alpha_at(VIGNETTE_START), 0.0);
        let corner = vignette_alpha_at(1.0);
        assert!((corner - VIGNETTE_ALPHA).abs() < 1e-6, "角上应取峰值");
        // 单调不减（暗角不能有第二个亮点）
        let mut prev = 0.0;
        for i in 0..=100 {
            let v = vignette_alpha_at(i as f32 / 100.0);
            assert!(v >= prev - 1e-9, "r={i} 处出现回落");
            prev = v;
        }
    }

    // Z 序（T48）不在这里测：模块顶部的 `const _: () = assert!(..)` 已经在
    // **编译期**钉死 `TERRAIN_Z < VEIL_Z < PAPER_Z`，运行期再断言一次是重复的
    // （且会被 clippy 判为 `assertions_on_constants`）。

    /// uniform 不得含 NaN——NaN 会让整块 uniform 静默失效。
    #[test]
    fn params_are_finite() {
        let p = PaperParams::new(Vec2::new(1600.0, 900.0), Vec2::new(1600.0, 900.0), 0.06);
        for v in [p.view, p.tune, p.grain_rgb, p.vig_rgb] {
            assert!(v.is_finite(), "uniform 出现非有限值: {v:?}");
        }
    }
}
