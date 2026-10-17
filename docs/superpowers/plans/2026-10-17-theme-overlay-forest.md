# 主题覆盖物 v1（森林最小闭环）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让作者圈一片「黑森林」作为地理事实（Place + 地界），主题按镜头远近自动演绎：远看一团绿晕，拉近长出树冠——换主题（Daylight/Dusk）换一副观感。

**Architecture:** 事实存 `Place.feature`（复用地形 kind 词汇）+ `World.place_geometry`（`GeometrySpan`，与势力领地同底座）；渲染层新增 `PassId::FeatureOverlay`，远带画地界晕、近带在地界 ∩ 网格采样出符号；圈选手势复用大陆意图圈模式（逐点左键 + 双击/回车闭合）。

**Tech Stack:** Rust 2021 / Bevy 0.19 / egui（app feature）/ serde_yaml；无新增第三方依赖。

**Spec:** `docs/superpowers/specs/2026-10-17-theme-overlay-geo-fact-design.md`（D1–D11、§3 验收 6 条、§6 架构落点）

## Global Constraints

- 依赖方向：`wv-core` 零 Bevy / 零网络；`wv-render` 不依赖 `wv-edit` / `wv-app`；`wv-core` **不依赖** `wv-terrain`（`Place.feature` 必须是裸 `u8`）。
- 渲染层**不做时间比较**：地界一律经 `wv_core::place_territory_at(world, map, day)` 求值。
- 改世界只走 `wv-edit::EditCommand` + `History`（AGENTS §3.3）。
- 校验只提示不阻止（§7.5 V1）；地界 span 只追加不覆盖（P6/P7）。
- `TimeSpan` 半开：`[from, to)`，`to = None` = 至今有效。
- 新 UI 文案必须同时进 `lookup_zh` 与 `lookup_en`（ADR-0018，漏写编译报错）。
- 提交信息格式：`<scope>: <动作>`。
- 门槛命令（每个 task 至少跑本 task 相关 crate 的测试，Task 8 跑全量）：
  `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` /
  `cargo test -p wv-render --features app` / `cargo app-test` / `cargo app-clippy` / `bash scripts/smoke-gui.sh`。

## Review Focus

1. **旧工程的 `Place` 字面量**（45 处）加字段后必须全部编译——漏一处是编译错误，但**新建的测试/示例里把 `feature` 误设成 `Some`** 会让本来普通的城市变成覆盖物事实、POI 点消失；所有既有构造点一律 `feature: None`。
2. **手工改 YAML 的地界**（不经编辑命令）会让 `x/y/w/h` 锚点过期 → 标签画在旧位置；v1 接受，文档已注明命令负责同步。
3. **凹多边形扇形三角化**：地界晕沿用 faction 的扇形 mesh，凹多边形会贴错区域（faction 已有同样的已知妥协）；v1 不修，测试不覆盖。
4. **空采样**（无栅格 / 全水域 / 全非森林且高度 ≤ 0）不得让森林消失：晕 + 名继续显示。
5. **继承底图**（城市子图没有自带栅格，用祖先图地形）：采样必须用 `active_terrain` + `raster_offset`，用错原点会导致树全落在地界外或图外。
6. **主题缺规则**（未来未知 feature / 未配规则的主题包）：必须有中性晕回退，不能什么都不画。

---

### Task 1: wv-core — `Place.feature` 字段与锚点拟合

**Files:**
- Modify: `crates/wv-core/src/map/distance.rs:93`（`Place` 结构体 + `fit_anchor_to_ring`）
- Compile-fix: 全部 `Place { ... }` 字面量（`cargo check --workspace --all-targets` 会逐一列出；已知在 `crates/wv-app/examples/gen_lod_demo.rs`、`crates/wv-app/examples/gen_layer_showcase.rs`、`crates/wv-app/src/http/**`、`crates/wv-app/src/http_integration.rs`、`crates/wv-app/src/state.rs`、`crates/wv-io/tests/**`）
- Test: `crates/wv-core/src/map/distance.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces: `Place.feature: Option<u8>`（serde 缺省跳过）；`Place::fit_anchor_to_ring(&mut self, ring: &[f32])`

- [ ] **Step 1: 写失败测试**（放在 distance.rs 的 tests 模块）

```rust
fn feature_place(feature: Option<u8>) -> Place {
    Place { id: Id::new_unchecked("place_dark_forest"), name: "黑森林".into(),
        map: Some(Id::new_unchecked("map_world")),
        x: None, y: None, w: None, h: None, feature }
}

