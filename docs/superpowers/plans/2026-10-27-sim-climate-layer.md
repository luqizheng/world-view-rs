# sim 引擎接入 · 气候地理层 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 shadow-empire-sim 以 submodule 挂入 wv，实现「季节 / 天气 / 旱涝灾害」纯函数气候层，在播放时以覆层 + 涌现标记呈现（零存储、作者可开关）。

**Architecture:** 气候 = `(climate_seed, day, 地理)` 的纯函数，求值与规则表收在 `wv-app/src/climate.rs`（唯一随机源 = sim crate 的 `SimRNG`，分域流）；结果经 wv-render 侧的 `ClimateOverlayState` Resource（wv-app 写、wv-render 读，`ViewState` 同款通道）交给渲染 pass。wv-core 只加 `climate_seed` 可选字段。渲染层与 sim crate 零接触（分层红线）。

**Tech Stack:** Rust 2021 / Bevy 0.19（渲染 pass）/ egui 0.36（工具条瓷砖）/ git submodule。

**Spec:** [docs/superpowers/specs/2026-10-27-sim-climate-layer-design.md](../specs/2026-10-27-sim-climate-layer-design.md)（含 D1–D7 用户拍板与 §7 裁量 5 条，本计划与之冲突时以 spec 为准）

## Global Constraints

- **分支纪律（AGENTS §13）**：Task 1 先建分支 `feat/sim-climate-layer` + worktree `.worktrees/feat/sim-climate-layer`（目录名=分支名），之后全部任务在 worktree 内进行；构建用全局共享 `CARGO_TARGET_DIR`（不覆盖）。
- **测试环境**：worktree 内跑测试**不用** `scripts/cargo-test.sh`；先 `export TEMP=D:\kudi-projects\worldview-rs\.target\testtmp; export TMP=$TEMP`（指主 checkout 的已信任目录）再直接 `cargo test`。
- **分层红线**：`shadow-empire-sim` 只允许出现在 `crates/wv-app/Cargo.toml`；wv-render / wv-core / wv-edit / wv-io / wv-ui 不得依赖它。wv-render 侧只收**纯数据**（wv-app 定义的结构映射过去）。
- **确定性**：随机只走 `shadow_empire_sim::SimRng`；天气按窗口取**独立派生流**（非顺序推进），保证任意 scrub 顺序结果一致；跨主体迭代显式稳定排序。
- **P7**：不新增任何「当前天气」存储字段；一切按天求值。
- **测试命名**：完整英文短句（`winter_precipitation_never_falls_as_rain`）；数值断言附 `got {}` 上下文；行程/公式类断言用手算期望值。
- **图标**：新图标取 Lucide，SVG 落 `crates/wv-ui/assets/ui/tool_icons/<lucide-name>.svg`，描边烘焙 `stroke="#ffffff"`。
- **i18n**：新增文案 zh/en 双语进 `I18nKey`。
- **成员 crate 禁止写死依赖版本**（根 `[workspace.dependencies]` 例外；path dependency 无版本号，不受此条约束）。
- **每任务收尾**：`cargo test -p <改动 crate>` 绿 + `cargo fmt --all -- --check` 后才 commit；提交信息 `<scope>: <动作>`。

## Review Focus

1. **旧工程 / 无模板（`meta.template == None`）**：气候必须照常工作（`climate_seed` 走常数基线派生），不能 panic——Task 2 的 `effective_climate_seed_without_template_falls_back_to_constant` 钉。
2. **非整除日历**（`days_per_year` 不被 4 整除，如 7）：季节划分不得丢天 / 越界——Task 3 的 `season_boundaries_with_indivisible_calendar_cover_every_day` 钉。
3. **时间轴倒放 / 跳天 scrub**：天气必须与求值顺序无关（同日重进结果不变）——Task 3 的 `weather_at_is_independent_of_evaluation_order` + Task 5 的 memo 正确性测试钉。
4. **无地形子图（城市图 / `terrain_grid_of` 返回 None）**：标记退化为空、天气照常显示——Task 5 的 `overlay_without_terrain_still_carries_weather` 钉。
5. **极大天数**（timeline 到 180001+）：窗口索引 / 取模运算不得溢出或回绕——Task 3 的 `weather_arithmetic_survives_large_day_counters` 钉。

