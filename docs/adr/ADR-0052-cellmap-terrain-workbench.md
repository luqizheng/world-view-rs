# ADR-0052：CellMap 地形编辑工作台——从「单格属性表单」到「可连续绘制的地形编辑器」

**状态**：提议（Proposed，2026-10-22 按用户默认档 B 起草，待用户复述「同意 ADR-0052，按此实施」转已接受）
**编号修正**（2026-10-23）：本 ADR 原起草为 0050，与 `feat/mode-content-matrix` 分支的
[ADR-0050-unified-inspector-panel](ADR-0050-unified-inspector-panel.md)、
[ADR-0051-mode-content-matrix](ADR-0051-mode-content-matrix.md) 撞号。
按用户裁定「你推荐」（甲方案）——既有分支编号不动，本 ADR 改取 **0052**，
配套的 GPU 逐像素渲染 ADR 由 0051 改取 **0053**。
**修订**：2026-10-22 UI/UX 评审后按用户裁定「全收」并入 R1–R7 与验收项 R11——
scrub 位移阈值与气泡（R1）、待落块与选区异色相（R2）、hover 双色描边（R3）、
读数条固定双段（R4）、泛洪改工具瓷砖（R5）、scrub 补 `Ctrl` ×0.1 精修档（R6）、§4.1-1 风险改写（R7）。
同日第二批裁定（用户「好」= Q-A/B/C 全甲）：底图纯色确认（现状已满足）+ 群系可见性缺口收口 → 新增 D10。
**日期**：2026-10-22
**决策者**：用户（对话回复「好」——即 §5 的 Q1–Q3 / Q5 / Q6 全默认通过；Q4 显式保留为待裁）
**关联**：[ADR-0040](ADR-0040-cell-inspector-tool.md)（CellInspector 工具与三通道编辑——本 ADR **在其上扩展，不推翻**）、[ADR-0043](ADR-0043-map-dock-split-buttons.md)（地图工具条矩形/椭圆框选——本 ADR **复用**，不重造）、[ADR-0033](ADR-0033-selection-retained-after-intent.md)（选区保留）、[ADR-0025](ADR-0025-selection-first-tooling.md)（选择优先工具）、[ADR-0039](ADR-0039-topographic-viewmode.md)（CellMap 工作区复用 Topographic 模式 + `show_biome` 旗标分叉，见 D10——对 ADR-0039 决策 4 的边界扩展）、GLOSSARY「框选 / 底图橡皮 / 单格填涂 / 落笔值 / 同值泛洪选择 / 邻域平滑」
**原则**：P1 / P3 / P6 / P7

> **配套说明**：本 ADR 只锁「CellMap 工作区里的地形编辑能力」。
> **不**触碰渲染技法（主题 / 覆盖物 / 海面配方）、**不**触碰 `TerrainCell` 三通道的数据形态、
> **不**触碰 YAML 与 `terrain.bin` 字节布局。所有写入仍经 `EditCommand → History`。

---

## 1. 处境（Context）

### 1.1 用户裁定

> 「围绕 cell map 地图，我要增强的编辑能力。cell map 唯一目标就是编辑地形。」

这句话把工作区的**定位**收窄了：CellMap 不是「看地图的地方」，也不是「改势力的地方」，
它**只有一个目标——编辑地形**。因此本 ADR 的判据是单一且可检验的：

> **以「把一个作者想表达的地形形状落进 `terrain.bin`」为任务，每一步操作是否足够快、足够准、可后悔？**

凡是不能满足这条的，都算缺陷；凡是与这条无关的能力（势力、事件、标签、时间轴），一律不进本 ADR。

### 1.2 现状能力盘点（实测代码，非推测）

| 维度 | 现状 | 判定 |
|---|---|---|
| 单格拾取 | `commit_edit_cell_on_click`（`editing.rs:782`）只认 `just_pressed`，无拖拽 | 🟡 只能点，不能画 |
| 悬停反馈 | `EditCellState.hover`（`wv-render/src/state.rs:822`）**全仓零写入**，只在 `clear()` / `on_map_change()` 被置 `None`；渲染层注释明写「不画 hover」 | ❌ 光标压在哪一格靠盲点 |
| 海拔输入 | `numeric_input_bounded`（`wv-ui/src/widgets/controls.rs:231`）正文区是 `Sense::hover()`，**无文本键入、无拖拽 scrub**，只有 ↑↓ 两枚步进按钮；单格海拔 `step = 1.0` | ❌ 阻断级（0 → 1200 m = 1200 次点击） |
| 三通道写入 | `EditCellCommand`（单格）/ `BatchEditCellsCommand`（N 格 1 撤销单位）齐备 | ✅ 命令层无缺口 |
| 选择通道 | 单格 `selected` + 多边形 `region_cells`（由 `SelectionState.closed` 栅格化而来）；矩形 / 椭圆框选已由 [ADR-0043](ADR-0043-map-dock-split-buttons.md) 落地（成形时合成闭合点环写进 `closed`） | 🟡 选择本身够用，**但工具互斥导致流程断裂**（见 §1.3 S0-1） |
| 区域可见性 | `sync_cell_region`（`cell_selection.rs:307`）：所有选中格合成 1 个填充 mesh + **外包矩形 4 边**，填充 alpha 脉动 | 🟡 大选区看不出边界落在哪些格 |
| 批量写入 | 海拔 ±delta、设水/陆、设群系（各 1 条 `BatchEditCellsCommand`） | ✅ 够用 |
| 派生运算 | `wv-terrain` 无泛洪 / 邻域平滑 / 阈值量化（`divide.rs` 的 `smooth_ring` 是**多边形**平滑，不是格网平滑） | 🟡 缺算子 |
| 撤销粒度 | 每次字段变化 = 1 条 `Transaction`；每格 1 command | 🟡 连点 30 次步进 = 30 个撤销单位 |
| 工具入口 | `map_controls.rs:433` 明确注释「EditCell dock 瓷砖后做」，当前只能走顶栏 CellMap 段或命令面板 | ❌ 工具无可见入口 |

### 1.3 S0：三个既有缺陷（实测，与「增强」同批修）

这三条不是「新能力」，是**现有实现与 [ADR-0040](ADR-0040-cell-inspector-tool.md) 的声称不符**，
且都直接打击 §1.1 的判据：

