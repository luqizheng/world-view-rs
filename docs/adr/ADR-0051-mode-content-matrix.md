# ADR-0051：三模式内容矩阵——Browse/Edit/CellMap 各看什么

**状态**：已接受（Accepted）
**日期**：2026-10-22
**决策者**：用户（对话拍板三分法 + 三格补充 + 群系色块 + Nature 开关 + 分界线）
**关联**：**修正 [ADR-0021](ADR-0021-render-recipe-and-treatment.md) 两格表语义与 [ADR-0039](ADR-0039-topographic-viewmode.md) 的 Topographic 视觉语义**（正文均不改，头部标注）；[ADR-0017](ADR-0017-label-layer.md)、[ADR-0030](ADR-0030-poi-rank-and-density-budget.md)、[ADR-0045](ADR-0045-nature-region-model.md)、[ADR-0050](ADR-0050-unified-inspector-panel.md)（同分支前置）、[设计稿](../superpowers/specs/2026-10-22-mode-content-matrix-design.md) v1.4

---

## 1. 处境

顶栏三段（Browse/Edit/CellMap）此前**只换底图**：内容层（POI/路网/标签/势力/事件点/钻入标记）在所有模式全画。ADR-0021 的 `MODE_TABLE` 早已把 Topographic 行声明成「内容全关」，但六类系统未消费该表（declared-but-unconsumed）。用户拍板三分法：**Browse 查阅+登记，Edit 关注势力/POI（事件与区域因有时间性而隐藏），CellMap 只关注自然（地形地貌），且是最底层的地形编辑器**——格子用固定群系色显示，不随主题。

## 2. 决策（Decision）

1. **接线 MODE_TABLE**（不散布 `view.mode` if）：六套标签系统、街网、道路、POI、事件点 gizmo、钻入标记全部消费既有表。标签门走「wanted 置空 → 既有隐藏尾部」（池是持久实体，跳过注册会冻结残影）；`hits.clear()` 归链首照常跑。
2. **Schematic 翻 EventDots=false**：事件有时间性，编辑态隐藏（用户裁定）。Browse 保持 true。
3. **拆 `PassId::RegionLayers`**（COUNT 24→25）：区域轮廓/底色/标签从与地形装饰共享的 TerrainDecor 拆出——Schematic 下「装饰 ✓（地貌演绎）而区域 ✗（作者内容）」必须两格独立。各行取值 = TerrainDecor 同值，唯 Schematic/Topographic=false；GoogleMap/Event/Coastline 对齐 TerrainDecor=false。
4. **SeaLabels Topographic 翻 true**：海名属地貌呈现（非实体标签），CellMap 保留。
5. **群系色块底图**：Topographic 地形画法改为 `shade_biome_raw`——`BIOME_RAW_COLORS[21]` 固定色板（同族同色相 + 编号明度阶梯），**不接收任何 palette 参数**（编译期保证 theme 无关）；biome=NONE 按 surface 回退；高度不参与颜色。修正 ADR-0039「等高分析底图」视觉语义（等高线降为可关参照层）。
6. **CellMap 工作区 Inspector 只留属性页签可点**（势力层/事态禁用，`tabs(disabled)`）。
7. **Nature 开关 + 候选 + 分界线**（T10–T12，同 ADR 后半）：map controls 的 Nature 开关放行 Topographic 下 `kind == Nature` 的区域层（Area 区仍藏）；全图候选枚举（指纹缓存）；点击候选命名（命名即物化，D4）；**群系分界线工具**——`ToolKind::BiomeBoundary`，可拖线段两侧各绑一群系，拖动扫过格「退让侧→推进侧」互转（第三种群系不动），一次拖动 = 1 条 `BatchEditCellsCommand`，线是会话工具不落盘。

## 3. 理由

- **接线而非散布 if**：ADR-0021 的表就是「模式观感集中地」；declared-but-unconsumed 本身是误导（vector_layers.rs 留有「后续接」TODO）。新语义也进表（翻格/拆 pass），保持单一事实源。
- **拆 pass 而非图层开关**：装饰与区域共享 TerrainDecor，Schematic 的「装饰 ✓ 区域 ✗」在共享格下不可表达。
- **固定色板而非主题色**：数据视图的语义是「看数据」——主题色是「看世界」的（Browse）。固定色板让编辑器在任何美术主题下稳定可辨（先例：ADR-0045 追记一固定诊断色）。

## 4. 代价

