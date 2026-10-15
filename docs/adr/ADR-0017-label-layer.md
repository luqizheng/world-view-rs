# ADR-0017：标签层——实体在地图上的文本投影

**状态**：已接受（Accepted，2026-10-14 用户逐条拍板 Q1–Q5）
**日期**：2026-10-14
**决策者**：用户
**关联**：P1（单机可运行）、P3（YAML 为主格式）、P4（多地图层级）、P5（图层化编辑）、P6（非破坏编辑）、P7（时间轴驱动）、AGENTS §3（数据与渲染分离）、§3.3（System 不得改世界）、§7.5 V2（历史可重写）、[ADR-0009](ADR-0009-graded-tile-zoom-to-street.md)（`LodTier` / `LevelZoom`）、[ADR-0010](ADR-0010-street-vector-layer.md)（街道名标注先例）、[ADR-0013](ADR-0013-layer-frame-binding.md)（`LayerFrame` 求值点 + 主题系统）、[ADR-0015](ADR-0015-city-submap-time-versioning.md)（`MapNode.time_span` / `StreetSegment.time_span`）、`GLOSSARY.md`（标签投影 / 锚点策略 / 字号阶梯）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

> **编号追记（2026-10-14，合并修订）**：本 ADR 在 `feat/label-layer` 分支起草时
> 编号为 ADR-0016。合并回 main 时发现 main 侧 [ADR-0016](ADR-0016-jrpg-antique-map-default-theme.md)
> （古舆图主题）已被先行接受并占用该编号——按 §2「永不重号」纪律，本 ADR
> **重编号为 ADR-0017**，内容逐字保留（仅编号与文内自引用替换）。
> 代码注释与文档中的「标签层 ADR-0016」一律指本 ADR（ADR-0017）。

---

## 1. 处境（Context）

`wv-app` 的地图目前能画**实体几何**（地点、街道、势力、事件叠加、栅格地形），但**不能画实体名字**——而名字是地图对读者最直接的入口。

两条已有线索，但都未成体系：

1. **街道名**（ADR-0010 Phase 5）：`wv-app/streets.rs` 的 `update_street_labels` 已能在 `LodTier::Edit` 时显示 `StreetSegment.name`——但只覆盖街道，孤立于其他实体。
2. **地名**：在 `vector_layers.rs` 的 `PlaceLabel` 组件池里有类似管线——但同样是孤立的，**与街道名是两套机制**。

调用层、配置、可见性规则、字体、碰撞、点击交互——这些**都各自写一遍**，等遇到第二个实体类型（Region）就要第三次。

更要紧的：**当前所有标签都是「装饰」**——`PlaceLabel` 是个 `Component`、街道名是个 `Component`，点击它们没有"跳到该实体"的语义。这与世界观工具的定位（**让人能顺着地图读世界**）不符：标签是导航入口，不是注释。

按 AGENTS §3「数据与渲染分离」与 P3（YAML 为主格式），标签**不能是作者在地图上手摆的自由文本**——那等于把"自由文本"和"实体名"两个真源并列，迟早漂移。

所以需要一次性把这件事拍板：**标签是实体的派生，不是有作者的输入。**

## 2. 决策（Decision）

### 2.1 图层挂载点

- `LayerKind` 枚举新增 `Label` 变体（与 `Terrain` / `Road` / `Place` / `Influence` / `EventPoint` / `Border` / `Custom` 平行）。
- `LayerFrame`（ADR-0013）的 `kind → 渲染组` 映射追加 `Label` → 标签渲染组；`visible` 与 `opacity` 与既有层同语义（YAML 声明 ∧ 会话开关）。
- `Label` 图层文件**只存配置**——`kind` / `entity_kinds` / `min_zoom_tier` / `font_size_ladder` / `max_labels` / `priority_kinds`。**不存文本本身**（见 §3 「不存文本」）。

### 2.2 数据源（实体白名单）