#[test]
fn fit_anchor_to_ring_sets_centroid_and_bbox() {
    let mut p = feature_place(Some(4));
    p.fit_anchor_to_ring(&[0.0, 0.0, 40.0, 0.0, 40.0, 20.0, 0.0, 20.0]);
    assert_eq!((p.x, p.y), (Some(20.0), Some(10.0)), "质心为 (20,10)");
    assert_eq!((p.w, p.h), (Some(40.0), Some(20.0)), "包围盒 40×20");
}

#[test]
fn fit_anchor_to_ring_with_degenerate_ring_leaves_anchors_untouched() {
    let mut p = feature_place(Some(4));
    p.fit_anchor_to_ring(&[1.0, 2.0]);
    assert_eq!((p.x, p.y, p.w, p.h), (None, None, None, None), "不足 3 点不写锚点");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-core fit_anchor_to_ring` → 编译错误（无 `feature` 字段/方法）

- [ ] **Step 3: 实现**

`Place` 增加字段（文档注释说明「值为 TerrainKind 编号；wv-core 不依赖 wv-terrain 故为裸 u8」）。`fit_anchor_to_ring`：点数 < 3 直接返回；质心 = 顶点平均（复用 `Region::centroid` 同口径）；`w/h` = 包围盒宽高。

- [ ] **Step 4: 修全部构造点**

`cargo check --workspace --all-targets` 逐个补 `feature: None`（既有语义零变化）。

- [ ] **Step 5: 跑测试通过** — `cargo test -p wv-core`

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "wv-core: Place 增加 feature 事实类型与地界锚点拟合"
```

---

### Task 2: wv-core — `place_geometry`、`place_territory_at`、`point_in_ring`

**Files:**
- Modify: `crates/wv-core/src/lib.rs:188`（`World` 字段 + 导出）
- Create: `crates/wv-core/src/model/place_territory.rs`
- Modify: `crates/wv-core/src/model/mod.rs`（`pub mod place_territory;`）
- Modify: `crates/wv-core/src/base/geometry.rs`（`point_in_ring`）
- Test: 两个模块的内联 tests

**Interfaces:**
- Produces:
  - `World.place_geometry: Vec<map::GeometrySpan>`
  - `wv_core::place_territory_at(world: &World, target_map: &Id, day: i64) -> Vec<(&Place, &[f32])>`
  - `wv_core::geometry::point_in_ring(ring: &[f32], x: f32, y: f32) -> bool`

- [ ] **Step 1: 写失败测试（territory 查询，镜像 faction 的 7 个用例）**

```rust
// 与 model/faction.rs 的 tests 同构，7 个：
fn place_territory_at_filters_by_map()
fn place_territory_at_filters_by_time_half_open()   // day=10 含、day=20 不含
fn place_territory_at_open_end_means_until_now()
fn place_territory_at_skips_orphan_geometry()       // owner 不在 distances.places
fn place_territory_at_skips_non_polygon_geometry()
fn place_territory_at_returns_slice_ref_into_world()
fn place_territory_at_returns_all_active()
```

- [ ] **Step 2: 写失败测试（点包含）**

```rust
#[test]
fn point_in_ring_covers_interior_and_excludes_outside() {
    let ring = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
    assert!(point_in_ring(&ring, 5.0, 5.0));
    assert!(!point_in_ring(&ring, 15.0, 5.0));
    assert!(!point_in_ring(&ring, 5.0, -1.0));
}
#[test]
fn point_in_ring_rejects_degenerate_ring() { assert!(!point_in_ring(&[0.0, 0.0, 1.0, 1.0], 0.5, 0.5)); }
```

- [ ] **Step 3: 跑测试确认失败** — `cargo test -p wv-core place_territory` → 编译错误

- [ ] **Step 4: 实现**

`GeometrySpan` 直接复用；查询逐条镜像 `factions_at`（map 过滤 → `span.contains` → 查 `world.distances.places` 找不到则跳过 → 只取 `GeometryData::Polygon` → 返回原始切片）。`point_in_ring` 用射线法，顶点数 < 6（不足 3 点）返回 false；`lib.rs` 顶部按 `factions_at` 的写法 `pub use model::place_territory::place_territory_at;`。

- [ ] **Step 5: 跑测试通过** — `cargo test -p wv-core`

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "wv-core: 地界几何与 place_territory_at 求值"
```

---

### Task 3: wv-io — `geometry/place/` 读写 + round-trip

**Files:**
- Modify: `crates/wv-io/src/project.rs`（`load_world` 约 737 行处、`save_world` 约 842 行 faction 分支后）
- Create: `crates/wv-io/tests/place_territory_yaml.rs`

**Interfaces:**
- Consumes: Task 1 的 `Place.feature`、Task 2 的 `World.place_geometry`
- Produces: 工程目录 `geometry/place/<map>__<owner>__<from>.yaml`；`Place` 的 `feature` 字段随 `distances/places.yaml` 自动 round-trip

- [ ] **Step 1: 写失败测试**

```rust
// 三个测试，模板抄 crates/wv-io/tests/region_yaml.rs：
#[test] fn place_feature_round_trips_through_yaml() {
    // feature: Some(4) → 序列化 → 反序列化相等；feature: None 序列化后不含 "feature"
}
#[test] fn project_saves_then_loads_place_geometry_round_trip() {
    // 一个带 feature=Some(4) 的 Place + 一条 GeometrySpan（owner=place.id, open span）
    // save_world(&world, &["distances/places".into(), "geometry/place".into()])
    // 断言 geometry/place/ 下有 1 个文件；load_world 后 places 与 place_geometry 无损
}
#[test] fn project_without_geometry_place_dir_loads_empty_vec() { /* 零迁移 */ }
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-io --test place_territory_yaml` → 断言失败（文件不存在）

- [ ] **Step 3: 实现加载与保存**

加载：`let place_geometry: Vec<wv_core::map::GeometrySpan> = load_dir(&self.root, "geometry/place")?;`，填入 `World`。
保存：`if wants("geometry/place") { save_entities(..., |g| format!("{}__{}__{}", g.map.as_str(), g.owner.as_str(), g.span.from))?; }`（与 faction 分支逐字同构）。

- [ ] **Step 4: 跑测试通过** — `cargo test -p wv-io`

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "wv-io: 地界 geometry/place 落盘与加载"
```

