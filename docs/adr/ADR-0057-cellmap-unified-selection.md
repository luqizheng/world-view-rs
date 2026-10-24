# ADR-0057：CellMap 统一选区——从「工具即模式」到「选区优先（先名词后动词）」

**状态**：已接受（Accepted）
**日期**：2026-10-25（同日评审修订：位图容量数字改正；D3 锁死 EditCell 只画不选、`allow_drag` 退役；新增 §6 验收标准 A1–A9；D8 点名 hover 与蚂蚁线的四态分工）
**决策者**：用户（对话裁定：范围 = **B 两步走**；拖绘笔与选区关系 = **裁剪到选区**；
2026-10-25 追加裁定：选区视觉 = **白色虚线蚂蚁线（沿方格边贴线，不做曲线化等几何优化；动画 = 两相位互换）**；
拖绘笔**半径可调**——修正 ADR-0052 D5「硬边 1 格、不引入半径与 falloff」的前半句，半径引入、falloff 仍不引入）
**关联**：[ADR-0040](ADR-0040-cell-inspector-tool.md)（补 B「Single/Batch 两态互斥」**被本 ADR 推翻**，正文不改）、[ADR-0052](ADR-0052-cellmap-terrain-workbench.md)（连续绘制 / 泛洪 / 派生算子——本 ADR **在其上重构选区层**；其 D5「硬边 1 格」**由本 ADR D9 修正**，正文不改）、[ADR-0043](ADR-0043-map-dock-split-buttons.md)（矩形/椭圆框选手势——本 ADR **复用**，不重造）、[ADR-0033](ADR-0033-selection-retained-after-intent.md)（选区保留口径沿用）、[ADR-0054](ADR-0054-world-cell-map-crate.md)（选区态落 wv-world-cell-map，红线不染）、[ADR-0025](ADR-0025-selection-first-tooling.md)（选择优先工具——本 ADR 是其精神在 CellMap 的彻底化）
**原则**：P6 / P7（选区为会话态，不进撤销栈、不落盘——存储层纪律不覆盖会话选择）

> **配套说明**：本 ADR 只锁「CellMap 工作区的选区模型与调整链路」。
> **不**触碰三通道数据形态、**不**触碰 YAML / `terrain.bin` 字节布局、
> **不**触碰渲染技法。所有世界写入仍经 `EditCommand → History`（AGENTS §3.3）。

---

## 1. 处境（Context）

### 1.1 用户裁定

> 「重新设计 cell map edit 的逻辑：1. 选择区域；2. 调整选择区域的属性、群系等属性。」

判据由此收敛为一条：

> **作者的心智模型是「先圈定一片地，再调这片地的属性」——工具链必须让「选区」成为一等对象，而不是某个工具的副产品。**

### 1.2 现状痛点（实测代码，非推测）

| # | 痛点 | 证据 | 后果 |
|---|---|---|---|
| **Q1** | 选区双轨制：`selected: Option<(i32,i32)>`（1 格）与 `region_cells: Vec<(i32,i32)>`（N 格）是两个独立字段 | `wv-world-cell-map/src/state.rs` `EditCellState` | 面板被迫做 `CellPanelMode` Single/Batch 互斥 + `BatchModeAutoLand` 边沿触发（`wv-app/src/ui/inspector/cell.rs`）；「以为在改整片、实际只改了一格」靠 UI 层互斥硬挡 |
| **Q2** | 选择能力散在 4 个互斥工具：EditCell 点选 / CellSelect 单选 / V 套索 / W 泛洪 | `MapToolId` 各分支 | 工具数 ≠ 能力数——四个工具产出的是同一样东西（一批格子），切来切去只为换一种「圈法」 |
| **Q3** | 选区不可组合：无加选 / 减选 | 全仓无 Shift/Alt 选区修饰 | 泛洪选完不能用套索修边；「这片连通区 + 旁边那块」做不到 |
| **Q4** | 无框选快捷通道进 CellMap 选区模型 | 矩形框选（ADR-0043）成形后写入 `SelectionState.closed`，与 `region_cells` 链路间接 | 最廉价的批量手段反而绕路 |
| **Q5** | 拖绘与选区零交互 | `cell_stroke.rs` 不读 `region_cells` | 「只在圈内涂」做不到——蒙版语义缺失 |

