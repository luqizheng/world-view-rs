# 网格可见性 × 缩放帧耗时 —— 先测后改报告 v0.1

> 日期：2026-10-23 · 作者：主程序（寇豆码）· 迭代档缺陷修复（**非新功能**）
> 对应两个用户症状：① cell-map 里 zoom in/out 只有个位数 FPS；② 工具条开了「网格」却看不到网格。
>
> 本文是**判据**不是说明文：每个数字都可复跑（§1 给出命令），
> 每个阈值都写了「为什么是这个值」，每个取舍都写了被放弃的方案与理由。

---

## 一、实测数字（A 项：先测后改）

### 1.1 探针与跑法

探针落在 `crates/wv-world-cell-map/src/terrain_view/perf_probe.rs`
（`#[ignore]` 测试，不进常规闸门；输入是完全确定的正弦起伏 1024² 地形，
无随机 / 无磁盘 / 无 GPU，两台机器上可逐值复算）。

```bash
# debug（改动前 opt-level 0 / 改动后见 §2）
cargo test -p wv-world-cell-map --lib -- --ignored --nocapture perf_probe_all
# release
cargo test -p wv-world-cell-map --lib --release -- --ignored --nocapture perf_probe_all
```

口径：每项暖机 1 次后跑 3 次取**最小值**（抖动全是外来噪声，取最小 = 最好的
机器状态下这个操作要多久）。规模按用户现场取：`SIDE = 1024`（lod-demo 的
raster 尺寸）、`cell_size = 4.0`（`extent 4096 / 1024`）。

### 1.2 数字表（改动**前**的档位：`[profile.dev]` 缺省 = opt-level 0）

| 热点 | 计入现场 | debug | release | debug/release |
|---|---|---|---|---|
| `grid_chains(1024,1024)` + `build_stroke_mesh` 全套 | 旧 `sync_grid_lines` 每档重建 | **0.74 ms** | **0.15 ms** | **4.9×** |
| `assemble_strokes`（DP 全链，234 链 / 64700 点） | `sync_isoline_overlay` 每跨档 | **15.56 ms** | **3.92 ms** | **4.0×** |
| `terrain_pixels(1024²)` 整图合成 | 换模式 / 换主题 / 换色 | **581.24 ms** | **16.64 ms** | **34.9×** |

### 1.3 这张表说了什么（以及**没**说什么）

1. **DP 是缩放路径上最贵的单项**：15.56ms/debug 一帧就吃掉 60fps 预算的全部
   还有余。它每「跨容差档」触发一次——旧量化是 1/64 格，滚轮一档×0.88 必然
   跨档，所以近观缩放时几乎**每档**都要付这 15.6ms。D 项把它降到 12 档 4 次。
2. **整图合成是 debug 档位最惨的受害者（34.9×）**：581ms 是「换模式 / 换主题
   几秒不动」的直接来源。它**不在**缩放热路径（键不含 scale），所以不是
   「缩放个位数 FPS」的成因，但它是 B 项（构建档位）最有力的单个证据。
3. **⚠️ 事实纠正：`grid_chains + build_stroke_mesh` 只有 0.74ms/debug**。
   也就是说「网格逐档重建」**不足以**解释个位数 FPS——它每档 0.74ms，即使
   每帧都重建也只占 16.7ms 预算的 4.4%。本轮仍然按 C 项把它改成零重建
   （消除该路径 + 顺带修可见性），但**个位数 FPS 的真实主嫌疑另有其人**：
   `sync_terrain_chunks` 每帧按视口 AABB 增删 chunk（缩放时视口每帧都变 →
   每帧 spawn/despawn mesh + material + 图像切片拷贝，未在本轮实测范围内）。
   已列入 §5「留待下轮」，不猜数字、不顺手改。

---

## 二、构建档位（B 项 · P0 最便宜的一刀）

### 2.1 决策

`Cargo.toml` 新档位（原文见该文件注释）：

| 目标 | opt-level | 覆盖范围 | 理由 |
|---|---|---|---|
| `[profile.dev]` | **1** | 工作区成员的默认档 | 成员代码以搭线/状态机/UI 为主，收益大部分拿到，增量编译代价最小 |
| `[profile.dev.package."*"]` | **2** | 全部第三方依赖（bevy / wgpu / egui / image…） | 依赖极少改动，一次编译长期受益；启动/首帧性价比最高的一档 |
| `wv-world-cell-map` | **3** | 整图合成 / 网格 / 等值线几何 | 逐 cell 紧循环，向量化收益显著，体量小重编快（热点） |
| `wv-terrain` | **3** | marching squares / DP | 同上（纯算法） |
| `wv-render` | **3** | 桥接层逐帧循环 | 同上（逐帧消费点） |
| `wv-app` | 2（保留） | 启动链主干 | 不被通配覆盖；启动期敏感的保险，见下 |

