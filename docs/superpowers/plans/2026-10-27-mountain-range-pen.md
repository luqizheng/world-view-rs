# 山系笔与垂直带谱 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 山系笔（既有 `BrushTool::MountainRange`）的 kind 赋值升级为「实质抬升门槛 + 绝对高度落带」双判据垂直带谱，手涂语义零改，渲染层零改动。

**Architecture:** 全部数据侧改动收在 `wv-terrain/src/intent.rs` 的 `apply_mountain_range` 一处 kind 赋值块 + 一个私有纯函数 `band_biome_for_height`；`MountainMassif` 经共用函数自动同享；wv-app 只动 i18n 字符串。无数据模型 / 存储格式 / 渲染管线改动。

**Tech Stack:** Rust（workspace，edition 2021）、现有 cargo test 门禁；无新依赖。

**Spec:** `docs/superpowers/specs/2026-10-27-mountain-range-pen-design.md`（决策依据）＋ `docs/adr/ADR-0076-mountain-range-pen-altitudinal-zonation.md`（已接受）

## Global Constraints

- 带谱常数**只**用 `crate::continent::{HILL_MIN_M, MOUNTAIN_MIN_M, SNOW_LINE_M}`（`wv-terrain/src/continent.rs:289-293`，`pub(crate)`，i16）——任何位置不得出现第二份 300/1200/2600 字面量（设计稿反目标 A1）。
- `BrushTool::ALL` 枚举顺序**只增不改**（`wv-render/src/state.rs`，`CommandId::SelectBrushTool` 下标绑定）——本计划不增删枚举值。
- 手涂（`PaintKind` / CellMap `CellPaintValue`）写入口径**零改动**（设计稿反目标 A2 / V1 V2）。
- 不新增参数、不新增依赖、不改 `wv-render` / `wv-world-cell-map` / 存储格式。
- 测试名英文完整短句；数值断言附实际值上下文（AGENTS §7.3/§7.4）。
- i18n 中英双语都必须非空（`top_option_bar.rs` tests 有全键覆盖测试）。
- 公开 API 文档注释说明为什么；中文注释是预期。

## Review Focus

1. **既有高原被山系笔掠过 → 标签必须不动**：门槛（增量 ≥500 m）之下不写任何 kind。钉在 Task 1 的 `weak_stroke_leaves_existing_highland_labels_untouched`。
2. **既有测试 `(20,20) 脊上是山地` 可能因新口径翻车**：新口径下山地要求绝对高度 ≥1200 m；若 seed 7 下 (20,20) 不足，改断言为按 `band_biome_for_height(高度)` 核对（保留回归力），不得简单删除（Task 1 Step 3 注记）。
3. **`to_height` 取整口径**：integration 断言的边界比较允许 ±1 m 容差并注释取整依据（Task 1）。
4. **出水行为回归**：门槛外出水格仍写草原（现状先例「新露出水面的低肩是草原」）；非核心水域仍压回海平面以下（浅滩行为不动）。钉在 Task 1 的 `emerged_shallow_shoulder_is_grassland`。
5. **低地山肩行为变化（山地→丘陵）是有意变更**：老世界重画山脉观感会变，实机验收清单里有此项（Task 3），不是回归。

---

### Task 0: 分支 + worktree + 文档随迁

**Files:**
- Create: 分支 `feat/mountain-range-pen`、worktree `.worktrees/feat/mountain-range-pen`
- 迁移（主 checkout 现有未提交文档）: `docs/superpowers/specs/2026-10-27-mountain-range-pen-design.md`、`docs/adr/ADR-0076-mountain-range-pen-altitudinal-zonation.md`、`docs/superpowers/plans/2026-10-27-mountain-range-pen.md`（三个 untracked）；`docs/adr/README.md`、`GLOSSARY.md`（两个已修改）

- [ ] **Step 1: 建分支与 worktree（AGENTS §13，worktree 目录名 = 分支名）**

```bash
cd /d/kudi-projects/worldview-rs
git branch feat/mountain-range-pen
git worktree add .worktrees/feat/mountain-range-pen feat/mountain-range-pen
```

- [ ] **Step 2: 文档随迁**（主 checkout 不留重复改动）

