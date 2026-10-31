//! 写类 4 个 skill（ADR-0014 §2.5 + §2.3 硬约束）。
//!
//! ## 不破 §3.3
//! 所有写操作经 [`HttpBridge::apply`] / [`undo`] / [`save`]，**绝不**直接
//! 改 World；handler 只构造 [`wv_edit::EditCommand`]（或 undo / save 的语义
//! 等价操作），由 Bevy 主帧的 [`crate::http::bridge::drain_messages`] 统一
//! apply。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::bridge::HttpBridge;
use crate::http::mcp::JsonRpcError;

/// 写类 skill 的 JSON Schema 列表。
///
/// **为什么是 `fn()` 不是 `const`**：`json!` 宏不工作在常量上下文
///（serde_json::Map::new / to_value 都不是 const fn）。改用函数 —— 每次
/// `tools/list` 调用构造一份新的 `Value`，对 LLM 客户端零成本。
pub fn write_tool_schemas() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "create_map",
            "新建一张地图（MapNode 节点 + 父引用回填）。v0.1 不带预填充栅格（地形走后续 skill）。",
            create_map_schema(),
        ),
        (
            "create_place",
            "新增一个地点（Place：id / name / 可选 map / 可选 x,y 可选 / w,h）。",
            create_place_schema(),
        ),
        (
            "undo",
            "撤销最后一次已提交的事务；返回该事务的脏路径集合。栈空时不报错，返回 false。",
            undo_schema(),
        ),
        (
            "save_project",
            "把当前世界全量保存到 `WorldProjection.project_dir`。无工程目录时报错。",
            save_project_schema(),
        ),
        (
            "set_nature_names",
            "批量命名自然区域（ADR-0082）：names = [{face_id, name}]。先把 get_naming_context 的两级备注（风格来源）展示给用户、用户确认后再提案名字调用本工具。空串名字 = 清名（回退派生名「草原N」）。命名就地生效且跨时间一致；保存工程用 save_project。",
            set_nature_names_schema(),
        ),
    ]
}

pub fn set_nature_names_schema() -> Value {
    json!({
        "type": "object",
        "required": ["map_id", "layer_id", "names"],
        "properties": {
            "map_id": {"type": "string", "description": "目标地图 id"},
            "layer_id": {"type": "string", "description": "自然层 id（get_naming_context 返回的 layer_id）"},
            "names": {
                "type": "array",
                "description": "命名表",
                "items": {
                    "type": "object",
                    "required": ["face_id", "name"],
                    "properties": {
                        "face_id": {"type": "integer", "description": "脸 id（get_naming_context 的 faces[].face_id）"},
                        "name": {"type": "string", "description": "新名字；空串 = 清名"}
                    },
                    "additionalProperties": false
                }
            }
        },
        "additionalProperties": false,
    })
}

pub fn create_map_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "新图 ID（必填；A-Za-z0-9_，§4.4）"},
            "name": {"type": "string", "description": "显示名"},
            "level": {"type": "string", "enum": ["world", "region", "city", "interior"], "description": "层级"},
            "parent": {"type": "string", "description": "父图 ID；根图为 null"},
            "size_w": {"type": "number", "description": "extent 宽度"},
            "size_h": {"type": "number", "description": "extent 高度"}
        },
        "required": ["id", "name", "level", "size_w", "size_h"],
        "additionalProperties": false
    })
}

pub fn create_place_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "地点 ID（必填；§4.4）"},
            "name": {"type": "string", "description": "显示名"},
            "map": {"type": "string", "description": "锚定到地图的 ID（可选）"},
            "x": {"type": "number", "description": "中心点 X（地图局部坐标）"},
            "y": {"type": "number", "description": "中心点 Y（地图局部坐标）"},
            "w": {"type": "number", "description": "范围宽度（与 h 成对）"},
            "h": {"type": "number", "description": "范围高度（与 w 成对）"},
            "kind": {
                "type": "string",
                "description": "POI 类别（ADR-0064；地图上画成彩色图标钉）：settlement / peak / port / station / scenic / food / lodging / medical / education / government / shop / ruins / arena / shrine / portal / danger / other"
            },
            "rank": {
                "type": "integer",
                "minimum": 0,
                "maximum": 3,
                "description": "呈现级别（ADR-0047）：3=国家级 / 2=区域市镇级 / 1=局部 / 0=微点；rank≥2 全档可见且画图标钉。缺省按 w/h 折算（有 w/h → 城市符号）"
            }
        },
        "required": ["id", "name"],
        "additionalProperties": false
    })
}

