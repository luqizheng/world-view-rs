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
            "新建一张地图（MapNode 节点 + 父引用回填）。level=world 且给 cell_size 时预填全海栅格（地形工具的前提，ADR-0099）；省略 cell_size = 不带栅格。",
            create_map_schema(),
        ),
        (
            "generate_continent",
            "大陆生成（ADR-0099）：按意图包络跑海岸线/起伏/森林荒漠/大陆架/离岸小岛。要求目标图已有栅格（create_map 时给 cell_size 即可）。",
            generate_continent_schema(),
        ),
        (
            "paint_terrain_intent",
            "地形意图（ADR-0099）：山脉 / 高原 / 丘陵 / 火山 / 湖 / 河，点状或区域一次生效。位置用图内局部 wu。",
            paint_terrain_intent_schema(),
        ),
        (
            "create_faction",
            "新增势力（ADR-0099）：id / name / color(#RRGGBB) / 可选 rank / seat / layer。已有同 ID 会报错，先 list_factions。",
            create_faction_schema(),
        ),
        (
            "divide_faction",
            "按意图多边形切出势力领土（ADR-0099，「切蛋糕」）：host=被切的宿主势力（null=空白地），mode=snap（吸附水陆/河/山脊）或 straight。回执带 snapped 标志表明是否真的吸附到地物。",
            divide_faction_schema(),
        ),
        (
            "reset_terrain",
            "把整张图的地形重置回它的底图（MapBase：缺省海洋），一条命令、可 undo。调地形配方时的重 roll 入口——重置后再 generate_continent 即可换一套岸线/起伏。",
            reset_terrain_schema(),
        ),
        (
            "create_place",
            "新增一个地点（Place：id / name / 可选 map / 可选 x,y 可选 / w,h）。",
            create_place_schema(),
        ),
        (
            "update_place",
            "改地点的名称 / 呈现级别 / 坐标（ADR-0099）。只改名/级别时不必带坐标。",
            update_place_schema(),
        ),
        (
            "set_place_kind",
            "改地点的 POI 类别（ADR-0099）；kind=null 清空。适用于把占位类别转正。",
            set_place_kind_schema(),
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
            "size_h": {"type": "number", "description": "extent 高度"},
            "cell_size": {
                "type": "number",
                "description": "每格世界单位（ADR-0099）。level=world 时给出 → 按 size/cell_size 铺全海栅格（地形/切蛋糕工具的前提，见 generate_continent）；省略 = 不带栅格（沿用父图底图或后续由 GUI 铺底）。此值被栅格尺寸钉死，后续地形调用的 cell_size 必须与之一致"
            }
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

/// `[[x, y], ...]` 点列的公共 schema 片段（图内局部 wu）。
fn points_schema(min_items: usize) -> Value {
    json!({
        "type": "array",
        "minItems": min_items,
        "items": {
            "type": "array",
            "minItems": 2,
            "maxItems": 2,
            "items": {"type": "number"}
        }
    })
}

pub fn generate_continent_schema() -> Value {
    json!({
        "type": "object",
        "required": ["map_id", "envelopes", "cell_size", "seed"],
        "properties": {
            "map_id": {"type": "string", "description": "目标地图 ID（须已有栅格）"},
            "envelopes": {
                "type": "array",
                "description": "大陆包络（每条 ≥3 点；图内局部 wu）。多圈 = 多块陆地",
                "minItems": 1,
                "items": points_schema(3)
            },
            "cell_size": {"type": "number", "description": "每格世界单位；必须与 create_map 时给的一致"},
            "seed": {"type": "integer", "description": "骰子种子；换值 = 换一份岸线细节"},
            "params": {
                "type": "object",
                "description": "大陆配方（ADR-0008 八参数）；省略 = 缺省值",
                "properties": {
                    "coast_wiggle": {"type": "number", "description": "海岸蜿蜒度 0–1（0=贴多边形边）"},
                    "coast_octaves": {"type": "integer", "description": "海岸细节 fBm octaves（2/4/6）"},
                    "relief": {"type": "string", "enum": ["hills", "mountains", "high_mountains"], "description": "起伏强度档"},
                    "forest_cover": {"type": "number", "description": "森林覆盖倾向 0–1"},
                    "desert_cover": {"type": "number", "description": "荒漠覆盖倾向 0–1"},
                    "shelf_cells": {"type": "integer", "description": "大陆架宽度（格）"},
                    "islands": {"type": "string", "enum": ["off", "low", "medium", "high"], "description": "离岸小岛档"}
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    })
}

pub fn paint_terrain_intent_schema() -> Value {
    json!({
        "type": "object",
        "required": ["map_id", "intent", "cell_size"],
        "properties": {
            "map_id": {"type": "string", "description": "目标地图 ID（须已有栅格）"},
            "cell_size": {"type": "number", "description": "每格世界单位；须与建图时一致"},
            "intent": {
                "type": "object",
                "required": ["kind"],
                "description": "地形意图；字段按 kind 取（点状用 center，线状用 pts，区域用 polygon）",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": [
                            "mountain_range", "plateau", "hills", "volcano", "lake", "river",
                            "mountain_massif", "hills_region", "plains_region", "lake_region"
                        ],
                        "description": "mountain_range/river 用 pts；volcano/lake 用 center；*_region / mountain_massif 用 polygon"
                    },
                    "pts": points_schema(1),
                    "polygon": points_schema(3),
                    "center": {"type": "array", "minItems": 2, "maxItems": 2, "items": {"type": "number"}},
                    "radius": {"type": "number", "description": "半宽（世界单位）"},
                    "strength": {"type": "number", "description": "强度：山脉/火山 [0.2,2.0]；高原/丘陵/湖 同义缩放"}
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    })
}

pub fn create_faction_schema() -> Value {
    json!({
        "type": "object",
        "required": ["faction"],
        "properties": {
            "faction": {
                "type": "object",
                "required": ["id", "name", "color"],
                "properties": {
                    "id": {"type": "string", "description": "势力 ID（§4.4）"},
                    "name": {"type": "string", "description": "显示名"},
                    "color": {"type": "string", "description": "势力色 #RRGGBB"},
                    "rank": {"type": "string", "description": "渲染提示（kingdom / tribe / free_city …；自由串）"},
                    "seat": {"type": "string", "description": "首府 Place ID（可选）"},
                    "layer": {"type": "string", "description": "所属势力层 ID（可选；省略=默认层）"}
                },
                "additionalProperties": false
            }
        },
        "additionalProperties": false
    })
}

pub fn divide_faction_schema() -> Value {
    json!({
        "type": "object",
        "required": ["map_id", "intent", "mode", "faction", "cell_size"],
        "properties": {
            "map_id": {"type": "string", "description": "目标地图 ID（须已有栅格）"},
            "host": {"type": ["string", "null"], "description": "被切的宿主势力 ID；null = 从空白地切"},
            "intent": points_schema(3),
            "mode": {"type": "string", "enum": ["snap", "straight"], "description": "snap=边界吸附水陆/河/山脊；straight=保留平滑边框"},
            "faction": {
                "type": "object",
                "required": ["id", "name", "color"],
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "color": {"type": "string", "description": "#RRGGBB"},
                    "rank": {"type": "string"},
                    "seat": {"type": "string"},
                    "layer": {"type": "string"}
                },
                "additionalProperties": false
            },
            "from": {"type": "integer", "description": "生效日（TimeSpan::from）；省略 = 0"},
            "cell_size": {"type": "number", "description": "每格世界单位；须与建图时一致"}
        },
        "additionalProperties": false
    })
}

/// 重置地形到底图（ADR-0099 追记 B）。
pub fn reset_terrain_schema() -> Value {
    json!({
        "type": "object",
        "required": ["map_id"],
        "properties": {
            "map_id": {"type": "string", "description": "目标地图 ID"}
        },
        "additionalProperties": false
    })
}

pub fn update_place_schema() -> Value {
    json!({
        "type": "object",
        "required": ["place_id"],
        "properties": {
            "place_id": {"type": "string", "description": "目标地点 ID"},
            "name": {"type": "string", "description": "新名称（省略 = 不改）"},
            "rank": {"type": "integer", "minimum": 0, "maximum": 3, "description": "新呈现级别（省略 = 不改）"},
            "x": {"type": "number", "description": "新锚点 X（省略 = 不改）"},
            "y": {"type": "number", "description": "新锚点 Y（省略 = 不改）"}
        },
        "additionalProperties": false
    })
}

pub fn set_place_kind_schema() -> Value {
    json!({
        "type": "object",
        "required": ["place_id", "kind"],
        "properties": {
            "place_id": {"type": "string", "description": "目标地点 ID"},
            "kind": {
                "type": ["string", "null"],
                "description": "新类别（PoiKind 词表，见 create_place 的 kind 说明）；null = 清空"
            }
        },
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
    /// 每格世界单位；`level == "world"` 时给出即预填充全海栅格（ADR-0099）。
    #[serde(default)]
    cell_size: Option<f32>,
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

/// 大陆配方入参（全部可选；未给的取 [`wv_terrain::continent::ContinentParams`] 缺省）。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContinentParamsInput {
    #[serde(default)]
    coast_wiggle: Option<f32>,
    #[serde(default)]
    coast_octaves: Option<u32>,
    #[serde(default)]
    relief: Option<String>,
    #[serde(default)]
    forest_cover: Option<f32>,
    #[serde(default)]
    desert_cover: Option<f32>,
    #[serde(default)]
    shelf_cells: Option<u32>,
    #[serde(default)]
    islands: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerateContinentInput {
    map_id: String,
    envelopes: Vec<Vec<(f32, f32)>>,
    cell_size: f32,
    seed: u64,
    #[serde(default)]
    params: Option<ContinentParamsInput>,
}

/// 地形意图入参（按 `kind` 取不同字段；字段缺失在 handler 里报参数错）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TerrainIntentInput {
    kind: String,
    #[serde(default)]
    pts: Option<Vec<(f32, f32)>>,
    #[serde(default)]
    polygon: Option<Vec<(f32, f32)>>,
    #[serde(default)]
    center: Option<(f32, f32)>,
    #[serde(default)]
    radius: Option<f32>,
    #[serde(default)]
    strength: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaintTerrainIntentInput {
    map_id: String,
    intent: TerrainIntentInput,
    cell_size: f32,
}

/// 势力定义入参（create_faction 与 divide_faction 共用）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FactionInput {
    id: String,
    name: String,
    color: String,
    #[serde(default)]
    rank: Option<String>,
    #[serde(default)]
    seat: Option<String>,
    #[serde(default)]
    layer: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateFactionInput {
    faction: FactionInput,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DivideFactionInput {
    map_id: String,
    #[serde(default)]
    host: Option<String>,
    intent: Vec<(f32, f32)>,
    mode: String,
    faction: FactionInput,
    #[serde(default)]
    from: Option<i64>,
    cell_size: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdatePlaceInput {
    place_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    rank: Option<u8>,
    #[serde(default)]
    x: Option<f32>,
    #[serde(default)]
    y: Option<f32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPlaceKindInput {
    place_id: String,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetTerrainInput {
    map_id: String,
}

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
            if let Some(cs) = input.cell_size {
                if !(cs.is_finite() && cs > 0.0) {
                    return Err(JsonRpcError::invalid_params(format!(
                        "cell_size 必须为正有限数：{cs}"
                    )));
                }
            }
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
            // 世界图预填海洋栅格（ADR-0099）：地形三命令与切蛋糕都要求
            // `terrain_of(map).is_some()`，HTTP 建图若不带栅格，后续
            // `generate_continent` / `paint_terrain_intent` / `divide_faction`
            // 会一律 hard-fail（builder check 问题 1）。
            //
            // 口径与 GUI「新建地图」一致（`new_map_dialog.rs:542`）：只对
            // `level == world` 预填，且**仅在调用方给了 cell_size 时**——
            // 不给 = 保持旧行为（不带栅格），不破既有调用方。
            let prefill = match (level, input.cell_size) {
                (wv_core::map::MapLevel::World, Some(cs)) => {
                    Some((cs, wv_core::map::MapBase::Ocean))
                }
                _ => None,
            };
            if let Some((cs, base)) = prefill {
                let w = ((input.size_w / cs).round() as u32).max(1);
                let h = ((input.size_h / cs).round() as u32).max(1);
                let cell = base.cell();
                let terrain = wv_core::terrain::TerrainData {
                    width: w,
                    height: h,
                    cells: vec![cell; (w as usize) * (h as usize)],
                };
                let mut node = node;
                node.base = Some(base);
                let cmd: Box<dyn wv_edit::EditCommand> =
                    Box::new(wv_edit::AddMapCommand::new(node, Some(terrain)));
                let dirty = bridge.apply(cmd).await.map_err(|e| {
                    JsonRpcError::internal_error(format!("create_map apply 失败: {e}"))
                })?;
                return Ok(json!({
                    "id": input.id,
                    "grid": {"width": w, "height": h, "cell_size": cs},
                    "dirty_paths": dirty,
                }));
            }
            // 不带栅格：沿用父图底图（既有行为）。
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
        "generate_continent" => {
            let input: GenerateContinentInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("generate_continent: {e}")))?;
            let map = wv_core::Id::new(&input.map_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("map_id 非法: {e}")))?;
            if !(input.cell_size.is_finite() && input.cell_size > 0.0) {
                return Err(JsonRpcError::invalid_params(format!(
                    "cell_size 必须为正有限数：{}",
                    input.cell_size
                )));
            }
            let params = match input.params {
                None => wv_terrain::continent::ContinentParams::default(),
                Some(p) => {
                    let d = wv_terrain::continent::ContinentParams::default();
                    wv_terrain::continent::ContinentParams {
                        coast_wiggle: p.coast_wiggle.unwrap_or(d.coast_wiggle),
                        coast_octaves: p.coast_octaves.unwrap_or(d.coast_octaves),
                        relief: match p.relief.as_deref() {
                            None => d.relief,
                            Some(s) => parse_relief(s)?,
                        },
                        forest_cover: p.forest_cover.unwrap_or(d.forest_cover),
                        desert_cover: p.desert_cover.unwrap_or(d.desert_cover),
                        shelf_cells: p.shelf_cells.unwrap_or(d.shelf_cells),
                        islands: match p.islands.as_deref() {
                            None => d.islands,
                            Some(s) => parse_islands(s)?,
                        },
                    }
                }
            };
            let cmd = wv_edit::GenerateContinentCommand::new(
                map,
                input.envelopes,
                params,
                input.seed,
                input.cell_size,
            );
            let dirty = bridge.apply(Box::new(cmd)).await.map_err(|e| {
                JsonRpcError::internal_error(format!("generate_continent 失败: {e}"))
            })?;
            Ok(json!({
                "generated": true,
                "dirty_paths": dirty,
            }))
        }
        "reset_terrain" => {
            let input: ResetTerrainInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("reset_terrain: {e}")))?;
            let map = wv_core::Id::new(&input.map_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("map_id 非法: {e}")))?;
            let cmd = wv_edit::ResetTerrainToBaseCommand::new(map);
            let dirty = bridge
                .apply(Box::new(cmd))
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("reset_terrain 失败: {e}")))?;
            Ok(json!({
                "reset": true,
                "dirty_paths": dirty,
            }))
        }
        "paint_terrain_intent" => {
            let input: PaintTerrainIntentInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("paint_terrain_intent: {e}")))?;
            let map = wv_core::Id::new(&input.map_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("map_id 非法: {e}")))?;
            if !(input.cell_size.is_finite() && input.cell_size > 0.0) {
                return Err(JsonRpcError::invalid_params(format!(
                    "cell_size 必须为正有限数：{}",
                    input.cell_size
                )));
            }
            let intent = build_intent(&input.intent)?;
            let cmd = wv_edit::IntentTerrainCommand::new(map, intent, input.cell_size);
            let dirty = bridge.apply(Box::new(cmd)).await.map_err(|e| {
                JsonRpcError::internal_error(format!("paint_terrain_intent 失败: {e}"))
            })?;
            Ok(json!({
                "painted": true,
                "dirty_paths": dirty,
            }))
        }
        "create_faction" => {
            let input: CreateFactionInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("create_faction: {e}")))?;
            let faction = build_faction(&input.faction)?;
            let id = faction.id.to_string();
            let cmd = wv_edit::AddFactionCommand::new(faction);
            let dirty = bridge
                .apply(Box::new(cmd))
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("create_faction 失败: {e}")))?;
            Ok(json!({
                "id": id,
                "dirty_paths": dirty,
            }))
        }
        "divide_faction" => {
            let input: DivideFactionInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("divide_faction: {e}")))?;
            let map = wv_core::Id::new(&input.map_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("map_id 非法: {e}")))?;
            let host = match input.host.as_deref() {
                Some(s) => Some(
                    wv_core::Id::new(s)
                        .map_err(|e| JsonRpcError::invalid_params(format!("host 非法: {e}")))?,
                ),
                None => None,
            };
            if !(input.cell_size.is_finite() && input.cell_size > 0.0) {
                return Err(JsonRpcError::invalid_params(format!(
                    "cell_size 必须为正有限数：{}",
                    input.cell_size
                )));
            }
            let mode = match input.mode.as_str() {
                "snap" => wv_edit::DivisionMode::Snap,
                "straight" => wv_edit::DivisionMode::Straight,
                other => {
                    return Err(JsonRpcError::invalid_params(format!(
                        "mode 未知：`{other}`；仅 snap / straight"
                    )))
                }
            };
            let faction = build_faction(&input.faction)?;
            let id = faction.id.to_string();
            let cmd = wv_edit::DivideFactionCommand::new(
                map,
                host,
                input.intent,
                mode,
                faction,
                input.from.unwrap_or(0),
                input.cell_size,
            );
            let snapped = cmd.snapped_flag();
            let dirty = bridge
                .apply(Box::new(cmd))
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("divide_faction 失败: {e}")))?;
            let snapped = snapped.lock().map(|v| *v).unwrap_or(false);
            Ok(json!({
                "id": id,
                "snapped": snapped,
                "dirty_paths": dirty,
            }))
        }
        "update_place" => {
            let input: UpdatePlaceInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("update_place: {e}")))?;
            let id = wv_core::Id::new(&input.place_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("place_id 非法: {e}")))?;
            if let Some(rank) = input.rank {
                if rank > 3 {
                    return Err(JsonRpcError::invalid_params(format!(
                        "rank 越界：{rank}（合法 0..=3）"
                    )));
                }
            }
            // `UpdatePlaceCommand` 要求整名（无 partial 语义）——省略 name 时
            // 先读当前值补上，让「只挪点」这类调用不必回传名字。
            let name = match input.name {
                Some(n) => n,
                None => {
                    let reply = bridge
                        .read(crate::http::bridge::ReadRequest::GetPlace { id: id.clone() })
                        .await;
                    match reply {
                        crate::http::bridge::ReadReply::Place(Some(p)) => p.name,
                        _ => {
                            return Err(JsonRpcError::invalid_params(format!(
                                "place_not_found: {}",
                                input.place_id
                            )))
                        }
                    }
                }
            };
            let cmd = wv_edit::UpdatePlaceCommand::new(id, name, input.rank, input.x, input.y);
            let dirty = bridge
                .apply(Box::new(cmd))
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("update_place 失败: {e}")))?;
            Ok(json!({
                "id": input.place_id,
                "dirty_paths": dirty,
            }))
        }
        "set_place_kind" => {
            let input: SetPlaceKindInput = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("set_place_kind: {e}")))?;
            let id = wv_core::Id::new(&input.place_id)
                .map_err(|e| JsonRpcError::invalid_params(format!("place_id 非法: {e}")))?;
            let kind = match input.kind.as_deref() {
                None => None,
                Some(raw) => Some(parse_poi_kind(raw)?),
            };
            let cmd = wv_edit::SetPlaceKindCommand::new(id, kind);
            let dirty = bridge
                .apply(Box::new(cmd))
                .await
                .map_err(|e| JsonRpcError::internal_error(format!("set_place_kind 失败: {e}")))?;
            Ok(json!({
                "id": input.place_id,
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
                Some(raw) => Some(parse_poi_kind(raw)?),
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

/// `PoiKind` 词表解析（非法值显式报错——工具面是 AI 的输入面，拼错必须可见）。
fn parse_poi_kind(raw: &str) -> Result<wv_core::distance::PoiKind, JsonRpcError> {
    let norm = raw.trim().to_ascii_lowercase();
    wv_core::distance::PoiKind::ALL
        .iter()
        .copied()
        .find(|k| k.key() == norm)
        .ok_or_else(|| {
            JsonRpcError::invalid_params(format!(
                "kind 非法：`{raw}`；词表：{}",
                wv_core::distance::PoiKind::ALL
                    .iter()
                    .map(|k| k.key())
                    .collect::<Vec<_>>()
                    .join(" / ")
            ))
        })
}

fn parse_relief(s: &str) -> Result<wv_terrain::continent::ReliefLevel, JsonRpcError> {
    use wv_terrain::continent::ReliefLevel;
    match s {
        "hills" => Ok(ReliefLevel::Hills),
        "mountains" => Ok(ReliefLevel::Mountains),
        "high_mountains" => Ok(ReliefLevel::HighMountains),
        other => Err(JsonRpcError::invalid_params(format!(
            "relief 未知：`{other}`；仅 hills / mountains / high_mountains"
        ))),
    }
}

fn parse_islands(s: &str) -> Result<wv_terrain::continent::IslandDensity, JsonRpcError> {
    use wv_terrain::continent::IslandDensity;
    match s {
        "off" => Ok(IslandDensity::Off),
        "low" => Ok(IslandDensity::Low),
        "medium" => Ok(IslandDensity::Medium),
        "high" => Ok(IslandDensity::High),
        other => Err(JsonRpcError::invalid_params(format!(
            "islands 未知：`{other}`；仅 off / low / medium / high"
        ))),
    }
}

/// 势力入参 → 领域模型（ADR-0099）。
fn build_faction(input: &FactionInput) -> Result<wv_core::Faction, JsonRpcError> {
    let id = wv_core::Id::new(&input.id)
        .map_err(|e| JsonRpcError::invalid_params(format!("faction.id 非法: {e}")))?;
    let seat = match input.seat.as_deref() {
        Some(s) => Some(
            wv_core::Id::new(s)
                .map_err(|e| JsonRpcError::invalid_params(format!("faction.seat 非法: {e}")))?,
        ),
        None => None,
    };
    let layer = match input.layer.as_deref() {
        Some(s) => Some(
            wv_core::Id::new(s)
                .map_err(|e| JsonRpcError::invalid_params(format!("faction.layer 非法: {e}")))?,
        ),
        None => None,
    };
    // 颜色是**自由串**（`Color` 是透明 String 包装，解析失败由渲染层回退，
    // §7.5 V1 不阻止作者）——但显然写错的格式仍显式提示，避免静默变成默认色。
    if wv_core::Color(input.color.clone()).to_rgb().is_none() {
        return Err(JsonRpcError::invalid_params(format!(
            "faction.color 无法解析为 #RRGGBB：`{}`",
            input.color
        )));
    }
    Ok(wv_core::Faction {
        id,
        name: input.name.clone(),
        color: wv_core::Color(input.color.clone()),
        layer,
        rank: input.rank.clone(),
        seat,
        memberships: Vec::new(),
    })
}

/// 地形意图入参 → [`wv_terrain::intent::Intent`]（按 kind 取字段，缺字段即参数错）。
fn build_intent(input: &TerrainIntentInput) -> Result<wv_terrain::intent::Intent, JsonRpcError> {
    use wv_terrain::intent::Intent;
    let need_pts = || -> Result<Vec<(f32, f32)>, JsonRpcError> {
        input
            .pts
            .clone()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| JsonRpcError::invalid_params(format!("{} 需要 pts", input.kind)))
    };
    let need_polygon = || -> Result<Vec<(f32, f32)>, JsonRpcError> {
        input
            .polygon
            .clone()
            .filter(|v| v.len() >= 3)
            .ok_or_else(|| {
                JsonRpcError::invalid_params(format!("{} 需要 ≥3 点的 polygon", input.kind))
            })
    };
    let need_center = || -> Result<(f32, f32), JsonRpcError> {
        input
            .center
            .ok_or_else(|| JsonRpcError::invalid_params(format!("{} 需要 center", input.kind)))
    };
    let need_radius = || -> Result<f32, JsonRpcError> {
        input
            .radius
            .ok_or_else(|| JsonRpcError::invalid_params(format!("{} 需要 radius", input.kind)))
    };
    let strength = input.strength.unwrap_or(1.0);
    match input.kind.as_str() {
        "mountain_range" => Ok(Intent::MountainRange {
            pts: need_pts()?,
            radius: need_radius()?,
            strength,
        }),
        "plateau" => Ok(Intent::Plateau {
            pts: need_pts()?,
            radius: need_radius()?,
            strength,
        }),
        "hills" => Ok(Intent::Hills {
            pts: need_pts()?,
            radius: need_radius()?,
            strength,
        }),
        "volcano" => Ok(Intent::Volcano {
            center: need_center()?,
            radius: need_radius()?,
            strength,
        }),
        "lake" => Ok(Intent::Lake {
            center: need_center()?,
            radius: need_radius()?,
            strength,
        }),
        "river" => Ok(Intent::River {
            pts: need_pts()?,
            strength,
        }),
        "mountain_massif" => Ok(Intent::MountainMassif {
            polygon: need_polygon()?,
        }),
        "hills_region" => Ok(Intent::HillsRegion {
            polygon: need_polygon()?,
        }),
        "plains_region" => Ok(Intent::PlainsRegion {
            polygon: need_polygon()?,
        }),
        "lake_region" => Ok(Intent::LakeRegion {
            polygon: need_polygon()?,
        }),
        other => Err(JsonRpcError::invalid_params(format!(
            "intent.kind 未知：`{other}`；见 paint_terrain_intent 的 schema 枚举"
        ))),
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
