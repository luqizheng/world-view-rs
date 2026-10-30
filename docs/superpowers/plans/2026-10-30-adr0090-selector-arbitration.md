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

### M2（下一轮）：D2 实体命中（Map z-index 仲裁 + Browse 只读）

- T4 Map 实体命中：按 `LayerFrame` 图层序自上而下（hidden 跳过），LinePoi 激活时
  线 POI 最高；命中 → `SelectedEntity` + 属性卡。Browse 复用同一仲裁去掉编辑入口。

### M3：D4 群系闸门 + 替换式赋势力

- T5 图层面板势力层编辑态 + 「群系」checkbox（默认勾）；Ctrl 多选群系 →
  `ReplaceFactionTerritoryFromBiomes` 新命令（多群系并集替换疆域，走 EditHistory 可撤销）。

### M4：D5 贴近群系

- T6 切蛋糕「贴近群系」checkbox（默认勾）：切线顶点投影吸附群系分界
  （几何源 `biome_boundary` / `BiomeField`）；不勾=自由切（现状）。

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
