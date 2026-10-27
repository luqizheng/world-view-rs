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
    asset::uuid_handle,
    image::{Image, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat},
    shader::{Shader, ShaderRef},
    sprite_render::{Material2d, Material2dPlugin},
};

use super::line_sdf::LineDashPack;
use super::pack::{CellRect, CoastEncoding, FieldPack};

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

/// T25：河流墨线宽（屏幕像素，SPEC §3.8.1 口径）。线状要素比边界重。
pub const RIVER_LINE_PX: f32 = 1.8;
/// T26：道路主干墨线宽（屏幕像素）。路网是远观构图骨架。
pub const ROAD_MAJOR_PX: f32 = 2.4;
/// T26：道路次级墨线宽（屏幕像素）。
pub const ROAD_MINOR_PX: f32 = 1.2;
/// 河流墨线不透明度（水色偏深的青蓝墨——河在纸面地图上是「蓝墨」不是黑墨）。
pub const RIVER_INK_ALPHA: f32 = 0.62;
/// 道路墨线不透明度（比 biome 界重、比海岸轻）。
pub const ROAD_INK_ALPHA: f32 = 0.5;

/// T14：高度梯度中心差分步长（格）。
///
/// ⚠️ **死槽**：T13 起解析梯度接管梯度后，本值不再参与任何计算；ADR-0077
/// D2 起**连解析梯度本身也退役**（Horn 3×3 统一承担，GPU 侧
/// `horn_grad_detail` / CPU 侧 `crate::shading::horn_gradient`），本槽位保留
/// 只为 uniform 布局稳定（WGSL 侧同注记，见 `terrain_field.wgsl` 该函数文档）。
pub const GRAD_STEP: f32 = 0.5;

/// T19：域扭曲幅度下限（格）。近观编辑态——「点到哪就是哪」。
pub const WARP_AMP_MIN: f32 = 0.05;
/// T19：域扭曲幅度上限（格）。远观——明显的手绘起伏。
pub const WARP_AMP_MAX: f32 = 0.45;

/// T19 的幅度拟合常数：`amp = clamp(K / px_per_cell, 0.05, 0.45)`。
///
/// 标定依据是 T19 **注记里的两个端点**（不是它的字面公式，见下）：
/// - 远观 `px_per_cell ≈ 0.5`（1 px 约 2 格）→ `amp = 0.45`，故 `K = 0.225`；
/// - 近观编辑 `px_per_cell ≈ 4`（诊断基线的「近观 4 px/格」）→ `amp ≈ 0.056`，
///   取到下限 0.05，与注记「≈0.1 屏幕px，点到哪就是哪」一致。
///
/// ## ⚠️ 规格锁 T19 的字面公式与其注记**互相矛盾**
///
/// 锁表写 `amp = clamp(2.0 × scale, 0.05, 0.45)`，注记说「近观编辑
/// `scale ≈ 0.25` 取 0.05 格」。但把 `scale` 理解为「每屏幕像素多少格」
/// （= `1 / px_per_cell`，也是与「scale 大 = 远观」唯一自洽的读法）：
///
/// | px_per_cell | scale | `2.0 × scale` | 公式结果 | 注记要求 |
/// |---|---|---|---|---|
/// | 0.5（远观） | 2.0 | 4.0 | 0.45 ✓ | 0.45 |
/// | 4（近观） | **0.25** | **0.5** | **0.45 ✗** | **0.05** |
///
/// 即：按字面公式，近观编辑会拿到**最大**抖动，与「保证点选精度」的
/// 设计意图完全相反。故此处按**注记的两个端点**拟合，并在
/// `P0_SPEC_LOCK` 待补追记中登记该矛盾（不静默改锁定值）。
///
/// 若将来裁定回到字面公式，只需改本函数体，shader 与 uniform 布局不变。
pub const WARP_K: f32 = 0.225;

