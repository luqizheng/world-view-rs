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

/// 一类符号的贴图资产（图集路径 + 横向格数）——ADR-0050。
///
/// **贴图是「地物符号」不是「底图瓦片」**：底图仍由 `terrain_view::cells` 逐格
/// 程序化合成（JRPG 瓦片图集已弃用），本结构只给 `terrain_decor` 的**符号**
/// 提供手绘 PNG。二者是规格 §2.B 里 `props_atlas` 与 `terrain_atlas` 的区别。
///
/// **格序是契约**：森林图集必须按「针叶 / 阔叶 / 稀树」从左到右排列，
/// 与渲染层的 `DecorVariant` 三个值一一对应；乱序会让针叶格长阔叶树。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorArt {
    /// 相对 `crates/wv-render/assets/` 的路径（编译期 `include_bytes!` 嵌入）。
    pub atlas: &'static str,
    /// 图集横向格数（当前森林族固定 3）。
    pub cells: u8,
}

impl DecorArt {
    /// 构造（便于常量表书写）。
    pub const fn new(atlas: &'static str, cells: u8) -> Self {
        Self { atlas, cells }
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

    // ── 浅海白波 ────────────────────────────────────────────────
    /// 白波上界（米）：浅于 `-wave_max_depth_m` 才画。
    pub wave_max_depth_m: i16,
    /// 密度衰减深度尺度（米）：`-10 m` 全画，`-wave_depth_scale_m` 稀到 0.15。
    pub wave_depth_scale_m: f32,

    // ── 贴图（ADR-0050）────────────────────────────────────────
    /// 森林族的手绘贴图；`None` = 走程序化图元（回退路径，缺资产不空帧）。
    ///
    /// 只有森林族有贴图：它是地图上覆盖面积最大、重复感最强的一族；
    /// 其余族继续程序化，避免一次性引入 8 张图集。
    pub forest_art: Option<DecorArt>,
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
    wave_max_depth_m: 220,
    wave_depth_scale_m: 220.0,
    // 日间森林贴图（ADR-0050）：3 格横排 = 针叶 / 阔叶 / 稀树。
    forest_art: Some(DecorArt::new("overlays/forest/decor_atlas_day.png", 3)),
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
    // 暮色走浅墨图集：暗底上深墨会糊成黑块（材质 tint 是乘法，提亮不了深墨）。
    forest_art: Some(DecorArt::new("overlays/forest/decor_atlas_dusk.png", 3)),
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
        assert_eq!(r.wave_max_depth_m, 220, "白波深度上界");
        assert!((r.wave_depth_scale_m - 220.0).abs() < 1e-6, "白波密度尺度");
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

    /// 森林贴图在两套配方里都已接线，且各指各的图集（ADR-0050）。
    ///
    /// 格数必须 = 3：渲染层按「针叶 / 阔叶 / 稀树」三变体取格，少一格会越界。
    #[test]
    fn forest_art_is_wired_for_both_palettes_with_three_cells() {
        let day = WATERCOLOR.forest_art.expect("日间必须有森林贴图");
        let dusk = WATERCOLOR_DUSK.forest_art.expect("暮色必须有森林贴图");
        assert_eq!(day.cells, 3, "森林图集格数 = 三变体");
        assert_eq!(dusk.cells, 3, "森林图集格数 = 三变体");
        assert_ne!(
            day.atlas, dusk.atlas,
            "日/暮各指各的图集（深墨在暗底上会糊成黑块）"
        );
    }
}
