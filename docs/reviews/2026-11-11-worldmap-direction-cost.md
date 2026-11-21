# 世界地图方向转向「时空一致性校验」——架构成本与可行性评估

> **评审草稿 · 未采纳 · 待用户拍板**
>
> - 评审人：技术架构总监 柯桥良
> - 日期：2026-11-11（含当日 team-lead 两条追加的补充轮次）
> - 性质：**评估档的架构半边**。只做成本与可行性评估，**不做方向判断**、不写代码、不改任何文档基线。视觉部分只答「是否触碰架构契约」，**不替美术定规格**（视觉方向包另见 `docs/reviews/2026-11-11-worldmap-visual-direction.md`）。
> - 依据（只读核对）：`README.md` §1/§9/§12、`AGENTS.md` §2/§3/§4/§6/§7、`docs/adr/ADR-0054`、`ADR-0100`~`0105`，以及 `crates/` 下源码（`wv-io/src/diagnostics.rs`、`wv-io/src/project.rs`、`wv-io/src/lib.rs`、`wv-app/src/edit/diagnostics_bridge.rs`、`wv-app/src/edit/validation_actions.rs`、`wv-app/src/ui/validation_panel.rs`、`wv-app/src/ui_time.rs`、`wv-app/src/state.rs`、`wv-app/src/edit/editing/history.rs`、`wv-app/src/main.rs`、`wv-core/src/map/distance.rs`、`wv-core/src/time/mod.rs`、`wv-render/Cargo.toml`、`wv-render/src/state.rs`、`wv-render/src/world_projection.rs`、`wv-world-cell-map/Cargo.toml`、`wv-world-cell-map/src/terrain_view/field/material.rs`、`wv-world-cell-map/src/palette.rs`、`wv-default-theme/Cargo.toml`）。
> - 前提假设：① 单机、磁盘 YAML 为 source of truth 不变；② 「校验结果可视化」的第一个闭环 = 校验器产出 + UI 可见，不要求重画地形；③ 1 人～小团队、低维护成本优先；④ `AGENTS.md` §2.1 的 10-crate 依赖红线不变；⑤ 视觉重做范围限定在 Browse / Map 两个 ViewMode 的取色与呈现。**假设变了结论要重估。**
> - 评审边界：两轮均未安装依赖、未新建工程、未跑构建或全量 `cargo test`、未改任何 `.rs` / ADR / README / AGENTS / GLOSSARY、未 commit。仅写本文件。

---

## 0. 结论先行

| # | 结论 | 依据强度 |
|---|---|---|
| **C1** | **`README.md` §9「阶段六 距离与校验 ⬜ 待启动」与代码不符。**「事件间位移校验 + 校验结果可视化」已在 `wv-io` 落地为阶段六 v0.2 并接入 GUI 校验面板。转向新重心不是从零开工，是**复用 + 补语义口径**。 | 源码实读，见 §1.1 / §1.3 |
| **C2** | **新重心不需要继续投入地形渲染**。`wv-world-cell-map` 与 `wv-render` 的地形管线在校验链上**零参与**，第一闭环对两者**零改动**。 | 依赖表 + 源码调用链 |
| **C3** | 最小改动面：档 A 零新增代码；档 B（要地图定位标注）只改 `wv-io` 一处函数，`wv-app` / `wv-core` / `wv-render` / `wv-world-cell-map` 全零改动。 | 见 §2 |
| **C4** | 真正的成本不在渲染、不在搭诊断通道，而在**语义扩面**（存在性/时序校验）与**两处契约红线**（诊断类型归属、V1 不阻断保存）。 | 见 §3 |
| **C5** | 推翻 ADR-0054 / ADR-0100~0105 这类裁定，**真正不可逆的是 YAML schema、区间化数据模型、crate 依赖红线与校验语义契约**；被点名的 ADR 本身基本属可回退的表现层/结构层投入。 | 见 §4 |
| **C6** | **「点诊断 → 跳地图 + 拨时间轴」这条链今天已完整接线并在用**（相遇提示 / 悬空引用错误）。`DiagAnchor.day` 不是纸面结构。**唯一缺口**：位移校验这一条用 `hint()` 构造、`anchor == None`，所以该行不跳不拨。「路线 1」是**补一条诊断的锚点**，不是从零搭。 | 见 §1.1b / §2.5 Q3 |
| **C7** | 视觉层表现资产全部**可回退**；一次 Browse / Map 画面重做**不需要**动 cell-map 地形数据模型、`terrain.bin` 格式或 ADR-0054 固定色禁令。**视觉重做与「转向校验」改动面不相交，可并行、互不阻塞**。 | 见 §4.2 |

> 本评估**不修改** `README.md` §9；仅记录代码事实与基线差异，供拍板时一并裁定是否需要同步基线（那是另一次须用户确认的文档动作）。

---

## 1. 现成可复用资产 vs 沉没成本

### 1.1 现成可复用资产（逐项给 crate / 模块 / 已有能力）

