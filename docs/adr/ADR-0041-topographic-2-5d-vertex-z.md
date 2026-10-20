# ADR-0041：2.5D vertex Z 挤压——`terrain_2_5d_enabled` 落地（仅 Topographic mode 启用）

**状态**：已接受（Accepted）
**日期**：2026-10-19
**决策者**：用户（对话拍板"开工"+「最 detail」是 CellInspector 的工作定位）
**关联**：[ADR-0039](ADR-0039-topographic-viewmode.md)（Topographic viewmode；本 ADR §「决策 6 字段预埋」已在该 ADR 落地，本 ADR 实现该字段）、[ART_STYLE_SPEC §1.1](ART_STYLE_SPEC_v0.1.md)（「斜投影俯瞰 2.5D oblique top-down,垂直方向 0.5× 压缩」规格锚点）、[MAP_LAYER_RENDERING_SPEC §0/2.3](MAP_LAYER_RENDERING_SPEC_v0.1.md)（四轴契约 + PassId 列表）、[ART_STYLE_SPEC §1.5](ART_STYLE_SPEC_v0.1.md)（光影"主光左上 45°"对 z 挤压方向约束）、[buildings 2.5D 先例](../../crates/wv-render/src/buildings.rs)（`extrude_dir_points_to_world_lower_right`）

> **配套说明**：本 ADR 锁 2.5D vertex Z 挤压。配套两条已上锁的 ADR：
> - [ADR-0039](ADR-0039-topographic-viewmode.md)——Topographic viewmode（已接受）
> - [ADR-0040](ADR-0040-cell-inspector-tool.md)——CellInspector 工具（已接受）
>
> 三件并行落地，分别 PR。本 ADR 实现 ADR-0039 §「决策 6」预留的 `terrain_2_5d_enabled` 字段；落地后该字段在 Topographic mode 下默认开，其他 mode 默认关。

---

## 1. 处境（Context）

[ADR-0039 §1](ADR-0039-topographic-viewmode.md) 已落 Topographic viewmode：用户「看到高度」诉求当前仅由「3 阶高度着色带」满足；用户在同一对话轮里追加「是否启动 3D」的子诉求——希望 Topographic mode 下高度能**立体感**地呈现，而不是仅靠色阶暗示。

### 1.1 现状能力盘点

- **当前架构**：`wv-render` 是 `Camera2d` + `OrthographicProjection` 纯 2D 投影（[recipe.rs](../../crates/wv-render/src/recipe.rs)、[state.rs](../../crates/wv-render/src/state.rs)）；
- **当前 TerrainChunk mesh 形态**：[terrain_chunks.rs](../../crates/wv-render/src/terrain_chunks.rs) 重 chunk-rebuild 时 `height` 字段**只进入 material 着色梯度**，**未进入顶点 Z**（mesh 仍是纯 2D quad）；
- **2.5D 已有先例**：[buildings.rs](../../crates/wv-render/src/buildings.rs) `extrude_dir_points_to_world_lower_right`——建筑已有 2.5D 投影方向锚点（左上方 45° 主光，阴影右下）；本 ADR 与该先例**保持同款投影方向**；
- **ART_STYLE_SPEC §1.1 现状**：规格已钉「垂直方向 0.5× 压缩」（[ART_STYLE_SPEC_v0.1.md §1.1](ART_STYLE_SPEC_v0.1.md)），是**规格已存在但代码未落地**的目标；
- **ADR-0039 §「决策 6」**：已在 ViewState 预埋 `terrain_2_5d_enabled: bool` 字段，本 ADR 实现该字段。

### 1.2 「启动 3D」与本 ADR 的关系

`[A、B、C、D]` 四档分级（用户语境原文，本对话轮给出）：

| 档 | 名 | 本 ADR 落点 |
|---|---|---|
| **A** | 纯 2D 着色（3 阶高度） | 已由 [ADR-0039](ADR-0039-topographic-viewmode.md) 实现 |
| **B** ⭐ | **2.5D vertex Z 挤压**（mesh 顶点 `z = height × compression`） | **本 ADR** |
| **C** | 2.5D 画法（沿 buildings 模板逐 chunk 手绘顶面+侧面+阴影） | 否决，理由见 §4.3 |
| **D** | 真 3D 透视（`Camera3d` + `Perspective`） | 否决，理由见 §4.3 |

**用户上一轮拍板「A + B 组合」**，本 ADR 即 B 部分。

