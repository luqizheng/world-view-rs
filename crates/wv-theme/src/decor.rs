//! 地形装饰的画法配方（DecorRecipe）——决定「符号长什么样、多密、多大」的
//! 数值参数（ADR-0048 追记二）。
//!
//! ## 为什么在这里
//!
//! ADR-0039 D1 把 cell 驱动的覆盖物变体做成了 `wv-render::terrain_decor` 里的
//! **Rust 常量表**，并写明「签名稳定后随 ADR-0023 M3 迁 ThemePack」——那次
//! 迁移一直挂账（README §12-30）。后果是**换主题时陆地视觉纹丝不动**：
//! 水墨包想用皴笔而不是三角山、想把山画得更疏，都做不到，只能改渲染层源码
//! ——违反 ADR-0043「加新风格 = 加新 crate、零改渲染层源码」。
//!
//! 本模块补上那次迁移：**数值配方住主题层，执行器留在渲染层**——与
//! [`crate::sea::SeaRecipe`]（海）、[`crate::overlays::FeatureRule`]（Place
//! 覆盖物）、[`crate::overlays::RegionOverlayRule`]（Region 覆盖物）同构。
//!
//! ## 边界：什么**不**进配方
//!
//! - **biome → 符号族的分类映射**（`decor_kind_for_biome`）：那是「这个群系
//!   属于哪一族」的**词表**，与 `wv_terrain::biome` 同源，属事实层；主题该管
//!   「长什么样」，不该管「是什么」。
//! - **符号的几何形状**（三角 / 圆 / 半圆 / 弧的构造）：那是渲染实现，不是
//!   美术参数。
//! - **视口裁剪 / 桶合批 / 确定性哈希**：渲染机制。

/// 一类符号的墨色 + 不透明度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorInk {
    /// sRGB 字节。
    pub color: [u8; 3],
    /// 不透明度（0..=1）。
    pub alpha: f32,
}

impl DecorInk {
    /// 构造（便于常量表书写）。
    pub const fn new(color: [u8; 3], alpha: f32) -> Self {
        Self { color, alpha }
    }
}

/// 气候带变体的墨色（RGBA，0..=1 浮点）。
///
/// 变体色是**顶点色**（ADR-0039 追记：顶点色承载全部颜色 → 单一白材质），
/// 故用浮点 RGBA 而非字节。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VariantInk {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl VariantInk {
    /// 构造。
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// → `[f32; 4]`（顶点色数组）。
    pub const fn to_rgba(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

/// 一档 LOD 的位图树参数（屏幕像素）。
///
/// **为什么按档给参数**：树的「多大 / 多密」在远景与近景的合理值差一个量级
/// （远景一棵树要占更大画面、少很多棵），而档位阈值属渲染机制（`decor_band`），
/// 但**每档的具体数值属美术参数**——故数值住主题、档位选择住渲染层。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForestSymbolBand {
    /// 采样步距（屏幕像素）：相邻两棵候选树的名义间距。
    pub spacing_px: f32,
    /// 符号边长（屏幕像素）：单棵树在画面上的高度。
    pub size_px: f32,
}

/// 地形森林的**位图符号**规格（设计文档 §七 Sprite 形态，2026-10-22）。
///
/// ## 形态二选一
///
/// 主题给了本规格 → 地形驱动的森林画**手绘树**（位图精灵）；
/// 没给（`None`）→ 走**程序化符号**（ADR-0031 D5 的默认形态：林冠三角点）。
/// 二者不叠加——同时画会得到「树上长树点」的脏画面。
///
/// ## 分工（沿用 ADR-0031 追记一）
///
/// - **位置** = 地形事实：森林群系 + 密度场（`wv_terrain::forest`）；
/// - **疏密** = 主题参数：`DecorRecipe::forest_density_gain` × 密度场；
/// - **形态** = 本规格（贴图 / 变体 / 大小 / 步距 / 分级 tint）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForestSymbolSpec {
    /// 贴图键（主题资产根下的相对路径；**横向图集**，变体自左向右）。
    pub texture: &'static str,
    /// 图集变体数（横向格数）。
    pub variants: u32,
    /// 图集单格**宽高比**（宽 ÷ 高）。
    ///
    /// **为什么需要**：委托设计的素材格不是正方形（单株 192×260），
    /// 用正方形 quad 会把树拉扁。`size_px` 记的是**高度**，宽度 = 高度 × 本值。
    pub cell_aspect: f32,
    /// 符号不透明度（0..=1）。
    pub alpha: f32,
    /// Edit 档参数（最近）。
    pub band_edit: ForestSymbolBand,
    /// Region 档参数。
    pub band_region: ForestSymbolBand,
    /// Nation 档参数（最远）。
    pub band_nation: ForestSymbolBand,
    /// 疏林 tint（乘法）：林冠略提亮。
    pub tint_sparse: [u8; 3],
    /// 常规林 tint（乘法）：原色。
    pub tint_normal: [u8; 3],
    /// 密林 tint（乘法）：林冠压深（与林底蒙版同向，密林整体更沉）。
    pub tint_dense: [u8; 3],
}

