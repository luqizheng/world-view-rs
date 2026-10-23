# 光标热路径 × 群系视图 × 锚点残留 —— 先测后改 v0.2

> 日期：2026-10-23 · 作者：主程序（寇豆码，本轮第三实例）· 迭代档缺陷修复（**非新功能**）
> 前序：`docs/PERF_GRID_AND_PROFILE_v0.1.md`（测量表与档位）、
> `docs/reviews/2026-10-23-cellmap-zoom-grid-acceptance.md`（验收单）。
>
> 本文是**判据**不是说明文：每个数字都能用文中的命令复跑，每个阈值写了
> 「为什么是这个值」，每个取舍写了被放弃的方案。

---

## 一、实现范围与依赖

### 1.1 本轮四个症状（用户实测，新构建 / 新 opt 档 / 网格 shader 已生效）

| # | 症状 | 上一轮状态 | 本轮处置 |
|---|---|---|---|
| 1 | 鼠标在地图上移动 → **40 FPS**（静止 120） | 未测过 | **测得根因并修**（§2.2 B） |
| 5 | 开「群系视图」（`nature_overlay`）→ **26 FPS** | 未测过 | **修**（§2.3 C） |
| 6 | 关群系视图后地图上还留着**红色锚点** | 未测过 | **修**（§2.4 D，用户已裁） |
| 3 | zoom in/out ≈110 FPS | 上一轮已修（个位数 → 110） | **不动**（用户明示） |
| 4 | 网格粗细两档「还能接受」，粗格边 alpha 叠加**不修** | 用户裁定 | 不动 |

### 1.2 依赖与边界

- 依赖：`wv-render` / `wv-world-cell-map` / `wv-app`（`--features app`）；**零新第三方依赖**
  （计时用 `std::time::Instant`）。
- 不改：`Cargo.toml` 的 `[profile.dev]`（本轮已有 `debug = "line-tables-only"` 压体积，
  改回去会撑爆 D: 盘）；`examples/*/yaml`；`cellmap_bridge ↔ wv-world-cell-map` 的重复代码。
- 不做：`cargo test --workspace`（磁盘），一律按 crate 跑。

---

## 二、系统实现方案

### 2.1 A 项 · per-system 计时器（前置，先装才许改）

**职责**：把「鼠标移动掉到 40 FPS」这件事落到**具体 system 名**上。Bevy 自带的
`SystemInformationDiagnosticsPlugin` 只给进程级 CPU，没有 per-system 拆分。

**数据结构**（`crates/wv-render/src/perf.rs`）

| 项 | 类型 | 说明 |
|---|---|---|
| `PerfRecorder` | `Resource` | 开关、`open: HashMap<&'static str, Instant>`、本帧样本、5s 窗口聚合、全量累计 |
| `SLOW_MS` | `f32 = 4.0` | 单帧单 system 超它**立即**打印一行（= 60FPS 预算的 1/4，低于它的抖动会把真凶淹掉） |
| `WINDOW` | `Duration = 5s` | 每窗口打一次 top5 + 最差帧（够跨一次缩放/框选，又短到能把数字和「我刚做了什么」对上） |
| `TOP_N` | `usize = 5` | 窗口汇总打印条数 |
| `snapshot()` | `Vec<(name, n, max_ms, avg_ms)>` | 按 max 降序；**测试/QA 直接读**，不依赖日志插件 |

**关键流程**

1. 装配处 `init_resource::<PerfRecorder>()` + `add_systems(Last, perf::report_frame)`；
2. 每个 system 用宏包一层：`perf_timed!("name", some::system)`，展开成
   `(start(name), system, end(name)).chain()`；
3. `begin` 按**名字分槽**记录起点（不同链条可能交错执行，单一「当前槽」会被覆盖）；
4. `end` 做差、入本帧缓冲、累计；`report_frame` 在 `Last` 里：逐条打 `>=4ms`、聚合窗口、
   到时打 top5 与最差帧。

