# GLOSSARY.md —— 术语字典

> 规约见 `AGENTS.md` §6.1：**讨论中出现新术语必须即时登记**，不允许事后补。
>
> 登记格式：术语 | 英文/拼音 | 定义 | 首次出现位置 | 所属维度
>
> 本文件为**全项目术语唯一权威源**，新术语即时登记；权威基线见 `docs/MAP_EDITOR_BASELINE_v1.md` §5。

## 核心术语

| 术语 | 英文 | 定义 | 所属维度 |
|---|---|---|---|
| 工程 | Project | 一个文件夹，包含一个完整世界的全部数据 | 元 |
| 世界 | World | 工程所描述的那套设定 | 元 |
| 图层 | Layer | 地图上可独立开关的一类要素 | 地理 |
| 时间点 | TimePoint | 时间轴上的一个位置，内部是**天数计数器**（`i64`，1-based）。纪元 / 年 / 月 / 日由可插拔显示历法（`Calendar`）派生，**不是存储字段** | 时间 |
| 时间区间 | TimeSpan | 两个时间点之间的范围，用于存储历史状态 | 时间 |
| 地点 | Place | 距离图中的节点 | 距离 |
| 边 | Edge | 地点之间的可通行路径，带距离与地形 | 距离 |
| 交通工具 | Vehicle | 定义速度、地形修正、前置条件的移动方式 | 距离 |
| 行程 | Journey | 从 A 到 B 的一条路径，含各段交通工具 | 距离 |
| 投影 | Projection | 某时刻对全部时间区间求值后的世界状态 | 时间 |
| 快照 | Snapshot | 某一时刻工程状态的完整保存 | 编辑 |
| 有效日速度 | effective daily speed | `speed_kmh × terrain_mult × (daily_hours / 24)` | 距离 |
| 换乘 | transfer | 同一地点切换到另一交通工具，零耗时状态转移 | 距离 |

## 事态与编辑术语（Phase 1）

> 来源：`docs/MAP_EDITOR_BASELINE_v1.md` §5.1（Phase 1 核心术语）。
> **单位 / 相遇提示 / 轨迹归属修正**来自 `docs/adr/ADR-0004-unit-ontology.md`（2026-10-11 裁定）。
> **相遇检测的判定口径 / 诊断分层**来自 `docs/adr/ADR-0005-encounter-hint.md`（2026-10-11 裁定）。

| 术语 | 英文 | 定义 | 所属维度 |
|---|---|---|---|
| **事态** | Situation | 一段有始有终的大事，**归纳容器 / group**：收拢若干事件节点（`steps`）与相关**单位**（`units`），节点可引用轨迹。是组织层，**不是** Event 的字段 | 事件 |
| **事态节点** | SituationStep | 事态的组成单元，**引用**一个 `EventId`，可带「链视角注释」与**轨迹引用**；**不复制事件内容** | 事件 |
| **单位** | Unit | 在地图上**独立运动的主体**（ADR-0004）。可引用角色 / 势力作 `owner`，也可**无主**（如「中央红军」）；`members` 列出成员角色。与 `Character` **分离**——单人移动也建 Unit | 事件 / 距离 |
| **轨迹** | Track | 某个**单位**在某张地图上的一段运动记录（waypoint 点列，点可选带时间戳）。运动主体是 `Track.unit`；事态节点通过 ID **引用**它（**多对多**）——**运动不归事态**（ADR-0004 推翻原「挂在事态节点上」口径） | 事件 / 距离 |
| **相遇提示** | Encounter Hint | 两个单位同时同地、而作者**未写事件**时给出的**提示**（ADR-0005）。判定三道门：时间重叠 + 空间贴近 + **未被事件覆盖**；命中已被覆盖则静默。**永不报错、不阻止保存**——留白是创作手段 | 编辑 |
| **相遇指纹** | Encounter Fingerprint | 提示的**稳定标识** `{单位A}\|{单位B}\|{地点}`（单位按字典序归并）。**刻意不含日期**——否则作者微调锚点会让提示「换一条」，永远认领不掉（ADR-0005 决策 4） | 编辑 |
| **诊断** | Diagnostic | 编辑器校验面板的一条结构化条目。`DiagLevel` 分**提示（Hint）**与**错误（Error）**：提示只列出、不阻止保存；错误才拦（ADR-0005 决策 5）。每条带 `fingerprint`（供认领）与可选 `anchor`（供点击定位） | 编辑 |
| **航点** | Waypoint | 轨迹上的一个点。二选一：引用 `PlaceId`（城市/室内推荐）或裸坐标 `(x,y)`（世界/区域荒野） | 距离 |
| **播放** | Playback | 时间轴以固定速率自动推进；**锁定在某个事态的 `span` 内**。不是独立播放器实体 | 时间 |
| **视图模式** | ViewMode | 地图基底的渲染方式 + 默认图层组合 + 可用工具集的三合一开关。四态：真实地形 / 色块地形 / 势力图 / 事件图 | 地理 |
| **色块地形** | Schematic Terrain | 一种视图模式：每种地形一种纯色，无立体感 | 地理 |
| **势力层** | Faction Layer | 一个可命名、可绘图、可重叠的区域集合；作者自定义其含义（国界/省份/贵族领/派系/教团/行会…）。**不预设角色，数量由作者定** | 地理 |
| **势力** | Faction | 势力层内的一个具体条目（如「大陈」「北境省」「镇北侯」「雅各宾派」） | 地理 |
| **势力归属** | Membership | 实体（人 **或** 势力层）与势力层之间的**多对多**归属关系，带**作者自定义的类型标签（String，非枚举）**与时间区间 | 地理 |
| **无主地** | Unclaimed | 所有势力层均未覆盖的区域，UI 灰显，**不报错** | 地理 |
| **声称 / 实控** | Claim / Control | 势力范围的两层语义：纸面声称 vs 实际管辖。**必须分开存储** | 地理 |
| **自然化** | Naturalize | 对笔刷所画区域的边界做去毛刺 + 样条平滑，保留大尺度特征、抹除小尺度锯齿 | 地理 |
| **沉浸模式** | Immersive Mode | 一键将所有浮动面板淡出至 10% 透明度，专注看地图 | 编辑 |
| **世界坐标** | World Space | 工程唯一的全局参考系，`Extent.origin` 描述各图在其父图中的位置 | 地理 |
| **图内局部坐标** | Local Space | 单张图内部的坐标，即 `GeometryData` 点列所用坐标系 | 地理 |
| **地图图标** | Icon / IconSpec | 声明在 `MapNode.icons` 上、渲染在该图上的一个标记。字段：`id` / `anchor` / `shape` / `size` / `color` / `drill_into`。声明顺序即绘制顺序（无 `z`） | 地理 |
| **图标锚点** | Icon Anchor | `IconSpec.anchor`：本图 `extent` 内的**归一化** `[0,1]²` 位置（左上原点、y 向下）。归一化使父图 `extent` 缩放时锚点不失效；缺省时渲染层按 `id` 哈希确定性摆位；越界值夹回 `[0,1]`，数据层不拒收 | 地理 |
| **钻入入口** | Drill Entry | 带 `drill_into` 的图标 = 双击进入该子图的入口；`None` = 纯注释标记，只画不可点。悬空引用由 `diagnose` 报 Error 级诊断但不阻止加载 | 地理 |
| **图标形状** | IconShape | 五种形状（菱形 / 圆形 / 十字 / 方形 / 四角星），缺省菱形（v0.2 §B.2 的钻入标记形状） | 地理 |
| **显式颜色优先** | Explicit Color Wins | `IconSpec.color`（`#RRGGBB`，`wv-core::color::Color`）声明后**优先于主题色**——不再因「已访问」转灰 | 地理 |
| **地形网格** | TerrainGrid | `wv-terrain` 的核心数据结构，承载高度场与地形类型 | 地理 |
| **矢量图层** | Vector Layer | 地图叠加层：地点标记、道路、边界、势力几何、事件点、文字标注。类比百度地图的「普通（矢量）地图」，与地形栅格图层分离、可独立开关（P5） | 地理 |
| **地形栅格图层** | Terrain Raster Layer | 地图底图：由地形网格逐格渲染出的栅格图像。两种实现：纯色平涂（色块地形）或瓦片图集贴图 | 地理 |
| **瓦片图集** | Tile Atlas / Tileset | 一张包含全部地形瓦片的美术图集图片；渲染时按 `f(kind, height, 邻居)` 查表选取瓦片 UV。仅存在于渲染层（`wv-app`），数据层不变 | 地理 |
| **过渡瓦片** | Transition Tile / Autotiling | 依据邻居地形自动选择边缘融合瓦片的做法。第一版先直铺（每格整瓦），过渡瓦片为后续增强（README §12-14） | 地理 |
| **衰减权重** | Falloff | 笔刷/选区按距离衰减的影响系数（高斯），用于软边缘与选区升降 | 编辑 |
| **主题包** | Theme Pack | 可整体替换的地图风格包（一个目录 + manifest + 5 类资产） | 美术 |
| **软件壳** | Locked Shell | 不替换的 UI 外壳（面板架构/字体/间距/图标语言） | 美术 |

