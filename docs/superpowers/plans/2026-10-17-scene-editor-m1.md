# 场景编辑器 M1（分区刷色）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在场景图（City / Interior 级）上用笔刷刷「场景分区」色块（贫民区/富人区/住宅/商业区/工业/农田/密林/空地/房间/走廊/大殿），首次刷色自动引导建基底网格，可撤销，地图色块视图与 CellMap 方格视图都能看见颜色。

**Architecture:** 复用 `TerrainGrid` 的 **biome 通道**承载分区码（21..=32 保留带），**不动二进制格式、不加第四通道、不改结构体**。颜色走**旁挂色表**（主题表 + CellMap 固定表各一份，沿用项目既有的「主题色/固定色」分裂），着色判定**纯格级**（biome 落在保留带即分区），因此渲染层零「场景图」概念。

**Tech Stack:** Rust 2021 / serde_yaml / Bevy 0.19（仅 wv-app feature="app" 下的 UI）

**Spec:** `docs/superpowers/specs/2026-10-17-scene-editor-design.md`（设计裁定 R1–R6 仍有效；**本计划是它的数据层修订版**——见下方「基线变更」）

> ## ⚠️ 基线变更（2026-10-29 重做，务必先读）
>
> 本计划最初写于 2026-10-17，基线是 `plan/2026-10-17-all-plans`。**12 天内项目地基已变两次**，
> 初稿的断言全部失效。本版按 **main @ b8b7aad** 重写。
>
> **已变的事实**（代码核实，非推测）：
> - 地形数据模型从「一维 kind」改为**三通道** `TerrainCell { height, surface, biome }`
>   （`crates/wv-terrain/src/lib.rs`）。**`TerrainKind` 类型已不存在**，
>   `is_water_kind(kind) = kind <= 1` 已不存在（改为 `is_water_cell(c)`，
>   口径是 `surface == WATER || height < 0`，`crates/wv-terrain/src/edit.rs:27`）。
> - 群系编号表**冻结 0..=20**（`crates/wv-terrain/src/biome.rs:25-67`），16=河流 17=绿洲
>   18=稀树草原 19=盐碱地 20=道路。**初稿打算占用的 16..=27 与既有群系撞了 5 个。**
> - 调色板槽位空间 **0..=9 已饱和**（`PaletteColors.core`/`kind_core` 各 `[Color;10]`），
>   越界静默回退 `core[8]`。**扩槽位会炸 `shade_google` 折叠 / `is_boundary` / tile atlas。**
> - 主题层已独立成 `wv-default-theme` crate；**Dusk 主题已按 ADR-0066 整族退役**，
>   `MapPalette` 只剩 `Daylight`。
> - 源文件大面积拆分：`wv-render/src/cellmap_bridge.rs` → `cellmap_bridge/`（11 子模块，
>   `conv_palette` 搬到了 `theme_conv.rs:48`）；`wv-edit/src/command/command_tests.rs` →
>   `command_tests/`（11 子模块）；`wv-app/src/edit/editing.rs` → `edit/editing/`；
>   `wv-app/src/ui/widgets/primitives.rs` → `ui/widgets/local.rs`。
>   **笔刷材料条的真实入口是 `ui/top_option_bar.rs:239` `material_strip`**。
> - i18n `ALL` = **574**（`crates/wv-app/src/i18n/keys.rs:1400`）。
> - `MapNode` 现有 18 个字段（新增 `comment` / `trashed` / `trashed_at`），
>   字面量构造 **139 处 / 88 处穷举式** → 加字段返工量巨大。
> - ADR 编号已到 **0086**，下一个可用 **0087**（`docs/adr/README.md`）。
> - `GLOSSARY.md` 已 **605 行**，已有「分区/District」「场景/Scene」「场景编辑器」
>   「场景类型」四条（`:388-391`，2026-10-22 登记）。**另有一个「分区/partition」
>   （势力图切蛋糕，`:542`）与一个「自动分区」（`:326`）——「分区」一词已三义。**
> - `docs/glossary.md` 是**过期快照**（454 行 vs 605 行，缺 2026-10-24 之后全部内容），
>   **只更新仓库根 `GLOSSARY.md`**。
>
> **本版的三个关键决策**（均经代码核实）：
>
> 1. **分区码占用 biome 通道 21..=32**，不改二进制版本、不加第四通道、
>    不动 254 处结构体字面量。代价是「同通道两语义」（场景图装分区、世界图装群系）——
>    一张图只用一种语义，且 `family_of` 对 ≥21 返回 `None` 使分区不落任何群系族，
>    主题覆盖物规则不会误触发。
> 2. **颜色走旁挂色表**：`PaletteColors.district: [Color;12]`（主题侧）
>    + `biome_raw::DISTRICT_RAW_COLORS`（CellMap 固定侧，ADR-0054 D4 硬约束）。
>    沿用项目既有的 `core`(主题) / `BIOME_RAW_COLORS`(固定) 分裂，不合并。
> 3. **着色判定纯格级**（biome 落在保留带即分区），**渲染层零「场景图」概念**。
>    这不是省事，是**必须**——`active_terrain`
>    （`crates/wv-render/src/terrain_view/terrain_resolve.rs`）沿父链继承，
>    「当前图」≠「正在渲的那张图」；按图级判定会对着一张世界图的地形盖分区色。

## Global Constraints

- 分层红线：wv-core/wv-terrain 零 Bevy 零网络；wv-edit 只经 EditCommand 改世界；
  wv-render 不依赖 wv-edit/wv-app（§2.2 五条 `cargo tree` 提交前必跑）。
- **wv-terrain 零第三方依赖**（`crates/wv-terrain/Cargo.toml` 的 `[dependencies]` 是空的），
  不得持有 `bevy::color::Color`，只能放 `&'static str` / `u8` / `[[u8;3];N]`。
