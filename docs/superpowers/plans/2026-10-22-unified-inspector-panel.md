# 统一 Inspector 面板实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把属性编辑收进右栏单窗口 Inspector——「属性 / 势力层 / 事态」三页签，属性页签按选中对象分派 Place / Region / Faction / Event / Cell 五类可编辑表单。

**Architecture:** `Panel::Inspector` 扩为模块目录 `ui/inspector/`（骨架 + 分派 + 每型一个表单文件）；写入全部走 `wv-edit` 的 `EditCommand`（新增 `UpdatePlaceCommand` / `UpdateEventCommand`，Region/Faction/Cell 复用既有命令）；Cell 走既有直提路径并补 `mark_dirty`；三个旧面板（CellEditor / Faction / Situation）内容迁入后停渲染，枚举项保留（布局兼容）。Bevy 16 参数上限用元组折叠绕开（仓内先例 `apply_pending_commands`）。

**Tech Stack:** Rust / Bevy 0.19 / bevy_egui 0.42（egui 0.36）/ wv-ui 控件库（`tabs` / `identity_header` / `value_box` / `numeric_input_bounded` / `radio`）。

**Spec:** `docs/superpowers/specs/2026-10-22-unified-inspector-panel-design.md`（含 §11 六条裁定，与本计划同行；执行者须先读 spec）

## Global Constraints

- AGENTS §13：先分支 `feat/unified-inspector`，再 `git worktree add .worktrees/feat/unified-inspector feat/unified-inspector`，worktree 用独立构建目录（不跨 worktree 共享 `CARGO_TARGET_DIR`）。
- AGENTS §3.3：System 不直接改世界——一切写入经 `EditCommand` 提交 `EditHistory`；动作收在 `inspector_actions.rs`（UI 只收输入）。
- AGENTS §2.1：改动只落 wv-app 与 wv-edit；`wv-render` 不得依赖 wv-edit / wv-app（`cargo tree -p wv-render | grep -E 'wv-edit|wv-app'` 必须为空）。
- 数据模型零变化：`Place` / `Event` 等字段与序列化不动；YAML round-trip 契约不变（P3/P6/P7）。
- 门禁（每 UI 任务后必跑，AGENTS §7.1）：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`cargo test -p wv-app --features app`（别名 `cargo app-test`）、`cargo clippy -p wv-app --all-targets --features app -- -D warnings`（`cargo app-clippy`）、`bash scripts/smoke-gui.sh`。
- 提交信息格式 `<scope>: <动作>`（AGENTS §8），如 `wv-edit: 新增 UpdatePlaceCommand`。
- UI 图标一律 Lucide（AGENTS §8）；本计划不需要新图标。
- 文本字段提交粒度：**照搬现状**（每次按键 `.changed()` 即提交，spec §9.7）。

## Review Focus

1. **有地界环的 Place 改锚点 → 双真相**：`Place.x/y` 对有环 Place 由 `fit_anchor_to_ring` 派生，表单必须只读。→ Task 6 单测 `place_has_territory_*` + Task 2 单测 `update_place_keeps_anchor_when_xy_none`。
2. **Cell 改了不重绘**：直提路径现状不调 `tex.mark_dirty`，改完地图可能不动。→ Task 10 补标脏 + 冒烟人工验证。
3. **选中对象被删后表单悬空**（删除/撤销后 `selection` 指向不存在的实体）：表单不得 panic，须落空态。→ Task 4 单测 `selection_alive_*` + 各表单渲染前的守卫。
4. **Bevy 16 参数上限**：Inspector 骨架实际需要 20+ 资源，裸签名编译不过。→ Task 4 签名钉死为元组折叠，`cargo app-test` 编译即验证。
5. **存量 `ui-layout.yaml` 带退役面板键**（`cell_editor` / `faction` / `situation`）：加载不得炸、键须被忽略。→ Task 12 单测 `stale_panel_keys_are_ignored`。

---

### Task 1: 分支与 worktree（AGENTS §13）

**Files:** 无代码改动。

- [ ] **Step 1: 建分支与 worktree**

