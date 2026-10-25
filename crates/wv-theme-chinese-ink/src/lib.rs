//! # wv-theme-chinese-ink —— 水墨主题包（宋元文人山水 · 美术定稿）
//!
//! **目的**：演示「加新风格 = 加新 crate、零改 wv-render 源码」的注入契约
//! （ADR-0023 M2），同时是**首个美术定稿主题包**——参数定稿白名单见
//! `docs/design/chinese-ink-theme-calibration.md`（2026-10-25 用户审批通过）。
//!
//! **风格锚点**：宋元文人山水——留白为语法、淡墨皴法、雅致灰调。
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
//! - **v1 不做贴图**（`atlas: None`，纯色路径锁定）；v2 水墨资产规格见方案 §9，
//!   装载通道转待裁（方案 §12.3）；
//! - **不**做 i18n：主题名走 i18n 是 M3 议题（M2 用 `display_name: &'static str` 单语）；
//! - **不**改 `wv-render` 任何源码——验证「加风格零改核心」。

use wv_theme::decor::{DecorInk, DecorRecipe, SymbolStyle, VariantInk as DecorVariantInk};
use wv_theme::overlays::{FeatureOverlaySet, FeatureRule, RegionOverlayRule, RegionOverlaySet};
use wv_theme::palette::PaletteColors;
use wv_theme::registry::ThemePackRegistry;
use wv_theme::sea::{RippleSpec, SeaRecipe};
use wv_theme::theme_pack::{ThemePack, TreatmentFlags};

/// 水墨地形色板 core 10 槽（**定稿保留** · 2026-10-25，方案 §3.1）。
///
/// **依据**：civ-atlas `fantasy.ts` 的水墨海色三档（`SEA = #8fabb0` /
/// `SEA_SHELF = #a8c2bd` / `SEA_DEEP = #6f8f99`）与宣纸底（`#E8E0CE`），
/// 陆色按「淡墨 + 留白」的原则给出低饱和初值。
///
/// **裁定**：初值即水墨基调，方案 §3.1 全保留；三个测试锚点
/// （`core[0]` / `core[1]` / `soften_toward`）依赖这组值，改动前先同步测试。
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

/// 水墨色板（`PaletteColors` 全 36 槽 · **美术定稿** · 2026-10-25，方案 §3）。
///
/// 地形 core 10 槽 + soften 双参数 + isoline / deep_ocean / water_shore 沿用初值；
/// 其余 31 项从日间值全面换入「焦浓重淡清」墨阶体系——**线要素一律去彩**
/// （棕褐 = 古舆图暖调，水墨必须中性墨 / 青墨），**底衬一律归纸色**
/// （宣纸底 `#E8E0CE`）。宣纸上文本要素 CR≥4.7、图形要素 CR≥3.0
/// （WCAG 估算表见方案 §10.1）。
const INK_PALETTE: PaletteColors = PaletteColors {
    core: INK_CORE_TERRAIN,
    // 水墨的底图淡化更弱（纸面本身就淡，无需再洗）。
    soften_keep: 0.92,
    // 宣纸底色。
    soften_toward: wv_theme::colors::hex(0xE8E0CE),
    // ── POI：赭墨锚点 + 纸色外环（环=纸色才「托」得住点）────────────
    poi_dot: wv_theme::colors::hex(0x7A5C40),
    poi_ring: wv_theme::colors::hex(0xE8E0CE),
    // ── 道路：浓墨一笔 / casing=纸色 / 线要素去彩────────────────────
    road_stroke: wv_theme::colors::hex(0x4A463E),
    road_casing: wv_theme::colors::hex(0xE8E0CE),
    street_major: wv_theme::colors::hex(0x5A564C),
    street_minor: wv_theme::colors::hex(0x7E7968),
    // ── 标签：中性墨阶（焦墨国名 → 浓墨地名 → 青墨海名）─────────────
    label_fill: wv_theme::colors::hex(0x322F29),
    label_region: wv_theme::colors::hex(0x26241F),
    label_place: wv_theme::colors::hex(0x322F29),
    label_water: wv_theme::colors::hex(0x35464A),
    // ── POI 圆点 / hover 文字：色相保留、明度入墨（红>蓝序保持）──────
    dot_place: wv_theme::colors::hex(0x8A6A4C),
    dot_region: wv_theme::colors::hex(0x5C6E72),
    dot_faction: wv_theme::colors::hex(0xA85442),
    dot_event: wv_theme::colors::hex(0x7A6288),
    text_place: wv_theme::colors::hex(0x5A4432),
    text_region: wv_theme::colors::hex(0x3E4A50),
    text_faction: wv_theme::colors::hex(0x6B4A38),
    text_event: wv_theme::colors::hex(0x4A4058),
    // ── 雪 / 冰：雪=留白（暖纸白高光 + 灰墨影，去冷蓝）──────────────
    snow_highlight: wv_theme::colors::hex(0xF5F1E6),
    snow_shadow: wv_theme::colors::hex(0xA8A496),
    ice_paper: wv_theme::colors::hex(0xEAE8DC),
    ice_ink: wv_theme::colors::hex(0x46545A),
    ice_shadow: wv_theme::colors::hex(0x9AA4A6),
    // ── 底衬：晕边 / 事件图底一律纸色────────────────────────────────
    halo_fill: wv_theme::colors::hex(0xE8E0CE),
    // 等值线墨色：重墨（等值线/海岸线主墨色）。
    isoline_ink: wv_theme::colors::hex(0x3A3834),
    // 编辑网格=淡墨虚层（辅助层让墨，网格系统另有 alpha 调制）。
    grid_line: wv_theme::colors::hex(0x948C7C),
    // 建筑：纸白轻灰顶 + 淡墨灰侧面（去陶土暖调）。
    building_top: wv_theme::colors::hex(0xD8D2C2),
    building_side: wv_theme::colors::hex(0x8C8574),
    // 事件图底=纸色（与日间「底=纸色」同构）。
    event_backdrop: wv_theme::colors::hex(0xE8E0CE),
    // 描线三档全部入墨阶：浅底=重墨 → 中底=浓墨 → 深底=焦墨。
    line_on_light: wv_theme::colors::hex(0x3A3731),
    line_on_mid: wv_theme::colors::hex(0x2E2B26),
    line_on_dark: wv_theme::colors::hex(0x26241F),
    // 深海：墨青（t=1.0 直达端点，与 core[0] 同族）。
    deep_ocean_blue: wv_theme::colors::hex(0x4A6B78),
    // 近岸浅滩：淡青（浅滩带关闭时仅作浅水端备份，值无害）。
    water_shore: wv_theme::colors::hex(0xB4C7C0),
};

