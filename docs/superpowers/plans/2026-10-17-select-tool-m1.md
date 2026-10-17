# 选择工具 M1 实施计划（pen 方法 + 意图菜单骨架 + 生成大陆接线）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地设计稿 M1——`SelectionState` pen 方法（画/闭合/锚点编辑）、右缘工具条「选择」瓷砖、底部意图菜单（生成大陆）、大陆圈画与城市落点两瓷砖退役、`drill_pick` Drawing 守卫。

**Architecture:** `SelectionState`（会话态资源，图局部 wu 坐标）住在 wv-render，输入/Gizmos 系统同 crate；wv-app 只读选区并派发既有 `GenerateContinentCommand`（经 `commit_continent_generation`）。`ContinentSketch` 整体退役，撤销回驻留 stash 机制换目标存活。

**Tech Stack:** Rust 2021 / Bevy 0.19（Gizmos 2D）/ egui 0.36（bevy_egui）/ 无新依赖。

**Spec:** `docs/superpowers/specs/2026-10-17-select-tool-pen-intent-design.md`（§3 手势全集 / §4 目录 / §12 裁定——本计划从它出发，执行者两份都读）。

## Global Constraints

- 门槛（每任务收尾必跑）：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`；涉 wv-app UI 的任务加 `cargo test -p wv-app --features app`、`cargo clippy -p wv-app --all-targets --features app -- -D warnings`；Task 7 加 `bash scripts/smoke-gui.sh`。
- **wv-render 禁止构造 `EditCommand`、禁止依赖 wv-edit / wv-app**（AGENTS §2.1 红线）。
- 选区是**会话态**，不落盘、不进 YAML。
- 成员 crate 不写死依赖版本；中文注释、测试名完整英文短句、断言带上下文。
- 坐标口径：**当前图局部 wu**（`view.cursor_world - drill::map_origin(...)`，与 `MeasureState::draft` 同源链）。
- 工具互斥沿用 map_controls 瓷砖 toggle 家族：开选择关测量，开测量/生成大陆面板路径关选择。

## Review Focus

1. **双击收笔/删锚撞上下钻标记** → 选区被切图清空蒸发。钉子：`hit_tests_claim_click` 纯函数（Task 2）+ `drill_pick` 守卫（Task 3）。
2. **撤销生成后旧圈覆盖作者新画的选区** → 钉子：`undo_of_generation_yields_to_newly_drawn_selection`（Task 5）。
3. **删锚致 <3 点后仍处 Closed 态**（悬空选区喂内核）→ 钉子：`deleting_anchor_below_three_dissolves_back_to_draft`（Task 1）。
4. **切图后残留选区**（画到新图上/生成到错图）→ 钉子：`map_change_clears_selection`（Task 1）+ `sync_selection_map_change` 系统（Task 3）。
5. **面板开着时选区被 Esc 清掉**再点生成 → 不得 panic，走既有 `ContinentNoticeNoEnvelopes` 提示分支（Task 5 保留该 guard，spec §12 反目标）。

---

### Task 1: `SelectionState` 资源与状态机（wv-render）

**Files:**
- Modify: `crates/wv-render/src/state.rs`（`ContinentSketch` 定义旁新增；本任务**不删**它）
- Test: 同文件 `mod tests`（state.rs 现有测试风格：纯逻辑直测）

**Interfaces:**
- Produces（后续任务全靠它，签名逐字实现）:

```rust
/// 选择工具会话态（2026-10-17 选择工具 spec §3）。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct SelectionState {
    pub enabled: bool,                          // 工具瓷砖切换
    pub draft: Vec<(f32, f32)>,                 // 画中锚点（图局部 wu）
    pub closed: Option<Vec<(f32, f32)>>,        // 闭合选区（≥3 点）；Some = Closed 态
    pub selected_anchor: Option<usize>,         // Closed 态被选中锚点
    pub last_click: Option<((f32, f32), f64)>,  // 双击判定（与 ContinentSketch 同款）
    pub last_map_id: Option<wv_core::Id>,       // 切图清空（MeasureState 同款）
}