impl ForestSymbolSpec {
    /// 构造（便于常量表书写）。
    pub const fn new(
        texture: &'static str,
        variants: u32,
        cell_aspect: f32,
        alpha: f32,
        band_edit: ForestSymbolBand,
        band_region: ForestSymbolBand,
        band_nation: ForestSymbolBand,
        tint_sparse: [u8; 3],
        tint_normal: [u8; 3],
        tint_dense: [u8; 3],
    ) -> Self {
        Self {
            texture,
            variants,
            cell_aspect,
            alpha,
            band_edit,
            band_region,
            band_nation,
            tint_sparse,
            tint_normal,
            tint_dense,
        }
    }
}

/// 地形装饰的画法配方。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorRecipe {
    // ── 高度阈值（米）────────────────────────────────────────────
    /// 山地阈值：格高 ≥ 此值且无更高邻格时画山符号。
    pub mountain_min_h: i16,
    /// 丘陵阈值：格高 ≥ 此值画丘陵符号。
    pub hill_min_h: i16,
    /// 雪帽阈值：山符号顶端 ≥ 此值叠雪帽。
    pub snow_cap_h: i16,
    /// 露岩阈值：山符号 ≥ 此值时雪帽内缘叠露岩。
    pub rock_h: i16,
    /// 脊点吸附阈值：格高 < 此值时才在步距块内扫峰找山。
    pub ridge_snap_min_h: i16,

    // ── 气候带（归一化 y，0 = 北）─────────────────────────────────
    /// 气候带切点：`< cut[0]` = 寒带（针叶）、`[cut[0], cut[1])` = 温带
    /// （阔叶）、`≥ cut[1]` = 干旱带（稀树）。
    pub climate_cuts: [f32; 2],

    // ── 变体墨色（顶点色）────────────────────────────────────────
    /// 寒带针叶色。
    pub conifer_ink: VariantInk,
    /// 温带阔叶色。
    pub temperate_ink: VariantInk,
    /// 干旱带稀树色。
    pub oasis_ink: VariantInk,
    /// 露岩色（山雪帽内缘的第二图元）。
    pub rock_ink: VariantInk,
    /// 山背光面色（山符号右侧的深色面）。
    pub mountain_shade_ink: VariantInk,

    // ── 族墨色（桶默认色）────────────────────────────────────────
    /// 森林族墨色。
    pub forest_ink: DecorInk,
    /// 平原族墨色。
    pub plains_ink: DecorInk,
    /// 丘陵族墨色。
    pub hills_ink: DecorInk,
    /// 山地族墨色。
    pub mountain_ink: DecorInk,
    /// 雪原族墨色。
    pub snow_ink: DecorInk,
    /// 沼泽族墨色。
    pub marsh_ink: DecorInk,
    /// 沙漠族墨色。
    pub desert_ink: DecorInk,
    /// 浅海白波墨色。
    pub wave_ink: DecorInk,

    // ── LOD 步距与尺寸（屏幕像素）───────────────────────────────
    /// Edit 档采样步距。
    pub spacing_px_edit: f32,
    /// Region 档采样步距。
    pub spacing_px_region: f32,
    /// Nation 档采样步距。
    pub spacing_px_nation: f32,
    /// Edit 档符号尺寸系数。
    pub size_mul_edit: f32,
    /// Region 档符号尺寸系数。
    pub size_mul_region: f32,
    /// Nation 档符号尺寸系数。
    pub size_mul_nation: f32,

    // ── 浅海白波（离岸驱动 · ADR-0048 追记四）────────────────────
    /// 白波离岸带的内界（格）：`d >= wave_band_min` 才画。
    ///
    /// **2026-10-21 追记四：判定从「绝对深度」改为「离岸距离」。** 旧判定
    /// （`height ∈ [-220, 0]`）与新引入的近岸泡沫带 / 浅滩带在同一位置叠加，
    /// 两套白色元素打架；改为离岸驱动后白弧退到「泡沫带之外、墨线圈之内」
    /// 的空隙带，成为两层之间的填充。
    pub wave_band_min_cells: u16,
    /// 白波离岸带的外界（格）：`d > wave_band_max` 不画（远海是安静的）。
    pub wave_band_max_cells: u16,
    /// 密度衰减尺度（格）：`d = wave_band_min` 全画，`d = wave_band_max`
    /// 稀到 0.15（线性衰减，与旧深度衰减同构、自变量换掉）。
    pub wave_density_span_cells: u16,

    // ── 森林密度调制（密度场 × 主题增益）────────────────────────
    /// 森林符号保留率增益：主题对森林疏密的**整体**调制倍率。
    ///
    /// **分工**：「哪里有森林」由地形事实决定——`wv_terrain::forest`
    /// 的密度场（群系 + 林线 + 聚集噪声，见
    /// `generate_forest_density_from_biome`）；「多密」由主题决定。
    /// 渲染层的实际保留率 = `密度场值 × 本增益`（裁剪到 `0..=1`）。
    ///
    /// `1.0` = 照密度场原样；`< 1.0` 更疏（林间空地更多）；`> 1.0` 更密。
    /// 归属理由：ADR-0048 追记二「数值配方住主题层，执行器留在渲染层」——
    /// 疏密是美术参数，主题该管「多密」，正如它管「长什么样、多大」。
    pub forest_density_gain: f32,

    // ── 森林底色蒙版（§九-1）───────────────────────────────────────
    /// 林底混色强度（0..=1）：森林区底色向 `PaletteColors::forest_floor`
    /// 混色的**最大**比例（实际混量 = 本值 × 密度场值）。
    ///
    /// `0.0` = 不画林底（默认关闭，接线零回归）；`0.5` = 密林核心半压向林下深绿。
    /// 与 [`Self::forest_density_gain`] 的分工：那管**树有多少**，本项管
    /// **地面多暗**——两者都吃密度场，但一个是符号保留率，一个是底色混量。
    pub forest_base_strength: f32,
    /// 林底起染阈值（密度场值）：密度低于本值的格**不染**林底。
    ///
    /// **为什么要有阈值**：密度场给草原 / 稀树草原也给了低密度（0.12～0.22，
    /// 见 `wv_terrain::forest`），若无阈值，整片草原都会被染成林底。
    /// 取 `0.40` 与 `classify_forest` 的「常规林」下界对齐（成片林才有林底），
    /// 但**独立可调**——不同主题可以给稀树也铺一层薄林底。
    pub forest_base_min_density: f32,

    // ── 森林形态（位图 vs 程序化，§七）─────────────────────────────
    /// 地形森林的**位图符号**规格；`None` = 走程序化符号（ADR-0031 D5）。
    ///
    /// **形态二选一，不叠加**：给了规格 → 渲染层画位图树并**停掉**程序化林冠点
    /// （否则「树上长树点」）；没给 → 保持程序化形态（零回归）。
    ///
    /// 贴图键走渲染层的内嵌资产表（`include_bytes!`），故键与
    /// `FeatureRule::symbol_texture` 同一套命名空间。
    pub forest_symbol: Option<ForestSymbolSpec>,

    // ── 森林团块（§七 的第二尺度：区块读法）────────────────────────
    /// 地形森林的**团块**规格（大块贴图铺满林地 + 边缘团块贴图）；`None` = 不铺。
    ///
    /// **为什么要这一层**：单株树（`forest_symbol`）在地图尺度上读作「零星几棵」，
    /// 而手绘地图的森林是**一整片**——参考图（`100+ Hand-drawn Trees and Plants`）
    /// 上半部分的团块就是这种读法。两者是不同尺度，**可以叠加**（团块打底、
    /// 单株点缀），不冲突；缺团块时只有单株，缺单株时只有团块，都能独立成立。
    pub forest_mass: Option<ForestMassSpec>,
}