```bash
git diff docs/adr/README.md GLOSSARY.md > .target/mrp-docs.patch
mkdir -p .worktrees/feat/mountain-range-pen/docs/superpowers/specs .worktrees/feat/mountain-range-pen/docs/superpowers/plans
mv docs/superpowers/specs/2026-10-27-mountain-range-pen-design.md .worktrees/feat/mountain-range-pen/docs/superpowers/specs/
mv docs/adr/ADR-0076-mountain-range-pen-altitudinal-zonation.md .worktrees/feat/mountain-range-pen/docs/adr/
mv docs/superpowers/plans/2026-10-27-mountain-range-pen.md .worktrees/feat/mountain-range-pen/docs/superpowers/plans/
git -C .worktrees/feat/mountain-range-pen apply .target/mrp-docs.patch
git restore docs/adr/README.md GLOSSARY.md
```

- [ ] **Step 3: Commit**

```bash
git -C .worktrees/feat/mountain-range-pen add -A docs
git -C .worktrees/feat/mountain-range-pen commit -m "docs: ADR-0076 山系笔与垂直带谱——设计稿/索引/术语登记/实施计划"
```

### Task 1: wv-terrain 带谱（TDD）

**Files:**
- Modify: `crates/wv-terrain/src/intent.rs`（imports `use crate::{...}` 约在 35-37 行、kind 赋值块约 510-518 行、`apply_mountain_range` 附近新私有函数、文件尾 `mod tests`）

**Interfaces:**
- Consumes: `crate::continent::{HILL_MIN_M, MOUNTAIN_MIN_M, SNOW_LINE_M}`（i16；`biome.rs:17` 有同款 import 先例）、`BIOME_SNOWFIELD`（加入 35 行起的 `use crate::{...}` 组）。
- Produces: `fn band_biome_for_height(new_h: f32) -> u8`（私有，`apply_mountain_range` 与测试共用；≥SNOW_LINE_M→`BIOME_SNOWFIELD`，≥MOUNTAIN_MIN_M→`BIOME_MOUNTAINS`，≥HILL_MIN_M→`BIOME_HILLS`，否则 `BIOME_GRASSLAND`）。

- [ ] **Step 1: 写失败测试**（`intent.rs` 的 `mod tests`，沿用既有 helper：`flat_land(w,h,height)` / `flat_sea(w,h,depth)` / `apply_intent(&mut g, &intent, seed, cell_size)`；测试 mod 内补 `BIOME_PLAINS`、`BIOME_SNOWFIELD` import）