| Crate | 模块 / 位置 | 已有能力（源码实读） | 与新重心的对应 |
|---|---|---|---|
| `wv-core` | `map/distance.rs` | `Vehicle::effective_daily_speed`、`Vehicle::segment_days`、`DistanceGraph::segment_days`、`shortest_journey`（Dijkstra，状态 `(PlaceId, VehicleId)` 支持换乘）、`ScheduledJourney.total_days`、`Feasibility{Reachable/ArrivesLate/Unreachable}`、`UnreachableReason{NoPath/NoUsableVehicle/RequirementsUnmet}` | 位移校验的**唯一权威算时入口**；`AGENTS.md` §4.2 指定，禁止第二份实现 |
| `wv-core` | `time/mod.rs` | `TimelineQuery::at` / `TimeSpan` / `state_spans` / `day_progress(current_day, …)` | 「某时刻状态」唯一求值入口（§3.2）；未来存在性/时序校验的现成底座 |
| `wv-core` | `model/event.rs`、`factions_at` / 势力层、`Unit` / `Track`、`Place` / `PlaceEdge`、`Map.extent` / `unit_per_km`、`encounter::detect_encounter_hints` | 事件带 `span` + `location.place` + `participants`；势力按日筛选；地点带 `(map,x,y)` 与 `effective_anchor()`；相遇检测已在 `wv-core` | 校验对象与定位锚点全部齐备，无需新增数据模型 |
| **`wv-io`** | **`src/diagnostics.rs::detect_travel_feasibility_hints`（第 558–665 行）** | **「阶段六 v0.2：事件间位移可行性提示」已实现**：按 `span.from` 排序 → 相邻事件对 → 共同参与者交集 → 双方 `location.place` 不同 → `gap = next.span.from - prev.span.to` → `shortest_journey` 取 `total_days` → `gap < total_days` 产出 Hint「预算时间不足：至少需 N 天，预算仅 M 天」；指纹 `travel_feasibility\|{prev}\|{next}\|{participants}` 稳定不含日期；7 条单测覆盖充足/不足/无共同参与者/同地/无路径/开放区间/指纹稳定 | **这就是阶段六第一格「事件间位移校验」的本体**，不是待建资产 |
| `wv-io` | `src/diagnostics.rs` | `Diagnostic{level,message,fingerprint,anchor,severity}`、**`DiagAnchor{map,at,day}`（`day` 注释原文「时间点（跳转时把时间轴拨到这里）」）**、`DiagLevel{Hint,Error}`、`Diagnostics{items}` 及 `hints()` / `errors()` / `dedup_by_fingerprint()` / `extend()`；ADR-0005 把错误与提示在类型层面分开；文件头原文「给提示做**回访定位**（点了要能跳到地图对应位置）」 | 诊断通道 + 回访定位的结构化载体已定型；UI「已读/忽略」认领锚在 `fingerprint` 上 |
| `wv-io` | `src/diagnostics.rs::hint()`（第 547–556 行） | **第 547 行注释原文：「构造一条提示级诊断（供其他派生检查复用，如「孤立事件」等后续检查）」** | 这是为「年代错置 / 孤立事件」等新规则**预留的扩展点**：新增派生检查 = 一条 `hint()` 或带 anchor 的 `Diagnostic`，无需新管线 |
| `wv-io` | `src/diagnostics.rs::derive_hints`（第 121–189 行） | 统一提示集合已在位：相遇检测（`detect_encounter_hints`，第 122 行，且**带 `anchor: Some(DiagAnchor{map, at, day})`**）→ 位移可行性（第 176 行）→ 跨级尺度一致性 ADR-0022 D4（第 179 行）→ 覆盖物地界（第 182 行）→ 词汇迁移 ADR-0032 D3（第 185 行）→ `dedup_by_fingerprint()`；`Project::diagnose()` 第 1470 行同样 `d.extend(derive_hints(&world))` | 磁盘侧 + 内存侧双通道自动带上任何新规则；势力层/归属边结构化诊断、分区诊断（ADR-0079 T13）同在该基建上 |
| `wv-io` | `src/project.rs` `diagnose()`（第 1414 行）/ `validate()`（第 1385 行） | `diagnose()` **永不失败**、返回结构化诊断，与 `validate()`（错误即 `Err`、CI / 命令行保存闸门）分工明载于 `diagnostics.rs` 文件头表格；H5 悬挂引用软失败（加载成功 + Error 级诊断 + 仍可编辑保存） | 与 §7.5 V1 同构的「只提示不阻止」范式现成 |
| `wv-io` | `src/lib.rs:32` | `pub use diagnostics::{derive_hints, DiagAnchor, DiagLevel, Diagnostic, Diagnostics};` | 类型已公开导出，下游无需新开模块 |
| `wv-app` | `src/edit/diagnostics_bridge.rs::derive_diagnostics(&World,&ThemePack)`（第 46 行，第 102 行 `d.extend(derive_hints(world))`） | 对**内存** `World` 跑同一套引用校验 + 全部提示，不敲盘；与 `Project::diagnose` 共享底层收集器；`anchor_for_dangling`（第 536 行起）为 **Error 级悬空引用**也派生 `DiagAnchor`（轨迹/事态/事件/地图四类），两条单测断言 `anchor.day == 12` / `== 3` | 编辑器侧诊断入口 + 锚点派生现成 |
| `wv-app` | **`src/edit/validation_actions.rs::jump_to_anchor_now`（第 32–57 行）** | **四步跳转已实现**：① `DrillState::drill_to(&world, &anchor.map)`；② `drill.sync_view(view)`；③ `view.camera_center = anchor.at` + `wv_render::camera::apply_view_to_camera(...)`；④ **`playback.current_day = anchor.day`** | 「点诊断 → 跳地图 + 拨时间轴」**已接线** |
| `wv-app` | `src/ui/validation_panel.rs::panel_validation`（第 125 行） | 错误/提示计数徽标 + 双列表；第 241/294 行 `Some(anchor)` 分支调用 `jump_to_anchor_now`；第 274 行注释明写「提示列表（相遇提示 / **事件位移可行性**等）」；另有 §12-39 世界图尺度只读三行（复用 `segment_days`） | **「校验结果可视化」已以 egui 面板形式落地**；无 anchor 的行退化为普通 label |
| `wv-app` | `src/state.rs::ProjectDiagnostics`（第 121 行）、`PlaybackState` / `ViewState` / `DrillState` | ECS Resource：`diagnostics` / `last_count` / `needs_diagnose`；`wv-render/src/state.rs:63` 明载「`current_day` 是**视图状态**，不是世界数据」 | 只读缓存 + 视图会话态现成 |
| `wv-app` | `src/edit/editing/history.rs::refresh_project_diagnostics`（第 124 行） | Update 阶段、早于 UI 帧、按 `needs_diagnose` 单点重算；第 122 行明载当前口径「≤ 几百事件单次亚毫秒，不做节流，规模上来再加重算预算」 | 刷新触发点现成，且**已有真实性能口径** |
| `wv-app` / `wv-render` | `src/ui_time.rs:7`、`wv-render/src/world_projection.rs:531/542`、`faction_overlays.rs:189`、`feature_overlays.rs:280`、`faction_partition.rs:164` | **拉取式刷新在位**：`current_day` 一变，`wv-render` 各 system 以 `let t = playback.current_day` 为输入，经 `TimelineQuery::at(&spans, t)` / `place_territory_at` / `partition_span_at` **求值**；`faction_partition.rs:6` 原文「渲染层禁自比时间，AGENTS §3.2」 | 「时间轴拨日 → 视图刷新」链路现成，且本身已合规 |
| `wv-app` | `src/main.rs::validate`（第 290 行）、`load`（第 273 行） | `wv validate <dir>` / `wv load <dir>` CLI 在位 | 无 GUI 的复跑入口现成（注意口径差异，见 §3.5 待实测 2） |
| `wv-edit` | `EditCommand` / `History` 两阶段提交 + 补偿回滚 | 编辑链完整 | 校验只读、不入编辑链，天然不违反「不得绕过 wv-edit 改世界」 |

### 1.1b 诊断锚点与跳转链（补充核实 · 回答「是补接线还是从零搭」）

```
[数据层 · wv-io]  derive_hints
   相遇提示         → Diagnostic { anchor: Some(DiagAnchor{ map, at, day }) }   ← 已带 anchor
   悬空引用 Error   → diagnostics_bridge::anchor_for_dangling → Some(anchor)      ← 已带 anchor
   位移可行性 Hint  → hint(fp, msg) → anchor: None                              ← 唯一缺口
        │
        ▼  Diagnostics → Project::diagnose()（磁盘）/ derive_diagnostics()（内存）
[wv-app]  state::ProjectDiagnostics (Res)
        │
        ▼  点击（ui::validation_panel::panel_validation）
[wv-app]  edit::validation_actions::jump_to_anchor_now  ← 纯函数，非 System、非 EditCommand
        ├─ DrillState::drill_to(&world, anchor.map) / sync_view(view)
        ├─ ViewState::camera_center = anchor.at → wv_render::camera::apply_view_to_camera
        └─ PlaybackState::current_day = anchor.day          ← 时间轴拨日已实现
        │
        ▼  拉取式刷新（AGENTS §3.4，不写「数据变了自动通知渲染」）
[wv-render]  let t = playback.current_day → TimelineQuery::at / *_at(…) 求值 → 更新渲染资源
```

**判定**：`DiagAnchor` 在 `wv-app` 侧**已经接线并上线**——点击带 anchor 的诊断会跳图、拨相机、拨时间轴；且 `wv-render` 侧的 `current_day` 消费链也已到位。**仅定义数据结构**的说法不成立。唯一未接的是**位移可行性提示这一条**：`hint()` 固定 `anchor: None`（第 553 行），所以该行不可点、不跳、不拨。因此「路线 1」是**补接线**（给这一条诊断填 anchor），不是从零搭；代价是 `wv-io` 一个函数内把 `hint(...)` 换成显式构造 `Diagnostic`，`wv-app` 侧零改动。

### 1.2 沉没成本（已投入但与新重心无关，点名，不评价好坏）

- `wv-world-cell-map`：`terrain_view/` GPU field 管线、`terrain_chunks`、`cell_selection`、`region_overlay`（国界/自然区覆盖）、`biome_boundary`、`CellPalette`、`GridConfig`、固定色 crate 级禁令（ADR-0054）。
- `wv-render`：LOD 分档、等高线、晕渲（`tri_scale_relief`）、方格平涂（`ViewBranch::CellFlat`）、街区 overlay、海岸三段色带、测量尺子相关呈现。
- `wv-theme` / `wv-default-theme`、色板 / 字体 / 图标令牌。
- `wv-io::terrain_bin`（`terrain.bin` v2，`WVTR` + `format_version=1`）。
- `docs/adr/` 中地形表现参数类 ADR 与 `README.md` §12 对应待裁条目：§12-49 气候纬度基准、§12-50 两套气候规则并轨、§12-53 米制回落、§12-55 图内 AMR、§12-59 方格平涂转正。
- §12-57 HTTP 工具面的「新增/更新无删除」「重跑国界只能新建一张图」。
- 最近 25 条几乎全落在上述三处渲染 crate 的提交。

> **补充注记（制作人实拍事实）**：`docs/audit/2026-10-29/shot_browse2.png` / `shot_map.png` / `shot_cellmap.png` 三张实拍里，三个 tab 共用同一套「纯饱和蓝海 + 硬边平涂色块」基底；上列晕渲 / 等高线 / 纸纹等近期表现投入**不仅与「转向校验」无关，同时也未改善用户当前正在看的 Browse / Map 两个模式**。即：这部分投入对**两条候选方向**都未产生用户可感知的收益。此为制作人亲眼看帧的结论，本评估只转录、不评价画面好坏、不替美术定规格。
>
> 按角色约定，仅点名，不评价好坏、不建议删除。

### 1.3 文档基线滞后勘误（供拍板时一并裁定）

