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
//!   美术参数。**ADR-0059 补丁**：「画不画 / 换画法」的**选择**归主题
//!   （[`SymbolStyle`]：Default / None / Tile），构造本身仍留渲染层——
//!   theme 是内容，渲染是机制。
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

/// 装饰符号的画法（ADR-0059）：渲染层内置默认为保底，theme 可改写。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SymbolStyle {
    /// 内置默认程序化形状（现状：山三角/林团/半圆丘……）。向后兼容的保底。
    #[default]
    Default,
    /// 不画该符号——表达交给其他层（等值线 / 皴法 / 海配方 / 地形色）。
    None,
    /// 贴图瓦片：键 → 渲染侧装载表。未知键 warn 后回退 `Default`
    /// （「缺资产不崩」口径，与 FeatureRule::symbol_texture 一致）。
    Tile(&'static str),
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

    // ── 符号画法（ADR-0059）──────────────────────────────────────
    /// 山符号的画法：渲染层内置默认为保底，theme 可改写。
    ///
    /// **分工**：「画不画 / 换画法」由主题裁定，「几何怎么构造」留在渲染层
    /// （`wv-render::terrain_decor` 的程序化三角化）——theme 是内容、
    /// 渲染是机制（本 crate 总纲）。v1 只有山：它是唯一被实机截图证实的
    /// 画法冲突；其余七类按证据追加（style-per-kind 模式即为此预留）。
    pub mountain_style: SymbolStyle,
    /// 森林符号的画法（ADR-0059 追记一，2026-10-25）：`Default` = 内置三变体
    /// 程序化形状（针叶三角排 / 阔叶圆团 / 稀树圆点），`None` = 群系格不铺林团
    /// （林身份交给地形色 + 地点覆盖物层），`Tile` = 预留贴图（v1 warn 回退）。
    ///
    /// **与山的分工差异**：森林的「变体」维度（寒带 / 温带 / 干旱）在贴图方案下
    /// 由**图集等分 + `(种子, 站点序号) % variants` 取模**承载——与覆盖物层
    /// `SYMBOL_ATLAS_VARIANTS = 3` 同构，故 `Tile(&'static str)` 单键足够表达，
    /// 无需在枚举里另写变体数（2026-10-25 主理人裁定）。
    ///
    /// **注意双通道**：具名森林地物的树图集走 `FeatureRule::symbol_texture`
    /// （ADR-0045），本字段只管**群系格**上的程序化林团——两者互不替代。
    pub forest_style: SymbolStyle,

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
}

/// 水彩装饰配方（日间）——与 2026-10-21 之前的渲染层硬编码**逐位一致**
/// （ADR-0048 追记二：抽取那一步以渲染输出哈希对拍验证行为不变）。
pub const WATERCOLOR: DecorRecipe = DecorRecipe {
    mountain_min_h: 1500,
    hill_min_h: 550,
    snow_cap_h: 1800,
    rock_h: 3400,
    ridge_snap_min_h: 1500,
    // 山符号画法：内置默认三角（ADR-0059 D4）——观感零变化的保底。
    mountain_style: SymbolStyle::Default,
    // 森林符号画法：内置三变体程序化形状（ADR-0059 追记一）——同上，保底默认。
    forest_style: SymbolStyle::Default,
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
};

/// 暮色装饰配方——暗纸上的符号要更沉、更疏。
pub const WATERCOLOR_DUSK: DecorRecipe = DecorRecipe {
    // 高度阈值与气候带同源（地形事实），只改颜色与密度感。
    // 山符号画法同内置默认（ADR-0059 D4）——与日间一致，显式钉住。
    mountain_style: SymbolStyle::Default,
    // 森林符号画法同内置默认（ADR-0059 追记一）——显式钉住，防继承漂移。
    forest_style: SymbolStyle::Default,
    spacing_px_edit: 30.0,
    spacing_px_region: 52.0,
    spacing_px_nation: 78.0,
    size_mul_edit: 0.9,
    size_mul_region: 0.75,
    size_mul_nation: 1.4,
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

    /// 两套内置配方的山符号画法都是 `Default`（ADR-0059 D4）——内置默认
    /// 三角是保底，观感零变化；「不画」的选择权归主题（水墨包的 `None`
    /// 锚点在 wv-theme-chinese-ink）。
    #[test]
    fn builtin_recipes_keep_default_mountain_style() {
        assert_eq!(WATERCOLOR.mountain_style, SymbolStyle::Default);
        assert_eq!(WATERCOLOR_DUSK.mountain_style, SymbolStyle::Default);
    }

    /// 两套内置配方的森林符号画法都是 `Default`（ADR-0059 追记一）——
    /// 程序化三变体林团是保底，内置主题观感零变化。「不画 / 换贴图」的
    /// 选择权归主题（水墨包显式 `Default` 的锚点在 wv-theme-chinese-ink：
    /// 苔点与林团是水墨正解，疏密由 `forest_density_gain` 调制）。
    #[test]
    fn builtin_recipes_keep_default_forest_style() {
        assert_eq!(WATERCOLOR.forest_style, SymbolStyle::Default);
        assert_eq!(WATERCOLOR_DUSK.forest_style, SymbolStyle::Default);
    }
}