---

### Task 4: wv-edit — 创建事实地点 / 改地界命令

**Files:**
- Modify: `crates/wv-edit/src/command.rs`（Region 段之后新增一节）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 1 `fit_anchor_to_ring`、Task 2 `World.place_geometry`
- Produces:
  - `AddFeaturePlaceCommand { place: Place, territory: GeometrySpan }`（`new` 构造）
  - `UpdatePlaceTerritoryCommand { place_id: Id, map: Id, span_from: i64, polygon: Vec<f32> }`（`new` 构造）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn add_feature_place_sets_anchors_from_ring_and_revert_removes_both() {
    // apply 后：places 1 条、place_geometry 1 条、x/y/w/h == 拟合值
    // revert 后：两者皆空
}
#[test]
fn add_feature_place_rejects_duplicate_id()
#[test]
fn update_place_territory_replaces_ring_and_resyncs_anchors() {
    // 改为 (0,0)-(20,10)：x/y 变 (10,5)，w/h 变 (20,10)；revert 全字段回滚
}
#[test]
fn update_place_territory_missing_span_is_an_error()
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-edit add_feature_place` → 编译错误

- [ ] **Step 3: 实现**

`AddFeaturePlaceCommand::apply`：id 冲突 → `EditError::Precondition`；`place.fit_anchor_to_ring(polygon)`；push place + span；dirty = `["distances/places", "geometry/place"]`。`revert` 按 id 移除两者。
`UpdatePlaceTerritoryCommand`：按 `(map, owner, span.from)` 定位 span，缓存旧 `Place` 全字段与旧 `GeometrySpan`，写新 `GeometryData::Polygon(polygon)` 并重拟合锚点；`revert` 恢复两者。

- [ ] **Step 4: 跑测试通过** — `cargo test -p wv-edit`

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "wv-edit: 事实地点与地界的编辑命令"
```

---

### Task 5: wv-render — 主题覆盖规则、分带与采样纯函数

**Files:**
- Create: `crates/wv-render/src/theme/overlays.rs`
- Modify: `crates/wv-render/src/theme/mod.rs`（`pub mod overlays;`）
- Modify: `crates/wv-render/src/theme/theme_pack.rs`（`ThemePack` 加 `overlays: FeatureOverlaySet`；`builtin::DEFAULT` / `builtin::DUSK` 各配一套）
- Modify: `crates/wv-theme-chinese-ink/src/lib.rs:38`（`overlays` 字段，墨色一套；外部 crate 加字段的编译必然）
- Test: `overlays.rs` 内联 tests；`theme_pack.rs` tests 追加

