# 势力层多兼容模型（FactionLayer / Membership / 声称实控）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 ADR-0001 的三层势力模型落到现有扁平实现上：新增 `FactionLayer`、`Membership`、`rank`、`seat`、`GeometrySpan.relation`，让「省 / 王国 / 贵族控多省 / 跨层派系 / 声称 vs 实控」全部可表达；旧 YAML 零迁移。

**Architecture:** `wv-core` 加纯数据模型与查询（`model/faction.rs`）；`wv-io` 加 `faction_layers/` 目录与迁移缺省；`wv-edit` 加层/归属/元数据命令；`wv-render` 重写 `faction_overlays`（层选择、填充、边界、声称样式、focus）；`wv-app` 暴露 `ViewMode::Faction` 并把势力面板改为按层分组 + 归属编辑器。渲染只读 `World`，编辑只走 `EditCommand`（AGENTS §3.3）。

**Tech Stack:** Rust 2021；无新增第三方依赖；serde 派生（缺省字段兼容旧 YAML）；Bevy 0.19 / bevy_egui 现有版本。

**Spec:** `docs/adr/ADR-0027-faction-layer-multi-compatible.md`（本计划唯一决策源）+ `docs/adr/ADR-0001-faction-layer-model.md`（三层模型背景）+ `GLOSSARY.md`（势力层/势力/势力归属/声称·实控）。

## Global Constraints

- **旧 YAML 零迁移**：`Faction` 新字段全部 `#[serde(default)]`；`GeometrySpan.relation` 缺省 `None`；缺 `faction_layers/` 目录时内存合成默认层，**不写盘**。
- **不升 `schema_version`**（仍是 2）：全是可选字段 + 可缺失目录。
- **P4/P6/P7**：`layer` / `target` / `seat` 只存 `Id`；领土与归属都带 `TimeSpan`；不存「当前归属」。
- **V1 校验不是闸门**：悬空 / 成环 / 未知 rank 只进 `diagnose()`，不阻止保存或编辑。
- **零新依赖**：不引入 petgraph / geo / uuid；环检测与点内判定手写。
- **`wv-core` 零 Bevy / 零 IO**；`wv-render` 不反向依赖 `wv-edit` / `wv-app`。
- 测试名用完整英文短句；数值断言附实际值上下文（AGENTS §7.3/§7.4）。
- 提交信息 `<scope>: <动作>`，如 `wv-core: 势力层与归属边模型`。
- 渲染层改动必须额外跑 `cargo app-test` / `cargo app-clippy` / `bash scripts/smoke-gui.sh`（AGENTS §7.1）。

## Review Focus

1. **`Membership` 结束日不被吞**：ADR-0001 的 `to` 与扁平 `TimeSpan.to` 撞名；Task 1 必须有一条回归测试断言 `to: 1200` 落盘后仍是 `Some(1200)`。
2. **旧工程 round-trip**：没有 `layer` / `rank` / `memberships` / `relation` / `faction_layers/` 的工程，加载后保存再加载，语义不变（Task 3）。
3. **时间过滤**：同一势力在 1000–1200 隶属 A、1200–1400 隶属 B；`memberships_of` / `subordinates_of` 按 day 正确切换（Task 2）。
4. **重叠合法**：省 + 贵族领 + 派系三层覆盖同一点，`query_at_point` 返回多项，不报错（Task 2）。
5. **删层不删势力**：删除一个势力层后，其下势力回落默认层；undo 恢复层与归属（Task 5）。
6. **边框屏幕宽度恒定**：缩放时 `stroke_world_thickness(px, view.scale)` 保证屏幕 px 不变；不许写 `px / scale`（历史 bug）。
7. **渲染零写世界**：`faction_overlays` 只读 `World` / `LayerFrame` / `FactionDisplay`；不得 `ResMut<EditHistory>`。
8. **一次只填一个层**：其他层只画边界；三层以上不糊（Task 7）。

## File Map

