# ADR-0088：CellMap 四条可用性小修——软边笔刷接选区蒙版 / 落笔表单作用域修正 / 海拔分层滑条挪 topOptionBar / 泛洪工具退役

- **状态**：已接受（Accepted，2026-10-29。D1–D3 为用户 2026-10-28 实机反馈原话 + 「开始修复」指令；D4 的「整工具退役」范围为执行侧裁量——用户原话「泛洪选择可以去除」，判据与点选默认泛洪保留、零能力损失，待实机验收确认）
- **决策者**：用户（2026-10-28 实机走查反馈四条；2026-10-29「开始修复」）
- **关联**：**修正 [ADR-0058](ADR-0058-cellmap-unified-property-form.md) D2**（落笔作用域）、**退役 [ADR-0052](ADR-0052-cellmap-terrain-workbench.md) D6/D6-A 的泛洪工具入口**（判据保留）、**迁移 [ADR-0055](ADR-0055-cellmap-hypsometric-tint.md) 的滑条落位**（槽 10.75 → topOptionBar）；承接 [ADR-0057](ADR-0057-cellmap-unified-selection.md) 统一选区与 [ADR-0074](ADR-0074-cellmap-point-select-arbitration.md) 追记四（默认点击=泛洪）

## 1. 处境

2026-10-28 用户实机走查 CellMap 报四条小问题：

1. **「笔刷的大小可以调节」**——调查发现尺寸控件其实**都在场**：软边笔刷的半径在 topOptionBar，EditCell 硬边笔的落笔半径在浮窗落笔表单。真正的缺陷是**作用域错位**：笔刷（Brush）激活时浮窗照样显示落笔表单（落笔地表/群系/增减/半径），而这套控件驱动的是 `CellPaintSession`——**只被 EditCell 硬边拖绘消费**，笔刷的落笔走 `commit_stroke_on_release` 读 `BrushState`。作者调浮窗里的半径，笔刷纹丝不动。
2. **「多选之后选择笔刷，应该只在选区内生效，实际上哪里都可以生效」**——EditCell 硬边拖绘早有 D4 选区蒙版（`stroke_footprint` ∩ 选区），但**软边笔刷路径完全绕过统一选区**：`PaintTerrainCommand` 对蒙版一无所知。
3. **「海拔分层增强放到 top option bar 上面，而且笔刷功能常开」**——海拔分层设色强度滑条（ADR-0055）挂在 map_controls 侧工具条槽 10.75，深埋且与「读当前视图」的用法不合；topOptionBar 则按「笔刷开着才出现」显隐。
4. **「map_control 最后一个图标（泛洪选择）可以去除」**——ADR-0074 追记四已裁定 CellMap 里**点选工具的默认点击 = 泛洪**（点谁淹谁，同判据进统一选区），与泛洪工具行为**完全重复**：独立工具只是同一手势的第二个入口。

## 2. 决策

| # | 决策 | 落点 |
|---|---|---|
| D1 | **软边笔刷接统一选区蒙版**：`PaintTerrainCommand` 增设 `mask: Option<Vec<(i32,i32)>>`（升序去重，apply 时蒙版外受影响格回滚快照值，`before`/`revert` 天然只见蒙版内真实改动）；`commit_stroke_on_release` 在 `Topographic ∧ 选区非空` 时以 `CellSelection::iter_cells` 构造蒙版。**涂抹与橡皮都蒙**（同走该命令）；**意图工具不蒙**（整段生成地形，裁切会产生半座山） | `wv-edit/src/command/terrain.rs`（`new_masked` + apply ②' 步）、`wv-app/src/edit/editing/stroke.rs`、单测 `masked_paint_only_changes_cells_inside_the_mask` |
| D2 | **落笔表单作用域收窄到 EditCell**：`paint_scope = edit_cell.tool_active`（原 `∨ Brush(PaintKind)` 废止）——落笔地表/群系/增减/半径四控件对笔刷是死控件，不再显示；笔刷的调参唯一入口 = topOptionBar（半径/材料/软度本来就有效） | `wv-app/src/ui/brush_panel.rs`、`wv-app/src/ui/inspector/cell.rs`（文档同步） |
| D3 | **海拔分层设色强度滑条挪 topOptionBar；CellMap 工作区条常驻**：`bar_visible(cellmap, brush_enabled)` —— Topographic 恒显（「笔刷功能常开」= 条与海拔滑条不随笔刷开关隐没），其他工作区维持原判据；笔刷关着时工具瓷砖与笔刷参数段**整体让位**（不画死参数），海拔滑条作为视图读色参数常在。map_controls 槽 10.75 与 `DockMetrics::tint_slider_slot` 退役 | `wv-app/src/ui/top_option_bar.rs`、`map_controls/{toolbar,mod,tests}.rs`、i18n 578→580（删 1 增 2，见 D4） |
| D4 | **泛洪选择工具退役，判据保留**：删 `MapToolId::FloodSelect` 变体 / 工具条瓷砖 / `flood_select_on_click` 系统 / 分发器与互斥分支；`FloodSelectState` 收窄为**纯判据**（删 `tool_active`/`close_tool`）——点选工具的默认点击泛洪（ADR-0074 追记四）照常读它；浮窗「泛洪判据」段**保留**，显示判据改为「点选工具激活 ∧ Topographic」；`to_terrain_grid` 辅助保留（默认泛洪与派生运算共用）；i18n 删 `MapControlsFloodSelectHint` | `wv-world-cell-map/src/state.rs`、`wv-app` 的 `state/dispatch/ui_command_palette/map_controls/edit{,/editing}/schedules/resources/i18n`、`ui_shell/tests.rs` |

