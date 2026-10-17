# 大陆配方内核（wv-terrain）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `wv-terrain`（纯数据层，零第三方依赖）实现「大陆配方」确定性内核：意图多边形 + 八参数 + 种子 → 双通道格子写入。

**Architecture:** 新模块 `continent.rs`，与既有 `intent.rs`（意图笔刷）、`generate.rs`（Perlin/fBm）并列。纯函数、无 Bevy、无 IO；海岸 = 有符号距离场 + fBm 扰动，高度 = 径向剖面 + 噪声调制，类型 = Q5 优先序高度分带，接缝 = `edit::falloff_weight` 高斯过渡。调用方是 wv-edit 的 `GenerateContinentCommand`（见计划 2）。

**Tech Stack:** Rust 2021, 无新增依赖（`wv-terrain` 保持零第三方依赖红线——坐标一律 `(f32, f32)`，**禁用 glam/Vec2**）。

**Spec:** `docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §8「2026-10-17 · 大陆生成工作流」（裁决 + 八参数表）+ GLOSSARY「大陆配方」词条 + README §12-40。

## Global Constraints

- `wv-terrain` 零第三方依赖（AGENTS §2.1）；新增依赖 = 直接拒收。
- 高度契约（Q1，已冻结）：`i16`，1 单位 = 1 米，海平面 = 0，负值水下。
- 类型 = `u8` 开放集，常量与 `intent.rs` 的 `KIND_*` 族同源（执行时读全 `intent.rs:40-72`，**不得自造新编号**）。
- 确定性（D6）：同 `(envelopes, params, seed)` 必产出逐位一致的网格。
- 越界/退化输入（空折线、<3 点多边形、空 envelopes）安全返回 0 改动，不 panic（V1 精神）。
- 测试名用完整英文短句；数值断言附实际值上下文（AGENTS §7.3/§7.4）。
- 提交信息 `<scope>: <动作>`，如 `wv-terrain: 大陆配方距离场原语`。

## Review Focus

1. **自交叉多边形**（作者随手圈出蝴蝶结形）：距离场/内点判定不 panic，产出仍确定——Task 1 测试。
2. **wiggle=0 严格贴边**：所有写入格不得越出包络减去大陆架的收缩区——Task 3 测试。
3. **同 seed 重跑逐位一致**（含跨 `&mut` 同一网格二次生成的叠加路径）——Task 5 测试。
4. **既有陆地不得被压低**（海洋世界打底，但图上可能有作者先画的岛）——Task 4 测试。
5. **空/退化输入零改动**：空 envelopes、两点「多边形」、非正 cell_size——Task 5 测试。

---

### Task 1: 多边形几何原语（内点判定 + 有符号距离）

**Files:**
- Create: `crates/wv-terrain/src/continent.rs`
- Modify: `crates/wv-terrain/src/lib.rs`（加 `pub mod continent;`，位置随 `intent` 之后）

**Interfaces:**
- Produces:
  - `pub(crate) fn point_in_polygon(p: (f32, f32), poly: &[(f32, f32)]) -> bool`（射线法；边界算在内）
  - `pub(crate) fn signed_distance(p: (f32, f32), poly: &[(f32, f32)]) -> f32`（内正外负；= 到最近边的距离 × 符号）
  - `pub(crate) fn polygon_bbox(polys: &[Vec<(f32, f32)>]) -> Option<((f32,f32),(f32,f32))>`（全部包络联合 bbox，min/max 角点）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn square_polygon_point_in_polygon_and_signed_distance() {
    let sq = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    assert!(point_in_polygon((5.0, 5.0), &sq), "中心在内");
    assert!(!point_in_polygon((15.0, 5.0), &sq), "外部");
    let d = signed_distance((5.0, 5.0), &sq);
    assert!((d - 5.0).abs() < 1e-3, "中心到边 5.0，got {d}");
    let d_out = signed_distance((13.0, 5.0), &sq);
    assert!((d_out + 3.0).abs() < 1e-3, "外 3 单位 = -3.0，got {d_out}");
}

#[test]
fn self_intersecting_bowtie_polygon_is_handled_without_panic() {
    let bowtie = vec![(0.0, 0.0), (10.0, 10.0), (10.0, 0.0), (0.0, 10.0)];
    let _ = signed_distance((5.0, 5.0), &bowtie); // 只要不 panic 即可
    let _ = point_in_polygon((2.0, 1.0), &bowtie);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-terrain continent`
Expected: 编译失败（函数未定义）。

- [ ] **Step 3: 实现三个原语**

射线法 + 逐边最近距离（点到线段投影，钳到端点）。模块头 `//!` 文档注释写明：大陆配方内核、ADR/裁决出处（plan v0.1 §8 2026-10-17）、坐标为图内世界单位。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-terrain continent`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/wv-terrain/src/continent.rs crates/wv-terrain/src/lib.rs
git commit -m "wv-terrain: 大陆配方内核——多边形距离场原语"
```

