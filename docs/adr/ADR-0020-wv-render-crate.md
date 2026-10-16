# ADR-0020：地图渲染拆为独立 crate `wv-render`

**状态**：已接受（Accepted，2026-10-16 用户口头裁定「go」+ 此前会话讨论链「Map render 应该是一个单独的 crate」）
**日期**：2026-10-16
**决策者**：用户（在「地图的各种表达方式，能不能给出建议」讨论链中口头判断「Map render 应该是一个单独的 crate」并以「go」接受本 ADR 落地；纯物理搬迁，不含 Recipe/Treatment 抽象）
**关联**：P1（单机可运行）/ P5（图层化编辑）/ AGENTS §2（架构纪律·分层与依赖方向）/ §3（数据与渲染分离）/ §7.1（测试闸门·app feature 隔离渲染层）/ ADR-0008（Google Default Map 体验）/ ADR-0012（VectorFlat 默认底图）/ ADR-0013（LayerFrame 求值点）/ ADR-0016（古舆图主题·MapPalette 与技法正交）/ ADR-0017（标签层·独立渲染组）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

`crates/wv-app/src/render/**` 当前承载地图的全部渲染逻辑，**15359 行**（约占 `wv-app` 全部代码 40506 行的 **37.9%**）。这一坨代码承担以下三类职责，但**没有任何模块边界**把它们分开：

1. **数据投影**（`world_projection.rs` / `compute_projection`）——只读 `WorldProjection` 经 `TimelineQuery::at` 求值出帧级渲染数据。
2. **绘制实现**（`terrain_view/*`、`vector_layers.rs`、`buildings.rs`、`streets.rs`、`faction_overlays.rs`、`brush_cursor.rs` 等）——基于投影数据生成 Bevy mesh / material / sprite。
3. **图层状态求值**（`layer_frame.rs` / ADR-0013）——kind→渲染组映射、可见性合取、z 槽位偏移。

调研发现这些职责与状态的耦合面：

| 耦合点 | 现状 | 受影响文件数 / 引用次数 |
|---|---|---|
| `crate::state` 引用 | render 模块 23 个文件 `use crate::state::{...}` | 23 文件 / 67 处 |
| `crate::theme` 引用 | render 模块 20 个文件 `use crate::theme::colors/palette` | 20 文件 / 92 处 |
| `crate::edit::editing` 引用 | **反向**：render 直接持有 `EditHistory` 写 | 1 文件（`measurement.rs`）/ 1 处 |
| `wv_edit` 引用 | render 直接构造 `EditCommand` | 1 文件（`streets.rs`）/ 4 处 |
| `wv_overture` 引用 | render 零引用（只在 `overture_cli.rs`） | 0 |
| UI 反向消费 render | 12 个 UI 面板 + 4 个编辑桥文件 `use crate::render::*` | 16 文件 |

**已经发生过的事情**（来自 `git log --oneline` 2026-10 最近 30 条）：

- 海洋色 4 层叠加被 4 个 commit（adaa113 / 3449f6a / a2555b2 / d42d7d2）反复打补丁。
- 远缩放要素被盖 1 commit（ecd510d）在 Z 序硬约束之外的另一条轴上抢救。
- 底图（base）概念从未建模 → 用 `MapBase::cell() = -3500` 反向驱动 `water_shade_at(-3500) = deep_ocean_blue`。
- 「手绘地图呈现」14 项 commit（65b9373→da655a7）每条都在 5+ 模块新增参数。

调研口径：这些 commit **不是独立 bug**，而是同一个架构缺口的多次显现——「地图表达」这个概念从未抽出来。

**测试闸门的不对称**（AGENTS §7.1，2026-10-13 用户裁定）：

- `cargo test -p wv-app`（不带 feature）报告 **0 测试目标**——整个 render 模块在测试层不可见。
- 渲染层必须显式 `cargo test -p wv-app --features app` 才能跑。
- AGENTS §2.3 已经把 `wv-core` 的「独立测试 / 独立跑 CLI」列为强约束——渲染层目前没有对应约束。

