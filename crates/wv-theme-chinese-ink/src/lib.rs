//! # wv-theme-chinese-ink —— 示例主题包（ADR-0023 M2）
//!
//! **目的**：演示"加新风格 = 加新 crate、零改 wv-render 源码"的注入契约。
//! **色值留空 / 占位**：本 crate 的 `ThemePack::palette` 全部字段**复用内置古舆图
//! 槽位**，仅换 `id` / `display_name` / `treatment`（示意）；色值校准待美术侧提供。
//!
//! ## 加载方式（游戏 app 视角）
//!
//! ```ignore
//! use wv_theme::registry::{MergedThemeRegistry, ThemePackRegistry};
//! use wv_theme_chinese_ink::ChineseInkRegistry;
//!
//! let merged = MergedThemeRegistry::merge(&[&ChineseInkRegistry]);
//! assert!(merged.lookup("chinese_ink").is_some());
//! ```
//!
//! ## 边界
//! - **不**做 `Box<dyn ThemePackRegistry>`：与 wv-render 同一红线（`&'static` 切片 + 单态化）；
//! - **不**做贴图（`atlas: None`，M2+ 接 `TileAtlasSpec` 时再补）；
//! - **不**做 i18n：主题名走 i18n 是 M3 议题（M2 用 `display_name: &'static str` 单语）；
//! - **不**改 `wv-render` 任何源码——验证"加风格零改核心"。

use wv_theme::decor::{DecorInk, DecorRecipe, VariantInk as DecorVariantInk};
use wv_theme::overlays::{FeatureOverlaySet, FeatureRule, RegionOverlayRule, RegionOverlaySet};
use wv_theme::palette::PaletteColors;
use wv_theme::registry::ThemePackRegistry;
use wv_theme::sea::{RippleSpec, SeaRecipe};
use wv_theme::theme_pack::{ThemePack, TreatmentFlags};

/// 水墨地形色板（初值 · 2026-10-21）。
///
/// **依据**：civ-atlas `fantasy.ts` 的水墨海色三档（`SEA = #8fabb0` /
/// `SEA_SHELF = #a8c2bd` / `SEA_DEEP = #6f8f99`）与宣纸底（`#E8E0CE`），
/// 陆色按「淡墨 + 留白」的原则给出低饱和初值。
///
/// **待美术校准**（ADR-0023 M2 的白名单流程尚未就绪）：这组值是**可调的初值**
/// 而非定稿——换主题时改这个常量即可，不必碰渲染层。
const INK_CORE_TERRAIN: [wv_theme::Color; 10] = [
    wv_theme::colors::hex(0x6F8F99), // 0 深水（墨青）
    wv_theme::colors::hex(0x8FABB0), // 1 浅水（淡青灰）
    wv_theme::colors::hex(0xD8CDB4), // 2 沙 / 宣纸赭
    wv_theme::colors::hex(0x9AA88C), // 3 草（淡墨绿）
    wv_theme::colors::hex(0x6E7F63), // 4 森林（墨绿）
    wv_theme::colors::hex(0xB5A88C), // 5 丘陵（淡赭）
    wv_theme::colors::hex(0x8A857A), // 6 山地（墨灰）
    wv_theme::colors::hex(0xEDE9DF), // 7 雪（纸白）
    wv_theme::colors::hex(0x8A9484), // 8 沼泽（灰绿）
    wv_theme::colors::hex(0xB09A78), // 9 道路（淡赭）
];

