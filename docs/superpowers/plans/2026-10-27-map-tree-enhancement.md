# 地图树增强实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 地图树从静态占位升级为真数据工作台：点击切换、右键复制/粘贴（深拷贝整棵子树）、删除进垃圾桶（软删可恢复、彻底删除不可撤销）、垃圾桶分区、新增按钮。

**Architecture:** 严格单亲树不变（多对多已否决，spec §5 D1）。软删 = 仅被删根节点打 `trashed` 标志，children 引用与钻取图标原样保留、显隐派生（spec §10 追记 1）；真删 = `PurgeMapCommand` 快照式移除 + wv-app 直应用通道豁免撤销（spec §10 追记 2）；跨图复用 = `PasteMapCommand` 深拷贝（ID 全量重生成、重名 +1、锚定视野中心）。

**Tech Stack:** Rust 2021 / Bevy 0.19 + egui 0.36（wv-app `app` feature 门后）/ serde_yaml。零新第三方依赖。

**Spec:** `docs/superpowers/specs/2026-10-27-map-tree-enhancement-design.md`（含 §10 追记，实施者须同读）

## Global Constraints

- 编辑一律经 `wv_edit::EditCommand` → `History`（AGENTS §3.3）；面板写回用 `layers_panel.rs:332` `commit_layer` 同款 `Transaction` 配方（`edit.history.commit(&mut projection.world, tx)` + `edit.record_dirty(dirty)`）。
- wv-core 零 Bevy/网络；wv-edit 不碰文件系统；依赖只向下（AGENTS §2）。
- 老工程 YAML 兼容：`trashed` 字段缺省 = false（`#[serde(default)]`），round-trip 后不产生 diff（`skip_serializing_if`）。
- 提交信息 `<scope>: <动作>`（如 `wv-edit: TrashMapCommand 软删整棵子树`）。
- 测试名完整英文短句；数值断言附实际值上下文（AGENTS §7.3/7.4）。
- UI 图标仅 Lucide：`trash-2/undo-2/plus/copy` 已在 `crates/wv-ui/assets/ui/tool_icons/`；缺 `paste.svg`（Lucide 同名，描边烘焙 `stroke="#ffffff"`，经 `egui::include_image!` 引 wv-ui primitives）。
- i18n：新文案一律走 `I18nKey`；加键须同步 `keys.rs` 枚举 + `ALL` 数组 + `table.rs` 中英两表 + `i18n/mod.rs` 计数断言字面量（现为 535，本计划加 12 键 → **547**，两处 assert + 注释一起改）。
- 受限执行环境跑测试用 `bash scripts/cargo-test.sh`；**worktree 内勿用**（TEMP 新路径必踩假失败），改 `export TEMP=<主checkout>/.target/testtmp` 后直接 `cargo test`。
- 渲染/UI 改动追加门禁：`cargo app-test`、`cargo app-clippy`、`cargo test -p wv-render --features app`、`bash scripts/smoke-gui.sh`（新增 Resource/改系统签名必跑）。
- 执行前按 AGENTS §13：`git branch feat/map-tree-enhancement` + `git worktree add .worktrees/feat/map-tree-enhancement feat/map-tree-enhancement`；`CARGO_TARGET_DIR` 用全局共享值，勿在 worktree 覆盖。
- GUI 实机验收会把工作区面板显隐写进 `examples/jrpg-world/ui-layout.yaml`——验收后 `git checkout` 还原，勿提交。

## Review Focus