- `LabelLayerConfig.entity_kinds: Vec<EntityKind>`（白名单）——作者声明"我要把哪几类实体的名字显示在地图上"。
- 支持的实体类型（v0.1）：
  - `Place`（点 / 城市 / 村庄 / 地标 / 首都等所有 PlaceKind）
  - `Region`（几何中心）
  - `Faction`（势力面中心）
  - `StreetSegment`（沿用 ADR-0010 的 `polyline_midpoint()` 锚点）
  - `Event`（事件点位置；只在 `EventPoint` 图层开启时同步可见）
- `entity_kinds` 为空 = 该图层不画任何标签（与"隐藏"语义区分——空 = 该层未启用）。

### 2.3 可见性求值（与 ADR-0015 同构）

单一实现点：

```text
fn label_visible_at(
    entity_kind: EntityKind,
    entity: &EntityRef,
    map_span: Option<&TimeSpan>,    // 来自 MapNode.time_span
    t: i64,
) -> bool
```

- `Place` / `Character` / `Faction`：走 `TimelineQuery::at(entity.state_spans, t)`（既有 P7 入口）
- `StreetSegment`：`seg.visible_at(map_span, t)`（ADR-0015 已落地）
- `Region`：暂无 `state_spans` 字段（v0.1 不带时间版本化，永久可见）→ **未来 Region 加 `state_spans` 时无缝接入本函数**
- `Event`：从 `event.time` 派生——事件发生瞬间的窗口由 `Event.duration` 决定（v0.1 用默认 `duration = 1` 天）

公式：与 `street_visible_at` 同构——`entity_visible_at(span) ∧ map_visible_at(span)`。`LabelLayerConfig.entity_kinds` 不参与求值，是白名单过滤。

### 2.4 字号阶梯（与 `LodTier` 挂钩）

- `font_size_ladder: BTreeMap<LodTier, f32>`——`Nation` / `Region` / `Edit` 三档各给一字号（屏幕像素）。
- 缺省值（v0.1）：
  - `Nation: 9.0`
  - `Region: 11.0`
  - `Edit: 14.0`
- 字号 = 屏幕像素 × `view.scale`（与 ADR-0008 §8 的 `px * view.scale` 同口径）——远观小字、近观大字。
- 作者不声明 ladder ⇒ 用缺省；声明则全权覆盖。

### 2.5 锚点策略

| 实体类型 | 锚点 | 计算位置 |
|---|---|---|
| `Place` | 实体点（`Place.position`） | 直接读 |
| `Region` | 几何中心 | 首次访问时缓存（`Region.label_anchor: Option<Point>`，缺省派生） |
| `Faction` | 势力面中心 | 同 Region |
| `StreetSegment` | 折线半程（ADR-0010 `polyline_midpoint()`） | 已实现 |
| `Event` | 事件点位置（`Event.location.center`） | 直接读 |

显式 `Region.label_anchor` 压过派生；`label_anchor` 缺省走几何中心（多边形顶点算质心或外包盒中心，**最容易回退**的形态）。

### 2.6 碰撞与优先级

- **v0.1 碰撞** = 屏幕像素 **AABB**（按字号 × 文本宽估值，AABB 排序 + 贪心）。
- 冲突时**按优先级保留**：优先级 = `PlaceKind` 权重（`Capital > Major > City > Town > Village > Landmark > …`）> 文本长度（更长的优先）> 词典序。
- 优先级配置 = `LabelLayerConfig.priority_kinds: Vec<PlaceKind>`——作者可重排。
- **超限截断** = `max_labels: Option<usize>`（默认 `200`），按优先级排序后取前 N——**不**在视口外多算。

### 2.7 交互（导航入口）

- **点击标签** → 派发 `SelectEntityCommand { kind, id }` → 选中状态写入 `WorldProjection.selection` → 详情面板打开该实体。
- **悬停标签** → 高亮 + 浮动提示（实体名 + 关键属性一行，如 "清云城 · 首都 · 1200–1450"）。
- 点击 / 悬停的范围 = 标签文字的 AABB 矩形（命中检测与渲染同源）。
- 标签点击**不**穿透到下面的地图（避免误钻入 / 误选 POI）。