| 基线写法 | 代码事实 | 影响 |
|---|---|---|
| §9「阶段六 距离与校验 ⬜ 待启动」，四格全部未勾 | `wv-io` 已实现「交通工具与地点边 / 路径搜索（含换乘）/ 事件间位移校验」并接入 GUI 校验面板；`wv-core` 已有完整距离图与 Dijkstra + 换乘 | 「从零做阶段六」的成本估算会**显著高估**；实际第一闭环是零新增代码 |
| 上游摘要「校验侧现状：反向索引第一版每次查询重建」 | §12-9 属实；但诊断重算另走 `needs_diagnose` 单点刷新，不是每帧重建 | 性能风险须区分「索引重建」与「诊断重算」两条不同路径 |

---

## 2. 最小改动面

### 2.1 直接回答两个点名问题

> **Q：是否必须动 `wv-world-cell-map` / `wv-render` 的地形管线？**
> **A：不必，且现状架构是刻意让诊断绕开 `wv-render` 的。** 校验结果的 UI 全部在 `wv-app` 的 egui 面板（`validation_panel.rs`），`wv-render/Cargo.toml` 无 `wv-io` 依赖（`AGENTS.md` §2.1 的 `wv-render` 允许依赖表里也没有 `wv-io`）；`wv-world-cell-map/Cargo.toml` 的依赖只有 `wv-core` / `wv-terrain` / `serde` / `serde_yaml` / `bevy`，**也不含 `wv-io`**。诊断数据在 crate 依赖图上根本到不了这两个 crate，地形管线连边都挨不上。

> **Q：新重心是否需要继续投入地形渲染？**
> **A：不需要。** 三条理由：
> 1. **数据源不涉地形**：`detect_travel_feasibility_hints` 只读 `World.events`（`span` / `location.place` / `participants`）与 `DistanceGraph`，不读 `TerrainData`、不读 GPU field。
> 2. 唯一与「地形」有关的是 `PlaceEdge.terrain` 字符串参与 `Vehicle.terrain_modifiers` 速度修正——属 `wv-core` 数据层，与 `wv-world-cell-map` 的取色 / 晕渲 / LOD 无关。
> 3. **呈现不涉地形绘制**：egui 列表 + `jump_to_anchor_now`（复用既有相机 / 钻取 / 时间轴），不新增任何地形绘制。
>
> 前提：世界图作为底图已够用。若未来要把校验结论绑定到更细的地形语义（例如「山路更慢」），只需在 `wv-core` 的 `PlaceEdge.terrain` / `Vehicle.terrain_modifiers` 补参数，**仍不需要 GPU 表现层投入**。

### 2.2 按 crate 列改动面（分「现在就要做」与「到那一步再说」）

**档 A｜零新增代码 —— 阶段六第一格已跑通**

```
[wv-core]  DistanceGraph::shortest_journey / segment_days / TimelineQuery::at
   ↓
[wv-io]    diagnostics::detect_travel_feasibility_hints → derive_hints → Diagnostics
   ↓
[wv-app]   edit::diagnostics_bridge::derive_diagnostics(&World, &ThemePack)
   ↓
[wv-app]   state::ProjectDiagnostics (Res) ← edit::editing::history::refresh_project_diagnostics
   ↓
[wv-app]   ui::validation_panel::panel_validation (egui 列表 + 点击跳转)
```

判定：加载工程 → 校验器产出「预算时间不足」Hint → 面板列出 → 保存照常成功。**闭环已在位，任何 crate 都无需改动。**

**档 B｜若「可视化」要求地图上的定位标注（而非仅面板文字）**

| Crate | 位置 | 改动 | 是否必须 |
|---|---|---|---|
| `wv-io` | `src/diagnostics.rs::detect_travel_feasibility_hints` | 把 `hint(fp, msg)` 换成显式构造 `Diagnostic` 并填 `anchor: Some(DiagAnchor{ map, at, day })`；`map` / `at` 由 `prev/next.location.place` 反查 `Place.map` + `Place::effective_anchor()`，`day` 取 `next.span.from`。**`DiagAnchor` 类型已在位，`jump_to_anchor_now` 已实现，不新增字段、不改 `Diagnostic` 结构** | **必须（档 B 唯一改动）** |
| `wv-app` | `src/ui/validation_panel.rs` / `src/edit/validation_actions.rs` | **零改动** —— 第 241/294 行已有 `Some(anchor)` 分支，`jump_to_anchor_now` 四步（含 `current_day = anchor.day`）已实现；补 anchor 后跳图 + 拨时间轴自动生效 | 零改动 |
| `wv-app` | `diagnostics_bridge.rs` / `state.rs` / `history.rs` / `app_bootstrap/resources.rs` | **零改动** —— `init_resource`、单点刷新、去重链已覆盖 | 零改动 |
| `wv-core` | — | **零改动** —— 距离、时间、锚点 API 全现成 | 零改动 |
| `wv-render` | — | **零改动**（第一闭环）。若坚持 GPU 画布 overlay 才需要，且必须先做类型下沉，见风险 R1 | 不必须 |
| `wv-world-cell-map` | — | **零改动** | 不必须 |

**档 C｜到那一步再说（语义扩面，仍不碰渲染三 crate）**

- 「国家 300 年建国、280 年有使节」类**存在性 / 时序校验**：在 `wv-io::diagnostics` 新增派生检查，复用 `wv_core::factions_at` + `TimelineQuery::at` + `state_spans`，经 `hint()`（第 547 行预留扩展点）或带 anchor 的 `Diagnostic` 汇入 `derive_hints`。
- 非相邻事件对的跨事件位移、角色 `state_spans` 位置随时刻求值：同上，落在 `wv-io` 规则 + `wv-core` 时间求值。
- 每条新规则 = 往 `derive_hints` 里 `extend` 一条 + 一条 `Hint` 级单测；`Project::diagnose()` 与 `derive_diagnostics()` 自动带上，通道、UI、ECS Resource 一律复用。

### 2.3 涉及文件树（只读核对过的落点，不新建文件）

```
crates/
├── wv-core/src/map/distance.rs                 # 现成，档 A/B/C 均零改动
├── wv-core/src/time/mod.rs                    # 现成，档 C 复用
├── wv-io/src/diagnostics.rs                   # 档 B 唯一改动点 / 档 C 新增规则点
├── wv-io/src/project.rs                       # diagnose() 已接 derive_hints
├── wv-io/src/lib.rs                           # DiagAnchor 等已 pub use
├── wv-app/src/edit/diagnostics_bridge.rs      # 现成，anchor_for_dangling 已派生锚点
├── wv-app/src/edit/validation_actions.rs      # jump_to_anchor_now 已实现四步跳转
├── wv-app/src/ui/validation_panel.rs          # 现成，零改动
├── wv-app/src/ui_time.rs / state.rs           # current_day 拉取式刷新
├── wv-app/src/edit/editing/history.rs         # refresh_project_diagnostics
├── wv-app/src/main.rs                         # wv validate CLI（口径见待实测 2）
├── wv-render/Cargo.toml                       # 确认无 wv-io 依赖
├── wv-render/src/world_projection.rs          # TimelineQuery::at 消费 current_day
└── wv-world-cell-map/Cargo.toml               # 确认无 wv-io 依赖
```

### 2.4 反悔成本说明

- 转向校验的反悔成本集中在**两处**：① 若为画布 overlay 把 `Diagnostic` 下沉到 `wv-core`（动所有消费方 import 与 §2.1 允许表）；② 若为省事把 `total_days` / 缺口写回事件，或把 `has_errors()` 接进保存闸门（污染 YAML / 破 V1）。二者一旦落地，回退要动数据迁移与语义契约，成本最高。
- **冻结地形渲染的反悔成本近零**：代码不删、ADR 只增不改（`AGENTS.md` §6.4），随时可恢复投入；已投入的地形 ADR 与提交属**沉没但不产生数据负债**。
- 前提：以上基于「第一闭环以 `wv-app` 的 egui 面板 + 跳转呈现」。若用户明确要求 GPU 画布 overlay，`wv-core` 类型下沉由「可不做」变为「必须做」，§2.2 档 B 结论需重估。

### 2.5 补充三问（team-lead 追加）

**Q1｜新增「事件间位移校验 / 年代错置」这类诊断，最小改动面是否只需 `wv-core` + `wv-io/diagnostics.rs` + `wv-app`，而完全不必触碰 `wv-world-cell-map` / `wv-render` 地形管线？**

**答：是，而且比这个设想更小。**