| # | 缺陷 | 证据 | 后果 |
|---|---|---|---|
| **S0-1** | 任何 `SelectMapTool` 分派都会 `edit_cell.clear()`（`ui_shell.rs:1385` 前段），而 `clear()` 连 `selected` / `region_cells` 一起杀 | 每次点工具瓷砖 → 已选格与已框选区一起消失 | 「框选一片 → 回面板批量改」在切工具的瞬间断链；作者必须先框选再切回，切回又清空 |
| **S0-2** | `MapToolId::EditCell` 的 `already_on` 判据是 `edit_cell.selected.is_some()`（`ui_shell.rs:1365`），**不是** `tool_active` | 未选中格时再点工具 = 重新开启（永远关不掉）；选中一格后点一次 = 关工具并清空选中 | 开关语义反了；命令面板 / 顶栏的 toggle 行为不可预测 |
| **S0-3** | `edit_cell.hover` 声明即弃用（[ADR-0040](ADR-0040-cell-inspector-tool.md) 决策 5 写了两态，只落了一态） | 全仓无写入点 | 文档声称的能力不存在（属于留痕纪律层面的债） |

> **S0 的处置口径**：S0-1 改为「只有**切走** EditCell 时才关工具，且不杀 `selected` / `region_cells`」；
> S0-2 的判据改 `tool_active`；S0-3 由 D2 补齐。

### 1.4 P0：三条硬伤

| # | 硬伤 | 量化影响 | 为什么是 P0 |
|---|---|---|---|
| **P0-A** | 海拔只能 ±1 步进 | `i16` 全域 ±32767 实际不可达；改一格到 1200 m 需 1200 次点击 | 一个只能靠 ±1 点击的海拔输入，**等于没有海拔编辑** |
| **P0-B** | 无悬停反馈 | 高缩放下 1 格仅数像素；作者点偏一格才发现 | 「逐格精确」是 CellMap 唯一的存在理由，瞄不准就没有理由 |
| **P0-C** | 无连续绘制 | 改 50 格 = 50 次点击 + **50 个撤销单位** | 地形编辑的主要形态是「涂一片」，不是「点一格」 |

### 1.5 诚实修正：上一轮盘点里被误判为「缺失」的能力

上一轮对话把「矩形框选」列为缺失能力，**这是错的**：矩形 / 椭圆框选在
[ADR-0043](ADR-0043-map-dock-split-buttons.md) 已落地（框选手势成形时合成闭合点环写进
`SelectionState.closed`，下游六个消费点零改动），`rasterize_polygon_cells`
（`cell_selection.rs:138`）天然接受矩形点环。

**CellMap 里真正的缺口不是「没有矩形选择」，而是「选择工具与单格编辑工具互斥，
切一次就丢一次状态」（S0-1）**。本 ADR 因此把预算从「造矩形选择」挪到「修断裂」。

同理，「按 biome 全选一片」按现有实现也不是完全缺失——多边形足够圈住大多数场景，
缺的是**不画多边形就能圈住连通同类片**的快捷通道（D6 的泛洪选择）。

---

## 2. 决策（Decision）

### D1 · 数值输入可键入、可拖拽 scrub（修 P0-A）

改造 `wv-ui::widgets::controls::numeric_input_bounded`，正文区从 `Sense::hover()` 升级为：

| 手势 | 行为 |
|---|---|
| 按下后位移 ≤ 4 px 并松开 | 进入文本编辑态（覆盖 `TextEdit`，**进入时全选既有值**，直接键入即覆盖；`Enter` 提交 / `Esc` 取消 / 失焦提交） |
| 按下后位移 > 4 px | **scrub**：每 `SCRUB_PX = 4` 像素对应 1 个 `step`；`Shift` ×10、`Ctrl` ×0.1（精修）、`Alt` ×100 |
| 点按 ↑ / ↓ | 保留现有 `step` 步进（**行为不变**，老路径零迁移） |

- **判定顺序是控件规格的一部分，不是实现细节**（egui `DragValue` / Figma / Blender 数值框的通用判定）：
  `press` 时**不**立即决定意图——位移超过 4 px 才进 scrub；未超过则 release 时进编辑态。
  不锁这条，「点按」与「拖拽」会在实现者手里各解释一遍：或「每次点击都闪一下编辑态」，或「永远进不了文本编辑」。
- **scrub 气泡**：scrub 进行中在光标上方浮动显示当前值 + 单位，给「正在拖动改值」即时反馈（误拖当场可见）；松手消失。
- **修饰键三档（快慢兼顾）**：`Ctrl` ×0.1 补齐**减速档**——批量增减 `step = 10` 时想表达 ±5 m 只有精修档够用。
  `Alt` 采用前需确认相机与全局快捷键无既有绑定（初查 `camera.rs:420-429` 仅绑 WASD / 方向键 / 中键拖拽，`Alt` 空闲，实施时复核）。

- **`step` 参数语义不变**：scrub 的 1 格 = 原 `step`。单格海拔 `step = 1.0` ⇒ 拖 4 px 变 1 m、`Shift` 变 10 m、`Ctrl` 变 0.1 m、`Alt` 变 100 m。
- **钳制不变**：`clamp(min, max)` 仍是唯一出口；非法文本（空 / 非数字 / NaN）提交时回滚到编辑前的值，不写 `0`。
- **调用方零改动**：`numeric_input_bounded` 当前仅被 `cell_editor_panel.rs` 两处使用（`:287` 单格海拔、`:597` 批量增减），签名不动。
- **`wv-ui` 契约不动**：[ADR-0026](ADR-0026-wv-ui-ui-widget-crate.md) 的「零兄弟 crate 依赖」不受影响，仍只用 `bevy` / `bevy_egui` / `egui_extras`。

### D2 · 悬停格高亮（修 P0-B / S0-3）

1. **写入**：新增系统 `track_edit_cell_hover`（`wv-app::edit`），每帧在
   `tool_active ∧ !ui.wants_pointer ∧ !playback.playing` 时调
   `cell_pick::pick_at` 写 `EditCellState.hover`；条件不满足写 `None`。
   与 `commit_edit_cell_on_click` 共用同一 gate 口径（`PointerOverUi` / 播放只读）。
2. **渲染**：新增 `CellHoverEntities`（`wv-render`），与 `CellSelectionEntities` **并存但独立**
   （同一时刻要同时画 hover 与 selected，不能复用实体集）：
   - **双色描边**：outline 与准星一律「白 `0xFFFFFF` 内线 + 深墨 `0x382F27`（`LINE_ON_DARK` 同源）外晕」双层画法
     （外晕宽 = 内线宽 + 2 px），不用降透明。理由：Topographic 底图水面（暗）与陆地（亮）明度跨度大，
     **单色细线无法同时在两种底色上满足 WCAG 2.2 UI 组件 3:1**；白 + 深外晕是地图准星的行业成熟解
     （Google / 高德系准星皆双色），不必发明。若沿用金色 `ACCENT_SELECT`，hover（临时）与 selected（committed）
     只剩 1 px vs 2 px 之差，高缩放下不可辨。
   - 1 格 outline，内线屏幕恒定 **1 px**（selected 是 2 px），**不脉动**（脉动 = 干扰瞄准）；
   - **十字准星**：以格心为中心、屏幕恒定长度的两条 1 px 线（长度 = `max(格宽 × 3, 24 px)`），同款双色；
     解决「格子太小看不见框」的场景；
   - gate = `recipe.enabled(PassId::CellSelection) ∧ hover.is_some()`。