---

### Task 1: 分支 + submodule + wv-app 依赖 + ADR + AGENTS 表增列

**Files:**
- Create: `third_party/shadow-empire-sim`（submodule gitlink，pin `ec0afe6`）
- Modify: `crates/wv-app/Cargo.toml`（`[dependencies]` 增一行）
- Modify: `AGENTS.md` §2.1 `wv-app` 行（允许依赖增列）与 §2.2 增一条 `cargo tree` 断言
- Create: `docs/adr/ADR-NNNN-sim-engine-submodule.md`（编号现场查 `ls docs/adr/` 取下一个，写处境/决策/理由/代价+被否决的 B 方案 wv 自研）

**Interfaces:**
- Produces: `wv-app` 可 `use shadow_empire_sim::SimRng;`（后续任务依赖）

- [ ] **Step 1: 建分支与 worktree**

```bash
git branch feat/sim-climate-layer
git worktree add .worktrees/feat/sim-climate-layer feat/sim-climate-layer
cd .worktrees/feat/sim-climate-layer
```

- [ ] **Step 2: 挂 submodule 并 pin**

```bash
git submodule add https://github.com/luqizheng/shadow-empire-sim.git third_party/shadow-empire-sim
git -C third_party/shadow-empire-sim checkout ec0afe6
```

预期：`third_party/shadow-empire-sim/rust/sim/Cargo.toml` 存在，包名 `shadow-empire-sim`。

- [ ] **Step 3: wv-app 增 path 依赖**

`crates/wv-app/Cargo.toml` `[dependencies]` 增：

```toml
shadow-empire-sim = { path = "../../third_party/shadow-empire-sim/rust/sim" }
```

- [ ] **Step 4: AGENTS.md §2.1 增列 + §2.2 断言**

§2.1 `wv-app` 行的允许依赖清单追加 `shadow-empire-sim`；§2.2 代码块追加：

```bash
# 确认 sim 只被 wv-app 依赖（应输出空；ADR-NNNN）
cargo tree --workspace --duplicates | grep -A2 'shadow-empire-sim' | grep -v 'wv-app' || true
grep -rln 'shadow_empire_sim' crates/ | grep -v 'crates.wv-app' （应输出空）
```

- [ ] **Step 5: 写 ADR**（处境=收窄后的气候层仍需 sim 的确定性配方与共享基建；决策=submodule+仅 wv-app 依赖；代价=拖着暂不消费的事件机器 + submodule 升级流程；被否决=B wv 自研小气候模块，代价是将来迁移/退役）

- [ ] **Step 6: 验证**

```bash
cargo tree -p wv-app | grep shadow-empire-sim   # 非空
cargo check -p wv-app                            # 过
```

- [ ] **Step 7: Commit**

```bash
git add .gitmodules third_party/shadow-empire-sim crates/wv-app/Cargo.toml AGENTS.md docs/adr/ Cargo.lock
git commit -m "wv-app: submodule 挂载 shadow-empire-sim（pin ec0afe6）+ 分层表增列 + ADR-NNNN"
```

### Task 2: wv-core `WorldMeta.climate_seed` 字段

**Files:**
- Modify: `crates/wv-core/src/lib.rs:150`（`WorldMeta` 结构体，`template` 字段之后）
- Test: `crates/wv-core/src/lib.rs` 尾部既有 `#[cfg(test)] mod tests`（无则新建）