**约束：核心七原则**（P1/P2/P3 不动；P5「图层化编辑」强化落点；P6/P7 不动）。核心纪律：分层依赖只向下、禁止 Bevy 渗进 `wv-core`、禁用 `std::fs`、YAML 是 source of truth、ECS 只放投影。

---

## 2. 决策（Decision）

新增平行 leaf-类 crate **`wv-render`**，与 `wv-edit` 同层（均由 `wv-core` + `wv-terrain` 支撑，**互不依赖**）。`wv-app` 改为同时依赖二者。

### 2.1 依赖图（确认 §2 顶层图生效）

```
wv-app    →  wv-edit, wv-render, wv-io, wv-core, wv-terrain, wv-overture
wv-edit   →  wv-core, wv-terrain
wv-render →  wv-core, wv-terrain, wv-overture? + bevy / bevy_egui / egui_extras
wv-io     →  wv-core, serde_yaml
wv-overture → wv-core, serde, serde_json
wv-core   →  serde, thiserror
wv-terrain →  （零依赖，纯算法）
```

> **wv-overture 是否纳入 wv-render 待裁**：`wv-overture` 现状只在 `wv-app/src/overture_cli.rs` 使用（CLI 子命令），render 模块零引用。本 ADR **不**把 `wv-overture` 划入 `wv-render`——它的归属是「CLI 数据导入辅助」，与渲染职责无重叠；下游若要把 CLI 也搬到 `wv-render-cli`（让 `wv-core` 校验器有渲染版对应物），再单开 ADR。

### 2.2 物理搬迁清单（确定动作）

新增 `crates/wv-render/`，**整组搬迁**以下文件，不改任何公开签名、不动任何内部逻辑——这一阶段是**纯机械重构**：

| 源文件 | 新归属（提议） |
|---|---|
| `crates/wv-app/src/render/{camera, layer_frame, map_frame, world_projection, zoom_level, brush_cursor, drill, buildings, faction_overlays, label_collision, label_halo, label_pick, measurement, streets, terrain_chunks, vector_layers}.rs` | `crates/wv-render/src/<name>.rs` |
| `crates/wv-app/src/render/terrain_view/**/*.rs` | `crates/wwv-render/src/terrain_view/<name>.rs`（整组保留子目录） |
| `crates/wv-app/src/render/mod.rs` | `crates/wv-render/src/lib.rs` |
| `crates/wv-app/src/theme/{palette, colors, colors_google, colors_dusk}.rs` | `crates/wv-render/src/theme/<name>.rs`（迁移后由 `wv-render` 重导出，UI 通过重导出继续消费） |
| `crates/wv-app/src/theme/{fonts, ui_tokens, ui_theme, mod}.rs` | **留在 wv-app**（fonts / ui_tokens / ui_theme 是 UI 专属调色，不属渲染） |

### 2.3 状态抽象——只动 render 真实消费的子集（Q1 选项 A 但缩范围）

调研发现 `state.rs` 共 1788 行、`pub struct/enum` 17 项，**只有 9 项被 render 模块 `use crate::state::*` 引用**：

| 状态 | 引用方 | wv-render 内的搬迁动作 |
|---|---|---|
| `WorldProjection` | `world_projection.rs` | **整 struct 搬 wv-render**，由 `wv-app` 在每帧顶端 sync wrapper |
| `World`（在 `WorldProjection` 内） | 投影用 | 留 wv-app，`wv-render` 通过 `*const World` 或 `&World` 接口只读 |
| `LayerFrame` | `layer_frame.rs` | **整 struct 搬 wv-render**（是渲染的输出而非输入） |
| `ViewState` / `BrushState` / `PlaybackState` / `DrillState` / `GridConfig` / `LodTier` / `MeasureState` | 7 个 render 文件直接 `Res<T>` | **留在 wv-app**，定义 trait `RenderInputs: Resource + ...`（详见 §2.4） |

**规则**：被 render 模块消费的 `Resource`，整 struct 跟着搬；不消费的留 wv-app；不能"消费但留在 wv-app"——避免双真相。