```bash
cd D:/kudi-projects/worldview-rs
git branch feat/unified-inspector
git worktree add .worktrees/feat/unified-inspector feat/unified-inspector
cd .worktrees/feat/unified-inspector
```

- [ ] **Step 2: 基线门禁确认全绿**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && cargo app-test && cargo app-clippy`
Expected: 全绿（基线必须干净，否则先报告不做）。

---

### Task 2: `UpdatePlaceCommand`（M0）

**Files:**
- Modify: `crates/wv-edit/src/command.rs`（插在 `UpdateRegionCommand` 之后、`// ── 事实地点与地界命令` 注释之前，约 :3196）
- Modify: `crates/wv-edit/src/lib.rs:26-28`（导出列表加 `UpdatePlaceCommand`）
- Test: 同文件 `mod tests`（:3828 起，照 `update_region_changes_name_and_polygon_and_reverts` 模式）

**Interfaces:**
- Produces: `UpdatePlaceCommand::new(id: Id, new_name: String, new_rank: Option<u8>, new_x: Option<f32>, new_y: Option<f32>) -> Self`；`apply` 后 dirty 路径 `vec!["distances/places".into()]`；`new_x/new_y` 为 `None` 时保留现值（≠写 None）。

- [ ] **Step 1: 写失败测试**（`mod tests` 内，fixture 照 `sample_region` 模式写 `sample_place`）

```rust
#[test]
fn update_place_changes_name_rank_anchor_then_reverts() {
    // apply 改 name/rank/x/y → 断言四字段已变；revert → 断言全部恢复
}
#[test]
fn update_place_keeps_anchor_when_xy_none() {
    // new_x/new_y = None → apply 后 place.x/y 保持原值（不是被写成 None）
}
#[test]
fn update_place_on_missing_id_fails() { /* TargetMissing */ }
#[test]
fn update_place_dirty_path_is_places_prefix() { /* "distances/places" */ }
```

- [ ] **Step 2: 跑测试确认失败** — Run: `cargo test -p wv-edit update_place`
  Expected: FAIL（类型不存在）。

- [ ] **Step 3: 实现命令**

照 `UpdateRegionCommand`（:3140）模板：`before: Mutex<Option<Place>>` 整实体快照；`apply` 找 `world.distances.places` 中 `id` 命中项（找不到 → `EditError::TargetMissing`），快照后写 `new_name` / `new_rank` / `new_x.unwrap_or(旧x)` / `new_y.unwrap_or(旧y)`；`revert` 整体写回快照。`label()` 返 `format!("改地点「{}」", self.id)`。

- [ ] **Step 4: 跑测试确认通过** — Run: `cargo test -p wv-edit update_place` → PASS。

- [ ] **Step 5: 门禁 + 提交**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
git add crates/wv-edit/src/command.rs crates/wv-edit/src/lib.rs
git commit -m "wv-edit: 新增 UpdatePlaceCommand（名称/级别/锚点，锚点 None=不改）"
```

---

### Task 3: `UpdateEventCommand`（M0）

**Files:**
- Modify: `crates/wv-edit/src/command.rs`（紧随 `UpdatePlaceCommand` 之后）
- Modify: `crates/wv-edit/src/lib.rs` 导出
- Test: 同文件 `mod tests`（fixture `sample_event`：手写最小 `Event`，`TimeSpan { from: 100, to: Some(200) }`）

**Interfaces:**
- Produces: `UpdateEventCommand::new(id, new_name: String, new_span: TimeSpan, new_location: Option<EventLocation>, new_participants: Vec<Id>, new_narrative: Option<String>, new_outcome: Option<String>) -> Self`；dirty 路径 `vec!["timeline/events".into()]`（落盘目录见 `wv-io/src/project.rs:639`）。
- **revert 用逐字段快照**（spec §11 Q1）：私有结构 `EventFields { name, span, location, participants, narrative, outcome }` 只缓存这六字段——`effects` / `narrations` 不碰，revert 不得退掉它们的改动。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn update_event_changes_fields_then_reverts() { /* 六字段 apply 全变 + revert 全回 */ }
#[test]
fn update_event_revert_preserves_untouched_effects() {
    // apply 前手动给 event.effects 推一条；apply + revert 后 effects 仍在
}
#[test]
fn update_event_on_missing_id_fails() {}
#[test]
fn update_event_dirty_path_is_events_prefix() { /* "timeline/events" */ }
```