### Task 2: `ContinentParams` 参数结构与缺省

**Files:**
- Modify: `crates/wv-terrain/src/continent.rs`

**Interfaces:**
- Produces:
  - `pub enum ReliefLevel { Hills, Mountains, HighMountains }` + `pub fn meters(self) -> i16`（600 / 2000 / 4000）
  - `pub enum IslandDensity { Off, Low, Medium, High }` + `pub fn threshold(self) -> f64`（0.0 / 0.60 / 0.70 / 0.78）
  - `pub struct ContinentParams { pub coast_wiggle: f32, pub coast_octaves: u32, pub relief: ReliefLevel, pub forest_cover: f32, pub desert_cover: f32, pub shelf_cells: u32, pub islands: IslandDensity }`
  - `impl Default for ContinentParams`：`0.40, 4, ReliefLevel::Mountains, 0.30, 0.15, 8, IslandDensity::Off`（裁决缺省，plan v0.1 §8 参数表）

- [ ] **Step 1: 写失败测试** `default_params_match_ruling_values`——逐字段断言上述七个值（附 got 上下文）。
- [ ] **Step 2: 跑测试确认失败** Run: `cargo test -p wv-terrain continent` → 编译失败。
- [ ] **Step 3: 实现枚举与结构**，`///` 注释逐字段说明对应面板参数与范围（0–100% / 低中高 2-4-6 octaves 等）。
- [ ] **Step 4: 跑测试确认通过**
- [ ] **Step 5: Commit** `wv-terrain: 大陆配方八参数结构与裁决缺省`

### Task 3: 海岸距离场 + fBm 扰动

**Files:**
- Modify: `crates/wv-terrain/src/continent.rs`