**`World` 内部那条引用**（`WorldProjection.world`）：**`World` 留 wv-app**，但 `WorldProjection` 整搬 wv-render。`wv-render::WorldProjection.world` 字段改为 `*const World` 借出只读指针，或新增 `WorldReader<'a>` 类型由 wv-app 在每帧顶端 sync。这是一处独立决策点，本 ADR 倾向 `&World` 接口更稳（Bevy 支持 `Res<'_, World>`），需在落地阶段拍板。

### 2.4 RenderInputs trait（Bevy-native 注入，替代 dyn Trait 抽象）

新增 `wv_render::RenderInputs` trait（在 `crates/wv-render/src/inputs.rs`），提供 §2.3 表中"留 wv-app"的 7 项状态的**只读访问**：

```rust
pub trait RenderInputs: Resource {
    fn view(&self) -> &ViewState;
    fn brush(&self) -> &BrushState;
    fn playback(&self) -> &PlaybackState;
    fn grid(&self) -> &GridConfig;
    fn drill(&self) -> &DrillState;
    fn measure(&self) -> &MeasureState;
    fn lod(&self) -> self::LodTier;          // 平凡 enum clone
}
```

`wv-app` 实现为 `struct AppRenderInputs { view, brush, ... }`，外覆 `Resource` 实现 `RenderInputs`。`wv-render` 各 System 签名从 `Res<ViewState>` 改为 `Res<AppRenderInputs>`。

**为何选 Resource + trait 而非 dyn Trait 注入**：Bevy 的 `Res<T>` 本身就是 ECS 注入机制，写 `dyn Trait` 是脱裤子放屁。`trait RenderInputs` 把"哪些状态是渲染必需的"显式化为接口，让 §2.3 表格的可读性成真。

**为何不直接全部 struct 搬家**：7 项状态中有 4 项（`BrushState` / `MeasureState` / `DrillState` / `GridConfig`）是**UI 面板也消费**的横切状态——若是搬给 wv-render，UI 就要反向依赖 wv-render，违反"应用层持有可工作区"的现有纪律（AGENTS §2.1 wv-app 描述）。

### 2.5 越界写的两处裁决（Q6）

| 文件 | 越界 | 裁决 |
|---|---|---|
| `measurement.rs:556` `mut edit: ResMut<editing::EditHistory>` | render 直接持有 `EditHistory` 写 | 接受先搬迁，**再**用编辑桥（dispatcher）解耦：render 不写历史，只 emit `EditIntent` Resource；`wv-app` 每帧将 intent 转 `EditCommand` 提交给 `EditHistory`。**此修复是本 ADR 的强制伴随动作**，是迁 crate 的"诚实搬迁"前提 |
| `streets.rs:409` `wv_edit::AddStreetSegmentCommand::new(...)` | render 直接构造 `EditCommand` | **同上解法**：render 只构造 `StreetDraft { layer_id, points, ... }` 资源，`wv-app` 在 `edit/streets_actions.rs` 翻译为 `AddStreetSegmentCommand`。这是 §3「不绕 wv-edit 直接改世界」的纪律强化 |

裁决理由：AGENTS §3.3「禁止在 System 中修改世界数据，System 只能生成 EditCommand，交给 wv-edit 执行」——这两处越界在 wv-app 内部是"自己改自己的历史"，但抽 crate 后变成"wv-render 反向依赖 wv-edit"，违反分层纪律。**不修这两处就抽 crate 是制造新的循环依赖**。

### 2.6 主题（Q2 选项 A）

`theme/{palette, colors, colors_google, colors_dusk}` 整组搬 `wv-render`。**`wv-render` 重导出**这些模块（`pub use theme::{palette, colors, colors_google, colors_dusk}`）给 `wv-app`，UI 面板的 import 不变（继续写 `use crate::theme::colors`，仅 `crate` 现在多一层）。

`theme/{fonts, ui_tokens, ui_theme}` **留在 wv-app**——字体打包（ADR-0019 提议态）与 UI 调色板与渲染算法无关。

### 2.7 Cargo 与 AGENTS §2.1 同步

