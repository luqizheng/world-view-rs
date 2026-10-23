//! `TerrainFieldMaterial` —— 地形场的 GPU 逐像素执行器（ADR-0053 D1，P1）。
//!
//! ## 这是什么
//!
//! 现行底图是 CPU 整图烘焙的 RGBA8 纹理，分辨率被 `tile_res_for()` 钉死
//! （1024 图 = 4 px/格），近观放大即见像素块、远观每格不足两像素。
//! 本材质把「颜色怎么来」整条决策搬到 fragment shader：
//!
//! ```text
//! CPU（[`FieldPack`]）              GPU（fragment，逐像素）
//! ─────────────────────              ─────────────────────────────
//! height  (R16Unorm, Linear)  ──┐
//! coast   (R16Unorm, Linear)  ──┼──▶  Catmull-Rom 16 tap ─▶ hillshade
//! attr    (Rgba8Unorm, Nearest)──┘         │
//!                                          ├─▶ 海岸三段色带（距离场驱动）
//!                                          └─▶ 墨线（屏幕像素宽 + fwidth AA）
//! ```
//!
//! ## 为什么 shader 只做「求值」不做「决策」
//!
//! SeaRecipe / DecorRecipe / PaletteColors 等**数值画法参数**原样搬进
//! uniform（ADR-0053 D5/T35：已定值不推翻，只换求值位置）。本文件只负责
//! 「在哪个像素求值」，不重推任何颜色公式——换主题观感必须与现行逐位等价。
//!
//! ## 分支：CellMap 与 Browse 各看什么（ADR-0051 内容矩阵的交互）
//!
//! mcm 分支的 [ADR-0051] 定了 CellMap（Topographic）地形画法 = 群系**固定色块**
//! `BIOME_RAW_COLORS[21]`（不随 ThemePack——「编辑器看原始数据，主题色只属于
//! 看世界」）。本材质的 [`ViewBranch`] 保留该语义：
//!
//! | 分支 | 取色 | 依据 |
//! |---|---|---|
//! | `BiomeRaw` | 群系固定色块（编译期常量表进 uniform） | ADR-0051 §5 |
//! | `Themed` | 主题色板（Topographic 四阶 / Browse 走 SeaRecipe） | ADR-0053 D7 |
//!
//! **分叉只发生在取色一步**，hillshade / 海岸色带 / 墨线三者在两分支共用。
//!
//! ## P1 范围（严格不越界到 P2）
//!
//! 本文件落地 **T1–T3 + T7–T18 + T23/T24/T27**：
//! 高度双三次插值、hillshade、海岸三段色带、岸线与 biome 墨线。
//!
//! **P2 才做的**（此处刻意不实现，避免「看着能跑就顺手做了」的隐性扩项）：
//! - T19–T22 域扭曲（手绘抖动）
//! - T28–T30 水彩颗粒
//! - T31–T34 缩放自适应三档（近观/过渡/远观）
//!
//! 它们的开关位与参数位**已在 uniform 里预留**（见 [`TerrainFieldMaterial`]），
//! P2 接线时无需改绑定布局——只改 WGSL 函数体。
//!
//! [ADR-0051]: https://…ADR-0051-mode-content-matrix
//! [`FieldPack`]: super::pack::FieldPack

