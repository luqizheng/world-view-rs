# 群系视图（nature_overlay）稳态性能 + 关闭即清零 验收单 v3（第 2 轮复签 · 过程审）

- **验收人**：严过关（质量监管）
- **日期**：2026-10-23 · 会话第 3 个验收实例
- **验收对象**：用户本轮真人实测的四条症状 —— ①鼠标移动 40 FPS ②开群系视图 26 FPS
  ③关群系视图后顶点手柄残留 ④粗格边 alpha 叠加（**用户已裁定不修**）
- **AC 编号**：沿用 v1（`2026-10-23-cellmap-zoom-grid-acceptance.md`）的 AC1–AC13 不重排；
  本轮新增 AC14–AC21 接着编号。
- **纪律声明**：整份从头写，未复制 v2 正文。**上轮遗留项**逐条只在 §3 以「当前状态」出现。
- **本轮限制（必须先说）**：本机无显示器、未起窗口、未录屏。所有帧率数字**我一个都没复算**；
  用户实测是我唯一来源，见 §1 第 1–4 行的「未覆盖」。

---

## 0. 我这轮实际跑的命令（可复跑，原文）

```bash
cargo test -p wv-render --features app --test acceptance_nature_overlay     # 本轮新增，9 passed / 2 failed
cargo test -p wv-render --features app --test acceptance_grid_visibility     # 上轮 11 条，11 passed
cargo test -p wv-render --features app --lib                                 # 343 tests，1 failed
cargo test -p wv-render --features app                                       # 被上一条的失败中断
cargo test -p wv-world-cell-map                                              # **整包编译失败**（example）
cargo test -p wv-world-cell-map --lib                                        # 225 tests，221 passed / 4 ignored
grep -rn "WV_PERF" crates --include=*.rs                                     # 工具已接线（app_bootstrap:266/323/452、wv-render/src/perf.rs）
```

> 过程中两次撞到工作树**不能编译**（`world_projection.rs:934` 与 `:975`，同一处
> `CandidateKey.cell` 的 `(i32,i64)` vs `(i32,i32)`），都是主程序在飞改动，现已消失。
> 记在这里是为了说明：本节所有结果取自「最后一次能编译的工作树」，中间态不计。

---

## 1. 采信矩阵（上游结论逐条复算；**事实格与归因格分开**）

| # | 上游说法（谁说的） | ① 我实际执行的命令 | ② 本机原始输出（摘要） | ③ 结论 |
|---|---|---|---|---|
| 1 | **事实**：静止 120 FPS（用户） | 无（无显示器/未起窗口） | — | **未覆盖**。不采信为「我复算过」；作为用户实测证据记录 |
| 2 | **事实**：鼠标移动 → 40 FPS（用户） | 同上 | — | **未覆盖**（同上） |
| 3 | **事实**：zoom in/out ≈110 FPS（用户） | 同上 | — | **未覆盖/采信用户**：上轮 AC2 的「个位数 FPS」按用户实测判**已解决**（我无复算能力，明写不冒充） |
| 4 | **事实**：开群系视图 → 26 FPS（用户） | 同上 | — | **未覆盖**。见下 #5：我只证明「该路径不再每帧重算」，**不等于** 26 FPS 已修 |
| 5 | **归因**：26 FPS 来自 `sync_region_outlines` 每帧全量重算 `segs` + 指纹（team-lead） | 读代码 + `acceptance_nature_overlay` 两个计数器 | 旧码：`segs` 与 `segments_fingerprint` 在无条件分支里（HEAD 版 `region_outline.rs:254-258`）；新码：`SegSourceKey` 命中即跳过（`:318-340`）。实测稳态 60 帧 `Δseg_recomputes = 0`、`Δstroke_rebuilds = 0`（10 区与 200 区两档都 0） | **采信·复现（事实层）**。归因（「这是 26 FPS 的主因」）**未采信**——我没有帧率证据；只证明这条路径的每帧成本已归零 |
| 6 | **事实**：`OutlineKey` 含 `world_thickness = px × view.scale`（team-lead） | 读 HEAD 版 `region_outline.rs:259-264` | `world_thickness: OUTLINE_STROKE_PX * view.scale` 进 `OutlineKey` | **采信·复现**。现码改为 `thickness_bucket: round(log2(scale)×2)`（`:145-157`），**部分修**：见 AC21 |
| 7 | **事实**：关群系视图后手柄残留（用户） | 读旧 `world_projection.rs:681-704` | 旧注释与 gate：「选中区域的方格描边 + 顶点手柄……**与 nature_overlay 无关**」，门控只有 `selection + Topographic` | **采信·复现**（这就是症状 3 的成因，代码级证据）。现码 `region_handles_visible(mode, nature_overlay)`（`:700-702`）+ 两处 gate（`:777-779`、`:929`） |
| 8 | **事实**：A 项 `WV_PERF=1` per-system 计时器（主程序） | `grep -rn "WV_PERF"` | `wv-render/src/perf.rs` + `app_bootstrap.rs:266/323/452`（注册 + 汇总系统在 `Last`） | **采信·复现**（工具已接线）。**我的断言一条都不依赖它**：它是诊断工具，不是判据 |
| 9 | **事实**：`region_handles_visible` 的判定已覆盖候选锚点 | 读 `world_projection.rs:929` | `if region_handles_visible(view.mode, view.nature_overlay) { ... 候选锚点 ... }` | **采信·复现**（第二处 gate 已在执行链上） |
| 10 | **归因**：`despawn_all` 就够了（隐含：关闭即清理） | `acceptance_nature_overlay::req_overlay_off_reclaims_outline_mesh_assets` | 关 overlay 后 `Assets<Mesh>` **仍为 1**；第 1 轮开关后变 **2** | **不采信（归因错）**：`despawn_all` 只 despawn 实体、`grep meshes.remove` 零命中 ⇒ 资产永不回收 → N5 |
| 11 | **事实**：`cargo test -p wv-world-cell-map` 可作回归门禁（v2 单沿用） | `cargo test -p wv-world-cell-map` | `error[E0277]: the trait bound WindowResolution: From<({float}, {float})> is not satisfied` → `example "grid_material_smoke"` 编译失败，**库测试一条都没跑** | **不采信**：整包门禁今天是红的（不是断言红，是**编译就红**）→ N6 |
| 12 | **事实**：`wv-render` 基线 324 全绿（team-lead 给的本轮前提） | `cargo test -p wv-render --features app --lib` | `343 passed; 1 failed`：`measurement::tests::nearest_measurement_prefers_the_closer_line_at_a_crossing`（`measurement.rs:1001`） | **不采信**：不是全绿 → N7 |

