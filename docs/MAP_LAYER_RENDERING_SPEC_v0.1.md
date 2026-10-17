# 地图呈现图层体系总览（Map Layer Rendering Spec v0.1）

> **定位**：回答「地图上看到的 layer 是怎么组织的、怎么驱动渲染的」——**现状快照**（as-is，2026-10-17），给读者一张导览图。
> **分工**：本文回答「**是什么**」；决策的「为什么」在各 ADR——[ADR-0013](adr/ADR-0013-layer-frame-binding.md)（图层帧）、[ADR-0016](adr/ADR-0016-jrpg-antique-map-default-theme.md)（建筑/古舆图主题）、[ADR-0017](adr/ADR-0017-label-layer.md)（标签层）、[ADR-0021](adr/ADR-0021-render-recipe-and-treatment.md)（渲染配方）、[ADR-0023](adr/ADR-0023-theme-pack-injection-contract.md)（主题包）。本文与 ADR 冲突时**以 ADR 为准**。
> **代码唯一出处**：`crates/wv-render/src/layer_frame.rs`（声明轴求值）与 `crates/wv-render/src/recipe.rs`（模式轴求值）。本文是导览，**不构成第二出处**；两处模块级文档注释与之同源。本文遵循 AGENTS §6.3：后续演进**追加**条目，不改正文既有结论。

---

## 0. 一句话结论

地图呈现由**四条正交的轴**共同决定「画不画、怎么画」：

| 轴 | 载荷（谁声明） | 求值点 | ADR |
|---|---|---|---|
| **模式轴**（技法：底图怎么画） | `ViewState.mode`——7 个 `ViewMode` | `RenderRecipe`（PreUpdate 单点求值） | ADR-0021 |
| **声明轴**（作者在 YAML 里声明的图层） | `MapNode.layers[]`——`LayerKind` 9 类 × `visible/opacity/z` | `LayerFrame`（Update 主链单点求值） | ADR-0013 / 0017 |
| **会话轴**（本会话临时开关，不落盘） | `ViewState` 的 8 个 `*_visible` 布尔 + `grid_lines` | 与声明轴在 `LayerFrame` 内**合取** | ADR-0013 §2.3 |
| **主题轴**（配色与装饰） | `ViewState.map_palette` / `theme_pack_id`（`ThemePack.treatment`） | 材质取色处；Recipe 将 treatment 视为输入 | ADR-0016 / 0023 |

四轴求值后收敛为 **20 条渲染通道（`PassId`）** 的 enabled 真值 + 各组 `LayerBinding`（可见 / 透明 / Z 偏移），渲染 gate 只读结果、不再散点判定。

在此之下还有两条**永不属于作者图层**的带（§7）：**编辑会话元素**（笔刷光标、街道橡皮筋预览、测量叠加）与**编辑网格**（独立会话开关）。

---

## 1. 作者侧：YAML 里声明了什么

每个 `MapNode`（图）自带 `layers` 清单（`maps/<id>/index.yaml`），每项一个 `Layer`（`wv-core/src/map/mod.rs`）：

| 字段 | 类型 | 语义 | 渲染消费 |
|---|---|---|---|
| `id` / `name` | `Id` / `String` | 图层标识与显示名 | 图层面板 |
| `kind` | `LayerKind` | 图层类别（下表）——决定渲染管线与编辑工具 | **`LayerFrame` 的映射键** |
| `visible` | `bool`（缺省 true） | 作者的持久可见性声明 | 与会话开关**合取**（§2.3） |
| `locked` | `bool`（缺省 false） | 锁定后不可编辑，但仍可见 | 编辑侧（渲染无关） |
| `opacity` | `f32` 0.0–1.0（缺省 1.0） | 图层透明度 | 落材质 alpha（clamp 0–1，V1 不拒收） |
| `z` | `i32`（缺省 0） | 「数值越大越靠上」——v1 语义为**槽位偏移**（§4） | `z_offset = clamp(z, ±4) × 0.05` 世界 Z |
| `source` | `Option<路径>` | 图层数据文件（相对工程根） | 街道（ADR-0010）/ 建筑 / 标签**配置**按 kind 读取 |

