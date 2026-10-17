# Continuous Zoom LOD Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 缩放全程细节连续平滑变化（等值线容差连续 + 矢量层淡入淡出），全量重算移出主线程（后台合成 + 落地校验）。

**Architecture:** 三条正交线全在 wv-render：① 等值线 `isoline_overlay.rs`——DP 容差从三档（方向反装）改连续函数并量化进结构键，提取/装配/Chaikin 链缓存于 `IsolineOverlay` 资源（缩放只重跑 DP+网格化）；② 矢量层 `buildings.rs` / `vector_layers.rs`——Nation 档硬剔除改 `lod_fade` smoothstep 淡出；③ 整图合成 `rebuild.rs`——`image_from_terrain` 移入 `bevy::tasks::AsyncComputeTaskPool`，主线程 poll 落地（`Assets<Image>` 非线程安全，资产写入必须主线程）。

**Tech Stack:** Rust 2021；零新增依赖（`bevy::tasks` 随 bevy 已在树，`std::thread::scope` 已用于行带并行合成）。

**Spec:** `docs/CONTINUOUS_LOD_PLAN_v0.1.md`（背景、实测基线 497ms→90ms、M1–M4 里程碑、被否决备选、待裁参数缺省值）。本计划的任务与 spec 里程碑映射：Task 1 = M1 容差方向修正 **与** M2 连续化合并（连续函数 `(0.5×scale).clamp(0.125, 0.5)` 在全部锚点上即方向修正，三档翻转无独立存在必要）；Task 2 = M1 Nation 档；Task 3 = M2 提取缓存；Task 4 = M3；Task 5+6 = M4。

## Global Constraints

- 全部改动限 `wv-render`（+ Task 5 在 `wv-app/app_bootstrap.rs` 注册 poll 系统）；`wv-core` / `wv-edit` 零改动（AGENTS §2.1）。
- 零新第三方依赖；`wv-render` 对 `wv-edit` / `wv-app` 零反向依赖。
- `view.scale` 口径 = **世界单位/屏幕像素，大 = 远**（spec §0 三证据）。所有阈值判断以此为方向。
- 不绕过 `RenderRecipe`（ADR-0021）：淡出是 pass 内视觉参数，`recipe.enabled()` 判定不动。
- 派生几何永不落盘（D9）：提取缓存只在进程内存。
- 测试名完整英文短句、数值断言附实际值（AGENTS §7.3/§7.4）。
- 每个 Task 收尾跑 `cargo test -p wv-render --features app`；Task 2/5 起加跑 `bash scripts/smoke-gui.sh`（渲染系统改动，AGENTS §7.1）。
- 提交信息 `<scope>: <动作>`。

## Review Focus

1. **跨 Nation 阈值（scale 3.0）key 不失效**：该阈值落在容差钳位区内部，若 key 不吸收 nation 位，类型边界残留/缺失——Task 2 `isoline_key_distinguishes_nation_tier` 钉住。
2. **scale 连续变化每帧重跑 DP+建网格**：容差不量化则缩放时逐帧重算——Task 1 `tol_quantum_changes_only_at_sixty_fourth_steps` 钉住量化档位有限。
3. **后台合成落地时世界已变**（快速连切 palette / 完成前加载工程）：旧像素贴进新世界——Task 5 `ctx_validation_rejects_stale_world` 钉住 data_hash/map 校验丢弃。
4. **App 退出时未完成 task**：关窗帧 poll 系统不再跑，task 被 drop——Task 5 验证步骤含关窗无 panic（smoke-gui + 手动关窗）。
5. **淡出误伤城市 POI**：Nation 档城市点保留是既有语义（`poi_kept_at_tier`）——Task 4 `city_pois_do_not_fade_at_nation_tier` 钉住 is_city 恒 fade=1。

---

### Task 1: 等值线 DP 容差连续化（含方向修正）

