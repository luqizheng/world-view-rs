# ADR-0068: 顶栏三段化——Edit 段并入 Map，Schematic 降级「色块画布」

- 状态：**已接受**（2026-10-26，用户裁定「Map 和 Edit 几乎一样」，随
  `wip/cartography-color` 落地）
- 关联：ADR-0021（模式 → 技法矩阵）、ADR-0051（模式内容矩阵）、
  ADR-0061（专业制图 / 顶栏 Map 段）、ADR-0066（单一默认风格——本 ADR
  的直接前因）、ADR-0058（CellMap 统一属性表单）

## 1. 处境（Context）

顶栏原四段 Browse / **Edit** / Map / CellMap 中，`Edit` =
`ViewMode::Schematic`（色块地形）——它的立身之本是「每种地形一色」的十色
对照，让作者一眼读出每格的地形类型。

ADR-0066 单一默认风格落地后，这个立身之本**悄然失效**：

1. `shade_flat` 读主题色板的 `core`，而默认风格的 `core` 已换成
   Google 专业色板——色块模式渲染出来与 `GoogleMap` **同源**（沙/草/丘/山
   折叠为同一近白）；
2. `Schematic` 行的 `TerrainDecor` pass 还开着，但装饰表已被 ADR-0066
   清空——无装饰可画；
3. 氛围 / 纸纹 / 图框三 pass 已于 ADR-0066 追记一全模式退役。

实测截图（`map-vs-edit`）证实：两段只剩「有没有道路 / POI pin」的差别，
底图几乎逐像素相同。同时 `kind_core`（风格层的十色逐格技术色）自
ADR-0066 起**零消费**——「色块对照」的概念只剩一个孤儿常量。

## 2. 决策（Decision）

**D1 · 顶栏改三段 Browse / Map / CellMap**：`Edit` 段移除；段数组与
`seg_disabled` 同步缩为三；CellMap 退出回落目标由 `Schematic` 改为
`GoogleMap`（原回落目标已不在顶栏）。

**D2 · `Schematic` 模式保留，降级为「色块画布」**：不删模式——它是
**笔刷涂地形 kind 的唯一可视反馈**。专业色板把陆上类折叠为近白，在
Browse / Map 里涂「山 vs 草」读不出差异；CellMap（Topographic）只显示
3 阶高度、不显示 kind。`conv_palette_terrain` 增 `Schematic` 分支：
`core` 换成风格层 `kind_core` 十色（纹理烘焙与群系面两条路都经它，一处
改动全覆盖）。快捷键 `2` 与命令面板入口不动（`BROWSING` 顺序不变，肌肉
记忆保留）。

**D3 · i18n 更名**：「色块地形」→「色块画布」（zh / en 同步改字面，
不增删 Key）。

## 3. 理由（Why This）

- **顶栏是工作区，不是模式清单**：Browse / Map 是「看图的两种画法」，
  CellMap 是「改格子的工作台」；Edit 段在装饰与色源双失后已无独立工作区
  语义，留着只会让「四段里有两段长一个样」。
- **为什么不删 `Schematic` 模式**（D2）：笔刷反馈缺口是真实功能——涂
  kind 看不见 = 盲涂。十色画布成本一行换色，且 `kind_core` 从此有了读者。
- **为什么回落改 Map**：退出 CellMap 的语义是「回到看图」；默认看图范式
  是专业制图（ADR-0061）。

## 4. 代价与风险（Cost）

- 顶栏从四段变三段：存量肌肉记忆里「第三段 = Edit」失效（CellMap 移到
  第三位）。快捷键 1–4 不变，可部分兜底。
- `Schematic` 从「顶栏可见的编辑态」降为「幕后画布」：作者可能再也发现
  不了它。缓解：命令面板可搜「色块画布」，笔刷场景（涂 kind 看不清时）
  是自然触达点。
- `kind_core` 十色沿用日间 JRPG 高饱和表：它是**工作对照色**而非呈现色，
  高饱和恰是目的（十档互异）；观感问题不构成回退理由。

## 5. 被否决的备选（Alternatives）

- **`Schematic` 模式整建制退役（8 → 7 模式）**：笔刷 kind 反馈无处安放
  （CellMap 不显示 kind），盲涂不可接受。否决。
- **恢复 Edit 段并还它十色**：与「专业呈现」的方向相反，且用户已裁定
  「几乎一样」——两段近乎相同的顶栏入口是负资产。否决。
- **Map 模式内加「色块叠加」会话开关**：多一个会话轴开关（`view.isolines`
  同款）却只服务笔刷一个场景；独立画布模式更简单、语义更干净。否决。

## 6. 验收（Acceptance）

1. 顶栏三段 Browse / Map / CellMap；CellMap 退出回落 Map；
2. 快捷键 `2` / 命令面板「切换到色块画布视图」仍可达 `Schematic`，
   地形显示 `kind_core` 十色（测试锚点
   `schematic_canvas_core_is_the_kind_distinct_palette`：十色互异 ∧
   与 Map 折叠色不同源）；
3. i18n：命令面板与模式名显示「色块画布 / Palette canvas」；
4. `wv-render` 382 测全绿；`wv-app` / 门禁不回归；smoke-gui PASS。