impl SelectionState {
    pub fn is_drawing(&self) -> bool;
    pub fn is_closed(&self) -> bool;
    pub fn add_point(&mut self, p: (f32, f32));
    /// ≥3 点才闭合（成功 `true`）；失败清空草稿（`ContinentSketch::try_close` 同语义）。
    pub fn try_close(&mut self) -> bool;
    pub fn discard_draft(&mut self);
    /// 全清（草稿 + 闭合 + 选中），不动 `enabled`。
    pub fn cancel(&mut self);
    /// stash 回驻留：直接落为闭合选区（替换现状）。
    pub fn close_with(&mut self, pts: Vec<(f32, f32)>);
    /// 生成即消费：取走闭合选区。
    pub fn consume_closed(&mut self) -> Option<Vec<(f32, f32)>>;
    pub fn move_anchor(&mut self, i: usize, p: (f32, f32)) -> bool; // 越界 false
    /// 删锚；剩余 <3 点时闭合**降级回画中**（closed→draft）。
    pub fn delete_anchor(&mut self, i: usize);
    /// 在顶点 `splice`（1..=len，i-1 与 i%len 之间）插入。
    pub fn insert_anchor(&mut self, splice: usize, p: (f32, f32));
    pub fn on_map_change(&mut self, new_map: Option<&wv_core::Id>); // 变了才 cancel
}
```

- [ ] **Step 1: 写失败测试**（state.rs `mod tests`，全部只调上面方法）：

```rust
#[test]
fn closed_selection_requires_three_anchors()          // 2 点 try_close=false 且草稿清空
#[test]
fn escape_discards_draft_keeps_closed_selection()     // closed 存在时 discard_draft 不动 closed
#[test]
fn cancel_clears_both_draft_and_closed()
#[test]
fn deleting_anchor_below_three_dissolves_back_to_draft() // 3 点闭环删 1 → is_drawing()==true
#[test]
fn insert_anchor_splices_between_neighbors()          // 4 点方形在 splice=2 插 → len 5 且 pts[2]==新点
#[test]
fn consume_closed_takes_ring_and_clears()
#[test]
fn map_change_clears_selection()                      // on_map_change(其它 id) 后 is_closed()==false
```

- [ ] **Step 2:** `cargo test -p wv-render selection` → FAIL（类型不存在）。
- [ ] **Step 3:** 实现 `SelectionState` + 方法（state.rs，紧邻 `ContinentSketch`，文档注释写明 spec 出处与会话态口径）。
- [ ] **Step 4:** `cargo test -p wv-render` → PASS。
- [ ] **Step 5:** Commit：`wv-render: SelectionState 选择状态机（pen 方法数据底座）`。

---

### Task 2: 命中纯函数 + `handle_click` 核心状态机（wv-render/selection.rs）

**Files:**
- Create: `crates/wv-render/src/selection.rs`（模块头注释：职责/边界——不碰 History、不落盘，spec §3）
- Modify: `crates/wv-render/src/lib.rs:46`（`pub mod continent_sketch` 旁加 `pub mod selection;`）

**Interfaces:**
- Consumes: Task 1 的 `SelectionState`。
- Produces（Task 3 系统逐字调用）:

```rust
pub const PICK_TOLERANCE_PX: f32 = 8.0;   // 锚点/边命中容差（屏幕像素；×view.scale 转 wu）
pub const CLOSE_SNAP_PX: f32 = 10.0;      // 首锚闭合吸附半径（像素）
pub const DOUBLE_CLICK_SECS: f64 = 0.4;   // 与 continent_sketch 一致
pub const DOUBLE_CLICK_WU: f32 = 12.0;

pub fn pick_anchor(pts: &[(f32, f32)], p: (f32, f32), tol: f32) -> Option<usize>;
/// 返回插入位 splice ∈ 1..=len（新点落在 pts[splice-1] 与 pts[splice % len] 之间）。
pub fn pick_edge_insertion(pts: &[(f32, f32)], p: (f32, f32), tol: f32) -> Option<usize>;
/// drill_pick 守卫（Task 3）：选择工具画中，或光标命中闭合选区锚点/边时，点击归选区。
pub fn hit_tests_claim_click(sel: &SelectionState, local: (f32, f32), tol: f32) -> bool;
/// 左键单击的核心语义（系统只做守卫与坐标转换）。`now` 秒；`tol`/`snap` 已换算成 wu。
pub fn handle_click(sel: &mut SelectionState, p: (f32, f32), now: f64, tol: f32, snap: f32);
```

`handle_click` 语义（唯一需要完整算法的地方，其余签名+测试自明）：

```text
Closed 态:
  pick_anchor 命中 i:
    与 last_click 近且间隔短（DOUBLE_CLICK 口径, 对锚点位置）→ delete_anchor(i); selected=None
    否则 selected=Some(i); last_click=Some((锚点坐标, now))
  否则 pick_edge_insertion 命中 splice → insert_anchor(splice, p); selected=Some(新位); last_click=None
  否则（空白）→ cancel(); last_click=None