**Files:**
- Modify: `crates/wv-render/src/terrain_view/isoline_overlay.rs`（`lod_bucket` / `dp_tol_for` / `IsolineKey.lod` 字段 / 三个测试）

**Interfaces:**
- Consumes: 无（首个任务）。
- Produces:
  - `fn dp_tol_continuous(scale: f32) -> f32`（私有）：`(0.5 * scale).clamp(0.125, 0.5)`——锚点 0.25→0.125（近·细）、0.5→0.25、≥1.0→0.5（远·粗）。
  - `fn tol_quantum(scale: f32) -> u8`（私有）：`((dp_tol_continuous(scale) * 64.0) as u8)`——容差量化到 1/64 格。
  - `IsolineKey` 字段 `lod: u8` → `tol_q: u8`（Task 3 的链缓存失效判定**不含**此字段）。

- [ ] **Step 1: 重写锚点测试（先失败）**

删 `dp_tolerance_buckets_match_plan_q8`（三档语义不存在了），新增两个测试进 `isoline_overlay.rs` 的 `mod tests`：

```rust
/// 连续容差锚点（spec §3.2 手算表）：近细远粗，两端钳位。
#[test]
fn continuous_dp_tolerance_hits_anchor_points() {
    let cases = [
        (0.1_f32, 0.125_f32),  // 深缩（近）→ 钳到最细
        (0.25, 0.125),          // 恰锚点
        (0.5, 0.25),            // 恰锚点
        (1.0, 0.5),             // 恰锚点
        (4.0, 0.5),             // 远观 → 钳到最粗
    ];
    for (scale, want) in cases {
        let got = dp_tol_continuous(scale);
        assert!((got - want).abs() < 1e-6, "scale={scale} 得 {got}，应 {want}");
    }
}

/// 量化档位有限：缩放跨越 1/64 容差步长才变档；远端钳位区同档。
#[test]
fn tol_quantum_changes_only_at_sixty_fourth_steps() {
    assert_eq!(tol_quantum(0.25), 8, "0.125×64");   // 最细档
    assert_eq!(tol_quantum(0.1), 8, "钳位区同档");
    assert_eq!(tol_quantum(1.0), 32, "0.5×64");     // 最粗档
    assert_eq!(tol_quantum(4.0), 32, "钳位区同档");
    assert!(tol_quantum(0.3) < tol_quantum(0.7), "中间值应分档");
}
```

同步改 `isoline_key_covers_structural_inputs_only`：基键与变体把 `lod: N` 全部换成 `tol_q: N`，并加一条 `tol_q: base.tol_q + 1` 变体。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-render --features app continuous_dp_tolerance`
Expected: FAIL——`dp_tol_continuous` 未定义。

- [ ] **Step 3: 实现两个函数并替换消费点**

删 `lod_bucket` / `dp_tol_for`，实现 `dp_tol_continuous` / `tol_quantum`（公式见 Interfaces）。`sync_isoline_overlay` 两处消费点替换：`let tol = dp_tol_for(view.scale)` → `dp_tol_continuous(view.scale)`；key 构造 `lod: lod_bucket(view.scale)` → `tol_q: tol_quantum(view.scale)`。新函数文档注释注明方向口径（scale 大 = 远）与 2026-10-15 侧记的关闭（指向 spec M1）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test -p wv-render --features app`
Expected: 全绿（含既有 `full_derivation_pipeline_yields_nonempty_mesh` 等端到端）。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-render/src/terrain_view/isoline_overlay.rs
git commit -m "wv-render: 等值线 DP 容差连续化（远粗近细，方向修正）"
```

---

### Task 2: Nation 档只画海岸线

**Files:**
- Modify: `crates/wv-render/src/terrain_view/isoline_overlay.rs`

**Interfaces:**
- Consumes: Task 1 的 `IsolineKey`（现含 `tol_q`）。
- Produces: `IsolineKey` 增 `nation: bool` 字段；`sync_isoline_overlay` 内 `let nation: bool = view.scale >= crate::state::LOD_NATION_SCALE;`（Task 3 缓存失效判定包含此位）。

- [ ] **Step 1: 写失败测试**

```rust
/// Nation 位是几何结构输入：跨 scale 3.0 时容差档不变（都在钳位区），
/// key 必须仍判不等——否则类型边界残留/缺失（spec §2.2c）。
#[test]
fn isoline_key_distinguishes_nation_tier() {
    let base = IsolineKey {
        nation: false,
        tol_q: 32,
        map: Some(wv_core::Id::new("m").expect("合法 id")),
        size: 512, res: 4, soften: false,
        treatment: [true, true, true],
    };
    let mut far = base.clone();
    far.nation = true;
    assert_ne!(base, far, "Nation 档几何不同（类型边界不画），key 必须不等");
}
```

`isoline_key_covers_structural_inputs_only` 的基键与全部变体补 `nation: false`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-render --features app isoline_key_distinguishes_nation_tier`
Expected: FAIL——字段不存在。

