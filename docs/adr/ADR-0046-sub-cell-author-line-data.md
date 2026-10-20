# ADR-0046: 亚格线状作者数据——手绘小河（小溪 / 小河）

> 状态：**已接受（Accepted）**（2026-10-20 用户拍板立项：「好」——作者小河走 ADR-0010 的作者数据路径）
> 关联：ADR-0037 D5（世界模板的 `rivers_seed` 派生河流——本 ADR **不取代**它，只补作者侧）、ADR-0010（街道矢量层——「双供给」先例）、ADR-0045（自然区域——河岸覆盖物走 region 声明）、ADR-0029（生物群系标记——边界快照先例）、GLOSSARY「街道矢量层 / 双供给 / 地物线」
> 原则：P2 / P3 / P4 / P6 / P7

## 处境

小溪、小河**小到不可能占据一个 cell**——作者要表达"这里有条溪"时，现有工具没有落点。
现状是两条互不相干的路：

| 现状 | 是什么 | 能否手绘 |
|---|---|---|
| **派生河流**（ADR-0037 D5） | `terrain + rivers_seed` → `wv_terrain::climate::generate_rivers` → 折线（`Vec<Vec<[f32;2]>>`）→ `ProjectionFrame.river_polylines` → `PassId::Rivers`（Z 0.082） | ❌ 确定性派生、**不落盘、不绑 Place、作者改不了单条河**（ADR-0037 D5 明写「代价是作者暂不能手改单条河」） |
| **笔刷河流**（`wv-terrain::intent::Intent::River`） | 从源头拖到河口，程序密采点落成格链，**河宽固定一格**，写格子（水面 + `BIOME_RIVER`） | ⚠️ 能画，但**粗**：一格宽、占格、不能蜿蜒表达一条溪 |

作者原话：「小溪，小河流。他们小到不可能占据一个 cell，所以要用**覆盖物 + place 对象**定义他们」。
即：**一个线状作者实体 + 一个 Place 锚点**。

ADR-0037 D5 已把方向指到 ADR-0010 的作者数据路径（街道那套 YAML 折线 + 双供给），
但**未立项**——本 ADR 补上。

## 决策

### D1 · 载体 = `Place` 增加折线字段（内联，不新集合）

```rust
pub struct Place {
    // …既有字段不动…
    /// 作者手绘的线状几何（图内局部 wu 扁平点列）。
    ///
    /// `None` = 点状地点（绝大多数）；`Some` = 线状地点（作者手绘的小河 / 小溪）。
    /// 与 `envelope`（面）正交：一个 Place 要么是点、要么是线，不兼得。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Vec<f32>>,
}
```

- **不新开 `rivers/` 集合**：小河需要**名字**（"通天河"）、**分类**（溪 / 河 / 运河）、
  **可被引用与选中**——`Place` 已全部具备（`name` / `biome` / `rank` / 标签 / 拾取 /
  图层面板的 place 族）。新集合要动 World / io / 命令 / 诊断 / 面板五层。
- **内联而非 `GeometryData`**：`GeometryData` 是 `#[serde(untagged)]`，`Polygon` 必须排
  在首位才能让裸数组匹配成多边形（`wv-core/src/map/mod.rs:546`）——再加一个线变体
  必须用显式形态（`!line` / 映射），作者手写 YAML 时易踩坑；且 `place_geometry` 的
  `GeometrySpan` 语义是**面地界**（`owner = Place.id` + `span` + 面几何），塞线进去
  会稀释那条语义。内联折线与 `Region.polygon` 同款（扁平点列 + 图内局部 wu），口径统一。
- **旧 YAML 零迁移**：`line` 是 serde-default 可选字段，既有工程读入不变。

### D2 · 锚点 = 折线中点（`x/y` 可空）

`Place.x` / `Place.y` 允许为 `None`（既有语义：「`None` 表示不锚定到地图」）。
线状地点的 `x/y` **缺省即由折线中点派生**——沿用街道的 `polyline_midpoint()`
先例（`wv-core/src/source/street.rs:173`）。作者显式写了 `x/y` 则以作者为准。

