# 统一 Inspector 面板设计（单面板 · 按选中对象分派属性）

**日期**：2026-10-22
**状态**：待评审（用户已口头拍板四项裁定，见 §2）
**关联**：[ADR-0040](../../adr/ADR-0040-cell-inspector-tool.md)（CellInspector 工具，**§决策 6 被本设计修正**）、[ADR-0025](../../adr/ADR-0025-selection-first-tooling.md)（选区优先）、[MAP_EDITOR_BASELINE_v1 §1.11](../../MAP_EDITOR_BASELINE_v1.md)（P-4 属性检查器「显示/编辑当前选中对象的全部属性」）、[ADR-0018](../../adr/ADR-0018-i18n.md)（i18n）、[ADR-0026](../../adr/ADR-0026-wv-ui-ui-widget-crate.md)（控件层）

---

## 1. 意图与「为什么」

用户诉求原话：

> 属性面板 inspect 应该是一个的，随着点击不同物体，就出现不同的选项。

现状是**面板按类型分裂**：`region_panel`（区域卡带改名/删除）、`faction_panel`（势力卡带名称/颜色/层/等级/驻地/归属边/几何）、`cell_editor_panel`（cell 三通道）、`inspector_panel`（只显示 `kind · id` 头 + 四条计数行）各占一个浮窗。作者要改一个对象的属性，得先知道「哪种对象在哪个面板里」——这是**按实现分类**而非**按任务组织**的界面，与基线 P-4 的原意（「右侧停靠 · 320px · 显示/编辑当前选中对象的全部属性」）也不一致。

**目标**：一个右栏单窗口，主体是「当前选中对象的属性」；点地图上不同对象，同一窗口里换出该类型的属性表单。

**为什么这是 Bevy 生态的标准做法**：`bevy_inspector_egui`（社区主流）即「左树 + 右属性区，属性区按选中实体类型切换」；`bevy_egui` 本身不管布局，惯例全在应用层。本项目世界数据在 YAML（Bevy 只是渲染投影），所以「Inspector 换内容」纯在 wv-app 的 egui 层，不碰分层红线。

---

## 2. 用户已拍板的四项裁定

| # | 裁定 | 状态 |
|---|---|---|
| D1 | **web 原型退役**：`~/Downloads/World-View UI Prototype/` 目录 + 残留 zip 已删（回收站，可恢复）。视觉基准改为 **Figma 为主** | ✅ 已执行 |
| D2 | **CellEditor 并入统一面板**：不再单开浮窗 | 本设计落地 |
| D3 | **外壳形状**：右栏单窗口，主体 = 当前选中对象的属性；顶部页签切「属性 / 势力层 / 事态」三种集合级操作；区域卡、势力卡、cell 三通道的内嵌编辑全部搬进 Inspector；人物/事件登记面板保持独立浮窗 | 本设计落地 |
| D4 | **属性页签覆盖范围**：只做**地图可点的四类 + Cell** = Place / Region / Faction / Event / Cell。人物与事态**不进** Inspector | 本设计落地 |
| D5 | **区域面板去留**：退化为「区域列表 + 新建按钮」，点行 = 选中（驱动 Inspector 显示区域属性） | 本设计落地 |
| D6 | **视觉稿来源**：先用现有参考（`.figma-ref/inspector_node_42_2.png` + 仓内 `wv-ui` 控件库）把结构与交互做对，视觉细节后补 | 本设计落地 |

---

## 3. 目标与非目标

### 目标

1. 一个 `Panel::Inspector` 窗口，承载属性 / 势力层 / 事态三个页签。
2. 属性页签按选中对象类型分派出**可编辑**属性表单：Place / Region / Faction / Event / Cell。
3. 各类型面板（region / faction / cell）里**内嵌的编辑卡**迁入 Inspector；列表面板本身留在原地当选择器。
4. 所有写入走 `wv-edit` 的 `EditCommand` / `History`（AGENTS §3.3、§2.1）。

### 非目标（明确不做）

- **人物 / 事态不进入属性页签**。人物无地图坐标（`Character` 模型只有 `state_spans`，无 x/y；渲染层零人物标签），地图上点不到；事态是集合型容器（`steps` 引用事件），属性表难定形。两者各自的登记面板维持现状。
- **不引入多选**。属性表单一次针对一个选中对象。
- **不改数据模型**。`Place` / `Region` / `Faction` / `Event` 的字段与序列化零变化；YAML round-trip 契约不变（P3）。
- **不做 Figma 新稿**。视觉后补（D6）。

---

## 4. 现状事实（实现者须知）

这些是读过代码后确认的**硬事实**，决定了下面的方案形状：

