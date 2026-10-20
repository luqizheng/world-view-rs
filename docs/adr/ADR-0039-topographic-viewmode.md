# ADR-0039：新增 `ViewMode::Topographic`——地形分析视图(水/陆二分 + height 3 阶着色)

**状态**：已接受（Accepted）
**日期**：2026-10-19
**决策者**：用户（对话拍板"A + B 组合 + go"，本 ADR 仅锁 A 部分——B 部分由 ADR-0041 锁）
**关联**：[ADR-0021](ADR-0021-render-recipe-and-treatment.md)（渲染配方真值表契约，新增 mode 必须加表项）、[ADR-0023](ADR-0023-theme-pack-injection-contract.md)（ThemePack 边界）、[ADR-0031](ADR-0031-terrain-decor.md)（terrain decor 旗标族）、[ART_STYLE_SPEC §1.4.5](ART_STYLE_SPEC_v0.1.md)（JRPG 色板）、[MAP_LAYER_RENDERING_SPEC §0/2.3](MAP_LAYER_RENDERING_SPEC_v0.1.md)（四轴 + 20 通道契约）

> **配套说明**：本 ADR 只锁"渲染 viewmode 增加"这件事。配套的两件事另行开 ADR：
> - **ADR-0040** —— CellInspector 工具（单格拾取 + 三通道编辑，本 ADR 不涉）
> - **ADR-0041** —— 2.5D vertex Z 挤压（`topographic.2_5d_enabled` 字段本 ADR 引入，实现由 ADR-0041 锁）
>
> 三份独立，分别 review，独立 commit。AGENTS §6.4「不分不开」—本 ADR 不锁这两件事。

---

## 1. 处境（Context）

用户提了一个具体诉求：「想给 cell 这一层的地图加一种视觉方式——只显示地形数据，即类型(水/陆地)、还有高度三档数据。」

### 1.1 现有 7 个 `ViewMode` 不满足

`crates/wv-render/src/recipe.rs::MODE_TABLE` 当前 7 行（`VectorFlat` / `RealTerrain` / `Schematic` / `GoogleMap` / `Faction` / `Event` / `Coastline`），对照诉求逐条筛选：

| 模式 | 调色 / 几何 | 失败原因 |
|---|---|---|
| `VectorFlat` | JRPG 10 色 core + Veil + Grain + TerrainDecor + Roads/POI/Labels 全开 | "TerrainDecor + Roads + Labels" 不让画面"只显示地形"——森林/草原符号干扰水域辨识 |
| `RealTerrain` / `Schematic` | 同上简版 | 同上 |
| `GoogleMap` | 关 Veil/Atmosphere/Buildings，但 TerrainDecor=关，FeatureOverlay/Rivers=开 | 仍画 Roads/POI/labels；调色是 Google，不是地形分析 |
| `Faction` | Faction Pass 开 | 干扰水域轮廓，名字是「势力图」 |
| `Event` | 极简底图但 Roads/POI/Labels 仍全开 | 「极简」指背景，不是「只剩地形」 |
| `Coastline` | 关 Veil/Grain/Atmosphere/Buildings/Faction/TerrainDecor/Rivers，但 Roads/POI/Labels 仍开 | 语义错位——`Coastline` 是「勾轮廓」分析（`recipe.rs` 注释），与「看地形」是两条不同的诉求；强行改它会两败俱伤 |

「复用现有 + 关层」（方案 A）**不算**视觉方式——是配置；「复用现有 + 改参数」（Coastline 改造）**会**语义错位；新 `ThemePack`（方案 D）会**违反**「ThemePack 是风格不是模式」的架构边界（[ART_STYLE_SPEC §2.A](ART_STYLE_SPEC_v0.1.md)）。

### 1.2 用户原话的精确锁定

> 「只显示地形数据，也就是类型 水/陆地，还有高度 3 个数据。」

- 「类型 水/陆地」 = `surface` 二值通道（`SURFACE_WATER=0` / `SURFACE_LAND=1`，[wv-terrain/src/lib.rs:55-59](crates/wv-terrain/src/lib.rs)）
- 「高度 3 个数据」 = `height` 通道按百分位 33/66 切成 **3 阶着色带**（低地 / 中地 / 高地）；**不是** 3 个 HUD 数字
- 「只显示」 = 关掉 Veil / Grain / Atmosphere / TerrainDecor / FeatureOverlay / Roads / Streets / POI / Buildings / Faction / Labels