- **wv-world-cell-map 不依赖 wv-default-theme**（ADR-0054 D4 固定色禁令）——
  两者通过 `crates/wv-render/src/cellmap_bridge/theme_conv.rs:48` `conv_palette` 桥接。
- 一切世界改动走 `EditCommand`（trait + before 缓存 + `Transaction`），禁止 System 直改 `World`。
- YAML round-trip 是硬契约：新增字段必须 `#[serde(default, skip_serializing_if = "Option::is_none")]` 并配 round-trip 测试。
- i18n：新文案 = `i18n/keys.rs` 变体 + `ALL` 数组 + `table.rs` 两个穷尽 match 各一条（漏 arm 编译错）。
- 提交信息 `<scope>: <动作>`；每个 Task 一提交，只 add 该任务列出的文件。
- 门禁：`cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` /
  `cargo fmt --all -- --check`，另加渲染层 `cargo test -p wv-app --features app`、
  `cargo clippy -p wv-app --all-targets --features app -- -D warnings`、`bash scripts/smoke-gui.sh`。
- worktree 内**不用** `scripts/cargo-test.sh`（它写固定路径，多 worktree 并行会互踩）；
  直接 `cargo test`，需要写盘时显式 `export TEMP=... TMP=...` 指向本 worktree 内的目录。

## Review Focus

1. **分区码误入群系段（≤20）** → 被当成森林/海洋渲染，或 `biome_raw::pick` 索引越界 **panic**。
   合理预期：21..=32 全部可渲；20 及以下行为逐字不变。→ Task 1 / Task 4 测试。
2. **场景图笔刷凭空造出地形** → 作者只想涂个分区色，结果地上隆起 40 m 山包。
   合理预期：刷分区**绝对不改高度**（含在海洋基底 -3500 m 上刷）。→ Task 2 测试。
3. **Topographic 模式一格分区就崩** → `biome_raw::pick` 越界 panic（表长 21，索引 21+）。
   合理预期：不崩；显示分区固定色。→ Task 4 测试。
4. **水彩晕开把分区色冲淡** → 5×5 邻域平均把 1 格的分区色稀释成周围自然地形色，
   分区几乎看不出（实测商业区赭黄被草原绿冲到 `[144,184,108]`）。
   合理预期：分区色不被邻域污染。→ Task 5 测试。
5. **撤销后格子残留** → 引导+首笔分两步撤销。合理预期：一步撤销，格子全退。→ Task 6 测试。

---

### Task 1: 分区编号带与词表（wv-terrain）

**Files:**
- Modify: `crates/wv-terrain/src/biome.rs`（`BIOME_ROAD = 20` 在 `:67` 之后加保留带常量；
  `pub type Biome = u8` 在 `:70`；族划分 doc 在 `:76-84` 补一句）
- Test: `crates/wv-terrain/src/biome.rs` `#[cfg(test)]`（`mod tests` 在 `:215`）

**Interfaces:**
- Produces:
  - `pub const DISTRICT_FIRST: Biome = 21;`
  - `pub const DISTRICT_COUNT: u8 = 12;`
  - `pub fn is_district(b: Biome) -> bool`
  - `pub fn district_index(b: Biome) -> Option<u8>`（0..=11）
  - `pub const DISTRICT_NAMES_ZH: [&str; 12]`

- [ ] **Step 1: 写失败测试**（追加到 `mod tests`，用 `super::*` 已有导入）

```rust
/// 场景分区保留带：从冻结群系表 21 之上起，12 个码位，带外不入。
#[test]
fn district_band_is_reserved_above_frozen_biomes() {
    assert_eq!(DISTRICT_FIRST, 21, "分区带必须从冻结表 20 之上起");
    assert_eq!(DISTRICT_COUNT, 12);
    assert_eq!(DISTRICT_NAMES_ZH.len(), 12);
    for i in 0..DISTRICT_COUNT {
        let b = DISTRICT_FIRST + i;
        assert!(is_district(b), "{b} 应是分区");
        assert_eq!(district_index(b), Some(i));
        assert!(!DISTRICT_NAMES_ZH[i as usize].is_empty(), "分区 {b} 缺名");
    }
    assert!(!is_district(DISTRICT_FIRST - 1), "20 是道路群系，不是分区");
    assert!(!is_district(DISTRICT_FIRST + DISTRICT_COUNT), "带外不是分区");
    assert_eq!(district_index(255), None);
    assert_eq!(district_index(0), None);
}
```

同时给既有的 `family_of_returns_none_for_unregistered_ids` 补一条断言：
分区带内编号也无族（分区不落任何群系族，主题覆盖物规则不误触发）。

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-terrain district_band` → FAIL（未定义）
- [ ] **Step 3: 实现** — 12 个分区码的语义（顺序固定，索引即色表索引）：

  | idx | 码 | 中文名 | 用途 |
  |---|---|---|---|
  | 0 | 21 | 未分区 | 场景基底（未涂色） |
  | 1 | 22 | 贫民区 | 城市 |
  | 2 | 23 | 富人区 | 城市 |
  | 3 | 24 | 普通住宅 | 城市 |
  | 4 | 25 | 商业区 | 城市 |
  | 5 | 26 | 工业区 | 城市 |
  | 6 | 27 | 农田 | 村落/农村 |
  | 7 | 28 | 密林 | 森林 |
  | 8 | 29 | 空地 | 森林/空地 |
  | 9 | 30 | 房间 | 地下城 |
  | 10 | 31 | 走廊 | 地下城 |
  | 11 | 32 | 大殿 | 地下城 |

  在 `pub type Biome = u8;` 之后加保留带区块（含 `DISTRICT_NAMES_ZH` 与两个判定函数）。
  `is_district` 用 `(DISTRICT_FIRST..DISTRICT_FIRST + DISTRICT_COUNT).contains(&b)`
  （**clippy `identity_op` / `manual_range_contains` 会挑手写比较**——已实测）。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-terrain` → 全 PASS
