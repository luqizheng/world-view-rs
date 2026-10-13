# ADR-0010：街道矢量层（双供给 + 批量渲染）

**状态**：已接受（Accepted）
**日期**：2026-10-13
**决策者**：用户（数据源拍板「2. Overture」；覆盖范围拍板「1. 都要」= 真实地图与虚构世界都要能缩放到街道）
**关联**：P1 / P2 / P3 / P4 / P5、`AGENTS.md` §2.1（分层）/§4.2（距离只存参数）/§5（二进制门槛）/§9（依赖纪律）、[ADR-0007](ADR-0007-overture-real-geography.md)（Overture 只存引用）、[ADR-0008](ADR-0008-google-default-map-experience.md)（Google 式道路双层视觉）、[ADR-0009](ADR-0009-graded-tile-zoom-to-street.md)（缩放模型）、README §12 第 21 条

---

## 1. 处境（Context）

[ADR-0009](ADR-0009-graded-tile-zoom-to-street.md) 解决了「能缩多深」，但缩到街道之后，**要有街道可看**。这里有一个容易被视觉假象掩盖的事实：

> **当前渲染出来的「道路」不是街道。**

证据链：

1. 渲染层的道路来自 `ProjectionFrame.road_segments`，其数据源是 `world.distances.edges`——即**地点之间的距离图**。它的语义是「城与城之间的行程连线」，是宏观行程图的投影，不是地物。
2. `LayerKind::Road` 与 `Layer.source` 是**声明了但无人消费**的空壳：全仓生产代码对二者的引用为 **0 处**。`examples/minimal-world/maps/world/index.yaml` 里写了 `kind: road, source: roads.yaml`，而 `roads.yaml` 在**任何**示例工程中都不存在。
3. 也就是说，`LayerKind::Road` 描述了一种**从未被实现**的能力，而视觉上「有路」的假象由 `distance.edges` 提供。

供给侧的规模现实（决定架构，不是细节）：

| 场景 | 几何规模 | 来源 |
|---|---|---|
| 真实地图（用户已选 Overture） | 单城 transportation 实测 **155k segments**（西雅图），几何量级 3–8 MB | Overture Parquet（ADR-0007：只存引用，不落盘） |
| 虚构世界 | 作者手绘 **200–2000 段**，几 MB YAML 量级 | 作者本人（Overture 里没有虚构世界的街道） |

而渲染现状是**每条线段 = 2 个 `Mesh2d` 实体**（casing + fill）。按此口径，西雅图一城就是 **31 万个实体**——不可交互。规模问题是架构问题，不是优化问题。

## 2. 决策（Decision）

1. **街道路网与城市间行程连线是两个语义层，永不合并。**
   - 街道 = **地物**，属于图层（`Layer`），有 `class`、有几何、可逐层开关。
   - `distance.edges` = **行程图**，属于距离维度，只存参数（§4.2）。
   - 二者可以同时可见，但独立着色、独立开关、独立 LOD；**禁止**让任一方兼职另一方。

2. **一个供给接口，两个实现。**
   - **虚构世界**：作者手绘，几何落在 `Layer.source` 指向的**图层数据文件**（YAML），规模小、可 Git、可由作者逐段编辑。
   - **真实世界**：Overture Parquet 引用，走 `MapNode.geographic_source`（ADR-0007 已定），**只存引用不落几何**。
   - 两者输出**同一份「路段流」**（点列 + `class` + 属性）交给渲染层；渲染层不关心来源。这是「一个接口 + 两个实现」，不是两条管线。

3. **渲染改为批量 / 实例化。** 弃用「一段 = 2 实体」的建法，改为按 `(class, 材质)` 分桶的批量网格。目标：单城 15 万段在缩放与平移时保持可交互。

4. **按 `class` 做 LOD 过滤 + 视口裁剪。** 缩小时丢弃 `footway` / `path` / `cycleway`，放大时逐档补回；几何按视口矩形裁剪后再上传。`class` 取值沿用 Overture 既有集合（`motorway, trunk, primary, secondary, tertiary, residential, service, path, cycleway, track, unknown, unclassified`）。