1. 根 `Cargo.toml` `[workspace.members]` 新增 `crates/wv-render`。
2. 根 `Cargo.toml [workspace.dependencies]` 不变—— `bevy / bevy_egui / egui_extras` 已在根定义。
3. `crates/wv-render/Cargo.toml` `[features]` 与 `wv-app` 同（`default = []`, `app = ["dep:bevy", ...]`）——保持「渲染层不污染默认构建」的红线（AGENTS §7.1 后段）。
4. `crates/wv-app/Cargo.toml`：
   - 新增 `wv-render = { workspace = true }`（去 `wv-overture` 之外的全部老依赖）
   - 删除可选 `bevy / bevy_egui / egui_extras / serde_json` 等（迁到 wv-render）
   - `required-features = ["app"]` 的 `[[bin]]` 与 `[[example]]` 保留
5. **AGENTS.md §2.1 同步增列** wv-render 行（细节见 §4 代价）。
6. **AGENTS.md §2.1 依赖图同步**：现有 ASCII 图加一行 `wv-app → wv-render`。

### 2.8 不属于本 ADR 的事项（边界声明）

- **Recipe + Treatment 抽象**（来自「地图的各种表达方式」调研的后续选项）**不在本 ADR**。本 ADR 只做「物理搬迁 + 状态抽象 + 越界写修复」，不重构 render 内部。Recipe 是搬迁之后用独立 ADR（拟 ADR-0021）讨论。
- **`wv-core::terrain` 重新评估是否抽出**——保留现状。
- **OGC / WMS / 在线瓦片等网络 IO**——P1 红线，本 ADR 不触及。
- **`wv-render-cli`（headless 渲染骨架）**——本 ADR 只铺路，不实施；若想建独立 CLI binary（用于 §2.3 提到的"wv-core 校验器有渲染对应物"），再开 ADR。

---

## 3. 理由（Rationale）

### 3.1 为什么必须现在抽（不能再延）

a. **量级**：render 占 wv-app 37.9% 是不争事实；迁完后 wv-app 退回 60%（25200 行）的"应用装配 + UI 面板 + 状态管理 + CLI"职责——这才是「应用层」应承担的规模。

b. **测试闸门的真伪**：AGENTS §2.3 已为 wv-core 立独立测试约束；§7.1 也为 wv-app 渲染层加了一道 `--features app` 闸门——这是「承认有缺口但只在原地止血」的临时方案，根因是「渲染层没有自己的归宿」。抽完 crate，`wv-render` 整套测试可以脱离 egui app shell 跑（`cargo test -p wv-render --features app`），把"应用 fixture 启动才能跑测试"的现状打破。

c. **零碎 commit 的根因止血**：调研确认最近 30 个 commit 中 4 个 commit 群组在打同一个根因（4 层叠加 / 远缩放 / 底图建模 / 手绘呈现 14 项）。根因是「render 内 5+ 模块散落『该不该画』判定」。本 ADR 不解决散落，但**为解决散落创造条件**：Recipe/Treatment 抽象（ADR-0021 拟）的改动面将从"碰 8+ 文件"收窄为"碰 wv-render 内的 3-5 个文件"。

d. **依赖方向的纪律强化**：现状 render 内嵌 `crate::state::*` 直接访问——这是"应用工作区状态被渲染任意读取"。抽 crate 后**强制**这一读取走 `RenderInputs` trait，让"渲染究竟依赖应用工作区的哪些数据"成为有类型的事实表，新增时一眼可审。

### 3.2 为什么是现在**纯物理搬迁**而不是同时做 Recipe

两点：

a. **AGENTS §6.4 不许沉默推翻**：本 ADR 已经够大（依赖图、新 crate、状态抽象、越界写修复、AGENTS 表格更新、ADR 索引更新、bevy feature 重排）——若同时强加 Recipe 抽象，等于把"为什么独立 crate"与"为什么 Recipe"两份独立决策混在一份 ADR 里，将来回头看无法分离讨论。

b. **回退成本的不对称**：纯物理搬迁的回退 = revert 一个 mega-commit；Recipe + 物理搬迁混合的回退 = revert 就要拆语义。**先抽再优化**。

### 3.3 为什么 §2.4 用 RenderInputs trait 而非 dyn Trait / 非"全 Resource 搬"

