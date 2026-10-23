# cell-map 缩放性能 + 网格可见性 验收单 v2（第 1 轮复签 · 过程审）

- **验收人**：严过关（质量监管）
- **日期**：2026-10-23
- **本轮对象**：主程序交件（双档网格 + shader 线宽 + 构建三档 + recipe 收口 + DP 桶化）
- **AC 编号**：沿用 v1（`docs/reviews/2026-10-23-cellmap-zoom-grid-acceptance.md`）不重排，便于逐轮 diff
- **本轮结论**：**有条件通过**（计数器层面 11/11 绿；三条视觉/FPS 项仍未覆盖，条件见 §5）
- **纪律声明**：本单整份重写，未复制 v1 正文。凡上游自报结论一律进 §1 复算后才采信。

---

## 0. 我这轮实际跑了什么（可复跑的原文命令）

```bash
# 1) 上游改了我的 harness（换材质注入 + 加 RenderRecipe）——先核对有没有动口径
git status --porcelain
git show HEAD:crates/wv-world-cell-map/src/terrain_view/isoline_overlay.rs | sed -n '78,100p'
git diff -- Cargo.toml
git diff -- crates/wv-render/src/recipe.rs

# 2) 独立复跑（不采信交件自报）
cargo test -p wv-render --features app --test acceptance_grid_visibility
cargo test -p wv-world-cell-map --test acceptance_grid_visibility

# 3) 门禁接线核查（「写了但没接进执行链的守卫等于没有守卫」的同构检查）
grep -rn "GridLinesMaterialPlugin|GridLinesMaterial" crates/wv-app/src/
```

- `crates/wv-render/tests/acceptance_grid_visibility.rs` 我逐行比对过：**改动只在注入口**
  （`Assets<ColorMaterial>` → `Assets<GridLinesMaterial>`、新增 `RenderRecipe` 注入、
  `material_assets()` 换计数对象），**断言、期望值、`req_/ctl_/neg_` 红绿约定一字未改** → 不打回。
- **接线实证**：`GridLinesMaterialPlugin` 已注册进真实 App（`wv-app/src/app_bootstrap.rs:113`），
  `sync_grid_lines` 仍在 Update 主链（`app_bootstrap.rs:363`）。
  ⇒ 不是「只在 example 里能跑、App 里没接」的两套世界。这一条是本轮我最担心的地方，已排除。

---

## 1. 上游结论复算（事实格 / 归因格分开）

