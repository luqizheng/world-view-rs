//! # wv-app::theme —— 外观令牌与字体（ADR-0020 拆分后的 UI 侧）
//!
//! UI 外壳「画什么颜色、用什么字体」的权威来源。**不**做渲染。
//!
//! ## 职责（ADR-0020 拆分后）
//! - **重导出**地图渲染配色四件（`colors` / `colors_dusk` / `colors_google` /
//!   `palette`，本体在 `wv_render::theme`）——UI 侧既有
//!   `crate::theme::colors::X` 路径零改动（ADR-0020 §2.6；`ui_tokens` /
//!   `top_bar` / `list_panels_panel` 等都直接消费它们）。
//! - **重导出**打包字体装配（`fonts`，本体在 wv-render——`LoadedFonts` 是
//!   Text2d 标签的资源、写路径不可拆，ADR-0020 落地注记）
//! - 界面外壳色令牌（`ui_tokens`，权威来自 `UI_DESIGN_SPEC` §2.2）
//! - 把 `ui_tokens` 投影成 egui `Visuals`（`ui_theme`）
//! - 打包字体装配：Inter 拉丁 + Noto Sans SC 中文（`fonts`，ADR-0019）
//!
//! ## 边界（不做什么）
//! - **不**做 Bevy 渲染。所有内容是纯数据 / 资源构造器。
//! - **不**保存用户偏好。`ui-layout.yaml` 的持久化在 `crate::ui_layout`。
//! - **不**持有渲染配色本体——那在 `wv_render::theme`（ADR-0020）。
//!
//! ## 主要导出
//! - `BASE`（重导出）—— 28 色板令牌
//! - `Theme` —— 来自 `ui_theme`（egui 主题）
//! - `setup_egui_fonts` —— 来自 `fonts`

// ADR-0026：ui_tokens / ui_theme 本体迁 `wv-ui`；此处 re-export 保持
// `crate::theme::ui_tokens::X` 等全部旧路径零改动。
#[cfg(feature = "app")]
pub use wv_ui::{ui_theme, ui_tokens};

// 渲染配色四件：本体在 wv-render（ADR-0020），这里 `pub use` 保持
// `crate::theme::colors::*` 等既有路径对全部 UI 代码继续可达。
pub use wv_render::theme::{colors, colors_dusk, colors_google, fonts, palette};

#[cfg(all(test, feature = "app"))]
mod wv_ui_world_color_parity_tests {
    //! 徽标底色 × 世界色板一致性守卫（§4.7「同色相」）。
    //!
    //! 原 `ui_tokens` 内置测试；ADR-0026 D4 随域移到 app 侧——断言对象改为
    //! `wv_ui::ui_tokens` × `crate::theme::colors`（wv_render re-export），
    //! `wv-ui` 本体保持零世界色板依赖。

    use wv_ui::ui_tokens::{BadgeTone, Rgb, ShellPalette};

    /// `bevy::Color` → 令牌层 `Rgb`（原 ui_tokens 测试 helper）。
    fn world_rgb(c: bevy::prelude::Color) -> Rgb {
        let [r, g, b, _] = crate::theme::colors::palette_to_color32(c).to_srgba_unmultiplied();
        Rgb {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
        }
    }

    #[test]
    fn badge_fills_mirror_the_world_link_palette() {
        let p = ShellPalette::DARK;
        assert_eq!(
            p.badge(BadgeTone::Ok).fill,
            world_rgb(crate::theme::colors::UI_REACHABLE)
        );
        assert_eq!(
            p.badge(BadgeTone::Warn).fill,
            world_rgb(crate::theme::colors::UI_LATE)
        );
        assert_eq!(
            p.badge(BadgeTone::Err).fill,
            world_rgb(crate::theme::colors::UI_UNREACHABLE)
        );
        assert_eq!(
            p.badge(BadgeTone::Event).fill,
            world_rgb(crate::theme::colors::ACCENT_EVENT)
        );
        // 底色恒定，只有前景随主题走。
        assert_eq!(
            p.badge(BadgeTone::Ok).fill,
            ShellPalette::LIGHT.badge(BadgeTone::Ok).fill
        );
    }
}
