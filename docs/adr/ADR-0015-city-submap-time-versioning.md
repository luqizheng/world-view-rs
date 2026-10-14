# ADR-0015：城市子图的时间版本化——双层 `TimeSpan` 模型

**状态**：已接受（Accepted，2026-10-14 用户「继续」拍板）
**日期**：2026-10-14
**决策者**：用户
**关联**：P6（非破坏编辑）、P7（时间轴驱动）、AGENTS §4.1（历史即区间）、§12（待决问题处置）、[ADR-0009](ADR-0009-graded-tile-zoom-to-street.md)（分级缩放 + 子图深度）、[ADR-0010](ADR-0010-street-vector-layer.md)（街道矢量层与 `StreetSegment` 数据模型）、[ADR-0013](ADR-0013-layer-frame-binding.md)（图层帧求值点）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

ADR-0009 解决了「缩多深」——世界 → 大区 → 区域 → 城市 → 城区 → 街道共 6 级，每级自带 `raster` 与 `Extent.origin`。
ADR-0010 解决了「街道在哪一层」——街道是 `LayerKind::Road` 图层，落在**城市子图**的 YAML 文件里（虚构路网手绘 / 真实路网 Overture 引用），不内联进世界地图。

但**两个 ADR 都没有回答一个问题**：

> 一座城市会随时间变化。1200 年的武昌与 1500 年的武昌不是同一张图——城墙扩建、街道新建、城门改名、甚至整座城迁址。子图层级的 P4 解决了「父子图空间隔离」，却没解决「同一张城市子图内部如何表达时间维度」。

证据链：

1. `StreetSegment` 当前**无 `TimeSpan` 字段**（`crates/wv-core/src/source/street.rs`）——一段街道一旦画下，永远存在。1200 年画的中山路与 1900 年画的中山路共存于同一份 YAML，渲染层无法按当前时间过滤。
2. `MapNode` 同样**无 `time_span` 字段**——城市子图本身不携带「这座城存在于哪段时间」的声明。若整座城只在 1200–1450 存在（如战乱中被毁），子图没有机制表达"此时此地无城"。
3. 既有时间机制止于 `GeometrySpan`（`crates/wv-core/src/map/mod.rs` L359）——疆界几何可随时间变化，但只覆盖 `Place` / `Faction` 的边界，**未触达**城市子图内部的图层内容（街道 / 未来可能加的建筑 / 地块等）。
4. `TimelineQuery::at`（P7 唯一入口）目前只对 `Place.state_spans` / `Character.state_spans` 等**实体级**区间求值——子图层级的求值通路**尚未铺设**。

不补齐这一层，"缩放到街道" 在长时间跨度的工程里只能呈现**某一时刻的快照**，作者无法让读者在不同年份看到城市演化的差异——这正是用户提问「城市随着事件流逝，城市就会变化，那么就是一个很大的挑战」想解决的问题。

## 2. 决策（Decision）

**双层 `TimeSpan` 模型**：

| 层 | 字段 | 语义 | 缺省 |
|---|---|---|---|
| **图层级（map scope）** | `MapNode.time_span: Option<TimeSpan>` | 该子图所代表的**时代窗口**；窗口外整张子图不显示 | `None` = 不限时代（向后兼容） |
| **段层级（segment scope）** | `StreetSegment.time_span: Option<TimeSpan>` | 该段街道的**存在窗口**；窗口外不渲染、不参与标注、不计入 LOD | `None` = 跟随所在子图（窗口内始终可见） |

### 2.1 `MapNode.time_span`（新增字段）

- 类型：`Option<TimeSpan>`（`None` 序列化跳过，零迁移；既有 YAML 不动）
- 语义：图层级"时代窗口"。例：
  - `time_span: {start: 1200, end: 1450}` → 武昌城（南宋—明初）只在 1200–1450 显示
  - `time_span: {start: 1850, end: 1900}` → 武昌城（晚清）只在 1850–1900 显示
  - 不设 → 任何时间都显示（**当前行为**，零回归）
- 落地后效果：
  - `DrillState` 钻入某图时，若 `TimelineQuery::at(t)` 命中 `MapNode.time_span` 之外，UI 给"该子图在本时代不存在"提示，钻取请求被驳回（**不阻止保存，仅交互反馈**——V1）
  - 渲染层：换级意图触发时，若目标图在当前时代外，则**禁止**自动跨级过渡（仍可手动在子图清单里选入，但底图给"空"标记）
- **跨时代城市**的两种写法：
  1. **同子图 + 段级 `time_span`**（推荐）：一座武昌城跨越 800 年，街道分段标 `time_span`；地图本身不限定时代
  2. **多子图分时代**：城市名 = `city_wuchang_song`、`city_wuchang_qing`，各带独立 `time_span`；父子图关系由 `parent` + 兄弟 `Id` 表达。适合"宋代武昌"与"清代武昌"已**完全不同**的工程