1. **老 YAML 无 `trashed` 字段**加载必须等价于未删，round-trip 不产生意外 diff → Task 1/6 测试钉住。
2. **删到当前打开的图 / `meta.root_map` 本尊**：会话内自动切回 + root_map 改写，下次启动不得落进隐形图（`ensure_root` 每帧钉 root_map）→ Task 2 测试。
3. **恢复跨重启**：垃圾桶是 YAML 状态、Undo 栈不持久——物理摘除图标/children 会让重启后恢复残缺 → 追记 1 的派生显隐由 Task 7 钉住（trashed 时图标/轮廓/钻入全隐藏）。
4. **副本与源共享状态**：layer id 撞车会覆盖 `world.streets` 键、Place id 撞车会污染距离图 → Task 4 测试断言全量 ID 换新 + 两轮粘贴名字 2/3。
5. **purge 半途残留或被 Ctrl+Z 复活**：目录/实体删不干净、或撤销栈复活已确认的永久删除 → Task 5（apply_detached 不入栈）+ Task 6（保存后目录物理消失）测试。

---

### Task 1: wv-core 垃圾桶字段与子树/存活工具

**Files:**
- Modify: `crates/wv-core/src/map/mod.rs`（`MapNode` 尾部加 2 字段 + serde 辅助）
- Modify: `crates/wv-core/src/lib.rs`（`impl World` 加 3 个方法，紧随 `measurement_of` 族）
- Test: `crates/wv-core/src/map/mod.rs` 与 `lib.rs` 既有 `#[cfg(test)]` 模块内追加

**Interfaces:**
- Produces: `MapNode { pub trashed: bool, pub trashed_at: Option<String> }`（`#[serde(default, skip_serializing_if = "is_false")]` / `skip_serializing_if = "Option::is_none"`，`fn is_false(b: &bool) -> bool` 私有辅助）
- Produces: `World::subtree_ids(&self, root: &Id) -> Vec<Id>`（BFS 含根，visited 集合防环/防重复）
- Produces: `World::map_alive(&self, id: &Id) -> bool`（节点存在 && 沿 `parent` 链上溯无 `trashed`；自身 trashed 即 false）
- Produces: `World::alive_roots(&self) -> Vec<Id>`（`parent.is_none() && !trashed`，按 `world.maps` 声明序）

- [ ] **Step 1: 写失败测试**

```rust
// map/mod.rs 测试模块
fn old_yaml_without_trashed_field_loads_as_alive() {
    let node: MapNode = serde_yaml::from_str("id: city_a\nname: 青州城\n").unwrap();
    assert!(!node.trashed, "缺省字段应为未删");
    assert_eq!(node.trashed_at, None, "got {:?}", node.trashed_at);
}

fn trashed_flag_round_trips_through_yaml() {
    let mut node = MapNode::default();
    node.trashed = true;
    node.trashed_at = Some("2026-10-27 15:00".into());
    let text = serde_yaml::to_string(&node).unwrap();
    assert!(text.contains("trashed: true"), "got:\n{text}");
    assert!(!text.contains("trashed_at: null")); // skip_serializing_if 生效
    let back: MapNode = serde_yaml::from_str(&text).unwrap();
    assert_eq!(back.trashed_at.as_deref(), Some("2026-10-27 15:00"));
}

// lib.rs 测试模块
fn subtree_ids_collects_all_descendants_without_duplicates() { /* 世界→城→Interior 三层 + 手工造环（A.children 含 B、B.children 含 A），断言无重复、无死循环 */ }
fn map_alive_false_inside_trashed_subtree() { /* 根 alive；根.trashed=true 后子城 map_alive==false；仅城 trashed 时根仍 alive */ }
fn alive_roots_follow_declaration_order_and_skip_trashed() { /* 两根，第二个 trashed → 只返回第一个 */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-core map 2>&1 | tail -5`（预期编译错误：no field `trashed`）

- [ ] **Step 3: 实现字段与三个方法**