Drawing 态:
  draft.len()>=3 且 dist(p, draft[0])<=snap → try_close()
  否则与 last_click 近且间隔短 → try_close()            # 双击收笔
  否则 add_point(p); last_click=Some((p, now))
```

- [ ] **Step 1: 写失败测试**（selection.rs `mod tests`）：

```rust
#[test]
fn pick_anchor_returns_index_within_tolerance()          // 距 3 wu 命中；tol=5
#[test]
fn pick_anchor_misses_beyond_tolerance()
#[test]
fn pick_edge_insertion_returns_splice_index()            // 方形(0,0)(10,0)(10,10)(0,10)，点(5,10) → splice=3
#[test]
fn pick_edge_insertion_misses_off_polygon()
#[test]
fn clicking_first_anchor_closes_draft()                  // 3 点 + 点首锚吸附内 → is_closed
#[test]
fn double_click_closes_draft()                           // 同点两次点击 → closed
#[test]
fn clicking_anchor_selects_and_double_click_deletes_it() // 先选 → 二次点击 → 删除且降级规则生效
#[test]
fn clicking_edge_inserts_and_selects_anchor()
#[test]
fn clicking_empty_cancels_closed_selection()
#[test]
fn hit_tests_claim_click_in_drawing_and_on_anchors()     // Drawing=true；Closed+锚点命中=true；Closed+远点=false
```

- [ ] **Step 2:** `cargo test -p wv-render selection` → FAIL。
- [ ] **Step 3:** 实现（`dist_point_segment` 私有 helper 用向量投影；不要引入几何依赖 crate）。
- [ ] **Step 4:** `cargo test -p wv-render` → PASS。
- [ ] **Step 5:** Commit：`wv-render: pen 选择命中判定与单击状态机`。

---

### Task 3: 输入/绘制系统 + drill 守卫 + 装配（wv-render）

**Files:**
- Modify: `crates/wv-render/src/selection.rs`（追加 4 个系统）
- Modify: `crates/wv-render/src/drill.rs:489`（`drill_pick` 加守卫）
- Modify: `crates/wv-render/src/plugins.rs:99-107`（EditorOverlayPlugin：init_resource + 系统注册，**旧 sketch 行暂留**——Task 5 才删）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（同款注册；init_resource 加 `SelectionState`，旧的不动）

**Interfaces:**
- Consumes: Task 1/2 全部；`crate::camera::{PointerOverUi, track_cursor_world}`、`crate::drill::map_origin`、`GridConfig::map_to_view`（绘制，`draw_continent_sketch` 同款）。
- Produces（app_bootstrap 注册序）:

```rust
pub fn sync_selection_map_change(drill: Res<DrillState>, projection: Res<WorldProjection>,
    mut sel: ResMut<SelectionState>);                    // sel.on_map_change(drill.current())
pub fn commit_selection_click(/* 同 commit_continent_sketch_click 参数 + mut sel */);
   // 键：Esc→closed?cancel():discard_draft；Backspace→draft.pop；Enter→try_close。
   // 左键 just_pressed → 换局部坐标 → handle_click(sel, p, now, PICK_TOLERANCE_PX*scale, CLOSE_SNAP_PX*scale)
pub fn drag_selected_anchor(buttons, view, playback, ui, grid, drill, projection, mut sel);
   // pressed(Left) 且 selected_anchor=Some → move_anchor(i, local)
pub fn draw_selection(gizmos, sel, grid, view);
   // Closed：金描边环 + 顶点圈（选中锚点更大实心）；Drawing：白折线 + 顶点 + 橡皮筋(末锚→cursor_world)
   // + ≥3 点时首尾虚线(0.35) + 光标吸附近首锚时画吸附圈。