---

## 2. AC 逐条实测证据

### 2.1 沿用 v1（AC1–AC13）

| AC | 条件 | 本轮实测证据 | 状态 |
|---|---|---|---|
| AC1 | fit 缩放下必须有可见网格 | `acceptance_grid_visibility` 11/11 绿（含 `req_world_fit_scale_must_still_show_a_grid`）+ 用户真人确认「网格正确」 | ✅ |
| AC2 | zoom 全程无个位数 FPS | **用户实测 ≈110 FPS**；我无复算能力（§1 #3） | ✅（用户实测背书） |
| AC3 | 12 档重建 ≤ 2 | `req_mesh_rebuilds_across_twelve_notches...` + `req_structural_rebuild_counter...` 双计数器同向 | ✅ |
| AC4 | 12 档每档都有网格 | `req_every_one_of_twelve_world_band_notches_shows_a_grid` ok | ✅ |
| AC5 | 开关关 → 实体 0 | `neg_grid_off_yields_zero_entities_without_erroring` ok | ✅ |
| AC6 | 地形层隐藏 → 实体 0 | `neg_terrain_layer_hidden_yields_zero_entities` ok | ✅ |
| AC7 | 关闭后 mesh 回收 0 | `neg_turning_grid_off_reclaims_mesh_assets` ok | ✅ |
| AC8 | 关闭后 material 不增长 | `neg_material_handles_do_not_grow_across_three_toggle_rounds` ok（只禁增长，不要求 0） | ✅ |
| AC9 | recipe 表值与行为互不否定 | `neg_recipe_gridlines_off_...` ok | ✅ |
| AC10 | 1px 线在 DPR 2 下不消失 | 无渲染帧、无 DPR 2 截图；用户未报 | ⚠️ 未覆盖 |
| AC11 | 粗档线落 `≡0 (mod m)`、`m` 为 2 的幂 | 结构性满足（上轮已验，本轮未回退） | ✅ |
| AC12 | 远观粗网格不把笔迹淹掉 | **用户裁定：粗格边 alpha 叠加可接受，判为不修** | ✅ **已接受，结案（用户裁定 2026-10-23）** |
| AC13 | 4096² 栅格最远端粗档可达 | 上轮手算 + 断言；本轮无改动路径 | ✅ |

### 2.2 本轮新增（AC14–AC21）

