//! # wv-io —— World View IO 层
//!
//! 负责**工程目录**的加载与保存。原则二：目录即工程。
//!
//! 本层的契约：
//! - 唯一允许触碰文件系统的地方（`wv-core` 纯内存，`wv-app` 不直接读盘）。
//! - YAML 读写必须 round-trip 稳定：`load(save(w)) == w`。
//! - 二进制分块的读写与《二进制格式规格》绑定。

pub mod binary;
pub mod error;
pub mod project;

pub use error::{IoError, IoResult};
pub use project::Project;

/// 当前支持的 schema 版本。写入工程时以此为准。
pub const CURRENT_SCHEMA_VERSION: u32 = 1;
