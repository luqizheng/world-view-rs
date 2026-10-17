# 连续缩放 LOD · 设计计划 v0.1

> **状态**：设计 spec（本文）+ 可执行计划已就绪——任务级实施计划见 `docs/superpowers/plans/2026-10-17-continuous-lod.md`（Task 1–6，TDD 步骤；其中 M1a 与 M2a 合并为 Task 1 连续容差）。M1–M4 **全部未动代码**（2026-10-17 用户裁定：全入计划，另开 session 统一执行）。
> **来源**：2026-10-17 两轮讨论：① 调色板切换卡顿（497ms→90ms 优化已落地，见 git log / memory「palette-switch-perf」）；② 用户提出「zoom-out 到一定程度不需要那么多细节」，问最优解，收敛为「连续 LOD 管线」四步。
> **裁决流程**：M 组按序执行（M2 依赖 M1，M3/M4 相互独立）。标「待裁 → 缺省」的参数按缺省建议落地，执行 session 如有异议再问用户。
> **关联**：ADR-0008（Google 壳、亚像素渲染）、ADR-0009（分级缩放 LodTier）、ADR-0012（等值线矢量平涂——**本计划的精神边界**）、ADR-0013（图层帧）、ADR-0021（渲染配方）、ADR-0023（主题包）；`docs/MAP_LAYER_RENDERING_SPEC_v0.1.md`；`docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md` §2.4（派生管线）。

## 0. 背景与实测基线

**问题**：世界图 zoom-out 时细节过多（等值线最密、mesh 最大、性能最差），且各层 LOD 切换是硬跳变。

**口径先行**（本计划全篇的事实基础）：`ViewState.scale` 是**世界单位 / 屏幕像素**，
**大 = 远（zoom-out），小 = 近（zoom-in）**。三处独立证据：
1. `terrain_chunks.rs` 视口公式：`视口半宽（世界单位）= window.width / 2 × scale`；
2. `state.rs`：`LOD_NATION_SCALE = 3.0`（Nation = 世界图 = 最远档，阈值最大）；
3. `streets.rs`：`world_thickness = stroke_px × view_scale`（恒定屏幕线宽 = 乘 scale）。

**2026-10-17 调色板切换实测**（examples/layer-showcase，1024² 格、res=4、4096² 纹理，release）：

| 热点 | 优化前 | 已落地优化后 | 本计划涉及 |
|---|---|---|---|
| 像素合成 `image_from_terrain` | 317ms | 31–34ms（行带并行，已合入工作区） | M4（后台化，彻底离主线程） |
| isoline 几何全量重算 | ~75ms | 切调色板帧 0（纯换色快路径，已合入工作区） | **M1 / M2（本计划主对象）** |
| chunk 克隆+重贴 | ~105ms | ~56ms | （不在本计划；另案：按 dirty 行带克隆） |

**已确认的反装缺陷**（`isoline_overlay.rs` `lod_bucket` 的 2026-10-15 侧记，当时留待拍板——
本计划 M1 即该拍板的落地）：远观（scale≥0.5）拿**最细**容差 0.125，近观反而拿最粗 0.5。
用户 2026-10-17 已拍板修正方向。

## 1. 里程碑总览

| # | 内容 | 规模 | 依赖 | 独立收益 |
|---|---|---|---|---|
| M1 | 等值线 LOD 方向修正 + Nation 档只画海岸线 | ~30 行 + 测试 | 无 | 远观「细节过多」观感直接解决 |
| M2 | 等值线提取缓存 + DP 容差连续化 | ~80 行 | M1 | 缩放全程无档位跳变、跨档重算变毫秒级 |
| M3 | 矢量层（建筑/POI）淡入淡出替代硬剔除 | ~60 行 | 无 | zoom-out 无「集体消失」感 |
| M4 | 全量重算后台化 + 交叉淡化 | ~120 行 | 无（建议最后做） | 换调色板/换模式/加载工程全程无长帧 |

---

## 2. M1 · 等值线 LOD 方向修正 + Nation 档海岸线-only

### 2.1 现状

`crates/wv-render/src/terrain_view/isoline_overlay.rs`：