5. **街道几何不进领域 YAML 的字段。** ADR-0007 定下的边界（「领域模型里没有几何坐标」）继续有效。手绘路网作为**图层数据文件**存放，不进 `Place` / `Event` / `Track` 的结构。单文件规模阈值与分片策略**尚未定**，登记为 README §12 第 21 条待决。

6. **`wv-overture` 是新的平行叶子 crate**，不依赖 Bevy，单向向渲染层注入几何，**不回写领域**（ADR-0007 已定）。Parquet / Arrow 依赖须逐条过 `AGENTS.md` §9 的依赖门槛（默认立场是拒绝）。

7. **`LayerKind::Road` 与 `Layer.source` 从空壳转正**：本次先补 `TODO(阶段三)` 标注与待决登记（§6.2 要求「新出现的未定论实现必须标注待决」），其语义在本 ADR 落定后由实现补齐。

## 3. 理由（Rationale）

- **用户意图**：用户选了 Overture 作真实数据源，并要求「都要」——虚构世界同样要能缩放到街道。后者排除了「只做真实地图」的省事路线，也决定了**手绘编辑器是必需品**，不是锦上添花。
- **P3（YAML 为主格式）**：手绘路网是**小规模矢量数据**，放 YAML 完全合规；真实地图的大规模几何**不落盘**（只存引用），所以也无需为它开二进制的例外。P3 的唯一例外（超大地图网格数组，§5）**没有被扩大**。
- **P2（目录即工程 / Git 友好）**：几 MB 的手绘 YAML 可进 Git、diff 可读（`Id` 是可读字符串，§4.4）。
- **P4（只存引用）**：街道引用 Overture 而非复制几何；图层引用数据文件而非内联。
- **§4.2（距离只存参数）**：路段长度**不存字段**，需要时由 Haversine 从点列派生（ADR-0007 已定该派生归 `wv-overture`）。存储层不出现「行程时间结论」，也不出现几何长度结论。
- **§2.1（分层约束）**：`wv-overture` 与 `wv-terrain` 同为**平行叶子**（纯数据、零 Bevy），`wv-core` 依旧零渲染依赖。街道路网的浮现**不会**把 Parquet 拖进数据层。
- **可回退**：第 5 条的「放进图层数据文件」是最容易回退的选择——将来若要改成矢量瓦片或其他容器，只换 `Layer.source` 的载荷格式，领域模型不动。

## 4. 代价（Consequences）

**负面后果，必须承认：**

1. **新增依赖**。Parquet / Arrow 是重依赖，须过 §9 门槛；若被否决，真实地图路线要改道（这会推翻本 ADR 的一部分，届时须新开 ADR）。
2. **instancing 改造触及渲染热点**。`vector_layers.rs` 是当前改动最活跃的文件，批量渲染会重写它的实体生成路径。
3. **手绘路网需要编辑器**。作者要能落点、拖拽、断线、按 `class` 改属性；按 §3.3 与 §2.1，所有编辑**必须**经 `wv-edit` 的 `EditCommand` / `History`，不得由 System 直改世界。
4. **两条供给线的质量不同源**。Overture 数据有权威但可能缺段、分类不符合作者预期；手绘数据完全可控但要作者亲力亲为。UI 必须让二者可区分（来源可标注），否则作者会困惑「为什么这段路不能改名」。
5. **`class` 集合是外部约定**。Overture 若调整分类，LOD 过滤表要跟着改；这也是 ADR-0007 把「Parquet schema 升级策略」列为待裁的原因之一。

**被否决的备选方案：**

| # | 备选 | 否决理由 |
|---|---|---|
| A | 沿用 `LayerKind::Road` 单层表示，但**不改**渲染方式 | 规模即崩：真实地图一城 31 万实体，不可交互。规模是架构问题，不是优化问题 |
| B | 让 `distance.edges` **兼职**街道 | 语义污染（行程图 ≠ 地物），且作者无法表达「一条街」——距离图的一条边是「两个地点之间的距离」，不是一条可以单独命名、分类、开关的路 |
| C | 把 Overture 几何**转换并落盘**成工程内的 YAML | 违反 ADR-0007（只存引用）与 P2/P3（几十 MB 几何进 Git） |
| D | **只做真实地图**，虚构世界放弃街道级 | 与项目定位冲突（虚构世界是首要场景），也直接违背用户「都要」的裁定 |
| E | 自研矢量瓦片格式（MVT-lite） | 现阶段没有多级瓦片化的必要（单城一次性载入 + 视口裁剪 + `class` LOD 已够）；且新二进制格式必须过 §5 门槛（该例外**只**面向网格数组），为一个未验证的需求开例外不可接受 |
| F | 手绘路网直接写进 `MapNode` 的新字段（如 `roads: Vec<...>`） | 与「图层化编辑」（P5）冲突：图层开关、锁定、透明度、z 序都是 `Layer` 的能力，重开一个平行字段等于绕过 P5 |