/// 水墨手绘旗标组（**美术定稿** · 2026-10-25，方案 §4）。
///
/// 从 `EMPTY`（干净矢量）翻到「HAND_DRAWN 去海岸双晕、去浅滩带」——
/// `EMPTY` 是水墨包最大的观感缺陷：`water_gradient=false` 使海深浅墨阶
/// 整层失效（退回深/浅两档平涂），纸纹 / 墨晕 / 边界扰动全关，宣纸感尽失。
/// 水墨是宣纸上的手绘，纸感与墨晕就是媒介本身。
const INK_TREATMENT: TreatmentFlags = TreatmentFlags {
    // 关：水墨海岸不画双晕圈——岸线 = 等值线单墨 + 顿笔积墨（方案 §5.6）。
    coast_halo: false,
    // 开：墨分五色——海的深浅墨阶是本主题的水彩感来源。
    water_gradient: true,
    // 雪影=灰墨淡施（snow_shadow 已改灰墨），高海拔层次保留。
    snow_shade: true,
    // 渲染未接线，恒 true 无副作用；ice_* 槽位已按水墨校准。
    sea_ice: true,
    // 宣纸纤维是媒介。
    paper_grain: true,
    // 河中线=水纹一笔。
    river_centerline: true,
    // 墨线不规则=笔触。
    boundary_wobble: true,
    // 墨线随缩放亚线性=笔迹观感。
    ink_sublinear: true,
    // 远景洗向纸色=烟云留白（三远之「远」）。
    paper_fog: true,
    // 陆格明度颗粒=墨色不匀（干笔擦）。
    land_grain: true,
    // 宣纸上墨必然洇——群系边界 2 格晕开。
    watercolor_bleed: true,
    // 关：浅滩带关闭（与 `shore_band_cells: 0` 双保险）。
    shore_shallow: false,
};