**Interfaces:**
- Produces: `WorldMeta.climate_seed: Option<u64>`（`#[serde(default, skip_serializing_if = "Option::is_none")]`）
- Produces: `pub fn effective_climate_seed(meta: Option<&WorldMeta>) -> u64` —— `Some(m)` 且 `m.climate_seed=Some(x)` 直取；`Some(m)` 且 None、有 template → `fnv1a64("world.climate", t.seed)`；`Some(m)` 无 template 或 `meta=None` → `fnv1a64("world.climate", 0)`（吃 Option 是因为 `World.meta` 本身是 `Option<WorldMeta>`，`World::default()` 的世界也要能求值）
- Produces: `pub fn fnv1a64(tag: &str, seed: u64) -> u64`（FNV-1a 64 位，`offset=0xcbf29ce484222325`，`prime=0x100000001b3`；先逐字节吃 `tag`，再 8 字节小端吃 `seed`。**不得**用 `std` 的 `DefaultHasher`——跨版本不稳定）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn world_meta_round_trips_without_climate_seed() {
    // 不含 climate_seed 的 YAML 反序列化成功，且序列化后不出现该键（skip）。
}
#[test]
fn world_meta_round_trips_with_explicit_climate_seed() {
    // load(save(meta with climate_seed=Some(7))) == meta。
}
#[test]
fn effective_climate_seed_prefers_explicit_field() {
    // Some(7) 时与 template.seed 无关：effective == 7。
}
#[test]
fn effective_climate_seed_derives_from_template_seed() {
    // None + template.seed=42 → fnv1a64("world.climate", 42)；断言具体常量值
    // （实现时打印一次并钉住，注释标注「派生锚点」）。
}
#[test]
fn effective_climate_seed_without_template_falls_back_to_constant() {
    // meta=None 整体 → fnv1a64("world.climate", 0)；钉同一锚点风格断言。
}
```

- [ ] **Step 2: 跑测试确认失败**（`cargo test -p wv-core effective_climate_seed`，预期编译错误：字段不存在）

- [ ] **Step 3: 实现**字段与两个函数（`WorldMeta` 增字段 + `impl WorldMeta` 或自由函数，文档注释写明「为什么 Option：旧工程零迁移；为什么派生：重生成世界换 seed 天气随滚」）

- [ ] **Step 4: 跑测试确认通过**（`cargo test -p wv-core`全绿）

- [ ] **Step 5: Commit** —— `git commit -m "wv-core: WorldMeta.climate_seed 可选字段 + effective_climate_seed 派生"`

### Task 3: wv-app 气候核心——季节 / 天气纯函数 + 规则表 + GLOSSARY

**Files:**
- Create: `crates/wv-app/src/climate.rs`（`mod climate;` 挂进 `crates/wv-app/src/lib.rs`，`#![cfg(feature = "app")]` 口径随模块内标注，与 `ui_time.rs` 同款）
- Modify: `GLOSSARY.md`（登记：气候层 Climate Layer / 天气窗口 Weather Window / 涌现标记 Emergent Marker / `climate_seed`）
- Test: `crates/wv-app/src/climate.rs` 内 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `shadow_empire_sim::SimRng`（`new(world_seed: i32, stream_name: Option<&str>)`、`next_float(min: f32, max: f32) -> f32`）
- Consumes: `wv_core::Calendar { days_per_year: i64, .. }`、Task 2 的 `effective_climate_seed`
- Produces:
  - `pub enum Season { Spring, Summer, Autumn, Winter }`
  - `pub enum WeatherKind { Clear, Rain, Snow }`
  - `pub const DAYS_PER_WINDOW: i64 = 8;`
  - `pub fn season_of(calendar: &Calendar, day: i64) -> Season` —— `doy = (day - 1).rem_euclid(days_per_year); idx = (doy * 4 / days_per_year) as usize`（**先乘后除**防截断偏斜，idx ∈ 0..4）
  - `pub fn weather_at(climate_seed: u64, calendar: &Calendar, day: i64) -> WeatherKind` —— 窗口 `w = (day - 1) / DAYS_PER_WINDOW`；`draw(w) = SimRng::new(domain(seed), Some(&format!("world.climate.{w}"))).next_float(0.0, 1.0)`；`domain(seed) = (seed ^ (seed >> 32)) as i32`；降水 = `draw(w) < precip_prob(season(w), prev_rained)`，`prev_rained = w > 0 && draw(w-1) < precip_prob(season(w-1), false)`；雪/雨由 `temp_of(season) <= 0` 定
  - `pub fn precip_prob(season: Season, prev_rained: bool) -> f32` —— 表：春 0.30 / 夏 0.65 / 秋 0.35 / 冬 0.12；`prev_rained` 加 0.15
  - `pub fn temp_of(season: Season) -> f32` —— 春 12 / 夏 26 / 秋 12 / 冬 -2（°C）
  - 规则常量全部收在 `mod rules`（`pub const`），禁止散落字面量（spec P3）