| # | 上游说法 | 我实际执行的命令 / 输出摘要 | 判定 |
|---|---|---|---|
| 1 | 「7 条断言现在全绿」 | 我自己跑 `cargo test -p wv-render --features app --test acceptance_grid_visibility` → `7 passed; 0 failed`（1.93s） | **采信·复现** |
| 2 | 「只改了注入口，断言没改」 | 逐行比对 + `git status`（该文件 untracked，`git diff` 不可用，故按原文逐行核） | **采信·复现** |
| 3 | 「`grid_line_fade` 公式与端点一字未改」 | 读 `grid_lines.rs:108-110`：`((px - MIN)/(FULL - MIN)).clamp(0,1)`，MIN=2/FULL=4 → 4→1、3→0.5、2→0 | **采信·复现**（我的 A1/A2 仍过） |
| 4 | 「fit 档 stride = 16、粗档 12.5px」 | 手算：`4.0/5.12 = 0.78125`；×8 = 6.25 < 8 → 取 16；×16 = **12.5** ✔ | **采信·复现** |
| 5 | 「alpha fit 档 0.48 × 0.85 = 0.408」 | 手算：`0.48 × max(0, 0.85) = 0.408` ✔（但 0.48 是本轮从 0.35 改的——见 N1） | **采信·复现（数值）/ 归因存疑（N1）** |
| 6 | 「粗档上限 `2^6 = 64`，4096² 栅格最远端需 64×」 | 手算：`cell_size 1.0 / scale 8.0 = 0.125px`；×32 = 4.0 < 8；**×64 = 8.0** ✔ 上限确实要 ≥64 | **采信·复现**（同时订正我 v1 的「≥32」——v1 用的是 4px 阈值，现设计要求 8px，故 32 不够） |
| 7 | 「旧 `tol_q: u8` 在 `scale > 8` 时 `as u8` 会按 256 绕回」 | `git show HEAD:...isoline_overlay.rs` → `tol_quantum = (dp_tol_continuous(scale) * 64.0) as u8`，而 `dp_tol_continuous = (0.5*scale).clamp(0.125, 0.5)` ⇒ 值域 **`[8, 32]`**（HEAD 自带单测也写 `tol_quantum(4.0) == 32`、`(1.0) == 32`） | **不采信（归因错）**：值域上限 32，**不可能触碰 256**。结论「桶化更好」成立，但给的理由是假的 → N2(a) |
| 8 | 「12 档重算次数 10 → 4」 | 新侧我复算得上：`bucket = round(log2(tol)×2)`，12 档序列 → `-6,-6,-5,-4,-3,-2,…,-2` = **4 次跨档** ✔。旧侧我复算：旧键是 `tol_q`（floor 量化）+ `nation`（**无** `scale_bits`，见 HEAD 版 `IsolineKey`）→ 序列 `8,8,10,14,21,30,32,32,32,32,32,32` = **5 次变化**，`nation` 布尔最多再 +1 = **6 次** | **部分不采信**：新侧 4 ✔，**旧侧 10 ✘（我算出 5~6）**。即真实收益 ≈ 1.25×，不是 2.5× → N2(b) |
| 9 | 「`rebuilds` 只统计结构重建，可直接当计数器」 | 读 `cellmap_bridge.rs:1374-1411`：键相等即 `refresh_ink` + return（不碰 `rebuilds`）；只有结构重建分支 `overlay.rebuilds += 1` | **采信·复现**（并据此加了 REQ-4） |
| 10 | 「`assemble_strokes` DP 15.56ms debug / 3.92ms release」等墙钟表 | 未复跑（无显示器工况下的探针，且按我的口径**墙钟不作判据**） | **未覆盖**（作参考值记录，不参与判定） |

---

## 2. AC 逐条实测证据

| AC | 条件 | 本轮实测证据 | 状态 |
|---|---|---|---|
| **AC1** | 世界级 fit 下必须有可见网格 | `req_world_fit_scale_must_still_show_a_grid` → **ok**（实体 1）；手算链：`cell_px 0.78125` → 细档 fade 0 → 粗档 stride **16** → 定位格 **12.5px** → `alpha = 0.408 > 0` | ✅ |
| **AC2** | zoom 全程无个位数 FPS | **无任何帧率实测**（无头环境 + 无录屏）。上游给的是探针毫秒/倍数，不是 FPS | ⚠️ 未覆盖 |
| **AC3** | 12 档重建 ≤ 2 | `req_mesh_rebuilds_...` → **ok**；新增 `req_structural_rebuild_counter_must_not_move_...`：`rebuilds` Δ = **0**，handle 变化 = **0**（两计数器一致，互不否定） | ✅ 实测 **0**（优于 ≤2 上限） |
| **AC4** | 12 档每档都有网格 | `req_every_one_of_twelve_world_band_notches_shows_a_grid` → **ok**（12/12 有实体，v1 的 4 档空已消除） | ✅ |
| **AC5** | 开关关 → 实体 0 | `neg_grid_off_...` → ok | ✅ |
| **AC6** | 地形层隐藏 → 实体 0 | `neg_terrain_layer_hidden_...` → ok | ✅ |
| **AC7** | 关闭后 mesh 回收 0 | `neg_turning_grid_off_reclaims_mesh_assets` → ok | ✅ |
| **AC8** | 关闭后 material 回收 | 现实现 `despawn` **连材质一起回收**（`grid_lines.rs:287-307`，实测关闭后 `Assets<GridLinesMaterial>` = 0）。按 team-lead 裁定另加「关→开→关 3 轮不增长」断言 `neg_material_handles_do_not_grow_across_three_toggle_rounds` | ✅（该新断言见 §4 实测行） |
| **AC9** | recipe 表值与行为互不否定 | 系统 gate 已读 `recipe.enabled(PassId::GridLines)`（`cellmap_bridge.rs:1341`）；`recipe.rs` Topographic 行 `false → true`。**但我发现新覆盖缺口并补了断言**（见 N3 / `neg_recipe_gridlines_off_...`） | ✅ |
| **AC10** | 1px 线 HiDPI 不变 0.5px 消失 | 线宽由 `fwidth` 求得（`grid_lines.wgsl:49-53`，`tune.x` = 1px），DPR 变化不改世界坐标导数 → **理论上等价于 1 逻辑 px**。无 DPR 2 截图 | ⚠️ 未覆盖（无渲染帧） |
| **AC11** | 粗档线落 `≡0 (mod m)`、`m` 为 2 的幂 | `grid_coarse_stride` 恒 2 的幂（断言 `a6_...` 遍历 6 个缩放点）；shader 用 `cell.x / stride` 取样 ⇒ 线必在 `k·stride` 格 ✔ | ✅（结构性满足） |
| **AC12** | 远观粗网格不把笔迹淹掉 | **无渲染帧**。且基色 alpha 本轮 **0.35 → 0.48（+37%）** 而粗档恒在 → 远观墨线总量**上升**，风险比 v1 更高 | ⚠️ 未覆盖（风险↑） |
| **AC13** | 4096² 栅格最远端仍可读 | 手算 + `a6` 断言：`0.125px × 64 = 8.0px`；上限 `2^6 = 64` 恰够 | ✅（v1 的「≥32」已按 8px 阈值订正为 ≥64） |