`is_false` 辅助仿 `is_zero_f32`（map/mod.rs:121 同款私有 fn）。`subtree_ids` 用 `VecDeque` + `HashSet` visited。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-core 2>&1 | tail -3`（预期全绿）

- [ ] **Step 5: 提交**

```bash
git add crates/wv-core/src/map/mod.rs crates/wv-core/src/lib.rs
git commit -m "wv-core: MapNode 增垃圾桶标志与子树/存活工具"
```

---

### Task 2: wv-edit `TrashMapCommand`（软删）

**Files:**
- Modify: `crates/wv-edit/src/command/map.rs`（文件尾部新命令）
- Test: `crates/wv-edit/src/command/command_tests.rs` 追加

**Interfaces:**
- Consumes: Task 1 的 `World::alive_roots` / `MapNode.trashed` / `trashed_at`
- Produces: `pub struct TrashMapCommand` + `TrashMapCommand::new(map_id: Id, trashed_at: impl Into<String>) -> Self`；`apply` 返回 `Ok(vec!["maps".into()])`

apply 语义（按序）：
1. 节点不存在 → `TargetMissing`；已 trashed → `Precondition("已在垃圾桶")`
2. 守卫：目标 `parent.is_none()` 且 `alive_roots().len() == 1`（即最后一棵存活根树）→ `Precondition("最后一张地图不可删除")`
3. 根节点打 `trashed = true` + `trashed_at`；**不动 children、不动图标**（spec §10 追记 1）
4. `meta.root_map == map_id` 时改写为 `alive_roots()` 首项（改写前重算——此时目标已打标故被排除），旧值缓存进 `Mutex<Option<Id>>`

revert：标志翻回 + `trashed_at = None` + root_map 还原缓存值。

- [ ] **Step 1: 写失败测试**

```rust
fn trash_marks_root_only_and_keeps_children_references() {
    // 世界→城：trash 城 → 城.trashed、世界.children 仍含城 id、世界 icons 不变
}
fn trash_twice_is_rejected() { /* 第二次 apply 返回 Precondition */ }
fn trash_last_alive_root_is_rejected() { /* 单根世界 → Precondition；错误信息含 "最后一张" */ }
fn trash_root_map_rewrites_meta_and_revert_restores_it() {
    // 双根（root_map=A）：trash A → meta.root_map == B；revert → root_map == A
}
```

- [ ] **Step 2: 跑测试确认失败** — Run: `cargo test -p wv-edit trash_ 2>&1 | tail -3`（编译错误：找不到 `TrashMapCommand`）
- [ ] **Step 3: 实现命令** — 结构仿 `MoveCityCommand` 的 `Mutex` 快照模式（map.rs:239 `before_snapshot` 同款）。
- [ ] **Step 4: 跑测试确认通过** — Run: `cargo test -p wv-edit 2>&1 | tail -3`
- [ ] **Step 5: 提交** — `git commit -m "wv-edit: TrashMapCommand 软删整棵子树（根打标+root_map 改写）"`

---

### Task 3: wv-edit `RestoreMapCommand`（整条还原）

**Files:** 同 Task 2

**Interfaces:**
- Produces: `RestoreMapCommand::new(map_id: Id) -> Self`；dirty 同 `["maps"]`

apply 语义：
1. 节点不存在 → `TargetMissing`；未 trashed → `Precondition("不在垃圾桶")`
2. `parent = Some(p)` 时：p 节点不存在 → `Precondition("原父图已被彻底删除")`；`!world.map_alive(&p)` → `Precondition("先恢复父图")`
3. `trashed = false` + `trashed_at = None`（children/图标从未被动过，无需还原）

- [ ] **Step 1: 写失败测试**

```rust
fn restore_unmarks_trashed_root() { /* trash→restore 后 trashed==false 且子孙 map_alive 全 true */ }
fn restore_blocked_while_parent_still_trashed() {
    // trash 城 C，再 trash 世界 W，restore C → Precondition("先恢复父图")；restore W 后 restore C 成功
}
fn restore_blocked_when_parent_was_purged() { /* 从 world.maps 手工移除父节点后 restore → Precondition */ }
```

- [ ] **Step 2: 确认失败** — `cargo test -p wv-edit restore_ 2>&1 | tail -3`
- [ ] **Step 3: 实现** — [ ] **Step 4: 通过**（`cargo test -p wv-edit`）
- [ ] **Step 5: 提交** — `git commit -m "wv-edit: RestoreMapCommand 整条还原（父链存活守卫）"`

---

### Task 4: wv-edit `PasteMapCommand`（深拷贝，两轮 TDD）

**Files:** 同 Task 2（命令放 map.rs；实体收集辅助函数私有）

**Interfaces:**
- Consumes: Task 1 `subtree_ids`
- Produces:
  - `PasteMapCommand::new(source_id: Id, target_parent: Option<Id>, anchor: Point) -> Self`（`Point` = `wv_core::base::geometry::Point`）
  - `pub fn created_root(&self) -> Option<Id>`（apply 后供 UI 导航；revert 后回 None）
  - 私有辅助：`unique_map_id(world, base) -> Id`（`<base>_2`、`_3`… 直到不撞）、`unique_sibling_name(world, parent, name) -> String`（`青州城 2`…）、`unique_layer_id(world, base) -> Id`（全图域查重——`world.streets/buildings` 按图层 ID 全局键控，撞车即覆盖）

**层级门控（apply 前置）**：`World→target 必为 None`；`Region|City→target.level == World`；`Interior→target.level == City`；违规 `Precondition`。源必须 `map_alive`。

**锚定**：`anchor` 先 clamp 进目标图 `extent`（有 extent 时）；`delta = anchor − 源根 extent.origin`（源根无 extent 时取 `outline_polygon` 质心，两者皆无不平移）；根节点 `extent.origin += delta`、`outline_polygon` 顶点整体 `+= delta`（`MoveCityCommand` map.rs:236 同款平移语义）；后代 LOCAL 数据不动。

- [ ] **Step 1: Round A 失败测试（节点与逐图数据）**

```rust
fn paste_city_clones_nodes_layers_and_terrain_with_new_ids() {
    // 世界 W(有 extent) → 城 C(layer: road 声明 + streets 文档 + terrain + measurements + Interior 子图 I)
    // paste C→W(anchor=中心) → 断言：新 id != 旧 id（地图/图层/街道键/地形键全异）、
    // children 挂上、I 也在、名字「青州城 2」、W.children.len()==2
}
fn paste_numbers_name_on_subsequent_collisions() { /* 连贴两次 → 青州城 2、青州城 3 */ }
fn paste_world_lands_as_independent_root() { /* target=None → 新根 parent None、不走名字补号（无同级） */ }
fn paste_level_gating_rejects_wrong_target() { /* City 贴 City、Interior 贴 World、World 贴 Some(target) → Precondition */ }
fn paste_anchor_clamped_into_target_extent() { /* extent 1000×1000，anchor 传 (9999,0) → 新根 origin.x ≤ 1000 */ }
```

- [ ] **Step 2: 确认失败** — `cargo test -p wv-edit paste_ 2>&1 | tail -3`
- [ ] **Step 3: 实现节点克隆**（BFS `subtree_ids` → 逐节点 clone + ID/名字/图层重生成 + `icons.drill_into` 重映射 + `Layer.source` 前缀改写 `maps/<old>/`→`maps/<new>/` + `terrains/streets/buildings/measurements` 按新键 clone + 锚定平移；ledger（`Mutex<Option<Ledger>>`）记录全部新建键供 revert）
- [ ] **Step 4: Round A 通过** — `cargo test -p wv-edit paste_ 2>&1 | tail -3`
- [ ] **Step 5: Round B 失败测试（实体与边）**

```rust
fn paste_copies_places_regions_seas_and_geometry_with_remapped_ids() {
    // C 上有 Place P(x,y) + place_geometry(span.map=C) + Region R + Sea S + faction_geometry(map=C, owner=势力F)
    // paste 后：新 P（id 异、map=新C、x/y 不变）、geometry map/owner 重映射（faction_geometry owner 仍指 F——势力是全局实体）、R/S id 异且 map=新C
}
fn paste_drops_edges_touching_places_outside_copy_set() {
    // P1、P2 在 C 上（P1–P2 边跟随）；P3 在 W 上（P1–P3 边丢弃）；断言新图边数==1 且端点是新 P1/P2
}
fn revert_paste_removes_every_created_entity() {
    // paste → revert → maps/places/edges/regions/seas/place_geometry/faction_geometry/terrains/streets/measurements 计数全部回到 paste 前；created_root()==None
}
```

- [ ] **Step 6: 确认失败 → 实现实体克隆**（按 `map ∈ subtree` 收集；`PlaceEdge` 仅当 a、b 都在复制集内才 clone 并重映射端点）→ **Step 7: `cargo test -p wv-edit` 全绿**
- [ ] **Step 8: 提交** — `git commit -m "wv-edit: PasteMapCommand 深拷贝整棵子树（ID 重生成/名字补号/实体与边收集）"`

---

### Task 5: wv-edit `PurgeMapCommand` + wv-app 直应用通道

**Files:**
- Modify: `crates/wv-edit/src/command/map.rs` + `command_tests.rs`
- Modify: `crates/wv-app/src/edit/editing.rs`（`EditHistory` 包装层加方法）

**Interfaces:**
- Consumes: Task 1 `subtree_ids` / `map_alive`
- Produces: `PurgeMapCommand::new(map_id: Id) -> Self`；dirty = `["maps","distances/places","distances/edges","geometry/place","geometry/faction","regions","seas"]`（streets/buildings/measurements 由 `wants("maps")` 联动路由，project.rs:1016-1026 已证实）
- Produces: `crate::edit::editing::EditHistory::apply_detached(&mut self, world: &mut World, cmd: Box<dyn wv_edit::EditCommand>) -> wv_edit::EditResult<Vec<String>>`（apply + record_dirty，**不** push 进 History——spec §10 追记 2）

apply 语义（快照全缓存，revert 可完整还原——防误入栈，正常永不 revert）：
1. 节点不存在 → `TargetMissing`；未 trashed → `Precondition("只有垃圾桶内的图才能彻底删除")`
2. `subtree_ids` 收集；`world.maps` 移除全部；在世父图 `children.retain` 摘根 + 移除 `icons.drill_into ∈ 集合` 的图标（缓存）
3. 移除并缓存：`terrains`/`measurements`（按图 id）、`streets`/`buildings`（按被删节点各 layer id）、`places`（`map ∈ 集合`）、`regions`/`seas`、`place_geometry`/`faction_geometry`（`map ∈ 集合`）、`edges`（任一端 ∈ 被删 places）

- [ ] **Step 1: 失败测试**（wv-edit）

```rust
fn purge_removes_subtree_entities_and_dangling_drill_icons() { /* 世界W(icon→C)→城C(Place/Region/Sea/geometry/edge) 全链 trash C→purge C → 全消失、W.children 空、W.icons 空 */ }
fn purge_rejects_alive_map() { /* 未 trash 直接 purge → Precondition */ }
fn revert_purge_restores_everything() { /* purge → revert → 全部计数复原（防误入栈兜底） */ }
```

（wv-app 侧）`apply_detached_records_dirty_without_pushing_history`：commit 一条普通命令 + apply_detached 一条 → `history.undo_len()` 不变、dirty set 增长（`crates/wv-app/src/edit/editing/tests.rs` 追加）。

- [ ] **Step 2: 确认失败** — `cargo test -p wv-edit purge_ 2>&1 | tail -3`
- [ ] **Step 3: 实现命令与方法** — [ ] **Step 4: `cargo test -p wv-edit && cargo app-test 2>&1 | tail -3` 通过**
- [ ] **Step 5: 提交** — `git commit -m "wv-edit,wv-app: PurgeMapCommand 真删整棵子树 + 直应用通道（豁免撤销）"`

---

### Task 6: wv-io `save_maps` 目录删除传播 + 垃圾桶 round-trip

**Files:**
- Modify: `crates/wv-io/src/project.rs:91` `save_maps`
- Test: `crates/wv-io` 既有集成测试文件追加（faction / terrain_bin 同款临时目录配方）

**Interfaces:**
- Consumes: Task 1 字段（序列化自然带出）、Task 5 purge（内存移除 → 保存删目录）
- Produces: `save_maps(dir, maps)` 新契约——keep-set = `maps` 参数全部 id（**含 trashed**）；`maps/` 下不在 keep 的子目录 `remove_dir_all`；删除 `maps.is_empty()` 的提前 return（空表 = 清空全部子目录）

- [ ] **Step 1: 失败测试**

```rust
fn trashed_map_survives_save_reload_as_trashed() { /* 建工程→trash→save→重开 → trashed==true 且 trashed_at 回读一致 */ }
fn purged_map_directory_removed_on_save() { /* 建工程→trash→purge→save → maps/<id>/ 整目录消失 */ }
fn save_clears_stale_directories_not_in_world() { /* 手工塞 maps/ghost/index.yaml → save 后消失 */ }
```

（受限环境：worktree 内 `export TEMP=<主checkout>/.target/testtmp` 再跑，勿用 cargo-test.sh）

- [ ] **Step 2: 确认失败** → **Step 3: 实现**（keep-set 仿 `save_entities` project.rs:33 模式；目录级 `remove_dir_all`）→ **Step 4: `cargo test -p wv-io 2>&1 | tail -3` 通过**
- [ ] **Step 5: 提交** — `git commit -m "wv-io: save_maps 补目录删除传播（keep-set 含垃圾桶条目）"`

---

### Task 7: trashed 派生显隐（wv-render）

**Files:**
- Modify: `crates/wv-render/src/drill.rs:503`（`drill_pick` 拒绝非 `map_alive` 目标）
- Modify: 钻取图标绘制入口与城市轮廓绘制入口（`grep -rn "IconSpec\|outline_polygon" crates/wv-render/src --include="*.rs"` 定位；两处各加 `world.map_alive` 门控）
- Test: wv-render 既有测试模块追加（跑 `cargo test -p wv-render --features app`）

**Interfaces:**
- Consumes: Task 1 `World::map_alive`
- Produces: 渲染侧私有辅助 `fn map_reachable(world: &World, id: &Id) -> bool`（= `world.map_alive(id)` 别名，单点收口便于将来换判据）

- [ ] **Step 1: 失败测试**

```rust
fn drill_pick_refuses_trashed_city() { /* 世界→城 trash 后 drill_pick(城) 返回 None/不进入 */ }
fn drill_icon_and_outline_hidden_while_child_trashed() { /* 收集图标/轮廓渲染列表，trashed 子图的两类实体缺席 */ }
```

- [ ] **Step 2: 确认失败** → **Step 3: 实现门控** → **Step 4: `cargo test -p wv-render --features app 2>&1 | tail -3` 通过**
- [ ] **Step 5: 提交** — `git commit -m "wv-render: 钻取图标/轮廓/钻入口按 trashed 派生显隐"`

---

### Task 8: 树面板重写（真数据 / 切换 / 右键菜单 / 新增按钮）

**Files:**
- Create: `crates/wv-app/src/ui/map_tree.rs`（`panel_map_tree` + `tree_row` 从 layers_panel.rs 迁入并重写；本任务先建骨架：工具栏 + 树 + 右键菜单 + 剪贴板）
- Modify: `crates/wv-app/src/ui/layers_panel.rs`（删 `panel_map_tree`/`tree_row`，模块注释同步）、`crates/wv-app/src/ui/mod.rs`（导出改指向 map_tree）、`crates/wv-app/src/app_bootstrap/schedules.rs`（系统签名不变则只补 `init_resource`）、`crates/wv-app/src/state.rs:371`（`Panel::MapTree` default_size `[204,330]`→`[204,336]`，注释注明上限 = Minimap 顶 396 − 56 − 4 防叠）
- Modify: i18n 四处（`keys.rs` 枚举 + `ALL` 数组 + `table.rs` 两表 + `mod.rs` 计数 535→547）
- Create: `crates/wv-ui/assets/ui/tool_icons/paste.svg` + `crates/wv-ui/src/widgets/primitives.rs` 导出 `ICON_PASTE`、`ICON_TRASH_2`、`ICON_UNDO_2`（后两个资产已在，仅补导出）

**Interfaces:**
- Consumes: Task 1 `alive_roots`/`map_alive`；Task 2/4 命令；`ResMut<DrillState>` + `drill.drill_to(&projection.world, &id)`（`debug_probe.rs:84` 同款；独立新根用 `settle_on`）
- Produces:
  - `#[derive(Resource, Default)] pub struct MapTreeUiState { pub collapsed: HashSet<Id>, pub selected: Option<Id>, pub trash_open: bool }`（默认全展开）
  - `#[derive(Resource, Default)] pub struct MapClipboard { pub source: Option<Id> }`
  - 12 个 i18n 键（zh / en）：`MapTreeActionCopy 复制/Copy`、`MapTreeActionPaste 粘贴/Paste`、`MapTreeActionDelete 删除/Delete`、`MapTreeTrashSection 垃圾桶/Trash`、`MapTreeTrashEmpty 垃圾桶是空的/Trash is empty`、`MapTreeActionRestore 恢复/Restore`、`MapTreeActionPurge 彻底删除/Delete forever`、`MapTreePurgeConfirmText 彻底删除后无法恢复，确定吗？/This cannot be undone. Delete permanently?`、`MapTreeActionEmptyTrash 清空垃圾桶/Empty trash`、`MapTreeEmptyTrashConfirmText 清空后无法恢复，确定清空吗？/Empty the trash? This cannot be undone.`、`MapTreeRestoreBlockedHint 先恢复父图后才能恢复此项/Restore its parent map first`、`MapTreeLastWorldHint 仅剩一张世界图，无法删除/The last world map cannot be deleted`