| 文件 | 职责 |
|---|---|
| `crates/wv-core/src/model/faction.rs` | `FactionLayer` / `Faction` / `Membership` / `FactionTerritory` / 查询 / 环检测 |
| `crates/wv-core/src/map/mod.rs` | `GeometrySpan.relation` |
| `crates/wv-core/src/lib.rs` | `World.faction_layers` + 重导出 |
| `crates/wv-io/src/project.rs` | `faction_layers/` 加载/保存；几何文件名 relation 后缀 |
| `crates/wv-io/src/diagnostics.rs` | 悬空 / 成环 / 重复层 id 诊断 |
| `crates/wv-edit/src/command.rs` | 层 CRUD、势力元数据、归属边、几何 relation |
| `crates/wv-render/src/faction_overlays.rs` | `FactionDisplay` + 分层填充/边界/声称/focus |
| `crates/wv-render/src/vector_layers.rs` | `build_stroke_batch_mesh` / `stroke_world_thickness` 提为 `pub(crate)` 供复用 |
| `crates/wv-render/src/state.rs` | `ViewMode::BROWSING` 加 `Faction` |
| `crates/wv-app/src/ui/faction_panel.rs` | 按层分组、层 CRUD、归属编辑器、focus、层选择器 |
| `crates/wv-app/src/edit/faction_panel_actions.rs` | 面板 → `EditCommand` 动作 |
| `crates/wv-app/src/i18n/*` | 新增键（zh/en） |
| `crates/wv-app/src/ui_shell.rs` / `ui_command_palette.rs` | 1–3 数字键、命令面板模式列表 |
| `GLOSSARY.md` / `README.md` / `docs/adr/README.md` | 术语、待裁、ADR 索引 |

---

### Task 1: wv-core 模型字段与 serde 兼容

**Files:**
- Modify: `crates/wv-core/src/model/faction.rs`
- Modify: `crates/wv-core/src/map/mod.rs`
- Modify: `crates/wv-core/src/model/mod.rs`（导出新类型）
- Modify: `crates/wv-core/src/lib.rs`（重导出 `FactionLayer` / `Membership` / `FactionColorMode`）

**Interfaces:**
- Produces:
  - `pub struct FactionLayer { pub id: Id, pub name: String, pub z: i32, pub color_mode: FactionColorMode, pub visible: bool }`
  - `pub enum FactionColorMode { Own, Inherit }`（`Default = Own`，serde `snake_case`）
  - `pub struct Membership { pub target: Id, pub kind: Option<String>, pub span: TimeSpan }`
  - `Faction` 新字段：`layer: Option<Id>` / `rank: Option<String>` / `seat: Option<Id>` / `memberships: Vec<Membership>`
  - `GeometrySpan.relation: Option<String>`

- [ ] **Step 1: 写失败测试**

在 `faction.rs` 的 `tests` 模块加：

```rust
#[test]
fn membership_end_day_survives_flattened_round_trip() {
    let yaml = "target: faction_kingdom_chen\nkind: 行政隶属\nfrom: 1000\nto: 1200\n";
    let m: Membership = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(m.target.as_str(), "faction_kingdom_chen");
    assert_eq!(m.span.from, 1000);
    assert_eq!(m.span.to, Some(1200), "结束日不能被 serde flatten 吞掉");
    let back = serde_yaml::to_string(&m).unwrap();
    let m2: Membership = serde_yaml::from_str(&back).unwrap();
    assert_eq!(m, m2, "round-trip 必须稳定");
}

#[test]
fn legacy_faction_yaml_without_new_fields_loads() {
    let yaml = "id: faction_qing\nname: 清廷\ncolor: \"#3366AA\"\n";
    let f: Faction = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(f.layer, None);
    assert_eq!(f.rank, None);
    assert_eq!(f.seat, None);
    assert!(f.memberships.is_empty());
}

#[test]
fn faction_layer_color_mode_is_own_by_default() {
    let l: FactionLayer = serde_yaml::from_str("id: layer_admin\nname: 行政区\n").unwrap();
    assert_eq!(l.color_mode, FactionColorMode::Own);
    assert_eq!(l.z, 0);
    assert!(l.visible, "缺省可见");
}

#[test]
fn geometry_span_relation_is_none_by_default() {
    let g = GeometrySpan {
        map: Id::new_unchecked("map_world"),
        owner: Id::new_unchecked("faction_qing"),
        span: TimeSpan::open(100),
        geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
        relation: None,
    };
    let yaml = serde_yaml::to_string(&g).unwrap();
    assert!(!yaml.contains("relation:"), "None 不应序列化，got {yaml}");
    let g2: GeometrySpan = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(g2.relation, None);
}
```

