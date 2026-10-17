# 场景编辑器 M1（分区刷色）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在非 World 级地图（City/Interior/Region）上用笔刷刷「场景分区」色块（贫民区/商业区/房间/走廊……），首次刷色自动引导建基底网格，可撤销，双主题色板可渲。

**Architecture:** 不新建数据层——复用每图一张的 `TerrainGrid` kind 通道（`World.terrains`）与既有 `PaintTerrainCommand`/chunk 渲染/撤销链。新增四块：`SceneType` 字段（wv-core）、场景 kind 词表+色板（wv-terrain / wv-render）、`CreateSceneBaseGridCommand` 首刷引导（wv-edit + wv-app 接线）、UI 三处（对话框不预填海洋+场景类型、笔刷面板场景词表、i18n 文案）。

**Tech Stack:** Rust 2021 / serde_yaml / Bevy 0.19（仅 wv-app feature="app" 下的 UI）

**Spec:** `docs/superpowers/specs/2026-10-17-scene-editor-design.md`（R1–R6 裁定 + §5.1/5.2 是本计划的直接依据）

## Global Constraints

- 分层红线：wv-core/wv-terrain 零 Bevy 零网络；wv-edit 只经 EditCommand 改世界；wv-render 不依赖 wv-edit/wv-app（§2.2 五条 `cargo tree` 提交前必跑）。
- **场景 kind 编号必须 > 1**（`is_water_kind(kind) = kind <= 1` 是水域唯一入口，触发即改写高度）；且不与已占用的 0..=9 冲突。本计划固定用 **16..=27**。
- 一切世界改动走 `EditCommand`（trait + before 缓存 + `Transaction`），禁止 System 直改 `World`。
- YAML round-trip 是硬契约：新增字段必须 `#[serde(default, skip_serializing_if = "Option::is_none")]` 并配 round-trip 测试。
- i18n：新文案 = `i18n/keys.rs` 变体 + `ALL` 数组 + `table.rs` 两个穷尽 match 各一条（漏 arm 编译错）。
- 依赖版本只写在根 `[workspace.dependencies]`；成员 crate 禁止写死版本。
- 提交信息 `<scope>: <动作>`；每个 Task 一提交，只 add 该任务列出的文件。
- 门禁：`cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check`，另加渲染层 `cargo test -p wv-app --features app`、`cargo clippy -p wv-app --all-targets --features app -- -D warnings`、`bash scripts/smoke-gui.sh`（M1 触及 UI 系统签名，烟测必跑）。
- ⚠️ 基线注意（2026-10-17 实况）：工作区有 4 个未提交文件，其中 `crates/wv-app/src/ui/new_map_dialog.rs` 的并行未提交改动**压在本计划 Task 7 的改动面上**。开工前与所有者确认该改动先落库/另置；执行期间禁止 `git add` 任何非本任务文件。

## Review Focus

1. **场景 kind 误入水域段（≤1）**→ 笔刷跨水陆自动改写高度，基底全毁。合理预期：16..=27 全部 `!is_water_kind`。→ Task 2 守卫测试。
2. **世界图被首刷引导误建格**→ 世界地形被基底色覆盖，等于清空大陆。合理预期：`level == World` 永不引导，维持现状报错。→ Task 6 测试。
3. **对话框 World 回归**→ 「默认海洋世界」（用户 2026-10-16 裁定）必须原样保留。合理预期：World 级仍预填海洋格 + `base: ocean`；非 World 级不预填。→ Task 7 测试。
4. **首刷撤销不彻底**→ 撤销后格子残留，再画行为怪异。合理预期：引导+首笔在**同一事务**里，一步 undo 格子消失。→ Task 6 事务测试。
5. **Dusk 主题场景色缺失**→ 切主题后分区渲染成沼泽回退色。合理预期：两套色板 12 个场景色各自可辨且不等于回退色。→ Task 3 测试。

---

### Task 1: `SceneType` 枚举与 `MapNode.scene_type` 字段

