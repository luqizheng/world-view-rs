# cell-map 缩放性能 + 网格可见性 验收单（v1 · 过程审）

- **验收人**：严过关（质量监管）
- **日期**：2026-10-23
- **验收对象**：`wv-render::cellmap_bridge::sync_grid_lines` 及其上下游（网格可见性 / 缩放重建）
- **代码基线**：`git HEAD 7a860a4`，工作树对 `crates/wv-render` / `crates/wv-world-cell-map` **零改动**（`git status --porcelain` 只有 `examples/*/ui-layout.yaml` 与未跟踪文件），即**修复尚未开始**。本单所有结论针对「修复前基线」，同时也构成修复的目标规格。
- **用户需求原文（唯一标尺）**：
  1. cell-map 里 zoom in / zoom out 时 FPS 只有个位数。
  2. 工具条开了「网格」但看不到网格。
- **三态口径**：✅ 达标 / ⚠️ 未覆盖或待裁定 / ❌ 不达标。**`blocked + fail > 0` 不得判过。**

---

## 0. 采信矩阵（上游结论逐条复算：事实格与归因格分开）

> 纪律：上游每条结论进本单前必须占一行，三列缺一不可。**只重跑数字对得上就采信，会放过「事实为真、因果错配」。**

| # | 上游结论（拆成事实/归因） | ① 我实际执行的命令 | ② 本机原始输出（摘要） | ③ 结论 |
|---|---|---|---|---|
| 1 | **事实**：淡出逻辑在 `grid_lines.rs:37-44`，`cell_px = cell_size/scale`，`<2px` 整层退场，基色 alpha 0.35 | `Read grid_lines.rs` | 常量 `GRID_LINE_BASE_ALPHA 0.35` 在 25 行、`GRID_FADE_START_PX 4.0 / END 2.0` 在 37-38 行、纯函数 `grid_line_fade` 42-44 行 | **采信·推断修正**：常量与纯函数确在 37-44；但**「整层退场」的判定不在本文件**，在 `cellmap_bridge.rs:1331-1334`。行号归属偏移，事实方向正确 |
| 2 | **事实**：消费点 `cellmap_bridge.rs:1331` | `Read cellmap_bridge.rs 1280-1401` | `let fade = grid_line_fade(grid.cell_size / view.scale);` (1331)、`if fade <= 0.0 { overlay.despawn(...) }` (1332-1335) | **采信·复现** |
| 3 | **事实**：lod-demo extent 4096 / raster 1024 → `cell_size = 4.0` | `Read examples/lod-demo/maps/map_world/index.yaml` + `Read sync_grid_config` | yaml：`extent.width: 4096.0`、`raster.width: 1024`；`grid.cell_size = (extent / tex.size as f32).max(0.01)` (2133) | **采信·复现**（4096/1024 = 4.0） |
| 4 | **事实**：世界级 `fit ≈ 5.12`，引 `zoom_level.rs:36-41` | 独立手算 + `Read zoom_level.rs` | 我算：`max(4096/1280, 4096/800) = max(3.2, 5.12) = 5.12`；代码 `fit_scale` 在 183-187，测试锚点 277-282 断言 5.12 | **采信·推断修正**：36-41 是**模块文档表格**，不是实现；实现是 `fit_scale`(183-187)。数值 5.12 已由我独立手算复现 |
| 5 | **事实**：`sync_grid_lines` **不读** `RenderRecipe`；`recipe.rs` Topographic 行把 `GridLines` 写 `false`（无消费者，死旗标） | `Read 1315-1346` + `Read recipe.rs 280-307` + `grep -rn "\.enabled(" \| grep -o "enabled(PassId::[A-Za-z]*" \| uniq -c` | 系统签名 1315-1324 无 recipe 参数；gate 1325 只读 `layers.terrain.visible` + `grid_lines_visible(&view)`；`recipe.rs:283` = `false, // GridLines`。消费点枚举 18 项，**无 GridLines** | **采信·复现**（并补充：另有 7 个 pass 同样无消费者，见 D5） |
| 6 | **事实**：`GridKey` 用 `view.scale.to_bits()` → 每滚轮档整层重建 mesh | `Read 1336-1346` + `Read grid_lines.rs:56-64` | `scale_bits: view.scale.to_bits() as u64` (1345)，`GridKey.scale_bits` 是 `PartialEq` 的字段 | **采信·复现**（并**实测**：12 档重建 7 次，见第 5 节 REQ-3） |
| 7 | **事实**：`tol_quantum` 1/64 量化 → 每档几乎必跨档重跑 DP | `Read isoline_overlay.rs:209-223` | `(dp_tol_continuous(scale) * 64.0) as u8` (222)；`dp_tol_continuous = (0.5*scale).clamp(0.125,0.5)` (216) | **采信·复现**（未单独实测 DP 次数，标为 D2 的**候选归因**，见下） |
| 8 | **事实**：`Cargo.toml` 无 `[profile.dev]` / 无 `[profile.dev.package."*"]` → **全栈** opt-level 0 | `cat Cargo.toml` | 文件里**有** `[profile.dev.package.wv-app] opt-level = 2` 与 `[profile.dev.package.image] opt-level = 2`；无全局 `[profile.dev]`、无通配包段 | **采信·推断修正**：方向成立（`wv-render` / `wv-world-cell-map` / bevy 仍是 0），但「全栈 0」不精确——`wv-app` 与 `image` 已是 2 |
| 9 | **归因**：用户症状 1（FPS 个位数）由 #5#6#7#8 共同导致 | 无（本机无显示器、未起窗口、未录屏） | — | **未覆盖**。无头环境测不出帧率；我**不采信**该归因链，只把它列为 D2（严重，归因待实测）。任何「FPS 已恢复」的结论都必须有真人试玩录屏，见第 7 节 |
| 10 | **归因**：用户症状 2（开了看不到）由淡出逻辑导致 | 独立手算 + 独立测试 `acceptance_grid_visibility`（两处 crate） | `cell_px(5.12)=0.78125 → fade=0`；ECS 实测：scale 5.12 下网格实体数 = **0**；12 档中 4 档无网格 `[2.7525, 3.9281, 5.6058, 8.0]` | **采信·复现**（手算、纯函数、ECS 实体数三条独立路径同向） |

