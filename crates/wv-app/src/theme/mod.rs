//! # wv-app::theme —— 外观令牌与字体
//!
//! 应用层全部「画什么颜色、用什么字体」的权威来源。**不**做渲染。
//!
//! ## 职责
//! - 28 色板令牌（`colors`，权威来自 `ART_STYLE_SPEC_v0.1` §1.4；ADR-0016 起
//!   承载「日间古舆图」默认主题定稿值）
//! - 暮色旧纸色板（`colors_dusk`，`colors` 的同槽位暗调镜像，ADR-0016）
//! - 地图主题运行时分发（`palette`：`MapPalette` + `PaletteColors`，ADR-0016）
//! - Google Default Map 色板（`colors_google`）
//! - 界面外壳色令牌（`ui_tokens`，权威来自 `UI_DESIGN_SPEC` §2.2）
//! - 把 `ui_tokens` 投影成 egui `Visuals`（`ui_theme`）
//! - 打包字体装配：Inter 拉丁 + Noto Sans SC 中文（`fonts`，ADR-0019）
//!
//! ## 边界（不做什么）
//! - **不**做 Bevy 渲染。所有内容是纯数据 / 资源构造器。
//! - **不**保存用户偏好。`ui-layout.yaml` 的持久化在 `crate::ui_layout`。
//!
//! ## 主要导出
//! - `BASE` —— 来自 `colors`（28 色板）
//! - `Theme` —— 来自 `ui_theme`（egui 主题）
//! - `setup_egui_fonts` —— 来自 `fonts`

#[cfg(feature = "app")]
pub mod colors;
#[cfg(feature = "app")]
pub mod colors_dusk;
#[cfg(feature = "app")]
pub mod colors_google;
#[cfg(feature = "app")]
pub mod fonts;
#[cfg(feature = "app")]
pub mod palette;
#[cfg(feature = "app")]
pub mod ui_theme;
#[cfg(feature = "app")]
pub mod ui_tokens;
