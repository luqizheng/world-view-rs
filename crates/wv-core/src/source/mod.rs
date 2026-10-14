//! # wv-core::source —— 外部数据来源
//!
//! **不**是项目自身定义的实体，而是「从外部导入的数据在本层有引用面」的桥类型。
//!
//! ## 职责
//! - [`street`]：街道矢量图层文档（ADR-0010）
//! - [`label`]：标签层数据模型与可见性求值（ADR-0016）
//! - [`overture_id`]：Overture Maps 的 ID 类型（ADR-0007）
//! - [`geographic_source`]：外部地理数据源（V1 协议）
//! - [`measurement`]：地图测量叠加（工具产物，每图一份 YAML）
//!
//! ## 边界（不做什么）
//! - **不**含 Overture 拉取逻辑（`wv-overture` crate 负责）
//! - **不**含 IO 读写（`wv-io` 负责）
//!
//! ## 主要导出
//! - [`street::StreetLayer`] / [`overture_id::OvertureId`] / [`geographic_source::GeographicSourceV1`]
//! - [`label::LabelLayerConfig`] / [`label::label_visible_at`]
//! - [`measurement::MeasurementOverlay`]

pub mod geographic_source;
pub mod label;
pub mod measurement;
pub mod overture_id;
pub mod street;
