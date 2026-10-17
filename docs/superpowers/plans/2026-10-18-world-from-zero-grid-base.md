# 地图底座三通道化（wv-terrain/wv-core/wv-io/wv-edit/wv-render/wv-app）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把地形格子从「高度 + kind(u8)」双通道迁移为设计稿 D11/D12 的三通道——`surface`（水/陆二值）+ `height`（纯海拔）+ `biome`（生物群系 u8 开放集）——并保持二进制 v1 旧文件可读、渲染与笔刷行为不回归。

**Architecture:** `TerrainCell` 换字段（`kind` 删除，`surface`/`biome` 顶上），wv-terrain 内部全部写入点同步迁移；渲染色与既有 10 槽调色板**零改动**——新增纯函数 `display_kind(cell)` 把三通道投影回旧 0–9 槽位，wv-render 仅在取色入口换一处调用。二进制升 version 2（载荷 `(i16,u8,u8)`），v1 解码分支按「**authoring 真值**」迁移（kind≤1→水、其余→陆+对应 biome，不看高度）。势力交互与标记段**不在本计划**，待本底座落地后另立计划。

**Tech Stack:** Rust 2021，**零新增依赖**（wv-terrain 保持零第三方；zstd/bincode 已在 wv-io）。

**Spec:** `docs/superpowers/specs/2026-10-18-world-from-zero-map-design.md`（D11/D12/D14/D16/D18/D19 + §3 迁移表）+ `docs/BINARY_FORMAT_SPEC.md` §3（版本策略已预写「新增第三通道→升 version 保留旧解码分支」）+ GLOSSARY「生物群系/生物群系标记」。

## Global Constraints

- `wv-terrain` 零第三方依赖（AGENTS §2.1）；坐标一律 `(f32, f32)`，禁 glam。
- 高度契约冻结：`i16`，1 单位 = 1 米，海平面 = 0（plan v0.1 §8 Q1）。
- `Surface`/`Biome` 均为 `u8` 开放集常量（沿 `TerrainKind` 先例，AGENTS 精神：编号→名称→色值由上层承载）。`BIOME_ROAD = 20` 是 19 清单之外**唯一**的工程扩展（道路非自然群系，保住既有道路笔刷），注释必须写明。
- 水的判读分两层，**不得混用**：authoring 真值 = `surface == SURFACE_WATER`（湖在 +10 m 仍是水）；湿判定 = `is_water_cell = surface==WATER || height<0`（渲染/笔刷域，陆格被塑到海下仍按湿处理——v1 行为保持）。
- 二进制纪律（AGENTS §5）：`TERRAIN_FORMAT_VERSION` 1→2；`docs/BINARY_FORMAT_SPEC.md` 按 §6.3 **追写** v2 节与版本表行，不改 v1 记载；v1 解码分支永久保留。
- 迁移方向：**只读迁移**（读 v1 → 内存 v2 → 下次保存写 v2 + `RasterRef.format_version: 2`），不改写旧文件语义。
- 测试名完整英文短句；数值断言附实际值（AGENTS §7.3/§7.4）；提交 `<scope>: <动作>`。
- **中间态纪律**：Task 1–6 必须在同一分支连续执行（`kind` 字段删除会瞬时打断下游 crate 编译）；Task 3 起逐 crate 恢复门禁，Task 6 收口全工作区；**禁止中间态合入 main**。Task 4 与 Task 5 无相互依赖，可并行。
- 渲染/UI 任务（Task 5/6）收口必跑 `cargo app-test`、`cargo app-clippy`、`bash scripts/smoke-gui.sh`（AGENTS §7.1）。

## Review Focus