3. **与 selected 的视觉分工**：hover 细线 + 准星（临时），selected 粗线 + 半透底 + 脉动（committed）。
   两者重合时 selected 覆盖 hover。

### D3 · 连续绘制：一次按下-抬起 = 1 个撤销单位（修 P0-C）

**形态**：`MapToolId::EditCell` 下，左键 `pressed` 期间连续采样，`just_released` 时**一次提交**。

| 阶段 | 行为 |
|---|---|
| `just_pressed` | 记录起点格，开 `CellStrokeState`（会话态），清空累积集合 |
| `pressed`（每帧） | `pick_at` 当前格；与上一格不同则用 **Bresenham 补间**（避免快速拖动漏格），去重后并入集合 |
| 绘制中 | 集合写入 `EditCellState.stroke_cells`，渲染层画**待落色块**（复用 `CellRegionEntities` 的批 mesh 形态，第二实例；**红橙 `STROKE_PENDING`（新增 `wv-theme` 常量，建议 `0xD9634E`）alpha 0.26，无脉动**）+ 面板读数显示「待落 N 格」 |
| `just_released` | 逐格读 `before` → 应用 D4 落笔值 → 组装 `Vec<CellChange>` → **1 条 `BatchEditCellsCommand`** → `History.commit` |
| `Esc`（绘制中） | 丢弃整个 stroke，不写世界 |

- **不写世界直到松手**：与笔刷 `commit_stroke_on_release`（`editing.rs:90`）**完全同款**
  ——拖拽期只累积点列，松手才构造命令。这条一致性是刻意的：两套「拖拽改地形」若一套实时写、一套松手写，
  撤销栈与 dirty 传播会出现两套语义。
- **不实时改地形**：实时预览需要在每帧重烘焙地形纹理（`TerrainTexture`），代价高。
  本 ADR 的预览只到「待落格色块」，**不做地形像素级实时预览**（列为 §4.2 v2）。
- **待落块必须与选区异色相**：选区填充是金色 `ACCENT_SELECT`（`0xF2C14E`，alpha 0.10–0.34 脉动）。
  「框选一片 → 再涂」是 S0-1 修完后的标准工作流，若待落块也用金，两种填充叠加后作者分不清
  哪些格已入笔触、哪些只是选区。**分工口径：金 = 已提交语义**（selected / region），
  **红橙 = 未提交语义**（stroke pending）；约束为两色色相角差 ≥ 30°，具体 hex 实施时可微调。
- **松手时校验**：任一格 `before` 与命令内记录不一致（期间被撤销 / 外部写入）→ 整笔拒收并告警
  （沿用 `push_cell_edit` 的 stale 快照口径，`cell_editor_panel.rs:771`）。

### D4 · 落笔值语义：surface / biome 绝对，height **相对**

`CellPaintValue`（会话态，挂在 `EditCellState` 上）：

```rust
pub struct CellPaintValue {
    /// 落笔写入的地表（绝对，二值）。
    pub surface: Surface,
    /// 落笔写入的群系（绝对）。
    pub biome: Biome,
    /// 落笔的海拔**增量**（米）。`0` = 不动海拔。
    pub height_delta: i16,
}
```

**为什么 height 必须是相对量**：地形的高度场是连续起伏，绝对设值会把「这一片本来多高」抹平——
这与 2026-10-20 用户在批量区的既有裁定同一条理由（`cell_editor_panel.rs:582-585` 注释：
「批量编辑只提供整体加减，不提供统一设为某值——绝对设值会让『这一片本来多高』的信息被覆盖掉」）。
本 ADR **把这个裁定从批量区扩展到连续绘制**，两条路径口径一致。

**单格海拔仍是绝对值**：点选一格 → 面板海拔输入框 → 键入 1200 = 该格就是 1200 m。
两条语义**并存且分工明确**：

| 路径 | height 语义 | 面板控件 |
|---|---|---|
| 单格（`selected`） | **绝对**（键入即设定） | 海拔输入框（D1 改造后） |
| 连续绘制 / 批量 | **相对**（delta） | 海拔增减输入框（默认 0 = 不动） |

面板上两个控件**必须分区标注**（「此格海拔」/「落笔增减」），不得相邻无标签地并排。

### D5 · 硬边单格：不引入半径与 falloff

CellMap 的笔刷形态 = **1 格、硬边、无衰减**。

- 理由：CellMap 的承诺是「逐格精确」。一旦有软边，作者无法在画面上判断某个边缘格到底落没落笔
  ——`wv_terrain::edit::falloff_weight` 的 50% 阈值规则（`edit.rs:237`）在逐格场景里是**不可见的知识**。
- **不复用 `BrushState`**：半径 / 软度 / 强度三个字段在硬边单格语义下全部无意义，塞进 `BrushState`
  会让状态机出现「恒为 1.0 / 恒为 0.0」的死字段。落笔值走独立的 `CellPaintValue`。
- 与 `BrushTool::PaintKind`（软边成片涂抹）的分工：`PaintKind` 是「画大致形状」，
  CellMap 连续绘制是「修边缘、画一格宽的线（堤坝 / 崖脚 / 一格宽的河）」。

### D6 · 派生运算收口到 `wv-terrain` 纯函数

新增三个纯函数（`wv-terrain`，零第三方依赖、无 Bevy、无文件 IO，符合 §2.1）：

| 函数 | 签名要点 | 用途 |
|---|---|---|
| `flood_region` | `(grid, seed, pred: impl Fn(TerrainCell) -> bool, connectivity: Conn4\|Conn8) -> Vec<(i32,i32)>` | 同值泛洪**选择**（不写世界） |
| `smooth_cells` | `(grid, cells: &[(i32,i32)], passes: u8, strength: f32) -> Vec<(i32,i32, Height)>` | 邻域平滑，**返回新值不回写**（由调用方组 `BatchEditCellsCommand`） |
| `quantize_heights` | `(grid, cells, step_m: i16) -> Vec<(i32,i32,Height)>` | 按 N m 阶梯取整（做等高线地形） |

