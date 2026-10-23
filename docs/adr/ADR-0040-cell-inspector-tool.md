# ADR-0040：`ToolKind::CellInspector` 工具——单格拾取 + 三通道编辑

**状态**：已接受（Accepted）。**2026-10-22 追记：决策 6（Inspector Panel 加 Cell 折叠区）被 [ADR-0050](ADR-0050-unified-inspector-panel.md) 修正**——实现一度做成独立浮窗 `Panel::CellEditor`，ADR-0050 把 Cell 表单改回 Inspector 属性页签并退役该浮窗。正文按只增不改保留原文。
**日期**：2026-10-19
**决策者**：用户（对话回复「好」——即 ADR §7 Q1-Q6 全默认通过）
**关联**：[ADR-0039](ADR-0039-topographic-viewmode.md)（Topographic viewmode，本 ADR 与之平行）、[ADR-0025](ADR-0025-selection-first-tooling.md)（「选区优先」工具哲学）、[ADR-0032](ADR-0032-place-vocab-unify.md)（BiomeFamily 族键）、[ADR-0029](ADR-0029-biome-marker.md)（biome 识别 marker，[biome_marker.rs](../../crates/wv-terrain/src/marker.rs)）、[ADR-0033](ADR-0033-selection-retained-after-intent.md)（意图后保留选区精神）、[MAP_LAYER_RENDERING_SPEC §0](MAP_LAYER_RENDERING_SPEC_v0.1.md)（四轴契约）、[ART_STYLE_SPEC §1.4](ART_STYLE_SPEC_v0.1.md)（色觉安全「色不是唯一载体」）

> **配套说明**：本 ADR 锁 CellInspector 工具——单格拾取 + 改 cell 三通道（surface / height / biome）。独立 commit，独立 review。**不**触碰 [ADR-0039](ADR-0039-topographic-viewmode.md)（Topographic viewmode）也不触碰计划中的 **ADR-0041**（2.5D vertex Z 挤压）——三件并行落地，分别 PR。

---

## 1. 处境（Context）

用户在 [ADR-0039 §1](ADR-0039-topographic-viewmode.md) 同轮对话里加了第二个诉求：

> 「我需要给 cell 的地图——用户可以单独选择这一个 cell，调整它的属性」

### 1.1 现状能力盘点（基于 [wv-edit/src/command.rs](../../crates/wv-edit/src/command.rs)、[wv-app/src/state.rs](../../crates/wv-app/src/state.rs)、[wv-render/src/recipe.rs](../../crates/wv-render/src/recipe.rs) 的实际阅读）

| 层 | 已有 | 缺什么 |
|---|---|---|
| `wv-edit` 的 EditCommand | `PaintTerrainCommand`（批量沿 stroke 改格）、`IntentTerrainCommand`（意图生成） | **没有"单格三通道"编辑命令**；最近一条 path 是把 stroke 退化为「半径=1」绕道走，撤销栈看到一个「笔刷记录」会让作者误以为是批量操作 |
| `wv-app` 工具层 | `BrushTool` 8 个（涂塑放三组，均批量）、`pan_tool`（左键平移）、`label_pick::hit_test`（命中 POI label） | **没有"点一格即选中"的工具**；命中目前只对 label |
| `wv-render` 渲染 | `PassId` 23 个 + `BrushCursor`（Z 0.9，仅笔刷光标） | **没有 cell-level 选中高亮的 PassId** |

**结论**：「选一格改一格」是 **三 crate 协同的真实新增**，不是改动一行业务逻辑。

### 1.2 与现有架构边界的关系

- **P4 多地图层级**：cell 隶属于具体 `MapNode`（[wv-core/src/map/mod.rs](../../crates/wv-core/src/map/mod.rs)）；cell-level 编辑命令的 `map: Id` 参数沿用 `PaintTerrainCommand` 同款，无 P4 边界问题。
- **P6 非破坏编辑**：编辑是改当前值，但 [wv-edit/src/command.rs:138-188](../../crates/wv-edit/src/command.rs) 的「snapshot → diff → revert」四步模式天然支持撤销/重做，每格 1 command 即可。
- **V2 历史可重写**：作者可随意改 cell 三字段，无需任何提示——本 ADR 不引入校验闸。
- **biome 字段不冗余**：[ADR-0039 §1.3](ADR-0039-topographic-viewmode.md) 已锁 biome 不动；本 ADR 对 biome 的编辑是「读写」而非「删字段」。