- `dyn Trait` 与 Bevy 的 `Res<T>` 同构，没必要。
- 全 Resource 搬会制造"UI 反向依赖 wv-render"——`BrushState / MeasureState / DrillState / GridConfig` 是 UI 面板（10 个面板）也消费的横切状态，把它们迁给 wv-render 会让 UI 重新设计。
- 留 7 项在 wv-app + 抽 1 个 trait 接口，是**当前数据流最少改**的解。

### 3.4 为什么 §2.5 越界写必须在本次修复而不是"先搬再补"

因为 `wv-render` 不能依赖 `wv-edit`（应用层该做的事不该渲染层顺手做）。先搬再补，等于"接受一个明知循环的 crate 依赖"——这是 AGENTS §2「依赖方向」红线的违反。**两处越界是抽 crate 的前提而非补充**。

---

## 4. 代价（Consequences）

### 4.1 正

1. **AGENTS §2.3 找到渲染对应物**：render 层有独立测试、独立 CLI 入口（未来）、独立 feature 隔离。
2. **依赖方向诚实化**：render 不再直接持有 `EditHistory`（两处越界 1 commit 修完），强化 §3.3 纪律。
3. **5+ 模块的判定查询化**：`RenderInputs` trait 让"render 需要哪些应用状态"成为有类型的事实表。
4. **测试闸门真正可独立跑**：`cargo test -p wv-render --features app` 与 `cargo test -p wv-app --features app` 解耦，前者不必拽 egui app shell。
5. **为 Recipe/Treatment 抽象铺路**：未来 ADR-0021 的改动面被本 ADR 的 crate 边界天然切窄。

### 4.2 负

| 编号 | 代价 | 缓解 |
|---|---|---|
| ① | **机械重构，diff 巨大**——23 render 文件 + 4 theme 文件搬到新 crate，import 路径全改 | 物理搬迁 commit 一笔独立；行为零变化的断言靠 §5 闸门（cargo test / clippy / fmt / app-test / GUI 烟测）兜底 |
| ② | **`state.rs` 7 项得留在 wv-app**，意味着 §2.4 trait 必须稳——trait 增加方法是一次性投入 | 仅 7 项方法签名；落地时 PR 单独审 |
| ③ | **`WorldProjection.world: *const World` 是非 Bevy 习惯用法**——`World` 是 wv-app 的，所有权属于 wv-app；wv-render 只借只读指针 | 详见 §2.3 末注——落地时拍板（倾向 `&World` 接口更 Bevy-native） |
| ④ | **两处越界写必须修**——`measurement.rs` + `streets.rs` 不能"先搬后改" | 本 ADR 把它们列为强制伴随；新增 dispatcher `Resource` + wv-app 翻译入口 |
| ⑤ | **`LayerFrame` 整搬 wv-render** 后，UI 面板引用 `crate::render::LayerFrame` 要改 `crate::wv_render::LayerFrame` 或走重导出 | 重导出兜底（§2.6 同样手法） |
| ⑥ | **`crates/wv-app/Cargo.toml [dev-dependencies] tower`**——axum 测试 fixture 仍留在 wv-app，http feature 的端到端测试不在 wv-render 管辖 | 无 action；AXUM 是 HTTP/MCP 范畴（ADR-0014），与渲染无关 |
| ⑦ | **AGENTS.md §2.1 表与图必须同步**——6 行表格 + ASCII 依赖图都得动 | PR 同步更新 |
| ⑧ | **docs/adr/README.md §4 + §5** 必须同步增列 | PR 同步更新；新 §5「wv-render 路径对照」只在首次物理搬迁后才有可写内容 |

### 4.3 与既有 ADR 的关系

- **ADR-0007 / ADR-0010 / ADR-0014** 用 `wv_overture` 的方式不变——`wv-overture` 仍平行叶子 crate，不在 `wv-render` 下。
- **ADR-0013**（LayerFrame）：本 ADR 把 LayerFrame 从 wv-app 子模块升为 wv-render 顶层——**§2.4 是 LayerFrame 求值点位置的微调**，ADR-0013 的"唯一求值点"语义不变。
- **ADR-0016**（古舆图主题）：`MapPalette` 整搬 wv-render，`wv-render` 重导出供 wv-app UI 消费——ADR-0016 的"MapPalette 与 ViewMode 正交"语义不变。
- **ADR-0019**（字体打包，提议态）：未触及——`theme/fonts.rs` 留 wv-app。
- **AGENTS §5**（二进制分块纪律）：未触及。
- **AGENTS §7.5 V2**（历史可重写）：未触及——抽 crate 不改变作者操作权限。

