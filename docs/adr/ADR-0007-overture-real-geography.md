# ADR-0007：以 Overture Maps 作为真实地理底图（只存引用，不落几何）

- **状态**：已接受（Accepted）
- **日期**：2026-10-13
- **决策者**：主理人 范可控（项目裁定）
- **关联**：`README.md` §3 原则二 / 原则三 / 原则四、`AGENTS.md` §4.2 / §4.3 / §4.4 / §5、ADR-0001（势力层 = `Vec<GeometrySpan>`）、README §12 待决 §15–§18（待裁）

---

## 背景与问题

项目要做的「西雅图地图」是**真实地理位置**上的设定作品（"Space Needle 在 1911 是否已建好"等时空校验会成为这类作品的常问）。这要求编辑器底层有一份**权威的真实地理底图**，而不是手画的多边形。

候选数据源是 [Overture Maps Foundation](https://github.com/OvertureMaps)（Linux Foundation 项目，开放许可、按主题发布为云原生 GeoParquet、提供 GERS 全球 ID）。该组织官方提供 6 大主题：`addresses` / `buildings` / `divisions` / `places` / `transportation` / `base`，西雅图区域可按 bbox 切片下载，体积小到几 MB。

问题不是「能不能拿到」（能，免费、合法），而是「**拿到后怎么落到我们的数据模型里**」——直接决定作者数据是否能保持「目录即工程、可手写、可 Git 版本管理」。

---

## 决策

**采用 Overture Maps 作为真实地理底图的外部数据源，但只引入「**引用层**」——`wv-core` / `wv-io` / 持久模型层**不直接消费 GeoParquet 几何**。具体规则：**

1. **`Place` 与 `Edge` 不承载 Overture 几何。** 真实世界要素以 `OvertureId`（GERS 字符串）与**可选**的 `bbox` 引用存在；几何走渲染层（`wv-app`）按需向本地 Parquet 切片查询。
2. **`Map` 增加 `geographic_source: Option<GeographicSourceV1>` 元数据**，声明本地图**是否**引用真实地理、引用哪个 Overture 主题、用哪个 bbox。**仅作声明，不加载几何**。真假地图由 `Option` 的 `None` vs `Some` 派生，**不另设 `geographic_kind` 字段**（避免枚举扩张，参见 §「硬约束」同步注脚）。
3. **新增独立 crate `wv-overture`**（`wv-io` 的同级、平行数据源模块）：
   - **不依赖 Bevy**（与 `wv-io` 同样约束）；
   - 负责「按 `Map.geographic_source` + bbox 下载/校验本地 Parquet 切片」；
   - 把 Overture segment 转成**渲染层**用的几何资源（`wgpu Mesh` 顶点缓冲、`Vec<Vec2>` 等）；
   - **不写回领域模型**——回到领域模型时只有引用、没有几何。
5. **可执行入口收口在 `wv-app` 的 CLI**：新增 `wv-overture fetch <map-id>` `dv-publish` 等子命令，前者下载 Parquet 切片到工程目录，后者把领域 `Place` 反向查询出坐标（供校验面板的「悬空检查」使用）。
6. **Overture 数据**不作为**向量源格式**（P3 例外仍只适用地形栅格，**不**为 Overture 开口子）；地图工程的 `.yaml` 仍只存引用与元数据，`.parquet` 作为**资源**（raster-ref 风格）放在 `maps/<id>/data/` 下。
7. **本 ADR 决定的是「引入」与「不落几何」，具体字段与 schema 待 PR 中定。**

---

## 硬约束

| # | 约束（逐字引用） | 如何约束本决策 |
|---|---|---|
| **P2** | **原则二 · 目录即工程**：一个文件夹 = 一个完整世界工程。可拷贝、可压缩、可 Git 版本管理。 | Overture Parquet 属于「工程的外部资源」，进 `data/` 当事资产；领域 YAML 仍只依赖**双向清单**与 `Id` 引用，**不复制几何** |
| **P3** | **原则三 · YAML 为主格式**：唯一例外为地形栅格 | Overture 不开二进制例外——它已是别人发布的二进制（Parquet）资源，**不是**我们的领域几何，落到本项目时还是「**不存**」（仅声明与引用） |
| **P4** | **原则四 · 多地图层级**：层级之间通过 ID 引用，不重复存储 | `OvertureId` 是引用；**不复制**被引用对象的 geometry / name / class |
| **§4.2** | **距离只存参数**：不存「行程时间结论」 | Overture `transportation` segment 自带 `length`（米），转 `PlaceEdge { distance_km, terrain }` 后由 `DistanceGraph::segment_days` 计算耗时 |
| **§4.3** | **只存引用，不重复存储** | 参见上文（1） |
| **§5** | **二进制分块纪律**：明确三条边界（超大足够多 / 索引 YAML 声明 / 元数据只存引用） | Overture Parquet **不是**我们的二进制——是别人的资源。这条仍适用：禁止内联，资源路径由 `Map.geographic_source.data` 字段声明 |

---

## 理由

**（1）P3 + §5 的反面印证。**

§5 要求二进制「**只能是大网格数组**、**索引 YAML 显式声明**、**元数据只存引用**」。本 ADR 严格遵守：领域 YAML 不存几何，不为 Overture 开 P3 例外口子——它是我们**引用的外部资源**，不是我们要的领域形态。

**（2）P2「可拷贝、可压缩」的体现。**

如果我们把几何落进 YAML，世界工程会从「几 MB」变成「几十 GB」。Overture 西雅图 `transportation` 完整切片就 100MB+ 量级，**不能**进 Git、也不能进领域 YAML。**只存引用**是唯一同时满足 P2 与「现实数据合理性的工具」两个需求的路径。

**（3）领域渲染与渲染资源的边界。**

真实地理几何只在**渲染**阶段需要（点选、贴线、查路）。拆 `x = Place { overture_id, ... }` 与渲染资源（`Mesh` 顶点列）是架构上必须隔开的；这与本项目「领域模型不依赖 Bevy」的纪律一致。

**（4）§4.2「距离只存参数」与 Overture segment 互补。**

Overture `transportation` segment **不带 `length` 字段**（2026-10-13 实测西雅图 bbox 下载 `t segment` 共 64k POI + 155k segments，segment 顶层 properties 只有 `names` / `subtype` / `class` / `connectors` / `speed_limits` / `sources` / `version`），长度需从 `geometry: LineString` 用 Haversine 算出。转换映射修正为：

| Overture `transportation` 字段 | 本项目 `PlaceEdge` 字段 | 说明 |
|---|---|---|
| `geometry: LineString`（端点经纬度列） | `distance_km = haversine_sum_m(geometry) / 1000` | ⚠️ 需 `wv-overture` 内置 `segment_length_m()` 工具函数（不仅是文档描述，要做成 API） |
| `subtype` | `terrain` 过滤门（**仅保留 `road`**） | subtype ∈ {`road` / `rail` / `water` / `footway`}；西雅图样本 154k 行中 99% 是 `road` subtype，但内含 77604 行 `class: footway`（**OSM-style 人行道也被归入 road subtype**）。第一版默认过滤 `road + class != footway`，footway 走「步行专用边」分支（待裁 F） |
| `class` | `terrain`（走 ADR-0001 §2.5 已有的 terrain 字符串体系，不动原表） | 实际取值：`motorway` / `trunk` / `primary` / `secondary` / `tertiary` / `residential` / `service` / `path` / `cycleway` / `track` / `unknown` / `unclassified`（共 12+ 种） |
| `speed_limits[].max_speed` + `unit` | **不进 `PlaceEdge`**（speed 是 Vehicle 的属性，不在 field） | 仅供 `wv-overture` 渲染层标注或 `wv-core.Vehicle` 模板预填充；**禁**直接进领域模型 |

行程耗时**只走**既有 `Vehicle::effective_daily_speed` × `DistanceGraph::segment_days` 入口，**不引入第二个公式**。`Overture` 字段集与 `PlaceEdge` 字段集**不重叠 —— `wv-overture` 是单向注入管道（Overture → 领域字段），反方向回灌会绕过 §4.2 落地判据。

**实测校验（2026-10-13）**：

```text
# 一条 residential segment，geometry 含 3 段 4 点
# 端点（西雅图 downtown）：
# (-122.34930, 47.62050) → (-122.34910, 47.62010)
#                        → (-122.34880, 47.61970)
#                        → (-122.34850, 47.61940)
Haversine 计算长度 ≈ 137.0 m
```

**CR-1 · 数值校准（2026-10-13）**：原文「440.8 m」是手算近似值，与上述 4 个真实端点的 Haversine 累加结果（**~137.0 m**）不一致。按 AGENTS.md §6.3「文档追写不改机制」保留原文不动，**追加**本校正说明：`wv-overture::geometry::tests::regression_segment_length_matches_seattle_residential_sample` 用同一组端点列锁定该 ~137 m 量级，作为 `segment_length_m()` 第一版实现的回归测试用例。

**（5）所不存决定了「作者可写」。**

只存 `OvertureId` 后，作者仍然手写一份西雅图工程：手动加 `Space Needle` 这个 `Place`，填一个 GERS ID、起个名字、记个加入时间。当 **手写领域 + 只读资源**是路径时，作者仍然是「在做事」，而不是「在点几何」。

---

## 后果

### 正面

- **世界工程保持 Git 友好**（领域 YAML 只几个 MB，几何资源可由 `wv-overture fetch` 按需重新下载）；
- **真实地理位置的时空校验可达成**（1911 Space Needle 不存在、GERS ID 可查 `buildings.start_date`—— Overture building schema 提供 `start_date` / `class`）；
- **与本项目已有架构保持一致**（领域层 = 零几何，Bevy 只在 `wv-app`、现加 `wv-overture` 平级，仍不渗透 `wv-core`）；
- **不破坏 §7.5 V2「作者可改历史」**——作者改的是「什么时候登了」的绘梨时间，不动「几何在哪里」，后者始终是事实。

### 负面 / 代价

- **首次下载需要联网**：违反「离线运行」原则吗？**不违反**——下载是「项目初始化」，运行时仍零网络依赖（Parquet 可装入 `.gitignore` 或 `data/`）；
- **作者需理解 `OvertureId`（GERS 是 UUID，不是可读字符串）**：实测 Space Needle 的 GERS ID 是 `2f099609-47e9-4c3e-bfeb-072ccd609a03` —— **8-4-4-4-12 形式的 hex），**直接违反 `AGENTS.md` §4.4「ID 字符集限制为 `[A-Za-z0-9_]`」规则**。本 ADR 决定**为 `OvertureId` 类型开一处明确例外**：
- **Parquet 版本依赖 Overture 发布节奏**：schema 变了，我们的转换表可能变——库抽表、领域不变。**需在 README §12 中记一条待裁**；
- **西雅图之外的世界**（虚构）仍需手绘：Overture 只覆盖地球，不覆盖幻想世界。虚构地图走原有手绘路径，与 Overture 不交叉。

> **UUID 例外条款（本 ADR 新增）**
>
> GERS ID 形态为标准 hex UUID（如 `2f099609-47e9-4c3e-bfeb-072ccd609a03`），与 `AGENTS.md` §4.4「`[A-Za-z0-9_]`」规则**不兼容**。本 ADR 在 `wv-core` 引入新类型 `OvertureId` 时**明确豁免 §4.4 字符集约束**，理由如下：
>
> | 项 | 内容 |
> |---|---|
> | **豁免范围** | **仅 `OvertureId` 一种类型**（Overture 数据自带）；`PlaceId` / `EventId` / `EdgeId` / `CharacterId` 等**全部领域 ID 仍受 §4.4 约束**，不可混用 UUID |
> | **类型隔离** | `OvertureId` 必为**独立 newtype**（`struct OvertureId(String)` + 校验函数），**不与 `Id(String)` 通用 ID 共用**；编译器层面拒绝 `Place.overture_id: OvertureId` 与 `Place.id: Id` 互换 |
> | **序列化** | 领域 YAML 序列化为 `overture_id: "2f099609-..."`（**带 UUID 形式的引号**），便于肉眼与 `PlaceId: city_seattle` 区分 |
> | **作者负担** | 作者手写 `Place` 时**不需要**手动填 Overture UUID（避免出错）；UI 提供「从 Overture 库搜 → 选 → 自动填」面板。手动填也行，校验函数保证格式合法 |
> | **撤销条款** | 一旦 Overture 改为发行可读字符串 ID，本例外作废，由新 ADR（不修本 ADR）撤销 |
> | **被否决方案** | (1) 不开豁免、所有 ID 强制 UUID —— 破坏 §4.4 + 后续手写痛苦；(2) 不开豁免、所有 ID 强制可读 —— 让 Overture ID 手写复制出错；(3) 类型弱类型（`OvertureId: String`）—— 失去编译期拦截，**否决** |

### 待裁

| 项 | 状态 | 说明 |
|---|---|---|
| **`Place` 领域模型是否加 `geographic_kind: Real\|Enum(Reality)` 字段** | **已决议**（2026-10-13） | **否决**：不加 `Map.geographic_kind` 字段，由 `Map.geographic_source` 是否存在**派生**该属性（保留现有 P3 + §5 的「只存引用」纪律）。理由：(α) §6.2「待决问题显式化」——「是否真实地图」是渲染层派生态，不值得进领域字段；(β) `Map.geographic_source: Option<GeographicSourceV1>` 的 `Option` 已经是「真假」区别（`None` = 虚构，`Some` = 真实，字段本身即是开关）；(γ) `geographic_kind: enum` 与 P2 / §4.3「数据承载开放集」违反（新增枚举字段）。`Some(...)` 的具体内容（theme / bbox / data 路径）**只在 `Map.geographic_source` 内**承载，不抽公共 `GroundId` 类型。详见 §决策 (2) 同步注脚。 |
| **`OvertureId` 字段名 + UUID 例外落地** | **已决议**（2026-10-13，**字段名 = `overture_id`**） | 字段名选 `overture_id`：与 Overture 源数据同名（作者一眼认出）、与 `Map.geographic_kind` 不撞语义、UI/CLI 可直接跳 Overture 文档。`gers_id` 因术语透明度差被否决、`real_world_id` 因与 `geographic_kind` 字段语义撞车被否决。UUID 例外见上文条款（独立 newtype + 类型隔离）。 |
| **F. `class: footway` 是否独立分支**（Overture 实测发现） | **已决议 · 2026-10-13（filter 派胜出）** | **决议**：第一版按「filter 派」实现（即 `class_to_terrain("footway") = None`，不生成 `PlaceEdge`），详见 [`wv-overture/src/transportation.rs`](file:///d:/kudi-projects/worldview-rs/crates/wv-overture/src/transportation.rs)。理由：(α) 西雅图样本 77604 行 footway 占 50%，全量引入会让 `DistanceGraph` 的拓扑膨胀一个数量级、检索成本 / 内存成本骤增；(β) 当前 `Vehicle` 没有「步行」类别，引入 `footway_only` 字段会迫使全模型扩展（§4.2 「距离只存参数」是单向的，加字段需整体调速）；(δ) `Place` 的可访问性靠「步行能走到」这件事已经由**可达字段**承载（PlaceGraph 的可达性通过公共交通段覆盖，无需 footway 投影）。**未来路径**（**不再讨论**）：若未来需要「步行」独立模型，新开 ADR 撤销本决议，**不**修改本 ADR（§6.4「只增不改」）。 |
| **离线包发布** | **待裁**（**本 ADR 新增发现**） | 检索时是「在领域模型 + 外部 Parquet」两个源之间查——是否上索引？何时倒排？ |
| **Parquet 版本升级策略** | **待裁**（**本 ADR 新增发现**） | Overture schema 升级时，`wv-overture` 的转换表需同步；该 table 需要版本号；待定 |

---

## 备选方案（被否决的）

| 方案 | 否决理由 |
|---|---|
| **A. 全量下载 Overture Parquet 落进领域模型** | ❌ 违反 P2（Git 不可管理、现实时间十GB级）与 P3（不该为 Overture 开例外） |
| **B. 走 OSM PBF / osm2pgsql** | ❌ OSM schema 许可不完整（缺 buildings 里 `start_date`）、服务端转换服务不可控；Overture 是后继者、API 与原许可覆盖 OSM |
| **C. 只选留「`Map.geographic_kind: Fictional`，Overture 不入场」** | ❌ 「在地球上」的虚构作品这个项目类型可被排除，不符合项目定位（“创作者可加任何世界”） |
| **D. 「一次性下载」转为「运行时拉取」** | ❌ 违反 P1「单机可运行」与 §6.2「外联接口仅限初始化」；以及 P1 运行期不联网 |
| **E. 「领域模型不走 Overture，只用 `wv-overture` 抽词」** | ❌ 没了引用层，领域与现实失去可校验联系，违背项目初衷 |

---

## 参考

- [Overture Maps GitHub Org](https://github.com/OvertureMaps)
- [Overture Maps 文档](https://docs.overturemaps.org/)
- [Overture Cloud Sources（云原生路线 + bbox SQL 示例）](https://docs.overturemaps.org/getting-data/cloud-sources/)
- [Overture `transportation` 主题（含 road bbox 查询）](https://docs.overturemaps.org/schema/)
- `README.md` §3、§12（待决）
- `AGENTS.md` §4.2 / §4.3 / §4.4（UUID 例外见正文）/ §5
- ADR-0001（势力层 = `Vec<GeometrySpan>`）—— `terrain` 字符串体系不被动
- ADR-0004（单位 = 独立运动主体）