/// T19 幅度曲线：远观大、近观小，钳在 `[0.05, 0.45]` 格。
///
/// 输入是「每格多少屏幕像素」（越大越近）。
pub fn warp_amp(px_per_cell: f32) -> f32 {
    (WARP_K / px_per_cell.max(1e-3)).clamp(WARP_AMP_MIN, WARP_AMP_MAX)
}

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
    /// 河流墨色（T25）——青蓝墨，河在纸面地图上是「蓝墨」不是黑墨。
    /// 与主题水色同族但更深一档（落在纸上的河线要压得住底色）。
    pub river_ink: LinearRgba,
    /// 纸底色（地形没画到的地方也是纸不是黑，T44）。
    pub paper: LinearRgba,
}

impl Default for FieldPalette {
    /// 冻结的**地形场四色 + 墨色**（ADR-0054 D4：固定色是本 crate 的默认
    /// 数据源）。值 = wv-theme `topographic::TopographicPalette::default()`
    /// 逐值抄录（2026-10-23），与 `biome_raw` 的 Topographic 取色同色族——
    /// P1 与 CPU 回退路径观感一致，不一致就是回归。
    ///
    /// ⚠️ **色彩空间**：下面全部按 **sRGB** 书写（取色器口径），经
    /// `Color::srgb(..)` 显式转线性——uniform 住在线性空间，直塞会被
    /// 当线性值输出后再亮一次（整幅偏白，实测踩过）。
    fn default() -> Self {
        // 冻结表（sRGB，Draft 提议值 2026-10-19）：水 #3A7DB5 /
        // 低地 #F0E5C9（沙调暖底）/ 中地 #C9B58A（土褐过渡）/ 高地 #8A7A66（岩褐）。
        let srgb = |r: f32, g: f32, b: f32| LinearRgba::from(Color::srgb(r, g, b));
        Self {
            water: srgb(0.227, 0.490, 0.710),
            land_low: srgb(0.941, 0.898, 0.788),
            land_mid: srgb(0.788, 0.710, 0.541),
            land_high: srgb(0.541, 0.478, 0.400),
            // 墨色与纸色也按 sRGB 书写（人眼/取色器给的都是 sRGB 值）。
            ink: srgb(0.16, 0.14, 0.13),
            // T25 河墨：取水色加深 35%（sRGB 空间乘法≈感知线性），
            // 与水色同族——河是「水的线状呈现」，异色会像笔刷描上去的。
            river_ink: srgb(0.227 * 0.65, 0.490 * 0.65, 0.710 * 0.72),
            paper: srgb(0.94, 0.91, 0.84),
        }
    }
}