> 说明：`GeometrySpan` 的 `relation` 必须 `skip_serializing_if = "Option::is_none"`，否则旧工程保存后会多出 `relation: null` 噪音（虽不影响加载，但违反「作者没动就别改文件」的 V1 精神）。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-core membership_end_day` → 编译失败（`Membership` 未定义）。

- [ ] **Step 3: 实现结构与缺省**

- `Faction` 保持 `id/name/color` 顺序，新字段追加在后；`Faction::new` 里新字段取缺省。
- `Membership` 用 `target`（**不要**沿用 ADR-0001 的 `to`）；`span` 用 `#[serde(flatten)]`。
- `FactionLayer` 加 `default_true()` helper（模块内私有）。
- `GeometrySpan` 加 `relation`，`#[serde(default, skip_serializing_if = "Option::is_none")]`。
- 修编译：所有 `Faction { id, name, color }` 字面量补 `..Faction::new(...)` 或显式字段；优先用 `Faction::new` 构造。
- 在 `model/mod.rs` / `lib.rs` 导出新类型。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-core faction` → PASS；再跑 `cargo test -p wv-core` 确认旧测试不破。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-core/src/model/faction.rs crates/wv-core/src/map/mod.rs crates/wv-core/src/model/mod.rs crates/wv-core/src/lib.rs
git commit -m "wv-core: 势力层与归属边模型（Membership.target 修正 serde 冲突）"
```

---

### Task 2: World 字段、默认层解析与查询 API

**Files:**
- Modify: `crates/wv-core/src/lib.rs`（`World.faction_layers`）
- Modify: `crates/wv-core/src/model/faction.rs`（查询/默认层/环检测）

**Interfaces:**
- Produces:
  - `World.faction_layers: Vec<FactionLayer>`
  - `pub const DEFAULT_FACTION_LAYER_ID: &str = "faction_layer_default";`
  - `pub fn default_faction_layer() -> FactionLayer`
  - `pub fn resolved_faction_layers(world: &World) -> Vec<FactionLayer>`（按 `(z, id)` 升序；需要时追加默认层）
  - `pub fn resolve_faction_layer_id(world: &World, f: &Faction) -> Id`（显式层存在则用之，否则默认）
  - `pub struct FactionTerritory<'a> { pub layer: Id, pub faction: &'a Faction, pub relation: Option<&'a str>, pub vertices: &'a [f32] }`
  - `pub fn faction_territories_at<'a>(world: &'a World, map: &Id, day: i64) -> Vec<FactionTerritory<'a>>`
  - `pub fn memberships_of<'a>(world: &'a World, faction: &Id, day: i64) -> Vec<&'a Membership>`
  - `pub fn subordinates_of<'a>(world: &'a World, faction: &Id, day: i64) -> Vec<&'a Faction>`（直接下级）
  - `pub fn query_at_point(world: &World, map: &Id, x: f32, y: f32, day: i64) -> Vec<(Id, Id, Option<String>)>`（layer, faction, relation；按三元组去重）
  - `pub fn membership_cycles(world: &World) -> Vec<Vec<Id>>`（诊断用）

- [ ] **Step 1: 写失败测试**

```rust
fn faction(id: &str, layer: Option<&str>, memberships: Vec<Membership>) -> Faction { /* ... */ }

#[test]
fn legacy_factions_resolve_to_default_layer() {
    let mut w = World::default();
    w.factions.push(faction("faction_qing", None, vec![]));
    let layers = resolved_faction_layers(&w);
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].id.as_str(), DEFAULT_FACTION_LAYER_ID);
    assert_eq!(resolve_faction_layer_id(&w, &w.factions[0]).as_str(), DEFAULT_FACTION_LAYER_ID);
}

#[test]
fn noble_controls_multiple_provinces_at_the_same_time() {
    let mut w = World::default();
    let lord = faction("faction_zhenbei", Some("layer_noble"), vec![]);
    let p1 = faction("faction_north", Some("layer_admin"), vec![
        Membership { target: Id::new_unchecked("faction_zhenbei"), kind: Some("实控".into()), span: TimeSpan::open(1042) },
    ]);
    let p2 = faction("faction_hedong", Some("layer_admin"), vec![
        Membership { target: Id::new_unchecked("faction_zhenbei"), kind: Some("实控".into()), span: TimeSpan::open(1042) },
    ]);
    w.factions.extend([lord, p1, p2]);
    let subs = subordinates_of(&w, &Id::new_unchecked("faction_zhenbei"), 1100);
    assert_eq!(subs.len(), 2, "镇北侯应直接控制两省，got {}", subs.len());
    let early = subordinates_of(&w, &Id::new_unchecked("faction_zhenbei"), 1000);
    assert!(early.is_empty(), "1042 之前不应有下属");
}

#[test]
fn one_province_can_have_multiple_lords() {
    // 两个贵族各一条 target=省 的边；subordinates_of(省) 返回两人。
}

#[test]
fn query_at_point_returns_all_overlapping_claims() {
    // 省 + 贵族两层同一点；断言返回两项，不报错。
}

#[test]
fn membership_cycle_is_detected() {
    // A→B→A；membership_cycles 非空。
}
```

