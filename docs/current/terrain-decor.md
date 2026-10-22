# 地形与装饰 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。

## 1. 地形装饰（程序化符号覆盖层）⚠️ 含被取代子句

**现行口径**：六类群系（森林 / 平原 / 丘陵 / 山地 / 雪原 / 沼泽）按**活动栅格 biome** 确定性采样演绎成程序化符号（`splitmix64` 抖动，**无 rand**）；LOD 三档（Region 46px / Edit 26px 步距）；渲染六桶合批 + `Assets::get_mut` **原地改写**；雪帽跨桶路由；`PassId::TerrainDecor` 接 MODE_TABLE + terrain 级联；符号颜色 v1 常量、主题化随 ADR-0023-M3。

- **⚠️ 被取代子句**：**D2「Nation 档不画装饰」** 已被 [ADR-0036](/adr/adr-0036-nation-decor-and-relief) 取代——现行是 **Nation 档也画装饰带**（96px 步距 / 符号 1.45×；后被 ADR-0042 调整为 72px / 1.6×）。D4 合批纪律继续有效。
- **溯源**：[ADR-0031](/adr/adr-0031-terrain-decor)（2026-10-18）→ 承接 [ADR-0036](/adr/adr-0036-nation-decor-and-relief) → [ADR-0042](/adr/adr-0042-hand-drawn-nation-style)（步距尺寸调整）

## 2. 全局档地貌呈现 ✅

**现行口径**：新增 `DecorBand::Nation`（**三档都画**；步距 96px → 经 ADR-0042 调整为 **72px** / 符号 1.45× → **1.6×**）；栅格烘焙加**坡度浮雕**（邻格高差 → 明度乘子，光源左上，clamp 0.84–1.16，**只陆格** / VectorFlat + RealTerrain）；`VEIL_MAX_ALPHA` 0.35 → **0.18**（纸面雾霭降档）；新增 `DecorKind::Desert`（沙漠 / 戈壁 / 盐滩 → 沙丘脊，第 7 桶）。数据零迁移。

- **溯源**：[ADR-0036](/adr/adr-0036-nation-decor-and-relief)（2026-10-19）——本文取代 ADR-0031 D2，后被 ADR-0042 微调步距与尺寸
- **配套**：[ADR-0035](/adr/adr-0035-jrpg-saturated-daylight-palette)（色板）· [ADR-0016](/adr/adr-0016-jrpg-antique-map-default-theme)（雾）

## 3. 海洋表现 ✅

**现行口径**：**离岸距离场** `wv-terrain::shore`——每格到最近陆格的**切比雪夫格距**（8 连通 BFS，截断在 `max_dist`），为「近岸浅滩带 / 浪花带」提供几何事实（水深 ≠ 离岸远近）。归属 = **数据层**（地形派生事实，与主题无关）；主题只拿配方 `SeaRecipe`。**S2（近岸浅滩带）已接并实机验收**；S3（白色浪花带）**未做**、方向预登记。

- **配套收口**：`wv-theme::sea::SeaRecipe`（海的数值配方全进主题层，渲染 API 收敛为单一 `&ThemePack`）+ 水面纸纹 + 离岸墨线。
- **溯源**：[ADR-0048](/adr/adr-0048-ocean-shore-presentation)（2026-10-21）
- **关联待决**：§12-48（已于 2026-10-21 追记关闭）

## 4. Topographic 视图模式 ✅

**现行口径**：新增 `ViewMode::Topographic`——水 / 陆二分 + height 3 阶着色（地形分析视图）。本 ADR **只锁**「渲染 viewmode 增加」这件事；配套的 CellInspector 工具与 2.5D vertex Z 分别独立成 ADR（0039 / 0040 / 0041 三件并行落地、分别 PR）。

- **溯源**：[ADR-0039](/adr/adr-0039-topographic-viewmode)（2026-10-19，`ADR-0039-topographic-viewmode.md`）

## 5. 2.5D vertex Z 挤压 ✅

**现行口径**：`terrain_2_5d_enabled` 落地——**仅 Topographic mode 启用**。

- **溯源**：[ADR-0041](/adr/adr-0041-topographic-2-5d-vertex-z)（2026-10-19）

## 6. 🔶 主题演绎规则（提议态）

**提议内容**（**未获决策者确认，不得据此实现代码**）：
- 新增 `DecorKind::OceanWaves`（浅海带白弧散点，密度随深度衰减，深海不画）；森林按 **y 轴气候变体**（<0.30 针叶排 / 温带阔叶不动 / ≥0.72 绿洲稀树，仅 Region+Edit）；山装饰 ≥3400m 雪岩双色（帽顶白雪 + 帽缘 `#C9B98A` 露岩，复用 Snow 桶加第二层图元）。规则先落 Rust 常量表（纯函数），签名稳定后随 ADR-0023 M3 迁 ThemePack YAML。
- **「关闭覆盖物显 biome 底色」** 确认现状收口；数据持久化已满足（terrain.bin），渲染结果不持久化（P7 派生）。
- **溯源**：[ADR-0039](/adr/adr-0039-theme-derivation-rules)（2026-10-20，`ADR-0039-theme-derivation-rules.md`，状态 = 提议）
- **⚠️ 索引表滞后说明**：`docs/adr/README.md` 索引表将该条记为「已接受」，与其**文件头部「提议」冲突**——按纪律（索引表与 ADR 头部不一致时以头部为准），本站按**提议**处理。

---

## 本域相关待决

| 编号 | 事项 |
|---|---|
| §12-29 | 地形边界渲染与草图式生成参数族（M0 实现对账） |
| §12-14 | 地形栅格瓦片化（(b) 已收敛为矢量平涂；(a)(c) 仍待裁） |
| §12-47 | 格子要不要有 span（时间轴上的地形）——当前按「栅格无时间轴」实现 |