1. **v1 迁移语义等价**：8 种旧 kind + 笔刷扩展 8/9 逐格迁移后，「湖在正海拔仍是水、陆在海下仍按湿」两向都不破——Task 3 表驱动测试钉死。
2. **display_kind 阈值边界**：300/1200/2600 米恰好落槽（含边界值本身），biome 优先于高度回退——Task 2 测试。
3. **笔刷跨海平面域对齐不回退**：画水推到 -40 m、画陆推到 +40 m（v1 `PAINT_SEA_DEPTH`/`PAINT_LAND_HEIGHT` 行为照旧）——Task 4 测试。
4. **round-trip 双稳定**：v2 encode/decode 逐格相等；v1 读入→v2 保存→再读逐格相等（幂等）——Task 3 测试。
5. **渲染零回归**：display 槽位即旧 kind 视觉，色板/图集数组不动；smoke-gui 日志无 panic/B000x——Task 5/6 收口。

---

### Task 1: TerrainCell 三通道化（wv-terrain 全量内部迁移）

**Files:**
- Modify: `crates/wv-terrain/src/lib.rs`（`Surface` 常量 + `TerrainCell` 换字段）
- Modify: `crates/wv-terrain/src/edit.rs`（`is_water_kind` 删除、笔刷域判定改 surface）
- Modify: `crates/wv-terrain/src/intent.rs`（`is_water_cell` 改首项、全部 `KIND_*` 写入点迁移）
- Modify: `crates/wv-terrain/src/continent.rs`（`band_kind` → `band_surface_biome`，`KIND_*` 写入点迁移）
- Test: 上述各文件既有 `#[cfg(test)]` 模块内更新

**Interfaces:**
- Produces:
  - `pub type Surface = u8;` `pub const SURFACE_WATER: Surface = 0;` `pub const SURFACE_LAND: Surface = 1;`（lib.rs，随 `TerrainKind` 旁）
  - `pub struct TerrainCell { pub height: Height, pub surface: Surface, pub biome: Biome }`，`Default = (0, WATER, NONE)`（全零格 = 海平面深海，与旧 kind=0 语义一致）
  - `pub fn is_water_cell(c: TerrainCell) -> bool`（edit.rs，接替 `is_water_kind` 成为唯一湿判定入口；`pub fn is_water_kind` **删除**）
  - `pub(crate) fn band_surface_biome(...) -> (Surface, Biome)`（continent.rs，替换 `band_kind`；Task 2 才补 `Biome` 常量，本任务先在 lib.rs 放最小占位 `pub type Biome = u8; pub const BIOME_NONE: Biome = 0;`，Task 2 扩充）
- Consumes: 无（首个任务）。

- [ ] **Step 1: 写失败测试**（在 intent.rs 测试模块追加）

```rust
#[test]
fn lake_surface_water_survives_positive_height() {
    // 山上挖湖：碗底海拔为正，surface 仍为 WATER（D14 语义派的通道基础）
    let mut grid = TerrainGrid::new(64, 64);
    // 先铺一块 200 m 高的陆地（surface LAND）
    for gy in 0..64 { for gx in 0..64 {
        grid.set(gx, gy, TerrainCell { height: 200, surface: SURFACE_LAND, biome: BIOME_NONE });
    }}
    apply_intent(&mut grid, &Intent::Lake { center: (32.0, 32.0), radius: 8.0, strength: 1.0 }, 7, 1.0);
    let c = grid.get(32, 32).unwrap();
    assert_eq!(c.surface, SURFACE_WATER, "got surface {}", c.surface);
    assert!(c.height > 0, "got {}", c.height);
}

#[test]
fn lowered_land_below_sea_level_stays_land_channel_but_reads_wet() {
    let mut grid = TerrainGrid::new(64, 64);
    for gy in 0..64 { for gx in 0..64 {
        grid.set(gx, gy, TerrainCell { height: 100, surface: SURFACE_LAND, biome: BIOME_NONE });
    }}
    paint(&mut grid, &Brush::raise_lower((32.0, 32.0), 8.0, -500.0), 1.0);
    let c = grid.get(32, 32).unwrap();
    assert_eq!(c.surface, SURFACE_LAND, "got surface {}", c.surface);
    assert!(is_water_cell(*c), "height {} should read wet", c.height);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-terrain`
