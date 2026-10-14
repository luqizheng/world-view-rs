//! # wv-app::theme —— 外观令牌与字体
//!
//! 应用层全部「画什么颜色、用什么字体」的权威来源。**不**做渲染。
//!
//! ## 职责
//! - 28 色板令牌（`colors`，权威来自 `ART_STYLE_SPEC_v0.1` §1.4）
//! - Google Default Map 色板（`colors_google`）
//! - JRPG 风格色板（`colors_jrpg`）
//! - 界面外壳色令牌（`ui_tokens`，权威来自 `UI_DESIGN_SPEC` §2.2）
//! - 把 `ui_tokens` 投影成 egui `Visuals`（`ui_theme`）
//! - 系统字体栈探测 + CJK 字体注册（`fonts`）
//!
//! ## 边界（不做什么）
//! - **不**做 Bevy 渲染。所有内容是纯数据 / 资源构造器。
//! - **不**保存用户偏好。`ui-layout.yaml` 的持久化在 `crate::ui_layout`。
//!
//! ## 主要导出
//! - `BASE` —— 来自 `colors`（28 色板）
//! - `Theme` —— 来自 `ui_theme`（egui 主题）
//! - `load_system_fonts` —— 来自 `fonts`

#[cfg(feature = "app")]
pub mod colors;
#[cfg(feature = "app")]
pub mod colors_google;
#[cfg(feature = "app")]
pub mod colors_jrpg;
#[cfg(feature = "app")]
pub mod fonts;
#[cfg(feature = "app")]
pub mod ui_theme;
#[cfg(feature = "app")]
pub mod ui_tokens;
