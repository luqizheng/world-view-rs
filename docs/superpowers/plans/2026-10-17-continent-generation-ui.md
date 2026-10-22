# 大陆生成 UI 接线（wv-edit 命令 + wv-app 工具/面板/撤销恢复）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把计划 1 的大陆配方内核接进编辑器：意图包络绘制工具、「生成大陆」属性面板（八参数）、`GenerateContinentCommand` 宏命令、撤销时被消费的圈回驻留。

**Architecture:** 三层接线——`wv-edit` 新命令（仿 `IntentTerrainCommand`，before 缓存可撤销）；`wv-render` 新增 `ContinentSketch` 会话资源 + 预览绘制（编辑会话态与 `BrushState`/测量同族）；`wv-app` 输入采集（仿 `commit_street_click` 模式）+ egui 属性面板（仿 `new_map_dialog` 的 dialog struct + ui/apply 拆分）+ 撤销恢复钩子（`editing.rs`）。

**Tech Stack:** Rust 2021 / Bevy 0.19 / egui 0.36（`#[cfg(feature = "app")]` 模块）。

**Spec:** plan v0.1 §8「2026-10-17 · 大陆生成工作流」（四项裁定：全局单一面板 / 生成即消费 / 撤销恢复圈 / 默认海洋）+ README §12-40。**前置：计划 1（内核）已合入。**

## Global Constraints

- 依赖方向（AGENTS §2.1）：`wv-render` 不得依赖 `wv-edit`/`wv-app`；命令只进 `wv-edit`；装配只在 `wv-app`。
- 改世界一律经 `EditCommand` + `History`（AGENTS §3.3）；System 不得直接改 `World`。
- 渲染层/UI 模块全在 `#[cfg(feature = "app")]` 下——**工作区默认门禁不编译它们**，必须额外跑：
  `cargo test -p wv-app --features app`、`cargo clippy -p wv-app --all-targets --features app -- -D warnings`、`cargo clippy -p wv-render --all-targets --features app -- -D warnings`、`bash scripts/smoke-gui.sh`。
- UI 文案走 i18n（ADR-0018）：新 Key 在 `i18n/keys.rs` + `table.rs` 各加 zh/en 两行；面板文案**不得**硬编码中文进 UI（开发者注释除外）。
- 模态弹窗必须 `Order::Foreground`（GLOSSARY「模态置顶」；用 `ui/widgets.rs` 的 `float_window` 工厂）。
- 命令的 `label()` 返回中文（`IntentTerrainCommand` 同款口径）。
- 提交信息 `<scope>: <动作>`。

## Review Focus

1. **无工程闸门穿透**（AGENTS 值班判据 `has_open_project`）：无工程时工具点击/生成必须早退——Task 3 测试。
2. **撤销恢复圈的边界**：撤销**别的**命令（如先画一笔再生成）不得误恢复圈；redo 再消费——Task 4 测试。
3. **生成命令撤销后网格逐格复原**（before 缓存完整性）——Task 1 测试。
4. **缺地形图时命令报 `TerrainMismatch`** 而非 panic（不替作者凭空造尺寸）——Task 1 测试。
5. **面板参数越界钳制**：滑杆保证 0–1/枚举；直填 seed 解析失败回退旧值不 panic——Task 2 代码 + 冒烟。

---

### Task 1: `GenerateContinentCommand`（wv-edit）

**Files:**
- Modify: `crates/wv-edit/src/command.rs`（`IntentTerrainCommand` 之后，~line 309 前插入）
- Modify: `crates/wv-edit/src/lib.rs`（re-export，随既有命令同款）

**Interfaces:**
- Consumes: `wv_terrain::continent::{generate_continents, ContinentParams}`（计划 1）；`command.rs` 内既有的 `TerrainData ↔ TerrainGrid` 桥（`command.rs:8-16` 一带，复用其转换函数，勿重写）。
- Produces:
  - `pub struct GenerateContinentCommand { pub map: wv_core::Id, pub envelopes: Vec<Vec<(f32, f32)>>, pub params: ContinentParams, pub seed: u64, pub cell_size: f32, before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>> }`
  - `pub fn new(map: wv_core::Id, envelopes: Vec<Vec<(f32, f32)>>, params: ContinentParams, seed: u64, cell_size: f32) -> Self`（seed **显式传入**，不 derive——骰子重roll 语义，与 `IntentTerrainCommand::new` 的差异要在 `///` 注明）
  - `impl EditCommand`：`label() -> "生成大陆"`；apply = 桥到 `TerrainGrid` → `generate_continents` → 写回 + before 缓存；undo = 回放 before。

