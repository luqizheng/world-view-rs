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
        Ok(output) => {
            let content = match output {
                // MCP `2025-03-26` 把 skill 输出塞进 `content: [{type:"text", text:"..."}]`。
                // 复杂结构走 JSON 字符串——LLM 客户端会自动 decode。
                tools::ToolOutput::Json(value) => {
                    let text = serde_json::to_string(&value)
                        .unwrap_or_else(|e| format!("{{\"serialize_error\":\"{e}\"}}"));
                    vec![ToolContent::text(text)]
                }
                // ADR-0078：截图以 image content 为主条目（LLM 客户端按图渲染，
                // 不进文本上下文），meta JSON 为次条目供程序化读取。
                tools::ToolOutput::Image {
                    data_base64,
                    mime,
                    meta,
                } => {
                    let meta_text = serde_json::to_string(&meta)
                        .unwrap_or_else(|e| format!("{{\"serialize_error\":\"{e}\"}}"));
                    vec![
                        ToolContent::image(data_base64, mime),
                        ToolContent::text(meta_text),
                    ]
                }
            };
            let result = serde_json::json!({
                "content": content,
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
/// 2026-10-28 追：设置对话框也可在运行中手动停止 / 换端口重启（经
/// `crate::http_integration::HttpService`），与启动期自动拉起共用本函数。
///
/// ## 参数
/// - `shutdown`：**优雅停机信号**——future 完成即停止 accept 并排空在途
///   请求（axum `with_graceful_shutdown`）。调用方传 `oneshot` 接收端；
///   sender 被 drop 同样视为停机信号（「持有者没了 = 该停」）。
/// - `on_listen`：绑定成功后的**一次性回调**（拿到真实 `SocketAddr`——
///   测试传端口 0 时由 OS 分配实际端口）。运行时启停用它向主帧回报
///   「已监听」，设置对话框据此把状态切到 Running。
///
/// ## 失败
/// 绑定失败（端口占用 / 无权限）→ stderr 报错 + 返回 `Err`，由 caller 决定
/// 是否提示用户。**不**自动 +1 重试（ADR §2.2 + §6 Q1）。
pub async fn start_mcp_server(
    bridge: HttpBridge,
    bind: &str,
    port: u16,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    on_listen: impl FnOnce(SocketAddr) + Send + 'static,
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
        let loc = crate::i18n::cli_locale();
        eprintln!(
            "{}",
            crate::i18n::t(loc, crate::i18n::I18nKey::HttpBindFailed)
                .replace("<addr>", &addr.to_string())
                .replace("<err>", &e.to_string())
        );
        ServerError::Bind(e.to_string())
    })?;

    // 实际绑定地址（端口 0 时由 OS 分配真实端口；生命周期测试与运行时
    // 启停的 Running 状态都以它为准，不能拿请求的 addr 糊弄）。
    let bound = listener
        .local_addr()
        .expect("刚 bind 成功的 listener 必然可取 local_addr");

    let loc = crate::i18n::cli_locale();
    eprintln!(
        "{}",
        crate::i18n::t(loc, crate::i18n::I18nKey::HttpListening)
            .replace("<addr>", &bound.to_string())
    );
    on_listen(bound);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| ServerError::Serve(e.to_string()))
}

#[derive(Debug)]
pub enum ServerError {
    BadAddress(String),
    Bind(String),
    Serve(String),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let loc = crate::i18n::cli_locale();
        let t = |k| crate::i18n::t(loc, k);
        match self {
            ServerError::BadAddress(inner) => {
                write!(
                    f,
                    "{}{}",
                    t(crate::i18n::I18nKey::HttpAddrParseFailed),
                    inner
                )
            }
            ServerError::Bind(inner) => {
                write!(f, "{}{}", t(crate::i18n::I18nKey::HttpTcpBindFailed), inner)
            }
            ServerError::Serve(inner) => {
                write!(f, "{}{}", t(crate::i18n::I18nKey::HttpServeFailed), inner)
            }
        }
    }
}