### 1.3 「工具」与「模式」的边界

现有 `ToolKind` / `BrushTool` 体系：

- `BrushTool`（[wv-app/src/ui/brush_panel.rs](../../crates/wv-app/src/ui/brush_panel.rs)）：8 个，涂塑放三组，**批改地形**语义；
- `pan_tool`（[wv-render/src/map_controls / ViewState.pan_tool](../../crates/wv-render/src/state.rs)）：左键平移地图；
- 测量工具（`measurement`）：独立 PassId + UI Panel。

「选一格改一格」在概念上**与 Brush 不是同一类**——Brush 是「批改」，本工具是「点取 + 改一格」。它应该进 `ToolKind` 顶层（与 `pan_tool` 同级），不是 `BrushTool` 的子分支。这条决策的重要性在于：**把工具语义钉清楚**，未来若再有「点一条边改连接」「点一个图标改属性」这类需求，能复刻同款模式——「先 ToolKind 顶层，再各自做面板」。

---

## 2. 决策（Decision）

**全部动作汇总：**

1. **新增 `ToolKind::CellInspector`**（`crates/wv-app/src/state.rs`）：进 `ToolKind` 枚举顶层；工具 dock（[feat/map-tool-dock](MAP_LAYER_RENDERING_SPEC_v0.1.md)）+ 1 个图标（Lucide `square-mouse-pointer`，[AGENTS §8](AGENTS.md) 唯一图标库）；快捷键延后。

2. **新增 `EditCellCommand`**（`crates/wv-edit/src/command.rs`），签名：
    ```rust
    struct EditCellCommand {
        map: Id,
        gx: i32, gy: i32,
        before: TerrainCell,           // undo 用，纯快照
        after: TerrainCell,            // redo 用
    }
    ```
    沿用 [command.rs:138-188](../../crates/wv-edit/src/command.rs)「snapshot → diff → revert」四步模板；`apply` 把 `after` 写到目标格；`revert` 写回 `before`；`label()` 返「改一格 (gx, gy)」。

3. **cell-level hit-test**（`crates/wv-app/src/ui/cell_pick.rs`，**新文件**）：沿 `label_pick` 模板复用骨架——输入鼠标视图坐标（ViewState 转世界坐标），按当前 `ViewState.cell_size` 反查 `TerrainGrid.sample_world` → 命中返回 `CellPickResult { map: Id, gx: i32, gy: i32 }`；不命中 `None`。该函数**只在 `ToolKind::CellInspector` 激活时**跑（其他工具不浪费采样）。

4. **新增 `PassId::CellSelection`**（`crates/wv-render/src/recipe.rs::PassId`）：第 24 个 pass；`MODE_TABLE` 8 个 mode 全开（会话轴——会话带）；`LayerFrame` 不级联（与 `tracks_visible` 同款）；高亮 = 1 格 outline + 半透底色，**默认 alpha 0.2**；Z 与 `BrushCursor` 同 0.9（顶层会话带，[MAP_LAYER_RENDERING_SPEC §7](../../crates/wv-render/src/MAP_LAYER_RENDERING_SPEC_v0.1.md)）。

5. **`ViewState::cell_selection: Option<CellPickResult>`**：`Option<CellPickResult>`；`hover` 与 `selected` 两种：hover 是当前 hover 的格（即时刷新），`cell_selection` 是「点选 committed」的格（即时落）；ESC 清空（沿用 [measurement.rs](../../crates/wv-render/src/measurement.rs) 的清空键）。

6. **Inspector Panel 加 Cell 折叠区**（`crates/wv-app/src/ui/inspector_panel.rs`）：当 `ToolKind::CellInspector` 激活 + 命中一格时，展开「Cell」折叠区，三个字段：
   - **Surface** —— 二选一按钮（`SURFACE_WATER` / `SURFACE_LAND`），旁边 1 行小字提示「改了不影响湿判定（`is_water_cell` 仍按 `height<0 ∨ surface==水`）」
   - **Height** —— `i16` 输入框，单位米（[wv-terrain/src/lib.rs:62-67](../../crates/wv-terrain/src/lib.rs)）；范围 ±32767；超范围报红字
   - **Biome** —— 21 项下拉按 `BiomeFamily` 折叠分组（沿 [ADR-0032 D2](ADR-0032-place-vocab-unify.md) 族键：`Forest` / `Grassland` / `Desert` / `Wetland` / `Cold` / `Highland` / `Water` / `Road` + `NONE`）；每一族下列出该族所有编号