- [ ] **Step 1: 写失败测试**（`command.rs` tests 模块，仿 IntentTerrainCommand 既有测试）
  - `apply_then_undo_restores_grid_cell_for_cell`
  - `missing_terrain_reports_mismatch_not_panic`
  - `empty_envelopes_is_a_noop_command`（apply 改 0 格，undo 也不炸）
- [ ] **Step 2: 跑测试确认失败** Run: `cargo test -p wv-edit generate_continent` → 编译失败。
- [ ] **Step 3: 实现**（before 缓存模式逐字段照抄 `IntentTerrainCommand`，含 `terrain()` 私有助手同款 `TerrainMismatch` 报错）。
- [ ] **Step 4: 验证** Run: `cargo test -p wv-edit && cargo clippy -p wv-edit --all-targets -- -D warnings`
- [ ] **Step 5: Commit** `wv-edit: GenerateContinentCommand 大陆生成宏命令`

### Task 2: 「生成大陆」属性面板（wv-app ui）

**Files:**
- Create: `crates/wv-app/src/ui/continent_dialog.rs`
- Modify: `crates/wv-app/src/ui/mod.rs`、`crates/wv-app/src/i18n/keys.rs`、`crates/wv-app/src/i18n/table.rs`

**Interfaces:**
- Consumes: `ui/widgets.rs` 的 `float_window`（Foreground 模态）；`wv_terrain::continent::{ContinentParams, ReliefLevel, IslandDensity}`。
- Produces:
  - `pub struct ContinentDialog { pub open: bool, pub params: ContinentParams, pub seed: u64, seed_text: String }` + `Default`
  - `pub fn continent_dialog(ctx: &egui::Context, dialog: &mut ContinentDialog, i18n: &I18n, on_generate: impl FnOnce(ContinentParams, u64))`——ui/apply 拆分：面板只收集参数，确认时回调（`new_map_dialog` 同款模式）。
  - i18n Key（新增，名称照此）：`continent_dialog_title`、`continent_param_seed`、`continent_param_coast_wiggle`、`continent_param_coast_detail`、`continent_param_relief`、`continent_param_forest`、`continent_param_desert`、`continent_param_shelf`、`continent_param_islands`、`continent_generate`、`continent_cancel`、`continent_tool_name`（Task 3 用）。zh/en 双语。
- 控件：seed 行 = 文本框 + 🎲 按钮（随机 u64；解析失败保持旧值）；wiggle/forest/desert = Slider 0.0–1.0（百分比显示）；coast_detail/relief/islands = 三/三/四档 `selectable_value`；shelf = Slider 0–32 格。

- [ ] **Step 1: 写失败测试** `default_dialog_params_match_ruling_and_seed_parse_falls_back`（Default = 裁决缺省；非数字 seed 输入不改变 seed）。
- [ ] **Step 2: 跑确认失败** Run: `cargo test -p wv-app --features app continent_dialog`
- [ ] **Step 3: 实现面板 + i18n Key**（挂进 ui_shell 的弹窗调用序——Task 4 接 open 触发）。
- [ ] **Step 4: 验证** Run: `cargo test -p wv-app --features app && cargo clippy -p wv-app --all-targets --features app -- -D warnings`
- [ ] **Step 5: Commit** `wv-app: 生成大陆属性面板与八参数 i18n`

### Task 3: 意图包络绘制工具 + 预览（wv-render 状态 + wv-app 输入）

**Files:**
- Modify: `crates/wv-render/src/state.rs`（新增资源）
- Create: `crates/wv-render/src/continent_sketch.rs`（预览绘制系统）
- Modify: `crates/wv-render/src/lib.rs`、`crates/wv-render/src/plugins.rs`（或编辑侧 Plugin 的注册点，执行时看 `brush_cursor` 挂在哪就挂哪）
- Modify: `crates/wv-app/src/ui/map_controls.rs`（工具条新瓷砖）、`crates/wv-app/src/state.rs`（工具枚举加变体，执行时找 `SelectBrushTool` 所在枚举）、`crates/wv-app/src/ui_shell.rs` 或输入分发处（点击采集系统）