| 事实 | 出处 | 影响 |
|---|---|---|
| 选中态是 `WorldProjection.selection: Option<SelectedEntity>`，`SelectedEntity = {kind: EntityKind, id: Id}` | `wv-core/src/lib.rs:110`、`wv-render/src/state.rs:46` | 属性分派的唯一入口已有，无需新造选择模型 |
| `EntityKind` 只有 5 变体：Place / Region / Faction / StreetSegment / Event。**无 Character / Situation** | `wv-core/src/source/label.rs:45` | D4 落地不需要扩变体（扩了要动 4 处穷尽匹配） |
| 地图上可点的只有 Place / Region / Faction / Event 四类的**标签** | `wv-render/src/drill.rs:845/1061/1251/1428` | 与 D4 的「四类 + Cell」完全吻合 |
| `StreetSegment` 无稳定 id，不注册命中 | `wv-render/src/label_pick.rs:17` | 属性分派里这一支是死路，不做表单 |
| Cell 选中态**不在** `WorldProjection.selection` 里，而在 `EditCellState.selected: Option<(i32,i32)>`（外加 `region_cells` 批量集） | `wv-render/src/state.rs:817-840` | Cell 分派要读第二个资源；批量模式也要能被 Inspector 覆盖 |
| **Cell 的活路径是直提**：`cell_editor_panel.rs` 里的 `push_cell_edit` / `apply_batch` 直接把 `EditCellCommand` / `BatchEditCellsCommand` 提交进 `History`。`EditCellState.pending` → `CommandId::CommitEditCell` 那条链**无人写入**（`pending` 字段全仓只有定义处 + 一个 `take()`，命令也没进命令面板表） | `wv-app/src/ui/cell_editor_panel.rs:707-782`；`wv-render/src/state.rs:826`；`wv-app/src/ui_shell.rs:1449` | 迁移时**照搬直提路径**，不要复活 `pending` 链（见 §11 Q3） |
| 直提路径**不**调 `TerrainTexture::mark_dirty` | 同上 | 迁移后要补上，否则改了 cell 地图不重绘（见 §11 Q3） |
| **Place / Event 没有 `Update*Command`**（Region 有 `UpdateRegionCommand`、Faction 有 `UpdateFactionCommand`）。但 Place 已有 `UpdatePlaceTerritoryCommand`（改地界 span）与 `UpdatePlaceLineCommand`（改折线），二者都会**重算 `Place.x/y/w/h`** | `wv-edit/src/command.rs:3140`；`wv-edit/src/nature_region.rs:653` | Place 的 `x/y/w/h` 是**派生量**，见 §11 Q2 |
| `Place::fit_anchor_to_ring` 由环重算 `x/y/w/h`（质心 + 包围盒） | `wv-core/src/map/distance.rs:287` | 有地界环的 Place，锚点/范围**不可直接编辑**（见 §11 Q2） |
| Place 的 biome 写入是「标记包络」语义（`PlaceBiomeMarkerCommand` 会改地形格） | `wv-edit/src/command.rs:1967` | Place 的 biome 字段**不提供自由下拉**，只读 + 「重生成标记」入口 |
| Region 卡内嵌编辑实际只有：改名 / 轮廓信息 / **类别转换按钮**（非 radio）/ 覆盖物键下拉（仅 Nature）/ 重生成边界（仅 Nature）/ 删除。**父区域无任何 UI** | `wv-app/src/ui/region_panel.rs:296-468` | §6.4 的「父区域下拉」是**净新增**，不是搬现有（见 §11 Q5） |
| `SetRegionParentCommand` **存在**且自带环检测（`is_descendant`）；但 `is_descendant` 是 `wv-edit` 私有 fn，应用层算不了后代集 | `wv-edit/src/nature_region.rs:182, 253` | 父区域下拉要么在 wv-edit 导出后代查询，要么在 wv-app 自己实现（见 §11 Q5） |
| `nature_region_actions` 只有 overlay / kind / regen / materialize / merge / candidate 六个动作，**无 parent** | `crates/wv-app/src/edit/nature_region_actions.rs` | 同上 |
| 势力几何的「+ 新建疆界」写的是**硬编码 1×1 占位三角形**（`GeometryData::Polygon`），无点位编辑器、无变体选择 | `crates/wv-app/src/edit/faction_panel_actions.rs:93-118` | 「增」= 占位三角形，非真编辑器（见 §11 Q7） |
| 势力驻地 / 归属目标当前是**自由文本 TextEdit**，不是下拉 | `crates/wv-app/src/ui/faction_panel.rs:357-380, 423` | 见 §11 Q8 |
| wv-edit **无 `AddEventCommand` / `AddCharacterCommand` / `AddSituationCommand`** | 命令清单 | 事件只能来自 YAML 导入，UI 无法新建（见 §11 Q9） |
| `wv-ui` **已有 `tabs` 控件**（`controls.rs:635`，分段药丸形态，非下划线 tab） | `crates/wv-ui/src/widgets/controls.rs:635` | §9.1 的「新增 tabs 控件」是多余的——**复用**即可 |
| `drill_pick` 有 `ui.wants_pointer` 门（点面板不吃地图点击） | `wv-render/src/drill.rs:507` | 列表行点击**不会**被地图的「点空白清选中」抢走（见 §11 Q6） |
| `WorldProjection` 定义在 **wv-render**（`wv-render/src/state.rs:46`），wv-app 通过 `state.rs:21` 重导出 | `crates/wv-app/src/state.rs:21` | wv-app 写 `projection.selection` 是允许的（既有 `region_panel` / `faction_panel` 已持 `ResMut<WorldProjection>`） |
| Faction 的内嵌编辑已铺开：名称 / 颜色 / 层 / 等级 / 驻地 / 归属边 / 几何 / 定位 / 删除 | `wv-app/src/ui/faction_panel.rs:270-545` | 整卡迁入；命令全在（`UpdateFactionCommand` / `SetFactionLayerCommand` / `SetFactionRankCommand` / `SetFactionSeatCommand` / `AddMembershipCommand` / `RemoveMembershipCommand` / `UpsertFactionGeometryCommand` / `RemoveFactionGeometryCommand`） |
| Bevy 系统参数上限 16 | ADR-0049 已踩过；`apply_pending_commands` 用**元组折叠**（19 参折成嵌套元组，每层算 1 参）逃生 | `crates/wv-app/src/ui_shell.rs:1162-1175` | 大一统 Inspector 系统会爆。**按元组折叠分组**（见 §6.2） |
| egui 同 scope 同 id 会冲突；换内容时同名字段撞 id | 仓内已有同类坑记录 | 各类型表单要用 `id_salt` 按类型隔离 |
| `float_window` 走 `egui::Window`（`FloatWindow`），面板 id = `("wv_panel", Panel)` | `wv-app/src/ui/widgets/local.rs:36` | 三个页签共用一个窗口 → 内部用 `ui.horizontal` 的段控件切 body，不能拆成三个 egui Window |
| `ui-layout.yaml` 的 `panels` 是 `BTreeMap<String, PanelLayout>`，键 = `Panel::key()`，加载时按 key 取 | `wv-app/src/ui_layout.rs:61-64` | 面板枚举若增删，布局文件需容错（未知键忽略） |

