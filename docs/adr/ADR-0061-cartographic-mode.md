# ADR-0061: 专业制图范式——把 `ViewMode::GoogleMap` 从死模式救活

- 状态：**草案**（待主理人审批）
- 关联：ADR-0012（VectorFlat 取代 Google 为默认）、ADR-0016（地图配色轴）、
  ADR-0021（模式 → 渲染技法矩阵 `MODE_TABLE`）、ADR-0043（主题层抽 crate）、
  ADR-0047 追记一（道路分级着色）、ADR-0060（山地 / POI 分层）

## 1. 处境（Context）

**主理人裁定（2026-10-25 19:48）**：不要水彩 / 水墨等艺术主题，要「专业的地图呈现」
——参照百度地图普通地图，**色块表示即可，不关心贴图**。

核对源码后发现：`ViewMode::GoogleMap` 这个模式**存在但从未真正可用**，四处断线：

1. **地形色板零消费**：`colors_google::GOOGLE_CORE_TERRAIN`（近白陆地 / 淡水 /
   公园绿）自 2026-10-13 定义以来**没有任何调用点**——注释里反复提到「`shade_google`
   会把沙 / 草 / 丘折叠到 `[2]`」，而**该函数从未存在**。切到 GoogleMap 后地形
   画的仍是主题色板（JRPG 水彩褐阶），所以「切过去也不像地图」。
2. **处理旗标照搬主题**：`conv_pack_with` 直接透传 `pack.treatment`，水彩包的
   `HAND_DRAWN`（海岸晕 / 水彩渐变 / 纸纹）在普通地图上照旧生效。
3. **覆盖物符号照画**：`MODE_TABLE` 的 GoogleMap 行关掉了 `TerrainDecor`，但
   **`FeatureOverlay` 仍为 true**——具名森林会叠一层手绘树图集，与「普通地图」
   的色块语义打架。
4. **UI 不可达**：`BROWSING` 只有三项（矢量 / 色块 / 势力），命令面板同样只注册
   这三项，`1/2/3` 快捷键也没有它。**用户根本切不到这个模式。**

## 2. 决策（Decision）

**D1 · 专业制图沿用 `ViewMode::GoogleMap`，不自建新模式**

该模式的语义、配色、矩阵行都已在位，缺的只是接线。另立 `Cartographic` 模式会
让 `MODE_TABLE` / `RenderRecipe` / 色板三处各多一份，且与既有模式高度重叠。

**D2 · 地形色板按模式分派（`conv_palette_terrain`）**

`conv_palette_terrain(palette, mode)`：GoogleMap 下把 `core` 换成
`GOOGLE_CORE_TERRAIN`、`soften_toward` 换成 `GOOGLE_SOFTEN_TOWARD`（近白纸面）；
其余模式原样。`conv_pack_with` 增 `mode` 参数并改用它——这是那份孤儿色板的
**第一个消费入口**。

**D3 · 处理旗标在 GoogleMap 下强制 `EMPTY`**

海岸晕 / 水彩渐变 / 雪影 / 纸纹都是「手绘」语汇。专业制图要的是一眼看懂的平涂，
故不读主题旗标，直接 `TreatmentFlags::EMPTY`。判据在 `conv_pack_with` 内（同一
入口，避免两处各判一次）。

**D4 · 覆盖物符号在 GoogleMap 下不画，地界晕保留**

森林 / 高地等具名地物的**绿地由群系色块表达**（`CORE_TERRAIN[4] = #C9E7C7`），
再叠树图集是两套语义打架。抽 `feature_symbol_alpha(mode, blend, opacity)`：
GoogleMap 恒 `0`，其余模式维持「近带满强度、远带退场」。

**地界晕保留**——它是「这片地物的辖域」这一**事实**的呈现，与画法无关；专业地图
同样需要「这一片是莲花山森林公园」的边界暗示。

**D5 · 道路分级**：见 ADR-0047 追记一（rank 三档 → 橙黄干线 / 饱和黄干道 / 白支路）。

**D6 · UI 可达**：`BROWSING` 追加第四项 `GoogleMap`（**只能追加**，前三项的
`1/2/3` 快捷键语义不动），命令面板注册「切换到普通地图（专业制图）」，快捷键 `4`。

