# 选择工具 M3 实施计划（地形四意图：高山 / 丘陵 / 平原 / 湖泊）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 意图菜单新增四种地形意图——闭合选区一键应用（无弹窗），`wv-terrain` 出区域掩膜算法，一意图一事务可撤销，掩膜外逐位不变。

**Architecture:** **计划修正**（相对 spec §9 的「wv-edit 新一族命令」）：给 `wv-terrain::Intent` 枚举加 4 个区域变体（`MountainMassif / HillsRegion / PlainsRegion / LakeRegion`，载荷 = 多边形），`apply_intent` 内部用「草稿克隆 → 内层意图 → 栅格化掩膜（噪声扰动边界）回拷」组合——`IntentTerrainCommand` **零改动**继承（一意图一事务 / seed 随命令 / 撤销重放）。理由：事务、dirty、撤销语义全部现成，新命令族是重复面。Ruling 记台账。

**Tech Stack:** 同前，无新依赖。

**Spec:** `docs/superpowers/specs/2026-10-17-select-tool-pen-intent-design.md` §4（算法映射）/ §12（一键应用、平原只在陆上、自交栅格化掩膜）/ §13（掩膜外逐位不变断言）。

## Global Constraints

- 同 M1/M2 六门禁；wv-terrain 零第三方依赖、纯函数 + 可变网格。
- 数值常量集中 intent.rs（沿 `RANGE_PEAK_M` 等先例）；强度初值 1.0；几何参数派生（山脊半径 = 短轴之半）。
- 语义边界（spec §4 表）：高山可跨海（岛链特性）；丘陵 / 平原只在陆上（纯海 = 0 改动 → wv-app notice）；湖泊质心须在陆上；平原**主动压平**到海平面以上低海拔带（`PLAINS_TARGET_M = 60` ± 15 噪声）。
- seed 确定性：沿 `Intent::derive_seed`（Debug 表示 FNV）——同参重算逐位一致。

## Review Focus

1. **掩膜外逐位不变**——Task 1 每个意图的测试都要断言 bbox 外格子逐位相等。
2. **退化环**（<3 点 / 零面积）安全返回 0，不 panic。
3. **湖心在海里** / **纯海丘陵平原** → 0 改动路径（wv-app 据此 notice）。
4. **noise 扰动边界的确定性**——同 seed 同环重算，掩膜边缘逐格一致。

---

### Task 1: `wv-terrain` 区域变体 + 掩膜机制

**Files:**
- Modify: `crates/wv-terrain/src/intent.rs`（4 个 `Intent` 变体 + `apply_intent` 分发 + 区域实现段：`region_spine` / `point_in_polygon` / `apply_region_masked` / 常量段追加 `PLAINS_TARGET_M` 等）

**Interfaces:**

```rust
pub enum Intent {
    /* 既有六变体不动 */
    /// 高山群：选区内沿主轴派生脊线跑 ridged 噪声；跨海段出岛链（特性）。
    MountainMassif { polygon: Vec<(f32, f32)> },
    /// 丘陵：区域内铺 `Hills` 低矮起伏；只在陆上生效。
    HillsRegion { polygon: Vec<(f32, f32)> },
    /// 平原：区域内**主动压平**到 `PLAINS_TARGET_M` ± 噪声；只在陆上生效。
    PlainsRegion { polygon: Vec<(f32, f32)> },
    /// 湖泊：区域定湖界的碗形洼地；质心在海上 = 无操作。
    LakeRegion { polygon: Vec<(f32, f32)> },
}

/// 主轴脊线：协方差主方向投影端点向质心收缩 20%（山脊不出掩膜）；半径 = 垂直半幅。
pub fn region_spine(polygon: &[(f32, f32)]) -> Option<(Vec<(f32, f32)>, f32)>;
```

掩膜机制（核心算法，测试钉行为）：

```text
apply_region_masked(grid, polygon, seed, cell_size, |scratch| { 内层意图或直写 }):
  1. scratch = grid.clone()
  2. 闭包改 scratch
  3. 只扫 polygon bbox：mask(p) = point_in_polygon(p, polygon 扰动版)
     扰动 = fbm(seed, "mask_wiggle") 逐格偏移查询点（幅度 = 2×cell_size，有界）
     内 → grid 写 scratch 同格；外 → 不动
```

- [ ] **Step 1: 失败测试**（intent.rs tests，手算期望值 + 数值上下文）：

