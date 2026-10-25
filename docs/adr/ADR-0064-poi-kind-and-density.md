# ADR-0064: POI 分类与屏幕网格密度——从「一堆红点」到「检索地图」

- 状态：**已接受**（2026-10-25，随 `feat/professional-cartography` 落地）
- 关联：ADR-0030（呈现级别 + 密度上限）、ADR-0047（统一呈现级别）、
  ADR-0061（专业制图范式）、ADR-0062/0063（同分支的矢量面 / 边界线）、
  专业地图样张 `preview/professional_map_preview.html`（勾选项「POI 分类符号」）

## 1. 处境（Context）

样张要求两件事，现状各缺一半：

| 样张要求 | 现状 | 缺口 |
|---|---|---|
| POI 有类别（餐饮 / 住宿 / 医疗 / 景点 / 车站） | `Place` 只有 `biome`（地形族） | **没有语义类别字段**；`PlaceDot` 只有 `is_city` 两档材质 |
| 密度像检索地图 | rank 硬门 → 排序 → **全局截断 400** | 密集城区的高 rank 点吃掉全部名额，稀疏区空着 |

第二个缺口的具体表现：全局 Top-N 是「按重要性」而不是「按空间均匀」。同一片
城区里 500 个点 vs 郊外 3 个点，截断后城区糊成一片、郊外一个不剩——而百度 /
Google 的观感恰恰是**每屏网格留几个**。

第一个缺口的根因是数据模型：`biome` 回答「这块地是什么地形」，表达不了
「同是平原上的饭店和医院」。

## 2. 决策（Decision）

**D1 · `Place.kind: Option<PoiKind>`，封闭枚举 12 类**

```yaml
# distances/places.yaml
- id: place_hospital_renbo
  name: 仁伯爵综合医院
  map: map_macau
  x: 120.0
  y: 340.0
  kind: medical        # other/settlement/peak/port/station/scenic/food/lodging/
                       # medical/education/government/shop
```

- **封闭枚举而非自由字符串**：图标集是有限集；自由串只会带来拼错与静默回退，
  新增类别必须同时给出配色——封闭枚举逼出这个决策。
- **缺省不落盘**（`skip_serializing_if`）：旧工程保存后不多出 `kind: null`
  噪音；`serde(default)` 保证旧 YAML 照常读入。**零迁移**。
- `PoiKind::key()` 与 serde 拼写由测试钉死一致（YAML 走 serde，主题色表走
  `key()`，两处拼写不许漂移）。

**D2 · 屏幕网格配额**

`select_pois` 在 rank 门之后加一步空间配额：

| 参数 | 值 | 理由 |
|---|---|---|
| 格边长 | `POI_GRID_PX = 96` 屏幕像素 × `view_scale` | 1600×900 视口 ≈ 17×10 格，格子恰是街区尺度 |
| 每格配额 | Edit 档 2，其余档 1 | 编辑态作者要看细节；远观只留地标 |
| 全局上限 | `POI_MAX_DOTS = 400`（保留） | 兜底极稀疏大图 |

先排序（rank > is_city > 名字长）再按格取，保证格内保留的是高优先级；
排序稳定 → 输出确定。全球面格按 `floor(pos / cell)` 落桶，负坐标同样正确。

**D3 · 分类色落 `wv-theme::colors_google`**

8 个新色（餐饮橙 / 住宿紫 / 医疗红 / 景点绿 / 交通蓝 / 行政青 / 教育靛 /
商业品红）+ 未分类与聚落沿用 `GOOGLE_PLACE_DOT`（Google 红）。两处有意的借用：
山峰借景点绿（地标 = 可游览）、港口并入交通蓝（同为「到达点」）。

为什么放 `wv-theme` 而不是 `wv-world-cell-map`：后者是**数据视图**的固定色
（ADR-0054 禁令），POI 分类色属于地图呈现的主题色。

**D4 · 仅专业制图按类分桶**

`GoogleMap` 下 12 组合批（白环 + 类别色内芯，索引 = `PoiKind::index()`）；
其余模式保持「城市 / 普通」两档材质——艺术主题刻意不把类别符号化（与水彩 /
水墨的手绘语汇打架）。

**D5 · 数据缺失的降级**

无 `kind` 时：有 `w/h` 的聚落 → `Settlement`，其余 → `Other`（红点白环）。
不从 `biome` 折算：带 `biome` 的地点**不生成 POI 点标**（覆盖物负责表现，
见 `world_projection::place_dot_for`），山峰 / 港口这类 biome 型地物要出现在
POI 层必须显式写 `kind`。

## 3. 理由（Why This）

- **作者可读**：`kind: medical` 是 YAML 里最直观的表达；比「按 biome 推导」
  准确，比自由标签可校验。
- **零迁移**：缺省不落盘 + serde default，旧工程打开即用；红点行为逐位不变。
- **空间均匀才是检索地图的密度语义**：全局 Top-N 是「重要性榜」，不是「地图
  可读性」；96px 网格是 OCR/视觉实验里的常用街区尺度。
- **按类分桶是合批的必然**：一个 mesh 一个材质一种色；12 个空池不产 mesh，
  实际开销只在有该类点的档位。

## 4. 代价与风险（Cost）

- **12 组 × 2（环 + 芯）= 24 个合批池**：典型视野只有 3~5 类有点，空池零 mesh；
  但实体清理逻辑多一条分支（模式切换时退场）。
- **YAML 手写**：作者要写 `kind` 才有分类色；编辑器 UI 尚无选择器（见遗留）。
- **网格边界会切散聚类**：一条街上的点被格线分到两格时，配额各算各的；观感上
  可接受（不追求严格均匀）。
