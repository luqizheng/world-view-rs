# 河流与道路=线段 POI（ADR-0070）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 群系 16/20 退役，河/路改为 `Place.line` 线段 POI（钉起终点 + 绕行/下切算法 + 河流和道路面板），道路全量统一为线（边由线推导、存量链式合并迁移）。

**Architecture:** 数据模型在 wv-core（LineKind / 新字段 / derived 边标记）；路径与雕刻算法在 wv-terrain 新模块 `line_path`；编辑命令在 wv-edit 新模块 `command/line_poi`；渲染侧 wv-render 删 seed 派生管线、两池收单池；交互/面板/迁移接线在 wv-app。推导边为运行时投影（serde skip），行程消费者零改动。

**Tech Stack:** Rust 2021（rust-version 1.80）、Bevy 0.19 + egui 0.36（wv-app/wv-render）、零新依赖。

**Spec:** `docs/superpowers/specs/2026-10-26-river-road-poi-design.md`（ADR-0070 已接受；计划与 spec 同行，执行者两者都读）

## Global Constraints

- **动工前置（AGENTS §13）**：先 `git branch feat/river-road-poi`，再 `git worktree add .worktrees/feat/river-road-poi feat/river-road-poi`（目录名=分支名），主 checkout 不改码。构建目录沿用全局 `CARGO_TARGET_DIR`（2026-10-25 裁定，勿在 worktree 覆盖）。
- **依赖红线**：wv-core / wv-terrain 零 Bevy、零网络；wv-render 不依赖 wv-edit/wv-app；本计划**零新第三方依赖**；成员 crate 不写死版本号。
- **门禁**：每任务收尾跑 `cargo fmt --all` + `bash scripts/cargo-test.sh -p <crate>`（TEMP 指向工作区，绕开受限环境 os error 5）；涉渲染/UI 的任务另跑 `cargo app-test`、`cargo app-clippy`；最终任务跑全量四门 + `bash scripts/smoke-gui.sh`。
- **serde 纪律**：新字段一律 `#[serde(default, skip_serializing_if = "Option::is_none")]`；round-trip 测试须断言**缺省不落盘**（同 `Place.kind` 先例 `place_without_kind_round_trips_without_emitting_the_field`）。
- **命令纪律**：改动世界一律 `EditCommand`（`apply` 返回脏路径、`revert` 快照回退，`Mutex<Option<_>>` before 缓存同款）；多命令一撤销用 `wv_edit::Transaction` + `history.commit`（`apply_regen_world` 先例，`crates/wv-app/src/edit/editing/terrain.rs:188`）。
- **i18n**：新文案 = `I18nKey` 变体 + `ALL` 数组**手动 +1**（`crates/wv-app/src/i18n/keys.rs:1302`，当前 518）+ zh/en 两表。
- **图标**：仅 Lucide，`route` SVG 落 `crates/wv-ui/assets/ui/tool_icons/route.svg`，描边烘焙 `stroke="#ffffff"`。
- **数值断言附上下文**（AGENTS §7.4）；测试名用完整英文短句。

## Review Focus

1. **旧工程涂格 16/20 的迁移幂等**——加载自动迁移后，撤销不保存再加载，不得重复造实体（T7 `re_running_migration_after_undo_produces_no_duplicates` / T9 接线测试）。
2. **`unit_per_km = None` 的地图**——不得推导边、不得 panic；宽度按 wu 解释（T5 `maps_without_unit_per_km_skip_derivation`、T11 面板单位标注）。
3. **手写 YAML 道路边与推导边共存**——同端点对手写边存在时不重复推导；保存后手写边仍在、推导边不落盘（T5 `existing_hand_edge_suppresses_derivation`、T2 round-trip）。
4. **线起终点退化输入**——同点/越图外/单点：算法层钳制或空返回，命令层 `Precondition` 拒绝（T3 `identical_endpoints_return_empty`、T6）。
5. **撤销跨越地形下切**——CreateLinePoi 撤销须同时回退 Place 与地形格；改道后旧下切保留（T6 `undo_restores_both_place_and_carved_cells`、T12 `rerouting_keeps_previous_carve`）。

---

### Task 1: wv-core——`LineKind` 与 `Place` 线段字段

