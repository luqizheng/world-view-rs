# 默认海洋锚定 + 新建图 16:9 缺省 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把「默认海洋世界」用测试钉死（行为已存在，缺锚定），并把新建地图/新建工程的幅面缺省从正方形改为 16:9（用户可改）。

**Architecture:** 纯 wv-app UI 层改动。`MapBase::Ocean`（`wv-core/src/map/mod.rs:165`，−3500 m 深水）已存在且两个对话框已默认使用——Task 1 只补回归测试。16:9 = `NewMapDialog` 的 `extent: f32` 拆成宽高两个字段 + 幅面档位沿宽取值，`new_project_dialog` 首图常量同步。

**Tech Stack:** Rust 2021 / egui 0.36（`#[cfg(feature = "app")]`）。

**Spec:** plan v0.1 §8「2026-10-17」两节（默认海洋世界 / 16:9 缺省可改）+ README §12-40。与计划 1/2 无依赖，可并行。

## Global Constraints

- UI 模块在 `app` feature 下：验证必须含 `cargo test -p wv-app --features app` + `cargo clippy -p wv-app --all-targets --features app -- -D warnings`。
- i18n（ADR-0018）：新增「宽高比」相关 Key 走 zh/en 双语。
- 不动 `MapBase` 数值（−3500 m 口径 2026-10-16 已定，map/mod.rs:172-185）。
- 16:9 是**缺省不是约束**：对话框可选其他比例；作者改了依作者（V1）。
- 提交信息 `<scope>: <动作>`。

## Review Focus

1. **既有正方形工程零迁移**：打开旧工程不触任何重排——本计划只动「新建」路径默认值（无测试可写则烟测兜底）。
2. **16:9 换算整数格**：4096 宽 → 2304 高 = 288 格宽 × 162 格高（cell 8 wu），断言无余数。
3. **grid_cells 联动**：改比例后格数必须随高度字段重算，不得留旧的单边算法。
4. **MapBase 语义不被顺手改**：只测不改为红线（锚定测试即守卫）。
5. **非 16:9 选择持久于对话框会话**（同一次打开内切换不丢）——dialog 状态字段测试。

---

### Task 1: 默认海洋锚定测试

**Files:**
- Modify: `crates/wv-app/src/ui/new_project_dialog.rs`（tests 模块）
- Modify: `crates/wv-app/src/ui/new_map_dialog.rs`（tests 模块）

**Interfaces:** 无新接口；只加测试。

- [ ] **Step 1: 写失败/补齐测试**
  - `world_map_prefill_is_ocean_below_sea_level`：断言 new_project 首图 prefill == `MapBase::Ocean` 且 `MapBase::Ocean.cell().height < 0`（got 上下文）。
  - `new_map_dialog_default_prefill_is_ocean`：同款断言 `NewMapDialog::default().prefill`。
  - 若两测试直接通过（行为已在）：保留为回归锚，Commit 注明「锚定既有行为」。
- [ ] **Step 2: 验证** Run: `cargo test -p wv-app --features app prefill`
- [ ] **Step 3: Commit** `wv-app: 默认海洋世界锚定测试`

### Task 2: 新建地图对话框 16:9 缺省

**Files:**
- Modify: `crates/wv-app/src/ui/new_map_dialog.rs`（`NewMapDialog` 结构、extent UI、grid_cells、apply）
- Modify: `crates/wv-app/src/ui/new_project_dialog.rs`（首图 extent/grid 常量）
- Modify: `crates/wv-app/src/i18n/keys.rs`、`table.rs`（宽高比 Key：`new_map_aspect`、`aspect_16_9`、`aspect_4_3`、`aspect_1_1`）

**Interfaces:**
- `NewMapDialog` 字段变更：`extent: f32` → `extent_w: f32, extent_h: f32`（Default = 4096.0 × 2304.0）。
- `grid_cells()` → `grid_cells_w() -> u32` / `grid_cells_h() -> u32`（各自 `extent / 8.0`，沿用 `GRID_CELL_SIZE`）。
- apply：`Extent::sized(self.extent_w, self.extent_h)` + `TerrainData { width: grid_cells_w(), height: grid_cells_h(), .. }`。
- UI：幅面档位沿用 `EXTENT_CHOICES`（作用于宽），比例下拉 [16:9（缺省）/ 4:3 / 1:1] 换算高 = 宽 × 9/16 等。
- 首图（new_project_dialog ~line 176-196）：`Extent::sized(4096.0, 2304.0)`，grid `512×288`。

- [ ] **Step 1: 写失败测试**
  - `default_new_map_extent_is_16_9_with_whole_cells`：`grid_cells_w()==512 && grid_cells_h()==288`（got 上下文）。
  - `aspect_switch_updates_height_only`：4:3 → 高 = 宽×3/4。
  - 更新既有 `grid_cells_derive_from_extent_divided_by_cell_size`（new_map_dialog.rs:394）至新双字段口径。
- [ ] **Step 2: 跑确认失败** Run: `cargo test -p wv-app --features app new_map`
- [ ] **Step 3: 实现**（字段拆分 + UI + apply + i18n；宽高均为档位宽 × 比例，非 16:9 不告警——V1）。
- [ ] **Step 4: 全量验证**

Run:
```bash
cargo test -p wv-app --features app
cargo clippy -p wv-app --all-targets --features app -- -D warnings
cargo fmt --all -- --check
bash scripts/smoke-gui.sh
```

- [ ] **Step 5: Commit** `wv-app: 新建地图缺省 16:9 幅面（可改）`