---

## 5. 目标结构

### 5.1 面板枚举与外壳

```
Panel::Inspector   ← 唯一属性面，右栏
  ├─ 页签：属性 | 势力层 | 事态
  ├─ 「属性」页签 → 按选中对象分派
  │    ├─ 无选中   → 空态（沿用现有 empty_state + 计数行）
  │    ├─ Place    → place_form()
  │    ├─ Region   → region_form()
  │    ├─ Faction  → faction_form()
  │    ├─ Event    → event_form()
  │    └─ Cell     → cell_form()   （EditCell 工具激活时）
  ├─ 「势力层」页签 → 势力层管理（层头增删改 + 势力列表）
  └─ 「事态」页签   → 事态列表（现状搬过来）
```

**面板枚举不变**（`Panel::ALL` 仍是 12 项）——`Panel::CellEditor` 与 `Panel::Faction` 的**内容**迁入 Inspector，但枚举项先留着，避免一次性动 `ui-layout.yaml` / 快捷键 / 命令面板 / i18n 全链。退役清单见 §8（可在 M2 再摘）。

> **为什么保留枚举项**：删 `Panel::CellEditor` 会牵动 `ui-layout.yaml` 的 `cell_editor` 键（存量工程文件里都有）、`ui_command_palette` 的 `CmdTogglePanelCellEditor`、`state.rs` 的 `default_pos`/`default_size`/`key`/`icon` 四处、以及 §2.2 布局互斥测试。留着的成本是「面板还在枚举里但不再有独立浮窗」，收益是 M1 改动面可控。摘除时一次性做完并附 YAML 迁移说明。
>
> **例外**：`Panel::CellEditor` 的浮窗在 M1 就必须**停止渲染**（内容已搬走），但**枚举项保留**。这会导致 CellMap 工作区在 M1→M2 之间开空屏（工作区只开 CellEditor）——**M1 必须同批把工作区改成开 `Panel::Inspector`**，两件事一起做，不留中间态。

### 5.2 三个页签的职责边界

| 页签 | 内容 | 数据来源 | 写入 |
|---|---|---|---|
| **属性** | 当前选中对象的属性表单（5 类） | `projection.selection` + `edit_cell.selected` | 各类 `EditCommand` |
| **势力层** | 势力层的增删改（层头：名称/z/配色策略/可见/删除）+ 势力列表 | `world.factions` / `world.faction_layers` | `AddFactionLayerCommand` / `UpdateFactionLayerCommand` / `RemoveFactionLayerCommand` |
| **事态** | 事态列表（现状 `situation_panel` 的内容原样搬入） | `world.situations` | 无（只读列表，编辑走时间轴） |

**页签状态的持久化**：页签切换是**会话状态**，不落 `ui-layout.yaml`（那是布局记忆，不是 UI 偏好）。放 `PanelState` 旁边的新字段 `inspector_tab: InspectorTab`，`PanelState` 已是会话态资源。

---

## 6. 实现设计

### 6.1 分派

```rust
// wv-app/src/ui/inspector/mod.rs

/// Inspector 的三个页签。**会话态**——不落 `ui-layout.yaml`
///（那是布局记忆，不是 UI 偏好），放 `PanelState` 旁的新字段。
pub enum InspectorTab { Properties, FactionLayers, Situations }

pub fn panel_inspector(
    mut contexts: EguiContexts,
    (mut projection, mut edit): (ResMut<WorldProjection>, ResMut<editing::EditHistory>),
    (mut panels, mut inspector_state): (ResMut<PanelState>, ResMut<InspectorState>),
    (view, playback, grid): (Res<ViewState>, Res<PlaybackState>, Res<GridConfig>),
    (mut edit_cell, mut display): (ResMut<EditCellState>, ResMut<FactionDisplay>),
    (shell_cfg, layouts, current_loc): (Res<UiShellConfig>, Res<LayoutFile>, Res<CurrentLocale>),
    (mut tex, mut notice, time): (ResMut<TerrainTexture>, ResMut<ShellNotice>, Res<Time>),
) { … }
```

