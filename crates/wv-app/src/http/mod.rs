//! # http —— LLM 可控的世界编辑（ADR-0014）
//!
//! `wv-app --features http` 时启用。MCP `2025-03-26` Streamable HTTP，绑
//! `127.0.0.1:8732`，让 Claude Desktop / Cursor / Zed / Continue 等 LLM
//! 客户端能读 / 改 / 存当前打开的世界。
//!
//! ## 不变量（与 AGENTS.md / ADR-0014 §2.3 一致）
//!
//! 1. **写类 skill 必须经 [`wv_edit::EditCommand`]**——handler 只发命令，
//!    **不**直写 [`crate::state::WorldProjection::world`]。
//! 2. **单线程命令队列**：所有 EditCommand 经 `tokio::mpsc` 投递到 Bevy
//!    主帧，由 [`bridge::drain_commands`] 系统统一调
//!    [`wv_edit::History::commit`] / [`wv_edit::History::undo`]。
//!    `World` 是工作副本，**只在主帧写**——避免与 GUI / 笔刷系统数据竞争。
//! 3. **读类 skill 直查 [`wv_core`]**，**不**经 ECS——P3 + §3.2 不破。
//! 4. **仅 loopback**——`bind = 127.0.0.1`，默认端口 8732；绑定失败 stderr
//!    报错并退出 server 任务，**不**阻塞 GUI 主循环。
//!
//! ## 模块导航表
//!
//! | 模块 | 一句话职责 |
//! |---|---|
//! | [`bridge`] | handler → EditCommand 的 mpsc 桥 + Bevy 主帧 drain 系统。 |
//! | [`server`] | axum 路由 `POST /mcp`（JSON-RPC 2.0），仅 loopback 8732。 |
//! | [`mcp`] | MCP `2025-03-26` 协议类型（`JsonRpcRequest` / `JsonRpcResponse` / `ToolDef`）。 |
//! | [`tools`] | 9 个 skill 的 `tools/list` 聚合点。 |
//! | [`filter`] | Mini-DSL filter 解析（list_* 用：等值 / 子串 + AND 合取）。 |
//! | [`skills`] | 每个 skill 一个 handler（read / write 各一组）。 |
//!
//! ## 测试
//!
//! `cargo test -p wv-app --features http`（别名 `cargo http-test`，见
//! `.cargo/config.toml`）覆盖：
//! - 9 个 skill 的 happy-path round-trip；
//! - 1 个并发写测试（验证 mpsc 串行化：handler 与主帧同时发命令不撕裂）；
//! - 1 个 MCP schema 校验（`tools/list` 返回的 schema 与 handler 入参
//!   字段一致）。

pub mod filter;
pub mod server;
pub mod tools;

pub mod bridge;
pub mod mcp;
pub mod skills;

/// 默认绑定端口（"wv" → w-v 谐音；可记）。
///
/// 通过 `bind(127.0.0.1:8732)` 启动；端口已被占用时，server 任务在 stderr
/// 报错并退出，**不**自动 +1 重试（ADR-0014 §2.2 + §6 Q1 已默认：避免静默
/// 偏移掩盖冲突）。
pub const DEFAULT_PORT: u16 = 8732;

/// 默认绑定地址（仅 loopback）。
pub const DEFAULT_BIND: &str = "127.0.0.1";