- [ ] **Step 1: 写失败测试**（全部手算期望值）

```rust
#[test] fn season_boundaries_follow_day_of_year() { /* dpy=360：day 1/90→Spring，91/180→Summer，181/270→Autumn，271/360/361→Winter(271..360)、Spring(361) */ }
#[test] fn season_boundaries_with_indivisible_calendar_cover_every_day() { /* dpy=7：day 1,2→Spring；3→Summer；5,6→Autumn；7→Winter；再对 day 1..=35 全量断言 idx 无 panic 且 4 季都出现 */ }
#[test] fn precip_prob_tables_match_hand_values() { /* Summer+prev==0.80（1e-6）；Winter+无prev==0.12；Spring==0.30 */ }
#[test] fn precipitation_at_freezing_falls_as_snow() { /* kind_from_temp 路径：Winter 有降水→Snow，Summer 有降水→Rain（经内部纯函数断言） */ }
#[test] fn weather_at_is_deterministic_for_same_seed_and_day() { /* seed=42 两次调用相等 */ }
#[test] fn weather_at_is_independent_of_evaluation_order() { /* 先算 day 200 再算 day 100 == 直接算 day 100（独立分域流的本质属性） */ }
#[test] fn weather_arithmetic_survives_large_day_counters() { /* day = 180_001 与 day = i64::MAX/4 不 panic 且二者结果互不影响 */ }
#[test] fn weather_golden_anchors_pin_stream_values() { /* seed=42, day ∈ {1, 9, 100, 3600} 的 WeatherKind 逐个钉死（实现时打印一次，注释标「黄金锚点，重钉需说明」） */ }
```

- [ ] **Step 2: 跑测试确认失败**（模块尚不存在，编译错误）

- [ ] **Step 3: 实现** `climate.rs`（上文 Produces 签名 + `mod rules` 常量表；Golden 锚点用 `println!` 跑一次取得后钉死）

- [ ] **Step 4: 跑测试确认通过**（`cargo test -p wv-app --features app climate`；注意 wv-app 无 feature 时本模块不编译——本模块标 `#![cfg(feature = "app")]`，测试命令必须带 feature）

- [ ] **Step 5: GLOSSARY 登记** 四词条（术语/英文/定义/首现位置=本计划/所属维度=时间·气候）

- [ ] **Step 6: Commit** —— `git commit -m "wv-app: climate 模块——季节/天气纯函数 + 规则表 + 术语登记"`

### Task 4: 湿润度场与灾害提议