**分派规则**（顺序即优先级）：

1. `EditCellState.tool_active == true` → **Cell 表单**（含批量：非空 `region_cells` 时显示批量模式）。此时 `projection.selection` 的实体表单**不显示**——ADR-0040 §决策 6 原意就是「CellInspector 工具激活 + 命中一格时，Inspector 出 Cell 字段」，工具态优先于实体选中态是同款语义。
2. 否则 `projection.selection == Some(sel)` → 按 `sel.kind` 分派：Place / Region / Faction / Event 各一表单；`StreetSegment` → 落到「该类型暂不支持」提示（现状它也点不到）。
3. 否则 → 空态。

**「点行 = 选中」**：Region 面板、Faction 列表、势力列表的每一行都可点击，点击后写 `projection.selection = Some(SelectedEntity::new(kind, id))`，Inspector 的属性页签随即切到该类型的表单。**这是本设计把「列表」和「属性」接起来的关键动作**——目前这两者是断的（`list_item` 传 `selected: false` 硬编码）。

**点击不与地图抢**：`drill_pick` 有 `ui.wants_pointer` 门（`wv-render/src/drill.rs:507`），指针在 egui 面板上时地图的「点空白清选中」**不会**触发。所以点列表行写 selection 不会被紧随其后的地图点击清掉。**无需额外抑制逻辑。**

**CellMap 工作区的联动**（`top_bar.rs:245-256`）：进 CellMap 工作区时顶栏会把**所有**面板隐藏、只开 `Panel::CellEditor`、并排 EditCell 工具。CellEditor 内容迁入 Inspector 后，**这一步必须改成开 `Panel::Inspector`**——否则工作区会开出一个空屏（`Panel::CellEditor` 的浮窗不再渲染内容）。同时工作区的备份/还原逻辑（`PanelState::cell_workspace_backup`）照旧。

### 6.2 Bevy 16 参数上限的对策

**实测需求远不止 16 个**。按类型逐个数：Region 表单要 `ViewState`（覆盖物键词表）+ `GridConfig`（cell_size / raster_to_map，用于「重生成边界」）+ `ShellNotice`（错误提示）+ `Time`；Place 表单要 `TerrainTexture`（「重生成标记」后要重绘）；Faction 表单要 `PlaybackState`（归属边的 from 时刻）+ `FactionDisplay`（定位高亮）；Cell 表单要 `SelectionState`（批量取消）+ `CellPanelMode` + `TerrainTexture`。加上窗口骨架的 `EguiContexts` / `UiShellConfig` / `LayoutFile` / `PanelState` / `WorldProjection` / `EditHistory` / `CurrentLocale` —— 合计 20+，硬超 16。

**对策：元组折叠**（仓内既有先例，`apply_pending_commands` 就是这么把 19 参降到限内的——`SystemParam` 对元组递归实现，每层元组算 1 个参数，解构保留原名，函数体不受影响）。

```rust
pub fn panel_inspector(
    mut contexts: EguiContexts,
    (mut projection, mut edit): (ResMut<WorldProjection>, ResMut<editing::EditHistory>),
    (mut panels, mut inspector_state): (ResMut<PanelState>, ResMut<InspectorState>),
    (view, playback, grid): (Res<ViewState>, Res<PlaybackState>, Res<GridConfig>),
    (mut edit_cell, mut display): (ResMut<EditCellState>, ResMut<FactionDisplay>),
    (shell_cfg, layouts, current_loc): (Res<UiShellConfig>, Res<LayoutFile>, Res<CurrentLocale>),
    (mut tex, mut notice, time): (ResMut<TerrainTexture>, ResMut<ShellNotice>, Res<Time>),
) { … }
```

**顶层 4 参 + 5 个元组 = 9 参**，安全。表单渲染函数是**纯函数**（`fn region_form(ui, shell, …) -> …`），不是独立 system——Rust 函数参数没有 16 上限，且不需要 `InspectorTarget` 资源做跨 system 通道（§6.2 原稿的 `InspectorTarget` 枚举因此**不需要存在**，分派在同一个函数体内完成）。

> **备选方案**（不推荐）：拆成 3 个 system（属性 / 势力层 / 事态），各自调 `float_window` 画**同一个** `Panel::Inspector` id。egui 同一 id 画两次会互相抢内存窗口状态，且页签切换要跨 system 传状态——比折叠元组复杂得多。元组折叠是仓内已验证的路。

### 6.3 新增两个 EditCommand

`Place` 与 `Event` 无 `Update*Command`，照 `UpdateRegionCommand` 的 snapshot→diff→revert 模板抄。