**对外接口**：`wv_render::perf::{PerfRecorder, report_frame, SLOW_MS, WINDOW, TOP_N}` +
`wv_render::perf_timed!`。**摘除方式 = 把宏调用换回裸 system**（被测系统签名一行没动）。

**关闭时的代价**：`begin` / `end` 各两次布尔判断直接返回，不取 `Instant`；但调度器仍要为
每个 system 多跑两个空 system（实测 <1µs/帧/对，全表 <0.1ms）。定标完成后整段摘掉。

**顺手清掉的两处上一轮残留**
- `crates/wv-render/src/vector_layers.rs:506-507`：同一个 `let __t = Instant::now();` 写了两遍 → 删；
- `crates/wv-render/src/world_projection.rs:640`：一个从未使用的 `let __t` → 删。

### 2.2 B 项 · 「鼠标移动 120 → 40 FPS」

**定位结论（实测，不是猜）**：落在 **`nature_region_actions::sync_candidate_input`**。

它每帧把光标换格当成「候选连通区变了」，于是每次换格都重做
**整张地形 → `Vec<TerrainCell>` 全量拷贝 → `TerrainGrid::from_cells` →
`biome_region_at`（内部再分配 1M `bool` 掩膜 + 4 邻域 BFS + 环提取）**。
鼠标静止 ⇒ 光标格不变 ⇒ 早退 ⇒ 120 FPS；鼠标一动 ⇒ 每帧付一次。

实测（`cargo test -p wv-app --features app --lib -- --ignored --nocapture perf_probe_cursor_cell_change`，
1024² 地形 = lod-demo raster，`cell_size = 4.0`）：

```
[PROBE] 旧路径单价：clone=0.63ms + from_cells=0.00ms + biome_region_at=1.20ms = 1.84ms/次   ← 碎片化群系
[PROBE] 旧路径单价（整张连成一片）：clone=0.73ms + from_cells=0.00ms + biome_region_at=12.01ms = 12.74ms/次（cells=1048576）
[PROBE] sync_candidate_input: n=31 max=2.14ms avg=0.07ms                                    ← 修后
```

`8.3ms（120FPS 基帧）+ 12.7ms ≈ 21ms ≈ 47 FPS`，与用户实测 40 FPS 同量级
（真实地形是混合：大片平原走 12.7ms、碎片化群系走 1.8ms）。

**职责**：把「光标格 → biome 连通区环」的派生从「每换一格全量重算」改成
「**换块才算，块内移动只做一次点包含判定**」。

**数据结构**（`crates/wv-app/src/edit/nature_region_actions.rs`）

| 项 | 内容 |
|---|---|
| `CandidateRingCache`（`Local`，`pub` —— 系统是 `pub`，`SystemParam` 类型必须同可见性） | `last_cell`、`key: Option<CandidateKey>`、`grid: Option<TerrainGrid>`、`rings: Vec<Vec<(f32,f32)>>`（外环+洞环，格角坐标）、`view_ring` |
| `CandidateKey` | `map` + `width/height/cells.len()` + `grid_bits[7]`（格尺寸 / 两套原点 / 幅面）+ `epoch`（消费 `CandidateOverlayCache::last_epoch`：history 形状 + dirty） |

**关键流程**

1. 算 `cursor_cell`（`cell_pick::pick_at`，O(1)）；
2. 算 `CandidateKey`；**变了** ⇒ 整份缓存作废 + `out.ring = None`；
3. 光标格没变 ⇒ 直接 return（原来就有这条）；
4. `cell_inside_rings(&cache.rings, gx, gy)`（even-odd 射线法，格中心判定）为真 ⇒
   **环一模一样**，`out.ring = cache.view_ring.clone()` 后 return（**这是 99% 的移动**）；
5. 真换块 ⇒ `grid` 惰性建一次（换图/编辑才重建），`biome_region_at` 一次，
   更新 `rings` / `view_ring`。

**对外接口**：`sync_candidate_input` 签名只多一个 `Res<CandidateOverlayCache>`，
输出 `CandidateRegionInput.ring` 语义不变（`None` = 光标不在地形上 / 无候选）。