### 1.3 对标裁定

经对话比较并裁定：**主体范式借 Photoshop 选区体系**（统一选区 + Shift/Alt 加减 +
选区运算 + 绘画被裁剪），**按属性选择借 QGIS**（select by expression，第二批）。
**明确不借 Figma**——矢量对象选中模型套不到栅格格子上；面板视觉继续沿用 Figma 设计稿，交互范式不借。

---

## 2. 决策

### D1 统一选区 `CellSelection`：一格集合，单格 = 1 格选区

`EditCellState.selected` 与 `region_cells` **合并退役**，代之以为一等的会话对象：

- 逻辑形态：当前图上的**格子集合**（可空；空 = 无选区）；
- 物理形态：`Vec<u64>` 位图，按图幅格数定容（默认 `cell_size=8`、图幅 4096 wu → 512² 格 ≈ 32 KB；即便 4096² 格也仅 2 MB；手摆位运算，**零新依赖**，守 §9）＋
  缓存的稠密索引迭代；稀疏场景提供 `iter_cells()`；
- 归属：`wv-world-cell-map` 会话 Resource（ADR-0054 分层不染；**不持世界数据、不写世界数据**）；
- 生命周期：Esc 清空、切图清空（沿用 `on_map_change` 口径）、**切工具保留**（ADR-0033 / ADR-0052 S0-1 口径的彻底化——选区与工具彻底解绑，该口径由「特意保留」变为「天然成立」）。

### D2 选区加减法：Shift = 并集，Alt = 差集

所有选择工具（点选 / 框选 / 套索 / 泛洪）统一响应修饰键：

| 修饰 | 语义 | 集合运算 |
|---|---|---|
| 无 | 替换选区 | `sel = new` |
| Shift | 加选 | `sel ∪= new` |
| Alt | 减选 | `sel −= new` |

泛洪选完用套索修边、点选补漏格，选区第一次**可组合**。

### D3 框选工具 R 进 CellMap 选区

矩形框选（复用 ADR-0043 手势与 `rasterize_polygon_cells` 栅格化，不重造）成形即写 `CellSelection`。
点选 S 仍保留（= 替换为 1 格选区；`CellSelect` 单选工具随之退役——它的能力被「点选 + 无修饰」完全覆盖）。
**职责分野就此锁死：选区一律由选择工具（S/R/V/W）产出，EditCell 只画不选**——
EditCell 单击不再写选区；`allow_drag` 字段随 CellSelect 一并退役
（它存在的唯一理由是区分 EditCell / CellSelect 两形态，两态既消，字段即死代码）。

### D4 拖绘笔裁剪到选区（蒙版语义）

EditCell 拖绘（ADR-0052 D3 的连续笔触）新增单点判据：

- `CellSelection` 非空 → 笔触格 **∩ 选区** 后才进入 `CellStrokeState`；
- 空 → 现状自由涂。

Esc 取消选区即恢复自由涂。撤销粒度不变（1 笔 = 1 条 `BatchEditCellsCommand`）。

### D5 选区不进撤销栈、不落盘

选区是**会话态**（同 Photoshop 口径）：撤销 / 重做只覆盖世界数据变更
（`EditCommand` 链），不恢复选区本身。P6/P7 约束的是存储层，不覆盖会话选择。

### D6 `CellPanelMode` / `BatchModeAutoLand` 退役

ADR-0040 补 B 的「Single/Batch 两态互斥」**废止**——单格不再是独立模式，只是 1 格选区；
「圈完自动落批量」边沿触发同步删除。面板逻辑简化为：**选区非空即显示读数条 +
调整表单**（读数条口径沿用 ADR-0052 D8：N 格 / 平均海拔 / 最高最低 / 群系分布）。