#[derive(AsBindGroup, Debug, Clone, Asset, TypePath)]
pub struct TerrainFieldMaterial {
    /// uniform 参数块。**公开**是刻意的：缩放档位、墨线强度、明暗系数等
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
    /// T25/T26（追记九）：折线距离场，`Rgba8Unorm` + `Linear`。
    /// R = 河流 / G = 道路主干 / B = 道路次级，编码 `d / 8`（无线处 = 1）。
    /// **`Linear`**：距离是连续量，与 coast 同理（Nearest 会出平顶带）。
    #[texture(7)]
    #[sampler(8)]
    pub line_dash: Handle<Image>,
    /// ADR-0072：biome 距离场 **R 通道**（d / 8），`Linear`——距离是连续量。
    #[texture(9)]
    #[sampler(10)]
    pub biome: Handle<Image>,
    /// ADR-0072：**同一份字节**的 `Nearest` 视图，专供 G/B（本格 / 最近异类
    /// 群系 ID）。ID 是离散编号，Linear 插值出「群系 5.5」是语义错误
    /// （同 attr 红线）；距离与 ID 必须各用各的采样器，故同字节绑两次。
    #[texture(11)]
    #[sampler(12)]
    pub biome_ids: Handle<Image>,
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
    /// 1 = Themed）、z = **米制纵向夸张**（= `crate::shading::Z_EXAG`，ADR-0077
    /// D1/D2——同物理坡跨图幅同法线的「纵向夸张旋钮」；旧格空间 0.55 已随
    /// T6 语义翻转退役）、w = `grad_step`（死槽，见 [`GRAD_STEP`]）。
    pub shape: Vec4,
    /// 预留（P2 域扭曲开关）。
    pub pad0: Vec4,
    /// **主光 L₁**：xyz = `crate::shading::LIGHT_MAIN`（方位 315°、高度角 45°
    /// 的真单位向量，直接 dot 无需归一化）；w = **米/格**——桥接按
    /// `crate::shading::meters_per_cell_of(cell_size, unit_per_km)` 填写
    /// （ADR-0077 D2 的米制换算唯一入口），`≤ 0` = 未接线 / 无比例尺，
    /// shader 侧回落 1.0。WGSL 注释与本字段互指。
    pub light_dir: Vec4,
    /// 预留（P2 水彩开关）。
    /// 预留槽。
    /// - `.x` — 未用
    /// - `.y` — **海岸纹理编码开关**：`0` = 无符号距离（T3 原值），
    ///   `1` = 有符号距离（[`CoastEncoding::Signed`]）。shader 必须与
    ///   `pack` 用同一约定解码，否则距离整体偏移一格。
    pub pad1: Vec4,
    /// 海岸三段累计边界（格）：x = 滩 / y = 浅 / z = 深；w = `wash_amt`（P2）。
    pub shore_edges: Vec4,
    /// **三光系数**（ADR-0077 D1）：x = `crate::shading::AMBIENT` /
    /// y = `DIFFUSE_MAIN` / z = `DIFFUSE_AUX`（和 = 1；单位光向量下明暗
    /// `I ∈ [0.4, 1.0]` 界内免费，旧 T17 明暗钳制对已退役）；w 预留。
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
    /// 河流墨色（T25）。
    pub river_ink: Vec4,
    /// 纸底色（T44）。
    pub paper: Vec4,
    /// 分层设色（ADR-0055）：8 阶海拔色带的 **7 条内边界 + 强度**。
    ///
    /// - 组 0 的 `.xyzw`、组 1 的 `.xyz` = 内边界（升序，**归一化**到
    ///   `R16Unorm` 高度纹理的域：`(h − H_MIN) / H_RANGE`，与 `pack` 同口径）；
    /// - 组 1 的 `.w` = 叠加强度 `0..1`（`0` = 纯群系色，`1` = 色带完全替代）。
    ///
    /// 只作用于 **CellMap / Topographic 分支的陆格**；Themed 分支与水面不读。
    pub elev_cuts: [Vec4; 2],
    /// 8 阶固定色带（**线性**空间，与 `theme` 同口径）。
    ///
    /// sRGB 真源 = [`crate::terrain_view::hypsometric::RAMP_SRGB`]（固定色，
    /// 不进 ThemePack——ADR-0051 §5 / ADR-0054 D2）。搬进 uniform 而不是烧进
    /// WGSL，是为了让 CPU 烘焙路径与 GPU 场路径共用**唯一一份**色表。
    pub elev_ramp: [Vec4; 8],
    /// 辅光方向 xyz（ADR-0077 双光源 L₂，= `crate::shading::LIGHT_AUX` 单位
    /// 向量）；w 预留。
    ///
    /// **追加在末尾是刻意的**：std140 布局下只有尾部追加才不扰动既有字段的
    /// 偏移——往中间插字段会把后面全部错位，且编译期不报错、运行期上传
    /// uniform 才炸（见本结构体顶部的布局警告）。WGSL 侧 `FieldParams`
    /// 同提交镜像追加。
    ///
    /// T6 起被 WGSL 双光源公式正式读取（T5 只是惰性预载，语义翻转日为 T6）。
    pub light_aux: Vec4,
}