- **96px 是拍定的初值**：密集城区仍需实机校准（可调 `POI_GRID_PX` /
  配额表，不影响数据结构）。

## 5. 被否决的备选（Alternatives）

- **自由字符串 `tags: map`**：灵活但无校验、易拼错、静默回退；图标集本就有限。
- **只按 `biome` 推导**：表达不了餐饮 / 医疗 / 车站；且 biome 型地点不生成点标。
- **首版直接上图片图标**：需要图标资产、图集、UV 合批改造；文字标签 + 颜色
  已经能读出类别（样张也是「彩色圆 + 名称」）。留作下一步。
- **保持全局 Top-N 只调大上限**：治标——城区的点会继续挤掉郊外。

## 6. 验收（Acceptance）

1. `poi_kind_key_matches_serde_spelling`（12 类拼写 + index 声明序）。
2. `place_without_kind_round_trips_without_emitting_the_field`（零迁移）。
3. `place_kind_round_trips_as_snake_case`（`kind: medical` 可读回）。
4. `select_pois_gates_by_rank_then_caps_by_priority`（原有语义不回归）。
5. `dense_cluster_keeps_quota_per_screen_cell_and_spares_sparse_areas`
   （密集格留 1~2、稀疏点不被挤掉、格内保留高优先级）。
6. `poi_kind_colors_are_distinct_for_classified_categories`（8 类互不相同，
   未分类/聚落回退红点）。

## 7. 遗留

- **作者 UI 选择器**：`kind` 目前只能手写 YAML；Inspector 尚无下拉。
- **分类图标**：当前是「统一 map-pin + 类别针体色」；按类别换 icon（山 / 医院 /
  餐具…）需要图标资产 + 图集合批（`symbol_atlas` 已有基础件）。
- **行政 / 省界 POI**：需要在 `PoiKind` 之外表达「层级」（国 / 省 / 市）。
- **视觉验收**：本机 agent 环境无法落盘截图（写盘被安全策略拒绝，os error 5），
  需在主理人桌面终端跑：
  ```bash
  WV_MODE=map WV_SHOT=D:\shot.png wv gui -m examples/jrpg-world
  ```

## 8. 追记一（2026-10-25 · 主理人两条裁定）

### 一、山峰 = 显式 `kind: peak`；`biome` 永不生成点

主理人原话：「山峰就是一个 poi。biome 不会生成任何点。全靠用户自己把山峰加入。」

这条**关闭**本节原先「山峰 POI 接通（ADR-0060 遗留）」的表述——它不是待补的
缺口，而是既定设计：

- 带 `biome` 的地点由覆盖物层表达，**永不出点标**（`place_dot_for` 的排除是
  正式行为，不是临时规避）；
- 作者要标一座峰，就加一个 `kind: peak` 的**点**地点，且**不写** `biome`
  （写了 biome 反而会被点标路径排除）。
- ADR-0060 里「山峰 = `Place{biome: Highland, w/h: None}`」在 POI 层**不成立**；
  它是「区域覆盖物」的建模方式，与 POI 点标是两条路。

### 二、POI 形状：统一 map-pin，后续按类补图标

主理人原话：「暂时用 map-pin。后面要补充不同 icon 做 poi」

- 专业制图的分类 POI 从「白环 + 实心圆」改为 **map-pin**（Lucide `map-pin`
  实心化：圆头 + 尖尾 + 白芯），类别只换针体色；
- 复用 `landmark_symbols::push_map_pin_tinted`（本次新增：形状不变、白芯不变，
  仅针体换色），一个白色材质 + 顶点色，一类一个合批 mesh；
- 尺寸与地物符号同源（屏幕常量 22.5px 高 + LOD 档微调），锚点 = pin 底尖；
- 「不同 icon」留作下一步（见遗留）。

**代价**：pin 比圆点占位大（22.5px 高 vs 原半径约 8px），近距离同格两点可能
视觉重叠；当前网格配额（96px / 1~2 个）可兜住，实机再校准。

## 9. 追记二（2026-10-25 · 聚落显式标注 + 作者 UI）

### 一、存量城市显式标 `settlement`（方案 A）

主理人裁定「按推荐」：

- **不做** `rank == 3 → Settlement` 的隐式推导——`rank` 是呈现重要性而不是类型，
  一座 rank 3 的圣地 / 港口会被误判成聚落；
- 改为显式字段。已补：`examples/jrpg-world/distances/places.yaml` 的 9 个聚落条目
  （含 3 个无锚点的距离图节点）；`gen_small_demo` / `gen_layer_showcase` /
  `gen_lod_demo` 的城市构造同步（生成器是那些工程的源头，改 YAML 会被覆盖）；
- 不写 `kind` 的地点仍回退红点，零迁移契约不变。

### 二、作者 UI：Inspector 类别下拉（已落地）

- **新命令** `SetPlaceKindCommand { id, kind: Option<PoiKind> }`：
  `Some(kind)` = 设为该类，`None` = 清空。
  **为什么不并进 `UpdatePlaceCommand`**：后者是整实体补丁、构造点分散在面板 /
  HTTP skill / 测试里，加字段要全量改；而「选一个类别」是独立作者意图，
  撤销粒度应当是**一次选择**。
- **表单位置**：属性页签 → 地点分支 → 「呈现级别」下一行「类别」下拉；
  选完立即提交（与级别下拉同一习惯，无确定按钮），Ctrl+Z 可回退。
- **YAML 语义**：选具体类别写 `kind: medical`；选「未分类」= 清空，不落盘。
- **i18n**：新增 13 个键 × 中英（`InspectorPlaceKind` + 12 个类别），
  `I18nKey::ALL` 508 → 521。
- **新建地点默认「未分类」**（按推荐）；图层 / 图例按类别分色**未做**（后放，
  见遗留）。