**Files:**
- Modify: `crates/wv-core/src/map/distance.rs`（`Place` 结构体，~line 223）
- Test: 同文件 `#[cfg(test)]`（round-trip 先例在同文件）

**Interfaces:**
- Produces: `pub enum LineKind { River, Road }`（`#[serde(rename_all = "snake_case")]`，置于 `PoiKind` 旁）；`Place.line_kind: Option<LineKind>`、`Place.line_width_m: Option<f32>`（serde 缺省不落盘）。后续所有任务的判别口径 =「`line.is_some() ∧ line_kind == Some(Road)` → 路；`line.is_some()` 其余 → 河」。

- [ ] **Step 1: 写失败测试**（三条，仿 `place_kind_round_trips_as_snake_case` / `place_without_kind_round_trips_without_emitting_the_field`）：`line_kind_round_trips_as_snake_case`（YAML 含 `line_kind: road`）、`place_without_line_fields_round_trips_without_emitting_them`（断言 YAML 不含 `line_kind`/`line_width_m`）、`legacy_line_place_defaults_to_river_semantics`（旧 `line` 无 `line_kind` 的 Place `line_kind` 读入为 `None`）。
- [ ] **Step 2: 跑测试确认失败**：`bash scripts/cargo-test.sh -p wv-core`，期望编译错（字段不存在）。
- [ ] **Step 3: 实现字段**——`LineKind` + 两字段加在 `kind` 之后，注释引 ADR-0070 D2 与「缺省=River」兼容语义。
- [ ] **Step 4: 跑测试确认通过**；`cargo clippy -p wv-core --all-targets -- -D warnings`。
- [ ] **Step 5: Commit**：`wv-core: 线段 POI 字段——LineKind 与 line_width_m（ADR-0070）`

### Task 2: wv-core——`PlaceEdge.derived` 运行时标记

**Files:**
- Modify: `crates/wv-core/src/map/distance.rs`（`PlaceEdge`，~line 71）
- Test: 同文件测试区

**Interfaces:**
- Produces: `PlaceEdge.derived: bool`，`#[serde(skip)]` + `Default`（false）。文档注释钉死：推导边**永不落盘**；保存/加载天然往返；`terrain = "road"` 仍是手写边的合法值（YAML 作者可写）。

- [ ] **Step 1: 写失败测试**：`derived_edge_serializes_without_marker_and_loads_false`（构造 `derived: true` 的边 → serde YAML 断言无 `derived` 字样 → 反序列化回 `derived == false`）。
- [ ] **Step 2: 确认失败**（编译错）。
- [ ] **Step 3: 实现字段 + 注释**。
- [ ] **Step 4: 通过 + clippy**。
- [ ] **Step 5: Commit**：`wv-core: PlaceEdge 增 serde-skip derived 标记（ADR-0070 D5）`

### Task 3: wv-terrain——`line_path::plan_polyline`（A* 两模式）

**Files:**
- Create: `crates/wv-terrain/src/line_path.rs`；Modify: `crates/wv-terrain/src/lib.rs`（`pub mod line_path;`）

**Interfaces:**
- Produces: `pub enum LinePathMode { Route, Carve }`；`pub fn plan_polyline(grid: &TerrainGrid, cell_size: f32, start: (f32, f32), end: (f32, f32), mode: LinePathMode, seed: u64) -> Vec<(f32, f32)>`（**图内局部 wu** 坐标；空 Vec = 不可达/退化输入）。
- Consumes: `TerrainGrid::{width, height, get, cell 中心换算}`。