UI 行为：
- 工具栏行（面板头下 28px）：`[+]`（→ `dialog.open_for(&world)`，`new_map_dialog.rs:119`）；`[粘贴]` 仅当剪贴板源是 `MapLevel::World` 时点亮 → `PasteMapCommand(target=None, anchor=(0,0))` + `drill.settle_on(created_root)`
- 树：`alive_roots()` 起递归，`collapsed` 控制 chevron；当前图行 accent 18% 高亮（`tree_row` 现款）；行点击 → `drill.drill_to`
- 右键菜单（egui `response.context_menu`，注意 Popup id 跨帧一致）：`复制`（任意存活行）→ `MapClipboard.source`；`粘贴`（门控：源 `City|Region` 且行 `level==World`、或源 `Interior` 且行 `level==City`，且**行 == view.current_map**；不满足 `enabled(false)` + tooltip 提示先切图）→ `PasteMapCommand(target=Some(row), anchor=当前视野中心换算目标图局部坐标)`（换算复用 city_intent 的屏幕→世界 util）+ 成功后 `drill.drill_to(created_root)`；`删除`（`alive_roots().len()==1 && 行是根` 时 `enabled(false)` + `MapTreeLastWorldHint`）→ 若 `view.current_map ∈ subtree` 先 `drill_to(存活首根)`，再 commit `TrashMapCommand`（trashed_at 取 `state::RecentProject` 同款时间戳形态）
- 提交一律走 `commit_layer` 同款配方（本文件内私有 `commit_map` 复制一份）