### 1.1 LayerKind 全集（9 类）

| `LayerKind` | 渲染内容 | 数据来源 | 出处 |
|---|---|---|---|
| `Terrain` | 地形底图（分块 / 瓦片图集 / 等值线平涂随模式）+ 参照网格 | 地形栅格（`RasterRef`，§5 二进制例外） | ADR-0013 / 0012 |
| `Road` | 道路 casing/fill + 街道网 | 手绘落 `source` YAML；真实世界走 Overture 引用 | ADR-0013 / 0010 |
| `Place` | POI 点标（红点白环）+ 地名标注 | 世界实体 `Place`（ID 引用，不内联） | ADR-0013 |
| `Influence` | 势力多边形叠加 | 势力几何（faction_overlays） | ADR-0013 |
| `EventPoint` | 事件高亮（Gizmos） | 世界实体 `Event` | ADR-0013 |
| `Building` | 2.5D 建筑挤出（阴影/侧面/顶面） | `source` → `BuildingLayer` YAML | ADR-0016 §6 |
| `Label` | 标签层（地名/街名/海名的文本投影） | `source` 只存**配置**（白名单/字号阶梯/上限）；文本由实体经 `label_visible_at` 派生 | ADR-0017 |
| `Border` | **无消费者**——预留声明位 | — | ADR-0013 §4 负③ |
| `Custom` | **无消费者**——预留声明位 | — | 同上 |

> 基线 `MAP_EDITOR_BASELINE_v1.md` §2.6 曾预想新增 `Faction` kind——实际以 `Influence` 落地（ADR-0013 表为准）。该节为历史讨论记录，按 AGENTS §6.3 不回改。

示例（既有工程同款）：

```yaml
layers:
  - id: l_terrain
    name: 地形
    kind: terrain            # visible/opacity/z 全缺省 → 渲染与历史版本逐位一致
  - id: l_roads
    name: 道路
    kind: road
    opacity: 0.8
    z: 1                     # 整组上移 0.05 世界 Z（槽位偏移）
    source: roads.yaml
```

---

## 2. 渲染侧：两级单点求值管线

原则三「YAML 是 source of truth，ECS 只是投影」在图层上的落地：**每帧两次纯函数求值**，渲染系统只读结果（拉动式，AGENTS §3）。

### 2.1 每帧时间线

```
PreUpdate   resolve_render_recipe   ← 读 ViewState + LayerFrame（上一帧值，晚一帧生效）
Update（MapRenderPlugin chain，plugins.rs）
  ├─ compute_projection          时间轴 → 渲染资源（TimelineQuery::at，P7）
  ├─ resolve_layer_frame         ← 声明轴 × 会话轴 合取求值（唯一求值点）
  ├─ sync_isoline_overlay / sync_terrain_chunks / reload_street_cache
  ├─ sync_vector_layers / update_street_labels / sync_faction_overlays / sync_map_frame
  └─ sync_building_overlays（after vector_layers）
```

### 2.2 第一级：`LayerFrame`（声明轴 × 会话轴，ADR-0013）

`LayerFrame::resolve(当前图, &ViewState)` 产出 **9 个槽位**（Terrain / Border / Influence / Road / EventPoint / Place / Building / Label / Custom）各自的 `LayerBinding`：

- **`visible` = YAML 声明 ∧ 会话开关**。图层面板布尔是「本会话临时隐藏」（不落盘），YAML 是持久声明；任何一方说不可见即不可见。未声明的 kind 沿用会话开关、其余中性（opacity 1.0 / z_offset 0.0）——**既有地图（layers 全缺省）渲染零变化**。
- **`opacity`** clamp 到 0.0–1.0，落各渲染组材质 alpha（在本色 alpha 之上相乘：势力 ×0.30 基数、网格线 ×0.35 基数……）。
- **`z_offset`** = `clamp(z, -4..=4) × SLOT_STEP(0.05)` 世界 Z，渲染时叠加到该组内建槽位上（§4）。