### D7 第二步登记（本 ADR 不实施）

用户裁定范围 = **B 两步走**。以下能力登记为第二批方向，本 ADR 只预留接口位（`CellSelection` 的集合运算 API），**不在本批实现**：

- 按属性选择（QGIS 式：群系 = X ∧ 海拔 ∈ [a,b] ∧ 水陆）；
- 选区运算菜单：反选 / 扩展 +1 / 收缩 −1。

### D8 选区视觉：白色虚线蚂蚁线（两相位互换）

用户 2026-10-25 裁定：选中方格在地图上用**白色虚线围绕外围**；边界**沿方格边逐段贴线**
（轴对齐阶梯线），**不做曲线化 / 圆角等几何优化**；动画 = 两相位互换
（原白下一步透明、原透明下一步变白）。工程化收窄两条：

1. **只画外围边界边**：选中格与未选中格（或图幅外）相邻的边才入 mesh；内部共享边不画
   （否则虚线叠虚线，大选区糊成一片）。Alt 减选挖出的空洞，其内边界由同一邻接判定天然覆盖。
   边界边本身就是轴对齐格边——「阶梯线」是天然形态，无需任何平滑处理。
   边界提取 O(选区格数)，邻接查位图 O(1)。
2. **两相位 mesh 互换**：同一组边界边按固定节距切成实/空段，建**两套 mesh（相位 A/B）**，
   计时器 ≈0.3 s 整组切换可见性——每帧零重建，只有选区变更 / 相机越档才重构。

几何纪律不变：世界空间 Mesh2D（**禁 Gizmos**，项目硬约束）；线宽与虚线节距按相机缩放
反向补偿，屏幕像素恒定（沿用 `THICKNESS_PX` 先例）。淡填充保留（读「面」），
蚂蚁线读「形」；ADR-0052 的外包矩形四边由蚂蚁线取代。
hover 准星（`CellHoverEntities`）**不动**：hover 是瞬时态、蚂蚁线是已提交态——
ADR-0052 §9.5 四态视觉分工在新模型下原样成立，实施者不得误删 hover 链路。

### D9 拖绘笔半径可调（修正 ADR-0052 D5 前半句）

用户 2026-10-25 裁定：地形涂抹的笔杆**大小可调**。

- footprint = 以笔触格为圆心、**格心距 ≤ R** 的圆盘格集合；**硬边、无 falloff**
  （格子三通道是离散值，软边没有语义落点——ADR-0052 D5 的后半句维持）；
- R ∈ 1–8 格，面板滑条（沿用 scrub 控件；R=1 时退化为现状单格笔触，行为零变化）；
- 与 D4 合取：落笔格 = footprint ∩ `CellSelection`（选区空 = 自由涂）；
- 撤销粒度不变：1 笔（按下-拖动-松手）= 1 条 `BatchEditCellsCommand`；
  Bresenham 补间不变（补间的是圆盘中心的轨迹）。

---

## 3. 理由

1. **名词→动词是栅格编辑器的收敛范式**：Photoshop（位图）与 QGIS（栅格/矢量混合）
   都收敛到「选区一等对象」；双轨制（Q1）与工具碎片化（Q2）是「选区不当一等对象」的直接症状。
2. **复杂度是净删除不是净新增**：`CellPanelMode` 互斥、`BatchModeAutoLand` 边沿触发、
   `selected`/`region_cells` 双字段、CellSelect 工具——四处复杂度换一个格集合对象。
3. **分层零风险**：选区态落 `wv-world-cell-map`（会话 Resource），写入仍走 wv-app
   `EditCommand → History`——ADR-0054 红线与 AGENTS §3.3 天然满足。
4. **两步走控风险**：第一步就收掉双轨制（最大痛点 Q1）与组合性（Q3），
   按属性选择（新 UI 面最大）留第二批，每步都可独立验收、独立回退。

## 4. 代价