**Interfaces:**
- Consumes: `crate::generate::Perlin::{new, fbm}`（`fbm(x: f64, y: f64, octaves: u32, lacunarity: f64, persistence: f64) -> f64`，输出约 -1..1）
- Produces: `pub(crate) fn coast_field(p: (f32, f32), polys: &[Vec<(f32, f32)>], params: &ContinentParams, noise: &Perlin, diag: f32) -> f32`——返回扰动后的有符号距离：`d' = signed_distance(对 polys 逐个取 max) + amp * fbm01`；`amp = params.coast_wiggle * 0.10 * diag`；fbm 采样坐标 = `p / (diag * 0.25)`，octaves = `params.coast_octaves`，lacunarity 2.0、persistence 0.5（Q15）。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn zero_wiggle_coast_field_equals_plain_signed_distance() {
    let sq = vec![vec![(0.0,0.0),(100.0,0.0),(100.0,100.0),(0.0,100.0)]];
    let p = ContinentParams::default();
    let n = Perlin::new(42);
    let f = coast_field((50.0, 50.0), &sq, &p, &n, 141.42);
    assert!((f - 50.0).abs() < 1e-3, "wiggle=0 应等于裸距离 50，got {f}");
}
```

（wiggle>0 的行为由 Task 5 的贴边性质测试间接锚定。）

- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**。多包络取各包络 signed_distance 的 max（就近海岸）；fbm01 = `fbm(...)*0.5+1.0` 钳 0..1。
- [ ] **Step 4: 跑测试确认通过**
- [ ] **Step 5: Commit** `wv-terrain: 大陆配方海岸距离场与 fBm 扰动`

### Task 4: 高度剖面 + Q5 分带 + 大陆架 + 森林/荒漠 + 融合

**Files:**
- Modify: `crates/wv-terrain/src/continent.rs`
- Consumes: `crate::intent` 的 `KIND_*` 常量族与分带阈值写法（执行时读 `intent.rs:40-80` 对齐，含雪线/丘陵/山地阈值）；`crate::edit::falloff_weight(distance, radius, softness)`

**Interfaces:**
- Produces:
  - `pub(crate) fn land_height(d: f32, inradius: f32, params: &ContinentParams, noise: &Perlin, p: (f32,f32)) -> i16`——`d`=coast_field 值（世界单位，内正）；`t = clamp(d / (0.6 * inradius), 0, 1)`，`base = relief_m × t`，内部 fBm 调制 ×(0.7 + 0.6·fbm02)；`inradius = 0.5 × 多边形面积开方`（由 bbox 传算）。
  - `pub(crate) fn band_kind(height_m: i16, d: f32, params: &ContinentParams, noise: &Perlin, p: (f32,f32)) -> TerrainKind`——按 Q5 优先序：`d ∈ [-shelf_wu, 0)` → 浅水（大陆架）；`height ≥ 雪线` → 雪原；山地/丘陵/森林(森林覆盖掩码)/沙漠(荒漠覆盖掩码)/平原按 `intent.rs` 既有分带阈值与 KIND 常量；深水 = intent.rs 的深水常量。森林/荒漠掩码：`fbm01(p/(diag*0.5)) < cover` 才落该类（diag 经 params 外部传入，签名可加 `diag: f32` 参数）。
  - `pub(crate) fn blended(existing: TerrainCell, new_height: i16, new_kind: TerrainKind, d: f32, cell_size: f32) -> TerrainCell`——`d ∈ [0, 6格×cell_size)` 海岸带内：new_height × `falloff_weight(d, 6格, 0.5)` 与 existing 高度线性过渡，kind 随高度走；内部（d ≥ 6 格）：高度取 `max(existing.height, new_height)`，kind 用 new（既有陆地不被压低）。

- [ ] **Step 1: 写失败测试**
  - `higher_cells_never_have_lower_priority_kind`——构造 0/300/800/2200/4500 m 五档高度，断言 kind 优先序单调不降（序 = intent.rs Q5 常量声明序）。
  - `blended_interior_never_lowers_existing_land`——existing height=800 陆地、new_height=50：结果 ≥ 800。
  - `shelf_band_is_shallow_water`——d = −2 格（shelf=8 格内）、wiggle=0：kind == 浅水常量。
- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**（阈值全部引用 intent.rs 既有常量；若 intent.rs 无雪线常量则用其分带写法同款数值并注明出处）。
- [ ] **Step 4: 跑测试确认通过**
- [ ] **Step 5: Commit** `wv-terrain: 大陆配方高度剖面/Q5 分带/大陆架/融合`

### Task 5: 组装入口 `generate_continents` + 确定性与安全

**Files:**
- Modify: `crates/wv-terrain/src/continent.rs`

**Interfaces:**
- Produces: `pub fn generate_continents(grid: &mut TerrainGrid, envelopes: &[Vec<(f32, f32)>], params: &ContinentParams, seed: u64, cell_size: f32) -> usize`
  - 就地写 grid，返回改动格数。遍历联合 bbox（外扩 `amp + shelf + 离岸带`）内每格：world 坐标 = `(gx+0.5)*cell_size, (gy+0.5)*cell_size`（格心，Q4 口径）；`d = coast_field(...)`；`d ≥ 0` → 陆地写入（高度剖面 + 分带 + 融合）；`d ∈ [-shelf, 0)` → 大陆架浅水；包络外但离岸掩码超阈值 → 小岛（半径 1-2 格的陆格，高度低丘陵档）。离岸小岛 = 独立低频 fbm（freq = diag×0.5）> `islands.threshold()`。
  - 退化输入（envelopes 空 / 任一多边形 < 3 点 / cell_size ≤ 0）→ 返回 0。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn same_seed_produces_bitwise_identical_grids() {
    let env = vec![vec![(64.0,64.0),(448.0,64.0),(448.0,240.0),(64.0,240.0)]];
    let mut a = TerrainGrid::new(64, 64);
    let mut b = TerrainGrid::new(64, 64);
    let n1 = generate_continents(&mut a, &env, &ContinentParams::default(), 7, 8.0);
    let n2 = generate_continents(&mut b, &env, &ContinentParams::default(), 7, 8.0);
    assert_eq!(a, b, "同 seed 应逐位一致");
    assert_eq!(n1, n2);
}

#[test]
fn zero_wiggle_generation_stays_inside_envelope_minus_shelf_growth() {
    // wiggle=0、islands=Off：所有 height>0 的格子，其裸 signed_distance > -shelf_wu
    // （海岸只允许向外长出大陆架，不允许凭空冒陆格）
}

#[test]
fn degenerate_inputs_change_nothing() {
    let mut g = TerrainGrid::new(8, 8);
    assert_eq!(generate_continents(&mut g, &[], &ContinentParams::default(), 1, 8.0), 0);
    let two_pts = vec![vec![(0.0,0.0),(10.0,10.0)]];
    assert_eq!(generate_continents(&mut g, &two_pts, &ContinentParams::default(), 1, 8.0), 0);
    assert_eq!(generate_continents(&mut g, &env_ok, &p, 1, 0.0), 0, "非正 cell_size");
}

#[test]
fn different_seed_different_layout_and_existing_land_survives() { /* 性质断言 */ }
```

- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**（组装前四个 task 的原语；深水基线与 `MapBase::Ocean` 的 −3500 m 口径一致：海洋格不被重写，仅陆架/岛写浅水与陆地）。
- [ ] **Step 4: 全量验证**

Run: `cargo test -p wv-terrain && cargo clippy -p wv-terrain --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 全绿。

- [ ] **Step 5: Commit** `wv-terrain: 大陆配方组装入口 generate_continents`
