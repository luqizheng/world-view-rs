//! HTTP 服务（axum）：`POST /mcp` 单一端点。
//!
//! ## 协议概要（MCP `2025-03-26` Streamable HTTP）
//! - 请求：`POST /mcp`，body = JSON-RPC 2.0
//! - 响应：`200 OK`，body = JSON-RPC 2.0 `result` / `error`
//! - 方法：`tools/list` / `tools/call`
//!
//! ## 绑定策略（ADR-0014 §2.2 + §6 Q1）
//! - 仅 `127.0.0.1`
//! - 默认端口 8732；绑定失败 stderr + 退出 server 任务（**不**自动 +1）
//!
//! ## 启动时机
//! 由 [`start_mcp_server`] 在 Bevy `Startup` schedule 中调用；handler
//! 持 `HttpBridge` 克隆，写入 `tokio::sync::mpsc::UnboundedSender` 端。

use axum::extract::State;
use axum::http::StatusCode as AxStatus;
use axum::Json as AxJson;
use axum::{routing::post, Router};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::http::bridge::HttpBridge;
use crate::http::mcp::{
    CallToolParams, JsonRpcError, JsonRpcRequest, JsonRpcResponse, ToolContent,
};
use crate::http::tools;

/// axum 的共享 state：handler 需要 `HttpBridge` 调 mpsc。
#[derive(Clone)]
struct AppState {
    bridge: HttpBridge,
}

/// `POST /mcp` 的 handler。**唯一**对外接口——所有 MCP 方法都走这里。
async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    AxJson(req): AxJson<JsonRpcRequest>,
) -> (AxStatus, AxJson<JsonRpcResponse>) {
    let resp = dispatch_request(&state.bridge, req).await;
    (AxStatus::OK, AxJson(resp))
}

/// JSON-RPC 2.0 入口：根据 `method` 字段路由。
async fn dispatch_request(bridge: &HttpBridge, req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "tools/list" => JsonRpcResponse::ok(req.id, tools::list_tools_result()),
        "tools/call" => match req.params {
            Some(p) => match serde_json::from_value::<CallToolParams>(p) {
                Ok(p) => call_tool(bridge, req.id, p).await,
                Err(e) => JsonRpcResponse::err(
                    req.id,
                    JsonRpcError::invalid_params(format!("tools/call params 非法: {e}")),
                ),
            },
            None => JsonRpcResponse::err(
                req.id,
                JsonRpcError::invalid_params("tools/call 必须带 params"),
            ),
        },
        other => JsonRpcResponse::err(req.id, JsonRpcError::method_not_found(other)),
    }
}

/// 执行单个 tool 调用，把结果包成 MCP `CallToolResult`。
async fn call_tool(
    bridge: &HttpBridge,
    id: serde_json::Value,
    params: CallToolParams,
) -> JsonRpcResponse {
    let args = params.arguments.unwrap_or(Value::Null);
    match tools::dispatch(bridge, &params.name, args).await {
        Ok(value) => {
            // MCP `2025-03-26` 把 skill 输出塞进 `content: [{type:"text", text:"..."}]`。
            // 复杂结构走 JSON 字符串——LLM 客户端会自动 decode。
            let text = serde_json::to_string(&value)
                .unwrap_or_else(|e| format!("{{\"serialize_error\":\"{e}\"}}"));
            let result = serde_json::json!({
                "content": [ToolContent::text(text)],
                "isError": false,
            });
            JsonRpcResponse::ok(id, result)
        }
        Err(err) => {
            // skill 自身报 INVALID_PARAMS / INTERNAL_ERROR 时：当作 `isError=true`
            // 包装返回（layer 不同：JSON-RPC 错误是协议层，isError 是 skill 语义层）。
            // v0.1 把所有非「方法不存在」错误都按 isError 透传——LLM 客户端一般能区分。
            let text = serde_json::to_string(&err).unwrap_or_else(|_| err.message.clone());
            let result = serde_json::json!({
                "content": [ToolContent::text(text)],
                "isError": true,
            });
            JsonRpcResponse::ok(id, result)
        }
    }
}

