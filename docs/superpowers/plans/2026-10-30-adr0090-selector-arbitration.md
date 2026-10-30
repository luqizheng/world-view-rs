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