Expected: FAIL——`TerrainCell` 无 `surface` 字段（编译错误即预期的「失败」形态）。

- [ ] **Step 3: 实现**

1. lib.rs：`Surface` 常量 + `Biome` 占位 + `TerrainCell` 换字段（Default 派生保持全零 = 深海）。
2. edit.rs：删 `is_water_kind`；`is_water_cell(c) = c.surface == SURFACE_WATER || c.height < 0` 移入并 `pub`；`apply_brush` 的域对齐（原 :186）改 `brush.target.surface == SURFACE_WATER`（`Brush.kind: Option<TerrainKind>` 本任务先改为 `Brush.target: Option<PaintTarget>`，`pub struct PaintTarget { pub surface: Surface, pub biome: Biome }`；`Brush::paint(center, radius, target: PaintTarget)` 签名同步）。
3. intent.rs：`is_water_cell` 改调 edit.rs 版；`KIND_WATER_SHALLOW` 写入 → `surface: SURFACE_WATER, biome: BIOME_NONE`；山/丘陵/草原肩写入 → `(SURFACE_LAND, BIOME_NONE / BIOME_NONE / BIOME_GRASSLAND)`（草原肩保留草原真值，v1 即如此）。
4. continent.rs：`band_kind` 改名 `band_surface_biome` 返回 `(Surface, Biome)`——雪/山/丘 → `(LAND, NONE)`（高度派生，D16）、深水 → `(WATER, NONE)`、浅水 shelf → `(WATER, NONE)`、林 → `(LAND, BIOME_FOREST)`、沙 → `(LAND, BIOME_DESERT)`、草 → `(LAND, BIOME_GRASSLAND)`；岛链 `(LAND, BIOME_NONE)`。 forest/sand/grass 三常量本任务在占位段一并给出（`BIOME_FOREST=1, BIOME_GRASSLAND=3, BIOME_DESERT=5`——编号表 Task 2 冻结，此处先按表取值）。
5. 既有测试全部按新字段机械改写（`kind: 3` → `biome: BIOME_GRASSLAND` 等），断言语义不变。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-terrain && cargo clippy -p wv-terrain --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS（wv-terrain 自身全绿；下游 crate 编译中断属预期，见 Global Constraints 中间态纪律）

- [ ] **Step 5: Commit**

```bash
git add crates/wv-terrain
git commit -m "wv-terrain: TerrainCell 三通道化（surface/height/biome）+ is_water_cell 唯一湿判定"
```

---

### Task 2: Biome 常量冻结 + display_kind 投影（wv-terrain，纯增量）

**Files:**
- Create: `crates/wv-terrain/src/biome.rs`
- Modify: `crates/wv-terrain/src/lib.rs`（`pub mod biome;` + `pub use biome::*;`）
- Test: `crates/wv-terrain/src/biome.rs` 测试模块