- [ ] **Step 1: 失败测试（纯逻辑）**

```rust
// map_tree.rs 内 #[cfg(test)]：门控纯函数
fn paste_menu_enabled_matches_level_and_current_map() { /* (源City,行World,当前)true；(源City,行World,非当前)false；(源Interior,行World)false */ }
fn delete_menu_enabled_blocks_last_alive_root() { /* 单根世界 → false；双根 → true */ }
fn i18n_all_length_matches_key_count() { /* 更新为 547（i18n/mod.rs 两处 assert + 注释） */ }
```

- [ ] **Step 2: 确认失败** — `cargo app-test 2>&1 | tail -3`
- [ ] **Step 3: 实现面板**（迁移 + 工具栏 + 树 + 右键 + 资源注册 + paste.svg 烘焙 `stroke="#ffffff"`）
- [ ] **Step 4: 通过 + 局部门禁** — `cargo app-test && cargo app-clippy && cargo test -p wv-ui 2>&1 | tail -3`
- [ ] **Step 5: 提交** — `git commit -m "wv-app: 地图树接真数据——点击切换/右键复制粘贴删除/新增按钮"`

---

### Task 9: 垃圾桶分区 + 两步确认

**Files:**
- Modify: `crates/wv-app/src/ui/map_tree.rs`（Task 8 的文件内追加分区）

