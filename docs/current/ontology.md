# 数据模型与本体 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。
> 每条附溯源链与修正注记。原文见 [ADR 档案](/adr/)。

## 1. 势力层模型 ✅

**现行口径**：政治划分用**数据**（势力层 / 势力 / 归属）承载，**不用枚举**；势力层是多边形集合；归属多对多、带自定义类型标签与时间区间。

- **实现补充**：`FactionLayer`（维度）· `Faction.layer/rank/seat/memberships` · `Membership { target, kind, span }`（`to` 改名 `target` 修 serde 冲突）· `GeometrySpan.relation`（claim/control）。旧 YAML 零迁移；默认层内存合成不写盘；V1 只诊断不阻止。
- **溯源**：[ADR-0001](/adr/adr-0001-faction-layer-model)（2026-10-11）→ [ADR-0027](/adr/adr-0027-faction-layer-multi-compatible)（2026-10-18）
- **关联待决**：§12-42（反向索引失效策略）· §12-43（贵族领地是否派生）· §12-44（逐层图层条目）· §12-45（人物 memberships）· §12-46（文件名可读性）

## 2. 势力领地与划分语义 ✅

**现行口径**：领地「**生成即存储**」——划分时一次生成新势力环 + 宿主余部，双双持久化；渲染只读 span。除法 = 同一刀切两沿（`DivideFactionCommand` **单事务原子 undo**，余部碎块化 v1 拒绝）。`GeometryData` 新增 **`PolygonWithHoles`**（国中国必然产生环形余部）。空势力 v1 = 派生补集不建实体。贴地吸附 = 水陆界面 / 河链骨架 / 山脊峰值三族，无果退化直线 + 提示，湖面归空势力。

- **溯源**：[ADR-0028](/adr/adr-0028-faction-division-and-territory)（2026-10-18）
- **注**：此法替代了 README §12-8「带洞多边形第一版不支持」的原待决口径（`PolygonWithHoles` 已落地）。

## 3. 事态本体 ⚠️ 含被修正子句

**现行口径**：新增「事态（Situation）」作为事件之上的组织层；节点只引用 `EventId` 不复制事件；**链的顺序只能由时间派生**（硬约束 A1，不能存序号）；**轨迹必须可选、可粗略**（A2，只两端点即可）。

- **⚠️ 被修正**：其 **Q4（「谁怎么动」记在哪里）** 的答案已被 [ADR-0004](/adr/adr-0004-unit-ontology) 修正——**轨迹归单位（Unit），不归事态节点**；A1 / A2 与其余结论继续有效。
- **溯源**：[ADR-0002](/adr/adr-0002-situation-ontology)（2026-10-11）
- **关联待决**：§12-11（U4 事态标签防重叠 / U7 事态嵌套仍待裁；U1/U2/U3/U5/U6 已闭合）

## 4. 单位本体 ✅

**现行口径**：**单位（Unit）**是地图上**独立运动**的主体，`Track.owner` 改为 `Track.unit`；事态是**归纳容器 / group**，**引用**运动而非拥有。「相遇却未写事件」只**提示**不报错（留白是创作手段）。

- **溯源**：[ADR-0004](/adr/adr-0004-unit-ontology)（2026-10-11）——本文同时修正 ADR-0002 Q4 与基线 §1.6 原口径。
- **关联待决**：§12-11（U4 / U7）

## 5. 相遇检测与「未写事件」提示 ✅

**现行口径**：相遇是**派生数据**（不建实体）；判定 = 时间重叠 + 空间贴近 + 未被覆盖**三道门**；候选日走 waypoint 锚点（**不逐日枚举**，跨千年史实会卡死）；指纹**不含日期** → 提示可认领、可回访定位；提示与错误在**类型层面**分开（`DiagLevel::{Hint, Error}`）。

- **溯源**：[ADR-0005](/adr/adr-0005-encounter-hint)（2026-10-11）

## 6. 历法（Calendar 双历制）✅

**现行口径**：`Calendar` 加判别字段 `kind`（缺省 `simple`，**完全向后兼容**）；`gregorian` 走 proleptic 公历（真实月长 + 闰年），**天数计数器仍是唯一真相**（P7 不动）；用 Hinnant 纯整数算法换算，**不引入 chrono**。

- **溯源**：[ADR-0006](/adr/adr-0006-gregorian-calendar)（2026-01）
- **现状追记**：历法支持真公历；`Event.narrative` + `Narration` 独立实体已收敛（§12-13）。

## 7. 生物群系标记 ⚠️ 含被修正子句

**现行口径**：`Place` 扩展 `biome` / `envelope` 两字段（**不新建实体集合**）；边界 = 生成快照存 `place_geometry`（重圈重生成，不逐帧派生）；「识别或生成」自动二选一（占包络 ≥8% 且 ≥4 格 → 识别，不写格子；否则写群系 + 环提取）；湖泊走 D14 语义派（水面 = 包络中位海拔，不挖碗）。

- **⚠️ 被修正**：其 **D5（群系→主题 feature 词汇的过渡桥）** 已被 [ADR-0032](/adr/adr-0032-place-vocab-unify) **取代**——词汇统一收口；其余决定不动摇。
- **溯源**：[ADR-0029](/adr/adr-0029-biome-marker)（2026-10-18）