**Interfaces:**
- Consumes: Task 1 的 `TerrainCell { height, surface, biome }`、`Surface` 常量、continent.rs 阈值常量（`SNOW_LINE_M=2600 / MOUNTAIN_MIN_M=1200 / HILL_MIN_M=300 / SHALLOW_WATER_MAX_DEPTH_M=-200`，本任务内改 `pub(crate)`）。
- Produces:
  - `pub const BIOME_NONE: Biome = 0;` 及 21 常量：`FOREST=1, PLAINS=2, GRASSLAND=3, GOBI=4, DESERT=5, SWAMP=6, WETLAND=7, TUNDRA=8, SNOWFIELD=9, JUNGLE=10, TROPICAL_RAINFOREST=11, HILLS=12, MOUNTAINS=13, OCEAN=14, LAKE=15, RIVER=16, OASIS=17, SAVANNA=18, SALT_FLAT=19, ROAD=20`（ROAD 注释：非自然群系的工程扩展，保道路笔刷）
  - `pub fn surface_for_biome(b: Biome) -> Surface`（OCEAN/LAKE/RIVER → WATER，其余 LAND）
  - `pub fn display_kind(c: &TerrainCell) -> u8`（三通道 → 旧 0–9 调色板槽位；湿格 → 高度 < -200 ? 0 : 1；陆格 biome 映射见 Step 3 表；`NONE` 落高度回退 2600/1200/300）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn display_kind_maps_each_land_biome_to_legacy_palette_slot() {
    let cell = |b: Biome| TerrainCell { height: 100, surface: SURFACE_LAND, biome: b };
    assert_eq!(display_kind(&cell(BIOME_DESERT)), 2);
    assert_eq!(display_kind(&cell(BIOME_GOBI)), 2);
    assert_eq!(display_kind(&cell(BIOME_SALT_FLAT)), 2);
    assert_eq!(display_kind(&cell(BIOME_GRASSLAND)), 3);
    assert_eq!(display_kind(&cell(BIOME_PLAINS)), 3);
    assert_eq!(display_kind(&cell(BIOME_SAVANNA)), 3);
    assert_eq!(display_kind(&cell(BIOME_OASIS)), 3);
    assert_eq!(display_kind(&cell(BIOME_FOREST)), 4);
    assert_eq!(display_kind(&cell(BIOME_JUNGLE)), 4);
    assert_eq!(display_kind(&cell(BIOME_TROPICAL_RAINFOREST)), 4);
    assert_eq!(display_kind(&cell(BIOME_HILLS)), 5);
    assert_eq!(display_kind(&cell(BIOME_MOUNTAINS)), 6);
    assert_eq!(display_kind(&cell(BIOME_SNOWFIELD)), 7);
    assert_eq!(display_kind(&cell(BIOME_TUNDRA)), 7);
    assert_eq!(display_kind(&cell(BIOME_SWAMP)), 8);
    assert_eq!(display_kind(&cell(BIOME_WETLAND)), 8);
    assert_eq!(display_kind(&cell(BIOME_ROAD)), 9);
}

#[test]
fn display_kind_falls_back_to_height_bands_when_biome_none() {
    let cell = |h: i16| TerrainCell { height: h, surface: SURFACE_LAND, biome: BIOME_NONE };
    assert_eq!(display_kind(&cell(299)), 3, "got below hill line");
    assert_eq!(display_kind(&cell(300)), 5, "got at hill line");
    assert_eq!(display_kind(&cell(1199)), 5);
    assert_eq!(display_kind(&cell(1200)), 6, "got at mountain line");
    assert_eq!(display_kind(&cell(2599)), 6);
    assert_eq!(display_kind(&cell(2600)), 7, "got at snow line");
}

#[test]
fn wet_cells_take_water_slots_by_depth_and_biome_beats_height() {
    let deep = TerrainCell { height: -3000, surface: SURFACE_WATER, biome: BIOME_NONE };
    let lake_on_mountain = TerrainCell { height: 10, surface: SURFACE_WATER, biome: BIOME_LAKE };
    assert_eq!(display_kind(&deep), 0);
    assert_eq!(display_kind(&lake_on_mountain), 1);
    // 沉入海底的陆格（塑笔刷产物）按湿渲染
    let sunken = TerrainCell { height: -100, surface: SURFACE_LAND, biome: BIOME_NONE };
    assert_eq!(display_kind(&sunken), 1);
}