### 2.2 两处「同一事实两个载体」的清理

- **删 `[profile.dev.package.image] opt-level = 2`**：已被 `package."*"` = 2
  完全覆盖。留着只会出现「改通配忘了改单条 → 单条赢 → 排查先怀疑错地方」。
- **保留 `wv-app = 2` 并写明理由**：它**不**被通配覆盖（Cargo 的 `*` 只覆盖
  依赖、不覆盖工作区成员），而 `[profile.dev]` 只给 1。启动卡顿的原始诊断
  （纹理合成 + PNG 解码 >5s → Windows 白屏）里，合成已迁去 `wv-world-cell-map`
  （3）、解码归 `image`（通配 2），但 wv-app 仍是启动链主干，2 是廉价保险；
  日后实测启动不敏感可降回 1。

### 2.3 预期效果（按 §1.2 外推，非实测墙钟）

| 路径 | 改动前（opt 0） | 改动后（opt 3） | 依据 |
|---|---|---|---|
| DP 跨档 | 15.56 ms | ≈3.9 ms（release 数） | 热点 crate = 3 |
| 整图合成 | 581 ms | ≈16.6 ms（release 数） | 热点 crate = 3 |
| bevy/wgpu 自身每帧开销 | 未优化 | opt 2 | 通配档 |

> 施工后**没有**再测一次「debug + 新档位」的对照：新档位下 wv-world-cell-map
> 已是 opt 3，其数字应≈上表 release 列（两者对本 crate 都是 opt 3，差别只有
> `debug_assertions`），故不再花一次全工程重编（约 2 分钟）换一个可预测的数字。
> 若 QA 要独立复核，命令同上，`cfg!(debug_assertions)` 会打印在探针末行。

---

## 三、网格（C 项：既要看得见，也要不再逐档重建）

### 3.1 根因（三重）

1. **整层退场**：旧 `sync_grid_lines` 在 `grid_line_fade(cell_size / scale) <= 0`
   时 `despawn`。世界级「全图适配」`scale = 5.12`、`cell_size = 4.0` →
   格距 `0.78px` < `2px` → fade = 0 → **一进 CellMap 就没有网格**。
   要看到得放大到 `scale < 2.0`（即拟合后再放大 2.6×）。
2. **逐档重建**：`GridKey` 含 `scale_bits`（`view.scale` 精确位比较）+
   线宽是世界宽度（`px × scale`）→ 滚轮每档都 despawn + 建 2050 条链 +
   上传全量网格 + `Assets::remove`。
3. **材质泄漏**：旧 `despawn` 只回收 mesh，`ColorMaterial` 句柄留着，
   「开关网格 N 次」= N 份死资产。

### 3.2 新模型：一个 quad + 屏幕空间 shader，双档网格

```
CPU（每帧）                      GPU（每像素）
────────────                    ────────────
gate: 开关 ∧ 图层 ∧ recipe
key:  size/height/cell/origin/palette   ──▶ 结构变才重建 mesh（1 个 Rectangle）
uniform: 墨色 + 基色 alpha              ──▶ 换色/拖透明度才写
（缩放：什么都不做）                     fwidth(cell) ─▶ 格距像素 ─▶ 细档/粗档
```

- **线宽与格距都由 `fwidth` 推出**：`cell = (world - origin) / cell_size`，
  `fwidth(cell.x)` 就是「一格占几个像素」的倒数。于是 1px 屏幕线宽天然成立，
  **缩放不需要任何 uniform 更新**（连 `view.scale` 都不进 shader）。
- **双档**（`grid_lines.wgsl` + `grid_lines.rs` 的同名纯函数）：
  - 细档：每格一条，格距 ≥ [`GRID_FINE_FULL_PX`]（4px）全显、≤
    [`GRID_FINE_MIN_PX`]（2px）退出；
  - 粗档：**不再整层退场**，改画「每 `stride` 格一条」，`stride` 取最小
    **2 的幂** 使屏上定位格 ≥ [`GRID_COARSE_MIN_PX`]（**4px**，见 §3.3 的订正）；
  - 某像素取两档较强者：`alpha = max(m_fine × fine, m_coarse × 0.85) × 基色`。

### 3.3 阈值与数字来源（禁裸数字）