### 1.3 跟 `cell.biome` 字段的关系

`TerrainCell` 实际三通道（`height` / `surface` / `biome`，[wv-terrain/src/lib.rs:69-75](crates/wv-terrain/src/lib.rs)），其中 `biome` **不冗余**（活跃消费者：`marker.rs:100` 群系识别 / `climate.rs:442` 气候内核 / `edit.rs:200` 笔刷复写）。用户已拍板「**X 路线**」：biome 字段保留，本 mode 渲染时**只读 `height + surface`，biome 不参与**——其他 mode 与 marker/climate/edit 仍照常读 biome，零数据改动。

---

## 2. 决策（Decision）

**全部动作汇总：**

1. **新增 `ViewMode::Topographic`**：`crates/wv-render/src/state.rs` 的 `ViewMode` 枚举 + 1 项，进 `ViewMode::ALL` 数组（第 8 项）；`i18n` 与 `label` 跟进。
2. **`MODE_TABLE` 加 1 行**（`crates/wv-render/src/recipe.rs::MODE_TABLE`）：仅 `Terrain`=`true`，其它 22 个 `PassId` 默认 `false`；叠加 `LayerFrame.terrain.visible=false` 的级联保留。表测试（`recipe.rs::pass_id_count_matches_mode_table`）自动接住新增。
3. **新增调色路径**（不走 ThemePack）：在 `crates/wv-render/src/theme/` 新增 `TopographicPalette` 数据结构，4 个令牌：
    - `water`（单色蓝，默认 `#3A7DB5`）
    - `land_low`（低地，默认 `#F0E5C9`）
    - `land_mid`（中地，默认 `#C9B58A`）
    - `land_high`（高地，默认 `#8A7A66`）
    默认值经美术裁定（draft 提议值，待用户实机校准后写定）；ThemePack 可声明 `topographic.*` 覆盖这 4 个令牌（D4 §6 待裁）。
4. **`TerrainChunk` 渲染分支新增 `match view.mode { Topographic => ... }`**（`crates/wv-render/src/terrain_chunks.rs::sync_terrain_chunks`）：该分支下读 `cell.height` + `cell.surface`，刻意不读 `cell.biome`；按本图百分位 33/66 分 3 阶，水/陆区分用 surface 通道（与 `display_kind` 湿判定分离，本 ADR 简化为：陆=land_*_low/mid/high，水=water）。
5. **百分位阈值缓存**：每图首次进入本 mode 时，对该 `TerrainGrid` 一次性求 33/66 百分位阈值（`O(N)`，N=栅格格数），存到 `ViewState::topographic_thresholds(per_map: HashMap<Id, (i16, i16)>)`。该值与 `terrain.bin` 文件的 `data_hash` 联动，hash 变才重算。
6. **`ViewState.terrain_2_5d_enabled: bool`**：新增字段（实现由 ADR-0041 锁，本 ADR 仅声明语义存在）。用于将来 Topographic 模式下「vertex.z = height × compression」开关；其他 mode 默认 `false`。
7. **Isoline / GridLines 在本 mode 下独立子开关**：默认 `Isoline=true`、`GridLines=false`；`LayerFrame.terrain.visible=false` 仍级联关（[MAP_LAYER_RENDERING_SPEC §2.3](MAP_LAYER_RENDERING_SPEC_v0.1.md)）。
8. **数据层零改动**：`cell.biome` 字段不动；不引入新的 `EditCommand`；`terrain.bin` 字节布局（I16 + U8 + U8）零变化；`wv-io` 迁移表零变化。**符合 P3 与 P6**。
9. **UI 入口**：顶栏 viewmode 下拉 + 1 项；i18n Key +1；`crates/wv-app/src/state.rs::ViewModeLabelExt` 加 1 条映射；快捷键延后（不强制首版）。
10. **API 表面约束**：`view.mode == ViewMode::Topographic` 是渲染层单点判断入口；不允许在 `wv-edit`、`wv-core` 引用 `ViewMode`（[AGENTS §2.1](AGENTS.md) 分层纪律）。