| 代价 | 说明 |
|---|---|
| `EditCellState` 重构 | `selected` / `region_cells` / `region_fingerprint` 字段退役；下游消费点（高亮渲染 `cell_selection.rs` / 批量表单 / 拖绘裁剪）全量改读 `CellSelection` |
| 面板重写 | `cell.rs` 表单砍掉两态分支；brush_panel 的 `cell_seg_active` 判据改「选区非空 ∨ EditCell 激活」 |
| 单测改写 | `state.rs` 的 S0-1 / 单选模式测试按新语义重写（`close_tool` 保留选区 → 天然成立，测试改钉「位图运算正确性」与「切图清空」） |
| 位图内存 | 默认 512² 格 ≈ 32 KB/图会话内存（4096² 格亦仅 2 MB），可忽略；多图并存时按当前图单份持有 |
| 蚂蚁线渲染 | 边界边提取 O(选区) + 双相位两套 mesh；选区变更 / 相机越档才重建，每帧零成本 |
| 笔刷半径 | 圆盘格枚举 R≤8 → 单点 ≤~200 格；footprint ∩ 选区位图按位与，O(footprint) |
| 学习成本 | 作者需知 Shift/Alt 修饰——读数条旁常驻一行提示兜底 |

## 5. 被否决的备选

| 备选 | 否决理由 |
|---|---|
| **A. 一步完整模型**（含按属性选择、选区运算菜单） | 用户裁定两步走；按属性选择是新 UI 面（查询构建器），与选区层重构捆绑会放大单次评审面 |
| **C. 只合数据、不动交互** | 只消 Q1，Q3/Q4/Q5 全留——选区仍不可组合、无框选直达、无蒙版；痛点消一半 |
| 拖绘与选区互不影响 | 用户裁定裁剪；蒙版语义是 Photoshop 收敛解，且「只在圈内涂」是地形编辑高频诉求 |
| 拖绘裁剪加面板开关（作者自选） | 多一个持久 UI 状态换边际便利；Esc 取消选区已是逃逸口，开关为冗余 |
| Figma 式对象选中模型 | 矢量对象范式不适用于栅格；格无「对象句柄」，多选框模型无法表达「半片群系连通区」 |
| 选区入撤销栈 / 落盘 | 选区非世界数据；入栈会让 P6 覆盖到会话态，撤销历史被「圈选动作」淹没 |
| 保留 CellSelect 工具 | 能力被「点选 + 无修饰」完全覆盖，保留即留互斥工具碎片（Q2 复发） |
| 蚂蚁线逐格四边全画 | 内部共享边虚线叠虚线，大选区糊成噪点——只画外围边界边（含空洞内边界） |
| 边界曲线化 / 圆角平滑 | 用户裁定不优化——格边阶梯线即最终形态；曲线化引入链装配 / 切线计算，零收益 |
| 蚂蚁线连续滚动（shader / 纹理偏移） | 两相位互换已逐字节命中裁定语义；连续滚动是每帧 / 纹理级复杂度，无收益 |
| Gizmos 画蚂蚁线 | 项目硬约束禁 Gizmos（缩放变粗 / 锯齿）；一律世界空间 Mesh2D |
| 软边 falloff 笔刷 | 格子三通道是离散值，软边无语义落点——维持硬边（ADR-0052 D5 后半句不动） |

---

## 6. 验收标准

第一批（D1–D6、D8、D9）合入前，以下逐条可检验；测试命名与断言上下文按 AGENTS §7.3 / §7.4。