> **2026-10-23 订正（N11）**：`GRID_COARSE_MIN_PX` 在二次定标时由 **8.0 → 4.0**、
> `GRID_COARSE_MAX_EXP` 由 **6 → 5**（≤32×）。原因：可达参数统一为
> `m = min{2^n : 2^n × cell_px ≥ 4}`（v2 复签的判据），**且用户已在 4.0 的实机上
> 目视验收过网格**（「正确」「粗细两档还能接受」）。下表与 §3.3 的手算锚点已按
> 4.0 重算——此前写 8.0 的版本会让任何人「按文档算期望值都错一倍」。
> 代码真源：`crates/wv-world-cell-map/src/terrain_view/grid_lines.rs:81-95`。

| 常量 | 值 | 为什么是这个值 |
|---|---|---|
| `GRID_LINE_BASE_ALPHA` | **0.48** | 旧 0.35 在 1px + 淡出双重削弱下读不出（症状成因之一）；0.48 落在验收要求的 0.45–0.5 中部——再高与等值线墨色抢视线，再低回到看不见 |
| `GRID_COARSE_ALPHA_K` | **0.85** | 粗档屏上格距恒 ≥4px，本不需淡出；0.85 是「骨架比细档轻一档」的表态，远观不压地形 |
| `GRID_LINE_PX` | **1.0** | `fwidth` 的分辨率下限就是 1px；更宽要 SDF，收益不抵复杂度 |
| `GRID_FINE_FULL_PX` | **4.0** | 1px 线 + 1px 隙的最小可辨周期 = 4px（沿用 2026-10-15 实测判断） |
| `GRID_FINE_MIN_PX` | **2.0** | 1px 线在 2px 格距下已占去一半面积，细线只剩摩尔纹 |
| `GRID_COARSE_MIN_PX` | **4.0**（原 8.0） | 4px 正是细档的「全显」锚点（`GRID_FINE_FULL_PX`）：细档在格距 4px 时读得最清、再密就淡出，粗档接过它——两档在**同一可读性口径**上交接，不会出现「细档已淡、粗档还稀」的空档。用户已目视验收 4.0 |
| `GRID_COARSE_MAX_EXP` | **5**（≤32×，原 6/64×） | 4096² 栅格（`cell_size 1.0`）在 `scale 8.0` 时细档 0.125px，按 4px 下限只需 **32×** → 4.0px（恰好可达）；再大意味着「一格已小到看不见」，粗档也失去定位意义 |

手算锚点（写进单测；**按 4px 下限重算**，2026-10-23 订正）：

| `scale` | `cell_px = 4.0/scale` | 细档 fade | 粗档 stride | 屏上定位格 |
|---|---|---|---|---|
| 8.0 | 0.5 | 0 | 8 | 4.0px |
| 5.12（fit） | 0.78125 | 0 | **8** | **6.25px** |
| 2.0 | 2.0 | 0 | 2 | 4.0px |
| 1.0 | 4.0 | 1.0 | 1 | 4.0px |
| 0.25 | 16.0 | 1.0 | 1 | 16.0px（纯细档） |

### 3.4 为什么放弃/保留哪些方案

- **采用 `Material2d` + WGSL（内联 shader）**：`fwidth` 是唯一能同时做到
  「1px 屏幕线宽」「零 uniform 更新」「双档无缝切换」的机制；CPU 三角网做不到
  屏幕常量线宽而不随 scale 重建。
- **shader 走 `load_internal_asset!` 内联，不用 `ShaderRef::Path`**：
  路径由 AssetServer 相对 **asset 根**解析，而 asset 根随运行目录变化
  （example 与 app 不同，`terrain_field_demo` 已踩过）。网格是 CellMap 常驻层，
  不能因「资产根没配好」静默不画。UUID 句柄一旦发布不得改。
- **阈值全部走 uniform（`tune` / `mix`），shader 侧零裸字面量**：Rust 常量是
  唯一真源；单测 `wgsl_reads_every_threshold_from_uniform` 钉住「shader 必须
  从 uniform 读，且不得出现 Rust 常量名」。
- **`grid_chains` / `build_stroke_mesh` 保留**（已不进渲染路径）：探针（§1）
  与 QA 的链数验收（2050 / 258）都要用它；下轮如确认无消费者可删。

### 3.5 一致性收口：recipe 说真话

旧 `sync_grid_lines` **完全不读** `RenderRecipe`，而 `MODE_TABLE` 里
`ViewMode::Topographic` 行的 `PassId::GridLines` 写 `false`——表在骗人
（CellMap 工作台里网格照画）。本轮二选一，取**「让它读 + 表改 true」**：

