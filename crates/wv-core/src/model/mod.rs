//! # wv-core::model —— 领域模型实体
//!
//! 世界里**有名字**的实体。每实体一文件（AGENTS §4.5），对应磁盘上一个 YAML。
//!
//! ## 职责
//! - [`character`]：人物（含时间区间化的状态片段，§4.1）
//! - [`event`]：事件（按地图 / 时间定位；§2.4）
//! - [`relation`]：关系网
//! - [`faction`]：势力本体（路线图 S12，§4.5）
//!
//! ## 边界（不做什么）
//! - **不**含派生（相遇提示 / 事态 / 旁白走 [`crate::derived`]）。
//! - **不**含几何（势力的几何存于 `World.faction_geometry`）。
//!
//! ## 主要导出
//! - [`character::Character`] / [`event::Event`] / [`faction::Faction`]

pub mod character;
pub mod event;
pub mod faction;
pub mod relation;
