# 势力交互 M1（wv-core/wv-render/wv-terrain/wv-edit/wv-app）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现势力图画法交互 M1（ADR-0028）：作者在宿主（或空势力）中间圈意图多边形 → 选贴地/越地 → 一次划分出新势力并重生成宿主余部（可为带洞环），共边无缝、原子可撤销。

**Architecture:** 洞支持（wv-core 变体 + render earcut + query 洞感知）→ 划分内核（wv-terrain 纯函数：栅格掩膜 → 环提取 → 贴地吸附）→ `DivideFactionCommand`（单事务：新势力 + 宿主余部替换，原子 undo）→ UI（意图菜单按钮 + 划分对话框，克隆 Forest 流先例）。渲染始终只读 span（ADR-0027 D2）；空势力 = 未分配即地形透出，v1 不建实体。

**Tech Stack:** Rust 2021，**零新增依赖**（earcut 自写；marching/chain 复用 `wv-terrain::isoline` 积木）。

**Spec:** `docs/superpowers/specs/2026-10-18-world-from-zero-map-design.md` §5 + 追记一；`docs/adr/ADR-0028-faction-division-and-territory.md`（D1–D6）；GLOSSARY「划分 / 贴地·越地 / 地物线」。

## Global Constraints

- `wv-terrain` 零第三方依赖；坐标一律 `(f32, f32)`，禁 glam（AGENTS §2.1）。
- 渲染**永不推导领地**（ADR-0027 D2 / ADR-0028 D1）：mesh 只从 span 读。
- `GeometryData::PolygonWithHoles` 的 untagged 序必须居 `Polygon` 之后、`Points` 之前（serde 兼容红线）；旧 YAML 零迁移。
- 划分是编辑命令：System 只生成命令（AGENTS §3.3）；划分内核纯函数、确定性、退化输入安全返回错误不 panic（V1 精神）。
- 单事务原子性：划分要么整体成立要么整体不动（ADR-0028 D2）；余部/新势力碎块化 → `Precondition` 拒绝。
- 测试名完整英文短句；数值断言附实际值；提交 `<scope>: <动作>`。
- 中间态纪律：Task 1–7 同分支连续执行，禁止中间态合入 main；全门禁在 Task 7 收口。
- wv-render 的活全在 `--features app` 门后，门禁必须带 feature 跑。

## Review Focus

1. **洞语义三处一致**：query（洞内=不在）、render（洞不填充+洞边描边）、round-trip（untagged 往返）——Task 1/2 测试。
2. **划分的构造性共边**：新势力环与宿主余部洞沿来自**同一次掩膜边界提取**，逐点相等——Task 3 测试。
3. **原子 undo**：划分后 revert，factions/faction_geometry/宿主 span 三者逐位回到 before——Task 5 测试。
4. **贴地退化不阻塞**：无地物窗口 → 直线环 + ok 回执；有湖 → 环点吸附湖岸——Task 4 测试。
5. **既有势力测试零回归**：recon §9 列出的 wv-core/wv-io/wv-edit/wv-render 势力测试全绿——Task 7 全门禁。

---

### Task 1: wv-core 洞支持（`PolygonWithHoles` + `point_in_geometry`）

**Files:**
- Modify: `crates/wv-core/src/map/mod.rs`（`GeometryData` 变体 + `point_count`）
- Create: `crates/wv-core/src/base/geometry.rs` 内追加 `point_in_geometry`
- Modify: `crates/wv-core/src/model/faction.rs`（`FactionTerritory` 增 `geometry: &'a GeometryData` 字段与访问器；`factions_at`/`faction_territories_at` 透传）
- Modify: `crates/wv-core/src/lib.rs`（re-export `point_in_geometry`）
- Test: 上述文件测试模块

**Interfaces:**
- Produces:
  - `GeometryData::PolygonWithHoles { outer: Vec<f32>, holes: Vec<Vec<f32>> }`（untagged 序：`Polygon` → `PolygonWithHoles` → `Points` → `Ref`）
  - `pub fn point_in_geometry(geo: &GeometryData, x: f32, y: f32) -> bool`（Polygon→ring；WithHoles→outer 且不在任何 hole；Ref/Points→false）
  - `FactionTerritory::geometry(&self) -> &GeometryData`（既有 `vertices()` 不动，render 平滑迁移）
  - `query_at_point` 改走 `point_in_geometry`（洞内不再命中）
- Consumes: 既有 `point_in_ring`。

- [ ] **Step 1: 写失败测试**（faction.rs 测试模块 + map/mod.rs 测试模块）

