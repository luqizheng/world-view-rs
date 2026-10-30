# 场管线收尾：模式路由 + 分块指纹 + Themed 观感核查 Implementation Plan（v3 · 基于 salvage 重做）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐场管线转正后的三笔欠账——按模式路由（修 Coastline/Event/Faction 错画成 Themed 与 oblique 被吃两个活缺陷）、编辑帧分块指纹（3ms→<0.5ms）、Themed 分支浮雕观感剥离核查，并留核查报告与 ADR。

**Architecture:** 路由判据收在桥接侧单一纯函数 `should_render_field`，场/chunk 两系统同消费；指纹为独立新模块（布局无关，直接复用已验证实现）；Themed 观感走 `WV_STRIP` 探针逐层剥离，调参待用户目验。零数据层改动。

**Tech Stack:** Rust 2021 / Bevy 0.19 / WGSL。无新第三方依赖。

**Spec:** `docs/superpowers/specs/2026-10-28-field-incremental-and-default-on-design.md`（v2；其中 D2 支持集按 7 档 ViewMode 实态修正——无 Schematic 档）

**Salvage（上一轮已验证实现，本轮的复用来源）：** `.superpowers/salvage/field-mode-routing/`
- `0002-wv-render-CPU-Themed-oblique-CPU.patch` —— T2 路由逻辑参考（**勿直接 apply**：锚在已拆分的旧单体 cellmap_bridge.rs 上，逻辑照搬、锚点重打）
- `t3-uncommitted-fingerprint.patch` —— **fingerprint.rs 完整实现（含 import 修正版）的提取来源**
- `fingerprint_full_566.rs` —— ⚠️ 旧版（缺一行 `use wv_core::terrain::{TerrainCellData, TerrainData};`），**不要直接用**；用它则需手工补该 import
- `0001-docs-ADR-0082.patch` —— T1 文档内容参考（ADR 编号必须改 ≥0087，0080–0086 已被占）

## Global Constraints

- **分支纪律（AGENTS §13）**：Task 1 建分支 `feat/field-finalize` + worktree `.worktrees/feat/field-finalize`；构建按现行 AGENTS（2026-10-27 追记）：**不设 CARGO_TARGET_DIR**，`.cargo/config.toml` 的 `target-dir = ".target"` 逐 checkout 隔离。
- **测试环境**：worktree 内不用 `cargo-test.sh`；先 `export TEMP=D:\kudi-projects\worldview-rs\.target\testtmp; export TMP=$TEMP` 再 `cargo test`。
- **分层红线**：改动落 `wv-world-cell-map` + `wv-render` + `wv-app`（debug_probe）；wv-core / wv-terrain / wv-io / wv-edit 零改动；禁依赖 theme crate。
- **双底图红线**：`should_render_field` 是唯一判据，`sync_terrain_chunks`（`cellmap_bridge/chunks.rs:45`）与 `sync_terrain_field`（`cellmap_bridge/field.rs:341`）两处 gate 同函数消费。
- **N1 教训**：重打/重算判据只比内容（指纹），不比变更戳。
- **新结构注意（v3 新增）**：cellmap_bridge 已拆 10 子模块；`field_branch_of(mode, terrain_visible)`（field.rs:276）已有 BiomeRaw/Themed/Flat 三分支——**路由与它正交组合**：路由决定「场 vs CPU」，branch 决定「场内部怎么画」；CPU 留守档整体退场后须复现转正前的 CPU 语义（含 terrain_visible=false 的表现）。
- **P7/红线**：零新增存储字段、零格式变更；`field_render` / `strip_mask` 会话态不落盘。
- **测试命名**：完整英文短句；数值断言附 `got {}` 上下文。
- **每任务收尾**：`cargo test -p <改动 crate>` 绿 + `cargo fmt --all -- --check` 过才 commit；提交信息 `<scope>: <动作>` 中文。渲染相关任务加 `cargo app-test` / `cargo app-clippy`。
- **Shell 纪律**：Bash 工具 cwd 可能被重置回主 checkout——**每条 git/cargo 命令显式 `cd` worktree 或用 `git -C`**；导出路径变量一律正斜杠（反斜杠会在 cwd 造垃圾目录，历史事故两起）。

## Review Focus

