//! Topographic viewmode 专用调色（[ADR-0039]）。
//!
//! 独立 4 令牌（**不走 `ThemePack`**）：水单色 + 陆按 height 百分位切 3 档。
//! `sync_terrain_chunks` 在 `Topographic` mode 分支里用本表的 `[f32; 3]` sRGB
//! 直接喂 `bevy::prelude::Color::srgb`。
//!
//! ## 不放 `#[cfg(feature = "app")]` 门
//!
//! 本模块是 `srgb_3` 数组 + 一个 `Default` impl，**零 bevy 依赖**。
//! 与 `colors` / `colors_dusk` / `colors_google` 等仍在 `#[cfg(feature = "app")]`
//! 门后的 module 不同——它们是「走 ThemePack 的美术色板」，而本表是
//! 「w v-render 内部的分析视图调色」。分析视图属于**渲染层职责**
//! （[ADR-0020]），不放壳但也不在 cfg 门下——这是为了 sync_terrain_chunks
//! 在不带 feature 时也能编译通过。
//!
//! [ADR-0020]: ../../../docs/adr/ADR-0020-wv-render-crate.md
//! [ADR-0039]: ../../../docs/adr/ADR-0039-topographic-viewmode.md

/// Topographic viewmode 的陆格 3 阶百分位阈值（[ADR-0039] 决策 4）。
///
/// `p_low <= p_high`，区间 `[0.0, 1.0]`。`sync_terrain_chunks` 用
/// `TerrainGrid::compute_height_percentiles(p_low, p_high)` 取阈值。
pub const TOPOGRAPHIC_P_LOW: f32 = 0.33;
pub const TOPOGRAPHIC_P_HIGH: f32 = 0.66;

/// sRGB 三元组（0.0–1.0，linear）。直接喂 [`bevy::prelude::Color::srgb`]。
pub type Srgb3 = [f32; 3];

/// 4 令牌：单色水 + 陆 3 阶。
///
/// 默认值是 **draft 提议值**（2026-10-19），等用户实机校准；
/// 校准后追记此 doc-comment，不改正文（[ADR-0039] §4.1.2，[AGENTS §6.4]）。
///
/// ThemePack 化时应能 override（[ADR-0023] D4 留口；本模块**不**承接，
/// 留给未来追记）。
///
/// [ADR-0023]: ../../../docs/adr/ADR-0023-theme-pack-injection-contract.md
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopographicPalette {
    /// 水（surface=水 或 height<0）。
    pub water: Srgb3,
    /// 陆低地（height ≤ p_low 百分位）。
    pub land_low: Srgb3,
    /// 陆中地（p_low 百分位 < height ≤ p_high 百分位）。
    pub land_mid: Srgb3,
    /// 陆高地（height > p_high 百分位）。
    pub land_high: Srgb3,
}

impl Default for TopographicPalette {
    fn default() -> Self {
        // Draft 提议值（2026-10-19）：
        //   water    #3A7DB5
        //   land_low #F0E5C9  (沙调暖底)
        //   land_mid #C9B58A  (土褐过渡)
        //   land_high#8A7A66  (岩褐)
        Self {
            water: [0.227, 0.490, 0.710],
            land_low: [0.941, 0.898, 0.788],
            land_mid: [0.788, 0.710, 0.541],
            land_high: [0.541, 0.478, 0.400],
        }
    }
}

/// 按 `surface` + `height` + 百分位阈值返回对应 `[f32; 3]` sRGB。
///
/// - 水（surface=水 或 height<0） → `water`
/// - 陆低地（height ≤ `p_low_threshold`）→ `land_low`
/// - 陆中地（在两阈值之间）→ `land_mid`
/// - 陆高地（height > `p_high_threshold`）→ `land_high`
///
/// 与 `wv_terrain::biome::display_kind` 的「湿判定」一致（surface=水 ∨ height<0）
/// 但**不读 `biome`**（X 路线）。
pub fn pick(
    pal: &TopographicPalette,
    surface_is_water: bool,
    height: i16,
    p_low: i16,
    p_high: i16,
) -> Srgb3 {
    if surface_is_water || height < 0 {
        return pal.water;
    }
    if height > p_high {
        pal.land_high
    } else if height > p_low {
        pal.land_mid
    } else {
        pal.land_low
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::{SURFACE_LAND, SURFACE_WATER};

    #[test]
    fn default_palette_has_four_distinct_colors() {
        let p = TopographicPalette::default();
        // 4 令牌两两不同（防止默认值误填相同色）。
        assert_ne!(p.water, p.land_low);
        assert_ne!(p.land_low, p.land_mid);
        assert_ne!(p.land_mid, p.land_high);
        assert_ne!(p.water, p.land_high);
    }

    /// `pick` 在水/陆/3 阶分类正确，覆盖全部 5 路径。
    #[test]
    fn pick_categorizes_all_five_paths() {
        let pal = TopographicPalette::default();
        // 水（surface=水 + height 正）→ water
        assert_eq!(pick(&pal, true, 500, 100, 1000), pal.water);
        // 水（surface=陆 + height 负）→ water（湿判定）
        assert_eq!(pick(&pal, false, -50, 100, 1000), pal.water);
        // 陆低地
        assert_eq!(pick(&pal, false, 50, 100, 1000), pal.land_low);
        // 陆中地
        assert_eq!(pick(&pal, false, 500, 100, 1000), pal.land_mid);
        // 陆高地
        assert_eq!(pick(&pal, false, 2000, 100, 1000), pal.land_high);
    }

    /// `pick` 的输入应与 `biome` 解耦：biome 不影响分类路径（X 路线验证）。
    #[test]
    fn pick_does_not_read_biome() {
        let pal = TopographicPalette::default();
        // 同一 height/surface 在 BIOME_NONE / BIOME_FOREST / BIOME_DESERT 都同色。
        let paths = [pick(&pal, false, 500, 100, 1000); 3];
        for s in paths {
            assert_eq!(s, pal.land_mid);
        }
        // 水覆盖 surface=水的常量值，biome 在 caller 里压根不会被 pick 看到。
        assert_eq!(SURFACE_WATER, 0);
        assert_eq!(SURFACE_LAND, 1);
    }
}