- [ ] **Step 3: 实现**

`IsolineKey` 加 `nation: bool`；key 构造处填 `nation`；派生段：

```rust
let nation = view.scale >= crate::state::LOD_NATION_SCALE;
let kinds = (!nation).then(|| kind_segments(w, h, |x, y| cell(x, y).kind));
```

类型边界的装配循环包进 `if let Some(kinds) = &kinds`；`kind_half` 的计算同样移入该分支（Nation 档不算）。海岸线段（coast）不受影响。

- [ ] **Step 4: 跑测试 + 冒烟**

Run: `cargo test -p wv-render --features app && bash scripts/smoke-gui.sh`
Expected: 测试全绿；smoke PASS。
人工验收：`WV_PERF=1` 跑 layer-showcase，远观（世界图整幅）类型边界消失、线变简。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-render/src/terrain_view/isoline_overlay.rs
git commit -m "wv-render: 等值线 Nation 档只画海岸线（与 POI/建筑同口径）"
```

---

### Task 3: 等值线提取缓存（缩放只重跑 DP）

**Files:**
- Modify: `crates/wv-render/src/terrain_view/isoline_overlay.rs`

**Interfaces:**
- Consumes: Task 1 `tol_q`、Task 2 `nation`、既有 `terrain_data_hash` / `data_hash` 字段（2026-10-17 已实现）。
- Produces:
  - `type IsolineChain = (Vec<wv_terrain::isoline::Pt>, bool);`（点列 + 闭合位）
  - `#[derive(PartialEq)] struct ChainCacheInputs { map: Option<wv_core::Id>, data_hash: u64, treatment: [bool; 3], nation: bool }`——**不含 tol_q**（链与容差无关）。
  - `struct CachedChains { inputs: ChainCacheInputs, coast_smoothed: Vec<IsolineChain>, kinds_smoothed: Vec<IsolineChain> }`
  - `IsolineOverlay` 增字段 `chains: Option<CachedChains>`；`despawn()` **不**清它（链是数据派生，与实体生命周期无关；只在 inputs 失配时重建）。
  - 纯函数 `fn assemble_strokes(coast: &[IsolineChain], kinds: &[IsolineChain], tol: f32, thin_half: f32, kind_half: f32, treatment: crate::theme::theme_pack::TreatmentFlags) -> (Vec<IsolineStroke>, Vec<IsolineStroke>, Vec<IsolineStroke>)`（strokes / coast_halo / coast_river；`IsolineStroke = (Vec<Pt>, bool, f32)` 半宽三元组）——现内联在 system 里的装配循环抽出。

- [ ] **Step 1: 写失败测试（纯函数段）**