7. **i18n 新增 key 族 `cell_inspector.*`**：zh/en 各加若干（标题、字段名、单位、撤销/重做、ESC 清空提示，共 ~10 条）

8. **撤销粒度**：每个格一次 `commit` 一次 `EditCellCommand`（决策 2 的纯快照模式天然支持），「先打打改 30 次退 30 次」流程无需批合并

9. **数据层零变化**：`TerrainCell` 三字段不动；`terrain.bin` 字节布局（I16 + U8 + U8）零变化；`wv-io` 迁移表零变化。**符合 P3 与 P6**。

10. **API 表面约束**：`ToolKind::CellInspector` 在 `wv-app` 顶层（不导入 wv-edit 内部），跨 crate 仅以 `EditCommand` trait object 形式流通（沿用 `wv-edit::dispatch`）

---

## 3. 理由（Rationale）

- **架构归位正确**：`ToolKind` 顶层 + `PassId` 会话轴 + Inspector Panel Cell 折叠区，三个落点分别承接已有架构的「工具」「高亮」「属性」三档。复用 [ADR-0025](ADR-0025-selection-first-tooling.md) 的「选区优先」哲学（先 ToolKind 选区，再动作），与「双击钻入图标」[ADR-0038](ADR-0038-icon-click-drill-in.md) 同属「先选后动」模型，未来若再有「选连接」「选 marker」类需求，能沿同款模式复用。
- **biome 下拉按 `BiomeFamily` 分组**而不是 21 个平铺：直接继承 [ADR-0032 D2](ADR-0032-place-vocab-unify.md) 的族键，既不重复设计，又让作者用熟悉分组定位。
- **`PassId::CellSelection` 不进 LayerFrame**：与 [MAP_LAYER_RENDERING_SPEC §5](../../crates/wv-render/src/MAP_LAYER_RENDERING_SPEC_v0.1.md)「`tracks_visible` 经 LayerKind 缺位走会话带」同款处理；会话轴布尔不进声明轴。
- **数据层零变化**:本 ADR 只读写 cell 三通道（不改存储），符合 P6「带区间的状态片段」精神（cell 字段本来就有，作者每改一次就发 EditCommand，撤销栈自然累积）。
- **EditCellCommand 命名**：与既有的「Paint*」「Intent*」「Set*」「Add*」「Remove*」「Update*」「Regenerate*」「Divide*」「Move*」「Upsert*」「Divide*」同款（看 [command.rs 目录](../../crates/wv-edit/src/command.rs)）。`Edit*` 是新前缀，但语义清晰——「modify 既有格的三字段」无歧义；若未来嫌前缀冲突再议。
- **撤销粒度每格 1 command**：与 [ADR-0033](ADR-0033-selection-retained-after-intent.md) 「意图后保留选区」一致——作者一次 mental 操作对应一次撤销单位，无需批合并；这是 V2「历史可重写」的延伸。
- **hover 与 selected 分两态**：与现有 label_pick 的命中测试同款——hover 驱动状态条坐标，selected 触发 Inspector；两者不冲突。

---

## 4. 代价（Consequences）

### 4.1 负面后果（必须承认）