---

## 2. 决策（Decision）

**全部动作汇总：**

1. **mesh 顶点写 z**：`terrain_chunks.rs::rebuild` 在生成每格顶点时，增加 `z = cell.height as f32 × compression × effective_scale`，其中 `effective_scale = ` `ViewState.scale × cell_size` 的组合决定的「世界坐标 y 单位/像素」，**z 与 xy 一致使用 OrthographicProjection 的尺度单位**——保持相机 OrthographicProjection 不变，**仍是 2D 投影，不引入 3D 透视**。

2. **`compression` 默认值 = `0.3`**（不是 ART_STYLE_SPEC §1.1 钉的 `0.5×`）。理由：
   - `0.5×` 适合小尺度（城市子图，32× 起步）；
   - **本 ADR 默认服务长途跨度地形**（如 guangdong 工程 50 km 量级，参考资料），`0.3×` 更平，避免山峰过度耸立遮挡背后内容；
   - 用户可在 ViewState 子参数 `topographic.compression: f32`（范围 `[0.0, 1.0]`）临时调节，默认初始值 `0.3`；
   - 范围上限 `1.0` 而非 `2.0`：compression > 1 让画面像建筑而非地形，与「Topographic mode」的语义不符。

3. **`terrain_2_5d_enabled` 字段语义落地**（继承 [ADR-0039 §「决策 6」](ADR-0039-topographic-viewmode.md)）：
   - `wv-render::state::ViewState.terrain_2_5d_enabled: bool`，默认 `false`
   - **`resolve_render_recipe` PreUpdate 阶段**：当 `view.mode == Topographic && view.terrain_2_5d_enabled == false` 时，自动设为 `true`（用户进 Topographic 自动开）
   - 其他 7 个 mode：**默认 `false`**，用户手动设也无效（mode-tab mode 强约束；与 `tracks_visible` 等会话布尔不同，`terrain_2_5d_enabled` 是 mode-coupled 开关）
   - 用户在 Topographic mode 内手动设 `false`：**允许**（"我只想看 2D 着色"是合法诉求）

4. **联动的渲染元素**（决策 1 的 mesh 顶点改 z 后，下面这些必须同步）：
   - **浮雕**：[terrain_view/rebuild.rs](../../crates/wv-render/src/terrain_view/rebuild.rs) 的 terrain-relief shading 沿 `z = cell.height × compression` 平移采样的像素坐标——视觉上浮雕与顶点同步隆起
   - **等值线**：[terrain_view/isoline_overlay.rs](../../crates/wv-render/src/terrain_view/isoline_overlay.rs) 的 isoline 描边 `z = ISOLINE_Z + offset + cell.height_at_pixel × compression`——线条平移到对应高度位置
   - **笔刷光标**：[brush_cursor.rs](../../crates/wv-render/src/brush_cursor.rs) 的 `Transform::from_xyz(...)` 沿 `z += max(brush.height_at_center × compression, 0)`——光标圆落点抬升到笔刷中心格的顶点 z
   - **选中格**（[ADR-0040](ADR-0040-cell-inspector-tool.md)）：`PassId::CellSelection` 的选中框 `Transform.z += cell.height × compression`——选中 1 格浮在对应高度位置
   - **测量叠加**：[measurement.rs](../../crates/wv-render/src/measurement.rs) **不联动**——测量是"最上层"的会话约束（[MAP_LAYER_RENDERING_SPEC §4 例外 ②](../../crates/wv-render/src/MAP_LAYER_RENDERING_SPEC_v0.1.md)）

5. **联动只在 Topographic mode + `terrain_2_5d_enabled == true` 时生效**：其他 mode 维持原 2D 平铺，零开销

6. **chunk rebuild 触发条件扩展**：除了既有的 `data_hash` 变化外，新增 `terrain_2_5d_enabled` 切换 / `compression` 变化也是 rebuild 触发——避免 z 切换时画面仍 2D 滞后。[isoline_overlay.rs](../../crates/wv-render/src/terrain_view/isoline_overlay.rs) 的「纯换色快路径」保持不变——z 不变时仍走快路径（hash 含 compression）。

7. **数据层零变化**：`TerrainCell.height` 字段不动（仍 i16 米单位）；`terrain.bin` 字节布局不变；`wv-io` 迁移表不变。`compression` 仅是渲染层 ViewState 子参数，**不进入工程 YAML**。**符合 P3、P6、P7**。

