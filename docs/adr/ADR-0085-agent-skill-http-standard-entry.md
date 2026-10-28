# ADR-0085：agent 驱动标配入口——skill + HTTP 直驱（`wv skill` / 复制按钮）

- **状态**：已接受（Accepted，2026-10-28，用户裁定重做方向选 C 完整闭环）
- **决策者**：用户对话拍板（2026-10-28「A」＝重做标配轮）
- **关联**：承接 [ADR-0084](ADR-0084-withdraw-mcp-stdio-proxy.md)（撤回 stdio 代理轮时定调「skill+HTTP=标配、MCP=可选增强」）；地基 [ADR-0014](ADR-0014-mcp-http-server.md)（HTTP 端点不动）、[ADR-0078](ADR-0078-http-ui-control-tools.md) / [ADR-0082](ADR-0082-nature-naming-and-comments.md)（15 工具面不动）；宿主 [ADR-0080 注记的设置 HTTP 分组]（settings-http-toggle，52b1b7e 本轮随行合入）

## 1. 处境

ADR-0084 撤回 stdio 代理后，agent 接入只剩裸 curl——能力都在（HTTP 端点 + 15 工具），
但**没有给 agent 看的说明书**：端口怎么发现、screenshot 的 base64 如何不撑爆上下文、
命名确认闸的流程、故障处置——这些知识散在已撤回的 SKILL.md 与各 ADR 里，散装用户
与他们的 agent 都拿不到。产品愿景（用户下载软件 → 自己的 agent 驱动）要求**开箱即用**：
零注册、零配置、任何有 shell 的 agent 通用。

## 2. 决策

| # | 决策 | 落点 |
|---|---|---|
| ① | **skill 单一真源**：`.agents/skills/wv-http/SKILL.md`——agent 驱动指南全文（HTTP 直驱、无握手声明、curl 配方、15 工具表、省 token 用法、验收/命名/抽查三流程、故障表、开发者补充段） | 仓库根 `.agents/skills/wv-http/` |
| ② | **`include_str!` 同源编译进二进制**：`wv_app::http::AGENT_SKILL_MD`——改 skill 只改那一个文件，三处输出同步 | `crates/wv-app/src/http/mod.rs` |
| ③ | **`wv skill` 子命令**：stdout 打印全文；`--out <path>` 落盘（规避 PowerShell 重定向编码坑）。feature=http 门（无 http 构建提示编译配方） | `crates/wv-app/src/main.rs` |
| ④ | **设置 → HTTP 服务 → 「复制 agent skill」按钮**：全文进剪贴板 + 2s 瞬时反馈（egui temp 存储，无常驻状态字段） | `settings_dialog.rs` + i18n 两键（572→574） |
| ⑤ | **不改 Cargo.toml**：`http` 仍非默认 feature（与 `app` 同例，显式构建是项目惯例）；**发布配方**统一 `--features app,http --release`，文档化在 README §10.1 | 零构建配置改动 |
| ⑥ | **内容哨兵测试**：`bundled_skill_keeps_core_sections_and_all_fifteen_tools` 钉死指南必须含核心段与全部 15 工具名——文件被清空/误改/工具面变更未同步时编译期后立即爆 | `main.rs` tests |

### 设计要点

- **include 跨 crate 目录到仓库根**是刻意的：skill 权威位置按 agent 生态惯例放
  `.agents/skills/`，且必须与仓库同版本流转；workspace 不发布 crates.io，无打包
  剥离风险。哨兵测试兜住文件缺失/漂移。
- **screenshot 的 token 纪律写进指南本体**：HTTP 响应永远带 base64（MCP image
  content），指南教 agent 用 `jq + base64 -d` 管道直落盘、不让响应体进上下文——
  这是 HTTP 直驱与 MCP 图片直回的唯一体验差，写清即无感。
- **无握手是特性**：端点只有 `tools/list` / `tools/call`（ADR-0014），指南显式
  声明「没有 initialize/ping，别试图握手」，把 ADR-0083 曾经要补的协议面知识
  转化为文档知识而非代码。

## 3. 理由

- **零摩擦路径存在且最短**：GUI 开着端点就在；`wv skill` / 复制按钮把说明书
  递到用户手里；agent 拿到指南即具备全部 15 工具的驱动能力——三步全是「已有
  能力的文档化」，零新协议面、零新依赖（AGENTS §9 默认拒绝立场天然满足）。
- **维护单点**：指南改一处、三处输出同源编译；工具面增删由哨兵测试强制同步
  （工具名清单断言），不会出现文档与实现漂移。
- **发布即标配**：release 构建带 `app,http` 后，散装用户「下载 → 打开 → 复制
  skill 给 agent」全程无 cargo 知识。

## 4. 代价 + 被否决的备选

**代价：**

- 指南与工具面是**弱同步**（靠哨兵测试钉工具名，但工具 schema 变更不爆测试）——
  已在指南里写「入参以 `tools/list` 返回为准」兜底。
- `wv skill` 在无 feature 构建里只有一句提示——接受（与 `gui`/`gallery` 同例）。
- 复制按钮进的是 http 门后的设置分组：无 feature 构建的用户看不到——但无
  feature 构建本来也没有 HTTP 服务，无物可驱。

**被否决的备选：**

| 备选 | 否决理由 |
|---|---|
| A. 纯内容（只落 SKILL.md + 文档） | 用户找不到文件路径；`wv skill`/按钮把获取成本压到一条命令/一次点击，是「标配」语义的最小完整实现 |
| B. 中量（skill + CLI，不碰 GUI） | 设置 HTTP 分组是天然宿主，用户在「启动服务」的同一屏拿到指南，动线完整；实现仅 ~30 行 |
| D. 顺带把 `http` 提进 default feature | 违项目显式构建惯例，且标准门禁会开始编译 axum/tokio 树拖慢全库测试——收益不抵 |
| E. 再起一个 MCP stdio 薄壳（Node/npx 分发） | ADR-0084 刚否决的路线的重演；标配入口闭环后若「可选增强」有真实需求再立项 |
