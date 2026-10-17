//! # wv-theme-chinese-ink —— 示例主题包（ADR-0023 M2）
//!
//! **目的**：演示"加新风格 = 加新 crate、零改 wv-render 源码"的注入契约。
//! **色值留空 / 占位**：本 crate 的 `ThemePack::palette` 全部字段**复用内置古舆图
//! 槽位**，仅换 `id` / `display_name` / `treatment`（示意）；色值校准待美术侧提供。
//!
//! ## 加载方式（游戏 app 视角）
//!
//! ```ignore
//! use wv_render::theme::registry::{MergedThemeRegistry, ThemePackRegistry};
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

#![cfg(feature = "app")]

use wv_render::theme::overlays::{FeatureOverlaySet, FeatureRule};
use wv_render::theme::palette::PaletteColors;
use wv_render::theme::registry::ThemePackRegistry;
use wv_render::theme::theme_pack::{ThemePack, TreatmentFlags};

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
    },
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
    use wv_render::theme::palette::PaletteColors;
    use wv_render::theme::registry::MergedThemeRegistry;

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
                wv_render::theme::theme_pack::builtin::ALL
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

    /// 水墨包也装载森林覆盖规则（占位值）——验证外部主题 crate 加 `overlays`
    /// 字段后规则通路仍成立。
    #[test]
    fn pack_has_forest_overlay_rule() {
        assert!(
            PACK.overlays
                .rule_for(wv_render::theme::overlays::FEATURE_FOREST)
                .is_some(),
            "水墨包应给 FEATURE_FOREST 配置占位规则"
        );
    }
}