8. **camera 不变**：`OrthographicProjection` 不变；`Camera2d` 不变；**不引入 `Camera3d` / `Perspective`**。这是 B 档与 D 档的根本区别。

9. **不与 [ADR-0040](ADR-0040-cell-inspector-tool.md) 联动成本由 CellInspector 承担**：选中 1 格的 z 平移走决策 4 的「选中格联动」分支——这是给 CellInspector 的天然互操作。

10. **performance 闸**：[terrain_chunks.rs](../../crates/wv-render/src/terrain_chunks.rs) 的 rebuild 路径按 `compression` 触发一次重 mesh 后稳定；运行期每帧不变；与现有 23 个 PassId 同款「数据 hash 没变就跳过 rebuild」的 lazy 模式保持。**不**引入 per-frame 重建。

---

## 3. 理由（Rationale）

- **架构归位**：[ART_STYLE_SPEC §1.1](ART_STYLE_SPEC_v0.1.md) 已把"2.5D oblique top-down, vertical 0.5× compression"作为规格写下——本 ADR 把规格从纸面落到代码，**符合 AGENTS §6.3「文档追写不改机制」**。本 ADR 与 ART_STYLE_SPEC 不是冲突，是滞后落地。
- **compression 选择 0.3 而非 0.5 的理由**：长途跨度地形（guangdong 级）在 0.5× 下山峰高度可能溢出视口，造成「看不见背后内容」；0.3× 是「仍能感知高度，但不会遮挡后续地形」的妥协值。0.5× 保留给后续小尺度子图（城市级）的精细地形渲染，由 `topographic.compression` ViewState 子参数覆盖。
- **camera 不变是真 3D 与 2.5D 的边界**：D 档（真 3D 透视）会撕开 wv-render 投影栈（label billboard、笔刷光标、子图对位、PoI 选区）；B 档用 vertex z 切入但不真转 3D，**子图对位仍 2D 正交**，仅 mesh 内部多了 z 维度——这个区别是 B 档能安全落地的**核心论据**。
- **与 [ADR-0039](ADR-0039-topographic-viewmode.md) 的字段预埋衔接**：决策 6 字段已由 ADR-0039 加入 ViewState，本 ADR 落地该字段实际行为；不在 ViewState 加新字段，避免 ViewState schema 的二次迁移。
- **联动渲染元素清单（决策 4）继承 [ART_STYLE_SPEC §1.5](ART_STYLE_SPEC_v0.1.md)「主光左上 45°」**：vertex z 挤压方向沿 ortho +y 上（即朝上），与 isoline / brush_cursor / cell_selection 的 z 平移方向同款——保证「高度感」与「光向感」不打架。
- **为什么 measurement 不联动**：[MAP_LAYER_RENDERING_SPEC §4 例外 ②](../../crates/wv-render/src/MAP_LAYER_RENDERING_SPEC_v0.1.md) 已锁「测量叠加有意不随 z_offset 漂移——用户原话『最上层』的强约束」；本 ADR 沿用同款约束。
- **「data_hash 联动 + 模式切换触发 rebuild」** 路径沿用 [isoline_overlay.rs:172 「纯换色快路径」](../../crates/wv-render/src/terrain_view/isoline_overlay.rs)，避免重复设计。

---

## 4. 代价（Consequences）

### 4.1 负面后果（必须承认）