/// 水墨色板（`PaletteColors` 全 36 槽）。
///
/// 地形 10 槽 + 纸色 / 深海 / 浅滩 / 等值线墨色走水墨值；其余槽位（POI /
/// 道路 / 标签 / 建筑）**沿用日间值**——它们与「水墨」观感关联最弱，等
/// 美术给出白名单后再逐项替换。
const INK_PALETTE: PaletteColors = PaletteColors {
    core: INK_CORE_TERRAIN,
    // 水墨的底图淡化更弱（纸面本身就淡，无需再洗）。
    soften_keep: 0.92,
    // 宣纸底色。
    soften_toward: wv_theme::colors::hex(0xE8E0CE),
    poi_dot: wv_theme::colors::PLACE_DOT,
    poi_ring: wv_theme::colors::POI_RING,
    road_stroke: wv_theme::colors::ROAD_STROKE,
    road_casing: wv_theme::colors::ROAD_CASING,
    street_major: wv_theme::colors::STREET_MAJOR,
    street_minor: wv_theme::colors::STREET_MINOR,
    label_fill: wv_theme::colors::LABEL_FILL,
    label_region: wv_theme::colors::LABEL_REGION,
    label_place: wv_theme::colors::LABEL_PLACE,
    label_water: wv_theme::colors::LABEL_WATER,
    dot_place: wv_theme::colors::DOT_PLACE,
    dot_region: wv_theme::colors::DOT_REGION,
    dot_faction: wv_theme::colors::DOT_FACTION,
    dot_event: wv_theme::colors::DOT_EVENT,
    text_place: wv_theme::colors::TEXT_PLACE,
    text_region: wv_theme::colors::TEXT_REGION,
    text_faction: wv_theme::colors::TEXT_FACTION,
    text_event: wv_theme::colors::TEXT_EVENT,
    snow_highlight: wv_theme::colors::SNOW_HIGHLIGHT,
    snow_shadow: wv_theme::colors::SNOW_SHADOW,
    ice_paper: wv_theme::colors::ICE_PAPER,
    ice_ink: wv_theme::colors::ICE_INK,
    ice_shadow: wv_theme::colors::ICE_SHADOW,
    halo_fill: wv_theme::colors::HALO_FILL,
    // 等值线墨色：淡墨（不是日间的褐墨）。
    isoline_ink: wv_theme::colors::hex(0x3A3834),
    grid_line: wv_theme::colors::GRID_LINE,
    building_top: wv_theme::colors::BUILDING_TOP,
    building_side: wv_theme::colors::BUILDING_SIDE,
    event_backdrop: wv_theme::colors::EVENT_VIEW_BACKDROP,
    line_on_light: wv_theme::colors::LINE_ON_LIGHT,
    line_on_mid: wv_theme::colors::LINE_ON_MID,
    line_on_dark: wv_theme::colors::LINE_ON_DARK,
    // 深海：墨青（不是日间的饱和深蓝）。
    deep_ocean_blue: wv_theme::colors::hex(0x4A6B78),
    // 近岸浅滩：淡青（与水墨海色同族）。
    water_shore: wv_theme::colors::hex(0xB4C7C0),
};