**Interfaces:**
- Produces:
  - `pub const FEATURE_FOREST: u8 = 4;`（对应 `theme::colors::CORE_TERRAIN` 索引 4）
  - `FeatureRule { halo_color: [u8;3], halo_alpha: f32, symbol_color: [u8;3], symbol_size_px: f32, symbol_spacing_wu: f32, symbol_texture: Option<&'static str>, halo_texture: Option<&'static str> }`（后两项 v1 恒 `None`，Task 9 接入）
  - `FeatureOverlaySet { forest: Option<FeatureRule> }` + `rule_for(&self, feature: u8) -> Option<&FeatureRule>`
  - `overlay_far_blend(scale: f32) -> f32`
  - `forest_sites(ring_local: &[f32], terrain: Option<&TerrainData>, cell_size: f32, raster_offset: (f32,f32), seed: u64, spacing_wu: f32) -> Vec<(f32,f32)>`

- [ ] **Step 1: 写失败测试（分带）**

```rust
#[test]
fn overlay_far_blend_anchors() {
    assert_eq!(overlay_far_blend(0.75), 0.0);
    assert_eq!(overlay_far_blend(1.25), 1.0);
    assert!((overlay_far_blend(1.0) - 0.5).abs() < 1e-6, "中点 smoothstep=0.5");
}
```

- [ ] **Step 2: 写失败测试（采样，手算口径）**

```rust
fn square_ring() -> Vec<f32> { vec![0.0,0.0, 100.0,0.0, 100.0,100.0, 0.0,100.0] }
fn grid(10, 10, kind, height) -> TerrainData // cell_size=10

#[test] fn forest_sites_are_inside_the_ring_and_deterministic()
// spacing=25 → 16 个候选；两次调用结果逐元素相等
#[test] fn forest_sites_prefer_forest_kind_cells()
// 左半 kind=4、右半 kind=3、height=10：结果非空且每点所在格 kind==4
#[test] fn forest_sites_fall_back_to_land_when_no_cell_is_forest()
// 全 kind=3、height=10 → 16 个
#[test] fn forest_sites_return_empty_when_all_water()
// 全 height=-5 → 0 个
#[test] fn forest_sites_return_empty_without_terrain()
// terrain=None → 0 个（回退晕，不画树）
#[test] fn forest_sites_honor_raster_offset()
// raster_offset=(10,10) 时同一点落在不同格，结果集随之变化
```

- [ ] **Step 3: 跑测试确认失败** — `cargo test -p wv-render --features app overlay` → 编译错误

- [ ] **Step 4: 实现**

`overlay_far_blend`：`t = clamp((scale-0.75)/0.5, 0, 1)`，`t*t*(3-2t)`（与 `lod_fade` 同族，中点 1.0）。
采样：ring bbox 内按 `spacing_wu` 布候选网格 → `splitmix64(seed ^ gx<<32 ^ gy)` 生成 ±0.35×spacing 抖动（**不引入 rand**）→ `point_in_ring` 过滤 → 局部坐标 + `raster_offset` 映射到格心 → 两遍规则（先收 kind==4；一条都没有则收 height>0）。`seed` 由调用方给 `place.id` 的稳定哈希。
主题规则（D10 要求两包至少 2 项可见差异）：
- DEFAULT：`halo_color [0x5F,0x7A,0x4A]`, `halo_alpha 0.30`, `symbol_color [0x3F,0x5A,0x35]`, `symbol_size_px 5.0`, `symbol_spacing_wu 24.0`
- DUSK：`halo_color [0x3E,0x4A,0x34]`, `halo_alpha 0.20`, `symbol_color [0x28,0x32,0x24]`, `symbol_size_px 4.0`, `symbol_spacing_wu 16.0`
- Chinese-ink：墨绿 + 更疏的 spacing（占位，注释标美术待校准）。

- [ ] **Step 5: 测试通过 + 主题规则断言**