## 动效 / 特效 / 音效术语（Phase 2）

> 来源：`docs/MAP_EDITOR_BASELINE_v1.md` §5.1b（Phase 2 新增术语，动画 / 特效 / 音效）。

| 术语 | 英文 | 定义 | 来源 |
|---|---|---|---|
| **抑制三分类** | Suppression Triage | 状态 / 动效 / 语义动效 三类，决定某视觉元素是否被 `intensity` 或速率抑制。**全项目单一权威见 §1.20** | 动画 §8.4 + 特效 §1.3 |
| **抑制判别口诀** | — | 两步判定：「去掉它会少看懂什么吗？」→ 不会=动效；会 →「丢事实还是丢『正在变化』？」→ 事实=状态 / 正在变化=语义动效 | 同上 |
| **动效分级** | Motion Tier | L0 瞬变 / L1 微反馈 80–120ms / L2 状态过渡 140–200ms / L3 强调动效。**仅 L3 可关闭** | 动画 §0 |
| **强度档位** | Intensity Level | `animation.intensity ∈ { full, reduced, off }`，只作用于 L3 | 动画 §0 |
| **生长笔尖** | Growth Head | 轨迹「已生长」末端的 6px 实心圆点，指示当前走到哪。**原称「发光游标」，已统一** | 动画 §1.2 |
| **当前时刻指示色** | Current-Time Color | `accent.cursor` 的语义：呈现于**时间轴游标 + 生长笔尖**两处（同一个 `current_day` 的两个视图）。**禁止拆成两个令牌** | 动画 §7.1 |
| **推算轨迹** | — | 顺序点（无时间戳）轨迹，由**均分到事态 span** 插值决定，不参与校验 | 动画 §1.3 |
| **粗轨迹** | Coarse Track | 只两端点（T3）的轨迹。与精轨迹**同视觉待遇**，仅加空心起点 + 极淡 `≈` 标识 | 动画 §1 |
| **状态—动效判别（R1–R4）** | State vs Motion Rule | §1.20 四条规则的合称 | 动画 + 特效 |
| **滞回** | Hysteresis | LOD 切换用 **±2%** 阈值防抖，避免连续缩放时一节节闪 | 动画 §4 |
| **元素级软切换** | Per-element Soft Switch | LOD 切换以元素为单位过渡，**不做全屏淡化** | 动画 §4 |
| **性质纹理** | Nature Texture | 势力层的斜纹 / 网点 / 十字纹，用于区分多层势力的性质。**归属待裁**（§4.2） | 特效 §1 |
| **四维描边语法** | — | 势力描边的四个正交维度：**色 / 粗 / 线型 / 偏移**，用于区分 ≥3 层嵌套势力 | 特效 §1 |
| **常驻底环** | Persistent Base Ring | 事件脉冲的**不闪烁**内环（1.0R, alpha 0.25）——属**状态**，高速档保留 | 特效 §4.2 |
| **涟漪环** | Ripple Ring | 事件脉冲的外扩环（1.0R→1.6R, alpha 0.5→0, 2.4s）——属**动效**，高速档暂停 | 特效 §4.2 |
| **锚点脉冲** | Anchor Pulse | 自然化完成后 60ms 的呼吸锚点反馈 | 动画 §3 |
| **falloff 双环** | Falloff Dual Ring | 笔刷光标的内外衰减双环可视化——属**状态**，不降级 | 特效 §2 |
| **即时非法反馈** | — | 非法操作「一动手即出现」的错误提示，不等松手 | 特效 §2 |
| **day 空间插值** | Day-space Interpolation | `progress = clamp((current_day − boundary_day) / delta_day, 0, 1)`，过渡进度是 `current_day` 的**纯函数**。时间轴驱动动画的推荐范式 | 动画 §5.1 |