```

drill_pick 守卫（标签命中**之前**插入）：

```rust
let sel = ...; // 新增 Res<SelectionState> 参数
if sel.enabled {
    if sel.is_drawing() { return; }
    if let Some(ring) = &sel.closed {
        // 命中锚点/边 → 点击归选区系统（标签/下钻让位，spec §3 命中优先级）
        if hit_tests_claim_click(...) 用 pick_anchor/pick_edge_insertion 判 local { return; }
    }
}
```

- [ ] **Step 1:** 实现 4 个系统（守卫链逐字照抄 `commit_continent_sketch_click:49-55`：`ui.wants_pointer || playback.playing || view.scale < 0.15 || project_dir.is_none()`）。
- [ ] **Step 2:** `drill_pick` 加守卫（新参数 `sel: Res<SelectionState>`）。
- [ ] **Step 3:** 注册：plugins.rs 与 app_bootstrap.rs 各加 `init_resource::<SelectionState>()`；两系统 `.after(camera::track_cursor_world)`（commit → drag 顺序），`sync_selection_map_change` 同链；`draw_selection` 进 gizmos 批（`draw_continent_sketch` 旁）。
- [ ] **Step 4:** `cargo test -p wv-render && cargo test -p wv-app --features app` → 全绿（旧工具未动，无行为变化）。
- [ ] **Step 5:** `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check` → 绿。
- [ ] **Step 6:** Commit：`wv-render: pen 选择输入/绘制系统与 drill 守卫`。

---

### Task 4: 工具条重排 + 城市落点工具退役（wv-app）

**Files:**
- Modify: `crates/wv-app/src/ui/map_controls.rs`（删城市瓷砖 + `CreateConfirm` 弹窗块 334-466 + 大陆圈画瓷砖；加选择瓷砖）
- Modify: `crates/wv-app/src/state.rs:44-100`（删 `CityToolMode` / `CityToolState` / `CityDraft`）
- Modify: `crates/wv-app/src/edit/editing.rs:635-760`（删 `collect_city_drag` / `commit_city_drag_on_release` 及其 `mod tests` 相关）
- Modify: `crates/wv-app/src/app_bootstrap.rs:152,225-226`（删 init_resource 与两系统注册）

**Interfaces:**
- Consumes: `wv_render::state::SelectionState`（Task 1）、`ICON_PEN`（`ui/mod.rs` 已有，pen.svg 已在 tool_icons/）。
- Produces: 新瓷砖布局 = `[测量线, 测量矩形] sep [选择, 生成大陆…] sep [网格, ＋, −, 全图]`（**8 瓷砖 2 分隔**）；map_controls 参数面变化：`city_tool: ResMut<CityToolState>` → 删、`sketch` 参数保留（Task 5 删）、新增 `selection: ResMut<wv_render::state::SelectionState>`。

选择瓷砖 toggle（互斥家族同款）：

```rust
selection.enabled = !selection.enabled;
if selection.enabled {
    measure.mode = MeasureMode::Off; measure.clear_draft();
} else {
    selection.cancel();
}
```

测量两块瓷砖的关闭分支同步补 `selection.enabled = false; selection.cancel();`。

- [ ] **Step 1:** 瓷砖重排（选择瓷砖 hint 沿本文件硬编码中文风格：`"选择：圈选区域（左键加点；双击/回车/点首锚闭合；Esc 丢弃）"`——旧提示未走 i18n 属既有债，不扩 scope）；删城市弹窗块与瓷砖。
- [ ] **Step 2:** 删 `CityToolMode`/`CityToolState`/`CityDraft`、editing.rs 城市两系统、注册、init_resource。
- [ ] **Step 3:** 更新 `toolbar_layout_sums_to_expected_total_height`：`8×34 + 7×4 + 2×8 = 316`，注释同步「2026-10-17 选择工具 M1 重排：−圈画 −城市 +选择」。
- [ ] **Step 4:** `cargo test -p wv-app --features app && cargo test --workspace` → 绿。
- [ ] **Step 5:** 三条 workspace 门槛 + 两条 app 门槛 → 绿。
- [ ] **Step 6:** Commit：`wv-app: 选择瓷砖上岗，大陆圈画/城市落点瓷砖退役（spec D5/D6）`。

---

### Task 5: 大陆生成重接线 + `ContinentSketch` 退役

**Files:**
- Modify: `crates/wv-app/src/edit/editing.rs`（`commit_continent_generation`、`sync_sketch_after_history_step`→`sync_selection_after_history_step`、`apply_history_command` 参数）
- Modify: `crates/wv-app/src/ui/continent_dialog.rs:185-241`（消费选区）
- Modify: `crates/wv-app/src/ui_shell.rs:1074-1078,1195-1211,1544,1719`（dispatcher 参数、GenerateContinent 分支、两处测试 world init）
- Modify: `crates/wv-app/src/app_bootstrap.rs:154,313-331`（init_resource 换名；sketch 注册删）
- Modify: `crates/wv-render/src/plugins.rs`（sketch 注册删）
- Modify: `crates/wv-render/src/lib.rs:46`（`pub mod continent_sketch` 删）
- Delete: `crates/wv-render/src/continent_sketch.rs`、`crates/wv-render/src/state.rs` 中 `ContinentSketch`
- Modify: `crates/wv-app/src/i18n/table.rs:458,918`（`ContinentNoticeNoEnvelopes` 双语文案改为指向选择工具）

**Interfaces:**
- Produces:

```rust
// editing.rs —— envelopes 恒为 vec![选区环]（新模型单选区）；失败时不预消费、无需回滚
pub fn commit_continent_generation(
    map: wv_core::Id, envelopes: Vec<Vec<(f32, f32)>>, params: ContinentParams, seed: u64,
    cell_size: f32, edit: &mut EditHistory, projection: &mut WorldProjection,
    tex: &mut TerrainTexture, selection: &mut SelectionState,   // ← 换型
    gen: &mut ContinentGenerationState,
) -> Result<(), String>;
// 成功：selection.cancel() + gen.stash = Some(Stash { envelopes, .. tx_seq 同旧 })

