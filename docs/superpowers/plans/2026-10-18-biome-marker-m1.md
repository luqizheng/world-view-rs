# 生物群系标记 M1（wv-terrain/wv-core/wv-edit/wv-app/wv-render）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 世界生成第三段「命名」：作者圈包络 + 起名 + 选生物群系 → 系统在包络内识别或生成该群系、写格子、存入标记（Place + 生成边界），湖泊按 D14 语义派就地成水面。

**Architecture:** 内核（wv-terrain `marker.rs`：掩膜 → 识别/生成 → 环提取，复用 ADR-0028 的 `mask_boundary_rings`）→ 命令（wv-edit `PlaceBiomeMarkerCommand`：地形 + Place + 边界 span 单事务原子）→ UI（既有 `FeaturePlaceDialog` 加群系选择器与动作层，纹理脏区刷新照 `commit_region_intent` 先例）→ 渲染（既有 feature overlay 改按 `place.biome` 映射主题词汇）。

**Tech Stack:** Rust 2021，零新增依赖（复用 `isoline::{kind_segments,assemble_chains,chaikin,simplify_dp}`、`divide::{rasterize_ring, mask_boundary_rings}`）。

**Spec:** `docs/superpowers/specs/2026-10-18-world-from-zero-map-design.md` D9/D12/D13/D14/D16/D18 + 追记二；`docs/adr/ADR-0029-biome-marker.md`（D1–D6）；GLOSSARY「生物群系标记 / 识别或生成 / 标记包络」。

## Global Constraints

- `wv-terrain` 零第三方依赖；格坐标与 wu 的换算只在 wv-edit 桥做（cell_size 除法，参考 DivideFactionCommand 的 C1 教训）。
- 标记边界 = 生成快照（ADR-0029 D2）；渲染只读 span；调整 = 重圈重生成。
- 湖按 D14：包络内**陆格** → 水面、`biome=LAKE`、`height=包络中位海拔`（湖面平）；不挖碗。`RIVER` 群系不做。
- 陆群系生成**跳过水格**（水域归空势力/水，Q3 精神）；目标群系与水对应的 surface 由 `surface_for_biome` 决定。
- 识别阈值：包络内目标群系占比 ≥8% 且 ≥4 格；边界取**最大外环** + 其洞（多块时取主块，M2 再议）。
- 命令必须原子：失败不改世界；undo 逐位还原地形 + Place + 边界；脏路径含 `distances/places`、`geometry/place`、`maps/<map>/terrain.bin`。
- 旧 YAML 零迁移：Place 新字段 serde default；旧 `place.feature` 路径保留为回退。
- 测试名完整英文短句；提交 `<scope>: <动作>`；渲染/UI 任务收口跑 app 门禁与 smoke。

## Review Focus

1. **D14 湖终态**：包络内陆格全变水、`biome=LAKE`、高度=中位海拔（湖面平，不是碗）——Task 1 测试。
2. **识别不写格子**：已有森林 ≥ 阈值 → 地形逐位不变，边界=该森林连通外环——Task 1 测试。
3. **生成跳过水格**：包络含湖时，陆群系不把水格改成陆——Task 1 测试。
4. **原子 undo**：地形 + Place + 边界三者一起回滚——Task 3 测试。
5. **旧工程兼容**：`Place` 新字段缺省加载、旧 feature 回退——Task 2/5 测试。

---

### Task 1: wv-terrain 标记内核（`marker.rs`）

**Files:**
- Create: `crates/wv-terrain/src/marker.rs`；Modify: `lib.rs`（`pub mod marker;`）、`divide.rs`（`mask_boundary_rings` 与 `split_outers_and_holes` 改 `pub(crate)`）
- Test: `marker.rs` 测试模块

**Interfaces:**
- Produces:
  - `pub enum MarkerOutcome { Identified, Generated }`
  - `pub struct MarkerRegion { pub outer: Vec<(f32,f32)>, pub holes: Vec<Vec<(f32,f32)>>, pub outcome: MarkerOutcome, pub cells_changed: usize }`（格坐标）
  - `pub fn mark_region(grid: &mut TerrainGrid, envelope: &[(f32,f32)], biome: u8, cell_size: f32) -> Result<MarkerRegion, MarkerError>`（wu 包络 → 格）
  - `pub enum MarkerError { TooFewPoints, EmptyRegion }`