| 诊断 | 实际最小改动面 | 证据 |
|---|---|---|
| 事件间位移校验 | **零改动**（已实现） | `wv-io/src/diagnostics.rs:558–665` 已落地并接入 `derive_hints:176`；7 条单测；`wv-core` 的 `shortest_journey` 是现成算时入口 |
| 年代错置（建国前使节） | **只改 `wv-io/src/diagnostics.rs` 一处**：新增一条派生检查 + 在 `derive_hints` 里 `extend`；`wv-core` 与 `wv-app` **零改动** | 第 547 行 `hint()` 注释明写「供其他派生检查复用，如『孤立事件』等后续检查」= 预留扩展点；`factions_at` / `TimelineQuery::at` / `state_spans` 均在位；UI 的 `Some(anchor)` 分支与 `jump_to_anchor_now` 已实现 |
| 汇入 Diagnostics | 零改动 | `Project::diagnose():1470` 与 `diagnostics_bridge::derive_diagnostics():102` 都已 `extend(derive_hints(...))`，磁盘侧 / 内存侧自动覆盖 |

**「不必触碰地形管线」的硬证据**：`wv-world-cell-map/Cargo.toml` 与 `wv-render/Cargo.toml` 的依赖清单里**都没有 `wv-io`**；`Diagnostic` 定义在 `wv-io`，依 `AGENTS.md` §2.1 的允许表，这两个 crate 在依赖图上无法引用它。诊断数据在架构上就流不进地形管线。

**反例（唯一会动到渲染层的情形，但仍不动「地形管线」）**：若要求把校验结果画成地图画布上的 GPU overlay（连线 / 高亮 / 热力），而非面板列表 + 点击跳转，则因 `wv-render` 禁依赖 `wv-io`，须二选一：把纯诊断类型下沉到 `wv-core`（动契约、须新开 ADR），或在 `wv-app` 把诊断投影成 `wv-render::RenderInputs` 能消费的形态（动输入 trait）。**两者都是新增一条 overlay 绘制通道，都不改 `TerrainData` / GPU field 地形管线**。所以「路线 1 不必触碰地形管线」对面板方案成立，对 GPU overlay 方案同样成立。

**Q2｜`DiagAnchor.day` 要「把时间轴拨到那天」且遵循「渲染层不做时间比较（必须经 `TimelineQuery::at`）」，跳转链在现有分层下正确接法是什么？会不会撞「wv-render 不反向依赖 wv-edit/wv-app」？**

**答：现有接法已经正确，不会撞。** 按「谁发命令 / 谁求值 / 谁消费」拆：

| 角色 | 落点 | 做什么 | 为什么合规 |
|---|---|---|---|
| **谁发命令** | `wv-app::ui::validation_panel::panel_validation` → `wv-app::edit::validation_actions::jump_to_anchor_now` | 收集点击；`jump_to_anchor_now` 是**纯函数**（非 Bevy System、非 `EditCommand`），只写 `DrillState` / `ViewState` / `PlaybackState` / Camera 四个**视图会话态** | 跳转**不改世界数据**，因此不经 `wv-edit`，也不触「wv-app 不得绕过 wv-edit 直接改世界」；`validation_actions.rs:19` 明写不用 `CommandId::DrillToSegment` 的理由 |
| **谁求值** | `wv-io::diagnostics`（数据层）构造 `DiagAnchor{map,at,day}`；`wv-app` 只把 `anchor.day` **原样赋给** `PlaybackState::current_day`（第 55 行） | `day` 是**已算好的整数**（来自 `Event.span.from` / `Track.span.from` / `h.span.from`），赋值处**不做任何时间比较** | §3.2 禁的是「渲染层做时间比较」；这里是数据层产结论 + 视图状态搬运。`wv-render/src/state.rs:63` 明载「`current_day` 是视图状态，不是世界数据」 |
| **谁消费** | `wv-render` 各 system：`let t = playback.current_day` → `TimelineQuery::at(&spans, t)`（`world_projection.rs:531/542`）、`place_territory_at`、`partition_span_at` | **拉取式刷新**（`AGENTS.md` §3.4）：时间轴一变，渲染层以 `t` 为输入**求值**并更新渲染资源，不做 `if time > x` 这类比较 | `faction_partition.rs:6` 原文「渲染层禁自比时间，AGENTS §3.2」；求值入口唯一，正是 `TimelineQuery::at` |

**依赖方向核对**：`jump_to_anchor_now` 里唯一跨 crate 调用是 `wv_render::camera::apply_view_to_camera(...)`——方向是 **`wv-app → wv-render`（向下）**，合规。`wv-render` 侧只依赖 `wv-core`（`TimelineQuery::at`）与自己的 `RenderInputs` / 视图 state，**不 import `wv-app` / `wv-edit` / `wv-io` 任何类型**；`DiagAnchor` 只在 `wv-io ↔ wv-app` 之间流动，从不进入 `wv-render`。因此**不撞**「wv-render 不反向依赖 wv-edit/wv-app」。

**Q3｜`DiagAnchor` 在 `wv-app` / `wv-render` 侧今天是否已有接线（点击诊断跳地图 + 拨时间轴），还是仅定义数据结构？**

**答：已接线并上线，不是仅定义结构；但位移诊断这一条没接上。**

| 环节 | 状态 | 证据 |
|---|---|---|
| 数据结构 | 已定义并 `pub use` | `wv-io/src/diagnostics.rs:65`；`wv-io/src/lib.rs:32` |
| 派生时填 anchor | **已接线** | 相遇提示 `derive_hints:133` 构造 `Some(DiagAnchor{map, at, day: h.span.from})`；Error 级悬空引用由 `diagnostics_bridge.rs:536` `anchor_for_dangling` 派生（四类实体），单测第 628/687 行断言 `anchor.day == 12` / `== 3` |
| 点击 → 跳图 + 拨相机 | **已接线** | `validation_panel.rs:253/306` 两处调用；`validation_actions.rs:46–53` 实现 `drill_to` / `sync_view` / `camera_center` / `apply_view_to_camera` |
| 点击 → 拨时间轴 | **已接线** | `validation_actions.rs:55` 原文 `playback.current_day = anchor.day;`；文件头第 16 行「让时间轴拨到提示发生时刻」 |
| 时间轴 → 视图刷新 | **已接线** | `wv-render` 多处 `let t = playback.current_day` + `TimelineQuery::at`；`wv-app/src/ui_time.rs:7` 日期显示同源于 `current_day` |
| **位移可行性提示这一条** | **未接** | `detect_travel_feasibility_hints` 用 `hint()` 构造，而 `hint()` 第 553 行固定 `anchor: None` → 面板该行走 `else` 分支，退化为不可点 label |

**结论**：「路线 1」是**补接线**（一处函数内给位移诊断填 anchor），不是从零搭；补完后 `wv-app` / `wv-render` 侧零改动即自动获得「跳地图 + 拨时间轴」。这也把 §2.2 档 B 的成本从「改 UI」进一步收敛到「只改一条诊断的构造方式」。

### 2.6 对设计侧「四件有界小件」的架构复核（交叉复核轮次）

> 来源：`game-design-director` 的方向评估 v2 复核结论。设计侧定性与我的取证大体一致，但**第 2 件的定性需要纠正**——它在当前 crate 边界下不是小件。此节只为避免两份草稿口径冲突，不做方向判断。

