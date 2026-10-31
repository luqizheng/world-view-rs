# ADR-0097: CellMap 双击居中编辑 dialog——四页签（群系命名/POI/单格/批量）

- 状态：已接受（2026-10-31 用户裁定：「双击弹出 dialog，固定在中央。录入之后点击确定关闭……地形编辑器这部分可以不要了」）
- 关联：ADR-0094（双击群系泛洪——本 ADR 接管其「双击后果」半场）、ADR-0089（Bevy 16 参数上限的 `drill_pick` 教训）、ADR-0045 D4（命名即物化）

## 1. 处境（Context）

ADR-0094 后双击群系 = 泛洪选中，但编辑框可达性断层：格子编辑住在笔刷窗 CellMap 编辑段（de072a3 靠双击开笔刷窗缓解），作者仍「不知道编辑框在哪里」。且用户点出工程性顾虑：往既有系统加职责会撞 Bevy 16 参数上限（`drill_pick` 曾以 17 炸合并）。用户同时裁定旧「地形编辑器」部分退役。

## 2. 决策（Decision）

1. **独立 dialog 系统 + 独立资源**（`cell_edit_dialog::CellEditDialogState`）：双击侧只置 `open_at(tab, anchor)`（参数折进既有元组，不涨签名）；dialog 系统 15 参数，居中锚定、可 X 关。
2. **四页签**：
   - **群系**：名称输入 → `materialize_nature_region_now` 物化（候选为纯派生，与 overlay 无关——「哪怕它没有创建」的命名半场）；
   - **POI**：选中地点的名称/级别/类别快改（`UpdatePlaceCommand` + `SetPlaceKindCommand` 同事务 = 一次撤销；`SetPlaceKindCommand` 随本轮补入 wv-edit re-export）；
   - **单格**：锚定格绝对设值——复用统一属性表单的 `render_fields`（提 pub(crate)）；
   - **批量**：整选区 ±海拔/换地表/换群系 + 派生运算——复用 `render_batch_section` + `render_derived_section`。
3. **选区守卫**：点/双击**已选中的格**不泛洪（保护选区）——「圈选后双击批量编辑」由此成立；页签按 `decide_tab` 裁决：泛洪改写选区（**双击序列累计**，`DoubleClickWatch.1`）→ 群系；1 格 → 单格；N 格 → 沿用上次页签（缺省批量）。
4. **地形编辑器退役**：笔刷面板 CellMap 编辑段撤除（`cell_form` 删除），只剩 EditCell 落笔表单 + 泛洪判据两段（工具参数）；读数条/派生运算迁入 dialog 批量页签。

## 3. 理由（Rationale）

- **可达性**：编辑框 = 双击直达，无「去哪里找」问题；
- **参数纪律**：独立系统是 16 参数上限下的唯一可扩展形态（用户明确点出此虑）；
- **零新机器**：单格/批量/物化全部复用现成子件与命令，dialog 只做装配与路由。

## 4. 代价（Consequences）

- **ADR 编号让位**：本决策原拟 0096，被并行会话的合成输入提交（`1c50115`）占用——顺延 0097（并行会话曾把本 dialog 半成品隔离至 `.superpowers/quarantine-parallel-session/`，依赖补齐后认领恢复）；
- **dialog 内批量「取消选区」清的是真选区**（`ResMut<CellSelection>` 直写）——与「dialog 不动选区」的直觉相悖但更实用；
- **de072a3 的「双击开笔刷面板」被取代**（回归测试改写）；
- **被否决的备选**：页签塞回统一 Inspector（ADR-0095 已拆窗，属性窗不管格子）；继续扩笔刷面板（可达性问题原样）。