- [ ] **Step 2: 跑测试确认失败** Run: `cargo test -p wv-core resolved_faction_layers` → 编译失败。

- [ ] **Step 3: 实现**

- `World` 字段加在 `factions` 之后：`pub faction_layers: Vec<FactionLayer>`。
- `resolved_faction_layers`：克隆已存层；若任一势力的解析层 id == 默认层，则追加默认层；排序 `(z, id)`。
- `faction_territories_at`：沿用 `factions_at` 的扫描顺序与过滤；`relation = g.relation.as_deref()`；`layer = resolve_faction_layer_id(...)`。
- `factions_at` 改为内部委托 `faction_territories_at`，返回 `(faction, vertices)`，保持旧顺序。
- `query_at_point` 用 `wv_core::geometry::point_in_ring(vertices, x, y)`（签名 `(&[f32], f32, f32)`），按 `(layer, faction, relation)` 去重。
- `membership_cycles`：建 `id → memberships[].target` 邻接表，DFS 三色标记；返回遇到的循环路径（只用于诊断，不去重）。
- `memberships_of` / `subordinates_of`：先 `TimePoint::from_day(day)`，再 `span.contains`。

- [ ] **Step 4: 跑测试确认通过** Run: `cargo test -p wv-core faction`。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-core/src/lib.rs crates/wv-core/src/model/faction.rs
git commit -m "wv-core: 势力层默认层与多对多归属查询"
```

---

### Task 3: wv-io 加载/保存与迁移

**Files:**
- Modify: `crates/wv-io/src/project.rs`
- Tests: 同文件 `#[cfg(test)]`

**Interfaces:**
- `Project::load_world` 读 `faction_layers/`（可缺失）。
- `Project::save_world` 的 `wants()` 分支加 `"faction_layers"`。
- 几何文件名：`map__owner__from` 为基础；`relation` 为 `Some(r)` 且 `r != "control"` 时追加 `__r<hash8>`（稳定 FNV-1a），避免同一 `(map, owner, from)` 的 claim/control 互相覆盖。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn legacy_world_without_faction_layers_round_trips() {
    // 用 Project::create 建临时工程；手写 factions/geometry，不含 faction_layers。
    // load_world → save_world(&[""]) → load_world；断言 factions 数量/字段与第二次相等。
}

#[test]
fn faction_layers_round_trip() {
    // 写 faction_layers/layer_admin.yaml；load → save → load；断言 z/color_mode/visible。
}