```rust
// map/mod.rs
#[test] fn polygon_with_holes_round_trips_through_yaml() { /* serde_yaml 往返：outer+1 hole 逐点相等 */ }
#[test] fn legacy_polygon_yaml_still_loads_and_serializes_as_before() { /* 旧 "P: [..]" 形态不受新变体影响 */ }
// faction.rs
#[test] fn query_at_point_misses_points_inside_holes() { /* 外环内、洞内的点不返回该势力；洞外环间返回 */ }
#[test] fn faction_territory_exposes_geometry_for_hole_aware_render() { /* geometry() 取回带洞 span */ }
```

- [ ] **Step 2: 跑测试确认失败**（无变体/无函数，编译错误即预期失败形态）
- [ ] **Step 3: 实现**（按 Interfaces；`point_count` 对 WithHoles = outer+holes 总点数）
- [ ] **Step 4: 跑测试确认通过**：`cargo test -p wv-core && cargo clippy -p wv-core --all-targets -- -D warnings && cargo fmt --all -- --check`
- [ ] **Step 5: Commit** `wv-core: GeometryData 带洞环变体 + point_in_geometry 洞感知判定`

---

### Task 2: wv-render 洞填充与洞描边（earcut 自写）

**Files:**
- Modify: `crates/wv-render/src/faction_overlays.rs`（`build_polygon_with_holes_mesh(outer, holes) -> Mesh`；`sync_faction_overlays` 按 `territory.geometry()` 分派；边界环 = outer + 各 hole）
- Test: 同文件测试模块

**Interfaces:**
- Consumes: Task 1 的 `FactionTerritory::geometry()`。
- Produces: `pub(crate) fn earcut_with_holes(outer: &[f32], holes: &[Vec<f32>]) -> Vec<usize>`（返回外环+洞并扁平化后的索引三角化；约定：洞点序与外环相反时自动翻转）。

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn earcut_rectangles_outer_with_one_hole_covers_area_excluding_hole() { /* 逐三角形质心采样：洞心不被覆盖、环间被覆盖；索引数 = (n_outer + n_hole - 2) */ }
#[test] fn polygon_with_holes_mesh_keeps_vertex_data_intact() { /* mesh 顶点含 outer+hole 全部点 */ }
#[test] fn hole_border_ring_is_stroked() { /* polygon_edges 对 hole 环也产出边 */ }
```

- [ ] **Step 2: 确认失败**；[ ] **Step 3: 实现**（earcut 标准算法：去洞桥接 →耳剪；零依赖）；
- [ ] **Step 4**: `cargo test -p wv-render --features app && cargo clippy -p wv-render --all-targets --features app -- -D warnings && cargo fmt --all -- --check`
- [ ] **Step 5: Commit** `wv-render: 势力领地带洞填充（自写 earcut）+ 洞沿描边`

---

### Task 3: wv-terrain 划分内核·掩膜与环提取（`divide.rs` 上半）

**Files:**
- Create: `crates/wv-terrain/src/divide.rs`；Modify: `lib.rs`（`pub mod divide;`）
- Test: `divide.rs` 测试模块

**Interfaces:**
- Produces（格坐标 `(f32,f32)` = 格心，调用方换算 wu）:
  - `pub fn rasterize_ring(w: u32, h: u32, ring: &[(f32, f32)]) -> Vec<bool>`（格心 even-odd）
  - `pub struct DivisionGeometry { pub inside: Vec<Vec<(f32, f32)>>, pub remainder_outer: Vec<Vec<(f32, f32)>>, pub remainder_holes: Vec<Vec<Vec<(f32, f32)>>>, }`（ring 均为逆/顺时针一致的闭合链；`inside.len() > 1 || remainder_outer.len() > 1` = 碎块化）
  - `pub fn divide_geometry(grid_w: u32, grid_h: u32, host_outer: &[(f32, f32)], host_holes: &[Vec<(f32, f32)>], intent: &[(f32, f32)]) -> Result<DivisionGeometry, DivideError>`（`DivideError::{TooFewPoints, Fragmented{inside, remainder}}`；逐格布尔：`in_host && in_intent`→新势力、`in_host && !in_intent`→余部；环提取 = 格边有向段跟踪（掩膜边界拓扑），chaikin(2) + simplify_dp 平滑；**新势力环与余部洞沿来自同一掩膜边界 → 逐点相等**）
- Consumes: `crate::intent::point_in_polygon`（改 `pub(crate)`）、`isoline::{chaikin, simplify_dp}`。

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn square_intent_inside_square_host_yields_nested_ring_and_ring_with_hole() { /* 20² 宿主、8² 居中意图：inside 1 环、余部 1 外环 + 1 洞；两环边长差 = 2×(8+1) 格（共边同长） */ }
#[test] fn intent_touching_host_border_cuts_without_hole() { /* 意图贴宿主一边：余部无洞 */ }
#[test] fn fragmented_remainder_is_rejected() { /* 意图横切地峡：Err(Fragmented) */ }
#[test] fn intent_outside_host_is_fully_inside_rejected_or_empty() { /* 意图在宿主外：inside 空 → Err（回执「不在宿主内」） */ }
#[test] fn degenerate_inputs_error_not_panic() { /* <3 点、空宿主 */ }
```

