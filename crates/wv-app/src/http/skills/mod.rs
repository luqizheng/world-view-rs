//! v0.1 的 9 个 skill（ADR-0014 §2.5）。
//!
//! ## 入口
//! 每个 skill 一个公开函数 `pub async fn handler(bridge, input) -> Result<Value, JsonRpcError>`。
//! [`crate::http::tools::dispatch`] 按 `name` 字符串路由到对应 handler。
//!
//! ## 输入 / 输出
//! - 输入：`serde_json::Value`，由本模块按 tool 的 schema 解析为强类型 struct。
//! - 输出：`serde_json::Value`（成功）或 [`JsonRpcError`]（失败）。
//!
//! ## 测试
//! 9 个 skill 各有 1 个 happy-path round-trip 测试；详见各模块底部。

pub mod read;
pub mod write;

pub use read::{dispatch_read, read_tool_schemas};
pub use write::{dispatch_write, write_tool_schemas};