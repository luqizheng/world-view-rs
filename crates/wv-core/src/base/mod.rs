//! # wv-core::base —— 通用基础类型
//!
//! 不依赖任何领域语义的「工具集」：ID、错误、几何、颜色。
//!
//! ## 职责
//! - [`id`]：`Id`（§4.4 字符集限制）/ `IdError`
//! - [`error`]：`CoreError` / `CoreResult`
//! - [`geometry`]：`Point`（归一化项目栅格坐标）
//! - [`color`]：核心语义色（与 `wv-app` 的色板分离）
//!
//! ## 边界（不做什么）
//! - **不**含时间、地图、领域模型。
//! - **不**含 IO 适配（那是 `wv-io`）。
//!
//! ## 主要导出
//! - [`crate::Id`] / [`crate::CoreError`] / [`crate::Point`] / [`crate::Color`]

pub mod color;
pub mod error;
pub mod geometry;
pub mod id;