- 理由：用户在 CellMap（= Topographic）里明确要用网格定位格子（本轮症状 2
  的诉求就是这个模式）；删掉这条 pass 等于放弃「模式级」控制权，其它模式
  将来想统一关网格就没地方写了。
- 系统 gate 现在是 `layers.terrain.visible ∧ view.grid_lines ∧
  recipe.enabled(GridLines)`。
- 表驱动测试 `recipe_table_says_on_iff_grid_actually_draws`：对全部 8 个
  `ViewMode` 求 `resolve_render_recipe` → 「表说开」必须等价于「系统真出实体」。

---

## 四、等值线跨档（D 项：不再每档重算 DP）

### 4.1 旧量化错在哪

`tol_quantum(scale) = (dp_tol_continuous(scale) × 64.0) as u8`（1/64 格步长）：

1. **一档必跨**：近观带 `tol = 0.5 × scale`，滚轮一档 ×0.88 → 量化值必减
   2–4 → 每档都重跑 DP + 建 mesh（§1.2 实测 15.56ms/debug）。
2. **`as u8` 绕回**：`scale > 8.0` 时 `tol × 64 = 32 × scale > 255`，
   `as u8` 按 256 取模 → 档值反而变小（`scale 12 → 128`），键非单调。

### 4.2 新模型：容差的 1/2 octave 对数档

```rust
dp_tol_bucket(scale)  = round(log2(dp_tol_continuous(scale)) × 2)   // 桶
dp_tol_for_bucket(b)  = 2^(b / 2)                                    // 档中心容差
```

- 几何**只用档中心容差**（不再用连续式）：否则键说「同档」而容差随 scale 漂移。
- 整条缩放范围只有 **5 个档**（tol 钳在 `[0.125, 0.5]` → `log2 ∈ [−3, −1]` →
  `b ∈ [−6, −2]`），相邻档差 ×1.414。
- 为什么 1/2 octave 而不用 1 octave：×2 一档时近观能看到线条细节成片增减
  （「线忽然变直」）；×1.414 一档的几何差在半像素量级读不出来，而重算次数
  只翻倍——12 档实测 4 次，仍有余量。

### 4.3 缩放 12 档的重算次数对比（起点 `scale 1.0`，每档 ×0.88）

| 步 | scale | tol = 0.5×scale | 旧 1/64 量化 | 新 1/2 octave 档 | 旧触发 | 新触发 |
|---|---|---|---|---|---|---|
| 0 | 1.000 | 0.500 | 32 | −2 | — | — |
| 1 | 0.880 | 0.440 | 28 | −2 | ✔ | |
| 2 | 0.774 | 0.387 | 24 | −3 | ✔ | ✔ |
| 3 | 0.682 | 0.341 | 21 | −3 | ✔ | |
| 4 | 0.600 | 0.300 | 19 | −3 | ✔ | |
| 5 | 0.528 | 0.264 | 16 | −4 | ✔ | ✔ |
| 6 | 0.464 | 0.232 | 14 | −4 | ✔ | |
| 7 | 0.409 | 0.204 | 13 | −5 | ✔ | ✔ |
| 8 | 0.360 | 0.180 | 11 | −5 | ✔ | |
| 9 | 0.316 | 0.158 | 10 | −5 | ✔ | |
| 10 | 0.279 | 0.139 | 8 | −6 | ✔ | ✔ |
| 11 | 0.245 | 0.125 | 8 | −6 | | |
| 12 | 0.216 | 0.125 | 8 | −6 | | |

**重算次数：旧 10 次 → 新 4 次**（第 2/5/7/10 步）。按 §1.2 折算：
`10 × 15.56ms = 155.6ms` → `4 × 15.56ms = 62.2ms`（debug）；
release 下 `10 × 3.92 = 39.2ms` → `4 × 3.92 = 15.7ms`。

> 远观带（`scale ≥ 1.0`）两者都是 0 次——`tol` 已钳在上限 0.5，压缩空间在
> 近观带（`scale < 1.0`），也就是作者实际逐格编辑的那一档。单测
> `twelve_zoom_steps_trigger_at_most_four_recomputes` 把上表钉成断言。

---

## 五、验收与开放项

### 5.1 断言（QA 独立复核用）