**Interfaces:**
- Consumes: Task 3 `RestoreMapCommand`（走 commit 进栈）、Task 5 `PurgeMapCommand` + `apply_detached`、Task 1 trashed 根枚举

UI 行为：
- 分区（divider + 头行）：`ICON_TRASH_2` + `MapTreeTrashSection` + 条目数 + chevron（`trash_open`）；右侧 `MapTreeActionEmptyTrash` 小按钮（空时 `enabled(false)`）
- 条目行：源根 `name` + 相对时间（`format_time_ago` 同款格式化，`state.rs` RecentProject 先例）；按 `trashed_at` 降序
- 条目右键：`恢复`（父链非存活 → `enabled(false)` + `MapTreeRestoreBlockedHint`）；`彻底删除` → **两步行内确认**（第一次点变红 `MapTreePurgeConfirmText`，第二次才 `apply_detached(PurgeMapCommand)`；点别处复位）
- `清空垃圾桶` 同两步确认（`MapTreeEmptyTrashConfirmText`）→ 逐条 `apply_detached`

- [ ] **Step 1: 失败测试（纯逻辑）**

```rust
fn trash_entries_list_trashed_roots_newest_first() { /* 两条 trashed 根按 trashed_at 降序 */ }
fn restore_menu_disabled_while_parent_trashed() { /* 城在世界之后被删 → 城条目恢复置灰 */ }
fn purge_uses_two_step_confirmation_state_machine() { /* ConfirmArmed 状态：一次点击只进 Armed，二次才触发回调 */ }
```

