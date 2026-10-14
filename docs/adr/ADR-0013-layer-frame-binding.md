# ADR-0013：图层帧接通——`MapNode.layers[].z/visible/opacity` 驱动渲染

**状态**：已接受（Accepted，2026-10-14 用户拍板「我决定做C」）
**日期**：2026-10-14
**决策者**：用户（六层渲染架构讨论收尾，在三阶段方案 A/B/C 中选定 C）
**关联**：P5（图层化编辑）、AGENTS §3（数据与渲染分离）、[ADR-0008](ADR-0008-google-default-map-experience.md)（Google 式地图体验）、[ADR-0009](ADR-0009-graded-tile-zoom-to-street.md)（LodTier）、[ADR-0010](ADR-0010-street-vector-layer.md)（街道矢量层）、`GLOSSARY.md`（图层帧 / 图层绑定 / 槽位偏移）
**约束**：本 ADR 一旦「已接受」，正文按 AGENTS §6.3 / §6.4 不再修改；若后续需要反向修正，再新开一份 ADR。

---

## 1. 处境（Context）

数据层（`wv-core::map::Layer`）早已声明了完整的图层属性：`visible: bool`、`opacity: f32`、`z: i32`（「在同级图层中的绘制顺序，数值越大越靠上」），示例工程（xinhai-1911 等）的地图 YAML 也写着这些字段。但渲染层（`wv-app`）**没有任何代码消费它们**——绘制顺序由各渲染模块的硬编码 f32 Z 常量决定（地形 0.0、道路 0.09–0.12、POI 0.21–0.22、势力 0.3、标注 0.35/0.4、笔刷光标 0.9），显隐由 `ViewState` 的 7 个会话布尔决定。

结果是 P5 只兑现了一半：作者在 YAML 里写的 `visible: false` / `opacity: 0.5` / `z: 1` 全是**死字**；「图层化编辑」退化成「每类要素一个不落盘的会话开关」。同时 `LayerKind` 七个变体中 `Border`、`Influence`、`Road` 半数走旁路（势力实际走 `faction_overlays`、街道走 `streets.rs` 自己的缓存），`MapNode.layers` 与渲染层是两套平行系统。

六层渲染架构讨论（2026-10-14）确认：这是 P5 兑现的关键缺口，需要一次「接通数据层」的决策（方案 C）。

## 2. 决策（Decision)

1. **新增图层帧（LayerFrame）**：`crates/wv-app/src/render/layer_frame.rs` 作为 `MapNode.layers[].z / visible / opacity` 的**唯一求值点**。每帧在 `compute_projection` 之前由 `resolve_layer_frame` 从「当前图（`DrillState` 所指）的 layers + `ViewState` 会话开关」求值出七类渲染组各自的 `LayerBinding { visible, opacity, z_offset }`，写入 `LayerFrame` 资源。符合 AGENTS §3：YAML 是 source of truth，ECS 只放投影；求值是纯函数，拉动式刷新。
2. **kind → 渲染组映射**（首版口径）：

   | `LayerKind` | 渲染内容 | 与会话开关的关系 |
   |---|---|---|
   | `Terrain` | 地形分块底图 + 等值线 + 编辑网格 | 合取 `terrain_visible` |
   | `Road` | 道路 casing/fill + 街道 + 街道名标注 | 合取 `roads_visible` |
   | `Place` | POI 点标 + 地名标注 | 合取 `places_visible` |
   | `Influence` | 势力多边形叠加（faction_overlays） | 合取 `factions_visible` |
   | `EventPoint` | 事件高亮（Gizmos） | 合取 `events_visible` |
   | `Border` / `Custom` | **无消费者**（预留声明位，渲染忽略） | — |

   **标注跟随要素 kind**：街道名随 `Road`、地名随 `Place`——隐藏 Road 即道路与街名一起退场（Google Maps 同款语义）。
3. **可见性 = 合取**：`effective_visible = YAML 声明 ∧ 会话开关`。图层面板布尔保留为「本会话临时隐藏」（不落盘），YAML 是持久声明；任何一方说不可见即不可见。**未声明的 kind 保持既有行为**（可见），既有地图渲染零变化。
4. **z 的语义 v1 = 槽位偏移**：作者 `z`（i32）按每格 `SLOT_STEP = 0.05` 世界 Z 平移该 kind 渲染组的**内建槽位**，clamp 到 `±Z_CLAMP = 4` 格（±0.2）。各渲染组**保留自己的内建槽位常量**，渲染时叠加 `z_offset`；组内次序（casing < fill < 标注）是编译期约束、不动。`z` 全缺省（=0）时渲染与历史版本逐位一致。
5. **opacity 落材质 alpha**：各组在本色 alpha 之上相乘——势力在 `FACTION_ALPHA = 0.30`、网格线在 0.35、道路/POI/标注在本色（Google 色 = 1.0）之上乘 `LayerBinding.opacity`；clamp 到 `0.0..=1.0`（数据层不拒收，渲染层钳——V1）。
6. **多个同 kind 图层取第一个声明**（与 `streets::target_road_layer` 的 find-first 口径一致）；其余声明位保留在数据里。
7. **每帧重算 + 手动相等比较**：绑定没变时不触发 Bevy change detection，下游补丁系统（分块/等值线/网格的材质与 Transform 补丁）保持静默；道路/POI/标注本就每帧全量重写 Transform，z 偏移零额外成本。