**自验（纪律 2：判定工具先自验才许对他人下结论）**

- 工具 = 本单配套的两份独立测试（`tools/qa` 不适用：本仓测试直接落在 `crates/*/tests/`，可独立跑）。
- 已知答案样本构造：`ctl_close_zoom_...`（**应命中**：scale 1.0 → cell_px 4.0px → 必须数出 1 个实体）与 `neg_grid_off_...`（**应不命中**：开关关 → 必须数出 0）。两者都通过 → **2/2 通过**，证明计数器既能数出 1 也能数出 0（不是恒 0 的假绿/恒绿的假红）。
- 真实命中抽样回原文复核 3 个：① `grid_line_fade(3.0)=0.5` ↔ 手算 `(3-2)/2=0.5`；② 12 档无网格档位 `[2.7525, 3.9281, 5.6058, 8.0]` ↔ 手算 `scale ≥ 2.0` 即不可见（序列 i=8..11 正好 4 档）；③ 重建 7 次 ↔ 可见档位 i=0..7 共 8 个取样点、相邻可见档 7 次换键。三条全部与手算一致。

---

## 1. 验收条件（AC，编号在后续轮次沿用不重排）

| AC | 条件（可验证） | 状态 |
|---|---|---|
| AC1 | 世界级「全图适配」缩放下（`scale = 5.12`），工具条「网格」开启时**必须有可见网格输出** | ❌ |
| AC2 | zoom in / zoom out 过程中不出现个位数 FPS（用户可感知流畅） | ⚠️ 未覆盖 |
| AC3 | 世界带 12 档等比缩放全程，网格 mesh **重建次数 ≤ 2** | ❌ |
| AC4 | 世界带 12 档等比缩放**每一档**都有可见网格（细档 <2px 退场时由粗档承接） | ❌ |
| AC5 | 开关关 → 网格实体数 = **0**（不是「没报错」） | ✅ |
| AC6 | 地形图层隐藏 → 网格实体数 = **0** | ✅ |
| AC7 | 关闭后 **mesh** 资产回收为 0 | ✅ |
| AC8 | 关闭后 **material** 资产回收（现状：保留 1 个缓存句柄） | ⚠️ 待裁定 |
| AC9 | `PassId::GridLines` 表值与 `sync_grid_lines` 实际行为**互不否定** | ❌ |
| AC10 | 1px 线在 HiDPI（DPR 2）下不变 0.5px 消失 | ⚠️ 待实现后验证 |
| AC11 | 粗档线只落在格索引 `≡ 0 (mod m)`，`m` 为 2 的幂（不破坏格定位） | ⚠️ 待接口 |
| AC12 | 远观粗网格与等值线/海岸线叠色不更脏（笔迹仍可辨） | ⚠️ 待渲染帧 |
| AC13 | 4096² 栅格（`cell_size = 1.0`）在最远端 `scale = 8.0` 仍可达 ≥4px 粗档 | ✅（手算 + 断言） |