- Consumes: `divide::{rasterize_ring, mask_boundary_rings, split_outers_and_holes}`、`biome::{surface_for_biome, BIOME_LAKE}`、`edit::is_water_cell`。
- 识别：包络内 `cell.biome == biome` 的格数 ≥ max(4, 8% 包络格) → 不写；边界取这些格的掩膜环（最大外环 + 洞）。
- 生成：遍历包络格；目标为水（`surface_for_biome(biome)==WATER`）：陆格 → surface=WATER、height=包络内**陆格高度中位数**、biome=目标；目标为陆：只改非水格（surface=LAND、biome=目标，高度不动）；然后取掩膜环。

- [ ] **Step 1: 写失败测试**：`lake_marker_flattens_to_local_median_height`（平地 100 m + 坑洼，包络内湖 → 全水面、高度=中位、无负值挖碗）；`existing_forest_is_identified_without_touching_cells`（先铺森林块 ≥8% → 地形逐位不变、outcome=Identified）；`generate_skips_water_cells_for_land_biomes`（包络含湖，生成草原 → 水格保持水）；`degenerate_envelope_errors_not_panics`。
- [ ] **Step 2: 确认失败**（模块不存在）。
- [ ] **Step 3: 实现**（按 Interfaces；环提取复用 divide 的私有函数，改 `pub(crate)`）。
- [ ] **Step 4**: `cargo test -p wv-terrain && cargo clippy -p wv-terrain --all-targets -- -D warnings && cargo fmt --all -- --check`
- [ ] **Step 5: Commit** `wv-terrain: 生物群系标记内核（识别/生成/湖就地水面）`

---

### Task 2: wv-core Place 扩展 + 诊断群系优先

**Files:**
- Modify: `crates/wv-core/src/map/distance.rs`（Place 两字段）
- Modify: 构造点：`crates/wv-app/src/edit/feature_place_actions.rs`、`crates/wv-edit/src/command.rs`（测试夹具）、`crates/wv-app/src/ui/hover_tooltip.rs`、`crates/wv-render/src/world_projection.rs`（测试夹具）
- Modify: `crates/wv-io/src/diagnostics.rs`（biome 优先比较）
- Test: distance.rs 测试 + diagnostics 测试

**Interfaces:**
- Produces:
  - `Place { ..., #[serde(default, skip_serializing_if="Option::is_none")] pub biome: Option<u8>, #[serde(default, skip_serializing_if="Option::is_none")] pub envelope: Option<Vec<f32>> }`
  - 诊断：`place.biome` 为 Some 时比较 `cell.biome == biome`；None 时走旧 `migrate_v1_kind(feature).1`。

- [ ] **Step 1**: 测试 `place_biome_and_envelope_round_trip_through_yaml`、`legacy_place_without_biome_loads`、`feature_hint_prefers_biome_over_legacy_feature`（wv-io）。
- [ ] **Step 2–4**: RED→GREEN + 两 crate 门禁。
- [ ] **Step 5: Commit** `wv-core,wv-io: Place 生物群系标记字段 + 诊断群系优先`

---

### Task 3: wv-edit `PlaceBiomeMarkerCommand`

**Files:**
- Modify: `crates/wv-edit/src/command.rs`
- Test: 同文件

**Interfaces:**
- Consumes: Task 1 `mark_region`；既有 `to_grid/from_grid`、`place_geometry_dirty_paths`。
- Produces:
  - `pub struct PlaceBiomeMarkerCommand { pub map, pub cell_size, pub place: Place, pub envelope: Vec<(f32,f32)>, before: Mutex<BeforeMarker> }`（`place.envelope` 由此命令写入；`from = place` 由调用方给？—— span `from` 取命令字段 `day`）
  - `::new(map, cell_size, day, name, biome, envelope)`
  - apply：地形 → `mark_region`（包络按 cell_size 换格）→ 写回 grid（仅 Generated 有改动）→ Place 以生成边界的质心/bbox 调 `fit_anchor_to_ring`，`envelope` 存 wu 原值 → push Place + GeometrySpan（Polygon 外环；有洞时前端 v1 只存外环并在 `holes` 非空时记 warn——M2 再扩 `place_territory_at` 洞支持）
  - undo：移除 Place + span、逐格还原地形
  - dirty：`["distances/places", "geometry/place", "maps/<map>/terrain.bin"]`