## 历法 / 旁白 / 几何化术语（2026-01）

> 来源：`docs/adr/ADR-0006-gregorian-calendar.md`（双历制）与 `examples/xinhai-1911/`（旁白实体、Place/Event 矩形几何化）。

| 术语 | 英文 | 定义 | 所属维度 |
|---|---|---|---|
| **双历制** | CalendarKind | `Calendar.kind` 可插拔历法枚举：`simple`（缺省，days_per_year/days_per_month 均分）与 `gregorian`（真公历，Hinnant 整数算法，含闰年）。存储层永远是天数计数器，历法只管显示派生（ADR-0006） | 时间 |
| **旁白** | Narration | 独立实体 `Narration{id,name,text,avatar?}`，存 `timeline/narrations/`，事件经 `Event.narrations` 多对多引用。承担过程叙述与因果链表达——**因果链用旁白文案 + 时间派生播放顺序表达，不设箭头字段**（ADR-0002 口径不变） | 事件 |
| **事件落点** | EventLocation | `Event.location: {place, x?,y?,w?,h?}`——引用 PlaceId 并可带子矩形覆盖；全缺省时按 Place 中心点判定，容差 EPS=1e-3 | 事件 / 地理 |
| **矩形地点** | Rectangular Place | `Place{x,y,w,h}`：w/h 均给出时为矩形（含边界），均缺省时为点图标；**w/h 必须成对** | 地理 |

## Google 式地图体验（2026-10）

> 来源：用户要求「大地图尽量模仿 Google Maps」与 `docs/adr/ADR-0008-google-default-map-experience.md`。

| 术语 | 英文 / 拼音 | 定义 | 所属维度 |
|---|---|---|---|
| **Google Default Map 式地图壳** | Google Default Map-style map shell | 离线模仿 Google 普通地图的信息层级与交互：近白陆地、淡水、公园绿、白路灰缘、红点白环 POI、左键拖动平移、贴边缩放控件；**不接入在线瓦片或专有数据**。首次出现：用户 2026-10-13 要求与 ADR-0008 | 地理 / UI |
| **道路双层描边** | Road casing and fill | 道路用浅灰外缘（casing）与白色主体（fill）两层表示；两层宽度差形成 Google 式路缘。首次出现：ADR-0008。（2026-10-14 追记：**街道**自 ADR-0010 §5「街道双层描边」起同口径——两个宽度档各画 casing + fill 两层；此前只画 fill，地图模式下白 fill 落在近白土地上等于隐形） | 地理 / 渲染 |
| **亚像素渲染** | Subpixel rendering | 每个逻辑地形格输出多个纹理像素，使水陆 / 绿地边界在放大与线性采样下得到平滑过渡；分辨率受 4096² 纹理预算约束。首次出现：ADR-0008 | 地理 / 渲染 |
| **地图朝向** | Map orientation | `MapNode.orientation`（度）：本图在屏幕上的旋转角，**0 = 正北朝上，正值 = 顺时针偏转**（90 = 北朝右）。是**地图的属性**（作者决定这张图的「北」朝哪）而非相机状态——YAML 是唯一存储处，渲染层只读（2026-10-14 起由 `wv-app` 的 `camera::apply_map_orientation` 每帧写进相机旋转，视图真正随它转动）。指南针面板经 `SetMapOrientationCommand` 编辑此值。首次出现：`crates/wv-core/src/map/mod.rs` + 指南针面板（e84eec9） | 地理 / 渲染 |
| **屏幕对齐文字** | Screen-aligned labels | 地名 / 街道名标注**不随地图旋转**的约束（2026-10-14 用户裁定）：地图按朝向旋转后，`Text2d` 标注实体取相机旋转的**逆**叠加，屏幕角恒为零；标注相对 POI 的「上移避让」偏移也按**屏幕上方**取世界方向（`camera::screen_vec_to_world`）。Google Maps 旋转时标签保持水平的同款行为。首次出现：`crates/wv-app/src/render/{camera,drill,streets}.rs`（2026-10-14） | 地理 / 渲染 |

## 缩放到街道：分级缩放与街道图层（2026-10）

> 来源：用户要求「google地图那样，缩放到街道」（范围与数据源由用户拍板「都要 / Overture / 你决定」），落地于 `docs/adr/ADR-0009-graded-tile-zoom-to-street.md` 与 `docs/adr/ADR-0010-street-vector-layer.md`。