**Files:**
- Modify: `crates/wv-core/src/map/mod.rs`（枚举 + 字段；`impl MapNode` 的所有构造点/Default 同步补字段）
- Test: `crates/wv-io/tests/example_project.rs`（追加测试）

**Interfaces:**
- Produces: `wv_core::map::SceneType`（`#[serde(rename_all = "snake_case")]`，变体 `Settlement / Farmland / Forest / Dungeon` → YAML `settlement / farmland / forest / dungeon`）；`MapNode.scene_type: Option<SceneType>`。

- [ ] **Step 1: 写失败测试**（`crates/wv-io/tests/example_project.rs`，照 `map_node_orientation_round_trips_and_defaults_to_zero` :129 的风格）

```rust
#[test]
fn map_node_scene_type_round_trips_and_defaults_to_none() {
    let mut node = minimal_node(); // 文件内既有 helper；若无则按 :134 的最小 YAML 字符串风格构造
    node.scene_type = Some(wv_core::map::SceneType::Dungeon);
    let text = serde_yaml::to_string(&node).unwrap();
    let back: wv_core::map::MapNode = serde_yaml::from_str(&text).unwrap();
    assert_eq!(node, back, "MapNode 的 YAML round-trip 必须稳定");
    assert!(text.contains("scene_type: dungeon"), "缺省跳过、存在时序列化: {}", text);

    let bare: wv_core::map::MapNode =
        serde_yaml::from_str("id: dungeon_a\nname: 试炼地窟\nlevel: interior\n").unwrap();
    assert!(bare.scene_type.is_none(), "旧 YAML 无该字段必须缺省 None");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-io map_node_scene_type` → FAIL（无 `SceneType`）
