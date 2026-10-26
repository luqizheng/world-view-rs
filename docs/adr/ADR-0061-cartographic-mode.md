# ADR-0061: 专业制图范式——把 `ViewMode::GoogleMap` 从死模式救活

- 状态：**已接受**（2026-10-25 落地；§1 的第 1 条病因与 D2 的接线效力经
  **追记一**修正，正文按 §6.3 保留原文）
- 关联：ADR-0012（VectorFlat 取代 Google 为默认）、ADR-0016（地图配色轴）、
  ADR-0021（模式 → 渲染技法矩阵 `MODE_TABLE`）、ADR-0043（主题层抽 crate）、
  ADR-0047 追记一（道路分级着色）、ADR-0060（山地 / POI 分层）、
  ADR-0062（群系矢量面——本 ADR 风险 1 的正解）

## 1. 处境（Context）

> ⚠️ 本节第 1 条的病因诊断**与事实不符**（地形早已不是主题色板），修正见 §8 追记一。
> 其余三条成立。

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

## 8. 追记一（2026-10-25 · 落地修正：五处补丁与一条被推翻的诊断）

本 ADR 落地并复核时发现四类必须修正的问题；正文按 §6.3 不改，逐条记于此。

### 一、§1 第 1 条病因**不成立**：地形早就不是主题色板

`shade_google` 一直存在，且**忽略传入的 palette**，直读 `wv-world-cell-map` 内的
同名抄录表（`palette.rs` 的 `GOOGLE_CORE_TERRAIN`，注释自述「抄录」）：

```rust
// 修正前
fn shade_google(_data: &TerrainData, kind: u8, _pal: &CellPalette) -> Color {
    match kind { 0 | 1 | 4 | 8 => palette::google_core_terrain(kind), _ => palette::GOOGLE_CORE_TERRAIN[2] }
}
```

所以「切到 GoogleMap 后地形画的仍是 JRPG 水彩褐阶」与实测相反——地形一直是
近白陆地 + 淡水 + 公园绿。真实原因是：treatment 照搬主题（第 2 条）、覆盖物符号
（第 3 条）、道路单一画法（ADR-0047 追记一）、以及**根本切不过去**（第 4 条）。

**连带后果**：D2 设置 `CellPack.palette.core` 对 Google 模式的地形**像素无效**
（唯一实际生效的是 `soften_toward`），验收测试
`google_mode_terrain_core_is_the_google_palette` 只断言结构体字段，是假绿。

**修正（P1-1）**：`shade_google` 改读 `pal.core[kind]` / `pal.core[2]`，主题侧
`GOOGLE_CORE_TERRAIN` 成为唯一真相源；`wv-world-cell-map` 本地表退为「无主题
环境（CellMap 工作台 / 测试夹具）的默认数据源」。新增哨兵色单测
`google_shade_reads_palette_core_not_the_local_copy` 钉住「跟着 palette 走」。

### 二、D3 只清了 treatment，漏了 SeaRecipe（P1-2）

海配方（水纹颗粒 / 离岸波纹 / 碎浪 / 近岸排线 / 浅滩积墨）与 `TreatmentFlags`
是**两套独立开关**：`cells.rs` 的海面绘制只按 SeaRecipe 字段是否非空生效，不看
treatment。D3 强制 EMPTY 而 `conv_pack_with` 仍 `conv_sea(&pack.sea)` 透传，
专业地图的水面会带回水彩笔触。

**修正**：GoogleMap 下走 `conv_sea_flat`（清空全部手绘层，保留深度渐变——它是
数据而非画法）。单测 `google_mode_flattens_sea_recipe_and_hypsometric`。

### 三、分层设色泄漏（P2-1）

`hypsometric` 被无条件透传，作者调过滑条后 Google 模式会出现海拔分带。专业制图
陆地单色，故 GoogleMap 下强制归零（同一条单测）。

### 四、D6「UI 可达」未达成（P1-3）

- `ui_shell/shell.rs` 只绑 `Num1/Num2/Num3`，**没有 `Num4`** → §6 验收标准 1 的
  「快捷键 4 可切」当时为假；
- 顶栏是 **Browse / Edit / CellMap 三段工作区**（硬编码 VectorFlat / Schematic /
  Topographic），**不遍历 `BROWSING`** → 普通地图不在顶栏；
- 唯一入口是命令面板。把一个「目标范式」藏在 Ctrl+K 里等于没有。