1. **首次进入 Topographic mode 时 chunk rebuild O(N)**：与 [ADR-0039 §4.1.1](ADR-0039-topographic-viewmode.md) 同款——大图（1024²）约 20 ms；项目已有「> 16,777,216 格拒绝加载」闸。
2. **compression 切换需 chunk rebuild**：每次用户拖动 `compression` 滑条即触发 rebuild，频繁拖动会卡顿——这是 v2 可考虑「rebuild 节流」的位置；本 ADR 不做（[§4.2](#42-后续可能不构成本-adr-决策) 留口）。
3. **compression 默认值 0.3 与 [ART_STYLE_SPEC §1.1](ART_STYLE_SPEC_v0.1.md) 0.5 不一致**：规格钉 0.5，本 ADR 默认 0.3。**这是规格与本 ADR 的有意偏差**——理由记 §1.1「适合长途跨度地形」。后续如要让 §1.1 改默认 0.3，应在 §1.1 加一行说明保持轨迹（注：本 ADR 不修改 ART_STYLE_SPEC，由用户决定要不要在那里追记）。
4. **联动渲染元素清单（决策 4）若漏一项，画面会出现「浮雕隆起但笔刷光标不抬升」的不一致**：写代码时必须严格按清单走 review；写 §6 落点表钉死。
5. **真 3D 不在本 ADR**：用户若之后期待「我要旋转地图看 3D 远景」——本 ADR 给不出（D 档才会）。README §12 待决可加一条「3D 旋转视图——项目期内不做」。
6. **compression 不支持 > 1.0**：是设计选择，不是 bug；但用户可能吐槽「为什么不能更高」——UI 可加 `0.0–1.0` 滑条范围限制，不需额外解释。
7. **跨子图（drill）时父图的 2.5D 与子图地形不在同一 z 平面**：父图 vertex z 是父地形；进入子图后 cell.height 全新计算，z 重排；这不构成 bug，但子图进入时第一次 rebuild 比父图慢——已知可接受。

### 4.2 后续可能（不构成本 ADR 决策）

- **compression 滑条 UI**（顶栏下拉旁）：若用户拖动频繁卡顿，反馈进来后开 v2 加 rebuild 节流（`compression` 变更累积 100ms 后再触发 rebuild）。
- **compression 自适应（小尺度子图用 0.5×,长途用 0.3×）**：通过 [DrillState.zoom_level](../../crates/wv-render/src/state.rs) 联动自动设。当前可手设，留口不主动做。
- **真 3D 旋转投影**（D 档）：明确不在本 ADR 范围；若用户后续强烈诉求，开 ADR-0042+ 走 [ADR-0020 wv-render-crate](../../crates/wv-render/src/) 的 3D 重构路径——但需先评估 wv-render 投影栈重构代价。
- **compression 与 [ART_STYLE_SPEC §1.1](ART_STYLE_SPEC_v0.1.md) 0.5 的统一**：若用户实机在长途跨度也想要 0.5，§1.1 可改写一行说明「长途地形建议 0.3」，并把规格默认值改 0.3——这是后续规格追记。

### 4.3 被否决的备选方案

| # | 备选 | 否决理由 |
|---|---|---|
| A | **复用 [ADR-0039](ADR-0039-topographic-viewmode.md) 纯 2D 着色，不加 Z 挤压** | 不响应用户「看高度立体感」的子诉求；规格 [§1.1](ART_STYLE_SPEC_v0.1.md) 长期悬而未落 |
| B | **真 3D 透视 + Camera3d + Perspective**（D 档） | 撕开 wv-render 投影栈——label billboard、笔刷光标、子图对位、PoI 选区全要适配 Camera3d；远超本 ADR 范围 |
| C | **2.5D 画法（沿 buildings 手绘顶面+侧面+阴影模板）** | 地形象素倍增（比建筑量大几个数量级），性能不可行；与「Topographic mode」分析视图语义不符 |
| D | **compression 默认 0.5**（照 [ART_STYLE_SPEC §1.1](ART_STYLE_SPEC_v0.1.md) 钉） | 长途跨度下 0.5 会造成「山峰遮挡背后」——guangdong 级工程已经验证（参考 Guandong zoom FPS attribution） |
| E | **compression 默认 1.0** | 把顶点 z 当"建筑高"，画面不像地形像城市场景；与 Topographic 语义不符 |
| F | **联动所有渲染元素（含 measurement）** | 测量叠加的「最上层」语义会破——用户测量时希望 line 总在最上层（[MAP_LAYER_RENDERING_SPEC §4 例外 ②](../../crates/wv-render/src/MAP_LAYER_RENDERING_SPEC_v0.1.md)） |
| G | **在所有 mode 默认开启 `terrain_2_5d_enabled`** | 子图对位 / label billboard 在其他 mode 下没适配 z 平移；当前仅 Topographic mode 全套已配套 |

---

## 5. 涉及与不涉及（AGENTS §10 自检）

**触及**：

- §6.4（本 ADR 即留痕）
- §10「依赖方向」：本 ADR 全在 wv-render；不跨 wv-core / wv-terrain / wv-edit 边界
- [§2.2 验证方式](../../AGENTS.md)：wv-core / wv-terrain / wv-edit 仍零 Bevy 依赖

**未触及**：

- §1 P1–P7 全部：
  - P3（terrain.bin 字节布局不动）
  - P4（子图对位仍 2D ortho，未撕多地图层级）
  - P6（仍每格 single-shot EditCommand，撤销栈不破坏）
  - P7（时间轴仍是 ortho 投影，未撕时间维度）
- §2.1 依赖方向
- §3.3 不直接改世界（系统层仍只产出 EditCommand）
- §7.5 V1/V2（与 cell 编辑无交叉）
- §8 代码规范：compression constant 在 wv-render 顶层 const，符合命名与文档约束

**触发但不冲突：**

- 配 [ADR-0039](ADR-0039-topographic-viewmode.md)：本 ADR 落地 ADR-0039 §「决策 6」预埋字段
- 配 [ADR-0040](ADR-0040-cell-inspector-tool.md)：本 ADR 决策 4「选中格联动」分支自动给 CellInspector 选中高亮附带 z 平移——两者**正交**，可分别 PR

---

## 6. 提议落点（仅 paper，不写码）

```
crates/wv-render/src/terrain_chunks.rs            :: rebuild mesh 顶点写 z = height × compression
crates/wv-render/src/terrain_view/rebuild.rs       :: 浮雕采样的 z 平移
crates/wv-render/src/terrain_view/isoline_overlay.rs :: isoline 描边的 z 平移 + 数据 hash 含 compression
crates/wv-render/src/brush_cursor.rs               :: 笔刷光标 z 平移
crates/wv-render/src/measurement.rs                :: 不联动（沿用现有约束，零改）
crates/wv-render/src/state.rs                      :: terrain_2_5d_enabled 字段语义落地；topographic.compression 子参数
crates/wv-render/src/recipe.rs                     :: resolve_render_recipe 增加 mode-coupled 自动 on（Topographic → true）
crates/wv-render/src/ui/cell_selection_render.rs   :: （[ADR-0040](ADR-0040-cell-inspector-tool.md) §6 已开）选中框 z 平移
crates/wv-render/src/view_recipe_consts.rs (或同名) :: compression 默认常量 0.3
crates/wv-render/src/                              :: 测试：topographic + 2.5d 切换 / compression 变化触发 rebuild / 子图进入后视觉一致

tests :: mesh 顶点 z 断言 +1（一个 1024² 平顶地形 → 期望 z=0 ；一个有山地形 → 期望 z 非零且按 compression 缩放）
        rebuild 触发条件 +1（compression 变化必 rebuild）
        mode-coupling 自动 on +1（Topographic 自动开）
        联动渲染元素 z 平移 +1（笔刷光标 / 浮雕 / isoline / CellSelection）
        measurement z 不变 +1（验证例外）
估算  :: ~150 行 + ~5-6 测试；与 ADR-0039 / ADR-0040 同列 PR 排队，但本 ADR 单独 PR
```

---

## 7. 待回答项（用户 review 时一并回我）

| 编号 | 问题 | 我的默认（已写入正文） |
|---|---|---|
| **R1** | `compression` 默认值 | **0.3**（不是 ART_STYLE_SPEC §1.1 的 0.5） |
| **R2** | `compression` 范围 | **[0.0, 1.0]**（>1 越界不让） |
| **R3** | 是否提供 UI 滑条 | **暂不提供**（首版写入代码常量 0.3；用户手动调可经 ViewState 内部接口；UI 滑条 v2） |
| **R4** | 用户手动切回 Topographic mode 内 `terrain_2_5d_enabled=false` | **允许**（"我只想 2D 着色"是合法诉求）|
| **R5** | 联动清单（决策 4）覆盖是否完整 | **覆盖 4 项**（浮雕 / isoline / 笔刷光标 / CellSelection）；**不覆盖 measurement**（沿用 [MAP_LAYER_RENDERING_SPEC §4 例外 ②](../../crates/wv-render/src/MAP_LAYER_RENDERING_SPEC_v0.1.md)） |
| **R6** | 是否同步修改 [ART_STYLE_SPEC §1.1](ART_STYLE_SPEC_v0.1.md) 把默认改为 0.3 | **本 ADR 不动 ART_STYLE_SPEC**；是否追记由用户决定 |

---

## 8. 何时从「提议」转「已接受」

- 用户在对话中明确复述「同意 ADR-0041，按此实施」即转 §状态为「已接受」
- 转「已接受」后，本 ADR 正文**不再修改**；后续推翻需新开 ADR（[AGENTS §6.4](../../AGENTS.md)）
