# ADR-0090 实施计划：三工作区统一选择器仲裁

> 来源：`docs/adr/ADR-0090-unified-selector-arbitration.md`（2026-10-30 落账 24c231e）。
> 执行方式：M1 先行（机械化三件，本轮），D2/D4/D5 每轮一个工作区节奏推进。

## 轮次与任务

### M1（本轮）：D3 + D2 的仲裁减法部分

- **T1 白名单收编**：`MapToolId::available_in`（`crates/wv-app/src/ui_command_palette.rs`）
  `Select(_) => mode == ViewMode::Topographic`；同步白名单注释、白名单测试
  （`Select` 断言从「三工作区」改「仅 Topographic」）。圈选瓷砖在 Browse/Map 灰化由
  既有四处置同源机制自动生效。
- **T2 圈选单击=单格**：CellMap 下圈选工具单击（按下-松开位移 < 4px slop，与
  `pin_drag.rs` 的 `DRAG_SLOP_PX` 同口径）→ 写 `CellSelection` 单格（替换语义，
  Shift=并 / Alt=差，沿「选区加减法」 glossary）。**泛洪裁量**（ADR-0088 D4
  「执行侧裁量」先例，零能力损失、待实机验收）：`FloodCriterion` 增 `Off` 变体并改
  默认值 → 判据 ≠ Off 时圈选单击=泛洪（保住 0088 手感），= Off（新默认）时=单格
  （D3 字面）；「泛洪判据」UI 段的显示条件从「点选激活」迁到「圈选激活」。
- **T3 点选减法**：`commit_point_select_on_click`（`crates/wv-app/src/edit/editing/cell.rs`）
  CellMap 分支删 Alt-单格与默认-泛洪两段 → 点选=图钉>群系区（点空地=不动）；
  浏览族分支删单格段（点选在 Browse/Map 暂为 no-op，等 T4 实体命中补位——**中间态
  能力回退：Browse/Map 点格看属性暂不可用**，已在计划显式登记）。

### M2：D2 实体命中（Map z-index 仲裁 + Browse 只读）——已落（2026-10-30）

- T4 落地范围：**线段 POI 命中进浏览族单击仲裁**（`drill_pick`：点选工具下点中
  河/路折线 = 选中该 Place，`EntityKind::Place` 属性表单同管线；road 层 hidden
  不可选；双击让位下钻；LinePoi 工具激活时其交互系统独占点击——那边对既有线
  的命中优先级最高，D2 的「编辑态最高」由既有 `line_poi_hit` 选中路径承载）；
  **标签层 hidden gate**（label 层关掉时命中表整体不参与）。
- 归属反转（红线驱动）：线命中几何三件（`point_polyline_dist` / `line_pts` /
  `line_poi_hit`）+ `LinePoiState` 四件套（`PendingLinePoi` / `DraggedEnd` /
  `EndpointDrag` / `LinePoiState`）自 wv-app **下沉 wv-render**（drill_pick 要
  读工具态；wv-render 禁依赖 wv-app），wv-app 经 re-export shim 保持旧路径。
- **z-index 全层仲裁的未竟部分**：building / event_point / 势力面（faction
  faces）今日无拾取面，与 M3 群系闸门一起排（届时势力面命中=群系闸门开启态）。
- 执行记录：rebase 到新 main（并行会话推进 9 提交；i18n 593→**609**，合并后
  **612**=609+3；camera_zoom 夹具债由 main 32fa27a 顺手带走）。门禁：wv-render
  428 / app lib 433 / workspace 1431 全绿 + 双 clippy + fmt + smoke-gui PASS。

### M3：D4 群系闸门 + 替换式赋势力——已落（2026-10-30）

- T5 落地（命名按数据模型校正：多选对象是**分区脸**，脸 kind 即群系族——
  ADR-0081「自然蛋糕」的脸本就按群系分界切出，与用户「国家按群系区分」
  同源）：
  - **`ReplaceFactionFacesCommand`**（wv-edit，fork 型快照撤销）：三条替换
    语义——名单内脸易主 / 目标势力原持有不在名单的脸转无主 / 他势力脸不动；
  - **`PartitionTool::Replace` + `PartitionReplaceState`**（目标势力下拉 +
    群系闸门 checkbox 默认勾 + Ctrl 多选集，`replace_pick` 纯函数）；
  - 分区 HUD 加「疆域替换」面板（政治层 only；自然层无疆域不显示）；
  - **图层面板入口**：选中 Influence 行出现「编辑疆域」按钮 → 进分区编辑态
    （Replace 工具）——「图层面板进入势力层编辑态」的落地形态；
  - 手势守卫链与易主同款（播放/UI 抢占/编辑态）+ 群系闸门不勾不选。
- 执行记录：门禁 app 434 / workspace 1433 全绿 + 双 clippy + fmt + smoke PASS；
  i18n 612→**618**（六键）；egui 0.36 Button 无 `.enabled`，用 `ui.add_enabled`。