- [ ] **Step 5: Commit** — `git add crates/wv-terrain/src/biome.rs && git commit -m "wv-terrain: 场景分区编号保留带 21..=32 与词表"`

### Task 2: 分区笔刷不改高度（wv-terrain）

**Files:**
- Modify: `crates/wv-terrain/src/edit.rs`（`apply_brush` 在 `:198`，
  「跨域推高度」if-else 在 `:240-246`）
- Test: `crates/wv-terrain/src/edit.rs` `#[cfg(test)]`（`mod tests` 在 `:316`）

**Interfaces:**
- Consumes: Task 1 `is_district`
- Produces: `apply_brush` 行为变更——`target.biome` 是分区码时，**只写 surface+biome，绝不动 height**。

- [ ] **Step 1: 写失败测试**

```rust
/// 场景分区笔刷**绝不改高度**——分区是同一张基底图上的涂色，
/// 作者只想标「这块是贫民区」，不该凭空多出一座 40 m 的山包。
#[test]
fn painting_a_district_never_changes_height() {
    let mut g = TerrainGrid::new(20, 20);
    for gy in 0..20 {
        for gx in 0..20 {
            g.set(gx, gy, TerrainCell {
                height: -3500,
                surface: SURFACE_WATER,
                biome: BIOME_OCEAN,
            });
        }
    }
    let b = Brush::paint(
        (10.5, 10.5),
        5.0,
        PaintTarget {
            surface: SURFACE_LAND,
            biome: crate::biome::DISTRICT_FIRST + 1,
        },
    );
    paint(&mut g, &b, 1.0);
    let cell = g.get(10, 10).unwrap();
    assert_eq!(cell.height, -3500, "刷分区不得改高度（实得 {}）", cell.height);
    assert_eq!(cell.biome, crate::biome::DISTRICT_FIRST + 1, "分区码应写入");
    assert_eq!(cell.surface, SURFACE_LAND, "分区笔刷写陆面");
}

/// 回归护栏：非分区笔刷的「跨域推高度」旧行为必须逐字不变。
#[test]
fn painting_a_normal_biome_still_pushes_height_across_sea_level() {
    let mut g = TerrainGrid::new(20, 20);
    for gy in 0..20 {
        for gx in 0..20 {
            g.set(gx, gy, TerrainCell {
                height: -3500,
                surface: SURFACE_WATER,
                biome: BIOME_OCEAN,
            });
        }
    }
    let b = Brush::paint(
        (10.5, 10.5),
        5.0,
        PaintTarget { surface: SURFACE_LAND, biome: BIOME_FOREST },
    );
    paint(&mut g, &b, 1.0);
    assert!(g.get(10, 10).unwrap().height >= 0, "非分区笔刷仍应推过海平面");
}
```

  测试模块需补 `use crate::{BIOME_OCEAN, SURFACE_WATER};`（现有导入在 `:317-318`）。
- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-terrain painting_a_district` → FAIL（高度被改成 +40）
- [ ] **Step 3: 实现** — 在 `apply_brush` 的 `if target.surface == SURFACE_WATER { … } else if cell.height < 0 { … }`
  外包一层 `if !crate::biome::is_district(target.biome) { … }`，并加注释说明「分区是同一张
  基底图上的涂色，不该造出地形起伏；高度域规则是为涂抹自然地表设计的，分区永远写陆面，不涉及水陆切换」。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-terrain` → 全 PASS
- [ ] **Step 5: Commit** — `git commit -m "wv-terrain: 分区笔刷只改分区码不改高度"`

### Task 3: 主题侧分区色表（wv-default-theme）

**Files:**
- Modify: `crates/wv-default-theme/src/colors.rs`（`pub const fn hex` 在 `:35`；
  `CORE_TERRAIN` 在 `:51`；在 `COAST_LINE`（`:67`）之前加分区色常量）
- Modify: `crates/wv-default-theme/src/palette.rs`（`PaletteColors` 在 `:100`，
  `core` 在 `:102`，`kind_core` 在 `:104` → 之后加 `district` 字段；
  `DEFAULT` 字面量在 `:220`，`core:` 在 `:221`、`kind_core:` 在 `:222` → 之后加 `district:`）
- Test: `crates/wv-default-theme/src/palette.rs` `#[cfg(test)]`（`mod tests` 在 `:271`）

**Interfaces:**
- Consumes: Task 1 `DISTRICT_COUNT`
- Produces: `PaletteColors.district: [Color; 12]`（索引 = `district_index`），唯一构造点是 `DEFAULT`。

- [ ] **Step 1: 写失败测试**