```rust
// wv-edit/src/command.rs

/// 改地点属性（名称 / 呈现级别 / 锚点）。
///
/// **锚点条件可写**：`new_x/new_y` 为 `None` 时表示「不改」。
/// 只有**无地界环、非折线**的点状 Place 才允许传值——有环的 Place
/// 其 x/y/w/h 由 `Place::fit_anchor_to_ring` 从环派生
/// （`UpdatePlaceTerritoryCommand` / `UpdatePlaceLineCommand` 都会重算），
/// 直接改会造出「两个真相」。表单层负责按「有无地界环」决定传不传。
/// **不**含 w/h：范围是包围盒的推论，与锚点同源。
/// **不**含 biome：biome 的真值是「标记包络」，`PlaceBiomeMarkerCommand`
/// 是它唯一的写入口。
pub struct UpdatePlaceCommand {
    pub id: Id,
    pub new_name: String,
    pub new_rank: Option<u8>,
    pub new_x: Option<f32>,   // None = 不改（表单层决定是否可写）
    pub new_y: Option<f32>,
    before: Mutex<Option<Place>>,   // 快照整个 Place，revert 原子写回
}

/// 改事件属性（名称 / 时间跨度 / 地点 / 参与者 / 叙述 / 结果）。
///
/// **不含** `effects` / `narrations`：两者本轮不提供编辑入口
/// （见 §11 Q1），但快照 revert 是整实体写回——若表单之外有别的
/// 路径改了它们，revert 会一并退回。缓解：revert 只写回本命令
/// 实际改过的字段（逐字段 before），而不是整实体替换。
pub struct UpdateEventCommand {
    pub id: Id,
    pub new_name: String,
    pub new_span: TimeSpan,
    pub new_location: Option<EventLocation>,
    pub new_participants: Vec<Id>,
    pub new_narrative: Option<String>,
    pub new_outcome: Option<String>,
    before: Mutex<Option<EventFields>>,  // 只快照本命令触及的字段
}
```

**revert 的两种写法**：
- `UpdatePlaceCommand`：Place 表单只碰 name / rank 两个字段，但快照整个 Place 写回是安全的（表单不提供并发改 Place 的其它入口）。**沿用 `UpdateRegionCommand` 的整实体写回**，与仓内既有模式一致。
- `UpdateEventCommand`：**逐字段 before**。理由是 `Event.effects` / `narrations` 虽不在表单里，但可能有其它路径改（如将来的事件效果编辑器）。整实体写回会把那些改动误退。

**时间维度的坑**：`Event.span` 是时间区间。改 `span` 是「改事件的发生时间」，属 P7 允许的（事件本身就是一个带区间的实体，不是「当前值」字段）；但**不允许**在事件里加 `current_day` 之类的当前值字段（P7 禁令）。

**参与者的 P4 检查**：`participants: Vec<Id>` 引用人物。改名单不校验目标存在（V1 守约：校验只提示不阻止）。

### 6.4 五个表单的字段清单

| 表单 | 字段 | 控件 | 写入命令 |
|---|---|---|---|
| **Place** | 名称 | text | `UpdatePlaceCommand` |
| | 呈现级别 rank | 下拉 0–3（`effective_rank` 折算只读提示） | `UpdatePlaceCommand` |
| | 锚点 X / Y | **无地界环且非折线** → 可编辑；**有环 / 折线** → 只读，标注「由地界/折线派生」 | `UpdatePlaceCommand`（`new_x/new_y` 传 `None` = 不改） |
| | 范围 W / H | 只读（包围盒推论，与锚点同源） | — |
| | 群系 biome | **只读** + 「重生成标记」按钮 | `RegenerateBiomeMarkerCommand` |
| | 所属地图 | 只读 | — |
| | 稳定 ID | 只读（mono） | — |
| **Region** | 名称 | text | `UpdateRegionCommand` |
| | 类别 kind | radio：Area / Nature | `nature_region_actions::set_region_kind_now` |
| | 父区域 parent | 下拉（**净新增**，需新写 `set_region_parent_now` + 后代集计算，见 §11 Q5） | `SetRegionParentCommand`（已存在，环检测在命令内） |
| | 覆盖物键 overlay | 下拉（主题已声明的键 + 「无」）；**仅 Nature 可见** | `set_region_overlay_now` |
| | 轮廓顶点数 / 洞数 | 只读 | — |
| | 重生成边界 | 按钮；**仅 Nature 可见** | `regenerate_nature_region_now` |
| | 删除 | 按钮（红） | `RemoveRegionCommand` |
| **Faction** | 名称 / 颜色 | text + color text | `UpdateFactionCommand` |
| | 所属层 | 下拉 | `SetFactionLayerCommand` |
| | 等级 rank | text（自由串） | `SetFactionRankCommand` |
| | 驻地 seat | **自由文本 Id**（沿用现状；改下拉见 §11 Q8） | `SetFactionSeatCommand` |
| | 归属边 memberships | 列表：目标（自由文本 Id）+ 关系 + 区间；增删 | `AddMembershipCommand` / `RemoveMembershipCommand` |
| | 几何 | 列表：变体 + 顶点数；「增」= 1×1 占位三角形（无点位编辑器，见 §11 Q7） | `UpsertFactionGeometryCommand` / `RemoveFactionGeometryCommand` |
| | 在势力图定位 | 按钮 | `FactionDisplay.focus`（会话态，非命令） |
| | 删除势力 | 按钮（红） | `RemoveFactionCommand` |
| **Event** | 名称 | text | `UpdateEventCommand` |
| | 时间跨度 from / to | numeric（天；`to` 空 = 开放） | 同上 |
| | 地点 location | Place Id（自由文本）+ 可选 X/Y/W/H 覆写 | 同上 |
| | 参与者 participants | 列表：Id 增删（自由文本，见 §11 Q8） | 同上 |
| | 叙述 narrative / 结果 outcome | 多行 text | 同上 |
| | effects / narrations | **只读列表**（本轮不提供编辑，见 §11 Q1） | — |
| **Cell** | 坐标 X / Y | 只读 | — |
| | 地表 surface | segmented：水 / 陆地 | `EditCellCommand`（**直提**，照搬 `push_cell_edit`，见 §11 Q3） |
| | 海拔 height | numeric（i16 米，钳制 ±32767） | 同上 |
| | 群系 biome | radio（7 族 + 无） | 同上 |
| | 批量模式 | 多边形选区贴合的 N 格 | `BatchEditCellsCommand`（已存在，`apply_batch`） |

