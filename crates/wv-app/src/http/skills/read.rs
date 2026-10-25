//! 读类 5 个 skill（ADR-0014 §2.5 + §2.5.1）。
//!
//! ## 不经 EditCommand
//! 读路径**直接**查 WorldProjection.world 的内存快照——handler 经
//! [`HttpBridge::read`] 把 `ReadRequest` 投递到 Bevy 主帧，主帧用
//! `TimelineQuery::at`（P7 唯一入口）求值后返回。**不**经 EditCommand。

use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::bridge::{HttpBridge, ReadReply, ReadRequest};
use crate::http::filter::Filter as ParsedFilter;
use crate::http::mcp::JsonRpcError;

/// 读类 skill 的 JSON Schema 列表。`tools/list` 把这些直接喂给 LLM。
///
/// **为什么是 `fn()` 不是 `const`**：`json!` 宏不工作在常量上下文
///（serde_json::Map::new / to_value 都不是 const fn）。改用函数 —— 每次
/// `tools/list` 调用构造一份新的 `Value`，对 LLM 客户端零成本。
pub fn read_tool_schemas() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "list_maps",
            "列出工程中所有地图的摘要（id / name / parent / size_w / size_h）；不暴露 layers / terrain / icons。",
            list_maps_schema(),
        ),
        (
            "get_map",
            "取单图完整 MapNode（含 layers / children / icons / extent / raster）。",
            get_map_schema(),
        ),
        (
            "list_places",
            "列出地点摘要（id / name / map / x / y），按 map_id 过滤；filter 用 Mini-DSL（name='X' AND kind~'Y'）。",
            list_places_schema(),
        ),
        (
            "get_place",
            "取单地点完整 Place（含 map / x / y / w / h）。",
            get_place_schema(),
        ),
        (
            "get_state_at_time",
            "P7 入口：返回指定图在指定时刻的「所有要素投影」（characters / events / places 摘要）。",
            get_state_at_time_schema(),
        ),
    ]
}

pub fn list_maps_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false,
    })
}

pub fn get_map_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "地图 ID（必填）"}
        },
        "required": ["id"],
        "additionalProperties": false,
    })
}

pub fn list_places_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "map_id": {"type": "string", "description": "按所在图 ID 过滤；省略 = 全部地点"},
            "filter": {"type": "string", "description": "Mini-DSL: name='X' AND name~'Y'。当前支持 `=` 与 `~`，仅 AND 合取"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 100},
            "cursor": {"type": "string", "description": "翻页游标（首次调用不传；后续从上一次响应的 next_cursor 取）"}
        },
        "additionalProperties": false,
    })
}

pub fn get_place_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "地点 ID（必填）"}
        },
        "required": ["id"],
        "additionalProperties": false,
    })
}