```rust
// ⚠️ 侧记承认方向反装，远观拿最细容差（错）
fn lod_bucket(scale: f32) -> u8 {
    if scale >= 0.5 { 0 }      // → dp_tol 0.125（最细）—— 远观，错
    else if scale >= 0.25 { 1 } // → 0.25
    else { 2 }                  // → 0.5（最粗）—— 近观，错
}
fn dp_tol_for(scale: f32) -> f32 {
    [0.125, 0.25, 0.5][lod_bucket(scale) as usize]
}
```

另外：类型边界（草/林/山 kind 场）在**所有**缩放档都提取并描边——世界图尺度上读不出来，纯浪费。

### 2.2 改法

**a) 方向翻转**（阈值不动，只换容差数组语义）：

```rust
/// DP 容差的 LOD 分档。2026-10-17 用户拍板修正方向：scale 大 = 远（§0 口径），
/// 远观拿粗容差（点少线简），近观拿细容差（放大细节浮现）。
/// 原 2026-10-15 侧记的「方向反装」自此关闭，留痕见 docs/CONTINUOUS_LOD_PLAN_v0.1.md M1。
fn lod_bucket(scale: f32) -> u8 {
    if scale >= 0.5 { 0 }      // 远 → 粗
    else if scale >= 0.25 { 1 }
    else { 2 }                  // 近 → 细
}
fn dp_tol_for(scale: f32) -> f32 {
    [0.5, 0.25, 0.125][lod_bucket(scale) as usize]
}
```

**b) Nation 档跳过类型边界**：与 POI/建筑的 Nation 剔除同口径（`state.rs` `LOD_NATION_SCALE = 3.0`）：

```rust
// 远观 Nation 档只画海岸线：类型边界在世界图尺度读不出来——
// 与 POI（vector_layers::poi_kept_at_tier）/ 建筑（buildings）的 Nation 剔除同口径。
let nation = view.scale >= crate::state::LOD_NATION_SCALE;
let kinds = (!nation).then(|| kind_segments(w, h, |x, y| cell(x, y).kind));
// ...提取与装配循环里 kinds 为 None 时整段跳过（连 kind_half 也不必算）。
```

**c) `IsolineKey` 必须加 `nation: bool` 字段**（关键细节，漏掉必出 bug）：
Nation 阈值 3.0 落在 lod bucket 0（scale≥0.5）**内部**——跨 3.0 时 lod 档不变，
key 若不吸收 nation 位，几何不会重建，类型边界残留/缺失。构造 key 时填入；
`despawn()` 清 key 的路径无需变（key 整体置 None）。

### 2.3 测试更新

- `dp_tolerance_buckets_match_plan_q8`：锚点全部反转——`dp_tol_for(1.0)=0.5`、
  `dp_tol_for(0.3)=0.25`、`dp_tol_for(0.1)=0.125`、`dp_tol_for(0.5)=0.5`（边界归远档）、
  `dp_tol_for(0.25)=0.25`。测试名与文档注释同步改为「远粗近细」语义。
- `isoline_key_covers_structural_inputs_only`：变体数组加一条 `nation: true` vs `nation: false`。
- 新增：`nation_tier_skips_kind_boundaries` 的纯断言可不必（管线内联在 system 里），
  key 变体测试已覆盖失效路径；kind 跳过本身靠人工验收。

### 2.4 验收

- `cargo test -p wv-render --features app` 全绿；
- `bash scripts/smoke-gui.sh` PASS（渲染系统改动必跑，AGENTS §7.1）；
- 人工：打开 examples/layer-showcase → 远观（世界图整幅）：线明显变简、类型边界消失；
  逐步放大：细节逐渐出现；深缩：与改动前观感一致（近观档本就该细）。

---

## 3. M2 · 等值线提取缓存 + DP 容差连续化

### 3.1 现状与思路

派生管线五段：`height_segments / kind_segments（提取）→ assemble_chains（装配）→ chaikin（平滑）→ simplify_dp（简化）→ build_stroke_mesh（网格化）`。
其中**前四段的前三段与视角无关**（只依赖地形数据 + treatment 几何位）；只有 DP 容差随缩放变。
现 `sync_isoline_overlay` 每次重算全链；2026-10-17 的 `data_hash` 快路径只救「纯换色」一种场景。

**分离**：把「提取 + 装配 + Chaikin 后的链」缓存进 `IsolineOverlay`（按 `data_hash + map + treatment几何位 + nation` 失效——这些字段 M1 后 key 里都有），缩放变化时只对缓存链重跑 DP + 网格化。DP 在平滑链上是毫秒级 → 跨档重算从 ~75ms 变毫秒级。