### 2.3 C 项 · 「群系视图 26 FPS」

**根因（三条，都在 `crates/wv-render/src/region_outline.rs::sync_region_outlines`）**

1. **每帧展开全部区域多边形**成 `Vec<(Vec2, Vec2)>`（`push_loop` 每个环还先分配一个
   `Vec<Vec2>`）再算 `segments_fingerprint` —— 纯 O(全部区域边数)/帧；
2. **`OutlineKey` 含 `world_thickness = 1.6 × view.scale`** → 缩放每档换键 →
   `build_stroke_batch_mesh` 整批重建；
3. 候选预览每条环都重算 `segs` + 指纹（光标在同一候选区内移动时结果完全一样）。

**数据结构**

| 项 | 内容 |
|---|---|
| `RegionOutlinePool::source: Option<OutlineSource>` | `key: SegSourceKey` + `segs` + `fingerprint` |
| `SegSourceKey` | `grid_bits[7]` + `in_cellmap` + `region_count` + `geom`（**全部多边形裸 `f32::to_bits` 的 FNV，无分配、无坐标变换**） |
| `OutlineKey` | `fingerprint` + **`thickness_bucket: i16`** + `z`（`world_thickness: f32` 已删除） |
| `thickness_bucket(scale)` | `round(log2(scale) × 2)`，档宽 ×1.414；`scale_of_thickness_bucket(b) = 2^(b/2)` |
| `RegionOutlinePool::{seg_recomputes, stroke_rebuilds}` | **计数器**（QA 口径，跨机器同值） |
| `preview_ring / preview_segs / preview_fp` | 候选预览的输入环 + 展开结果 + 指纹缓存 |

**关键流程**

1. 每帧只做一次「裸位模式」`geom` 指纹（200 区域 × 40 点 ≈ 8k 次 xor/mul，十微秒级）；
2. 键未变 ⇒ **不展开 `segs`、不算线段指纹**；
3. 线宽按档取档中心烘焙（`1.6 × 2^(b/2)`，屏上宽度落在 1.6×[0.84,1.19] ≈ 1.34–1.90px，
   对一条 1.6px 的边界线读不出来）；**同档内连续几档缩放不再重建**；
4. `build_stroke_batch_mesh` 只在「内容指纹变 **或** 线宽档变」时调用，并计 `stroke_rebuilds`；
5. 候选预览：环内容不变 ⇒ `segs`/指纹/mesh 全不重算。

**为什么是 1/2 octave 而不是 1 octave**：×2 一档会让 1.6px 线在 0.8–1.6px 之间跳（肉眼可见
粗细跳变）；×1.414 的 ±19% 读不出来，而重建次数只翻倍。与上一轮等值线 `dp_tol_bucket`
同口径（同一套「缩放不该驱动几何重建」的判断，两处一致）。

**不做**：没有把线宽搬进 uniform / shader（那要改 `ColorMaterial` 为自定义材质，
影响面比收益大）；也没有做「档内线性插值」。

**对外接口**：`RegionOutlinePool` 新增 3 个 `pub` 计数器；`sync_region_outlines` 签名不变。

### 2.4 D 项 · 锚点残留（制作人已裁，照做）

**职责**：关掉 `nature_overlay` 时，**顶点手柄 + 阶梯描边 + 候选高亮一起隐藏**；
**选中态保留**（`projection.selection` 不动 → Inspector 仍显示选中的 Region，
不写世界、不清数据）。

**数据结构 / 接口**：纯函数
`wv_render::world_projection::region_handles_visible(mode, nature_overlay) -> bool`
（`Topographic ∧ nature_overlay`）。判定只看入参，**不读也不写 `selection`**。

**关键流程**：`draw_map_gizmos` 里两处（选中装饰 / 候选锚点）都改成过这道门。