- [ ] **Step 2: 确认失败** — `cargo test -p wv-edit update_event` → FAIL。

- [ ] **Step 3: 实现**

同模板；`apply` 命中 `world.events`（`Vec<model::event::Event>`），先填 `EventFields` 快照再写六字段；`revert` 只写回这六字段。`label()` 返 `format!("改事件「{}」", self.id)`。

- [ ] **Step 4: 确认通过** — `cargo test -p wv-edit update_event` → PASS。

- [ ] **Step 5: 门禁 + 提交** — `wv-edit: 新增 UpdateEventCommand（逐字段快照，不碰 effects/narrations）`

---

### Task 4: Inspector 骨架——模块 + 页签条 + 分派（M1）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/mod.rs`
- Modify: `crates/wv-app/src/ui/mod.rs`（`mod inspector_panel` → `mod inspector`；`pub use inspector::panel_inspector;` 保持 `ui::panel_inspector` 路径不变，`app_bootstrap.rs:472` 零改动）
- Delete: `crates/wv-app/src/ui/inspector_panel.rs`（内容迁入 `inspector/mod.rs`）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（注册 `InspectorState` 资源，仿 :174 `init_resource` 注释段）
- Test: `crates/wv-app/src/ui/inspector/mod.rs` 内 `mod tests`

**Interfaces:**
- Produces:
  - `#[derive(Resource)] pub struct InspectorState { pub tab: InspectorTab }`（`Default` = `Properties`）
  - `pub enum InspectorTab { Properties, FactionLayers, Situations }`
  - `pub enum FormKind { None, Place, Region, Faction, Event, Cell, UnsupportedSegment }`
  - `pub fn resolve_form(selection: Option<&wv_core::SelectedEntity>, cell_tool_active: bool) -> FormKind`（Cell 优先，spec §11 Q4）
  - `pub fn selection_alive(world: &wv_core::World, sel: &wv_core::SelectedEntity) -> bool`（按 kind 查 `world.distances.places` / `world.regions` / `world.factions` / `world.events`；`StreetSegment` 恒 false）
  - `pub fn panel_inspector(...)`（签名钉死，元组折叠）：

```rust
pub fn panel_inspector(
    mut contexts: EguiContexts,
    (mut projection, mut edit): (ResMut<WorldProjection>, ResMut<editing::EditHistory>),
    (mut panels, mut inspector): (ResMut<PanelState>, ResMut<InspectorState>),
    (view, playback, grid): (Res<ViewState>, Res<PlaybackState>, Res<GridConfig>),
    (mut edit_cell, mut display): (ResMut<EditCellState>, ResMut<FactionDisplay>),
    (shell_cfg, layouts, current_loc): (Res<UiShellConfig>, Res<LayoutFile>, Res<CurrentLocale>),
    (mut tex, mut notice, time): (ResMut<TerrainTexture>, ResMut<ShellNotice>, Res<Time>),
)
```

- Consumes: `wv_ui::widgets::controls::tabs(ui, shell, id, labels, selected, disabled) -> Option<usize>`（`crates/wv-ui/src/widgets/controls.rs:635`）。

- [ ] **Step 1: 写失败测试（纯函数）**

```rust
#[test]
fn resolve_form_prefers_cell_tool_over_entity_selection() {
    // cell_tool_active=true 且 selection=Some(Place) → FormKind::Cell
}
#[test]
fn resolve_form_dispatches_by_kind() { /* Place/Region/Faction/Event → 各自；StreetSegment → UnsupportedSegment；None → None */ }
#[test]
fn selection_alive_false_for_missing_entity() { /* world 无该 id → false；在场 → true */ }
#[test]
fn selection_alive_false_for_street_segment() { /* 无稳定 id，恒 false */ }
```