### 3.2 连续容差（待裁 → 缺省）

三档跳变改为连续函数。缺省公式（在原三锚点上严格插值，便于回归锚定）：

```rust
/// 连续 DP 容差：scale∈[0.25, 1.0] 线性于 scale；两端钳位。
/// 锚点：0.25→0.125（近·细）、0.5→0.25、≥1.0→0.5（远·粗）。
fn dp_tol_continuous(scale: f32) -> f32 {
    (0.5 * scale).clamp(0.125, 0.5)
}
```

**防每帧重算**：scale 连续变化不能每帧跑 DP + 建网格。缺省：把容差量化到
1/64 格（`(tol * 64.0).floor() / 64.0`），量化后的 tol 进 `IsolineKey`（替换现有 `lod: u8`
字段）。量化步长 = 观感上不可分辨的容差变化；参考 `atmosphere.rs` `scale_bucket`
量化到 1/8 octaves 的同类手法。

### 3.3 结构

```rust
pub struct IsolineOverlay {
    // ...现有字段...
    /// 尺度无关的中间产物：提取+装配+Chaikin 后的链（海岸 + 类型边界分组存）。
    /// 失效条件 = data_hash / map / treatment 几何位 / nation 任一变化。
    chains: Option<CachedChains>,
}
struct CachedChains {
    coast_smoothed: Vec<(Vec<Pt>, bool)>,   // 海岸链（含晕/河中线的同一来源）
    kinds_smoothed: Vec<(Vec<Pt>, bool)>,   // 类型边界链（Nation 档可为空）
    key_inputs: IsolineKey,                  // 生成时的键（不含 tol 量化位）
}
```

注意：`empty_settled` 稳态语义保留（无几何也缓存空链，指纹相同不重提取）。

### 3.4 验收

- 缩放全程（0.1 → 5.0 慢扫）无档位跳变感（对比 M1 后的三档跳变）；
- WV_PERF 探针法（§7 附录）复测：慢扫期间单帧 ≤ 30ms；
- `dp_tol_continuous` 锚点测试（0.25/0.5/1.0/4.0/0.1 五点手算断言）；
- 既有 `full_derivation_pipeline_yields_nonempty_mesh` 端到端测试改造后仍绿。

---

## 4. M3 · 矢量层淡入淡出替代硬剔除

### 4.1 现状

| 层 | 硬切换点 | 行为 |
|---|---|---|
| 2.5D 建筑 | `buildings.rs:209` `tier == LodTier::Nation` | 整层不画 |
| POI | `vector_layers.rs:452` `tier != LodTier::Nation \|\| dot.is_city` | 非城市点剔除 |
| 街道 | `streets.rs`（`lod_tier` 消费点） | 同体系 |

zoom-out 跨阈值（1.0 / 3.0）时整层「集体消失/出现」。

### 4.2 改法（待裁 → 缺省）

alpha 随 scale 插值，阈值只作淡出曲线**中点**：

```rust
/// 淡出因子：scale ≥ hi 完全显示，≤ lo 完全透明，之间 smoothstep。
/// 缺省：lo = 阈值 × 0.75，hi = 阈值 × 1.125（淡出带 ±50% 于阈值两侧，
/// 即约半个缩放倍程内完成过渡；观感不适再调）。
fn lod_fade(scale: f32, threshold: f32) -> f32 {
    let (lo, hi) = (threshold * 0.75, threshold * 1.125);
    let t = ((scale - lo) / (hi - lo)).clamp(0.0, 1.0);
    smoothstep(t) // 3t²−2t³
}
```

- 建筑：`ColorMaterial.color` 的 alpha 乘 fade（材质按实体/按层共享的现状执行时确认，
  逐实体系则写入各自材质）。
- POI：同样乘 alpha；`is_city` 的点不淡（城市点 Nation 档保留是既有语义）。
- **不绕过 RenderRecipe**（ADR-0021）：pass 级开关仍在 recipe，fade 只是 pass 内的
  视觉参数；`recipe.enabled()` 判定保持原样。

### 4.3 验收

- zoom-out 时建筑/POI 渐隐，无集体消失瞬间；
- `lod_fade` 锚点测试（阈值处 ≈ 中点值、两端钳 0/1）；
- smoke-gui PASS。