- [ ] **Step 3: 实现** — `map/mod.rs`：定义 `SceneType`（放在 `MapLevel` 旁 :28-34 附近，derive 对齐 `MapLevel`）；`MapNode` 增字段 `#[serde(default, skip_serializing_if = "Option::is_none")] pub scene_type: Option<SceneType>`（放 `time_span` :83 附近）。`cargo check --workspace` 找出所有**字面量构造 MapNode** 的编译错误点（`new_map_dialog.rs`、测试 stub 等）逐一补 `scene_type: None`。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-io` → 全 PASS（含既有 round-trip）
- [ ] **Step 5: Commit** — `git add crates/wv-core/src/map/mod.rs crates/wv-io/tests/example_project.rs`（+被波及的构造点文件）`git commit -m "wv-core: MapNode 增可选 scene_type 场景类型"`

### Task 2: 场景 kind 词表常量（wv-terrain）

**Files:**
- Create: `crates/wv-terrain/src/scene.rs`
- Modify: `crates/wv-terrain/src/lib.rs`（`pub mod scene;` + re-export）
- Test: `crates/wv-terrain/src/scene.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `wv_terrain::scene::{SCENE_KIND_BASE, SCENE_KIND_SLUM, SCENE_KIND_RICH, SCENE_KIND_RESIDENTIAL, SCENE_KIND_COMMERCIAL, SCENE_KIND_INDUSTRIAL, SCENE_KIND_FARMLAND, SCENE_KIND_FOREST, SCENE_KIND_CLEARING, SCENE_KIND_ROOM, SCENE_KIND_CORRIDOR, SCENE_KIND_HALL}`（u8，依次 **16..=27**）与 `pub const ALL_SCENE_KINDS: [u8; 12]`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn scene_kinds_stay_out_of_water_and_terrain_range() {
    for &k in &ALL_SCENE_KINDS {
        assert!(k > 1, "场景 kind {} 不得落进水域段(<=1)", k);
        assert!(!crate::edit::is_water_kind(k), "kind {} 被判为水域", k);
        assert!(!(0..=9).contains(&k), "kind {} 撞世界地形段", k);
    }
    assert_eq!(ALL_SCENE_KINDS.len(), 12);
    assert_eq!(ALL_SCENE_KINDS[0], SCENE_KIND_BASE);
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-terrain scene_kinds` → FAIL（模块不存在）
- [ ] **Step 3: 实现** — 12 个 `pub const`（每个带一行中文 doc 注释：基底=未分区/贫民区/富人区/普通住宅/商业区/工业区/农田/密林/空地/房间/走廊/大殿）+ `ALL_SCENE_KINDS`。文件头注释写明设计出处 §5.1 与「编号 >1」红线。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-terrain` → PASS
- [ ] **Step 5: Commit** — `git commit -m "wv-terrain: 场景分区 kind 词表 16..=27"`

### Task 3: 场景色板（wv-render，双主题）+ 色值分派

**Files:**
- Modify: `crates/wv-render/src/theme/palette.rs`（`PaletteColors` 增 `pub scene: [Color; 12]` 字段；`core_terrain` :155-160 增场景段分派）
- Modify: `crates/wv-render/src/theme/colors.rs`（Daylight 的 `SCENE_TERRAIN`）
- Modify: `crates/wv-render/src/theme/colors_dusk.rs`（Dusk 镜像）
- Modify: `crates/wv-app` 侧瓷砖取色 `terrain_swatch`（`crates/wv-app/src/ui/shell.rs:48-58`）——若 `crate::theme::colors` 是 wv-render 的再导出则零改动；若是副本则同步加分派
- Test: `crates/wv-render/src/theme/palette.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `PaletteColors::core_terrain(kind: u8) -> Color` 对 16..=27 返回场景色；>27 与现状一样回退 `core[8]`。0..=9 行为不变。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn scene_kinds_get_dedicated_colors_in_both_palettes() {
    for p in [MapPalette::Daylight, MapPalette::Dusk] {
        let c = palette::colors(p);
        for &k in &wv_terrain::scene::ALL_SCENE_KINDS {
            assert_ne!(c.core_terrain(k), c.core[8], "palette {:?} kind {} 落回沼泽色", p, k);
        }
        assert_eq!(c.core_terrain(28), c.core[8], "未登记 kind 仍须回退");
        assert_eq!(c.core_terrain(3), c.core[3], "世界地形段行为不变");
    }
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-render scene_kinds_get` → FAIL（当前 16..=27 全部落回退色）
- [ ] **Step 3: 实现** — `colors.rs` / `colors_dusk.rs` 各加 `pub const SCENE_TERRAIN: [Color; 12]`，两主题 12 色按下表（**初始值为助手拍板，视觉验收可再调**；顺序同 ALL_SCENE_KINDS）：

| kind | 义 | Daylight | Dusk |
|---|---|---|---|
| 16 | 未分区基底 | `#b8b09a` | `#4a463c` |
| 17 | 贫民区 | `#8a7a5f` | `#3a3428` |
| 18 | 富人区 | `#cbb27a` | `#6b5a35` |
| 19 | 普通住宅 | `#d8c9a3` | `#5c5340` |
| 20 | 商业区 | `#c98a4b` | `#7a4e26` |
| 21 | 工业区 | `#8f8577` | `#413d36` |
| 22 | 农田 | `#b5b356` | `#4f4d26` |
| 23 | 密林 | `#4f7a3a` | `#243a1c` |
| 24 | 空地 | `#a3a86e` | `#454732` |
| 25 | 房间 | `#a89a8a` | `#473f36` |
| 26 | 走廊 | `#7d7268` | `#332e29` |
| 27 | 大殿 | `#b39a6b` | `#55462c` |

`PaletteColors` 增 `pub scene: [Color; 12]`，两个构造点（Daylight/Dusk）各填入对应常量；`core_terrain` 开头加：`if (16..=27).contains(&kind) { return self.scene[(kind - 16) as usize]; }`。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-render` → PASS；再 `grep -n "fn terrain_swatch" crates/wv-app/src/ui/shell.rs` 确认瓷砖取色路径吃到同一分派（副本则同步）。
- [ ] **Step 5: Commit** — `git commit -m "wv-render: 场景分区双主题色板与 kind 取色分派"`

### Task 4: i18n 文案（17 个新 Key）

**Files:**
- Modify: `crates/wv-app/src/i18n/keys.rs`（变体 + `ALL` :870，386 → 403）
- Modify: `crates/wv-app/src/i18n/table.rs`（`lookup_zh` 与 `lookup_en` 各 17 arm）

**Interfaces:**
- Produces: 12 个 `SceneKind*` Key + 4 个 `SceneType*` Key + 1 个对话框标签 Key，`zh / en` 文案固定为下表（后续任务按名引用）：

| Key | zh | en |
|---|---|---|
| SceneKindBase | 未分区 | Unzoned |
| SceneKindSlum | 贫民区 | Slums |
| SceneKindRich | 富人区 | Rich District |
| SceneKindResidential | 普通住宅 | Residential |
| SceneKindCommercial | 商业区 | Commercial |
| SceneKindIndustrial | 工业区 | Industrial |
| SceneKindFarmland | 农田 | Farmland |
| SceneKindForest | 密林 | Dense Forest |
| SceneKindClearing | 空地 | Clearing |
| SceneKindRoom | 房间 | Room |
| SceneKindCorridor | 走廊 | Corridor |
| SceneKindHall | 大殿 | Great Hall |
| SceneTypeSettlement | 聚落 | Settlement |
| SceneTypeFarmland | 农田 | Farmland |
| SceneTypeForest | 森林 | Forest |
| SceneTypeDungeon | 地下城 | Dungeon |
| NewMapSceneTypeLabel | 场景类型 | Scene type |

- [ ] **Step 1: 实现**（纯声明式，无独立测试——穷尽 match 即编译期测试）— keys.rs 每个 variant 带 doc 注释（照 `TerrainKindDeepWater` :365 风格）；`ALL` 长度 386→403。
- [ ] **Step 2: 验证** — `cargo check -p wv-app` → 无漏 arm 报错；`grep -c "SceneKind\|SceneType\|NewMapSceneType" crates/wv-app/src/i18n/keys.rs` ≥ 17。
- [ ] **Step 3: Commit** — `git commit -m "wv-app: 场景分区/场景类型 i18n 文案"`

### Task 5: `CreateSceneBaseGridCommand`（wv-edit）

**Files:**
- Modify: `crates/wv-edit/src/command.rs`（新命令 struct + impl，放 `PaintTerrainCommand` :101 附近）
- Modify: `crates/wv-edit/src/lib.rs`（re-export）
- Test: `crates/wv-edit/src/command.rs` `#[cfg(test)]`（同文件既有测试风格）

**Interfaces:**
- Consumes: `wv_terrain::scene::SCENE_KIND_BASE`（Task 2）；wv-core `World` 的地形/节点访问 API（`world.terrain_of(&id)` / `world.set_terrain(...)`——以 command.rs:129-133、:433-439 现有调用为准；**移除条目**若无现成 API，则在 `wv-core` 补 `remove_terrain(&mut self, id: &Id) -> Option<TerrainData>` 并在本任务一并测试）。
- Produces: `CreateSceneBaseGridCommand::new(map: wv_core::Id, cell_size: f32) -> Self`；`impl EditCommand`（label 中文「铺设场景基底网格」）。语义：该图 `level == World` → `Err`；已有栅格 → `Err`（新错误变体 `EditError::SceneGridExists`）；否则按 `extent / cell_size` 逐格铺 `{height: 0, kind: 16}`（extent 为 None 或尺寸非正 → 256×256 兜底；`cell_size <= 0.0` → 8.0 兜底），写入 `World.terrains`。`revert` 移除本次写入（before 恒 None，用 `Mutex<Option<()>>` 记「是否由我写入」）。脏路径 `["maps/<id>/terrain.bin"]`。

- [ ] **Step 1: 写失败测试**（手算期望值）

```rust
#[test]
fn scene_base_grid_fills_flat_unzoned_cells_from_extent() {
    let mut world = world_with_city_fixture(); // 文件内既有 fixture 风格；city 图 extent 1024×1024、无栅格
    let cmd = CreateSceneBaseGridCommand::new("city_a".into(), 8.0);
    let dirty = cmd.apply(&mut world).unwrap();
    let g = world.terrain_of(&"city_a".into()).unwrap();
    assert_eq!((g.width, g.height), (128, 128), "1024/8=128，got {}x{}", g.width, g.height);
    assert!(g.cells.iter().all(|c| c.height == 0 && c.kind == 16), "全格 height=0 kind=16");
    assert_eq!(dirty, vec!["maps/city_a/terrain.bin".to_string()]);
    cmd.revert(&mut world).unwrap();
    assert!(world.terrain_of(&"city_a".into()).is_none(), "revert 必须移除本次网格");
}

#[test]
fn scene_base_grid_refuses_world_level_and_existing_grid() { /* World 图 → Err；已有栅格图 → Err(SceneGridExists)，World.terrains 不变 */ }
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-edit scene_base_grid` → FAIL
- [ ] **Step 3: 实现**（struct 字段/转换复用 command.rs:19-44 的 `to_grid`/`from_grid` 私有函数与 `PaintTerrainCommand` 的写回模式）
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-edit` → 全 PASS
- [ ] **Step 5: Commit** — `git commit -m "wv-edit: CreateSceneBaseGridCommand 场景基底网格铺设"`

### Task 6: 首刷引导接线（wv-app editing.rs）

**Files:**
- Modify: `crates/wv-app/src/edit/editing.rs`（`commit_stroke_on_release` :88-198 内，提交前插入判定；判定逻辑抽成纯函数）
- Test: `crates/wv-app/src/edit/editing.rs` `#[cfg(test)]`（随 `--features app` 跑）

**Interfaces:**
- Consumes: Task 5 `CreateSceneBaseGridCommand::new`；`DrillState::current()`（图 id 单一真源，editing.rs:132-139 现有用法）；`Res<GridConfig>.cell_size`。
- Produces: `fn stroke_commands(level: MapLevel, has_terrain: bool, map: wv_core::Id, cell_size: f32, stroke: TerrainStroke) -> Vec<Box<dyn EditCommand>>`——`level != World && !has_terrain` → `[CreateSceneBaseGridCommand, PaintTerrainCommand]`；否则 `[PaintTerrainCommand]`（World 无栅格维持现状：提交后由 `TerrainMismatch` 报错提示，editing.rs:189-196）。提交点把现有的单命令提交改为**一个 `Transaction` 装全部命令**（一次 undo 单位）。

- [ ] **Step 1: 写失败测试**（纯函数，不起 Bevy）

```rust
#[test]
fn first_paint_on_scene_map_prepends_base_grid_in_one_transaction() {
    let cmds = stroke_commands(MapLevel::Interior, false, "dungeon_a".into(), 8.0, test_stroke());
    assert_eq!(cmds.len(), 2, "引导+首笔两命令");
    assert_eq!(cmds[0].label(), "铺设场景基底网格");

    let cmds = stroke_commands(MapLevel::Interior, true, "dungeon_a".into(), 8.0, test_stroke());
    assert_eq!(cmds.len(), 1, "已有栅格不再引导");

    let cmds = stroke_commands(MapLevel::World, false, "world_main".into(), 8.0, test_stroke());
    assert_eq!(cmds.len(), 1, "世界图永不引导，维持现状报错路径");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-app --features app first_paint_on_scene_map` → FAIL
- [ ] **Step 3: 实现** — 抽 `stroke_commands` 并改 `commit_stroke_on_release` 走 `Transaction { commands: stroke_commands(...) }` 一次 commit。
- [ ] **Step 4: 事务级验证** — 追加一个走 `History::commit` 的集成测试：city 图无栅格 → 提交 → `terrain_of` 存在且笔画处 kind 正确；`history.undo()` 一步 → `terrain_of` 回到 None。跑 `cargo test -p wv-app --features app` → 全 PASS。
- [ ] **Step 5: Commit** — `git commit -m "wv-app: 场景图首刷自动铺设基底网格（同事务可一步撤销）"`

### Task 7: 新建地图对话框——非 World 不预填 + 场景类型选择

**Files:**
- Modify: `crates/wv-app/src/ui/new_map_dialog.rs`（状态 :80-99 增 `scene_type`；组装 :398-424 分流）
- Test: `crates/wv-app/src/ui/new_map_dialog.rs` `#[cfg(test)]`（沿用该文件 UI/apply 拆分后的 apply 侧测试风格）

**Interfaces:**
- Consumes: Task 1 `SceneType`；Task 4 `NewMapSceneTypeLabel` 等文案。
- Produces: 对话框行为——`level == World`：**现状不变**（默认 Ocean 预填栅格、`base: Some(Ocean)`、无场景类型）；`level != World`：`AddMapCommand::new(node, None)`（**不预填栅格**，渲染期沿父链继承见 terrain_resolve.rs:101-114）、`node.base = None`、`node.scene_type = Some(选中值)`。场景类型下拉仅非 World 显示，缺省按层级：Interior → `Dungeon`，其余 → `Settlement`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn non_world_dialog_omits_terrain_and_sets_scene_type() {
    let cmds = build_dialog_commands(dialog_fixture(MapLevel::Interior)); // apply 侧既有入口/fixture，按实况命名
    let add = only_add_map(&cmds);
    assert!(add.terrain.is_none(), "非 World 不得预填栅格");
    assert!(add.node.base.is_none(), "非 World 无重置锚点");
    assert_eq!(add.node.scene_type, Some(SceneType::Dungeon), "Interior 缺省地下城");
}

#[test]
fn world_dialog_keeps_ocean_prefill_unchanged() {
    let cmds = build_dialog_commands(dialog_fixture(MapLevel::World));
    let add = only_add_map(&cmds);
    let t = add.terrain.as_ref().unwrap();
    assert!(t.cells.iter().all(|c| c.kind == 0 && c.height == -3500), "默认海洋裁定不得回归");
    assert_eq!(add.node.base, Some(MapBase::Ocean));
    assert_eq!(add.node.scene_type, None);
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-app --features app dialog` → 按实况（若该文件无测试先例，测试放同文件新 `#[cfg(test)]`，fixture 手工构造 `NewMapDialog` 状态）
- [ ] **Step 3: 实现**（下拉控件用项目标准 ComboBox 工厂；文案接 Task 4 Key）
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-app --features app` → 全 PASS
- [ ] **Step 5: Commit** — `git commit -m "wv-app: 新建地图非 World 级不预填海洋并增场景类型"`

### Task 8: 笔刷面板场景词表模式

**Files:**
- Modify: `crates/wv-app/src/ui/widgets/primitives.rs`（`terrain_grid` :333-370 泛化出 `kind_grid(ui, shell, selected: &mut u8, loc, kinds: &[u8])`；原 `terrain_grid` 改为传 `&TERRAIN_KIND_ALL`（0..=10）的薄封装，既有调用点零改动）
- Modify: `crates/wv-app/src/ui/shell.rs`（`scene_kinds_for(scene_type: Option<SceneType>) -> &'static [u8]` + `scene_kind_name(loc, kind: u8) -> &'static str`，与 `TERRAIN_KEYS` :26-45 同风格）
- Modify: `crates/wv-app/src/ui/brush_panel.rs`（:154-171 调用点：当前图 `level != World` 时改走场景词表）
- Test: `crates/wv-app/src/ui/shell.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 2 常量、Task 3 取色（`terrain_swatch` 自动生效）、Task 4 文案。
- Produces: `scene_kinds_for` 固定映射：`Some(Settlement) → [16,17,18,19,20,21,22]`；`Some(Farmland) → [16,22]`；`Some(Forest) → [16,23,24]`；`Some(Dungeon) → [16,25,26,27]`；`None → ALL_SCENE_KINDS` 全 12 个。选中名显示用 `scene_kind_name`。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn scene_vocabulary_is_filtered_by_scene_type() {
    assert_eq!(scene_kinds_for(Some(SceneType::Dungeon)), &[16u8, 25, 26, 27]);
    assert_eq!(scene_kinds_for(Some(SceneType::Settlement)), &[16u8, 17, 18, 19, 20, 21, 22]);
    assert_eq!(scene_kinds_for(None).len(), 12, "未选类型给全词表");
    assert_eq!(scene_kind_name(loc_zh(), 25), "房间");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-app --features app scene_vocabulary` → FAIL
- [ ] **Step 3: 实现**（brush_panel 查当前图：`drill.current()` → `world.node(...)`，取 `level` 与 `scene_type`；场景模式下瓷砖行与选中名都走场景函数；`kind_grid` 5 列布局与 26px 尺寸照抄原实现）
- [ ] **Step 4: 跑测试确认通过 + 实机验收** — `cargo test -p wv-app --features app` → PASS；`cargo run --features app` 手动验收：进 Interior 图→瓷砖只有 未分区/房间/走廊/大殿→刷出一块房间色→换 Dusk 主题色变。
- [ ] **Step 5: Commit** — `git commit -m "wv-app: 笔刷面板按场景类型切换分区词表"`

### Task 9: 收口——术语登记 + 全门禁 + 烟测

**Files:**
- Modify: `GLOSSARY.md`（追加 3 词条，**只增不改**）
- 无代码改动

**Interfaces:** 无（收口任务）

- [ ] **Step 1: GLOSSARY 追加词条**（格式照既有条目，含首次出现位置指向设计文档）：
  - **场景分区**（Scene District / zone）——场景图上按 kind 刷出的功能色块；载体是 TerrainGrid 的 kind 通道，词表 `wv_terrain::scene`。
  - **场景类型**（SceneType）——MapNode 可选字段（settlement/farmland/forest/dungeon），驱动笔刷词表与生成器参数集。
  - **基底网格**（Scene Base Grid）——场景图首次刷色时自动铺设的全 `kind=16`、`height=0` 平坦网格。
- [ ] **Step 2: 全门禁**（逐条跑，全绿才算过）
  ```bash
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  cargo test -p wv-app --features app
  cargo clippy -p wv-app --all-targets --features app -- -D warnings
  cargo tree -p wv-core | grep -i bevy          # 应输出空
  cargo tree -p wv-terrain | grep -i bevy       # 应输出空
  cargo tree -p wv-render | grep -E 'wv-edit|wv-app'  # 应输出空
  bash scripts/smoke-gui.sh                      # 5s 存活 + 日志无 panic/B000x
  ```
- [ ] **Step 3: 手动验收清单**（WV_SHOT 取图按 memory 配方）：新建 Interior 图（无海洋底）→ 双击钻入 → 父图地形随父链继承可见 → 首刷自动出基底色 → 刷房间/走廊 → 一步撤销连格带笔全退 → 重画 → 保存重开工程，色块仍在（round-trip 落盘）。
- [ ] **Step 4: Commit** — `git add GLOSSARY.md && git commit -m "docs: 场景分区/场景类型/基底网格术语登记"`

## M1 明确不做（留给后续里程碑）

- 属性面板改 `scene_type`（M1 只在建图时定；改错就删图重建，或并入 M2）
- 生成器（M2）、标记（M3）、道路档位（M4）、存在时间段（M5）
- `ResetTerrainToBaseCommand` 对场景图的适配（场景图不暴露该功能）