| 件 | 设计侧定性 | 架构侧复核 | 依据 |
|---|---|---|---|
| **1** 新增「事件引用实体早于其存在起点」提示级诊断（280 使节 / 300 建国） | 纯逻辑小件，复用 `hint()` + `DiagAnchor` | **同意**。落点 `wv-io/src/diagnostics.rs` 一处：新增派生检查 + `extend` 进 `derive_hints`（第 547 行 `hint()` 注释即预留扩展点），`wv-core` / `wv-app` 零改动，级别必须 `Hint`（V1）。**一处修正**：`hint()` 第 553 行固定 `anchor: None`，若要「点击跳图 + 拨时间轴」须显式构造 `Diagnostic` 并填 `Some(DiagAnchor{map,at,day})`；只用 `hint()` 则该行退化为不可点 label | §1.1b / §2.5 Q3 |
| **2** `wv-render` 消费 `Diagnostics`，把 `DiagAnchor` 画成地图本体的冲突标记 / 连线 / 气泡 | 有界小件 | **不同意定性：这是四件里唯一需要动契约的一件。** `Diagnostic` / `Diagnostics` / `DiagAnchor` 定义在 `wv-io`；`AGENTS.md` §2.1 的 `wv-render` 允许依赖表**无 `wv-io`**，`wv-render/Cargo.toml` 依赖表里也确实没有（第 21 行的 `wv-io` 只出现在注释「与 wv-io 同一纯数据依赖」，指 `serde`/`serde_yaml`）。故「`wv-render` 直接消费 `Diagnostics`」在当前分层下编译不过或破红线。两条合规路径：**(A)** 留在 `wv-app` 的 egui 层，用已实现的 `jump_to_anchor_now` 做「跳转型可视化」——零新增依赖、零契约改动，**这是第一闭环的推荐解**；**(B)** 若必须 GPU overlay，先把纯诊断类型下沉 `wv-core`（`wv-render` 已允许依赖）或由 `wv-app` 投影成 `RenderInputs` —— **动契约，须新开 ADR 并同步 §2.1 红线表 / §2.2 断言**。设计侧「别改 ADR-0005 的 Hint/Error 分离与不阻止保存口径」的提醒完全正确，但仅守住它不够，还得守住依赖红线 | §3.1 R1 / §2.5 Q1 反例 |
| **3** 新增一个故意埋了 §1 两句矛盾的示例工程（数据，不是代码） | 数据底子够：`unit_per_km: 4`、`distance_km` 边现成 | **同意，但有配套前提**：「三天前在北境今天到南疆」若要被**现有**诊断捕获，须满足相邻事件 + 共同参与者 + 双方 `location.place` + `prev.span.to` 非空 + `gap > 0` + 路径可通；而「300 年建国 / 280 年使节」属**存在性 / 时序矛盾，现有诊断完全不覆盖**，必须等第 1 件新规则落地才会报出来。故示例工程与第 1 件是**配套件**：只埋数据不写规则会得到「零诊断」的假阴性。数据层不动 schema | §3.3 R3 |
| **4** 清单销账：README:286-290 阶段六四项 ⬜、:521 §12-39「未实现」 | 文档状态滞后于代码，是「待决项净增」的机制 | **核实属实，且比「代码已完工」更强**：README:286-290 四项全 ⬜，而前三项在 `wv-core` / `wv-io` 已实现、第四项以 egui 面板落地；README:521 §12-39 标「未实现」，但 `validation_panel.rs::scale_hint_texts`（第 36–121 行）已实现，第 434–459 行手算测试断言 `3.2×3.2 km` / `1.6×0.8 km` / `0.04·0.03·0.01 天` —— **该条自定的关闭条件（「面板出现换算行 + 天数/公里经手算期望值测试锚定」）实际已满足**。销账是文档动作，须用户确认后另做（本评审不改 README） | §1.3 |

**关于设计侧请我判断的边界（是否放宽到跨角色 / 跨非相邻事件）**

**答：同意「属产品范围 / 口径问题、不是架构问题，先按现状收口」。** 补三条架构侧依据，说明「先收口」不只是范围判断，也是避免踩门禁的正确顺序：

1. **复杂度与重算预算门禁**：相邻事件对是 O(N)，放宽到「所有事件对」是 O(N²)；`derive_hints` 在 `needs_diagnose` 单点重算，`history.rs:122` 明载现行口径「≤ 几百事件单次亚毫秒、不做节流」。放宽会直接撞 §3.4 的重算预算门禁——超阈则新规则不得合入，须先改索引 / 重算策略并开 ADR。
2. **「跨角色」会引入时间求值**：若按角色在 t 时刻的位置校验，须经 `TimelineQuery::at` 求 `state_spans`（§3.2），那是「新规则 + 时间求值」，落点仍在 `wv-io` 一处、不动架构，但已不是「纯逻辑小件」，需按 §3.1–3.3 的判据四条（独立复算 / 误伤 / 负向 / 一致性+可达性）单独给断言。
3. **现有「跳过」口径要一并定**：实现显式跳过 `NoPath` / `NoUsableVehicle`（源码 630–637「留给后续检查」）、开放区间、同地、无共同参与者。放宽时这几类若不一起定口径，示例工程会出现「明明矛盾却零诊断」的假阴性。

建议顺序：**先按现状收口 → 用第 1 件补「存在性 / 时序」整类（这是 §1 两句典型问题里尚未覆盖的一半）→ 待用户拍板后再谈跨角色 / 非相邻，且先过 §3.4 门禁**。

### 2.7 方案 A（`wv-app` egui 叠层浮层）定稿后的改动面校准

> 来源：`game-design-director` v3 §3 路线 A 明确选定——`wv-app` 的 **egui 叠层浮层**，用已有 `anchor.at` 换算屏幕坐标叠在世界地图上方；**不是** `wv-render` 的 GPU canvas overlay，**也不只是**「面板列表 + 点击跳转」。方案 B（下沉 `wv-core` + 新开 ADR）明确不选。

**结论：低成本口径成立，不需要按「契约改动」重估。** 三条确认：

- 诊断数据不进 `wv-render` → 不触发纯诊断类型下沉 `wv-core`，§3.1 R1 的唯一红线情形**未发生**；
- `wv-app` 允许依赖 `wv-io`（§2.1），叠层读同 crate 的 `ProjectDiagnostics` Resource → **零新增依赖**；
- 不动 §2.1 允许表、不动 `Diagnostic` 字段集合、不动 ADR-0005 的 Hint/Error 分离 → **零新 ADR**。

**但「零契约改动」≠「零代码」。相较档 A，方案 A 仍有两块新增实现，且全在允许依赖方向内：**

| 新增块 | 落点 | 依据 / 注意 |
|---|---|---|
| ① egui 叠层绘制 system（读 `ProjectDiagnostics`，把带 anchor 的条目画成标记 / 连线，点击走已有 `jump_to_anchor_now`） | `wv-app`，新增一个 UI system | `validation_panel.rs` 已有 `Res<ProjectDiagnostics>` / `Res<crate::state::GridConfig>` / `Query<Camera2d>` 三件输入在手边；`crate::state::GridConfig` 是 `wv_render::state` 的重导出（`state.rs:21-25`），`GridConfig::map_to_view` 定义在 `wv-world-cell-map/src/state.rs:424`，`wv-app` 侧现成可调（`edit/line_poi_tool.rs:1500` 已在用）→ **零新增依赖** |
| ② 世界 / 视图坐标 → 屏幕坐标的**反向**换算 | 建议 `wv-render::camera`（与 `screen_to_world` 同一条变换链），由 `wv-app` 调用 | `wv-render/src/camera.rs:355` 只有 `screen_to_world`（注释明写「变换链唯一实现」），**没有 `world_to_screen`**。若在 `wv-app` 自行反推会出现「同一变换链两处实现」，与 `camera.rs:350` 口径冲突。补在 `camera.rs` 是纯函数、方向 `wv-app → wv-render` 向下、**不消费诊断数据**、不动依赖表，故不违反「wv-render 不反向依赖 wv-edit/wv-app」 |

> ② 不违背设计侧「`wv-render` 侧只做一件与诊断无关的内部修复」的定性——坐标换算工具同样与诊断无关，属同一类内部改动；只是该修复清单需从 1 件补为 2 件。

**执行期最需要盯的一条：`DiagAnchor.at` 目前大量是占位值，不是真实地点坐标。**

| 产出点 | `at` 现状 |
|---|---|
| 相遇提示 `derive_hints:133-137` | `(0,0)`，注释原文「at 在 Waypoint (place, day) 严格判定里没有几何意义」 |
| 跨级尺度提示 `diagnostics.rs:536-540` | `(0,0)`、`day: 0` 占位 |
| 悬空引用 `anchor_for_dangling`（`diagnostics_bridge.rs:552-581`） | track 用 `Waypoint.pos`；event 用 `loc.x.or(place.x)`；situation / map 用 `(0,0)` |
| 位移可行性 `diagnostics.rs:661` | `hint()` → `anchor: None` |

因此 egui 叠层若统一按 `anchor.at` 画标记，**多数条目会叠在世界原点**。这与 `world_projection.rs:544-548` 的 `Vec2::ZERO` 是同一个「地点/事件 → 真实坐标」缺口的两面。项目已有唯一入口：**`Place::effective_anchor() -> Option<(f32, f32)>`（`wv-core/src/map/distance.rs:473`；`world_projection.rs:375` 注释明载「ADR-0046 D2 定的**唯一入口**」）**。故 §2.2 档 B「只改 `wv-io` 一处函数填 anchor」需精确化为一句话：

> **把 `diagnostics.rs:661` 的 `out.push(hint(fp, message))` 换成显式构造 `Diagnostic`，且 `anchor.at` 必须经 `Place::effective_anchor()` 求出真实坐标（再按需 `GridConfig::map_to_view`）、`day` 取 `next.span.from`、`map` 取 place 所在图。**

两点附带提醒：

