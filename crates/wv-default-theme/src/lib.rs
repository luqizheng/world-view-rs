//! # wv-default-theme —— World View 的**默认地图风格**
//!
//! **一句话**：本 crate 持有地图「长什么样」的**全部数据**；「怎么画」留在
//! `wv-render`（画、采样、视口裁剪）。
//!
//! ## 定位（ADR-0066）
//!
//! 本 crate 取代原 `wv-theme`（+ `wv-theme-chinese-ink`）。变化有三处：
//!
//! 1. **多主题机制退役**：主理人 2026-10-25 裁定「软件不需要做 theme 的
//!    贴图功能」——艺术主题（水彩 / 暮色 / 水墨）整族删除，只留**一个**默认
//!    风格。随之删除的还有主题注册表（`registry`）与外部主题包机制。
//! 2. **默认风格 = 专业制图**：色板取百度 / Google 普通地图那套（近白陆地 +
//!    淡水 + 公园绿 + 橙黄分级道路 + 分类 POI），`colors_google` 是本 crate
//!    的主色表。
//! 3. **名字不带「theme」**：`ThemePack` 这套「可插拔风格包」契约已随多主题
//!    机制一起简化——现在只有一个默认包，调用方直接读常量。
//!
//! ## 职责
//! - [`colors_google`] —— **专业制图色板**（主色表）：地形十色、道路三档、
//!   POI 分类色、注记色、海岸线色、边界色；
//! - [`colors`] —— 主题**不变**的语义色与编辑器色（派系 / 语义色 / 强调色 /
//!   描线三档 / 网格 / 建筑）；
//! - [`palette`] —— [`PaletteColors`]（36 槽色板结构）与 [`MapPalette`]（配色
//!   轴枚举，现为**单值**）；
//! - [`theme_pack`] —— 风格包契约（[`ThemePack`] / [`TreatmentFlags`]）：一个
//!   默认包 `builtin::DEFAULT`；
//! - [`decor`] —— 地形装饰画法配方 `DecorRecipe`（ADR-0048）；
//! - [`sea`] —— 海的画法配方 `SeaRecipe`（ADR-0048）；
//! - [`overlays`] —— 覆盖物**数据契约**：`FeatureRule` / `FeatureOverlaySet` /
//!   `RegionOverlaySet`；
//! - [`topographic`] —— Topographic 视图模式的调色数据。
//!
//! ## 边界（不做什么）
//! - **不画任何东西**：零 ECS 访问、零渲染系统；
//! - **不采样**：树冠落位与镜头分带是渲染运行时，留在 `wv-render`；
//! - **不装字体**：字体装配留在渲染侧（`Text2d` 标签系统）；
//! - **不依赖任何兄弟 crate**（除 `wv-terrain` 这一纯数据叶子）。
//!
//! ## 与 `wv-world-cell-map` 的关系
//!
//! 后者**不依赖**本 crate（ADR-0054 D4 固定色禁令）：它是数据视图工作台，
//! 用自己的 `CellPalette` 平面镜像。本 crate 的 `ThemePack → CellPack` 换算
//! 在 `wv-render` 侧（唯一双向桥接点）。

/// `bevy::color::Color` 的重导出——构造色板时需要。
pub use bevy::color::Color;

pub mod colors;
pub mod colors_google;
pub mod decor;
pub mod overlays;
pub mod palette;
pub mod sea;
pub mod theme_pack;
pub mod topographic;