- [ ] **Step 2–4: RED→GREEN**（门禁同前）
- [ ] **Step 5: Commit** `wv-terrain: 划分内核掩膜与环提取（同一刀出两沿）`

---

### Task 4: wv-terrain 划分内核·贴地吸附（`divide.rs` 下半）

**Files:**
- Modify: `crates/wv-terrain/src/divide.rs`
- Test: 同上

**Interfaces:**
- Produces:
  - `pub enum EdgeMode { Snap, Straight }`
  - `pub struct Features { pub water_rings: Vec<Vec<(f32, f32)>>, pub river_points: Vec<(f32, f32)>, pub ridge_points: Vec<(f32, f32)> }`（格坐标）
  - `pub fn extract_features(grid: &TerrainGrid) -> Features`（水陆界面 = `isoline::kind_segments` 于湿判定 `u8` 场 assemble；河点 = 湿格中湿邻数 ≤2 的链上格心；脊点 = 3×3 局部极大且 ≥300 m）
  - `pub fn snap_ring(ring: &[(f32, f32)], features: &Features, search: f32) -> Vec<(f32, f32)>`（逐顶点最近地物点，窗口外原样；`Straight` 由调用方跳过）
  - `pub fn divide(grid: &TerrainGrid, host_outer, host_holes, intent, mode: EdgeMode, search_cells: f32) -> Result<DivisionGeometry, DivideError>`（组合 3+4：先几何、后贴地——对 inside/余部洞沿的每个环 snap；Straight 跳过）
- Consumes: `crate::edit::is_water_cell`、`crate::isoline::{kind_segments, assemble_chains, chaikin, simplify_dp}`、Task 3 全部。

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn lake_shore_snaps_border_to_water_interface() { /* 平地 + 居中湖（apply_lake），意图跨越湖岸：贴地环至少 1 点落在湖岸格心 1 格内；Straight 模式不吸 */ }
#[test] fn no_feature_nearby_falls_back_to_straight_ring() { /* 纯平地无水无山：snap 环 == 原环（±容差），且 divide 返回 Ok（退化不阻塞，Q2） */ }
#[test] fn ridge_peak_attracts_border_vertex() { /* 单峰（Intent::MountainRange 单点）：峰顶 1 格内有吸附点 */ }
```

- [ ] **Step 2–4: RED→GREEN**
- [ ] **Step 5: Commit** `wv-terrain: 贴地吸附（水陆界/河链/山脊峰值）+ 直线退化`

---

### Task 5: wv-edit `DivideFactionCommand`

**Files:**
- Modify: `crates/wv-edit/src/command.rs`
- Test: 同文件测试模块

**Interfaces:**
- Produces:
  - `pub enum DivisionMode { Snap, Straight }`（serde 不需要——命令不落盘）
  - `pub struct DivideFactionCommand { map: Id, host: Option<Id>, intent: Vec<(f32, f32)>, mode: DivisionMode, faction: Faction, from: i64, cell_size: f32, before: Mutex<BeforeState> }`，`::new(map, host: Option<Id>, intent, mode, faction: Faction, from, cell_size)`
  - apply：宿主 span（host=None → 全图陆地，无宿主写回）→ `divide(...)` → 碎块化/不在宿主内 → `Precondition`（人话中文）；成功 = `factions.push(faction)` + `faction_geometry.push(新 span)` + 宿主 span **原位替换**（`from` 不变）；undo：移除新 faction 及其全部 geometry、宿主 span 逐位还原
  - dirty = `faction_dirty_paths()`（复用既有 `["factions","geometry/faction"]`）
- Consumes: Task 3/4 `divide`（经 `TerrainData`→`TerrainGrid` 桥，坐标 = 图内局部 wu ÷ cell_size）；既有 `UpsertFactionGeometryCommand` 的身份规则（同 `(map,owner,from,relation)` 原位替换）。

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn divide_creates_faction_and_shrinks_host_atomically() { /* 方形宿主 faction_a + 居中意图：apply 后 factions +1、a 的 span 为带洞环、新 span 与洞沿逐点相等 */ }
#[test] fn divide_undo_restores_host_and_removes_new_faction_bitwise() { /* revert 后 factions/faction_geometry == before */ }
#[test] fn divide_from_null_host_writes_no_host_span() { /* host=None：只 +1 faction +1 span */ }
#[test] fn divide_fragmenting_intent_is_precondition_error() { /* 地峡横切 → Err，世界零改动 */ }
#[test] fn divide_duplicate_faction_id_is_precondition_error() { /* */ }
```