### 4.4 文件级搬迁清单（落地前 PR 必须 contain）

- [ ] 23 个 render/ 顶层文件搬到 `crates/wv-render/src/<name>.rs`
- [ ] `render/terrain_view/**`（8 个文件）整组搬到 `crates/wv-render/src/terrain_view/`
- [ ] `render/mod.rs` → `crates/wv-render/src/lib.rs`
- [ ] `theme/{palette, colors, colors_google, colors_dusk}.rs` → `crates/wv-render/src/theme/`
- [ ] `theme/{fonts, ui_tokens, ui_theme, mod}.rs` 留 `wv-app/src/theme/`
- [ ] `crates/wv-render/src/inputs.rs` 新增——`RenderInputs` trait
- [ ] `wv-app` 新增 `src/render_bridge.rs`：AppRenderInputs 实现 + 翻译 dispatcher
- [ ] `wv-app/src/edit/streets_actions.rs`（新增）：streets.rs 越界翻译落地
- [ ] `wv-app/src/edit/measurement_actions.rs`（新增）：measurement.rs 越界翻译落地
- [ ] `crates/wv-render/Cargo.toml`（新增文件）
- [ ] 根 `Cargo.toml` `[workspace.members]` 加 `crates/wv-render`
- [ ] `crates/wv-app/Cargo.toml`：去 `optional = true` 的 bevy 系 deps，加 `wv-render = { workspace = true }`
- [ ] `crates/wv-app/Cargo.toml` 加 `[dev-dependencies] wv-render = { workspace = true }`（让非 feature 测试也用得上 wv-render 的纯算法部分，如 isoline 数学）
- [ ] `AGENTS.md` §2 分层图与 §2.1 表加 wv-render 行
- [ ] `docs/adr/README.md` §4 索引表加本 ADR；§5 路径对照（仅首迁后）补 wv-render 节

---

## 5. 被否决的备选（Rejected Alternatives）

| 备选 | 否决理由 |
|---|---|
| **A. 同步做 Recipe 抽象 + 抽 crate** | 让本 ADR 变成"两个独立决策的拼盘"；违反 §3.2。Recipe 单独 ADR（拟 ADR-0021）。 |
| **B. 仅作名字空间拆分（在 wv-app 内）** | 治标不治本——`cargo test -p wv-render` 跑不起来、`wv-render` 反向依赖 `wv-edit` 仍可发生；调研的 4 组零散 commit 根因仍在原地。 |
| **C. dyn Trait 注入（每 System 一个 dyn）** | Bevy 的 `Res<T>` 本身就是注入，多绕一层 dyn 是脱裤子放屁，且每帧 vtable。 |
| **D. wv-render 直接依赖 wv-edit**（接受越界写先搬不修） | 明知分层循环；违反 AGENTS §2 红线；review 时必驳回。 |
| **E. 全 Resource 搬 wv-render（含 7 项 UI 横切态）** | 让 UI 反向依赖 wv-render——把这 10 个 UI 面板的 import 一并改，scope 远超 render 物理搬迁。 |
| **F. 拆 wv-render + wv-render-theme（theme 与 render 拆两 crate）** | 过度拆。colors / palette 与 cells.rs / atmosphere.rs 是耦合渲染算法的（如 `water_shade_at` 直接读 `palette.soften_color`），把它们拆成两 crate 会引 dyn dispatch。 |
| **G. 同时引入 wv-render-cli（headless 渲染 binary）** | scope 蔓延；本 ADR 只铺路不实施。CLI 是 §2.3 的未来延伸，独立 ADR。 |

---

## 6. 待裁 / 未在本 ADR 拍板的子点