| AC | 条件（可验证） | 实测证据 / 命令 | 状态 |
|---|---|---|---|
| **AC14** | 「鼠标移动 60 帧」与「静止 60 帧」在 `segs` 重算次数、`build_stroke_batch_mesh` 调用次数上**完全相等且都为 0**（selection / polygon / 相机不变） | `req_moving_and_stationary_frames_have_identical_zero_recompute_counts` **ok**：两档 Δ 均为 `(0, 0)` 且相等 | ✅ |
| **AC15** | 同上，但计数对象换成「选中区域栅格化次数」 | `RegionDecorCache::rasterizations` **只在 `draw_map_gizmos` 里写**，该系统要 `Gizmos` 参数，无头跑不起来 | ⚠️ 未覆盖（见 §5 残留接口） |
| **AC16** | `nature_overlay` 开，区域数 10 与 200 两档，稳态每帧 `segs` 重算 == 0，只有首帧 > 0 | `req_steady_state_seg_recompute_is_zero_at_both_scales` **ok**：首帧 Δ=1（自证计数器可信），其后 60 帧 Δ=0（两档都是） | ✅ |
| **AC17** | 关 `nature_overlay` ⇒ 顶点手柄**绘制计数 == 0** | 判定谓词 `neg_handle_visibility_follows_the_nature_overlay_switch` **ok**（`false`/`true` 两向都断言）；`handles_drawn` 的实际值未覆盖（同 AC15 原因）；两处 gate 接线已核（`:777-779`、`:929`） | ⚠️ 半覆盖（判定 ✅ / 计数 ⚠️） |
| **AC18** | 关 `nature_overlay` ⇒ 区域轮廓实体数 == 0，**且** `projection.selection` 仍是原值（两条同时断） | `neg_overlay_off_clears_all_outline_entities`（10/200 两档）ok + `neg_overlay_off_keeps_projection_selection` ok | ✅ |
| **AC19** | 连续开关 20 次、移动 600 帧、区域数 200 ⇒ 各项计数不得累积增长 | `req_twenty_toggle_rounds_must_not_accumulate_mesh_assets` **FAILED**：第 1 轮关闭后 `Assets<Mesh>` = **2** > 基线 **1** | ❌ |
| **AC20** | 关 `nature_overlay` ⇒ 轮廓 mesh 资产回收为 **0** | `req_overlay_off_reclaims_outline_mesh_assets` **FAILED**：实得 **1** | ❌ |
| **AC21** | 群系视图内「视图变化」（zoom）不得重建轮廓几何：12 档 ≤ 2 次 | 未立项断言（见下「观察」）。手算：世界带 12 档等比 → 每档 ×`50^(1/11)`=**1.427** > `thickness_bucket` 档宽 ×`2^0.5`=**1.4142** ⇒ **每档必跨档**，重建 ≈ **11 次**；滚轮实际一档 ×0.88 时每档 ×`log2`0.1845×2=0.369 → 约每 2.7 档跨一次 ⇒ 12 档 ≈ **4–5 次**（好于旧实现 12 次，但远不是 0） | ⚠️ 观察（不判缺陷，见 §4 N8） |

**汇总：✅ 15 条 / ⚠️ 4 条 / ❌ 2 条（共 21 条）。** `fail = 2 > 0` ⇒ 不得判过。

---

## 3. 上轮遗留项当前状态（D / N 系列）

| 编号 | 上轮描述 | 本轮状态 | 依据 |
|---|---|---|---|
| D1 | fit 下网格整层退场（阻断） | **已修** | `acceptance_grid_visibility` 11/11 绿；用户真人确认网格可见 |
| D2 | 逐档重建 + DP 逐档 + opt 0（严重） | **已修** | 两计数器 12 档 Δ=0；`Cargo.toml` 三档仍在 |
| D3 | 「≤2 重建」与「1px 屏幕常量」互斥（严重） | **已修**（网格） | 单 quad + shader `fwidth` |
| D4 | `PassId::GridLines` 表与行为互相否定（一般） | **已修** | `neg_recipe_gridlines_off_...` ok |
| D5 | 25 pass 中 8 个无消费点（建议） | **未修**（降到 7 个） | 建议级，仍不阻塞 |
| D6 | `despawn` 在 mesh 缺省时泄漏实体（建议） | **已修**（网格侧） | `neg_despawn_without_mesh_still_despawns_the_entity` ok |
| D7 | `GridConfig::default().cell_size = 8.0` 口径陷阱（建议） | **未修** | 仍存在，建议级 |
| N1 | alpha 0.35→0.48 理由引用了不存在的验收区间（一般） | **结案（用户裁定）**：用户接受当前墨色叠加，判为不修。**但「依据」那句话仍是假的**，建议顺手改成「视觉观感调整」，不阻塞 | §1 #? / 本轮 AC12 |
| N2 | perf 文档两处数据（`u8` 绕回、10→4）经复算不成立（一般） | **未复核**（本轮未重跑 `docs/PERF_GRID_AND_PROFILE_v0.1.md` 的算术） | ⚠️ 挂账，不阻塞 |
| N3 | harness 无法区分 recipe 关 / layers 关（建议） | **已修**（v2 补了断言，本轮仍在且绿） | `neg_recipe_gridlines_off_yields_zero_entities_even_with_terrain_visible` ok |
| N4 | Rust 与 WGSL 的 alpha 合成公式「同构」措辞过强（建议） | **未修**（措辞） | 建议级 |

---

## 4. 本轮新增缺陷