```rust
#[test]
fn massif_ridge_runs_along_long_axis()                 // 2:1 长方形 → 脊线沿长轴；radius ≈ 短轴/2
#[test]
fn massif_raises_inside_and_keeps_outside_bitwise()    // 高地内 h 明显上升；bbox 外逐位 ==
#[test]
fn hills_region_only_changes_land_cells()              // 掩膜内海格不动（Hills 语义继承）
#[test]
fn plains_region_flattens_land_to_target_band()        // 掩膜内 200m 高地 → 60±15m；海格不动；外逐位 ==
#[test]
fn lake_region_digs_water_bowl_below_sea()             // 掩膜内 h<0 且中心最深；质心在海 → 0 改动
#[test]
fn degenerate_region_returns_zero_changes()            // 空环 / 2 点 → 0、不 panic
```

- [ ] **Step 2:** FAIL（编译）→ **Step 3:** 实现。
- [ ] **Step 4:** PASS → Commit：`wv-terrain: 区域地形意图（掩膜栅格化 + 四变体）`。

---

### Task 2: `wv-edit` 复用验证（零改动确认）

**Files:**
- Modify: `crates/wv-edit/src/command.rs` tests（追加，不改实现）

- [ ] **Step 1: 测试**：

```rust
#[test]
fn region_intent_command_undo_restores_bitwise()          // commit → 网格指纹变 → undo → 逐位复原
#[test]
fn region_intent_seed_is_deterministic_for_same_polygon() // 同环两次构造 → 同 seed
```

- [ ] **Step 2:** 直接实现测试（命令零改动；若签名不匹配则 Ruling 记台账）→ Commit：`wv-edit: 区域意图命令复用验证（undo/seed 确定性）`。

---

### Task 3: `wv-app` 菜单接线（四按钮一键 + 无操作通知）

**Files:**
- Modify: `crates/wv-app/src/edit/editing.rs`（`pub fn commit_region_intent(map, intent, cell_size, edit, projection, tex, notice, frame) -> bool`——before/after diff 标脏沿 `apply_reset_terrain_to_base` 同款；0 改动 → `Err` 语义由调用方 notice）
- Modify: `crates/wv-app/src/ui/intent_menu.rs`（目录 → 6；四按钮：点击 → 从 `selection.closed` 构造 `Intent` 变体 → `commit_region_intent` → 成功 `selection.cancel()`（spec D9；**不回驻留**，spec §4））
- Modify: `crates/wv-app/src/i18n/keys.rs` + `table.rs`（`IntentMenuHills/Plains/Lake` + `IntentMenuMountains` 4 键 zh/en + 无操作通知键 `IntentNoticeNoOp`）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（无新资源；既有注册不动）

**Interfaces:**

```rust
// editing.rs
pub fn commit_region_intent(
    map: wv_core::Id, intent: wv_terrain::intent::Intent, cell_size: f32,
    edit: &mut EditHistory, projection: &mut WorldProjection,
    tex: &mut TerrainTexture, notice: &mut ShellNotice, frame: u64,
) -> bool;   // true = 已生效（含选区清空由调用方做）；0 改动 → false + notice 已发
```

- [ ] **Step 1: 失败测试**（intent_menu tests）：

```rust
#[test]
fn intent_catalog_has_six_entries_for_world_and_region()  // 2→6（FAIL）
```

（commit_region_intent 的行为由 Task 1/2 的 wv-terrain/wv-edit 测试背书；本任务只测目录计数 + 接线编译。）

- [ ] **Step 2:** FAIL → 实现（四按钮 + i18n + commit 路径）。
- [ ] **Step 3:** 六门禁 + smoke 全绿。
- [ ] **Step 4:** Commit：`wv-app: 地形四意图一键应用（菜单接线 + 无操作通知）`。

---

### Task 4: spec 追记 + 收尾

- [ ] spec M2 追记后加 **M3 完成注记**（含计划修正说明：复用 IntentTerrainCommand 而非新命令族）。
- [ ] 六门禁复跑 → Commit：`docs: 选择工具 M3 完成追记`；台账收口 + 工作区清理。

## Self-Review 记录

- spec §4 表五行全覆盖（森林除外，M4）；§12 六裁定中「一键」「平原只在陆上」「自交栅格化」进 Task 1/3；§13 掩膜外逐位不变 → Task 1 逐测试断言。
- 签名一致：`region_spine` / `commit_region_intent` 定义与消费处逐字核对；`Intent` 新变体被 `derive_seed`（Debug）自动覆盖。
- 城市对话框 / 大陆流程不受影响（不触其路径）。
