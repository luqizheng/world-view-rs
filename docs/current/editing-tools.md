# 编辑工具与交互 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。

## 1. 选择优先工具架构 ⚠️ 含被取代子句

**现行口径**：
- **D2 Pen 方法共享件**：`SelectionState` + `selection` 模块，Figma pen 手感全集；命中优先级 = **锚点 / 边 > 图标下钻 > 空白**；Drawing 态 `drill_pick` 全让位
- **D3 意图目录 v1**：生成大陆（选区喂八参面板）+ 城市（M2）+ 地形四意图（M3 掩膜一键）+ 森林（M4 骑覆盖物 v1）
- **D4 城市 portal 收敛**：多边形只当**位置圈**、**质心落点**、面积定 **16:9 画布**、**自动钻入**
- **D5 两族退役**：`ContinentSketch` / `CityToolState` 退役，撤销回驻留改单槽位让位

- **⚠️ 被取代子句**：**D1「意图生效即选区清空」**（圈区域 → 底部意图菜单 → 意图生效清空）已被 [ADR-0033](/adr/adr-0033-selection-retained-after-intent) **取代**——现行是**意图生效后保留选区**。其余决定（D2–D5）有效。
- **溯源**：[ADR-0025](/adr/adr-0025-selection-first-tooling)（2026-10-17）→ 承接 [ADR-0033](/adr/adr-0033-selection-retained-after-intent)（2026-10-18）
- **关联待决**：§12-6（UI 交互模型：地图上直接编辑 vs 属性面板编辑）· §12-40（大陆生成配方与意图包络工作流）

## 2. 意图生效后保留选区 ✅

**现行口径**：**不切图**的全部意图（大陆 / 地形四意图 / 森林 / 划分势力）成功后**不再主动清空选区**。点空白 / Esc / 切工具 / 切图仍清；删除 `ContinentGenerationStash` / `ContinentGenerationState` / 撤销回驻留判定（选区从未离开）；撤销 / 重做只回滚**世界数据**、**不触碰选区**；城市意图因自动钻入随切图清空。

- **溯源**：[ADR-0033](/adr/adr-0033-selection-retained-after-intent)（2026-10-18，用户选定方案 A）——本文取代 ADR-0025 §2 D1 子句。

## 3. 地图工具条（11 槽 + 分裂按钮 + 地形橡皮刷）✅

**现行口径**：工具条从「一比一平铺 Figma 十三项」改为 **11 槽 + 两组分裂按钮**：
- **选择组**：多边形 / 矩形 / 椭圆；**测量组**：线段距离 / 面积（功能不变只换图标）
- 新增**矩形 / 椭圆选择器**（框选手势，成形时**合成闭合点环**写进 `SelectionState.closed`，下游六个消费点零改动）
- 新增**地形橡皮刷**（`BrushTool::Eraser` **枚举末位追加**，`Brush.exact` 整格直写通道把笔下格子回填成本图 `MapBase` 态：海洋 -3500 m / 陆地 +80 m，缺省兜底 Ocean）
- 画笔 / 生成大陆 / 磁吸三枚瓷砖**退出工具条**（`MapToolId` 变体不删，快捷键与命令面板入口保留）
- 小三角方位：横排 = 右缘居中 / 纵排 = 下缘居中；11 槽等宽等高；网格新增 `G` 键；tooltip 全接 i18n

- **数据层零迁移**（YAML / `MeasurementKind` / `World` 全不动）
- **溯源**：[ADR-0043](/adr/adr-0043-map-dock-split-buttons)（2026-10-20，`ADR-0043-map-dock-split-buttons.md`，两轮实机反馈迭代）

> **追记（2026-10-23）· 测量清理双入口**
> 1. **工具条第 12 项**：测量组右侧新增「清除全部测量」图标钮（槽位编号记 6.5，不重排 ADR-0043 的 0–10 号）；一键清空**当前图**全部测量，走 `ClearMeasurementsCommand` → 一次 `Ctrl+Z` 整体还原；当前图无测量时按钮禁用。命令面板同源条目「清除全部测量」。
> 2. **`Del` 删一条**：渲染层 `delete_measurement_under_cursor` 取光标容差内**最近**的一条测量（与测量橡皮共用 `nearest_measurement` 命中几何），经 `EditIntents` → `RemoveMeasurementCommand` 提交，逐步可撤销。不需要先切测量工具；输入框聚焦（`wants_keyboard`）时让位给文字编辑。
>
> 语义分工：**按钮 / 命令面板 = 清一图**，**`Del` / 测量橡皮 = 删一条**。数据层零迁移。

## 4. CellInspector 单格编辑器 ✅

**现行口径**：`ToolKind::CellInspector` 工具——单格拾取 + 改 cell **三通道**（surface / height / biome）。独立 commit、独立 review；**不触碰** Topographic viewmode（[ADR-0039](/adr/adr-0039-topographic-viewmode)）也**不触碰** 2.5D vertex Z（[ADR-0041](/adr/adr-0041-topographic-2-5d-vertex-z)）。

- **溯源**：[ADR-0040](/adr/adr-0040-cell-inspector-tool)（2026-10-19）

---

## 本域相关的作者操作层铁律

> **校验不是闸门（AGENTS §7.5）**：悬空引用、逻辑矛盾等问题**必须被检出并列出**，但**不得阻止保存、不得阻止编辑**（V1）。作者可以回头改**已发生**事件的参与者 / 时间 / 地点，工具**不阻止、不追问**（V2）。

- **溯源**：`AGENTS.md` §7.5（[规约投影](/specs/agents)）· README 原则六 P6
- **注**：P6 约束的是**存储结构**（区间不覆盖、可回溯），**不是作者的手**——两者层次分明。