1. **路由翻转瞬间的双底图**：支持集档 chunk 全退场、CPU 留守档场 quad 全 teardown，任何残留 = 双重底图。→ T2 `routing_flip_leaves_exactly_one_base_layer`。
2. **oblique 判据与 mode 合取**：`topographic_compression > 0` 只在 Topographic 生效；VectorFlat+compression>0 不得回落 CPU。→ T2 真值表行。
3. **CPU 留守档的转正前语义复现**：Faction/Event/Coastline 路由回 CPU 后，terrain_visible=true/false 两种表现都必须与转正前一致（不能因 Flat 分支的存在而画出「场 Flat 素底」）。→ T2 `cpu_reserved_modes_reproduce_pre_flip_semantics`。
4. **指纹缓存跨图/跨分支污染 + 退化输入**：structure_changed 丢缓存；0×0/1×1/全水不 panic。→ T3。
5. **探针缺省零行为变化**：`WV_STRIP` 未设置 = 完整管线，生产路径逐位不变。→ T4。

---

### Task 1: 分支 + ADR（编号现场取 ≥0087）+ SPEC LOCK 追记 + GLOSSARY

**Files:**
- Create: `docs/adr/ADR-00XX-field-mode-routing-fingerprint.md`（编号 `ls docs/adr/` 现场取，≥0087；内容从 salvage 0001 patch 提取改编——编号替换 + 修订记录补一段「v1 分支因 main 结构漂移放弃、本 ADR 为重做版」）
- Modify: `docs/adr/README.md`（索引追加 + ADR-0053 承接标注）
- Modify: `docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md`（追记编号现场顺延，主 checkout 已有十八；salvage 0001 里的追记内容改编）
- Modify: `GLOSSARY.md`（四词条：场支持集/模式路由/分块指纹/剥离对照——先查主 checkout 是否已有同名词条）

**Interfaces:** 无代码。ADR 编号对后续任务的文档引用生效（统一 `ADR-00XX`）。

- [ ] **Step 1: 建分支与 worktree**（主 checkout：`git branch feat/field-finalize && git worktree add .worktrees/feat/field-finalize feat/field-finalize`）
- [ ] **Step 2-4: ADR 改编落盘 + 索引 + 追记 + GLOSSARY**（内容基准 = salvage 0001 patch 中对应文件，三处改编：编号、修订记录、spec 引用路径核对）
- [ ] **Step 5: Commit** `docs: ADR 场管线模式路由+分块指纹+观感核查立项（重做版）`

### Task 2: should_render_field 路由（修两个活缺陷）

**Files:**
- Modify: `crates/wv-render/src/cellmap_bridge/chunks.rs:45`（gate 替换）
- Modify: `crates/wv-render/src/cellmap_bridge/field.rs:341`（gate 替换）+ 新函数落点（field.rs 或 mod.rs，随既有布局）
- Test: `cellmap_bridge/field.rs` 既有测试模块旁

**Interfaces:**
- Consumes: `ViewState`（field_render/mode/topographic_compression，state.rs 未变）、`ViewMode` 7 档、`field_branch_of`（正交不互斥——路由在 branch 选择之前）。
- Produces:
  - `pub fn should_render_field(view: &ViewState) -> bool` = `view.field_render && supports_field(view.mode) && !oblique_active(view)`
  - `pub fn supports_field(mode: ViewMode) -> bool` = `matches!(m, VectorFlat | GoogleMap | RealTerrain | Topographic)`
  - `fn oblique_active(v: &ViewState) -> bool` = `v.mode == ViewMode::Topographic && v.topographic_compression > 0.0`
- 参考：salvage 0002 patch 的函数体与 5 个测试（逻辑照搬、锚点重打）；refactor 轮已把 `bridge_meters_per_cell` 提 pub(crate)（10090f4），同类可见性口径。

- [ ] **Step 1: 写失败测试**（真值表 5 测试照 salvage，新增 `cpu_reserved_modes_reproduce_pre_flip_semantics`：留守档 + terrain_visible 双态断言走 chunk 路径——以 gate 判据互补性纯函数断言 + 既有系统级统计口径实现）
- [ ] **Step 2: 确认失败 → Step 3: 实现 → Step 4: 验证**（`cargo test -p wv-render --features app` + `cargo app-test` + `cargo app-clippy` + smoke-gui；基底债务照旧注明）
- [ ] **Step 5: Commit** `wv-render: 场管线按模式路由——CPU 留守档不再错画 Themed，oblique 回落 CPU`

### Task 3: 分块指纹（实现直接复用 salvage）