- [ ] **Step 1: 写失败测试**（模块内，手造网格）：`routing_mode_detours_around_ridge`（10×10 网格、中部竖山脊高 2000m、两侧 50m；断言 Route 路径任一点不落山脊列，且路径长度 > 直线距离 ×1.3，附实际长度）、`carve_mode_takes_near_direct_path`（同网格 Carve 路径长度 < 直线 ×1.15）、`identical_endpoints_return_empty`、`out_of_bounds_points_return_empty`、`same_seed_same_path`（同 seed 两次结果逐点相等）。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**——格坐标换算（wu→cell）；A* 8 邻域，代价 = 步长 ×（1 + `LAMBDA_UP=8.0` × max(0,Δh) + 0.1 × max(0,-Δh)）+ 蜿蜒项 `noise_cost = 0.3 × cell_size × splitmix64(seed, cell) 归一抖动`（Route）；Carve 仅 0.2 × max(0,Δh)（弱坡度权重）。回溯格心 → Chaikin 平滑 2 轮 → wu。
- [ ] **Step 4: 通过 + clippy**（wv-terrain 零 Bevy 断言：`cargo tree -p wv-terrain | grep -i bevy` 为空）。
- [ ] **Step 5: Commit**：`wv-terrain: line_path 起终点路径算法——绕行/近直连两模式（ADR-0070 D3）`

### Task 4: wv-terrain——`line_path::carve_corridor`（河谷/道路走廊下切）

**Files:**
- Modify: `crates/wv-terrain/src/line_path.rs`

**Interfaces:**
- Produces: `pub fn carve_corridor(grid: &mut TerrainGrid, pts: &[(f32, f32)], strength: f32, cell_size: f32, width_cells: u32, mark_water: bool) -> usize`（返回改动格数）。`mark_water=true` = 河（写 `SURFACE_WATER`）；false = 路（走廊压平不写水面）。
- Consumes: `intent.rs` 既有 `apply_river`（私有，`width_cells == 1 && mark_water` 时内部直接委托，保证行为逐位不变）。

- [ ] **Step 1: 写失败测试**：`river_carve_descends_monotonically_to_mouth`（平地 + 中段鼓包网格，断言沿链高度严格递减、末段 ≥1m，附实际值）、`road_carve_flattens_without_water_surface`（断言走廊格高度下降且 `surface != SURFACE_WATER`）、`wide_corridor_touches_flanking_cells`（width_cells=3 断言中轴左右各 1 格被改）、`width_one_river_matches_legacy_apply_river`（同输入下与 `apply_intent(Intent::River)` 结果逐格相等）。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**——`apply_river` 泛化：主链密采样（同款 step=cell_size/3）；宽度膨胀 = 对链上每格向法线两侧补 `width_cells-1` 格；每格取「两岸最低 − CUT·s、上游 − DROP」下切（常量复用 `RIVER_CUT_M`/`RIVER_DROP_M`）；`mark_water` 决定 surface 写入。
- [ ] **Step 4: 通过 + clippy**。
- [ ] **Step 5: Commit**：`wv-terrain: carve_corridor——河/路走廊下切与宽度推广（ADR-0070 D3）`

### Task 5: wv-core——推导边（线即路网核心）

**Files:**
- Create: `crates/wv-core/src/map/road_derive.rs`；Modify: `crates/wv-core/src/map/mod.rs`（`pub mod road_derive;`）
- Test: `crates/wv-core/src/map/road_derive.rs` 测试区

**Interfaces:**
- Produces:
  - `pub const ROAD_SNAP_TOLERANCE_WU: f32 = 32.0;`
  - `pub fn derive_road_edges(world: &World, tolerance_wu: f32) -> Vec<PlaceEdge>`——遍历 `line_kind == Road ∧ line.is_some() ∧ map 锚定` 的 Place；线两端各找容差内最近锚定 Place（非线 POI）；两端命中 → 一条边 `{ id: "road_<a>_<b>"（字典序）, distance_km: 线长 wu ÷ unit_per_km, terrain: "road", bidirectional: true, rank: None, derived: true }`。`unit_per_km = None` / 端点未命中 → 跳过并计入返回的诊断信息（`pub struct RoadDeriveReport { pub edges: Vec<PlaceEdge>, pub skipped_no_scale: usize, pub skipped_no_anchor: usize }`，函数改返此结构）。
  - `pub fn rebuild_derived_edges(world: &mut World, tolerance_wu: f32) -> RoadDeriveReport`——`edges.retain(|e| !e.derived)` 后追加新推导边（**不进脏路径**：调用方不得因 derived 变化标脏 `distances/edges`）。
  - `pub fn road_line_effective_rank(place: &Place, distance_km: Option<f32>) -> u8`——显式 rank 赢；否则按公里折算 **≥100→3 / ≥30→2 / 否则 1**（与 `PlaceEdge::effective_rank` 同表同注释，冻结契约；`distance_km = None`（无比例尺）→ 1）。