### 2.2 `StreetSegment.time_span`（新增字段）

- 类型：`Option<TimeSpan>`（`None` 序列化跳过，无名同理）
- 语义：段级存在窗口
- 序列化示例：
  ```yaml
  segments:
    - class: primary
      points: [[100, 200], [300, 400]]
      name: 中山路
      time_span: {start: 1450, end: 1900}   # 1450 年命名并启用，1900 年后改名/废止
    - class: residential
      points: [[500, 600], [700, 800]]
      # 无 time_span → 跟随所在子图
  ```
- 求值点：**单一实现**为 `wv_core::time::TimelineQuery::at`，新增 `street_visible_at(seg, t) -> bool`（P7 唯一入口的延伸），与 `place_at(t)` / `character_at(t)` 同构
- 与 `StreetLayer` 现有字段关系：
  - `class` / `points` / `name` 不变
  - `time_span` 为可选，与 `name` 同一序列化策略（`skip_serializing_if = "Option::is_none"`）

### 2.3 双层交互规则

`TimelineQuery::at(value, t)` 求城市子图在 `t` 时刻的可见集 =：

```
visible_segments(t) = {
    seg | seg.time_span.is_none()                          # 段无窗口：跟随子图
      ∨ seg.time_span.as_ref().unwrap().contains(t)        # 段有窗口：被窗口包住
}
∧ MapNode.time_span.as_ref()
      .map_or(true, |w| w.contains(t))                     # 子图无窗口：永远
                                                          # 子图有窗口：被窗口包住
```

- 单调性：`time_span: None` 是全集的特殊情况（永远可见），不引入特殊分支代码
- 渲染层：每帧仅渲染 `visible_segments(t)`；未通过的段从 mesh / 标注 / LOD 桶中**完全消失**（不是淡出，是不参与）
- 编辑器：街道笔刷画下的新段**默认无 `time_span`**（沿用现状）——若作者想让一段只存在于 1450–1900，需手填字段；不自动引入，避免新手作者踩坑

### 2.4 编辑闭环（命令层）

- `AddStreetSegmentCommand` 增加可选 `time_span: Option<TimeSpan>`；未指定 → `None`
- `EditStreetSegmentTimeSpanCommand { layer, index, new: Option<TimeSpan> }`（新命令）——改某段窗口；revert 缓存旧值；UI 在街道编辑面板里加"存在起止"输入
- 校验层（V1，**只提示不阻止**）：段级 `time_span` 落在子图 `time_span` 之外 → 提示"该段所在子图在本时代不存在"，不阻止保存（与 P6 一致）
- 历史撤销：所有时间相关命令经 `wv-edit::History`，P6 不破

### 2.5 测试与文档

- `wv-core` 单元测试（沿用 AGENTS §7.2 边界覆盖）：
  - `map_node_without_time_span_is_always_visible()`
  - `map_node_with_time_span_hides_outside_window()`
  - `segment_without_time_span_follows_its_map()`
  - `segment_with_time_span_hides_outside_window()`
  - `segment_window_inside_map_window_is_strict_subset()`（验证单调性）
- `wv-io` round-trip：`StreetSegment` 加 `time_span` 字段后既有 YAML 无损（缺省跳过）
- `wv-edit`：3 个时间相关命令各 1 happy-path + 1 越界
- `wv-app`：1 个渲染层集成测试——`visible_segments(t)` 喂入不同 `t` 时 mesh 桶容量符合预期
- `README.md` §12 第 31 条待决登记：`MapNode.time_span` 是否需要支持"该图仅在特定时代被自动隐藏 vs. 一直可见但内容空"两种语义的区分（暂按统一规则走）
- `docs/adr/README.md` §4 表格追加 ADR-0015

### 2.6 与七原则的自检

| 原则 | 触及情况 |
|---|---|
| P1 单机可运行 | ✅ 不引入新依赖；时间求值用既有 `TimelineQuery` |
| P2 目录即工程 | ✅ 字段落 YAML，无新存储路径 |
| P3 YAML 为主格式 | ✅ `time_span` 是结构化区间字段，非二进制 |
| P4 多地图层级 | ✅ 跨时代城市可拆为多子图（§2.1 第 2 写法），不强制 |
| P5 图层化编辑 | 不触及（独立轴） |
| P6 非破坏编辑 | ✅ 改 `time_span` 经 EditCommand，区间不覆盖 |
| P7 时间轴驱动 | ✅ 段级 / 图级可见性**全部**走 `TimelineQuery::at` |

## 3. 理由（Rationale）

