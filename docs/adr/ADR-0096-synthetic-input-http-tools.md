# ADR-0096: 合成输入入 HTTP 工具面——AI 自驱验收闭环

- 状态：已接受（2026-10-31 用户裁定：「测试的这些点击，或者操作全部封装为 ai 可以调用的，这样方便你操作，还有截图，你可以通过 mcp 获取截图，不用被我不小心开了个其他应用挡住了你的截图」）
- 关联：ADR-0078（UI 操控四工具——本 ADR 增量两工具）、ADR-0085（SKILL.md 单一真源 + 哨兵测试）、ADR-0089（交互语法——合成输入按同一套裁定语义走）

## 1. 处境（Context）

每轮 UI 改动的验收都靠作者人手走清单（点 POI / 选群系 / 开面板 / 切模式）。三个摩擦：

1. **AI 不能自驱**：验收操作没有任何程序化入口，AI 只能出清单等作者跑、等人读结果；
2. **截图遮挡顾虑**：作者担心随手开个别的窗口会挡住 AI 的截图；
3. **双击/拖动手势无法用「发一条消息」表达**。

## 2. 决策（Decision）

1. **合成输入工具 `enqueue_ui_input`**：事件队列（`down` / `up` / `key=escape`，坐标 = 根图帧世界坐标）经 HTTP 桥入队；`apply_synthetic_input` 系统**每帧消费一个**——覆盖 `ViewState::cursor_world` + `ButtonInput` 按下。**两次独立调用同坐标 down 即成双击**（双击判定按 `just_pressed` 事件计，分帧天然成立）；`down`…`up` 分帧 = 拖动手势（切刀可用）。
2. **切工具 `set_map_tool`**：走命令面板既有 `SelectMapTool` 通路（互斥全关 / 面板联动零重复实现）；词表 14 词（point-select / select-四形状 / brush / eraser / edit-cell / boundary / landmark / measure-* / pan）。
3. **截图澄清（现状确认，非新实现）**：`screenshot` 走 Bevy `Screenshot::primary_window` = **GPU 回读**，OS 层窗口遮挡不影响画面（最小化才有问题——文档已有说明）。用户顾虑已被既有实现覆盖。
4. **链序契约**：`apply_synthetic_input` 排在 `track_cursor_world` 之后、一切 `just_pressed` 消费方之前（同一条 chain）；一帧一事件是双击语义的前提。

## 3. 理由（Rationale）

- **走同一套裁定语义**：合成输入写的就是真实输入资源（`ButtonInput` / `cursor_world`），点选仲裁 / 泛洪兜底 / 双击分流全部原样生效——AI 验收 = 人手验收的同构重放，不出「第二套测试语义」；
- **工具面复用**：HTTP 桥 / MCP schema / SKILL.md 单一真源 / 哨兵测试全现成，增量 = 2 工具 + 1 系统 + 1 队列资源；
- **被否决的备选**：
  - OS 级 SendInput / 窗口消息注入：绕不开 egui hit-test 与窗口遮挡，且依赖窗口前台，弃；
  - 直接调裁定函数（绕过输入层）：测不到「输入→裁定→渲染」整条链，等于没验收，弃。

## 4. 代价（Consequences）

- **合成点击绕过 egui 命中**：真实用户点在面板下的行为与合成点击不同（后者直达画布）——验收脚本要自己避开面板区，或先把面板关掉（有 `set_panel_visible`）；
- **Bevy 后台挂起无帧**：窗口最小化时合成事件积压不消费（同截图限制），文档已述；
- **词表维护**：MapToolId 新变体需同步 `parse_map_tool`（穷尽匹配在编译期逼不出本表——注释已注明）；
- **每帧一事件的时序耦合**：若未来把消费系统挪出主 chain（改并发），双击语义会碎——链序契约已写进模块文档。