| 术语 | 英文 / 拼音 | 定义 | 所属维度 |
|---|---|---|---|
| **分级瓦片缩放** | Graded (discrete-level) zoom | 缩放采用**离散级别 + 级间平滑插值**，而非无极连续缩放。理由：`f32` 在世界坐标 4096 量级下相邻可表示间距约 0.0005，深缩放到 10⁵ 倍会放大成约 50 像素抖动；分级让每级在**自己的原点附近**工作。首次出现：ADR-0009 | 地理 / 渲染 |
| **级（缩放级别）** | Zoom level | 一个离散缩放档。**由「图在层级树中的深度」派生**，规划 6 级（世界 → 大区 → 区域 → 城市 → 城区 → 街道）；`MapLevel` 四值枚举**不扩充**——级数是参数，不是结构。首次出现：ADR-0009 | 地理 |
| **图原点** | Extent origin | `Extent.origin: Option<Point>`：本图左上角在**父图局部坐标系**中的位置。`None` = 作者未声明（序列化时跳过，既有 YAML 零迁移）。它补上了「多地图层级」在坐标上一直缺的一环。**派生**：未声明时由父图上指向本图的钻入图标反推（见下条）。首次出现：ADR-0009（§6 追加细化类型） + `crates/wv-core/src/map/mod.rs` | 地理 |
| **落点派生** | Origin derivation | 子图未声明 `Extent.origin` 时，用父图上第一个「`drill_into == 本图` 且显式声明 `anchor`」的图标反推落点：图标标记的是子图**中心**，故左上角 = 图标点 − 子图 `extent/2`。优先级：显式 `origin` ＞ 图标派生 ＞ `(0,0)`。理由：世界图上那个城市圆点本就是「城市在哪」的**唯一**书面表达，不该让作者把同一位置写两遍（原则四）。只认显式 `anchor`——哈希回退摆位是渲染层避免重叠的权宜，不决定世界坐标。首次出现：ADR-0009 §6 + `crates/wv-core/src/map/coord.rs` | 地理 |
| **还原帧** | MapFrame | `wv_core::coord` 的逐层上溯结果：`{world_origin, extent}`。把上溯预先算好，供「图内局部坐标 ↔ 世界坐标 ↔ 归一化坐标（UV）」统一换算——**唯一实现点**（基线 §2.15 的「禁止多 System 各写一遍」由此可执行）。首次出现：ADR-0009 + `crates/wv-core/src/map/coord.rs` | 地理 |
| **视图空间** | View space | 渲染与相机所用的**唯一**坐标系：**就是根图（世界）坐标系**（基线 §2.15）。相机、`ViewState::cursor_world`、钻取落点、所有 mesh 顶点都在此空间；本图局部坐标必须先经 `GridConfig::map_to_view` 换算进来，而根帧内容（轨迹 / 事项事件）是恒等映射（`projection::root_to_view`，所属帧仍待裁见 README §12-26）。首次出现：ADR-0009 §9 + `crates/wv-app/src/state.rs` | 地理 / 渲染 |
| **栅格原点** | Raster origin | `GridConfig.raster_origin`：当前**显示的那张栅格**在世界里的左上角。栅格属于祖先图，故它恒为根图图帧原点 `(0,0)`。**地形专用**：栅格采样、笔刷、chunk 划分、小地图、地形回退 sprite。2026-10-14 由 `GridConfig.origin` **更名**而来（原名同时被当成图帧原点用，正是 ADR-0009 §9 那个偏移 bug 的成因）。首次出现：ADR-0009 §9 + `crates/wv-app/src/state.rs` | 地理 / 渲染 |
| **图帧原点** | Map frame origin | `GridConfig.map_origin`：**当前这张图**的 `extent` 左上角在视图空间（= 世界）里的位置，由 `Extent.origin` 沿父链累积得到（`wv_core::resolve_frame_in`，见「还原帧」）。**本图局部内容专用**：地点、街道、街道名、地图外框、编辑预览；唯一换算是 `GridConfig::map_to_view(local)`。与栅格原点**只在当前图自带栅格时相等**——继承底图时二者相差一个图帧原点，混用会把本图内容整体画到视野之外（武昌城案例见 ADR-0009 §9）。首次出现：ADR-0009 §9 + `crates/wv-app/src/state.rs` | 地理 / 渲染 |
| **街道矢量层** | Street vector layer | 街道**地物**图层（`Layer` 语义），与 `distance.edges` 的**城市间行程连线**是两个永不合并的语义层。渲染为批量/实例化的矢量线，按 `class` 做 LOD + 视口裁剪。首次出现：ADR-0010 | 地理 / 渲染 |
| **双供给** | Dual supply | 街道几何的「一个接口、两个实现」：虚构世界 = 作者手绘落 YAML 图层文件；真实世界 = Overture Parquet **只存引用**。两者产出同一份路段流，渲染层不关心来源。首次出现：ADR-0010 | 地理 / 数据流 |
| **路段流** | Road segment stream | 供给层交给渲染层的统一产物：点列 + `class` + 属性。**不含长度结论**——需要长度时由点列 Haversine 派生（§4.2）。首次出现：ADR-0010 | 地理 / 数据流 |
| **街道类别** | Road class | 街道段的 12 类词汇（wv_core::street::RoadClass，命名对齐 Overture），虚构与真实路网共用；lod_rank() 是 LOD 分档的**唯一派生入口**。数据只存类别，线宽 / 颜色 / 档位由渲染层派生。首次出现：ADR-0010 §5 | 地理 / 数据 |
| **街道名标注** | Street label | 有 `name` 的街道段在**编辑级视野**（LodTier::Edit）显示的中点文字标签；锚点 = `StreetSegment::polyline_midpoint()`（按累计长度取半程，折线弯曲也不钉端点）。无名段不产生任何标注——手绘的大多数小巷无名，命名不是门槛。首次出现：ADR-0010 §5（Phase 5 首片） | 地理 / 渲染 |
| **底图切片** | Raster slicing | `wv raster-slice <工程> <map_id> [size]`：沿坐标链把来源图栅格被子图覆盖的那块**最近邻采样**成子图自己的 `terrain.bin`（出界取海平面，不镜像对侧）。是「按父图 bbox 一键生成子图栅格」的落地（README §12-24），切片后底图继承（ADR-0009 §7）对该图退场、精度升级为自身。首次出现：ADR-0009 §5 落地记录 | 地理 / 数据流 |
| **虚拟脏路径** | Virtual dirty path | 街道编辑命令上报的落盘标记 `streets/<layer_id>`——不是真实文件路径，由 `wv-io` 落盘时映射回 `maps/<map>/<source>`。为什么绕一层：街道文件物理上在 `maps/<map>/` 下，若直接报真实路径，`terrain_is_dirty` 的 `maps/<map>/` 前缀匹配会把「画一笔街道」误判成「整张图地形脏了」而白白重写 `.bin`。首次出现：ADR-0010 §5（第 3 项） | 地理 / 数据流 |
| **来源图** | Source map (of the displayed raster) | 当前视图**实际**显示其栅格的那张图。当前图自带 `raster` 时就是它自己；否则是沿 `parent` 向上**最近**一张自带栅格的祖先。由 `DrillState` 的当前图推得，**不落进 `World`**（P7）；`grid.cell_size` / `grid.raster_origin` 一律与来源图同源（**2026-10-14 更名**：原 `grid.origin` 即今天的 `raster_origin`；本图局部内容改走**图帧原点**，原字段一名两用是 ADR-0009 §9 那个偏移 bug 的成因）。首次出现：ADR-0009 §7 + `crates/wv-app/src/render/terrain_view/mod.rs` | 地理 / 渲染 |
| **底图继承** | Raster inheritance | 图没有自带栅格时，沿 `parent` 向上取最近一张来源图的地形**直接渲染**，不复制数据。理由：工程是渐进长出来的，城市还没刷地形时城市视图应继续看到父图那一块，而不是黑屏；城市一旦有了 `terrain.bin`，纹理自动切过去。代价：继承来的精度就是父图的精度（街道级会糊，README §12-24）。首次出现：ADR-0009 §7 | 地理 / 渲染 |
| **级缩放区间** | Level zoom band | 一级允许的缩放范围，锚在「整幅图适配窗口」的尺度上：`fit = max(w/window.w, h/window.h)`、`min = fit/80`、`max = fit*1.5625`。**各区间的绝对数值有意重叠**（子图区间跨过父图内界），故换级时 `scale` 无需改动。由 `crates/wv-app/src/render/zoom_level.rs` 的 `LevelZoom` 承担；旧的全局常量 `SCALE_MIN=0.1` / `SCALE_MAX=8.0` 已删除。首次出现：ADR-0009 §8 + `crates/wv-app/src/render/zoom_level.rs` | 地理 / 渲染 |
| **换级** | Level crossing | 滚轮在**当前级边界上继续同向滚动**所触发的层级切换：往外 → 上一级（`drill.up()`），往里 → 光标下的子图（`drill.enter()`）。触发条件因此是「已在边界 **且** 继续滚」，不是「进入区间」；意图经 `ZoomCrossRequest` 交给 `drill::apply_zoom_cross` 独家执行（地图路径的唯一写入口仍是 `DrillState`）。**连续性的证明**：子图 `fit` 更小 ⇒ `parent.min ≤ child.max`，故换级前后 `scale` 同值合法。首次出现：ADR-0009 §8 + `crates/wv-app/src/render/{camera,drill}.rs` | 地理 / 交互 |
| **钻取过渡目标缩放** | Fly-in target scale | 双击/面包屑换级时相机过渡的终点缩放 = `clamp(min(from_scale, child.fit))`：整幅子图铺满窗口，但作者已贴得更近时不为换级拉远视野；末尾 clamp 保证终点必落在**新级区间**内。取代历史上硬编码的 `4.0`（分级后会越界，进城后第一次滚轮被硬拽）。首次出现：ADR-0009 §8 + `render::zoom_level::fly_in_scale` | 地理 / 交互 |