pub fn undo_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

pub fn save_project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMapInput {
    id: String,
    name: String,
    level: String,
    #[serde(default)]
    parent: Option<String>,
    size_w: f32,
    size_h: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreatePlaceInput {
    id: String,
    name: String,
    #[serde(default)]
    map: Option<String>,
    #[serde(default)]
    x: Option<f32>,
    #[serde(default)]
    y: Option<f32>,
    #[serde(default)]
    w: Option<f32>,
    #[serde(default)]
    h: Option<f32>,
    /// POI 类别（词表 = `PoiKind::key()`；非法值报 INVALID_PARAMS）。
    #[serde(default)]
    kind: Option<String>,
    /// 呈现级别（0..=3）；缺省按 w/h 折算。
    #[serde(default)]
    rank: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UndoInput {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveProjectInput {}

/// 路由：写类 skill 的 dispatch。
pub async fn dispatch_write(
    bridge: &HttpBridge,
    name: &str,
    args: Value,
) -> Result<Value, JsonRpcError> {
    match name {
        "create_map" => {
            let input: CreateMapInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("create_map: {e}")))?;
            let id = wv_core::Id::new(&input.id).map_err(|e| {
                JsonRpcError::invalid_params(format!("id 非法（§4.4 字符集）: {e}"))
            })?;
            let parent = match input.parent.as_deref() {
                Some(s) => Some(
                    wv_core::Id::new(s)
                        .map_err(|e| JsonRpcError::invalid_params(format!("parent 非法: {e}")))?,
                ),
                None => None,
            };
            let level = parse_level(&input.level)?;
            let node = wv_core::map::MapNode {
                id: id.clone(),
                comment: None,
                name: input.name.clone(),
                level,
                parent,
                children: Vec::new(),
                icons: Vec::new(),
                layers: Vec::new(),
                extent: Some(wv_core::map::Extent::sized(input.size_w, input.size_h)),
                raster: None,
                geographic_source: None,
                time_span: None,
                unit_per_km: None,
                orientation: 0.0,
                // v0.1 不声明底图；后续 skill 可单独设，或直接「重置到底图」（按 Ocean 兜底）。
                base: None,
                outline_polygon: None,
                // ADR-0075 软删字段：新建图天然未删。该构造点此前漏补，致
                // `--features http` 整体编译不过——http 不在标准门禁内的盲区实证。
                trashed: false,
                trashed_at: None,
            };
            // v0.1 不带预填充栅格；地形由后续 skill 单独填。
            let cmd: Box<dyn wv_edit::EditCommand> =
                Box::new(wv_edit::AddMapCommand::new(node, None));
            let dirty = bridge
                .apply(cmd)
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("create_map apply 失败: {e}")))?;
            Ok(json!({
                "id": input.id,
                "dirty_paths": dirty,
            }))
        }
        "create_place" => {
            let input: CreatePlaceInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("create_place: {e}")))?;
            let id = wv_core::Id::new(&input.id).map_err(|e| {
                JsonRpcError::invalid_params(format!("id 非法（§4.4 字符集）: {e}"))
            })?;
            let map = match input.map.as_deref() {
                Some(s) => Some(
                    wv_core::Id::new(s)
                        .map_err(|e| JsonRpcError::invalid_params(format!("map 非法: {e}")))?,
                ),
                None => None,
            };
            // w/h 必须成对出现（§2.x Place 字段约束）；单边报 INVALID_PARAMS。
            if input.w.is_some() != input.h.is_some() {
                return Err(JsonRpcError::invalid_params(
                    "w / h 必须同时出现或同时省略（点 vs 矩形的二元语义）",
                ));
            }
            // POI 类别词表（ADR-0064）：非法值显式报错——静默回退会让作者
            // 以为写对了（工具面是 AI 的输入面，拼错必须可见）。
            let kind = match input.kind.as_deref() {
                None => None,
                Some(raw) => {
                    let norm = raw.trim().to_ascii_lowercase();
                    match wv_core::distance::PoiKind::ALL
                        .iter()
                        .copied()
                        .find(|k| k.key() == norm)
                    {
                        Some(k) => Some(k),
                        None => {
                            return Err(JsonRpcError::invalid_params(format!(
                                "kind 非法：`{raw}`；词表：{}",
                                wv_core::distance::PoiKind::ALL
                                    .iter()
                                    .map(|k| k.key())
                                    .collect::<Vec<_>>()
                                    .join(" / ")
                            )))
                        }
                    }
                }
            };
            // 呈现级别上限 3（ADR-0047 词表：0..=3）。
            if let Some(rank) = input.rank {
                if rank > 3 {
                    return Err(JsonRpcError::invalid_params(format!(
                        "rank 越界：{rank}（合法 0..=3）"
                    )));
                }
            }
            let place = wv_core::distance::Place {
                kind,
                id,
                name: input.name.clone(),
                map,
                x: input.x,
                y: input.y,
                w: input.w,
                h: input.h,
                feature: None,
                biome: None,
                envelope: None,
                rank: input.rank,
                line: None,
                line_kind: None,
                line_width_m: None,
            };
            let cmd: Box<dyn wv_edit::EditCommand> = Box::new(wv_edit::AddPlaceCommand::new(place));
            let dirty = bridge.apply(cmd).await.map_err(|e| {
                JsonRpcError::internal_error(format!("create_place apply 失败: {e}"))
            })?;
            Ok(json!({
                "id": input.id,
                "dirty_paths": dirty,
            }))
        }
        "undo" => {
            let _input: UndoInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("undo: {e}")))?;
            let (did, dirty) = bridge
                .undo()
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("undo 失败: {e}")))?;
            Ok(json!({
                "did_undo": did,
                "dirty_paths": dirty,
            }))
        }
        "save_project" => {
            let _input: SaveProjectInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("save_project: {e}")))?;
            let outcome = bridge
                .save()
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("save_project 失败: {e:?}")))?;
            Ok(json!({
                "saved": true,
                "dirty_paths": outcome.dirty_paths,
            }))
        }
        "set_nature_names" => {
            #[derive(serde::Deserialize)]
            struct NameEntry {
                face_id: u32,
                name: String,
            }
            #[derive(serde::Deserialize)]
            struct SetNamesInput {
                map_id: String,
                layer_id: String,
                names: Vec<NameEntry>,
            }
            let input: SetNamesInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("set_nature_names: {e}")))?;
            let map = wv_core::Id::new(&input.map_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("map_id 非法: {e}")))?;
            let layer = wv_core::Id::new(&input.layer_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("layer_id 非法: {e}")))?;
            let cmd = wv_edit::SetNatureFaceNamesCommand {
                map,
                layer,
                names: input
                    .names
                    .into_iter()
                    .map(|n| (n.face_id, n.name))
                    .collect(),
                before: std::sync::Mutex::new(None),
            };
            let dirty = bridge.apply(Box::new(cmd)).await.map_err(|e| {
                JsonRpcError::internal_error(format!("set_nature_names 失败: {e:?}"))
            })?;
            Ok(json!({
                "named": true,
                "dirty_paths": dirty,
            }))
        }
        other => Err(JsonRpcError::method_not_found(other)),
    }
}

