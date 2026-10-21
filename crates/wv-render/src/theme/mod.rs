//! # wv-render::theme —— 渲染侧对主题层的**消费面**（ADR-0043）
//!
//! **主题的数据与契约已搬到独立 crate [`wv_theme`]**——主题是内容不是机制。
//! 本模块保留两类东西：
//!
//! 1. **重导出**（`pub use wv_theme::…`）——既有调用点路径
//!    （`crate::theme::palette::colors(…)`、`crate::theme::colors::X`、
//!    `crate::theme::overlays::{FeatureRule, …}`）**一行不改**；
//! 2. **渲染运行时**——[`fonts`]（字体装配，消费方是本 crate 的 `Text2d`
//!    标签系统 ×7）与 [`overlays`] 的采样 / 分带函数（`forest_sites` /
//!    `overlay_far_blend`，需要高度场与视口）。
//!
//! ## 边界
//! - 本 crate **不持有**任何色板数据、装饰旗标定义、主题包契约；
//! - 外部主题 crate 只依赖 `wv_theme`，**不依赖 `wv-render`**（ADR-0043 D2）。
//!
//! ## cfg 门
//!
//! **全部子模块在 `app` feature 之下**——`wv-theme` 依赖 `bevy`（非 optional，
//! 同 wv-ui 的 ADR-0026 D2 口径），而本 crate 的红线是「不带 feature 时零
//! Bevy 依赖树」（ADR-0020）。故 `wv-theme` 是 optional 依赖，随 `app` 一起
//! 打开。代价：`theme::topographic` 由「无 cfg 门」改为有门（ADR-0043 追记）。
//!
//! **2026-10-20 合并追记**：合并 `feat/cell-selection` 后本模块**已有实际
//! 消费点**——`terrain_view::cells`（Topographic per-cell 着色）与
//! `cell_selection`（选中高亮）都经 `crate::theme::topographic::pick` 取色。
//! 两者都在 `app` feature 之下，与本模块的 cfg 门一致，门的位置无碍；
//! 此处修正 ADR-0043 追记里「无实际消费点」的表述。

// 数据与契约（ADR-0043 D3：重导出，路径零变更）。
#[cfg(feature = "app")]
pub use wv_theme::colors;
#[cfg(feature = "app")]
pub use wv_theme::colors_dusk;
#[cfg(feature = "app")]
pub use wv_theme::colors_google;
#[cfg(feature = "app")]
pub use wv_theme::palette;
#[cfg(feature = "app")]
pub use wv_theme::registry;
#[cfg(feature = "app")]
pub use wv_theme::sea;
#[cfg(feature = "app")]
pub use wv_theme::theme_pack;

// Topographic 调色（[ADR-0039]）——纯数据。
//
// [ADR-0039]: ../../../docs/adr/ADR-0039-topographic-viewmode.md
#[cfg(feature = "app")]
pub use wv_theme::topographic;

// 渲染运行时（留在本 crate，ADR-0043 D4 / D5）。
#[cfg(feature = "app")]
pub mod fonts;
#[cfg(feature = "app")]
pub mod overlays;