1. **撤销栈线性增长**：作者连续改 N 格产生 N 条 `EditCellCommand`，撤销 N 次才能全退。若同图改 100 格要按 100 次退。缓解：第 2 阶段可加「inspector 内 commit 合并」（同次打开期间 N 次提交合并为 1 条 command）。本 ADR 不做，作为 §4.2 v2 留口。
2. **本 ADR 不实现多格选区**：按住 Shift 选连续格 / 框选 N×N / 套索边 不做；本 ADR 仅单格点取。理由：多格选区是「批量」概念，与本 ADR「点取一格」诉求不等价；强行单 ADR 同时实现多格会扩 3× scope。
3. **`is_water_cell` 判定可能困惑**：作者把「陆 + height=10m」改成「陆 + height=-50m」，`is_water_cell`（[wv-terrain/src/edit.rs](../../crates/wv-terrain/src/edit.rs)）会判为湿，但 `surface` 仍 `SURFACE_LAND`。`display_kind`（[biome.rs:144-178](../../crates/wv-terrain/src/biome.rs)）已处理这种"数据不一致"——按湿判定优先给水槽位，但 UI 上需给一行提示（决策 6 已含）。
4. **i18n 工作量**：~10 条新 key，中英双写（[ADR-0018](ADR-0018-i18n.md) 流程）。
5. **`ToolKind::CellInspector` 与 `BrushTool` 的区分首次确立**：未来若有人误把 inspector 写成 `BrushTool` 子分支，需 review 时拦截（code style 注释在 [wv-app/src/state.rs](../../crates/wv-app/src/state.rs) 「`ToolKind` 顶层」段）。
6. **不联动 [ADR-0039](ADR-0039-topographic-viewmode.md) 的百分位缓存**：cell 编辑后 `ViewState::topographic_thresholds` 缓存需 `data_hash` 联动重算（决策 5 的「`data_hash` 联动」已涵盖）——这是 clean 联动，不是副作用。

### 4.2 后续可能（不构成本 ADR 决策）

- **多格选区 v2**：按住 Shift 选连续 / 矩形框选 / 套索边——若用户后续反馈，统一扩 `ViewState::cell_selection` 为 `Vec<CellPickResult>` + modifier key 处理；本 ADR 不预留数据结构（避免提前抽象）。
- **撤销批合并 v2**：Inspector 内打开期间所有 commit 合并为 1 条 command，需要在 Inspector Panel 加 `commit_session` 包装——待用户反馈量再开。
- **CellInspector 跨子图**：[wv-app/src/state.rs DrillState](../../crates/wv-app/src/state.rs) 当前子图也会带 TerrainGrid；本 ADR 默认只对当前图生效；如跨子图需求出现，开 ADR-0042+ 处理。
- **`CellInspector` 与 `Faction` map 的 cell 改写联动**：cell 改了可能影响势力多边形遮罩——但势力层在 [ADR-0027](ADR-0027-faction-layer-multi-compatible.md) 是 `Membership.target` 维度，不依赖 cell；本 ADR 不动 Faction。

### 4.3 被否决的备选方案

| # | 备选 | 否决理由 |
|---|---|---|
| A | **把 CellInspector 写进 `BrushTool` 子分支**（如 `BrushTool::Inspect`） | 语义错位：Brush 是「批改」，Inspector 是「点取 + 改一格」；混进 Brush 会让 `BrushState` 不得不支持 `radius=0` 退化形态污染既有笔刷族 |
| B | **作为 [ADR-0039](ADR-0039-topographic-viewmode.md) Topographic mode 自带能力**，仅在 Topographic 可用 | 「选 cell 改属性」诉求不是「看地形」的下游——是工具，不是 mode；与 [ADR-0025](ADR-0025-selection-first-tooling.md) 「工具与模式分离」哲学一致 |
| C | **Cell 作为一等公民独立 `LayerKind::Cell` + 全局 cell 选区 + 自定义 query API**（类似 Place） | 战略型（~800+ 行 + 多 ADR）；本 ADR 范围只想满足"点一格改一格"，不过度设计；后续 v2+ 可开 ADR-0043+ 走「Cell 二等公民」路径 |
| D | **复用现有 `PaintTerrainCommand` 退化 `stroke.radius=1`** | 撤销栈看到一条「笔刷（1 笔）」标签，与作者操作不符；语义糊；维护也累（paint 的 brush 退化形态会污染 BrushState 状态机） |
| E | **`PassId::CellSelection` 走 LayerFrame 进声明轴**（与 `terrain` 同款） | 「选中一格」是会话行为，不是作者声明；放 LayerFrame 会让作者能在 YAML 里声明「永远选中那格」，违背直觉 |

---

## 5. 涉及与不涉及（AGENTS §10 自检）

**触及**：

- §6.4（本 ADR 即留痕）
- §10「依赖方向」：`EditCellCommand` 在 wv-edit；`ToolKind::CellInspector` 在 wv-app；`PassId::CellSelection` 在 wv-render——三条均不跨分层；wv-core / wv-terrain 仍零 Bevy 依赖（[§2.2 验证方式](../../AGENTS.md)）

**未触及**：