pub static PACK: ThemePack = ThemePack {
    id: "chinese_ink",
    display_name: "水墨（占位）",
    palette: INK_PALETTE, // 水墨色板（初值，见上方注释；待美术校准）
    treatment: TreatmentFlags::EMPTY,
    atlas: None,
    // 海的画法配方（ADR-0048）：水墨包的海不画浅滩色带、不画纸纹颗粒——
    // 留白是水墨的语法；离岸墨线波纹仍保留但更疏更淡（皴法的水岸线）。
    // TODO(美术校准): 参数为占位值，待水墨白名单就绪后替换。
    // 装饰配方（ADR-0048 追记二）：水墨的装饰比水彩**更疏更沉**——
    // 留白是水墨的语法，密匝匝的符号会把纸面填满。高度阈值与气候带同源
    // （地形事实），差异做在墨色（更灰）与密度（步距更宽、符号更小）。
    decor: DecorRecipe {
        spacing_px_edit: 40.0,
        spacing_px_region: 64.0,
        spacing_px_nation: 96.0,
        size_mul_edit: 0.8,
        size_mul_region: 0.65,
        size_mul_nation: 1.2,
        conifer_ink: DecorVariantInk::new(0.16, 0.24, 0.18, 0.80),
        temperate_ink: DecorVariantInk::new(0.18, 0.22, 0.17, 0.78),
        oasis_ink: DecorVariantInk::new(0.22, 0.28, 0.18, 0.70),
        rock_ink: DecorVariantInk::new(0.40, 0.39, 0.35, 0.85),
        mountain_shade_ink: DecorVariantInk::new(0.20, 0.19, 0.17, 0.70),
        forest_ink: DecorInk::new([0x2A, 0x33, 0x28], 0.75),
        plains_ink: DecorInk::new([0x4A, 0x52, 0x40], 0.45),
        hills_ink: DecorInk::new([0x55, 0x50, 0x3E], 0.50),
        mountain_ink: DecorInk::new([0x33, 0x32, 0x2E], 0.60),
        snow_ink: DecorInk::new([0xE8, 0xEA, 0xE4], 0.80),
        marsh_ink: DecorInk::new([0x35, 0x44, 0x39], 0.55),
        desert_ink: DecorInk::new([0x6B, 0x5E, 0x46], 0.45),
        wave_ink: DecorInk::new([0xD8, 0xDC, 0xD6], 0.35),
        ..wv_theme::decor::WATERCOLOR
    },
    sea: SeaRecipe {
        depth_scale_m: 2000.0,
        depth_exponent: 1.0,
        shore_band_cells: 0.0,
        shore_peak: 0.0,
        water_grain: 0.0,
        ripple_warp_cells: 1.5,
        ripples: &[
            RippleSpec {
                dist_cells: 5.0,
                sigma_cells: 0.7,
                alpha: 0.20,
            },
            RippleSpec {
                dist_cells: 12.0,
                sigma_cells: 0.9,
                alpha: 0.10,
            },
        ],
        ink: [0x3A, 0x38, 0x34],
        // 水墨的浪花是「皴」不是「白浪」：一道贴岸的淡墨痕，强度远低于
        // 水彩的白泡沫，且不破碎——水墨的岸线是连续的皴笔。
        foam: &[RippleSpec {
            dist_cells: 1.2,
            sigma_cells: 0.75,
            alpha: 0.16,
        }],
        foam_ink: [0x6A, 0x68, 0x62],
        foam_break: 0.0,
        foam_warp_cells: 1.4,
        foam_break_freq: 0.0,
        // 水墨的近岸不画排线（那是钢笔/铜版画的手法）——留白 + 一道淡墨皴线。
        shore_hatch: None,
        // 外缘积墨环：水墨的边缘更硬（墨是顿出来的，不是化开的）——强度更高、
        // 宽度更窄。
        shore_edge_strength: 0.45,
        shore_edge_width_cells: 0.8,
        shore_edge_ink: Some([0x2E, 0x2C, 0x28]),
    },
    // TODO(美术校准): 墨绿晕 / 符号色与「更疏」的采样间距均为占位值，
    // 待美术侧给出水墨白名单后替换——v1 水墨包不可选，不参与验收（D13）。
    overlays: FeatureOverlaySet {
        forest: Some(FeatureRule {
            halo_color: [0x44, 0x52, 0x42],
            halo_alpha: 0.25,
            symbol_color: [0x2B, 0x35, 0x2A],
            symbol_size_px: 6.0,
            symbol_spacing_wu: 32.0,
            symbol_texture: None,
            halo_texture: None,
        }),
        // 其余族 v1 无演绎（ADR-0032 D2：字段一次定型，规则待美术校准）。
        grassland: None,
        desert: None,
        wetland: None,
        cold: None,
        highland: None,
        water: None,
        road: None,
    },
    // 区域覆盖物键（ADR-0045 D10）：水墨版四档，墨色更灰、采样更疏。
    // 键集与内置古舆图**同款**——换主题不应让作者的区域声明突然"失配"
    // （那会画成缺失覆盖物标记）；差异做在颜色 / 尺寸 / 间距上。
    region_overlays: RegionOverlaySet::new(&[
        (
            "conifer",
            RegionOverlayRule {
                variant: "conifer",
                color: [0x2A, 0x33, 0x2A],
                alpha: None,
                size_scale: 0.85,
                spacing_scale: 1.15,
            },
        ),
        (
            "broadleaf",
            RegionOverlayRule {
                variant: "broadleaf",
                color: [0x2B, 0x35, 0x2A],
                alpha: None,
                size_scale: 0.85,
                spacing_scale: 1.15,
            },
        ),
        (
            "sparse",
            RegionOverlayRule {
                variant: "sparse",
                color: [0x3A, 0x44, 0x36],
                alpha: None,
                size_scale: 0.85,
                spacing_scale: 1.15,
            },
        ),
        (
            "boulder_field",
            RegionOverlayRule {
                variant: "boulder",
                color: [0x5C, 0x5A, 0x52],
                alpha: Some(0.75),
                size_scale: 0.85,
                spacing_scale: 1.3,
            },
        ),
    ]),
};

