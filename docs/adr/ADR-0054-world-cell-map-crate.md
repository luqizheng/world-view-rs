# ADR-0054：抽取 wv-world-cell-map crate——世界级地形工作台独立成 crate（地图工作台 crate 族第一块）

> **状态**：已接受（Accepted，2026-10-23 用户回复「go」按此实施）。**D7 前置已满足**：`feat/terrain-field-p1` 已于同日合入 main（并行会话完成，含 stroke.rs，main 至 `2396ca4`）。
> **日期**：2026-10-23
> **决策者**：用户（2026-10-23 三条考虑 + crate 族命名 wv-world-cell-map / wv-city-map / wv-building-map + 硬约束「不允许引入 theme 的任何 crate」）
> **关联**：[ADR-0020](ADR-0020-*.md)（wv-render 抽出——同法先例）、ADR-0026 / ADR-0043 D1（wv-ui / wv-theme 抽出先例，bevy 非 optional 口径）、[ADR-0039-topographic-viewmode](ADR-0039-topographic-viewmode.md)（CellMap 工作区）、[ADR-0051](ADR-0051-mode-content-matrix.md)（三模式内容矩阵——Topographic 固定色块）、[ADR-0052](ADR-0052-cellmap-terrain-workbench.md)（CellMap 工作台定位与编辑增强——增强实施落点随本 ADR 迁至新 crate）、[ADR-0053](ADR-0053-terrain-gpu-field-rendering.md)（GPU 场管线——P1–P5 已在 `feat/terrain-field-p1` 落地 wv-render，随本 ADR 迁入新 crate）、场景编辑器设计（`docs/superpowers/specs/2026-10-17-scene-editor-design.md`——与 wv-city-map 关系待裁，见 §6-1）
> **原则**：分层纪律（AGENTS §2——本 ADR 修订架构图与红线表）；P5 图层化（PassId / Z 体系不动）；P1–P7 数据语义零触碰

---

## 1. 处境（Context）

### 1.1 用户裁定（2026-10-23）

三条考虑 + 族愿景：

1. **cell map 只关注地形地貌、POI、河流**——对事件、事态、人物不关心，应更偏向地形编辑；
2. **cell map 的颜色是固定的**，不受任何 theme 影响（硬约束：**不允许引入 theme 的任何 crate**）；
3. **cell map 分离出来，要能独立地被 AI 优化**；
4. 命名 **wv-world-cell-map**——后期还有 **wv-city-map**、**wv-building-map**，「因为编辑器可能会不同」。

### 1.2 现状事实（实测代码，2026-10-23，含 field 分支）

**CellMap 渲染实体全部住在 wv-render（main 全 crate 32,151 行）：**

| 件 | main | field 分支新增 | 内容 |
|---|---|---|---|
| `terrain_view/`（11 子模块） | 7,412 | `field/` 8 件 +3,566 | CPU 烘焙（`cells.rs` 2,860 / 图集 / 网格线 / 等值线 / 大气）；field = GPU 场管线（bicubic / coast_sdf / line_sdf / material / pack / paper / symbol_atlas） |
| `assets/shaders/` | — | +770 | `terrain_field.wgsl` 672 / `paper_overlay.wgsl` / `spike_material.wgsl` |
| `examples/` | — | +1,389 | `terrain_field_demo.rs` 1,307（独立验收 demo）/ `r1_spike.rs` |
| `terrain_chunks.rs` | 1,149 | — | 分块 + mip 链 + 视口拣选 |
| `cell_selection.rs` | 665 | — | 选区栅格化 / 高亮 mesh |
| `region_overlay.rs` | 910 | — | 自然区覆盖（CellMap 专属） |
| `biome_boundary.rs` | 256 | — | 群系分界线工具 |
| **合计** | **≈10.4k** | **+5,750**（26 commits） | 抽出后 wv-render 约剩 2.2 万行（main 口径） |

**耦合统计：**

- `terrain_view` 引 `crate::state` 12 处（ViewState / ViewMode / DrillState / GridConfig）；
- 主题取色集中在 `cells.rs`（73 处，`shade_*` 家族）与 `mod.rs` 测试（47 处含测试）；
- wv-render 内约 10 个模块消费 `terrain_view`（brush_cursor / drill / map_frame / vector_layers / feature_overlays / paper_overlay / plugins / recipe / state / selection）；
- wv-app 8 个文件消费 `EditCellState` / `GridConfig`（editing / ui_shell / inspector/cell / map_controls / …）。