`theme_pack.rs` 追加：`default_pack_has_forest_rule`、`dusk_differs_from_default_in_at_least_two_visual_fields`（比较 halo_color / halo_alpha / symbol_spacing_wu，≥2 项不同）、`unknown_feature_has_no_rule`（`rule_for(9) == None`）。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "wv-render: 覆盖物主题规则、镜头分带与近景采样"
```

---

### Task 6: wv-render — `FeatureOverlay` pass、覆盖层系统、POI 抑制

**Files:**
- Modify: `crates/wv-render/src/recipe.rs`（`PassId` 末尾加 `FeatureOverlay`、`COUNT` 20→21、`ALL` 加项、`MODE_TABLE` 每行末尾加真值、`!layers.place.visible` 级联加本 pass、测试追加）
- Create: `crates/wv-render/src/feature_overlays.rs`
- Modify: `crates/wv-render/src/faction_overlays.rs`（`build_polygon_mesh` 改 `pub(crate)` 复用）
- Modify: `crates/wv-render/src/world_projection.rs:154`（抽 `place_dot_for`，feature 地点不生成 POI 点）
- Modify: `crates/wv-app/src/app_bootstrap.rs:267`（主链 `sync_faction_overlays` 之后注册 `sync_feature_overlays`）
- Modify: `crates/wv-render/src/lib.rs`（`pub mod feature_overlays;`）
- Test: `recipe.rs` / `world_projection.rs` 内联

**Interfaces:**
- Consumes: Task 2 `place_territory_at`、Task 5 `FEATURE_FOREST` / `overlay_far_blend` / `forest_sites` / `FeatureRule`、`terrain_view::terrain_resolve::active_terrain`
- Produces: `FeatureOverlayEntities` Resource；`pub fn sync_feature_overlays(...)`；`PassId::FeatureOverlay`

- [ ] **Step 1: 写失败测试（recipe）**

```rust
#[test] fn feature_overlay_enabled_in_all_modes_except_coastline()
// VectorFlat/RealTerrain/Schematic/GoogleMap/Faction/Event = true；Coastline = false
#[test] fn hiding_place_layer_disables_feature_overlay()
// layers.place.visible = false → recipe.enabled(FeatureOverlay) == false
#[test] fn pass_id_count_matches_all_len() // PassId::COUNT == PassId::ALL.len()
```

- [ ] **Step 2: 写失败测试（POI 抑制）**

```rust
#[test] fn feature_place_produces_no_poi_dot()
// place_dot_for(&Place { feature: Some(4), x/y Some, w/h Some }, &GridConfig::default()) == None
#[test] fn ordinary_place_still_produces_city_dot()
// feature: None, w/h Some → Some(dot.is_city)
```

- [ ] **Step 3: 跑测试确认失败** — `cargo test -p wv-render --features app feature_overlay` → 编译错误

- [ ] **Step 4: 实现 recipe**

枚举末尾追加 `FeatureOverlay`，`COUNT = 21`，`ALL` 同步；`MODE_TABLE` 七行末尾各加一列（七个模式：除 Coastline 外全 `true`）；`!layers.place.visible` 的分支列表加 `PassId::FeatureOverlay`。

- [ ] **Step 5: 实现覆盖层系统**

```rust
pub struct FeatureOverlayEntities { pub halos: Vec<Entity>, pub symbols: Vec<Entity> }
pub fn sync_feature_overlays(
    mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>, playback: Res<PlaybackState>,
    projection: Res<WorldProjection>, grid: Res<GridConfig>,
    layers: Res<LayerFrame>, recipe: Res<RenderRecipe>,
    mut layer: ResMut<FeatureOverlayEntities>,
)
```
门：`recipe.enabled(FeatureOverlay)`（图层已由 recipe 级联）→ 每帧 despawn 全部再重建。**主题规则取 `theme::theme_pack::builtin::for_palette(view.map_palette)`（D10）——不读 `ActiveThemePack`**（该资源目前无同步系统，会永远停在 Daylight）。地图/日：`place_territory_at(&projection.world, &current_map, playback.current_day)`。取地形：`active_terrain(&projection.world, Some(&current_map))`；`raster_offset = (grid.map_origin - grid.raster_origin)`。远带：每个事实地点一个扇形 halo mesh（`build_polygon_mesh`），alpha = `rule.halo_alpha * (0.4 + 0.6*blend)`，z = `0.06`。近带：`forest_sites(..., seed = stable_hash(place.id), spacing_wu = rule.symbol_spacing_wu)` 合并成**单个** mesh（每个符号 4 顶点 6 索引），世界尺寸 = `rule.symbol_size_px * view.scale`，alpha = `1 - blend`（`< 0.05` 跳过），z = `0.07`。无规则 → 中性灰绿晕（alpha 0.12）+ 无符号。

- [ ] **Step 6: POI 抑制**

`world_projection.rs` 抽 `fn place_dot_for(p: &Place, grid: &GridConfig) -> Option<PlaceDot>`；`feature.is_some()` 返回 `None`；现有测试如直连字段则改走该函数。

- [ ] **Step 7: 注册系统**

`app_bootstrap.rs` 主链 `faction_overlays::sync_faction_overlays` 后追加 `feature_overlays::sync_feature_overlays`（仍在 `map_frame` 之前）；系统元组参数已顶满 20，如超限则按现有 `brush_cursor` 模式单列 `.add_systems(... .after(...))`。

- [ ] **Step 8: 测试通过 + Commit**

```bash
cargo test -p wv-render --features app && git add -A && git commit -m "wv-render: FeatureOverlay 通道与地界晕/近景符号渲染"
```

---

### Task 7: wv-app — 圈地界工具、命名对话框、工具条与文案

**Files:**
- Modify: `crates/wv-render/src/state.rs`（`TerritorySketch` Resource + 状态机方法 + tests）
- Create: `crates/wv-render/src/territory_sketch.rs`（点击采集 + Gizmos 绘制，镜像 `continent_sketch.rs`）
- Modify: `crates/wv-render/src/lib.rs`（`pub mod territory_sketch;`）
- Create: `crates/wv-app/src/edit/feature_place_actions.rs`
- Modify: `crates/wv-app/src/edit/mod.rs`
- Create: `crates/wv-app/src/ui/feature_place_dialog.rs`
- Modify: `crates/wv-app/src/ui/mod.rs`
- Modify: `crates/wv-app/src/ui/map_controls.rs`（②.6 之后加「森林地界」瓷砖，互斥关掉 measure/city/continent sketch）
- Modify: `crates/wv-app/src/i18n/keys.rs` + `table.rs`（4 个 Key × zh/en）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（`init_resource` 两个 Resource；系统注册）
- Test: `state.rs` 状态机测试；`feature_place_actions.rs` 提交测试

**Interfaces:**
- Consumes: Task 4 命令、Task 5 `FEATURE_FOREST`
- Produces:
  - `wv_render::state::TerritorySketch { enabled, feature, active, last_click, pending }`
  - `add_feature_place_now(world, edit, view, playback, name, feature, ring) -> Option<Id>`
  - `FeaturePlaceDialog { open, name }`

- [ ] **Step 1: 写失败测试（状态机）**

```rust
#[test] fn territory_sketch_closes_into_pending_with_three_points()
// add_point ×3 + try_close → pending Some(3 点)、active None
#[test] fn territory_sketch_discards_under_three_points()
#[test] fn territory_sketch_discard_active_keeps_pending()
```

- [ ] **Step 2: 实现状态机 + 点击/绘制系统**

镜像 `continent_sketch.rs` 的早退链（UI 指针 / 播放 / 无工程锁 / 光标坐标），差异：`try_close` 写入 `pending` 而非 `envelopes`；`pending` 存在时启用对话框。绘制：进行中折线 + 顶点（Gizmos，同款常量）。

- [ ] **Step 3: 写失败测试（提交动作）**

```rust
#[test] fn add_feature_place_now_writes_open_span_and_anchors() {
    // 构造 WorldProjection::default + EditHistory::default + ViewState + PlaybackState{current_day: 42}
    // 调用后：places 1 条 feature=Some(4)、place_geometry 1 条 span.from=42/to=None、
    // x/y/w/h 已拟合、返回 Some(id)
}
```

- [ ] **Step 4: 实现动作层与对话框**

`add_feature_place_now`：id = `place_feature_<unix_ms>`；`Place { feature: Some(feature), map: Some(view.current_map), .. }`；`GeometrySpan { map, owner, span: TimeSpan { from: playback.current_day, to: None }, geometry: Polygon(flatten(ring)) }`；`Transaction::push(AddFeaturePlaceCommand)` + `commit`（镜像 `region_panel_actions::push_cmd`）。
对话框：标题 / 名称输入 / 「创建」/「取消」；创建按钮在名称为空时禁用；确认调 `add_feature_place_now` 并清 `pending`。

- [ ] **Step 5: 工具条 + i18n**

瓷砖文案：`FeatureForestToolHint`（森林：圈地界；左键加点；双击/回车闭合；Esc 丢弃）、`FeaturePlaceDialogTitle`（新建地理事实）、`FeaturePlaceNameLabel`（名称）、`FeaturePlaceCreate`（创建）。开工具时 `sketch.feature = FEATURE_FOREST`。

- [ ] **Step 6: 注册**

`app_bootstrap`：`init_resource::<TerritorySketch>()` / `FeaturePlaceDialog::default()`；`commit_territory_sketch_click` 排在 `track_cursor_world` 之后；`draw_territory_sketch` 与 `draw_continent_sketch` 同批；对话框放进 `EguiPrimaryContextPass` 既有的浮动面板集合。

- [ ] **Step 7: 测试通过 + Commit**

```bash
cargo test -p wv-render --features app && cargo app-test
git add -A && git commit -m "wv-app: 森林地界圈选工具与命名对话框"
```

---

### Task 8: 校验提示、全量闸门与 GUI 烟测

**Files:**
- Modify: `crates/wv-io/src/diagnostics.rs`（`detect_feature_territory_hints` + `derive_hints` 调用）
- Create: `crates/wv-io/tests/feature_territory_hints.rs`

**Interfaces:**
- Consumes: Task 2 `point_in_ring`、Task 5 `FEATURE_FOREST`（wv-io 不依赖 wv-render！**在 wv-io 内用字面量 4**，注释指向 `wv-render::theme::overlays::FEATURE_FOREST`）

- [ ] **Step 1: 写失败测试**

```rust
#[test] fn forest_territory_without_forest_cells_emits_hint()
// 1 个 feature=Some(4) 的 Place + 地界 + 全 kind=3 的 10×10 地形
// → 恰好 1 条 Hint，fingerprint 含 place id，message 非空
#[test] fn forest_territory_with_forest_cells_emits_no_hint()
#[test] fn orphan_territory_emits_hint_without_blocking()
// owner 不在 places → 1 条 Hint（不是 Error）
#[test] fn no_terrain_emits_no_hint()  // 没栅格不算问题（晕仍可见）
```

- [ ] **Step 2: 实现**

`detect_feature_territory_hints(world)`：遍历 place feature 地点 → 每条 `geometry/place` span（owner 匹配）；该图**没有自带栅格**则跳过（继承底图不在提示范围）；有栅格且 `MapNode.extent` 为 `Some` 时，把环内格心按 `x/extent.w*data.width`、`y/extent.h*data.height` 映射到格，统计 kind==feature 的格；零命中 → `hint("feature-no-cells|{place_id}", "...")`。孤儿 owner → `hint("feature-orphan-territory|{owner}", "...")`。`derive_hints` 末尾 `d.items.extend(detect_feature_territory_hints(world));` 并 `dedup_by_fingerprint`。

- [ ] **Step 3: 测试通过**

`cargo test -p wv-io`

- [ ] **Step 4: 全量闸门**

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p wv-render --features app
cargo app-test
cargo app-clippy
bash scripts/smoke-gui.sh
```
Expected: 全绿；烟测进程 5 秒存活 + 日志无 panic/B000x。