## 3. 理由（Why This）

- **「模式决定技法」本来就是 ADR-0021 的分工**：`RenderRecipe` 管「画哪些 pass」，
  本次补的是「每个 pass 用哪套数据」。把它们放进 `MODE_TABLE` 之外的渲染入口，
  而不是新建模式或新建主题包，符合既有边界。
- **为什么不做「专业制图 ThemePack」**：`ViewMode`（技法轴）与 `ThemePack`（配色轴）
  是两个正交轴（ADR-0016）。把「专业制图」做成主题包，会迫使 author 在
  「切模式」时同时切主题，两个轴就纠缠了。当前方案的代价是「模式的配色判据散在
  两个函数里」（`conv_pack_with` 与 `feature_symbol_alpha`），但都只有一处。
- **零新数据**：绿地色块来自既有群系栅格，道路分级来自既有 `effective_rank`，
  POI 红点来自既有 `place_dots`——不引入任何新数据源或新资产。

## 4. 代价与风险（Cost）

- `conv_pack_with` 签名 +1 参数，6 处调用点同步（`conv_pack` 传 `VectorFlat` 保持
  基准恒定）。
- **风险 1（真实存在）**：群系是**栅格**，绿地边缘是格子边界（像素化）。百度 / Google
  的绿地面是多边形矢量数据，本方案拿不到那种干净边界。这是本范式**当前最大的
  观感缺口**，解法见 §5。
- **风险 2**：GoogleMap 的地形底图走旧纹理管线（亚像素合成），与 VectorFlat 的
  矢量管径不同；两条路径的观感不会完全一致。
- **风险 3**：`EMPTY` treatment 后海岸线只剩色差（无海岸晕），近白底图上水陆
  边界可能偏弱，需实机确认是否要单独补一条细海岸线。

## 5. 被否决的备选（Alternatives）

- **新建 `ViewMode::Cartographic`**：与 GoogleMap 语义重叠，三处矩阵各多一份。否决。
- **新建「专业制图」ThemePack**：混淆技法轴与配色轴（见 §3）。否决。
- **给群系边界做 marching squares 矢量化填充**：`isoline_overlay` 只出**边界线**，
  「多边形平涂的 mesh 化填充」在 ADR-0012 里就是「留待 M2 后半」的未完成项；
  真要做是新建一条填充管线（等值线提取 → 三角化 → mesh），工作量数天。**本次不做，
  但这是解决风险 1 的正解**，建议单独立 ADR。

## 6. 验收标准（Acceptance）

1. `BROWSING` 四项、快捷键 `4` 可切、命令面板可搜到「普通地图」。
2. 切过去后地形为近白陆地 + `#AADAFF` 淡水 + `#C9E7C7` 公园绿，**无**海岸晕 /
   纸纹 / 水彩渐变。
3. 具名森林**不出现树图集**，只有地界晕。
4. 道路三档可辨（橙黄 / 黄 / 白），放大到 Edit 档才见白色支路。
5. 单测锚点：`google_mode_terrain_core_is_the_google_palette`（色板 + treatment）、
   `google_mode_draws_no_feature_symbols`、
   `google_road_casing_is_thicker_than_fill_for_every_grade`、
   `google_road_grades_have_distinct_fills`、`road_grade_spans_split_prefix_by_rank`。
6. `wv-render` 365 测、`wv-app` 316 测全绿。

## 7. 遗留

- **绿地边缘像素化**（风险 1）：需矢量化填充管线，另立 ADR。
- **POI 无分类**：百度有 6~7 类（餐饮 / 酒店 / 医疗 / 景点 / 码头），当前只有
  红点白环一种。`Place` 无分类字段，需先裁「加 `kind` 字段」还是「按 biome / rank
  推导」。
- **无建筑轮廓块**：城区那层浅灰多边形缺数据源，待确认工程侧是否有建筑面数据。
- 艺术主题（水彩 / 水墨 / 手绘贴图）**搁置不删**：`ViewMode::VectorFlat` 下的
  ThemePack 体系仍完整，随时可回。