| 编号 | 描述 | 复现步骤 | 级别 | 建议处理 |
|---|---|---|---|---|
| **N5** | **群系视图开关泄漏 mesh 资产**：`RegionOutlinePool::despawn_all`（`region_outline.rs:171-182`）只 `despawn` 实体，不 `meshes.remove`；`outline_material` / `preview_material` / `fill_materials` 也一并留着 | `cargo test -p wv-render --features app --test acceptance_nature_overlay req_overlay_off_reclaims_outline_mesh_assets` → 关后 `Assets<Mesh>`=1（应 0）；`req_twenty_toggle_rounds_...` → 第 1 轮 2 份 > 基线 1 | **一般** | `despawn_all` 改成能拿到 `&mut Assets<Mesh>` / `&mut Assets<ColorMaterial>`，实体 + mesh + 材质一起回收（照网格 `D6` 修法）。灰盒影响：200 区场景每轮开关留一份全量合批 mesh |
| **N6** | **`wv-world-cell-map` 整包门禁跑不到**：`cargo test -p wv-world-cell-map` 在编译 `examples/grid_material_smoke.rs` 时失败（`WindowResolution: From<(f32,f32)>` 不满足），于是**该包的 221 条库测试一条都没执行**。v2 单把它写成「全绿」，其实门禁入口是坏的 | `cargo test -p wv-world-cell-map` → `error[E0277]`；`cargo test -p wv-world-cell-map --lib` → 225 tests / 221 passed | **一般** | 修 example 的窗口分辨率构造（bevy 0.19 的 `WindowResolution` 不再 `From<(f32,f32)>`）。**这是「写了但接不进执行链」的教科书案例**：守卫存在，入口编译不过，红绿都看不见 |
| **N7** | **`wv-render` lib 回归 1 条红**：`measurement::tests::nearest_measurement_prefers_the_closer_line_at_a_crossing`（`measurement.rs:1001` 断言 `nearest_measurement(&world,&map,Vec2::new(50.0,40.0),8.0).is_none()` 失败） | `cargo test -p wv-render --features app --lib` → `343 passed; 1 failed` | **严重** | 本轮 `measurement.rs` 改了 +303 行（不在我的验收范围），但它把包级门禁打红 ⇒ **任何「`cargo test -p wv-render` 全绿」的说法在此之前不成立**。请修或明确回退该重构 |
| **N8** | **手柄/阶梯/候选锚点三处绘制各自持有一份 gate 片段**：`region_handles_visible` 已存在，但 `:777`、`:929` 两处各写一遍 `if`，且 `draw_selected_region` 内部又对 `selection`/`mode` 复判（`:823-830`）。三处一旦有一处漏改就复现本轮症状 | 代码审查（`world_projection.rs:700-702` / `:777-779` / `:823-831` / `:929`） | **建议** | 抽一个 `plan_region_handles(...) -> Plan`（一次判定、返回要画的线段与锚点），三处都消费它；QA 也能直接断言 `Plan.anchors.len()` |
| **N9** | **`thickness_bucket` 在世界带等比缩放下等于没档化**：12 档等比每档 ×1.427 > 档宽 ×1.4142 ⇒ 每档必跨档，轮廓几何仍逐档重建（≈11 次）；真实滚轮 ×0.88 时约 4–5 次 | 手算（§2.2 AC21）+ 代码 `region_outline.rs:145-157` | **建议** | 若要让「群系视图 + zoom」也零重建，唯一出路与网格 `D3` 同一条：几何按**格单位**烘焙，屏幕线宽由 `Transform`/材质施加，键内彻底去掉 scale。**本轮不要求**（用户没报该路径） |

> **不计为缺陷的设计主张差异**：
> - 粗格边 alpha 叠加的观感（**用户已裁定接受**，且属于「需求没定过的阈值」）；
> - `thickness_bucket` 选 1/2 octave 而非 1 octave（是取舍，不是错，问题只在 N9 的量化边界上）；
> - `RegionDecorCache` 的缓存键含 `raster: (w,h)`（幅面变就重算一次栅格化，合理）。

---

## 5. 测试文件与自验（纪律 2）

**新增文件（只加测试，未改任何实现）**：`crates/wv-render/tests/acceptance_nature_overlay.rs`

- 运行：`cargo test -p wv-render --features app --test acceptance_nature_overlay`
- 结果：**11 tests → 9 passed / 2 failed**（两条红即 N5 的缺陷证据，保留、未 `#[ignore]`、未放宽）。

**判定工具自验（计数器必须先证明「能数出数」）**

| 样本 | 构造 | 期望 | 实测 |
|---|---|---|---|
| 命中 | 改一个区的多边形 → 期望 `Δseg_recomputes = 1` 且 `Δstroke_rebuilds > 0` 且**内容指纹变** | 报 1 / 变 | ✅ `ctl_counter_moves_on_a_real_data_change` ok |
| 不命中 | 什么都不改跑 60 帧 → 期望 `Δ = (0,0)` | 报 0 | ✅ `ctl_steady_frames_do_not_move_the_counters` ok |
| 正对照 | 1 区 + 开 overlay → 实体/资产各 1（证明数得出 1） | 1 / 1 | ✅ `ctl_overlay_on_spawns_exactly_one_outline_entity` ok |

⇒ **3/3 通过**。三条同向，说明「数出 0」（AC14/AC16）不是恒 0 的假绿。

**真实命中抽样回原文复核（3 个）**