/// 地形森林的**团块**规格（§七 的第二尺度，2026-10-22 追记）。
///
/// ## 两段式
///
/// 1. **填充**（`fill_*`）：在密度达标处逐块铺贴图——贴图按**世界锚定**的
///    UV 连续平铺（相邻块共用同一映射，看不出格子），于是整片林地是一张
///    连续的手绘森林底纹；
/// 2. **边缘**（`patch_*`）：林地**边界**处的格位盖「团块」贴图（一丛林的
///    小图，自带不规则边缘）——把填充层的格状边界打散成树冠轮廓。
///
/// ## 与单株树的分工
///
/// - 团块层：**区域**读法（远 / 中景的主力）；
/// - 单株层（`ForestSymbolSpec`）：**细节**读法（近景，落在团块之上）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForestMassSpec {
    /// 填充贴图键（可平铺的俯视森林底纹）；`None` = **不铺底纹**。
    ///
    /// **为什么可以是 None**：委托设计的用法（素材说明图右侧示例）是
    /// 「单株 + 树簇堆叠」——林地靠**树簇重叠**成片，不铺连续底纹。
    /// 底纹仍保留为可选手段（大块密林需要压暗时再加）。
    pub fill_texture: Option<&'static str>,
    /// 填充采样步距（屏幕像素）：步距越小越细，代价按平方增长。
    pub fill_step_px: f32,
    /// 填充贴图一次重复的屏幕像素（世界锚定：`tile_wu = tile_px × view.scale`）。
    pub fill_tile_px: f32,
    /// 起铺密度（密度场值）：低于本值的格不铺（草原 / 稀树不铺）。
    pub fill_min_density: f32,
    /// 填充整体不透明度。
    pub fill_alpha: f32,
    /// 填充 tint（乘法）。
    pub fill_tint: [u8; 3],
    /// 边缘团块贴图键；`None` = 只铺填充、不盖边缘团块。
    ///
    /// **横向图集**：`patch_variants` 格并排，逐块按哈希选格（同地形同位置
    /// 恒选同一格——重开工程不跳变）。
    pub patch_texture: Option<&'static str>,
    /// 边缘团块图集**变体数**（横向格数）。`1` = 单张图。
    ///
    /// **为什么要变体**：单张团块在密集林区会明显重复（同一团反复出现）。
    pub patch_variants: u32,
    /// 边缘团块单格**宽高比**（宽 ÷ 高）：这批手绘格是 192×225，非正方形，
    /// 用正方形 quad 会把树冠压扁。
    pub patch_aspect: f32,
    /// 边缘团块**高度**（屏幕像素）。
    pub patch_px: f32,
    /// 边缘团块不透明度。
    pub patch_alpha: f32,
    /// 边缘团块 tint（乘法）。
    pub patch_tint: [u8; 3],
}