- **三个函数都不改世界**：返回变更集，写入由 `BatchEditCellsCommand` 统一落盘。
  这条约束保证「撤销单位 = 1 次操作」，也让 `wv-terrain` 保持纯函数可测。
- **泛洪选择的交互形态 = 独立工具瓷砖，不是面板按钮**：新增 `MapToolId::FloodSelect`
  （`map_controls` 第 13 槽，Lucide `paint-bucket`），active 态下左键点击地图 = 以该格为种子泛洪、
  结果写入 `region_cells`（复用既有选区渲染与批量通道）；`Esc` / 右键退回前一工具。
  判据选择（按地表 / 按群系 / 按海拔容差）留在 CellEditor 面板做 segmented——
  **面板管参数、瓷砖管手势**：杜绝「点完按钮后地图点击行为变了、界面上却没有任何工具态提示」的隐式模式
  （与 [ADR-0040](ADR-0040-cell-inspector-tool.md) §1.3「工具语义要钉清楚」同旨）。
- **泛洪选择的判据**：默认按 `biome` 相等；面板给「按地表 / 按群系 / 按海拔容差」三种。
- **M2 只做 `flood_region` + `smooth_cells`**，`quantize_heights` 留 M3。

### D7 · CellMap 工作区的工具可见性

1. **补 `EditCell` 的 dock 瓷砖**（补 [ADR-0040](ADR-0040-cell-inspector-tool.md) 决策 2 欠的账）：
   `map_controls` 第 12 槽，Lucide `square-mouse-pointer`，active 态绑 `edit_cell.tool_active`。
   `MapToolId::FloodSelect` 为第 13 槽（Lucide `paint-bucket`，交互形态见 D6）。
2. **CellMap 工作区面板编排不变**（`top_bar.rs:232` 的快照/还原机制已正确）；
   只在 M3 评估是否把「读数条」做成 CellMap 专属顶栏条（v1 留在面板内，避免动顶栏布局）。
3. **S0-1 修复**：`SelectMapTool` 分派改为
   - 切走 EditCell → 只 `tool_active = false`（新方法 `close_tool()`），**不杀** `selected` / `region_cells`；
   - 切图 / `Esc` → 仍走全量 `clear()`。
   与 [ADR-0033](ADR-0033-selection-retained-after-intent.md)「选区不因其它动作蒸发」同旨。
4. **S0-2 修复**：`already_on` 判据改 `edit_cell.tool_active`。

### D8 · 常驻读数条

CellEditor 面板顶部（模式切换段之上）常驻一行等宽读数：

```
光标  128, 340 · 1 240 m · 陆 · 森林   │   选中  132, 341 · 1 190 m · 森林
```

- **固定双段，语义不换槽位**：左段永远读 `hover`（光标下的格），右段永远读 `selected`（已提交的格）；
  任一段无数据就显示 `—`。**不允许**「同一槽位在 hover 缺失时静默回退到 selected」——
  一行读数两种来源，作者会误读「海拔 1 240 m」指哪格（hover 与 selected 常常不是同一格）。
- 理由：地形编辑的每一步都在和数字打交道，「必须先点下去才能看到这格多高」是多余的往返。
- 该行**只读**，不参与写入，不引入新状态。

### D9 · 明确不做（本 ADR 边界）

| 不做 | 理由 |
|---|---|
| 实时地形像素级预览（拖拽中看到地形变色） | 需逐帧重烘焙 `TerrainTexture`；列为 v2 |
| 图章 / 复制粘贴一片地形 | 需求未验证，M3 之后再看 |
| 统一选择模型（把 `selected` + `region_cells` 合成一个 `CellSelection`） | 用户裁 Q5 = 暂不；重构收益不足以抵消回归面 |
| 方向键 nudge | 与相机 WASD / 方向键冲突（`camera.rs:420-429`），**保留待裁**（§6 Q4） |
| CellMap 跨子图编辑 | 沿用 [ADR-0040](ADR-0040-cell-inspector-tool.md) 默认：只对当前图生效 |
| 任何 YAML / `terrain.bin` 布局改动 | 数据层零迁移是本 ADR 的硬约束 |

### D10 · CellMap 底图：群系可见的纯色格（2026-10-22 第二批裁定）

**裁定来源**（用户原话）：
> 「cell map 的时候，应该是保持纯色吧。不加 theme」
> 「之前的设计只有水/陆两种颜色，现在增加了群系，是不是要加多点颜色用于区别」→「好」（Q-A/B/C 全甲）

**现状实测**：
- 「纯色不加 theme」**现状已满足**：Topographic 渲染分支（`cells.rs::write_cell_split`）res 恒 1、
  一像素直铺，注释明写「无边界/揉边/淡化/浮雕——分析视图四色是语义色，任何后处理都是污染」。
  本条裁定是对现状方向的确认与锁定，不是修改。
- **缺口是群系不可见**：[ADR-0039](ADR-0039-topographic-viewmode.md) 决策 4「X 路线」刻意只读
  `height + surface`，调色仅 4 语义令牌（水单蓝 + 陆 3 阶）。在 CellMap 编辑语境下后果：
  ① 涂森林与涂沼泽画面零区别（盲改）；② 泛洪选择（D6，默认按群系判据）结果不可视；
  ③ 落笔值面板选群系时无法对照底图核对。

| 子决策 | 内容 |
|---|---|
| **D10-1 色源** | 复用 `wv-theme::CORE_TERRAIN` 10 色（零新色板），biome→槽位映射：DESERT→`sand`、GRASSLAND→`grass`、FOREST→`forest`、HILLS→`hill`、MOUNTAINS→`mountain`、SNOWFIELD→`snow`、SWAMP→`marsh`、ROAD→`road`、`NONE`→中性陆色兜底。区分度已过设计（色板注释：「森林与草靠明度+色相差区分」），映射通路与 brush swatch 表（`shell.rs:74-80`）同款。**水格照旧水色，biome 不参与水格显示**（排干成陆后群系色即显） |
| **D10-2 海拔可视** | 陆格着色 = **色相（群系）+ 明度（海拔 3 阶）** 双通道合成：明度调制复用 ADR-0039 的百分位 33/66 阈值缓存（`ViewState::topographic_thresholds`），调制幅度 **±12%** 封顶——雪原（`0xF5F8F9` 已近白）/深林（`0x3F7A46`）等极值色上 3 阶差会小于草原/丘陵，接受，等高线开关（ADR-0039 决策 7）兜底。**一格仍是纯色**，不引入晕开/装饰后处理 |
| **D10-3 架构落点** | Topographic 渲染分支加**运行时旗标** `ViewState.show_biome: bool`（默认 `false`）。默认 `false` = ADR-0039 分析视图原语义不动（不读 biome）；CellMap 工作区进入时置 `true`、退出还原（走 `top_bar.rs` 既有工作区快照/还原机制）。**不新增 ViewMode**——CellMap 仍是 UI 编排，MODE_TABLE / 模式轴 / 断言零改动 |