**汇总：✅ 4 条 / ⚠️ 5 条 / ❌ 4 条。** `blocked(D1,D3) + fail = 4 > 0` → 不得判过。

---

## 2. 独立复算（期望值的来源，逐条可复算）

**已知**（手算，不读代码常量）：

- `cell_size = 4096.0 / 1024 = 4.0` 世界单位/格
- `fit = max(4096/1280, 4096/800) = max(3.2, 5.12) = 5.12`（世界单位/屏幕像素）
- 世界级缩放带 = `[fit/32, fit×1.5625] = [0.16, 8.0]`（32× 与 1.5625× 是 ADR-0038 要求值）

**格子屏幕边长与可见性（三点手算）**

| scale | `cell_px = 4.0 / scale` | 要求（`<2px` 退场、`≥4px` 全显、其间线性） | 期望结论 |
|---|---|---|---|
| **5.12** | `4.0/5.12 = 0.78125` | `0.78 < 2` | `fade = 0` → **不可见**（细档合法退场 ⇒ 必须有粗档承接） |
| **2.0** | `4.0/2.0 = 2.0` | 恰在退场终点 | `fade = 0` → **不可见** |
| **1.0** | `4.0/1.0 = 4.0` | 恰在全显起点 | `fade = 1` → **全显** |

**由上式反解可见域**：可见 ⇔ `4.0/scale > 2` ⇔ **`scale < 2.0`**。世界带 `[0.16, 8.0]` 内 `scale ∈ [2.0, 8.0]` 全部无网格；按等比档数计 `log(8/2) / log(8/0.16) = 1.386/3.912 = 35%` 的档位无网格。**这就是用户「开了看不到」的量化解释。**

**粗档可达性（双档方案的设计约束）**

| scale | 细档 px | `m=8` 粗档 px | 粗档可见 |
|---|---|---|---|
| 0.16 | 25.0 | 200.0 | ✅ |
| 1.0 | 4.0 | 32.0 | ✅ |
| 2.0 | 2.0 | 16.0 | ✅ |
| 5.12 | 0.78125 | **6.25** | ✅ |
| 8.0 | 0.5 | **4.0** | ✅（恰在 4px 起点） |

→ 1024² 栅格的世界级只须 `m = 8`。**但 4096² 栅格（`cell_size = 1.0`）在 `scale = 8.0` 时细档 `0.125px`，须 `m = 32` 才到 4px** ⇒ 粗档倍率上限必须 ≥ 32 且为 2 的幂。

**现行网格成本（用于对比，非判据）**

- 1024² 栅格细档：`2 × (1024+1) = 2050` 条链 → 4100 个四边形
- `m = 8` 粗档：`2 × (1024/8+1) = 258` 条链 → 516 个四边形（≈1/8）

---

## 3. 测试用例与期望值（期望值一律写字面量，不写 `== GRID_XXX`）

配套独立测试（新增文件，不改实现）：

- `crates/wv-world-cell-map/tests/acceptance_grid_visibility.rs`（纯函数层，`cargo test -p wv-world-cell-map --test acceptance_grid_visibility`）→ **5 passed / 0 failed**
- `crates/wv-render/tests/acceptance_grid_visibility.rs`（ECS 层，`cargo test -p wv-render --features app --test acceptance_grid_visibility`）→ **4 passed / 3 failed（失败的 3 条即缺陷证据）**

