//! # wv-theme —— 地图主题层：色板 + 装饰旗标 + 覆盖物规则 + 主题包契约
//!
//! **一句话**：主题是**内容**，不是**机制**——本 crate 持有前者，后者留在
//! `wv-render`（画、采样、视口裁剪）。
//!
//! ## 职责（ADR-0043）
//! - [`palette`] —— 地图主题运行时分发：`MapPalette`（配色轴）+ `PaletteColors`
//!   （36 槽色板）；
//! - [`colors`] / [`colors_dusk`] / [`colors_google`] —— 内置色板数据
//!   （日间 / 暮色 / Google 普通地图）；
//! - [`overlays`] —— 覆盖物**数据契约**：`FeatureRule` / `FeatureOverlaySet`
//!   （外部主题 crate 的构造面，ADR-0023 D2）；
//! - [`theme_pack`] —— 主题包契约：`ThemePack` / `TreatmentFlags` /
//!   `TileAtlasSpec`；
//! - [`registry`] —— 主题注册表：`ThemePackRegistry` / `MergedThemeRegistry`；
//! - [`decor`] —— 地形装饰的画法配方 `DecorRecipe`（高度阈值 / 气候带 /
//!   墨色 / LOD 步距，ADR-0048 追记二）：ADR-0039 D1 挂账的「随 ADR-0023 M3
//!   迁 ThemePack」在本次补上——**数值配方住主题层，执行器在渲染层**；
//! - [`sea`] —— 海的画法配方 `SeaRecipe`（深度量程 / 浅滩带 / 纸纹 / 墨线波纹，
//!   ADR-0048）：与 [`overlays`] 同构——数值配方住主题层，执行器在渲染层；
//! - [`topographic`] —— Topographic 视图模式的调色数据。
//!
//! ## 边界（不做什么）
//! - **不画任何东西**：零 ECS 访问、零渲染系统；
//! - **不采样**：树冠落位（`forest_sites`）与镜头分带（`overlay_far_blend`）
//!   是渲染运行时，留在 `wv-render`（ADR-0043 D4）；
//! - **不装字体**：字体装配的消费方是 `wv-render` 的 `Text2d` 标签系统
//!   （ADR-0019 D2 落地注记），留在渲染侧；
//! - **不依赖任何兄弟 crate**（除 `wv-terrain` 这一纯数据叶子）：主题数据
//!   不承载世界数据（P4）。
//!
//! ## 用法（外部主题 crate）
//!
//! ```ignore
//! use wv_theme::overlays::{FeatureOverlaySet, FeatureRule};
//! use wv_theme::registry::ThemePackRegistry;
//! use wv_theme::theme_pack::{ThemePack, TreatmentFlags};
//!
//! pub static PACK: ThemePack = ThemePack { /* … */ };
//! ```
//!
//! 主题 crate 只依赖 `wv-theme`，**不依赖 `wv-render`**（ADR-0043 D2）——
//! 这正是「加新风格 = 加新 crate、零改渲染层源码」得以成立的前提。

pub mod colors;
pub mod colors_dusk;
pub mod colors_google;
pub mod decor;
pub mod overlays;
pub mod palette;
pub mod registry;
pub mod sea;
pub mod theme_pack;
pub mod topographic;