impl ForestMassSpec {
    /// 构造（便于常量表书写）。
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        fill_texture: Option<&'static str>,
        fill_step_px: f32,
        fill_tile_px: f32,
        fill_min_density: f32,
        fill_alpha: f32,
        fill_tint: [u8; 3],
        patch_texture: Option<&'static str>,
        patch_variants: u32,
        patch_aspect: f32,
        patch_px: f32,
        patch_alpha: f32,
        patch_tint: [u8; 3],
    ) -> Self {
        Self {
            fill_texture,
            fill_step_px,
            fill_tile_px,
            fill_min_density,
            fill_alpha,
            fill_tint,
            patch_texture,
            patch_variants,
            patch_aspect,
            patch_px,
            patch_alpha,
            patch_tint,
        }
    }
}

/// 水彩装饰配方（日间）——与 2026-10-21 之前的渲染层硬编码**逐位一致**
/// （ADR-0048 追记二：抽取那一步以渲染输出哈希对拍验证行为不变）。
pub const WATERCOLOR: DecorRecipe = DecorRecipe {
    mountain_min_h: 1500,
    hill_min_h: 550,
    snow_cap_h: 1800,
    rock_h: 3400,
    ridge_snap_min_h: 1500,
    climate_cuts: [0.30, 0.72],
    conifer_ink: VariantInk::new(0.18, 0.36, 0.21, 0.85),
    temperate_ink: VariantInk::new(0.17, 0.34, 0.19, 0.85),
    oasis_ink: VariantInk::new(0.29, 0.42, 0.21, 0.80),
    rock_ink: VariantInk::new(0.788, 0.725, 0.541, 1.0), // #C9B98A
    mountain_shade_ink: VariantInk::new(0.28, 0.24, 0.20, 0.85),
    forest_ink: DecorInk::new([0x2B, 0x57, 0x31], 0.85),
    plains_ink: DecorInk::new([0x6B, 0x8F, 0x4E], 0.55),
    hills_ink: DecorInk::new([0x8A, 0x74, 0x44], 0.60),
    mountain_ink: DecorInk::new([0x4C, 0x44, 0x3A], 0.65),
    snow_ink: DecorInk::new([0xFD, 0xFE, 0xFF], 0.90),
    marsh_ink: DecorInk::new([0x4E, 0x6B, 0x5A], 0.65),
    desert_ink: DecorInk::new([0xA8, 0x84, 0x52], 0.55),
    wave_ink: DecorInk::new([0xFD, 0xFE, 0xFF], 0.50),
    spacing_px_edit: 26.0,
    spacing_px_region: 46.0,
    spacing_px_nation: 72.0,
    size_mul_edit: 1.0,
    size_mul_region: 0.8,
    size_mul_nation: 1.6,
    wave_band_min_cells: 3,
    wave_band_max_cells: 9,
    wave_density_span_cells: 6,
    // 森林密度照密度场原样（1.0 = 不额外调制）。
    forest_density_gain: 1.0,
    // 林底蒙版（§九-1）：默认开启，密林核心压到半程林下深绿。
    forest_base_strength: 0.55,
    // 起染阈值对齐 classify_forest 的「常规林」下界（成片林才有林底）。
    forest_base_min_density: 0.40,
    // 森林形态（§七 Sprite 形态，2026-10-22）：手绘位图树（3 变体横向图集）。
    // 给了规格 → 渲染层改画位图树并停掉程序化林冠点（形态二选一）。
    forest_symbol: Some(ForestSymbolSpec::new(
        "overlays/forest/tree_atlas16.png",
        16,
        192.0 / 260.0,
        0.95,
        ForestSymbolBand {
            spacing_px: 34.0,
            size_px: 30.0,
        },
        ForestSymbolBand {
            spacing_px: 56.0,
            size_px: 32.0,
        },
        ForestSymbolBand {
            // 远档（世界级视距）：2026-10-22 实机验收后从 120/46 收紧到 72/36。
            // **为什么**：森林只占图幅约 7%，120px 步距下整屏候选仅 ~100 个，
            // 落到林地上只剩个位数——世界级视距「整张图只长两棵树」，读不出
            // 森林。72px 步距把候选提到 ~300 个，林地得十余棵，成片感回来；
            // 尺寸同时从 46 收到 36（远档符号不该比近档大出一半）。
            spacing_px: 72.0,
            size_px: 36.0,
        },
        // 分级 tint：疏林提亮、常规原色、密林压深（乘法）。
        [242, 248, 238],
        [228, 236, 222],
        [196, 208, 190],
    )),
    // 森林树簇（§七 第二尺度）：**不铺底纹**，靠行 3「密林树木」+ 行 4
    // 「森林树簇」16 变体在密度边界上重叠成片——这是素材说明图右侧示例的用法。
    //
    // 底纹（`fill_*`）仍保留为可选手段，但本主题关掉：示例地图里森林是
    // 「一簇簇树冠叠出来」的，铺一层连续底纹反而糊成色块。
    forest_mass: Some(ForestMassSpec::new(
        None,
        24.0,
        380.0,
        0.52,
        0.72,
        [236, 244, 232],
        Some("overlays/forest/forest_cluster16.png"),
        16,
        192.0 / 225.0,
        118.0,
        0.95,
        [236, 242, 230],
    )),
};