---

## 3. 理由（Rationale）

- **架构归位正确**：模式轴本职是「看哪些 + 怎么画」，与本 ADR 诉求一一对应；与 [MAP_LAYER_RENDERING_SPEC §2.3](MAP_LAYER_RENDERING_SPEC_v0.1.md) 「mode 表 + LayerFrame 级联」契约完全齐缝，新增 ViewMode 是已有契约的预期动作。
- **不污染 ThemePack**：调色独立路径（决策 3）使本 mode 不受 `ThemePack.treatment` 旗标族影响，也保证「分析视图」不被任何 JRPG pack 拉到 JRPG 观感里去——这条边界贴 [ART_STYLE_SPEC §2.A](ART_STYLE_SPEC_v0.1.md) 「软件壳不可换」的精神。
- **biome 字段不消失，符合 X 路线**：biome 在 marker / climate / edit 都仍消费，本 mode 渲染只是「不读」，不动字段（[wv-terrain/src/lib.rs:69-75](crates/wv-terrain/src/lib.rs)）。这保留了 [ADR-0032](ADR-0032-place-vocab-unify.md)（group 系）与 [ADR-0029](ADR-0029-biome-marker.md)（marker 系）的设计。
- **百分位缓存避免每帧 O(N) 重算**：决策 5 的 `data_hash`-driven cache 让切图/地形编辑后仅在数据变化时重求，开销摊到编辑动作而非每帧。
- **viewmode 8 个仍受断言守护**：[recipe.rs:412](crates/wv-render/src/recipe.rs) 已有 `assert_eq!(MODE_TABLE.len(), ViewMode::ALL.len())`，新增一项继续被断言守；不会留「忘加表项」的隐藏 bug。
- **`terrain_2_5d_enabled` 字段提前预埋**：决策 6 的字段在 wv-render state.rs 加一个 `bool` 是廉价动作；若无此字段，ADR-0041 实施时要回 ViewState 加字段、一起做迁移，更贵。这是「为后续铺路而不做后续」的合规手法——字段加但本 ADR 不实现。

---

## 4. 代价（Consequences）

### 4.1 负面后果（必须承认）

1. **百分位求值的初次开销**：每张图首次进入本 mode 时 `O(N)` 求分位；N=栅格总格数（[wv-terrain/src/lib.rs:122](crates/wv-terrain/src/lib.rs)）。1MB 地形（256²）约 < 1 ms；16MB（1024²）约 20 ms；超过 16,777,216 格的栅格已在 [wv-terrain/src/lib.rs:122] 处被加载侧拒绝，本 ADR 不需更严的闸。
2. **调色 4 令牌需美术实机校准**：默认色值（决策 3）是提议值，与 [ADR-0035 §1.4.5](ART_STYLE_SPEC_v0.1.md) 色板无冲突，但不保证最佳观感；若校准后分歧大，可经「提议」状态追加修订（不破坏 AGENTS §6.4 「已接受」正文不动原则）。
3. **本 mode 不会自动联动 `terrain_2_5d_enabled`**：决策 6 仅声明字段，ADR-0041 实现前默认 `false`，用户进 Topographic 看不到 2.5D 直到 ADR-0041 落地。这是有意的——避免本 ADR 提前承担 ADR-0041 的范围。
4. **i18n 多 1 条**：zh/en 各加「Topographic」/「等高分析」一枚 Key，幅度可控（[ADR-0018 i18n](ADR-0018-i18n.md) 兜底）。
5. **顶栏下拉 + 1 行**：`Vec<ViewMode>` 多 1 项，UI 不需重排。

### 4.2 后续可能（不构成本 ADR 决策）

- **Height 分位带改成 5 阶**（与 isoline 步阶同源）：未来若用户反馈「3 阶不够细」，可经 `topographic.levels: {3|5}` mode 子参数扩展，不重开 ADR。
- **ThemePack 覆盖 topographic 4 令牌**：决策 3 留口但未实现。如用户实机需要某 pack 自定义，加 [ADR-0023](ADR-0023-theme-pack-injection-contract.md) 的子决策即可。
- **CellInspector 在本 mode 下高亮选中格**：与本 ADR 配套的下游，落到 ADR-0040。