- Consumes: `MapNode::km_per_wu`（`crates/wv-core/src/map/mod.rs:462` 一带的换算入口）、Task 1/2 字段。

- [ ] **Step 1: 写失败测试**（手算期望值）：`derived_distance_km_uses_line_length_over_unit_per_km`（线长 100 wu、`unit_per_km = 8.0` → 断言 `distance_km ≈ 12.5`，附实际值）、`derived_rank_folds_by_kilometres_like_place_edge`（100km 线 → 3；30km → 2；5km → 1）、`maps_without_unit_per_km_skip_derivation`（report.skipped_no_scale == 1，edges 空）、`existing_hand_edge_suppresses_derivation`（同端点对手写边在 → 不产推导边）、`line_endpoint_snaps_within_tolerance_only`（容差内/外两例）。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**（纯函数，无 IO）。
- [ ] **Step 4: 通过 + clippy**。
- [ ] **Step 5: Commit**：`wv-core: 线即路网——derive_road_edges 与公里折算（ADR-0070 D5）`

### Task 6: wv-edit——`CreateLinePoiCommand` / `UpdateLinePoiCommand`

**Files:**
- Create: `crates/wv-edit/src/command/line_poi.rs`；Modify: `crates/wv-edit/src/command.rs`（`mod line_poi; pub use line_poi::*;`）

**Interfaces:**
- Produces:
  - `pub struct CreateLinePoiCommand { pub map: Id, pub place: Place, pub carve: Option<CarveSpec>, cell_size: f32, before_cells: Mutex<Option<Vec<TerrainCell>>>, before_terrain: Mutex<Option<(Vec<TerrainCell>, /*width,height*/ (u32,u32))>> }`——实际字段私有化裁量归实现，**对外构造器必须可从（map, place, carve, cell_size）一步构造**。`pub struct CarveSpec { pub pts: Vec<(f32,f32)>, pub strength: f32, pub width_cells: u32, pub mark_water: bool }`。apply：`AddPlace` 语义 + carve 时 `carve_corridor`；脏路径 = `["distances/places", "terrain/<map>"]`（对齐 `IntentTerrainCommand` 的地形脏路径拼法）。revert：删 Place + 回放地形快照。
  - `pub struct UpdateLinePoiCommand { pub id: Id, pub new_name: Option<String>, pub new_width_m: Option<f32>, pub new_rank: Option<u8> }`——`None` = 不改（`UpdatePlaceCommand` 同语义）；`line` 本身的改动继续走既有 `UpdatePlaceLineCommand`。
- Consumes: Task 4 `carve_corridor`、既有 `AddPlaceCommand` 脏路径 `"distances/places"`。

- [ ] **Step 1: 写失败测试**（`command_tests` 邻近）：`create_line_poi_adds_place_and_carves_in_one_command`、`undo_restores_both_place_and_carved_cells`（undo 后 places 无该 ID 且地形逐格等于快照）、`create_rejects_degenerate_line`（`line.len() < 4` → `Err(Precondition)`）、`update_line_poi_none_leaves_field_untouched`。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**（快照模式仿 `IntentTerrainCommand`，`crates/wv-edit/src/command/terrain.rs:150`）。
- [ ] **Step 4: 通过 + clippy**。
- [ ] **Step 5: Commit**：`wv-edit: 线段 POI 创建/属性命令——一次撤销含下切（ADR-0070）`

### Task 7: wv-edit——`MigrateLegacyLineDataCommand`（存量迁移）

**Files:**
- Modify: `crates/wv-edit/src/command/line_poi.rs`

