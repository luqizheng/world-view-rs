# ADR-0078：HTTP UI 操控工具——视图切换 / 面板开关 / 截图回传

- 状态：已接受（2026-10-28，用户委托「你看着办」后按既有裁量落地）
- 编号追记：**0078**——0077 已被并行轮次 cellmap-relief-shading 的 spec（`docs/superpowers/specs/2026-10-28-cellmap-relief-shading-design.md`，main 未跟踪文件）预留，避开五方撞号重演。
- 关联：`docs/superpowers/specs/2026-10-28-http-ui-control-design.md`、`docs/superpowers/plans/2026-10-28-http-ui-control.md`
- 关联 ADR：**扩展 ADR-0014**（MCP HTTP server）的工具面，不推翻其任何结论；ADR-0014 2026-10-19 复议已预告「第一刀 = screenshot 工具」，本 ADR 是那一刀的落地。

## 处境

ADR-0014 的 9 个工具全部面向**世界数据**（list/get/create/undo/save），一个都碰不到 UI。而项目的实机验收循环长期靠两样东西补位：`WV_MODE` / `WV_SHOT` 环境变量探针（启动一次性的）+ 人工点击。AI 助手在验收渲染效果时的真实诉求是**运行中操控**：切到 CellMap 看一眼、关掉笔刷面板截一张、再切回 Browse 对比——这些当前必须人手完成。2026-10-28 用户裁定：把「整天测试的功能」直接做成 HTTP 服务能力。

## 决策

1. **D1 扩展而非新起**：在 ADR-0014 的 axum 服务（`127.0.0.1:8732`，`--features http`）上追加 **4 个 UI 操控工具**（9 → 13），不加第二个 HTTP 栈、不开新端口：
   - `get_ui_state()` → `{ view_mode, current_map, panels: [{panel, visible}] }`——验收循环的「定位」调用；
   - `set_view_mode(mode)` → 模式词表与 `WV_MODE` 探针**同源**（browse / map / cellmap / faction，另收 realterrain / event / coastline），语义 = 顶栏切换（`ViewState::mode` 直写）；
   - `set_panel_visible(panel, visible)` → panel 词表 = `Panel` 枚举 15 变体的 snake_case，语义 = 面板右键开关（`PanelState::set_visible`）；
   - `screenshot(save_to?)` → **MCP image content**（base64 PNG）回传 + meta 文本（落盘路径 / 宽高）；`save_to` 可指定落盘路径，缺省落 OS 临时目录 `wv-screenshots/`。
2. **D2 消息走同一 mpsc 桥**：新增 `HttpMessage::Ui(UiRequest, oneshot::Sender<UiReply>)` 变体；`drain_messages` 增加一个 UI 钩子参数（旧签名保留为包装，无钩子时 UI 消息回「桥未接入」错误）；Bevy 侧 `drain_http_commands` 系统持 `ResMut<ViewState>` / `ResMut<PanelState>` / `Commands` 处理 UI 消息。**UI 状态与 World 数据一样只在主帧读写**——ADR-0014 §2.3「单线程命令队列」纪律原样延伸。
3. **D3 截图时序 = 请求→主帧 spawn→轮询落盘→回执**：主帧收到请求即 `commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path))`（与 `WV_SHOT` 探针同款），把 `(path, reply)` 压进 `PendingShots` 资源；每帧轮询文件出现 → 读字节 → 解析 PNG IHDR 取宽高 → base64 → oneshot 回执。handler 侧 `tokio::time::timeout(10s)` 兜底（窗口最小化时渲染器可能不产出帧）。
4. **D4 image content**：`ToolContent` 增加 `Image { data, mimeType }` 变体（MCP `2025-03-26` 原生支持 image content，2026-10-19 复议正是为此）；`tools::dispatch` 返回值升级为 `ToolOutput`（`Json | Image`）枚举，`call_tool` 按变体组 content 数组。
5. **D5 安全面与 v0.1 同水位**：仍 loopback only、无鉴权。UI 操控不放大风险面——写类工具本就能改世界数据，能读写世界的调用方没有理由不能截图看结果。
6. **D6 词表单一真源**：模式词表复用 `WV_MODE` 的既有映射（`debug_probe`），panel 词表从 `Panel` 枚举手工映射（枚举无 serde 名，15 个变体写显式 match——加面板时编译器迫使补全）。

## 理由

- **扩展而非新起**：ADR-0014 已把 mpsc 桥 / 主帧 drain / 协议层全部铺好，新起一个 HTTP 栈意味着第二套端口管理、第二套消息纪律，纯成本。
- **截图走 Bevy `Screenshot` 而非 OS 抓屏**：桌面抓屏在本机实测全废（`WV_SHOT` 配方记忆），in-app Screenshot 是唯一验证过的取图法；且它天然截到的是**渲染管线产物**（含 egui 全部叠加层），正是验收要看的画面。
- **轮询文件而非 observer 直接回执**：`save_to_disk` 的 observer 回调拿不到我们的 oneshot sender（闭包形态受限），落盘后轮询是同为纯函数、可单测的形态。
- **base64 不引新依赖**：`base64 0.22` 已在依赖树（axum 传递），只是显式声明，§9 纪律过。

## 代价

- 工具面 9 → 13，`tools/list` 契约测试与 README §10.1 文档同步扩。
- `screenshot` 响应体含整张 PNG 的 base64（1920×1080 全屏约 1–3 MB 文本）——LLM 客户端按 image content 渲染，不进上下文文本；但走 HTTP 的字节量实打实。v0.1 不做缩放/裁剪参数，实测需要再加（待裁，见 spec §8）。
- UI 操控工具**不进撤销栈**（视图切换 / 面板显隐本就是会话态，GUI 同款语义；`ui-layout.yaml` 的显隐落盘行为与 GUI 点击一致）。

## 被否决的备选

| 备选 | 否决理由 |
|---|---|
| 新开独立 HTTP 端口 / REST API（非 MCP） | 第二套协议栈 + 第二套鉴权认知；调用方（LLM 客户端）已按 MCP 消费；9 工具的桥接基建白铺 |
| 截图 observer 闭包里直接回 oneshot | `save_to_disk` 返回的 observer 形态固定，塞不进自定义回调；硬造等价 observer = 重复实现 Bevy 内部 |
| 合成鼠标/键盘事件（万能操控） | 依赖窗口焦点与坐标换算，脆且慢；本轮非目标（spec §7），真实需求先用语义化工具满足 |
| 截图直接返文件路径（不回 base64） | 调用方（远程 LLM）拿不到本机文件系统；MCP image content 是为这场景设计的 |