#[test]
fn surface_for_biome_classifies_water_biomes() {
    assert_eq!(surface_for_biome(BIOME_OCEAN), SURFACE_WATER);
    assert_eq!(surface_for_biome(BIOME_LAKE), SURFACE_WATER);
    assert_eq!(surface_for_biome(BIOME_RIVER), SURFACE_WATER);
    assert_eq!(surface_for_biome(BIOME_FOREST), SURFACE_LAND);
    assert_eq!(surface_for_biome(BIOME_NONE), SURFACE_LAND);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-terrain biome`
Expected: FAIL——`biome` 模块不存在。

- [ ] **Step 3: 实现 `biome.rs`**

常量按 Interfaces 冻结。`display_kind` 映射表（唯一权威，渲染/工具一律经它）：
- 湿判定（`surface==WATER || height<0`）→ `height < SHALLOW_WATER_MAX_DEPTH_M(-200) ? 0 : 1`；
- 陆格按 biome：`DESERT|GOBI|SALT_FLAT→2`；`PLAINS|GRASSLAND|SAVANNA|OASIS→3`；`FOREST|JUNGLE|TROPICAL_RAINFOREST→4`；`HILLS→5`；`MOUNTAINS→6`；`SNOWFIELD|TUNDRA→7`；`SWAMP|WETLAND→8`；`ROAD→9`；`OCEAN|LAKE|RIVER→`湿槽位（按高度）；`NONE→`高度回退（`>=2600→7, >=1200→6, >=300→5, else 3`）。
- continent.rs 四个阈值常量改 `pub(crate)` 供本模块引用，编号来源注释指向设计稿 §6.1 清单。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-terrain && cargo clippy -p wv-terrain --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/wv-terrain
git commit -m "wv-terrain: biome 21 常量冻结 + display_kind 三通道→调色板槽位投影"
```

---

### Task 3: 二进制 v2 + v1 读迁移（wv-core + wv-io）

**Files:**
- Modify: `crates/wv-core/src/lib.rs:341-355`（`TerrainCellData` 换字段）
- Modify: `crates/wv-io/src/terrain_bin.rs`（version 2 编解码 + v1 迁移分支）
- Modify: `crates/wv-io/src/diagnostics.rs:311-366`（`ring_has_feature_cell` 的 kind 比较改 biome）
- Modify: `docs/BINARY_FORMAT_SPEC.md`（追写 v2 节 + 版本表行）
- Test: `crates/wv-io/src/terrain_bin.rs` 测试模块

**Interfaces:**
- Consumes: Task 1/2 的 `Surface`/`Biome` 常量（wv-io 依赖 wv-core 与 wv-terrain？——**否**：wv-io 只依赖 wv-core。迁移表在本模块**独立落一份** `fn migrate_v1_cell(kind: u8, height: i16) -> (u8, u8)`，注释指向 wv-terrain::biome 冻结表，两表一致性由 Task 3 测试钉死）
- Produces:
  - `pub struct TerrainCellData { pub height: i16, pub surface: u8, pub biome: u8 }`（wv-core，serde 兼容：旧 YAML 不内联格子，无迁移面）
  - `TERRAIN_FORMAT_VERSION: u32 = 2`；`decode_terrain` 接受 version 1（迁移解码）与 2，其余拒绝
  - `encode_terrain` 载荷 `zstd(bincode(Vec<(i16, u8, u8)>))`，`write_terrain` 回写 `RasterRef.format_version: 2`

- [ ] **Step 1: 写失败测试**（terrain_bin.rs 测试模块）

```rust
#[test]
fn v1_payload_migrates_each_kind_to_authoring_truth() {
    // 表驱动：8 旧 kind + 笔刷扩展 8/9 → (surface, biome)；不看高度
    let cases: [(u8, u8, u8); 10] = [
        (0, 0, 0), (1, 0, 0),                       // 深水/浅水 → (WATER, NONE)
        (2, 1, 5), (3, 1, 3), (4, 1, 1),            // 沙→DESERT 草→GRASSLAND 林→FOREST
        (5, 1, 0), (6, 1, 0), (7, 1, 0),            // 丘陵/山地/雪 → (LAND, NONE) 高度派生
        (8, 1, 6), (9, 1, 20),                      // 沼泽→SWAMP 道路→ROAD
    ];
    for (kind, surface, biome) in cases {
        let bytes = v1_encode_for_test(4, 4, &[(kind, 100); 16]);
        let data = decode_terrain(&bytes).unwrap();
        let c = &data.cells[0];
        assert_eq!((c.surface, c.biome), (surface, biome), "kind {} got {:?}", kind, (c.surface, c.biome));
    }
}

#[test]
fn v1_lake_at_positive_height_stays_water_after_migration() {
    // kind=1（浅水）+ 高度 +10：authoring 真值是水，不因正海拔翻案
    let bytes = v1_encode_for_test(2, 2, &[(1, 10); 4]);
    let data = decode_terrain(&bytes).unwrap();
    assert_eq!(data.cells[0].surface, 0, "got {:?}", data.cells[0]);
}

#[test]
fn v1_read_resave_v2_round_trips_stably() {
    // 10 种 kind 全覆盖、高度跨正负：迁移 → 保存 → 再读必须逐格相等（幂等）
    let v1_cells: Vec<(i16, u8)> = (0..16).map(|i| (i as i16 * 7 - 50, i as u8 % 10)).collect();
    let v1 = v1_encode_for_test(4, 4, &v1_cells);
    let once = decode_terrain(&v1).unwrap();
    let v2 = encode_terrain(&once).unwrap();
    let twice = decode_terrain(&v2).unwrap();
    assert_eq!(once.cells, twice.cells, "migration must be idempotent");
}

#[test]
fn unsupported_version_is_rejected() { /* 既有测试保留：version 99 仍拒，错误信息含 "99" */ }
```

（`v1_encode_for_test(4, 4, &[(kind, height); n])` 是测试模块内的私有助手：按 v1 布局手工拼 `zstd(bincode(Vec<(i16,u8)>))` + version=1 头。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-io terrain`
Expected: FAIL——version 1 头被现解码拒绝 / 字段不存在。

- [ ] **Step 3: 实现**

1. wv-core：`TerrainCellData { height, surface, biome }`（`surface`/`biome` 不加 serde default——二进制载荷不经 serde 路径命名兼容，`TerrainData` 整体仅在内存与 bincode 出现）。
2. terrain_bin.rs：`TERRAIN_FORMAT_VERSION = 2`；`decode_terrain` 按 header version 分支——2 → `Vec<(i16,u8,u8)>` 直读；1 → `Vec<(i16,u8)>` 逐格 `migrate_v1_cell`（authoring 真值表，见测试；注释注明与 `wv_terrain::biome` 冻结表对齐、由测试钉死）；其余版本照旧拒绝。`encode_terrain` 写三字段；`write_terrain` 的 `RasterRef.format_version` 写 2。
3. diagnostics.rs `ring_has_feature_cell`：`cell.kind != kind` 改为 biome 比较（feature-territory 提示语义不变）。
4. `docs/BINARY_FORMAT_SPEC.md`：§2 后**追加**「§2.1 `format_version = 2`（2026-10-18）」小节（载荷 `(i16,u8,u8)`、v1 迁移表全文）；§3 版本表加一行 `| 2 | 三通道（surface/biome） | 读 1 迁移，写 2 |`——不改 v1 原文（AGENTS §6.3）。
5. 既有测试更新：`header_carries_magic_version_and_dimensions` 断言 version==2；`encode_decode_round_trips_exactly` 等按新字段机械改。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-core -p wv-io && cargo clippy -p wv-core -p wv-io --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/wv-core crates/wv-io docs/BINARY_FORMAT_SPEC.md
git commit -m "wv-io: 地形二进制 v2（三通道载荷）+ v1 只读迁移分支 + 格式规格追写"
```

---

### Task 4: 笔刷与编辑命令通道迁移（wv-edit）

**Files:**
- Modify: `crates/wv-edit/src/command.rs`（`to_grid`/`from_grid` :19-48、`PaintTerrainCommand` 测试 :2627-2680）
- Test: `crates/wv-edit/src/command.rs` 测试模块

**Interfaces:**
- Consumes: Task 1 `Brush { target: Option<PaintTarget>, .. }`、`PaintTarget { surface, biome }`；Task 3 `TerrainCellData { height, surface, biome }`。
- Produces: `PaintTerrainCommand` 行为不变（签名不动），通道语义更新——`to_grid`/`from_grid` 三字段逐格拷贝。

- [ ] **Step 1: 写失败测试**（改造既有 `paint_command_apply_then_revert_restores_grid` :2627 并新增）

```rust
#[test]
fn paint_command_writes_surface_and_biome_then_revert_restores() {
    // 原 kind==3 断言改为：biome==BIOME_GRASSLAND && surface==SURFACE_LAND
    // revert 后三字段逐格回到 before 快照
}

#[test]
fn water_target_pushes_height_below_sea_level() {
    // Brush::paint(.., PaintTarget { surface: SURFACE_WATER, biome: BIOME_NONE })
    // 施加后格高 < 0（PAINT_SEA_DEPTH=-40 域对齐保持）
}

#[test]
fn land_target_pushes_height_above_sea_level() {
    // 对称：land target 施加于海底 → 高度 > 0（PAINT_LAND_HEIGHT=40）
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-edit paint`
Expected: FAIL——`TerrainCellData` 无 `kind` 字段（编译错误）。

- [ ] **Step 3: 实现**

`to_grid`/`from_grid` 改三字段拷贝；测试按 Step 1 断言改写（`BIOME_GRASSLAND` 经 `wv_terrain::biome` 引用）。`IntentTerrainCommand`/`GenerateContinentCommand` 不改（走 `apply_intent`/`generate_continents`，Task 1 已迁移）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-edit && cargo clippy -p wv-edit --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/wv-edit
git commit -m "wv-edit: 笔刷命令三通道拷贝 + 画水/画陆域对齐测试钉死"
```

---

### Task 5: 渲染接入 display_kind（wv-render，与 Task 4 可并行）

**Files:**
- Modify: `crates/wv-render/src/terrain_view/cells.rs`（:112-123 湿判定包装、:160-161 水梯度、:500/:668 取 kind 两处）
- Test: 既有测试模块（若有）+ 渲染门禁

**Interfaces:**
- Consumes: Task 2 `display_kind(&TerrainCell) -> u8`、Task 1 `is_water_cell`。
- Produces: 色板/图集**零改动**（`PaletteColors.core: [Color; 10]`、`TileAtlas` 原样），取色入口统一走投影。

- [ ] **Step 1: 改造并自证**

1. cells.rs 内私有 `is_water_kind` 包装（:112-114）删除，四处调用改 `is_water_cell(cell)`（湿判定语义与 v1 逐点一致）。
2. `:500` 与 `:668` 的 `let kind = cell.kind` → `let kind = display_kind(cell)`。
3. `:160-161` 水梯度端点（kinds 1/0）改由 `display_kind` 结果驱动，不读通道。
4. `shade_*` 系列签名**不动**（仍收 `kind: u8` 槽位）——投影发生在入口一处。

- [ ] **Step 2: 跑渲染门禁**

Run: `cargo test -p wv-render && cargo clippy -p wv-render --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/wv-render
git commit -m "wv-render: 取色入口统一走 display_kind 投影，色板零改动"
```

---

### Task 6: wv-app 收口 + 全门禁 + 烟测

**Files:**
- Modify: `crates/wv-app/src/edit/editing.rs`（:162 Brush 构造、:475-480/:970 格子拷贝）
- Modify: `crates/wv-app/src/ui/shell.rs`（:21-32 `TERRAIN_KEYS` 旁新增笔刷目标表）
- Modify: `crates/wv-app/src/ui/brush_panel.rs`（:165 目标查名）
- Modify: `crates/wv-app/src/ui/new_project_dialog.rs`（:538/:573 断言）
- Modify: `crates/wv-app/src/ui/new_map_dialog.rs`（:868-871 断言）
- Test: 各文件测试模块 + 全工作区门禁

**Interfaces:**
- Consumes: Task 1/2/4 全部产物。
- Produces: **笔刷目标表**（唯一权威，10 个色板 → `PaintTarget`）：

| 色板（TERRAIN_KEYS 序） | PaintTarget |
|---|---|
| 深水 / 浅水 | `(SURFACE_WATER, BIOME_NONE)` |
| 海滩 | `(SURFACE_LAND, BIOME_DESERT)` |
| 草原 | `(SURFACE_LAND, BIOME_GRASSLAND)` |
| 森林 | `(SURFACE_LAND, BIOME_FOREST)` |
| 丘陵 | `(SURFACE_LAND, BIOME_HILLS)` |
| 山地 | `(SURFACE_LAND, BIOME_MOUNTAINS)` |
| 雪原 | `(SURFACE_LAND, BIOME_SNOWFIELD)` |
| 沼泽 | `(SURFACE_LAND, BIOME_SWAMP)` |
| 道路 | `(SURFACE_LAND, BIOME_ROAD)` |

（`TERRAIN_KEYS` 十个显示名**原样保留**——它是显示词汇表，不是数据通道；丘陵/山地/雪原既可笔刷写 biome（D9 标记同款语义）也可由高度派生显示，两路并存即 D16。）

- [ ] **Step 1: 写失败测试**

```rust
// new_map_dialog.rs 既有断言改造：
#[test]
fn ocean_prefill_is_water_surface_and_land_prefill_is_land() {
    // ocean.kind <= 1  →  ocean.surface == SURFACE_WATER
    // land.kind > 1    →  land.surface == SURFACE_LAND
}

// shell.rs 新增：
#[test]
fn brush_target_table_covers_all_ten_swatches() {
    // 10 个 TERRAIN_KEYS 每项都有 PaintTarget；水板 surface==WATER，其余 LAND
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo app-test`
Expected: FAIL——`kind` 字段不存在。

- [ ] **Step 3: 实现**

1. editing.rs：`Brush { kind: Some(brush.kind) }` → `target: Some(target)`（来自目标表）；格子拷贝改三字段。
2. shell.rs：目标表 `const BRUSH_TARGETS: [PaintTarget; 10]`（按 TERRAIN_KEYS 序，值见上表）。
3. brush_panel.rs：`terrain_name_at(loc, brush.kind as usize)` → 按 `display_kind(格)` 查名（读数语义不变）。
4. 两个 dialog 的断言改 surface。

- [ ] **Step 4: 全工作区门禁 + 烟测**

Run:
```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
cargo test -p wv-app --features app && cargo clippy -p wv-app --all-targets --features app -- -D warnings
bash scripts/smoke-gui.sh
```
Expected: 全绿；smoke 日志无 panic / B000x；打开旧工程（v1 terrain.bin）加载不报错（走迁移分支）。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-app
git commit -m "wv-app: 笔刷目标表 + 断言/读数三通道收口（底座迁移完成）"
```

---

## 收尾检查

- [ ] `cargo tree -p wv-core | grep -i bevy` 与 `cargo tree --workspace --duplicates` 为空（分层红线）
- [ ] `docs/BINARY_FORMAT_SPEC.md` v2 节为**追加**，v1 记载未被修改
- [ ] 用任一现有工程（v1 terrain.bin）冷启动：加载成功、渲染与迁移前肉眼一致、再保存后 `index.yaml` 中 `format_version: 2`
- [ ] GLOSSARY 若执行中出现新术语（如「湿判定」）即时登记（AGENTS §6.1）

## Execution Handoff

本计划 6 个任务、接口链单一（cell 字段 → 常量/投影 → 二进制 → 命令 → 渲染 → UI），顺序依赖强、中间态不可合入——适合**单执行者顺序执行**（Native），或每个任务配独立 reviewer 的 Subagent-driven。后续计划（势力交互、生物群系标记）依赖本底座 + ADR-0027 计划 Task 6–12，另立文档。
