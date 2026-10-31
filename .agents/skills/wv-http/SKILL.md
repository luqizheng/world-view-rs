---
name: wv-http
description: 通过 wv（World View）的内嵌 HTTP 接口驱动地图编辑器——看画面、切视图、开关面板、读写世界数据、给自然区起名、跑验收而省 token。零注册：只要 wv GUI 在跑，用 curl / HTTP 客户端直接调即可。当需要「看 wv 现在的画面」「切视图 / 开关面板」「查 / 改世界数据」「给自然区域起名」「实机验收」时使用。
---

# wv HTTP 直驱指南（标配入口）

## 这是什么

wv 是单机地图世界观编辑器。GUI 运行时在本机开一个 **JSON-RPC 2.0 HTTP 端点**
（MCP `2025-03-26` 工具面，**不需要任何握手 / 注册**），供 AI agent 读写当前
打开的世界、操控 UI、截图。

**零前置**：不用装 MCP 客户端、不用注册 server、不用改任何配置——GUI 开着就有。

## 前置（一次性确认）

1. **GUI 在跑**（HTTP 服务活在 GUI 进程里，随 GUI 启动自启）。
   开发者从源码启动：`cargo run -p wv-app --features http --bin wv -- gui`
   （可加 `-m examples/jrpg-world` 直开演示工程）；散装用户打开 wv 即可。
2. **端口**：默认 `127.0.0.1:8732`。GUI 启动参数 `--http-port <N>` 可改；
   真实地址见 设置 → HTTP 服务 分组（那里也能停止 / 换端口重启）。
3. 服务不可达时先 `GET` 一下别的端口没用——直接看下文故障表。

## 调用方式（curl 即可）

**只有两个方法**：`tools/list`（发现工具）与 `tools/call`（调用工具）。
没有 `initialize` / `ping`——不需要握手，直接 POST。

```bash
# 列出全部 15 个工具（含各自的 inputSchema）
curl -s http://127.0.0.1:8732/mcp \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'

# 调用示例：拿 UI 快照（view_mode / 当前图 / 全面板显隐，一行 JSON）
curl -s http://127.0.0.1:8732/mcp \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","method":"tools/call","id":2,
       "params":{"name":"get_ui_state","arguments":{}}}'
```

响应是标准 JSON-RPC 2.0：`result.content[0]` 为文本（`text`）或图片
（`image`，`data` 为裸 base64 PNG，无 `data:` 前缀）。

## 工具面（15 个）

| 类 | 工具 | 用途 |
|---|---|---|
| 数据读 | `list_maps` `get_map` `list_places` `get_place` `get_state_at_time` | 查地图树 / 图层与 comment / 地点 / 某天投影 |
| 数据写 | `create_map` `create_place` `undo` `save_project` | 建图 / 建点 / 撤销 / 保存（写操作落盘前记得 `save_project`） |
| UI 操控 | `get_ui_state` `set_view_mode` `set_panel_visible` `screenshot` | UI 快照 / 切视图 / 开关面板 / 截图 |
| 合成输入（ADR-0096） | `enqueue_ui_input` `set_map_tool` | 注入点击/抬起/Esc（世界坐标，每帧消费一个）；切地图工具 |
| 地图导航（ADR-0096 追记） | `set_current_map` `set_zoom` | 钻入目标图（等价面包屑）；设定缩放（世界单位/屏幕像素） |
| 自然区命名 | `get_naming_context` `set_nature_names` | 取工程与图的 comment + 自然脸清单 / 批量写回名字 |

- `set_view_mode` 词表：`browse` `map` `cellmap` `faction` `realterrain` `event` `coastline`。
- `set_panel_visible` 词表 = UI 面板菜单里的面板名（snake_case，ADR-0095 后含 faction / situation 共 14 个）。
- **自驱验收（ADR-0096）**：`enqueue_ui_input` 注入合成点击（世界坐标 = 根图帧；两次独立调用同坐标 down 即双击），`set_map_tool` 切工具（point-select / select-* / brush / edit-cell…），`set_current_map` 钻入目标图、`set_zoom` 定缩放——配合 `screenshot`（GPU 回读，被别的窗口挡住也不影响）即可全程无人值守验收。
- `create_place` 支持 `kind`（PoiKind 词表：settlement/port/ruins…）与 `rank`（0..3，≥2 画图标钉），建点即可直接是「城市符号」而非裸点标。
- 各工具入参 schema 以 `tools/list` 返回为准（有 `additionalProperties: false`，别猜字段）。