/// 注册表持有的静态切片（`&[...]` 临时数组的栈上生命周期不够 `&'static`，
/// 必须显式 `static`——与 `wv-render::theme::theme_pack::builtin::ALL` 同档）。
static CHINESE_INK_PACKS: &[(&str, &ThemePack)] = &[("chinese_ink", &PACK)];

/// 示例注册表。
pub struct ChineseInkRegistry;

impl ThemePackRegistry for ChineseInkRegistry {
    fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
        CHINESE_INK_PACKS
    }
    fn registry_id(&self) -> &'static str {
        "chinese_ink"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_theme::palette::PaletteColors;
    use wv_theme::registry::MergedThemeRegistry;

    /// 主题包 id 稳定（持久化键 / 跨工程引用依赖）。
    #[test]
    fn pack_id_is_stable() {
        assert_eq!(PACK.id, "chinese_ink");
    }

    /// 注册表暴露该 pack。
    #[test]
    fn registry_exposes_pack() {
        let r = ChineseInkRegistry;
        let packs = r.packs();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].0, "chinese_ink");
        assert_eq!(packs[0].1.id, "chinese_ink");
    }

    /// 跨 crate 合并：内置 + chinese_ink = 两个 id 都在查表里。
    #[test]
    fn merge_with_builtin_finds_chinese_ink() {
        struct BuiltinOnly;
        impl ThemePackRegistry for BuiltinOnly {
            fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
                wv_theme::theme_pack::builtin::ALL
            }
            fn registry_id(&self) -> &'static str {
                "builtin_only"
            }
        }
        let merged = MergedThemeRegistry::merge(&[&BuiltinOnly, &ChineseInkRegistry]);
        assert!(merged.lookup("antique_default").is_some());
        assert!(merged.lookup("chinese_ink").is_some());
        assert_eq!(merged.len(), 2);
    }

    /// 色值占位 = DAYLIGHT（M2 契约演示；M2+ 美术校准后此测试改"逐位匹配 chinese_ink 白名单"）。
    #[test]
    fn pack_palette_is_the_ink_palette_not_daylight() {
        // 2026-10-21：占位期结束——PACK 自带水墨色板（不再沿用 DAYLIGHT）。
        // 关键槽位锚点：海两槽来自 civ-atlas 的水墨海色，纸色是宣纸底。
        assert_ne!(
            PACK.palette.core,
            PaletteColors::DAYLIGHT.core,
            "水墨色板不应再是 DAYLIGHT"
        );
        // 直接与 hex 构造的对拍值比较（`wv_theme::colors::hex` 是同一来源）。
        assert_eq!(
            PACK.palette.core[0],
            wv_theme::colors::hex(0x6F8F99),
            "深水 = 墨青"
        );
        assert_eq!(
            PACK.palette.core[1],
            wv_theme::colors::hex(0x8FABB0),
            "浅水 = 淡青灰"
        );
        assert_eq!(
            PACK.palette.soften_toward,
            wv_theme::colors::hex(0xE8E0CE),
            "纸色 = 宣纸底"
        );
    }

    /// 水墨包也装载森林族覆盖规则（占位值）——验证外部主题 crate 在族键 API
    /// （ADR-0032）下 `overlays` 规则通路仍成立（族类型经 wv-render 再导出）。
    #[test]
    fn pack_has_forest_overlay_rule() {
        let rule = PACK
            .overlays
            .rule_for(wv_theme::overlays::BiomeFamily::Forest)
            .expect("水墨包应给森林族配置占位规则");
        // Task 9 裁定：水墨包暂不接贴图（纯色回退），待美术校准后再定
        // 水墨树 / 晕资产——避免把日间古舆图图集错配到水墨主题。
        assert!(
            rule.symbol_texture.is_none() && rule.halo_texture.is_none(),
            "水墨包暂留纯色路径（贴图待美术校准）"
        );
    }
}