**Interfaces:**
- Produces: `pub struct MigrateLegacyLineDataCommand { /* before 快照：cells 分图、被删边、被加 Place */ }`，`MigrateLegacyLineDataCommand::new() -> Self`；`pub fn needs_line_migration(world: &World) -> bool`（存在 biome==16/20 的格，或 `terrain=="road"` 的边，或 `meta.template.rivers_seed` 存在而根图无河线 POI）。apply 五件事：① 全部地形格 biome 16 → `SURFACE_WATER + BIOME_NONE`、20 → `BIOME_NONE`（height 不动）；② 道路边链式合并（仅穿 2 度顶点；≥3 度路口与端点为线端点）→ 每链一个 `Place { id: "road_mig_<端点A>_<端点B>", line_kind: Road, line: 链折线 wu, rank: 链上原边最高 effective_rank }`；③ 从 `world.distances.edges` 删除这些道路边；④ **模板派生河转实体**（spec §4.6）：`meta.template.rivers_seed` 存在时 `wv_terrain::climate::generate_rivers` 产折线 → 每条一个 `Place { id: "river_tpl_<序号>", line_kind: River }`（无下切）——此后加载不再派生；⑤ 返回脏路径（全部 `terrain/<map>` + `distances/places` + `distances/edges`）。链 ID 确定性：端点 ID 字典序 + 链折线坐标哈希（splitmix64，`wv-terrain/src/hash.rs` 同款手法）。
- Consumes: `PlaceEdge::effective_rank`、`wv_terrain::climate::generate_rivers`（wv-edit 依赖 wv-terrain 合法，AGENTS §2.1）。

- [ ] **Step 1: 写失败测试**（手算 fixture）：`chain_merge_preserves_topology`（十字路口网 5 节点 4 边 → 4 条线，端点恰为 5 节点中度数≠2 者，边删除后 `derive_road_edges`（Task 5）恢复 4 条边且 distance_km 之和 = 原 4 边之和 ±10%）、`painted_river_cells_become_surface_water`、`painted_road_cells_become_biome_none`、`migration_undo_restores_original_cells_and_edges`、`re_running_migration_after_undo_produces_no_duplicates`（apply→revert→再 apply，Place 数与首跑相等）、`sea_route_edges_untouched`（`terrain=="sea"` 边原样）、`template_rivers_land_as_entities_on_first_load`（带 `rivers_seed` 夹具 → 河线 Place 落盘计数 ≥1，且二次 apply 不增殖）。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**——链合并：建邻接表（road 边）→ 从度数≠2 节点 DFS 穿度数 2 节点成链；坐标取边两端 Place 锚点连线（2 度节点即中间折点）。
- [ ] **Step 4: 通过 + clippy**。
- [ ] **Step 5: Commit**：`wv-edit: 存量迁移命令——涂格转换与道路边链式合并（ADR-0070 D5）`

### Task 8: wv-render——渲染侧单池化与锥形宽

**Files:**
- Modify: `crates/wv-render/src/world_projection.rs`（删 seed 派生块 378-411 与 `river_key`/`current_for_river` 管线；`RiverLine` 增 `width_wu: f32`；`author_river_polylines` 改名/合并为 `river_polylines` 单池，来源=全部河线 Place；道路段来源改为路 POI 线）
- Modify: `crates/wv-render/src/vector_layers.rs`（两河池合一；锥形宽：逐段宽 = `width_wu × lerp(0.3, 1.0, 弧长占比)`；路池消费线段）
- Test: `crates/wv-render/src/world_projection.rs` 测试区（app-test 跑）

**Interfaces:**
- Produces: `pub struct RiverLine { pub rank: u8, pub pts: Vec<Vec2>, pub width_wu: f32 }`（width_wu==0 → 旧 rank 缺省宽，兼容测试夹具）；`frame.river_polylines` 为唯一河池（原 `author_river_polylines` 字段删除，调用点全改）；`frame.road_segments` 供给源 = 路 POI 线（每线 `road_line_effective_rank` + 折线切段）。
- Consumes: Task 1 `line_kind`、Task 5 `road_line_effective_rank`、`river_rank_from_length_frac`（河 rank 沿用 ADR-0047 D5）。