- [ ] **Step 2–4: RED→GREEN**（`cargo test -p wv-edit` + clippy + fmt）
- [ ] **Step 5: Commit** `wv-edit: DivideFactionCommand 单事务划分（新势力+宿主余部，原子 undo）`

---

### Task 6: wv-app 划分对话框 + 意图菜单接线 + i18n

**Files:**
- Create: `crates/wv-app/src/ui/faction_divide_dialog.rs`；Modify: `crates/wv-app/src/ui/mod.rs`、`ui/intent_menu.rs`、`ui/app_bootstrap`（系统注册，`crates/wv-app/src/app_bootstrap.rs`）、`edit/faction_panel_actions.rs`（`divide_faction_now`）、`i18n/keys.rs` + `i18n/table.rs`
- Test: intent_menu 既有计数测试更新 + dialog 单测

**Interfaces:**
- Consumes: Task 5 命令；`SelectionState::consume_closed()`；`wv_core::faction::query_at_point`（宿主解析：质心命中**面积最小**的含入势力 → 宿主；无 → None=null）；`AddFeaturePlaceCommand`/`feature_place_actions` 为结构先例。
- Produces:
  - `FactionDivisionDialog { open: bool, map: Option<Id>, ring: Vec<(f32, f32)>, name: String, mode: DivisionMode, host: Option<Id> }`，`open_with(map, ring, host)`；`divide_faction_now(world, edit, view, dialog, ...) -> bool`（提交命令 + notice + `selection.cancel()`）
  - intent_menu 新按钮 **「划分势力」**（世界级；`intent_count_for` 7→8，门 = 有栅格 + 选区已闭合）；宿主解析在点击时做一次
  - i18n 新键：`IntentMenuDivideFaction`、`DivisionDialogTitle/ModeSnap/ModeStraight/NameLabel/Create/Cancel`、`DivisionNoticeCreated/NoHostTerrain/Fragmented/NotInHost/DuplicateId`
  - `from` = 当前时间轴日（实现时定位 timeline 状态资源；不可得则 0 + TODO(势力 M2)）

- [ ] **Step 1: 写失败测试**

```rust
// intent_menu.rs
#[test] fn intent_catalog_counts_include_divide_faction() { /* 世界级 8；城市级仍 0（改写既有两测试） */ }
#[test] fn host_resolution_picks_smallest_containing_faction() { /* 两国嵌套：质心 → 内国；无覆盖 → None */ }
// faction_divide_dialog.rs
#[test] fn dialog_prefills_name_and_defaults_to_snap_mode() { /* */ }
```

- [ ] **Step 2–4: RED→GREEN**（`cargo app-test` / `cargo app-clippy`）
- [ ] **Step 5: Commit** `wv-app: 划分势力意图 + 对话框（贴地/越地/命名）+ i18n`

---

### Task 7: 全门禁 + 烟测 + 文档收口

- [ ] 全门禁：`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`cargo test -p wv-app --features app`、`cargo clippy -p wv-app --all-targets --features app -- -D warnings`、`bash scripts/smoke-gui.sh`
- [ ] 手工验收路径（写进提交说明）：打开任意有地形工程 → 选择工具圈一块 → 意图菜单「划分势力」→ 命名创建 → 势力图模式（按键 3）看填色与国境 → 再在内国中间圈一块（国中国、宿主出洞）→ Ctrl+Z 逐步撤销
- [ ] Commit `docs: 势力交互 M1 收口（门禁与验收记录）`（如有零星修正）

## 收尾检查

- [ ] 依赖红线五条 `cargo tree` 断言为空（AGENTS §2.2）
- [ ] 既有势力测试（recon §9 清单）零回归
- [ ] GLOSSARY/spec/ADR 与实现一致（洞、空势力、贴地三族、直线退化）
- [ ] ADR-0028 状态经用户确认后翻「已接受」

## Execution Handoff

7 任务、强顺序链（洞 → 内核 → 命令 → UI），与 grid-base 同型——**Native 内联执行**（本会话已验证此路径两次）。后续 M2 候选：重调意图多边形的重划分入口（Q1 的 UI 面）、真分水岭山脊、余部碎块化的多环支持、空势力实体化（政治/历史段）。