- [ ] **Step 2: 确认失败** → **Step 3: 实现分区** → **Step 4: `cargo app-test 2>&1 | tail -3` 通过**
- [ ] **Step 5: 提交** — `git commit -m "wv-app: 地图树垃圾桶分区——恢复/彻底删除/清空（两步确认）"`

---

### Task 10: ADR + 术语 + 全门禁 + 实机验收

**Files:**
- Create: `docs/adr/ADR-0074-map-tree-trash-and-deep-copy.md`（**执行时先 `ls docs/adr/` 确认 0074 未被并行占用**，被占则顺延；处境/决策（spec §5 D1-D8 + §10 追记 2 条）/理由/代价 + 被否决备选：多对多挂关系、硬删无垃圾桶、独立 trash.yaml 清单、物理摘除 children/图标）
- Modify: `docs/adr/README.md`（索引）、`GLOSSARY.md`（登记：**垃圾桶** trash bin、**软删** soft delete/trashed、**深拷贝副本** deep-copy duplicate——含定义/首次出现/所属维度）

- [ ] **Step 1: 落 ADR + 索引 + GLOSSARY**
- [ ] **Step 2: 全门禁**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
cargo app-test && cargo app-clippy && cargo test -p wv-render --features app
bash scripts/smoke-gui.sh   # 新增 Resource + 系统签名改动，必跑
```

- [ ] **Step 3: 实机七步走查**（spec §9；截图/操作后 `git checkout -- examples/jrpg-world/ui-layout.yaml` 还原被记盘的显隐）
- [ ] **Step 4: 提交** — `git commit -m "docs: ADR-0074 地图树垃圾桶与深拷贝 + 术语登记"`