---

## 5. M4 · 全量重算后台化 + 交叉淡化

### 5.1 现状

`rebuild_texture_on_mode_change`（换调色板/换模式/soften）在 Update **同步**跑
`image_from_terrain`（行带并行后仍 31–34ms 占主线程）+ 同帧 `sync_terrain_chunks`
全图克隆 + 重贴（~56ms）。切一次 ≈ 一帧 90ms 长帧。

### 5.2 改法

**a) 后台计算**（`bevy::tasks` 已随 bevy 在依赖树，零新依赖）：

```rust
/// 挂起中的整图合成任务。切调色板/模式时创建；完成帧在主线程落地。
#[derive(Resource, Default)]
struct PendingTerrainSynth {
    task: Option<bevy::tasks::Task<(Vec<u8>, u32)>>, // (pixels, res)
    // 落地所需上下文（mode/soften/palette/map 的快照，防完成后世界已变）
    ctx: Option<SynthCtx>,
}
```

- 触发处（rebuild 的条件命中段）不再同步调 `image_from_terrain`，改为
  `AsyncComputeTaskPool::get().spawn(async move { … })`——**闭包内只做纯计算**
  （clone 所需的 `TerrainData` + 参数进 task），禁触碰任何 ECS / `Assets`；
- Update 新增 poll 系统：task 完成帧在**主线程**做 `replace_terrain_texture` +
  `chunks_dirty` 置位（`Assets<Image>` 非线程安全，资产写入必须主线程——2026-10-17
  调研结论）；
- 防重入：新请求到来时 drop 旧 Task（结果作废），用 `ctx` 快照校验完成后世界未变
  （map id + data_hash 相同才落地，否则丢弃重排）。

**b) 交叉淡化**：机制已存在——`rebuild.rs::spawn_basemap_fadeout`（旧底图精灵 0.4s 淡出，
`LEVEL_FADE_SECS`）。后台化后把「旧画面保留至新纹理落地」接上：完成帧换纹理 + spawn
淡出精灵（复用现有函数，等价于换图路径的既有观感）。**注意**：现状换调色板不 spawn
淡出（只有换图 spawn）；后台化期间旧调色板画面天然保留在屏上，落地帧是否再叠 0.4s
淡出精灵 → 待裁 → 缺省**不叠**（旧→新色板瞬时切换，与现状一致；叠加会双保险但拖慢感知）。

**c) 触发场景统一**：palette / soften / mode / 工程加载后的首建全走同一队列；
笔刷 `flush_terrain_texture` 的增量 patch **保持同步**（dirty 矩形小，几十格毫秒级，
不值得异步）。

### 5.3 验收

- WV_PERF 探针法：切调色板全程无 > 20ms 帧（触发帧只创建 task，落地帧只写资产）；
- 快速连点切换不崩、不出现「旧调色板闪回」；
- smoke-gui + `cargo app-test` 全绿；
- 视觉：切换期间旧画面持续显示（无白屏/无闪烁），落地帧换新。

### 5.4 风险

- App 退出时未完成 task：bevy 任务池随 runtime 回收，无泄漏路径，但**执行 session
  须验证**（关窗时 poll 系统不再跑，task 被 drop——确认无 panic）；
- 与 M2 的交互：isoline 的 `chunks_dirty` 消费时序不变（落地帧才置位），无冲突。

---

## 6. 被否决的备选（留痕）

| 备选 | 否决理由 |
|---|---|
| **SDF 距离场**（海岸线烧进距离场纹理，shader 端按屏幕密度采样描边/晕） | 渲染质量上最优（无限缩放零跳变），但违反 ADR-0012 立项精神：等值线是**几何对象**（可挂动画、可点选、可挂样式），SDF 把线烧死进像素，图层化编辑与未来海岸线交互失去落点 |
| **按缩放重建纹理**（远观合成低分辨率底图） | GPU mip + `select_mip` 已是硬件级连续 LOD；合成成本与 zoom 无关（数据决定），重建纹理是重复劳动 |
| **提取降采样**（远观用 1/4 高度场做 marching squares） | M2 落地后跨档重算已毫秒级，收益不再显著；且不同密度曲线形状跳变明显，观感劣化 |

## 7. 执行批次建议与门禁

