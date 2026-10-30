# 实施计划：ADR-0089 地图交互语法（feat/interaction-grammar）

> ADR：`docs/adr/ADR-0089-map-interaction-grammar.md`（已接受）。
> 验收口径 = ADR §5 六条，逐条映射到下方任务。分支 `feat/interaction-grammar`，基于 main `26dbf3b`。

## 实锚结论（2026-10-30 盘点）

- 双击开属性的既有机制 = `projection.selection = Some(...)` + `panels.visible.insert(Panel::Inspector, true)`（`pin_drag.rs:120-131`，CellMap 图钉已合规）。
- 浏览族双击下钻在 `drill.rs::drill_pick`（双击 → `marker_hit` → `drill.enter`）；标签命中在双击判定**之前**早退（`drill.rs:649-653`）。
- CellMap 进/出编排：`top_bar/bar.rs:288-320`（进=快照面板+开 Brush/Landmark+排 `SelectMapTool(EditCell)`）；`ui_command_palette::exit_cellmap_workspace`（出=还原快照+排 `SelectMapTool(EditCell)` 充当关动作）。
  - **坑**：排 EditCell 充当关动作依赖「进入必开 EditCell」——D5 改排 PointSelect 后该前提失效，退出时 EditCell 已关会撞白名单报错。退出编排改排 `SelectMapTool(PointSelect)`（全关+开默认，一石二鸟）。
- `DrillState::enter` 只进**直接子图**（`state.rs:1236`）；marker id = 子图 id。
- 双击常量散落：`drill.rs` 0.40s/12px、`pin_drag.rs` 0.40s/6px、`selection.rs` RIGHT_CLICK_MAX_PX=6px、`region_select.rs` 4px。
- 测量（`MeasureState`）与 LinePoi（`wv-app/src/state.rs:90`）**没有可调参数**（模式选择在工具条瓷砖）——ADR D4「补参数段」按「无参数不发明」降级为显隐规则文档化（P3：宽度等新数据字段另立待裁，不在本轮）。
- 右键单击取消选中现状仅 nature overlay（`region_select.rs:166`）；通用化时手势让位闸门=圈选激活/测量非 Off/LinePoi 开/边界线在场/EditCell 激活。
- i18n 计数基线 603；新增键须同步 `ALL` 数组与计数断言。

## 任务

| # | 任务 | 落点 | 验收映射 |
|---|---|---|---|
| T1 | 阈值单点常量：新建 `wv_render::input`（DOUBLE_CLICK_SECS=0.40 / DOUBLE_CLICK_SLOP_PX=12.0 统一两处 / DRAG_START_SLOP_PX=4.0 / CLICK_MAX_PX=6.0）；rewire drill/pin_drag/selection/region_select | `wv-render/src/input.rs` + 四处消费 | §5-1 附带 |
| T2 | 默认工具显式化：①`exit_cellmap_workspace` 排 PointSelect；②dispatch `SetViewMode` 浏览档切后排 PointSelect；③top_bar Browse/Map 段与 CellMap 段进**浏览性进入**排 PointSelect（编排不变，不再排 EditCell）；④工程加载后排 PointSelect | `ui_command_palette.rs`、`dispatch.rs`、`top_bar/bar.rs`、`project_load.rs` | §5-4 |
| T3 | 槽3改名「圈选」：i18n 键文案（选择组→圈选，zh+en）；命令面板条目同步；计数不变 | `i18n/table.rs` | §5-6 |
| T4a | `wv_render::PropertyWindowRequest`（Resource，Option<SelectedEntity>）：drill_pick 双击分支改写——标签双击/图钉双击（去 `!is_double` 门）/标记双击→Place(id 相同) 存在则选中+请求，**无 Place 的子图回退 drill.enter**（Interior 等无属性实体保留唯一入口，验收注记偏差）；单像素行为不变 | `drill.rs` + `lib.rs` | §5-2/3 |
| T4b | 消费系统 `open_property_window_request`（wv-app）：取请求→`Panel::Inspector` 可见；注册进 schedules（drill 链后） | `ui_shell/`（或 `edit/`）+ `schedules.rs` | §5-2 |
| T4c | CellMap 格子双击属性：`commit_point_select_on_click` 加双击跟踪（Local），双击=照常仲裁选中+确保 Inspector 可见 | `edit/editing/cell.rs` | §5-2 |
| T4d | 「编辑城市」按钮：`place_form` 在「当前图 children 含 place.id」时画主按钮→置 `CityEditRequest`；消费系统做编排（快照→Brush/Landmark→Topographic→排 `SelectMapTool(Brush)`→`drill.enter`）；编排抽 `ui_command_palette::enter_cellmap_workspace(panels, palette, tool)` 与 top_bar 共用 | `inspector/place.rs`、`ui_command_palette.rs`、`state.rs`、`dispatch.rs`、`schedules.rs` | §5-2 |
| T5 | 右键单击取消选择通用化：新系统（`edit/` 下）——右键 click（CLICK_MAX_PX 内）∧ 无手势闸门 → 清 `projection.selection` + `CellSelection`；退役 `region_select::cancel_selection_on_right_click` | `edit/region_select.rs` 或新文件 + `schedules.rs` | §5-1 |
| T6 | option bar 显隐规则文档化：`bar_visible` 文档改写为「随激活工具有无参数（当前唯一参数族=笔刷；CellMap 海拔滑条=视图参数常驻）」；LinePoi/测量无参数不发明（记待裁） | `top_option_bar.rs` | §5-5 |

## 测试

- drill：双击标记（有 Place）→ 不 enter + 请求在场；无 Place 子图 → enter；双击图钉 → 选中+请求；标签双击 → 请求。
- dispatch：SetViewMode 后 pending=PointSelect；exit_cellmap_workspace 排 PointSelect；CellMap 段进入不再排 EditCell。
- cell.rs：双击后 `panels.visible` 含 Inspector（复用既有 fixture）。
- T5：右键 click 清双选中；拖拽不清；手势激活时不清。
- i18n：哨兵计数（新增键 +1/+2）。
- 门禁：`bash scripts/cargo-test.sh` 全 workspace + `cargo app-test`/`app-clippy` + fmt + clippy + `bash scripts/smoke-gui.sh`（动了 schedules/新 Resource，必跑）。

## 刻意偏差（记入提交与汇报）

1. T4a：无 Place 的子图（Interior 等）双击保留 drill.enter——属性窗体无对应实体，导航是唯一入口；ADR §5-2 验收按「城/场景标记」口径不受影响。
2. T6：LinePoi/测量参数段不实现——两工具现无任何可调参数，发明「宽度」= 数据 schema 变更（P3），列待裁不动作。
3. T2-④：HTTP `ui.rs` 直写 `view.mode` 不排队（agent 通道，palette 不在手）；语法主路径（顶栏/快捷键/命令面板/工程加载）全覆盖。