- **与 [ADR-0039](ADR-0039-topographic-viewmode.md) 的关系是边界扩展，不是推翻**：决策 4「X 路线」
  的语义（分析视图只看水/陆 + 海拔 3 阶）完整保留；`show_biome` 只在 CellMap 编辑语境下打开
  biome 读取。分析视图与编辑视图从同一条渲染分支分叉，配色知识仍在渲染层单点
  （`view.mode == Topographic ∧ show_biome` 是渲染层单点判断入口，`wv-edit` / `wv-core` 不引用旗标，
  符合 ADR-0039 决策 10 的 API 表面约束）。

---

## 3. 理由（Rationale）

1. **判据单一**：§1.1 的「快 / 准 / 可后悔」三个词直接映射到 P0-A（快）、P0-B（准）、P0-C（可后悔——
   一次笔触一个撤销单位）。三条硬伤不是堆功能，是把定位欠的债还上。
2. **D3 与笔刷同款是正确的保守**：`commit_stroke_on_release` 已经是本项目「拖拽改地形」的既定形态。
   让 CellMap 走同一形态，撤销栈、dirty 传播、播放期只读三条规则都不需要第二套实现。
3. **D4 的相对高度不是新发明**：它是 2026-10-20 批量区裁定的延伸。同一份 UI 里出现两种高度语义
   是有风险的，但「绝对 vs 相对」的差异**本来就存在**（点一格改一格 vs 刷一片），
   把差异显式化成两个分区控件，比让连续绘制也能绝对设值要安全得多。
4. **D5 拒绝软边是刻意的减法**：CellMap 的价值在可预测性。软边的 50% 阈值在成片涂抹里是经验，
   在逐格编辑里是黑箱。
5. **D6 的三个函数都在 `wv-terrain`**：泛洪 / 平滑 / 量化是**地形数据的领域运算**，不是 UI 行为。
   放 `wv-app` 会让 `wv-terrain` 的网格知识外泄到应用层，且无法被 CLI / `wv-edit` 复用。
   与 `is_water_cell` 放 `wv-terrain` 是同一条理由（`edit.rs:24-26`）。
6. **S0 与本 ADR 同批修**：S0-1 直接破坏「框选 → 批量改」的主流程，不修的话 D3 / D6 的价值会被
   工具切换的断裂吃掉一半。把它们捆在一个 ADR 里，是为了让「切工具不丢选区」和「连续绘制」
   两件事一起被验收。
7. **D10 加群系色是 WYSIWYG 底线，不是加功能**：biome 是三通道之一，CellMap 的判据是「编辑地形」——
   涂了看不见等于盲改。复用 `CORE_TERRAIN` 而非新造色板，是因为区分度已经过设计且映射通路现成；
   用「色相 + 明度」双通道而不是加第 5+ 个语义令牌，是因为一格一色的前提下这是把两个数据通道
   同时可视的唯一无损做法。

---

## 4. 代价（Consequences）

### 4.1 负面后果（必须承认）

1. **scrub 误触发改值**：正文区在面板内，拖拽**永远不会平移地图**——实际风险是
   「想点按进编辑却手抖位移 > 4 px」被判定为 scrub 改了值。缓解三件套：
   4 px 位移阈值（低于阈值不进 scrub）、scrub 气泡即时显示当前值（误拖当场可见）、
   `Esc` 回滚到拖前值；触屏用户仍可用 ↑↓ + 文本键入。
2. **悬停高亮带来每帧一次 `pick_at`**：常数极小（一次除法 + 两次 floor），但它是**每帧**新增开销。
   实测影响应在噪声内；若在 512² 图上出现掉帧，退路是「鼠标静止 30 ms 才拾取」。
3. **连续绘制期间看不到地形变化**：只有待落格色块。作者画一条一格宽的山脊时，判断「这条线对不对」
   要等松手。这是本 ADR 最大的体验缺口，明确接受。
4. **一个撤销单位可能覆盖几百格**：`BatchEditCellsCommand` 存的是每格 `before` / `after`（约 4 字节 × 2 / 格），
   500 格 ≈ 4 KB，可接受；但撤销栈在长时间绘制后会累积。缓解：不做额外措施（笔刷同款问题，未爆过）。
5. **`numeric_input_bounded` 是全项目控件**（当前仅 2 个调用方）：
   改造后 `Shift` / `Ctrl` / `Alt` 三个修饰键在该控件 scrub 中被吃掉，若外层有同名快捷键会冲突。已知无冲突，但需回归。
6. **`CellPaintValue` 与 `BrushState` 并存**：两套「落笔值」概念。缓解：两者只共享 `surface` / `biome`
   的语义，UI 上分属两个面板（Brush 面板 / CellEditor 面板），不并排。
7. **CellMap 底图与分析视图视觉语言分叉**（D10）：`CORE_TERRAIN`（10 色彩度较高）与 Topographic
   分析四色不是一套视觉语言；雪原 / 深林等极值色上明度 3 阶差被 ±12% 封顶压扁。前者是刻意的
   （编辑视图要「数据通道可辨」不要「分析语义一致」），后者接受并由等高线开关兜底。

### 4.2 后续可能（不构成本 ADR 决策）

- **实时地形预览 v2**：拖拽期把待落格写入一张**临时覆盖纹理**（不碰 `terrain.bin`、不碰 `World`），
  松手清空。需要新 `PassId` + 一张 RGBA 覆盖图；是否值得取决于 §4.1-3 在实际使用中有多痛。
- **量化 / 阈值转水 v2**（`quantize_heights` + `height < X → water`）：做等高线风格的常规操作。
- **图章 v2**：复制 N×N 块作为落笔图案。
- **多格 nudge**:选区整体平移（复制到偏移位置），与「方向键 nudge」是两件不同的事。

### 4.3 被否决的备选方案