| # | 断言 | 落点 |
|---|---|---|
| 1 | `scale = fit`（5.12）时网格实际可见 | `cellmap_bridge::tests::grid_alpha_stays_positive_at_fit`：实体 == 1 ∧ uniform 基色 alpha > 0 ∧ 定位格 12.5px ≥ 8px |
| 2 | 12 档缩放 mesh 重建次数 ≤ 2（实测 **0**） | `twelve_zoom_notches_trigger_zero_rebuilds`：公开计数 `GridLinesOverlay::rebuilds` == 1（只首帧）且 mesh handle 不变 |
| 3 | 负向：开关关 / 图层隐 / recipe 关 → 实体 0 且 mesh+material 回收 | `grid_despawns_and_reclaims_both_assets` |
| 4 | 表说开 ⇔ 真开 | `recipe_table_says_on_iff_grid_actually_draws`（8 模式全跑） |
| 5 | 等值线 12 档 ≤4 次重算 | `isoline_overlay::tests::twelve_zoom_steps_trigger_at_most_four_recomputes` |

命令：

```bash
cargo test -p wv-world-cell-map -p wv-render           # 全绿
cargo test -p wv-render --features app --test acceptance_grid_visibility
cargo check -p wv-app --features app
```

### 5.2 GPU 冒烟（shader 真能在显卡上编译）

`cargo test` 是纯 ECS，抓不到 WGSL 的语法/语义错（写错一个 `fwidth` 参数，
测试全绿而画面空白）。故加一条 GPU 冒烟闸门：

```bash
cargo run -p wv-world-cell-map --example grid_material_smoke 2>&1 | tail -30
# 期望末行：GRID_SMOKE_OK frames=30，且全程无 naga / Shader validation 报错
```

实测（2026-10-23，NVIDIA RTX 4060 Laptop / Vulkan / bevy 0.19.1）：

```
INFO bevy_render::renderer: AdapterInfo { name: "NVIDIA GeForce RTX 4060 Laptop GPU", backend: Vulkan }
GRID_SMOKE_OK frames=30
```

- 管线建成 = WGSL 通过 naga 校验（含 `fwidth` / `log2` / `exp2` / `discard`）；
- 无 `array stride must be a multiple of 16` panic = uniform 布局（全 `Vec4`）正确；
- 冒烟相机用默认 2D 正交（1 世界单位 = 1px → 格距 4px、`stride` = 2），
  **细档与粗档两条分支都被走到**。

### 5.3 留待下轮（明确不做 / 未做）

1. **个位数 FPS 的真实主嫌疑**：`sync_terrain_chunks` 每帧按视口 AABB 增删
   chunk（缩放时视口每帧变化 → 每帧 spawn/despawn + 图像切片拷贝）。
   §1.2 已证「网格重建 0.74ms」不足以解释，需单独探针（本轮预算不含）。
2. **`examples/` 下的 YAML / demo 未动**（禁令），`terrain_field_demo`
   仍用 `ShaderRef::Path` 路径加载，未切内联。
3. **`grid_chains` / `build_stroke_mesh` 已成为「非渲染路径」的公共函数**：
   下轮确认无消费者后可删（或收进 `#[cfg(test)]`）。
4. **`cellmap_bridge` ↔ `wv-world-cell-map` 的重复代码归并**：按禁令另开 ADR。
5. **粗/细档交界处的 popping**：stride 跳变（如 8→16）时粗线会「跳一格」。
   当前用 2 的幂保证落格缘、跳变不常见（一档 ×2 才跳一次）；若要平滑需
   在 shader 里做两档混合，本轮不做。
6. **像素级截图验收未做**：§5.2 只证「管线建成 + 30 帧无错」，没有落 PNG
   逐像素比对（`Screenshot` + 隐藏窗口的落盘路径还要另试）。视觉确认留给
   QA / 作者实机看一眼——判据是「世界级 fit 下能看到 12.5px 一格的粗网格」。
7. **打包 release 的启动时间/白屏**未复核（改动只碰 dev 档位，release 不变）。
8. **（2026-10-23 追记 · N9）线宽档化在世界带等比缩放下等于没档化**：轮廓线宽按
   `1/2 octave`（档宽 ×1.4142）量化，而世界带 12 档等比缩放每档 ×`50^(1/11)`
   = **1.427** > 1.4142 ⇒ **每档必跨**，`sync_region_outlines` 仍逐档重建
   （≈11 次；真实滚轮 ×0.88 时约 4–5 次，好于旧实现 12 次，但不是 0）。
   本轮**不修**（用户没报「群系视图 + zoom」这条路径）；下轮与
   `sync_terrain_chunks` 的「每帧整图 clone（lod-demo ≈67MB）」一起处理。
   若届时仍要修，最小改动是把档宽从 1/2 缩到 1/4 octave（重建翻倍但仍远小于旧值），
   或把线宽搬进材质 uniform（需把 `ColorMaterial` 换成自定义材质，影响面更大）。