### 2.8 字体

- 字体由主题决定（沿用 ADR-0013 主题系统与 `theme/fonts`）。
- 标签**不**做 per-label 字体 override——保持主题切换语义一致。
- 字号由本层配置（`font_size_ladder`）控制，不与字体耦合。

### 2.9 与既有街道名标注的关系

- `wv-app/streets.rs::update_street_labels`（ADR-0010 Phase 5）→ 由本 ADR **接管**为统一管线入口。
- 接管不是删除：函数保留作为低层渲染实现，调用方改为 `LabelLayer` 经 `LayerFrame` 调度。
- 既有 `StreetLabel` 组件池与 `StreetEditorState.name` 面板**不动**——它们是数据 + 编辑入口，渲染走新管线。

### 2.10 跨子图（明示不在 scope）

- 标签**仅在所属 `MapNode` 内渲染**——父图不显示子图的标签。
- 父图的"摘要标签"是另一个 ADR scope（涉及子图清单、钻入引导；当前 v0.1 不引入）。

## 3. 理由（Rationale）

- **为什么不是"作者摆的文本"**：违背 P3 + P4。文本是实体的属性，不是地图上的装饰；让作者在地图上手摆会与"实体改名"漂移。
- **为什么单一函数 `label_visible_at` 而不是各自求值**：与 `street_visible_at`（ADR-0015）同构——所有"实体的派生可见性"沿同一条链走，便于将来 Region / Event 加 `state_spans` 时**无侵入接入**。
- **为什么字号阶梯绑 `LodTier` 而不是 `view.scale`**：阶梯是**离散的、与数据层对齐的**——`view.scale` 连续，但缩放是分级动画（ADR-0009），连续值会跳。LodTier 是稳定档。
- **为什么 v0.1 用 AABB 而不是 R-tree / 力导**：单图 200 上限的贪心 AABB 已够用；R-tree 是性能优化（>2000 标签时考虑），不是 v0.1 必备。**最容易回退**的形态。
- **为什么点击派发 `EditCommand`**：与"双击钻入"同链路，标签 = 导航入口，不是装饰——这正是 worldview 比 Inkarnate 多的那一层。P3 不破（点击不写领域）。
- **为什么接管街道名而不是并行**：双套标签管线是技术债，迟早合并——在 v0.1 接管成本最低（街道名标注只覆盖 `LodTier::Edit`，新管线天然兼容）。
- **为什么不上本地化**：v0.1 单语言（中文 / 英文任作者选）；多语言是**实体属性**（`Place.name_i18n: HashMap<Locale, String>`）的扩展，不属本 ADR。

## 4. 代价（Consequences）

- **正**：标签 = 实体派生 = 改实体名标签自动更新 = 时间滑块拖到 1100 年标签自然消失 = 点击标签跳到实体详情。Inkarnate 学不来的导航深度。
- **正**：P5 兑现到"作者可声明哪些实体投影成标签"，与既有层同框架。
- **负①（数据层新增枚举变体）**：`LayerKind` 加 `Label` 变体——serde 兼容（`#[serde(rename_all = "snake_case")]` + 新增 `label` 字符串），既有 YAML 不破。
- **负②（接入点扩散）**：`LayerFrame` 求值点 + `vector_layers.rs` / `streets.rs` 多处需要按 `Label` 分支——属于"接通数据层"性质（与 ADR-0013 同型），按既有纪律做即可。
- **负③（Region / Event 暂无 `state_spans`）**：v0.1 这两类标签永久可见。属于**最容易回退**的形态——未来加字段时无缝接入。
- **负④（碰撞算法 V1 简单）**：AABB + 贪心在标签密集区可能漏放最优解。登记 README §12 第 33 条待决——R-tree / 力导是 v0.2+ 优化。
- **负⑤（接管街道名标注）**：行为变化——当前 `roads_visible: false` 时**不**隐藏街道名（ADR-0010 已知）；接管后**会**隐藏（与 Google Maps 标注-要素同步的语义一致）。依赖旧行为的作者会看到差异。

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **作者手摆自由文本** | 违反 P3 + P4；与"实体改名"漂移；与 Inkarnate 同质化（无差异化） |
| **标签作为 `MapNode` 新字段**（如 `labels: Vec<LabelSpec>`）| 与 `Layer` 框架平行，绕过 P5；与"图层开关、z、opacity"分裂 |
| **每个实体类型各自一套标签管线** | 街道名、地名、势力名各写一遍——已经发生；本 ADR 正是要合并 |
| **字号 = 屏幕像素常量，不分 LOD** | 远观一屏字海、近观字偏小；分档更友好 |
| **点击标签 = 高亮 POI** | 标签是独立实体，不一定对应 POI；语义错位 |
| **标签不接管街道名标注** | 双套管线是技术债；接管成本最低 |
| **R-tree 碰撞 / 力导布局** | 200 标签内 AABB 贪心已够；过早优化 |
| **v0.1 上多语言标签** | 实体属性扩展项，不属本 ADR；过早引入 |