/// 启动 axum server，绑 `127.0.0.1:port`。**阻塞**直到 server 任务结束。
///
/// ## 调用模型
/// `wv-app --features http` 启动 GUI 时由 `app_bootstrap` 在一个独立
/// `tokio::runtime` 里调 [`start_mcp_server`]；它阻塞在 `axum::serve`
/// 的 `.await`，主帧的 Bevy 调度继续跑（`axum::serve` 不占 GUI 主线程）。
///
/// ## 失败
/// 绑定失败（端口占用 / 无权限）→ stderr 报错 + 返回 `Err`，由 caller 决定
/// 是否提示用户。**不**自动 +1 重试（ADR §2.2 + §6 Q1）。
pub async fn start_mcp_server(
    bridge: HttpBridge,
    bind: &str,
    port: u16,
) -> Result<(), ServerError> {
    let addr: SocketAddr = format!("{bind}:{port}")
        .parse::<SocketAddr>()
        .map_err(|e| ServerError::BadAddress(e.to_string()))?;

    let state = Arc::new(AppState { bridge });
    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        // 端口占用的 stderr 消息：给出可操作的指引（用户可换端口），
        // 但**不**自动 +1（避免静默偏移掩盖冲突，ADR §6 Q1）。
        eprintln!(
            "[wv-app http] 绑定 {addr} 失败: {e}。\n\
             当前端口被占用，请用 `--http-port <N>` 指定另一端口\n\
             （详见 ADR-0014 §6 Q1）。"
        );
        ServerError::Bind(e.to_string())
    })?;

    eprintln!("[wv-app http] MCP server 监听 {addr}（POST /mcp）");

    axum::serve(listener, app)
        .await
        .map_err(|e| ServerError::Serve(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("绑定地址解析失败: {0}")]
    BadAddress(String),
    #[error("TCP 绑定失败: {0}")]
    Bind(String),
    #[error("HTTP 服务运行失败: {0}")]
    Serve(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::{drain_messages, HttpBridge};
    use serde_json::json;

    /// 构造「桥 + 一个空 World + 空 History」的三件套，handler 测试专用。
    fn app_state() -> (Arc<AppState>, tokio::sync::mpsc::UnboundedReceiver<crate::http::bridge::HttpMessage>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let state = Arc::new(AppState { bridge: bridge.clone() });
        (state, rx)
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    #[test]
    fn tools_list_returns_nine_tools() {
        let (state, _rx) = app_state();
        let (_status, AxJson(resp)) = rt().block_on(mcp_handler(
            State(state.clone()),
            AxJson(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "tools/list".into(),
                params: None,
                id: json!(1),
            }),
        ));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        let tools = resp.result.unwrap()["tools"].as_array().unwrap().clone();
        assert_eq!(tools.len(), 9);
    }

    #[test]
    fn tools_call_unknown_method_returns_minus_32601() {
        let (state, _rx) = app_state();
        let (_status, AxJson(resp)) = rt().block_on(mcp_handler(
            State(state.clone()),
            AxJson(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "tools/frobnicate".into(),
                params: None,
                id: json!("req-x"),
            }),
        ));
        let raw = serde_json::to_string(&resp).unwrap();
        assert!(raw.contains("\"code\":-32601"), "未知方法应得 -32601：{raw}");
    }

    #[test]
    fn tools_call_known_skill_round_trip() {
        let rt = rt();
        let raw = rt.block_on(async {
            let (state, mut rx) = app_state();
            // 起后台 drain 让 read path 闭环。
            let drain = tokio::spawn(async move {
                let mut world = wv_core::World::default();
                let mut history = wv_edit::History::default();
                for _ in 0..100_000 {
                    drain_messages(&mut rx, &mut world, &mut history, None);
                    tokio::task::yield_now().await;
                }
            });
            let (_status, AxJson(resp)) = mcp_handler(
                State(state.clone()),
                AxJson(JsonRpcRequest {
                    jsonrpc: "2.0".into(),
                    method: "tools/call".into(),
                    params: Some(json!({
                        "name": "list_maps",
                        "arguments": {},
                    })),
                    id: json!(42),
                }),
            )
            .await;
            drain.abort();
            serde_json::to_string(&resp).unwrap()
        });
        assert!(raw.contains("\"isError\":false"));
    }

    #[test]
    fn tools_call_missing_params_is_minus_32602() {
        let (state, _rx) = app_state();
        let (_status, AxJson(resp)) = rt().block_on(mcp_handler(
            State(state.clone()),
            AxJson(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                method: "tools/call".into(),
                params: None,
                id: json!(1),
            }),
        ));
        let raw = serde_json::to_string(&resp).unwrap();
        assert!(raw.contains("\"code\":-32602"));
    }
}