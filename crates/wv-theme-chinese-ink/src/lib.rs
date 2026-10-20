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

use wv_theme::overlays::{FeatureOverlaySet, FeatureRule, RegionOverlayRule, RegionOverlaySet};
use wv_theme::palette::PaletteColors;
use wv_theme::registry::ThemePackRegistry;
use wv_theme::theme_pack::{ThemePack, TreatmentFlags};

/// 水墨主题包占位。
///
/// **色值与内置古舆图完全相同**（M2 仅做契约演示，不引入未经校准的色值）——
/// 游戏接入后视觉上与"日间古舆图"无差异，但 id 路径走的是 `chinese_ink`，
/// 验证"ThemePackRegistry lookup + per-pack rendering"通路。
///
/// M2 验收：装载后报"色值未校准"告警 + 兜底到内置默认——本 M2 阶段**不**实现
/// 告警路径（M2 验收点见 ADR-0023 §6.2 §5）；色值校准时由美术侧提供白名单
/// 槽位值后逐项替换。
pub static PACK: ThemePack = ThemePack {
    id: "chinese_ink",
    display_name: "水墨（占位）",
    palette: PaletteColors::DAYLIGHT, // TODO(美术校准): 待白名单值就绪后替换
    treatment: TreatmentFlags::EMPTY,
    atlas: None,
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
    fn pack_palette_is_daylight_placeholder() {
        assert_eq!(PACK.palette.core, PaletteColors::DAYLIGHT.core);
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