补充口径：

- **同 kind 只认第一个声明**（与 `streets::target_road_layer` 的 find-first 一致）；其余声明位保留在数据里。
- **子图 layers 接管**：求值永远取 `DrillState` 所指**当前图**的 layers——钻入子图后父图声明不再生效（P4：层级间只引用、不继承渲染状态）。
- **手动相等比较**：绑定没变不写资源，不触发 Bevy change detection，下游材质/Transform 补丁系统保持静默。

### 2.3 第二级：`RenderRecipe`（模式轴，ADR-0021）

`resolve_render_recipe` 每帧 PreUpdate 求值一次，产出 20 条 `PassId` 通道的 enabled 真值：

1. **`MODE_TABLE` 查表**：7 个 `ViewMode`（VectorFlat / RealTerrain / Schematic / GoogleMap / Faction / Event / Coastline）× 20 通道的硬编码真值表——模式自带的观感集中于此（如 Isoline 仅 VectorFlat；GoogleMap/Coastline 关 Atmosphere/Veil/Grain/Buildings/Faction）。新增模式加一行，漏加被表测试抓住。
2. **LayerFrame 级联叠加**：声明轴的可见性在模式之上再关一票——
   `terrain` 不可见 → Terrain/Isoline/GridLines/MapFrame/Atmosphere 全关；
   `road` → Roads + Streets；`place` → Poi + PlaceLabels；`label` → PlaceLabels + StreetLabels + SeaLabels；`building` → Buildings；`influence` → Faction；`event_point` → EventDots。
3. **fail-safe**：未知模式 = 全关；新 pass 默认 disabled——「忘接」显眼、好排查。

**两轴正交、不合并**（ADR-0021 §2）：Recipe 只把 LayerFrame 当输入；kind 语义（合取 / 缺省 / 首声明）仍归 `layer_frame.rs` 单一职责。Recipe **只管开不开**，不管几何/材质/调色；treatment 装饰旗标由 `ThemePack` 承载（ADR-0023 D4），Recipe 视为输入。

---

## 3. kind → 渲染组 → 通道对照（现状）

| 绑定槽位 | 渲染模块（wv-render） | 通道（PassId） | 备注 |
|---|---|---|---|
| `terrain` | `terrain_chunks` / `terrain_view/*` | Terrain / Isoline / GridLines / MapFrame / Atmosphere | terrain 关则整组底图族级联关 |
| `road` | `vector_layers` / `streets` | Roads / Streets | 街道名标注的 Z 取**标签**绑定 z_offset |
| `place` | `vector_layers`（POI）/ `drill`（`PlaceLabel` 池） | Poi / PlaceLabels | 地名标注 = `place ∧ label` 双门 |
| `label` | `streets`（街名）/ `drill`（地名）/ `label_halo` / `label_collision` | PlaceLabels / StreetLabels / SeaLabels | 标签内容不由 LayerFrame 承载（ADR-0017） |
| `influence` | `faction_overlays` | Faction | Faction **模式**强制画，`influence` 关仍能压掉 |
| `event_point` | 事件 Gizmos | EventDots | z_offset **不生效**（Gizmos 无逐实体 Z，ADR-0013 负②） |
| `building` | `buildings` | Buildings | 阴影/身体/顶面三槽 |
| `border` / `custom` | — | — | 无消费者，绑定槽位在（显式差距表，ADR-0013 §3） |
| —（无 kind） | 轨迹 | Tracks | 仅会话开关 `tracks_visible`（ADR-0013 §6 待裁） |
| —（无 kind） | `measurement` | Measurement | 见 §7 |
| —（无 kind） | `brush_cursor` | BrushCursor | 见 §7 |

---

