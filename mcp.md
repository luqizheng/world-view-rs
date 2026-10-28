# wv agent 接入指南：skill（零注册）与 MCP（可选增强）

`wv` 对 AI agent 开放一套工具面（15 个）：世界数据读写、undo / 保存、
UI 操控（查状态 / 切视图 / 开关面板 / 截图）、自然区 LLM 命名。真源在
**GUI 进程内**——它常驻 `127.0.0.1:8732` 的 JSON-RPC 端点（仅 loopback）。
**两种接入方式**，按需取用：

| | 方式一：skill + HTTP（**推荐入口**） | 方式二：MCP 注册（可选增强） |
|---|---|---|
| 用户要做什么 | 零注册——把 skill 文件指给 agent | 每个客户端注册一次（Settings → MCP / config） |
| 适用 agent | 任何有 shell 的 agent | 支持 MCP 的客户端 |
| 通信 | agent 用 curl 打 HTTP 端点 | 结构化工具调用（schema 自动发现） |
| 截图 | 落盘 → agent 读文件 | 图片直回上下文 |

```text
[方式一] agent ──curl──▶ HTTP 127.0.0.1:8732/mcp ◀── wv gui
[方式二] MCP 客户端 ⇄ stdio ⇄ wv mcp（代理） ──▶ 同一端点
```

## 一、方式一：skill + HTTP（零注册，面向「下载软件的用户」）

**第 1 步**：启动 GUI（常驻后台即可）：

```bash
cargo run -p wv-app --features http --bin wv -- gui -m examples/jrpg-world
```

（启动前先杀残留 `wv.exe`——会锁构建产物、占端口 8732。）

**第 2 步**：把 skill 指给 agent——随仓库/软件分发的是
`.agents/skills/wv-mcp/SKILL.md`，里面写了工具清单、典型流程（验收轮 /
自然区命名确认闸 / 数据抽查）与故障表。agent 读了它就知道怎么用下面的端点。

**第 3 步**：agent 直接用 shell 打端点（MCP 协议都不需要）：

```bash
# 列工具
curl -s http://127.0.0.1:8732/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
# 调用（例：查 UI 快照）
curl -s http://127.0.0.1:8732/mcp -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_ui_state","arguments":{}}}'
```

## 二、方式二：MCP 注册（面向要更好 DX 的用户）

### ZCode（Settings → MCP）

| 字段 | 值 |
|---|---|
| 名称 | `wv` |
| 类型 | `stdio` |
| command | `C:\Users\leo\.cargo\bin\cargo.exe` |
| args | `run` `--manifest-path` `D:\kudi-projects\worldview-rs\Cargo.toml` `-p` `wv-app` `--features` `http` `--bin` `wv` `--` `mcp` |
| timeoutMs | `60000`（首次可能要编译） |

等价的用户级配置文件（`~/.zcode/cli/config.json` 的 `mcp.servers` 节点）：

```json
{
  "wv": {
    "type": "stdio",
    "command": "C:\\Users\\leo\\.cargo\\bin\\cargo.exe",
    "args": [
      "run",
      "--manifest-path",
      "D:\\kudi-projects\\worldview-rs\\Cargo.toml",
      "-p",
      "wv-app",
      "--features",
      "http",
      "--bin",
      "wv",
      "--",
      "mcp"
    ],
    "timeoutMs": 60000
  }
}
```

> 注意：ZCode **不读**仓库根的 `.mcp.json`（那是 Claude Code 系格式，本仓库
> 已不提供该文件）。workspace 级注册走 `<repo>/.zcode/config.json` 的
> `mcp.servers`（该目录已 gitignore，适合机器本地路径）；同名服务器用户级
> 覆盖 workspace 级。

### 其他 MCP 客户端（Claude Code / Cursor 等）

stdio 注册同上（command + args 同款）。Claude Code 系的 `.mcp.json` 写法：

```json
{
  "mcpServers": {
    "wv": {
      "command": "C:\\Users\\leo\\.cargo\\bin\\cargo.exe",
      "args": [
        "run",
        "--manifest-path",
        "D:\\kudi-projects\\worldview-rs\\Cargo.toml",
        "-p",
        "wv-app",
        "--features",
        "http",
        "--bin",
        "wv",
        "--",
        "mcp"
      ]
    }
  }
}
```

**生效顺序**（方式二）：注册 → **先开一次 GUI** → 重启会话 → 工具出现
（形如 `mcp__wv__get_ui_state`）。GUI 开过一次后，代理把工具列表缓存到
`%TEMP%/wv_mcp_tools_cache.json`——此后 GUI 没开也能注册出工具，调用才
要求 GUI 在线。

## 三、工具清单（15）

| 组 | 工具 |
|---|---|
| 数据读（6） | `list_maps` `get_map` `list_places` `get_place` `get_state_at_time` `get_naming_context` |
| 数据写（5） | `create_map` `create_place` `undo` `save_project` `set_nature_names` |
| UI 操控（4） | `get_ui_state` `set_view_mode` `set_panel_visible` `screenshot` |

## 四、故障速查

| 症状 | 原因 / 处置 |
|---|---|
| `-32000 wv gui 未运行…` | GUI 没开或端口不对——启动 GUI 或核对 `--http-port` 与 `wv mcp --port` |
| 注册后服务器 `failed` / 超时 | 首次编译慢——`timeoutMs` 加大；或先手动 `cargo build -p wv-app --features http --bin wv` |
| 构建报 `os error 5` | `wv.exe` 残留锁产物——`taskkill //F //IM wv.exe` 后重试 |
| 端口绑定 10048 | 8732 被旧实例占用——同上，杀干净再起 |
| 工具在但调用全失败 | GUI 主帧卡在弹窗输入——到 GUI 窗口处理 |

## 五、分发路线（待办，2026-10-28 判定）

当前目标用户 = 作者 + 本机 agents（cargo 已在，方式一二皆通）。面向外部
用户分发时按 **A→B** 两步：

- **A（前置）**：GitHub Releases 挂预编译 `wv.exe`——外部用户的真实门槛
  是 GUI 二进制而不是代理（npx 装了代理没有 GUI 一样用不了）；注册命令改
  填 exe 绝对路径，release 包**随附 skill 目录**（方式一即开即用）。
- **B（可选）**：npm 包装器（esbuild/biome 模式）给真 `npx` 体验；代理
  逻辑薄到可用 Node 重写，届时 npx 装的是纯 JS 壳 + 平台二进制。

## 六、省 token 建议的用法

- 验收画面走 `screenshot`（图片直回上下文），不要抓屏脚本 + 读文件双跳；
- 查状态走 `get_ui_state`（一行 JSON），不要 grep 日志；
- 切视图 / 开面板走 `set_view_mode` / `set_panel_visible`，不要模拟击键；
- 自然区命名先 `get_naming_context`（两级备注是风格来源，**给用户确认后**
  再 `set_nature_names` 写回）。

驱动细节见 `.agents/skills/wv-mcp/SKILL.md`；架构与决策见
`docs/adr/ADR-0083-mcp-stdio-proxy-and-skill.md`。
