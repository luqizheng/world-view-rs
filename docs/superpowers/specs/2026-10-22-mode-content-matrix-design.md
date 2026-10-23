# 三模式内容矩阵设计（Browse/Edit/CellMap 各看什么）

**日期**：2026-10-22（v1.4，增补群系分界线工具）
**状态**：待评审
**关联**：[ADR-0021](../../adr/ADR-0021-render-recipe-and-treatment.md)（RenderRecipe/MODE_TABLE——**本设计的接线对象**）、[ADR-0039](../../adr/ADR-0039-topographic-viewmode.md)（Topographic）、[ADR-0017](../../adr/ADR-0017-label-layer.md)（标签层）、[ADR-0045](../../adr/ADR-0045-nature-region-model.md)（区域三件套）、[统一 Inspector 设计](2026-10-22-unified-inspector-panel-design.md)
**实现域**：wv-render 渲染层 + egui（**不是** Bevy Node UI）

---

## 1. 处境

顶栏三段（Browse/Edit/CellMap）目前**只换底图**：POI、路网、标签、事件点、钻入标记在 CellMap 照画。builder check 发现关键事实：**ADR-0021 的 `RenderRecipe::MODE_TABLE` 早已把 Topographic 行声明成「内容全关」**（recipe.rs:263-288：Streets/Poi/Buildings/Faction/PlaceLabels/StreetLabels/SeaLabels/EventDots/DrillMarkers/FeatureOverlay/TerrainDecor 全 false，并注明 DrillMarkers「分析视图不要钻入子图入口」）——但 POI 绘制、路网 mesh、五套标签、事件点、钻入标记这些系统**还没消费这张表**（vector_layers.rs:679 留着「recipe 路径放 MODE_TABLE 后续接」的 TODO）。

用户拍板的三分法：

> 1. **Browse**：添加事件、事态，也可以录入人物
> 2. **Edit**：关注势力、POI 等，也可以录入人物；**事件与区域不显示（它们有时间性）**
> 3. **CellMap**：只关注自然（地形地貌），其他不管

## 2. 内容矩阵（已裁定）

| 内容层 | Browse | Edit | CellMap | 依据 |
|---|---|---|---|---|
| 地形栅格 | ✓ | ✓ | ✓ | — |
| **群系色块底图（固定色板）** | ✗ | ✗ | **✓** | **v1.2 用户裁定**：CellMap 用颜色区分群系，颜色固定不随主题——最底层的地形编辑器（见 §2.5） |
| 等高线（Isoline） | ✗ | ✗ | ✓（用户可关） | MODE_TABLE 既有 Isoline=Topographic true；作海拔参照叠在色块上 |
| POI 图标 + 地点标签 | ✓ | ✓ | ✗ | 用户裁定；接线 Poi/PlaceLabels |
| 路网（mesh + 路名） | ✓ | ✓ | ✗ | 同上；接线 Streets/StreetLabels |
| 势力填色 / 边界 / 标签 | ✓ | ✓ | ✗ | 势力属 Edit（faction_overlays **已消费**表，标签待接） |
| **事件点 + 事件标签** | ✓ | **✗** | ✗ | 用户：有时间性；EventDots 行已声明，Schematic 翻 false |
| **区域轮廓 / 覆盖物 / 标签** | ✓ | **✗** | ✗ | 同上；需从 TerrainDecor 拆出独立 pass |
| 事实地界晕（feature_overlays） | ✓ | ✓ | ✗ | **已消费**表（FeatureOverlay=false） |
| 森林等地形装饰（TerrainDecor） | ✓ | ✓ | ✗ | **MODE_TABLE 既有裁定**（分析视图保持干净）——CellMap 无森林符号是 ADR-0021 既有语义，非本次新裁 |
| 作者小河 / 派生河（Rivers） | ✓ | ✓ | ✓ | **MODE_TABLE 既有裁定**：水系是水陆地貌的一部分（真=保留） |
| 海名（SeaLabels） | ✓ | ✓ | **✓** | **本次翻表**：Topographic false→true（海名属地貌呈现） |
| 钻入子图标记 + 双击钻入 | ✓ | ✓ | ✗ | 表已声明 DrillMarkers=false；补接线 + 补 drill_pick 门 |
| 地形格网 | 用户开关 | 用户开关 | 关 | **表已裁定** GridLines=false（分析视图无编辑格）；V1 不承诺格网 |
| 登记入口（人物/事件/事态浮窗） | 可开 | 可开 | 面板快照屏蔽 | 现状即满足（用户：「好」） |
| Inspector 页签 | 属性/势力层/事态 | 同左 | **仅属性可点** | 用户裁定（U1：`tabs(disabled: &[1,2])`） |

