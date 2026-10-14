//! MCP `2025-03-26` Streamable HTTP 协议类型。
//!
//! 协议要点（最小集，足够 v0.1 的 9 个 skill）：
//! - 客户端 `POST /mcp`，body 是 JSON-RPC 2.0；
//! - 服务端 `200 OK` 回 JSON-RPC 2.0 `result` / `error`；
//! - 两种方法：`tools/list`（返回 9 个 skill 的 schema）、`tools/call`（执行）。
//!
//! ## 为什么自己定义类型而不是直接用 `serde_json::Value`
//!
//! - **schema 自描述**——`tools/list` 的输出是 LLM 选择工具的依据，类型名
//!   与字段名直接成为 schema 的一部分；用 `Value` 等于把 schema 写在文档里。
//! - **编译期检查**——handler 入参结构是 `#[derive(Deserialize)]` 的 struct，
//!   字段拼错在测试里就报；`Value` 路径要等到 LLM 调才暴露。
//! - **错误归一**——`ToolResult::is_error` 与 `JsonRpcResponse::error` 都走
//!   `McpError` 一种形态，回执的 JSON 形状 LLM 可循。

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 请求（只列 v0.1 实际用的字段）。
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    /// 必须为 `"2.0"`。**未严格校验**——MCP 客户端层做，handler 假定合规。
    pub jsonrpc: String,
    /// 方法名：`tools/list` 或 `tools/call`。
    pub method: String,
    /// 可选；`tools/list` 一般不带，`tools/call` 必有。
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    /// 请求 id，用于回执配对。**number 或 string 都允许**（JSON-RPC 2.0），
    /// 这里用 `Value` 直接回传，调用方按原样收。
    pub id: serde_json::Value,
}

/// JSON-RPC 2.0 响应。
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    /// 与请求 `id` 一致；id 类型由请求决定，这里原样回。
    pub id: serde_json::Value,
    /// 成功时 `Some`；与 [`Self::error`] 互斥。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// 失败时 `Some`；与 [`Self::result`] 互斥。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: serde_json::Value, err: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(err),
        }
    }
}

/// JSON-RPC 2.0 错误对象。`code` 走 [-32099, -32000] 之外的保留段，
/// `-32601`（Method not found）与 `-32602`（Invalid params）用得多。
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    /// `-32601` 方法未找到。
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
            data: None,
        }
    }

    /// `-32602` 参数不合法（schema 校验失败 / 类型不对）。
    pub fn invalid_params(detail: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: detail.into(),
            data: None,
        }
    }

    /// `-32603` 内部错误（apply 失败、IO 错、History 错）。
    pub fn internal_error(detail: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: detail.into(),
            data: None,
        }
    }
}

/// MCP `tools/list` 返回的单个 tool 描述。
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema 形态的对象（`type: "object"` + `properties` + `required`）。
    pub input_schema: serde_json::Value,
}

/// `tools/list` 响应的 result 段。
#[derive(Debug, Clone, Serialize)]
pub struct ListToolsResult {
    pub tools: Vec<ToolDef>,
}

/// `tools/call` 请求的 params。
#[derive(Debug, Clone, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    /// skill 的入参；handler 按 tool 的 input_schema 反序列化。
    #[serde(default)]
    pub arguments: Option<serde_json::Value>,
}

/// `tools/call` 响应的 result 段（MCP 规范）。
#[derive(Debug, Clone, Serialize)]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    /// `true` 表示「调用执行了，但 skill 自己判定为失败」——
    /// **不是** JSON-RPC 错误（layer 不同）。
    #[serde(rename = "isError")]
    pub is_error: bool,
}

/// 单条 content。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolContent {
    /// 文本载荷。Skill 的 JSON 输出以单字符串塞进这里（最简形态，
    /// 大多数 LLM 客户端会再 decode JSON；v0.1 不发明自定义 content type）。
    Text { text: String },
}

impl ToolContent {
    pub fn text(s: impl Into<String>) -> Self {
        ToolContent::Text { text: s.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_response_ok_serializes_with_id_and_result() {
        let resp = JsonRpcResponse::ok(serde_json::json!(1), serde_json::json!({"ok": true}));
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
        assert!(s.contains("\"id\":1"));
        assert!(s.contains("\"result\":{\"ok\":true}"));
        assert!(!s.contains("error"));
    }

    #[test]
    fn jsonrpc_response_err_serializes_with_id_and_error() {
        let resp = JsonRpcResponse::err(
            serde_json::json!("req-1"),
            JsonRpcError::method_not_found("tools/frobnicate"),
        );
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"id\":\"req-1\""));
        assert!(s.contains("\"code\":-32601"));
        assert!(s.contains("Method not found"));
        assert!(!s.contains("\"result\""));
    }

    #[test]
    fn call_tool_result_text_carries_payload() {
        let r = CallToolResult {
            content: vec![ToolContent::text("hi")],
            is_error: false,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"type\":\"text\""));
        assert!(s.contains("\"text\":\"hi\""));
        assert!(s.contains("\"isError\":false"));
    }
}
