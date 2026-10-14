//! # wv-core::derived —— 派生层
//!
//! 由「实体 + 时间」计算出来的视图。**不**独立存盘——它们是模型的派生物。
//!
//! ## 职责
//! - [`encounter`]：相遇提示（按轨迹的「同一时刻同一地点」检测，V1 守约）
//! - [`situation`]：事态（**归纳容器**，收拢事件与单位）
//! - [`narration`]：旁白（独立叙事实体，事件只存 ID 引用）
//!
//! ## 边界（不做什么）
//! - **不**派生 IO 提示。`wv-io::diagnostics` 是 IO 层的诊断。
//!
//! ## 主要导出
//! - [`encounter::detect_encounter_hints`] / [`situation::Situation`] / [`narration::Narration`]

pub mod encounter;
pub mod narration;
pub mod situation;