**Files:**
- Modify: `crates/wv-app/src/climate.rs`（追加地理段）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: Task 3 全部；`wv_terrain::{TerrainGrid, TerrainCell, SURFACE_WATER}`（`TerrainCell { height: Height, surface: Surface, biome: Biome }`；水面 = `surface == SURFACE_WATER`）；`wv_core::WorldTemplateKind`（`Ocean/ColdNorth/AridBelt`）
- Produces:
  - `pub fn distance_to_water(grid: &TerrainGrid) -> Vec<u32>` —— 两遍 chamfer（前向：`min(d[x-1,y], d[x,y-1], d[x-1,y-1], d[x+1,y-1]) + 1`；后向对称；水面格 0，不可达 `u32::MAX`）
  - `pub fn moisture_at(grid, dist: &[u32], template: Option<WorldTemplateKind>, season: Season, cell: (u32, u32)) -> f32` —— `clamp(base + season_bias + water_bonus - elev_penalty, 0.0, 1.0)`；`base`：Ocean 0.70 / ColdNorth 0.55 / AridBelt 0.30 / None 0.55；`season_bias`：春 +0.05 / 夏 +0.10 / 秋 0.00 / 冬 -0.15；`water_bonus = 0.25 * max(0.0, 1.0 - dist as f32 / 5.0)`；`elev_penalty = 0.30 * ((height.clamp(0, 3000)) as f32 / 3000.0)`；水面格恒 0.0
  - `pub struct DisasterProposal { pub place: Id, pub pos_wu: (f32, f32), pub kind: DisasterKind, pub severity: f32 }`、`pub enum DisasterKind { Drought, Flood }`
  - `pub fn disaster_thresholds() -> (f32, f32)` —— 干旱 0.35 / 洪涝 0.75（读侧唯一入口）
  - `pub fn disasters_for_day(world: &World, grid: &TerrainGrid, calendar: &Calendar, climate_seed: u64, day: i64) -> Vec<DisasterProposal>` —— 对每个**陆地格**判旱（moisture < 0.35 **且 `temp_of(season) > 0`**——冷干是冬季常态不是灾害，RimWorld 生长季/Azgaar 冰川判定的同一温度轴纪律；severity = (0.35-m)/0.35）或涝（`1 <= dist <= 2` 且 season==Summer 且 moisture > 0.75，severity=(m-0.75)/0.25）；灾害格归给**半径 30 格内最近 Place**（`world.maps` 根图 places 的格坐标换算，`unit_per_km` 由地图 index 提供）；无 Place 覆盖的格**跳过**（v1 只在地点冒标记，spec §7 简化）；按 `(place.id asc, |severity| 同点取 max)` 聚合，输出按 severity 降序、截断 32 条（密度预算纪律）

- [ ] **Step 1: 写失败测试**（fixture 6×5：第 0 行全水面 `surface=SURFACE_WATER, height=-100`，其余 5 行陆地 `height=100`）

```rust
#[test] fn chamfer_distance_measures_rows_from_water_row() { /* dist[1][*]==1, [2]==2, [3]==3, [4]==4, [5]==5（手算） */ }
#[test] fn moisture_in_humid_template_with_spring_rain_matches_hand_value() { /* ColdNorth+Spring，d=1: 0.55+0.05+0.20-0.01==0.79（1e-3，附 got） */ }
#[test] fn drought_needs_above_freezing_season() { /* AridBelt+Winter，d=5: m=0.19<0.35 但 temp(-2)≤0 → 无灾害（冷干不是灾害） */ }
#[test] fn warm_arid_inland_cells_drought() { /* AridBelt+Spring，d=5: m=0.30+0.05+0.00-0.01==0.34<0.35 且 temp(12)>0 → 旱灾 severity=(0.35-0.34)/0.35≈0.029 */ }
#[test] fn summer_flood_hits_shoreline_band_only() { /* ColdNorth+Summer：d=1 → m=0.84 涝 sev=0.36；d=2 → 0.79 涝 sev=0.16；d=3 → 0.74 不涝 */ }
#[test] fn disasters_without_nearby_places_are_dropped() { /* 世界无任何 Place → 空输出（v1 口径） */ }
#[test] fn disaster_markers_aggregate_by_place_and_cap_at_32() { /* 两格同属一 Place → 1 条取 max severity；33 格 33 Place → 32 条且按 severity 降序 */ }
```

- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**（chamfer 与公式按 Interfaces 逐行落；`disasters_for_day` 的 Place 遍历**显式按 `place.id` 排序**后再处理，守确定性纪律）
- [ ] **Step 4: 跑测试确认通过**
- [ ] **Step 5: Commit** —— `git commit -m "wv-app: 气候地理——chamfer 距水场 + 湿润度 + 灾害提议"`

### Task 5: 渲染侧纯数据资源 + ViewState 开关 + app 同步系统

**Files:**
- Modify: `crates/wv-render/src/state.rs`（`ViewState` 增 `climate_visible`；新增 `ClimateOverlayState` 等类型 + `impl Default`；Resource 注册处随渲染 Plugin）
- Create: `crates/wv-app/src/climate_sync.rs`（挂 `mod climate_sync;`）
- Test: 两文件各自 `mod tests`