**为什么把「阶梯描边」也算进去**：它与手柄同源同语义（都是「数据视图里的选中装饰」），
只留白阶梯线、去掉红锚点会变成同一族的半截状态。若制作人希望关 overlay 后仍显示阶梯描边，
把门控从 `if` 外面挪到 `draw_region_decor` 的手柄循环里即可（一行）。

### 2.5 B/D 的伴随修复：选中区域装饰缓存（`RegionDecorCache`）

**职责**：选中区域的「阶梯描边 + 顶点手柄 + 候选锚点」派生几何按**键**缓存。

**数据结构**（`crates/wv-render/src/world_projection.rs`）

| 字段 | 说明 |
|---|---|
| `key: Option<DecorKey>` | `(区域 id, 多边形位模式指纹, grid_bits[7], 幅面)` |
| `edges / pairs` | 阶梯边 + 多边形顶点（绘制输入） |
| `cand_key / cand_pairs` | 候选锚点的按格派生缓存（`(候选格, 多边形指纹, grid, 幅面)`） |
| `rasterizations` / `candidate_derivations` | **累计计数器**（QA：断言 1 的「选中区域栅格化 == 0」） |
| `handles_drawn` / `outline_segments_drawn` | **每帧归零**的绘制计数（QA：断言 2 的「手柄绘制 == 0」） |

**关键流程**：`selected_region_decor()`（派生，带缓存）与 `draw_region_decor()`（绘制）
拆开 —— 拆开的唯一理由是**可测**：不需要 `Gizmos` 就能断言「键不变 ⇒ 不再栅格化」，
见 `world_projection::region_handles_tests::selected_region_decor_is_derived_once_then_cached`。

**顺带**：候选锚点块（`selection` 为空时）原来**每帧**把 1024² 地形搬进 `TerrainGrid` +
`biome_region_at`；现在按 `(候选格, 多边形指纹)` 缓存。

---

## 三、工程组织

| 文件 | 动作 | 一句话 |
|---|---|---|
| `crates/wv-render/src/perf.rs` | **新增** | per-system 计时器 + `perf_timed!` 宏（可整段摘除） |
| `crates/wv-render/src/lib.rs` | 改 | `pub mod perf`（`#[cfg(feature = "app")]`） |
| `crates/wv-render/src/region_outline.rs` | 改 | `segs`/指纹源缓存 + 线宽档量化 + 3 个计数器 + 候选预览环缓存 + `steady_state_tests` |
| `crates/wv-render/src/world_projection.rs` | 改 | `RegionDecorCache`、`region_handles_visible`、派生/绘制拆分、候选锚点缓存、删残留 `__t` |
| `crates/wv-render/src/vector_layers.rs` | 改 | 删重复的 `let __t = Instant::now();`（506-507） |
| `crates/wv-app/src/edit/nature_region_actions.rs` | 改 | `CandidateRingCache` + `cell_inside_rings` + 探针测试（`#[ignore]`） |
| `crates/wv-app/src/app_bootstrap.rs` | 改 | `init_resource` 两个新资源、`Last` 挂 `report_frame`、主链 35 个 system 套 `perf_timed!` |
| `crates/wv-app/src/i18n/keys.rs` | 改（**代改，非本轮范围**） | `ALL: [I18nKey; 458]` → `461`（数组实有 461 条；不修则 `wv-app` 整 crate 编译失败） |

---

## 四、资产接入清单

本轮**无新资产**（不加贴图 / 音效 / 字体 / shader）。

| 资产类型 | 来源角色 | 接入方式 | 验证点 |
|---|---|---|---|
| —（无） | — | — | — |
| 日志通道 `wv_perf` | 主程序 | `bevy::log::info!(target: "wv_perf", …)` | `WV_PERF=1` 时能在 stdout 看到 `wv_perf` 行；不设则该 target 无输出 |

---

## 五、构建步骤

1. **编译渲染层**
   `cargo check -p wv-render --features app`
   期望：`Finished`（仅一条既有的 `hit_test` dead_code warning，非本轮引入）。