// Bevy 0.19 的真实导出路径（逐条核过 bevy_render-0.19.1 / bevy_image-0.19.1
// 源码，不凭记忆——ADR-0053 R1 风险「WGSL/派生宏路径随版本变」的处置）：
// - `TextureFormat` / `TextureDimension` / `Extent3d` 在 `render_resource`
//   （`Extent3d` 经 `bevy::image` 重导出）。
// - `ShaderType` derive 宏在 `render_resource::encase`（trait 本身在
//   `render_resource` 顶层）。
// - `ShaderRef` 在 `bevy::shader`。
// - 图像类型名就是 `bevy::image::Image`（**没有** `BevyImage` 这个别名）。
use bevy::{
    image::{Image, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::ShaderRef,
    sprite_render::{Material2d, Material2dPlugin},
};

use super::pack::FieldPack;

// ── P0 规格锁常量（T7–T18 / T23 / T24 / T27）────────────────────────────────

/// T7：滩涂带累计边界（格）。`smoothstep(0, T7, d)`。
pub const SHORE_EDGE: f32 = 1.0;
/// T8：浅水带累计边界（格）。`smoothstep(T7, T8, d)`。
pub const SHALLOW_EDGE: f32 = 3.0;
/// T9：深水带累计边界（格）。`smoothstep(T8, T9, d)`。
pub const DEEP_EDGE: f32 = 9.0;

/// T11：岸线墨线宽（**屏幕像素**，SPEC §3.8.1 口径）。
pub const COAST_LINE_PX: f32 = 1.2;
/// T24：biome 边界线宽（屏幕像素）。比海岸浅一档——地貌交界是「暗示」不是「划线」。
pub const BIOME_LINE_PX: f32 = 0.8;

/// T14：高度梯度中心差分步长（格）。
pub const GRAD_STEP: f32 = 0.5;
/// T15：高程夸张系数。不夸张则 2000m 山只占 0.25 格、hillshade 读不出来。
pub const Z_EXAG: f32 = 0.55;
/// T17：明暗乘子下界（防陡崖压黑）。
pub const RELIEF_LO: f32 = 0.86;
/// T17：明暗乘子上界（防陡崖过曝）。
pub const RELIEF_HI: f32 = 1.14;

/// T16：光源方向（**左上 45°**，与全部地物符号/阴影同向）。
///
/// 归一化的 `(-1, 1, 1)/√3`。z 分量 = `1` 表示「光从观察者方向偏上打过来」
/// ——纯 2D hillshade 的 z=0 会让所有法线与光垂直、整幅图恒定亮度。
pub const LIGHT_DIR: [f32; 3] = [
    -std::f32::consts::FRAC_1_SQRT_2,
    std::f32::consts::FRAC_1_SQRT_2,
    std::f32::consts::FRAC_1_SQRT_2,
];

/// P2 预留：域扭曲幅度上限（T19：`amp = clamp(2.0×scale, 0.05, 0.45)`）。
///
/// P1 **不接线**——保留常量是为了让 P2 的数值有单一权威源，
/// 不必回到规格锁表里抄第二遍。
#[allow(dead_code)]
pub const WARP_AMP_MIN: f32 = 0.05;
#[allow(dead_code)]
pub const WARP_AMP_MAX: f32 = 0.45;

// 注：完整的群系固定色板是 **21 槽**（冻结群系表编号，ADR-0051 §5
// `BIOME_RAW_COLORS[21]`）。P1 **不**把它搬进 uniform——21 个 vec4 = 336 字节，
// 会改 uniform 布局且此刻还用不上。WGSL 侧先用 5 档按族近似色板，
// 保证「CellMap 的颜色与主题无关」这一语义成立；P2 随域扭曲一起接真表。

// ── 材质 ────────────────────────────────────────────────────────────────────

/// 取色分支（ADR-0051 内容矩阵 × ADR-0053 着色器 的交互点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewBranch {
    /// CellMap / Topographic：群系固定色块，**不随主题**。
    BiomeRaw,
    /// Browse / Edit：主题色板。
    Themed,
}

/// P1 的主题侧输入（数值来自现行 CPU 路径，已在 wv-theme 定过值）。
///
/// **只搬运，不重推公式**（T35 / ADR-0053 D5）——「shader 只管在哪个像素求值，
/// 不管求什么」。
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct FieldPalette {
    /// Topographic 四阶：浅水。
    pub water: LinearRgba,
    /// 低地。
    pub land_low: LinearRgba,
    /// 中地。
    pub land_mid: LinearRgba,
    /// 高地。
    pub land_high: LinearRgba,
    /// 岸线 / biome 界墨色。墨是纸上的固定媒介色，**不随主题色板**。
    pub ink: LinearRgba,
    /// 纸底色（地形没画到的地方也是纸不是黑，T44）。
    pub paper: LinearRgba,
}