/// 场 shader 的**静态句柄**（编译期内联，不经 AssetServer）。
///
/// ## 为什么不用 `ShaderRef::Path`（2026-10-24 实测踩坑）
/// 原实现返回 `"shaders/terrain_field.wgsl".into()`，那是**相对 asset 根**解析的：
/// - 在 `terrain_field_demo` 里 asset 根 = `crates/wv-world-cell-map/assets` ⇒ 能解析；
/// - 在主 App 里 asset 根 = `crates/wv-app/assets` ⇒ 报
///   `ERROR bevy_asset::server: Path not found: .../crates/wv-app/assets/shaders/terrain_field.wgsl`，
///   材质拿不到 shader ⇒ **地形整片不画**（且不 panic，只有一条 error 日志）。
///
/// 这与 `grid_lines.rs::GRID_LINES_SHADER` 是同一个教训、同一套接法：
/// 常驻渲染层的 shader 必须编译期内联，不能依赖「运行目录恰好有个 assets 文件夹」。
/// **UUID 一旦发布不得修改**（它是句柄身份）。
pub const TERRAIN_FIELD_SHADER: Handle<Shader> =
    uuid_handle!("6a1f4c02-7d15-4b93-9e42-c0b8a37f51dd");

impl Material2d for TerrainFieldMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(TERRAIN_FIELD_SHADER)
    }
}

/// 装配材质（把 [`FieldPack`] 的字节缓冲变成六张 GPU 纹理——biome 一份字节双采样器视图）。
///
/// 采样器按 P0 锁死：
/// - height / coast → `Linear`（T1/T3；shader 要在亚格处插值）
/// - attr → **Nearest**（T2；biome 是 ID，插值即语义错误）
/// - line_dash → `Linear`（追记九；距离是连续量，同 coast 理由）
/// - biome → `Linear`（R=距离）+ `Nearest` 视图（G,B=群系 ID，同 attr 红线）
///
/// 全部走 `Images` 手动注入（不经 `AssetServer`）——纹理来自 `FieldPack`
/// 的内存缓冲，不是磁盘资产；这也是 wv-render「禁文件 IO」红线的必然结果。
pub fn build_material(
    images: &mut Assets<Image>,
    pack: &FieldPack,
    params: FieldParams,
) -> TerrainFieldMaterial {
    build_material_with_lines(images, pack, None, params)
}

/// 同 [`build_material`]，可附加折线距离纹理（T25/T26）。
///
/// `lines` 为 `None` 时绑定一张全 1 纹理（解码后距离饱和，shader 侧
/// 自然不画任何线）——保证 binding 布局恒定，调用方无需两套材质。
pub fn build_material_with_lines(
    images: &mut Assets<Image>,
    pack: &FieldPack,
    lines: Option<&LineDashPack>,
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
    // T25/T26：折线距离场。空纹理解码后 d = 8 格，任何线宽阈值都画不出来。
    let dash = match lines {
        Some(l) => l.clone(),
        None => LineDashPack::empty(pack.width, pack.height),
    };
    let line_dash = images.add(data_texture(
        dash.width,
        dash.height,
        &dash.tex,
        TextureFormat::Rgba8Unorm,
        ImageSamplerDescriptor::linear(),
    ));
    // ADR-0072：biome 距离场。同一份字节建两张 Image（Linear 给 R=距离 /
    // Nearest 给 G,B=群系 ID）——多一份 GPU 上传是双采样器语义的代价，
    // 换来「距离亚格连续、ID 严格分片」两不误。
    let biome = images.add(data_texture(
        pack.width,
        pack.height,
        &pack.biome_tex,
        TextureFormat::Rgba8Unorm,
        ImageSamplerDescriptor::linear(),
    ));
    let biome_ids = images.add(data_texture(
        pack.width,
        pack.height,
        &pack.biome_tex,
        TextureFormat::Rgba8Unorm,
        ImageSamplerDescriptor::nearest(),
    ));
    TerrainFieldMaterial {
        params,
        height,
        coast,
        attr,
        line_dash,
        biome,
        biome_ids,
    }
}