- [ ] **Step 1**: 测试 `marker_command_generates_cells_place_and_span_atomically`（生成草原：cells 变、place.biome 正确、span==环）；`marker_command_identify_leaves_cells_bitwise`；`marker_command_undo_restores_cells_place_and_span_bitwise`；`marker_command_at_real_cell_size_uses_grid_units`（cell_size=8，包络 wu 换算）。
- [ ] **Step 2–4**: RED→GREEN + 门禁。
- [ ] **Step 5: Commit** `wv-edit: PlaceBiomeMarkerCommand 单事务标记（地形+Place+边界）`

---

### Task 4: wv-app 对话框群系选择 + 动作层 + i18n

**Files:**
- Modify: `crates/wv-app/src/ui/feature_place_dialog.rs`（biome 选择器 + 标题/按钮文案换标记口径）、`crates/wv-app/src/edit/feature_place_actions.rs`（新动作 `place_biome_marker_now` + 纹理脏区）、`crates/wv-app/src/ui/intent_menu.rs`（按钮文案「标记…」）、`i18n/keys.rs` + `table.rs`
- Test: dialog 单测 + intent 计数不变（按钮数量不变）

**Interfaces:**
- Consumes: Task 3 命令；`TerrainTexture::mark_dirty`（参考 `commit_region_intent` 的 cells diff → bbox）。
- Produces:
  - `FeaturePlaceDialog { biome: u8 }`（默认 `BIOME_FOREST`）；`open_with` 重置默认
  - `place_biome_marker_now(world, edit, playback, map, name, biome, ring, cell_size, day) -> Option<Id>`
  - i18n：`IntentMenuMarker`（替换按钮文案）、`MarkerBiomeLabel`、`FeaturePlaceDialogTitle` 改「放置标记」、19 个群系名（只做可选子集：森林/平原/草原/戈壁/沙漠/沼泽/湿地/苔原/雪原/丛林/热带雨林/丘陵/山地/绿洲/稀树草原/盐碱地/湖泊 → 17 个键；海洋/河流不做）
- 纹理刷新：动作前后 cells diff（生成时才变），`tex.mark_dirty(rect)`；识别分支不标脏。

- [ ] **Step 1**: 测试 `dialog_defaults_to_forest_biome`、`biome_catalog_has_no_ocean_or_river`、`intent_menu_still_has_eight_entries`（计数不变）。
- [ ] **Step 2–4**: RED→GREEN + `cargo app-test`/`app-clippy`。
- [ ] **Step 5: Commit** `wv-app: 标记对话框群系选择 + 动作层纹理刷新 + i18n`

---

### Task 5: wv-render 群系 → 主题覆盖物映射 + 全门禁

**Files:**
- Modify: `crates/wv-render/src/feature_overlays.rs`（`rule_for` 前先把 `place.biome` 映射成旧 feature：森林族→4，其余 None；`place.feature` 回退）
- Test: feature_overlays 单测

**Interfaces:**
- Produces: `pub(crate) fn overlay_feature_for_place(place: &Place) -> Option<u8>`（biome 森林族 → Some(4)；biome 其他 → None；无 biome → place.feature）。

- [ ] **Step 1**: 测试 `forest_biome_maps_to_legacy_feature_four`、`non_forest_biome_has_no_overlay_rule`、`legacy_feature_still_works_without_biome`。
- [ ] **Step 2–4**: RED→GREEN + `cargo app-test` / `app-clippy` / smoke。
- [ ] **Step 5: Commit** `wv-render: 标记群系映射主题覆盖物词汇`
- [ ] **Step 6**: 全门禁（workspace test/clippy/fmt + app + smoke）+ 手工验收记录（圈森林 → 生成；圈湖 → 水面平；撤销）

## 收尾检查

- [ ] 依赖红线五条 `cargo tree` 断言为空
- [ ] 旧工程加载：Place 无 biome/envelope 正常；feature overlay 仍显示
- [ ] GLOSSARY/spec/ADR 与实现一致；ADR-0029 经用户确认后转「已接受」

## Execution Handoff

6 任务，强顺序（内核 → 数据 → 命令 → UI → 渲染 → 门禁），Native 内联执行。M2 候选：标记边界逐帧派生、RIVER 线状标记、多块群系（多环）、place_territory_at 洞支持、群系词汇统一（D18 收口）。
