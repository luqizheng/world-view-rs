# ADR-0022：城市子图的空间形态——轮廓、平移与外部导入

**状态**：已接受（Accepted，2026-10-16）
**日期**：2026-10-16
**决策者**：用户（2026-10-16「地图规模」讨论；D1–D5 已逐条与用户拍板；D2 geo_anchor 在最后一轮被用户裁定**冗余删除**——`geographic_source.bbox` 已承担所有 lat/lon ↔ x/y 投影）
**关联**：P1 / P2 / P3 / P4 / P6、AGENTS §3 / §4.2 / §4.3 / §7.5 V1-V2、ADR-0007（Overture）/ [ADR-0009](ADR-0009-graded-tile-zoom-to-street.md)（多尺度 + `Extent.origin`）/ 0010（街道）/ 0013（图层帧）/ 0015（时间版本化）/ 0020（`unit_per_km`）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

ADR-0009 / 0010 / 0015 / 0017 已建起多尺度城市子图的骨架：

- ADR-0009 子图通过 `Extent.origin` 嵌进父图坐标系（§6.2 派生规则）
- ADR-0010 街道图层落在城市子图 YAML（手绘 / Overture 双供给）
- ADR-0015 城市图级 / 段级 `time_span` 处理时间维度
- ADR-0017 标签层处理实体在地图上的文本投影
- `MapNode.unit_per_km` 已存在但被注释为「不进入坐标链、仅供测量显示」（`map/mod.rs` L85-97）

但 **五个子问题** 在「城市子图的**空间**形态」上还没有显式答案：

1. 城市在世界图上的**轮廓**——当前只有 `IconSpec`（一个点），没有 polygon
2. 城市在父图上**平移**时各字段怎么联动
3. **外部导入**（Overture lat/lon）如何落到 parametric 子图
4. **每图尺度**（1 格 = 多少 km）怎么声明
5. **跨级尺度**（城市内部 wu vs 世界图 wu）是否联动

证据链：

1. **城市轮廓缺位**：`crates/wv-app/examples/gen_layer_showcase.rs`（2026-10-16）已生成 18 个演示点，但所有「城市」都只是 IconSpec 钉子，没有 polygon 表达。
2. **跨级尺度无诊断**：`unit_per_km` 在仓库 `crates/wv-core/src/map/mod.rs` L85-97 注释「不进入坐标链」；全仓**没有**「两级间尺度一致性」诊断——作者可能世界 4 km/wu + 城市 50 m/wu + 城市 extent 5 wu（≈ 20 km 见方）+ 城市本地 1024 wu（≈ 50 km 见方）——20 vs 50 不触发任何警告。
3. **外部导入是 Overture 直写不是 lat/lon → x/y 投影**：`wv-overture`（AGENTS §2.1 平行 crate）当前负责 Parquet 切片下载与几何转换；转换基准——「以哪个 anchor 为投影原点」——**没有显式字段**。

### 1.4 关键澄清：`geographic_source` ≠ geo_anchor

`MapNode.geographic_source: Option<GeographicSourceV1>`（ADR-0007）已经存在，**已承担所有 lat/lon ↔ x/y 投影需求**（通过 `bbox` 字段）。原拟新增的 `geo_anchor` 字段与其语义重叠——经用户最后一轮裁定，**geo_anchor 不引入**，所有 lat/lon 表达走既有 `geographic_source`。

> **lat/lon 在本项目的角色**：仅作为「本图对应真实世界哪一片区域」的**外部数据锚**（用于 Overture 导入与状态栏读数显示）。它**不**参与 placement / transform / 跨级派生——placement 由 x/y 表达。

不补齐这一层，「缩放到街道」只能呈现「城市 = 一颗钉子」的快照；作者无法表达「这座城占地约 50 km²、从父图拖到世界另一处同时子图几何不动」这类创作需求。

### 1.1 边界声明（不在本 ADR）

- **本 ADR 不引入 lat/lon 输入 UX**：地理一致性由既有 `geographic_source` 承担；`MapNode` 不携带 lat/lon 字段。
- **MapNode 不携带高度**：altitude 归 `TerrainCellData.height`；placement = 2D，terrain = 2.5D（h + type），building = 3D。三段正交，不互相反向耦合。
- **轮廓 LOD 何时显隐**（zoom 几级开始显示）：属渲染配方（[ADR-0021](ADR-0021-render-recipe-and-treatment.md)）/ LayerFrame（ADR-0013）扩展项，不在本 ADR。
- **街道 L1 / 街区层**：未来城市子图内部细化，独立议题。
- **建筑挤出 / 2.5D**：见 ADR-0016 §6。