**批次**（用户另有其他计划将合并执行，供排期参考）：
- M1：独立小改动，可随任意批次走；
- M2：独立，建议单独一批（动 `IsolineOverlay` 结构 + key 语义）；
- M3：纯视觉参数，适合与其他 UI 计划合并；
- M4：基建级（任务池 + 生命周期），单独一批，smoke-gui 必跑。

**每步门禁**（AGENTS §7.1 全套）：
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test -p wv-app --features app        # cargo app-test
cargo clippy -p wv-app --all-targets --features app -- -D warnings   # cargo app-clippy
bash scripts/smoke-gui.sh
```

**测量附录（WV_PERF 探针法，量完即删）**：
1. 热点处临时 `let t0 = Instant::now(); … if std::env::var("WV_PERF").is_ok() { eprintln!("[perf] …", t0.elapsed()…) }`；
2. 触发探针：app_bootstrap 临时挂一个按 `time.elapsed_secs()` 定时翻转 `view.map_palette` 的系统（3s/4.5s 各一次，6s `std::process::exit(0)`）；
3. `WV_PERF=1 ./target/release/wv.exe gui -m examples/layer-showcase 2>perf.log`；
4. 量完删净（rebuild.rs / terrain_chunks.rs / isoline_overlay.rs / app_bootstrap.rs 四处）。

## 8. 变更检查清单（AGENTS §10 映射）

- 触及原则：P5（图层化）——M3 淡出不改图层开关语义，仅视觉参数；P7 无涉（无时间语义）。
- 依赖方向：全部改动在 `wv-render`（+M4 poll 系统注册在 `wv-app/app_bootstrap`）；零新第三方依赖（`bevy::tasks` 随 bevy 已在树）；`wv-core`/`wv-edit` 零改动。
- 术语：`GLOSSARY.md` 登记「连续 LOD」「提取缓存」（2026-10-17 已登记）。

---

## 9. 落地实测记录（2026-10-17）

**环境**：release 构建（`cargo build --release -p wv-app --features app`），
Windows 10.0.26300 / 本机，`examples/layer-showcase`（1.5 MB，世界图 1024²）。
探针法按 §7 附录（`WV_PERF=1`，3.2s / 4.4s 各翻一次 palette；1.2–2.8s 对数慢扫
scale 0.1→5.0；6s 退出）。探针已删净（`grep -rn "WV_PERF\|palette_flip_probe" crates/` 为空）。

| 场景 | 基线（本计划之前） | 本次实测 | 判据 | 结果 |
|---|---|---|---|---|
| 切 palette 触发帧 | 同步整图 ~90ms（含 synth 31–34ms） | **建 task 0.7–0.9ms** | 触发帧 ≤ 20ms | ✅ |
| 切 palette 落地 | — | `synth land` 6.5–6.6ms（主线程写资产）；**整帧 153 / 156ms** | 全程 ≤ 20ms | ❌ |
| 慢扫缩放（提取缓存） | 三档重算，每档 ~75ms | DP+网格 **10–12ms**（`chains=false`）；整帧典型 17–25ms | 单帧 ≤ 30ms | ⚠️ 峰值 41ms |
| 缩放的提取重建 | 每次全链 | 只在结构变化时 4.8ms（`chains=true`） | — | ✅ |

**结论**：

- M1/M2/M3 达成：容差连续化 + 链缓存把「缩放重算」从 ~75ms 降到 10–12ms；
  Nation 档只画海岸线；淡入淡出替代硬剔除（观感部分需人工终验）。
- M4 的**合成**部分达成：触发帧 0.9ms、落地 6.6ms；但落地帧整帧仍是 **153ms**——
  剩余热点不在合成，而在 `sync_terrain_chunks` 消费 `chunks_dirty` 时的**全量
  重贴 + 67MB 级克隆**（与 2026-10-17 调色板性能记忆一致）。「无 > 20ms 帧」判据
  因此**未满足**，需要下一个独立任务（chunk 增量/GPU 侧重贴），不在本计划范围。
- 慢扫的 41ms 峰值同样来自 chunk LOD 切换的重建，而非 isoline（后者 10–12ms）；
  「≤ 30ms」判据**未完全满足**，记录待裁。
- 人工观感验收（远观类型边界消失、淡出无集体消失）仍待用户实机确认。
