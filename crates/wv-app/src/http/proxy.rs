//! `wv mcp`——MCP stdio 代理（ADR-0083）。
//!
//! ## 为什么存在
//!
//! GUI 内嵌的 MCP HTTP 服务（ADR-0014/0078/0082，`POST /mcp`）**没有
//! `initialize` 握手**——真 MCP 客户端（ZCode / Claude Desktop 等）握手
//! 不过、无法注册。本代理补齐协议面：
//!
//! ```text
//! MCP 客户端 ⇄ stdio（newline-delimited JSON-RPC）⇄ 本代理 ⇄ HTTP POST /mcp ⇄ wv gui
//! ```
//!
//! - `initialize` / `ping` / 通知：**本地应答**（GUI 侧没有这些方法）；
//! - `tools/list` / `tools/call`：**原样转发**到 `http://127.0.0.1:<port>/mcp`
//!   （工具定义与执行永远单一真源在 GUI 侧，代理零工具表维护）。
//!
//! ## 前置
//!
//! `wv gui`（`--features http`）已在运行；默认端口 8732（`--port` 覆盖，
//! 与 GUI 的 `--http-port` 对齐）。GUI 未运行时转发返回 -32000 错误——
//! 代理不负责拉起 GUI（MCP 客户端启动代理时拉 GUI 会造成进程树混乱）。
//!
//! ## 上行 HTTP 客户端
//!
//! 手写最小 HTTP/1.1 POST（tokio TcpStream，仅 loopback、无 TLS）——
//! 不为此新增 reqwest 依赖（AGENTS §9 默认拒绝）。

use serde_json::{json, Value};

/// 本地能应答的方法（不经转发）。
pub fn is_local_method(method: &str) -> bool {
    matches!(
        method,
        "initialize" | "ping" | "notifications/initialized" | "notifications/cancelled"
    )
}

/// 本地应答（`Some` = 要回给客户端的 JSON-RPC 成功结果；通知类返回 `None`
/// 不回包）。转发方法在此返回 `None`（由调用方走 HTTP）。
pub fn local_response(id: Value, method: &str) -> Option<Value> {
    match method {
        "initialize" => Some(json!({
            "id": id,
            "result": {
                // 协议版本：回客户端声明里的最新共同版本太复杂，固定一个
                // 广泛支持的日期版本——客户端（ZCode 等）按可协商处理。
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "wv", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "wv gui 的世界数据与 UI 操控工具（世界读写/undo/save/截图/视图/面板/自然区命名）。GUI 须已运行：cargo run -p wv-app --features http --bin wv -- gui"
            }
        })),
        "ping" => Some(json!({"id": id, "result": {}})),
        // 通知无 id、无回包。
        _ => None,
    }
}

/// 组转发请求体（`tools/list` / `tools/call` 原样带 method + params）。
pub fn forward_body(id: Value, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

/// 组「上游不可达」错误回执。
pub fn unreachable_error(id: Value, port: u16) -> Value {
    json!({
        "id": id,
        "error": {
            "code": -32000,
            "message": format!(
                "wv gui 未运行或端口不对（127.0.0.1:{port}）。先启动：cargo run -p wv-app --features http --bin wv -- gui（或 --http-port 对齐）"
            ),
        }
    })
}

/// 极小 loopback HTTP/1.1 POST 客户端：发 JSON、读回 JSON。
///
/// 只支持 content-length 响应（axum 默认带）；连接拒绝 → `Err`。
async fn http_post_json(port: u16, body: &Value) -> Result<Value, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let payload = body.to_string();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| format!("read: {e}"))?;
    let raw = String::from_utf8_lossy(&buf);
    let body_start = raw
        .find("\r\n\r\n")
        .ok_or_else(|| "bad response: no header/body split".to_string())?;
    serde_json::from_str(raw[body_start + 4..].trim()).map_err(|e| format!("bad json: {e}"))
}