**Interfaces:**
- Produces:
  - `#[derive(Resource, Default)] pub struct ContinentSketch { pub envelopes: Vec<Vec<(f32, f32)>>, pub active: Option<Vec<(f32, f32)>> }`（wv-render::state；世界单位）
  - `pub fn draw_continent_sketch(...)`（wv-render::continent_sketch，Gizmos 画已闭合包络白线 + 进行中折线 + 顶点；Z 高于地形，参考 `brush_cursor` 的 CURSOR_Z=0.9 层）
  - 交互（wv-app 输入系统，仿 `commit_street_click` 早退链）：左键加点；≥3 点时双击/回车闭合→`envelopes.push`；Esc 丢弃当前 `active`；无工程（`has_open_project`）一律早退。
  - 工具变体命名 `Tool::ContinentSketch`（以既有工具枚举实际名对齐）。

- [ ] **Step 1: 写失败测试**（wv-app 侧，纯逻辑部分）`sketch_clicks_close_and_escape_discard_active`——把输入采集拆成可测纯函数（点列 + 事件 → `ContinentSketch` 变更），仿 streets 采集函数的测法。
- [ ] **Step 2: 跑确认失败**
- [ ] **Step 3: 实现**三层（render 资源+绘制 / app 工具枚举+工具条瓷砖 / 输入采集接线）。
- [ ] **Step 4: 验证** Run: `cargo test -p wv-app --features app && cargo clippy -p wv-app --all-targets --features app -- -D warnings && cargo clippy -p wv-render --all-targets --features app -- -D warnings`
- [ ] **Step 5: Commit** `wv-app+wv-render: 意图包络绘制工具与预览`

### Task 4: 生成接线 + 撤销恢复圈

**Files:**
- Modify: `crates/wv-edit/src/history.rs`（若缺 `pub fn len(&self) -> usize` 则补，返回已提交条目数）
- Modify: `crates/wv-app/src/state.rs`（`ContinentGenerationStash { envelopes: Vec<Vec<(f32,f32)>>, params: ContinentParams, seed: u64, committed_len: usize }` 资源，Option 包裹）
- Modify: `crates/wv-app/src/edit/editing.rs`（undo/redo 路径，~line 447-500）
- Modify: Task 2 面板的 `on_generate` 接线处（ui_shell）

**Interfaces:**
- Consumes: `History::{commit, undo, redo, len}`；`GenerateContinentCommand`；`ContinentSketch`。
- 行为规格：
  - 生成：envelopes 非空才可点；`Transaction::new()` push 单条 `GenerateContinentCommand` → `edit.history.commit` → 成功后 `sketch.envelopes.clear()`（生成即消费）并写 stash（含 commit 后的 `history.len()`）。
  - undo：`history.undo` 成功后，若 stash 存在且 `history.len() < stash.committed_len` → `sketch.envelopes = stash.envelopes.clone()`（圈回来），**不清 stash**（redo 还要用）。
  - redo：`history.redo` 成功后，若 stash 存在且 `history.len() >= stash.committed_len` → `sketch.envelopes.clear()`（再消费）。
  - 「取消」= 面板关闭，sketch 原样保留。

- [ ] **Step 1: 写失败测试**
  - `undo_after_generate_restores_envelopes_and_grid`
  - `undo_of_other_command_does_not_restore_envelopes`（生成 → 再画一笔意图山 → undo 一次：山没了、圈**不**回来，因为 len 仍 ≥ committed_len-? —— 断言按行为规格推导的期望）
  - `redo_reconsumes_envelopes`
- [ ] **Step 2: 跑确认失败**
- [ ] **Step 3: 实现**（editing.rs 的 undo/redo 已集中在 ~line 447 的处理函数，钩子加在成功分支内）。
- [ ] **Step 4: 验证** Run: `cargo test -p wv-app --features app && cargo clippy -p wv-app --all-targets --features app -- -D warnings`
- [ ] **Step 5: Commit** `wv-app: 大陆生成接线——生成即消费与撤销恢复圈`

### Task 5: 入口收口 + 全门禁

- [ ] **Step 1: 命令面板 / 工具条入口补全**（`SelectBrushTool`/`ToggleGrid` 同款动作枚举加 `GenerateContinent` 打开面板）。
- [ ] **Step 2: 全量验证**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p wv-app --features app
cargo clippy -p wv-app --all-targets --features app -- -D warnings
bash scripts/smoke-gui.sh
```
Expected: 全绿 + 烟测 PASS（5 秒存活、日志无 panic）。

- [ ] **Step 3: 实机手验**（人工）：圈两个圈 → 生成 → 看大陆；Ctrl+Z 圈回来；改蜿蜒度再生成。可用 WV_SHOT 探针截图（见记忆 wv-app-screenshot-verification-recipe）。
- [ ] **Step 4: Commit** `wv-app: 大陆生成入口收口与烟测`