**汇总：✅ 8 / ⚠️ 3 / ❌ 0。**

---

## 3. 上轮 7 条缺陷的当前状态

| 缺陷 | v1 级别 | 当前状态 | 我的判定依据 |
|---|---|---|---|
| **D1** 世界级 fit 下整层退场 | 阻断 | **已修** | 双档：细档只退出自身，粗档恒在；`fade=0 ⇒ despawn` 路径已删（gate 里不再有 fade 判定） |
| **D2** 逐档重建 + DP 逐档重跑 + opt 0 | 严重 | **已修（附两处数据订正）** | 键去 `scale_bits`（`GridKey` 只剩 size/height/cell_bits/origin/palette）→ 实测重建 0；`Cargo.toml` 三档（成员 1 / 依赖 2 / 三个热点 crate 3）；DP 改 1/2 octave 桶。**但收益被高报**（N2） |
| **D3** 「≤2 重建」与「1px 屏幕常量」互斥 | 严重 | **已修** | 结构改造正是 v1 要求的方向：单 quad + shader `fwidth`，`half_w` 不再进 CPU；`GridKey` 无 scale |
| **D4** `PassId::GridLines` 表与行为互相否定 | 一般 | **已修** | 系统读 recipe + 表改 true，两边说同一句话 |
| **D5** 25 pass 中 8 个无消费点 | 建议 | **未修（缩小为 7 个）** | `GridLines` 已加入消费侧；其余 7 个仍无消费者。建议级，不阻塞 |
| **D6** `despawn` 在 mesh 为 None 时泄漏实体 | 建议 | **已修** | 拆成三个独立 `if let`（entity / mesh / material 各自回收）＋ 我加了回归断言 `neg_despawn_without_mesh_still_despawns_the_entity` |
| **D7** `GridConfig::default().cell_size = 8.0` 口径陷阱 | 建议 | **未修** | 仍存在，建议级 |

---

## 4. 本轮新增缺陷

