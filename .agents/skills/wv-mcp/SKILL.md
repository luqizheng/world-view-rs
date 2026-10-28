---
name: wv-mcp
description: 通过 wv 的 MCP 工具面驱动 World View——GUI 验收、UI 操控、截图、世界数据读写、自然区 LLM 命名。当需要「看 wv 现在的画面」「切视图/开关面板」「查/改世界数据」「给自然区域起名」「跑验收而不烧 token」时使用。
---

# wv MCP 驱动指南

## 前置（一次性确认）

1. **GUI 在跑**（MCP 工具的真源在 GUI 进程里）：

   ```bash
   cargo run -p wv-app --features http --bin wv -- gui -m examples/jrpg-world
   ```

   默认监听 `127.0.0.1:8732`（`--http-port` 可改）。**先杀残留 wv.exe 再启动**（`taskkill //F //IM wv.exe`）——旧实例会锁构建产物、串日志。

2. **MCP server 注册**：经 ZCode **Settings → MCP** 注册（stdio，command=
   `cargo.exe`，args=`run --manifest-path <repo>\Cargo.toml -p wv-app
   --features http --bin wv -- mcp`）——详见仓库根 **`mcp.md`**。ZCode 不读
   `.mcp.json`；GUI 开过一次后工具列表有缓存，GUI 关着也能注册。未注册时用
   文末 curl 兜底。

## 工具面（15 个，全部经 `wv` server）

| 类 | 工具 | 用途 |
|---|---|---|
| 数据读 | `list_maps` `get_map` `list_places` `get_place` `get_state_at_time` | 查世界结构 / 地点 / 某天投影 |
| 数据写 | `create_map` `create_place` `undo` `save_project` | 建图 / 建点 / 撤销 / 保存 |
| UI 操控 | `get_ui_state` `set_view_mode` `set_panel_visible` `screenshot` | 查快照 / 切视图 / 开关面板 / 截图回图 |
| 自然区命名 | `get_naming_context` `set_nature_names` | 取两级备注+脸清单 / 批量写回名字 |

## 省 token 的用法（本 skill 存在的理由）

- **验收画面** → `screenshot`：图片直接回进上下文（MCP image content）。
  不要再用 PrintWindow 抓屏 + Read 图片的双跳（两次大 payload）。
- **查状态** → `get_ui_state`：一行 JSON 拿视图模式 / 当前图 / 全面板显隐。
  不要 grep 日志。
- **切视图 / 开面板** → `set_view_mode` / `set_panel_visible`：替代
  drive_wv.py 的 PostMessage 击键（那套在窗口未聚焦时会丢 UP 事件）。
- **改数据** → `create_place` 等写工具 + `save_project`：替代手编 YAML 再重启。
- **回归检查**：`get_ui_state` 断言 + `screenshot` 目测，一轮 ≤ 数百 token。

## 典型流程

### A. 实机验收一轮（切蛋糕 / 自然区）

1. `get_ui_state` → 确认 `view_mode` 与 `current_map`；
2. `set_view_mode("cellmap")` 或 `"map"` → 到目标工作区；
3. `screenshot` → 看画面（红圈 / 工具条灰化 / 自然区色块）；
4. 有问题：把截图现象 + `get_ui_state` 回执一起描述给用户。

### B. 自然区 LLM 命名（确认闸必须走）

1. `get_naming_context`（map_id 取 get_ui_state 的 current_map）；
2. **把 `project_comment` 与 `map_comment` 展示给用户，等用户确认风格**——
   未经确认不得直接命名；
3. 按确认后的风格为每张脸提案名字 → `set_nature_names`（批量，空串=清名回
   派生名「草原N」）→ `save_project`。

### C. 数据抽查

`list_maps` → `get_map`（看 layers/comment）→ `get_state_at_time`（某天投影）。
只读路径零风险；写路径记得 `save_project`，坏了 `undo`。

## 故障

| 症状 | 处置 |
|---|---|
| 工具调用报 -32000 | GUI 没跑 / 端口不对——启动 GUI 或核对 `--http-port` 与 `wv mcp --port` |
| `wv mcp` 报「需要 --features http」 | 注册命令缺 feature，检查 `.mcp.json` args |
| 构建报 os error 5 | wv.exe 残留锁产物——taskkill 后重试 |
| initialize 成功但 tools/call 超时 | GUI 主帧卡死（弹窗等输入）——看 GUI 窗口 |

## 无 MCP 时的主路径（零注册）

本 skill 的定位：**随软件分发给「下载 wv 的用户」**——agent 读了即可驱动，
不需要任何客户端注册。GUI 常驻 `127.0.0.1:8732/mcp`（JSON-RPC over POST），
没有 MCP 注册时本节就是**主路径**而非兜底：

```bash
curl -s http://127.0.0.1:8732/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

`screenshot` 的 image content 是 base64——兜底场景建议让 GUI 落盘再 Read
（工具会写 `wv-screenshots/`）。