## 地形边界渲染与草图式生成（2026-10）

> 来源：2026-10-14 三轮设计讨论（网格编辑 · Google 式平涂渲染 · 多边形地物工具）。计划与待裁清单（Q 组）见 `docs/TERRAIN_RENDER_AND_SKETCH_TOOL_PLAN_v0.1.md`；方向（D 组）已定，参数待裁。

| 术语 | 英文 / 拼音 | 定义 | 所属维度 |
|---|---|---|---|
| **等值线边界** | contour boundary | 从高度场提取的等值线作为边界的事实来源：海岸线 = 海平面等值线、崖线 = 坡度阈值线；地表覆盖（森林/沙漠等）边界则归类型场（软边过渡）。渲染期派生，永不落盘 | 地理 / 渲染 |
| **类型优先序** | type precedence | 多种地形类型在同一格点交汇时的全局裁决序（计划 Q5 缺省建议：深水 < 岸 < 沼泽 < 平原 < 沙漠 < 森林 < 丘陵 < 山地 < 雪原）。只影响渲染归属与生成默认，不改数据 | 渲染 / 编辑 |
| **鞍点规则** | saddle rule | marching squares 遇对角同型（棋盘格）歧义时的确定性裁决（计划 Q6 缺省建议：四邻均值法）。同数据必同结果 | 渲染 |
| **晕渲图** | hillshade | 由高度场派生的山体阴影叠加（光源方位 / 俯角 / 强度参数化），经典制图缺省 315° / 45°。渲染期计算 | 地理 / 渲染 |
| **LOD 抽稀** | LOD simplification | 远景按 Douglas-Peucker 容差抽稀等值线折线以减顶点；容差随 LOD 分档 | 渲染 |
| **草图式生成** | sketch-based generation | 作者画粗略意图（多边形 / 脊线）、算法展开成细节网格的生成范式 | 编辑 |
| **意图包络** | intent envelope | 多边形工具中作者圈出的约束区：是**意图不是边界**，真实边缘由剖面衰减 + 噪声在包络附近生成，偏离有界 | 编辑 |
| **地物配方** | landform recipe | 每种地物的生成参数集（剖面曲线 + 噪声参数 + 类型分带 + 过渡带宽度）。YAML 数据化，内置缺省 + 工程可覆盖 | 编辑 / 数据 |
| **高度分带** | elevation banding | 生成时按高度区间推导默认地形类型的规则表（如雪线以上 = 雪/岩）。只是生成默认，落盘的是格子类型 | 编辑 / 数据 |
| **融合权重** | blend weight | 生成结果与既有高度场按衰减曲线混合的系数；复用笔刷「衰减权重（Falloff）」的高斯实现，公式不二实现 | 编辑 |
| **手势即命令** | gesture as command | 多边形工具的架构口径：圈选 + 配方 + 种子 = 一条宏 `EditCommand`，执行后网格是唯一事实，多边形只留撤销历史、不留存为实体 | 编辑 / 架构 |
| **混合像素模式** | hybrid pixel mode | 像素纹理填充 + 矢量曲线裁切遮罩的渲染模式：填充像素化、边缘平滑。与整瓦片 autotile（边缘硬）相区别 | 地理 / 渲染 |
| **抖动** | dithering | 像素风瓦片伪造平滑边缘的棋盘格过渡画法——整瓦片路线下边缘平滑的妥协手段 | 美术 / 渲染 |
| **行进方块** | Marching Squares | 对标量场逐对偶格提取等值线的经典算法：按四角样本内外之分查 16 案例表输出线段。`wv_terrain::isoline::height_segments` 是唯一实现点（plan Q4/Q6 裁定其采样与鞍点口径） | 渲染 / 算法 |
| **格心采样** | Center sampling | 等值线提取的采样口径：每格存的值即格心样本，等值点落在相邻格心连线上（曲线穿行格子内部）；单格小岛 = 内切菱形（plan Q4） | 渲染 / 数据 |
| **链装配** | Chain assembly | 把 marching squares 输出的离散线段按端点接成折线链；端点匹配用 `f32::to_bits` 位级精确比对（相邻对偶格共享边交点由同公式算出，位级一致，不需要浮点容差） | 渲染 / 算法 |
| **等值线叠加层** | Isoline overlay | `VectorFlat` 浏览态在平涂底图之上的几何边界描边（海岸线 + 类型场边界），单 mesh 批量、派生不落盘；实现 `wv-app/render/terrain_view/isoline_overlay.rs`，重算触发 = 结构键 + `chunks_dirty` | 地理 / 渲染 |
| **矢量地形** | Vector terrain | `VectorFlat` 浏览态的 UI 定名（2026-10-14 用户裁定，此前顶栏叫「等值线」——等值线只是它的描边产物）：Google 地图矢量瓦片式渲染 = 高度场平涂填充（水陆边由双线性高度决定）+ 等值线叠加层描边。浏览态两模式之一（另一个是「色块地形」），见 `ViewMode::BROWSING` | 地理 / 渲染 |
| **色块地形** | Schematic terrain | `Schematic` 浏览态的既有 UI 名：一格一色，直接呈现每格的地形类型（`colors::core_terrain`）。浏览态两模式之一 | 地理 / 渲染 |
| **预填充** | prefill | 「新建地图」对话框按选择整图铺满海洋（高度 < 0 + 水系类型）或陆地（高度 ≥ 0 + 陆系类型）的初始栅格；同时写高度与类型——VectorFlat 填充「高度赢过类型」口径下二者必须一致 | 编辑 / 数据 |