| # | 备选 | 否决理由 |
|---|---|---|
| A | **连续绘制实时写世界**（拖拽期直接改 `World`，松手补命令） | 违反 AGENTS §3.3「System 不得直接改世界」；且与笔刷 `commit_stroke_on_release` 形成两套语义 |
| B | **每格 1 条 command 累积在同一个 Transaction 里逐帧 commit** | `History::commit` 一旦提交就进撤销栈，无法「先提交后合并」；要合并得给 History 加 merge API（改动面远大于收益） |
| C | **把 CellMap 绘画塞进 `BrushTool` 子分支**（`BrushTool::CellPaint`） | 与 [ADR-0040](ADR-0040-cell-inspector-tool.md) §4.3-A 同一条否决理由：Brush 是批改、Inspector 是点取，混进去会让 `BrushState` 出现「半径恒 1 / 软度恒 0」的死字段 |
| D | **海拔走滑块（slider）而非 scrub + 键入** | ±32767 全域上滑块的分辨率不可用（1 m 精度下拖 1 px 跳几百米）；滑块适合有界小范围参数，不适合米级绝对高度 |
| E | **悬停高亮复用 `CellSelectionEntities`** | 同一时刻要同时画 hover 与 selected，复用必然互相覆盖；且两者视觉参数不同（1 px vs 2 px、是否脉动） |
| F | **把泛洪 / 平滑做进 `wv-app` 的 panel 里** | 领域运算外泄到应用层；`wv-terrain` 的网格知识应留在 `wv-terrain`（同 `is_water_cell` 的归属理由） |
| G | **切工具时保留 `tool_active`（不关工具）** | 与 [ADR-0040](ADR-0040-cell-inspector-tool.md) §1.3「工具语义要钉清楚」相悖：两个工具同时在 active 态会让「左键会做什么」不可预测 |

---

## 5. 实施分期与验证

### M1 · 修硬伤（P0-A / P0-B / S0）

```
crates/wv-ui/src/widgets/controls.rs        :: numeric_input_bounded 正文区 scrub + TextEdit（D1）
crates/wv-app/src/edit/editing.rs           :: track_edit_cell_hover（D2 写入侧）
crates/wv-render/src/state.rs               :: （无字段新增；hover 复用既有字段）
crates/wv-render/src/cell_selection.rs      :: CellHoverEntities + sync_cell_hover（D2 渲染侧）
crates/wv-app/src/ui_shell.rs               :: already_on 判据(=tool_active) + 切走只 close_tool（S0-1/2）
crates/wv-render/src/state.rs               :: EditCellState::close_tool() 新方法
crates/wv-app/src/ui/map_controls.rs        :: EditCell dock 瓷砖（D7-1）
i18n                                        :: 新 key ~6 条（瓷砖 tooltip / 读数条标签）
```

**验收**：① 海拔可键入 1200 并回车生效（进入编辑态时旧值全选，直接键入即覆盖）；
② 拖拽海拔正文 4 px 变 1 m、`Shift` 变 10 m、`Ctrl` 变 0.1 m，scrub 中有浮动值气泡；
③ 悬停出现细框 + 准星，**在水面与陆地两种底色上均肉眼可辨**（对比度缺陷不测就发现不了）；
④ 框选一片 → 切 EditCell → 选区仍在；⑤ 工具瓷砖二次点击能关。

### M2 · 连续绘制 + 派生运算

```
crates/wv-app/src/edit/editing.rs           :: CellStrokeState + paint_edit_cell_on_drag（D3）
crates/wv-render/src/state.rs               :: ViewState.show_biome 旗标（D10-3）
crates/wv-render/src/terrain_view/cells.rs   :: Topographic 分支 show_biome 群系着色 + 明度调制（D10-1/2）
crates/wv-app/src/ui/top_bar.rs              :: CellMap 进入/退出置位 show_biome（D10-3，走既有快照/还原）
crates/wv-render/src/cell_selection.rs      :: 待落色块第二实例（D3 预览，红橙 STROKE_PENDING）
crates/wv-theme/src/colors.rs              :: STROKE_PENDING 常量（D3，与 ACCENT_SELECT 色相角差 ≥ 30°）
crates/wv-terrain/src/edit.rs               :: flood_region + smooth_cells（D6）
crates/wv-app/src/ui/map_controls.rs        :: FloodSelect dock 瓷砖（D6，第 13 槽）
crates/wv-app/src/ui/cell_editor_panel.rs   :: 落笔值区（D4）+ 泛洪判据 segmented + 平滑按钮 + 读数条（D8）
```

**验收**：① 拖拽 50 格松手 = 撤销栈 +1，撤销后 50 格全回来；② 快速拖动不漏格（Bresenham 补间，
测试用「每帧跳 10 格」的采样序列断言连续性）；③ 切到 FloodSelect 瓷砖后点一下 = 选中一片森林；
④ 平滑 1 遍后锯齿消失且海拔不进位漂移（均值在中值 ±1 内）；
⑤ 底图群系可辨：涂森林与涂沼泽颜色不同（D10-1），同群系不同海拔明度可辨（D10-2），
水格不受 biome 干扰、退出 CellMap 后分析视图恢复四色原样（D10-3 旗标还原）。

### M3 · 收尾（可选）

量化、边界收缩/扩张、CellMap 专属顶栏读数条评估。

### 测试门槛（AGENTS §7.1）

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

新增测试：`numeric_input_bounded` scrub 累加与钳制 + 4 px 阈值判定（5）、`close_tool` 不清 `selected`（2）、
`flood_region` 4/8 连通与越界（5）、`smooth_cells` 保边界与不漂移（3）、
Bresenham 补间连续性（2）、`CellPaintValue` 应用语义（3）、
`show_biome` 着色（水格不受 biome / `NONE` 兜底 / 明度调制 ±12% 钳制）（3）。

---

## 6. 待裁（用户显式保留）

| 编号 | 问题 | 现状 | 我的建议 |
|---|---|---|---|
| **Q4** | 方向键 nudge 选中格 | `camera.rs:420-429` 已把 WASD + 方向键绑平移 | **不抢方向键**。若要做，用 `Ctrl + 方向键`（±1 格）/ `Ctrl + Shift + 方向键`（±10 格）；需先确认相机系统的修饰键判断不冲突。**在 M2 之后的独立小 PR 里决定** |
| **Q7**（新增） | 泛洪选择的默认判据 | 提案为「按群系」 | 若作者的常用动作是「圈住一片同海拔的山脉」，默认应改为「按海拔容差 ±N m」。**倾向按群系**（同群系 = 语义同类），但请在实际使用后反馈 |

---

## 7. 涉及与不涉及（AGENTS §10 自检）

**触及**：

- §6.4（本 ADR 即留痕）
- §3.3「不直接改世界」：D3 的所有写入仍经 `BatchEditCellsCommand`；D6 的三个函数**只返回变更集**，
  不持有 `&mut World`