### 6.5 视觉参照

`.figma-ref/inspector_node_42_2.png`（Figma `42:2` 的 Inspector 面板）的形态：

```
┌──────────────────────────────┐
│ [icon] Government Office  [pin]│   ← identity_header（wv-ui 已有）
│        Point of interest·Civic │
│ poi_government_001   Selection pinned │  ← id 行
│ ──────────────────────────── │
│ Type        [Government    ▾] │   ← 字段行：label 左 + 控件右
│ Position    [1142.00 / 496.00]│
│ Elevation   [842          m] │
│ ☑ Visible on map          🔒  │
│ [⛶ Focus F]  [✎ Edit details] │   ← 底部按钮对
└──────────────────────────────┘
```

**对齐要点**：
- 页签条 = `wv_ui::widgets::controls::tabs`（**已有**，分段药丸形态）。
- 头部 = `wv_ui::widgets::identity_header`（已有，Figma `42:2` 提取）。
- 字段行 = `value_box`（只读）/ `numeric_input_bounded`（可编辑）/ `radio` / 下拉。左标签右控件，标签列定宽。
- 底部按钮对 = Focus（相机移到对象）/ Edit details（展开更多字段）。
- **类型色点**：沿用现状 `inspector_panel.rs:129` 的 kind→色映射（Place=accent / Region=gold / Faction=danger / Event=success / Segment=t3）。
- 面板宽度：现状 `Panel::Inspector` 默认 264px。属性表单比现状宽——**改 300px**（Figma `42:2` 的 Inspector 宽 292px，与 web 原型 `.w-inspector { width: 292px }` 一致）。这会让它与下方 `CellEditor`/`Character` 面板在右列重叠——**M1 同期把 `Panel::CellEditor` 的默认位置腾空**（见 §8 M1 任务 3）。

### 6.6 现有面板的迁移

| 现有内容 | 去向 | 处理 |
|---|---|---|
| `region_panel::region_card`（改名/轮廓信息/类别转换/覆盖物键/重生成/删除） | → `region_form()` | 整卡搬入，命令不动。**父区域是净新增**（§11 Q5） |
| `region_panel` 顶部的「+ 新建区域」按钮 | 留在 region_panel | 区域列表是选择器 |
| `region_panel` 里的区域列表项 | 留在 region_panel，**改为可点**（点行 = 选中） | 唯一改动：`list_item` 的 `selected` 从硬编码 `false` 改为「是否 == 当前选中」+ 加 `on_click` |
| `faction_panel::faction_card`（名称/颜色/层/等级/驻地/归属/几何/定位/删除） | → `faction_form()` | 整卡搬入，命令不动 |
| `faction_panel` 的**层头**（名称/z/配色策略/可见/删除）+「+ 新建势力层」 | → Inspector「势力层」页签 | 搬入 |
| `faction_panel` 的**势力列表**（按层分组的卡容器） | 「势力层」页签内保留为列表 | 列表项改为可点（点行 = 选中 → 属性页签） |
| `cell_editor_panel` 全部内容 | → `cell_form()` | 整块搬入；`CellPanelMode`（单格/批量）保留；写入路径**照搬直提**（`push_cell_edit` / `apply_batch`），并补 `tex.mark_dirty`（§11 Q3） |
| `situation_panel` 全部内容 | → Inspector「事态」页签 | 整块搬入 |
| `inspector_panel` 现有的 4 条计数行 + 3 张 situation 卡 | 保留在「属性」页签的**空态** | 无选中时显示（现状不变） |
| `list_panels_panel`（人物/事件列表） | **不动** | 人物/事件不进 Inspector（§3 非目标）。事件列表行是否可点见 §11 Q9 |

### 6.7 层与页签的落地位置

代码组织（避免 `inspector_panel.rs` 膨胀到 1000+ 行）：

```
crates/wv-app/src/ui/inspector/
  mod.rs            ← panel_inspector（窗口 + 页签条 + 分派 + 元组折叠解构）
  place.rs          ← place_form()
  region.rs         ← region_form()
  faction.rs        ← faction_form()
  event.rs          ← event_form()
  cell.rs           ← cell_form()
  layers.rs         ← 势力层页签（层头 + 势力列表）
  situations.rs     ← 事态页签
crates/wv-app/src/edit/
  inspector_actions.rs  ← 新增：属性表单 → EditCommand 的动作层
```

**分层纪律**：UI 文件只收输入 + 呈现；所有「提交 EditCommand」集中在 `inspector_actions.rs`（沿用 `region_panel_actions` / `faction_panel_actions` 的既有约定，AGENTS §3.3）。

---

## 7. 约束