## 5. 落地记录

> 本节只记录**已经发生**的落地，不构成新的编号空间。

- **本次**：本 ADR；`crates/wv-core/src/map.rs` 的 `Layer.source` 与 `LayerKind` 补上 `TODO(阶段三·街道图层)` 标注（指明二者当前无人消费、渲染层的「道路」实为 `distance.edges`）；README §12 第 21 条待决登记。
- **待做（顺序即依赖序）**：
  1. 路段流的类型与供给接口（`wv-core` 侧只定义契约，不含几何实现）；
  2. `vector_layers.rs` 批量/实例化改造 + `class` LOD + 视口裁剪；
  3. 手绘路网数据文件 + 编辑器命令（`wv-edit` 的 `EditCommand`）；
  4. `wv-overture` crate（Parquet 读取 → 渲染几何），依赖过 §9 门槛；
  5. 与 [ADR-0009](ADR-0009-graded-tile-zoom-to-street.md) 的街道级栅格底图合成（底图管地表，矢量管路网）。
- **待做第 1 项部分完成（2026-10-13 追加）：数据模型与 IO 已落地**——
  `crates/wv-core/src/street.rs` 新增文档模型：[`wv_core::street::StreetLayer`]（`Layer.source` 指向的 YAML 根，`segments` 折线段列表）、[`wv_core::street::StreetSegment`]（`class` + `points: Vec<Point>`，本图局部坐标）、[`wv_core::street::RoadClass`]（12 类，命名对齐 Overture，`lod_rank()` 给出 LOD 分档的唯一派生入口）。几何只存参数不存渲染结论（线宽 / 颜色 / LOD 档位由渲染层按 class 派生）。
  `crates/wv-io/src/project.rs` 新增 `Project::load_streets(rel_path)` / `write_streets(rel_path, &layer)`（相对工程根、自建父目录；`crates/wv-io/src/error.rs` 加 `IoError::ReadFile` 变体让读失败带路径）。
  round-trip 测试：`crates/wv-io/tests/street_layer_yaml.rs`（5 项：无损 round-trip、缺 class 缺省 Residential、空文档、工程内首存自建目录、缺文件报路径）。
  **尚未做**：`vector_layers.rs` 消费路段流（待做第 2 项）、手绘编辑命令（第 3 项）、Overture 段落到 `RoadClass` 的映射（第 4 项，该 crate 为协作者在制，不越权）。
- **待做第 2 项完成（2026-10-13 追加）：路段流接入渲染**——
  新模块 `crates/wv-app/src/streets.rs`：`StreetLayerCache`（Resource，按当前图 id 缓存，图未变不重复读盘）与 `reload_street_cache` 系统（只加载当前图 `kind: road && visible && source` 的层；V1 对打不开的工程/缺失文件仅告警不崩溃）；纯函数 `street_kept_at_tier(lod_rank, LodTier)` 实现 class LOD（Nation 只留主干、Region 留到 residential、Edit 全留）、`street_streams()` 把折线切成线段流并按 rank 聚档。
  `crates/wv-app/src/vector_layers.rs`：`VectorLayerEntities` 增设 `street_major` / `street_minor` 两个池，复用既有「规格不变只更新 Transform / 变了才换 Mesh」的批量策略；常量 `STREET_MAJOR_PX=4.5`、`STREET_MINOR_PX=2.75`，Z 序压在行程连线（0.1）之上、POI（0.21）之下。**视口裁剪本项未做**（城市图段数量级小，等真实数据接入后再裁）。
  演示数据：`examples/xinhai-1911/maps/city_wuchang/roads.yaml`（11 段）+ `index.yaml` 追加 `layer_city_wuchang_streets`，双供给之手绘 YAML 侧即此形态。
  测试：`crates/wv-app/src/streets.rs` 5 项（tier 筛选、折线切分、退化折线、rank 聚档、端到端磁盘加载 + 同图判重）；GUI 冒烟无 panic（初版曾因漏 `init_resource::<StreetLayerCache>` 启动即 panic，已修）。
  **尚未做**：手绘编辑命令（第 3 项）、Overture 映射（第 4 项，协作者在制）、与 ADR-0009 的底图合成（第 5 项）。