## 省 token 的用法（本 skill 存在的理由）

- **看画面** → `screenshot`：**务必用管道把 base64 直接落盘，别让响应体进上下文**
  （整张 PNG 的 base64 很大，读进对话就是灾难）：

  ```bash
  curl -s http://127.0.0.1:8732/mcp -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","method":"tools/call","id":3,
         "params":{"name":"screenshot","arguments":{"save_to":"shot.png"}}}' \
    | jq -r '.result.content[0].data' | base64 -d > shot.png
  # 然后用 Read 看 shot.png
  ```

  （`save_to` 只控制 wv 侧落盘备份；HTTP 响应里永远带 base64，所以管道是关键。）
- **查状态** → `get_ui_state`：一行 JSON 拿视图模式 / 当前图 / 全面板显隐，
  替代 grep 日志。
- **切视图 / 开面板** → `set_view_mode` / `set_panel_visible`：替代模拟击键
  （PostMessage 那套在窗口未聚焦时会丢事件）。
- **改数据** → `create_place` 等写工具 + `save_project`：替代手编 YAML 再重启。
- **回归检查**：`get_ui_state` 断言 + `screenshot` 目测，一轮 ≤ 数百 token。

## 典型流程

### A. 实机验收一轮

1. `get_ui_state` → 确认 `view_mode` 与 `current_map`；
2. `set_view_mode`（如 `"cellmap"` / `"map"`）→ 到目标工作区；
3. `screenshot`（管道落盘）→ 看画面；
4. 有问题：把截图现象 + `get_ui_state` 回执一起描述给用户。

### B. 自然区 LLM 命名（确认闸必须走）

1. `get_naming_context`（map 取 `get_ui_state` 的 `current_map`）；
2. **把 `project_comment` 与 `map_comment` 展示给用户，等用户确认风格**——
   未经确认不得直接命名；
3. 按确认后的风格为每张脸提案名字 → `set_nature_names`（批量；空串 = 清名
   回派生名「草原N」）→ `save_project`。

### C. 数据抽查

`list_maps` → `get_map`（看 layers / comment）→ `get_state_at_time`（某天投影）。

## 故障表

| 现象 | 原因与处置 |
|---|---|
| connection refused | GUI 没开，或服务被停（设置 → HTTP 服务 → 启动）。开发者：先 `taskkill //F //IM wv.exe` 杀残留再启（旧实例锁构建产物与端口） |
| `-32601` method not found | 本端点只有 `tools/list` / `tools/call`——没有 `initialize` / `ping`，别试图握手 |
| `-32602` invalid params | 入参不合 schema；对照 `tools/list` 的 `inputSchema`（注意 `additionalProperties: false`） |
| screenshot 超时（10s） | 窗口最小化 / 后台挂起无帧产出；把窗口恢复前台再试 |
| 端口变了 | `--http-port` 启动参数，或设置对话框里换端口重启过——以设置 → HTTP 服务 显示的地址为准 |

---

## 开发者补充（散装用户可忽略）

- **本文件是单一真源**：`wv skill`（stdout / `--out <path>`）与设置对话框
  「复制 agent skill」按钮输出的都是 `include_str!` 本文件——改这里即三处同步。
- 构建配方：HTTP 面与 GUI 需 `--features http`（其内部含 `app`）。
  发布构建统一 `cargo build -p wv-app --features app,http --release`。
- 门禁：`cargo test -p wv-app --features http`（别名 `cargo http-test`）+
  `scripts/smoke-http.sh`（15 工具实机 smoke）。
- 设计沿革：skill+HTTP=标配入口 / MCP=可选增强 的定位见
  ADR-0084（撤回 stdio 代理轮）与 ADR-0085（本轮标配入口）。