## 6. 待裁（Open Questions）

- **Q1**：`LayerKind::Label` 是否进 `LayerFrame` 的 `kind → 渲染组` 映射？**当前按"进"**——作为独立渲染组；`z` 走 ADR-0013 的槽位偏移口径。
- **Q2**：标签 `z` 默认 = 标注带（0.35–0.4），高于 `Place` 图层？**当前按"高于"**——地名应在 POI 之上。
- **Q3**：接管街道名标注的范围——`LodTier::Edit` 之外（`Region` / `Nation`）是否也由新管线接管？**当前按"全接管"**——统一档位阶梯。
- **Q4**：`max_labels` 默认 200 是否合适？**当前按 200**——城市级实测量级；超大城市可调高。
- **Q5**：标签字体是否支持主题内 per-`PlaceKind` 映射（如首都用衬线 / 村庄用无衬线）？**当前按"不支持"**——单一主题字体；后续走主题扩展。

后续若发现 v0.1 不足，按 ADR-0013 / 0012 / 0015 模式以新 ADR 增补（如 v0.2 把跨级摘要标签补齐时新开 ADR-0018）。

## 6.1 收敛（2026-10-14）：Q1–Q5 逐条拍板

> 依 AGENTS.md §6.3「追加不覆盖」**追加**。§2 / §3 / §4 / §5 正文不变；本节落实
> §6 的 5 条待裁的最终选择，并把它们从「待裁」转入「决策」。

| # | 议题 | 拍板 | 与原 §2 / §6 推荐的关系 |
|---|---|---|---|
| **Q1** | `LayerKind::Label` 是否进 `LayerFrame` 的 `kind → 渲染组` 映射？ | **进**（独立渲染组） | 与 §2.1 / §6 推荐一致 |
| **Q2** | 标签 `z` 默认值？ | **0.35–0.40**（标注带，高于 Place） | 与 §2.1 / §6 推荐一致 |
| **Q3** | 接管街道名标注的范围？ | **全接管**（含 `LodTier::Region` / `Nation`） | 与 §2.9 / §6 推荐一致——`wv-app/streets.rs::update_street_labels` 整张被新管线接管，旧 `LodTier::Edit`-only 行为是临时方案 |
| **Q4** | `max_labels` 默认值？ | **200**（城市级实测量级） | 与 §2.6 / §6 推荐一致 |
| **Q5** | 标签字体是否支持主题内 per-`PlaceKind` 映射？ | **不支持**（单一主题字体） | 与 §2.8 / §6 推荐一致——per-`PlaceKind` 字体走后续主题扩展（独立 ADR） |

**后续约束**：上述 5 条与 §2 决策构成 ADR-0017 的全部口径。实现期若发现任一条
不适用，按 AGENTS §6.4 新开 ADR 修正（**不**改本 ADR 正文）。