```rust
/// 缓存链复用口径：同一组链 + 同容差 → 装配结果与直接提取装配逐点一致
/// （这里钉行为语义：海岸链产出主线 + 全开 treatment 产晕与河中线）。
#[test]
fn assemble_strokes_from_cached_chains_yields_main_halo_and_river() {
    let coast = vec![(vec![Pt { x: 0.5, y: 0.5 }, Pt { x: 8.5, y: 0.5 }], false)];
    let (strokes, halo, river) = assemble_strokes(
        &coast, &[], 0.125, 0.5, 1.0,
        crate::theme::theme_pack::TreatmentFlags::HAND_DRAWN,
    );
    assert_eq!(strokes.len(), 1, "一条海岸链 = 一条主线，得 {}", strokes.len());
    assert_eq!(halo.len(), 1, "coast_halo 全开 → 每链一晕");
    assert_eq!(river.len(), 1, "river_centerline 全开 → 每链一河中线");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-render --features app assemble_strokes`
Expected: FAIL——函数未定义。

- [ ] **Step 3: 抽纯函数 + 接缓存**

1. 把 system 内「for c in assemble_chains(&coast) { chaikin; dp; push ×4 }」与 kinds 同款循环抽成 `assemble_strokes`（体与现状同逻辑：coast 链 clone 进 strokes/halo/river，kinds 链先 0.30 预抽稀再 chaikin 再 dp，进 strokes）。
2. `CachedChains` 存 **Chaikin 之后、DP 之前** 的链：`sync_isoline_overlay` 里若 `overlay.chains` 的 `inputs` 与当前（map/data_hash/treatment/nation）相等则复用，否则重跑 提取+装配+Chaikin 并写缓存。
3. DP 容差从 `dp_tol_continuous(view.scale)` 每次对缓存链跑（毫秒级）。

- [ ] **Step 4: 跑测试 + 观感**

Run: `cargo test -p wv-render --features app`
Expected: 全绿；`empty_settled` 稳态语义不破（空链也缓存：inputs 相等 + 空链 → 直接 empty 分支）。
人工：慢扫缩放 0.1→5.0，档位跳变感消失（对比 Task 2 后的三档）。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-render/src/terrain_view/isoline_overlay.rs
git commit -m "wv-render: 等值线提取缓存——缩放只重跑 DP 与网格化"
```

---

### Task 4: 建筑/POI 淡入淡出替代硬剔除

**Files:**
- Modify: `crates/wv-render/src/state.rs`（`lod_fade`，与 `lod_tier` 同居）
- Modify: `crates/wv-render/src/buildings.rs`（Nation 硬跳过 → alpha 淡出）
- Modify: `crates/wv-render/src/vector_layers.rs`（非城市 POI 硬剔除 → 淡出）

**Interfaces:**
- Consumes: `state.rs` 既有 `LOD_NATION_SCALE = 3.0`。
- Produces: `pub fn lod_fade(scale: f32, threshold: f32) -> f32`——`lo = threshold × 0.75` 处 0，`hi = threshold × 1.125` 处 1，中间 smoothstep `3t²−2t³`（spec §4.2 缺省）。

- [ ] **Step 1: 写失败测试**

`lod_fade_anchors_at_threshold_band` 写进 `state.rs` 测试模块；`city_pois_do_not_fade_at_nation_tier` 写进 `vector_layers.rs` 测试模块（它消费的 `poi_alpha_multiplier` 是该文件私有函数）：

```rust
/// 淡出带锚点（手算）：lo=2.25（0）、阈值 3.0（t=0.667 → 0.740）、hi=3.375（1）。
#[test]
fn lod_fade_anchors_at_threshold_band() {
    assert_eq!(lod_fade(2.25, 3.0), 0.0, "带下沿全透明");
    assert_eq!(lod_fade(3.375, 3.0), 1.0, "带上沿全显示");
    let mid = lod_fade(3.0, 3.0);
    assert!((mid - 0.740).abs() < 1e-3, "阈值处 smoothstep(2/3)≈0.740，得 {mid}");
}

