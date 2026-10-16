# ADR-0021：表达配方（RenderRecipe）与装饰族（Treatment）——渲染判定的收口与手绘表现的预设化

**状态**：提议（Proposed，待用户拍板）
> **2026-10-16 修订追记**：本 ADR 的 **D2（Treatment 独立轴）已被 [ADR-0023](ADR-0023-theme-pack-injection-contract.md) D4（已接受）修订**——`TreatmentFlags` 归入 `ThemePack` 字段（`wv-render/src/theme/theme_pack.rs`），**不再**经 `view.treatment` 独立持有、**不再**做独立轴切换。本 ADR D2 的「特性归轴表」（§2 D2 §4）与「HandDrawn 默认零回归」前提继续有效；两档预设改由主题 crate 承载（`HandDrawn` = 内置古舆图包、`Skyline` = 未来独立主题 crate）。**D1（Recipe 收口）不受影响，仍独立待拍板**（ADR-0023 §3「先后顺序」明确 0023 落地后 0021 更容易做）。
**日期**：2026-10-16
**决策者**：待定（源自 2026-10-16「地图的各种表达方式，能不能给出建议」调研的后续项；ADR-0020 §2.8 明确留予本 ADR）
**关联**：P3（YAML 为主格式，仅作潜力方向）/ P5（图层化编辑）、AGENTS §3（数据与渲染分离）/ §6.4、ADR-0012（VectorFlat 默认底图）/ 0013（LayerFrame 唯一求值点）/ 0016（主题轴与正交性）/ 0017（标签层）/ 0020（wv-render 独立 crate，§2.8 留项）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

「地图的各种表达方式」调研（2026-10-16）确认：最近 30 个 commit 中 4 个 commit 群组（海洋色 4 层叠加 / 远缩放要素被盖 / 底图建模 / 手绘呈现 14 项）在打**同一个根因**——「表达」这一层没有抽象，被散落到多个文件与多个状态容器里。ADR-0020 已把渲染层物理收进 `wv-render`（改动面收窄），本 ADR 处理其中的**内部抽象**。

### 1.1 症状 A：「该不该画」的判定散落各处（实测）

`view.mode` 判定点当前**20+ 处、跨 8 个模块**：

| 文件 | 位置 | 判定内容 |
|---|---|---|
| `buildings.rs` | :212 | `GoogleMap \| Coastline` → 不画建筑 |
| `atmosphere.rs` | :209 | `GoogleMap \| Coastline` → 不铺纸面氛围（雾霭 + 噪点）|
| `terrain_view/isoline_overlay.rs` | :180 | 仅 `VectorFlat` → 画等值线 + 水彩晕 |
| `faction_overlays.rs` | :134 | 仅 `Faction` → 画势力多边形 |
| `terrain_view/cells.rs` | :340 | `soften && !(Coastline \| GoogleMap)` |
| `terrain_view/cells.rs` | :343 | `wobble: !(GoogleMap \| VectorFlat)` |
| `terrain_chunks.rs` | :307 | `google_mode` 分支出不同的分块策略 |
| `vector_layers.rs` | :243/:250/:257 | 按模式选道路/POI 色板 |
| `drill.rs` / `streets.rs` | 8 处 | 标注 label/halo 色按模式 `match` |

**同一「模式排除对」（`GoogleMap | Coastline`）在 buildings 与 atmosphere 两处重复出现**——新加一个模式时，哪些模块要排除它、排除清单是什么，没有单一真源；靠读代码、靠记忆。

### 1.2 症状 B：手绘表现族 14 项恒开、无族抽象

「手绘地图呈现」系列 14 个 commit 的产出**全部恒开**（默认观感即手绘），每一项的开关/参数都是各模块内的常量或散点判定：