fn parse_level(s: &str) -> Result<wv_core::map::MapLevel, JsonRpcError> {
    use wv_core::map::MapLevel;
    match s {
        "world" => Ok(MapLevel::World),
        "region" => Ok(MapLevel::Region),
        "city" => Ok(MapLevel::City),
        "interior" => Ok(MapLevel::Interior),
        other => Err(JsonRpcError::invalid_params(format!(
            "level 未知: `{other}`；仅 world/region/city/interior"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::drain_messages;
    use serde_json::json;

    use wv_core::map::MapLevel;

    fn setup() -> (
        HttpBridge,
        tokio::sync::mpsc::UnboundedReceiver<crate::http::bridge::HttpMessage>,
        wv_core::World,
        wv_edit::History,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        (
            bridge,
            rx,
            wv_core::World::default(),
            wv_edit::History::default(),
        )
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    /// 起一个 tokio runtime 并发跑 drain（tokio::spawn），让 `dispatch_write`
    /// 的 send + await 链路能闭环。这是 production 形态（Bevy 主帧是另一个
    /// 线程跑 drain）的最小测试替身——同 `skills/read.rs` 的同款工具。
    async fn dispatch_write_with_drain(
        bridge: HttpBridge,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::http::bridge::HttpMessage>,
        mut world: wv_core::World,
        mut history: wv_edit::History,
        name: &'static str,
        args: Value,
    ) -> Result<Value, JsonRpcError> {
        let drain = tokio::spawn(async move {
            for _ in 0..100_000 {
                drain_messages(&mut rx, &mut world, &mut history, None);
                tokio::task::yield_now().await;
            }
        });
        let result = dispatch_write(&bridge, name, args).await;
        drain.abort();
        result
    }

    #[test]
    fn create_map_with_dangling_parent_returns_internal_error() {
        // 与 create_map_with_existing_parent_succeeds 互为对照：父引用
        // 悬空时 AddMapCommand 应报 EditError（目标不存在），dispatch_write
        // 把它包成 INTERNAL_ERROR（-32603）。LLM 客户端看到 isError=true。
        let rt = rt();
        let err = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_write_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "create_map",
                    json!({
                        "id": "city_a",
                        "name": "城市甲",
                        "level": "city",
                        "parent": "ghost_parent",
                        "size_w": 1024.0,
                        "size_h": 1024.0,
                    }),
                )
                .await
            })
            .unwrap_err();
        assert_eq!(err.code, -32603, "EditError → INTERNAL_ERROR");
        assert!(err.message.contains("父地图"));
    }

    #[test]
    fn create_map_with_existing_parent_succeeds() {
        let rt = rt();
        let v = rt
            .block_on(async {
                use wv_core::map::{Extent, MapNode};
                use wv_core::Id;
                let (bridge, rx, mut world, history) = setup();
                world.maps.push(MapNode {
                    comment: None,
                    id: Id::new_unchecked("world_main"),
                    name: "主世界".into(),
                    level: MapLevel::World,
                    parent: None,
                    children: vec![],
                    icons: vec![],
                    layers: vec![],
                    extent: Some(Extent::sized(4096.0, 4096.0)),
                    raster: None,
                    geographic_source: None,
                    time_span: None,
                    unit_per_km: None,
                    orientation: 0.0,
                    base: None,
                    outline_polygon: None,
                    trashed: false,
                    trashed_at: None,
                });
                dispatch_write_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "create_map",
                    json!({
                        "id": "city_a",
                        "name": "城市甲",
                        "level": "city",
                        "parent": "world_main",
                        "size_w": 1024.0,
                        "size_h": 1024.0,
                    }),
                )
                .await
            })
            .unwrap();
        // 合并 main（unit_per_km）时丢失的断言补回：create_map 成功路径
        // 应回报新图 id 与非空脏路径（`maps` 目录）。
        assert_eq!(v["id"], "city_a");
        assert!(
            v["dirty_paths"].as_array().is_some_and(|p| !p.is_empty()),
            "create_map 应产生脏路径；raw = {v}"
        );
    }

    #[test]
    fn create_map_with_unknown_level_is_invalid_params() {
        let (bridge, _rx, _world, _history) = setup();
        let err = rt()
            .block_on(dispatch_write(
                &bridge,
                "create_map",
                json!({
                    "id": "x", "name": "X", "level": "galaxy",
                    "size_w": 1.0, "size_h": 1.0,
                }),
            ))
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn create_map_with_invalid_id_char_is_invalid_params() {
        let (bridge, _rx, _world, _history) = setup();
        let err = rt()
            .block_on(dispatch_write(
                &bridge,
                "create_map",
                json!({
                    "id": "city-with-dash",
                    "name": "X",
                    "level": "city",
                    "size_w": 1.0, "size_h": 1.0,
                }),
            ))
            .unwrap_err();
        assert_eq!(err.code, -32602, "§4.4 字符集违规应被拦截");
    }

    #[test]
    fn create_place_round_trip() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_write_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "create_place",
                    json!({
                        "id": "qingyun",
                        "name": "青云城",
                        "x": 100.0, "y": 200.0,
                    }),
                )
                .await
            })
            .unwrap();
        assert_eq!(v["id"], "qingyun");
    }

    #[test]
    fn create_place_requires_w_h_to_be_paired() {
        let (bridge, _rx, _world, _history) = setup();
        let err = rt()
            .block_on(dispatch_write(
                &bridge,
                "create_place",
                json!({"id": "p", "name": "P", "w": 10.0}),
            ))
            .unwrap_err();
        assert_eq!(err.code, -32602);
        let err = rt()
            .block_on(dispatch_write(
                &bridge,
                "create_place",
                json!({"id": "p2", "name": "P2", "h": 10.0}),
            ))
            .unwrap_err();
        assert_eq!(err.code, -32602);
    }

    #[test]
    fn undo_returns_dirty_paths() {
        // **必须共享 world/history**：create_place 与 undo 用同一个 setup，
        // 否则 history 在两次 block_on 间被丢弃，undo 看到空栈返回 false。
        let rt = rt();
        let v = rt.block_on(async {
            let (bridge, rx, world, history) = setup();
            // 起 drain（move rx），让后续 dispatch 闭环。
            let drain = tokio::spawn(async move {
                let mut world = world;
                let mut history = history;
                let mut rx = rx;
                for _ in 0..100_000 {
                    drain_messages(&mut rx, &mut world, &mut history, None);
                    tokio::task::yield_now().await;
                }
            });
            let v1 = dispatch_write(
                &bridge.clone(),
                "create_place",
                json!({"id": "qingyun", "name": "青云城"}),
            )
            .await
            .unwrap();
            let v2 = dispatch_write(&bridge, "undo", json!({})).await.unwrap();
            drain.abort();
            (v1, v2)
        });
        assert_eq!(v.0["id"], "qingyun");
        assert_eq!(v.1["did_undo"], true);
        assert!(
            v.1["dirty_paths"]
                .as_array()
                .unwrap()
                .contains(&json!("distances/places")),
            "应回报 commit 时的脏路径"
        );
    }

    #[test]
    fn undo_with_empty_history_returns_did_undo_false() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_write_with_drain(bridge, rx, world, history, "undo", json!({})).await
            })
            .unwrap();
        assert_eq!(v["did_undo"], false);
    }

    #[test]
    fn save_project_without_open_project_is_internal_error() {
        let rt = rt();
        let err = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_write_with_drain(bridge, rx, world, history, "save_project", json!({}))
                    .await
            })
            .unwrap_err();
        assert_eq!(
            err.code, -32603,
            "无 project_dir → INTERNAL_ERROR（V1 守约）"
        );
    }

    #[test]
    fn unknown_write_skill_is_method_not_found() {
        let (bridge, _rx, _world, _history) = setup();
        let err = rt()
            .block_on(dispatch_write(&bridge, "delete_map", json!({})))
            .unwrap_err();
        assert_eq!(err.code, -32601);
    }
}