/// 既有语义不破：城市 POI 在 Nation 档不淡（`poi_kept_at_tier` 的保留位）。
#[test]
fn city_pois_do_not_fade_at_nation_tier() {
    let fade = lod_fade(LOD_NATION_SCALE * 2.0, LOD_NATION_SCALE);
    assert_eq!(poi_alpha_multiplier(true, fade), 1.0, "is_city 恒不淡");
    assert!(poi_alpha_multiplier(false, fade) < 1e-6, "非城市远端淡尽");
}
```

（`fn poi_alpha_multiplier(is_city: bool, fade: f32) -> f32` 放 `vector_layers.rs`，供 spawn/材质 alpha 两处共用。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-render --features app lod_fade`
Expected: FAIL——`lod_fade` 未定义。

- [ ] **Step 3: 实现**

`state.rs` 加 `lod_fade`（公式见 Interfaces）；`vector_layers.rs` 加 `poi_alpha_multiplier` 并在 POI 材质 alpha 处乘它（`fade == 0` 时跳过 spawn，等价原剔除）；`buildings.rs` 把 `tier == LodTier::Nation → 不画` 的分支改为 alpha 乘 `lod_fade(view.scale, LOD_NATION_SCALE)`，fade 为 0 不 spawn。**不动** `recipe.enabled`（ADR-0021 pass 开关原样）。

- [ ] **Step 4: 跑测试 + 冒烟**

Run: `cargo test -p wv-render --features app && bash scripts/smoke-gui.sh`
Expected: 全绿 + PASS；人工：zoom-out 建筑渐隐、无集体消失瞬间。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-render/src/state.rs crates/wv-render/src/buildings.rs crates/wv-render/src/vector_layers.rs
git commit -m "wv-render: 建筑/POI 按 scale 淡入淡出替代 Nation 硬剔除"
```

---

### Task 5: 整图合成后台化 + 落地校验

**Files:**
- Modify: `crates/wv-render/src/terrain_view/cells.rs`（`write_terrain_pixels` 拆出 `pub fn terrain_pixels`）
- Modify: `crates/wv-render/src/terrain_view/rebuild.rs`（触发段改后台 spawn；新增 poll 系统）
- Modify: `crates/wv-app/src/app_bootstrap.rs`（注册 poll 系统，排 `rebuild_texture_on_mode_change` 之后）

**Interfaces:**
- Consumes: `cells.rs` 既有 `write_terrain_pixels`（行带并行）、`TileAtlas: Clone`（已 derive）、`terrain_data_hash`（isoline_overlay.rs，需 `pub(crate)` 可见）。
- Produces:
  - `pub fn terrain_pixels(data: &TerrainData, mode: ViewMode, atlas: Option<&TileAtlas>, soften: bool, palette: &PaletteColors, treatment: TreatmentFlags) -> (Vec<u8>, u32)`（`write_terrain_pixels` 原 bag，改名公开；`image_from_terrain` 改为调它再包 `Image`——行为逐位不变）。
  - `#[derive(Clone)] pub struct SynthCtx { pub mode: ViewMode, pub soften: bool, pub palette: MapPalette, pub map: Option<wv_core::Id>, pub data_hash: u64 }`
  - `pub struct SynthResult { pub pixels: Vec<u8>, pub res: u32, pub ctx: SynthCtx }`
  - `#[derive(Resource, Default)] pub struct PendingTerrainSynth { pub task: Option<bevy::tasks::Task<SynthResult>>, pub ctx: Option<SynthCtx> }`
  - `fn ctx_matches_world(ctx: &SynthCtx, world: &wv_core::World, current: Option<&wv_core::Id>) -> bool`（map 同源 + `terrain_data_hash` 相等）
  - `pub fn land_pending_terrain_synth(...)`（Update 系统，app_bootstrap 注册）

- [ ] **Step 1: 写失败测试（纯函数段）**

```rust
/// 落地校验：完成时刻世界已换（data_hash / 来源图变）→ 拒绝落地，等价丢弃重排。
#[test]
fn ctx_validation_rejects_stale_world() {
    let world = test_world_with_terrain("map_a", 8);
    let ctx = SynthCtx { /* map_a 的指纹 */ };
    assert!(ctx_matches_world(&ctx, &world, Some(&id_a)), "同世界应放行");
    let ctx_b = SynthCtx { /* map_b / 或不同指纹 */ };
    assert!(!ctx_matches_world(&ctx_b, &world, Some(&id_a)), "异世界应拒绝");
}
```