1. `stroke_rebuilds` 的 3 个自增点（`region_outline.rs:375/389`（轮廓）、`:565/579`（预览））↔ 与 `build_stroke_batch_mesh` 的调用点一一对应，无多计/漏计。
2. `seg_recomputes` 唯一自增点 `:339`，位于 `if !cached { ... }` 内 ↔ 与「缓存命中不计数」的口径一致（这正是 AC14/AC16 的判据要求）。
3. `region_handles_visible` 的两处 gate（`:777-779` 选中手柄、`:929` 候选锚点）↔ 与用户症状 3 的两条绘制路径一一对应，没有第三条漏网的锚点绘制。

---

## 6. 一致性 / 可达性 / 误伤评估（本轮）

**一致性（表 vs 行为互不否定）**

- `region_handles_visible(mode, nature)` 的**注释**说「关群系视图 ⇒ 手柄与阶梯描边一起隐藏，选中态保留」，`AC18` 实测：实体 0 + `selection` 原值 ✓，两边说同一句话。
- `RegionOutlinePool` 的两个计数器与「mesh 内容指纹」两个独立口径在 12 档 / 60 帧 / 两规模下同向（0 与 0），无互相否定。

**可达性（极值真的跑过）**

- 区域数极值 **200** 档（10 与 200 各跑满 60 帧）；帧数 **60**；开关轮次 **20**（跑到第 1 轮就以「2 > 1」判红，未提前跳过）；移动帧 60；这些数字全部按 team-lead 给的口径字面量执行。
- 「600 帧移动」未单独跑：`req_moving_and_stationary...` 用的是 60 帧，泄漏类断言用 20 轮开关 × 200 区。**600 帧那档没跑到**，如实标注（不补跑的理由：计数器在 60 帧已是 0，600 帧只可能更长；但「更长的帧序列会不会漏」我没证）。

**误伤评估（这次修复会不会引入新问题）**

| 风险 | 判据 | 现状 |
|---|---|---|
| 为了不重算 `segs` 而漏掉「区域刚被编辑」 | `ctl_counter_moves_on_a_real_data_change`：改多边形必须 +1 | ✅ 命中 |
| 缓存键漏了某个输入（换图/换格径/换幅面） | 键含 `grid_bits[7]` + `in_cellmap` + `region_count` + `geom`；`RegionDecorCache` 键含 id/poly/grid_bits/raster | ✅ 结构上覆盖（换图/换格径未单独跑断言，列入下轮） |
| 关卡时把手柄连同 `selection` 一起清掉（可用性倒退） | `neg_overlay_off_keeps_projection_selection` | ✅ |
| 关 overlay 后**资产回收**改为 `meshes.remove` 可能删掉正在被重用的句柄 | `ctl_steady_frames_do_not_move_the_counters` + 开关轮次断言 | ⚠️ 未实现（N5 未修），修时须同时保证「开→关→开」仍能重建 |

---

## 7. 验收结论

**不通过。**

> ⚠️ **本节结论已被 §9.5 取代**：主程序当天补件（资产回收 + 手柄计划纯函数）后，我做了第二轮复检，
> 现判定为**有条件通过**。本节保留是为了留痕「当时为什么判红」，不删除。

不是「用户症状没修」——用户报的四条里，三条在**断言层面**已达标（zoom 110 FPS 由用户实测背书、鼠标移动/规模无关两计数为 0、关 overlay 实体清零 + 选中态保留 + 手柄判定谓词两向正确）。判红的是三件**主程序自己能一天内收掉的事**：

1. **AC19 / AC20（N5，一般）**：群系视图开关泄漏 mesh（实测关后仍 1 份、第 1 轮即 2 份）。
2. **N7（严重）**：`wv-render` lib 1 条红 —— `measurement::tests::nearest_measurement_prefers_the_closer_line_at_a_crossing`。**它让「回归全绿」这句话今天不成立**。
3. **N6（一般）**：`cargo test -p wv-world-cell-map` 因 example 编译失败**整包门禁跑不到**，221 条库测试根本没执行。

**三条 ⚠️ 的诚实说明（不因「计数器全绿」而消失）**

- **AC15 / AC17**：`RegionDecorCache::{rasterizations, handles_drawn}` 只在 `draw_map_gizmos` 里写，无头测试跑不起来。**我今天的「手柄没了」只证到判定谓词与两处 gate 接线，没有证到绘制计数为 0。**
- **AC10**：DPR 2 下的 1px 线仍未验（无渲染帧）。
- 更重要的是：**本机全程无窗口、无录屏、无 GPU 呈现。用户报的 120 / 40 / 26 / 110 FPS 四个数字我一个都没复算。** 本轮所有结论的适用范围是「计数器层面」——**「全绿」不等于「能玩」**。26 FPS 是否真的回来了，只有真人再试一次能回答（`WV_PERF=1` 的 per-system 表可以直接指出残余大头，这是 A 项该干的活）。

**退回给**：主程序（lead-programmer-r3）。补件三条，都小：