**Interfaces:**
- Consumes: Task 3/4 全部、`PlaybackState::current_day`（`crates/wv-app/src/state.rs`）、`WorldProjection.world`、`crate::edit::line_poi_tool::terrain_grid_of(world: &World, map: &Id) -> Option<TerrainGrid>`
- Produces（wv-render 侧，**纯数据、零 sim 依赖**）:
  - `ViewState.climate_visible: bool`（默认 `false`；会话态不落盘，先例 `isolines: state.rs:272`）
  - `pub enum ClimateSeason { Spring, Summer, Autumn, Winter }`、`pub enum ClimateWeather { Clear, Rain, Snow }`、`pub enum ClimateMarkerKind { Drought, Flood }`
  - `pub struct ClimateMarker { pub pos: Vec2, pub kind: ClimateMarkerKind, pub severity: f32 }`（`pos` = **map-local 世界单位**，与 Place POI 同一坐标系——两套原点陷阱：不要用栅格原点）
  - `pub struct ClimateOverlayState { pub season: ClimateSeason, pub weather: ClimateWeather, pub markers: Vec<ClimateMarker>, pub source_day: i64 }` + `Default`（Clear / 空 markers），注册为 Bevy Resource
- Produces（wv-app 侧）:
  - `pub fn compute_overlay(world: &World, root_map_id: &Id, day: i64) -> ClimateOverlayState`（内部：`effective_climate_seed` → `season_of`/`weather_at` → `terrain_grid_of(world, root_map_id)` 拿 Some 才算 markers，None 则 markers 空——Review Focus #4；类型映射 wv-app → wv-render 纯手写 `match`）
  - `pub fn sync_climate_overlay(/* Bevy 参数：Res<WorldProjection>, Res<PlaybackState>, ResMut<ClimateOverlayState> 等 */)` —— `Local<Option<(i64, bool)>>` 记 `(day, climate_visible)` 缓存键，仅变化时重算（scrub 正确性靠纯函数，性能靠 memo）；在 wv-app 既有 view 同步系统注册处挂 Update（检索现有写 `ViewState` 字段的 system 注册点，同 Plugin 同 SystemSet）

- [ ] **Step 1: 写失败测试**（wv-app 侧）

```rust
#[test] fn overlay_without_terrain_still_carries_weather() { /* 空 World fixture（无 terrain.bin）：markers 空、weather 非 Clear 的锚点日仍在 */ }
#[test] fn overlay_reflects_day_change_and_ignores_same_day() { /* compute_overlay 纯函数：day 100→200 字段变；同 day 两次调用全等（PartialEq derive） */ }
#[test] fn marker_positions_are_in_map_local_wu() { /* fixture place 已知 wu 坐标 → marker.pos 一致（1e-3） */ }
```

- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**（wv-render 类型 + Resource 注册；wv-app `compute_overlay` + 薄 system；映射代码是 `match` 手写，不许 serde 跨 crate 复用）
- [ ] **Step 4: 跑测试确认通过**（`cargo test -p wv-app --features app` 与 `cargo test -p wv-render --features app`... 渲染层测试命令用 `cargo app-test`）
- [ ] **Step 5: Commit** —— `git commit -m "wv-app/wv-render: ClimateOverlayState 通道 + 气候会话开关 + memo 同步系统"`

### Task 6: 渲染 pass——天气色调 + 灾害标记

**Files:**
- Create: `crates/wv-render/src/climate_pass.rs`（挂 `mod climate_pass;` 进 wv-render lib）
- Test: 文件内 `mod tests`（可测的纯部分：颜色映射、可见性 gate 逻辑）

**Interfaces:**
- Consumes: Task 5 的 `ClimateOverlayState` / `ViewState.climate_visible`；既有 fullscreen 覆层 pass 先例（wv-render 内 veil/atmosphere pass，检索 `veil` 定位模仿其 mesh/material 与 Z 槽位）；既有 POI/事件点的 map-local→屏幕投影路径（检索 `events_visible` 在 `layer_frame.rs` 的消费点，**同一投影函数**）
- Produces:
  - 天气 tint：`ViewState.climate_visible == true` 时全屏半透明四边形，色值钉死——Rain `rgba(0.42, 0.49, 0.62, 0.10)` / Snow `rgba(0.87, 0.91, 0.95, 0.12)` / Clear 不画（美术待裁③的 v1 缺省，改色只动 `mod colors` 常量）
  - 灾害标记：菱形 + severity 环（环半径 = 6px + 10px×severity，屏幕恒定像素），Drought `#C2884E` / Flood `#4E7FC2`，画在事件高亮 pass 之后的 Z 槽位；gate 同 tint
  - 空 mesh 不建实体（Bevy slab 空几何报错前科）