- [ ] **Step 2: 确认失败** — `cargo app-test resolve_form` → FAIL。

- [ ] **Step 3: 实现骨架**

`panel_inspector` 主体：`float_window`（照搬现 `inspector_panel.rs` 的窗口装配）→ body 内先画 `tabs`（labels = 三页签显示名，本任务先用硬编码中文「属性 / 势力层 / 事态」，Task 13 换 i18n）→ 按 `inspector.tab` 分派。「属性」页签：`resolve_form` + `selection_alive` 守卫（不活 → 空态）；五种 FormKind 本任务各渲染一行占位文案（`FormKind::Place` → 「Place 表单（M2）」），空态与 4 条计数行**原样保留**现 `inspector_panel.rs` 的实现。「势力层 / 事态」页签渲染「M4 接入」占位。旧 `inspector_panel.rs` 的纯函数（`format_span` / `year_day` / `kind_label`）与其测试随文件迁入。`app_bootstrap` 补 `.init_resource::<ui::inspector::InspectorState>()`。

- [ ] **Step 4: 测试过 + 编译过（16 上限验证）** — `cargo app-test` → PASS（签名编译即证明元组折叠成立）。

- [ ] **Step 5: 冒烟 + 提交** — `bash scripts/smoke-gui.sh` PASS 后提交 `wv-app: Inspector 扩为三页签单窗口骨架（分派/存活守卫/元组折叠）`

---

### Task 5: CellEditor 停渲染 + CellMap 工作区改开 Inspector（M1）

**Files:**
- Modify: `crates/wv-app/src/app_bootstrap.rs:473`（删 `ui::panel_cell_editor` 注册）
- Modify: `crates/wv-app/src/ui/top_bar.rs:254`（`set_visible(Panel::CellEditor, true)` → `set_visible(Panel::Inspector, true)`）
- Modify: `crates/wv-app/src/state.rs:214`（`Panel::Inspector` 默认尺寸 264→`[300.0, 420.0]`；`Panel::CellEditor` 的 `default_pos`/`default_size` 原样保留——不再被渲染）
- Modify: `crates/wv-app/src/ui_layout.rs`（非重叠测试 §9.3-1：排除 `Panel::CellEditor`，注释「内容已迁 Inspector（spec 2026-10-22），默认位仅兼容保留」）
- Modify: `crates/wv-app/src/ui/cell_editor_panel.rs`（`panel_cell_editor` 系统删除注册后，模块降级为 `cell_form` 的函数库——本任务只删系统注册，文件内容 Task 10 迁）
- Test: `crates/wv-app/src/ui_layout.rs` 内 `mod tests`

**Interfaces:**
- Produces: CellMap 工作区（顶栏第三段）进入后打开的是 `Panel::Inspector`；`Panel::CellEditor` 枚举项保留但无渲染系统。

- [ ] **Step 1: 写失败测试** — 在 `ui_layout.rs` tests 加：`inspector_default_size_widens_to_300()`（断言 `default_size` = `[300.0, 420.0]`）。跑 `cargo app-test inspector_default_size` → FAIL。

- [ ] **Step 2: 实现四处改动**（上述 Files 全部），非重叠测试若因 Inspector 变高与 Character（y=520）重叠而红：Inspector 底 = 56+420 = 476 < 520，不重叠，预期不动。

- [ ] **Step 3: 全门禁**（含 `cargo app-test` / `cargo app-clippy` / `smoke-gui.sh`；smoke 验证默认启动不炸）。

- [ ] **Step 4: 实机验证 V9** — 顶栏点 CellMap：应打开 Inspector（此时 Inspector 骨架已能显示 Cell 占位行）。此步人工，截图留档。

- [ ] **Step 5: 提交** — `wv-app: CellMap 工作区改开 Inspector，CellEditor 面板停渲染`

---