## 3. 理由

- **D1**：蒙版做在命令层（`apply` 内回滚）而非把笔触按格拆散，软边权重在选区边界自然保留——「内部照常、外部一格不动」与 EditCell 硬边的 D4 蒙版语义完全同构；`before` 只记真实改动，撤销粒度不变（P6）。意图不蒙是范围取舍：蒙了必然出现「半座山」，比「越界」更怪；等实机反馈再裁。
- **D2**：显示「调了没反应」的控件比不显示更糟——它正是问题 1 的根因。ADR-0058 D2 的本意是「作用域由激活工具决定」，本次只是修正「哪些工具映射到落笔作用域」：`CellPaintSession` 的消费方只有 EditCell，作用域就该只有 EditCell。
- **D3**：海拔分层是「读这幅图」的参数，不是「这支笔」的参数；槽 10.75 竖排还要独占 2 槽高（2026-10-24 撑宽问题的历史包袱）。topOptionBar 本来就是「当前工具/当前视图的参数条」，且用户点名要它常开——条常驻 + 参数段让位，两全。
- **D4**：ADR-0074 追记四之后，泛洪工具与点选默认点击是**同一行为的两个入口**，多出的瓷砖只占槽位不提供能力。判据（按地表/按群系/按海拔容差）仍是默认点击的活参数，浮窗判据段改随点选工具显示即可。

## 4. 代价 + 被否决的备选

- **代价**：
  - `PaintTerrainCommand` 增一个字段与 ②' 回滚步（蒙版非空时 O(格数) 次 `binary_search`，512² 图一笔最坏 ~5M 次比较，实测无感）；
  - `FloodSelectState` 少了工具互斥身份——若未来泛洪需要「独立于点选的预览态」，得重立工具（git 历史可捞）；
  - map_controls 纵排少了 3 槽高的内容，布局守点测试按「只减不增、断言更宽裕」口径保留原数。
- **被否决的备选**：
  - *蒙版做在 wv-app 侧把笔触拆成逐格 BatchEditCellsCommand*——丢软边权重语义，且绕开 `apply_stroke` 的跨域高度规则；否。
  - *浮窗落笔表单在笔刷激活时照常显示但置灰*——置灰仍占半屏，且暗示「这里和笔刷有关」；直接不显示更诚实；否。
  - *海拔滑条同时留在槽 10.75（双入口）*——两处状态同源但视觉冗余，视图参数不该有两家；否。
  - *泛洪只摘瓷砖留工具（命令面板可达）*——留下一具无瓷砖的僵尸工具与浮窗判据段的「FloodSelect 激活」死判据；行为重复的前提下整退役更干净；否。

## 5. 验收口径

1. CellMap 多选若干格 → 笔刷涂抹 / 橡皮：只有选区内的格变化；清空选区（Esc 或批面板「取消」）后恢复全图可涂。
2. 笔刷激活时浮窗**不**再显示落笔地表/群系/增减/半径（EditCell 激活时才显示）；topOptionBar 半径滑条对笔刷始终有效。
3. CellMap 工作区 topOptionBar 恒在；笔刷关着时条上只剩笔刷药丸（熄灭）+ 海拔分层滑条；滑条拖动 = 原槽 10.75 同款效果。
4. map_controls 最后一个图标（泛洪）消失；点选工具激活时浮窗出现「泛洪判据」段；CellMap 里点选工具单击仍按判据泛洪选中同值连通区，Shift 并 / 无修饰替换不变。