## 古舆图主题与氛围渲染（2026-10）

> 来源：用户 2026-10-14「日间古舆图 / 暮色旧纸」配色定稿与五条渲染规则，决策见 `docs/adr/ADR-0016-jrpg-antique-map-default-theme.md`；定稿色值见 `docs/ART_STYLE_SPEC_v0.1.md` §1.4.4。

| 术语 | 英文 / 拼音 | 定义 | 所属维度 |
|---|---|---|---|
| **古舆图** | Antique Map | 日间古舆图配色的正式名：泛黄宣纸底 `#F2EAD8` + 褐墨线条 `#4A3F35` + 灰蓝水 `#98A9B9` + 灰橄榄绿 `#A8B898`，低饱和历史感 JRPG。ADR-0016 定稿的**默认地图观感**（`theme/colors.rs` 现行值） | 美术 |
| **暮色旧纸** | Dusk | 古舆图的暗调变体：深褐灰纸底 `#3A342E` + 暗棕路 `#705844` + 深蓝灰水 `#44505C`。对应 `MapPalette::Dusk`，适合黄昏 / 雨夜 / 夜景叙事场景 | 美术 |
| **地图主题** | MapPalette | 与视图模式（`ViewMode`，技法轴）**正交**的配色轴：`Daylight`（古舆图）/ `Dusk`（暮色旧纸）两档。GoogleMap 模式继续绑 `colors_google` 不受影响；轮廓图三色不随其切换。持久化于 `ui-layout.yaml` 的 `map_theme` 字段。`ViewMode::JRPG` 已移除，色板切换职责由它承接。首次出现：ADR-0016 + `theme/palette.rs` | 渲染 |
| **纸面雾霭** | Aerial veil | 缩放联动降饱和的实现载体：纸色半透明全图四边形（Z 在等值线之上、道路之下），alpha 随相机缩放**连续**变化——远景要素向纸色退散的手绘空气感。不改 CPU 纹理（每帧只改 alpha）。首次出现：ADR-0016 + `render/terrain_view/atmosphere.rs` | 渲染 |
| **纸张噪点** | Paper grain | 世界锁定的极淡程序化噪声叠加层（定种子生成、alpha ≈ 0.035），模拟旧纸纤维；随图平移缩放，如真实纸张。首次出现：ADR-0016 + `render/terrain_view/atmosphere.rs` | 美术 / 渲染 |
| **文字晕边** | Label halo | 地名 / 街名 `Text2d` 的纸色描边晕：每条标注 4 向偏移副本先画 halo 再画本体，保证文字在绿地 / 水面上可读（暮色用深纸色）。首次出现：ADR-0016 + `render/label_halo.rs` | 渲染 |
| **建筑挤出** | Building extrusion | 2.5D 假立体画法：建筑轮廓多边形沿固定世界方向挤出侧面 quad + 顶面平涂 + 柔和投影（ART_STYLE_SPEC §1.5：左上光、右下投影、长度 0.5× 高度、alpha 0.35、禁纯黑）。建筑数据走「街道同款」作者数据路径（ADR-0010 模式），渲染不依赖数据来源 | 渲染 |