/// **增量补丁**：把 `pack` 里 `rect` 覆盖的 texel 写进材质已有的数据纹理。
///
/// 返回 `false` = 不能增量（句柄没了 / 幅面不符 / 缓冲长度不符），调用方
/// **必须**改走 [`build_material`] 全量重建。返回 `true` 时 `rect` 之外
/// 的像素一个字节都没动。
///
/// ## 为什么必须在材质上原地改，而不是「重建材质再换」
///
/// 一笔 brush 走重建路径的代价是：整组 `Image` 全量分配 + 材质全量重建 +
/// 旧材质 `Assets::remove`（实测 1024² 共 48.4 ms）。而**真正会变的只有
/// brush 盖住的那几百个 texel**——剩下的 99.97% 是与上次逐字节相同的数据。
/// 原地补丁只付一次 `Assets::get_mut`（Bevy 会整张重传，见下「已知代价」）。
///
/// ## 三张必须同进同退
///
/// 先对**全部**目标做前置校验，通过了才动手写：半途失败会留下「高度是新的、
/// 海岸是旧的」这种错位画面，比不做补丁更难查。
///
/// ## 已知代价（不属于本函数的职责）
///
/// `Assets::<Image>::get_mut` 会把资产标脏，Bevy 的 `RenderAssetPlugin` 因此
/// **整张**重新抽取上传（Bevy 没有「局部纹理更新」的公开路径）。故本函数
/// 省掉的是 CPU 端的 EDT + 打包（48 ms 里的大头），不是 GPU 上传。
/// 要连上传一起省，得自研 `RenderAsset` 走 `write_texture` 子矩形——属下一轮。
pub fn patch_material_images(
    images: &mut Assets<Image>,
    material: &TerrainFieldMaterial,
    pack: &FieldPack,
    rect: CellRect,
) -> bool {
    // (句柄, 每 texel 字节数, 源缓冲)
    let targets = [
        (&material.height, 2usize, &pack.height_tex),
        (&material.coast, 2, &pack.coast_tex),
        (&material.attr, 4, &pack.attr_tex),
        (&material.biome, 4, &pack.biome_tex),
        (&material.biome_ids, 4, &pack.biome_tex),
    ];

    // ── 前置校验：三张全过才写 ──
    for (handle, _, src) in targets {
        let Some(image) = images.get(handle.id()) else {
            return false;
        };
        if image.width() != pack.width || image.height() != pack.height {
            return false;
        }
        // `RenderAssetUsages::default()` 含 `MAIN_WORLD`，`data` 必然存在；
        // 长度不符说明这张 `Image` 不是本 pack 建的，宁可全量重来。
        if image.data.as_ref().is_none_or(|d| d.len() != src.len()) {
            return false;
        }
    }

    let r = rect.intersect(pack.width, pack.height);
    if r.is_empty() {
        return true;
    }

    for (handle, bpp, src) in targets {
        let Some(mut image) = images.get_mut(handle.id()) else {
            return false;
        };
        let Some(dst) = image.data.as_mut() else {
            return false;
        };
        copy_rect_rows(dst, src, pack.width, bpp, r);
    }
    true
}

