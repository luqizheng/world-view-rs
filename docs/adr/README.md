# 架构决策记录（ADR）

> 本目录存放 World View 项目的 **ADR（Architecture Decision Record，架构决策记录）**。
> ADR 记录**已经做出的**决策及其理由，**不是**提出新方案的地方。

---

## 1. 什么是 ADR

ADR 是「一次架构/设计决策的档案」。每条 ADR 回答四个问题：

1. **当时的处境是什么？**（Context）
2. **决定了什么？**（Decision）
3. **为什么这么定？**（Rationale —— 必须能回溯到设计原则或用户裁定）
4. **代价是什么？**（Consequences —— 含负面后果与被否决的备选方案）

**ADR 的性质** —— 与基线文档的分工：

| 文档 | 回答什么 | 变化频率 |
|---|---|---|
| `docs/MAP_EDITOR_BASELINE_v1.md` | **完整的设计/架构内容**（规格全集） | 随讨论持续增补 |
| **ADR（本目录）** | **决策的「为什么」与「否决了什么」** | 决策落定时一次性写入，此后只增不改 |

> 基线文档回答「**是什么**」，ADR 回答「**为什么是这个，而不是别的**」。两者互相引用，不互相替代。

**ADR 与 `AGENTS.md` §6.3「文档追写不改机制」的关系**：
ADR 一旦定为「已接受」，其正文**不再修改**。若决策被推翻，**新开一份 ADR** 并把旧的标记为「被取代」，写明「被 ADR-XXXX 取代」。这与 §6.3「追加不覆盖、保留可追溯性」是同一纪律的落地。

---

## 2. 编号规则

| 规则 | 内容 |
|---|---|
| **起始编号** | **`ADR-0001`**（本仓库此前从无 ADR，从 0001 开始） |
| **增长方式** | 每新增一份，**递增 1**（`ADR-0005`、`ADR-0006`…） |
| **永不重号** | 一个编号一旦占用，**永久保留**。ADR 被废弃/取代时，**编号不回收、不复用** |
| **文件名** | `ADR-NNNN-<kebab-case-短名>.md`，如 `ADR-0001-faction-layer-model.md` |
| **标题** | 正文首行 `# ADR-NNNN：<中文标题>` |

> **不要**参考其他项目（如《暗流涅槃》）的 ADR 序列——那是独立编号空间，与本仓库无关。

---

## 3. 状态取值

每份 ADR 的头部 `**状态**` 字段取以下四值之一：

| 状态 | 含义 |
|---|---|
| **提议（Proposed）** | 已写出，尚未获得决策者确认。**此时不得据此实现代码** |
| **已接受（Accepted）** | 决策者已确认，**是本项目的现行约定**。代码与文档必须遵守 |
| **已废弃（Deprecated）** | 曾经生效，现已不再推荐；但在明确取代者出现前仍然是事实标准 |
| **被取代（Superseded）** | 已被另一份 ADR 完全取代。**头部必须写明 `被 ADR-NNNN 取代`** |

> 其他状态值（如「草稿」「暂缓」）**不引入**。需要区分「未决」时用「提议」，需要区分「暂缓」时用「已废弃 + 待裁小节」。

---

## 4. 当前 ADR 列表