### 4.3 被否决的备选方案

| # | 备选 | 否决理由 |
|---|---|---|
| A | **复用 `VectorFlat`，把作者层全关** | 「视觉方式」需入 mode 表（ADR-0021 契约）；且 LayerFrame 改作者工程 YAML 会污染数据作者；不可作为工程默认值 |
| B | **复用 `Coastline` 改造**：改其调色与关项 | 「Coastline」语义是「勾轮廓分析」，与「看地形」诉求不同；强行合并会让两者都变糊，且未来若两者独立诉求重做时已被锁 |
| C | **新 `ThemePack::topographic` + `treatment=DATA_VIEW`** | ThemePack 是「风格」非「模式」（[ART_STYLE_SPEC §2.B](ART_STYLE_SPEC_v0.1.md)）；且 theme 切换会影响所有 7 个 mode，无法把分析视图与其它 mode 解耦 |
| D | **新 `ViewMode::TerrainOnly`，仍走 ThemePack core.\* 10 色** | 不满足用户「水/陆二分 + 3 阶高度」核心诉求；保留 10 类 core 多色会让「分析视图」被 JRPG 调色拉到 JRPG 观感 |
| E | **新增等高分析位图模式**（整张位图已绘好，非派生） | 违反 ART_STYLE_SPEC §3.2 「terrain_atlas 必须 40 块 10×4」契约；不可作为通用机制 |

---

## 5. 涉及与不涉及（AGENTS §10 自检）

**触及**：

- §6.4（新增 ViewMode 是契约扩展，本 ADR 即留痕）
- §10 变更检查清单中「依赖方向」（新增 ViewState 字段在 wv-render，未跨 wv-core / wv-terrain / wv-edit 边界）

**未触及**：

- §1 P1–P7 全部（biome 字段不动 / 数据层零变化 / 模式决策不破坏 P4 多地图层级）
- §2.1 依赖方向（wv-core / wv-terrain 仍零 Bevy 依赖）
- §3.3 不直接改世界（系统层仍只能产出 EditCommand，本 ADR 不引入新命令）
- §7.5 V1/V2（作者可改 cell，校验只提示不阻止——本 ADR 渲染层不引入新校验）

**触发但不冲突：**

- 配合 ADR-0041（2.5D vertex Z 挤压）——「`terrain_2_5d_enabled`」字段在本 ADR 仅声明，落地由 ADR-0041 锁
- 配合 ADR-0040（CellInspector 工具）——本 mode 下选中一格的高亮是否使用 `PassId::CellSelection`，由 ADR-0040 决定

---

## 6. 提议落点（仅 paper，不写码）

```
crates/wv-render/src/state.rs        :: enum ViewMode +1, ALL +1, terrain_2_5d_enabled: bool
crates/wv-render/src/recipe.rs       :: MODE_TABLE +1 行（断言自动接住）
crates/wv-render/src/theme/mod.rs    :: 新建 topographic.rs (TopographicPalette 结构 + 默认值)
crates/wv-render/src/terrain_chunks.rs :: sync_terrain_chunks 新增 match 分支
crates/wv-app/src/state.rs           :: ViewModeLabelExt +1 条映射
crates/wv-app/src/ui/top_bar.rs      :: viewmode 下拉 +1 行
crates/wv-app/i18n/                  :: 新增 zh/en 各 1 Key "Topographic"
crates/wv-render/src/state.rs        :: 百分位缓存 HashMap<Id, (i16, i16)> + data_hash 联动
tests                                :: mode 表测 +1;percentile 求值 +1;topographic 分支 +1（地面平、海面、Biome_NONE 仍 low/mid/high 不变）共 ~5 项
估算                                  :: ~120 行 + ~5 测试
```

---

## 7. 提议何时从「提议」转「已接受」

- 用户在对话中明确复述「同意 ADR-0039，按此实施」即转 §状态为「已接受」
- 转「已接受」后，本 ADR 正文**不再修改**；后续推翻需新开 ADR（AGENTS §6.4）