| # | 验收项 | 锚点 |
|---|---|---|
| A1 | 位图运算：`replace` / `union` / `subtract` / `is_empty` / `iter_cells` 对随机格集与 `Vec` 参照实现逐格一致 | `wv-world-cell-map` `CellSelection` 单测 |
| A2 | 边界边提取：L 形选区只出外围边；Alt 减出的空洞内边界同样出边；内部共享边零产出 | 蚂蚁线边界提取单测 |
| A3 | 双相位：相位 A/B 严格互补（A 实段 = B 空段）；计时器到点整组翻转；选区不变时不重建 mesh | 渲染同步单测 |
| A4 | Shift/Alt 修饰语义四工具一致（点选 S / 框选 R / 套索 V / 泛洪 W）：无修饰 = 替换、Shift = 并、Alt = 差 | 命令分发单测 |
| A5 | 拖绘裁剪：选区非空时落笔格 = footprint ∩ 选区；选区空 = 自由涂（现状）；R=1 时与现状逐格一致 | `cell_stroke` 单测 |
| A6 | 撤销粒度：1 笔 / 1 次批量应用 = 1 条 `BatchEditCellsCommand`（ADR-0052 D3 口径不动） | 现有测试保绿 |
| A7 | 生命周期：切工具保留选区；Esc 清空；切图清空（ADR-0033 / S0-1 口径在 `CellSelection` 上重写） | `state.rs` 重写测试 |
| A8 | EditCell 只画不选：单击不改选区；`allow_drag` / `CellSelect` / `CellPanelMode` / `BatchModeAutoLand` 全仓零残留引用 | 全仓 grep |
| A9 | 闸门：`cargo test --workspace` / `app-test` / `app-clippy` / `fmt --check` / `smoke-gui` 全绿 | AGENTS §7.1 |

---

## 7. 影响面（实施落点预告）

| 落点 | 动作 |
|---|---|
| `wv-world-cell-map/src/state.rs` | 新增 `CellSelection`（位图 + 集合运算 + `iter_cells()`）；`EditCellState` 删 `selected`/`region_cells`/`region_fingerprint`/`allow_drag`，留 `tool_active`/`hover`/`last_map_id` |
| `wv-world-cell-map/src/cell_selection.rs` | 高亮渲染改消费 `CellSelection`：淡填充（面）+ **外围边界蚂蚁线**（双相位 mesh，计时器切换；D8），外包矩形四边退役 |
| `wv-app/src/edit/cell_stroke.rs` | 拖绘入口加「footprint ∩ 选区」判据（D4）；footprint 圆盘枚举（D9） |
| `wv-app/src/ui/inspector/cell.rs` | `CellPanelMode` / `BatchModeAutoLand` 退役；表单两态分支砍半；**加「笔刷半径」滑条**（D9） |
| `wv-app/src/ui/brush_panel.rs` | `cell_seg_active` 判据改「选区非空 ∨ 工具激活」 |
| 命令分发（`ui_shell.rs`） | `CellSelect` 分支退役；新增框选 R 工具映射；Shift/Alt 修饰键注入选择落笔点 |
| GLOSSARY | 登记「统一选区（CellSelection）」「选区加减法」「蒙版裁剪（拖绘）」「蚂蚁线（marching ants，两相位）」 |

> 实施纪律：动工按 AGENTS §13「先分支，再 worktree，后写码」；
> 闸门按 §7.1 五条（含 `app-test` / `app-clippy` / `smoke-gui`）。

---

## 补记（2026-10-25 · 实施前方案评审裁定）

ADR 接受后、动工前的方案评审对照实测代码逐条核实，产出六项裁定，作为本 ADR 的
实施口径追加于此；正文不改（AGENTS §6.4 只增不改）。评审发现的「正文笔误 /
漏列 / 与既有 ADR 冲突」以本补记为准。

### 补记 1（F1）：废止 ADR-0052 D3-A「起笔清 region_cells」——与 D4 蒙版物理冲突

ADR-0052 D3-A 规定连续笔触起笔时清空 `region_cells`（实测落点：
`wv-app/src/edit/cell_stroke.rs` L59-62 `clear_region_for_stroke`、L183 调用点、
L386 测试钉死该行为）。这与本 ADR D4「拖绘裁剪到选区」**物理冲突**——起笔即清
选区，蒙版永远为空，「只在圈内涂」不可能成立。