- §7.1 测试门槛：新增约 23 条测试（见 §5）
- §2.1 依赖方向：`wv-terrain` 新增函数零第三方依赖；`wv-ui` 契约不变；`wv-render` 仍不依赖 `wv-edit`
- [ADR-0039](ADR-0039-topographic-viewmode.md) 决策 4 的**边界扩展**（D10）：`show_biome` 旗标只在
  CellMap 语境打开 biome 读取，分析视图原语义（只读 `height + surface`）完整保留；
  决策 10 的 API 表面约束延续（旗标不进 `wv-edit` / `wv-core`）

**未触及**：

- §1 P1–P7 全部：P3（YAML 为主格式）不动，P6（非破坏编辑）由 `BatchEditCellsCommand` 的
  before/after 快照保证，P7（时间轴驱动）与 cell 编辑正交
- §4 数据模型约定：`TerrainCell` 三字段不动，`terrain.bin` 字节布局（I16 + U8 + U8）零变化
- §2.1 依赖方向（详见上）
- [ADR-0040](ADR-0040-cell-inspector-tool.md) 的 D1–D10：本 ADR 只**补**其决策 5 未落地的一半（hover）
  与决策 2 未落地的工具瓷砖，**不推翻任何已接受条款**
- [ADR-0043](ADR-0043-map-dock-split-buttons.md) 的框选：**复用**，不重造

---

## 8. 转「已接受」的条件

用户在对话中明确复述「同意 ADR-0052，按此实施」即转状态为「已接受」。
转已接受后，本 ADR 正文**不再修改**；后续推翻需新开 ADR（[AGENTS §6.4](../../AGENTS.md)）。
**在此之前不得据此实现代码**（ADR README §3「提议（Proposed）… 此时不得据此实现代码」）。

---

## 9. 修订记录 · 2026-10-24 UX 审视后并入（用户裁定「全收 + 并入」）

**背景**：Figma 界面工坊（主理人 + 布局师 + 交互师三人组）对 ADR-0052 做落地前 UX 审视，
识别出 8 项状态机 / 交互 / 反馈补强。2026-10-24 08:02 用户裁定「全收 + 并入」（含前一日已裁定
的 D3-A 起笔清空 region_cells）。

**留痕口径**：本节以**追写**方式补子条款编号（D1-A / D1-B / D3-A / D3-B / D3-C / D3-D / D3-E /
D6-A / D8-A / D8-B / Gate-1 / Gate-2），**不修改 D1–D10 既有条款文字**——通过子条款建立语义映射，
原 D 系列仍为权威源。子条款未与母条款冲突时母条款照旧生效。

### 9.1 状态机扩展（并入 D3 起笔 / D6 泛洪）

**D3-A · 起笔清空 region_cells**（2026-10-24 07:39 用户裁定「#1 好」）：
`mouse_down + drag > 4px` 进入 `stroke-drawing` 瞬间清空 `region_cells`。
S0-1 的「切工具保留选区」**不延伸**到「工具内起笔」——起笔是比切工具更明确的意图信号
（从「选择」切换到「涂画」）。**选区与笔触不可同框**：视觉优先级与操作绑定都不可推理。

**D3-B · 拖绘中右键 = Esc 等价**（裁定 #3）：
右键丢弃整笔，不半笔提交。半笔提交的 History 语义不直觉；右键在地图编辑器的直觉是
「取消 / 上下文菜单」而非「部分确认」。想提前提交松左键即可。

**D3-C · Esc 大笔触确认阈值**（裁定 #2）：
`stroke_cells.len() ≤ 50` 静默丢弃；`> 50` 时 toast 确认
「确认丢弃 N 格笔触？[丢弃] [继续]」。阈值 50 可调。小笔触丢弃无成本（重画快），
大笔触误丢成本高（用户刚画了几百格），一句确认可防手滑。

**D3-D · 拖绘中切工具 / 切图 / 拉出画布**：

| 场景 | 行为 | 理由 |
|---|---|---|
| 切工具（按 V/H 等快捷键） | 丢弃整笔，无提示 | `CommandPalette` 互斥已切上下文；半笔 `CellStrokeState` 跨工具存活无意义 |
| 切图（MapTree 点击） | 全量 `clear()`（含 stroke） | 上下文整体切换，S0-1「切图才全量 clear」在此严格执行 |
| 鼠标移出画布 viewport（窗口内） | `pick_at = None` 帧不延伸，stroke 存活 | 用户可能在沿边缘调整路径 |
| 鼠标移出窗口 | egui 鼠标捕获丢失时 stroke 冻结在最后有效格，re-enter 后恢复延伸 | — |
| 跨入另一子图区域 | `pick_at` 只对当前图有效 → stroke 不延伸 | CellMap 默认只对当前图生效 |

通用规则：**只要左键仍 pressed，stroke 存活；pick_at 返回 None 的帧 = 不延伸帧**；release 时提交已落格。

**D6-A · 泛洪选择交互形态**（裁定 #5 的状态机部分，对应原 D6 交互细节化）：
click 立即泛洪（**无预览→确认中间态**）；结果**替换** `region_cells`（不累加）；
Shift 并集留 v2。判据 segmented（按群系 / 按地表 / 按海拔容差）放 CellEditor 面板；
选「按海拔容差」时出现 scrub 容差输入框（沿用 D1 手势）。`flood_region` 慢时用状态栏
「计算中…」做感知补偿。

### 9.2 输入补强（并入 D1 scrub）

**D1-A · SCRUB_ARMED 8px 起手宽限期**（裁定 #6）：
- `press` → 位移 `≤ 4px` release → 进 TEXT_EDIT
- `press` → 位移 `> 4px 且 ≤ 8px` release → **SCRUB_ARMED 宽限期**，静默回滚原值（吞手抖，不进编辑态）
- `press` → 位移 `> 8px` → 进 SCRUB_STEPPING，4px/step 写值
- SCRUB_ARMED 期间 value bubble 已弹出但**尚未跨过一个 step**——值没有改变
- 编辑态按键：`Enter` 提交 / `Esc` 回滚 / `Blur` 合法则提交否则回滚 / `Tab` 提交并跳下一字段

理由：手抖的特征是「快且短」的位移（4–8px），movement-based 宽限期比 time-based 延迟
（80ms 等）更精准且零副作用；不动了再回退判定的完整版引入不确定性，不采纳。

**D1-B · 数值非法反馈**（裁定 #1-B 部分）：
非法文本（空 / 非数字 / NaN）提交时回滚原值不写 0（D1 既有）+ 输入框红闪 200ms +
tooltip「非法输入，已恢复原值」。超出范围（height 越界）钳制到合法 max/min 并 tooltip
「已钳制到 X」。