- **为什么不是"必填 `time_span`"**：违背 V1（校验只提示不阻止）。街道史观覆盖完整时间线是常态——大多数手绘路网不存在"这段路何时建何时废"的概念，强制填写等于把作者的精力锁死在元数据上。`None` 缺省是**对作者时间预算的尊重**。
- **为什么图级 + 段级两层而不是只段级**：图级表达**整座城市**的兴废（"宋代武昌"与"清代武昌"是不同的城市）；段级表达**一段路**的兴废。粒度不同，混在同一字段会丢失语义。例：图级设 1200–1450 时，本图**所有**段都被压住，段级 `time_span` 即使设 1400–1410 也无效——这正是图级窗口应有的否决权。
- **为什么不引入"版本号 / 快照 ID"**：时间版本化是 `TimeSpan` 的本职，与项目其他实体（`Place` / `Character` / `Faction`）同构。引入新维度会破坏 `TimelineQuery::at` 作为 P7 单一入口的承诺，渲染层被迫写两套过滤。
- **为什么不直接复制"宋代武昌 / 清代武昌"两份世界**：双层模型已经能覆盖；引入重复世界是 P4 禁止的（ADR-0003 A3），且增加 Git 冲突面。**多子图分时代**只用于"城市本身已不同"的极端情况。
- **为什么不走"沿用 `GeometrySpan`"**：那个是为疆界几何设计的，绑定 `map` + `owner`，街道语义不匹配（街道没"边界"概念，是连续线段）；强行套用会让 `StreetLayer` 变成另一种 `GeometrySpan` 容器，模型变臃肿。

## 4. 代价（Consequences）

- **正**：作者可在长跨度工程里呈现城市演化；P7 兑现扩展到子图内容；与既有 `state_spans` 模式同构，学习成本低。
- **负①（数据层改动）**：`StreetSegment` 加字段、`MapNode` 加字段、`TimelineQuery` 增可见性函数。三个改动文件，零 API 破坏（`Option` 缺省）。
- **负②（编辑器 UI）**：街道笔刷面板需增"存在起止"输入；`MapNode` 编辑器需增"时代窗口"编辑入口。两条独立 UI 任务，工作量中等。
- **负③（求值性能）**：每帧 O(段数) 过滤。对城市图段数（万级）实测无压力；超大规模场景（Overture 全城 15 万段）需配合视口裁剪（ADR-0010 §4 尚未做）才有意义——本 ADR 不背锅。
- **负④（语义一致性）**：作者可能误把"该段有 `time_span`"理解为"该段是临时的"而非"该段只在某些时代存在"——需在文档与 UI 提示中明确（GLOSSARY.md 新增"段时代窗口"条目）。
- **负⑤（多子图分时代的歧义）**：城市名相同时父子图识别靠 `Id`，人工命名易撞车。登记 README §12 第 32 条待决——可考虑命名规约（如 `city_wuchang_1200_1450`）但不在本 ADR scope。

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **必填 `time_span`（无 `None`）** | 违背 V1 与作者时间预算；大多数手绘路网无明确兴废年代 |
| **仅段级、不设图级** | 失去"整座城兴废"的表达力；不得不让每段都重复子图窗口 |
| **版本号 / 快照 ID 模型** | 与 P7 唯一入口冲突；与 `Place.state_spans` 不一致 |
| **多份世界（宋代一份、清代一份）** | 违反 ADR-0003 A3；增加 Git 冲突；与 P4 精神相悖 |
| **沿用 `GeometrySpan` 表达街道** | 街道是连续线段而非多边形边界；模型强行套用会变臃肿 |
| **每张图强制带 `time_span`** | 既有大量示例工程无时代窗口；零迁移约束要求 `None` 缺省 |
| **渲染层做时间过滤（不落 `TimeSpan`）** | 渲染层职责是渲染，不是数据决策；违反 AGENTS §3 与 §4.1 |

## 6. 待裁（Open Questions）

- **Q1**：`MapNode.time_span` 之外是否需子图清单层做"该图在 t 时代不可见时灰显 vs. 隐藏"——倾向灰显（让作者看见图存在、只是不在当前时代），但需 UI 验证。
- **Q2**：跨时代城市的"自动选最近时代子图"是否要做——倾向不做（让作者显式钻取，避免静默跳到不希望的版本）。
- **Q3**：段级 `time_span` 与图层可见性（ADR-0013 `Layer.visible`）的优先级——两者是正交轴，但 UI 同时存在两个时间相关开关可能让作者混淆，需要面板布局验证。
- **Q4**：手绘街道 UI 的"存在起止"输入形态——日期字符串（按 `Calendar` 解析）vs. 直接数值对。当前按 `TimeSpan` 数值对（项目内日历是 P7 不变量）。
- **Q5**：未来"建筑 / 地块"等子图内容若要扩展本模型，是否复用同一组 `time_span` 字段——倾向是（模式同构），但需待首例建筑实现时验证。