pub static PACK: ThemePack = ThemePack {
    id: "chinese_ink",
    display_name: "水墨",
    palette: INK_PALETTE, // 水墨色板（美术定稿，方案 §3）
    treatment: INK_TREATMENT,
    atlas: None,
    // 海的画法配方（ADR-0048 · 美术定稿，方案 §5）：水墨包的海不画浅滩色带、
    // 不画浅滩段纸纹颗粒——留白是水墨的语法；离岸墨线波纹两圈（近浓远清的
    // 青墨）、贴岸一道连续不破碎的淡青墨皴痕，岸线上窄而硬的深青墨顿笔
    // （积墨环，band=0 时环心锁定岸线）。
    decor: DecorRecipe {
        spacing_px_edit: 40.0,
        spacing_px_region: 64.0,
        spacing_px_nation: 96.0,
        size_mul_edit: 0.8,
        size_mul_region: 0.65,
        size_mul_nation: 1.2,
        // 山符号不画（ADR-0058 D5）：宣纸底上那一排排具象黑三角与方案 §7 的
        // highland 留白裁定自相矛盾——山体表达收敛到等值线重墨 + 海岸顿笔
        // 积墨 + 海配方皴线 + 地形色阶，「不画」是水墨画法的正常态而非异常。
        mountain_style: SymbolStyle::None,
        // 变体墨色（顶点色）：压冷压深一档，与纸面拉开。
        conifer_ink: DecorVariantInk::new(0.14, 0.22, 0.17, 0.78),
        temperate_ink: DecorVariantInk::new(0.17, 0.22, 0.16, 0.75),
        oasis_ink: DecorVariantInk::new(0.24, 0.27, 0.18, 0.68),
        // 露岩=岩墨灰（水彩同位是亮赭，水墨必须去赭）。
        rock_ink: DecorVariantInk::new(0.38, 0.37, 0.34, 0.82),
        // 山背光面=淡墨重施（斧劈皴的阴面）。
        mountain_shade_ink: DecorVariantInk::new(0.18, 0.17, 0.15, 0.68),
        // 族墨色：雪=纸白族（去冷调），白弧=纸色（墨青海面上的「负空间」）。
        forest_ink: DecorInk::new([0x2A, 0x33, 0x28], 0.75),
        plains_ink: DecorInk::new([0x4A, 0x52, 0x40], 0.45),
        hills_ink: DecorInk::new([0x55, 0x50, 0x3E], 0.50),
        mountain_ink: DecorInk::new([0x33, 0x32, 0x2E], 0.60),
        snow_ink: DecorInk::new([0xEA, 0xE8, 0xDE], 0.80),
        marsh_ink: DecorInk::new([0x35, 0x44, 0x39], 0.55),
        desert_ink: DecorInk::new([0x6B, 0x5E, 0x46], 0.45),
        wave_ink: DecorInk::new([0xE6, 0xE3, 0xD8], 0.32),
        // 浅海白波带：收窄避开贴岸皴痕与积墨环——水墨近岸层次是
        // 「皴痕→空→墨线」，白弧只填空隙（方案 §6.5）。
        wave_band_min_cells: 4,
        wave_band_max_cells: 8,
        wave_density_span_cells: 4,
        // 「疏」的主杠杆：密度场整体 72 折——林间空地是留白在陆上的形态。
        forest_density_gain: 0.72,
        ..wv_theme::decor::WATERCOLOR
    },
    sea: SeaRecipe {
        depth_scale_m: 2600.0,
        depth_exponent: 0.65,
        shore_band_cells: 0.0,
        shore_peak: 0.0,
        water_grain: 0.028,
        ripple_warp_cells: 1.5,
        ripples: &[
            // 近岸第一道皴：稍近岸（避开 foam 带）、线稍锐、墨稍提——第一笔要「见笔」。
            RippleSpec {
                dist_cells: 4.5,
                sigma_cells: 0.65,
                alpha: 0.24,
            },
            // 远处第二道：清墨、线更糊（墨晕开）；两圈即止，远处是安静的水。
            RippleSpec {
                dist_cells: 10.0,
                sigma_cells: 0.85,
                alpha: 0.11,
            },
        ],
        // 波纹墨=青墨：与墨青海面同族（暖褐墨在青灰海上会脏）。
        ink: [0x3E, 0x4E, 0x54],
        // 水墨的浪花是「皴」不是「白浪」：一道贴岸的连续淡青墨痕，不破碎。
        foam: &[RippleSpec {
            dist_cells: 1.3,
            sigma_cells: 0.75,
            alpha: 0.15,
        }],
        foam_ink: [0x5C, 0x70, 0x78],
        foam_break: 0.0,
        foam_warp_cells: 1.2,
        foam_break_freq: 0.0,
        // 水墨的近岸不画排线（那是钢笔/铜版画的手法）——留白 + 一道淡墨皴线。
        shore_hatch: None,
        // 外缘积墨环（顿笔）：水墨的边缘更硬（墨是顿出来的，不是化开的）——
        // 强度略收防糊边、宽度更窄；band=0 时环心落在岸线本身。
        shore_edge_strength: 0.42,
        shore_edge_width_cells: 0.75,
        shore_edge_ink: Some([0x33, 0x45, 0x4E]),
    },
    // 覆盖物（ADR-0032 族键）：森林族「苔点」演绎——近看是点皴、远看融成
    // 淡墨林影（文人画「个字点/介字点」）；其余 7 族留白不画（平原/大漠/
    // 沼泽/雪原即留白；山由 DecorRecipe 皴法 + 等值线承担；海由 SeaRecipe
    // 演绎；道路由墨线色族演绎）。「不画」是显式零规则（INK_BLANK_RULE）
    // 而非缺省 None——渲染侧据此绕开暖绿回退晕，「留白」由主题自己裁定
    // （方案 §7 + §12.1，2026-10-25 源码勘察后落地）。
    overlays: FeatureOverlaySet {
        forest: Some(FeatureRule {
            halo_color: [0x3E, 0x4C, 0x3E],
            halo_alpha: 0.14,
            symbol_color: [0x2A, 0x34, 0x2A],
            symbol_size_px: 7.5,
            symbol_spacing_wu: 36.0,
            symbol_texture: None,
            halo_texture: None,
        }),
        grassland: Some(INK_BLANK_RULE),
        desert: Some(INK_BLANK_RULE),
        wetland: Some(INK_BLANK_RULE),
        cold: Some(INK_BLANK_RULE),
        highland: Some(INK_BLANK_RULE),
        water: Some(INK_BLANK_RULE),
        road: Some(INK_BLANK_RULE),
    },
    // 区域覆盖物键（ADR-0045 D10）：水墨版四档，墨色更沉、符号更小、间距更疏
    // （与地形装饰拉开密度差，避免同屏双份苔点）。键集与内置古舆图同款——
    // 换主题不应让作者的区域声明失配（那会画成缺失覆盖物标记）。
    region_overlays: RegionOverlaySet::new(&[
        (
            "conifer",
            RegionOverlayRule {
                variant: "conifer",
                color: [0x24, 0x30, 0x26],
                alpha: None,
                size_scale: 0.80,
                spacing_scale: 1.20,
            },
        ),
        (
            "broadleaf",
            RegionOverlayRule {
                variant: "broadleaf",
                color: [0x28, 0x33, 0x27],
                alpha: None,
                size_scale: 0.80,
                spacing_scale: 1.20,
            },
        ),
        (
            "sparse",
            RegionOverlayRule {
                variant: "sparse",
                color: [0x3C, 0x46, 0x36],
                alpha: None,
                size_scale: 0.80,
                spacing_scale: 1.25,
            },
        ),
        (
            "boulder_field",
            RegionOverlayRule {
                variant: "boulder",
                color: [0x5E, 0x5C, 0x54],
                alpha: Some(0.72),
                size_scale: 0.80,
                spacing_scale: 1.30,
            },
        ),
    ]),
};

