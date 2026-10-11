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
pub mod situation_io;
pub mod terrain_bin;

pub use error::{IoError, IoResult};
pub use project::Project;
pub use terrain_bin::{
    decode_terrain, encode_terrain, read_terrain, write_terrain, TERRAIN_FORMAT_VERSION,
};

/// 当前支持的 schema 版本。写入工程时以此为准。
///
/// **v2**（基线 §2.16）：时间模型重做（`TimePoint` 改为天数计数器）+ 事态 / 轨迹落地。
/// 读到 v1 工程时**不做自动迁移**（v1 的「纪元+年+日」在缺少历法定义时无法可靠换算），
/// 由 [`project::Project::open`] 给出可操作的错误指引。
pub const CURRENT_SCHEMA_VERSION: u32 = 2;
