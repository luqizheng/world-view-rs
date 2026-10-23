//! 地形场渲染管线（ADR-0053 D1/D2）——GPU 逐像素求值的数据面与执行器。
//!
//! ## 这是什么
//!
//! 现行底图走 **CPU 整图烘焙**：`terrain_view::cells` 把 `TerrainData` 合成
//! 一张 RGBA8 纹理，分辨率被 `tile_res_for()` 钉死（1024 图 = 4 px/格）。
//! 近观放大 8 倍即见像素块，远观 1.56 px/格时每格不足两像素——**位图分辨率
//! 上限是架构问题，调参修不动**（ADR-0053 §1 诊断）。
//!
//! 本模块改 **GPU 逐像素求值**：CPU 只打包三张数据纹理（每格一 texel，
//! 共 8 MB），颜色/明暗/墨线全部在 fragment shader 里按屏幕像素求值。
//! 分辨率上限因此消失。
//!
//! ## 三层分工
//!
//! | 子模块 | 层 | 职责 | 依赖 |
//! |---|---|---|---|
//! | [`coast_sdf`] | 数据 | 海岸距离场（Felzenszwalb EDT） | 纯 Rust |
//! | [`pack`] | 数据 | 三张数据纹理打包（height/coast/attr） | 纯 Rust |
//! | [`material`] | 执行 | `TerrainFieldMaterial` + WGSL 逐像素求值 | Bevy |
//!
//! **数据层不引入 Bevy**：`coast_sdf` 与 `pack` 都是纯函数，能在
//! `cargo test --workspace`（不带 feature）下被测试——AGENTS.md §2.3
//! 「数据层必须能独立测试」的红线。`material` 是唯一的 Bevy 触点。
//!
//! ## 规格出处
//!
//! `docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md`（P0 规格锁，53 项常量 T1–T53）。
//! 本目录落地 T1–T6（数据面）与 T7–T18 + T23/T24/T27（P1 求值面）；
//! T19–T22 域扭曲 / T28–T30 水彩 / T31–T34 缩放自适应属 **P2**，参数位已预留。

pub mod coast_sdf;
pub mod material;
pub mod pack;

pub use coast_sdf::{distance_field, distance_to_coast, FAR};
pub use material::{
    build_material, default_params, FieldPalette, FieldParams, TerrainFieldMaterial,
    TerrainFieldMaterialPlugin, ViewBranch,
};
pub use pack::{FieldPack, COAST_MAX_CELLS, H_MAX, H_MIN};
