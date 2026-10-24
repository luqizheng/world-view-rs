# ADR-0055：CellMap 海拔分层设色——用颜色在平面上读出高度（不引入 3D）

**状态**：已接受（Accepted）

> **2026-10-24 追记**：决策 3「逐图百分位」被 [ADR-0056](ADR-0056-hypsometric-absolute-ladder.md)
> 修正为「**绝对地形学阶梯 + 窄域兜底**」——百分位会让编辑反馈漂移，
> 并在 CPU 增量补丁路径上产生阶接缝。其余决策（A 叠层、8 阶、固定色带、
> 双路径、强度会话态）**全部不变**。正文按只增不改保留。
**日期**：2026-10-24
**决策者**：用户（对话裁定三问：「A 叠层」「8 阶」「百分位」）
**关联**：**修正 [ADR-0051](ADR-0051-mode-content-matrix.md) §5「高度不参与颜色」**（正文不改，头部标注）；落地 [ADR-0039](ADR-0039-topographic-viewmode.md) 决策 4 的「高度着色」诉求但改口径（3 阶 → 8 阶，等分 → 百分位）；与 [ADR-0053](ADR-0053-terrain-gpu-field-rendering.md) 的 GPU 场管线**双路径同时接线**；[ADR-0041](ADR-0041-topographic-2-5d-vertex-z.md)（2.5D）**不在本 ADR 范围**

---

## 1. 处境（Context）

用户提问：「cell map 如何在平面图中体现海拔不同高度？① 是否需要引入 3D？② 用颜色标注不同高度，类似模拟城市 2000 那种表现方式？」

### 1.1 现状（实测，2026-10-24）

| 能力 | 真实状态 |
|---|---|
| CellMap 底图取色 | 群系固定色块，**高度完全不参与颜色**（[ADR-0051] §5 / `biome_raw_approx`） |
| 明暗 | 已有 hillshade（解析梯度 × 左上 45° 光，`Z_EXAG=0.55`，钳 0.86–1.14，[ADR-0053](ADR-0053-terrain-gpu-field-rendering.md) P1）——只能读「坡」，读不出「高」：两片同样平的高原与平原同色同明暗 |
| 3 阶高度色阶 | 已实现但**只在 Browse / Themed 分支**（`themed_land`，`CUT_LOW=0.33 / CUT_HIGH=0.66`）；CellMap 不走它 |
| 2.5D | **空壳**：`terrain_2_5d_enabled` / `topographic_compression` 字段在、mode 耦合自动开在，但**无消费者**。唯一实现是 CPU 烘焙路径的 `oblique_skew_pixels`（整图按**行 y** 斜切成平行四边形，不按格高度位移，且调用处传 `0.0`）。且 [ADR-0053] 的 GPU 路线下底图是全屏 quad，**没有顶点可写 z**——[ADR-0041] 的 vertex-Z 方案在 GPU 路线上自然失效 |
| 等值线 | 会话开关（默认关），可作**绝对海拔**参照 |

### 1.2 为什么不是 3D

真 3D（`Camera3d` + Perspective）会撕开 `Camera2d` 投影栈：标签 billboard、笔刷光标、子图对位、格拾取全要重适配——[ADR-0041] §4.3 已把 D 档否决，[ADR-0053] 又把底图固化为 GPU 平面 quad。**用户诉求是「在平面上读出高度」，颜色是零投影代价的解法。**

---

## 2. 决策（Decision）

1. **A 方案：海拔色阶作为「叠层」**（用户裁定）——群系色仍是底，海拔色按**强度**混入：
   `color = mix(群系色, 色带[band], strength)`，`strength ∈ [0,1]`。
   - `0` = 纯群系色块（ADR-0051 原观感）；
   - `0.35` = 默认「双读」（群系可辨、海拔可读）；
   - `1` = 纯海拔色阶。
2. **8 阶**（用户裁定）。阶数、色带都是 crate 级**固定常量**（[`hypsometric::RAMP_SRGB`]），
   **不进 ThemePack**——守 [ADR-0051] §5 与 [ADR-0054](ADR-0054-world-cell-map-crate.md) D2 的「固定色 crate 级禁令」。
3. **逐图百分位切分**（用户裁定）：7 条内边界 = 第 `i/8` 分位（`i = 1..8`），
   **样本只含陆格**（`surface == LAND && height >= 0`）。改掉 ADR-0039 的「等分 p33/p66 / 3 阶」。
   - 理由：高度分布重尾，等分绝对高度会让 95% 的格挤在一阶；百分位保证每阶人口相等，
     层次在所有图上都读得出来。