- [ ] **Step 1: 写失败测试**：`river_pool_sources_from_line_places_only`（无模板 world、两条河线 Place → 2 条 RiverLine；`meta.template` 存在也不派生）、`river_taper_reaches_thirty_percent_at_source`（首段宽/末段宽 ≈ 0.3 ±0.05，附实际值）、`road_segments_source_from_road_line_places`（1 条路线 Place → 段数 = 点数-1，rank 正确）、`legacy_edge_only_world_renders_no_roads`（只有手写边的 world → road_segments 空）。
- [ ] **Step 2: 确认失败**（`cargo app-test` 跑：`bash scripts/cargo-test.sh -p wv-render --features app -- --nocapture` 或别名）。
- [ ] **Step 3: 实现**——删 `climate::generate_rivers` 渲染消费（`wv-terrain` 的该函数保留，Task 12 生成侧仍用）；河池收集沿原 `author_river_polylines` 路径（`place_index.anchored_by_map`），`line_kind==Road` 排除；`line_width_m` → wu 换算走 map 比例尺（无比例尺 = 按 wu 原值）。
- [ ] **Step 4: 通过 + `cargo app-clippy`**。
- [ ] **Step 5: Commit**：`wv-render: 河路渲染单池化——删 seed 派生、锥形宽、路=线 POI（ADR-0070）`

### Task 9: wv-app——加载期迁移接线与推导边重建

**Files:**
- Modify: `crates/wv-app/src/ui_shell/project_load.rs`（世界装载完成后：`needs_line_migration` → `MigrateLegacyLineDataCommand` 经 `Transaction` commit 进 `EditHistory` + notice 列计数）
- Modify: `crates/wv-app/src/edit/`（编辑提交后钩子：凡脏路径含 `distances/places` 或迁移后 → `rebuild_derived_edges(&mut projection.world, ROAD_SNAP_TOLERANCE_WU)`；加载完成后同样重建一次）
- Modify: `crates/wv-app/src/edit/diagnostics_bridge.rs`（迁移计数与 `skipped_no_scale` 进诊断，V1 只提示）
- Test: `crates/wv-app/src/ui_shell/tests.rs` 或邻近测试模块

**Interfaces:**
- Produces: `pub fn sync_derived_road_edges(world: &mut World) -> RoadDeriveReport`（wv-app 侧唯一入口，内部 `rebuild_derived_edges` + 诊断聚合）；调用点三处：load 完成、每次 `history.commit` 之后（含 undo/redo）、面板写回。
- Consumes: Task 5/7。

- [ ] **Step 1: 写失败测试**：`loading_legacy_world_migrates_and_reports_counts`（夹具世界带涂格 16/20 与 road 边）、`undo_migration_then_reload_does_not_duplicate`（undo → 构造重载 → Place 数不变）、`derived_edges_are_rebuilt_after_place_edits`（加路 POI → world.distances.edges 含 derived 边；**保存路径不含它**——`save_world` 后 YAML 无新行）。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**。注意迁移命令进 History 的通道用 `Transaction`（与 `apply_regen_world` 同款）；加载在后台线程——迁移在 `WorldProjection` 装配后主线程首帧执行。
- [ ] **Step 4: 通过 + app-clippy**。
- [ ] **Step 5: Commit**：`wv-app: 加载期迁移接线与推导边重建（ADR-0070 D5）`

### Task 10: wv-app——线段 POI 工具入口（工具条 + 分裂按钮 + i18n）

**Files:**
- Modify: `crates/wv-app/src/ui_command_palette.rs`（`MapToolId::LinePoi(wv_core::distance::LineKind)` 变体 + 命令注册表条目）
- Modify: `crates/wv-app/src/ui_shell/dispatch.rs`（`SelectMapTool(LinePoi(kind))` 分支：置 `LinePoiState` + 开 `Panel::LinePoi`）
- Modify: `crates/wv-app/src/ui/map_controls/widgets.rs` + `toolbar.rs`（新 `line_poi_split` 分裂按钮：主键=河，菜单=河/路；**仅 `lod_tier(view.scale) == LodTier::Edit` 渲染**）
- Create: `crates/wv-ui/assets/ui/tool_icons/route.svg`（Lucide route，白描边）
- Modify: `crates/wv-app/src/i18n/keys.rs` + `table.rs`（`LinePoiTool`、`LinePoiKindRiver`、`LinePoiKindRoad`、`LinePoiPanelTitle`、`LinePoiName`、`LinePoiWidth`、`LinePoiWidthUnitWu`、`LinePoiMode`、`LinePoiModeRoute`、`LinePoiModeCarve`、`LinePoiRank` —— `ALL` 518→529）
- Test: `crates/wv-app/src/ui/map_controls/` 测试区（Popup id 稳定先例见 ADR-0043 坑记录）