| 用例号 | 模块 | 步骤 | 期望结果（字面量） | 优先级 | 现状 |
|---|---|---|---|---|---|
| A1 | 淡出纯函数 | `grid_line_fade(4.0/5.12)` / `(4.0/2.0)` / `(4.0/1.0)` | `0.0` / `0.0` / `1.0`，且 `cell_px` = `0.78125` / `2.0` / `4.0` | P0 | ✅ pass |
| A2 | 淡出边界 | `fade(3.0)`、`fade(2.01)`、`fade(2.0)`、`fade(4.0)`、`fade(0.5)`、`fade(100.0)` | `0.5`、`0.005±1e-4`、`0.0`、`1.0`、`0.0`、`1.0` | P0 | ✅ pass |
| A3 | 可见域 | `fade(4.0/5.12)`、`fade(4.0/1.99)`、`fade(4.0/2.0)`、`fade(4.0/8.0)` | `0.0`、`>0`、`0.0`、`0.0` | P0 | ✅ pass（缺陷证据：fit 档为 0） |
| A4 | 粗档可达性 | 对 `scale ∈ {0.16,1,2,5.12,8}` 算 `m=8` 粗档 px 并取 fade | 全部 `≥4.0` 且 `fade=1.0`；`1.0/8×32 = 4.0` 可达 | P0 | ✅ pass |
| A5 | 成本口径 | `grid_chains(1024,1024)` vs `grid_chains(128,128)` | `2050` 条 / `258` 条 | P1 | ✅ pass |
| NEG-1 | 开关 | 开关关 → update | 实体 `0`，`Assets<Mesh>` `0` | P0 | ✅ pass |
| NEG-2 | 图层 | 地形层隐藏（开关开）→ update | 实体 `0`，`Assets<Mesh>` `0` | P0 | ✅ pass |
| NEG-3 | 回收 | scale 1.0 建好 1 实体 → 关开关 → update | 实体 `0`、mesh 资产 `0`、material 资产 `≤1` | P0 | ✅ pass |
| CTL-1 | 阳性对照 | scale 1.0 连跑 4 帧 | 实体 `1`；4 帧内 handle 不变、mesh 资产 `1` | P0 | ✅ pass |
| REQ-1 | 可见性 | scale `5.12` → update | 实体 `1` | P0 | ❌ fail（实得 `0`） |
| REQ-2 | 全带可见 | 12 档 `scale_i = 0.16×50^(i/11)`，逐档 update | 无一档实体为 0 | P0 | ❌ fail（4 档为 0：`2.7525 / 3.9281 / 5.6058 / 8.0`） |
| REQ-3 | 重建计数 | 12 档逐档推进，统计有实体时 mesh handle 变化次数 | `≤ 2` | P0 | ❌ fail（实得 **7**） |

**「等接口」（需要主程序补的公开接口，本单不伪造）**

1. `grid_chains_strided(width, height, stride, half_w)`（或等价）：粗档链装配。现状只能靠 `grid_chains(降采样幅面)` 间接证明数量级（A5）。
2. **粗档倍率选择函数**：`coarse_multiplier(cell_px) -> u8`（2 的幂，最小满足 `m × cell_px ≥ 4`，上限 ≥32）。AC11 的对齐断言依赖它。
3. 若采用 shader 线宽：需要可断言的线宽口径（如 `grid_screen_width_px(view) -> f32`），否则 AC10 只能靠截图人工判。

---

## 4. 误伤评估（修网格可见性会不会引入新问题，逐条给判据）