## 设计上不做（负向清单）

> 来源：`docs/MAP_EDITOR_BASELINE_v1.md` §5.1c（Phase 2 明确「设计上不做」项，反向登记）。

| 项 | 所属 | 理由（一句话） |
|---|---|---|
| BGM / 环境音 / 氛围音 | 音效 | 编辑器不是游戏 |
| UI 点击音 | 音效 | 零信息增量 |
| **笔刷与一切连续操作音效** | 音效 | 视觉反馈密度已足够 |
| 加载完成音 / 游戏化音效 / 空间音频 | 音效 | 同上 |
| 墨水扩散（笔刷涂抹延迟） | 动画 | 所见即所得不可妥协 |
| 角色拖尾 | 动画 | 与「位置唯一性校验」冲突 |
| 撤销倒放录像 | 动画 | 撤销不是回放 |
| 缩放 / 平移惯性 | 动画 | 跟手优先于顺滑 |
| 全屏 LOD 淡化 | 动画 | 连续缩放会一节节闪 |

## 作废 / 降级术语

> 来源：`docs/MAP_EDITOR_BASELINE_v1.md` §5.2（作废/降级术语）。

| 术语 | 处置 | 原因 |
|---|---|---|
| **战役 / Campaign** | ❌ **作废** | D1 裁定改用「事态 / Situation」 |
| **链步 / CampaignStep** | ❌ **作废** | 同上，改用 `SituationStep` |
| **势力层级 / FactionLevel** | ❌ **作废** | D2 裁定：不设层级枚举 |
| **国境线 / 省份 / 贵族封地**（作为独立概念） | 🟡 **降级** | 改为「势力层」的示例，不再单独登记 |
| **地图模式 / Map Mode** | ❌ **作废** | 统一称「视图模式」（避免混淆） |
| **轨迹主体 `Track.owner`**（注释为「角色或势力」） | ❌ **作废** | ADR-0004：改用 **`Track.unit`（单位）**——旧口径二义，且「中央红军」这类无主主体无法表达 |

## 待决术语（语义未收敛，暂缓冻结）

> 现有 2 条 + 合并 `docs/MAP_EDITOR_BASELINE_v1.md` §5.3（待裁术语）的 3 条。（2026-10-13 陆续追加 ADR-0009 / ADR-0010 落地项 7 条：多级栅格驻留、地面尺度、街道几何存放与分片、多入口落点歧义、渲染层测试闸门、继承底图精度提示、换级淡化。）（2026-10-14 追加 ADR-0009 §9 落地项 1 条：轨迹/事件坐标所属帧。）

| 术语 | 状态 | 说明 |
|---|---|---|
| 二进制分块 | 待裁（README §12-4） | 格式版本策略未定；基线 §5.3「地形双通道编码」是同一主题，已合并为下一条 |
| requirements 表达式 | 待裁（README §12-3） | 条件表达式引擎语法范围未定 |
| 地形双通道编码（含二进制分块格式） | 待裁（README §12-10） | I16 高度 + U8 类型两通道是否合并为一个 `terrain.bin`。基线 §5.3 原条目，与「二进制分块」格式版本策略合并登记 |
| 带洞多边形 | 待裁（README §12-8） | 第一版不支持，作者用多 sub-span 近似；未来可加 `PolygonWithHoles` 变体 |
| 反向索引失效策略 | 待裁（README §12-9） | 第一版「每次查询重建」，N 超 1 万时改增量 |
| 地图空间落点 | 待裁（README §12-12） | 第一版锚点归 `IconSpec`（`children` 仍只存 `Id`）；待裁哈希回退是否长期保留、是否把「钻入入口」从 `icons` 拆成独立字段 |
| 多级栅格驻留策略 | 待裁（README §12-19） | 每图自带 `raster` 后同时驻留几级？懒加载范围（祖先链 / 加相邻级预取）、驻留上限、卸载时机未定。数据模型方向已由 ADR-0009 定死 |
| 地面尺度 | 待裁（README §12-20） | 「一格 = 多少公里」。真实地图可由 `geographic_source.bbox` 派生；虚构地图无真值。**不是**要恢复 `width_km`——2026-10-12 裁定（甲）不变 |
| 街道几何存放与分片 | 待裁（README §12-21） | 手绘路网放 `Layer.source` 图层文件的**方向已定**（ADR-0010）；单文件段数阈值、分片命名与索引约定未定。**不**为街道几何开二进制例外 |
| 多入口落点歧义 | 待裁（README §12-22） | 同一子图有多个钻入图标且都带 `anchor`、而子图未声明 `origin` 时取「第一个」；是否改为校验提示或定义入口优先级未定。「显式 `origin` 压过派生」这条优先级不动 |
| 渲染层测试闸门 | 待裁（README §12-23） | §7.1 三条门槛命令都不编译 `wv-app` 的 `#[cfg(feature = "app")]` 模块（实测 `cargo test -p wv-app` = `0 passed`）。补 `--features app` 还是单列一条渲染层检查命令未定 |
| 继承底图精度提示 | 待裁（README §12-24） | 底图向上继承后（ADR-0009 §7），世界图 4.9 m/格 放到街道级会糊。提示形式（校验面板 / 状态栏 / 地图树角标）与是否要「按父图 bbox 生成子图栅格骨架」未定。按 V1 只提示、不阻止 |
| 换级视觉过渡 | 待裁（README §12-25） | `§2⑦` 的「淡化」未实现——今天换底图是整张纹理硬切换（`rebuild_texture_on_mode_change`）。淡化形式、时长、可否关闭未定；不得引入位移插值、不得延迟层级切换 |
| 轨迹/事件坐标所属帧 | 待裁（README §12-26） | 视图空间就是根帧，轨迹 waypoint 与事项事件位置今天走恒等映射（`projection::root_to_view`），数据里没有「这份坐标属于哪张图」的声明。是否允许按子图局部坐标书写、城市内事件在父图视野下如何呈现未定 |
| 换级淡化 | Level-cross basemap fade | 换图（换级/钻取）时的视觉过渡：新底图**同帧即时重建**（切换不延迟），旧底图以克隆像素的淡出精灵（`BasemapFadeOut`，z=0.05，盖分块底图、在道路之下）在 `LEVEL_FADE_SECS = 0.4s` 内线性降透明至归零回收。只淡出旧图、不淡入新图；无位移插值；换模式/换 soften 不触发。实现：`crates/wv-app/src/render/terrain_view/mod.rs`。首次出现：ADR-0009 §5 追写 + `terrain_view.rs` | 地理 / 渲染 |
| 淡出精灵 | Fade-out sprite | 换级淡化的载体：`spawn_basemap_fadeout` 把**旧纹理像素克隆**成新 `Image`（句柄会被原地覆写，不克隆会瞬间变新图）挂到 `Sprite` 上逐帧降 alpha，归零即 despawn。首次出现：ADR-0009 §5 追写 + `terrain_view.rs` | 地理 / 渲染 |

