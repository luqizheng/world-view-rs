# 山系笔与山脉垂直带谱 — 设计文档

- 日期：2026-10-27（brainstorming 会话收敛，用户已口头批准方向）
- 状态：设计定稿，待 builder check → ADR 落盘 → 实施计划
- 触及原则：P6/P7 不触及（数据模型零改动）；§4.2 精神（阈值常数单一来源）需在实现中守住；V1/V2 不触及（手涂语义不变）。

---

## 1. 动机

用户两条痛点（2026-10-27 会话原话）：

1. **涂抹山脉纹理难看、完全乱套**——逐格涂抹没有任何共享的走向/结构约束，出来的是补丁不是山系。
2. **一大段山脉全是一个群系「山地」**——真实山脉沿高度有垂直带谱（altitudinal zonation）：山脚衔接周边环境、山腰丘陵、高山山地、雪线以上积雪。

用户最初提议「引入地球的板块系统」来约束山脉走向。会话中已裁定（用户确认「就这么定」）：

- **不引入板块实体**。采纳板块的本质而非其物理：山脉 = 有结构的对象——轴线给走向、剖面给起伏、高度给带谱。板块只是获得轴线的手段之一，而轴线画一条线就有。
- **操作模型**：山系笔（mountain-range pen）为主力，手涂只做微调。

## 2. 现状盘点（2026-10-27 代码核实）

**大半机械已经存在，本设计的真实增量比表面看起来小：**

| 已存在 | 位置 | 状态 |
|---|---|---|
| `Intent::MountainRange { pts, radius, strength, seed }` | `wv-terrain/src/intent.rs` | 沿折线 ridged 噪声长脊、起伏连贯、两头收尖、跨海成岛链；`RANGE_PEAK_M = 2400 m × strength`，调幅 `0.78 ± 0.32` |
| kind 赋值（**只有一档**） | 同上 | 高度**增量** > `RANGE_KIND_ADD_M`(500 m) 才写 `BIOME_MOUNTAINS`，低肩保持原群系 |
| 地图级笔刷工具 `BrushTool::MountainRange` | `wv-render/src/state.rs`（`ALL[1]`） | 提交 `IntentTerrainCommand`；**一次笔触 = 一个事务 = 一次可撤销**（`wv-app/src/edit/editing/stroke.rs`） |
| 参数 UI | topOptionBar（`wv-app/src/ui/top_option_bar.rs`） | 意图档已有 半径 + 强度 两参 |
| 选区版 `Intent::MountainMassif` | `wv-terrain/src/intent.rs` | 选区内沿主轴派生脊线，复用同一 `apply_mountain_range` |
| 带谱常数 | `wv-terrain/src/continent.rs:289-293` | `SNOW_LINE_M = 2600` / `MOUNTAIN_MIN_M = 1200` / `HILL_MIN_M = 300`（`pub(crate)`，与 intent 同 crate） |
| 高度分带显示 | `wv-terrain/src/biome.rs::display_kind` | 仅对 `BIOME_NONE` 格按 2600/1200/300 回退；**贴了标签的格子「标签即显示」** |
| 手涂 | map 级 `PaintKind`；CellMap 工作台 `CellPaintValue`（surface/biome/height_delta 逐格） | 无结构——正是「乱套」的来源，降级为微调后保留 |

**缺口只有两个：**

- **G-缺口 ①**：intent 的 kind 赋值只有「山地」一档，无带谱。
- **G-缺口 ②**：「山系笔作为主力」的定位从未明确——它只是 7 块工具瓷砖里的第 2 块，与手涂 `PaintKind`（默认档）平起平坐。

## 3. 目标 / 非目标 / 反目标

**目标**

- G1 一条拖画出来的山脉具备：连贯走向（已有）+ 完整垂直带谱（新）+ 山脚跟随周边环境（低肩不写 kind，天然免费）。
- G2 手涂降级为微调工具，**语义零改动**。
- G3 带谱阈值与既有常量同源（`continent.rs` 的 2600/1200/300），任何位置不得出现第二份硬编码。

**非目标**

- N1 不做板块实体 / 板块运动模拟（2026-10-27 用户裁定）。
- N2 不新增群系词汇——无「高山草甸 / 裸岩」，**雪原封顶**（用户确认「就这么定」时未要求细分；动群系词汇表与 0–9 色板槽位的代价不值当下）。
- N3 不做纬度 / 气候调制（既有「气候带忽略」裁定）。
- N4 手涂笔刷不加走向感知（以后的事）。
- N5 渲染层预期零改动（坡度浮雕 / 分层设色跟随高度场，数据结构了纹理自然结构）；山形装饰符号与脊线对齐属后续轮。

**反目标（出现即失败）**

- A1 带谱阈值出现第二份硬编码副本。
- A2 手涂语义被改（V1/V2：作者贴的标签 = 作者意志，`display_kind` 显示跟随标签，带谱**不**回头改手涂结果）。

## 4. 设计决定（已拍板）

### D1 带谱梯

`apply_mountain_range` 的 kind 赋值从单档扩为带谱，**双判据**：

