# ADR-0083：`wv mcp` stdio 代理 + skill——让 agent 经 MCP 驱动测试与操作

- **状态**：已接受（Accepted，2026-10-28，用户裁定「添加 wv mcp 以及 skill，一些测试和操作可以通过 mcp 做，减少 token」）
- **决策者**：用户对话拍板（2026-10-28）
- **关联**：扩展 [ADR-0014](ADR-0014-mcp-http-server.md)（传输层补全）、[ADR-0078](ADR-0078-http-ui-control-tools.md)（工具面 15 个不动）、[ADR-0082](ADR-0082-nature-naming-and-comments.md)（命名流程的 agent 侧载体）

## 1. 处境

GUI 内嵌的 MCP HTTP 服务（工具面已达 15 个：世界读写 9 + UI 操控 4 + 自然区命名 2）
一直只能用 curl 手打：`POST /mcp` **没有 `initialize` 握手**，真 MCP 客户端
（ZCode / Claude Desktop）握手不过、无法注册——15 个工具全在裸奔。同时，
agent 驱动验收走的是「bash 抓屏脚本 + Read 图片 + grep 日志」的 token 重路。

## 2. 决策

| # | 决策 | 落点 |
|---|---|---|
| ① | **`wv mcp` stdio 代理子命令**：本地答 `initialize`（serverInfo=wv，capabilities.tools）/ `ping` / 通知；`tools/list` / `tools/call` **原样转发**到 `http://127.0.0.1:<port>/mcp`（默认 8732，`--port` 对齐 GUI） | `crates/wv-app/src/http/proxy.rs` + `main.rs` 子命令（feature=http） |
| ② | **`.mcp.json` 注册**：workspace 根注册 `wv` server（stdio，`cargo run -p wv-app --features http --bin wv -- mcp`）——MCP 客户端重启会话即得 15 工具 | `.mcp.json` |
| ③ | **skill**：`.agents/skills/wv-mcp/`——前置（启动 GUI / 杀残留）、15 工具清单、省 token 用法表（screenshot 直回图 / get_ui_state 替代日志 grep / set_view_mode 替代 PostMessage）、三条典型流程（验收轮 / 命名闭环含确认闸 / 数据抽查）、故障表、curl 兜底 | `.agents/skills/wv-mcp/SKILL.md` |

### 设计要点

- **代理零工具表**：工具定义与执行的单一真源在 GUI 侧；代理只补协议面
  （initialize/ping），工具增删不动代理（GUI 工具面 15→N 代理无感）。
- **上行 HTTP 客户端手写**（tokio TcpStream，最小 HTTP/1.1 POST，仅
  loopback）——不为此新增 reqwest（AGENTS §9）；tokio 只补 `io-std`/`io-util`
  两个标准 feature。
- **代理不拉起 GUI**：GUI 未运行时转发回 -32000（带启动指引）——MCP 客户端
  启动代理时拉 GUI 会造成进程树混乱（谁杀谁）。

## 3. 理由

- **stdio 代理优于给 GUI 加 initialize**：GUI 的 HTTP 面保持极简（两方法）；
  协议兼容责任在代理（一处、可测）；且 stdio 是 MCP 客户端注册的通用形态
  （HTTP MCP 各家支持参差）。
- **token 账**：截图直回（省一次落盘+Read 双跳）、状态一行 JSON（省日志
  grep 往返）、击键改结构化调用（省 drive 脚本重试）——单轮验收从「多工具
  大 payload」降到「几百 token 一轮」。

## 4. 硬约束

- 代理**不缓存**工具表与回执（转发即所得）；不改写 tools/* 的任何 payload；
- 本地方法白名单穷尽：`initialize`/`ping`/通知本地答，`tools/*` 转发，
  其余 -32601（防把代理当万能 JSON-RPC 通道）；
- 上行只 loopback 明文（与 GUI HTTP 同安全水位，ADR-0014）。

## 5. 后果

**正面**：ZCode 重启会话后 15 工具原生在场；skill 把验收/命名流程文档化
（agent 不再各自摸索）；协议面与工具面解耦（后续加工具零代理改动）。

**负面 / 代价**：代理经 `cargo run` 启动有编译等待（skill 注明可换已构建
二进制）；MCP stdio 无鉴权（与 HTTP 面同水位，本机工具可接受）；GUI 必须先
在跑——「GUI 未启」是工具调用的常态错误而非异常（-32000 文案带指引）。

**被否决的备选**：GUI 直升完整 MCP HTTP（streamable HTTP 各客户端支持
参差，且 initialize 状态机与 Bevy 帧循环耦合）；reqwest 代理（新增依赖树）；
代理内置工具表（双真源必漂移）。

## 6. 与既有 ADR 的关系

- **补全** ADR-0014 的传输面（HTTP → stdio 代理桥）；
- **不动** ADR-0078/0082 的工具面与语义（15 个工具原样）；
- skill 是 `.agents/skills/` 仓库惯例的第一个 MCP 驱动类条目。

## 7. 追记（2026-10-28，只增不改）：ZCode 注册实测修正

- **ZCode 不读 `.mcp.json`**（Claude Code 系格式）——注册面是
  **Settings → MCP**（落 `~/.zcode/cli/config.json` 或 workspace
  `.zcode/config.json` 的 `mcp.servers`，后者 gitignore）。根目录
  `.mcp.json` 已删，注册说明收口到仓库根 **`mcp.md`**。
- **tools/list 缓存**：MCP 客户端在会话启动时列工具，而 GUI 往往还没开
  ——代理把成功的 tools/list 回执缓存到 `%TEMP%/wv_mcp_tools_cache.json`，
  上游不可达时 tools/list 回缓存（tools/call 不吃缓存，照旧 -32000）。
  GUI 开过一次后，注册与 GUI 启动顺序解耦。

- **接入定位裁定（2026-10-28 用户拍板「用户下载软件→agent 使用→通信」）**：
  **skill+HTTP 是第一入口**（零注册、随软件分发、任何有 shell 的 agent 可
  用——GUI 端点本就是 JSON-RPC over POST），MCP 注册降为可选增强（图片直
  回 / schema 化）。`mcp.md` 重构为「两种接入方式」；分发路线待办记入该文
  （A=Releases 预编译 exe + 随附 skill；B=npm 包装器，代理可 Node 重写）。