## 4. Z 槽位全景（内建槽位 + 偏移）

各渲染组保留**内建槽位常量**（组内次序是编译期断言保护的，不动），整组再叠加 `LayerBinding.z_offset`：

| 世界 Z | 内容 | 常量（文件） | 所属绑定 |
|---|---|---|---|
| 0.00 | 地形分块 | `TERRAIN_Z`（terrain_chunks） | terrain |
| 0.03 | 参照网格 | `GRID_Z`（terrain_view/grid_lines） | terrain（级联） |
| 0.04 | 等值线平涂 | `ISOLINE_Z`（terrain_view/isoline_overlay） | terrain |
| 0.05 | 地图外框 / 底图远观淡出 | `FRAME_Z`（map_frame）/ `FADEOUT_Z`（terrain_view/rebuild） | terrain |
| 0.06 | 底色纱（veil）/ 外框内参考网格 | `VEIL_Z`（atmosphere）/ `GRID_Z`（map_frame） | terrain |
| 0.09–0.12 | 道路 casing/fill、街道 casing/fill、街道预览 | `ROAD_CASING_Z`…`STREET_PREVIEW_Z`（vector_layers） | road |
| 0.125–0.145 | 建筑阴影/侧面/顶面 | `BUILDING_SHADOW_Z`…（buildings） | building |
| 0.21–0.22 | POI 外环/内点 | `POI_OUTER_Z` / `POI_INNER_Z`（vector_layers） | place |
| 0.30 | 势力多边形 | `FACTION_Z`（faction_overlays） | influence |
| 0.35 | 街名标注 | `STREET_LABEL_Z`（streets） | label |
| 0.40 | 地名标注 | `LABEL_Z`（drill） | label |
| 0.45 | 测量叠加 | `MEASURE_Z`（measurement） | —（会话带，§7） |
| 0.50 | 颗粒（grain） | `GRAIN_Z`（atmosphere） | terrain |
| 0.90 | 笔刷光标 | `CURSOR_Z`（brush_cursor） | —（编辑带，§7） |

**作者 `z` 的 v1 语义 = 槽位偏移**（ADR-0013 §2.4）：每格 ±0.05 世界 Z、钳 ±4 格（±0.2）——防呆而不拒收（V1）；`z` 全缺省时渲染与历史版本逐位一致。被否决的「按 z 全量分带重排」及其理由见 ADR-0013 §5；若作者批量用 z 后发现偏移模型不可直觉预测，重开 ADR 做 v2（ADR-0013 §6）。

**例外**：① `EventPoint` 的 z 在 Gizmos 路径不生效（ADR-0013 负②）；② 测量叠加**有意不随** z_offset 漂移——用户原话「最上层」的强约束（measurement.rs）；③ 标签晕边用局部 `HALO_LOCAL_Z = -0.005` 贴底（label_halo），不走作者 z。

---

## 5. 会话轴：`ViewState` 开关全景

图层面板 / 快捷键 / 地图工具条控制的会话布尔（不落盘；与 YAML 声明合取，§2.2）：

| 开关 | 控制 | 是否经 LayerFrame |
|---|---|---|
| `terrain_visible` | 地形底图族 | ✅（级联 §2.3） |
| `roads_visible` | 道路 + 街道 | ✅ |
| `places_visible` | POI + 地名标注 | ✅ |
| `labels_visible` | 全部标签（ADR-0017） | ✅ |
| `buildings_visible` | 2.5D 建筑 | ✅ |
| `factions_visible` | 势力叠加 | ✅ |
| `events_visible` | 事件高亮 | ✅ |
| `tracks_visible` | 轨迹 | ❌ 无 kind，消费侧直读（待裁） |
| `grid_lines` | 参照网格 | ❌ 独立于笔刷（33b65b9 解耦），与 `labels_visible` 同族「看图方式」 |

> 同族的 `terrain_soften`（底图淡化）与 `map_palette` / `theme_pack_id` 属**主题轴**（§6），不是图层开关。