/// 暮色装饰配方——暗纸上的符号要更沉、更疏。
pub const WATERCOLOR_DUSK: DecorRecipe = DecorRecipe {
    // 高度阈值与气候带同源（地形事实），只改颜色与密度感。
    spacing_px_edit: 30.0,
    spacing_px_region: 52.0,
    spacing_px_nation: 78.0,
    size_mul_edit: 0.9,
    size_mul_region: 0.75,
    size_mul_nation: 1.4,
    // 暗纸上的林底要比日间更沉一档：否则「密林深、林间浅」的分层被纸色吃掉。
    forest_base_strength: 0.62,
    // 暮色用暮色图集（同一套委托设计素材的暗调版本），步距比日间略宽。
    forest_symbol: Some(ForestSymbolSpec::new(
        "overlays/forest/tree_atlas16_dusk.png",
        16,
        192.0 / 260.0,
        0.92,
        ForestSymbolBand {
            spacing_px: 36.0,
            size_px: 30.0,
        },
        ForestSymbolBand {
            spacing_px: 60.0,
            size_px: 32.0,
        },
        ForestSymbolBand {
            spacing_px: 78.0,
            size_px: 36.0,
        },
        // 暮色更沉：三级 tint 一律压深一档。
        [226, 232, 222],
        [210, 218, 206],
        [176, 188, 174],
    )),
    // 暮色树簇：同一套图集（暗调版），tint 压沉一档；同样不铺底纹。
    forest_mass: Some(ForestMassSpec::new(
        None,
        24.0,
        380.0,
        0.52,
        0.68,
        [206, 214, 202],
        Some("overlays/forest/forest_cluster16_dusk.png"),
        16,
        192.0 / 225.0,
        118.0,
        0.93,
        [204, 212, 200],
    )),
    ..WATERCOLOR
};