1. **门槛（要不要动你）**：只处理山系笔**实质抬升**的格子——高度增量 ≥ `RANGE_KIND_ADD_M`(500 m)。门槛之下一律不写 kind，**既有地貌的标签原样保留**（山系笔贴着旧高原/旧山体走过，不回头重贴它们的群系）。
2. **落带（动成什么）**：门槛之上按**增量后的绝对高度**落带（与 `display_kind` 的 NONE 回退口径同源）：

| 增量后高度 | 写入 kind |
|---|---|
| 刚出海、< 300（`HILL_MIN_M`） | `BIOME_GRASSLAND`（保留现状先例：「新露出水面的低肩是草原」） |
| 300 – 1200 | `BIOME_HILLS` |
| 1200 – 2600 | `BIOME_MOUNTAINS` |
| ≥ 2600（`SNOW_LINE_M`） | `BIOME_SNOWFIELD` |

- 旧 `RANGE_KIND_ADD_M` 的语义从「写山地的阈值」变为「带谱的实质抬升门槛」，常量沿用、含义收编，不新增第二份阈值。
- **裁定记录（2026-10-27， brainstorm 会话推导自 A2，builder check Q1/Q2 的收口）**：曾考虑「纯绝对高度判据」（山系笔 reach 内所有格子按落带重贴，含既有高原边缘）——否决，因其违反 A2（带谱不回头改非本次创作的结果）。出水低肩写草原沿用既有代码先例而非字面执行「不写」。
- 水体不写 kind：跨海段海底若仍 < 0 保持水体；抬升出水的格子按陆地带谱走（岛链山坡同样有带谱）。
- **雪帽稀疏属预期**：默认强度 1.0 下脊线中心 1872–2688 m（调幅所致），只有部分高峰过雪线——这是「走向噪声带来峰间差异」的正确表现。若实机验收觉得雪太少，调 `RANGE_PEAK_M` 或默认 strength，**不动判据口径**。

### D2 生效面

山系（`MountainRange`）与高山群（`MountainMassif`）同享带谱（共用 `apply_mountain_range`，天然同步）。火山 / 高原 / 丘陵意图的 kind 口径本次**不动**（它们有自己的语义，先不动免scope扩张）。

### D3 主力编排

- 山系笔 = 既有 `BrushTool::MountainRange`，**不改枚举顺序**（`ALL` 下标即 `CommandId::SelectBrushTool` 绑定，只增不改）。
- i18n 文案改「山系笔」语义（中英两键），瓷砖图标可沿用现图标（不强制换）。
- 手涂 `PaintKind` 文案补「微调」提示（tooltip 一行，Key 复用新增）。
- CellMap 工作台不新增入口——它是微调面板；山系笔入口 = 现有三处（地图工具条瓷砖 / 命令面板 / 快捷键）。

### D4 参数

radius / strength 沿用现状（topOptionBar 已有），seed 沿用笔触种子。**不新增参数。**

### D5 撤销与落盘

一次拖画 = 一个 `IntentTerrainCommand` 事务（现状，明确保留）。数据模型零改动，YAML/terrain.bin 格式不动。

## 5. 留给 builder 的自由度

- 改动面预计：`wv-terrain/src/intent.rs`（带谱函数 + 测试）、`wv-app` i18n 键、topOptionBar 提示 Key。`wv-render` / `wv-world-cell-map` 预计零改动——若发现必须改，先停下来对照 §3 反目标。
- 测试（AGENTS §7.2 口径）：**手算期望值**断言四档边界（299/300/1199/1200/2599/2600 附近的格子 kind）+ 门槛边界（增量恰在 500 上下）；**既有高原被山系笔掠过时标签不变**（门槛保护，builder check Q1 的回归守点）；低肩保持原群系（沙漠里画山 → 山脚是沙漠）；跨海段岛链（出水低肩 = 草原）；`MountainMassif` 同享断言。round-trip 不受影响（数据模型未动）。
- 常量可见性（`pub(crate)` 是否够 / 是否 re-export）由 builder 定。
- 术语登记（实施时）：GLOSSARY 补「山系笔 (mountain-range pen)」「垂直带谱 (altitudinal zonation)」。
- ADR：本设计须配 ADR（编号按 `docs/adr/README.md` 顺延），四件事齐：处境 / 决策 / 理由 / 代价 + 被否决的备选（板块系统、新增群系词汇、增量判据保留）。**起草后待用户落盘确认，不越权提交。**

## 6. 已知不确定（builder 核实项，不阻塞设计）

1. 山系笔在**世界地图**（world_main，cell_size 大）上的半径/强度手感——实机验证项。
2. 世界工作台工作区下地图级笔刷路径是否可用（工作区切换的门控）；若不可用，是否需要在工作台暴露山系笔入口——**此项若成立，带回来问用户，勿自行扩 scope**。
3. 默认强度下雪帽比例的实机观感（D1 已给调参指引）。

## 7. 验收口径

- 实机：在 jrpg-world 世界图上拖一条横穿森林与草原的山系 → 山脚各自衔接森林/草原、山腰丘陵、山脊山地、个别高峰带雪帽；山系笔掠过既有高原 → 高原群系原样不动；一次 Ctrl-Z 整条消失。
- 门禁：`bash scripts/cargo-test.sh`（环境 temp 约束）+ clippy + fmt 全绿；触 feature 门内文件则 `cargo app-test` / `cargo app-clippy`；触系统签名则 `bash scripts/smoke-gui.sh`。