---

## 2. 决策（Decision）

五项决定（D1–D5）**正交为主**；D1 / D2 / D4 / D5 可独立接受或否决，D3 是 D1 的副作用落地。

### D1 城市轮廓归属 —— 父图存 polygon

1. `MapNode` 新增字段：
   ```rust
   #[serde(default, skip_serializing_if = "Option::is_none")]
   pub outline_polygon: Option<GeometryData>,
   ```
2. 多边形顶点 = **子图 LOCAL 坐标**（与街道同口径，`Vec<f64>` 扁平 `[x0,y0,x1,y1,…]`，复用 `GeometryData::Polygon`——`map/mod.rs` L521-531）
3. 父图渲染时通过 `coord::resolve_frame_in`（ADR-0009 §6 派生）一次性平移到父图坐标系——**复用单一换算入口**
4. **首写即定**：轮廓由 `AddCityCommand`（D5）一次性填入（用户在父图上画框）；后续子图内部编辑**不**回写轮廓——父图轮廓是声明，非派生
5. `None` = 旧城市按当前 `IconSpec` 退化为点；**不**引入「自动派生长方形轮廓」伪数据（守 P3 + §6.2 待决）

### D2 城市平移 —— `MoveCityCommand` 双字段同步

1. 新增 `MoveCityCommand { city_id, delta }`——`delta` 是父图局部 wu（与 `Point` 同型，零转换）
2. 命令执行后写入：
   - `MapNode.extent.origin += delta`（若 `extent.origin` 是 `Some`；`None` 时由 `resolve_frame_in` 重派生，不在此命令覆盖——per ADR-0009 §6.2）
   - `MapNode.outline_polygon`（若 `Some`）所有顶点 += delta
3. **不动**：
   - 子图 LOCAL 几何（街道、地点、标注、building…）——子图局部坐标系稳定（per ADR-0009 §9「本图 local」）；「位置」是父图的事，「几何」是子图的事
   - 引用该城市的 `Place` / `Track` / `Faction` 实体——它们是独立实体；「Place 在城里」是逻辑声明不是坐标从属；硬联动会让 Place 跟着被拖，体验反而差（V2 §7.5「作者是主人」）
4. 走 `wv-edit::History`，revert 缓存旧值，P6 不破

### D3 河流覆盖语义（建城时一次性）

1. `AddCityCommand` 落地时扫描**当前父图**上的河流几何（Q-A 待裁，可能在 `distance.edges` 或未来独立 river layer）：
   - 与子图 `outline_polygon` 求交（在父图坐标系里做 clipping）
   - 把穿过轮廓的河段**复制**进子图初始数据（写到子图的 river layer 文件）
2. **之后**子图河流独立：父图河流编辑**不**反向传播（守 P4 + §7.5 V2 V1）
3. 是 `AddCityCommand` 落地时的**一次性 IO 同步动作**，非持续同步；河流水复用天然有「父图先存在 → 子图后建」与「子图先建 → 父图后添河流」两种时序，**只处理前者**（后者不补）
4. 与 ADR-0015 §2.4 城市时间版本化的同款精神：「数据进入子图即独立」

### D4 地图尺度声明 + 外部导入（经 `geographic_source`）

1. **沿用** `MapNode.unit_per_km: Option<f32>`——保留为「地图级声明」
2. 来源：
   - 真图：自动由 `geographic_source.bbox × extent` 派生（已有 `MapNode::km_per_wu()`，`map/mod.rs` L436）
   - 虚构图：作者手填；无则状态栏只显示 wu
3. **跨级尺度不联动**：
   - 每图 `unit_per_km` 独立声明
   - 一致性靠作者意图
   - V1 校验提示（`DiagLevel::Hint`，不阻止）：
     ```
     expected_world_extent_w = (city_local_extent_w / city.unit_per_km) / world.unit_per_km
     declared_world_extent_w  = city.extent.width
     ratio = declared / expected
     |ratio - 1.0| > 0.1  → Hint「两级间尺度有 X% 偏差」
     ratio < 0.5 || > 2.0 → Hint「两级尺度明显不匹配」
     ```
