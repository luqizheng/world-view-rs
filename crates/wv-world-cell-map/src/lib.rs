//! 世界级地形工作台：CellMap 的渲染与编辑域（ADR-0054）。
//!
//! ## 定位
//! CellMap 工作区（[ADR-0052]）的域 crate——只关心地形地貌、POI、河流，
//! 对事件、事态、人物不关心。唯一目标：把作者想表达的地形形状
//! 快、准、可后悔地落进 `terrain.bin`（编辑链路仍走 wv-app 的
//! `EditCommand → wv-edit`，本 crate 只读世界、只写渲染产物）。
//!
//! ## 固定色 crate 级禁令（ADR-0054 D2/D4）
//! 本 crate **不依赖任何 theme crate**。色值是数据：[`palette::CellPalette`] /
//! [`palette::TreatmentFlags`] / [`palette::SeaRecipe`] 是 wv-theme 对应类型
//! 的**平面镜像**（同名字段），默认源 = [`palette::CellPack::FIXTURE`]
//! （冻结的 Daylight 内置色 + 手绘旗标 + 水彩海配方）。
//! `ThemePack → CellPack` 换算与 `ViewMode → ShadeMode` 映射在 wv-render 侧
//! （只有它许依赖 wv-theme）；主题浏览态经同一套纯函数消费自己的色包。
//!
//! ## 边界（不做什么）
//! - **不读 wv-render 的呈现态**（`ViewState` / `WorldProjection` /
//!   `DrillState`）——需要这些量的系统（视图重建 / 等值线开关 / 选区同步…）
//!   留在 wv-render 的 `cellmap_bridge`，经纯函数入参把平面数据带进来。
//! - **不写世界**——无 `wv-edit` 依赖，编辑意图一律上抛。
//! - **不做相机 / 钻取 / Z 槽位编排**——那是 wv-render 的呈现基础设施。
//!
//! ## 模块
//! - [`palette`]——冻结色包与常量（固定色的唯一取值点）。
//! - [`state`]——CellMap 会话态（`EditCellState` / **`CellSelection` 统一选区，
//!   ADR-0057** / `GridConfig`，自 wv-render 迁入；wv-render 侧 re-export
//!   保旧路径）。
//! - [`terrain_view`]——地形纹理合成（CPU 烘焙回退）+ GPU 场管线（field/）
//!   + 网格线 / 等值线 / 大气 / 图集 / 群系固定色。
//! - [`terrain_chunks`]——分块 + mip 链 + 视口拣选。
//! - [`cell_selection`]——选区栅格化与高亮。
//! - [`cell_paint`]——单格连续绘制（落笔值 / 笔触态 / Bresenham 补间）。
//! - [`cell_flash`]——撤销白闪 / 拒收红闪（短时闪视觉会话态）。
//! - [`region_overlay`]——自然区覆盖（固定诊断色）。
//! - [`biome_boundary`]——群系分界线工具的几何内核。
//!
//! [ADR-0052]: ../docs/adr/ADR-0052-cellmap-terrain-workbench.md
//! [ADR-0054]: ../docs/adr/ADR-0054-world-cell-map-crate.md

pub mod biome_boundary;
pub mod cell_flash;
pub mod cell_paint;
pub mod cell_selection;
pub mod palette;
pub mod region_overlay;
pub mod shade;
pub mod state;
pub mod terrain_chunks;
pub mod terrain_view;