1. `RegionOutlinePool::despawn_all` 回收资产（mesh + 三个材质句柄），并保证「开→关→开」仍能重建（N5 / AC19 / AC20）。
2. 修 `measurement` 那条红（或明确回退该重构），让 `cargo test -p wv-render --features app` 真正全绿（N7）。
3. 修 `examples/grid_material_smoke.rs` 的 `WindowResolution` 构造，让 `cargo test -p wv-world-cell-map` 能跑到库测试（N6）。

**下一轮（v4）我会按复签流程重跑**：AC 编号沿用，逐条给实测 + 上轮每条缺陷状态 + 本轮新增 + 可交付判定。**另需**：真人录屏（静止 / 鼠标移动 / 开群系视图 三段各 ≥10s，含 `WV_PERF=1` 的 per-system 输出）——没有这一段，「26 FPS 修好了」我不会签字。

---

## 8. 附：等接口清单（残项，仅 1 条）

`AC15` / `AC17` 的**计数**部分需要一个无头可驱动的入口。两个选项（主程序选，我不改实现）：

- **A（推荐，改动小）**：把 `draw_selected_region` / 候选锚点块从 `draw_map_gizmos` 里抽成「计划」纯函数，返回要画的线段与锚点集合（`RegionHandlePlan`），`draw_map_gizmos` 只做消费；QA 直接断言 `plan.anchors.len()`。
- **B（不改结构）**：接受这两条计数只能靠真人 + `WV_PERF` 旁证，**把 AC15 / AC17 的覆盖等级永久标注为「人工验证」**，不写进自动化门禁。

其余接口（`RegionOutlinePool::{seg_recomputes, stroke_rebuilds}`、`RegionDecorCache`、`region_handles_visible`）**主程序本轮已按需提供**，无缺口。

---

## 9. 第二轮复检（同日 · 主程序补件后）——本单结论以本节为准

### 9.1 复检命令与实测

```bash
cargo test -p wv-render --features app --test acceptance_nature_overlay   # 15 tests / 14 passed / 1 failed
cargo test -p wv-world-cell-map                                          # lib 225(221+4ign) / 集成 7 / examples 编译通过
cargo test -p wv-render --features app --lib                             # 345 tests / 1 failed（measurement，见 9.4）
cargo check -p wv-app --features app                                     # Finished（仅 1 条 dead_code 警告）
```

### 9.2 AC 状态变更（原 ✅15/⚠️4/❌2 ⇒ 现 ✅19/⚠️2/❌1）

| AC | 原状态 | 现状态 | 依据（本轮实测） |
|---|---|---|---|
| **AC15** 选中区域栅格化稳态 Δ=0 | ⚠️ 未覆盖 | **✅** | 主程序把绘制抽成纯函数 `plan_region_handles(...) -> RegionHandlePlan` + `RegionDecorCache`；`req_steady_plans_do_not_rasterize_again` 前半段绿：首帧 `rasterizes = 1`（自证），随后 60 帧 **Δ = 0** |
| **AC17** 关 overlay ⇒ 手柄绘制数 0 | ⚠️ 半覆盖 | **✅** | `req_handle_plan_is_empty_when_overlay_off` 绿：有选中 ⇒ `handle_count() == 0`、`stepped_edges` 空、`selection` 仍 `Some`；只有候选 ⇒ `candidate_anchors.len() == 0`；`ctl_handle_plan_is_populated_when_overlay_on` 作正对照（`> 0`） |
| **AC19** 20 轮开关不累积 | ❌ | **✅** | `req_twenty_toggle_rounds_must_not_accumulate_mesh_assets` 绿 |
| **AC20** 关 overlay 资产回收 0 | ❌ | **✅** | `req_overlay_off_reclaims_outline_mesh_assets` 绿（`despawn_all` 已走 `reclaim_mesh` / `reclaim_material`） |
| **AC22a**（**新**）锚点抽稀稳态不得重算 | — | **✅**（主程序补 `AnchorKey` 后转绿） | `req_steady_plans_do_not_rasterize_again` 全绿：首帧 `anchor_decimates = 1`，60 稳态帧 Δ = 0 |
| **AC22b**（**新**）抽稀缓存必须**按 scale 分档**（否则 smooth zoom 每帧重算） | — | **✅**（第二轮补件后转绿） | `req_anchor_decimation_cache_is_bucketed_by_scale` 绿：`×1.02` Δ=0、`×4` 恰好 +1。代码核对：`world_projection.rs:887-888` `bucket_scale = scale_of_bucket(scale_bucket(view.scale))` —— 键里是**档号派生的 spacing**，不再是连续 `scale` |
| **AC23**（**新**）缓存失效时必须清空锚点（不得继续画上一帧的手柄/描边） | — | **✅** | `req_stale_anchors_are_cleared_when_the_condition_goes_away` 绿：取消选中 ⇒ 手柄 0 + 描边 0；候选格消失 ⇒ 候选锚点 0。代码侧清空点：`world_projection.rs:881-882 / 960-961 / 984-985` |
| **AC24**（**新**）跨档时锚点不得整批「跳位」 | — | **⚠️ 未覆盖** | 主程序自陈：`anchor_spacing` 未做档内插值，跨档时锚点整批换位（一次跳变），与线宽档同一取舍。**只有真人目视能判**，我无渲染帧 |
| AC10 / AC21 | ⚠️ / ⚠️ | ⚠️ / ⚠️ | 仍未覆盖（无渲染帧 / 观察项） |