- **待做第 3 项（编辑闭环）完成（2026-10-13 追加）：命令与存储链路已通，编辑器 UI 未做**——
  `wv-core`：`World.streets: BTreeMap<Id, StreetLayer>`（键 = **图层 ID**，与 `terrains` 同构；磁盘真源仍是各 `Layer.source` 的 YAML，`World` 只持运行副本，Undo/脏路径语义不变）。
  `wv-edit`：`AddStreetSegmentCommand { layer, segment }`——目标图层未加载时静默新建空文档再追加（V1：画下的第一笔就是文档的诞生，不追问）；revert 按缓存的追加前段数截回；脏路径 = **虚拟命名空间** `streets/<layer_id>`。
  `wv-io`：加载器把每个 `kind: road && source` 的图层文件读进 `World.streets`；落盘侧 `write_streets_layers` 把虚拟脏路径映射回真实文件 `maps/<map>/<source>`，且路由独立于 `wants("maps")`（只画一笔街道不得触发 index.yaml / terrain.bin 重写）。**失败语义分级**：`source` 指向的文件不存在 → 跳过（既有工程本就有悬空声明，如 minimal-world；V1 作者还没写完不算坏工程）；文件存在但解析不了 → 照旧报错。
  `wv-app`：`StreetLayerCache` 取数源从「每次换图重读磁盘」改为 `World.streets`（磁盘只在工程加载时读一次），刷新条件 = 换图 ∨ `WorldProjection` 变更——手绘的一笔即刻出现在渲染缓存（所见即所得的根基）。
  测试：wv-edit 4 项（apply/revert、静默新建、未 apply 先 revert 报错、双命令独立撤销）、wv-io save_world 1 项（虚拟脏路径命中正确落盘位置、不误触发地形重写、round-trip）、wv-app streets.rs 改造并新增编辑闭环测试。GUI 上「画一笔街道」的交互（笔刷面板 / 点击落点）属编辑器 UI，留待下一轮。
- **待做第 4 项（编辑器 UI）完成（2026-10-13 追加）：街道笔刷面板 + 点击落点已接通**——
  `wv-app/streets.rs`：`StreetEditorState { enabled, class, last_click, drawn }`（会话状态 Resource，不存街道数据）；`commit_street_click` 系统——左键逐点连线（上一落点 → 本次点击 = 一直段，`StreetSegment::straight` + `AddStreetSegmentCommand` 走 `EditHistory`，与地形笔刷同一条「系统只生成命令」的纪律），右键结束连线；点取自 `ViewState.cursor_world`（`track_cursor_world` 每帧无条件刷新，无 Window 依赖可测）；世界 → 本图局部走 `coord::resolve_frame_in`，与标记/栅格同链；目标图层 = 当前图第一个「`kind: road` 且可见、未锁定、有 source」的图层（找不到 `warn` 一次）。
  手势让渡：`camera_pan` 的左键平移让渡条件扩展为 `brush.enabled || streets.enabled`（与地形笔刷同一条 Google Maps 手势规则）。
  `ui.rs` `panel_brush`：新增「街道」区——开关、类别下拉（`RoadClass::ALL` + 新增 `RoadClass::label()` 中文显示名，YAML 仍存 snake_case 英文键）、已画段数、「结束连线」按钮。
  测试：wv-app `street_clicks_commit_segments_through_the_history`（两次左键经 Schedule 实跑落成 1 段 + 脏路径入 `dirty_set` + 右键结束；教训：`ButtonInput::clear()` 只清 `just_*` 不清 `pressed`，二次点击前必须 `release`）。wv-app 204 passed；GUI 冒烟 12 s 存活、6 行 stderr、0 panic。
  **尚未做**：Overture 映射（第 4 项原文，协作者在制 wv-overture，不越权）、与 ADR-0009 的底图合成、线段预览光标（画线时暂无橡皮筋预览）。