| # | 风险 | 判据（可复跑 / 可检查） | 现状 |
|---|---|---|---|
| ① | 远观粗网格与等值线/海岸线叠色更脏，把笔迹彻底淹没 | 粗档线宽仍为屏幕 1px、alpha 与细档同源（`0.35 × layers.terrain.opacity × fade`）；z 序仍在底图之上、等值线描边之下（`0.03 < 0.04`）。**必须**提供远观（`scale 5.12`）与近观（`scale 1.0`）各 1 帧实际渲染图，人工判「笔迹是否仍可辨」 | ⚠️ 待渲染帧 |
| ② | 1px 线在 HiDPI（DPR 2）下变成 0.5px 消失 | 若把 `half_w` 从 `0.5 × scale`（世界单位）改成常数世界单位（如 `0.5`），在 `fit 5.12` 下屏幕宽度只剩 `0.098px` → **必然消失**。判据：断言网格世界半宽 = `0.5 × view.scale`（⇒ 屏幕恒 1 逻辑 px）；并在 DPR 2 截图上确认线可见 | ⚠️ 待实现后验证 |
| ③ | 粗档降采样让「格定位」在奇数列对不齐 | 粗档线必须落在格索引 `≡ 0 (mod m)`，且 `m` 为 **2 的幂**（保证粗线是细线的子集、且中心对称）。反例：`m = 10` 或非 0 起点 → 作者按粗线定位会偏半格 | ⚠️ 待接口 2 |
| ④ | shader 线宽方案在 `fwidth` 为 0 的边缘（极远/极近）闪烁 | 极远 `scale 8.0`（细档 0.5px）与极近 `scale 0.16`（25px）两端各录 ≥30 帧，同 scale 相邻帧的图像差像素数 = **0**；`fwidth` 必须 clamp 下界（线长→0 时 `fwidth→0` 会除爆） | ⚠️ 仅当采用 shader 方案才适用 |

**重要提示（误伤评估结论）**：①②④ 的共同前提是「线宽仍由**屏幕像素**定义」。若主程序为达成 AC3（重建 ≤2）而把线宽改成「世界单位常量」或「按档烘焙」，则 ②（HiDPI 消失）与 ①（远观变脏）**必然**被触发。**AC3 与「1px 屏幕常量线宽」必须在同一方案里同时成立**——见 D3。

---

## 5. 边界与异常（负向断言 + 极值可达性）

**负向断言（不是「没报错」）**

- 开关关 → 实体数 `0`（NEG-1 ✅）；地形图层隐藏 → 实体数 `0`（NEG-2 ✅）。
- 关闭后 `Assets<Mesh>` = `0`（NEG-3 ✅）；`Assets<ColorMaterial>` ≤ `1`（现状恰为 `1`：`overlay.material` 是**跨重建复用**的缓存句柄，不是每帧增长 → AC8 ⚠️ 请制作人裁定「缓存 1 个句柄」是否可接受）。

**极值可达性（上限必须用极值跑一遍，不是估计）**

- 12 档 × 来回：本单跑的是「12 档单向推进」（`0.16 → 8.0`）。**来回各 12 档**尚未跑（REQ-3 已红，来回跑重复得同一结论，等修复后再补双向）。补件要求里已列入。
- 4096² 栅格最远端：`cell_px = 1.0/8.0 = 0.125`，`m=32 → 4.0px` → 可达（A4 ✅，字面量断言）。
- 幅面极值：`tex.size == 0` 时 `sync_grid_lines` 走 `despawn` 分支（1367-1370），`build_stroke_mesh` 返回 `None` 时也 despawn（1374-1378）——两条都是「宁可无网格也不插空 mesh」（bevy #24874 保护），✅ 与实现一致。

---

## 6. 性能判据口径（硬性）

- **一律用计数器**：重建次数、实体数、asset 数、12 档内 handle 变化次数。本单 REQ-3 用的就是「handle 变化次数 = 7」，可复跑、跨机器同值。
- **墙钟毫秒只作参考值记录，不作判据**——debug 构建（`wv-render` / `wv-world-cell-map` 全栈 opt-level 0，见采信矩阵 #8）的绝对毫秒没有可比性。
- 本轮我**没有**记录任何墙钟数字：无头环境未起窗口，任何帧时间都不代表用户机器上的 FPS。AC2 因此判 ⚠️ 未覆盖。
- 若要给 FPS 一个可验收的替代口径：`scene 内 mesh 重建次数 + DP 重跑次数` 两个计数器降到 0（静止视野）/ ≤2（12 档），**且**真人试玩录屏中 zoom 全程无可感知卡顿。二者缺一不可。

---

## 7. 一致性与可达性检查

**一致性（表 vs 行为，互不否定）**