/// 把 `src` 中 `rect` 覆盖的字节按行搬到 `dst`（同一幅面、同一 `bpp`）。
///
/// 每行是一次 `copy_from_slice`：`R16` / `Rgba8` 都是紧密打包的行主序，
/// 行内矩形是连续段，行间跨 `tex_w × bpp` 的 stride。
fn copy_rect_rows(dst: &mut [u8], src: &[u8], tex_w: u32, bpp: usize, r: CellRect) {
    let stride = tex_w as usize * bpp;
    let run = r.width() as usize * bpp;
    for y in r.y0..r.y1 {
        let at = y as usize * stride + r.x0 as usize * bpp;
        dst[at..at + run].copy_from_slice(&src[at..at + run]);
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
/// 群系填色强度的中性默认（ADR-0072 T3）——demo / 测试路径用；
/// 主 App 的真源在 `ThemePack.biome_tint`。
pub const BIOME_TINT_DEFAULT: f32 = 0.5;

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
        // shape.z = 米制纵向夸张（ADR-0077 D1/D2，真源 = shading::Z_EXAG）；
        // shape.w = grad_step 死槽（布局稳定，见常量区注记）。
        shape: Vec4::new(
            px_per_cell,
            match branch {
                ViewBranch::BiomeRaw => 0.0,
                ViewBranch::Themed => 1.0,
            },
            crate::shading::Z_EXAG,
            GRAD_STEP,
        ),
        // T19 域扭曲幅度写进 pad0.x（P1 预留的槽位，P2 接线不改绑定布局）。
        // pad0.z = 群系填色强度（ADR-0072 T3）：中性默认 0.5，主 App 由
        // 桥接按 `ThemePack.biome_tint` 覆写。
        pad0: Vec4::new(warp_amp(px_per_cell), 0.0, BIOME_TINT_DEFAULT, 0.0),
        // 主光 L₁（ADR-0077 D1，真 45° 单位向量，直接 dot）；w = 米/格槽：
        // **0 = 桥接未接线 → shader 回落 1.0**（`meters_per_cell_of` 的
        // 接线在 wv-render 桥接侧——格距与比例尺是呈现态，本 crate 不知道）。
        light_dir: Vec4::new(
            crate::shading::LIGHT_MAIN[0],
            crate::shading::LIGHT_MAIN[1],
            crate::shading::LIGHT_MAIN[2],
            0.0,
        ),
        pad1: Vec4::ZERO,
        shore_edges: Vec4::new(SHORE_EDGE, SHALLOW_EDGE, DEEP_EDGE, 0.0), // w = wash_amt（P2）
        // 三光系数（ADR-0077 D1）：和 = 1，单位光向量下明暗界内免费——
        // 旧 T17 钳制对（RELIEF_LO/HI）随 T6 语义翻转整体退役。
        relief: Vec4::new(
            crate::shading::AMBIENT,
            crate::shading::DIFFUSE_MAIN,
            crate::shading::DIFFUSE_AUX,
            0.0,
        ),
        theme: [
            rgba4(pal.water),
            rgba4(pal.land_low),
            rgba4(pal.land_mid),
            rgba4(pal.land_high),
        ],
        ink: rgba4(pal.ink),
        river_ink: rgba4(pal.river_ink),
        paper: rgba4(pal.paper),
        // 分层设色默认关（强度 0）；内边界先按等分占位，真实值由
        // `apply_hypsometric` 在整图重打时按逐图百分位写入（ADR-0055）。
        elev_cuts: equal_cuts_uniform(),
        elev_ramp: ramp_uniform(),
        // 辅光 L₂（ADR-0077 L₂ = shading::LIGHT_AUX，真 45° 单位向量），
        // w 预留 0。与主光只差 x 符号——受光盲区互补（见 shading 模块文档）。
        light_aux: Vec4::new(
            crate::shading::LIGHT_AUX[0],
            crate::shading::LIGHT_AUX[1],
            crate::shading::LIGHT_AUX[2],
            0.0,
        ),
    }
}

/// 等分内边界占位（1/8 .. 7/8），只在「尚未写入真值」时存在。
fn equal_cuts_uniform() -> [Vec4; 2] {
    let c = |i: usize| (i + 1) as f32 / 8.0;
    [
        Vec4::new(c(0), c(1), c(2), c(3)),
        Vec4::new(c(4), c(5), c(6), 0.0),
    ]
}