**Interfaces:**
- Produces: `MapToolId::LinePoi(LineKind)`；`Panel::LinePoi`（`crates/wv-app/src/state.rs` Panel 枚举 + PanelState open 集合）；**空壳 `pub struct LinePoiState { pub tool_active: bool, pub kind: LineKind }`**（state.rs，Task 11 在此扩展 mode/width/name/pending/selected 字段——本任务先立最小形态让 dispatch 可写）。
- Consumes: Task 1 `LineKind`。

- [ ] **Step 1: 写失败测试**：`line_poi_tile_hidden_outside_edit_tier`（Nation/Region 档 UI 不出现——沿用 ADR-0043 弹层测试手法的状态断言）、`line_poi_split_menu_switches_kind`、`i18n_all_length_matches_key_count`（529）。
- [ ] **Step 2: 确认失败**（app-test）。
- [ ] **Step 3: 实现**（分裂按钮仿 `selection_split`，`widgets.rs:16`；图标 `egui::include_image!` 路径仿 `wv-ui primitives.rs` 用法）。
- [ ] **Step 4: 通过 + app-clippy**。
- [ ] **Step 5: Commit**：`wv-app: 线段 POI 工具条入口——分裂按钮与 Edit 档门控（ADR-0070 D4）`

### Task 11: wv-app——钉点交互与河流和道路面板

**Files:**
- Create: `crates/wv-app/src/ui/line_poi_panel.rs`（面板：河/路两段；新建态=当前工具设置，选中态=命中线 POI 的属性）；Modify: `crates/wv-app/src/ui/mod.rs`（挂载）
- Modify: `crates/wv-app/src/state.rs`（**扩展** Task 10 的 `LinePoiState` 为 `{ tool_active, kind, mode: wv_terrain::line_path::LinePathMode, width_m: f32, name: String, pending: Option<PendingLinePoi>, selected: Option<Id> }`；`PendingLinePoi { pub start: Vec2, pub preview: Vec<Vec2>, pub end_cell: (i32,i32) }`——预览仅在 `end_cell`/mode 变化时重算，防每帧 A*）
- Modify: 地图交互系统（点击命中：第一击=起点、第二击=终点+预览、回车/确认=提交 `CreateLinePoiCommand`（+CarveSpec when 强制模式）；Esc=取消；点中既有线 → `selected`）
- Test: 面板纯函数测试 + `ui_shell/tests.rs` 集成（意图→命令翻译先例 `intent_translator.rs`）

**Interfaces:**
- Produces: `LinePoiState`（上）；面板属性写回走 `UpdateLinePoiCommand`（Task 6）。
- Consumes: Task 3 `plan_polyline`、Task 4 `carve_corridor`、Task 6 命令、Task 10 入口。宽度输入：map 有 `unit_per_km` → 米；无 → wu 并用 `LinePoiWidthUnitWu` 标注。

- [ ] **Step 1: 写失败测试**：`pin_two_points_then_confirm_emits_create_command`（含强制模式断言 CarveSpec）、`preview_recomputes_only_on_end_cell_change`（两次同 cell 点击零重算——重算计数器断言）、`selecting_existing_line_fills_panel_fields`、`panel_writeback_uses_update_command`、`cancel_clears_pending`。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**（面板控件用 wv-ui 既有件：`FieldInput`/`slider_row`/`square-check`；undo 一次 = 一个 Create 事务）。
- [ ] **Step 4: 通过 + app-clippy + `bash scripts/smoke-gui.sh`**（新增 Resource 装配必跑）。
- [ ] **Step 5: Commit**：`wv-app: 钉点交互与河流和道路面板（ADR-0070 D4）`

### Task 12: wv-app——端点拖拽（改道/延伸/汇流吸附）与生成器统一