```rust
/// 带谱阶梯的六个手工边界值：299/300、1199/1200、2599/2600。
#[test]
fn band_biome_ladder_matches_absolute_thresholds() {
    assert_eq!(band_biome_for_height(299.0), BIOME_GRASSLAND);
    assert_eq!(band_biome_for_height(300.0), BIOME_HILLS);
    assert_eq!(band_biome_for_height(1199.0), BIOME_HILLS);
    assert_eq!(band_biome_for_height(1200.0), BIOME_MOUNTAINS);
    assert_eq!(band_biome_for_height(2599.0), BIOME_MOUNTAINS);
    assert_eq!(band_biome_for_height(2600.0), BIOME_SNOWFIELD);
}

/// 实质抬升格按绝对高度落带：山腰是丘陵、山脊是山地，门槛外不写 kind。
#[test]
fn range_kinds_follow_absolute_ladder_on_raised_cells() {
    let mut g = flat_land(40, 40, 0);
    let intent = Intent::MountainRange {
        pts: vec![(8.5, 20.5), (32.5, 20.5)],
        radius: 5.0,
        strength: 1.0,
    };
    apply_intent(&mut g, &intent, 7, 1.0);
    let (mut hills, mut mountains) = (0, 0);
    for y in 0..40 {
        for x in 0..40 {
            let c = g.get(x, y).unwrap();
            match c.biome {
                BIOME_HILLS => {
                    hills += 1;
                    assert!(c.height >= 300 - 1, "丘陵格 ({x},{y}) 高度 {} 应 ≥300（±1 取整容差）", c.height);
                    assert!(c.height < 1200, "丘陵格 ({x},{y}) 高度 {} 应 <1200", c.height);
                }
                BIOME_MOUNTAINS => {
                    mountains += 1;
                    assert!(c.height >= 1200 - 1, "山地格 ({x},{y}) 高度 {} 应 ≥1200（±1 取整容差）", c.height);
                }
                BIOME_SNOWFIELD => {
                    assert!(c.height >= 2600 - 1, "雪原格 ({x},{y}) 高度 {} 应 ≥2600（±1 取整容差）", c.height);
                }
                _ => {}
            }
        }
    }
    assert!(hills > 0, "低地山肩应出现丘陵格，实得 {hills}");
    assert!(mountains > 0, "山脊应出现山地格，实得 {mountains}");
}

/// 弱笔触（strength 0.2 → 最大增量 2400×0.2=480 < 500 门槛）掠过既有高原：
/// 高度可被抬，但作者标签一个都不许动。
#[test]
fn weak_stroke_leaves_existing_highland_labels_untouched() {
    let mut g = flat_land(40, 40, 0);
    for y in 18..=22 {
        for x in 18..=22 {
            let mut c = g.get(x, y).unwrap();
            c.height = 1500;
            c.biome = BIOME_PLAINS;
            g.set(x, y, c);
        }
    }
    let intent = Intent::MountainRange {
        pts: vec![(10.5, 20.5), (30.5, 20.5)],
        radius: 5.0,
        strength: 0.2,
    };
    apply_intent(&mut g, &intent, 7, 1.0);
    for y in 18..=22 {
        for x in 18..=22 {
            let c = g.get(x, y).unwrap();
            assert_eq!(c.biome, BIOME_PLAINS, "高原格 ({x},{y}) 标签被山系笔改写（应保持 PLAINS）");
        }
    }
}

/// 出水低肩维持现状先例：新露出水面的低肩是草原。
#[test]
fn emerged_shallow_shoulder_is_grassland() {
    let mut g = flat_sea(40, 40, -200);
    let intent = Intent::MountainRange {
        pts: vec![(5.5, 20.5), (35.5, 20.5)],
        radius: 6.0,
        strength: 1.0,
    };
    apply_intent(&mut g, &intent, 11, 1.0);
    for x in 5..=35 {
        if let Some(c) = g.get(x, 20) {
            if c.height > 0 && c.height < 300 {
                assert_eq!(c.biome, BIOME_GRASSLAND, "出水低肩 ({x},20) 高度 {} 应为草原", c.height);
            }
        }
    }
}

/// MountainMassif 走同一 apply_mountain_range，带谱自动同享。
#[test]
fn massif_kinds_follow_same_ladder() {
    let mut g = flat_land(40, 40, 0);
    let intent = Intent::MountainMassif { polygon: vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)] };
    apply_intent(&mut g, &intent, 5, 1.0);
    let mountains = (0..40)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .filter(|&(x, y)| g.get(x, y).unwrap().biome == BIOME_MOUNTAINS)
        .count();
    assert!(mountains > 0, "高山群应出现山地格，实得 {mountains}");
    for y in 0..40 {
        for x in 0..40 {
            let c = g.get(x, y).unwrap();
            if c.biome == BIOME_HILLS {
                assert!(c.height < 1200, "高山群丘陵格 ({x},{y}) 高度 {} 应 <1200", c.height);
            }
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-terrain intent`（worktree 内）
Expected: FAIL——`band_biome_for_height` 未定义 / ladder 断言不成立（现状门槛上无差别写 MOUNTAINS）。

- [ ] **Step 3: 实现**——`intent.rs` 三处：
  1. imports：`use crate::{...}` 组补 `BIOME_SNOWFIELD`；新增 `use crate::continent::{HILL_MIN_M, MOUNTAIN_MIN_M, SNOW_LINE_M};`（照抄 `biome.rs:17`）。
  2. 新私有函数（放 `apply_mountain_range` 上方，带「为什么」文档注释：绝对高度口径与 `display_kind` NONE 回退、ADR-0056 阶梯同源）：