| **职责域目录** | Domain directory | wv-app 与 wv-core 按职责域分目录：前者 
ender/（地图渲染）、edit/（UI 动作→EditCommand）、ui/（面板/组件）、	heme/（外观令牌）；后者 ase/ 	ime/ map/ model/ derived/ source/。**目的**：让 AI 通过目录名快速定位（无需扫 28 个平铺文件）。模块挂点与公开 API 不变。首次出现：2026-10-14 目录级别重构 + docs/adr/README.md §5 | 工程 |
| **保路径重导出** | Path-preserving re-export | wv-core 因 AGENTS.md §3 与文档直接引用顶层路径（如 wv_core::time::TimelineQuery），lib.rs 用 pub use base::id;（**模块**）与 pub use base::id::Id;（**类型**）两套并存的 re-export 让「wv_core::id 模块路径」与「wv_core::Id 类型路径」都仍可达。**命名空间不同**，不冲突。**作用**：目录重构不动公共 API。首次出现：2026-10-14 目录级别重构 + docs/adr/README.md §5 | 工程 |
| **欢迎页** | Welcome screen | 无工程在手时的全屏接管界面（压暗 + 居中卡片）：「新建工程… / 打开工程… / 最近工程」。点不到地图与面板；是「先立工程才能画」的 UI 侧入口。判据 `WorldProjection::has_open_project()`（= `project_dir.is_some()`）。首次出现：2026-10-14 用户需求（新建项目对话框 + 启动锁定）+ `crates/wv-app/src/ui/welcome_screen.rs` | 工程 / 编辑 |
| **无工程锁** | No-project lock | 「程序刚打开不能画图」的三道门禁同一判据（`has_open_project`）：① 欢迎页压暗吃点击（UI 侧）；② `collect_brush_stroke` / `commit_street_click` 直接早退（数据侧，防穿透）；③ 命令分发器挡下 NewMap / Save / SaveAs / Export（命令侧）。NewProject / OpenProject / CloseProject 是入口本身，不拦。首次出现：2026-10-14 用户需求 + `ui_shell.rs` / `camera.rs` / `streets.rs` | 工程 / 编辑 |
| **图层帧** | LayerFrame | `MapNode.layers[].z / visible / opacity` 的**每帧投影**：`render/layer_frame.rs` 的 `resolve_layer_frame` 从当前图（`DrillState` 所指）的 layers + `ViewState` 会话开关求值出七类渲染组各自的 `LayerBinding`，写入 `LayerFrame` 资源。求值是纯函数、拉动式（AGENTS §3）；手动相等比较，绑定不变不触发 Bevy change detection。**kind→渲染组映射与全部语义的唯一出处**（ADR-0013）。首次出现：2026-10-14 六层渲染架构方案 C + ADR-0013 + `render/layer_frame.rs` | 渲染 |
| **图层绑定** | LayerBinding | 一个渲染组从作者图层声明求出的三元绑定：`visible`（**YAML 声明 ∧ 会话开关**的合取）、`opacity`（clamp 0..=1，落材质 alpha）、`z_offset`（槽位偏移的世界 Z 值）。未声明 kind = 中性绑定（沿用会话开关、不透明、不偏移）——既有地图渲染零变化。首次出现：ADR-0013 + `render/layer_frame.rs` | 渲染 |
| **槽位偏移** | Slot offset | ADR-0013 对作者 `z: i32` 的 v1 语义：按每格 `SLOT_STEP = 0.05` 世界 Z 平移该 kind 渲染组的**内建槽位**（各渲染模块的既有 Z 常量：道路 0.09–0.12、POI 0.21–0.22、势力 0.3、标注 0.35/0.4……），clamp ±`Z_CLAMP = 4` 格。**不是**「按 z 排序全量分带」——那会拆掉组内次序编译期断言、冲突标注远偏移，留待 v2（ADR-0013 §6 待裁）。`z` 全缺省时渲染与历史版本逐位一致。首次出现：ADR-0013 + `render/layer_frame.rs` | 渲染 |
| **模态置顶** | Modal pinning | UI 层序约定（2026-10-14 用户要求「弹窗/浮动都在最顶层」）：egui 绘制序 `Background < Middle < Foreground < Tooltip`。浮动面板走缺省 `Middle`（点谁谁置顶）；**一切模态弹窗**（新建地图 / 新建工程 / 命令面板 / 欢迎页 / 启动加载）必须在 `Order::Foreground` 压住面板；Tooltip 在 Foreground 之上，弹窗内 `on_hover_text` 不受影响。命令面板窗口曾缺省 Middle 而**低于**自己的 Foreground 遮罩——鼠标点击全被遮罩吃掉（已修）。约定表见 `ui/widgets.rs` `float_window` 文档。首次出现：2026-10-14 用户需求 + `ui/new_map_dialog.rs` / `ui/command_palette_window.rs` / `ui/widgets.rs` | 工程 / 渲染 |