4. **外部导入**（Overture 等）：
   - 必须经 `geographic_source.bbox` 投影（`geo_anchor` 字段**不引入**——见 §1.4 关键澄清）
   - `geographic_source = None` 时导入按钮**禁用**，提示「先声明此图的 `geographic_source`（bbox 锚定区域）」
   - 投影默认 **equirectangular**（简单、低纬误差 < 0.1%）；按 `center_lat` 自动切 mercator 留 v0.2（Q3）
   - 导入完成后数据落进本图 `Layer.source`，是 x/y；改 `geographic_source` **不**回追已导入数据

### D5 EditCommand 集合（新增）

| 命令 | 字段 | 行为 |
|---|---|---|
| `AddCityCommand` | `id, parent, extent, outline_polygon, name, base` | 创建城市 `MapNode` + 一个 `IconSpec`（含 `drill_into`）+ 落地 D3 河流复制 |
| `MoveCityCommand` | `city_id, delta` | D2 双字段同步 |
| `SetCityOutlineCommand` | `city_id, new_polygon` | 替换 `outline_polygon`（用户在父图上重画） |

全部走 `wv-edit::History`，**不**经渲染层直接改（AGENTS §3.3 红线）。

### 分期（D1–D5 落地次序，各自独立可 revert）

- **M1（D1）**：数据形态 + 类型 + YAML 兼容（`Option` 缺省 = 零迁移）
- **M2（D2 / D5 命令）**：`AddCityCommand` + `MoveCityCommand` + `SetCityOutlineCommand`；UI 一个画方框工具 + 一个拖拽工具
- **M3（D3 / D4 校验）**：河流复制 + 跨级一致性 V1 诊断

---

## 3. 理由（Rationale）

### D1
- **反向（子图贡献给父图）否决**：城市移动 / 删除后父图残留旧轮廓（生命周期不一致）；P4「父持子 ID」原则不破
- **「占位轮廓」仅运行时**：磁盘不存伪数据（P3 / §6.2 待决——待决问题不允许用「临时这样实现」绕过）

### D2
- **子图几何不动**：子图 LOCAL 坐标系是稳定的（per ADR-0009 §9「本图 local」）；「位置」是父图的事，「几何」是子图的事——边界清晰
- **不动 Place/Track**：实体独立性——Place「在城里」是逻辑声明不是坐标从属；硬联动会让 Place 跟着被拖，体验反而差
- **「仅改父图 position」备选否决**：父子图坐标从此漂移——子图 LOCAL 与父图坐标再无对应关系，跨级派生全废

### D3
- **一次性复制非持续同步**：避免「子图反向耦合父图」的污染（per ADR-0015 §2.4 同款精神）；V2 §7.5「作者是主人」

### D4
- **跨级不联动**：ADR-0009「extent 是同一个单位」是几何口径，不该被解读为 km 口径联动；作者是主人，工具不该替他做尺度决策
- **V1 提示而非阻止**：与 AGENTS §7.5 V1 一致——「让作者看见问题」而非「代替作者做决定」
- **`geographic_source` 承担 lat/lon**：原拟 `geo_anchor` 字段与既有 `geographic_source.bbox` 语义重叠；用户裁定不引入新字段，避免「位置一致性问题」与「lat/lon 输入 UX 边界」

### D5
- **命令而非直接改字段**：P6 不破；可撤销；符合 AGENTS §3.3

---

## 4. 代价（Consequences）

**正**：
1. 城市轮廓有显式数据形态；
2. 平移可撤销，P6 不破；
3. 跨级尺度自由表达；
4. 外部导入链路清晰（Overture → `geographic_source.bbox` → x/y）；
5. 无新增 lat/lon 字段——避免「geo_anchor vs geographic_source」语义重叠与一致性问题；
6. M1 工作量轻（只新增 1 字段）。

**负**：
1. `MapNode` 新增 1 个字段（`outline_polygon`）；
2. 新增 3 个 EditCommand；
3. 河流复制逻辑在 `AddCityCommand` 里实现，工作量中等；
4. V1 跨级一致性诊断需要新 `DiagLevel::Hint` 形态；
5. 「占位轮廓」运行时策略要 UI / 渲染层配合（M1 同步）；
6. 没有 lat/lon 输入 UX——若未来想加「按 lat/lon 落点」需新开 ADR（已锁定「本 ADR 不引入」）。

---

## 5. 被否决的备选（Rejected Alternatives）

### D1 被否决
| 备选 | 否决理由 |
|---|---|
| **子图反向贡献给父图** | 城市移动 / 删除后父图残留旧轮廓；破坏 P4「父持子 ID」 |
| **自动按子图 extent 派生长方形轮廓** | 引入伪数据；非作者意图；违反 P3 / §6.2 |