- [ ] **Step 1: 写失败测试**（纯逻辑：`fn weather_tint(ClimateWeather) -> Option<Color>` 三分支 + gate 布尔组合真值表）

- [ ] **Step 2: 跑测试确认失败**
- [ ] **Step 3: 实现**（模仿 veil pass 与 POI 投影路径；`Cargo.toml` 无需新依赖）
- [ ] **Step 4: 验证**

```bash
cargo app-test && cargo app-clippy
```

- [ ] **Step 5: Commit** —— `git commit -m "wv-render: 气候覆层 pass——天气 tint + 灾害菱形标记"`

### Task 7: 工具条开关瓷砖 + 图标 + i18n

**Files:**
- Create: `crates/wv-ui/assets/ui/tool_icons/cloud-rain.svg`（Lucide `cloud-rain`，描边烘焙 `#ffffff`）
- Modify: `crates/wv-app/src/ui/map_controls/toolbar.rs:317` 附近（isolines 瓷砖之后加「气候」瓷砖，同款 toggle 模式翻转 `view.climate_visible`）
- Modify: 时间轴面板（检索 `crates/wv-app/src/ui/` 下 timeline 面板文件）：当前日行追加「季节·天气 + 灾害 chips」，数据源 = `ClimateOverlayState`（读侧只读，不重算）；chip 样式与正式事件区分（描边小徽标，非事件高亮色）
- Modify: `crates/wv-app/src/i18n*`（`I18nKey` 增四键 zh/en：气候瓷砖 tooltip「气候覆层（季节·天气·灾害）"/"Climate overlay (season · weather · disasters)"；`DroughtLabel`「旱灾"/"Drought"；`FloodLabel`「洪涝"/"Flood"；`SeasonLabel`「季节"/"Season"）

- [ ] **Step 1: 下载 Lucide `cloud-rain` SVG → 烘焙描边 → 落盘 wv-ui assets（kebab-case 同名）**
- [ ] **Step 2: i18n 四键**（照既有 key 增列流程；zh/en 都加）
- [ ] **Step 3: 工具条瓷砖**（toolbar.rs，与 isolines 同款 `add_toggle_tile` 式调用；挂既有图标加载路径）
- [ ] **Step 4: 时间轴当前日气候行**（季节·天气文本 + 灾害 chips，均从 `ClimateOverlayState` 派生；无工程/默认态不渲染该行）
- [ ] **Step 5: 验证** `cargo app-test && cargo app-clippy`（全绿）
- [ ] **Step 6: Commit** —— `git commit -m "wv-app/wv-ui: 气候覆层工具条瓷砖 + 时间轴气候行 + cloud-rain 图标 + i18n"`

### Task 8: 实机验收 + 全门禁收尾

- [ ] **Step 1: 全门禁**

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check
cargo test -p wv-app --features app && cargo clippy -p wv-app --all-targets --features app -- -D warnings
bash scripts/smoke-gui.sh
```

（worktree 内测试先 `export TEMP` 指主 checkout `.target/testtmp`，见 Global Constraints）

- [ ] **Step 2: 实机走查**（WV_SHOT 或用户手跑）：打开 `examples/jrpg-world` → 播放模式 → 开气候瓷砖 → 核对：季节随 day 变化、冬天雪/夏雨的 tint、旱涝菱形出现在合理地点、倒放 scrub 天气稳定、关瓷砖画面回到原样、无 panic（日志干净）
- [ ] **Step 3: 遗漏修补后逐项 commit**；分支收尾走 finishing-a-development-branch（合并等用户指令）