impl Default for FieldPalette {
    /// 默认取 `wv-theme` 的 `topographic::TopographicPalette` 四色 + 墨色。
    ///
    /// 与 `topographic::pick` 同色族，**保证 P1 与现行 CPU 路径观感一致**——
    /// 不一致就是回归。
    ///
    /// ⚠️ **色彩空间**：`TopographicPalette` 给的是 **sRGB** 分量
    /// （`Srgb3`，注释里写作 `#3A7DB5` 这类十六进制）。材质 uniform 住在
    /// **线性**空间，直接塞进 `LinearRgba::rgb` 会被当成线性值，
    /// 输出到 sRGB 帧缓冲时再亮一次——整幅图明显偏白偏淡（实测踩过）。
    /// 必须走 `Color::srgb(..)` 做显式转换。
    fn default() -> Self {
        let t = wv_theme::topographic::TopographicPalette::default();
        // sRGB → 线性：`Color::srgb` 构造 sRGB 色，转 `LinearRgba` 时做 gamma 解码。
        let srgb =
            |c: wv_theme::topographic::Srgb3| LinearRgba::from(Color::srgb(c[0], c[1], c[2]));
        Self {
            water: srgb(t.water),
            land_low: srgb(t.land_low),
            land_mid: srgb(t.land_mid),
            land_high: srgb(t.land_high),
            // 墨色与纸色也按 sRGB 书写（人眼/取色器给的都是 sRGB 值），
            // 同一路径转线性——避免两种口径混用。
            ink: LinearRgba::from(Color::srgb(0.16, 0.14, 0.13)),
            paper: LinearRgba::from(Color::srgb(0.94, 0.91, 0.84)),
        }
    }
}

/// 地形场材质：数据纹理 + uniform 参数，唯一 CPU→GPU 通道。
///
/// 绑定布局（WGSL 侧同步）：
/// - `binding 0` = uniform（参数）
/// - `binding 1/3/5` = 三张数据纹理
/// - `binding 2/4/6` = 对应 sampler
#[derive(AsBindGroup, Debug, Clone, Asset, TypePath)]
pub struct TerrainFieldMaterial {
    /// uniform 参数块。**公开**是刻意的：缩放档位、墨线强度、明暗钳制等
    /// 旋钮由调用方（demo / 未来笔刷路径 / 调试面板）按需改写，
    /// 不必为此在每次改动后重建整张材质资产。
    #[uniform(0)]
    pub params: FieldParams,
    /// T1：height，`Linear`。
    #[texture(1)]
    #[sampler(2)]
    pub height: Handle<Image>,
    /// T3：coast_dist，`Linear`。
    #[texture(3)]
    #[sampler(4)]
    pub coast: Handle<Image>,
    /// T2：attr，**`Nearest`**（biome 是 ID，线性插值出 `biome 5` 是语义错误）。
    #[texture(5)]
    #[sampler(6)]
    pub attr: Handle<Image>,
}

/// 与 WGSL `FieldParams` 逐字段对应的 uniform 块。
///
/// ## 布局对齐（踩过硬的坑，务必按此写）
///
/// **每组标量必须用 `Vec4`，不能用 `[f32; 4]`。**
///
/// encase 把 Rust 的 `[f32; 4]` 当作 **`array<f32, 4>`**（元素 stride = 4），
/// 而不是 `vec4<f32>`（stride = 16）。WGSL 的 uniform 地址空间要求数组元素
/// stride 是 16 的倍数，于是运行期直接 panic：
/// `array stride must be a multiple of 16 (current stride: 4)`
/// （见 `encase-0.12.1/src/types/array.rs` 的 `UNIFORM_COMPAT_ASSERT`）。
///
/// 这个错误**编译期完全看不出来**——`cargo check` 过、clippy 过、单测过，
/// 只有真跑起来、`prepare_assets` 上传 uniform 时才炸。故本结构体的每个
/// 字段一律 `Vec4`，与 WGSL 的 `vec4<f32>` 严格一一对应。
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct FieldParams {
    /// xy = 格宽 / 格高；zw 预留。
    pub grid: Vec4,
    /// x = `px_per_cell`（墨线折算基准）、y = 取色分支（0 = BiomeRaw /
    /// 1 = Themed）、z = T15 `z_exag`、w = T14 `grad_step`。
    pub shape: Vec4,
    /// 预留（P2 域扭曲开关）。
    pub pad0: Vec4,
    /// T16 光源方向 xyz；w 预留。
    pub light_dir: Vec4,
    /// 预留（P2 水彩开关）。
    pub pad1: Vec4,
    /// 海岸三段累计边界（格）：x = 滩 / y = 浅 / z = 深；w = `wash_amt`（P2）。
    pub shore_edges: Vec4,
    /// xy = T17 明暗钳制；z = `lod_blend`（P2）；w 预留。
    pub relief: Vec4,
    /// 主题四阶色：water / land_low / land_mid / land_high。
    ///
    /// **必须是 `Vec4` 而不是 `[[f32; 4]; 4]`**：encase 对嵌套数组
    /// `[[f32; 4]; N]` 推不出内层是 `vec4`，会按 `f32` 的 stride=4 布局，
    /// 而 WGSL 的 `array<vec4<f32>, N>` 要求 stride=16——
    /// 运行时 panic「array stride must be a multiple of 16 (current stride: 4)」。
    /// 这个错误**编译期完全看不出来**，只有真正跑起来上传 uniform 才炸。
    pub theme: [Vec4; 4],
    /// 墨色。
    pub ink: Vec4,
    /// 纸底色（T44）。
    pub paper: Vec4,
}