| 编号 | 标题 | 状态 | 决策摘要（一句话） | 关联 |
|---|---|---|---|---|
| [ADR-0001](ADR-0001-faction-layer-model.md) | 势力层模型（不设层级枚举） | 已接受（Accepted） | 政治划分用**数据**（势力层/势力/归属）承载，**不用枚举**；势力层是多边形集合，归属多对多且带自定义类型标签与时间区间 | 基线 §2.5、用户裁定 D2、P4/P6 |
| [ADR-0002](ADR-0002-situation-ontology.md) | 事态本体（Situation + 时间升序派生） | 已接受（Accepted） | 新增「事态」作为事件之上的组织层，节点只引用 `EventId` 不复制事件；链的顺序**只能由时间派生**（A1），轨迹**可选、可粗略**（A2）。**⚠️ 其 Q4（「谁怎么动」记在哪里）的答案已被 [ADR-0004](ADR-0004-unit-ontology.md) 修正**——轨迹归**单位**，不归事态节点；A1 / A2 继续有效 | 基线 §1.1–1.9、§2.7、用户裁定 D1、P4/P6/P7 |
| [ADR-0003](ADR-0003-cross-map-reference.md) | 跨地图引用（绝不复制子图内容） | 已接受（Accepted） | 跨地图事态只靠 `Place` 的 ID 层级串联，**绝不复制子图内容**（A3） | 基线 §1.7、§2.8、P4 |
| [ADR-0004](ADR-0004-unit-ontology.md) | 单位本体（运动归 Unit，事态只是归纳容器） | 已接受（Accepted） | 新增**单位（Unit）**作为在地图上**独立运动**的主体，`Track.owner` 改为 `Track.unit`；事态是**归纳容器 / group**，**引用**运动而非拥有；「相遇却未写事件」只**提示**不报错（留白是创作手段） | 基线 §1.1 / §1.5 / **§1.6（修正其原口径）** / §2.7、用户裁定 Q1甲 / Q2乙 / Q3 / P1–P3、P4/P6/P7 |
| [ADR-0005](ADR-0005-encounter-hint.md) | 相遇检测与「未写事件」提示 | 已接受（Accepted） | 相遇是**派生数据**（不建实体）；判定 = 时间重叠 + 空间贴近 + 未被覆盖三道门；候选日用 waypoint 锚点（**不逐日枚举**，跨千年史实会卡死）；指纹**不含日期** → 提示可认领、可回访定位；提示与错误在**类型层面**分开（`DiagLevel::{Hint, Error}`） | ADR-0004 §待裁 U4、AGENTS §7.5 V1、基线 §2.21 |
| [ADR-0006](ADR-0006-gregorian-calendar.md) | 历法支持真公历（Calendar 双历制） | 已接受（Accepted） | `Calendar` 加判别字段 `kind`（缺省 `simple` 完全向后兼容）；`gregorian` 走 proleptic 公历（真实月长 + 闰年），天数计数器仍是唯一真相（P7 不动）；用 Hinnant 纯整数算法换算，**不引入 chrono** | 基线 §2.16、P3/P7、样例 xinhai-1911 |
| [ADR-0007](ADR-0007-overture-real-geography.md) | 以 Overture Maps 作为真实地理底图（只存引用，不落几何） | 已接受（Accepted） | 引入 Overture 作为外部地理资源，但**只存引用**（`OvertureId` + `bbox`），领域模型不消费几何；新增平行 crate `wv-overture` 负责 Parquet 切片下载与渲染层几何转换；不破 P2（Git 友好）/ P3（不为 Overture 开例外）/ P4（只存引用）/ §4.2（距离只存参数）；为 `OvertureId` 单点豁免 §4.4 UUID 字符集约束（独立 newtype） | README §12 §15-§18、AGENTS §4.2 / §4.3 / §4.4（UUID 例外）/ §5 |
| [ADR-0008](ADR-0008-google-default-map-experience.md) | Google Default Map 式地图体验 | 已接受（Accepted） | 大地图默认改为离线 Google Default Map 视觉与交互：左键拖动平移、笔刷显式开启、近白底图 + 亚像素边界 + 道路 casing/fill + 红点白环 POI + 右下角导航控件；不接在线瓦片，数据 / 时间求值 / 图层架构不变。**⚠️ 其 §2④（底图边界使用亚像素渲染）已被 [ADR-0012](ADR-0012-terrain-default-vector-flat-shading.md) 部分修正**——浏览态地形默认底图切到等值线矢量平涂（plan v0.1 Q11 路线），亚像素纹理合成降为 plan v0.1 D3 C 类可切换模式；本 ADR 的其余七项决策（左键平移 / 笔刷显式开启 / 近白陆地色板 / 道路 casing+fill / POI 红点白环 / 右下角控件 / 不接在线瓦片）继续有效 | P1 / P3 / P5 / P7、ART_STYLE_SPEC 地图风格部分、STYLE_SWAPPABLE_ARCHITECTURE、ADR-0012（§2④ 被部分修正） |
| [ADR-0009](ADR-0009-graded-tile-zoom-to-street.md) | 分级瓦片缩放（世界 → 街道） | 已接受（Accepted） | 「缩放到街道」用**离散级别 + 级间平滑插值**，不用无极连续缩放：级别由**图在层级树中的深度**派生（不扩 `MapLevel` 枚举）；`Extent` 加 `origin`（子图在父图中的落点，缺省 `(0,0)`），每图自带 `raster` 与自带原点以规避 `f32` 大坐标精度崩溃；坐标变换集中到 `wv_core::coord` 单一实现点。**⚠️ §2 决策⑤的「往里缩放换级」已被 [ADR-0038](ADR-0038-icon-click-drill-in.md) 修正**（进入子图 = 双击钻入图标；分级区间/坐标嵌套/外界回退不变）（2026-10-19） | P1 / P2 / P4 / P5 / P7、ADR-0007、ADR-0008、基线 §2.12 / §2.15、README §12-19 / §12-20 |
| [ADR-0010](ADR-0010-street-vector-layer.md) | 街道矢量层（双供给 + 批量渲染） | 已接受（Accepted） | 街道**不与** `distance.edges`（城市间行程连线）合并，是独立的**地物图层**；一个供给接口两个实现（虚构 = 作者手绘落 YAML 图层文件，真实 = Overture 只存引用）；渲染从「一段 2 实体」改为批量/实例化 + `class` LOD + 视口裁剪；几何不进领域 YAML；`wv-overture` 为平行叶子 crate | P1 / P2 / P3 / P4 / P5、AGENTS §2.1 / §4.2 / §5 / §9、ADR-0007、ADR-0008、ADR-0009、README §12-21 |
| [ADR-0011](ADR-0011-bevy-0-19-upgrade.md) | 渲染引擎升级到 Bevy 0.19 | 已接受（Accepted） | 引擎 0.15.3 → **0.19.1**、bevy_egui 0.32 → **0.42**（不跳 0.20）；**在 Phase 4 之前**一次做完，先建可回退点、独立提交可整体 revert；以编译器驱动迁移 + 官方迁移指南核对「能编译但行为变了」的部分；实测迁移面极小（全仓**零 `*Bundle` 用法**） | P1、AGENTS §7.1 / §8、ADR-0008 / ADR-0009 / ADR-0010 |
| [ADR-0012](ADR-0012-terrain-default-vector-flat-shading.md) | 浏览态地形默认底图由亚像素纹理切到等值线矢量平涂 | 已接受（Accepted） | 浏览态地形默认底图由 **ADR-0008 §2④** 的「亚像素纹理合成」改为**等值线矢量平涂**（marching squares → Chaikin → DP → 多边形平涂）；亚像素纹理合成保留为可切换模式（plan v0.1 D3 的 C 类）、瓦片图集保留为 B 类；ADR-0008 其它七项决策继续有效 | AGENTS §3 / §6.4 / P1 / P2 / P3 / P4 / P5、ADR-0008（§2④ 被部分修正）、ADR-0009（LodTier 同构复用）、plan v0.1 Q11、README §12-14(b) |
| [ADR-0013](ADR-0013-layer-frame-binding.md) | 图层帧接通——`MapNode.layers[].z/visible/opacity` 驱动渲染 | 已接受（Accepted） | 新增 **LayerFrame**（`render/layer_frame.rs`）作为图层声明的唯一求值点：kind→渲染组映射、**可见 = YAML 声明 ∧ 会话开关**、opacity 落材质 alpha、**z 语义 v1 = 槽位偏移**（每格 0.05，clamp ±4，缺省渲染逐位不变）；标注跟随要素 kind（街名随 Road、地名随 Place）；Border/Custom 暂无消费者、EventPoint 的 z 在 Gizmos 路径不生效（待裁） | P5、AGENTS §3 / §6.4 / §7.5 V1、ADR-0008 / 0009 / 0010、GLOSSARY（图层帧/图层绑定/槽位偏移） |
| [ADR-0014](ADR-0014-mcp-http-server.md) | LLM 可控的世界编辑——HTTP/MCP 服务与 skill 表面 | 已接受（Accepted） | `wv-app` 新增 `http/` 模块（feature-gated），MCP `2025-03-26` HTTP/SSE 仅 loopback，默认端口 8732；写类 skill **必须**走 `EditCommand → mpsc → Bevy 主帧 drain_commands → History.apply`（§3.3），读类 skill 直查 `wv-core`；v0.1 上 9 个 skill（list_maps/get_map/list_places/get_place/get_state_at_time/create_map/create_place/undo/save_project），「list 返摘要 + get 返详情」双步契约（§2.5.1），硬顶 `limit ≤ 100`，filter 用 Mini-DSL（等值/子串 + AND）；MCP spec 锁版本 | P1 / P3、AGENTS §2.1 / §3 / §3.3 / §6.4 / §7.5 V1-V2 / §9 / §12、ADR-0007（外部接口先例） |
| [ADR-0015](ADR-0015-city-submap-time-versioning.md) | 城市子图的时间版本化——双层 `TimeSpan` 模型 | 已接受（Accepted） | 双层 `TimeSpan`：`MapNode.time_span`（图级时代窗口）+ `StreetSegment.time_span`（段级存在窗口）；二者皆 `Option`，缺省零迁移；可见性求值统一走 `TimelineQuery::at`（P7 唯一入口延伸）；不改 `class` / `points` / `name`；编辑经 EditCommand | P6 / P7、AGENTS §4.1 / §12、ADR-0009（子图深度）/ 0010（街道数据模型）/ 0013（图层帧求值点） |
| [ADR-0016](ADR-0016-jrpg-antique-map-default-theme.md) | 古舆图主题定稿为默认地图观感（日间默认 + 暮色切换） | 已接受（Accepted） | 日间古舆图色板（宣纸底 `#F2EAD8` 一族）重写 `colors.rs` 成**默认观感**；新增 **`MapPalette { Daylight, Dusk }`** 与 ViewMode **正交**（暮色可切、`ui-layout.yaml` 持久化）；**`ViewMode::JRPG` 移除**（从未暴露，语义回归「技法+图层」）；色板载体仍 Rust 常量、palette.yaml 显式延期（README §12-30）；硬编码色全部令牌化。技法层不动（ADR-0012 正交）。**⚠️ 日间地形十色与日间淡化强度已被 [ADR-0035](ADR-0035-jrpg-saturated-daylight-palette.md) 取代**（2026-10-18）；暮色旧纸与其余令牌继续有效 | P3 / P5、AGENTS §6.4 / §12、ADR-0008（§2② 色板语义被显式取代）/ 0012 / 0013、ART_STYLE_SPEC §1.4.4（历史）/ §1.4.5（现行）、GLOSSARY（古舆图/暮色旧纸/地图主题/纸面雾霭/纸张噪点/文字晕边） |
| [ADR-0017](ADR-0017-label-layer.md) | 标签层——实体在地图上的文本投影 | 已接受（Accepted） | 标签 = 实体在地图上的**派生**（非作者手摆文本）；`LayerKind::Label` 进 `LayerFrame` 独立渲染组（Q1）；图层文件只存配置不存文本；可见性走单一函数 `label_visible_at`（与 `street_visible_at` 同构）；字号阶梯绑档位；AABB 碰撞 v0.1 + `max_labels=200`（Q4）；街名标注全档接管（Q3）；点击派发选中（`SelectedEntity` = 会话状态，不走 EditCommand）。**原编号 ADR-0016（分支起草），合并时因撞号重编号**（文件头追记） | P1 / P3 / P4 / P5 / P6 / P7、AGENTS §3 / §3.3 / §7.5 V2、ADR-0009（LodTier）/ 0010（街道名先例）/ 0013（LayerFrame + 主题）/ 0015（time_span 联合过滤）、ADR-0016（编号冲突，古舆图主题） |
| [ADR-0018](ADR-0018-i18n.md) | UI 多语言（zh/en）——手写键查表 + ui-layout.yaml 持久化 | 已接受（Accepted） | 新增 `wv-app::i18n`：`Locale { Zh, En }`（`Default = Zh`）+ `I18nKey` 枚举（80-110 变体）+ `t()` 直返 `&'static str`；持久化复用 `ui_layout::LayoutFile.language: Option<String>`（与 theme/density/map_theme 同口径）；`CurrentLocale` Resource 默认 `Zh`，打开工程时回灌；顶栏右侧增 EN/中 切换器；CLI（wv + overture）纳入；HTTP/Skill 仅 Claude Desktop 可见文字纳入，LLM 协议纯 ASCII 字段保持常量。无新依赖 | P1 / P2、AGENTS §2.1 / §9 / §6.4、ADR-0014（skill 协议字段稳定性）/ 0016（顶栏右侧区）、`ui_layout.rs:7-22`（UI 偏好纪律） |
| [ADR-0019](ADR-0019-bundled-ui-fonts.md) | 界面字体打包——拉丁 Inter + 中文 Noto Sans SC（编译期嵌入） | 提议（Proposed，待用户确认转已接受） | `include_bytes!` 嵌入两份 OFL 字体（`assets/fonts/`）：拉丁 Inter-Regular、中文 Noto Sans SC-Regular（Open Sans 无 CJK 字形，取配套物）；egui `Proportional = [Inter, NotoSC, 内置]` 逐字形回退、`Monospace` 数字仍走系统等宽；Bevy `Text2d` 用 NotoSC 单句柄；系统 CJK 探测删除、等宽探测保留；只用 Regular（600 字重接线为独立任务）。**追记 2026-10-20**：追加第三份字体霞鹜文楷 v1.530（OFL-1.1，ADR-0042 D4 画布标签书法体，UI 不用）。**修正 `ART_STYLE_SPEC_v0.1` §4.2 硬约束 2**（「必须系统字体栈」→「打包授权字体」，文内已追记） | P1 / P2、AGENTS §6.3 / §6.4 / §9、ART_STYLE_SPEC §4.2（被修正）、ADR-0016（LOCKED SHELL，正交）/ 0018（回退结构兼容）、`assets/fonts/README.md` |
| [ADR-0020](ADR-0020-wv-render-crate.md) | 地图渲染拆为独立 crate `wv-render` | 已接受（Accepted） | render 模块 15359 行（占 wv-app 37.9%）独立为平行 leaf crate；`wv-edit` / `wv-render` 互不依赖；新增 `RenderInputs` trait（Bevy-native 注入）；强制修复两处越界写（measurement.rs 持 `EditHistory` / streets.rs 构造 `EditCommand`）改走 intent dispatcher；theme/{palette,colors,colors_google,colors_dusk} 整搬并由 wv-render 重导出；**纯物理搬迁**，零 Recipe/Treatment 抽象（留待 ADR-0021） | P5、AGENTS §2 / §2.1 / §2.3 / §3.3 / §7.1、ADR-0013（LayerFrame 求值点）/ 0016（MapPalette 与 ViewMode 正交）/ 0017（标签独立渲染组） |
| [ADR-0021](ADR-0021-render-recipe-and-treatment.md) | 表达配方（RenderRecipe）与装饰族（Treatment） | 已接受（Accepted，2026-10-17） | **D1 表达配方**——`recipe.rs` 落地：`RenderRecipe` Resource（20 PassId）+ `MODE_TABLE` 模式×pass 真值表 + PreUpdate 单点求值 + LayerFrame 叠加，4 个"该不该画"mode 散点（buildings / faction / atmosphere / isoline）迁移收口，其余 ViewMode match 经核对属选色逻辑归 ThemePack 方向；**D2 装饰族——被 [ADR-0023](ADR-0023-theme-pack-injection-contract.md) D4 修订**：`TreatmentFlags` 归入 ThemePack（HandDrawn/EMPTY 两极 const，9 旗标已 gate 接线），不执行原文"独立轴"；6 项待裁随接受收敛 | P3（潜力，推迟 YAML 化）/ P5、AGENTS §3 / §6.4 / §12、ADR-0012 / 0013（kind 轴单点）/ 0016（轴正交先例）/ 0017 / 0020（§2.8 留项）/ **0023（D2 修订来源）** |
| [ADR-0022](ADR-0022-city-submap-spatial-form.md) | 城市子图的空间形态——轮廓、平移与外部导入 | 已接受（Accepted，2026-10-16） | 五项决定（D1–D5）：**D1** 城市轮廓 `MapNode.outline_polygon: Option<GeometryData>` 落子图 LOCAL 坐标、父图经 `resolve_frame_in` 平移渲染、缺省运行时退化为点不写盘；**D2** `MoveCityCommand` 同步 origin + outline 双字段、子图 LOCAL 与 Place/Track 不动；**D3** `AddCityCommand` 一次性复制父图河段入子图后独立；**D4** 沿用 `unit_per_km` 跨级不联动 + V1 校验提示、外部导入经既有 `geographic_source.bbox` 投影默认 equirectangular、**不引入** `geo_anchor`；**D5** 新增 3 个 EditCommand；M1/M2/M3 分期各自可 revert | P1 / P2 / P3 / P4 / P6、AGENTS §3 / §4.2 / §4.3 / §7.5 V1-V2、ADR-0007（Overture / `geographic_source`）/ 0009（多尺度 + Extent.origin）/ 0010（街道）/ 0013（图层帧）/ 0015（时间版本化）/ 0020（unit_per_km） |
| [ADR-0023](ADR-0023-theme-pack-injection-contract.md) | 主题包（Theme Pack）注入契约——地图渲染可复用与游戏风格注入的统一接入点 | 已接受（Accepted，2026-10-16） | 五项决定（D1–D5）：**D1** `ThemePack` 聚合 `PaletteColors` + `TreatmentFlags` + `Option<TileAtlasSpec>`，挂为 `ActiveThemePack` Resource；**D2** 开放注册用 cargo 多 crate + 静态切片（`&'static ThemePack`）+ `ThemePackRegistry` trait，**不**用 `Box<dyn>` 动态分发；**D3** 持久化键沿用 `ui-layout.yaml::map_theme` 字段名（零迁移），语义升格为 ThemePack id，未知 id V1 兜底 + 状态栏告警，**运行时跟软件 / 持久化跟工程 / 不按元素混搭**；**D4** 重新定位 ADR-0021 D2 的 Treatment 归入 ThemePack（而非独立轴）；**D5** Plugin 拆分 `MapRenderPlugin` / `EditorOverlayPlugin` / `MapShellPlugin`。M1（commit `4174e32`）+ M2（`4be00c6`）已落地：`wv-theme-chinese-ink` 示例 crate + `game_consumer` 验收（2 packs 合并、游戏只装 MapRender+MapShell）；M3 YAML 化推迟（README §12-30） | P3 / P5、AGENTS §2 / §3 / §6.4 / §7.5 V1 / §12、ADR-0012 / 0013（kind 轴单点）/ 0016（MapPalette 调色板轴被 ThemePack 吸收）/ 0020（§2.8 留项接续）/ **0021（D1 独立 / D2 归入本 ADR）**、`docs/STYLE_SWAPPABLE_ARCHITECTURE.md`（契约骨架来源，§5 C 显式偏离项） |
| [ADR-0024](ADR-0024-vehicle-daily-speed-unit.md) | 交通工具「有效日速度」单位修正 —— `speed × daily_hours`（km/日） | 已接受（Accepted，2026-10-17，执行期裁决） | `Vehicle::effective_daily_speed` 由 `speed × mult × (hours/24)`（实为 km/h）改为 `speed × mult × hours`（km/日）；`segment_days` 唯一入口不变、schema 零迁移；既有行程天数结论缩小约 24 倍（原值为单位错误）；受影响测试按手算新值更新 | AGENTS §4.2 / §6.4、README §12-39、`docs/superpowers/plans/2026-10-17-continent-scale-hint.md` |
| [ADR-0025](ADR-0025-selection-first-tooling.md) | 选择优先工具架构 —— 选择工具 + 意图菜单 + Pen 方法（大陆圈画/城市落点退役） | 已接受（Accepted，2026-10-17，用户对话拍板；**§2 D1「意图生效即选区清空」被 [ADR-0033](ADR-0033-selection-retained-after-intent.md) 取代**） | 五项决定：**D1** 选择优先模型（圈区域 → 底部意图菜单 → 意图生效清空；目录按图级过滤）；**D2** Pen 方法共享件（`SelectionState` + `selection` 模块，Figma pen 手感全集，命中优先级 锚点/边 > 图标下钻 > 空白，Drawing 态 drill_pick 全让位）；**D3** 意图目录 v1=生成大陆（选区喂八参面板）+ 城市（M2）+ 地形四意图（M3 掩膜一键）+ 森林（M4 骑覆盖物 v1）；**D4** 城市 portal 收敛（多边形只当位置圈、质心落点、面积定 16:9 画布、自动钻入）；**D5** `ContinentSketch`/`CityToolState` 两族退役，撤销回驻留改单槽位让位 | P6（宏命令可撤销）/ AGENTS §2.1 / §3 / §7.5、GLOSSARY「选择工具与意图菜单」、ADR-0022（portal 承接）、spec `2026-10-17-select-tool-pen-intent-design.md` |
| [ADR-0026](ADR-0026-wv-ui-ui-widget-crate.md) | 界面控件层独立成 crate（`wv-ui`）—— egui 控件 + 界面令牌的编译级隔离 | 已接受（Accepted，2026-10-17，用户对话拍板） | 六项决定：**D1** 新建平行叶子 crate `wv-ui`（迁入 ui_tokens/ui_theme/UiShell/widgets 全部 + tool_icons 资产，约 4600 行）；**D2** 依赖白名单 bevy+bevy_egui+egui_extras 均**非 optional**——控件层本体就是 egui，标准 `cargo test --workspace` 从此天然编译控件层；**D3** 域件留 wv-app（`float_window` Panel 包装、`terrain_grid`+地形两 helper），旧路径经 shim 零改动；**D4** 世界色一致性测试（§4.7 同色相守卫）随域移到 wv-app，断言 `wv_ui::ui_tokens` × `crate::theme::colors`；**D5** 边界口径：egui 控件与界面令牌 = wv-ui，地图渲染/GPU 主题/打包字体 = wv-render，二者互不依赖；**D6** `app-test`/`app-clippy` 保留覆盖 panel 层，控件层盲区消除 | P3 / P5、AGENTS §2 / §2.2 / §7.1 / §8、ADR-0019 / **0020（同款方法论）** / 0023（复用动机先例） |
| [ADR-0027](ADR-0027-faction-layer-multi-compatible.md) | 势力层多兼容实现（FactionLayer / Membership / 声称实控） | 已接受（Accepted，2026-10-18，用户对话拍板） | 把 ADR-0001 的三层模型落到扁平代码：`FactionLayer`（维度，数据非枚举）+ `Faction.layer/rank/seat/memberships` + `Membership { target, kind, span }`（**`to` 改名 `target` 修 serde 冲突**）+ `GeometrySpan.relation`（claim/control）；领土仍存 `World.faction_geometry`；势力图一次填一个层、其他层只画边界；旧 YAML 零迁移、默认层内存合成不写盘；V1 只诊断不阻止 | P4 / P6 / P7、V1-V2、AGENTS §3 / §4.1 / §6.4、ADR-0001 / 0013 / 0021 / 0025、GLOSSARY（势力层/势力归属/声称·实控） |
| [ADR-0028](ADR-0028-faction-division-and-territory.md) | 势力领地表示与划分语义（势力交互 M1） | 已接受（Accepted，2026-10-18） | 收敛 spec §7 Q1/Q2/Q3/Q8：领地=生成即存储（划分时一次生成新势力环+宿主余部并双双持久化，渲染仍只读 span，ADR-0027 D2 不动摇）；除法=同一刀切两沿（DivideFactionCommand 单事务原子 undo，余部碎块化 v1 拒绝）；GeometryData 新增 PolygonWithHoles（国中国必然产生环形余部）；空势力 v1=派生补集不建实体；贴地吸附=水陆界面/河链骨架/山脊峰值近似三族，无果退化直线+提示，湖面归空势力 | P3/P4/P6/P7、V1、ADR-0027 D2、GLOSSARY（划分/贴地·越地/地物线）、spec 2026-10-18-world-from-zero-map-design §5+追记一 |
| [ADR-0029](ADR-0029-biome-marker.md) | 生物群系标记（命名段）存储与生成语义 | 已接受（Accepted，2026-10-18） | Place 扩展 `biome`/`envelope` 两字段（不新建实体集合）；边界=生成快照存 place_geometry（重圈重生成，不逐帧派生）；「识别或生成」自动二选一（占包络 ≥8% 且 ≥4 格 → 识别，不写格子；否则写群系+环提取）；湖泊走 D14 语义派（水面=包络中位海拔，不挖碗），RIVER 群系留 M2；群系→主题 feature 词汇过渡映射（森林族→4，其余 None）。**⚠️ 其 D5（过渡桥）已被 [ADR-0032](ADR-0032-place-vocab-unify.md) 取代**——词汇统一收口，其余决定不动摇 | D9/D12/D13/D14/D16/D18、ADR-0028、V1、GLOSSARY（生物群系标记） |
| [ADR-0030](ADR-0030-poi-rank-and-density-budget.md) | POI 呈现级别（rank）与屏幕密度预算 | 提议（Proposed，2026-10-18） | Place 扩展可选 `rank`（0 网络节点/1 普通/2 市镇/3 国家，None 按旧两档折算 w/h→3 否则 1，零迁移）；呈现门槛=档位×最低级别硬门（Nation≥3/Region≥2/Edit 全见），删非城市淡出带；密度预算 POI_MAX_DOTS=400 按 (rank,is_city,名字长) 裁剪（纯渲染投影）；POI 按材质合批 ≤4 mesh + 资产原地复用；道路=段列缓存+线宽量化 4%+mesh 复用；标签候选走锚定桶+视口+级别（O(n²) 输入有界化） | P3/P6/P7/V1、ADR-0022/0027/0029（同扩 Place 零冲突）、GLOSSARY（呈现级别/屏幕密度预算/线宽量化） |
| [ADR-0031](ADR-0031-terrain-decor.md)（追记一） | 地形装饰——栅格群系驱动的程序化符号覆盖层 | 已接受（Accepted，2026-10-18，用户对话拍板） | 六类群系（森林/平原/丘陵/山地/雪原/沼泽）按**活动栅格 biome** 确定性采样演绎成程序化符号（splitmix64 抖动，无 rand）；LOD 三档（Nation 不画 / Region 46px / Edit 26px 步距）；渲染六桶合批 + `Assets::get_mut` 原地改写（ADR-0030 D4 纪律）；雪帽跨桶路由；`PassId::TerrainDecor` 接 MODE_TABLE + terrain 级联；符号颜色 v1 常量、主题化随 0023-M3。**追记一（2026-10-22）**：森林桶保留率改 = **密度场 × 主题增益**（群系驱动密度场 `wv_terrain::forest::generate_forest_density_from_biome` + `DecorRecipe.forest_density_gain`）——「哪里有林」是地形事实、「多密」是主题参数；**不推翻 D5**（符号仍程序化，无位图瓦片） | P3 / P5 / P7、AGENTS §3 / §6.4、ADR-0025（覆盖物前身）/ 0029（biome 数据源）/ 0030（合批纪律）/ 0032 D2（FeatureRule 字段冻结→密度只加 DecorRecipe）/ 0048 追记二（配方住主题层）、ART_STYLE_SPEC §3.6、GLOSSARY（森林密度场 / 聚集噪声） |
| [ADR-0032](ADR-0032-place-vocab-unify.md) | Place 地理事实词汇统一（feature → biome 收口，群系族规则键） | 已接受（Accepted，2026-10-18，用户对话拍板） | `biome`（冻结群系表）为唯一地理事实词汇：`feature` 降级 serde 只读脚手架，加载时按 `place_feature_to_biome` 表迁移（{2,3,4,8,9} 与格子侧同值逐位不变，{0,1,5,6,7} 升级真实群系）+ 诊断提示（内存驻留 feature 作证据，保存后自消散）、写回只写 biome、迁移窗口一个版本周期；主题规则键改**群系族**（`BiomeFamily` 落 wv-terrain::biome，21 编号→8 族全覆盖、与 display_kind 槽位对齐），`FeatureOverlaySet` 全族字段一次定型；渲染三臂桥塌缩为一行；死代码 `add_feature_place_now` 删除；round-trip 契约显式修订（迁移收敛性） | spec D18 + 追记四、ADR-0029（D5 被取代）/ 0023（主题包兼容）/ 0030 / 0031、V1、GLOSSARY（群系族） |
| [ADR-0033](ADR-0033-selection-retained-after-intent.md) | 意图生效后保留选区（推翻「意图生效即选区清空」） | 已接受（Accepted，2026-10-18，用户选定方案 A） | 不切图的全部意图（大陆 / 地形四意图 / 森林 / 划分势力）成功后**不再主动清空选区**；点空白 / Esc / 切工具 / 切图仍清；删除 `ContinentGenerationStash` / `ContinentGenerationState` / 撤销回驻留判定（选区从未离开）；撤销/重做只回滚世界数据、不触碰选区；城市意图因自动钻入随切图清空 | P6（宏命令仍可撤销）/ AGENTS §6.4 / §7.5 V2、ADR-0025（§2 D1 子句被取代）、spec `2026-10-17-select-tool-pen-intent-design.md`（D9 / §4 被取代） |
| [ADR-0034](ADR-0034-north-up-coordinate-convention.md) | 上北下南——地理导入与图内坐标统一为 y 向上 | 已接受（Accepted，2026-10-18，用户对话拍板） | 世界坐标 `+Y` 向上为唯一约定：导入投影改 `v=(lat-south)/range`（去 `1-`）；DEM 中间格式 row 0 = 南；UV/锚点语义改为西南原点、y 向上；`uv_to_lat_lon` 改 `south + v*range`；指南针 N 在上不再与画面矛盾；手绘地图零改动。被否决：翻转相机 / 南在上 / 仅翻数据。代价：广东工程全量重导入；xinhai-1911 名义经纬度读数翻转（已知偏差） | P3 / P7、AGENTS §6.3 / §6.4、ADR-0007 / 0022、基线 §2.15、ART_STYLE_SPEC（指南针） |
| [ADR-0035](ADR-0035-jrpg-saturated-daylight-palette.md) | 日间地形色板换 JRPG 中高饱和 A 方案 | 已接受（Accepted，2026-10-18，用户实机验收拍板「commit 颜色不错」） | 日间 `CORE_TERRAIN` 十色换中高饱和（草 `#8CB95F` / 林 `#3F7A46` / 浅水 `#57A0C0` / 深水 `#2C6389` …）；日间 `soften_keep` 0.35→**0.85**（`JRPG_SOFTEN_KEEP`——中高饱和色被 0.35 洗成灰白，实测草绿退 `#CED9AE`）；`SNOW_HIGHLIGHT` `#FBF7EB`→`#FCFEFF`。暮色旧纸、Google 色板与其余令牌全部不动，本次是纯色值 + 一个强度参数。被否决：只换色保留 0.35 / 只调强度 / 暮色同步换 / 全图统一高饱和。**⚠️ 取代 ADR-0016 的日间地形十色与日间淡化强度子句**；暮色是否跟进 JRPG 色系为待裁 | P3 / P5、AGENTS §6.3 / §6.4、ADR-0016（日间地形色被取代）/ 0023（PaletteColors 载体）、ART_STYLE_SPEC §1.4.4（历史）/ §1.4.5（现行）、commit `442ec82` |
| [ADR-0038](ADR-0038-icon-click-drill-in.md) | 进入子图的唯一手势是双击钻入图标（撤销「缩放越界进入」） | 已接受（Accepted，2026-10-19，用户对话拍板） | 进入子图唯一手势 = **双击钻入图标**；缩放撞级内界后继续往里滚只被级区间夹住停住，不再触发换级；`ZoomCross` 删 `Into` 变体（`camera_zoom` 不再做 `marker_hit`）；撞**外界**继续往外滚回父级保留（不对称是刻意的：缩小=看周边，进入=明确点选）；双击维持不改单击；**级内最大放大收浅 `LEVEL_ZOOM_SPAN` 80→32**（80× 是旧滚轮进城的遗产，深放大无内容支撑；满配栅格格子级精修偏紧为已知代价）；数据模型零改动。被否决：维持缩放换级（时机事故）/ 单击进入（与选中落笔争第一下）/ 退出也收编按钮。**修正 ADR-0009 §2 决策⑤的往里半边**，其余决策（分级区间/`Extent.origin`/`MapFrame`/飞入过渡）不动；`outline_polygon` 轮廓热区、hover 提示、按栅格分辨率自适应上限列为后续可选 | AGENTS §6.4、ADR-0009（进入方向被修正）/ 0022（轮廓）/ 0025（命中优先级不受影响）、GLOSSARY（换级修正/级缩放区间/图标钻取） |
| [ADR-0043](ADR-0043-map-dock-split-buttons.md) | 地图工具条改 11 槽 + 选择/测量两组分裂按钮 + 地形橡皮刷（重置为底图） | 已接受（Accepted，2026-10-20，用户对话拍板 + 两轮实机反馈迭代） | 工具条从「一比一平铺 Figma `3:21848` 十三项」改为 **11 槽 + 两组分裂按钮**（选择组：多边形/矩形/椭圆；测量组：线段距离/面积，**功能不变只换图标**）；新增**矩形 / 椭圆选择器**（框选手势，成形时**合成闭合点环**写进 `SelectionState.closed`，下游六个消费点零改动）；新增**地形橡皮刷**（`BrushTool::Eraser` 枚举**末位追加**，`Brush.exact` 整格直写通道把笔下格子回填成本图 `MapBase` 态：海洋 -3500 m / 陆地 +80 m，缺省兜底 Ocean）；画笔 / 生成大陆 / 磁吸三枚瓷砖**退出工具条**（`MapToolId` 变体不删，快捷键与命令面板入口保留）；小三角方位横排=右缘居中 / 纵排=下缘居中；11 槽等宽等高；网格新增 `G` 键；tooltip 全接 i18n。被否决：末尾追加三瓷砖（回到 14 项压时间轴）/ 交叉轴 `Align::Center`（Frame 按包围盒收缩 → 工具条鼓成半屏面板）/ 新增 `MeasureMode::Segment`（需求方裁定测量就两种）/ 矩形椭圆引入新数据类型（`closed` 是六处公共接口）。**数据层零迁移**（YAML / `MeasurementKind` / `World` 全不动） | AGENTS §6.4、GLOSSARY（分裂按钮/框选/底图橡皮）、ADR-0025（命中优先级：框选形状整块吃掉点击）/ 0033（选区保留不受影响） |

