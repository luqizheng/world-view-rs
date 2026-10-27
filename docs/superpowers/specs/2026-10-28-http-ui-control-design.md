# HTTP UI 操控工具 设计稿（2026-10-28）

> ADR-0078 已接受。本文是给实施者的细节契约：schema、消息形态、时序、测试面。
> 决策依据（为什么）看 ADR-0078，这里只写「是什么」。

## 1. 目标与非目标

**目标**：让 LLM 客户端（经 MCP）与 AI 助手（经 curl）在 GUI 运行中完成验收循环：
定位（`get_ui_state`）→ 操控（`set_view_mode` / `set_panel_visible`）→ 观察（`screenshot`）。

**非目标（v0.1 明确不做）**：

- 合成鼠标 / 键盘事件（万能操控——脆、慢、坐标换算两套原点陷阱）；
- 相机控制（zoom / pan / drill——下一轮候选，见 §8）;
- 对话框开关（对话框不是 `Panel`，走的是另一套 dialog 状态）；
- 播放器控制（时间轴 play/pause——同上下一轮候选）。

## 2. 工具契约（4 个，tools/list 9 → 13）

### 2.1 `get_ui_state`

```json
{ "name": "get_ui_state", "inputSchema": { "type": "object", "properties": {} } }
```

返回（JSON 文本 content）：

```json
{
  "view_mode": "cellmap",
  "current_map": "world_main",
  "panels": [ { "panel": "brush", "visible": true }, { "panel": "minimap", "visible": true } ]
}
```

- `view_mode`：规范词（七选一，见 §2.2）；
- `current_map`：`ViewState::current_map` 镜像（wv-render 唯一真源的派生值，够用）；
- `panels`：**全量 15 个** Panel 的显隐（不是只列可见的——调用方要能关掉一个已开的）。

### 2.2 `set_view_mode`

```json
{ "name": "set_view_mode",
  "inputSchema": { "type": "object", "required": ["mode"],
    "properties": { "mode": { "type": "string",
      "description": "browse|map|cellmap|faction|realterrain|event|coastline（与 WV_MODE 探针同源）" } } } }
```

词表映射（**与 `debug_probe::apply_env_view_mode` 同源，实现收口到一个共享函数**）：

| 词 | ViewMode |
|---|---|
| `browse` / `vector` / `vector_flat` | `VectorFlat` |
| `map` / `google` / `cartographic` | `GoogleMap` |
| `cellmap` / `topographic` | `Topographic` |
| `faction` | `Faction` |
| `realterrain` / `real_terrain` | `RealTerrain` |
| `event` | `Event` |
| `coastline` | `Coastline` |

返回：echo 新的 `get_ui_state` 同构快照（调用方免二次查询）。
未知词：`UiReply::Error`（isError=true，附合法词表）。

### 2.3 `set_panel_visible`

```json
{ "name": "set_panel_visible",
  "inputSchema": { "type": "object", "required": ["panel", "visible"],
    "properties": { "panel": { "type": "string" }, "visible": { "type": "boolean" } } } }
```

panel 词表 = `Panel` 枚举 15 变体 snake_case：
`map_tree` `layers` `brush` `inspector` `timeline` `faction` `region` `situation`
`validation` `minimap` `character` `event` `cell_editor` `line_poi` `landmark`。

实现：`PanelState::set_visible`（与 GUI 面板右键开关同一入口，`ui-layout.yaml`
落盘行为随既有 save 路径，不在本工具里特殊处理）。
返回：echo 新快照；未知 panel：`UiReply::Error`（附合法词表）。

### 2.4 `screenshot`

```json
{ "name": "screenshot",
  "inputSchema": { "type": "object",
    "properties": { "save_to": { "type": "string", "description": "可选：PNG 落盘路径（缺省落 OS 临时目录 wv-screenshots/）" } } } }
```

返回：**image content 为主条目**，meta 文本为次条目：

```json
{ "content": [
    { "type": "image", "data": "<base64 PNG>", "mimeType": "image/png" },
    { "type": "text",  "text": "{\"saved_path\":\"...\",\"width\":1920,\"height\":1080}" }
  ], "isError": false }
```

- 默认落盘名：`wv-shot-<unix 毫秒>.png`，目录 `std::env::temp_dir()/wv-screenshots/`（不存在则建）；
- `save_to` 给了就用给定路径（父目录不存在 → Error）；
- 宽高从 PNG IHDR（字节 16..24）解析，纯函数可单测，不引图片解码依赖。

## 3. 消息与桥

### 3.1 类型（`http/bridge.rs`）