#[cfg(test)]
mod tests {
    use super::*;

    /// 日间配方必须钉住 2026-10-21 之前的渲染层硬编码值——
    /// 抽取这一步「行为不变」的锚点（改值须连带改 ADR-0048 追记二）。
    #[test]
    fn watercolor_recipe_pins_the_previous_hardcoded_values() {
        let r = WATERCOLOR;
        assert_eq!(r.mountain_min_h, 1500, "山阈值");
        assert_eq!(r.hill_min_h, 550, "丘阈值");
        assert_eq!(r.snow_cap_h, 1800, "雪帽阈值");
        assert_eq!(r.rock_h, 3400, "露岩阈值");
        assert_eq!(r.ridge_snap_min_h, 1500, "脊点吸附阈值");
        assert!((r.climate_cuts[0] - 0.30).abs() < 1e-6, "寒带切点");
        assert!((r.climate_cuts[1] - 0.72).abs() < 1e-6, "干旱带切点");
        assert_eq!(r.spacing_px_edit, 26.0, "Edit 步距");
        assert_eq!(r.spacing_px_region, 46.0, "Region 步距");
        assert_eq!(r.spacing_px_nation, 72.0, "Nation 步距");
        assert!((r.size_mul_edit - 1.0).abs() < 1e-6, "Edit 尺寸");
        assert!((r.size_mul_region - 0.8).abs() < 1e-6, "Region 尺寸");
        assert!((r.size_mul_nation - 1.6).abs() < 1e-6, "Nation 尺寸");
        assert_eq!(r.wave_band_min_cells, 3, "白波离岸带内界");
        assert_eq!(r.wave_band_max_cells, 9, "白波离岸带外界");
        assert_eq!(r.wave_density_span_cells, 6, "白波密度衰减尺度");
    }

    /// 变体色必须是配方值（旧值是函数体里的 const）。
    #[test]
    fn variant_inks_match_previous_constants() {
        let r = WATERCOLOR;
        assert_eq!(r.conifer_ink.to_rgba(), [0.18, 0.36, 0.21, 0.85]);
        assert_eq!(r.temperate_ink.to_rgba(), [0.17, 0.34, 0.19, 0.85]);
        assert_eq!(r.oasis_ink.to_rgba(), [0.29, 0.42, 0.21, 0.80]);
        assert_eq!(r.mountain_shade_ink.to_rgba(), [0.28, 0.24, 0.20, 0.85]);
    }