### D3 · 与格子的关系 = 亚格、不占格、不写 biome

作者小河**不占格、不写 `surface` / `biome`**——它是纯矢量叠加，与"一格宽的河"
（`Intent::River`）并存：

| | `Intent::River`（笔刷） | 作者小河（本 ADR） |
|---|---|---|
| 粒度 | 一格宽 | 亚格（任意折线） |
| 数据 | 写进格子（`surface` + `BIOME_RIVER`） | 独立实体（`Place.line`） |
| 能否蜿蜒表达一条溪 | ❌ | ✅ |
| 与地形栅格的关系 | 是地形的一部分 | 是矢量图层，与栅格正交 |

### D4 · 渲染 = 双供给（派生 ∪ 作者）

`PassId::Rivers` 改为同时画：

1. **派生河**（ADR-0037 D5，`rivers_seed`）——模板世界的水系，现状行为不变；
2. **作者河**（本 ADR）——`Place.line` 非空的地点。

先例：街道的**双供给**（GLOSSARY「双供给」——手绘 YAML / Overture 只存引用）。
两条河没有 id 冲突面（派生河无实体），叠加绘制即可；作者河与派生河相交不裁剪、不融合
（v1 不做几何求交——零依赖红线，AGENTS §2.1）。

### D5 · 河岸的覆盖物 = region 声明（不归河对象）

「小河两岸长什么」属于**表现层**：由 ADR-0045 的自然区域承担——沿河画一圈 region，
`overlay` 声明覆盖物键。河对象本身**不带装饰字段**（否则又是一处表现层落数据）。

### D6 · 编辑命令 = 三个，照街道命令的形状

`AddPlaceLineCommand` / `UpdatePlaceLineCommand` / `DeletePlaceLineCommand`
（照 `AddStreetSegmentCommand` / `DeleteStreetSegmentCommand` 的形状，
`wv-edit/src/command.rs:2384`），单事务可撤；折线点的增删走同一组命令。

### D7 · 时间 = v1 不带 span

线的显隐沿用图级 `MapNode.time_span`（与 `Place` 一致）。**不带独立时间窗**——
`StreetSegment` 有 `time_span`（街道的开通年份），线状地点 v1 不引入；
若作者要「这条溪某个年份才开凿」→ §待裁。

### D8 · 诊断 = 提示不阻止（V1）

- 折线点数 < 2 → 校验面板提示「折线退化」；
- `line` 存在但 `map` 为 `None` → 提示「未锚定到地图，该河不可见」；
- `line` 与 `envelope` 同时存在 → 提示「线与面互斥」。

均**不阻止保存**（AGENTS §7.5 V1）。

## 代价

- `Place` 加一个字段（全工作区 Place 字面量构造点需补字段；`Place` 无 `Default`，
  需逐处补 `..` 之外的值或加构造器）；
- `PassId::Rivers` 的取数要并两路（派生折线 + 作者折线），且作者河要从 `Place` 投影
  进 `ProjectionFrame`（或渲染层直接读 `world.places`——需定谁负责，见 §待裁）；
- 编辑 UI（手绘折线的交互）尚未设计——本 ADR 只定数据契约与渲染口径。

## 被否决的备选

| 备选 | 否决理由 |
|---|---|
| 新开 `rivers/` 实体集合 | 名字 / 分类 / 引用 / 标签 / 拾取全部重造；Place 恰好已是「名字 + 锚点 + 类别」的实体 |
| 给 `GeometryData` 加 `Line` 变体 | `untagged` 裸数组匹配坑（作者手写 YAML 易错）；`place_geometry` 的面语义被稀释；内联口径与 `Region.polygon` 更统一 |
| 用 `place_geometry` 存线（`GeometrySpan`） | `GeometrySpan` 是「地界（面）+ 有效区间」；线塞进去会让"占位 / 诊断 / 时间"三处语义漂移 |
| 让作者小河写进格子（加宽 `Intent::River`） | 亚格精度丢失；一条溪会占掉若干整格的 biome 与 surface，污染地形事实层 |
| 作者河取代派生河 | 派生水系是模板世界的确定性资产（重生成必一致，ADR-0037）；作者河是补充，不是替代 |

