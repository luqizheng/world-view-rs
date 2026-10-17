//! # wv-render —— 地图渲染与视图（独立渲染层 crate，ADR-0020）
//!
//! 把 `wv-core` 的世界投影到屏幕上的一层。Bevy 系统、资源、组件的**渲染**侧实现，
//! 自 wv-app 的 `render/` + `theme/{colors,colors_dusk,colors_google,palette}` 整建制
//! 迁出（ADR-0020）。与 `wv-edit` 互为双子——前者写、后者读，**互不依赖**，
//! 由 `wv-app` 同时装配。
//!
//! ## 职责
//! - 相机（缩放 / 平移 / 笔刷拾取）
//! - 地图外框 + 分级缩放区间
//! - 地形底图（色块 / 真实地形 / LOD tile atlas / 等值线矢量平涂）
//! - 矢量层（道路 casing / fill / POI 红点白环）
//! - 势力多边形叠加 + 笔刷光标 + 地图三级钻取
//! - 图层帧（`MapNode.layers[].z/visible/opacity` → 渲染绑定的唯一求值点，ADR-0013）
//! - 渲染配色（古舆图 / 暮色旧纸 / Google 色板，ADR-0016）
//! - 视图状态容器（`ViewState` / `WorldProjection` / `DrillState` 等，自 wv-app::state 迁入）
//!
//! ## 边界（不做什么）
//! - **不**改世界数据。编辑意图一律走 [`editing_bridge`] 发给 `wv-app`，
//!   由应用层翻译成 `wv-edit` 的 `EditCommand` 提交（ADR-0020 §2.5）。
//! - **不**依赖 `wv-edit` / `wv-app`（AGENTS §2.1 分层红线，§2.2 有 `cargo tree` 断言）。
//! - **不**做时间比较。任何「某时刻的状态」经 `wv_core::time::TimelineQuery::at`。
//! - **不**持有 UI 外壳/面板与 egui 令牌（那是 `wv-app` 的事；渲染配色在本 crate
//!   `theme`，UI 调色在 `wv-app::theme`）。
//!
//! ## i18n 状态（ADR-0018 §6.5）
//!
//! 渲染层无用户可见中文文案：测量叠加只显示数字 + 通用单位（`wu` / `km`），
//! 街名/标签/钻取均用 `Place` / `Region` 等域数据自带的名字。文件里出现的
//! `bevy::log::warn!` 是开发者日志（AGENTS §8 风格约定：中文），不进入 UI。
//!
//! ## 主要导出
//! - `state::*` —— `ViewState` / `WorldProjection` / `DrillState` / `GridConfig` 等视图状态
//! - `theme::palette` —— `MapPalette` + 日间/暮色色板（ADR-0016）
//! - `editing_bridge` —— 编辑意图 / 回执（渲染层不直接碰 `wv-edit`）
//! - `TerrainTexture` / `TileAtlas` —— 来自 `terrain_view`
//! - `draw_vector_layers` —— 来自 `vector_layers`

#[cfg(feature = "app")]
pub mod brush_cursor;
#[cfg(feature = "app")]
pub mod buildings;
#[cfg(feature = "app")]
pub mod camera;
#[cfg(feature = "app")]
pub mod continent_sketch;
#[cfg(feature = "app")]
pub mod drill;
#[cfg(feature = "app")]
pub mod editing_bridge;
#[cfg(feature = "app")]
pub mod faction_overlays;
#[cfg(feature = "app")]
pub mod label_collision;
#[cfg(feature = "app")]
pub mod label_halo;
#[cfg(feature = "app")]
pub mod label_pick;
#[cfg(feature = "app")]
pub mod layer_frame;
#[cfg(feature = "app")]
pub mod map_frame;
#[cfg(feature = "app")]
pub mod measurement;
#[cfg(feature = "app")]
pub mod plugins;
#[cfg(feature = "app")]
pub mod recipe;
#[cfg(feature = "app")]
pub mod selection;
#[cfg(feature = "app")]
pub mod state;
#[cfg(feature = "app")]
pub mod streets;
#[cfg(feature = "app")]
pub mod terrain_chunks;
#[cfg(feature = "app")]
pub mod terrain_view;
#[cfg(feature = "app")]
pub mod theme;
#[cfg(feature = "app")]
pub mod vector_layers;
#[cfg(feature = "app")]
pub mod world_projection;
#[cfg(feature = "app")]
pub mod zoom_level;