### 9.3 本轮新增缺陷

| 编号 | 描述 | 证据 | 级别 | 建议处理 |
|---|---|---|---|---|
| **N10** | **手柄锚点抽稀未进缓存**（AC22a，**已修**）：`plan_region_handles` 里 `rasterize_polygon_cells` 已按键缓存，但 `decimate_poly` 每帧重算 | 首轮复检：`anchor_decimates` 60 帧 = **61** | 一般 | 主程序已加 `AnchorKey`（poly + spacing + map_origin）→ 转绿 |
| **N10b** | **同一处缓存的键用连续 `spacing`，等于按原始 scale 缓存**（AC22b）：`spacing = anchor_spacing(view.scale, …)` 随 scale 连续变化，`×1.02` 就换键 ⇒ 平滑 zoom（滚轮惯性 / zoom-to-bounds 补间）**每帧都重抽稀**，与「没缓存」等价 | `req_anchor_decimation_cache_is_bucketed_by_scale`：`scale ×1.02` 后 `anchor_decimates` 1 → **2** | **一般** | 把 `spacing` 换成**档号**再进键（复用同文件已有的 `thickness_bucket` / `log2×2` 量化）；密度随档差 ±19%，与线宽同一个论证。**不要**按原始 scale 位模式进键 |
| **N11** | **文档与实现不一致**：`docs/PERF_GRID_AND_PROFILE_v0.1.md:128/140/141` 写 `GRID_COARSE_MIN_PX = 8.0`、`GRID_COARSE_MAX_EXP = 6`（≤64×），而代码是 `4.0` / `5`（`GRID_COARSE_MAX_MULT = 32`），断言（`acceptance_grid_visibility.rs:200`）也跟着 4px 走 | `grep -n GRID_COARSE_MIN_PX crates/**/grid_lines.rs` = `4.0`；同 `docs/PERF_GRID_AND_PROFILE_v0.1.md:140` = `8.0` | **一般** | 以**代码 + 用户实机确认**为准（4px / 32×），改文档三处；并**留痕**：「8px→4px 是谁、何时、为什么」（v1 的 D7 就是「两个载体不一致」留下的坑）。**我的自纠**：v2 单说「粗档上限需 ≥64」是按文档算的；按代码 4px，`0.125 × 32 = 4.0px` 恰好可达，32× 够 |

### 9.4 归因订正（我上一版写错了责任人）

- **N7（measurement 红）不属主程序本轮范围**。证据：`git show HEAD:crates/wv-render/src/measurement.rs | grep -c nearest_measurement` = **0**（该函数在 HEAD 里根本不存在），`git diff --numstat` = `+259/−44` ⇒ 是**另一路在飞改动**（测量工具 + i18n 一族）带进来的。它仍然是 `cargo test -p wv-render` 全绿的门禁红，**但应由那路改动的所有者收口**，不算主程序的补件缺口。
- **N6 已修（主程序执行 → 我复跑转绿）**：`cargo test -p wv-world-cell-map` 现在整包能跑完（lib 221 绿 / 集成 7 绿 / examples 编译通过）。
- **a6 的时间线（制作人 2026-10-23 纠正，本节按纠正后的账记）**：不是我先前写的「本就不需要裁决」。
  实际顺序是 **① 15:39 制作人亲手复跑 → 红在 `acceptance_grid_visibility.rs:195` 的 `coarse >= 8.0 - 1e-4`；
  ② 制作人裁决取 4px 口径；③ 主程序执行：改断言到 `:200`（`4.0`）并把该测试里三处 8px 派生期望
  重新手算（`0.78125 × 8 = 6.25px`、`0.125 × 32 = 4.0px`）；④ 我复跑 `--test acceptance_grid_visibility` = 7/7 绿。**
  ⇒ 记作「**制作人裁决 → 主程序执行 → 质量复跑转绿**」。我先前说「不需要裁决」是错的：那会让下一个人
  以为当时是假红，把一次真实裁决记成噪声。
- **`i18n::keys::ALL` 同理**：主程序报的 458 是他报之前的真实状态，随后**由主程序代改**为 461；
  我复跑的 `cargo check -p wv-app --features app` Finished 是**改后**的绿。记作「主程序执行 → 质量复跑转绿」，
  不写作「本来就没问题」。

**本节缺陷的当前状态（主程序同日补件后，我逐条复跑确认）**：**N10 已修**、**N10b 已修**、**N11 已修**；
**N5 / N6 已修**（见 9.4）；**N7 非本轮对象**（归因订正见 9.4）；**N8 未修**（三处 gate 已合并为
`plan_region_handles` 的**单处**判定，但 `draw_selected_region` 内的二次复判仍在，建议级，不阻塞）；**N9 未修**（AC21 观察项）。