2. **跑 per-system 计时器（人工）**
   ```bash
   WV_PERF=1 cargo run -p wv-app --features app      # Windows: set WV_PERF=1 && cargo run …
   ```
   操作：光标静止 10s → 在地图上**连续移动** 10s → 开「群系视图」10s → 关掉它 10s。
   期望：每次移动/开层后 5s 内出现
   `wv_perf window: frames=… top5: <name> max=… avg=… n=… | …` 与
   `wv_perf worst frame #N: work=…ms top=<name> …ms`；
   单帧超 4ms 的会先出现 `wv_perf frame N: <name> X.XXms`。

3. **复跑定位探针（无需窗口）**
   ```bash
   cargo test -p wv-app --features app --lib -- --ignored --nocapture perf_probe_cursor_cell_change
   ```
   期望：三条 `[PROBE]` 行 —— 旧路径单价（碎片 / 整片两档）+ 修后 `sync_candidate_input`
   的 `max/avg`（本轮实测 `max=2.14ms avg=0.07ms`）。

4. **断言 1–3（计数器，无墙钟）**
   ```bash
   cargo test -p wv-render --features app --lib -- steady_state
   cargo test -p wv-render --features app --lib -- region_handles
   ```
   期望：`6 passed` / `5 passed`。

5. **回归**
   ```bash
   cargo test -p wv-render --features app
   cargo test -p wv-world-cell-map
   cargo test -p wv-render --features app --test acceptance_grid_visibility
   ```
   期望：上一轮 7 条网格可见性全绿；`wv-world-cell-map` 基线 217 全绿。

---

## 六、技术债与风险

1. **跨连通区仍付一次全量重算**（12.7ms@1024² 整片）：本轮做到「块内移动 0 成本」，
   「块间切换第一帧」还是贵的。要彻底解决需要把 biome 连通区做成**分块（chunk）增量**
   缓存，或给 `wv_terrain::region` 加一个「可复用掩膜 / 只扫 bbox」的入口 —— 另开单。
2. **`TerrainGrid` 缓存的身份靠 `CandidateKey`**（图 + 幅面 + `CandidateOverlayCache::last_epoch`）。
   若有绕过 `History` 直接改 `TerrainData` 的路径（目前没有），会读到旧栅格。**这条是
   「缓存失效靠协议而不是靠类型」的典型技术债**，已在 key 里放 `epoch` 兜住常规路径。
3. **`draw_region_decor` 仍每帧把阶梯边逐条 `gizmos.line_2d`**：选中一个大区域时边数上万，
   未实测其帧成本（本轮已把它与派生解耦，若实测 >2ms，下一步改成 `Mesh2d` 合批）。
4. **`sync_region_outlines` 的底色覆盖仍是 O(边数)/帧**（`region_fill_fingerprint` 每区域一次
   哈希，无分配）。200 区域时量级与 `geom` 指纹相当，可接受；若要再省，可与 `geom` 合并成一次遍历。
5. **`cellmap_bridge::sync_missing_overlay_markers` 每帧算 `shapes_fingerprint`**（O(缺失形状数)）：
   已有早退，量级小，本轮未动（明示范围外）。
6. **`wv-render` 里 `measurement::tests::nearest_measurement_prefers_the_closer_line_at_a_crossing`
   当前失败**：`measurement.rs` 工作树有 303 行**非本轮的**在飞改动（`nearest_measurement` 新增、
   `hit_test` 变为 dead code），该测试属那批改动。本轮未碰 measurement.rs。
7. **`wv-app/src/i18n/keys.rs:1179`**：工作树里数组有 461 条而常量写 458 → 整个 `wv-app`
   编不过。本轮为确保可验证**代改**为 461（一行，见 §三）。若该文件的作者另有安排，请以他们的为准。
8. **1/2 octave 线宽量化**：屏上宽度误差 ±19%（1.34–1.90px）。若日后有人抱怨边界线粗细
   随缩放跳变，第一步是把档宽从 1/2 缩到 1/4 octave（重建次数翻倍，仍远小于旧实现）。