// 撤销回驻留（spec §4 裁定）：undo 且 redo_top==tx_seq → 仅当 selection 空闲（无 closed 且无草稿）
// 时 close_with(stash.envelopes[0])，否则新选区赢、gen.stash=None；
// redo 且 undo_top==tx_seq → 若 closed==stash 环则 cancel()（再消费），新选区不动。
pub fn sync_selection_after_history_step(
    redo: bool, history: &History, selection: &mut SelectionState, gen: &mut ContinentGenerationState);
```

continent_dialog_system 消费端：

```rust
let Some(ring) = selection.closed.clone() else {
    notice.error(t(loc, I18nKey::ContinentNoticeNoEnvelopes), frame); return;   // Review Focus 5
};
match commit_continent_generation(map, vec![ring], params, seed, cell_size,
    &mut edit, &mut projection, &mut tex, &mut selection, &mut gen) { ... }
```

ui_shell dispatcher：`(mut sketch, mut gen)` → `(mut selection, mut gen)`（`ResMut<wv_render::state::SelectionState>`）；`CommandId::GenerateContinent` 分支 `sketch.enabled = true` → `selection.enabled = true`（cancel 不做——面板开着允许先圈）；Undo/Redo 调用换实参。

- [ ] **Step 1: 适配 5 个既有 stash 测试**（editing.rs `mod tests`，换 `SelectionState` 断言）：

```rust
fn undo_after_generate_restores_selection_and_grid()        // 原 undo_after_generate_restores_envelopes_and_grid
fn undo_of_other_command_does_not_restore_selection()
fn redo_reconsumes_selection()
fn undo_of_generation_yields_to_newly_drawn_selection()     // 原 keeps_outlines_drawn_afterwards：生成后画新选区 → undo → 新选区保留、stash 丢弃
fn redo_of_other_command_does_not_consume_selection()
```

- [ ] **Step 2:** `cargo test -p wv-app` → 先 FAIL 后改实现（重接线按 Interfaces 逐字）。
- [ ] **Step 3:** 删 `ContinentSketch` / `continent_sketch.rs` / 两处注册 / init_resource；`ContinentNoticeNoEnvelopes` zh 改「没有圈出大陆范围：先用「选择」工具圈一块闭合区域」、en 改 "Nothing to generate: draw a closed outline with the select tool first"。
- [ ] **Step 4:** 全部六条门槛命令 → 绿。
- [ ] **Step 5:** Commit：`wv-app: 大陆生成改吃 SelectionState，ContinentSketch 退役（spec D5）`。

---

### Task 6: 意图菜单 + i18n 新键（wv-app）

**Files:**
- Create: `crates/wv-app/src/ui/intent_menu.rs`（模块头：职责/边界——只读选区、只开面板/取消，不碰 History）
- Modify: `crates/wv-app/src/ui/mod.rs`（`pub mod intent_menu;` + 导出）
- Modify: `crates/wv-app/src/i18n/keys.rs` + `table.rs`（zh ~458 区、en ~918 区，各加 3 键）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（egui pass 链 `ui::map_controls` 之后插 `ui::intent_menu`）

**Interfaces:**

```rust
/// 目录按图级过滤（spec §4 裁定：有地形栅格 ∧ 级∈{World,Region}；M1 仅大陆一项）。
fn intent_count_for(level: wv_core::map::MapLevel) -> usize;   // World|Region → 1，其余 0
pub fn intent_menu(
    contexts: EguiContexts, shell_cfg: Res<UiShellConfig>, current_loc: Res<CurrentLocale>,
    drill: Res<DrillState>, projection: Res<WorldProjection>,
    selection: Res<wv_render::state::SelectionState>,
    mut continent_dialog: ResMut<crate::ui::ContinentDialog>,
    startup_req: Res<StartupLoadRequest>,
);
```

渲染：`selection.is_closed()` 才画；`egui::Area::new(Id::new("wv_intent_menu_v1")).anchor(Align2::BOTTOM_CENTER, (-18.0, 0.0))`——**注意**：底部居中锚点 y=0 贴底，x 用 -0?  用 `vec2(0.0, -18.0)`（距底 18px）；Frame 复用 map_controls 的 panel_bg/描边/圆角/投影规格。内容：`intent_count_for(level)==0` → `t(IntentMenuNoIntents)` 弱化文本；否则按钮 `t(IntentMenuGenerateContinent)` → `continent_dialog.open = true`，旁弱化提示 `t(IntentMenuCancelHint)`。无工程/启动加载早返（同 map_controls:92）。

i18n 三键（zh / en）：

```text
IntentMenuGenerateContinent => "生成大陆…" / "Generate continent…"
IntentMenuCancelHint        => "Esc 取消选择" / "Esc to cancel selection"
IntentMenuNoIntents         => "本图暂无可用意图" / "No intents available on this map"
```

- [ ] **Step 1: 失败测试**（intent_menu.rs `mod tests`）：

```rust
#[test]
fn intent_catalog_empty_for_city_level()      // City→0、Interior→0
#[test]
fn intent_catalog_nonempty_for_world_and_region() // World→1、Region→1（M1 仅大陆）
```

- [ ] **Step 2:** FAIL → 实现 `intent_count_for`。
- [ ] **Step 3:** 实现 `intent_menu` 系统 + i18n 三键（enum 两个表全加，漏表编译失败会被 match 兜住）。
- [ ] **Step 4:** app_bootstrap egui 链注册（`ui::map_controls` 之后；链内 `Res`/`ResMut<SelectionState>` 相邻系统已 `.chain()` 保证序）。
- [ ] **Step 5:** 六条门槛 → 绿。
- [ ] **Step 6:** Commit：`wv-app: 选区意图菜单骨架（生成大陆入口 + 图级目录谓词）`。

---

### Task 7: 烟测 + ADR + spec 追记

**Files:**
- Create: `docs/adr/ADR-0025-selection-first-tooling.md`（编号前先 `ls docs/adr/` 核实 0025 未被占用；四件事：处境/决策/理由/代价+被否备选——决策=选择优先模型、pen 方法共享、两瓷砖退役、城市 portal 收敛；被否=zoom-through 跨图、多边形即城市轮廓）
- Modify: `docs/superpowers/specs/2026-10-17-select-tool-pen-intent-design.md`（文末追记：M1 完成情况 + 两处勘误——①§3「拖拽=相机平移」错误，平移是右/中键（`camera_pan` 缺省），左键拖=移锚；②Gizmos 无多边形填充原语，闭合区先只有描边+顶点，填充延后）
- GLOSSARY 已有词条不动。

- [ ] **Step 1:** `bash scripts/smoke-gui.sh` → 进程 5s 存活 + 日志无 panic/B000x（新增系统与资源装配，§7.1 必跑）。
- [ ] **Step 2:** 六条门槛命令最后过一遍 → 全绿。
- [ ] **Step 3:** 写 ADR-0025（对照 `docs/adr/README.md` 流程；索引里登记）。
- [ ] **Step 4:** spec 追记两处勘误 + M1 段完成注记。
- [ ] **Step 5:** Commit：`docs: ADR-0025 选择优先工具架构 + 选择工具 M1 追记`。

---

## Self-Review 记录

- Spec 覆盖：§3 手势全集 → Task 1-3；§4 目录/大陆行 → Task 5-6；§5 城市 → **不在 M1**（M2）；验收 1/2/3/7/8/9 → Task 1-6，验收 4/5/6 属 M2/M3。§12 六裁定 → Task 2（命中优先级）、Task 5（回驻留/谓词留 M2 用）、Task 4（瓷砖退役）。
- 类型一致：`SelectionState` 字段/方法名在 Task 1 定义后，2/3/4/5 引用处逐字核对过；`sync_selection_after_history_step`/`commit_continent_generation` 签名 Task 5 内自洽。
- Review Focus 五条各有钉子测试（见各 Task）。
- 比例：计划 ≈ spec 的 1.2 倍，代码块只含签名/测试断言/`handle_click` 算法（签名与测试定不了的唯一算法）。