```rust
/// 带谱落带：只对实质抬升格调用；阈值与 `display_kind` 的 NONE 回退、
/// 大陆分带同源（ADR-0076 D1），禁止在此之外再写 300/1200/2600 字面量。
fn band_biome_for_height(new_h: f32) -> u8 {
    if new_h >= SNOW_LINE_M as f32 { BIOME_SNOWFIELD }
    else if new_h >= MOUNTAIN_MIN_M as f32 { BIOME_MOUNTAINS }
    else if new_h >= HILL_MIN_M as f32 { BIOME_HILLS }
    else { BIOME_GRASSLAND }
}
```

  3. kind 赋值块（`if new_h >= 0.0` 分支内）：`add >= RANGE_KIND_ADD_M` 支路 `biome = BIOME_MOUNTAINS` 改为 `biome = band_biome_for_height(new_h)`；`RANGE_KIND_ADD_M` 的文档注释同步改为「实质抬升门槛：门槛之下不写 kind（保既有地貌标签），门槛之上按绝对高度落带」。`else if water0` 支路与浅滩压回逻辑**不动**。
  4. **既有测试核对**：`mountain_range_is_higher_in_middle_than_at_ends` 的 `assert_eq!(... (20,20) ... BIOME_MOUNTAINS, "脊上是山地")`——若 seed 7 下 (20,20) 新高度 <1200 导致翻车，把断言改为 `assert_eq!(g.get(20,20).unwrap().biome, band_biome_for_height(g.get(20,20).unwrap().height as f32), "脊上 kind 应与带谱一致")`，不得删除断言。

- [ ] **Step 4: 跑测试确认全绿**（含被改的既有测试）

Run: `cargo test -p wv-terrain`
Expected: PASS（intent 模块全部 + 原有 suite）。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-terrain/src/intent.rs
git commit -m "wv-terrain: 山脉意图 kind 改双判据垂直带谱——500m 实质抬升门槛 + 绝对高度落带（ADR-0076 D1/D2）"
```

### Task 2: wv-app 文案（山系笔 / 微调标注）

**Files:**
- Modify: `crates/wv-app/src/i18n/table.rs:138-145`（zh 段）、`table.rs:828-840` 附近（en 段）

**Interfaces:**
- Consumes: 既有键 `BrushToolNameRange` / `BrushHintRange` / `BrushToolNamePaint` / `BrushHintPaint`（`keys.rs:259-273`，**不新增键**）。

- [ ] **Step 1: 改四处字符串**（中英各两键）
  - `BrushToolNameRange`：zh `"山脉"` → `"山系笔"`；en `"Mountain range"` → `"Mountain range pen"`。
  - `BrushHintRange`：zh 改 `"拖一条脊线长出山脉，走向与垂直带谱（山脚接周边、雪线积雪）程序补；单点 = 一座圆山"`；en 对应改写（保持既有长句折行风格）。
  - `BrushHintPaint`：zh 改 `"微调用：按住左键拖动，把选中的地形类型涂到格子上"`；en 前缀 `For touch-up: `。

- [ ] **Step 2: 跑覆盖测试确认双语非空断言仍绿**

Run: `cargo test -p wv-app --features app top_option_bar`
Expected: PASS（7 工具名称/提示中英非空全覆盖测试不加改动即通过）。

- [ ] **Step 3: Commit**

```bash
git add crates/wv-app/src/i18n/table.rs
git commit -m "wv-app: 山系笔文案——山脉→山系笔、涂抹标注微调用（ADR-0076 D3）"
```

### Task 3: 门禁 + 实机验收

**Files:** 无新增（验证任务）。

- [ ] **Step 1: 全门禁**（worktree 内；⚠️ worktree 跑测试**不要**用 `bash scripts/cargo-test.sh`——它强制新 testtmp 会踩代理权限门；把 TEMP/TMP 指向主 checkout 的信任 testtmp 后直接跑）

```bash
export TEMP=/d/kudi-projects/worldview-rs/.target/testtmp TMP=/d/kudi-projects/worldview-rs/.target/testtmp
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p wv-app --all-targets --features app -- -D warnings
```

Expected: 全绿。smoke-gui 本轮可跳过——未触渲染系统签名 / Plugin 装配 / 新增 Resource（AGENTS §7.1 触发条件不满足），如有疑虑跑 `bash scripts/smoke-gui.sh` 亦无害。

- [ ] **Step 2: 实机验收（用户）**——jrpg-world 世界图：
  1. 山系笔拖一条横穿森林与草原的山系 → 山脚各自衔接森林/草原、山腰丘陵、山脊山地、个别高峰带雪帽；
  2. 山系笔掠过既有高原 → 高原群系原样不动；
  3. 一次 Ctrl-Z 整条消失；
  4. 低地山肩从「山地」变「丘陵」属有意变更（Review Focus 5），观感异常再回报；
  5. **世界图上山系笔工具可达性**（设计稿 §6-2）：在世界地图视图确认能选中并拖画山系笔——若工作区门控导致不可达，带回来问用户，勿自行扩 scope 加入口。