- **编辑器补全（2026-10-13 追加）：删除命令 + 橡皮筋预览**——
  `wv-edit`：`DeleteStreetSegmentCommand { layer, index }`（apply 缓存被删段、revert 插回原索引；越界报 `Precondition`，UI 侧先判可删性）。测试 +2（删除还原同索引 / 越界拒绝且不改文档），wv-edit 20 passed。
  `wv-app/streets.rs`：`target_road_layer(node)` 抽成公共助手（多入口口径同 README §12-22）。
  `wv-app/ui.rs` `panel_brush`：「撤销上一段」按钮（置灰由当前段数驱动；点击经 `DeleteStreetSegmentCommand` 走 History，脏路径 `streets/<layer>`）。
  `wv-app/vector_layers.rs`：`street_preview` 池 + `STREET_PREVIEW_PX=1.75` / `STREET_PREVIEW_Z=0.12`——画线中渲染「上一落点 → 光标」橡皮筋；世界 → 视图 = `grid.origin + (世界 − 本图帧原点)`，与街道同链；预览属编辑会话，不受 `roads_visible` 影响。
- **街道双层描边（2026-10-14 追加）：地图模式下「白底白线」不可见的修复**——
  处境：街道自第 2 项起只画**一层 fill**，而 GoogleMap 模式下该 fill 是白 `#FFFFFF`、土地是近白 `#F8F9FA`（`palette_google.rs` 的 `GOOGLE_ROAD_FILL` / 土地色）——两者亮度只差 6/255，城市级线宽又只有 3 像素上下，抗锯齿一摊开就等于隐形。道路（行程连线）从 ADR-0008 起就是 casing+fill 两层，**街道这条链没跟上**。
  决策：街道两个宽度档各画两层，复用既有的 `sync_road_strokes` 与当前模式的道路材质——casing 宽度 = fill + `STREET_CASING_EXTRA_PX=2.5`，Z 各低 0.002（`STREET_MAJOR_CASING_Z=0.108` / `STREET_MINOR_CASING_Z=0.113`，仍压在行程连线 `ROAD_Z` 之上、fill 之下）。
  理由：与道路同一口径，不新立换算、不新增材质、不动数据模型；Google 式地图里肉眼看到的轮廓线本来就是 casing（浅灰 `#DADCE0`），fill 只是路面。
  代价：街道实体数 ×2（城市图段数量级小；视口裁剪仍欠，见本文「待做第 2 项」）；街道仍是细线，城市级约 3 像素，若要更醒目需按 LOD 档放大线宽——那是另一个决策，不夹带在本修复里。
  被否决的备选：(a) 把 fill 换成深色——推翻 ADR-0008 的 Google 式观感；(b) 只加深 fill 颜色——「白色路面」语义会被破坏，且与 casing 的层次关系颠倒；(c) 用底图淡化压暗土地——那是栅格层的事，会连带影响地形/势力等其它图层。
- **街道名标注（2026-10-13 追加，Phase 5 首片）**——
  数据：`StreetSegment.name: Option<String>`（serde default + skip，无名是常态不落 YAML 字节）；`StreetSegment::polyline_midpoint()` 按累计长度取半程点（折线中段弯曲时标签也不钉在端点）。
  渲染：`streets::update_street_labels`（`StreetLabel` 组件池，仿 `PlaceLabel`）——只在 `LodTier::Edit` 显示（省/国家级一屏几十条街名即字海，Google 地图同样拉近才出街名）；字号 = 9px × view.scale（屏幕像素恒定）；z = 0.35（街道线之上、地名之下）。
  编辑：`StreetEditorState.name` 面板输入框，落进下一笔 `AddStreetSegmentCommand`；空串 = 无名。
  示例：xinhai 武昌城 roads.yaml 三段命名（中山路 / 起义街 / 环山街）。