**修正**：快捷键补 `Num4`；顶栏改四段 **Browse / Map / Edit / CellMap**（`Map` 段
追加而非插队，前三段顺序与肌肉记忆不动）；分段逻辑由「按下标」改为「按目标模式」
判定，避免第 N 段语义漂移；`state.rs` / `dispatch.rs` / `shell.rs` 三处过时注释
同步更正（旧注释称「顶栏也列 BROWSING」与实况不符）。

### 五、海岸线缺失（P1-4）

`MODE_TABLE` 的 GoogleMap 行 `Isoline = false`，专业制图的水陆边界只剩烘焙纹理的
亚像素色差；而 `GOOGLE_COAST_LINE` / `GOOGLE_COAST_LINE_COLOR` 两个令牌自 2026-10-13
起零消费（与 §1 第 1 条同一种病）。样张图例明写「水 · 面 #AADAFF / 线 7FBEEA」。

**修正**：

1. GoogleMap 行 `Isoline = true`；
2. `sync_isoline_overlay` 的 treatment 改为与 `conv_pack_with` **同源**（GoogleMap
   强制 EMPTY）——否则一开就把海岸晕 / 河中线带回来；
3. GoogleMap 只取**海岸链**（类型边界由 ADR-0062 的矢量面承担，双线不可读）；
4. 线色走 `GOOGLE_COAST_LINE_COLOR`；
5. 该模式的海岸线视为**底图要素**，不受 `view.isolines` 会话开关支配（其余模式不变）。

### 六、绿地边缘像素化（原风险 1）→ 已由 ADR-0062 解决

原 §4 把它列为「本范式当前最大的观感缺口」、§5 把矢量化填充估成「工作量数天」。
实测低估了复用面：EDT / marching squares / Chaikin / DP / earcut 全部已在仓库，
新代码只是「距离场接线 + 环嵌套 + 缓存」。实施与决策见 **ADR-0062**。

### 增补验收（对应本节修正）

| # | 锚点 |
|---|---|
| A1 | `google_shade_reads_palette_core_not_the_local_copy`（色板真相源） |
| A2 | `google_mode_flattens_sea_recipe_and_hypsometric`（海配方 + 分层设色） |
| A3 | `isoline_draws_in_vector_flat_topographic_and_cartographic`（模式表） |
| A4 | `google_mode_collapses_elevation_bands_into_one_land_color` 改喂生产口径色板包 |
| A5 | ADR-0062 的 11 条单测（5 条距离场 + 6 条成面） |

门禁实测：`wv-render` 372 测、`wv-world-cell-map` 299 测、`app-test` 311 测、
`smoke-gui.sh` PASS。`wv-io` 与 `app` 的 5 条写盘测试在本机报 os error 5，属已知
执行环境约束（AGENTS §2.2 追记），与本次改动无关。

## 9. 追记二（2026-10-26 · 实机截图两处收口：图框退场 + 一处死配置）

实机对比（Map 档 vs Browse 档）暴露出两处与「普通地图」范式相悖的残留。

### 一、专业制图不画地图外框与 1/4 幅面参考网格

- **现象**：Map 档地图上横贯三道淡线（竖 3 / 横 3 的 1/4 幅面参考网格）外加一圈
  外框，视觉上被读成「编辑器网格」，与百度式普通地图的满屏画布气质直接冲突。
- **决策**：`ViewMode::GoogleMap` 的 `PassId::MapFrame` 置 **false** —— 外框与参考
  网格一起退场。其余模式（VectorFlat / RealTerrain / Schematic / Faction / Event /
  Coastline）保持原状；Topographic 本来就关。
- **理由**：外框是**印刷品**语汇（图廓），参考网格是**编辑器**语汇（定位辅助）。
  专业制图是「满屏画布」——Google / 百度的普通地图两者都没有。定位需求已由
  状态栏经纬度读数（ADR-0016 时期）承担，不需要画面上的辅助线。

### 二、`PassId::MapFrame` 此前是**死配置**（本追记一并修正）

`map_frame::sync_map_frame` 一直只判 `view.terrain_visible`，从不读渲染配方——
mode 表里那一格写什么都没人看（与本仓库 2026-10-23 发现的 `PassId::GridLines`
同类：表在骗人）。本次让系统真正读配方，表才成为唯一真相源。

顺带兜底：`MapRenderPlugin` 内 `init_resource::<RenderRecipe>()`——只装该 Plugin
的游戏路径不会因缺资源 panic（Default = 全关，fail-safe）。

### 增补验收

| # | 锚点 |
|---|---|
| B1 | `cartographic_mode_hides_map_frame_and_reference_grid`（全模式逐行锚定） |
| B2 | 既有 `unknown_mode_yields_all_disabled` 仍断言 VectorFlat 的 MapFrame 开（对照） |