- `recipe.rs:283` Topographic 行 `PassId::GridLines = false`（注释「分析视图不要编辑格」），但 `sync_grid_lines` **不读** `RenderRecipe`（1315-1326 只读 `layers.terrain.visible` + `view.grid_lines`），而 Topographic 下 `LayerFrame.terrain.visible` 为真 ⇒ **表说「不要」，行为是「要」**。二者互相否定 → AC9 ❌（D4）。
- 门禁接线核查（对应「一个写了但没接进门禁的守卫等于没有守卫」）：`sync_grid_lines` **确实**被注册进执行链 —— `wv-app/src/app_bootstrap.rs:363`，且排在 `sync_grid_config`(350) 之后、`sync_terrain_chunks`(365) 之前。（`grep -rn "sync_grid_lines"` 复核）。✅ 这条不是缺陷，写入本单是为了留证：网格不显示**不是**「系统没接上」造成的，是淡出判定的问题。
- 附带发现（建议级）：`PassId::ALL` 25 条中**有 8 条没有任何 recipe 消费点**：`GridLines / StreetLabels / SeaLabels / Measurement / BrushCursor / Tracks / EventDots / DrillMarkers`（`grep -rn "\.enabled("` 枚举，18 个有消费者）。这是系统性「表驱动只做了一半」，不是本轮新增，但会持续制造「表改了行为没变」的错觉。

**可达性（上限能不能真的达到）**

> AC3「12 档重建 ≤ 2」与「线宽 = 屏幕常量 1px」在**现行结构**下数学互斥：
>
> - 几何里烘焙的半宽是 `half_w = 0.5 × view.scale`（世界单位，1372 行）——**它就是 scale 的函数**。
> - 12 档跨 `8.0/0.16 = 50×`。若键内保留 `scale` ⇒ 每档重建（实测 7 次（只有 8 档可见））；
> - 若键内去掉 `scale` ⇒ 任何 scale 变化都不重建 ⇒ 半宽固定 ⇒ 屏幕线宽在 50× 范围内从 `1px` 漂到 `50px` 或 `0.02px` ⇒ 违反「1px 屏幕常量」。
> - **唯一出路**：几何用**格单位**烘焙（与 scale 无关）+ 每帧用 `Transform::scale` / 材质 / shader 施加屏幕线宽 ⇒ 键内不再有 scale ⇒ 12 档重建 = **0 ≤ 2**，且 1px 屏幕常量仍成立 → **可达**。
>
> 结论：AC3 不是「调小重建频率」能达成的，必须**把 scale 从网格几何里剥出去**。这是本单最重要的一条裁定（D3）。

---

## 8. 缺陷清单

| 编号 | 描述 | 复现步骤 | 级别 | 建议处理 |
|---|---|---|---|---|
| **D1** | 世界级 fit 缩放下网格整层退场：`cell_px = 0.78125 < 2` → `fade = 0` → `despawn`。用户症状 2 的直接原因。12 档中 4 档无网格 | 开工具条「网格」→ 世界图默认视野（`scale 5.12`）→ 看不到任何格线。复跑：`cargo test -p wv-render --features app --test acceptance_grid_visibility req_world_fit` | **阻断** | 实现双档网格：细档（现行）+ 粗档（`m ∈ {1,2,4,8,16,32}`，取最小满足 `m·cell_px ≥ 4`）；`<2px` 只退细档，粗档继续画 |
| **D2** | 每次缩放档整层重建：`GridKey` 含 `view.scale.to_bits()`（1345）→ 每档新键 → 2050 链/4100 四边形重建；叠加 `tol_quantum`(1/64) 使等值线 DP 每档几乎必重跑；`wv-render`/`wv-world-cell-map`/bevy 全栈 opt-level 0。**用户症状 1 的候选归因，未实测帧率** | 滚轮连滚 → 观察 `Assets<Mesh>` handle 每次都变（REQ-3：12 档 7 次） | **严重**（归因待实测） | 见 D3 的结构改造；DP 侧把 `tol_quantum` 提到 1/16 或按 1.5× 档步量化；性能结论必须由真人录屏背书 |
| **D3** | 「12 档重建 ≤2」与「1px 屏幕常量线宽」在现行结构下**数学互斥**（推导见第 7 节）；不改结构则 AC3 永不可达 | 试把 `scale_bits` 从键里删掉 → 线宽在 50× 范围内漂移（`half_w` 是 scale 的函数） | **严重** | 几何以格单位烘焙 + `Transform::scale`/材质/shader 施加屏幕线宽；键内去掉 scale，改为「重建次数 = 幅面/格径/主题变化次数」 |
| **D4** | `PassId::GridLines` 表值（Topographic `false`）与 `sync_grid_lines` 行为（不读 recipe，照画）**互相否定** | Topographic 模式 + 开「网格」→ 网格照常出现，与表注释「分析视图不要编辑格」冲突 | **一般** | 二选一：① 让 `sync_grid_lines` 消费 `recipe.enabled(PassId::GridLines)`；② 把 Topographic 行改 `true` 并在注释里声明「本 pass 无 mode 轴」。**不许两边都留着** |
| **D5** | 表驱动只做一半：25 个 pass 中有 8 个无 recipe 消费点（`GridLines/StreetLabels/SeaLabels/Measurement/BrushCursor/Tracks/EventDots/DrillMarkers`） | `grep -rn "\.enabled(" crates \| grep -o "enabled(PassId::[A-Za-z]*" \| sort -u`（17 个真实消费点 + 1 个 `PassId::X` 占位） | **建议** | 加一条表驱动测试：「每个 `PassId` 要么有消费点，要么显式标注 `// 无消费者`」。本轮不阻塞 |
| **D6** | `GridLinesOverlay::despawn` 用 `if let (Some(entity), Some(mesh)) = (self.entity.take(), self.mesh.take())`：两值都会先 `take()`，但任一为 `None` 时**实体不会被 despawn** → 潜在实体泄漏（当前状态机不会产生该组合，属脆弱） | 代码审查（`grid_lines.rs:67-75`）；构造 `entity=Some, mesh=None` 即泄漏 | **建议** | 改成先 `if let Some(e) = self.entity.take() { ... }`，mesh 单独回收 |
| **D7** | 测试陷阱：`GridConfig::default().cell_size = 8.0`，与 `sync_grid_config` 实算的 `4.0` 不同口径。任何用缺省资源算期望值的测试会把每个数算错 2× | `GridConfig::default()` 见 `wv-world-cell-map/src/state.rs:74-84` | **建议** | 手算常量写死（本单做法），或把 `Default` 的 `cell_size` 注释标成「仅占位，不代表任何真实图」 |

