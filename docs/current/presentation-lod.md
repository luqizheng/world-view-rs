# 呈现与 LOD · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。

## 1. 标签层 ✅

**现行口径**：标签 = 实体在地图上的**派生**（**非作者手摆文本**）；`LayerKind::Label` 进 `LayerFrame` **独立渲染组**；图层文件只存配置**不存文本**；可见性走单一函数 `label_visible_at`（与 `street_visible_at` 同构）；字号阶梯绑档位；AABB 碰撞 v0.1 + **`max_labels = 200`**（优先级 = 文本长度）；**街名标注全档接管**；点击派发选中（`SelectedEntity` = **会话状态，不走 EditCommand**）。

- **溯源**：[ADR-0017](/adr/adr-0017-label-layer)（2026-10-14，用户逐条拍板 Q1–Q5）
- **注**：原编号 ADR-0016（分支起草），合并时因撞号重编号（文件头有追记）
- **关联待决**：§12-36（标签层 v0.1 碰撞算法精度上限）· §12-37（图标主题切换能力）

## 2. 全要素统一呈现级别 ✅

**现行口径**（ADR-0047 五项决定）：
- **D1** `PlaceEdge.rank: Option<u8>`（serde 缺省**零迁移**；`None` 按 `distance_km` 折算：≥100 → 3 / ≥30 → 2 / 否则 1，**冻结契约测试钉死**，显式永远赢）
- **D2** 统一门槛 **`rank_visible_at_tier`**（**Nation ≥3 / Region ≥2 / Edit 全见**）——ADR-0030 词表**扩表**非新表，`poi_visible_at_rank` 降级为别名
- **D3** 街道经 `presentation_rank_from_lod`（0–2 → 3 / 3–5 → 2 / 6–7 → 1）**委托统一门槛**，8 lod × 3 档与旧阈值**逐位等价**（行为零变化）
- **D4** 世界道路段列按 rank 降序 + 三档前缀切割位 + 分档指纹（缩放零重算，跨档 O(1) 切片）
- **D5** 河流按折线长 ÷ 图幅宽折算（≥0.40 → 3 / ≥0.15 → 2 / 否则 1），派生河与作者河（[ADR-0046](/adr/adr-0046-sub-cell-author-line-data) 双供给）**同折算同门槛**

- **溯源**：[ADR-0047](/adr/adr-0047-uniform-presentation-rank)（2026-10-20，用户裁定「按照你的推荐把」）
- **注**：顺手修三处带病测试（cells 未关晕开 / decor 缺 RegionOverlayIndex / world_projection 夹具丢 rank 字段）

## 3. 🔶 POI 呈现级别与密度预算（提议态）

**提议内容**（**未获决策者确认，不得据此实现代码**）：
- `Place` 扩展可选 `rank`（0 网络节点 / 1 普通 / 2 市镇 / 3 国家；`None` 按旧两档折算 w/h → 3 否则 1，**零迁移**）
- 呈现门槛 = 档位 × 最低级别硬门（Nation ≥3 / Region ≥2 / Edit 全见），删非城市淡出带
- 密度预算 **`POI_MAX_DOTS = 400`**，按 `(rank, is_city, 名字长)` 裁剪（纯渲染投影）
- POI 按材质合批 ≤4 mesh + 资产原地复用；道路 = 段列缓存 + 线宽量化 4% + mesh 复用；标签候选走锚定桶 + 视口 + 级别（O(n²) 输入有界化）

- **溯源**：[ADR-0030](/adr/adr-0030-poi-rank-and-density-budget)（2026-10-18，状态 = 提议；实现按本 ADR 进行，用户可否决后重开）
- **⚠️ 与 ADR-0047 的关系**：ADR-0047 **扩表**接入（非取代）——`Place::rank` 与 `PlaceEdge::rank` 共用 `rank_visible_at_tier` 一张真值表。ADR-0047 已接受，其统一门槛属**现行**；ADR-0030 的 POI 密度预算部分仍属**提议**。

## 4. 各档呈现的现行门槛速查

| 档位（LodTier） | 最低可见级别 | 说明 |
|---|---|---|
| Nation（全局） | rank ≥ 3 | 国家级 / 城际干线 |
| Region（区域） | rank ≥ 2 | 市镇级 / 城镇间 |
| Edit（编辑） | 全部可见 | — |

> **唯一实现入口**：`rank_visible_at_tier`（ADR-0047 D2）。任何档位门槛的重复实现在纪律上属违规（§4.2 精神：单一换算入口）。