**Files:**
- Create: `crates/wv-world-cell-map/src/terrain_view/field/fingerprint.rs`（**从 `t3-uncommitted-fingerprint.patch` 提取**：`patch -p1 < <salvage>/t3-uncommitted-fingerprint.patch` 后只取 fingerprint.rs 与 field/mod.rs 的 mod 声明；或手工复制 + 确认 import 行在位）
- Modify: `crates/wv-world-cell-map/src/terrain_view/field/pack.rs`（pack_content_hash 文档注记「对账基准」——salvage patch 同款）
- Modify: `crates/wv-render/src/cellmap_bridge/field.rs`（TerrainFieldRender 增 `fingerprint: Option<BlockFingerprint>` + sync_terrain_field 指纹段接线——集成逻辑参考 t3 patch 中对旧单体的 hunk，锚点重打到 field.rs 现位）

**Interfaces:**
- Produces: `FP_BLOCK=256`、`BlockFingerprint::{new_full, update_region, total}`、增量≡全量 property 测试 ×5+（salvage 实现在上一分支已全绿：337+7）。
- 集成纪律：脏矩形与补丁同帧同源（`take_field_dirty()` 一次取用两处消费）；structure_changed 丢缓存；total 进 `FieldKey.terrain_hash` 槽、重打判据一行不动（N1）。

- [ ] **Step 1: 提取 fingerprint.rs 并确认 5+ 测试在位**（`cargo test -p wv-world-cell-map fingerprint` 绿）
- [ ] **Step 2: 桥接集成**（新锚点）+ 集成测试（t3 patch 中 3 条改编）
- [ ] **Step 3: 验证**（cell-map 全绿 + app-test + app-clippy + smoke；编辑帧 perf_timed 观察）
- [ ] **Step 4: Commit** `wv-world-cell-map,wv-render: 编辑帧分块指纹——3ms 全量扫改脏块增量`

### Task 4: WV_STRIP 剥离探针 + Themed 浮雕对比帧

**Files:**
- Modify: `crates/wv-app/src/debug_probe.rs`（`WV_STRIP=<0-31>` → `ViewState.strip_mask: u32`，state.rs 新会话字段默认 0 不落盘）
- Modify: `crates/wv-render/src/cellmap_bridge/field.rs`（strip_mask 写 `FieldParams.pad1.x` 未用槽）+ WGSL Themed 分支位域解码（bit0 色板/bit1 soften/bit2 wash/bit3 veil/bit4 浮雕，清零=退回裸底色，bit4 清零=relief 恒 1.0）
- Test: `strip_mask_zero_is_identity` + `strip_mask_decode_truth_table`（Rust 镜像 fn）

- [ ] **Step 1-3: 失败测试 → 实现 → 绿**（探针缺省零行为变化）
- [ ] **Step 4: 采集对比帧**（jrpg-world map_1 同机位 × mask {0,1,2,4,8,16} × {近观,远观} → `.target/strip-shots/`；结论写任务报告：冲淡源定位 + 调参候选，**不落参**）
- [ ] **Step 5-6: 门禁 + Commit** `wv-render,wv-app: WV_STRIP 剥离探针——Themed 加工链逐层对比帧`

### Task 5: 核查报告 + 全门禁收尾

**Files:**
- Modify: `docs/TERRAIN_FIELD_P0_SPEC_LOCK_v1.0.md`（追记：核查报告——预算表逐项数字 + 支持集定稿 + Themed 观感结论与调参提案）

- [ ] **Step 1: 预算实测**（编辑单帧 ≤8ms@1024²；4096² 合成图 patch 全链 ≤16ms；guangdong VRAM ≤1.5GB；支持集逐档 + 留守档 WV_SHOT 比对）
- [ ] **Step 2: 全门禁**（workspace test / clippy / fmt / app-test / app-clippy / smoke-gui；基底债务照旧注明）
- [ ] **Step 3: 追记落盘 + 手跑清单**（用户目验项：Themed 对比帧挑层、留守档恢复语义确认）
- [ ] **Step 4: Commit** `docs: 场管线转正后核查报告`

---

## Self-Review 记录

- **Spec 覆盖**：D1 顺序=T2→T3→T4→T5；D2 路由=T2（支持集按 7 档修正）；D3 指纹=T3（salvage 复用 + 判据修正已在 salvage 实现中体现）；D4 观感=T4；D5 预算=T5；D6 留痕=T1/T5。无缺。
- **Salvage 复用核对**：fingerprint.rs 从 t3 patch 提取（含 import 修正）；T2 逻辑照搬锚点重打；T1 内容改编换号 ≥0087；fingerprint_full_566.rs 标记勿直用。
- **类型一致**：`should_render_field/supports_field`（T2 定义 T5 复用）；`BlockFingerprint`（T3）；`strip_mask`（T4 单向）。
- **Review Focus 5 条**：1→T2；2→T2；3→T2（v3 新增）；4→T3；5→T4。