impl Material2d for TerrainFieldMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_field.wgsl".into()
    }
}

/// 装配材质（把 [`FieldPack`] 的字节缓冲变成三张 GPU 纹理）。
///
/// 采样器按 P0 锁死：
/// - height / coast → `Linear`（T1/T3；shader 要在亚格处插值）
/// - attr → **Nearest**（T2；biome 是 ID，插值即语义错误）
///
/// 全部走 `Images` 手动注入（不经 `AssetServer`）——纹理来自 `FieldPack`
/// 的内存缓冲，不是磁盘资产；这也是 wv-render「禁文件 IO」红线的必然结果。
pub fn build_material(
    images: &mut Assets<Image>,
    pack: &FieldPack,
    params: FieldParams,
) -> TerrainFieldMaterial {
    let height = images.add(data_texture(
        pack.width,
        pack.height,
        &pack.height_tex,
        TextureFormat::R16Unorm,
        ImageSamplerDescriptor::linear(),
    ));
    let coast = images.add(data_texture(
        pack.width,
        pack.height,
        &pack.coast_tex,
        TextureFormat::R16Unorm,
        ImageSamplerDescriptor::linear(),
    ));
    let attr = images.add(data_texture(
        pack.width,
        pack.height,
        &pack.attr_tex,
        TextureFormat::Rgba8Unorm,
        // T2 红线：Nearest。biome 是 ID，Linear 会插出不存在的编号。
        ImageSamplerDescriptor::nearest(),
    ));
    TerrainFieldMaterial {
        params,
        height,
        coast,
        attr,
    }
}

/// 把裸字节缓冲包成 Bevy `Image`。
fn data_texture(
    width: u32,
    height: u32,
    bytes: &[u8],
    format: TextureFormat,
    sampler: ImageSamplerDescriptor,
) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        format,
        bevy::asset::RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

/// 把 [`FieldParams`] 按 T13–T18 的默认值填好（`FieldPalette` + 分支）。
///
/// 让 demo 与未来笔刷路径有单一构造入口——避免「每处各填一半参数，
/// 结果 P2 接线时发现某个旋钮没人接」。
pub fn default_params(
    grid: [f32; 2],
    px_per_cell: f32,
    branch: ViewBranch,
    pal: FieldPalette,
) -> FieldParams {
    let rgba4 = |c: LinearRgba| Vec4::new(c.red, c.green, c.blue, c.alpha);
    FieldParams {
        // 全部 `Vec4`——`[f32; 4]` 会被 encase 当 `array<f32, 4>`，
        // 运行期 panic「array stride must be a multiple of 16」。
        grid: Vec4::new(grid[0], grid[1], 0.0, 0.0),
        shape: Vec4::new(
            px_per_cell,
            match branch {
                ViewBranch::BiomeRaw => 0.0,
                ViewBranch::Themed => 1.0,
            },
            Z_EXAG,
            GRAD_STEP,
        ),
        pad0: Vec4::ZERO,
        light_dir: Vec4::new(LIGHT_DIR[0], LIGHT_DIR[1], LIGHT_DIR[2], 0.0),
        pad1: Vec4::ZERO,
        shore_edges: Vec4::new(SHORE_EDGE, SHALLOW_EDGE, DEEP_EDGE, 0.0), // w = wash_amt（P2）
        relief: Vec4::new(RELIEF_LO, RELIEF_HI, 0.0, 0.0),                // z = lod_blend（P2）
        theme: [
            rgba4(pal.water),
            rgba4(pal.land_low),
            rgba4(pal.land_mid),
            rgba4(pal.land_high),
        ],
        ink: rgba4(pal.ink),
        paper: rgba4(pal.paper),
    }
}

/// 插件注册（材质是资源类型，必须显式注册才有资产管线）。
pub struct TerrainFieldMaterialPlugin;

impl Plugin for TerrainFieldMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<TerrainFieldMaterial>::default());
    }
}
