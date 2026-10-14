//! `tools/list` 聚合点 + `tools/call` 路由。
//!
//! ## 顺序约定
//! 所有 9 个 skill 在 [`all_tools`] 中按「先读后写」排列；`tools/list` 直接
//! 返回这个序列——LLM 客户端看到稳定的 schema 顺序，方便对齐 prompt 缓存。
//!
//! ## dispatch
//! [`dispatch`] 按 skill 名字分发：先查读类，命中即返回；未命中再查写类。
//! 不存在 → [`JsonRpcError::method_not_found`]（`-32601`）。

use serde_json::{json, Value};

use crate::http::bridge::HttpBridge;
use crate::http::mcp::{JsonRpcError, ToolDef};
use crate::http::skills;

/// 全部 9 个 skill 的 schema 描述（用于 `tools/list`）。
pub fn all_tools() -> Vec<ToolDef> {
    let mut out: Vec<ToolDef> = Vec::with_capacity(9);
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
) -> Result<Value, JsonRpcError> {
    // 先查读类，再查写类——避免一个名字同时出现在两边（v0.1 不会，但
    // 防御性检查比运行时 panic 友好）。
    let read_names: Vec<&'static str> =
        skills::read_tool_schemas().into_iter().map(|(n, _, _)| n).collect();
    let write_names: Vec<&'static str> =
        skills::write_tool_schemas().into_iter().map(|(n, _, _)| n).collect();
    if read_names.contains(&name) {
        return skills::dispatch_read(bridge, name, args).await;
    }
    if write_names.contains(&name) {
        return skills::dispatch_write(bridge, name, args).await;
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
    fn all_tools_exposes_exactly_nine_skills_in_stable_order() {
        let tools = all_tools();
        assert_eq!(tools.len(), 9);
        let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
        // 顺序：先 5 个读，再 4 个写
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
            ]
        );
    }

    #[test]
    fn list_tools_result_matches_mcp_shape() {
        let v = list_tools_result();
        let arr = v["tools"].as_array().expect("tools 必为 array");
        assert_eq!(arr.len(), 9);
        // MCP `2025-03-26` 的 tool 定义 schema 字段叫 `inputSchema`（驼峰）。
        for t in arr {
            assert!(t["name"].is_string());
            assert!(t["description"].is_string());
            assert!(t["inputSchema"].is_object());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }
}