    /// 族墨色与旧 `DecorKind::ink()` 逐位一致。
    #[test]
    fn family_inks_match_previous_table() {
        let r = WATERCOLOR;
        assert_eq!(r.forest_ink, DecorInk::new([0x2B, 0x57, 0x31], 0.85));
        assert_eq!(r.plains_ink, DecorInk::new([0x6B, 0x8F, 0x4E], 0.55));
        assert_eq!(r.hills_ink, DecorInk::new([0x8A, 0x74, 0x44], 0.60));
        assert_eq!(r.mountain_ink, DecorInk::new([0x4C, 0x44, 0x3A], 0.65));
        assert_eq!(r.snow_ink, DecorInk::new([0xFD, 0xFE, 0xFF], 0.90));
        assert_eq!(r.marsh_ink, DecorInk::new([0x4E, 0x6B, 0x5A], 0.65));
        assert_eq!(r.desert_ink, DecorInk::new([0xA8, 0x84, 0x52], 0.55));
        assert_eq!(r.wave_ink, DecorInk::new([0xFD, 0xFE, 0xFF], 0.50));
    }

    /// 高度阈值必须单调递增（山 > 丘、露岩 > 雪帽）——错序会让山先长雪再长岩。
    #[test]
    fn height_thresholds_are_ordered() {
        let r = WATERCOLOR;
        assert!(r.mountain_min_h > r.hill_min_h, "山阈值应高于丘");
        assert!(r.rock_h > r.snow_cap_h, "露岩应高于雪帽");
        assert!(r.snow_cap_h > r.mountain_min_h, "雪帽应高于山阈值");
    }

    /// 气候带切点必须 0 < cut[0] < cut[1] < 1——否则有一段纬度落不进任何带。
    #[test]
    fn climate_cuts_are_ordered_inside_unit_range() {
        let r = WATERCOLOR;
        assert!(r.climate_cuts[0] > 0.0 && r.climate_cuts[0] < r.climate_cuts[1]);
        assert!(r.climate_cuts[1] < 1.0);
    }

    /// 两套配方在可见维度上确有差异（防「改了日间忘配暮色」）。
    #[test]
    fn dusk_recipe_differs_visibly_from_daylight() {
        assert_ne!(
            WATERCOLOR.spacing_px_nation,
            WATERCOLOR_DUSK.spacing_px_nation
        );
        assert_ne!(WATERCOLOR.size_mul_nation, WATERCOLOR_DUSK.size_mul_nation);
    }

    /// 位图森林形态（§七 Sprite 形态，2026-10-22）：日间声明位图树、
    /// 暮色换暮色图集；档位步距与尺寸的远 / 近关系必须成立。
    #[test]
    fn watercolor_recipe_declares_bitmap_forest_form() {
        let spec = WATERCOLOR
            .forest_symbol
            .expect("日间水彩应声明位图森林形态");
        assert_eq!(
            spec.texture, "overlays/forest/tree_atlas16.png",
            "日间应指向日间树图集（2026-10-22 起用俯视生成版）"
        );
        assert_eq!(spec.variants, 16, "树图集变体数（行1 阔叶 8 + 行2 针叶 8）");
        assert!(
            spec.alpha > 0.0 && spec.alpha <= 1.0,
            "不透明度应在 (0,1]，got {}",
            spec.alpha
        );
        // 远档更疏（每屏树更少）、近档更密。
        assert!(
            spec.band_nation.spacing_px > spec.band_region.spacing_px
                && spec.band_region.spacing_px > spec.band_edit.spacing_px,
            "步距应 Nation > Region > Edit，got {} / {} / {}",
            spec.band_nation.spacing_px,
            spec.band_region.spacing_px,
            spec.band_edit.spacing_px
        );
        // 远档树更大（抵消粗步距的稀疏感）。
        assert!(
            spec.band_nation.size_px > spec.band_edit.size_px,
            "远档树应更大，got {} vs {}",
            spec.band_nation.size_px,
            spec.band_edit.size_px
        );
        // 暮色：同一形态、换图集。
        let dusk = WATERCOLOR_DUSK
            .forest_symbol
            .expect("暮色应声明位图森林形态");
        assert_eq!(
            dusk.texture, "overlays/forest/tree_atlas16_dusk.png",
            "暮色应指向暮色树图集"
        );
        assert_eq!(dusk.variants, spec.variants, "两套图集变体数应一致");
    }