| # | commit | 特性 | 现状位置 |
|---|---|---|---|
| 1 | 65b9373 | 海色按高度渐变 | `cells.rs` `water_shade_at` |
| 2 | 865bc57 | 标注三级字重梯度 | `label_collision` 字号阶梯 |
| 3 | 386571d | 雪独立色 + 冷调阴影 | `cells.rs` `snow_shade_at` |
| 4 | 45bd159 | 海岸线多层水彩晕（内/外两圈） | `isoline_overlay.rs` `HALO_1/2_*` |
| 5 | cd4aa40 | 海冰层 | `cells.rs` |
| 6 | d32c590 | 纸纹缩放微调 grain uv | `atmosphere.rs` `GRAIN_*` |
| 7 | 5b25f90 | POI 悬停气泡 | `ui/hover_tooltip.rs`（UI 侧）|
| 8 | 875bb2f | 河水中线 | `isoline_overlay.rs` `RIVER_*` |
| 9 | 726cd0f | POI 圆点色进主题调色板 | `palette.rs` / `drill.rs` |
| 10 | 93becde | 墨线亚线性缩放 | `isoline_overlay.rs` |
| 11 | f86c937 | 笔刷内环「已下笔」高光 | `brush_cursor.rs` |
| 12 | f44240c | POI hover 文字色按 kind | `ui/hover_tooltip.rs` |
| 13 | da655e7 | 海名 entity（数据层） | `wv-core::sea::Sea` |
| 14 | 3ae74e7 | 海名渲染 | `drill.rs` `update_sea_labels` |

没有「干净矢量观感」这一档——想要无纸感、无水彩、无扰动的清爽地图，当前**无法切换**；将来「加一种观感族」（冬日 / 海岸主题……）又要逐模块加 `if`。

### 1.3 已知不一致（散落的直接代价）

`terrain_soften`（底图淡化开关）在 `Coastline | GoogleMap` 被静默忽略；`wobble` 在 `GoogleMap | VectorFlat` 被静默忽略；两处 `GoogleMap | Coastline` 排除对重复书写。这些**不是 bug**（都是有意的设计折衷），但它们的存在方式（散点 `matches!`）意味着**改一处漏一处**。

### 1.4 边界声明（不在本 ADR）

- **`MapBase::Ocean.cell() = -3500` 与 `water_shade_at(-3500) → deep_ocean_blue` 的反向耦合**：数据层常量反向依赖渲染公式，是独立议题，另开 ADR 处理（拟 ADR-0022）。
- **渲染栈（指令列表重写全部渲染模块）**：调研方案乙，已否决（见 §5）。
- **Recipe 的 YAML 化**：P3 潜力方向，本 ADR 明确推迟（§4 待裁）。

---

## 2. 决策（Decision）

两项决定（D1 / D2）**可独立接受或否决**。

### D1 表达配方 RenderRecipe（渲染判定的单点求值）

1. 新增 `wv-render/src/recipe.rs`：每帧由 `resolve_render_recipe` 求值一次，输出 `RenderRecipe` 资源。求值是**纯函数**（输入全部来自资源，无副作用），可脱离 Bevy App 单测。
2. 输入：`ViewMode`（技法）、`MapPalette`（主题）、`Treatment`（装饰族，D2 引入；D1 先行时恒为默认档）、`LodTier`、`LayerFrame`（ADR-0013 的 kind 轴）、`ViewState` 会话开关。
3. 输出：

   ```rust
   pub struct RenderRecipe {
       passes: [PassPlan; PassId::COUNT],   // 每条 pass 的元信息
       pub treatment: TreatmentFlags,       // 装饰旗标（D2 的载荷）
   }
   pub struct PassPlan { pub enabled: bool }
   pub enum PassId { Terrain, Isoline, GridLines, MapFrame, Veil, Grain,
                     Roads, Streets, Poi, Buildings, Faction,
                     PlaceLabels, StreetLabels, SeaLabels,
                     Measurement, BrushCursor, Tracks, EventDots, DrillMarkers }
   ```