**Files:**
- Modify: `crates/wv-app/src/ui/line_poi_panel.rs` + 交互系统（拖顶点：命中半径内最近顶点；拖端点至目标河线容差内 → 吸附最近点；提交 = `UpdatePlaceLineCommand`，延伸段 = `CreateLinePoiCommand` 同名合并语义——**裁量**：v1 延伸=整线重发 `UpdatePlaceLineCommand`，新增下切只作用新段：`carve_corridor(新段 pts)` 与旧段格集求差）
- Modify: `crates/wv-app/src/ui_shell/dispatch.rs` RegenWorld 与 `new_project_dialog.rs`：地形生成后**不再依赖渲染期派生**——`climate::generate_rivers`（保留于 wv-terrain）产出的折线直接经 `plan_polyline` 语义落 `CreateLinePoiCommand`（无下切，绕行模式）进同一 `Transaction`；`rivers_seed` 继续做种子
- Modify: `crates/wv-app/src/bin/gen_demo/main.rs`（示例世界若涂 16：改走河实体）
- Test: 同上两处测试区

**Interfaces:**
- Consumes: Task 4/6/8；`climate::generate_rivers`（`crates/wv-terrain/src/climate.rs:187`）。

- [ ] **Step 1: 写失败测试**：`rerouting_keeps_previous_carve`（改道后旧走廊格不变、仅新增段下切）、`endpoint_snaps_to_other_river_line`（拖至 8 wu 内 → 端点落目标线最近点，误差 <1e-3）、`regen_world_creates_river_entities`（RegenWorld 后 places 含河线 POI、渲染池非空且与 seed 无关二次 Regen 数量一致）、`template_new_project_has_river_entities`。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**。
- [ ] **Step 4: 通过 + app-clippy + smoke-gui**。
- [ ] **Step 5: Commit**：`wv-app: 线 POI 端点拖拽与生成器河实体统一（ADR-0070 D3/D5）`

### Task 13: 群系涂抹入口退役与全量收口

**Files:**
- Modify: `crates/wv-app/src/ui/inspector/cell_paint_panel.rs:345`（`BIOME_GROUP_ORDER` 删 `&[BIOME_ROAD, BIOME_RIVER]` 行；分组数断言若硬编码则同步 -1）
- Modify: `crates/wv-app/src/ui/shell.rs:62-90`（`brush_target` 删 `9 => LAND(BIOME_ROAD)` 分支与笔刷色板「道路」swatch；`_ => WATER` 兜底保留）
- Modify: `README.md`（§12 若有涉及条目则**追加**说明，不改旧文；无则不动）
- Test: 上述两文件测试区

**Interfaces:** Consumes: 无新接口；纯退役。

- [ ] **Step 1: 写失败测试**：`biome_swatch_grid_contains_no_river_or_road`（遍历 `BIOME_GROUP_ORDER` 断言无 16/20）、`brush_target_has_no_road_entry`（swatch 9 落 `_` 兜底）。
- [ ] **Step 2: 确认失败**。
- [ ] **Step 3: 实现**（wv-terrain 的 `BIOME_RIVER`/`BIOME_ROAD` 常量与 `family_of` **保留**——开放集读取防御，仅退役作者入口；`feature_place_dialog` 测试本就断言河不入标记，零改动）。
- [ ] **Step 4: 全量门禁**：`bash scripts/cargo-test.sh`（workspace）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`cargo app-test`、`cargo app-clippy`、`bash scripts/smoke-gui.sh`；§2.2 五条 `cargo tree` 断言（重点 `cargo tree -p wv-render | grep -E 'wv-edit|wv-app'` 为空）。
- [ ] **Step 5: Commit**：`wv-app: 群系涂抹入口退役——河/路不再入笔刷与 CellMap 表单（ADR-0070 D1）`

---

## 自审记录（写完即查，未另开评审）

- **Spec 覆盖**：D1→T7①③/T13；D2→T1/T2；D3→T3/T4/T6/T12；D4→T10/T11；D5→T2/T5/T7②/T8/T9/T12；模板派生河→T7④（存量世界加载）+T12（regen/新建）；§5 裁量 11-15 分别落 T2/T5/T5/T7/T9/T11。无缺口。
- **类型一致**：`LineKind` 全链同名；`RiverLine.width_wu` T8 定义 T8 内自用；`ROAD_SNAP_TOLERANCE_WU` T5 定义 T9 消费。
- **比例**：13 任务 ≈ spec 的 1.8 倍长，无正文转写。
