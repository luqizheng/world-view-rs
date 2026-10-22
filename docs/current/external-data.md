# 外部数据 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。

## 1. Overture 真实地理底图（只存引用）✅

**现行口径**：引入 Overture Maps 作为**外部地理资源**（非引入 OSM PBF 服务端转换），但**只存引用**：
- 领域模型 `Place` / `Edge` 只存 **`OvertureId`（GERS 字符串）引用，不落几何**
- 新增**平行 crate `wv-overture`**（与 `wv-io` 同级、不依赖 Bevy）作外部资源访问入口
- `Map.geographic_source: Option<GeographicSourceV1>` 元数据字段（主题 / bbox / 路径）走 `RasterRef` 风格声明，**不开 P3 例外口子**
- **不加** `geographic_kind: Real|Fictional` 字段——真假由 `Option` 的 `None`/`Some` 派生
- 字段命名 = `overture_id`；**`OvertureId` 单点豁免 §4.4 UUID 字符集约束**（GERS 实测为 hex UUID，独立 newtype；领域其余 ID 仍受约束）
- 数据流：Parquet 落 `data/` 是「**资源**」不是「数据源同步」——**一次性** Haversine → `distance_km`，落进领域后**不再保留坐标**；`class` 一次性转写 `terrain`；`overture_id` 保留作引用；**不回流写 Overture**

- **溯源**：[ADR-0007](/adr/adr-0007-overture-real-geography)（2026-10-13）
- **关联待决**：§12-15/16/17 已关闭；§12-18 的 **SPI 多源扩展仍待裁**（第一版仅 Overture 一源）

## 2. 街道矢量层（双供给 + 批量渲染）✅

**现行口径**：街道**不与** `distance.edges`（城市间行程连线）合并，是**独立的地物图层**；一个供给接口**两个实现**（虚构 = 作者手绘落 YAML 图层文件；真实 = Overture 只存引用）；渲染从「一段 2 实体」改为**批量 / 实例化 + `class` LOD + 视口裁剪**；**几何不进领域 YAML**；`wv-overture` 为平行叶子 crate。

- **溯源**：[ADR-0010](/adr/adr-0010-street-vector-layer)（2026-10-13）
- **配套**：[ADR-0015](/adr/adr-0015-city-submap-time-versioning)（`StreetSegment.time_span`）· [ADR-0047](/adr/adr-0047-uniform-presentation-rank)（街道 `presentation_rank_from_lod`）
- **关联待决**：§12-21（街道几何存放与规模阈值——单图层文件容纳段数上限、分片约定）· §12-34（建筑数据来源与高度夸张系数）

---

## 本域在原则上的边界（重要）

| 约束 | 内容 |
|---|---|
| **P2（目录即工程）** | Parquet 落工程目录 `data/`，不写注册表 / AppData |
| **P3（YAML 为主格式）** | 外部 Parquet 仅作**资源**，**不开**二进制例外——地形栅格仍是 P3 例外的**唯一受益者**（`OvertureId` 是字符集例外的单点豁免，性质不同） |
| **P4（只存引用）** | 跨地图 / 外部实体一律存 `Id`，不复制几何 |
| **§4.2（距离只存参数）** | Overture 几何一次性转 `distance_km` 参数，不存行程结论 |

- **溯源**：`AGENTS.md` §4.2 / §4.3 / §4.4 / §5（[规约投影](/specs/agents)）· README §3 原则二 / 三 / 四