- **AGENTS §3.3**：System 不直接改世界，一律 `EditCommand`。
- **AGENTS §2.1**：改动只在 wv-app（应用层），wv-core / wv-edit / wv-render / wv-ui 的依赖方向不变。**不得**在 wv-render 引入对 wv-edit 的引用。
- **AGENTS §6.3**：本设计修正 ADR-0040 §决策 6（「Inspector Panel 加 Cell 折叠区」→ 独立浮窗 → 本次改回「Cell 表单进 Inspector 属性页签」）。修正要**新开 ADR 追记**，不改 ADR-0040 正文。
- **AGENTS §8**：新增 UI 控件一律取自 Lucide（如需新图标）。
- **P3 / P6 / P7**：数据模型零变化；写入走带快照的命令；不引入任何「当前值」字段。
- **§7.1 门禁**：`cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` + `cargo test -p wv-app --features app` + `cargo clippy -p wv-app --all-targets --features app -- -D warnings` + `bash scripts/smoke-gui.sh`。

---

## 8. 实施分片

| 阶段 | 内容 | 产出 |
|---|---|---|
| **M0** | 新增 `UpdatePlaceCommand` + `UpdateEventCommand`（含单测：apply→revert 往返；Event 用逐字段快照，Place 的 x/y 条件可写） | 数据层可写 |
| **M1** | Inspector 骨架：页签条（复用 `wv-ui::tabs`）+ 分派 + 5 个空表单占位；`Panel::CellEditor` 停渲染（枚举项保留）；`Panel::Inspector` 宽度 264→300 并调右列布局；**同批把 CellMap 工作区改为开 `Panel::Inspector`** | 单窗口成型 |
| **M2** | 5 个表单逐个落地（Place → Region → Faction → Event → Cell），每型一个命令通一个表单；Cell 走直提路径 + 补 `tex.mark_dirty`；**删死代码** `EditCellState.pending` 字段 + `CommandId::CommitEditCell` 分支 | 属性可编辑 |
| **M3** | 列表可点化（region / faction 行 → 选中）；`inspector_actions.rs` 收口 | 列表↔属性接通 |
| **M4** | 「势力层」「事态」页签落地；`faction_panel` / `situation_panel` / `cell_editor_panel` 降级为「内容已搬走」的薄壳或退役 | 面板收编完成 |
| **M5** | 视觉对齐 Figma（宽度/字段行/底部按钮对/类型色点）；i18n 补齐新文案 | 验收 |
| **M6** | 写 ADR（追记 ADR-0040 §决策 6 修正 + 新面板结构），更新 README / 基线文档的 P-4 描述 | 留痕 |

---

## 9. 留给实现者的判断（不阻塞开工）

1. **页签条控件**：`wv-ui` **已有** `controls::tabs`（分段药丸形态：容器 `bg_input` 底 + radius 4，选中段 `bg_raised` 底 + `t1` 字）。**直接复用**。它不是下划线 tab 形态——若 Figma 后续给出下划线版，再在 wv-ui 加变体而非在 wv-app 私造。
2. **表单字段的布局密度**：264→300px 后，「标签 + 控件」两列是否够放 `Event` 的时间跨度（from/to 两格 + 单位）。倾向 2 列网格 + 标签列 72px 定宽。
3. **`Place.rank` 的取值范围**：模型是 `Option<u8>`，`effective_rank` 折算 1/3。UI 给 0–3 全档还是只给 1/2/3？倾向 0–3（与模型同域），并在只读区提示折算规则。
4. **`ui-layout.yaml` 迁移**：M1 改 `Panel::Inspector` 宽度会让存量工程的 `inspector.width: 264` 继续生效（新默认不生效）。要么 M1 顺带在加载时钳制下限 300，要么接受旧工程仍是 264（表单横向滚动）。倾向**加载时不钳制**（尊重作者手调），M5 验收时用新建工程看新默认。
5. **`.figma-ref/inspector_node_42_2.png` 的「Selection pinned」**：Figma 稿里选中态有 pinned 语义（选中项钉在面板顶部）。本设计**不做** pin（点谁看谁），列为 v2。
6. **仓内 `assets/ui-design/`（v0.1 HTML 原型 + `UI_DESIGN_SPEC_v0.1.md`）**：D1 只裁了 Downloads 里的 Figma-Make React 原型；仓内这套是**另一个**原型，且 spec 通篇拿它当验收依据。本设计**不动它**——它退役与否是独立决策，需另行裁定（若退役，spec 里「原型已验证」的措辞要按 AGENTS §6.3 追加修订说明，不能删改原文）。
7. **文本字段的提交粒度**：现状 `region_panel` 是**每次按键提交一次**（`text_edit_singleline(...).changed()` 即提交），打 5 个字进 5 条撤销栈。新表单是照搬这个行为（一致性优先，改动面小），还是改成失焦/回车才提交（撤销栈更干净，但要引入缓冲）？**倾向照搬现状**——本轮不引入新交互范式，且 `inspector_actions` 的形状更简单。留待实机验收时若撤销栈明显难用再改。

---

## 10. 验收判据