pub fn get_state_at_time_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "map_id": {"type": "string", "description": "地图 ID（必填）"},
            "day": {"type": "integer", "description": "时间轴上的天数（必填，§3.2 P7）"}
        },
        "required": ["map_id", "day"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListMapsInput {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GetMapInput {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListPlacesInput {
    #[serde(default)]
    map_id: Option<String>,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default = "default_limit")]
    limit: u32,
    #[serde(default)]
    cursor: Option<String>,
}

fn default_limit() -> u32 {
    100
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GetPlaceInput {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GetStateAtTimeInput {
    map_id: String,
    day: i64,
}

/// 路由：读类 skill 的 dispatch。`name` 必须命中 5 个之一；否则
/// `Err(JsonRpcError::method_not_found)`。
pub async fn dispatch_read(
    bridge: &HttpBridge,
    name: &str,
    args: Value,
) -> Result<Value, JsonRpcError> {
    match name {
        "list_maps" => {
            let _input: ListMapsInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("list_maps: {e}")))?;
            let reply = bridge.read(ReadRequest::ListMaps).await;
            Ok(match reply {
                ReadReply::MapSummaries(v) => serde_json::to_value(v)
                    .map_err(|e| JsonRpcError::internal_error(e.to_string()))?,
                _ => unreachable!("ListMaps 应回 Mapsummaries"),
            })
        }
        "get_map" => {
            let input: GetMapInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("get_map: {e}")))?;
            let id = wv_core::Id::new(&input.id)
                .map_err(|e| JsonRpcError::invalid_params(format!("get_map.id 非法: {e}")))?;
            let reply = bridge.read(ReadRequest::GetMap { id }).await;
            Ok(match reply {
                ReadReply::MapNode(Some(node)) => serde_json::to_value(&node)
                    .map_err(|e| JsonRpcError::internal_error(e.to_string()))?,
                ReadReply::MapNode(None) => json!({
                    "error": "map_not_found",
                    "id": input.id,
                }),
                _ => unreachable!("GetMap 应回 MapNode"),
            })
        }
        "list_places" => {
            let input: ListPlacesInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("list_places: {e}")))?;
            let map_id = match input.map_id {
                Some(s) => Some(
                    wv_core::Id::new(&s)
                        .map_err(|e| JsonRpcError::invalid_params(format!("map_id: {e}")))?,
                ),
                None => None,
            };
            let filter =
                match input.filter {
                    Some(s) if !s.is_empty() => Some(ParsedFilter::parse(&s).map_err(|e| {
                        JsonRpcError::invalid_params(format!("filter 解析失败: {e}"))
                    })?),
                    _ => None,
                };
            if input.limit > 100 {
                return Err(JsonRpcError::invalid_params(
                    "limit 上限 100；超出会触发分页，用 cursor 续取",
                ));
            }
            let reply = bridge
                .read(ReadRequest::ListPlaces {
                    map_id,
                    filter,
                    cursor: input.cursor,
                    limit: input.limit,
                })
                .await;
            Ok(match reply {
                ReadReply::PlaceSummaries { items, next_cursor } => json!({
                    "items": items,
                    "next_cursor": next_cursor,
                }),
                _ => unreachable!("ListPlaces 应回 PlaceSummaries"),
            })
        }
        "get_place" => {
            let input: GetPlaceInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("get_place: {e}")))?;
            let id = wv_core::Id::new(&input.id)
                .map_err(|e| JsonRpcError::invalid_params(format!("get_place.id 非法: {e}")))?;
            let reply = bridge.read(ReadRequest::GetPlace { id }).await;
            Ok(match reply {
                ReadReply::Place(Some(place)) => serde_json::to_value(&place)
                    .map_err(|e| JsonRpcError::internal_error(e.to_string()))?,
                ReadReply::Place(None) => json!({
                    "error": "place_not_found",
                    "id": input.id,
                }),
                _ => unreachable!("GetPlace 应回 Place"),
            })
        }
        "get_state_at_time" => {
            let input: GetStateAtTimeInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("get_state_at_time: {e}")))?;
            let map_id = wv_core::Id::new(&input.map_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("map_id 非法: {e}")))?;
            let reply = bridge
                .read(ReadRequest::GetStateAtTime {
                    map_id,
                    day: input.day,
                })
                .await;
            Ok(match reply {
                ReadReply::StateAtTime(v) => v,
                _ => unreachable!("GetStateAtTime 应回 StateAtTime"),
            })
        }
        other => Err(JsonRpcError::method_not_found(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::drain_messages;
    use serde_json::json;

    /// 构造「世界 + 两个子图 + 一个 Place」的测试夹具。
    fn setup() -> (
        HttpBridge,
        tokio::sync::mpsc::UnboundedReceiver<crate::http::bridge::HttpMessage>,
        wv_core::World,
        wv_edit::History,
    ) {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let mut world = wv_core::World::default();
        world.maps.push(MapNode {
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![Id::new_unchecked("city_a")],
            icons: vec![],
            layers: vec![],
            extent: Some(wv_core::map::Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });
        world.maps.push(MapNode {
            id: Id::new_unchecked("city_a"),
            name: "城市甲".into(),
            level: MapLevel::City,
            parent: Some(Id::new_unchecked("world_main")),
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });
        world.distances.places.push(wv_core::distance::Place {
            kind: None,
            id: Id::new_unchecked("qingyun"),
            name: "青云城".into(),
            map: Some(Id::new_unchecked("world_main")),
            x: Some(100.0),
            y: Some(200.0),
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
        });
        (bridge, rx, world, wv_edit::History::default())
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    /// 起一个 tokio runtime 并发跑 drain（tokio::spawn），让 `dispatch_read`
    /// 的 send + await 链路能闭环。这是 production 形态（Bevy 主帧是另一个
    /// 线程跑 drain）的最小测试替身。
    async fn dispatch_read_with_drain(
        bridge: HttpBridge,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::http::bridge::HttpMessage>,
        mut world: wv_core::World,
        mut history: wv_edit::History,
        name: &'static str,
        args: Value,
    ) -> Result<Value, JsonRpcError> {
        // 起后台 drain 任务：每 100µs 排空一次。生产场景 Bevy 主帧以 60fps
        // 跑（≈16ms / 帧），100µs 比那还快——LLM 调用本就比帧慢，够用。
        let drain = tokio::spawn(async move {
            for _ in 0..100_000 {
                drain_messages(&mut rx, &mut world, &mut history, None);
                tokio::task::yield_now().await;
            }
        });
        let result = dispatch_read(&bridge, name, args).await;
        drain.abort();
        result
    }

    #[test]
    fn list_maps_round_trip() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_read_with_drain(bridge, rx, world, history, "list_maps", json!({})).await
            })
            .unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "world_main");
    }

    #[test]
    fn get_map_round_trip_existing() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_read_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "get_map",
                    json!({"id": "city_a"}),
                )
                .await
            })
            .unwrap();
        assert_eq!(v["id"], "city_a");
        assert_eq!(v["level"], "city");
    }

    #[test]
    fn get_map_missing_returns_error_payload_not_none() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_read_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "get_map",
                    json!({"id": "ghost"}),
                )
                .await
            })
            .unwrap();
        assert_eq!(v["error"], "map_not_found");
    }

    #[test]
    fn list_places_with_filter_and_limit() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_read_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "list_places",
                    json!({"filter": "name ~ '青'", "limit": 50}),
                )
                .await
            })
            .unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        assert_eq!(v["items"][0]["id"], "qingyun");
    }

    #[test]
    fn list_places_limit_above_100_is_invalid_params() {
        // 这条路径在 dispatch 阶段就拒，**不**经 bridge.read——不需要 drain。
        let (bridge, _rx, _w, _h) = setup();
        let err = rt()
            .block_on(dispatch_read(&bridge, "list_places", json!({"limit": 200})))
            .unwrap_err();
        assert_eq!(err.code, -32602, "limit > 100 → INVALID_PARAMS");
    }

    #[test]
    fn get_place_round_trip() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_read_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "get_place",
                    json!({"id": "qingyun"}),
                )
                .await
            })
            .unwrap();
        assert_eq!(v["id"], "qingyun");
        assert_eq!(v["name"], "青云城");
        assert_eq!(v["x"], 100.0);
        assert_eq!(v["y"], 200.0);
    }

    #[test]
    fn get_state_at_time_round_trip_with_p7_query() {
        let rt = rt();
        let v = rt
            .block_on(async {
                let (bridge, rx, world, history) = setup();
                dispatch_read_with_drain(
                    bridge,
                    rx,
                    world,
                    history,
                    "get_state_at_time",
                    json!({"map_id": "world_main", "day": 1}),
                )
                .await
            })
            .unwrap();
        assert_eq!(v["map_id"], "world_main");
        assert_eq!(v["day"], 1);
        assert!(v["characters"].is_array());
        assert!(v["events"].is_array());
        assert!(v["places"].is_array());
    }

    #[test]
    fn unknown_read_skill_is_method_not_found() {
        let (bridge, _rx, _w, _h) = setup();
        let err = rt()
            .block_on(dispatch_read(&bridge, "list_narrations", json!({})))
            .unwrap_err();
        assert_eq!(err.code, -32601);
    }

    #[test]
    fn additional_properties_are_rejected() {
        let (bridge, _rx, _w, _h) = setup();
        let err = rt()
            .block_on(dispatch_read(
                &bridge,
                "get_map",
                json!({"id": "world_main", "evil": true}),
            ))
            .unwrap_err();
        assert_eq!(err.code, -32602, "未知字段应被拒绝");
    }
}