- 设计侧提到把 `Vec2::ZERO` 换成 `drill.rs:1684-1702` 的解析——该段**确实存在**，但它用 `loc.x.unwrap_or(place.x.unwrap_or(0.0))` 自己解坐标，**绕开了 `effective_anchor()` 唯一入口**；`drill.rs:448/1341` 与 `world_projection.rs:188/588` 才是走唯一入口的写法。建议修复时复用 `effective_anchor()`，避免「同一坐标两套口径」。
- `active_events` 存的是 `Vec2` **视图空间**坐标（其余 push 点都经 `grid.map_to_view`），替换 `Vec2::ZERO` 时须同样套 `map_to_view`，不能直接塞图内局部 wu。

**统一口径确认（与设计侧一致）**：唯一锚点断点 = `wv-io/src/diagnostics.rs:661` 的 `out.push(hint(fp, message))` + `hint():553` 的 `anchor: None`；`diagnostics_bridge.rs:102` 只 `d.extend(derive_hints(world))`、不补。跳转机制（`validation_actions.rs:13-16, 32-55`）、面板可点、`needs_diagnose` 重算、Error / 相遇提示锚点**均已接线**。

---

## 3. 架构风险（按严重度排序）

### 3.0 先回答点名项：诊断/校验结果走什么通道回 UI 才不破三条约束

**现状合规通道（推荐继续沿用）**

```
[纯数据层] wv-core: TimelineQuery::at / DistanceGraph::shortest_journey
      │  所有 gap 与 total_days 的比较都发生在 wv-io（数据层），不在渲染层
      ▼
[wv-io]   diagnostics::derive_hints → Diagnostics { level: Hint|Error, message,
      │  fingerprint, anchor: Option<DiagAnchor>, severity }
      ▼
[wv-app]  edit::diagnostics_bridge::derive_diagnostics(&World, &ThemePack)  ← 内存 World，不敲盘
      ▼
[wv-app]  state::ProjectDiagnostics (Res；needs_diagnose 单点刷新，UI 每帧只读)
      ▼
[wv-app]  ui::validation_panel::panel_validation (egui) → validation_actions::jump_to_anchor_now
      │                                             └→ PlaybackState.current_day = anchor.day
      │                                                        ▼（拉取式刷新）
      │                                    wv-render: TimelineQuery::at(t) 求值 → 渲染资源
      └── 保存路径 Project::save_world / save_project **不读**该 Resource

[wv-render / wv-world-cell-map]  不在这条链的诊断数据面上（wv-render 只消费 current_day）
```

| 既有约束 | 现状如何满足（源码依据） | 新重心下的保持条件 |
|---|---|---|
| **渲染层不做时间比较**（§3.2） | 唯一时间比较是 `gap = next.span.from - prev_to` 与 `gap < journey.total_days`，位于 `wv-io/src/diagnostics.rs:626/638`；`wv-render/src/world_projection.rs:542` 只用 `TimelineQuery::at(&spans, t)` **求值**；egui 面板只消费字符串与计数 | 新增「某时刻状态」必须经 `TimelineQuery::at`；诊断若进画布，传给渲染层的须是**已判定结论**（定位坐标 + day + 文案），不得给原始事件让渲染层自算 |
| **`wv-render` 不反向依赖 `wv-edit`/`wv-app`**（§2.1 更严：允许表里也没有 `wv-io`） | `Diagnostic` 定义在 `wv-io`；UI 在 `wv-app`；`wv-render/Cargo.toml` 无 `wv-io`；`cargo tree -p wv-render \| grep -E 'wv-edit\|wv-app'` 应为空（§2.2 已有断言家族） | 两条合规路径：**(A)** 继续在 `wv-app` egui 层做「跳转型可视化」——零新增依赖，第一闭环推荐；**(B)** 若必须 GPU overlay，先把纯诊断数据类型下沉到 `wv-core`（`wv-render` 已允许依赖），由 `wv-app` 填充、`wv-render` 只读——**这是动契约，须新开 ADR** |
| **只提示不阻止保存**（§7.5 V1 / V2） | `travel_feasibility` 走 `hint()` → `DiagLevel::Hint`；`derive_hints` 永不产 `Error`（`diagnostics.rs:115`）；`has_errors()` 只驱动 UI 徽标；`save_project` 不触发校验（README §10.1 ④）；`Project::validate()` 只看目录约定 | 不得把 `has_errors()` 接进保存闸门；不得把 `total_days` / 缺口天数写回 `Event` / `Character`（§4.2 禁「行程时间结论」字段，YAML 是 source of truth） |

### 3.1 R1【最高】诊断类型归属 `wv-io` 与 `wv-render` 依赖红线的冲突

**风险**：新重心最自然的第一反应是「把校验结果画到世界地图上」。但 `Diagnostic` / `Diagnostics` / `DiagAnchor` 定义在 `wv-io`，而 `AGENTS.md` §2.1 的 `wv-render` 允许依赖表**没有 `wv-io`**（源码 `wv-render/Cargo.toml` 亦无）。实现者只有两条路：给 `wv-render` 加 `wv-io` 依赖（破红线表），或让 `wv-render` 自己重算时间结论（破 §3.2）。这是唯一「一处改错同时踩两条以上硬约束」的点，且触发概率最高。

| 判据 | 可复跑验证方式 |
|---|---|
| 独立复算 | 不依赖实现者说明：静态核 `wv-render/Cargo.toml` 与 §2.1 允许表；跑 `cargo tree -p wv-render \| grep -E 'wv-io\|wv-edit\|wv-app'`（应为空），并 Grep `wv-render/src` 无 `wv_io::` 引用 |
| 误伤评估 | 核对新增呈现是否引入**第二个诊断重算点**：`needs_diagnose` 应仍是唯一刷新触发点；面板计数与 `derive_diagnostics` 输出条数一致，不重复计数 |
| 负向断言 | 构造一条 `wv-render` 内 `use wv_io::diagnostics::Diagnostic` 或 `next.span.from - prev.span.to` 的写法，断言 §2.2 门禁 / 评审必须拒收（此类写法不应能存在于仓库） |
| 一致性 + 可达性 | 从 `shortest_journey().total_days` → `Diagnostic.message` → 面板文案 → `fingerprint` → `DiagAnchor{map,at,day}` → `jump_to_anchor_now` → `TimelineQuery::at(t)`，逐跳核对数值未被二次加工 |

### 3.2 R2【高】V1「只提示不阻止」的语义漂移与 YAML 污染

**风险**：`wv-app::edit::diagnostics_bridge::derive_diagnostics` **会**产出 `Error` 级诊断（悬空引用），UI 用 `n_errors` 显示 Err 徽标；而 `wv-io/src/diagnostics.rs:96` 的 `has_errors()` 注释写「编辑器据此决定**能否继续保存**」，与 `wv-app/src/state.rs:116–119`「校验面板据此**只列、不阻止**保存」及 §7.5 V1 口径**在同仓内不一致**。一旦有人按前一句把 `has_errors()` 接进保存闸门，即破项目定位；若再把 `total_days` / 缺口天数写回事件以「方便展示」，即破 §4.2 并污染磁盘 YAML（不可逆）。

> 排序说明：若按「后果不可逆性」单一维度，R2 应列第一（YAML 是 source of truth）；本表按「触发概率 × 后果」综合排序，故 R1 在前。

| 判据 | 可复跑验证方式 |
|---|---|
| 独立复算 | 对同一工程做 `load(save(w)) == w` round-trip 前后结构/字节比对，独立确认「跑过诊断」不改变任何落盘字段（不读诊断代码，只看 YAML） |
| 误伤评估 | 构造「同时存在 Hint 与 Error 级诊断」的工程，走正常保存路径，确认**未被误阻断**（不得出现按 `has_errors()` 拒绝保存的行为） |
| 负向断言 | 构造一条「诊断含 Error → `save_project` 返回失败」的实现，断言该行为必须判为**违反规范**（系统不得因诊断拒绝保存） |
| 一致性 + 可达性 | 确认 `Diagnostics` 的消费方只有 `ProjectDiagnostics` Resource、GUI 面板与 CLI `diagnose()`；`save_project` / `Project::save_world` / HTTP 写类 skill 均不读它；链路单向、终点不是磁盘 |

### 3.3 R3【中】现有位移校验的覆盖口径 ≠ 产品定位的「时空一致性」

