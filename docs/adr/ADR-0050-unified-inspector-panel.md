# ADR-0050：统一 Inspector 面板——单面板按选中对象分派属性

**状态**：已接受（Accepted）
**日期**：2026-10-22
**决策者**：用户（对话拍板四项裁定：原型退役、CellEditor 并入、属性为主+页签、四类+Cell 覆盖）
**关联**：[ADR-0040](ADR-0040-cell-inspector-tool.md)（**决策 6 被本 ADR 修正**）、[ADR-0025](ADR-0025-selection-first-tooling.md)（选区优先）、[ADR-0026](ADR-0026-wv-ui-ui-widget-crate.md)（控件层）、[ADR-0017](ADR-0017-label-layer.md) §2.7（选中会话态）、[MAP_EDITOR_BASELINE_v1 §1.11](../MAP_EDITOR_BASELINE_v1.md)（P-4）、[设计稿](../superpowers/specs/2026-10-22-unified-inspector-panel-design.md)（全文含六条裁定与实现细节）

---

## 1. 处境（Context）

属性编辑按**类型**分裂在四个浮窗里：区域卡（`region_panel`）、势力卡（`faction_panel`）、Cell 编辑器（`cell_editor_panel`）、计数式 Inspector（`inspector_panel`，只显示 `kind · id` 头 + 四条计数行）。作者要改一个对象的属性，得先知道「哪种对象在哪个面板里」——按实现分类而非按任务组织的界面。

基线 P-4 的原意就是「属性检查器：显示/编辑**当前选中对象的全部属性**（右侧停靠）」，Figma `42:2` 与 web 原型也都是单一 Inspector。用户诉求原话：

> 属性面板 inspect 应该是一个的，随着点击不同物体，就出现不同的选项。

这与 Bevy 生态惯例一致（`bevy_inspector_egui` 即「左树 + 右属性区，按选中实体类型切换」）。

builder check 还发现既有实现里两处**与文档不符的事实**：`EditCellState.pending` → `CommandId::CommitEditCell` 是全仓无人写入的死链；`Place.x/y/w/h` 对有地界环的 Place 是 `fit_anchor_to_ring` 的派生量。

## 2. 决策（Decision）

1. **单窗口三页签**。`Panel::Inspector` 成为右栏唯一属性面：页签条（复用 `wv_ui::controls::tabs`）切「属性 / 势力层 / 事态」。「属性」页签按选中对象分派 Place / Region / Faction / Event / Cell 五类表单（Cell 仅在 EditCell 工具激活时出现——工具即模式）；「势力层」「事态」两页签收编原 `faction_panel` / `situation_panel` 的集合层内容。
2. **三个旧面板退役**：`Panel::ALL` 移除 CellEditor / Faction / Situation（12→9 项），`app_bootstrap` 不再注册三个系统；枚举变体暂留（存量 `ui-layout.yaml` 的键兼容）。人物 / 事件登记面板保持独立浮窗（职能是「新建入口」，不是属性编辑）。
3. **新增两个 EditCommand**：`UpdatePlaceCommand`（名称/级别/锚点；锚点 `None` = 不改）与 `UpdateEventCommand`（六字段，**逐字段快照** revert——`effects` / `narrations` 不进快照，撤销不会误退它们）。
4. **Place 锚点条件可写**：无地界环且非折线的点状 Place 可编辑（位置是作者手摆的真值）；有环 / 折线的只读并注明派生（`fit_anchor_to_ring` 从环重算 x/y/w/h，直接改会造「两个真相」）。
5. **Cell 写入走直提**并补 `tex.mark_dirty`：沿用 `push_cell_edit` / `apply_batch` 活路径（旧代码直提不标脏，改了格地图可能不重绘——本 ADR 修复并加两条单测钉死）；**删除死链** `EditCellPending` + `CommandId::CommitEditCell`。
6. **列表行点击 = 选中**：区域 / 势力列表的行可点，写 `WorldProjection.selection` 驱动属性页签（选谁是会话态，不进撤销栈）。
7. **Region 父区域下拉本轮不做**（净新增功能，`SetRegionParentCommand` 已就位，列 v2）；**驻地 / 参与者保持自由文本 Id**（人物 Id 在 UI 列不全，下拉是半成品；V1 校验兜底）。
8. **Bevy 16 参数上限**：`panel_inspector` 以**元组折叠**装配（20+ 资源折成嵌套元组，每层算 1 参；仓内先例 `apply_pending_commands`）。