## 3. 理由（Rationale）

- **为什么合取而不是取代会话开关**：面板布尔是编辑器交互（临时看底层），YAML 是作者声明（持久层序）。取代会破坏既有面板与测试；合取让两者语义都成立，且把「YAML 有否决权」这一 P5 关系立住了。
- **为什么 z 选槽位偏移而不是全量分带**：①零回归——既有地图（z 全 0）渲染逐位不变；②组内次序是**编译期断言**保护的载荷结构（`vector_layers.rs` 的 `const _` assert），全量分带要拆掉它并重新设计「标注带」（街名 0.35 / 地名 0.4 这两个远偏移存在的唯一理由是压在势力面 0.3 之上，分带后该保证无法维持）；③增量最小、可回退（AGENTS §12：收敛前的实现选最容易回退的方案）。
- **为什么标注跟随要素 kind 而不是独立「标注层」**：`LayerKind` 没有标注变体；为标注新开 kind 是数据模型改动，超出「接通」的范围。Google Maps 的「Transit 开关连标签一起隐」证明这是合理的作者心智模型。
- **为什么 Border / Custom 暂无消费者仍接进 LayerFrame**：绑定槽位的存在让「数据层声明了什么」与「渲染消费了什么」的差距变成**显式的、有类型的**一张表，后续补消费者（行政边界几何、自定义图层）只改渲染模块，不求值点。

## 4. 代价（Consequences）

- **正**：P5 从「半成品」变「作者声明真实驱动渲染」；图层帧是纯函数，测试覆盖 8 个语义点；渲染组次序保留编译期断言。
- **负①（行为变化）**：街道名标注此前**不受**任何开关控制（`update_street_labels` 无门），现在随 Road 图层隐藏——这是修复「隐藏道路留一屏街名」的不一致，但依赖旧行为的作者会看到街名消失。
- **负②**：`EventPoint` 的 `z` 偏移在 Gizmos 路径**不生效**（Gizmos 无逐实体 Z），事件层只有 visible/opacity 三者之二。
- **负③**：`LayerKind::Border` / `Custom` 声明位依旧没有渲染物；`Track`（轨迹）没有对应 kind，仍纯走会话开关。
- **负④**：图层面板只读不写——面板勾选仍是会话态，作者改 YAML 仍靠手编文件；「面板 → EditCommand → YAML」的写回是后续任务。

## 5. 被否决的备选（Rejected Alternatives)

| 备选 | 否决理由 |
|---|---|
| **按 z 全量分带重排**（排序声明层 → 重发各组的绝对 Z） | 真正实现「数值越大越靠上」的相对序，但：拆掉组内次序编译期断言；标注远偏移（0.35/0.4）与分带冲突，需先重设计「标注带」；既有地图视觉回归。留待作者反馈偏移模型不好用时，以新 ADR 重开。 |
| **把作者 z 直接当渲染 Z**（`z as f32`） | 粒度失控：z=1 就越过 POI（0.21），且组内 casing/fill 无法保持相对次序。 |
| **要求 layers 全声明、无声明即不画** | 破坏既有全部示例工程；违背 V1「只提示不阻止」。 |
| **把会话布尔并进 LayerFrame 后删除 ViewState 字段** | 面板、快捷键、测试全要跟着改；超出「接通数据层」的 scope。 |

## 6. 待裁（Open Questions）

- **z 语义 v2**：作者批量使用 z 后若发现「偏移模型」不可直觉预测（如想要 `road: z1 > place: z0` 严格相对序），需重开 ADR 做全量分带 + 标注带设计。
- **EventPoint 的 z**：等 Gizmos 换 Mesh2d 或 bevy 提供 2D gizmo depth 后补齐。
- **图层面板写回**：`SetLayerVisibilityCommand` / `SetLayerOpacityCommand` / `ReorderLayerCommand` 落 `wv-edit`，面板从会话开关升级为「声明编辑 + 会话覆盖」双层。
- **轨迹（Track）的 LayerKind 变体**：加 `Track` 变体是 serde 兼容扩展，但「轨迹是否作者声明的地图图层」是语义问题，未裁。