---

## 5. 模块路径对照（目录化重构 · 2026-10-14）

> 本节**不**是新的设计决策，而是 2026-10-14「目录级别重构」对模块路径的**事实归档**。
> ADR 正文一律不动（§6.4「只增不改」）。代码与文档看到旧路径时**优先**按本节对照。
>
> 重构内容（详见 `.trae/specs/` 落地）：
> - `wv-app` 由 28 个平铺 `.rs` 重组为 `render/` `edit/` `ui/` `theme/` 四个目录，加顶层 4 个 `ui_*` 外壳服务
> - `wv-core` 分为 `base/` `time/` `map/` `model/` `derived/` `source/` 六个目录

### wv-app 路径对照

| 旧路径 | 新路径 | 备注 |
|---|---|---|
| `wv_app::camera` | `wv_app::render::camera` | |
| `wv_app::zoom_level` | `wv_app::render::zoom_level` | |
| `wv_app::map_frame` | `wv_app::render::map_frame` | |
| `wv_app::world_projection` | `wv_app::render::world_projection` | |
| `wv_app::terrain_view` | `wv_app::render::terrain_view` | `terrain_view/` 目录整体平移 |
| `wv_app::terrain_chunks` | `wv_app::render::terrain_chunks` | |
| `wv_app::vector_layers` | `wv_app::render::vector_layers` | |
| `wv_app::streets` | `wv_app::render::streets` | |
| `wv_app::faction_overlays` | `wv_app::render::faction_overlays` | |
| `wv_app::brush_cursor` | `wv_app::render::brush_cursor` | |
| `wv_app::drill` | `wv_app::render::drill` | |
| `wv_app::editing` | `wv_app::edit::editing` | |
| `wv_app::faction_panel_actions` | `wv_app::edit::faction_panel_actions` | |
| `wv_app::validation_actions` | `wv_app::edit::validation_actions` | |
| `wv_app::diagnostics_bridge` | `wv_app::edit::diagnostics_bridge` | |
| `wv_app::colors` | `wv_app::theme::colors` | |
| `wv_app::colors_google` | `wv_app::theme::colors_google` | |
| `wv_app::colors_jrpg` | `wv_app::theme::colors_jrpg` | |
| `wv_app::fonts` | `wv_app::theme::fonts` | |
| `wv_app::ui_tokens` | `wv_app::theme::ui_tokens` | |
| `wv_app::ui_theme` | `wv_app::theme::ui_theme` | |
| `wv_app::app_bootstrap` | `wv_app::app_bootstrap` | 留顶层（装配入口） |
| `wv_app::state` | `wv_app::state` | 留顶层（横切工作区） |
| `wv_app::ui_shell` | `wv_app::ui_shell` | 留顶层（跨面板服务） |
| `wv_app::ui_command_palette` | `wv_app::ui_command_palette` | 留顶层（跨面板服务） |
| `wv_app::ui_layout` | `wv_app::ui_layout` | 留顶层（跨面板服务） |
| `wv_app::ui_time` | `wv_app::ui_time` | 留顶层（跨面板服务） |
| `wv_app::overture_cli` | `wv_app::overture_cli` | 留顶层（CLI 子命令） |
| `wv_app::ui` | `wv_app::ui` | 目录（已存在） |
| `crates/wv-app/src/<xxx>.rs` | `crates/wv-app/src/<render\|edit\|theme>/<xxx>.rs` | 物理位置（除上表「留顶层」外） |