/// 处理一行客户端请求 → 回包行（通知返回 `None`）。
///
/// `port` 上游端口。转发失败（连不上）组 [`unreachable_error`]。
pub async fn handle_line(line: &str, port: u16) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(json!({
                "id": null,
                "error": {"code": -32700, "message": format!("parse error: {e}")}
            }));
        }
    };
    // 通知（无 id）不回包。
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let id = id.expect("刚判过 is_none");
    if let Some(local) = local_response(id.clone(), method) {
        return Some(local);
    }
    if !matches!(method, "tools/list" | "tools/call") {
        return Some(json!({
            "id": id,
            "error": {"code": -32601, "message": format!("method not found: {method}（代理只本地答 initialize/ping，其余仅转发 tools/*）")}
        }));
    }
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    match http_post_json(port, &forward_body(id.clone(), method, params)).await {
        Ok(upstream) => Some(upstream),
        Err(_) => Some(unreachable_error(id, port)),
    }
}

/// stdio 主循环：stdin 每行一条 JSON-RPC，回包写 stdout（每行一条）。
///
/// 退出条件：stdin 关闭（EOF）。返回进程退出码（0）。
pub async fn run_stdio(port: u16) -> i32 {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                if let Some(resp) = handle_line(&line, port).await {
                    let _ = stdout.write_all(format!("{resp}\n").as_bytes()).await;
                    let _ = stdout.flush().await;
                }
            }
            Ok(None) => return 0, // EOF
            Err(e) => {
                eprintln!("wv mcp: stdin 读取失败: {e}");
                return 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::HttpBridge;
    use crate::http::server::mcp_router;
    use axum::body::Body;
    use tower::util::ServiceExt;

    #[test]
    fn initialize_is_answered_locally_with_server_info() {
        let resp = local_response(json!(1), "initialize").expect("initialize 本地答");
        let info = resp["result"]["serverInfo"]["name"].as_str();
        assert_eq!(info, Some("wv"), "serverInfo.name 应为 wv");
        assert!(
            resp["result"]["capabilities"]["tools"].is_object(),
            "必须声明 tools capability（否则客户端不列工具）"
        );
    }

    #[test]
    fn notifications_get_no_response_and_ping_answers() {
        assert!(local_response(json!(null), "notifications/initialized").is_none());
        assert_eq!(
            local_response(json!(7), "ping").expect("ping")["result"],
            json!({})
        );
        assert!(is_local_method("notifications/cancelled"));
        assert!(!is_local_method("tools/list"), "tools/* 必须转发");
    }

    #[test]
    fn forward_body_carries_method_and_params() {
        let body = forward_body(json!(3), "tools/call", json!({"name": "get_ui_state"}));
        assert_eq!(body["method"], "tools/call");
        assert_eq!(body["params"]["name"], "get_ui_state");
        assert_eq!(body["jsonrpc"], "2.0");
    }

    #[tokio::test]
    async fn tools_list_proxies_end_to_end_through_http_router() {
        // 端到端：handle_line 的转发体 → 真实 axum /mcp 路由 → 回包里有 15 工具。
        // （转发体与生产代理同一条 forward_body；路由与 start_mcp_server 同源。）
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let router = mcp_router(bridge);
        // 起 drain 让读类 skill 的 send+await 闭环（同 server.rs 测试形态）。
        tokio::spawn(async move {
            let mut world = wv_core::World::default();
            let mut history = wv_edit::History::default();
            for _ in 0..100_000 {
                crate::http::bridge::drain_messages(&mut rx, &mut world, &mut history, None);
                tokio::task::yield_now().await;
            }
        });
        let body = forward_body(json!(9), "tools/list", Value::Null);
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("request"),
            )
            .await
            .expect("oneshot");
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let upstream: Value = serde_json::from_slice(&bytes).expect("json");
        let tools = upstream["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 15, "GUI 工具面 15 个应原样透出");
    }
}