4. **只叠陆格**：水面继续走海岸三段色带（`wd` 驱动）；海拔色带对水面无意义。
5. **与 hillshade 相乘**：色带读「高」、晕渲读「坡」，两者正交互补；顺序保持现状
   （`land_col = base * relief`）。
6. **双路径同时接线**（重要）：`field_render` 默认 `false`，用户当前看到的是 CPU 烘焙路径。
   故两条路径都实现，且**共用同一份色表与同一个切分函数**：
   - CPU 烘焙（当前默认）：`CellRenderer` 在 Topographic 早返回分支逐格叠色；
   - GPU 场（[ADR-0053] 目标路径）：`FieldParams.elev_cuts` / `elev_ramp` 进 uniform，
     fragment shader 内 `elev_band(h)` + `mix`。
7. **强度是会话态**：`ViewState.hypsometric_tint: f32`，默认 `0.35`，不落盘。
   - CPU 路径：强度变化 = **重烘整图**（`TerrainTexture.last_hypsometric` 参与重建判据）；
   - GPU 路径：强度变化 = **只改 uniform**（不进 `FieldKey`，不重打 12 MB 数据纹理）。
   - UI：map controls 槽 10.75 滑条，**仅 CellMap 显示**。
8. **数据层零改动**：`TerrainCell` 三通道不动；`terrain.bin` 字节布局不动；`wv-io` 迁移表不动。
   **符合 P3 / P6 / P7**。
9. **百分位只需算一次**：CPU 路径在整图合成时算（与既有 p33/p66 同位置）；
   GPU 路径在重打时算并缓存在 `TerrainFieldRender.cuts`，缩放 / 拖强度**不重算**。

---

## 3. 理由（Rationale）

- **颜色是「平面读海拔」的标准解法**：传统地形图 = 分层设色（hypsometric tint）+ 等高线 +
  晕渲三件套；本项目晕渲已有、等高线已有（会话开关）、缺的正是分层设色。
- **叠层而不是替代**：CellMap 的首要职责是**编辑群系与高度**，群系色块是编辑时的视觉语言
  （群系分界线工具依赖它）。直接换成海拔色会让编辑失去参照；`strength` 一个旋钮覆盖
  「纯群系 / 双读 / 纯海拔」三种诉求，代价只有一条 `mix`。
- **固定色带**：数据视图要跨主题稳定（ADR-0051 §5 的原裁定），色带若进 ThemePack，
  切主题就会改变「海拔读数」的含义。
- **百分位 vs 等分**：见决策 3。代价（无绝对海拔语义）由等值线承担。
- **共用一份色表**：色带在 Rust 常量、经 uniform 进 shader——CPU 与 GPU 两条路径**不许**
  各写一份色值（那是最容易漂的地方）。`hypsometric::RAMP_SRGB` 是唯一真源。

---

## 4. 代价（Consequences）

### 4.1 负面后果（必须承认）

1. **百分位没有绝对海拔含义**：一片平地也会出现最高一阶（最亮的近纸白）。
   这是用户选「百分位」的直接推论，不是 bug；要到绝对海拔参照就开等值线。
2. **色相轴被两件事争用**：群系与海拔都想要「颜色」。`strength=1` 时群系不可辨、
   `strength=0` 时海拔不可辨；默认 0.35 是折中，不是最优。
3. **CPU 路径拖滑条要重烘整图**：1024² 约数十 ms（与既有模式切换同量级）。GPU 路径无此代价。
   若实机拖动手感差，后续可加「滑动结束后再烘」的节流（v2，不在本 ADR）。
4. **补丁不刷新百分位**：GPU 路径的增量补丁只改纹理，不重算 `cuts`（brush 编辑后阶阈值
   到下次重打才更新）。这是有意取舍：每次笔画 O(N log N) 排序不划算，且百分位是**参照系**
   不是内容。
5. **两种路径可能短暂不一致**：CPU 路径按「合成那一刻的数据」算 cuts，GPU 路径按「重打那一刻」
   算。同一帧同数据下两者同值（同一函数、同一归一化口径）；换路径那一刻允许不同帧，属可接受。
6. **ADR-0041 的 2.5D 在 GPU 路线上失效**：本 ADR 不复活它。若用户后续仍要立体感，
   需新开 ADR（shader 屏幕空间位移 或 回到 tessellated mesh），不在本 ADR 范围。

### 4.2 被否决的备选