| # | 判据 | 怎么验 |
|---|---|---|
| V1 | 打开 Inspector，只有一个窗口，三页签可切 | 实机截图 |
| V2 | 点地图上 Place → 属性页签出 Place 表单；点 Region → 出 Region 表单；势力标签 → 势力表单；事件标签 → 事件表单 | 实机 |
| V3 | EditCell 工具激活 + 点一格 → 属性页签出 Cell 表单（地表/海拔/群系三通道可改），**且地图立刻重绘** | 实机 + 撤销验证 |
| V4 | 改任一字段 → 撤销栈可退；`cargo test` 全绿 | 门禁 |
| V5 | 改 Place 名称 / Event 时间 → YAML 落盘正确，`load(save(w)) == w` | round-trip 测试 |
| V6 | 不选任何对象 → 属性页签显示空态 + 计数行（现状不变） | 实机 |
| V7 | `cargo tree -p wv-render \| grep -E 'wv-edit\|wv-app'` 仍为空 | 门禁 |
| V8 | 烟雾测试：5 秒存活 + 日志无 panic/B000x | `scripts/smoke-gui.sh` |
| V9 | 进 CellMap 工作区 → 打开的是 Inspector（属性页签出 Cell 表单），不是空屏 | 实机 |

---

## 11. 裁定汇总（builder check 提出的问题）

以下 6 条**会改变实现方案**，均已核实过代码事实。**裁定已定**（Q2 / Q4 两轮问询未获答复，按推荐项定稿；改动成本低且可回退）。若日后要改，是局部改动，不牵动整体骨架。

| # | 问题 | 裁定 | 理由 / 代价 |
|---|---|---|---|
| Q1 | `Event` 的 `effects` / `narrations` 本轮是否可编辑？ | **不编辑，只读列表** | 表单不含这两个字段 → `UpdateEventCommand` 的 revert 用**逐字段快照**（只写回它改过的字段），避免误退别处对 effects 的改动。将来要做效果编辑器时再改 revert 形状。 |
| Q2 | `Place` 的锚点 X/Y 与范围 W/H 能否直接改？ | **分情况**：无地界环且非折线的点状 Place → 可编辑；有地界环的（城市/地界）与折线 Place（ADR-0046 小河）→ **只读**，标注「由地界/折线派生」 | `Place::fit_anchor_to_ring` 从环重算 x/y/w/h，`UpdatePlaceTerritoryCommand` / `UpdatePlaceLineCommand` 都会触发——可编辑会造「两个真相」（改了 x/y，下次重生成边界又被覆盖）。点状 Place 的位置就是它的真值，让它可编辑是正常作者需求。`UpdatePlaceCommand` 因此需带 `new_x/new_y`（可空）字段。 |
| Q3 | Cell 写入路径：直提 vs 复活 pending 链？ | **照搬直提路径**（`push_cell_edit` / `apply_batch`），**并补 `tex.mark_dirty`** | `EditCellState.pending` → `CommandId::CommitEditCell` 全仓无人写入（命令也没进命令面板表），是死链；直提路径是活的。迁移时补上地形纹理脏标记（现在直提不标脏，改了 cell 地图可能不立即重绘）。**同时把 `pending` 字段与 `CommandId::CommitEditCell` 一并删掉**（死代码清理，AGENTS §11 反模式表里的「临时这样实现」类）。 |
| Q4 | Cell 表单何时接管 Inspector？ | **EditCell 工具激活即显示**（不要求在 CellMap 工作区内） | 工具即模式，模式即该显示什么——与 ADR-0040 §决策 6 同款语义。命令面板按 C 开工具时会抢走 Inspector，这是可接受的代价（用户主动切了工具）。CellMap 工作区**另外**要把「只开 CellEditor」改成「只开 Inspector」（否则开空屏）。 |
| Q5 | Region 的「父区域」下拉本轮做不做？ | **本轮不做** | 它是**净新增功能**（现有 UI 完全没有这个控件），与本轮「把散落的属性编辑收进一处」的主线无关；且需要额外解决后代集计算（`is_descendant` 是 wv-edit 私有 fn）。列为 v2 候选。**不做 ≠ 永久排除**——字段在数据模型里已存在（`Region.parent`），将来加下拉时命令 `SetRegionParentCommand` 已就位（自带环检测），只需在 wv-edit 导出一个「取后代集」的公共函数。 |
| Q6 | 势力驻地 / 事件参与者用下拉还是自由文本？ | **保持自由文本 Id** | 与现状一致（现状 `faction_panel.rs:357-380, 423` 就是自由文本），一行 TextEdit 搬过去即可。人物 Id 目前 UI 里根本列不出来（人物无标签、无列表联动），做下拉也做不出完整版，反而是半成品。悬空引用靠 V1 校验面板提示（§7.5 V1：只提示不阻止）。 |

### 11.1 一并记录的既有事实修正

builder check 推翻了本设计初稿的 5 处说法，已逐条核实并改写（§4 表格已更新）：

1. Cell 的 `pending` 链是**死代码**（原稿以为它是活路径并要求复用）→ 改为照搬直提路径（Q3）。
2. `Place.x/y/w/h` 对有地界环的 Place 是**派生量**（原稿以为可直接编辑）→ 分情况处理（Q2）。
3. Region 的**父区域控件不存在**（原稿写成「已铺开，整卡搬入」）→ 本轮不做（Q5）。
4. `wv-ui` **已有 `tabs` 控件**（原稿说需要新增）→ 直接复用。
5. `UpdatePlaceTerritoryCommand` / `UpdatePlaceLineCommand` / `SetRegionParentCommand` **存在**（在 `wv-edit/src/nature_region.rs`，不在 `command.rs`；原稿的 grep 漏了这个文件）→ Q2 / Q5 的命令基础已就位。