**编辑语义不变**：只动呈现。标签不画 ⇒ 不注册命中 ⇒ 点不到，符合「CellMap 只管格子」。

### 2.5 群系色块底图（v1.2 新增 · CellMap 的地形画法）

用户裁定：**CellMap 是最底层的地形编辑器**——格子用颜色区分群系，颜色**固定、不随 ThemePack**。这重定义了 Topographic 的地形画法：不再是 ADR-0039 的等高分析底图（`shade_topographic` 只读 height、biome 不参与），而是**原始数据视图**：每格颜色 = 冻结群系表的固定色。

- **固定色板**：`BIOME_RAW_COLORS: [Color32; 21]`，落 `wv-render` 常量（先例：region_overlay 的「固定诊断色，不随主题色板」，ADR-0045 追记一）。同族同色相、按群系序号做明度阶梯（森林系深绿→浅绿；水系深蓝→浅蓝…），**不经 ThemePack / TreatmentFlags**（水彩晕开等在本分支不生效——数据视图要的是诚实）。
- **biome = NONE 的回退**：surface=水 → 固定水蓝；surface=陆 → 固定灰米。回退色同为常量。
- **高度不参与颜色**：改海拔不变色（颜色传达群系；海拔参照交给等高线 + cell 高亮）。平坦纯色，不做浮雕/晕开。
- **对 ADR-0039 的修正**：Topographic 的地形画法由「等高分析底图」改为「群系原始数据视图」——等高线降级为可关的参照层（Isoline 表格不动）。ADR-0051 一并记。

### 2.6 CellMap 自然区开关（v1.3 新增）

用户裁定：CellMap 要有开关显示所有 **Nature 类 Region**（`RegionKind::Nature`，ADR-0045），放 **map controls 与 grid 并列**；开启后程序按地形/群系自动高亮这些区域，作者可命名（如「迷幻深林」「末日火山」）；**未命名不落盘**（= ADR-0045 D4「命名即物化」，既有规则）。

**既有机械**（不重造）：`RegionKind::Nature` ✓；`candidate_at`（光标处候选派生，biome 连通区）✓；`materialize_nature_region_now(name, cell)` ✓；候选金预览（Z 0.077）✓。

**新增四件**：
1. **开关**：map controls 的 grid 按钮旁新增 Nature 开关（Lucide 图标），写会话态 `ViewState.nature_overlay: bool`（不落盘）。
2. **放行规则**：RegionLayers 在 Topographic 的渲染条件改为「`nature_overlay == true` 且 region `kind == Nature`」——Area 区在 CellMap 永远隐藏；其他模式不受开关影响（RegionLayers 本来就画）。
3. **全图候选枚举**：开关开启时按 biome 连通区**全图**枚举未命名候选（复用 `isoline::kind_segments` + `assemble_chains`，ADR-0045 D1 同源），画细虚线金框（与既有单候选预览同色系）；按地形指纹缓存（先例 `EditCellState.region_fingerprint`），地形编辑即失效。候选排除已被命名 Nature 区覆盖的格。
4. **命名入口（新 UI，补用户预言的缺口）**：开关开启时**点击候选** → 候选上下文进 `NatureRegionUiState`（既有资源）→ Inspector「属性」页签出**候选命名表单**（名字输入 + 物化 + 丢弃；优先级在 Cell 工具之下、空态之上）。物化 = 走既有 `materialize_nature_region_now` 落盘，转正后画三件套。