- [ ] **Step 5: 人工验收（§3 六条）**

打开 `examples/xinhai-1911`（或新建工程）：圈黑森林 → 远看晕 + 名、拉近见树 → 切 Dusk 观感变化 → 手改 YAML 加第二条 span 拖时间轴 → 给事件填 `location.place = place_feature_...` 验证落点。

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "wv-io: 覆盖物事实的校验提示（V1 只提示）"
```

---

### Task 9: 贴图接入（用户供图后执行；D12）

> **阻塞条件**：作者把 PNG 贴进会话或放入 `crates/wv-render/assets/overlays/forest/`。Task 1–8 不依赖本 task。
> **资产规格**：见 spec §6.6（PNG / 透明背景 / 单色墨稿优先 / 128×128 / 2–4 个变体；可选 256×256 无缝晕纹理）。
> **生图提示词**：见 spec §6.6.1——**v1 用 A（JRPG 古舆图，D13）**；B（水墨）仅留给未来可切换的 ink 主题；C 可选晕纹理；D 山脉 v2。
> **风格纪律**：v1 不做湿笔晕染/纸张渗透；近带是树冠**符号**，不是水墨画。

**Files:**
- Create: `crates/wv-render/assets/overlays/forest/`（PNG 资产，作者提供）
- Modify: `crates/wv-render/src/theme/overlays.rs`（内置包规则填 `symbol_texture`）
- Modify: `crates/wv-render/src/feature_overlays.rs`（纹理加载 + UV mesh；纯色路径保留）
- Test: `overlays.rs` / `feature_overlays.rs` 内联

**Interfaces:**
- Consumes: Task 5 `FeatureRule.symbol_texture`、Task 6 `sync_feature_overlays`
- Produces: `build_symbol_mesh_uv(sites: &[(f32,f32)], size: f32, variants: usize) -> (Mesh, Vec<u32>)`（每 quad 带 UV；variant 索引供测试）

- [ ] **Step 1: 写失败测试（纯函数）**

```rust
#[test] fn symbol_mesh_has_uvs_and_four_vertices_per_site()
// 3 个 site → 12 顶点、18 索引、12 个 UV
#[test] fn symbol_variant_selection_is_stable_per_site()
// 同一 site 多次选择同 variant；相邻 site 不保证不同
#[test] fn missing_texture_falls_back_to_flat_color()
// rule.symbol_texture = Some("overlays/forest/does_not_exist.png")
// 首帧（句柄未就绪）→ 仍产生纯色 mesh，不 panic、不空帧
```

- [ ] **Step 2: 资产归一化（我执行，非代码任务）**

用 `scripts/normalize_overlay_asset.py`：先看行密度定 `--cut-bottom`（切掉地面阴影与水印）→ 白底转 alpha → 118px 内适配到 128 画布；暗色主题加 `--light R,G,B` 另出浅墨版。多张拼图集时记录每个变体的 UV 矩形到 `FeatureRule` 旁的常量表。
**2026-10-17 已完成**：`tree_a.png`（Daylight，`--crop 416,1634,433,1372`）+ `tree_a_dusk.png`；`tree_b.png`（彩色单株，`--crop 216,1843,203,1815 --alpha-gain 1.9`）+ `tree_b_dusk.png`；`tree_c.png`（针叶三株，`--crop 480,1574,324,1340 --alpha-gain 1.7`）+ `tree_c_dusk.png`；`forest_patch.png`（A4，`--crop 264,1808,224,1800 --alpha-gain 1.4`，**自带地面，不进 tree 轮换**）；`forest_fill.png`（A5，裁 `0,1900,0,1900`、逐通道去纸底、512×512，**地界单张图章**）；`mountain_a.png` / `mountain_a_dusk.png`（A6，`--crop 230,1900,620,1340 --size 512`，横向内容；**v2 山脉才用，v1 不接线**）。**彩色稿按原色使用、gain 1.4–1.9；墨线稿用默认 1.35**。**不要用 `Image.paste(..., mask)` 拼画布**——会把 alpha 平方、RGB 预乘，导致整体偏暗半透明（本次踩过）。

- [ ] **Step 3: 实现**

`FeatureOverlayEntities` 增加 `textures: HashMap<&'static str, Handle<Image>>` 缓存（`AssetServer::load`，加载失败在下一帧检查并 warn + 清除句柄 → 回退纯色）。符号路径：有纹理 → 每 site 一个带 UV 的 quad 合批成一个 mesh + `ColorMaterial { texture, color: 白色（墨稿按主题染色时用 symbol_color） }`；无纹理 → 走 Task 6 的纯色路径。晕纹理可选：给 halo 多边形补 UV（`(x-min)/tile`），Image 采样器设 Repeat；没给则纯色。