> **不计为缺陷的设计主张差异**：`GRID_FADE_START/END = 4px/2px` 的取值本身（需求只说「远观要淡出」，没定阈值）。我不主张改它，只要求「退场时必须有承接档」（D1）。

---

## 9. 验收结论

**不通过。**

- 阻断 1 条：**D1**（世界级视野下网格完全不可见 = 用户症状 2 未解决）。
- 严重 2 条：**D2**（缩放重建风暴，归因待实测）、**D3**（AC3 与屏幕常量线宽结构互斥）。
- 一般 1 条：**D4**（recipe 表与行为互相否定）。
- 三态汇总：**✅ 4 / ⚠️ 5 / ❌ 4**，`blocked + fail = 4 > 0`。

**关于用户症状 1（FPS 个位数）的诚实说明**：
本单**未能验证**该症状是否消失。自动化测试只能证明「没崩」「计数器是多少」，证明不了「跟手、不卡」。
本轮全部证据来自无头 ECS 测试（无窗口、无 GPU 呈现、无试玩）。**任何「FPS 已恢复」的结论文档若不附真人试玩录屏（连续 zoom in/out 全带、单手滚轮、含首帧），一律不予采信。**

**退回给**：主程序（lead-programmer）。补件要求（三条，缺一不可）：

1. **双档网格**：细档保留、粗档按 `m = min{2^n : 2^n × cell_px ≥ 4}` 且 `n ≤ 5`（上限 32）选择；`m` 必须是 2 的幂，且粗线落在格索引 `≡ 0 (mod m)`（AC1/AC4/AC11）。
2. **把 scale 从网格几何里剥出去**：几何以格单位烘焙，屏幕线宽由 `Transform`/材质/shader 施加；重建键内不再含 `view.scale`，改为幅面/格径/原点/主题（AC3、D3）。
3. **表与行为对齐**：`PassId::GridLines` 二选一（消费 recipe 或删分歧），并给 `PassId::ALL` 补「有消费者 / 显式无消费者」的表驱动测试（AC9、D5）。

修复后本单按「复签」流程重跑：AC 编号沿用，逐条给实测证据 + 上轮 7 条缺陷的当前状态（已修/未修/回归重开）+ 本轮新增缺陷 + 可交付判定。