**底色用主题规则色**（Nature 区覆盖物规则，ADR-0045 D2）——CellMap 虽是数据视图，但覆盖物声明本身就是「这区是什么」的数据；轮廓用固定诊断色。可翻案。

### 2.7 群系分界线（v1.4 新增 · 编辑工具）

用户裁定（语义 **B 分界线**）：自然区的边界是一条**可拖动的线**；拖动它，扫过的格子群系跟着变——**线驱动群系**（与 ADR-0045「群系驱动边界」的派生方向互逆，但**不推翻它**：派生仍负责「线画完之后」的轮廓与候选）。

- **分界语义（B）**：线两侧各绑一个群系（A | B）。拖动 = 推移分界：扫过格里 `biome == 退让侧` 的格子改成 `推进侧`；**第三种群系不动**（水、山不被森林线误染）。两侧判定按旧线段的叉积符号；双向拖动对称成立。
- **线是会话工具态，不落盘**：真相仍是 `terrain.bin` 的格子群系（P3/P7 不破）。工具激活时线常驻（可反复拖），切走工具即消失。
- **拖完之后**：未命名候选实时跟随（指纹缓存失效重算）；**命名区边界不自动重生成**（D6 快照语义，走既有「重生成边界」按钮手动刷——自动重生成列为后续）。
- **撤销粒度**：一次拖动（松手）= **1 条 `BatchEditCellsCommand`**（既有命令复用，零新命令）。
- **线形态**：v1 直线段；折线（贴合弯曲边界）后补。
- **工具归位**：`ToolKind::BiomeBoundary` 顶层（ADR-0040「工具语义钉清楚」先例）；**仅 Topographic 可用**（地形级编辑属 CellMap）。地图工具条 + 1 个 Lucide 图标。
- **交互**：工具激活后在地图上拖出线段即创建；**两侧群系在线中点两侧自动采样**探测，Inspector 表单两个下拉可改（探测错时兜底）；拖线身 = 平移、拖端点 = 旋转/伸缩；拖动中实时高亮「将被改写」的格子（预览），松手提交。
- **渲染**：预览走新会话轴 `PassId::BiomeBoundary`（照 CellSelection 同款：全模式 true、由 `tool_active` 控制）。

## 3. 实现落点

**门控车辆（关键裁定）**：**接线既有 MODE_TABLE**（ADR-0021 本意，消灭 declared-but-unconsumed 的误导），**不**散布 `view.mode` if。需要新语义的（Schematic 藏事件/区域）也走表：翻对应格 / 拆新 pass。

| # | 落点 | 改法 |
|---|---|---|
| A1 接线 | `vector_layers.rs`：`sync_street_layers`（Streets）、`sync_poi` + `sync_landmarks`（Poi） | 消费 recipe 对应格（false ⇒ 不重建/隐藏） |
| A2 接线 | 五套标签系统（drill.rs place/faction/event/region + streets.rs 路名；海名见 C1） | 消费各自表格。**标签池是持久实体**（隐藏非销毁）——false ⇒ 走「隐藏池」尾部，不能只跳过注册（否则冻结标签残留屏上）；`update_place_labels` 是链首、独占 `hits.clear()`，照常跑 |
| A3 接线 | 事件点 gizmo（`world_projection.rs:684`，现只吃 `layers.event_point.visible`） | 改为「图层开关 ∧ EventDots 表格」双门 |
| A4 接线 | 钻入标记（`drill.rs` draw_drill_markers）+ `drill_pick` 双击路径 | 消费 DrillMarkers（已声明 false）；drill_pick 在 Topographic 不进钻取 |
| B1 新语义 | EventDots 的 Schematic 格 false→**翻表**；事件标签复用该格（一格管「事件内容层」整体，不新增 pass） | 用户裁定：事件在编辑态隐藏 |
| B2 新语义 | **新 `PassId::RegionLayers`**：把 region_outline / region_overlay 从共享的 TerrainDecor 拆出（Z 槽位不变，MAP_LAYER_RENDERING_SPEC 补条目）；Schematic=false、Topographic=false；区域标签同格 | 用户裁定：区域在编辑态隐藏。**不拆则无法与「装饰 ✓ in Edit」共存**（两者现共享 TerrainDecor pass） |
| C1 翻表 | SeaLabels Topographic false→true + 标签系统接线 | 本次裁定：海名属地貌 |
| T1 色块 | `terrain_view/cells.rs`：Topographic 分支改走新 `shade_biome_raw`（读 biome，查 `BIOME_RAW_COLORS` 固定表；NONE 按 surface 回退）；绕过 TreatmentFlags | v1.2 裁定：群系色块，theme 无关（§2.5） |
| U1 | `inspector/mod.rs`：工作区激活（`cell_workspace_backup.is_some()`）⇒ `tabs(disabled: &[1,2])` | wv-ui tabs 本就支持 disabled |
| D1 | **新 ADR-0051**：翻表两处（SeaLabels/EventDots-Schematic）+ 拆 RegionLayers pass + 「事件/区域内容层编辑态隐藏」语义——按 AGENTS §6.4，翻 ADR-0021 已接受的表语义必须留 ADR | 留痕 |