| 子点 | 内容 | 落地阶段 |
|---|---|---|
| `WorldProjection.world: *const World` vs `&World` 接口 | 两种用法都对 Bevy 友好，倾向 `&World` 但需 wv-app 落地时拍板 | wv-render 物理搬迁 PR |
| `World` 自身的依赖方向 | `World` 留 wv-app，但 `WorldProjection.world` 引用指过去——若 wv-app 把 `World` 改成更复杂的所有权（如 `Mutex<World>`）会再次越界 | 与本表第 1 行同步拍 |
| RenderInputs trait 是否暴露 i18n key / 当前 locale | 当前 i18n 只在 UI 消费；本 ADR 不引入 locale trait method | 若渲染需要（如标签字体按 locale 切），再开 ADR |
| `render_bridge.rs` 翻译 dispatcher 的同步频率 | 每帧 vs 每变更——影响性能 | wv-render 落地 PR 配套 benchmark |

---

## 7. 落地闸门

按 AGENTS §7.1 / §10，本 ADR 一旦「已接受」，落地 PR 必须过以下闸门：

1. `cargo fmt --all -- --check` 全绿
2. `cargo clippy --workspace --all-targets -- -D warnings` 全绿
3. `cargo test --workspace` 全绿（无 feature 路径仍要绿——这是验证 render 搬迁没破默认构建）
4. `cargo test -p wv-app --features app` 全绿（验证 wv-app 内遗留代码与 wv-render 通过桥跑通）
5. `cargo test -p wv-app --features app --features http` 全绿（http feature 是 §7.1 后段没特别列、但 ADR-0014 已要求）
6. `cargo clippy -p wv-app --all-targets --features app -- -D warnings` 全绿
7. **GUI 烟测**：`bash scripts/smoke-gui.sh` 跑过（与 AGENTS §10 / §7.1 「GUI 启动烟测必须过一遍」对齐——render 拆 crate 是最容易踩 B0001 一类 run-time panic 的改动类型）
8. **行为零变化**：`render_output/` 下任一截图 hash 与搬迁前一致（P5 metadata 写作"渲染层不应有用户可见行为变化"，除非 trait 引入新 lint 警告否则字节级等同）

8 项闸门全过后，本 ADR 才算落地完成。**任一红灯即回滚 PR**。

### 7.1 落地闸门与 AGENTS §10 的双向引用

| AGENTS §10 检查项 | 本 ADR §7 对应闸门 |
|---|---|
| `cargo test` / `clippy` / `fmt` 全绿 | 闸门 1–3 |
| `cargo app-test` / `cargo app-clippy` | 闸门 4 + 6（同一命令）|
| `bash scripts/smoke-gui.sh` | 闸门 7 |
| `wv-core` 仍零 Bevy 依赖 / `wv-render` 仍不反向依赖 `wv-edit` / `wv-app` | 闸门 2 后必须再跑 §2.2 五条 `cargo tree`，并把输出贴进 PR 描述 |

迁移 PR 描述里必须**显式贴** §2.2 五条 `cargo tree` 的输出（任何一条非空即拒收），理由：分层纪律单靠 review 记忆不客观。

---

## 8. 与七条原则的兼容性复述

| 原则 | 本 ADR 触/未触 | 说明 |
|---|---|---|
| P1 单机可运行 | 未触 | wv-render 不引入网络/在线依赖 |
| P2 目录即工程 | 未触 | wv-render 不引入外部状态/注册表 |
| P3 YAML 为主格式 | 未触 | wv-render 只读 wv-core / wv-terrain 的 YAML 模型 |
| P4 多地图层级 | 未触 | 渲染地图层级是已有 ADR-0009 职责 |
| P5 图层化编辑 | **强化** | RenderInputs trait 把"渲染需要哪些应用状态"显式化；LayerFrame 整搬 wv-render 求值点更纯 |
| P6 非破坏编辑 | 未触 | 抽 crate 不动存储层；越界写修复只是把"渲染构造 EditCommand"翻译为"渲染 emit intent + app 翻译" |
| P7 时间轴驱动 | 未触 | 仍走 wv-core::time::TimelineQuery |

---

## 9. 一句话重述

把 render 38% 这一坨代码从 wv-app 抽出为平行 `wv-render` crate，强制两处越界写修复——纯物理搬迁，零行为变化，零 Recipe / Treatment 抽象——为未来「表达模型抽象」ADR 铺路。
