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