## 3. 理由（Rationale）

- **为什么单面板**：作者心智是「我看这个对象 / 我改这个对象」，不是「我找区域面板」。按选中分派把「找面板」这一步消掉了。
- **为什么 Cell 工具优先**：EditCell 工具激活时作者在改格子，不在看实体——两个表单同屏会互相打架（ADR-0040 §决策 6 的原意即工具态出 Cell 字段）。
- **为什么逐字段快照**：整实体写回的 revert 会把表单外路径对 `effects` 的改动一并误退——「撤销一次改名，抹掉作者加的效果」是数据丢失。
- **为什么删死链而不是复活它**：`pending` 无人写入、命令没进面板表；救死链要多一帧延迟且无收益。直提路径已在生产使用。
- **为什么枚举变体暂留**：删变体会让存量工程的 `ui-layout.yaml` 出现孤儿键。枚举项不在 `Panel::ALL` 里即不出现在任何用户可见列表。

## 4. 代价（Consequences）

- `panel_inspector` 是全仓参数最多的系统（元组折叠后 9 个顶层参数）；后续加表单字段不再加系统参数（表单是纯函数）。
- 三个退役面板的文件暂留（`layer_controls` / `format_span_v03` / selection 助手被 Inspector 引用），M5 后归位。
- 存量工程的 `ui-layout.yaml` 里 `faction` / `situation` / `cell_editor` 键成为无害冗余（读入但忽略）。
- Event 的 `effects` / `narrations` 本轮只读（计数展示）；要编辑它们需先改 `UpdateEventCommand` 的快照形状。
- 命令面板 / 设置对话框自动少三项（`Panel::ALL` 驱动），i18n 的 3 个 `CmdTogglePanel*` 键成为死键（暂留不炸）。

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **拆三个 system 共用窗口 id**（每页签一个系统各画 `float_window`） | egui 同 id 双画会抢窗口记忆状态；页签态要跨系统传。元组折叠是仓内已验证的路。 |
| **复活 `pending` → `CommitEditCell` 链** | 全仓无人写入的死链；复活要补注册 + 多一帧延迟，且 dispatcher 路径的标脏行为与直提不一致。直提 + 补 `mark_dirty` 更简单。 |
| **Place 锚点一律可编辑** | 有地界环的 Place 其 x/y/w/h 由环拟合派生——可编辑会造「改完又被重生成覆盖」的双真相。 |
| **Place 锚点一律只读** | 点状 Place 的位置是作者手摆的真值，锁死它等于「改位置只能手编 YAML」。 |
| **驻地 / 参与者改下拉** | 人物 Id 在 UI 里列不出来（无标签无列表联动），下拉做出来是半成品。 |
| **Cell 表单仅限 CellMap 工作区显示** | 工具即模式；工作区只是「顺带开面板」的快捷方式，加第二个判据徒增状态。 |

## 6. 与 ADR-0040 的关系

ADR-0040 §决策 6 写「Inspector Panel 加 Cell 折叠区」；落地时做成了独立浮窗（`Panel::CellEditor`，2026-10-20）。本 ADR 把它**改回 ADR-0040 的原意**并延伸：Cell 表单进 Inspector 属性页签 + 工具态优先的分派规则。按 §6.3 只增不改，ADR-0040 正文不动，仅头部加被修正标注。