| 编号 | 描述 | 证据（可复跑） | 级别 | 建议处理 |
|---|---|---|---|---|
| **N1** | 基色 alpha **0.35 → 0.48**，理由写作「0.48 落在**验收要求**的 0.45–0.5 区间中部」——**我从没出过这个区间**（v1 明确写「alpha 与细档同源 0.35」，且把阈值取值列为「不计为缺陷的设计主张」）。这是把一个不存在的验收要求当成依据 | `git show HEAD:...grid_lines.rs` 中 `GRID_LINE_BASE_ALPHA = 0.35`；现为 0.48（`grid_lines.rs:56`） | **一般** | 二选一：① 找到该区间的出处（制作人裁定 / ADR）并写进注释；② 把措辞改成「视觉观感调整，待渲染帧举证」。**另：+37% 墨色叠加「粗档恒在」，直接抬高 AC12 风险**——这条不能只靠断言过 |
| **N2** | perf 说明两处数据经我独立复算不成立：**(a)** 「旧 `tol_q` 在 `scale > 8` 会按 256 绕回」→ 值域 `[8,32]`，不可能绕回；**(b)** 「12 档重算 10 → 4」→ 新侧 4 ✔，旧侧按 HEAD 版键（`tol_q` + `nation`，无 `scale_bits`）算出 **5~6**，不是 10 | 见 §1 第 7、8 行 | **一般** | 改 `docs/PERF_GRID_AND_PROFILE_v0.1.md` 与代码注释。数据订正不是抠字眼：若制作人按「2.5× 收益」判「FPS 已解决」，真实只有 ~1.25×，会误判 |
| **N3** | 上游改 harness 时把 `Terrain` 与 `GridLines` 两格**同时**置成同一个值，于是「层隐藏 → 0 实体」无法区分是 recipe 关的还是 layers 关的 ⇒ **本轮新加的 recipe 收口（D4 的修）零覆盖** | 读 harness `recipe_with_grid(terrain_visible)`；我补的 `neg_recipe_gridlines_off_yields_zero_entities_even_with_terrain_visible` 就是补这个洞 | **建议** | 已由我补断言（不改实现）。请保留该断言，别在下次改动时删掉 |
| **N4** | Rust `grid_line_alpha = BASE × max(fade, K)` 与 WGSL `max(m_fine, m_coarse) × K` **不是同一式**（前者把细档淡出与粗档系数取 max，后者对覆盖率加权取 max）。在格线中心两者等价，格内非零区 Rust 偏保守 | `grid_lines.rs:142-144` vs `grid_lines.wgsl:70-79` | **建议** | 注释里「同构」改成「在格线中心等价」；或让 Rust 侧只作为**上界**用途说明 |

---

## 5. 可交付判定

**有条件通过。**

- 计数器口径：**11 条断言全绿**（7 原有 + 4 新增），`❌ = 0`、`blocked = 0` → 不构成「不得判过」。
- 但**三条 ⚠️ 全部属于「机器测不出来的那一类」**：AC2（跟不跟手）、AC10（HiDPI 线是否还在）、AC12（远观脏不脏）。
  按本岗纪律：**视觉交付必须有实际渲染帧，只看断言不算验收通过**；**「全绿」不等于「能玩」**。

**放行条件（三条必交物，缺一不得对用户宣布修好）**

1. **真人试玩录屏**：关掉一切、从地址栏冷启动、只用滚轮 zoom in/out 走完世界带（含首帧与最远/最近端），全程无可感知卡顿。
2. **两帧实际渲染图**：远观 `scale 5.12`（判「12.5px 一格粗网格可见 + 笔迹仍可辨 + 不压地形」）与近观 `scale 1.0`（判「细档 1px 线连续、无摩尔纹」）。**N1 把墨色加了 37%，这两帧是它唯一的判据。**
3. **两处数据订正**（N2）写回 `docs/PERF_GRID_AND_PROFILE_v0.1.md` 与代码注释。

满足 1–3 后我出 v3 终审；**在此之前：「修好了」只能说「计数器层面已修，视觉与手感待验」。**

---

## 6. 本轮我改了什么（只动 QA 资产，不动实现）

- 新增 4 条断言（`crates/wv-render/tests/acceptance_grid_visibility.rs`）：
  `req_structural_rebuild_counter_must_not_move_across_twelve_notches`（AC3 双计数器互证）、
  `neg_recipe_gridlines_off_yields_zero_entities_even_with_terrain_visible`（补 N3 覆盖洞）、
  `neg_material_handles_do_not_grow_across_three_toggle_rounds`（AC8 替代判据）、
  `neg_despawn_without_mesh_still_despawns_the_entity`（D6 回归）。
- 新增 2 条断言（`crates/wv-world-cell-map/tests/acceptance_grid_visibility.rs`）：
  `a6_coarse_stride_is_power_of_two_and_locator_cells_stay_readable`（AC11 + AC13 可达性）、
  `a7_alpha_never_drops_to_zero_once_the_switch_is_on`（AC1 的正向版本：「开关开 = 一定看得见」）。
- 未放宽任何原有断言；`req_` 三条保持原名原义（现已全绿，若回退会立刻再红）。