- [ ] **Step 4: 测试通过 + 实机验收**

```bash
cargo test -p wv-render --features app && cargo app-test && bash scripts/smoke-gui.sh
```
实机：远看晕、近看树；删掉 PNG 重跑 → 回退纯色不崩。

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "wv-render: 森林覆盖物贴图接入（缺资产回退纯色）"
```

---

## Self-Review

- **Spec coverage**：D1–D8 → Task 1–6；D9 范围 → 全计划只做森林；D10 → Task 5 两包规则 + Task 7 骑 `ToggleMapPalette`（无需新 UI）；D11 → Task 7 开放 span；D12 → Task 9（资产到货后执行，资产缺失自动回退）；验收 1（圈+命名+落盘）→ Task 3/7；2（远带晕+名）→ Task 6（名沿用既有 PlaceLabels；w/h 由 Task 1/4 写入保证 Nation 档存活）；3（近带树冠）→ Task 5/6；4（切主题）→ Task 5；5（span+时间轴）→ Task 3/4；6（事件引用）→ Task 1 既有能力 + Task 3 round-trip。T1–T7 不进 v1。
- **类型一致性**：`place_territory_at`（Task 2）→ Task 6 消费；`FeatureRule`/`FEATURE_FOREST`（Task 5）→ Task 6/7；`TerritorySketch.pending`（Task 7）→ 对话框同 task；`AddFeaturePlaceCommand`（Task 4）→ Task 7。
- **Review Focus 覆盖**：① Task 1 Step 4 + 测试只用 `feature: None`；② 文档已记；③ 有意不测（faction 同款）；④ Task 5 `return_empty_without_terrain` + Task 6 回退中性晕；⑤ Task 5 `honor_raster_offset` + Task 6 `active_terrain`；⑥ Task 5 `unknown_feature_has_no_rule` + Task 6 回退晕。
- **比例**：计划 ≈ 450 行 / spec ≈ 130 行；代码块只含签名、测试断言与文件清单，无完整函数体。