- §1 P1–P7 全部：biome 字段保留、表面/高度/biome 三通道无改动、cell-level 编辑走 `EditCommand`/`History` 不绕过 wv-edit（[§3.3](../../AGENTS.md)）、世界工作副本仍由 wv-app 持有
- §2.1 依赖方向（三条均合规）
- §3.3 不直接改世界：本 ADR 的所有改 cell 路径都经 `EditCommand`，不绕过 wv-edit
- §7.5 V1：作者改 cell 不被校验拦；hint 提示（决策 6）不是校验闸
- §8 代码规范：本 ADR 新增的 `cell_pick.rs` 走 [AGENTS §8](../../AGENTS.md) 的命名与文档规范；新增 Lucide `square-mouse-pointer.svg` 烘焙 `stroke="#ffffff"`

**触发但不冲突：**

- 配 [ADR-0039](ADR-0039-topographic-viewmode.md)：本 ADR `cell_selection` 与 Topographic mode 可同时开，即「Topographic 模式下选 cell 看 + 改三通道」
- 配未来 ADR-0041：2.5D vertex Z 下高亮 1 格的 Z 偏移由 [ADR-0041](ADR-0041)（尚未起草）锁

---

## 6. 提议落点（仅 paper，不写码）

```
crates/wv-app/src/state.rs                       :: ToolKind 顶层 +1
crates/wv-app/src/ui/map_tool_dock.rs            :: 工具 dock +1 图标（Lucide square-mouse-pointer）
crates/wv-app/src/ui/cell_pick.rs                :: 新文件，hit-test 反查
crates/wv-app/src/ui/inspector_panel.rs          :: Cell 折叠区 +3 字段
crates/wv-app/src/ui_shell.rs                    :: 工具切换 dispatch +1 条
crates/wv-edit/src/command.rs                    :: EditCellCommand + impl EditCommand
crates/wv-edit/src/dispatch.rs（或同名）          :: 注册到 dispatched set
crates/wv-render/src/recipe.rs                   :: PassId::CellSelection +1；MODE_TABLE 列长 +1（断言自动守）
crates/wv-render/src/state.rs                    :: cell_selection 字段 + topographic_thresholds 数据 hash 联动
crates/wv-render/src/ui/cell_selection_render.rs :: 新文件，1 格 outline + 半透底
crates/wv-app/assets/ui/tool_icons/              :: 新增 square-mouse-pointer.svg
crates/wv-app/i18n/                              :: cell_inspector.* Key 族 ~10 条
crates/wv-ui/i18n（若 UI 库迁过去后）             :: 同上 key 移至 wv-ui 的 zh/en.toml

tests :: EditCellCommand round-trip +1; cell_pick 命中边界 +1
        Inspector 字段回写 +1; PassId::CellSelection enabled 全 mode +1
        撤销 N 步正确性 +1（集成）
估算 :: ~300 行 + ~5-6 测试；3 个 PR 内合理拆分（comm/edit/test-pipeline 等）
```

---

## 7. 待回答项（用户 review 时一并回我）

| 编号 | 问题 | 我的默认（已写入正文） |
|---|---|---|
| **Q1** | CellInspector 是否所有 mode 都可用 | **是**（工具与模式分离） |
| **Q2** | 改 surface（陆 → 水，height 仍 +100m）是否允许 | **是**；UI 加 1 行提示「湿判定按 height<0 ∨ surface 水」 |
| **Q3** | 改 biome（陆 → BIOME_RIVER）是否允许 | **是**；UI 加 1 行提示「落 BIOME_LAKE / RIVER / OCEAN 时湿判定才生效」 |
| **Q4** | 多格选区（Shift 连选 / 框选） | **本次不做**，v2 留口 |
| **Q5** | 撤销粒度 | **每格 1 command**（不复用 inspector 批合并） |
| **Q6** | CellInspector 与 Topographic 的关系 | **正交**——`ToolKind=CellInspector` 与 `ViewMode=Topographic` 独立组合；两者同时开时「Topographic 观感下选一格看 + 改」 |

---

## 8. 何时从「提议」转「已接受」

- 用户在对话中明确复述「同意 ADR-0040，按此实施」即转 §状态为「已接受」
- 转「已接受」后，本 ADR 正文**不再修改**；后续推翻需新开 ADR（[AGENTS §6.4](../../AGENTS.md)）