**不动**：测量层、通用选区高亮（ADR-0033）、paper_overlay、笔刷光标（表已裁 false）、BrushLayer、LabelLayerConfig 的 zoom/time 过滤（正交）。

## 4. 约束

- 渲染层不比较时间（AGENTS §3.2）——本设计全部按 `view.mode` 门，无时间比较。
- wv-render 依赖红线不破；门禁六件套 + 冒烟。
- MODE_TABLE 锚定测试（recipe.rs 内既有）随翻表更新。

## 5. 验收判据

| # | 判据 |
|---|---|
| V1 | CellMap：底图为**群系固定色块**（改一格群系→该格颜色立即变；切主题→颜色不变）+ 等高线 + 河系 + 海名 + cell 高亮；POI/路/势力/事件/区域/装饰/钻入标记全部消失（无格网——表既有裁定） |
| V2 | CellMap 里点地图不吃实体命中；EditCell 点格正常；双击不触发钻入 |
| V3 | 切回 Edit（Schematic）：POI/路/势力恢复；**事件点与区域层不出现** |
| V4 | 切回 Browse（VectorFlat）：全部恢复 |
| V5 | CellMap 下 Inspector 仅「属性」页签可点（另两个变灰） |
| V6 | 命令面板切 Topographic（不经工作区）⇒ 内容层同样消失（门在模式上）；切 Schematic ⇒ 只藏事件/区域 |
| V7 | 播放时间轴时事件标签仍随时间投影（Browse 下）——证明藏的是「层」不是「时间语义」 |
| V8 | CellMap 开 Nature 开关：未命名候选全部金虚线高亮；点候选 → 属性页签出命名表单；命名物化 → 转正三件套 + 落盘；关开关 → 全部消失（Area 区始终不可见） |
| V9 | 开关关闭或未物化时工程目录无新增 regions/*.yaml（D4：未命名不落盘） |
| V10 | BiomeBoundary 工具：拖线身平移、拖端点旋转；松手 = 扫过格群系互换（第三种群系不动）、1 条撤销可退、地图重绘；未命名候选轮廓实时跟随；命名区轮廓不动（手动重生成才更新） |

## 6. builder check 澄清后我定的三处（可翻案）

- **海名保留在 CellMap**：翻 SeaLabels 表格（原声明 false）。海名属地貌呈现，非实体标签。
- **钻入标记/双击钻入在 CellMap 关闭**：表已声明 false，补接线即可；CellMap 中途钻子图也会打断 EditCell 的图绑定。
- **格网不自动开**：MODE_TABLE 已裁定 Topographic 无编辑格（Isoline 分析视图语义）；V1 按此改写，不加自动开关逻辑。
- **Topographic 地形画法改群系色块**（v1.2）：修正 ADR-0039 的「等高分析底图」视觉语义——等高线降为参照层。若你要的是「色块上叠等高线之外还要地形浮雕」，说一声，那是加回 shade 分支的混合模式。
