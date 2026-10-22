# 平台与基础设施 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。

## 1. 渲染引擎升级到 Bevy 0.19 ✅

**现行口径**：引擎 0.15.3 → **0.19.1**、`bevy_egui` 0.32 → **0.42**（**不跳 0.20**）；**在 Phase 4 之前**一次做完，先建可回退点、独立提交可整体 revert；以编译器驱动迁移 + 官方迁移指南核对「能编译但行为变了」的部分；实测迁移面极小（全仓**零 `*Bundle` 用法**）。

- **溯源**：[ADR-0011](/adr/adr-0011-bevy-0-19-upgrade)（2026-10-13）

## 2. LLM 可控的世界编辑（HTTP / MCP 服务）✅

**现行口径**：`wv-app` 新增 `http/` 模块（feature-gated）；MCP `2025-03-26` HTTP/SSE **仅 loopback**，默认端口 **8732**。
- **写类 skill 必须**走 `EditCommand → mpsc → Bevy 主帧 drain_commands → History.apply`（§3.3 不破）；**读类 skill 直查 `wv-core`**
- v0.1 上 **9 个 skill**：`list_maps` / `get_map` / `list_places` / `get_place` / `get_state_at_time` / `create_map` / `create_place` / `undo` / `save_project`
- **「list 返摘要 + get 返详情」双步契约**；硬顶 **`limit ≤ 100`**；filter 用 Mini-DSL（等值 / 子串 + AND 合取）；MCP spec **锁版本**

- **溯源**：[ADR-0014](/adr/adr-0014-mcp-http-server)（2026-10-14）
- **关联待决**：§12-30（Q1 端口占用策略已默认 stderr + 退出 / Q2 鉴权预留 / Q3 schema 版本锁 / Q4 批量 skill / Q6 save_project 是否触发校验；(Q5) 已收敛）
- **纪律**：ADR-0014 是「外部接口先例」——新增外部接口（如未来的其他集成）参照本 ADR 的边界处理方式（只读路径 / 写路径分离）

## 3. UI 多语言（zh / en）✅

**现行口径**：新增 `wv-app::i18n`——`Locale { Zh, En }`（`Default = Zh`）+ `I18nKey` 枚举（**手写**，80–110 变体）+ `t()` 直返 `&'static str`；持久化复用 `ui_layout::LayoutFile.language: Option<String>`（与 theme / density / map_theme 同口径）；`CurrentLocale` Resource 默认 `Zh`，打开工程时回灌；**顶栏右侧增 EN/中 切换器**；CLI（`wv` + `overture`）纳入；HTTP / Skill 仅 Claude Desktop **可见文字**纳入，**LLM 协议纯 ASCII 字段保持常量**。**无新依赖**。

- **溯源**：[ADR-0018](/adr/adr-0018-i18n)（2026-10-15，用户三问裁定）
- **配套**：[ADR-0014](/adr/adr-0014-mcp-http-server)（skill 协议字段稳定性）· [ADR-0016](/adr/adr-0016-jrpg-antique-map-default-theme)（顶栏右侧区）

## 4. 🔶 界面字体打包（提议态）

**提议内容**（**未获决策者确认转「已接受」；实现已按本决策落地**）：
- `include_bytes!` 编译期嵌入 OFL 字体（`assets/fonts/`）：拉丁 **Inter-Regular** + 中文 **Noto Sans SC-Regular**（Open Sans 无 CJK 字形，取配套物）
- egui `Proportional = [Inter, NotoSC, 内置]` **逐字形回退**；`Monospace` 数字仍走**系统等宽**；Bevy `Text2d` 用 NotoSC 单句柄
- 系统 CJK 探测**删除**、等宽探测**保留**；只用 Regular（600 字重接线为独立任务）
- **追记 2026-10-20**：追加第三份字体**霞鹜文楷 v1.530**（OFL-1.1，ADR-0042 D4 画布标签书法体，**UI 不用**）
- 修正 `ART_STYLE_SPEC_v0.1` §4.2 硬约束 2（「必须系统字体栈」→「打包授权字体」）

- **溯源**：[ADR-0019](/adr/adr-0019-bundled-ui-fonts)（2026-10-16，状态 = **提议**）
- **状态依据**：ADR 原文「用户已下达字体指令，实现已落地；**待用户确认后转「已接受」**」——因此本站归入提议区，但其技术实现已在代码中。

---

## 本域相关的流程纪律（现行铁律）

| 纪律 | 内容 |
|---|---|
| **渲染层测试闸门** | `cargo test --workspace` 三条命令**不编译** `#[cfg(feature = "app")]` 下的模块——渲染层必须单独跑 `cargo app-test` / `cargo app-clippy`（§7.1） |
| **GUI 启动烟测** | 涉及渲染系统签名 / Plugin 装配 / 新增 Resource 的改动，提交前必跑 `bash scripts/smoke-gui.sh`（5 秒存活 + 日志干净 = PASS） |
| **§6.4 重大决策必须开 ADR** | 「不许沉默推翻」——推翻既有 ADR / 基线结论而没有对应 ADR 的改动，视为**未完成** |

- **溯源**：`AGENTS.md` §7.1 / §6.4（[规约投影](/specs/agents)）
- **关联待决**：§12-23（渲染层测试闸门，已收敛）· §12-39 第一组（panel 层何时脱离 `app` feature 门）