4. **各 sync 系统的 gate 改读配方**：散落的 `if view.mode == X || !layers.y.visible || …` 收敛为 `if !recipe.pass(PassId::Z).enabled { return; }` + 该 pass 自身的几何/材质判断（后者留在原地——配方管**开不开**，不管**怎么画**）。
5. **与 LayerFrame 的关系（关键不变量）**：LayerFrame 继续是「YAML 声明 × 会话开关」的 **kind 轴**唯一求值点（ADR-0013 不动）；Recipe 是「技法 × 主题 × 装饰 × 缩放」的**模式轴**求值点，把 LayerFrame 当输入之一消费。**两轴正交、各为单点、不合并、不双真相**。z 序仍由各渲染组内建槽位 + LayerFrame 的 `z_offset` 决定（ADR-0013 §2.4 语义不动），Recipe 不碰 Z。
6. **参数类型化，禁止 `serde_json::Value` 参数袋**（原方案丙的风险点，显式否决）。每 pass 需要的新参数进 `RenderRecipe` 的具名字段或对应 pass 的 dedicated struct。
7. 已知不一致（§1.3）转为配方表项：`soften 生效模式 = {Schematic, RealTerrain, VectorFlat?…}`、`veil 排除 = {GoogleMap, Coastline}`、`wobble 排除 = …` 全部成为**表里的一行列举**，新增模式时改表即生效、漏改会被测试抓住。

### D2 装饰族 Treatment（表达的第三轴）

1. 表达三轴命名定稿：

   | 轴 | 类型 | 现值 | 语义 |
   |---|---|---|---|
   | **技法** Technique | `ViewMode`（不动）| VectorFlat / Schematic / … | 用什么管径画地图 |
   | **风格** Theme | `MapPalette`（不动）| Daylight / Dusk | 用什么配色 |
   | **装饰** Treatment | **新增** `Treatment` 枚举 | `HandDrawn`（默认）/ `Skyline` | 叠多少手绘装饰 |

2. 首版两档：
   - `HandDrawn`——**默认**，= 现观感（14 项手绘特性全开，零回归）；
   - `Skyline`——干净矢量观感：关水彩晕/雪调/海冰/河中线/纸纹/边界扰动等装饰，保留基础几何描边与平涂。
3. **装饰旗标经 Recipe 下发**（D1 的 `TreatmentFlags`）：各渲染点从「恒开」改为「读旗标」——例如 `isoline_overlay` 读 `recipe.treatment().coast_halo`、`cells` 读 `recipe.treatment().water_gradient`。**新增装饰 = 加旗标 + 在渲染点读它**，不再散落 `if`。
4. 特性归轴（初版口径，M2 落地时细化）：
   - **进 Treatment**：水彩晕（#4）、河中线（#8）、海冰（#5）、海色渐变（#1）、雪调（#3）、纸纹（#6）、雾霭、边界扰动（wobble）、墨线亚线性（#10）；
   - **不走 Treatment**（归各自既有轴）：标注字重（#2，随 Label 层）、POI 行为与 hover（#7/#9/#12，随 Place 层 / UI）、海名（#13/#14，是**内容**不是装饰，随 Sea/Label）、笔刷内环（#11，编辑态反馈非观感——**待裁**）。
5. **持久化 v1**：与 `map_theme` 同处（`ui-layout.yaml`），复用既有接线；「图级声明」（每张图自带 Treatment，写进地图 YAML）**推迟**，见 §4 待裁。
6. 默认档为 `HandDrawn` 时，**既有工程渲染逐位不变**（零回归是接受本决策的前提条件）。

### 分期（两项决定各自可先行）

- **M1（D1 单独接受即可开工）**：Recipe 骨架 + 20+ 判定点收口 + §1.3 不一致转表。不引入 Treatment（`TreatmentFlags` 全部恒 `true`）。
- **M2（D2 接受后）**：Treatment 枚举 + 两档预设 + 旗标下发 + 顶栏/命令面板入口。

---

## 3. 理由（Rationale）

- **为什么值得收口**：§1.1 的「模式排除对」重复书写已经是事实；调研确认的 4 组零散补丁中至少 2 组（海洋色稀释、远缩放要素被盖）的排查困难部分来自「判定无处可看」。单点表让「这一帧为什么画/不画 X」可回答、可测试、可 diff。
- **为什么 Recipe 与 LayerFrame 不合并**：两者的输入与消费者都不同（kind×YAML vs 模式×装饰），合并会产生「一个求值点做两件事」的耦合；正交双单点是本项目 ADR-0013 单点模式的自然延伸。
- **为什么 Treatment 独立轴而非 ViewMode 新成员**：`ViewMode × Treatment` 若并入单枚举是笛卡尔积爆炸（7 技法 × N 装饰族），且语义正交（同一「矢量地形」既可手绘也可清爽）。
- **为什么默认 HandDrawn**：零回归是硬前提（ADR-0020 闸门 8 的同类纪律）；「干净观感」是**新增能力**而非行为改变。
- **为什么 M1/M2 分期**：Recipe 是内部重构（无用户可见变化），Treatment 是用户能力（可见）；分期的回退粒度与验收标准都更清晰。
- **为什么先不做 YAML 化**：AGENTS §12 —— 收敛前选**最容易回退**的方案。Rust 常量表先立住语义，YAML 化等作者真的想编辑配方时再议。

