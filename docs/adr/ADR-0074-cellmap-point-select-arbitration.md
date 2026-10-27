# ADR-0074：CellMap 点选三段裁定（图钉 > 群系区 > 格）

| | |
|---|---|
| **状态** | 提议态（2026-10-27，随 feat/cellmap-pointselect-poi-biome 实施） |
| **处境** | 工具条槽 1（点选工具，ADR-0057 补记 6）在 CellMap 工作区只认格：`commit_point_select_on_click` 反查 `(gx, gy)` 写统一选区，POI 与群系区完全不在它的字典里。而 CellMap 里两者**画得出**——地物图钉（`landmarks_want` 绕过 Poi pass 的 Topographic 例外）与群系 Nature 区（`nature_overlay` 旁路）——却**选不了**：浏览态承担实体选中的 `drill_pick` 在 Topographic 下被 MODE_TABLE 的 `DrillMarkers=false` 早退，`LabelHits` 也因 `PlaceLabels=false` 恒空。作者看见图钉/群系色块，点下去却只能选到钉底下那格。 |
| **决策** | ① CellMap（Topographic 档）内点选单击按**三段裁定**：**图钉**（rank ≥ 2 点状地点，18px 屏幕半径内取最近）> **群系区**（`nature_overlay` 开时，光标落在 Nature 区多边形内）> **格**（原 ADR-0057 行为与修饰键语义原样保留）。② 命中前两段写 `WorldProjection.selection`（Place / Region `SelectedEntity`），不再写格。③ **单焦点模型**：实体选中与格选区互斥——选实体清格选区、选格清实体选中、点图外/`Esc` 双清。④ 实体命中**不吃修饰键**（Shift/Alt 并/差只对格有意义）。⑤ 浏览态零改动（仍走 `drill_pick` 标签命中）。 |
| **理由** | ① 用户原话即裁定输入：「开启群系的时候，他是选择群系……讨论什么时候是选择 POI，什么时候是 cell」+「先解决 cell map：能够选择 POI 和群系」。② 优先序=目标特异性：图钉是最小、最上层、作者瞄得最准的目标（与 ADR-0017 §2.7「标签命中优先于一切点击语义」同一精神）；群系区是面状语义目标，仅在作者显式开了群系 overlay（「我现在按群系思考」的意图声明）时才接管；格是兜底。③ 单焦点的技术动因：`resolve_form` 里 Cell 表单**恒优先**于实体选中（设计稿 §11 Q4「我在改格子」心智），若不清格选区，选了 POI 表单也看不见，等于白选。④ 只做 CellMap：浏览态已有成熟入口，重复做会与 `drill_pick` 抢点击。 |
| **代价** | ① **线段 POI（河/路）仍选不到**——无图钉、命中需折线距离测试，本轮不装（后续可用 `RiverLine.pts` 视空间折线 + 屏幕px 容差补第三段之前的并列段）。② CellMap 里想选「群系色块底下的格」必须先关 `nature_overlay`（overlay 开 = 群系接管），这是用户自己定的语义，接受。③ `Esc` 语义微扩：原只清格选区（ADR-0057 补记 3），现一并清实体选中——手势优先级（笔触/圈选先吃 Esc）不变。④ 图钉命中不走 `LabelHits` 注册表而是点击时直查当前图锚定桶：单发动作免每帧注册成本，代价是与 `landmark_dots` 的可见性口径（viewport 裁剪）有一处**宽容差**——视口外一两个身位的图钉也可点中，视为特性不是 bug。 |
| **被否决的备选** | ① **修饰键分流**（如 Ctrl 点=选 POI）：作者要先记快捷键，「单项选择器」的直觉是点谁选谁，否。② **命中也注册进 `LabelHits`**：要动 wv-render 三标签系统的清场时序（place 标签系统负责 clear，CellMap 下它空转），且每帧注册只为单击服务，否。③ **新增第四个工具槽**（「实体选择工具」）：工具条已 14 槽，用户明确要的是「第一个图标随上下文变形」，否——后续轮（笔刷态变形）同此理。 |

## 实施落点

- `crates/wv-app/src/edit/editing/cell.rs`：纯函数 `point_in_flat_polygon` / `pick_place_pin` / `pick_nature_region` + `commit_point_select_on_click` 的 Topographic 裁定分支 + `clear_cell_selection_on_escape` 双清。
- 测试：`crates/wv-app/src/edit/editing/tests.rs`（夹具 `pin` / `nature_region` / `cellmap_world`，10 条新断言）。

## 关联

- ADR-0057（统一选区）、ADR-0058（统一属性表单，`resolve_form` Cell 优先即本 ADR 单焦点的另一半）、ADR-0017 §2.7（标签命中优先级）、ADR-0045（Nature 区「命名即物化」——无名区不选）、ADR-0030（rank 门槛——rank ≥ 2 才有图钉）。