```rust
/// 场景分区色板：12 槽对应分区保留带 21..=32。
///
/// 为什么旁挂一张表而不扩 `core`：`core` / `kind_core` 的十槽空间已被
/// 「群系 → 旧调色板槽位」投影占满，槽位语义被 `shade_google` 折叠规则、
/// `is_boundary` 边界判定、tile atlas 三处共同依赖——扩槽位会连带炸这三处。
#[test]
fn district_palette_has_one_slot_per_district() {
    let p = PaletteColors::DEFAULT;
    assert_eq!(p.district.len(), 12, "分区色板须与分区带等长");
    let fallback = p.core[8];
    for (i, c) in p.district.iter().enumerate() {
        assert_ne!(*c, fallback, "分区色槽 {i} 落进回退色");
    }
    for i in 0..p.district.len() {
        for j in (i + 1)..p.district.len() {
            assert_ne!(p.district[i], p.district[j], "分区色槽 {i} 与 {j} 必须不同");
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-default-theme district_palette` → FAIL（无字段）
- [ ] **Step 3: 实现** — 12 个主题色（Daylight；**Dusk 已退役，无第二套**）：

  | idx | 语义 | Daylight |
  |---|---|---|
  | 0 | 未分区 | `#b8b09a` |
  | 1 | 贫民区 | `#8a7a5f` |
  | 2 | 富人区 | `#cbb27a` |
  | 3 | 普通住宅 | `#d8c9a3` |
  | 4 | 商业区 | `#c98a4b` |
  | 5 | 工业区 | `#8f8577` |
  | 6 | 农田 | `#b5b356` |
  | 7 | 密林 | `#4f7a3a` |
  | 8 | 空地 | `#a3a86e` |
  | 9 | 房间 | `#a89a8a` |
  | 10 | 走廊 | `#7d7268` |
  | 11 | 大殿 | `#b39a6b` |

  在 `colors.rs` 加 `pub const DISTRICT_COLORS: [Color; 12]`（用同文件的 `hex()`），
  在 `palette.rs` 加字段与 `district: colors::DISTRICT_COLORS,`。
  配色意图注释：功能区之间靠**色相 + 明度**双重区分。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-default-theme` → PASS
- [ ] **Step 5: Commit** — `git commit -m "wv-default-theme: 分区色板 12 色"`

### Task 4: CellMap 侧固定色 + CellPalette 镜像 + pick 越界修复（wv-world-cell-map）

**Files:**
- Modify: `crates/wv-world-cell-map/src/terrain_view/biome_raw.rs`
  （`pick` 在 `:30`；`BIOME_RAW_COLORS` 在 `:46`；`BIOME_NAMES_ZH` 在 `:75`；
  `mod tests` 在 `:126`，use 导入在 `:127-131`）
- Modify: `crates/wv-world-cell-map/src/palette.rs`
  （`hex` 在 `:23`；`CORE_TERRAIN` 在 `:31`；`CellPalette` 在 `:98`、`core` 在 `:100`；
  `impl CellPalette` 在 `:174`；`CellPack::fixture()` 在 `:424`、`CellPalette {` 在 `:426`、
  `core:` 在 `:427`）
- Modify: `crates/wv-render/src/cellmap_bridge/theme_conv.rs`（`conv_palette` 在 `:48`，
  `CellPalette {` 在 `:49`、`core:` 在 `:50` → 之后加 `district:`）
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 1 `is_district` / `district_index`；Task 3 `PaletteColors.district`
- Produces:
  - `biome_raw::DISTRICT_RAW_COLORS: [[u8; 3]; 12]`
  - `biome_raw::pick(biome, surface)` **对 ≥21 返分区色、对 >32 返兜底色而非 panic**
  - `CellPalette.district: [Color; 12]` + `CellPalette::district_color(index) -> Color`

- [ ] **Step 1: 写失败测试**

```rust
/// 场景分区（biome 21..=32）走**独立固定色表**，不进群系色表。
///
/// 回归护栏两条：
/// 1. 分区色不得等于草原色——否则分区与自然地形混作一谈；
/// 2. 越界编号不得 panic——`BIOME_RAW_COLORS[biome]` 直查会越界崩，
///    开放集里未登记编号是常态。
#[test]
fn pick_returns_district_color_instead_of_panicking() {
    let d = biome::DISTRICT_FIRST;
    assert_eq!(DISTRICT_RAW_COLORS.len(), 12);
    for i in 0..biome::DISTRICT_COUNT {
        let rgba = pick(d + i, SURFACE_LAND);
        assert_eq!(rgba, rgb_to_rgba(DISTRICT_RAW_COLORS[i as usize]), "分区 {i} 应取分区固定色");
    }
    let out = pick(200, SURFACE_LAND);
    assert_eq!(out.len(), 4, "越界应回退而非崩");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-world-cell-map pick_returns_district` → FAIL（panic）
- [ ] **Step 3: 实现**
  - `pick()` 加分支：`if let Some(i) = biome::district_index(biome) { return rgb_to_rgba(DISTRICT_RAW_COLORS[i as usize]); }`，
    并把末尾的 `BIOME_RAW_COLORS[biome as usize]` 改成 `.get(biome as usize).copied().unwrap_or(FALLBACK_LAND)`
    （**当前 biome≥21 直接 panic**——必修项）。
  - `BIOME_NAMES_ZH` **保持 21 项不动**（它是「冻结群系表」的名字表）；分区名走
    `DISTRICT_NAMES_ZH`（Task 1 已定义于 wv-terrain，此处不需要 re-export）。
  - `BIOME_LEGEND_GROUPS` **不含分区**（分区是功能区不是自然群系）；
    `biome_legend_groups_cover_every_biome_exactly_once` 的 `seen[0u8;21]` **不动**。
  - `CellPalette` 加 `district: [Color; 12]` + `district_color(index)` 方法；
    `CellPack::fixture()` 填入 12 个值（与 `DISTRICT_RAW_COLORS` 同色但 `Color` 形）。
  - `conv_palette`（`theme_conv.rs:48`）加 `district: p.district,`。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-world-cell-map` → 全 PASS；
  `cargo build --workspace --all-targets` → 无遗漏构造点
- [ ] **Step 5: Commit** — `git commit -m "wv-world-cell-map: 分区固定色与 pick 越界修复"`

### Task 5: 渲染着色分支 + 分区边界（wv-world-cell-map）

**Files:**
- Modify: `crates/wv-world-cell-map/src/terrain_view/cells.rs`
  （`display_slot` `:45`；`slot_is_water` `:51`；`shade_for_mode` `:91`；
  `shade_google` `:140`；`base_color_of` `:296`；`blended_base` `:376`；
  `land_grain` `:421`；`block_for` `:750`；`is_boundary` `:915`（`differs` 闭包 `:917-927`）；
  `sample_vector_flat` `:991`（shader 调用 `:1029`）；`sample_kind` `:1060`（shader 调用 `:1081`）；
  `write_cell_split` `:1255`（`block_for` 调用 `:1293`、shader 调用 `:1332`、
  `apply_elevation_band` 调用 `:1335`）；`terrain_pixels` `:1455`；`mod tests` `:1835`；
  `topographic_fixture` `:1962`）
- Test: `crates/wv-world-cell-map/src/terrain_view/cells.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 1 `district_index`；Task 3/4 两侧色表
- Produces: 4 个取色决策点在调用 shader **之前**判分区并直接返回分区色。

- [ ] **Step 1: 写失败测试**

```rust
/// 场景分区格必须渲成分区色，**不得**落进群系槽位兜底（草原绿）。
///
/// 覆盖两条取色路径：Topographic（方格视图，固定色）与 VectorFlat（平涂视图，主题色）。
#[test]
fn district_cell_renders_with_district_color_not_grassland() {
    let mut data = topographic_fixture();
    let district = wv_terrain::biome::DISTRICT_FIRST + 4; // 商业区
    data.cells[5].biome = district;
    data.cells[5].surface = wv_terrain::SURFACE_LAND;
    data.cells[5].height = 0;

    // ① Topographic：分区固定色。
    let pixels = topographic_pixels(&data);
    let o = ((4 + 1) * 4) as usize; // (x=1, y=1)，4×4 网格 stride=4
    let got = [pixels[o], pixels[o + 1], pixels[o + 2]];
    let raw = crate::terrain_view::biome_raw::DISTRICT_RAW_COLORS[4];
    assert_eq!(got, [raw[0], raw[1], raw[2]], "Topographic 下分区格应取分区固定色");

    // ② VectorFlat：主题分区色。
    let pack = CellPack::fixture();
    let (px, res) = terrain_pixels(&data, ShadeMode::VectorFlat, None, false, &pack);
    // 平涂是亚像素分辨率——取格心那个像素。stride = 格宽 × res（不是 4）。
    let stride = 4 * res;
    let cx = res + res / 2;
    let cy = res + res / 2;
    let o = ((cy * stride + cx) * 4) as usize;
    let got = [px[o], px[o + 1], px[o + 2]];
    let Color::Srgba(c) = pack.palette.district[4] else { panic!("应为 Srgba") };
    let want = [
        (c.red * 255.0).round() as u8,
        (c.green * 255.0).round() as u8,
        (c.blue * 255.0).round() as u8,
    ];
    // 陆格颗粒（ADR-0042）会让色值有 ±4.5% 抖动——容差比较。
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert!((*g as i32 - *w as i32).abs() <= 6,
            "平涂视图下分区格应取主题分区色（通道 {i}：得 {g} 期望 {w}）");
    }
    // 并且必须显著区别于草原色——分区色真的出现了，不是「没崩」。
    let Color::Srgba(gr) = pack.palette.core[3] else { panic!("应为 Srgba") };
    let gd = (got[0] as i32 - (gr.red * 255.0) as i32).abs()
        + (got[1] as i32 - (gr.green * 255.0) as i32).abs()
        + (got[2] as i32 - (gr.blue * 255.0) as i32).abs();
    assert!(gd > 60, "分区色不得接近草原色（实测差 {gd}）");
}

/// 相邻的两个不同分区必须被判为**边界**——否则分区交界没有揉边/抗锯齿。
#[test]
fn adjacent_districts_are_detected_as_boundary() {
    let mut data = topographic_fixture();
    data.cells[5].biome = wv_terrain::biome::DISTRICT_FIRST + 1;
    data.cells[6].biome = wv_terrain::biome::DISTRICT_FIRST + 4;
    let pack = CellPack::fixture();
    let renderer = CellRenderer::new(&data, ShadeMode::VectorFlat, None, false, &pack, None);
    assert_eq!(display_slot(&data.cells[5]), display_slot(&data.cells[6]),
        "前置：两个分区的 display 槽位相同");
    assert!(renderer.is_boundary(1, 1), "相邻的两个不同分区应判为边界");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-world-cell-map district_cell_renders` → FAIL（渲成草原色）
- [ ] **Step 3: 实现**
  - 加 helper（放 `cells.rs` 顶部，紧邻 `slot_is_water`）：
    ```rust
    /// 场景分区的显示色（biome 落在 21..=32 保留带 → 旁挂分区色表）。
    ///
    /// 为什么绕过 `display_slot`：分区的 biome 编号不在冻结群系表里，
    /// `display_kind` 对未登记编号落 `_ => 3`（草原兜底）——分区会被渲成草绿。
    ///
    /// 为什么判定是格级而非图级：`active_terrain` 沿父链继承（场景图可能
    /// 借用父图地形），「当前图」≠「正在渲的那张图」。格级判定天然正确。
    fn district_color(c: &TerrainCellData, pal: &CellPalette) -> Option<Color> {
        let idx = wv_terrain::biome::district_index(c.biome)?;
        pal.district.get(idx as usize).copied()
    }
    ```
  - **4 个取色点**各改成 `match district_color(cell, pal) { Some(c) => c, None => (self.shader)(data, kind, pal) }`
    —— **分支必须在 shader 调用之前**（不是后处理覆盖），否则 `shade_google`（`:140`）
    会把分区 display_kind=3 折叠成近白陆色。
    4 处都能零成本拿到 cell：`base_color_of` 有 `cell` 局部变量；
    `sample_vector_flat` 用 `(cy * self.w + cx)` 索引；`sample_kind` 有 `cell`；
    `write_cell_split` 有 `cell`。
  - **`write_cell_split` 的图集快路径要绕开**：分区格的 display 槽位恒为 3，
    `self.block_for(kind).is_some()`（`:1293`）会命中**草原瓦片块**并提前 return，
    分区色根本不出现。改成 `let is_district = district_color(cell, self.palette).is_some();`
    并 `if !is_district && self.block_for(kind).is_some() {` —— 分区走下面的纯色路径。
  - **`blended_base`（`:376`）要跳过分区**：5×5 邻域平均会把分区色稀释成周围
    自然地形色（实测商业区赭黄被草原绿冲到 `[144,184,108]`，几乎看不出）。
    ① 分区格直取自身分区色，不进平均；② 邻域里的分区格也跳过，
    否则草地会被染成分区色。
  - **海拔色带跳过分区格**：`apply_elevation_band` 调用点（`:1335` 与 `sample_kind` 内一处）
    加 `district_color(cell, …).is_none()` 闸口——场景图是平图，海拔色带会把分区色整体偏移。
  - **`is_boundary`（`:915`）**：`differs` 闭包从吃 `u8` 槽位改成带 `Option<u8>` 分区索引，
    首行加「分区不同 → 必是边界」（含「一个是分区另一个不是」）。调用点同步改。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-world-cell-map` → 全 PASS；
  `cargo clippy -p wv-world-cell-map --all-targets -- -D warnings` → 干净
  （**注意**：测试代码里的 `1 * res`、`1 * 4` 会被 clippy `identity_op` 挑出，要写成 `res` / `4`）
- [ ] **Step 5: Commit** — `git commit -m "wv-world-cell-map: 分区色着色分支与分区边界识别"`

### Task 6: 基底网格命令 + 首刷引导（wv-core + wv-edit + wv-app）

**Files:**
- Modify: `crates/wv-core/src/lib.rs`（`terrain_of` `:313`、`set_terrain` `:318` 之后加 `remove_terrain`）
- Create: `crates/wv-edit/src/command/scene.rs`（`CreateSceneBaseGridCommand`）
- Modify: `crates/wv-edit/src/command.rs`（`mod` 声明在 `:19-30` → 加 `mod scene;`；
  `pub use` 重导出在 `:31-42` → 加 `pub use scene::*;`；`EditError` 在 `:126` → 加变体）
- Modify: `crates/wv-app/src/edit/editing/stroke.rs`（`commit_stroke_on_release` 在 `:9`；
  `Transaction::new()` `:112`、`tx.push(cmd)` `:113`、`history.commit` `:114`）
- Test: `crates/wv-edit/src/command/scene.rs` + `crates/wv-app/src/edit/editing/tests.rs`

**Interfaces:**
- Consumes: Task 1 `DISTRICT_FIRST`（基底用「未分区」= 21）
- Produces:
  - `wv_core::World::remove_terrain(&mut self, map: &Id) -> Option<TerrainData>`
  - `wv_edit::command::CreateSceneBaseGridCommand::new(map: Id, cell_size: f32) -> Self`
  - `EditError::SceneGridExists`（新增变体）
  - wv-app 侧纯函数
    `fn scene_stroke_commands(level: MapLevel, has_terrain: bool, map: Id, cell_size: f32, stroke: TerrainStroke) -> Vec<Box<dyn EditCommand>>`

- [ ] **Step 1: 写失败测试**（wv-edit，手算期望值）

```rust
#[test]
fn scene_base_grid_fills_flat_unzoned_cells_from_extent() {
    let mut world = World::default();
    world.maps.push(map_node("city_a", None, wv_core::map::MapLevel::City)); // extent 1024×1024
    let cmd = CreateSceneBaseGridCommand::new(Id::new_unchecked("city_a"), 8.0);
    let dirty = cmd.apply(&mut world).unwrap();
    let g = world.terrain_of(&Id::new_unchecked("city_a")).unwrap();
    assert_eq!((g.width, g.height), (128, 128), "1024/8=128");
    assert!(g.cells.iter().all(|c| c.height == 0
        && c.biome == DISTRICT_FIRST
        && c.surface == wv_terrain::SURFACE_LAND));
    assert_eq!(dirty, vec!["maps/city_a/terrain.bin".to_string()]);
    cmd.revert(&mut world).unwrap();
    assert!(world.terrain_of(&Id::new_unchecked("city_a")).is_none(), "revert 必须移除本次网格");
}
```

wv-app 侧（纯函数，不起 Bevy）：

```rust
#[test]
fn first_paint_on_scene_map_prepends_base_grid_in_one_transaction() {
    let cmds = scene_stroke_commands(MapLevel::Interior, false, Id::new_unchecked("dungeon_a"), 8.0, test_stroke());
    assert_eq!(cmds.len(), 2, "引导+首笔两命令，同一事务");
    assert_eq!(cmds[0].label(), "铺设场景基底网格");
    let cmds = scene_stroke_commands(MapLevel::Interior, true, Id::new_unchecked("dungeon_a"), 8.0, test_stroke());
    assert_eq!(cmds.len(), 1, "已有栅格不再引导");
    let cmds = scene_stroke_commands(MapLevel::World, false, Id::new_unchecked("world_main"), 8.0, test_stroke());
    assert_eq!(cmds.len(), 1, "世界图永不引导，维持现状报错路径");
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-edit scene_base_grid` → FAIL
- [ ] **Step 3: 实现**
  - `wv-core/src/lib.rs`：加 `remove_terrain`（`self.terrains.remove(map)`）。
  - `wv-edit/src/command/scene.rs`：`CreateSceneBaseGridCommand { map, cell_size, created: Mutex<bool> }`；
    `apply`：图不存在 → `EditError::TargetMissing`；图 `level == World` → `EditError::Precondition`；
    已有栅格 → `EditError::SceneGridExists`（**新增变体**——注意所有 `match EditError` 的穷尽点要同步）；
    否则按 `extent / cell_size` 逐格铺 `{height:0, surface:SURFACE_LAND, biome:DISTRICT_FIRST}`，
    写入 `world.set_terrain`。`revert` 移除本次写入。
    尺寸兜底：extent 缺省或尺寸非正 → 256×256；cell_size ≤ 0 → 8.0。
  - wv-app `stroke.rs`：抽 `scene_stroke_commands` 纯函数并让 `commit_stroke_on_release` 走
    `Transaction { commands: scene_stroke_commands(...) }` **一次 commit**（引导+首笔 = 一个撤销单位）。
    注意该文件现有早退链：`just_released`(`:27`) → 加载遮罩(`:30`) → 空点列(`:34`) →
    playback(`:38`) → `view.scale < 0.15`(`:43`) → `drill.current()`(`:60`) →
    intent 构造失败(`:71`) → 提交(`:112-114`) → 错误提示(`:132`)。
- [ ] **Step 4: 跑测试确认通过 + 事务级验证** — `cargo test -p wv-edit` / `cargo test -p wv-app --features app` → 全 PASS。
  追加一个走 `History::commit` 的集成测试：city 图无栅格 → 提交 → `terrain_of` 存在且笔画处 biome 正确；
  `history.undo()` **一步** → `terrain_of` 回到 None。
- [ ] **Step 5: Commit** — `git commit -m "wv-edit,wv-app: 场景基底网格命令与首刷同事务引导"`

### Task 7: 分区材料条 UI（先做人类城市词表）

**Files:**
- Modify: `crates/wv-app/src/ui/top_option_bar.rs`（`material_strip` 在 `:239`，
  `for k in 0u8..10` 在 `:245`；`egui::Panel::top("wv_top_option_bar")` 在 `:105`）
- Modify: `crates/wv-app/src/ui/shell.rs`（`TERRAIN_KEYS` 在 `:19`（9 项）；
  `terrain_name_at` 在 `:32`；`terrain_swatch` 在 `:40`；`brush_target` 在 `:65`）
- Test: `crates/wv-app/src/ui/shell.rs` `#[cfg(test)]`

**Interfaces:**
- Consumes: Task 1 `DISTRICT_NAMES_ZH` / `district_index`；Task 3/4 两侧色表
- Produces:
  - `shell::MATERIALS: &[u8]`（**唯一权威材料表**，取代散落的 `0u8..9` / `0u8..10` 硬编码）
  - `shell::DISTRICT_MATERIALS: &[u8]`（**本任务只放人类城市词表**：未分区 + 贫民/富人/住宅/商业/工业）
  - `shell::terrain_name_at(loc, idx)` 对分区码返回 `DISTRICT_NAMES_ZH`（中文，不走 i18n）
  - `shell::terrain_swatch(kind)` 对分区码返回分区色
  - `shell::brush_target(kind)` 对分区码返回 `PaintTarget { surface: LAND, biome: kind }`

- [ ] **Step 1: 写失败测试**

```rust
/// 现状 bug：top_option_bar 画 10 块但 TERRAIN_KEYS 只有 9 项
/// → 第 10 块 tooltip="?" 且 brush_target 落 WATER（涂出海）。
#[test]
fn materials_table_has_no_zombie_swatch() {
    assert_eq!(MATERIALS.len(), 9, "退役道路档后材料表应是 9 项");
    assert!(!MATERIALS.contains(&9), "kind 9（道路）已退役，不得出现在材料表");
    for &k in MATERIALS {
        assert_ne!(terrain_name_at(Locale::ZhHans, k as usize), "?", "材料 {k} 无显示名");
    }
}

/// 本任务只做人类城市词表：未分区 + 贫民/富人/住宅/商业/工业（6 块）。
#[test]
fn district_materials_cover_city_vocabulary() {
    assert_eq!(DISTRICT_MATERIALS.len(), 6);
    assert!(DISTRICT_MATERIALS.iter().all(|k| wv_terrain::biome::is_district(*k)));
    assert_eq!(terrain_name_at(Locale::ZhHans, 30), "房间"); // 词表可查全 12 个
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-app --features app materials_table` → FAIL
- [ ] **Step 3: 实现**
  - **顺手修僵尸砖**：`top_option_bar.rs:245` 的 `for k in 0u8..10` 改为遍历 `MATERIALS`（9 项）
    ——现状第 10 块 tooltip 是 "?"、点了涂海（`shell.rs` 的 `brush_target` 对 9 落 `_ => WATER`），
    是 ADR-0070 D1 退役道路色板时漏改循环上界留下的。**这是既有 bug，M1 一并修。**
  - **分区条另起一块**：`material_strip` 是 `exact_size(BAR_H)` 的单行横条
    （9×20=180px 已占位；再加 6 块 = 300px，1280 窗口会溢出）。
    做法：新增 `egui::Panel::top("wv_top_option_bar_district")`（独立第二行），
    **只在「Paint 工具激活 ∧ 当前图是场景图」时显示**——门控语义天然落在面板出不出来上。
    本任务 6 块单行 6×20=120px，宽裕。
  - **判「当前图是场景图」**：`drill.current()` 取 id → `projection.world` 取 node →
    `level == City || level == Interior`（判定按「图级 ≠ World」，不按语义——设计文档 §5.2）。
  - `terrain_name_at` / `terrain_swatch` / `brush_target` 三处都要认分区码：
    名字走 `DISTRICT_NAMES_ZH`（中文，不走 i18n——与 CellMap 侧 `biome_name()` 现状一致，
    省 40 行 i18n，代价是分区名无英文版）；色走分区色表；
    `brush_target` 返回 `PaintTarget { surface: SURFACE_LAND, biome: kind }`。
  - **地下城/森林词表留到后续**：12 个码位与名字已登记（Task 1），
    `DISTRICT_MATERIALS` 本任务只列人类城市的 6 个；扩展只需改这一个常量。
- [ ] **Step 4: 跑测试确认通过 + 实机验收** — `cargo test -p wv-app --features app` → PASS；
  `cargo run --features app` 手动验收：进 Interior 图 → 顶栏下方出现分区条 6 块 →
  选「贫民区」刷一块 → 格子变色且**地形高度不变** → 撤销 → 恢复。
- [ ] **Step 5: Commit** — `git commit -m "wv-app: 分区材料条与材料表单一化（顺修僵尸瓷砖）"`

### Task 8: 新建地图对话框——非 World 不预填海洋

**Files:**
- Modify: `crates/wv-app/src/ui/new_map_dialog.rs`（`NewMapDialog` 在 `:81`；
  `create_map` 在 `:449`；`Extent::sized` 调用在 `:483`；预填栅格在 `:495-499`；
  `AddMapCommand::new` 调用在 `:502`）
- Test: `crates/wv-app/src/ui/new_map_dialog.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: 对话框行为——`level == World`：**现状不变**（默认 Ocean 预填栅格、
  `base: Some(Ocean)`）；`level != World`：`AddMapCommand::new(node, None)`（**不预填栅格**）、
  `node.base = None`。
  （`scene_type` 字段**不在 M1 范围**——见下。）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn non_world_dialog_omits_terrain_prefill() {
    let add = only_add_map(&build_dialog_commands(dialog_fixture(MapLevel::Interior)));
    assert!(add.terrain.is_none(), "非 World 不得预填栅格");
    assert!(add.node.base.is_none());
}

#[test]
fn world_dialog_keeps_ocean_prefill_unchanged() {
    let add = only_add_map(&build_dialog_commands(dialog_fixture(MapLevel::World)));
    let t = add.terrain.as_ref().unwrap();
    assert!(t.cells.iter().all(|c| c.surface == SURFACE_WATER && c.height == -3500),
        "默认海洋裁定不得回归");
    assert_eq!(add.node.base, Some(MapBase::Ocean));
}
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test -p wv-app --features app dialog_omits_terrain` → FAIL
- [ ] **Step 3: 实现** — 组装段按 `dialog.level` 分流：World 走现状；非 World 传 `None` 栅格 + `base: None`。
  注意 `MapBase::Ocean.cell()` 现在是 `{height:-3500, surface:0, biome:14}`（biome 由 0 变 14，
  2026-10-27 咸水 B 案），断言按 `surface` 写，不要写 `kind`。
- [ ] **Step 4: 跑测试确认通过** — `cargo test -p wv-app --features app` → PASS
- [ ] **Step 5: Commit** — `git commit -m "wv-app: 新建地图非 World 级不预填海洋"`

### Task 9: 收口——术语登记 + 全门禁 + 烟测

**Files:**
- Modify: `GLOSSARY.md`（605 行；「分区/District」在 `:391`，「场景编辑器」在 `:389`）
- No 代码改动

> **⚠️ 术语现状**：`GLOSSARY.md:388-391` 已有「场景/Scene」「场景编辑器/Scene Editor」
> 「场景类型/Scene Type」「分区/District」四条（2026-10-22 登记）。**本任务只补 1 条，
> 不重复登记。** 另：「分区」一词已三义（`:326` 自动分区 / `:391` District /
> `:542` partition 势力图），新词条务必带限定词。
> **只改仓库根 `GLOSSARY.md`**——`docs/glossary.md` 是过期快照（454 行），不动。

- [ ] **Step 1: 术语补录（只 1 条）** — 「**场景分区带**」（District Band）——biome 通道的
  21..=32 保留带，承载场景分区码（未分区/贫民区/富人区/普通住宅/商业区/工业区/农田/
  密林/空地/房间/走廊/大殿）；载体是 `TerrainGrid` 的 **biome 通道**（不是 kind 通道——
  kind 已于 2026-10-18 退役为三通道 height/surface/biome）；**分区不属任何群系族**
  （`family_of` 返 None），主题覆盖物规则不误触发；**分区格不参与水彩晕开**
  （5×5 邻域平均会把分区色冲淡）。
  同时**订正 `:391`「分区/District」词条**里「走每图一张 `TerrainGrid` 的 kind 通道；
  kind 编号须 > 1（水域判定 `kind <= 1` 是唯一入口）」这句——kind 通道已退役，
  改为 biome 通道口径。按 AGENTS.md §6.3 **追加说明而非静默改写**。
- [ ] **Step 2: 全门禁**（逐条跑，全绿才算过）
  ```bash
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all -- --check
  cargo test -p wv-app --features app
  cargo clippy -p wv-app --all-targets --features app -- -D warnings
  cargo tree -p wv-core | grep -i bevy                    # 应输出空
  cargo tree -p wv-terrain | grep -i bevy                 # 应输出空
  cargo tree -p wv-render | grep -E 'wv-edit|wv-app'      # 应输出空
  bash scripts/smoke-gui.sh                                # 5s 存活 + 日志无 panic/B000x
  ```
- [ ] **Step 3: 手动验收清单**（WV_SHOT 取图按 memory 配方）：新建 Interior 图 → 双击钻入 →
  首刷自动出基底色 → 刷贫民区/商业区 → 相邻分区交界有边界 → 一步撤销连格带笔全退 →
  重画 → 保存重开工程，色块仍在 → 切 CellMap 视图看分区固定色。
- [ ] **Step 4: Commit** — `git add GLOSSARY.md && git commit -m "docs: 场景分区带术语登记与分区词条订正"`

## M1 明确不做（留给后续里程碑）

- **`scene_type` 字段**（`MapNode.scene_type`）：设计文档裁定要做，但**加字段的返工量是
  88 处穷举式 `MapNode` 字面量**（跨 6 crate、45 文件）。**M1 改用「面板手选词表」**：
  分区条按场景图类型给对应词表（本任务只做人类城市 6 块）。
  待返工可控时再落字段（届时走 `World.scene_types: BTreeMap<Id, SceneType>`
  + `maps/<id>/scene.yaml` 的 per-map 附属文件范式，**避开 88 处返工**——
  先例：`MeasurementOverlay`）。
- 生成器（M2）、标记点（M3）、道路档位（M4）、存在时间段（M5）
- 分区**边界描边矢量层**（M1 只有色块 + 像素台阶边界）
- 分区**跨图继承**（场景图没自己的 `terrain.bin` → 笔刷报「地图 X 无地形网格」，既有契约）
- GPU 场路径（`view.field_render = true`）下的分区精确色（`biome_raw_approx` 会把分区显示成绿族）
- 分区名的 i18n（英文版）