**风险**：`detect_travel_feasibility_hints` 只覆盖「按 `span.from` 排序后的**相邻**事件对 + 共同参与者非空 + 双方都有 `location.place` 且不同 + `prev.span.to` 为 Some + `gap > 0` + `shortest_journey` 成功」；源码第 630–637 行明确「不通（NoPath / NoUsableVehicle）→ **不报本检查**，留给后续检查」。它**不覆盖**：`README.md` §1 第二个典型问题「国家 300 年建国，为什么 280 年就有它的使节」（存在性 / 时序校验）、非相邻事件对、角色 `state_spans` 位置随时刻的求值（现实现只读 `Event.location.place`，未走 `TimelineQuery::at`）。因此转向「时空一致性校验」的真实工作量在**语义扩面**，不是渲染、也不是搭通道。此外 `travel_feasibility` 用 `hint()` 构造，`anchor` 为 `None`，面板该行**不可点击跳转**——这是「校验结果可视化」目前唯一的可见缺口（补法见 §2.5 Q3）。

| 判据 | 可复跑验证方式 |
|---|---|
| 独立复算 | 用**手算期望值**构造最小工程（一处建国日 + 一条早于建国日的使节事件），独立算出应出的诊断条数，再与 `derive_hints` 实际输出比对（§7.2「行程时间计算必须用手算期望值」） |
| 误伤评估 | 构造合法世界（时间充足、同地、无共同参与者、开放区间），断言诊断集合为空；确认语义扩面不把合法设定误判为矛盾 |
| 负向断言 | 构造一条「确定不可达」的位移（预算 2 天、路径最少 2.5 天），断言**必须**产出 `travel_feasibility\|` 指纹的 Hint，不得漏报（现有 `travel_feasibility_hints_when_budget_below_path_min` 已覆盖） |
| 一致性 + 可达性 | 每条诊断的 `fingerprint` 稳定不含日期（现有 `travel_feasibility_fingerprint_is_stable_under_time_edit`）；补 `anchor` 后每条都能经 `jump_to_anchor_now` 跳到 `(map, at, day)`，且 `day` 与 `TimelineQuery::at` 的权威状态一致 |

### 3.4 重算预算（门禁类 · 不虚构运行时降级）

> **归类依据**：诊断重算是**离线派生**，结果只有「算 / 不算」两态，没有「降质但仍可信」的运行时形态；为它硬造自动降级属过度工程，故按门禁卡点入表。三条阈值均取自仓库现行口径，非新拟。

| 指标 | 数值目标 | 测量方式 | 超阈处置（门禁卡点 · 写明触发点与责任人） | 归类依据 |
|---|---|---|---|---|
| 单次诊断重算耗时 | ≤ 亚毫秒，规模 ≤ 几百事件（`history.rs:122` 现行口径：不做节流） | 在 `refresh_project_diagnostics`（真实函数）入口打点计时，用 `bash scripts/cargo-test.sh` 环境跑 `examples/xinhai-1911`，不跑全量 `cargo test` | 卡在「新增全量重算点」评审：由**主程序在阶段六实施时**判定；超阈则**不得**新增第二个重算触发点，必须复用 `ProjectDiagnostics::needs_diagnose` 单点刷新；不达标的新校验规则不得合入 | 无运行时降级语义，只能靠「少算」 |
| 反向索引策略 | N ≤ 1 万沿用「每次查询重建」（README §12-9 明载阈值） | 统计 `timeline/events/` 实体文件数与单次 `derive_hints` 调用次数 | 卡在 §12-9 收敛评审：N > 1 万由**主程序在阶段六**判定，须先开 ADR 改增量维护，否则不引入「时间滑块逐帧全量校验」 | 索引策略是构建期决策，无运行时动作 |
| 诊断集合稳定性 | 同一 `World` 重跑集合一致；指纹不含日期 | 现有 `diagnose_is_deterministic_across_calls`（`wv-io/tests/example_project.rs:564`）+ GUI 拖动时间滑块相邻帧比对 | 卡在新增校验规则合入：由 **QA 出断言**判定；出现诊断抖动（指纹随日期变）则该规则不得合入 | 一致性/可达性属正确性门禁 |

### 3.5 待实测项（只写验证方法，本轮不执行）

| 验证问题 | 预期结论 | 验证方法（一句话） | 由谁在哪个阶段验证 |
|---|---|---|---|
| 磁盘侧 `Project::diagnose()` 与 GUI 内存侧 `derive_diagnostics()` 的位移提示集合是否一致 | 一致（`project.rs:1470` 与 `diagnostics_bridge.rs:102` 都调 `derive_hints`） | 对同一工程分别跑两者，按 `fingerprint` 比对差集 | 主程序 · 阶段六实施首日 |
| `wv validate` CLI 是否暴露位移提示 | 否（`main.rs:293` 走 `project.validate()`，只看目录约定） | 对含已知不可达位移的工程跑 `wv validate`，检查输出是否含 `travel_feasibility` | QA · 阶段六验收 |
| 给位移诊断补 `anchor` 后，点击是否真的跳图 + 拨时间轴 | 是（`validation_panel.rs` 的 `Some(anchor)` 分支 + `validation_actions.rs:55` 已在位） | 构造一条带 anchor 的位移提示，在 GUI 点击，观察相机与 `current_day` 是否变化 | QA · 阶段六验收 |
| 若把诊断画到地图画布，是否必然新增 `wv-render → wv-io` 依赖 | 是（故违反 §2.1），须改为类型下沉 `wv-core` 或留在 `wv-app` | 静态核 `wv-render/Cargo.toml` 与 §2.1 允许表，并跑 `cargo tree -p wv-render \| grep wv-io` | 主程序 · 方向拍板后、动工前 |
| 「建国前使节」类存在性校验能否复用 `factions_at` + `TimelineQuery::at` | 可（两者均在位），但需新增诊断规则 | 在示例工程构造早于建国日的使节事件，手算期望条数后比对 | 主程序 · 阶段六语义扩面 |
| 全量时空校验（所有角色 × 事件 × 时刻）下 `derive_hints` 实测耗时 | 现行「亚毫秒 / ≤ 几百事件」口径可能不成立 | 用 `bash scripts/cargo-test.sh`（受限环境 TEMP 权限）跑计时用例 | 主程序 · 阶段六实施 |

---

## 4. 推翻 ADR-0054 / ADR-0100~0105：不可逆 | 可回退

> 判据：**「不可逆」= 改动会要求存量工程迁移、改变已落盘数据的解释，或改变所有下游消费方共同依赖的契约**；「可回退」= 只改绘制参数、模块位置或工具面能力，磁盘 YAML 与领域语义零触碰。