### 9.3 反馈形态（并入 D3 stale / D8 读数 / 新视觉态）

**D3-E · stale 整笔拒收反馈**（裁定 #8）：
松手时 `before != 快照` → 整笔拒收；画面红闪 200ms（`stroke_cells` 区域 `ERROR` 色描边）+
toast「整笔已拒收：N 格快照不一致，已放弃本次笔触」，3s 自动消失，带「详情」展开看哪几格
的 `before` 被外部改动。**不用模态弹窗**——拒收是可恢复的编辑拒绝，不是错误；用户重画即可。

**D8-A · Undo/Redo 后 250ms 白闪指示**（裁定 #4）：
撤销 / 重做后对受影响格做 **250ms 白色脉冲**（纯视觉标记，不动值）。目的：两笔相近色
撤销时画面差异肉眼不可辨，用户按 Ctrl+Z 后不知发生了什么；白闪 = 「这几格刚刚变了」的
定位信号。与 D2 hover / selected 脉动**不冲突**：白闪是 z 顶层短促标记，250ms 后消失；
其下 hover / selected 视觉不变。撤销后用户继续 hover 该格仍可读新值。

**D8-B · 底栏 stroke 进度瞬态槽**（裁定 #7）：
stroke 期间底栏常驻「⏳ 笔触进行中 · 已落 N 格」（N 实时累加，pick_at + Bresenham 补间）；
release / Esc 后消失。**与 D8 读数条分工**：读数条是持久信息（hover/selected 读数），
底栏是瞬态操作反馈，不与 D8 固定双段冲突。

### 9.4 异常态与门控（新章节）

**Gate-1 · 时间轴播放中的工具门控**（裁定 #5）：
`playback.playing = true` 时：
- CellMap 工具瓷砖（EditCell / FloodSelect / Brush / BrushErase）**灰化不可点**
- 面板读数条 / 落笔值区 / 单格 / 批量 / 派生运算**保持可见**（只读态）
- Tooltip「时间轴播放中，暂停后可编辑」
- 暂停后工具自动恢复可用

理由：禁工具而非禁面板——播放中查看地形数据（读数条）是定位时间点的合法只读操作，
禁整个面板会把只读查看也阻掉。禁工具瓷砖 = 只挡写入入口，读通道保持畅通。

**Gate-2 · 无图打开的空态**：
CellMap 顶栏段灰化不可点；Tooltip「请先打开一张地图」。MapTree 正常可用。
空态画面：「🗺️ 请先选择一张地图 / 在左侧地图树中选择或新建地图后即可进入地形编辑工作台」。

### 9.5 视觉分工四态总表（并入 D2 / D3 / D10，补 Bu 漏列的 region 态）

| 属性 | hover (D2) | selected | stroke (D3) | region |
|---|---|---|---|---|
| 色值 | 白内 `0xFFFFFF` + 深墨外 `0x382F27` | 金 `ACCENT_SELECT` | 红橙 `STROKE_PENDING` | 半透金底（同 `ACCENT_SELECT`） |
| 线宽 / 透明度 | 内 1px / 外晕 +2px | 2px + 半透底 alpha 0.10–0.34 | 填充 alpha 0.26，无描边 | 填充 alpha 0.10–0.34 |
| 脉动 | ❌ | ✅ | ❌ | ✅ |
| 准星 | ✅ | ❌ | ❌ | ❌ |
| 触发 | `hover.is_some()` | `selected.is_some()` | `stroke_cells` 非空 | `region_cells` 非空 |
| 消失 | 光标离开格 / 关工具 | 切图 / Esc 全量 clear | release 提交 / Esc 丢弃 / 切工具 | **起笔清空（D3-A）** / 切工具保留（S0-1）/ 切图 / Esc |

**绘制顺序**（每帧自下而上）：`region` → `stroke` → `selected` → `hover`。
**stroke 覆盖 selected**（stroke 期间 selected 视觉不渲染）；hover 永远画在最上层。

### 9.6 令牌补充

| 令牌 | 加在哪里 | 用途 | 色值 |
|---|---|---|---|
| `STROKE_PENDING` | `wv-theme/src/colors.rs` | stroke 填充色（与 `ACCENT_SELECT` 色相角差 ≥ 30°） | `0xD9634E` |
| `UNDO_FLASH` | `wv-theme/src/colors.rs` | D8-A 白闪标记色 | `0xFFFFFF` |

`ACCENT_SELECT` / `CORE_TERRAN` / `LINE_ON_DARK` 已有，不新增。

### 9.7 落地到实施分期（对 §5 的增补）

| 分期 | 增补内容 |
|---|---|
| **M1** | D1-A SCRUB_ARMED + D1-B 非法反馈 |
| **M2** | D3-A/B/C/D 起笔时序 + 右键 + Esc 阈值 + 边界行为 + D3-E stale 反馈 + D6-A 泛洪交互形态 + D8-A 白闪 + D8-B 底栏进度 |
| **M3** | Gate-1 播放门控 + Gate-2 无图空态 |

### 9.8 测试门槛补充（AGENTS §7.1）

新增测试约 12 条：

- D3-A 起笔清空 region_cells（2：起笔清、切工具保留）
- D3-B 右键 = Esc 等价（1）
- D3-C Esc ≤50 格静默 / >50 格确认（2）
- D3-D 拉出画布 stroke 存活（1）
- D1-A SCRUB_ARMED 4–8px 静默回滚 / >8px 进 stepping（3）
- D1-B 非法回滚不写 0 + 钳制 tooltip（1）
- D3-E stale 拒收反馈触发（1）
- D8-A 撤销白闪标记（1）

### 9.9 与既有条款的关系自检

- **未触及**：D1 4px 阈值（D1-A 只加起手宽限期，不改判定阈值本身）；D3「松手才提交」
  （D3-A/B/C/D 是手势时序补强，提交语义不变）；D8 固定双段（D8-B 底栏是独立槽位）；
  D10 群系可见底图（四态总表是渲染叠序，不改色源）。
- **已扩展**：D3 起笔 / 丢弃 / stale 反馈细节；D6 泛洪交互形态；D1 手势时序。
- **新增章节**：§9.4 异常态与门控（Gate-1 / Gate-2）。
- **新视觉态**：D8-A 白闪（`UNDO_FLASH`），与 D2 hover 脉动 / selected 脉动共存三层视觉，
  绘制序中白闪 z 最顶层 250ms 短促。

---

**修订记录标注**：本节 2026-10-24 追写，D1–D10 既有条款文字零改动。
转「已接受」的条件（§8）不变——用户复述「同意 ADR-0052，按此实施」即转状态。