---

## 6. 主题轴（简述）

- **配色**：`MapPalette`（日间古舆图 / 暮色旧纸，ADR-0016）→ M2 起 `theme_pack_id` 为权威、`map_palette` 兼容 fallback（ADR-0023 D3）。切换触发底图纹理重建与矢量材质刷新（缓存键含主题）。
- **装饰**：`ThemePack.treatment` 承载装饰旗标（veil / grain / atmosphere 等，ADR-0023 D4 吸收了 ADR-0021 D2）——决定「画风层」开不开，与图层开关正交。

---

## 7. 编辑会话元素（永不属于作者图层）

以下渲染物**不进** `LayerFrame`（ADR-0013 模块边界）：它们是编辑会话的脚手架，不随任何 `LayerKind` 声明显隐——

- **笔刷光标**（`brush_cursor`，Z 0.9 恒顶层）；
- **街道橡皮筋预览**（`streets`，Z 0.12）;
- **测量叠加**（`measurement`，Z 0.45 + 文字/描边分层；Z 有意不随 z_offset 漂移）；
- **钻取标记**（`drill` 的子图入口图标——其 `PlaceLabel` 文字属标签层管线，图标本身属导航）；
- **编辑网格**：归属会话开关 `grid_lines`（§5），但 Z 上压在地形之上（0.03）、随 `terrain` 级联关（§2.3）。

---

## 8. 已知缺口与待裁（截至 2026-10-17）

| # | 缺口 / 待裁 | 出处 |
|---|---|---|
| 1 | `Border` / `Custom` 声明位无渲染消费者 | ADR-0013 §4 负③ |
| 2 | `EventPoint` 的 z 偏移不生效（Gizmos 无逐实体 Z） | ADR-0013 §6 |
| 3 | 轨迹（Track）无 LayerKind 变体，纯会话开关 | ADR-0013 §6 |
| 4 | 图层面板**只读不写**——`SetLayerVisibilityCommand` 等写回 wv-edit 未做，作者改 YAML 靠手编 | ADR-0013 §6 |
| 5 | z 语义 v2（全量分带）待作者反馈后重开 | ADR-0013 §6 |
| 6 | `Layer.source` 曾是空壳，街道链路已补（ADR-0010）；建筑/标签配置读取各自落位，分片阈值待裁（README §12-21） | README §12 |
| 7 | 标签碰撞 v0.1 精度上限（AABB 贪心，README §12-36） | ADR-0017 |
| 8 | 街名标注随 `Road` 图层隐藏是 ADR-0013 的**行为变化**（旧版街名不受任何开关控制） | ADR-0013 §4 负① |

---

## 9. 文档地图

- [ADR-0013](adr/ADR-0013-layer-frame-binding.md) —— 图层帧：kind→渲染组映射、合取、槽位偏移 v1 的**决策档案**
- [ADR-0017](adr/ADR-0017-label-layer.md) —— 标签层：`Label` kind 独立渲染组、文本派生、碰撞
- [ADR-0021](adr/ADR-0021-render-recipe-and-treatment.md) —— 渲染配方：模式 × 通道真值表、与 LayerFrame 的正交边界
- [ADR-0023](adr/ADR-0023-theme-pack-injection-contract.md) —— 主题包注入契约：treatment 归属
- [ADR-0016](adr/ADR-0016-jrpg-antique-map-default-theme.md) —— 古舆图主题 + 2.5D 建筑
- 基线 `MAP_EDITOR_BASELINE_v1.md` §1.10（ViewMode 比图层高一档）、§2.6（图层体系历史讨论）
- 术语：`GLOSSARY.md`「图层帧 / 图层绑定 / 槽位偏移 / 渲染配方 / 渲染通道」
- 代码：`crates/wv-render/src/layer_frame.rs`（9 槽求值 + 8 测试）、`recipe.rs`（真值表 + 表测试）、`plugins.rs`（Update 主链装配）