### D2 被否决
| 备选 | 否决理由 |
|---|---|
| **仅改父图 position，子图不变**（最简）| 父子图坐标从此漂移——子图 LOCAL 与父图坐标再无对应关系，跨级派生全废 |
| **平移带子图几何 transform**（初版措辞）| 误解——子图 LOCAL 稳定，平移只动父图那一侧的字段，详见 §2 D2 |

### D4 被否决
| 备选 | 否决理由 |
|---|---|
| **跨级尺度强联动** | 违反「作者是主人」；多尺度变换数学复杂；表达力损失（艺术夸张、戏剧比例尺做不到） |
| **选填联动（声明「按父缩放 X 倍」）** | X 倍数本身就要算；折中但复杂；v0.1 不需要 |
| **新增 `geo_anchor` 字段承担 lat/lon** | 与 `geographic_source.bbox` 语义重叠；用户裁定冗余删除（最后一轮） |

---

## 6. 待裁（Open Questions）

| # | 问题 | 倾向 | 落点 |
|---|---|---|---|
| **Q1** | 轮廓 LOD 何时显隐（zoom 几级开始显示）| 留 ADR-0021 渲染配方族 / LayerFrame 扩展 | 后续 ADR |
| **Q2** | 默认城市 extent 在 cities 下的预填值 | 倾向 1024² × 0.5° × 0.4° | UI 设计 |
| **Q3** | mercator 自动切换的纬度阈值 | 留 v0.2 | v0.2 |
| **Q4** | 河流数据当前在哪（`distance.edges` 错用 / 独立 layer？）| **已收敛**（2026-10-16 M3 落地时探查）：**河流仅是 `wv-terrain` 的 `Intent::River` 在地形 height 上的模式，**不**是 first-class 几何实体**。因此 D3 「复制河段入子图」**当前为 no-op**：没有几何可以复制。等价事实：「城市建在河上 / 山顶」由地形 height 表达，由 V2 §7.5「作者是主人」豁免（不阻止、不警告）。**未来若引入 first-class river layer，需新开 ADR 把 D3 重启**。 | resolved |
| **Q5** | 轮廓放「层级缩放」还是「图层帧」（ADR-0013 哪个轴）| 倾向图层帧——可独立开关，符合 P5 | M1 渲染层配合 |
| **Q6** | `MoveCityCommand.delta` 单位（wu vs 父图 extent 归一化）| 倾向 wu（与 `Point` 同型，零转换） | D5 |

---

## 7. 落地闸门（若接受）

1. **M1**：新增字段 + YAML round-trip 兼容测试（既有工程零修改）+ `outline_polygon` 序列化测试覆盖 `GeometryData::Polygon` 形态
2. **M2**：3 个 EditCommand + UI 一个画方框工具 + 一个拖拽工具；Q4 河流定位同步收敛
3. **M3**：河流复制 + V1 跨级一致性诊断
4. 三阶段各自独立 commit，各自独立可 revert
5. 全门禁绿（AGENTS §7.1）：
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo fmt --all -- --check`
   - `cargo test -p wv-app --features app`（D1 渲染层配合必跑）
   - `cargo clippy -p wv-app --all-targets --features app -- -D warnings`
   - `bash scripts/smoke-gui.sh`（UI 工具改动必跑）
6. §10 变更检查清单逐条自检：触及 P1/P2/P3/P4/P6；`wv-core` 仍零 Bevy / 零网络依赖；无「当前值」字段；新术语登记 GLOSSARY；待决登记 README §12

---

**相关文档**：[ADR-0007](ADR-0007-overture-real-geography.md)（Overture 平行 crate / `geographic_source`）/ [ADR-0009](ADR-0009-graded-tile-zoom-to-street.md)（多尺度 + `Extent.origin`）/ [ADR-0010](ADR-0010-street-vector-layer.md)（街道双供给）/ [ADR-0013](ADR-0013-layer-frame-binding.md)（图层帧求值点）/ [ADR-0015](ADR-0015-city-submap-time-versioning.md)（§2.4 编辑闭环对照）/ [ADR-0016](ADR-0016-jrpg-antique-map-default-theme.md)（§6 建筑挤出对照）/ [ADR-0020](ADR-0020-wv-render-crate.md)（`unit_per_km` 渲染消费）/ `docs/MAP_EDITOR_BASELINE_v1.md` §2.15（坐标变换）