- MODE_TABLE 行长 25，后续每加 pass 全行补格（锚定测试把守）。
- CellMap 无森林装饰符号（TerrainDecor=false 既有裁定）——用户确认：符号延后（前三轮程序化符号观感不佳），待方案落地后另行补充。
- Schematic 下事件点/事件标签/区域层消失——依赖它们做编辑参照的工作流改为切 Browse。
- 分界线拖动后命名区边界不自动重生成（D6 快照语义，手动重生成）；自动重生成列后续。

## 5. 被否决的备选

| 备选 | 否决理由 |
|---|---|
| 每系统散布 `view.mode` 早退 | 重散 ADR-0021 集中的模式逻辑；declared-but-unconsumed 继续误导。 |
| 区域层用 LayerFrame 开关而非拆 pass | LayerFrame 是声明轴（作者 YAML），模式观感是模式轴——两轴正交（ADR-0021 §2）。 |
| 群系色板进 ThemePack | 用户明裁定「颜色固定不受 theme 影响」；数据视图要跨主题稳定。 |
| 前锋线语义（拖过全染成目标群系） | 用户选 B 分界线：两侧各绑一群系、只互相转换——水/山不被误染。 |
| 分界线落盘为区域几何 | 真相仍是 cell.biome（P3/P7）；线是会话工具，区域轮廓继续由连通性派生。 |

## 6. 追记（2026-10-23 实机验收追加裁定）

实机验收 CellMap 两处残留修饰，用户裁定去除：

1. **Topographic 翻 `Isoline=false`**（推翻 ADR-0039 决策 7「等值线分析视图
   默认开」）：海岸等值线在数据视图里呈现为「海面上的黑线」，属修饰线，
   与「反映方格状态」冲突。分析参照语义回归 VectorFlat（全仓唯一开档）。
   连带清理：`isoline_overlay` 的 cellmap 固定色/关晕补丁整体退役（被翻表
   取代，零 theme 诉求由「不画」达成）。
2. **地物符号退役城堡/小镇插画，统一 map-pin**：`LandmarkKind::Citadel/Town`
   的手绘水彩 PNG 贴图路径（`ensure_landmark_png_loaded` + Sprite 池）与
   程序化插画构建器（`push_citadel`/`push_town`）整体删除，改为单一
   `push_map_pin`（圆头 + 尖尾 + 白芯，Lucide `map-pin` 实心化，深墨 +
   白芯常量色）；门控从 `layers.place.visible` 补接
   `MODE_TABLE(FeatureOverlay)`——Topographic 下地物符号不再绘制
   （此前 FeatureOverlay=false 已声明但未消费，CellMap 里城堡照画的缺口）。
   等级语义由标签层承担，符号层不区分 Citadel/Town。

代价：`wv-render/assets/symbols/` 资产与 PNG 解码路径删除（include_bytes
二进制不再入库）；ADR-0042 的「参考图城堡插画」口径由 map-pin 取代。

### 追记修正（同日第二轮）：等值线不删死，改会话开关

§6-1 的「Topographic 翻 `Isoline=false`」被同日用户追加裁定修正为**会话开关**：
MODE_TABLE 两行（VectorFlat / Topographic）恢复 `true`（模式轴开档不动），
新增 `ViewState.isolines`（**默认关**）在等值线系统单控会话轴——与 GridLines
同款「mode 表不关，会话开关单控」口径。开关 UI 在 map controls 网格线隔壁
（Lucide `waves` 瓷砖，仅两开档模式显示）。VectorFlat 默认观感随之少一层
等值线——属本条裁定的直接推论（等值线默认是修饰线，要时再开）。

### 追记修正（同日第三轮）：pin 只用于 CellMap + mesh 尺寸 bug

§6-2「两类符号统一 map-pin」被实机验收推翻，修正为**模式分流**：

1. **map-pin 只用于 CellMap**：Topographic 下地物符号画 pin（数据视图的
   地点标记，独立于 FeatureOverlay 门——该行此格仍为 false，主题地界覆盖
   层照旧不进 CellMap）；其余 FeatureOverlay 开档模式恢复城堡/小镇插画
   （`push_citadel` / `push_town` 从历史找回）。place 图层关则都不画。
2. **mesh 尺寸 bug**：符号绘制把「画布基准 26px」乘进了缩放系数
   （`s = 26 × view_scale`），而局部坐标单位本就是屏幕像素 → 符号放大
   26 倍，Browse 出现盖住半个大陆的巨型墨团（白圈 = pin 白芯）。修正为
   `s = tier(1.0/1.25/1.5) × view_scale`。旧 PNG Sprite 路径无此 bug
   （`custom_size` 只用 side_wu），mesh 兜底路径生产环境从未走过，故
   PNG 退役后才暴露。`BASE_H_PX` 常量随之退役。