## 触及原则

| 原则 | 触及 | 说明 |
|---|---|---|
| P1 单机可运行 | 未触 | 零新增依赖、零网络 |
| P2 目录即工程 | 触 | 线随 Place 落 `places/<id>.yaml`（§4.5 每实体一文件），不写 AppData |
| P3 YAML 为主 | 触 | 纯 YAML 折线点列，**不新增二进制例外**（折线不是"超大地图网格数组"） |
| P4 多地图层级 | 未触 | 线锚定单图（`Place.map`），跨图引用仍只走 Place id |
| P5 图层化编辑 | 触 | 作者小河走 `Road` / `Place` 族既有图层口径，**不新增 `LayerKind`** |
| P6 非破坏编辑 | 未触 | 新增 / 修改 / 删除线均经 `wv-edit` 命令（可撤），不覆写历史值 |
| P7 时间轴驱动 | 触（须明写） | v1 线不带时间窗（D7），沿用图级 `MapNode.time_span` |

## 待裁（移出本 ADR）

1. **编辑交互形态**——手绘折线用现成的 Pen 方法（选区工具那套，GLOSSARY「Pen 方法」）
   还是新的折线工具？现有 Pen 方法是多边形闭合，线段版本可复用大半。
2. **取数责任**——作者河是渲染层直接读 `world.places`，还是经 `ProjectionFrame`
   （时间轴求值后的投影）？后者更符合 AGENTS §3「世界访问只读 + 投影」，前者更省一层。
3. **线的独立时间窗**——「这条溪某年开凿」是否需要（对齐 `StreetSegment.time_span`）。
4. **作者河与派生河的视觉区分**——是否给作者河不同的线宽 / 颜色 / 标注？

---

## 追记一（2026-10-20，实现落点与遗留项）

### 实现落点（本轮落地）

| ADR 条款 | 落点 | 状态 |
|---|---|---|
| D1 `Place.line` | `wv_core::map::distance::Place::line: Option<Vec<f32>>`（serde default → 旧 YAML 零迁移） | ✅ |
| D2 锚点 = 折线中点 | `Place::anchor_from_line()`（**弧长**中点，仿 `polyline_midpoint`）+ `effective_anchor()` | ✅ |
| D3 不占格不写 biome | 折线是纯矢量，不经 `TerrainGrid` | ✅ |
| D4 双供给渲染 | `ProjectionFrame::author_river_polylines` + `PassId::Rivers` 合批（派生 ∪ 作者） | ✅ |
| D5 河岸覆盖物归 region | 走 ADR-0045 的 `Region.overlay`（机制在，不另建） | ✅ |
| D6 三个编辑命令 | `AddPlaceLineCommand` / `UpdatePlaceLineCommand` / `DeletePlaceLineCommand` | ✅ |
| D7 不带 span | 折线无时间窗，沿用图级 `MapNode.time_span` | ✅ |
| D8 诊断 | 校验面板三条提示：折线退化 / 未锚定到地图 / 与面包络互斥（**提示级**，不阻止保存） | ✅ |

### 遗留项（明确登记）

1. **手绘折线的交互未做**：ADR D6 只定数据契约与渲染口径；画折线的手势
   （复用 Pen 方法的线段版还是新工具）属交互设计议题，未在本轮落地。
2. **作者河与派生河不融合**：相交不裁剪、不求并（零依赖红线，AGENTS §2.1
   不引几何库）——作者河是叠在派生河之上的第二道水线。
3. **折线无宽度 / 样式**：作者河与派生河共用同一材质（`RIVER_Z` 0.082 同层），
   两者视觉上不可区分（待裁：是否给作者河单独的线宽 / 颜色）。
