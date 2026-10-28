//! `tools/list` 聚合点 + `tools/call` 路由。
//!
//! ## 顺序约定
//! 所有 15 个 skill 在 [`all_tools`] 中按「先读后写再 UI」排列；`tools/list`
//! 直接返回这个序列——LLM 客户端看到稳定的 schema 顺序，方便对齐 prompt 缓存。
//!
//! ## dispatch
//! [`dispatch`] 按 skill 名字分发：先查读类，命中即返回；未命中再查写类、
//! UI 操控类（ADR-0078）。不存在 → [`JsonRpcError::method_not_found`]（`-32601`）。

use serde_json::{json, Value};

use crate::http::bridge::HttpBridge;
use crate::http::mcp::{JsonRpcError, ToolDef};
use crate::http::skills;

/// skill 的回执形态（ADR-0078 D4）：绝大多数 skill 是 JSON 文本；
/// `screenshot` 要以 MCP image content 回传整张 PNG，故用枚举区分，
/// `server::call_tool` 按变体组 content 数组。
#[derive(Debug, Clone)]
pub enum ToolOutput {
    /// JSON 结果——按既有契约序列化成单条 text content。
    Json(Value),
    /// 图片结果——image content 为主条目，meta JSON 为次条目 text。
    Image {
        data_base64: String,
        mime: &'static str,
        meta: Value,
    },
}

/// 全部 15 个 skill 的 schema 描述（用于 `tools/list`）。
pub fn all_tools() -> Vec<ToolDef> {
    let mut out: Vec<ToolDef> = Vec::with_capacity(15);
    for (name, desc, schema) in skills::read_tool_schemas() {
        out.push(ToolDef {
            name,
            description: desc,
            input_schema: schema,
        });
    }
    for (name, desc, schema) in skills::write_tool_schemas() {
        out.push(ToolDef {
            name,
            description: desc,
            input_schema: schema,
        });
    }
    for (name, desc, schema) in skills::ui::ui_tool_schemas() {
        out.push(ToolDef {
            name,
            description: desc,
            input_schema: schema,
        });
    }
    out
}

/// 按名字分发到对应 handler。
///
/// ## 失败模式
/// - 名字不存在 → `method_not_found`（`-32601`）
/// - 输入参数校验失败 → 由各 handler 自行转 `invalid_params`（`-32602`）
/// - apply 抛 `EditError` → `internal_error`（`-32603`）
pub async fn dispatch(
    bridge: &HttpBridge,
    name: &str,
    args: Value,
) -> Result<ToolOutput, JsonRpcError> {
    // 先查读类，再查写类、UI 类——避免一个名字同时出现在两边（防御性：
    // 运行时重复注册比 panic 友好）。
    let read_names: Vec<&'static str> = skills::read_tool_schemas()
        .into_iter()
        .map(|(n, _, _)| n)
        .collect();
    let write_names: Vec<&'static str> = skills::write_tool_schemas()
        .into_iter()
        .map(|(n, _, _)| n)
        .collect();
    if read_names.contains(&name) {
        return skills::dispatch_read(bridge, name, args)
            .await
            .map(ToolOutput::Json);
    }
    if write_names.contains(&name) {
        return skills::dispatch_write(bridge, name, args)
            .await
            .map(ToolOutput::Json);
    }
    if skills::ui::ui_tool_schemas()
        .into_iter()
        .any(|(n, _, _)| n == name)
    {
        return skills::ui::dispatch_ui(bridge, name, args).await;
    }
    Err(JsonRpcError::method_not_found(name))
}

/// `tools/list` 响应的 result 段包装。
pub fn list_tools_result() -> Value {
    json!({
        "tools": all_tools().iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "inputSchema": t.input_schema,
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_exposes_exactly_thirteen_skills_in_stable_order() {
        let tools = all_tools();
        assert_eq!(tools.len(), 15);
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        // 顺序：先 5 个读，再 4 个写，再 4 个 UI 操控（ADR-0078）
        assert_eq!(
            names,
            vec![
                "list_maps",
                "get_map",
                "list_places",
                "get_place",
                "get_state_at_time",
                "create_map",
                "create_place",
                "undo",
                "save_project",
                "get_ui_state",
                "set_view_mode",
                "set_panel_visible",
                "screenshot",
            ]
        );
    }

    #[test]
    fn list_tools_result_matches_mcp_shape() {
        let v = list_tools_result();
        let arr = v["tools"].as_array().expect("tools 必为 array");
        assert_eq!(arr.len(), 15);
        // MCP `2025-03-26` 的 tool 定义 schema 字段叫 `inputSchema`（驼峰）。
        for t in arr {
            assert!(t["name"].is_string());
            assert!(t["description"].is_string());
            assert!(t["inputSchema"].is_object());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }
}