（`test_world_with_terrain` 就地搭 8×8 双格世界；两个 ctx 用真实 `terrain_data_hash` 算出。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p wv-render --features app ctx_validation`
Expected: FAIL——类型未定义。

- [ ] **Step 3: 实现后台合成 + 落地**

1. `cells.rs`：`write_terrain_pixels` → `pub fn terrain_pixels`（签名见 Interfaces），`image_from_terrain` 改调它。
2. `rebuild.rs` 触发段（`rebuild_texture_on_mode_change` 命中重建条件时）：不再同步调 `image_from_terrain`，改为 clone `TerrainData` + `TileAtlas` + 参数进闭包，`bevy::tasks::AsyncComputeTaskPool::get().spawn` 跑 `terrain_pixels`，产 `SynthResult` 存 `PendingTerrainSynth`。防重入：新请求 drop 旧 task。**闭包内零 ECS / 零 Assets 触碰。**
3. `rebuild.rs` 新增 `land_pending_terrain_synth`：task 完成帧在主线程做 `ctx_matches_world` 校验 → `replace_terrain_texture`（由 pixels 建 Image）+ `tex.chunks_dirty = Some([0, 0, size, size])`。校验失败丢弃（世界已变，重建条件会在后续帧重新触发）。
4. `app_bootstrap.rs` 注册 poll 系统，`.after(rebuild_texture_on_mode_change)`。笔刷增量 `flush_terrain_texture` 路径**保持同步**（dirty 矩形小）。
5. 交叉淡化：落地帧不叠 `spawn_basemap_fadeout`（spec §5.2b 缺省——旧 palette 画面在后台期天然保留，落地瞬时切换与现状一致）。

- [ ] **Step 4: 跑测试 + 冒烟 + 关窗验证**

Run: `cargo test -p wv-render --features app && cargo app-test && bash scripts/smoke-gui.sh`
Expected: 全绿 + PASS。手动：启动中直接关窗（未完成 task 被 drop）无 panic；快速连点切 palette 无旧色闪回。

- [ ] **Step 5: Commit**

```bash
git add crates/wv-render/src/terrain_view/cells.rs crates/wv-render/src/terrain_view/rebuild.rs crates/wv-app/src/app_bootstrap.rs
git commit -m "wv-render: 整图合成后台化——主线程只校验落地资产"
```

---

### Task 6: 端到端实测验收 + 记录回填

**Files:**
- Modify: `docs/CONTINUOUS_LOD_PLAN_v0.1.md`（文末新增「§9 落地实测记录」小节）

**Interfaces:**
- Consumes: Task 1–5 全部交付；spec §7 测量附录（WV_PERF 探针法 + 定时翻 palette 探针，量完删净）。

- [ ] **Step 1: 插桩复测**

按 spec §7：临时探针（synthesis 落地帧 / chunks / isoline）+ `palette_flip_probe` 定时翻转，`WV_PERF=1 ./target/release/wv.exe gui -m examples/layer-showcase 2>perf.log`。

- [ ] **Step 2: 验收判据**

- 切 palette 全程无 > 20ms 帧（触发帧只建 task，落地帧只写资产）；
- 慢扫缩放 0.1→5.0 无 > 30ms 帧（提取缓存生效）；
- 三个探针删净（`grep -rn "WV_PERF\|palette_flip_probe" crates/` 为空）。

- [ ] **Step 3: 记录回填 + Commit**

数字（含与基线 497ms / 90ms 的对比表）写入 spec §9；提交：

```bash
git add docs/CONTINUOUS_LOD_PLAN_v0.1.md
git commit -m "docs: 连续 LOD 落地实测记录"
```
