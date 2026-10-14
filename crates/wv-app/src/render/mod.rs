//! # wv-app::render —— 地图渲染与视图
//!
//! 把 `wv-core` 的世界投影到屏幕上的一层。Bevy 系统、资源、组件的**渲染**侧装配点。
//!
//! ## 职责
//! - 相机（缩放 / 平移 / 笔刷拾取）
//! - 地图外框 + 分级缩放区间
//! - 地形底图（色块 / 真实地形 / LOD tile atlas）
//! - 矢量层（道路 casing / fill / POI 红点白环）
//! - 势力多边形叠加 + 笔刷光标 + 地图三级钻取
//! - 图层帧（`MapNode.layers[].z/visible/opacity` → 渲染绑定的唯一求值点，ADR-0013）
//!
//! ## 边界（不做什么）
//! - **不**改世界数据。修改一律经 `crate::edit::*`。
//! - **不**做时间比较。任何「某时刻的状态」经 `wv_core::time::TimelineQuery::at`。
//! - **不**持有 UI 外壳/面板（那是 `crate::ui::*` 的事）。
//! - **不**持有外观令牌（那是 `crate::theme::*` 的事）。
//!
//! ## 主要导出
//! - `WorldProjection`（时间投影）—— 来自 `world_projection`
//! - `TerrainTexture` / `TileAtlas` —— 来自 `terrain_view`
//! - `draw_vector_layers` —— 来自 `vector_layers`

#[cfg(feature = "app")]
pub mod brush_cursor;
#[cfg(feature = "app")]
pub mod buildings;
#[cfg(feature = "app")]
pub mod camera;
#[cfg(feature = "app")]
pub mod drill;
#[cfg(feature = "app")]
pub mod faction_overlays;
#[cfg(feature = "app")]
pub mod label_halo;
#[cfg(feature = "app")]
pub mod layer_frame;
#[cfg(feature = "app")]
pub mod map_frame;
#[cfg(feature = "app")]
pub mod measurement;
#[cfg(feature = "app")]
pub mod streets;
#[cfg(feature = "app")]
pub mod terrain_chunks;
#[cfg(feature = "app")]
pub mod terrain_view;
#[cfg(feature = "app")]
pub mod vector_layers;
#[cfg(feature = "app")]
pub mod world_projection;
#[cfg(feature = "app")]
pub mod zoom_level;
