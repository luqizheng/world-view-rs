//! # wv-render::theme —— 地图渲染配色（ADR-0020 自 wv-app::theme 迁入）
//!
//! 渲染侧「画什么颜色」的权威来源。**不**做渲染，也**不**管 UI 调色
//! （UI 外壳色令牌 `ui_tokens` / egui 主题 `ui_theme` / 打包字体 `fonts`
//! 留在 `wv-app::theme`，ADR-0019）。
//!
//! ## 职责
//! - 28 色板令牌（`colors`，权威来自 `ART_STYLE_SPEC_v0.1` §1.4；ADR-0016 起
//!   承载「日间古舆图」默认主题定稿值）
//! - 暮色旧纸色板（`colors_dusk`，`colors` 的同槽位暗调镜像，ADR-0016）
//! - 地图主题运行时分发（`palette`：`MapPalette` + `PaletteColors`，ADR-0016）
//! - Google Default Map 色板（`colors_google`）
//! - 覆盖物演绎规则（`overlays`：feature 规则、镜头分带淡化、近带确定性采样）
//!
//! - 打包字体装配（`fonts`，ADR-0019；`LoadedFonts` 消费方是本 crate 的
//!   Text2d 标签系统 ×7，故随渲染走——ADR-0020 落地注记）
//!
//! `wv-app::theme` 对以上五件做 `pub use` 重导出——UI 侧既有
//! `crate::theme::colors::X` / `crate::theme::fonts::setup_egui_fonts`
//! 路径零改动（ADR-0020 §2.6）。

#[cfg(feature = "app")]
pub mod colors;
#[cfg(feature = "app")]
pub mod colors_dusk;
#[cfg(feature = "app")]
pub mod colors_google;
#[cfg(feature = "app")]
pub mod fonts;
#[cfg(feature = "app")]
pub mod overlays;
#[cfg(feature = "app")]
pub mod palette;
#[cfg(feature = "app")]
pub mod registry;
#[cfg(feature = "app")]
pub mod theme_pack;