### M4：D5 贴近群系——已落（2026-10-30）

- T6 落地：**`biome_snap::BiomeSnapIndex`**（wv-world-cell-map 新域模块，零
  Bevy）：构建时逐格标「哪条边与异群系/图外相接」的**锋线边掩码**（O(格数)，
  一笔一次、落笔时建、预览帧零重建）；吸附 = 笔画点投影到半径内最近锋线边段
  （**真半径截断**——盒搜索粗筛 + 距离过滤，均匀区内不强扭笔画）。
  不用显式折线提取的理由：锋线在格数据上是阶梯，一笔切分只要求每个采样点
  各自落在锋线上，不需要锋线的连通序。
- 接线：切刀手势落笔建索引（`CutStrokeSession.snap_index`）→ 采集点与提交
  笔画同走吸附（`stroke_face` 判脸也用吸附后路径）→ 预览 gizmo 所见即所得；
  HUD 加「贴近群系」checkbox（**默认勾**，关 = 自由切现状）；吸附半径 =
  屏幕 12px 折格数（至少 1 格）。
- **有意行为变化**：切刀默认吸附群系锋线（D5 默认勾裁定）；均匀区内无锋线
  可吸时笔画原样（视觉无异）。
- 执行记录：门禁 app 434 / workspace 1435 全绿 + 双 clippy + fmt + smoke PASS；
  i18n 618→**619**（一键）。ADR-0090 全部决策（D1–D6）至此**实施完毕**，
  待实机验收后合并。

## 门禁

`bash scripts/cargo-test.sh` + `cargo clippy --workspace --all-targets -- -D warnings` +
`cargo fmt --all -- --check` + 渲染层 `cargo app-test` / `cargo app-clippy` + 改动触及
手势/系统装配时 `bash scripts/smoke-gui.sh`。

## 原则声明

触及 P5（图层化编辑：hidden 层不可选=图层语义延伸）、P6 不触及（选区是会话态不写世界）。

## M1 执行勘记（2026-10-30，T1 已落、T2/T3 转 M1b）

T1 白名单收编已随分支首提交落地。T2/T3 勘察发现三个计划未载的硬约束，升格为
M1b 独立轮次，避免半换防（T3 单落会造成泛洪能力真空）：

1. **多边形手势冲突**：现状多边形=逐点点击成草稿（`wv-render/selection.rs::
   commit_selection_click`→`handle_click`），「单击无位移=选单格」与之撞——
   M1b 须先把多边形改成**拖拽手绘成环**（拖拽路径=环点），三形状统一拖拽语义，
   单击位让给单格。手势级改动，须实机验收。
2. **写格落点**：`CellSelection` 归 wv-app，圈选手势系统在 wv-render——「单击
   写单格」必须在 wv-app 侧另立系统读呈现态（wv-render 禁反向依赖/写世界红线），
   不能顺手塞进 `commit_selection_click`。
3. **测试面**：T3 牵动 `edit/editing/tests.rs` 13 处 schedule 挂载点（Alt-单格/
   泛洪断言重写），与 T2 同批落。

## M1b 任务清单（下一轮，T2+T3 同批）

- ~~多边形改拖拽手绘（wv-render/selection.rs），三形状统一「拖拽成形」~~
  **2026-10-30 用户裁定改道：不改手势，增「单格」为圈选第四形状变体
  （`SelectionShape::Cell`，与多边形/矩形/椭圆同级）——单击直选一格归属
  显式形状而非手势推断，多边形逐点点击保留，手势冲突消解**；
- ~~wv-app 新系统：单击 slop 内 → 单格~~ → 落地为 `commit_cell_shape_click`
  （`SelectionState.enabled ∧ shape==Cell` ∧ Topographic 才生效）；
  `FloodCriterion` 增 `Off`（**新默认**）→ 判据≠Off 时点击=泛洪（ADR-0088
  手感由判据档位承载，**执行侧裁量待实机验收**）；默认档从 Biome 改 Off
  =有意行为变化；
- 泛洪判据段显示 gate 从 `point_select.tool_active` 迁 `select_env.enabled ∧
  shape==Cell`（brush_panel）+ 判据 segmented 增「关」档（cell_paint_panel）；
- T3 点选减法已落：cell.rs 删 Alt-单格/泛洪/浏览族三段，点选=图钉>群系区
  （点空地不动）；**中间态：Browse/Map 点格看属性暂不可用**（M2 实体仲裁
  接管）；tests.rs 重写 4 + 新增 3 + 删 1（泛洪/Alt 断言迁圈选单格形状）。

### M1b 执行记录（2026-10-30）

门禁：app lib 429/429、workspace 1428/1428、clippy workspace+app `-D warnings`、
fmt、smoke-gui PASS。新增资产：lucide `mouse-pointer-click.svg`（单格形状图标，
与 EditCell 瓷砖的 `square-mouse-pointer` 区分）。i18n 593→596（两处定长哨兵
同步：`ALL` 数组长度 + `seen.len()`）。