```rust
pub enum HttpMessage { /* 既有 Read/Apply/Undo/Save */ Ui(UiRequest, oneshot::Sender<UiReply>) }

pub enum UiRequest {
    GetState,
    SetViewMode { mode: String },
    SetPanelVisible { panel: String, visible: bool },
    Screenshot { save_to: Option<String> },
}

pub enum UiReply {
    State(UiStateSnapshot),   // get / set 的回执（set = echo 新快照）
    Shot(ScreenshotPayload),  // saved_path + width + height + base64
    Error(String),
}

pub struct UiStateSnapshot { pub view_mode: String, pub current_map: String, pub panels: Vec<(String, bool)> }
pub struct ScreenshotPayload { pub saved_path: String, pub width: u32, pub height: u32, pub base64_png: String }
```

### 3.2 drain 变形（`drain_messages`）

```rust
pub fn drain_messages(rx, world, history, save_dir) -> bool          // 旧签名保留 = 无 UI 钩子
pub fn drain_messages_with(rx, world, history, save_dir, ui: &mut dyn FnMut(UiRequest, Sender<UiReply>)) -> bool
```

- `drain_messages` = `drain_messages_with(..., &mut |_, reply| { let _ = reply.send(UiReply::Error("UI 桥未接入".into())) })`；
- 无钩子的形态（独立单测 / 未来 headless 消费方）收到 UI 消息不 panic、不丢消息——回执明确报「未接入」。

### 3.3 Bevy 侧（`http_integration.rs`）

`drain_http_commands` 增参 `ResMut<ViewState>`、`ResMut<PanelState>`、`Commands`、
`ResMut<PendingShots>`；UI 消息的 `SetViewMode` / `SetPanelVisible` / `GetState`
当帧同步完成并回执；`Screenshot` 当帧只 spawn + 入 `PendingShots`（回执由轮询系统发）。

```rust
#[derive(Resource, Default)]
pub struct PendingShots(Vec<PendingShot>);   // { path: PathBuf, reply: oneshot::Sender<UiReply> }
```

新增系统 `poll_pending_shots`（Update）：文件存在 → 读 → IHDR → base64 → 回执 → 移除；
文件 60 帧仍未出现 → 回 Error（兜底，正常路径 1–2 帧即出）。

## 4. 协议层（`http/mcp.rs` + `http/server.rs`）

- `ToolContent` 增 `Image { data: String, #[serde(rename = "mimeType")] mime_type: String }`（tag 仍 `type`，`rename_all = "lowercase"` 天然出 `"image"`）；
- `tools::dispatch` 返回 `ToolOutput`：

```rust
pub enum ToolOutput { Json(Value), Image { data_base64: String, mime: &'static str, meta: Value } }
```

- `call_tool`：`Json` 走既有 text content 路径；`Image` 组 `[image, text(meta)]` 双 content。

## 5. 模块布局

```
crates/wv-app/src/http/
  bridge.rs          # + UiRequest/UiReply/快照结构 + drain_messages_with
  tools.rs           # dispatch 返回 ToolOutput；注册 4 个新 schema
  skills/ui.rs       # 新文件：4 工具 schema + 纯函数 handler（handle_ui_request）+ 词表映射
  mcp.rs             # + ToolContent::Image
http_integration.rs  # drain_http_commands 扩参 + PendingShots + poll_pending_shots
```

**测试友好性纪律**（承 ADR-0014）：`skills/ui.rs` 的核心逻辑是纯函数
`handle_ui_request(req, &mut ViewState, &mut PanelState, shot_dir) -> UiReply`
——不依赖 Bevy 运行时；`poll` 的「文件出现→回执」也是纯函数。

## 6. 测试面

| 测试 | 覆盖 |
|---|---|
| `drain_messages` 旧签名收到 Ui 消息回「未接入」 | 桥回归 |
| `drain_messages_with` 把 Ui 消息路由给钩子 | 桥新路径 |
| `parse_view_mode` 全词表 round-trip + 未知词报错 | 词表单一真源 |
| `parse_panel` 全 15 词 + 未知词报错 | 词表 |
| `handle_ui_request` 三态（get/set×2）对默认 `ViewState`/`PanelState` 的快照断言 | 语义 |
| `png_dimensions` 手工构造 IHDR 字节断言 w/h | 截图元数据 |
| poll 纯函数：文件不存在→Pending、存在→回执 | 截图时序 |
| axum oneshot `tools/list` 长度 9 → **13** | 契约 |
| axum oneshot `set_view_mode` 带 UI 钩子 drain 的端到端 | 端到端 |
| `ToolContent::Image` serde 形态（`"type":"image"` + `mimeType`） | 协议 |

## 7. 安全面

loopback only + 无鉴权与 ADR-0014 v0.1 同水位（D5）。`save_to` 是唯一的新文件写入口：
接受任意路径 = 本机调用方可写任意 PNG——与写类工具能改工程目录属同一信任级别，不另设白名单。

## 8. 待决 / 下一轮候选（不阻塞本轮）

- 截图缩放 / 区域裁剪参数（响应体 1–3 MB 的瘦身）；
- 相机控制（zoom / pan / `drill_to` map_id）；
- 播放器控制；
- `get_ui_state` 增补时间轴 / 选区 / 笔刷状态。