| 不可逆（数据模型 / 文件格式 / crate 与语义契约） | 可回退（表现层 / 参数口径 / 结构位置 / 工具面） |
|---|---|
| **磁盘 YAML schema 与 `load(save(w)) == w` round-trip 契约**（P3、§4.5 每实体一文件）：任何字段增删都要求存量工程迁移 | **ADR-0100** CellMap「方格平涂」转正：`ViewBranch::CellFlat` / WGSL 早退 / uniform 色表 / `WV_CELLRAW` 对照探针——纯 GPU 取色决策，不进 YAML |
| **§4.2「距离只存参数」**：`PlaceEdge` 只存 `distance_km` + `terrain`，`Vehicle::effective_daily_speed` 与 `DistanceGraph::segment_days` 为唯一换算入口。推翻即改变所有已落盘距离数据的解释 | **ADR-0101** 晕渲表现升级 / **ADR-0102** 等高线与湖泊读图层 / **ADR-0103** LOD 分档阈值：着色、线宽、可见性档位的参数口径 |
| **P6/P7 区间化存储**：`state_spans` / `TimeSpan` / `TimelineQuery::at` 为「某时刻状态」唯一入口（§3.2、§4.1）；改成「当前值」会覆盖历史 | **ADR-0105** 边缘带自动子图：复用既有 `RasterRef` + P4 子图，README §12-55 追记明载「**不需要 terrain.bin v3**」——它刻意避开了文件格式变更，故可回退 |
| **`wv-core` 零 Bevy / 零 fs / 零网络**、依赖只能向下（§2.1）；§2.3 明确 wv-core 必须能「独立做校验器」。破坏后所有 crate 的独立测试与 CLI 同时失效 | **ADR-0054 的实施形态**：`terrain_view/` GPU 场管线、`terrain_chunks`、`cell_selection`、`region_overlay`、`biome_boundary`、`CellPalette`、`GridConfig`——整建制代码，冻结或合并回 `wv-render` 都不动数据 |
| **`wv-edit` 的 `EditCommand` / `History` 两阶段提交 + 补偿回滚**，与「wv-app 不得绕过 wv-edit 直接改世界」：涉及 undo/redo 正确性 | **ADR-0054 本身**（crate 拆分 + 依赖方向 + 固定色 crate 级禁令 + bevy 非 optional）：属**结构性、可逆但成本中等** —— 推翻需同步改 §2.1 红线表、§2.2 的 `cargo tree` 断言与 re-export 兼容路径，并会作废 crate 族（`wv-city-map` / `wv-building-map`）立项前提；但**不改 YAML、不改数据语义** |
| **§7.5 V1「只提示不阻止」+ `DiagLevel` 的 Hint/Error 类型分离 + `fingerprint` 语义**（ADR-0005）：UI「已读/忽略」认领状态锚在其上，换指纹 = 重置作者记录 | `wv-theme` / `wv-default-theme`、色板、字体、图标与界面令牌 |
| **`wv-app` 持有可变 `World` 工作副本 + 磁盘 YAML 为 source of truth**（用户裁定，§2.1 补记） | **视觉层（补充）**：`wv-default-theme` 的 `PaletteColors` 36 槽 / `ThemePack` / 纸纹 `overlay` / `SeaRecipe` / POI 图标 / CJK 字体 —— **可回退**。依据：`wv-default-theme/Cargo.toml` 只依赖 `bevy` + `wv-terrain`，**不依赖 `wv-io` / `wv-core` 领域模型**、不写世界数据、不落盘；色值/配方是运行时 uniform 与 struct，不进 YAML schema、不改 `terrain.bin` 格式、不动 §2.1 crate 边界；ADR-0043 补记明载「加新风格 = 加新 crate、零改渲染层源码」，设计上即可插拔。**注意**：`wv-world-cell-map/src/palette.rs:307` 有 `SeaRecipe` 的 1:1 镜像，改其字段需双写同步——属「可回退但需双写」，不是不可逆 |
| **`wv-io::diagnostics::{Diagnostic, Diagnostics, DiagAnchor}` 的字段集合**：所有消费方（GUI 面板 / bridge / HTTP / CLI）共同依赖 | `terrain.bin` v2（`WVTR` + `format_version=1`）：派生栅格缓存，可从栅格参数重生成（**前提**：它仍是缓存而非唯一数据源；§5 二进制门槛） |
| `PlaceEdge.derived` 用 `#[serde(skip)]` 永不落盘、`Place` 旧 `feature` 字段只读迁移窗口：写侧契约，改了会与作者手写边争真值 | HTTP 工具面「新增/更新无删除」「重跑国界只能新建图」（§12-57）：工具面能力缺口，重做不改数据 |
| —（视觉重做不新增不可逆项，见 §4.2） | 最近 25 条落在三处地形渲染 crate 的提交、`docs/adr/` 中地形表现参数类 ADR、§12-49/50/53/55/59 待裁条目：冻结即可，不产生数据负债 |

**关于被点名 ADR 的专门判断**

- `ADR-0100`~`0103` 是**纯呈现裁定**（取色分支、晕渲、读图层、LOD 阈值），全部落在右列；`ADR-0105` 之所以可回退，正是因为它在三个数据模型候选中选了 (c)「边缘带自动子图」而**否决**了会升 `terrain.bin` v3 的 (a)/(b)。
- `ADR-0054` 动的是 **crate 边界而非数据模型**，且其抬头明写「P1–P7 数据语义零触碰」。它可回退，但回退会连带修改 §2.1/§2.2 的架构基线——**这是本表里唯一「可回退但需同步改约束文档」的一项**，与右列其余「改代码即可」的项不同，须单独计价。
- 左列才是转向新重心时**真正不能碰**的东西：新校验规则只能读这些契约、不能改它们。

### 4.2 视觉重做与「转向校验」的并行性（补充两问）

**Q｜一次 Browse / Map 的画面重做，是否需要动 `wv-world-cell-map` 的地形数据模型、`terrain.bin` 格式，或 ADR-0054 的固定色 crate 级禁令？**

**答：都不需要（在 §0 前提⑤ 下）。三条证据 + 一条反例：**

| 项 | 判定 | 证据 |
|---|---|---|
| 地形数据模型 | **不动** | `wv-app/src/state.rs:528–530` 模式映射：`"browse" → ViewMode::VectorFlat`、`"map" → ViewMode::GoogleMap`、`"cellmap" → ViewMode::Topographic`——Browse / Map 是**两个独立 ViewMode**，不是 CellMap。地形数据模型（`TerrainData.cells` 的 `height/surface/biome` 三元组）在 `wv-core::terrain` / `wv-terrain`，与配色正交 |
| `terrain.bin` 格式 | **不动** | v2 是 `WVTR` + `format_version=1` 的栅格缓存（§5 二进制纪律：仅超大地图网格数组可用二进制）；改取色 / 纸纹 / 海色不读写它 |
| ADR-0054 固定色禁令 | **不推翻** | `wv-world-cell-map/src/terrain_view/field/material.rs:25–36` 明写「分叉只发生在**取色一步**」：`BiomeRaw` = CellMap 固定色块（ADR-0051 §5），`Themed` = 主题色板（**Browse 走 SeaRecipe**，ADR-0053 D7）。Browse / Map 走 `Themed` 分支，本就吃主题色，不在固定色禁令的约束面内。`wv-world-cell-map/src/shading.rs:170` 原文「**首版标定值，实机验收后可调**（改这里 = 改 Browse/Map 的海色）」——直接给出「改哪里」的落点 |

**反例（唯一会撞裁定的情形，需提前点名以免视觉方向包踩线）**：若美术要求「让 CellMap（Topographic）也吃主题色板 / 让三 tab 统一色源」，那就是**直接推翻 ADR-0054 的 crate 级硬约束**（「不允许引入 theme 的任何 crate」），须新开 ADR 并同步改 §2.1 红线表与 §2.2 断言——这已不是「画面重做」，而是架构裁定变更。只要重做范围限定在 `VectorFlat` / `GoogleMap` 两档的取色与呈现，就不触碰该禁令。

> **收口状态**：`art-model-director` 视觉方向包 v2.1 已明确「三模式共用一套画面语言 **≠** 三 tab 统一色源」，`CellPalette` 固定色作为唯一颜色例外原样保留，不走新开 ADR 流程；8 项施工无一项让 `wv-render` 读 `wv_io::diagnostics`。**故本反例已消解，不是未决风险**；若未来用户明确要求 CellMap 吃主题色板，属另一次立项，不在本轮收口范围。

**并行性结论**：视觉重做的落点是 `wv-default-theme`（色板 / `SeaRecipe` / 覆盖物规则 / 图标）、`wv-render` 取色分支与 `shading.rs` 标定值；转向校验的落点是 `wv-io/src/diagnostics.rs`。**两者改动面不相交，且都不触碰 YAML schema 与 `terrain.bin`**，因此**可以并行、互不阻塞**；唯一需要协调的是「三 tab 共用同一基底」这一用户观感问题若被解释为要统一色源，需先过一次架构裁定（见上反例）。

---

## 5. 前提假设、信息时效与评审边界

- **前提假设**（任一变化需重估）：单机单工程；YAML 为 source of truth；第一闭环以 `wv-app` egui 面板 + 跳转呈现；§2.1 依赖红线不变；1 人～小团队、低工具链优先；视觉重做限定在 Browse / Map 两档取色。
- **信息时效**：`docs/adr/` 共百余份、README §12 有数十条待裁，本评估所引的口径（尤其 §12-49/50/53/55/59 与 ADR-0100~0105 的状态）以**仓库当前文件为准**，可能随用户裁定变化；涉及平台规则、商店审核、分成比例一类的外部信息本次不涉及，若后续进入阶段 5 打包发布，须以各平台官方文档为准。
- **本轮未做**：方向判断、引擎选型（本单引擎已定，不做中立对比以避制造伪需求）、发布管线与平台合规清单（非阶段 5，不做）、视觉规格制定（归 `art-model-director`）、任何实测验证（全部落成 §3.5 待实测项，交主程序在实施阶段或 QA 出断言）。
- **四轮动作**：只读核对 + 写本文件。未改基线、未写 ADR / GLOSSARY / README / AGENTS、未改 `.rs`、未 commit、未跑全量 `cargo test`。补充轮次追加 §1.1b / §1.2 注记 / §2.5 / §4 视觉行 / §4.2，交叉复核轮次追加 §2.6，方案 A 定稿校准追加 §2.7；**未改动首轮任何结论**（C1–C5 均被后续证据加强，C6/C7 为新增，方案 A 确认 C3 的低成本口径成立）。