#[test]
fn two_relations_same_owner_and_from_round_trip() {
    // 同一 (map, owner, from) 两条 span：relation=None 与 Some("claim")。
    // save 后目录下应有 2 个 yaml；load 回来两条都在。
}
```

- [ ] **Step 2: 跑测试确认失败**（`faction_layers` 字段不存在 / 第二条被覆盖）。

- [ ] **Step 3: 实现**

- 在 `load_world` 的势力段加 `let faction_layers: Vec<FactionLayer> = load_dir(&self.root, "faction_layers")?;`，并填入 `World`。
- 在 `save_world` 的 `wants("factions")` 旁加 `wants("faction_layers")` → `save_entities(..., &world.faction_layers, |l| l.id.as_str().to_string())`。
- 把 `geometry/faction` 的 key 闭包抽成 `fn faction_geometry_key(g: &GeometrySpan) -> String`；`fnv1a` 内联。
- **不**在加载时把默认层写进 `world.faction_layers`。

- [ ] **Step 4: 跑测试** `cargo test -p wv-io faction`。

- [ ] **Step 5: Commit** `wv-io: faction_layers 目录与关系几何文件键`

---

### Task 4: wv-io 诊断（V1 只提示）

**Files:**
- Modify: `crates/wv-io/src/diagnostics.rs`

**Interfaces:**
- `diagnose()` 追加（全部 `DiagLevel::Hint` 或 `Error`，**不影响 `validate()` 的保存路径**；按现有约定）：
  - `Faction.layer` 悬空 / `memberships[].target` 悬空 / `Faction.seat` 悬空；
  - `Membership.span.to < from`；
  - `membership_cycles(world)` 非空（每个环一条）；
  - 重复 `FactionLayer.id`（同名 id 不同文件）。

- [ ] **Step 1: 写失败测试**（构造最小 World，断言 `diagnose()` 含对应条目与 level）。
- [ ] **Step 2: 实现**（复用 `wv_core::membership_cycles`；文本给可回访 id）。
- [ ] **Step 3: 跑测试** `cargo test -p wv-io diagnostics`。
- [ ] **Step 4: Commit** `wv-io: 势力层与归属边诊断`

---

### Task 5: wv-edit 命令

**Files:**
- Modify: `crates/wv-edit/src/command.rs`

**Interfaces（全部 `EditCommand`，dirty = `["factions", "faction_layers"]`）:**
- `AddFactionLayerCommand { layer: FactionLayer }`
- `UpdateFactionLayerCommand { id, new_name, new_z, new_color_mode, new_visible }`（before 快照）
- `RemoveFactionLayerCommand { id }`：删除层 + 把引用它的 `Faction.layer` 置 `None`；revert 恢复两者
- `SetFactionLayerCommand { id, layer: Option<Id> }`
- `SetFactionRankCommand { id, rank: Option<String> }`
- `SetFactionSeatCommand { id, seat: Option<Id> }`
- `AddMembershipCommand { faction: Id, membership: Membership }`
- `RemoveMembershipCommand { faction: Id, target: Id, from: i64 }`（同一 target 可有多条，按 `from` 定位）
- 现有 `UpdateFactionCommand` 不动（name/color）；现有 `UpsertFactionGeometryCommand` 接受带 `relation` 的 `GeometrySpan`（结构体已扩，签名不变）。

- [ ] **Step 1: 写失败测试**（apply/revert round-trip、dirty paths、删层回落默认层、重复 membership 不合并）。
- [ ] **Step 2: 实现**（照 `AddFactionCommand` 的 before/after 模板；错误用 `EditError::TargetMissing`）。
- [ ] **Step 3: 跑测试** `cargo test -p wv-edit faction`。
- [ ] **Step 4: Commit** `wv-edit: 势力层/归属/元数据命令`

---

### Task 6: wv-render 分层填充（FactionDisplay + 只填当前层）

**Files:**
- Modify: `crates/wv-render/src/faction_overlays.rs`
- Modify: `crates/wv-render/src/lib.rs`（导出 `FactionDisplay`）
- Modify: `crates/wv-render/src/plugins.rs` / `crates/wv-app/src/app_bootstrap.rs`（`init_resource::<FactionDisplay>()`）

**Interfaces:**
- `pub struct FactionDisplay { pub active_layer: Option<Id>, pub show_other_borders: bool, pub show_claims: bool, pub focus: Option<Id> }` + `Default`（`show_other_borders=true`, `show_claims=true`）。
- 纯函数（可单测）：
  - `fn visible_territories<'a>(world, map, day, display) -> Vec<FactionTerritory<'a>>`（层可见 ∧ 当前层填充 / 其他层按 `show_other_borders`；claim 按 `show_claims` 过滤）
  - `fn fill_alpha(territory, is_active, focus) -> f32`
  - `fn is_in_focus(faction_id, focus, world, day) -> bool`（自身 + 直接下级）

- [ ] **Step 1: 写失败测试**（当前层有 fill；其他层无 fill；`show_claims=false` 过滤 claim；focus 外 alpha 更低）。
- [ ] **Step 2: 实现**：沿用 `build_polygon_mesh` 与 `ColorMaterial`；每帧 despawn + spawn（现状），但只填当前层；其他层本轮**不画任何东西**（边界在 Task 7 补）。
- [ ] **Step 3: 跑测试** `cargo test -p wv-render faction`。
- [ ] **Step 4: Commit** `wv-render: 势力层当前层填充`

---

### Task 7: wv-render 边界、rank 线宽、声称/实控样式

**Files:**
- Modify: `crates/wv-render/src/faction_overlays.rs`
- Modify: `crates/wv-render/src/vector_layers.rs`（`build_stroke_batch_mesh` / `stroke_world_thickness` 改 `pub(crate)`）

**Interfaces:**
- `fn border_px(rank: Option<&str>, depth: u8) -> f32`（D6 表；未知 rank 走默认）
- `fn polygon_edges(vertices: &[f32]) -> Vec<(Vec2, Vec2)>`（闭合环）
- `fn dash_segments(edges, dash_px, gap_px, view_scale) -> Vec<(Vec2, Vec2)>`
- 每个势力：一条合并边界 mesh（casing 可两条：深墨 + 势力色内线；v1 先单色深墨 + 势力色 1px 内线）。
- Z：填充 `0.30 + layer.z * 0.002`；边界 `+0.001`；再叠 `LayerFrame.influence.z_offset`。

- [ ] **Step 1: 写失败测试**（`border_px` 映射；dash 段数 > 0 且总长不超原边；rank None 深度派生）。
- [ ] **Step 2: 实现**（`stroke_world_thickness(px, view.scale)`，**禁止** `px / scale`）。
- [ ] **Step 3: 跑测试** + `cargo app-clippy`。
- [ ] **Step 4: Commit** `wv-render: 势力边界与声称实控样式`

---

### Task 8: wv-render 其他层只画边界 / focus / 层可见性

**Files:**
- Modify: `crates/wv-render/src/faction_overlays.rs`

**Interfaces:**
- 当前层：填充 + 边界；其他层：仅边界（细线，alpha ≈ 0.55）。
- `focus`：自身 + `subordinates_of` 满强度，其余 alpha 降至 0.06（填充）/ 0.20（边界）。
- 层 `visible=false` 或 `ViewState.factions_visible=false` → 该层不画。
- `LayerFrame.influence.visible` 仍是总门（Recipe `PassId::Faction` 已 gate）。

- [ ] **Step 1: 写失败测试**（其他层只产生边界实体；focus 外 alpha；层隐藏零实体）。
- [ ] **Step 2: 实现**（复用 Task 6/7 的纯函数；注意不要每帧 `materials.add` 泄漏——实体 despawn 后句柄释放即可）。
- [ ] **Step 3: 跑测试** `cargo test -p wv-render faction`。
- [ ] **Step 4: Commit** `wv-render: 势力层边界-only 与 focus`

---

### Task 9: wv-app 暴露势力图（第三个浏览模式）

**Files:**
- Modify: `crates/wv-render/src/state.rs`（`ViewMode::BROWSING = [VectorFlat, Schematic, Faction]`）
- Modify: `crates/wv-app/src/ui_shell.rs`（数字键 1–3）
- Modify: `crates/wv-app/src/ui_command_palette.rs`（列表自动跟随 `BROWSING`；修正硬编码 0/1 的测试）
- Modify: `crates/wv-app/src/i18n/*`（`ViewModeFaction` 已存在，确认中英文文案）

- [ ] **Step 1: 写失败测试**（`BROWSING.len() == 3`；`BROWSING[2] == Faction`；命令面板第 3 项存在）。
- [ ] **Step 2: 实现**（数字键数组加 `Num3`；任何 `0..BROWSING.len() as u8` 的断言更新）。
- [ ] **Step 3: 跑 `cargo test -p wv-app --features app` + 启动烟测**。
- [ ] **Step 4: Commit** `wv-app: 势力图加入浏览模式与数字键`

---

### Task 10: wv-app 势力面板按层分组 + 层 CRUD

**Files:**
- Modify: `crates/wv-app/src/ui/faction_panel.rs`
- Modify: `crates/wv-app/src/edit/faction_panel_actions.rs`
- Modify: `crates/wv-app/src/i18n/table.rs` + `keys.rs`

**Interfaces（动作层）:**
- `add_layer_now(...)` / `update_layer_now(...)` / `remove_layer_now(...)`
- `set_faction_layer_now(...)` / `set_faction_rank_now(...)` / `set_faction_seat_now(...)`
- 面板：
  - 顶层「+ 新建势力层」；每个层一个 `CollapsingHeader`（名称、z、color_mode、visible）。
  - 层下按现有 `faction_card` 列出；卡片加「层」「等级」「驻地」三个输入。
  - 删除层走 `RemoveFactionLayerCommand`（受 V1：不阻止；势力回落默认层）。

- [ ] **Step 1: 写动作层测试**（用 `WorldProjection` + `EditHistory` 调 action；断言命令生效与 dirty）。
- [ ] **Step 2: 实现 UI**（复用 `float_window` / `space` / `UiShell` 令牌；不新增控件库）。
- [ ] **Step 3: `cargo app-test` / `cargo app-clippy`。**
- [ ] **Step 4: Commit** `wv-app: 势力面板按层分组与层编辑`

---

### Task 11: wv-app 归属编辑器 + focus + 层选择器

**Files:**
- Modify: `crates/wv-app/src/ui/faction_panel.rs`
- Modify: `crates/wv-app/src/edit/faction_panel_actions.rs`
- Modify: `crates/wv-app/src/i18n/*`

**Interfaces:**
- 势力卡内「归属」列表：每行 `target` / `kind` / `from` / `to` + 删除；底部「+ 添加归属」。
- 「定位」按钮：`FactionDisplay.focus = Some(id)`；「清除」置 None。
- 面板顶部（或势力图顶栏）层选择器：写 `FactionDisplay.active_layer`；选项来自 `resolved_faction_layers`。
- 归属编辑经 `AddMembershipCommand` / `RemoveMembershipCommand`，不做即时校验（V1）。

- [ ] **Step 1: 写动作层测试**（添加/删除归属、重复 target 不同 from 共存）。
- [ ] **Step 2: 实现 UI**。
- [ ] **Step 3: `cargo app-test` / `cargo app-clippy` + `bash scripts/smoke-gui.sh`。**
- [ ] **Step 4: Commit** `wv-app: 势力归属编辑器与 focus`

---

### Task 12: 文档、术语与全门禁

**Files:**
- Modify: `GLOSSARY.md`（追加：**势力等级提示 Rank**、**势力层选择器 Faction Layer Picker**）
- Modify: `README.md` §12（追加待裁：反向索引、派生领地、Layer 条目、人物归属、同日起算 relation 文件名）
- Modify: `docs/adr/README.md`（ADR-0027 行）
- Modify: `docs/MAP_LAYER_RENDERING_SPEC_v0.1.md`（§1.1 `Border` 消费者 + §8 缺口表更新为「势力边界走 Faction 通道」——按 §6.3 追加，不改旧结论）

- [ ] **Step 1: 文档追写**（全部追加式，不改既有结论）。
- [ ] **Step 2: 全门禁**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p wv-app --features app
cargo clippy -p wv-app --all-targets --features app -- -D warnings
bash scripts/smoke-gui.sh
```

- [ ] **Step 3: 人工验收**
  - 打开 `xinhai-1911`：旧势力落在「势力」默认层，势力图可见，保存后 YAML 不炸。
  - 新建「行政区」层 + 两个省（会员边指向一个王国）+ 一个贵族（两条实控边）：势力图切层可见省界；贵族 focus 高亮两省。
  - 关闭「行政区」可见 → 该层不画；`factions_visible` 总开关仍能全关。
- [ ] **Step 4: Commit** `docs: 势力层多兼容术语、待裁与 ADR 索引`

---

## 收尾检查

- [ ] `cargo tree -p wv-core | grep -i bevy` → 空
- [ ] `cargo tree -p wv-render | grep -E 'wv-edit|wv-app'` → 空
- [ ] 旧 YAML 夹具（无新字段）load→save→load 语义一致
- [ ] `Membership` 结束日回归测试在 CI 中常驻
- [ ] 势力图三层叠加不糊（人工看一次截图）
- [ ] 所有新术语已在 `GLOSSARY.md` 登记
- [ ] ADR-0027 已进 `docs/adr/README.md` 索引

## Execution Handoff

计划完成后按 `superpowers:executing-plans` 或 `superpowers:subagent-driven-development` 执行；Task 1–5 是数据/IO/编辑层，可先合；Task 6–11 是渲染/UI 层，必须在 Task 5 之后；Task 12 最后跑全门禁。