impl std::error::Error for ServerError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::{drain_messages, HttpBridge};
    use serde_json::json;

    /// 构造「桥 + 一个空 World + 空 History」的三件套，handler 测试专用。
    fn app_state() -> (
        Arc<AppState>,
        tokio::sync::mpsc::UnboundedReceiver<crate::http::bridge::HttpMessage>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let state = Arc::new(AppState {
            bridge: bridge.clone(),
        });
        (state, rx)
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    /// **真·smoke test**：构造 axum Router（与生产 server.rs 一致的路由表），
    /// 用 `tower::ServiceExt::oneshot` 跑一次端到端 POST /mcp（不绑端口，
    /// 内存里走完整 handler 链路），验证从「客户端 JSON-RPC 请求」到
    ///「handler 回 JSON-RPC 响应」无任何中间层失真。
    ///
    /// 这条用例是 `cargo http-test` 的 end-to-end 闸门——任何把 handler
    /// 接到错误路由、改坏 schema、或破坏 axum `IntoResponse` 形态的改动
    /// 都会被这条用例捕到。生产路径的端口绑定走 `start_mcp_server`，
    /// 已在另外的测试覆盖（绑定失败 stderr + 退出）。
    #[tokio::test]
    async fn end_to_end_post_mcp_tools_list_returns_fifteen_skills() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::util::ServiceExt;

        let (state, mut rx) = app_state();
        // 起后台 drain，让读类 skill 的 send + await 闭环（handler 在
        // tokio runtime 里，drain 在另一个 task——同 production 形态）。
        let drain = tokio::spawn(async move {
            let mut world = wv_core::World::default();
            let mut history = wv_edit::History::default();
            for _ in 0..100_000 {
                drain_messages(&mut rx, &mut world, &mut history, None);
                tokio::task::yield_now().await;
            }
        });

        let app = Router::new()
            .route("/mcp", post(mcp_handler))
            .with_state(state.clone());

        let body = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "id": 1
        }))
        .unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "POST /mcp 应回 200 OK");

        let body_bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            15,
            "应暴露 15 个 skill（9 数据 + 4 UI + 2 命名）"
        );

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            vec![
                "list_maps",
                "get_map",
                "list_places",
                "get_place",
                "get_state_at_time",
                "get_naming_context",
                "create_map",
                "create_place",
                "undo",
                "save_project",
                "set_nature_names",
                "get_ui_state",
                "set_view_mode",
                "set_panel_visible",
                "screenshot",
            ]
        );

        for t in tools {
            assert!(t["name"].is_string());
            assert!(t["description"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
        }

        drain.abort();
    }

    /// ADR-0078 端到端：`set_view_mode` 经 handler → bridge.ui_send →
    /// 带 UI 钩子的 drain（就地改 `ViewState`）→ echo 快照回 JSON text。
    /// 这条链路在真机上是 drain_http_commands 的资源形态；测试里用等价的
    /// 本地 `ViewState`/`PanelState` 替身。
    #[tokio::test]
    async fn end_to_end_post_mcp_tools_call_set_view_mode_routes_through_ui_hook() {
        use crate::http::bridge::{drain_messages_with, UiReply};
        use crate::state::PanelState;
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::util::ServiceExt;
        use wv_render::state::ViewState;

        let (state, mut rx) = app_state();
        let drain = tokio::spawn(async move {
            let mut world = wv_core::World::default();
            let mut history = wv_edit::History::default();
            let mut view = ViewState::default();
            let mut panels = PanelState::default();
            for _ in 0..100_000 {
                drain_messages_with(
                    &mut rx,
                    &mut world,
                    &mut history,
                    None,
                    &mut |req, reply| {
                        let r =
                            crate::http::skills::ui::handle_ui_request(req, &mut view, &mut panels)
                                .unwrap_or_else(|| {
                                    UiReply::Error("内部错误：非截图请求无回执".into())
                                });
                        let _ = reply.send(r);
                    },
                );
                tokio::task::yield_now().await;
            }
        });

        let app = Router::new()
            .route("/mcp", post(mcp_handler))
            .with_state(state.clone());

        let body = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "set_view_mode",
                "arguments": {"mode": "cellmap"}
            },
            "id": 9
        }))
        .unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(
            resp["result"]["isError"].as_bool(),
            Some(false),
            "set_view_mode(cellmap) 应成功；raw = {resp}"
        );
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            payload["view_mode"], "cellmap",
            "echo 快照应带新模式；raw = {payload}"
        );

        drain.abort();
    }

    /// end-to-end smoke：POST /mcp `tools/call` 命中 list_maps（读类），
    /// 验证 handler → bridge.read → drain → 回执 → CallToolResult 完整链路。
    #[tokio::test]
    async fn end_to_end_post_mcp_tools_call_list_maps_routes_through_handler() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::util::ServiceExt;

        let (state, mut rx) = app_state();
        let drain = tokio::spawn(async move {
            let mut world = wv_core::World::default();
            let mut history = wv_edit::History::default();
            for _ in 0..100_000 {
                drain_messages(&mut rx, &mut world, &mut history, None);
                tokio::task::yield_now().await;
            }
        });

        let app = Router::new()
            .route("/mcp", post(mcp_handler))
            .with_state(state.clone());

        let body = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "list_maps",
                "arguments": {}
            },
            "id": 7
        }))
        .unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let resp: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(resp["id"], 7);
        assert_eq!(
            resp["result"]["isError"].as_bool(),
            Some(false),
            "list_maps 走通 = isError:false；raw = {resp}"
        );
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .expect("content[0].text 必为字符串");
        let payload: Value = serde_json::from_str(text).expect("text 必为合法 JSON");
        assert!(
            payload.is_array(),
            "list_maps 应返 Vec<MapSummary>；raw = {payload}"
        );

        drain.abort();
    }

    #[test]
    fn tools_list_returns_fifteen_tools() {
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
        assert_eq!(tools.len(), 15);
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
        assert!(
            raw.contains("\"code\":-32601"),
            "未知方法应得 -32601：{raw}"
        );
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