    /// 分级 tint 必须单调：疏林提亮 → 密林压深（密林整体更沉）。
    #[test]
    fn forest_symbol_tints_darken_with_density() {
        let spec = WATERCOLOR.forest_symbol.expect("日间应声明位图森林形态");
        let lum = |c: [u8; 3]| (c[0] as u32 + c[1] as u32 + c[2] as u32) / 3;
        assert!(
            lum(spec.tint_sparse) > lum(spec.tint_normal),
            "疏林应比常规林亮：{:?} vs {:?}",
            spec.tint_sparse,
            spec.tint_normal
        );
        assert!(
            lum(spec.tint_normal) > lum(spec.tint_dense),
            "常规林应比密林亮：{:?} vs {:?}",
            spec.tint_normal,
            spec.tint_dense
        );
    }

    /// 森林团块（§七 第二尺度）：日间声明填充 + 边缘两段；贴图一次重复
    /// 明显大于采样步距（否则贴图被切成碎片、看不出「一片林子」）。
    #[test]
    fn watercolor_recipe_declares_forest_mass() {
        let m = WATERCOLOR.forest_mass.expect("日间水彩应声明森林团块");
        assert_eq!(m.fill_texture, None, "填充应指向可平铺森林底纹");
        assert_eq!(
            m.patch_texture,
            Some("overlays/forest/forest_cluster16.png"),
            "边缘应指向团块图集（委托设计的 8 变体）"
        );
        assert_eq!(
            m.patch_variants, 16,
            "树簇图集应是 16 变体（行3 密林 + 行4 树簇）"
        );
        assert!(
            (m.patch_aspect - 192.0 / 225.0).abs() < 1e-6,
            "团块格宽高比应按 192x225 素材，got {}",
            m.patch_aspect
        );
        assert!(
            m.fill_tile_px > m.fill_step_px * 4.0,
            "贴图重复周期应远大于步距（got tile={} step={}）",
            m.fill_tile_px,
            m.fill_step_px
        );
        assert!(
            m.fill_min_density > 0.0 && m.fill_min_density <= 1.0,
            "起铺密度应在 (0,1]，got {}",
            m.fill_min_density
        );
        assert!(
            m.patch_px > m.fill_step_px,
            "边缘团块应明显大于填充步距（got patch={} step={}）",
            m.patch_px,
            m.fill_step_px
        );
        // 边缘团块比填充更大也更不透明才压得住边界。
        assert!(m.patch_alpha > 0.0 && m.patch_alpha <= 1.0);
        assert!(m.fill_alpha > 0.0 && m.fill_alpha <= 1.0);
    }

    /// 暮色团块：同一套贴图、tint 更沉（暗纸上再铺亮绿会发灰）。
    #[test]
    fn dusk_forest_mass_is_darker_but_same_textures() {
        let day = WATERCOLOR.forest_mass.expect("日间应有团块");
        let dusk = WATERCOLOR_DUSK.forest_mass.expect("暮色应有团块");
        assert_eq!(day.fill_texture, dusk.fill_texture, "填充贴图开关应一致");
        assert_eq!(
            day.patch_variants, dusk.patch_variants,
            "两套树簇图集变体数应一致"
        );
        let lum = |c: [u8; 3]| (c[0] as u32 + c[1] as u32 + c[2] as u32) / 3;
        assert!(
            lum(dusk.fill_tint) < lum(day.fill_tint),
            "暮色填充 tint 应更沉：{:?} vs {:?}",
            dusk.fill_tint,
            day.fill_tint
        );
        assert!(
            lum(dusk.patch_tint) < lum(day.patch_tint),
            "暮色边缘 tint 应更沉：{:?} vs {:?}",
            dusk.patch_tint,
            day.patch_tint
        );
    }
}