/// 把 [`crate::terrain_view::hypsometric::RAMP_SRGB`] 转成 uniform 用的线性色。
fn ramp_uniform() -> [Vec4; 8] {
    std::array::from_fn(|i| {
        let c = crate::terrain_view::hypsometric::RAMP_SRGB[i];
        let lin = LinearRgba::from(Color::srgb(
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
        ));
        Vec4::new(lin.red, lin.green, lin.blue, 1.0)
    })
}

/// 写入分层设色的真值（ADR-0055）。
///
/// `cuts_norm` = [`crate::terrain_view::hypsometric::normalized_cuts`] 的产物
/// （7 条，升序，`R16Unorm` 域）；`strength` 会被钳到 `[0, 1]`——
/// 越界强度在 shader 里会让 `mix` 外插，出现不属于任何一阶的颜色。
pub fn apply_hypsometric(params: &mut FieldParams, cuts_norm: &[f32; 7], strength: f32) {
    params.elev_cuts = [
        Vec4::new(cuts_norm[0], cuts_norm[1], cuts_norm[2], cuts_norm[3]),
        Vec4::new(
            cuts_norm[4],
            cuts_norm[5],
            cuts_norm[6],
            strength.clamp(0.0, 1.0),
        ),
    ];
}

/// 按 [`CoastEncoding`] 设置 uniform 的解码开关。
///
/// 这是 `pack` 与 `shader` 之间的**唯一耦合点**：两者必须同时切换，
/// 否则距离场整体偏移一格（症状是海陆交界与色带都错开半格，但不报错）。
pub fn set_coast_encoding(params: &mut FieldParams, encoding: CoastEncoding) {
    params.pad1[1] = match encoding {
        CoastEncoding::Unsigned => 0.0,
        CoastEncoding::Signed => 1.0,
    };
}

/// 插件注册（材质是资源类型，必须显式注册才有资产管线）。
pub struct TerrainFieldMaterialPlugin;