## 4. 代价（Consequences）

- **正**：
  1. 「画什么」有单一真源，新模式接入 = 表加列 + 测试覆盖一行；
  2. §1.3 三处不一致转为表项，漏改可被单测抓住；
  3. Treatment 给 14 项手绘投资一个收拢点，`Skyline` 是第一个可交付的「另一种观感」；
  4. Recipe 纯函数可脱离 Bevy 单测（AGENTS §7.1 渲染层测试缺口的局部补强）。
- **负**：
  1. **新间接层**：每 sync 系统多一次查表（编译期数组索引，成本可忽略，但阅读多一跳）；
  2. **~10 模块的 gate 行机械改写**（M1 的一次性面）；
  3. **Recipe 管不到 pass 内部构成**：「海洋色 4 层叠加」那类问题是单 pass 内部的调色链，Recipe 只让「哪几层参与」显式（经 TreatmentFlags），**不**重构调色公式本身——对本类问题的收益是部分的，需诚实声明；
  4. 与 LayerFrame 的正交边界靠文档 + 测试维持，违反无编译期强制。
- **待裁（落地前收敛）**：
  | 子点 | 选项 |
  |---|---|
  | Treatment 默认档 | A. HandDrawn（推荐，零回归）/ B. Skyline |
  | 粒度 | A. 整族两档（推荐，v1 简单）/ B. 逐项子开关 |
  | 持久化位置 | A. ui-layout.yaml（推荐，与 map_theme 同处）/ B. 图 YAML（作者意图）/ C. 暂不持久化 |
  | 笔刷内环（#11）归 Treatment 还是留编辑态 | 待 M2 |
  | Recipe 是否预留 YAML 化接口（serde 字段）| 待 M1 落地时评估 |

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **A. 渲染栈（`Vec<Box<dyn DrawPass>>` 指令列表）** | 调研方案乙：拆 pass 是事实上的「重写全部渲染模块」，且早期引入与 LayerFrame 双轨真相期；代价远超收益。 |
| **B. `serde_json::Value` 参数袋** | 类型失控，后期失去编译期保障；D1 §2.6 显式否决。 |
| **C. Treatment 并入 `ViewMode`** | 笛卡尔积爆炸 + 语义正交性破坏（违反 ADR-0016 §2.2 确立的轴正交纪律）。 |
| **D. 合并 Recipe 进 LayerFrame** | kind 轴与模式轴输入/消费者不同；合并产生双重职责求值点，违反 ADR-0013「单点做一件事」。 |
| **E. 立即把 Recipe YAML 化** | AGENTS §12：收敛前选最易回退方案；Rust 常量先行。 |
| **F. 不做任何抽象，维持散点判定** | 症状 A/B 是已发生的事实（30 commit 中的 4 组补丁），维持现状 = 继续付散落成本。 |

## 6. 落地闸门（若接受）

1. M1：Recipe 纯函数单测（模式 × 装饰 × 缩放矩阵的 enabled 真值表）+ 现有 20+ 判定点收口后**全量回归**（`cargo test -p wv-render --features app` + GUI 烟测）；
2. M2：`HandDrawn` 档下渲染**与现状逐位一致**（截图 hash 或像素断言）；`Skyline` 档人工验收；
3. 两阶段各自独立 commit、独立可 revert。

---

**相关文档**：`docs/adr/ADR-0013-layer-frame-binding.md`（kind 轴单点）、`docs/adr/ADR-0016-jrpg-antique-map-default-theme.md`（主题轴正交先例）、`docs/adr/ADR-0020-wv-render-crate.md`（§2.8 留项）