### wv-core 路径对照

**保路径（B1 承诺）**：AGENTS.md §3 与文档直接引用的顶层路径**全部可达**。
- 类型层：`wv_core::Color` / `wv_core::Id` / `wv_core::TimeSpan` / `wv_core::MapNode` / `wv_core::Track` / …（由 `lib.rs` 顶层 `pub use` 维持）
- 模块层：`wv_core::time` / `wv_core::map` / `wv_core::track` / `wv_core::coord` / `wv_core::distance` / `wv_core::unit` / `wv_core::character` / `wv_core::event` / `wv_core::relation` / `wv_core::faction` / `wv_core::encounter` / `wv_core::situation` / `wv_core::narration` / `wv_core::street` / `wv_core::overture_id` / `wv_core::geographic_source` / `wv_core::geometry` / `wv_core::color` / `wv_core::id` / `wv_core::error`（由 `lib.rs` 顶层 `pub use ::module` 维持）

**物理位置**（与上表路径都仍然可达，下表是「源文件在哪」）：

| 旧物理位置 | 新物理位置 | 顶层引用 |
|---|---|---|
| `crates/wv-core/src/id.rs` | `crates/wv-core/src/base/id.rs` | `wv_core::id` / `wv_core::Id` |
| `crates/wv-core/src/error.rs` | `crates/wv-core/src/base/error.rs` | `wv_core::error` / `wv_core::CoreError` |
| `crates/wv-core/src/geometry.rs` | `crates/wv-core/src/base/geometry.rs` | `wv_core::geometry` / `wv_core::Point` |
| `crates/wv-core/src/color.rs` | `crates/wv-core/src/base/color.rs` | `wv_core::color` / `wv_core::Color` |
| `crates/wv-core/src/time.rs` | `crates/wv-core/src/time/mod.rs` | `wv_core::time::TimelineQuery` 等 |
| `crates/wv-core/src/track.rs` | `crates/wv-core/src/time/track.rs` | `wv_core::track` / `wv_core::track::Track` |
| `crates/wv-core/src/map.rs` | `crates/wv-core/src/map/mod.rs` | `wv_core::map::MapNode` 等 |
| `crates/wv-core/src/coord.rs` | `crates/wv-core/src/map/coord.rs` | `wv_core::coord` / `wv_core::coord::MapFrame` |
| `crates/wv-core/src/distance.rs` | `crates/wv-core/src/map/distance.rs` | `wv_core::distance` / `wv_core::distance::DistanceGraph` |
| `crates/wv-core/src/unit.rs` | `crates/wv-core/src/map/unit.rs` | `wv_core::unit` / `wv_core::Unit` |
| `crates/wv-core/src/character.rs` | `crates/wv-core/src/model/character.rs` | `wv_core::character` / `wv_core::Character` |
| `crates/wv-core/src/event.rs` | `crates/wv-core/src/model/event.rs` | `wv_core::event` / `wv_core::Event` |
| `crates/wv-core/src/relation.rs` | `crates/wv-core/src/model/relation.rs` | `wv_core::relation` |
| `crates/wv-core/src/faction.rs` | `crates/wv-core/src/model/faction.rs` | `wv_core::faction` / `wv_core::Faction` |
| `crates/wv-core/src/encounter.rs` | `crates/wv-core/src/derived/encounter.rs` | `wv_core::encounter` / `wv_core::detect_encounter_hints` |
| `crates/wv-core/src/situation.rs` | `crates/wv-core/src/derived/situation.rs` | `wv_core::situation` / `wv_core::Situation` |
| `crates/wv-core/src/narration.rs` | `crates/wv-core/src/derived/narration.rs` | `wv_core::narration` / `wv_core::Narration` |
| `crates/wv-core/src/street.rs` | `crates/wv-core/src/source/street.rs` | `wv_core::street` / `wv_core::StreetLayer` |
| `crates/wv-core/src/overture_id.rs` | `crates/wv-core/src/source/overture_id.rs` | `wv_core::overture_id` / `wv_core::OvertureId` |
| `crates/wv-core/src/geographic_source.rs` | `crates/wv-core/src/source/geographic_source.rs` | `wv_core::geographic_source` / `wv_core::GeographicSourceV1` |
| `crates/wv-core/src/terrain`（inline） | `crates/wv-core/src/lib.rs` 内联 `pub mod terrain` | `wv_core::terrain`（**未动**） |