**「固定色」已是既成事实**（不必由本 ADR 争取）：`biome_raw.rs` 冻结群系表 `BIOME_RAW_COLORS[21]`（「不收 palette 参数，theme 无关」）；`0600a68`（Topographic 固定色块底图）、`82049ab`（CellMap 零 theme 影响）、`7e0e57b`（关海岸外晕）、`275b253`（区域线白实线）、`b0adb01`（等值线固定深灰）、`1f09336`（pin 模式分流）。

### 1.3 为什么是现在

field 分支已把 ADR-0053 P1–P5 落进 `wv-render/src/terrain_view/field/`——**正是用户要「独立被 AI 优化」的那个域**。它每继续长一节，抽取的搬迁面就大一圈；而 ADR-0052 的编辑增强（hover / scrub / 泛洪 / 平滑）尚未开工，落点若不先定，又会长在 wv-render 里。

---

## 2. 决策（Decision）

**D1 新 crate：`wv-world-cell-map`**（crate 名 `wv_world_cell_map`）。命名把「世界级」编进名字，为族成员 wv-city-map / wv-building-map 留位。**整建制搬迁** = §1.2 表全部：`terrain_view/`（含 `field/`）、三份 shader、两个 example、`terrain_chunks.rs`、`cell_selection.rs`、`region_overlay.rs`、`biome_boundary.rs`，以及 `state.rs` 中的 `EditCellState` 与 `GridConfig` 两个类型（**ViewState 本体不动**，留 wv-render）。

**D2 依赖方向与红线。** 允许：`wv-core`、`wv-terrain`、`bevy`（非 optional）；`bevy_egui` 仅当编辑悬停门控需要 `PointerOverUi` 时引入（brush_cursor 同款需求，引时在此追记）。**禁止**：`wv-theme`、`wv-edit`、`wv-app`、`wv-render`、网络、文件系统。**`wv-render → wv-world-cell-map` 单向依赖**：浏览态主题底图（VectorFlat 纹理 / GPU 场 uniform）消费它的引擎。§2.2 新增断言：`cargo tree -p wv-world-cell-map | grep -E 'wv-theme|wv-edit|wv-app|wv-render'` 应输出空。

**D3 bevy 非 optional**（先例 ADR-0043 D1、ADR-0026）：标准 `cargo test --workspace` 天然覆盖新 crate 单测——「AI 独立优化」迭代时门禁全在默认路径，不给 §7.1 添新盲区。

**D4 固定色口径：色值是数据，不是类型，更不是 shader 常量。**

- 新 crate 自有平面色结构（如 `CellPalette`：纯 `[f32;4]` / u8 数组），**零 wv-theme 类型**；
- CellMap 档默认源 = 内嵌冻结群系表 `BIOME_RAW_COLORS`（`biome_raw.rs` 随迁）；
- `shade_*` 随迁但取色改为 `CellPalette` 入参；`ThemePack → CellPalette` 换算在 **wv-render 侧**（只有它许依赖 wv-theme）；
- `ViewMode` 不进新 crate：crate 自有 `ShadeMode`（地形着色档位），wv-render 负责 `ViewMode → ShadeMode` 映射。GPU 场管线同理——uniform 吃平面色值（ADR-0053 D5/D7 语义不变，uniform 的填充方分为「CellMap 冻结表」与「wv-render 主题换算」两个来源）。**固定体现在默认数据源，不烧进 shader 常量**——否则主题浏览态无法共用同一场管线（备选 E 的死因）。

**D5 边界机制——crate 零 wv-render import。** 视图态经平面数据资源 `CellViewInfo`（ShadeMode、scale、视口、当前图 id、origin、Z 基准）入 crate，由 **wv-render 侧 adapter systems** 每帧从 ViewState / WorldProjection 投影同步；PassId Z 槽位等 wv-render 侧常量同样经入参传入。路径兼容：wv-render re-export 旧路径（`wv_render::state::EditCellState`、`wv_render::cell_selection::*` 等），**wv-app 初期零改动**。

**D6 族定位与克制。** **不建 wv-map-common、不预制「通用工作台框架」**；camera / drill / map_frame / layer_frame / recipe（PassId）/ zoom_level / vector_layers / streets / buildings / 标签 / 测量全部留 wv-render——族模式 = **浏览/呈现居中（wv-render）+ 每层一个工作台 crate**，共享基础设施不进工作台。wv-city-map / wv-building-map 立项时各自开 ADR；届时若出现真共享件再下沉（wv-terrain 是现成的数据层归宿）。

**D7 时序：先合 field 分支，后抽 crate。** `feat/terrain-field-p1` 合入 main 后，抽取一次性把 `field/` 随 `terrain_view/` 整体迁走；**不在飞行中的分支上做路径搬迁**（26 commits 的 rebase 代价远大于搬迁本身）。ADR-0053 正文零改动，P1–P5 落点变更由本 ADR 记录（§6.3 追写纪律）。ADR-0052 正文同样不动，其 D1–D8 编辑增强的实施落点改为新 crate，关系在此登记。