impl Plugin for TerrainFieldMaterialPlugin {
    fn build(&self, app: &mut App) {
        // shader 走**编译期内联**（理由见 [`TERRAIN_FIELD_SHADER`]）。
        // 相对路径 = 相对本文件（`src/terrain_view/field/material.rs`）→ 三级上跳到 crate 根。
        bevy::asset::load_internal_asset!(
            app,
            TERRAIN_FIELD_SHADER,
            "../../../assets/shaders/terrain_field.wgsl",
            Shader::from_wgsl
        );
        app.add_plugins(Material2dPlugin::<TerrainFieldMaterial>::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T19 幅度曲线的两个端点必须与注记一致（见 [`WARP_K`] 的矛盾说明）。
    #[test]
    fn warp_amp_hits_both_documented_endpoints() {
        // 远观：1 px ≈ 2 格 → 上限 0.45
        assert!((warp_amp(0.5) - 0.45).abs() < 1e-4, "远观应取上限");
        // 近观编辑：诊断基线 4 px/格 → 下限 0.05
        assert!(
            warp_amp(4.0) < 0.06,
            "近观应贴近下限（≈0.056），实际 {}",
            warp_amp(4.0)
        );
        assert!(warp_amp(4.0) >= WARP_AMP_MIN);
        // 极端近观不得更大（单调不增）
        assert!(warp_amp(16.0) <= warp_amp(4.0) + 1e-6);
    }

    /// 幅度全区间恒在钳制范围内，且远离奇点不 panic。
    #[test]
    fn warp_amp_stays_within_lock_range() {
        for cpx in [0.01f32, 0.1, 0.5, 1.0, 3.0, 8.0, 32.0, 1000.0] {
            let a = warp_amp(cpx);
            assert!(
                (WARP_AMP_MIN..=WARP_AMP_MAX).contains(&a),
                "px_per_cell={cpx} → amp={a} 越界"
            );
            assert!(a.is_finite());
        }
        // 0 不得产生 inf/NaN
        assert!(warp_amp(0.0).is_finite());
    }

    /// 默认参数把 uniform 填满，且不含 NaN（NaN 会让整个 uniform 静默失效）。
    #[test]
    fn default_params_are_finite() {
        let p = default_params([512.0, 512.0], 6.0, ViewBranch::Themed, Default::default());
        let all = [
            p.grid,
            p.shape,
            p.pad0,
            p.light_dir,
            p.pad1,
            p.shore_edges,
            p.relief,
            p.ink,
            p.paper,
        ];
        for v in all {
            assert!(v.is_finite(), "uniform 出现非有限值: {v:?}");
        }
        for v in p.theme {
            assert!(v.is_finite());
        }
        for v in p.elev_cuts {
            assert!(v.is_finite(), "分层设色内边界出现非有限值: {v:?}");
        }
        for v in p.elev_ramp {
            assert!(v.is_finite(), "分层设色色带出现非有限值: {v:?}");
        }
        assert!(
            p.light_aux.is_finite(),
            "light_aux 出现非有限值: {:?}",
            p.light_aux
        );
    }

    /// T6 语义翻转锚：uniform 槽位从「旧光向 + 明暗钳制对 + 格空间夸张」切到
    /// ADR-0077「双光源单尺度 + 米制夸张」。WGSL 没有单测，这里是公式接线在
    /// Rust 侧的唯一定位点——任何一项漂移都说明槽位语义被静默改写（会让
    /// shader 误读、画面变）。
    #[test]
    fn default_params_fill_dual_light_and_metric_exaggeration_slots() {
        let p = default_params([512.0, 512.0], 6.0, ViewBranch::Themed, Default::default());
        // 主光槽：xyz = shading::LIGHT_MAIN（真 45° 单位向量，直接 dot）。
        for k in 0..3 {
            assert!(
                (p.light_dir[k] - crate::shading::LIGHT_MAIN[k]).abs() < 1e-6,
                "light_dir[{k}] = {} 应为 {}",
                p.light_dir[k],
                crate::shading::LIGHT_MAIN[k]
            );
        }
        // w = 米/格槽：默认 0 = 桥接未接线（shader 回落 1.0）；真值由
        // wv-render 桥接按 meters_per_cell_of 填。
        assert_eq!(
            p.light_dir.w, 0.0,
            "light_dir.w = 0 表示桥接未接线（回落 1.0）"
        );
        // 三光系数槽：relief = (AMBIENT, DIFFUSE_MAIN, DIFFUSE_AUX, 0)，
        // 和 = 1（界内性前提，shading 侧有锚点测试）。
        assert_eq!(
            p.relief,
            Vec4::new(
                crate::shading::AMBIENT,
                crate::shading::DIFFUSE_MAIN,
                crate::shading::DIFFUSE_AUX,
                0.0
            ),
            "relief 应为三光系数 {:?}",
            [
                crate::shading::AMBIENT,
                crate::shading::DIFFUSE_MAIN,
                crate::shading::DIFFUSE_AUX
            ]
        );
        // shape.z = 米制纵向夸张（shading::Z_EXAG = 2.0；旧格空间 0.55 退役）。
        assert_eq!(
            p.shape.z,
            crate::shading::Z_EXAG,
            "shape.z 应为米制夸张 {}",
            crate::shading::Z_EXAG
        );
        // 辅光槽（T5 预载、T6 起被公式读取）：xyz = shading::LIGHT_AUX，w 预留 0。
        for k in 0..3 {
            assert!(
                (p.light_aux[k] - crate::shading::LIGHT_AUX[k]).abs() < 1e-6,
                "light_aux[{k}] = {} 应为 {}",
                p.light_aux[k],
                crate::shading::LIGHT_AUX[k]
            );
        }
        assert_eq!(p.light_aux.w, 0.0, "light_aux.w 预留槽应填 0");
    }
}