## 8. Place 地理事实词汇统一 ✅

**现行口径**：`biome`（**冻结群系表**）是唯一地理事实词汇。`feature` 降级为 serde **只读脚手架**，加载时按 `place_feature_to_biome` 表迁移（同值逐位不变，其余升级真实群系）+ 诊断提示；写回只写 biome；迁移窗口一个版本周期。主题规则键改**群系族**（`BiomeFamily` 落 `wv-terrain::biome`，21 编号 → 8 族全覆盖）。round-trip 契约按「迁移收敛性」修订。

- **溯源**：[ADR-0032](/adr/adr-0032-place-vocab-unify)（2026-10-18）——本文取代 ADR-0029 D5。
- **配套**：[ADR-0023](/adr/adr-0023-theme-pack-injection-contract)（主题包兼容）· [ADR-0031](/adr/adr-0031-terrain-decor)（同以冻结群系表为数据源）

## 9. 自然区域模型 ✅

**现行口径**（ADR-0045 五项决定 + 四则追记）：
- **D1** 自动划分按 **biome 连通区**（复用 `isoline::kind_segments` + `assemble_chains`；每格恰属一区，共边由构造保证）
- **D2** 覆盖物归 **region**（`Region.overlay` 开放字符串键，词表归 `ThemePack`）——与 ADR-0039 的「cell 派生变体不持久化」**并存不冲突**
- **D3** 扩展现有 `Region` 实体（`kind: Area|Nature` / `overlay` / `parent` / `holes`，全 serde default → 旧 YAML 零迁移；**不新开** `NatureRegion`）
- **D4** 自动分区**不落盘**，命名即物化
- **D5** 合并 = 几何并集（外环 + 洞），旧成员变子块
- **追记一**：覆盖物键在当前主题无规则 → **白底红斜线缺失标记**（接管底色与装饰，**不回退**）；族规则缺失仍回退（两者有意不统一）
- **追记二**：间距倍率语义修正（`spacing_scale > 1` = 更疏）
- **追记三 / 四**：作者入口已接（区域面板：命名物化 / 覆盖物键 / 重生成边界 / 缺规则红提示）；两套原点换算链（`raster_to_map = map_origin - raster_origin`）；区域可视化三件套（轮廓 / 底色覆盖 / 候选区预览）+ 作者小河交互 + 分池分样式

- **溯源**：[ADR-0045](/adr/adr-0045-nature-region-model)（2026-10-20，用户逐项拍板）
- **关联待决**：§12-47（格子要不要有 span）

## 10. 亚格线状作者数据（手绘小河）✅

**现行口径**：`Place` 加 `line: Option<Vec<f32>>`（图内局部 wu 内联折线，**不新开 `rivers/` 集合**、**不给 `GeometryData` 加线变体**）；锚点 `x/y` 缺省取折线中点；亚格**不占格不写 biome**，与一格宽的 `Intent::River` 并存；渲染走**双供给**（派生 `rivers_seed` ∪ 作者）；河岸覆盖物归 ADR-0045 的 region 声明；三个编辑命令照街道形状；v1 线不带 span。

- **溯源**：[ADR-0046](/adr/adr-0046-sub-cell-author-line-data)（2026-10-20）——补 ADR-0037 D5 作者侧，**不取代**派生河流。

## 11. 交通工具「有效日速度」单位 ✅

**现行口径**：`Vehicle::effective_daily_speed` = `speed × mult × hours`（**km/日**）；`segment_days` 是唯一换算入口，schema 零迁移。既有行程天数结论相对旧值缩小约 24 倍（原值为单位错误）。唯一速度换算入口不可在别处重复实现（§4.2）。

- **溯源**：[ADR-0024](/adr/adr-0024-vehicle-daily-speed-unit)（2026-10-17，执行期裁决；用户复核可推翻）
- **关联待决**：§12-20（地面尺度表示）· §12-39 第二组（世界图尺度只读提示）

## 12. 存储纪律（AGENTS §4 硬约束）✅

**现行口径**（非 ADR，但属现行铁律，来自 `AGENTS.md` §4）：
- **历史即区间**：随时间变的量必须区间化，**禁止**「当前值」字段
- **距离只存参数**：禁止任何「行程时间结论」字段；唯一入口 `Vehicle::effective_daily_speed` / `DistanceGraph::segment_days`
- **只存引用不重复存储**：父子 / 参与者 / 所在地点一律 `Id` 引用（P4）
- **ID 是可读字符串**：字符集 `[A-Za-z0-9_]`，禁自增整数 / UUID（`OvertureId` 为单点例外）
- **每实体一文件**（P2 + Git 友好）

> **作者操作层例外（§7.5 V2）**：历史**可重写**——作者可以改已发生事件的参与者 / 时间 / 地点，工具不阻止、不追问。P6 约束的是**存储结构**，不是**作者的手**。

- **溯源**：`AGENTS.md` §4 / §7.5（[规约投影](/specs/agents)）· README 七原则 P2–P7