### 9.5 修订后的判定

**通过（限「计数器与自动化门禁」层面）——但不得据此对用户宣布「26 FPS 修好了」。**

- 三态（AC22 拆分 + 新增 AC23/AC24 后）：**✅ 23 / ⚠️ 3 / ❌ 0（共 26 条）**。`blocked = 0`、`fail = 0`。
- ⚠️ 三条全部是「机器测不出来」的那一类：**AC10**（DPR 2 的 1px 线）、**AC21**（群系视图内 zoom 仍每跨档重建一次）、**AC24**（跨档锚点整批跳位）。
- 本轮我独立复跑的跑数（不是采信主程序）：
  - `--test acceptance_nature_overlay` → **17 tests / 17 passed / 0 failed**
  - `-p wv-world-cell-map` 整包 → lib **221** 绿（4 ignored 为探针）+ 集成 **7** 绿 + examples 编译通过
  - `-p wv-app --features app` → `Finished`
  - `-p wv-render --features app --lib` → **346 tests / 345 passed / 1 failed**（唯一红 = `measurement::…at_a_crossing`，**非本轮对象**，见 9.4）
- **我复核过自己的门禁文件没被改动**：`acceptance_nature_overlay.rs` 现有 `#[test]` = 17（我自己从 11 → 15 → 16 → 17 加的），`×1.02` / `× 4.0` / `for _ in 0..FRAMES` 等判据字面量一字未改，`FRAMES = 60` / `N_LARGE = 200` / `TOGGLE_ROUNDS = 20` 未被放宽。

**三条硬约束（对外表述口径，任何人不得越过）**

1. **真人录屏 + `WV_PERF=1` 的 per-system 输出**（静止 / 鼠标移动 / 开群系视图 / 关掉，各 ≥10s）：到手之前，对外只写「26 FPS 修复未做真人验证」。用户报的 120/40/26/110 四个数字**我一个都没复算**（本机无窗口、无 GPU 呈现）。
2. **`measurement::nearest_measurement_prefers_the_closer_line_at_a_crossing` 那条红在收口前**，任何人不得声称 `cargo test -p wv-render` 全绿（team-lead 已按同一口径对外表述）。
3. **AC24（跨档锚点跳位）** 由真人在录屏时顺手看一眼；若肉眼可见，把它降到 1/4 octave（重算次数翻倍，仍是每跨档一次）。

**本轮已关闭的条件（留痕，不删）**

1. ~~AC22b / N10b：`spacing` 换档号~~ → **已修**：`world_projection.rs:887-888` 用 `scale_of_bucket(scale_bucket(view.scale))`，我的三条判据全绿。
2. ~~N11：文档 8.0/64× 与代码 4.0/32× 矛盾~~ → **已修**：`docs/PERF_GRID_AND_PROFILE_v0.1.md:128` 改 4px、`:133-134` 加订正引用块、`:147-148` 两行常量与手算表按 4px 重算。
3. ~~N10：`decimate_poly` 每帧重算~~ → **已修**（首轮复检 60 帧 61 次 → 现在 Δ=0），并顺带把「线宽档」与「手柄档」合并成同一个 `scale_bucket`（同一事实只留一份载体）。

### 9.6 复检新增的测试（4 条，全部落盘、非临时脚本）

`crates/wv-render/tests/acceptance_nature_overlay.rs` 追加：

| 测试 | 作用 | 现状 |
|---|---|---|
| `ctl_handle_plan_is_populated_when_overlay_on` | 自验·**命中**：开 overlay + 选中 ⇒ 手柄 > 0 | ✅ |
| `req_handle_plan_is_empty_when_overlay_off` | AC17：关 overlay ⇒ 手柄/描边/候选锚点全 0，且 `selection` 不动 | ✅ |
| `req_steady_plans_do_not_rasterize_again` | AC15（栅格化，✅）+ AC22（抽稀，❌） | 1 红 |
| `req_candidate_derivation_is_cached_and_gated` | 候选按格派生稳态 Δ=0 + 同一道 overlay 门控 | ✅ |
| `req_anchor_decimation_cache_is_bucketed_by_scale` | AC22b：抽稀缓存**按 scale 分档**（×1.02 不重算 / ×4 恰好 +1） | ✅（补件后转绿） |
| `req_stale_anchors_are_cleared_when_the_condition_goes_away` | AC23：缓存失效时必须清空锚点（取消选中 / 候选格消失） | ✅ |

**当前跑数**：`cargo test -p wv-render --features app --test acceptance_nature_overlay` → **17 tests / 17 passed / 0 failed**。

自验计数：**命中 2 条**（`ctl_counter_moves_on_a_real_data_change`、`ctl_handle_plan_is_populated_when_overlay_on`）+ **不命中 2 条**（`ctl_steady_frames_do_not_move_the_counters`、`req_steady_plans_do_not_rasterize_again` 的栅格化段）= **4/4**，故「数出 0」不是恒 0 的假绿。