| # | 备选 | 否决理由 |
|---|---|---|
| A | 真 3D（`Camera3d` + Perspective） | 撕投影栈（标签 / 笔刷 / 子图对位 / 拾取）；ADR-0041 §4.3 已否决 |
| B | 2.5D vertex Z（ADR-0041 原案） | GPU 底图是全屏 quad，无顶点可写 z；复活需重构底图几何 |
| C | 严格单选档（群系色块 / 海拔色阶二选一） | 编辑群系时要看高度、看高度时要看群系，来回切成本高；被 A 的 `strength` 连续量覆盖 |
| D | 等分绝对高度（沿用 ADR-0039 的 p33/p66 思路扩到 8 阶） | 重尾分布下绝大多数格挤在一阶，读不出层次 |
| E | 色带进 ThemePack（随主题变） | 数据视图必须跨主题稳定（ADR-0051 §5 / ADR-0054 D2） |
| F | 只做 GPU 路径 | `field_render` 默认 `false`，用户当前看到的是 CPU 路径——只做 GPU 等于「做了但看不见」 |
| G | 只做 CPU 路径 | ADR-0053 的目标路径就是 GPU 场；不做会让两条路径观感分叉 |

---

## 5. 涉及与不涉及（AGENTS §10 自检）

**触及**：

- §6.4（本 ADR 即留痕；**推翻 ADR-0051 §5 的「高度不参与颜色」**，按「不许沉默推翻」补此文件）
- §7.1 渲染层门禁：`cargo test/clippy --features app` + `smoke-gui.sh` 必须跑
- §10「依赖方向」：改动落在 `wv-world-cell-map` / `wv-render` / `wv-app` 三个下游层，
  **未跨** `wv-core` / `wv-terrain` / `wv-edit` 边界

**未触及**：

- P1–P7：P3（`terrain.bin` 不动）、P6（无新 EditCommand，撤销栈不动）、P7（纯渲染，无时间语义）
- §2.1 依赖方向：`wv-world-cell-map` 仍不依赖 theme / 兄弟 crate；色带是 crate 常量
- §3.3 不直接改世界：零新命令
- §7.5 V1/V2：与 cell 编辑无交叉

---

## 6. 落点

```
crates/wv-world-cell-map/src/terrain_view/hypsometric.rs   :: 新增：8 阶色带 + 百分位切分 + 归一化 + 单测
crates/wv-world-cell-map/src/terrain_view/mod.rs           :: 挂模块
crates/wv-world-cell-map/src/palette.rs                    :: CellPack.hypsometric（会话强度骑在合成入参包上）
crates/wv-world-cell-map/src/terrain_view/cells.rs          :: Topographic 早返回分支叠色；Cuts 取代 (p33,p66)
crates/wv-world-cell-map/src/terrain_view/rebuild.rs        :: SynthCtx 的 PartialEq 比强度
crates/wv-world-cell-map/src/terrain_view/setup.rs          :: new_terrain_texture 记 last_hypsometric
crates/wv-world-cell-map/src/terrain_view/terrain_resolve.rs:: replace_terrain_texture 同上
crates/wv-world-cell-map/src/terrain_view/terrain_texture.rs:: TerrainTexture.last_hypsometric
crates/wv-world-cell-map/src/terrain_view/field/material.rs :: FieldParams.elev_cuts / elev_ramp + apply_hypsometric
crates/wv-world-cell-map/assets/shaders/terrain_field.wgsl   :: elev_band / elev_ramp_color + CellMap 分支 mix
crates/wv-render/src/state.rs                              :: ViewState.hypsometric_tint（默认 0.35）
crates/wv-render/src/cellmap_bridge.rs                     :: conv_pack_with / 重建判据 / 场 uniform + cuts 缓存
crates/wv-app/src/ui/map_controls.rs                        :: 槽 10.75 滑条（仅 CellMap）
crates/wv-app/src/{app_bootstrap,ui_shell,edit/editing}.rs  :: 合成入口带强度
```

---

## 7. 验收

- 单测：百分位每阶人口相等 / 水格不参与 / 边界含等号 / 强度 0 恒等、1 等于色带 /
  色带明度单调 / 归一化与 `pack::H_MIN/H_MAX` 同口径。
- 实机：CellMap 下拖滑条 0 → 1，颜色从群系色连续过渡到「绿→纸白」8 阶；水面不变；
  Browse 模式不受影响；切主题不改 CellMap 观感。
- 门禁：`cargo test --workspace`、`cargo clippy --workspace --all-targets -D warnings`、
  `cargo fmt --check`、`cargo test -p wv-app --features app`、`cargo app-clippy`、`bash scripts/smoke-gui.sh`。

---

## 8. 后续（不构成本 ADR 决策）

- 强度滑条节流（CPU 路径重烘）。
- 色带与等值线阈值同源（同一组 cuts 画等高线，避免两套线）。
- 雪线 / 绝对海拔上限（若实机认为「平地也出最高阶」不可接受，可加 `height >= X` 的门）。
- 2.5D 若复活，另开 ADR。