/// 显式留白规则（方案 §7 裁定 + §12.1 源码勘察结论，2026-10-25）。
///
/// 给族规则但**晕 / 符号双零**：`wv-render` 的覆盖物渲染在
/// `halo_alpha = 0` 时不生成晕实体（`feature_overlays.rs` 的
/// `if halo_alpha > 0.0` 门），`symbol_spacing_wu = 0` 时 `forest_sites`
/// 返回空采样（`theme/overlays.rs` 的非正间距早退）——符号分支随之
/// `continue`。两者叠加 = 「本族不演绎」的显式声明。
///
/// **为什么不用 `None`**：族无规则时渲染走 `FALLBACK_HALO_COLOR`
/// （`[0x6A, 0x70, 0x5A]` 暖绿 + 0.12 alpha）回退晕——那是内置日间主题的
/// 中性色，画在宣纸底上是外来色。显式零规则把「留白」收归主题自己的裁定，
/// 顺带绕开回退分支，无需扩契约字段、无需改渲染层（ADR-0043 红线不破）。
/// `symbol_size_px = 0` 是第三道保险：即便未来间距语义变化，零尺寸符号
/// 也不可见。
const INK_BLANK_RULE: FeatureRule = FeatureRule {
    halo_color: [0x3E, 0x4C, 0x3E],
    halo_alpha: 0.0,
    symbol_color: [0x2A, 0x34, 0x2A],
    symbol_size_px: 0.0,
    symbol_spacing_wu: 0.0,
    symbol_texture: None,
    halo_texture: None,
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

    /// 展示名去掉「（占位）」——美术定稿后不再是占位包（方案 §13 步骤 6）。
    #[test]
    fn display_name_is_finalized() {
        assert_eq!(PACK.display_name, "水墨");
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

    /// 焦墨锚点：国名/大区标签（方案 §3.2 定稿 `#26241F`）。
    #[test]
    fn label_region_is_burnt_ink() {
        assert_eq!(
            PACK.palette.label_region,
            wv_theme::colors::hex(0x26241F),
            "label_region = 焦墨（全色板最深）"
        );
    }

    /// 第一道皴墨锚点：近岸波纹 alpha（方案 §5.4 定稿 0.24——第一笔要「见笔」）。
    #[test]
    fn first_ripple_alpha_is_calibrated() {
        assert!(
            (PACK.sea.ripples[0].alpha - 0.24).abs() < 1e-6,
            "第一道皴 = 近浓（0.24）"
        );
    }

    /// 「疏」的主杠杆锚点：森林密度增益（方案 §6.5 定稿 0.72）。
    #[test]
    fn forest_density_gain_is_calibrated() {
        assert!(
            (PACK.decor.forest_density_gain - 0.72).abs() < 1e-6,
            "林间空地是留白在陆上的形态"
        );
    }

    /// 山符号画法锚点（ADR-0058 D5）：水墨包显式 `None`——山体表达收敛到
    /// 等值线重墨 + 海岸顿笔积墨，「不画」是水墨画法的正常态。
    ///
    /// 防回归：有人删掉该字段回退 `Default`（内置水彩三角）——宣纸上那列
    /// 具象黑三角会随 `..wv_theme::decor::WATERCOLOR` 继承回来。
    #[test]
    fn pack_mountain_style_is_none() {
        assert_eq!(
            PACK.decor.mountain_style,
            wv_theme::decor::SymbolStyle::None,
            "水墨山符号应不画（ADR-0058 D5）"
        );
    }

    /// 色板占位期已结束（2026-10-21 起），核心槽位锚定水墨值：
    /// 海两槽来自 civ-atlas 的水墨海色，纸色是宣纸底。
    #[test]
    fn pack_palette_is_the_ink_palette_not_daylight() {
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

    /// 水墨包也装载森林族覆盖规则——验证外部主题 crate 在族键 API
    /// （ADR-0032）下 `overlays` 规则通路成立（族类型经 wv-render 再导出）。
    #[test]
    fn pack_has_forest_overlay_rule() {
        let rule = PACK
            .overlays
            .rule_for(wv_theme::overlays::BiomeFamily::Forest)
            .expect("水墨包应给森林族配置苔点规则");
        // v1 纯色路径锁定（方案 §9）：贴图装载通道转待裁（§12.3），
        // 避免把日间古舆图图集错配到水墨主题。
        assert!(
            rule.symbol_texture.is_none() && rule.halo_texture.is_none(),
            "水墨包 v1 留纯色路径（贴图待 v2）"
        );
    }

    /// 显式留白锚点（方案 §12.1 数据层解法，2026-10-25）：7 个非森林族
    /// 全部配置零规则——`halo_alpha = 0` 令渲染不生成晕实体、
    /// `symbol_spacing_wu = 0` 令 `forest_sites` 空采样不画符号。
    ///
    /// 这条锚点防两种回归：① 有人把族改回 `None`（暖绿回退晕会重新
    /// 出现在宣纸上）；② 有人给零规则填了非零浓度（留白变成淡色块）。
    #[test]
    fn blank_families_are_explicitly_declared_as_zero() {
        use wv_theme::overlays::BiomeFamily;
        let families = [
            BiomeFamily::Grassland,
            BiomeFamily::Desert,
            BiomeFamily::Wetland,
            BiomeFamily::Cold,
            BiomeFamily::Highland,
            BiomeFamily::Water,
            BiomeFamily::Road,
        ];
        for family in families {
            let rule = PACK
                .overlays
                .rule_for(family)
                .expect("留白族应有显式零规则（绕开渲染层回退晕）");
            assert_eq!(rule.halo_alpha, 0.0, "晕浓度应为 0（不画晕）");
            assert_eq!(rule.symbol_spacing_wu, 0.0, "符号间距应为 0（空采样）");
            assert_eq!(rule.symbol_size_px, 0.0, "符号尺寸应为 0（第三道保险）");
        }
    }
}