**D8 AGENTS.md 同步修订**（随本 ADR 转已接受执行）：§2 架构图加 `wv-world-cell-map`（wv-render 下游）、§2.1 红线表加行、§2.2 断言列表增补 D2 那条。

---

## 3. 理由（Why This）

1. **定位已锁死**——ADR-0052 §1.1 判据「cell map 唯一目标就是编辑地形」，势力 / 事件 / 人物 / 时间轴一律不进。crate 边界 = 定位的物理化：依赖图替 review 者守门。
2. **固定色已是既成事实**（§1.2 提交链）——抽 crate 只是把「运行时不碰 theme」升级为「依赖图里没有 theme」。
3. **field 管线 5,750 行已是现实**——「独立被 AI 优化」需要一个小而自足、默认门禁全覆盖的域。在 3.2 万行的 wv-render 里优化与在 ~1.6 万行自足 crate 里优化，上下文与回归面天差地别；terrain_field_demo example 随迁后即现成的优化验收靶。
4. **族第一块定模式**——命名、依赖方向、固定色口径、克制边界四件事在第一块钉死，city / building 后续照此模板，各自 ADR。

---

## 4. 代价与风险（Cost）

- 一次性搬迁 ≈ **1.6 万行**（main 侧 10.4k + field 5.75k）；最大机械量在 `cells.rs` 73 处 theme 取色入参化与 wv-render 内 ~10 模块的 import / adapter 改写。
- `CellViewInfo` 每帧投影是新的维护面。缓解：投影函数集中在 wv-render 一处，adapter systems 不含业务逻辑。
- re-export 是过渡债：退役条件 = wv-app 直连新 crate 的 import 改完后删除（不设死期限）。
- 渲染系统级验证口径不变：`app-test` / `app-clippy` / `smoke-gui` 照跑；**搬迁 commit 要求功能零 diff**（纯移动 + import / 入参改写），WV_SHOT 截图对比搬迁前后 CellMap / 浏览态一致。
- 双 ADR 交接面：ADR-0052 的 D1（scrub 控件在 wv-ui）、D6（算子在 wv-terrain）不受影响；ADR-0053 的 D1–D9 决策全部不变，只有代码落点变更。

---

## 5. 被否决的备选（Alternatives）

| 备选 | 否决理由 |
|---|---|
| A. 不抽，wv-render 内模块归堆 | 修不掉「AI 优化域 = 整个 wv-render」；族愿景下 wv-render 只会越长越重 |
| B. 只抽零 Bevy 纯像素内核 | ADR-0052 增强件（hover 系统 / 连续绘制 / 选区 mesh）全是 Bevy 运行时件，内核装不下；域不完整则优化仍要跨 crate |
| C. 新 crate 反向依赖 wv-render（用它的 camera / PassId） | 依赖倒置 = 双子耦合复辟（ADR-0020 红线），且 theme 禁令会被连带击穿 |
| D. 先建 wv-map-common 共享基座 | wv-city-map / wv-building-map 零代码，预制共享 = 过早抽象；第二块真出现时才知道共享面长什么样 |
| E. 固定色烧进 shader 常量 | 与 ADR-0053 D5/D7 冲突——主题浏览态要共用同一场管线，烧死 = 管线写两遍 |

---

## 6. 待裁与登记

1. **wv-city-map 与场景编辑器设计（2026-10-17，已定稿未实施）的关系**：城市编辑器落地时，「场景编辑器即 wv-city-map 本体」还是另立——届时开 ADR 裁决；不裁决前两线不得并进。
2. **POI / 河流薄 pass 迁移时机**：`landmark_symbols`（浏览 / CellMap 双模式共用）与河流线（vector_layers 侧）初期留 wv-render；若 CellMap 工作台需要自足的 pin / 河流表达，作后续小 ADR 或本 ADR 追记。
3. **bevy_egui 是否入列**：见 D2，引时追记。
4. **re-export 退役时点**：见 §4。

---

## 7. 验收口径

- 全门禁：`cargo test / clippy / fmt --workspace` + `app-test` + `app-clippy` + `bash scripts/smoke-gui.sh`；
- §2.2 新断言：`cargo tree -p wv-world-cell-map | grep -E 'wv-theme|wv-edit|wv-app|wv-render'` 零输出；
- 搬迁 commit 语义 = 纯移动（无功能 diff），WV_SHOT 截图对比搬迁前后 CellMap / 浏览态一致；
- `terrain_field_demo` example 在新 crate 下可独立构建运行（AI 优化的验收靶）。