**裁定**：ADR-0052 D3-A 自本 ADR 合入起**废止**（ADR-0052 正文不改）；
ADR-0052 §9.5「选区与笔触不可同框」的口径随之**翻转为必同框**（选区非空时笔触
被裁剪到选区内）。实施动作：`clear_region_for_stroke` 及其调用点删除；
L386 钉死旧行为的测试**删除并反转**为钉「起笔不清选区」。

### 补记 2（F2）：§7 影响面落点表更正

§7 落点表漏列以下消费点，实施时一并纳入（均有实测代码锚点）：

| 漏列落点 | 锚点 | 动作 |
|---|---|---|
| `wv-render/src/cellmap_bridge.rs` `sync_cell_region` | L800 | 填充 + 外包 4 边 = **蚂蚁线真正落点**（改消费 `CellSelection`） |
| `wv-render/src/cellmap_bridge.rs` `sync_cell_selection` | L2038 | 单格 outline → **退役**，由 1 格蚂蚁线取代 |
| `wv-app` 派生算子 `derived_op.rs` | — | 平滑 / 量化作用域改读 `CellSelection` |
| `wv-app/src/ui/cell_paint_panel.rs` | L373 | 读数 / 应用入口改读 `CellSelection` |
| `wv-app/src/ui/inspector/mod.rs` | L208-242 | `BatchModeAutoLand` 边沿触发拆除 |
| `wv-app/src/ui/toolbar.rs` | L76-173 | CellSelect 瓷砖退役 |
| `wv-app/src/ui/ui_command_palette.rs` | L205-216 | 命令面板项清理 |
| i18n | `keys.rs` L774/884/1576、`table.rs` 三处 | CellSelect 文案清理 |
| `wv-app/src/ui_shell/tests.rs` | L560-632 | 工具分发测试改写 |

### 补记 3（F3）：Esc 清选区 = 新增入口（现状无实现）

正文 D1 / A7 写「Esc 清空」时**误以现状已有该入口**；实测全仓 grep `Escape`
仅 4 处且均与选区清理无关——Esc 清选区是**全新增**的输入入口，非沿用。
实施优先级：低于「进行中手势的取消」（拖绘 / 套索进行中的 Esc 优先取消手势），
作为独立小项交付。

### 补记 4（F4）：D9 圆盘口径钉死为「格心距 < R」

正文 D9 写「格心距 ≤ R」，与「R=1 时退化为现状单格笔触」自相矛盾
（≤1 会含正交邻格 = 十字 5 格）。**裁定**：footprint = 以笔触格为圆心、
**格心距严格小于 R** 的圆盘格集合——R=1 → 仅圆心 1 格（退化为现状，行为零变化）；
R=2 → 含对角共 9 格。正文「≤」视为笔误，以本补记为准。

### 补记 5（F5）：套索锚点活链接 = B 方案（基座快照 + 活链接）

用户裁定 V 套索采用 **B 方案**：

- 套索**闭合那刻**快照基座选区与修饰键 op（无修饰时基座 = 空选区）；
- 拖动锚点实时调形期间，每帧 `CellSelection` = 基座 ∪/− 当前多边形格集；
- 任何新选择动作 / 取消 / 切工具**断开活链接**，并把最后结果钉进 `CellSelection`；
- 承载点 = `sync_region_cells` 改造（保留拖锚实时调格能力），指纹缓存语义随之迁移。

### 补记 6（F6）：点选 S 的载体 = `CellSelect` 改名 `PointSelect` + 独立激活态

正文 D3「点选 S 仍保留、CellSelect 退役」的实施口径钉死为：

- `MapToolId::CellSelect` **改名** `PointSelect`——A8「全仓零残留」覆盖枚举
  变体 / 命令面板 / i18n / 工具条瓷砖 / 测试全部改名点；
- 激活态独立为小 Resource（仿 `FloodSelectState` 形态），**不再与 EditCell
  共用 `tool_active`**——D3 职责分野在状态层的落实；
- `commit_edit_cell_on_click` 改造为写 `CellSelection`（带修饰键运算）；
- hover 准星 gate 扩为「EditCell ∨ PointSelect」。
