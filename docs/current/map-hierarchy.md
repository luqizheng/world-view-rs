# 地图层级与导航 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。

## 1. 跨地图引用 ✅

**现行口径**：跨地图事态**只靠 `Place` 的 ID 层级串联，绝不复制子图内容**（硬约束 A3）。地图父子关系只存子图 `Id`（P4）。

- **溯源**：[ADR-0003](/adr/adr-0003-cross-map-reference)（2026-10-11）

## 2. 分级瓦片缩放 ⚠️ 含被修正子句

**现行口径**：「缩放到街道」用**离散级别 + 级间平滑插值**，不用无极连续缩放；级别由**图在层级树中的深度**派生（不扩 `MapLevel` 枚举）；`Extent` 加 `origin`（子图在父图中的落点，缺省 `(0,0)`）；**每图自带 `raster` 与自带原点**以规避 `f32` 大坐标精度崩溃；坐标变换集中到 `wv_core::coord` 单一实现点。

- **⚠️ 被修正**：§2 决策⑤「**往里缩放换级**」已被 [ADR-0038](/adr/adr-0038-icon-click-drill-in) 修正——进入子图改为**双击钻入图标**（分级区间 / 坐标嵌套 / 外界回退**不变**）。
- **溯源**：[ADR-0009](/adr/adr-0009-graded-tile-zoom-to-street)（2026-10-11）
- **关联待决**：§12-19（多级栅格驻留策略）· §12-22（子图落点多入口歧义）· §12-24（继承底图精度上限）· §12-25（换级过渡已收敛：旧底图整幅淡出 0.4s）· §12-26（轨迹 / 事件坐标所属帧）

## 3. 进入子图的唯一手势 ✅

**现行口径**：进入子图唯一手势 = **双击钻入图标**。缩放撞**级内界**后继续往里滚只被级区间夹住停住，**不再触发换级**；撞**外界**继续往外滚回父级保留（不对称是刻意的：缩小 = 看周边，进入 = 明确点选）；双击维持不改单击；**级内最大放大 `LEVEL_ZOOM_SPAN` 80 → 32**。数据模型零改动。

- **溯源**：[ADR-0038](/adr/adr-0038-icon-click-drill-in)（2026-10-19）——本文修正 ADR-0009 §2 决策⑤的「往里」半边。

## 4. 上北下南（坐标系约定）✅

**现行口径**：世界坐标 **`+Y` 向上**是唯一约定。导入投影改 `v=(lat-south)/range`（去 `1-`）；DEM 中间格式 row 0 = **南**；UV / 锚点语义改为**西南原点、y 向上**；`uv_to_lat_lon` 改 `south + v*range`；手绘地图**零改动**。

- **代价（已知）**：广东工程需全量重导入；`xinhai-1911` 名义经纬度读数翻转。
- **溯源**：[ADR-0034](/adr/adr-0034-north-up-coordinate-convention)（2026-10-18，用户拍板）

## 5. 城市子图的空间形态 ✅

**现行口径**（ADR-0022 五项决定）：
- **D1** 城市轮廓 `MapNode.outline_polygon: Option<GeometryData>` 落子图 LOCAL 坐标；父图经 `resolve_frame_in` 平移渲染；缺省运行时退化为点、**不写盘**
- **D2** `MoveCityCommand` 同步 `origin` + `outline` 双字段；子图 LOCAL 与 Place/Track 不动
- **D3** `AddCityCommand` 一次性复制父图河段入子图后独立
- **D4** 沿用 `unit_per_km` 跨级不联动 + V1 校验提示；外部导入经既有 `geographic_source.bbox` 投影默认 equirectangular；**不引入** `geo_anchor`
- **D5** 新增 3 个 EditCommand；M1/M2/M3 分期各自可 revert

- **溯源**：[ADR-0022](/adr/adr-0022-city-submap-spatial-form)（2026-10-16）
- **关联待决**：§12-20（地面尺度表示）

## 6. 城市子图的时间版本化 ✅

**现行口径**：**双层 `TimeSpan`**——`MapNode.time_span`（图级时代窗口）+ `StreetSegment.time_span`（段级存在窗口）；二者皆 `Option`，**缺省零迁移**；可见性求值统一走 `TimelineQuery::at`（P7 唯一入口延伸）；不改 `class` / `points` / `name`；编辑经 EditCommand。

- **溯源**：[ADR-0015](/adr/adr-0015-city-submap-time-versioning)（2026-10-14）
- **关联待决**：§12-31（时代窗口外 UI 行为：灰显 vs 隐藏）· §12-32（跨时代城市子图命名规约）

## 7. 世界模板与完全重生成 ✅

**现行口径**：新建工程对话框加「世界模板」下拉——`Ocean`（既有默认）/ `ColdNorth`（双大陆 + 单纬度温度轴，算法落 `wv_terrain::climate`）。`WorldMeta.template: Option<WorldTemplate{kind, seed, rivers_seed}>` 随 `world.yaml` 落盘（`None` = 旧工程，零迁移）；**生成模板不写 `MapNode.base`**（重置会毁模板）。「完全重生成」走 `RegenWorldCommand`（命令面板入口 + `terrain.bin.bak.<ts>` 留退路）。

- **溯源**：[ADR-0037](/adr/adr-0037-world-template-and-regen)（2026-10-19）
- **配套**：作者小河补作者侧见 [ADR-0046](/adr/adr-0046-sub-cell-author-line-data)（不取代派生河流）