---

## 6. 三条硬约束的归属

以下三条硬约束由创意总监提出（基线 §6.3），**逐字**写入了对应 ADR 正文，是**不可违反项**：

| # | 硬约束 | 落在哪份 ADR |
|---|---|---|
| **A1** | 链的顺序只能由时间派生，不能存序号 | ADR-0002 |
| **A2** | 轨迹必须可选、可粗略（只两端点） | ADR-0002 |
| **A3** | 跨地图事态只能靠 `Place` 的 ID 层级串联，绝不能复制子图内容 | ADR-0003 |

---

## 7. 新增 ADR 的流程

1. 复制一份现有 ADR 作为模板（推荐复制 ADR-0001，结构最完整）。
2. 分配**下一个未占用的编号**，按 §2 的命名规则建文件。
3. 状态填「**提议**」。
4. 决策者确认后改为「已接受」。
5. **更新本文件 §4 的表格**。
6. 若本 ADR 取代了旧 ADR：把旧 ADR 头部状态改为「**被取代**」并写明 `被 ADR-NNNN 取代`。**不要删改旧 ADR 的正文**。
| [ADR-0036](ADR-0036-nation-decor-and-relief.md) | 全局档地貌呈现——Nation 装饰带 + 坡度浮雕 + 雾降档 + 沙漠装饰 | 已接受（Accepted，2026-10-19，用户对话拍板） | 新增 `DecorBand::Nation`（96px 步距 / 符号 1.45×，三档都画）；栅格烘焙加坡度浮雕（邻格高差 → 明度乘子，光源左上，clamp 0.84–1.16，只陆格 / VectorFlat+RealTerrain）；`VEIL_MAX_ALPHA` 0.35→0.18；新增 `DecorKind::Desert`（沙漠/戈壁/盐滩 → 沙丘脊，第 7 桶）。数据零迁移。**⚠️ 取代 ADR-0031 D2「Nation 不画」** | P3 / P5、AGENTS §6.3 / §6.4、ADR-0031（D2 被取代）/ 0035（色板）/ 0016（雾）/ 0023（ThemePack） |
| [ADR-0037](ADR-0037-world-template-and-regen.md) | 世界模板（冷北/空海洋）+ 完全重生成 | 已接受（Accepted，2026-10-19，用户按推荐拍板） | 新建工程对话框加「世界模板」下拉（ADR-0037 D1/D2）：`Ocean`（既有默认）/ `ColdNorth`（双大陆 + 单纬度温度轴，算法落 `wv_terrain::climate`）。`WorldMeta.template: Option<WorldTemplate{kind,seed,rivers_seed}>` 随 `world.yaml` 落盘（`None` = 旧工程，零迁移）；生成模板不写 `MapNode.base`（重置会毁模板）。后续「完全重生成」走 `RegenWorldCommand`（M2-4，命令面板入口 + `terrain.bin.bak.<ts>` 留退路） | P1 / P2 / P3 / P7、ADR-0036（观感）、ADR-0029（群系表）、README §12-40 |
| [ADR-0039](ADR-0039-theme-derivation-rules.md) | 主题演绎规则——cell 属性驱动的覆盖物变体 | 已接受（Accepted，2026-10-20，用户拍板：小弧程序化 / 绿洲不进 Nation / 覆盖物不持久化） | 新增 `DecorKind::OceanWaves`（浅海带白弧散点，密度随深度衰减，深海不画）；森林按 y 轴气候变体（<0.30 针叶排 / 温带阔叶不动 / >=0.72 绿洲稀树，仅 Region+Edit）；山装饰 >=3400m 雪岩双色（帽顶白雪 + 帽缘 #C9B98A 露岩，复用 Snow 桶加第二层图元）。规则先落 Rust 常量表（纯函数），签名稳定后随 ADR-0023 M3 迁 ThemePack YAML。「关闭覆盖物显 biome 底色」确认现状收口；**数据持久化已满足（terrain.bin），渲染结果不持久化（P7 派生）** | P5 / P7、AGENTS §5、ADR-0023（M3 前哨）/ 0031（装饰系统扩展）/ 0036（Nation 档纪律） |
| [ADR-0044](ADR-0042-hand-drawn-nation-style.md)（追记三） | civ-atlas 方法论落地——水彩晕开 + 山是符号不是颜色（色板仍守 SPEC） | 已接受（Accepted，2026-10-20，用户裁定「采用 civ-atlas 的方式实现，不采用颜色体系」） | `TreatmentFlags.watercolor_bleed` 新旗标 + `cells.rs` `base_color_of` / `blended_base`（5×5 盒式模糊纯函数，烘焙/行带/patch 三契约保持）；gen_demo 删 MOUNTAINS/SNOWFIELD 分带（D13 的 rock_break/ridge_break 一并撤销）；装饰层高度优先出山形（≥1500 山 / ≥550 丘，biome 笔刷意图回退）+ **脊点吸附**（步距块内最高格，符号放到峰上，山链沿脊线成串） | P7、civ-atlas `.ref/`（方法来源）、ADR-0035（色板不变）/ 0039（装饰）/ 0042 D13（部分被取代） |
| [ADR-0043](ADR-0043-theme-crate-extraction.md) | 主题层抽为独立 crate（`wv-theme`）——默认主题与 wv-render 彻底分离 | 已接受（Accepted，2026-10-20，用户裁定「把 theme 抽取来成为一个 crate」） | 新建平行叶子 crate `wv-theme`（依赖仅 `bevy` 非 optional + `wv-terrain` 取 `BiomeFamily`）：持色板 / 装饰旗标 / 覆盖物**数据契约** / `ThemePack` 契约 / 注册表 / topographic 调色。`wv-render` 侧只留**重导出**（`crate::theme::colors::X` 等路径零改动）+ 渲染运行时（`fonts` 装配、`forest_sites` 采样、`overlay_far_blend` 分带）；`overlays.rs` 按性质切契约/运行时两半。`wv-render → wv-theme` 为 **optional**（随 `app` feature，不带 feature 仍零 Bevy 树）。`wv-theme-chinese-ink` 改依赖 `wv-theme`，**不再依赖 wv-render**——「加新风格 = 加新 crate、零改渲染层源码」成立。egui 桥接 `palette_to_color32` 迁 wv-app（egui 是界面关注点），路径随迁 | P4 / P7、ADR-0020（render 抽 crate）/ 0023（ThemePack 契约）/ 0026（uv-ui 抽 crate 方法论）、AGENTS §2.1 / §2.2 |
| [ADR-0048](ADR-0048-ocean-shore-presentation.md) | 海洋表现——离岸距离场、近岸浅滩带、离岸墨线与水面纸纹 | 已接受（Accepted，2026-10-21，用户裁定「先抽 SeaRecipe 再调配方」） | **S0 调参**：`water_depth_t` 线性→平方曲线（浅水段动态范围 ×3；修订 ADR-0035 锚值 `JRPG_SOFTEN_KEEP` 0.85→0.92、ADR-0036 锚值 `VEIL_MAX_ALPHA` 0.18→0.12）｜**S1 距离场**：新 `wv_terrain::shore`（切比雪夫格距 8 连通多源 BFS，截断 12 格，**有界支撑**→patch 脏区按支撑半径膨胀即保「局部=全图」逐字节一致）｜**S2 浅滩带**：离岸 ≤6 格向主题 `water_shore` 偏移（新增色板槽位 + `TreatmentFlags.shore_shallow`）｜**S2.5 配方抽取**：新 `wv-theme::sea::SeaRecipe`（深度量程/指数、浅滩带宽/峰值、水纹振幅、离岸波纹圈、抖动、墨色），渲染 API 收敛为单一 `&ThemePack` 入参，**逐位不变**（烘焙 PNG SHA256 对拍）｜**P0 水面纸纹**（逐像素双倍频值噪声，晕开之后施加；水域 std 0.0→0.8~1.7、唯一色 1→19）｜**P0 离岸墨线**（距离场双线性采样 + 三圈高斯带 4/9/15 格 + 值噪声抖动打散同心圆；场截断半径改由 `field_radius()` 推导 12→19，patch 膨胀自动跟随）｜**P1 浅滩峰值** 0.85→0.55｜`wv-theme-chinese-ink` 自带配方（零改渲染层源码，ADR-0043 注入契约在海面成立）。**S3 白色浪花带未做**（方向预登记） | P7（演绎不落盘）、ADR-0035（色板锚值被修订）/ 0036（雾锚值被修订）/ 0039（主题演绎）/ 0043（ThemePack 注入契约）/ 0044（水彩晕开——tint 同收 base_color_of）、civ-atlas `.ref/`（`RIPPLE_D=[4,9,15]`、`RIPPLE_SIG=0.55`、`RIPPLE_A=[0.42,0.30,0.18]`、`grain=1+0.035*(fiber-0.5)`）、README §12-48 |
| [ADR-0049](ADR-0049-theme-pack-switching.md) | 主题包切换链路（ADR-0023 M2 落地）| 已接受（Accepted，2026-10-21，用户裁定「go」） | **权威从 `map_palette` 迁到 `theme_pack_id`**（`MapPalette` 降级为「内置包快捷选择器」）；**解析结果挂 `ViewState.active_theme`**（不新增 Resource 参数——Bevy system 参数上限 16，`poll_open_project_dialog` 原本正好 16 个）；同步系统挂 `PreUpdate` 最前（`sync_active_theme_pack`，只在 id 变化时写）；消费点 17 处全量改读 `view.active_theme`；合成指纹 `SynthCtx` / `TerrainTexture` 从 `MapPalette` 改 **`theme_id`**（`MapPalette` 表达不了外部包）；切换入口 = 命令面板「切换地图主题」**遍历注册表所有包**（不新增 UI）；`map_theme` 存主题包 id，存量键走兼容回退（**零迁移**）；`wv-theme-chinese-ink` 提升为 wv-app **运行时依赖**。**后果**：ADR-0043「加新风格 = 加新 crate、零改渲染层源码」端到端成立——水墨包的 `SeaRecipe` / `DecorRecipe` 终于生效 | P4 / P7、ADR-0023（M2 落地）/ 0043（注入契约）/ 0048（配方）/ 0016（配色轴）、README §12-48 |
| [ADR-0050](ADR-0050-unified-inspector-panel.md) | 统一 Inspector 面板——单面板按选中对象分派属性 | 已接受（Accepted，2026-10-22，用户拍板四项裁定） | `Panel::Inspector` 成为右栏唯一属性面：页签条（复用 `wv-ui::tabs`）切「属性 / 势力层 / 事态」；属性页签按选中分派 Place / Region / Faction / Event / Cell 五类表单（Cell 仅 EditCell 工具激活时出现）。**修正 ADR-0040 决策 6**：Cell 表单回 Inspector、`Panel::CellEditor` 浮窗退役。三旧面板退役（`Panel::ALL` 12→9，枚举变体暂留兼容旧 ui-layout.yaml）。新增 `UpdatePlaceCommand`（锚点 None=不改；有地界环/折线只读——`fit_anchor_to_ring` 派生量）与 `UpdateEventCommand`（逐字段快照 revert，不碰 effects/narrations）。Cell 写入走直提并**补 `tex.mark_dirty`**（旧直提不标脏、地图不重绘——两条单测钉死）；删死链 `EditCellPending` / `CommitEditCell`。列表行点击=选中（写 `selection` 会话态）。`panel_inspector` 以元组折叠装配（20+ 资源，仓内先例 `apply_pending_commands`）。Region 父区域下拉本轮不做（v2）；驻地/参与者保持自由文本 Id | P3 / P6 / P7、ADR-0040（决策 6 被修正）/ 0025 / 0017 §2.7、基线 §1.11 P-4、设计稿 `docs/superpowers/specs/2026-10-22-unified-inspector-panel-design.md` |
| [ADR-0042](ADR-0042-hand-drawn-nation-style.md) | 手绘 Nation 表现层——山链 / 等深海 / 陆格颗粒 | 已接受（Accepted，2026-10-20，用户对比手绘参考图后拍板「开干」） | 生成端（wv-gen-demo）：山脊高斯 22→10 格 + 边缘噪声化、雪线 2250→2750 / 山地 1150→1500、气候纬度边界噪声化、深海五档渐变至 -3200。渲染端：陆格 ±4.5% 确定性颗粒（全路径同源）、`core.mountain` `#8A7A66`→`#9E9070`（**修正 ADR-0035**，SPEC §1.4.5 已追记）、山形图标加背光面、Nation 步距 96→72 / 符号 1.6×、波浪提亮。纸纹叠加当日落地（追记 D11，程序化噪声）；书法字体仍待裁 | P7、ADR-0035（D7 修正）/ 0036（步距尺寸调整）/ 0039（OceanWaves 提亮）/ 0031（图标画法） |
| [ADR-0045](ADR-0045-nature-region-model.md)（追记一/二/三/四） | 自然区域模型——覆盖物的归属层、biome 连通区分区、嵌套与合并 | 已接受（Accepted，2026-10-20，用户逐项拍板 D1–D5） | **D1** 自动划分按 **biome 连通区**（内核复用 `isoline::kind_segments` + `assemble_chains`；每格恰属一区、共边由构造保证；坑：地图外边界无线，须沿图幅矩形补边才闭合）；**D2** 覆盖物归 **region**（`Region.overlay` 开放字符串键，词表由 `ThemePack` 拥有；与 ADR-0039 的「cell 派生变体不持久化」**并存不冲突**——新增的是第二条来源，region 显式声明优先；v1 只改装饰符号，底色仍由 cell biome 决定）；**D3** 扩展现有 `Region` 实体（加 `kind: Area\|Nature` / `overlay` / `parent` / `holes`，全 serde default → 旧 YAML 零迁移；**不新开 `NatureRegion`** 避免与 `Region` 语义重叠）；**D4** 自动分区**不落盘**、命名即物化（几百个 YAML 不可接受）；**D5** 合并 = 几何并集（掩膜并 → 外环+洞）+ 旧成员变子块；另 D6 边界=快照+可重生成 / D7 空名不显示标签 / D8 最深优先 / D9 解析顺序唯一入口 / D10 ThemePack 增规则表（ADR-0023 M3 第一块）。**追记一（2026-10-20）**：覆盖物键在当前主题无规则 → **白底红斜线缺失标记**（接管底色与装饰，Z 0.078，固定诊断色，三档 LOD 全画，不落盘），**不回退**（回退 = 拿另一个变体冒充作者声明）；族规则缺失仍回退（回退色仍是该族事实，语义诚实）——两者**有意不统一**。**追记二（2026-10-20）**：实现落点表（D1–D10 全部落地）+ **间距倍率语义修正**（`spacing_scale > 1` = 更疏；「更密」在固定步距下不可达）+ 未完成项清单。**追记三（2026-10-20）**：**作者入口已接**（区域面板：名字输入 + 物化按钮 + 覆盖物键下拉 + 重生成边界 + 缺规则红提示）+ **两套原点修正**（区域多边形=图内局部 wu，格子=栅格系；子图上错位 → 换算链全程携带 `raster_to_map = map_origin - raster_origin`，2 测试钉死）。**追记四（2026-10-20）**：**区域可视化三件套**（轮廓线框 Z 0.076 / 底色覆盖=主题规则色半透明层 Z 0.064 改键即生效不进烘焙 / 候选区金色预览 Z 0.077 光标格节流）+ **作者小河交互**（左键加点 Esc 丢弃 右键收线；一个事务两条命令一次撤销整体回退）+ **作者河分池分样式**（亮青蓝 1.6px @ 0.084）| P2 / P3 / P4 / P5（不引入新图层）/ P7、ADR-0039（不推翻）/ 0023 / 0043 / 0029 / 0028、README §12-47（格子 span 待决） |
| [ADR-0046](ADR-0046-sub-cell-author-line-data.md)（追记一） | 亚格线状作者数据——手绘小河（小溪 / 小河） | 已接受（Accepted，2026-10-20，用户拍板立项） | `Place` 加 `line: Option<Vec<f32>>`（图内局部 wu 内联折线，**不新开 `rivers/` 集合**、**不给 `GeometryData` 加线变体**——untagged 裸数组匹配坑 + 面语义稀释）；锚点 `x/y` 缺省取折线中点（沿用 `polyline_midpoint` 先例）；亚格**不占格不写 biome**，与一格宽的 `Intent::River` 并存；渲染走**双供给**（派生 `rivers_seed` ∪ 作者，仿街道双供给）；河岸覆盖物归 ADR-0045 的 region 声明；三个编辑命令照街道形状；v1 线不带 span。**追记一（2026-10-20）**：实现落点表（D1–D8 全部落地）+ 三条遗留项（手绘交互 / 不融合 / 样式不可区分）| P2 / P3 / P4 / P5 / P6 / P7、ADR-0037 D5（不取代）/ 0010（双供给先例）/ 0045 |
| [ADR-0047](ADR-0047-uniform-presentation-rank.md) | 全要素统一呈现级别——道路与河流接入「rank × 档位」体系 | 已接受（Accepted，2026-10-20，用户裁定「按照你的推荐把」） | **D1** `PlaceEdge.rank: Option<u8>`（serde 缺省零迁移；`None` 按 distance_km 折算 ≥100→3 / ≥30→2 / 否则 1，冻结契约测试钉死，显式永远赢）；**D2** 统一门槛 `rank_visible_at_tier`（Nation ≥3 / Region ≥2 / Edit 全见）——ADR-0030 词表**扩表**非新表，`poi_visible_at_rank` 降级为别名；**D3** 街道经 `presentation_rank_from_lod`（0–2→3 / 3–5→2 / 6–7→1）委托统一门槛，8 lod × 3 档与旧阈值逐位等价（行为零变化）；**D4** 世界道路段列按 rank 降序 + 三档前缀切割位 + 分档指纹（缩放零重算，跨档 O(1) 切片）；**D5** 河流按折线长 ÷ 图幅宽折算（≥0.40→3 / ≥0.15→2 / 否则 1），派生河与作者河（ADR-0046 双供给）同折算同门槛。顺手修三处带病测试（cells 未关晕开 / decor 缺 RegionOverlayIndex / world_projection 夹具丢 rank 字段的合并回归） | P3 / P7、AGENTS §4.2 / §7.5 V1、ADR-0030（扩表不取代）/ 0010（阈值收口）/ 0021（模式轴正交）/ 0037 D5 / 0046、README §12-48（道路半边关闭） |

---
> **⚠️ 编号撞号备注（2026-10-20 追加）**：`ADR-0044` 已被 ADR-0042 的「追记三」占用（civ-atlas 方法论落地，commit `eef816c`，索引行指向 ADR-0042 文件），故本次两份新 ADR 从 **0045** 起（ADR-0045 自然区域 / ADR-0046 亚格线状作者数据）。上文 0042 那条备注的「后续新 ADR 从 0042 起」已过时，按 §6.3 只增不改、保留原文。引用时务必带文件名全称。


**相关文档**：`AGENTS.md`（协作规约与设计原则）、`docs/MAP_EDITOR_BASELINE_v1.md`（设计/架构基线）

> **⚠️ 编号撞号备注（2026-10-20 登记）**：`ADR-0039` 被两份文件并用——`ADR-0039-theme-derivation-rules.md`（主题演绎规则）与 `ADR-0039-topographic-viewmode.md`（Topographic 视图模式，并行会话产出，索引行见其提交历史）。两份均已接受、正文均有效；按 §6.3 只增不改，均不重编号，后续新 ADR 从 **0042** 起（0040/0041 已被 cell-inspector / 2.5D-vertex-z 占用）。引用时务必带文件名全称。
