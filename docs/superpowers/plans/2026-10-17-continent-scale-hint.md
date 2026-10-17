# 世界图尺度只读提示（校验面板）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 世界图声明 `unit_per_km` 后，校验面板显示只读换算行：图幅 X×Y km、陆地近似尺度、步行/骑马/帆船横穿天数——「叙事换算自动化」，V1 只提示不阻止。

**Architecture:** 两小层——`wv-core` 加一个纯函数（陆地 bbox → km，参数化 cell_size）；`wv-app` 校验面板加只读行，横穿天数经 `Vehicle::segment_days` 唯一换算入口（`wv-core/src/map/distance.rs:53`），参考载具为三个内置常量。**不新增任何速度公式**（AGENTS §4.2）。

**Tech Stack:** Rust 2021；wv-core 纯逻辑 + wv-app egui。

**Spec:** README §12-39（关闭条件含手算期望值测试锚定）+ ADR-0022 D4（`unit_per_km` 语义）。与计划 1/2/3 无依赖，可并行。

## Global Constraints

- **唯一换算入口**：天数只准调 `Vehicle::segment_days(distance_km, terrain)`；出现第二处 `km / speed` 即违规。
- `unit_per_km` 语义执行时核实（ADR-0022 D4）：字段名按「units per km」读，km = 世界单位 ÷ unit_per_km；若实测相反（km per unit），测试期望值随之取倒数并在 PR 说明注明。
- wv-core 零 Bevy / 零 fs；纯函数可独立测试。
- 无 `unit_per_km` 或无陆地 → 面板不显示该行（不告警——V1 只提示，缺数据是正常态）。
- i18n：面板行文案 zh/en 双语。
- 提交信息 `<scope>: <动作>`。

## Review Focus

1. **全海洋图**（无 height>0 格）：返回 None，面板静默——Task 1 测试。
2. **unit_per_km 缺失**：不显示、不参与计算、不 panic——Task 3 行为。
3. **手算期望值**：20×10 格陆地、cell 8 wu、upk 100 → 宽 1.6 km；步行日速 40 km/日（5 km/h×8 h）→ 对角线天数断言——Task 1/2 测试（AGENTS §7.2 硬要求）。
4. **负高度岛屿忽略**：水下格不计入陆地 bbox——Task 1 测试。
5. **`instant` 载具防御**：参考载具不设 instant；若作者后续接入自定义载具列表，`None` 天数显示「—」不 panic——Task 3。

---

### Task 1: `wv-core` 陆地尺度纯函数

**Files:**
- Create: `crates/wv-core/src/map/scale_hint.rs`
- Modify: `crates/wv-core/src/map/mod.rs`（`pub mod scale_hint;`）

**Interfaces:**
- Produces:
  - `pub struct LandExtentKm { pub width_km: f32, pub height_km: f32, pub diagonal_km: f32 }`
  - `pub fn land_extent_km(world: &World, map: &Id, cell_size_wu: f32) -> Option<LandExtentKm>`——对 `world.terrain_of(map)` 的 height>0 格取 bbox（格数），× cell_size_wu → 世界单位，÷ `map.unit_per_km?` → km；无陆地 / 无地形 / 无 unit_per_km → `None`。

- [ ] **Step 1: 写失败测试**（手算期望值，AGENTS §7.2/§7.4）
  - `land_extent_converts_cells_to_km_with_hand_computed_values`：20×10 陆格、cell 8、upk 100 → width 1.6 km / height 0.8 km / diagonal = √(1.6²+0.8²)（断言带 got）。
  - `all_ocean_map_returns_none`（含负高度岛格仍 None）。
  - `missing_unit_per_km_returns_none`。
- [ ] **Step 2: 跑确认失败** Run: `cargo test -p wv-core scale_hint`
- [ ] **Step 3: 实现**（bbox = 格坐标 min/max；对角线 = 勾股；f32 即可）。
- [ ] **Step 4: 验证** Run: `cargo test -p wv-core && cargo clippy -p wv-core --all-targets -- -D warnings`
- [ ] **Step 5: Commit** `wv-core: 陆地尺度换算 land_extent_km`

### Task 2: 参考载具常量（wv-app）

**Files:**
- Modify: `crates/wv-app/src/ui/validation_panel.rs`（或其 mod，执行时看面板文件组织）

**Interfaces:**
- Produces: `const REF_WALK: Vehicle`（speed_kmh 5.0 / daily_hours 8.0）、`REF_HORSE: Vehicle`（8.0 / 8.0）、`REF_SAIL: Vehicle`（10.0 / 12.0）；terrain 参数分别传 `"plain"` / `"plain"` / `"sea"`（无 modifiers → ×1.0）。
- 天数 = `REF_*.segment_days(diagonal_km, terrain)`（None → 显示「—」）。

- [ ] **Step 1: 写失败测试** `reference_vehicles_daily_speeds_match_hand_computed_values`——40.0 / 64.0 / 120.0 km/日（经 `effective_daily_speed`，断言带 got）。
- [ ] **Step 2: 跑确认失败** Run: `cargo test -p wv-app --features app validation`
- [ ] **Step 3: 实现**（`Vehicle` 构造看 `wv-core/src/map/distance.rs` 结构体字段；只填所需字段）。
- [ ] **Step 4: Commit** `wv-app: 校验面板参考载具常量`

### Task 3: 面板只读行 + i18n

**Files:**
- Modify: `crates/wv-app/src/ui/validation_panel.rs`（`panel_validation` 内加段）
- Modify: `crates/wv-app/src/i18n/keys.rs`、`table.rs`

**Interfaces:**
- 行文案 Key：`scale_hint_extent`（"图幅 {w}×{h} km"）、`scale_hint_land`（"陆地约 {w}×{h} km"）、`scale_hint_crossing`（"横穿约 步行{a}天 · 骑马{b}天 · 帆船{c}天"）；zh/en 双语。
- cell_size 来源：`GridConfig::cell_size`（wv-render::state，与 `GRID_CELL_SIZE` 同口径）。
- 显示条件：`land_extent_km(...)` 返回 Some 才渲染整段。

- [ ] **Step 1: 写失败测试** `scale_hint_rows_hidden_without_unit_per_km_or_land`（构造两种缺失世界断言不渲染；有数据断言文案含手算天数 "0.045"→显示侧断言用计算值）。
- [ ] **Step 2: 跑确认失败**
- [ ] **Step 3: 实现**
- [ ] **Step 4: 全量验证**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p wv-app --features app
cargo clippy -p wv-app --all-targets --features app -- -D warnings
bash scripts/smoke-gui.sh
```

- [ ] **Step 5: Commit** `wv-app: 校验面板世界图尺度只读提示（§12-39）`