### Task 6: `place_form` + `inspector_actions::update_place_now`（M2）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/place.rs`
- Create: `crates/wv-app/src/edit/inspector_actions.rs`（新动作层；`edit/mod.rs` 补 `pub mod inspector_actions;`）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`（`FormKind::Place` 分派接 `place::place_form`）
- Test: `inspector_actions.rs` 内 `mod tests`（纯函数部分）+ `place.rs` 内 `mod tests`

**Interfaces:**
- Produces:
  - `pub fn place_has_territory(world: &wv_core::World, place_id: &wv_core::Id) -> bool`（`world.place_geometry.iter().any(|s| &s.owner == place_id)`；`place_geometry: Vec<GeometrySpan>`，`wv-core/src/lib.rs:236`）
  - `pub fn update_place_now(world: &mut WorldProjection, edit: &mut EditHistory, view: &ViewState, id: Id, new_name: String, new_rank: Option<u8>, new_x: Option<f32>, new_y: Option<f32>)`（照 `region_panel_actions::update_region_now` 的 `push_cmd` 模式）
  - `pub fn place_form(ui: &mut egui::Ui, shell: &UiShell, place: &wv_core::distance::Place, can_edit_anchor: bool, projection: &mut WorldProjection, edit: &mut EditHistory, view: &ViewState, tex: &mut TerrainTexture, current_loc: &CurrentLocale)`
- Consumes: Task 2 的 `UpdatePlaceCommand`；`feature_place_actions::regenerate_marker_now`（重生成标记按钮，现状复用）。

- [ ] **Step 1: 写失败测试**

```rust
// inspector_actions.rs
#[test]
fn place_has_territory_true_only_when_geometry_span_exists() {}
// place.rs
#[test]
fn place_anchor_editable_false_when_territory_or_line() { /* 纯判定：has_territory || line.is_some() → false */ }
```

- [ ] **Step 2: 确认失败** — `cargo app-test place_` → FAIL。

- [ ] **Step 3: 实现**

表单字段（spec §6.4 Place 行）：`identity_header`（类型色点 accent + name + `Place · {id}`）；名称 text（每按键提交 `update_place_now`）；rank 下拉 0–3；锚点 X/Y —— `can_edit_anchor` 为 false 时 `value_box` 只读并附注「由地界/折线派生」，true 时 `numeric_input_bounded` 可编辑；W/H 只读 `value_box`；biome 只读 + 「重生成标记」按钮（调 `regenerate_marker_now`）；map / id 只读。字段行控件全部出自 wv-ui 既有件。

- [ ] **Step 4: 测试过 + 全门禁 + 冒烟**。

- [ ] **Step 5: 提交** — `wv-app: Inspector 属性页签接入 Place 表单（锚点按地界环分流只读）`

---

### Task 7: `region_form` 搬迁（M2）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/region.rs`（从 `region_panel.rs:296-470` 的 `region_card` 迁入，改名为 `pub fn region_form(...)`，签名参数照 `region_card` 原样（`ui, shell, region, view, projection, edit, current_loc, overlay_keys, cell_size, raster_to_map, notice, frame`））
- Modify: `crates/wv-app/src/ui/region_panel.rs`（删 `region_card` 及其调用；列表项保留）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`（`FormKind::Region` 分派；Region 分派处需现取 `overlay_keys` / `cell_size` / `raster_to_map` —— 取法照 `region_panel.rs` 现有调用点逐参复刻）

**Interfaces:**
- Consumes: 既有 `region_panel_actions::*` / `nature_region_actions::*`（命令零改动）。
- Produces: `region_form`，供 Inspector 分派调用；region_panel 不再渲染卡体。

- [ ] **Step 1: 迁移实现**（无新纯函数；既有测试随迁）。

- [ ] **Step 2: 全门禁 + 冒烟**（region_panel 缩为列表 + 新建按钮，Inspector 接住 Region 属性）。

- [ ] **Step 3: 实机验证 V2（Region 分支）**：点地图区域标签 → Inspector 出区域表单，改名可撤销。人工截图。

- [ ] **Step 4: 提交** — `wv-app: 区域卡编辑迁入 Inspector 属性页签`

---

### Task 8: `faction_form` 搬迁（M2）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/faction.rs`（从 `faction_panel.rs:270-545` 的 `faction_card` 迁入，改名 `pub fn faction_form(...)`；层头与层管理**留下**，Task 12 进页签）
- Modify: `crates/wv-app/src/ui/faction_panel.rs`（删卡体调用，列表保留）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`（`FormKind::Faction` 分派）

**Interfaces:**
- Consumes: 既有 `faction_panel_actions::*` 八个命令动作 + `FactionDisplay.focus`。
- Produces: `faction_form`。

- [ ] **Step 1: 迁移**（要点：`faction_card` 内的 `CollapsingHeader` 外壳去掉——Inspector 已有 identity_header；内容逐行保留）。
- [ ] **Step 2: 全门禁 + 冒烟**。
- [ ] **Step 3: 实机验证 V2（Faction 分支）**：点势力标签 → 势力表单；「在势力图定位」仍生效。
- [ ] **Step 4: 提交** — `wv-app: 势力卡编辑迁入 Inspector 属性页签`

---

### Task 9: `event_form`（M2）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/event.rs`
- Modify: `crates/wv-app/src/edit/inspector_actions.rs`（加 `pub fn update_event_now(world, edit, view, id, EventDraft)`；`EventDraft` 含 Task 3 的六字段，照 `update_place_now` 模式）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`（`FormKind::Event` 分派）
- Test: `inspector_actions.rs` tests

**Interfaces:**
- Consumes: Task 3 `UpdateEventCommand`。
- Produces: `pub fn event_form(ui, shell, event: &wv_core::Event, projection, edit, view, current_loc)`。

- [ ] **Step 1: 写失败测试** — `update_event_now_pushes_command_and_marks_dirty()`（stub `EditHistory` 模式照 `faction_panel_actions` 既有测试；断言 dirty 记录含 `timeline/events`）。
- [ ] **Step 2: 确认失败** — `cargo app-test update_event_now` → FAIL。
- [ ] **Step 3: 实现表单**：name text；span from/to `numeric_input_bounded`（to 空 = 开放区间）；location：Place Id 自由文本 + 可选 x/y/w/h 四格；participants：Id 列表增删（自由文本）；narrative / outcome 多行 text；effects / narrations 只读计数行。全部每改动即提交。
- [ ] **Step 4: 测试过 + 全门禁 + 冒烟**。
- [ ] **Step 5: 实机验证 V2（Event 分支）+ V5（改事件时间 → 保存 → YAML 正确）**。
- [ ] **Step 6: 提交** — `wv-app: Inspector 属性页签接入 Event 表单`

---

### Task 10: `cell_form` 搬迁 + 补标脏 + 删死代码（M2）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/cell.rs`（从 `cell_editor_panel.rs` 迁入：字段区、`CellPanelMode`、批量区、`push_cell_edit` / `apply_batch` / `read_cell` 及其测试）
- Modify: `crates/wv-app/src/ui/cell_editor_panel.rs`（清空为空模块或删除——`ui/mod.rs` 同步摘除）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`（`FormKind::Cell` 分派接 `cell::cell_form`）
- Modify: `crates/wv-render/src/state.rs:793-826`（删 `EditCellPending` 与 `EditCellState.pending` 字段及文档注释）
- Modify: `crates/wv-app/src/ui_command_palette.rs:160-167`（删 `CommitEditCell` 变体及注释）
- Modify: `crates/wv-app/src/ui_shell.rs:1441-1511`（删 `CommandId::CommitEditCell` 分支）

**Interfaces:**
- Consumes: `EditCellCommand` / `BatchEditCellsCommand`（直提，spec §11 Q3）。
- Produces: `pub fn cell_form(ui, shell, projection, edit, edit_cell: &mut EditCellState, tex: &mut TerrainTexture, notice: &mut ShellNotice, time: &Time, mode: &mut CellPanelMode, current_loc)`；提交成功后 `tex.mark_dirty([gx, gy, gx+1, gy+1])`（批量 = 变更格 bbox，`TerrainTexture::mark_dirty(&mut self, rect: [u32; 4])`，`terrain_texture.rs:73`）+ `projection.dirty = true`。

- [ ] **Step 1: 迁移 + 补标脏**（`push_cell_edit` / `apply_batch` 的 `commit` 成功分支加 `mark_dirty` 与 `projection.dirty = true`，照 `ui_shell.rs:1492-1499` 既有收尾语义；函数签名因此要多收 `tex` / `projection`）。
- [ ] **Step 2: 删三处死代码**（wv-render 的 `pending`、命令变体、分发分支）。
- [ ] **Step 3: 验证死代码删净** — Run: `grep -rn "EditCellPending\|CommitEditCell" crates/` → 无输出。
- [ ] **Step 4: 全门禁 + 冒烟**（`cargo tree -p wv-render | grep -E 'wv-edit|wv-app'` 顺带跑一次，应为空）。
- [ ] **Step 5: 实机验证 V3 + V9**：CellMap 进工作区 → Inspector 出 Cell 表单；改海拔 → **地图立即重绘**（Review Focus #2 的实机判据）；撤销可退。
- [ ] **Step 6: 提交** — `wv-app: Cell 编辑迁入 Inspector（直提补标脏），删 pending 死链（wv-render）`

---

### Task 11: 列表行可点 = 选中（M3）

**Files:**
- Modify: `crates/wv-app/src/ui/region_panel.rs`（列表行 `list_item` 接 `selected` 判定 + 点击写 `projection.selection`）
- Modify: `crates/wv-app/src/ui/faction_panel.rs`（势力列表行同款）
- Test: 现有文件 tests（若加纯函数则随加）

**Interfaces:**
- Consumes: `list_item(...) -> egui::Response`（返回 Response，`primitives.rs:117`）；`wv_core::SelectedEntity::new(kind, id)`。
- Produces: 点行 → `projection.selection = Some(SelectedEntity::new(EntityKind::Region / Faction, id))`；行高亮 = `selection` 与该行一致。

- [ ] **Step 1: 实现两处列表行接线**（region 列表若现为 CollapsingHeader 卡，改为 `list_item` 行——卡体已在 Task 7 迁走）。
- [ ] **Step 2: 全门禁 + 冒烟**。
- [ ] **Step 3: 实机验证**：区域面板点一行 → Inspector 出该区域表单；势力列表同理；点地图空白 → 选中清除、Inspector 回空态（`drill_pick` 既有行为，`wants_pointer` 门保证面板点击不被地图抢，spec §6.1）。
- [ ] **Step 4: 提交** — `wv-app: 区域/势力列表行点击驱动 Inspector 选中`

---

### Task 12: 「势力层」「事态」页签落地 + 旧面板退役（M4）

**Files:**
- Create: `crates/wv-app/src/ui/inspector/layers.rs`（从 `faction_panel.rs` 迁层头管理 + 势力列表）
- Create: `crates/wv-app/src/ui/inspector/situations.rs`（从 `situation_panel.rs`（104 行）整文件迁）
- Modify: `crates/wv-app/src/ui/inspector/mod.rs`（两页签接真身，替换占位）
- Modify: `crates/wv-app/src/state.rs:130-142`（`Panel::ALL` 移除 `Faction` / `Situation` / `CellEditor` 三项；枚举变体与 `title`/`key`/`icon` 的 match 臂保留——pub API 无 dead-code 告警）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（删 `ui::panel_faction` / `ui::panel_situation` 注册；`ui_command_palette` 面板开关随 `Panel::ALL` 自动消失）
- Delete: `crates/wv-app/src/ui/faction_panel.rs`、`situation_panel.rs`、`cell_editor_panel.rs`（若 Task 10 未删净）；`ui/mod.rs` 摘除
- Test: `crates/wv-app/src/ui_layout.rs` tests

**Interfaces:**
- Produces: 右栏只剩 Inspector 一个窗口；存量 `ui-layout.yaml` 的 `faction` / `situation` / `cell_editor` 键成为无害冗余。

- [ ] **Step 1: 写失败测试** — `ui_layout.rs` 加 `retired_panels_absent_from_all()`（断言 `Panel::ALL` 不含 `Faction` / `Situation` / `CellEditor`）与 `stale_panel_keys_are_ignored()`（含退役键的 YAML 反序列化成功不炸——容忍性钉死）。跑 `cargo app-test retired_panels` → `retired_panels_absent_from_all` FAIL（`stale_panel_keys_are_ignored` 可能今日已绿，作为回归钉）。
- [ ] **Step 2: 迁移 + 退役**（上述 Files）。
- [ ] **Step 3: 全门禁 + 冒烟**（存量工程如 `examples/minimal-world` 的旧 ui-layout.yaml 打开不炸——冒烟默认工程即覆盖）。
- [ ] **Step 4: 实机验证 V1**：单窗口三页签全可用；顶栏面板菜单不再列三个退役项。
- [ ] **Step 5: 提交** — `wv-app: 势力层/事态收编为 Inspector 页签，三个旧面板退役`

---

### Task 13: i18n + 视觉收尾（M5）

**Files:**
- Modify: `crates/wv-app/src/i18n/keys.rs`（新键族：`InspectorTabProperties` / `InspectorTabFactionLayers` / `InspectorTabSituations`、五表单的 label 键、`InspectorSelectionMissing`、`InspectorAnchorDerived` 等，**逐键 zh/en 双表齐全**）
- Modify: `crates/wv-app/src/i18n/table.rs`（zh 段 + en 段同步）
- Modify: `crates/wv-app/src/ui/inspector/*.rs`（硬编码文案换 `t_key`；Task 4 的页签硬编码中文一并替换）

**Interfaces:**
- Consumes: 既有 `I18nKey` / `t_key` 机制（ADR-0018）。

- [ ] **Step 1: 补键**（keys.rs 加枚举项 + table.rs 双语；i18n 完整性由既有 workspace 测试把守——键表不齐会红）。
- [ ] **Step 2: 替换硬编码文案**（faction_form 原面板本就硬编码中文，spec §6.6 允许沿用——本任务统一接 i18n）。
- [ ] **Step 3: 全门禁 + 冒烟**。
- [ ] **Step 4: 实机视觉核对**（对照 `.figma-ref/inspector_node_42_2.png`：identity_header、左标签右控件、类型色点、宽 300）。
- [ ] **Step 5: 提交** — `wv-app: Inspector 全量文案接 i18n，视觉对齐 Figma 42:2`

---

### Task 14: ADR-0050 + 文档留痕（M6）

**Files:**
- Create: `docs/adr/ADR-0050-unified-inspector-panel.md`（处境 / 决策 / 理由 / 代价 + 被否决的备选；决策含：单窗口三页签、五类分派、Cell 直提、Place 锚点分流、Region 父区域不做、自由文本 Id、三面板退役；**修正 ADR-0040 §决策 6**——正文不改动 ADR-0040）
- Modify: `docs/adr/README.md` §4 索引表（追加 ADR-0050 行，关联列写「修正 ADR-0040 决策 6」）
- Modify: `docs/adr/ADR-0040-cell-inspector-tool.md` **仅头部**（状态行加一行：「决策 6 于 2026-10-22 被 ADR-0050 修正（Cell 表单并入统一 Inspector）」——按 ADR 流程头部标注取代关系，不改正文）
- Modify: `README.md`（P-4 相关描述更新：属性检查器 = 单窗口三页签）
- Modify: `docs/MAP_EDITOR_BASELINE_v1.md`（§1.11 P-4 行**追加**追记注，不改原行——AGENTS §6.3）

- [ ] **Step 1: 写 ADR**（四段齐全 + 被否决备选：拆三 system 共用窗口 id / 复活 pending 链 / Place 锚点一律可编辑）。
- [ ] **Step 2: 更新三处文档**。
- [ ] **Step 3: 全门禁末跑 + 提交** — `docs: ADR-0050 统一 Inspector 面板（修正 ADR-0040 决策 6）`

---

## 收尾（全部任务后）

- [ ] 合并回 main 前跑一遍完整门禁六件套 + `git worktree remove .worktrees/feat/unified-inspector` + 删分支（按用户当时指示）。
- [ ] 实机验收单 V1–V9（spec §10）逐项过。
