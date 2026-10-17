# 主题覆盖物机制设计：地理事实 × 镜头分带演绎（v1 = 森林最小闭环）

> 状态：定稿（用户 2026-10-17「写计划」= 批准；D10/D11 按推荐口径落定，可推翻）
> 实施计划：`docs/superpowers/plans/2026-10-17-theme-overlay-forest.md`（Task 1–8）
> 来源：2026-10-17 brainstorming 会话（用户逐条拍板，见 §2 决策记录）
> 关联：ADR-0023（ThemePack，已接受）/ ADR-0017（Region）/ ADR-0009（分级缩放）/ ADR-0004（轨迹·单位）
> 参考：docs/CONTINUOUS_LOD_PLAN_v0.1.md（地形细节度，与本设计正交）
> 参考图：图1 = JRPG 风世界图远镜头（山脉走向 + 稀疏山形符号）；图2/3 = 水墨风 zoom in（山体晕染 + 等高线 + 田块）

---

## 1. 意图与为什么

作者要在地图上声明地理事实——「这里有一片黑森林」「这里有一条哈兰山脉」——而事实的**长相**不归作者管：同一份事实，在世界级远镜头下是符号化的（一团绿晕、沿走向两三枚山形符号，图1），拉近后是质感化的（树冠纹理、跟着等高线走的水墨山体，图2/3），且**换一套主题包就换一副长相**（JRPG 一套、水墨一套）。

> **主题口径澄清（2026-10-17）**：默认主题就是 **JRPG 古舆图**（ADR-0016），今天的 Daylight / Dusk 两包同属这一家族（Dusk 只是暗调变体）；水墨是 `wv-theme-chinese-ink` 示例包、**当前编辑器不可选**，属后续 M2。「JRPG 一套、水墨一套」是目标形态，v1 的两套覆盖物规则指的是 **Daylight / Dusk 两个古舆图变体**（D10）。

这套机制的三个分工边界（用户裁定）：

- **作者管事实**：圈范围、命名、定类型（、画走向——v2）；
- **主题管长相**：每种事实类型 × 每个镜头档的演绎规则全部住在 ThemePack 里；
- **城市编辑器管细活**：街巷建筑等更细的呈现不进本机制（ADR-0022 地盘）。

## 2. 已定决策（对话逐条拍板）

| # | 决策 | 内容 | 出处 |
|---|---|---|---|
| D1 | 装饰由谁决定 | 作者只声明事实，长相完全交主题按镜头演绎；细活归城市编辑器 | 用户 |
| D2 | 镜头模型 | **同图连续缩放**（非跨图跳转）；机制核心 = 同图内 zoom 分带；分带度量衡与 isoline `lod_bucket` 同源（屏幕像素 : 世界单位） | 用户 |
| D3 | 事实承载 | **地理事实 = Place 扩展，不新造实体**。「黑森林就是一个 Place」；事件（EventLocation）/ 足迹（waypoint）/ 行程（PlaceEdge）引用能力现成 | 用户拍板 + 查证 |
| D4 | 地界 | **每一个 Place 都有地界**（多边形 + 时间区间），与势力领地**共 `GeometrySpan` 底座、不共语义**（地界只有事实层，无声称/实控）；时间演化 = 追加 span，旧界不删 | 用户拍板 + 查证 |
| D5 | Place 新字段 | `feature: Option<TerrainKind>`（类型词汇）+ `spine: Option<折线>`（山脉走向，v2） | 设计默认（用户未反对） |
| D6 | 山脉也是 Place | 锚点 = 脊线中点、地界 = 沿脊线带状多边形、`spine` 存走向线本体 | 用户确认 |
| D7 | 创作手势 | 森林/田地 = 直接圈地界多边形；山脉 = 画走向线 + 宽度 → 带状地界自动派生（生成默认 + 可改） | 用户确认 |
| D8 | 分带分工 | **远带读地界**（符号化）、**近带读地界 ∩ 网格采样**（质感化）、中间交叉淡化；网格 kind 笔刷降级为**可选**的近景质感精化 | 设计默认（用户未反对） |
| D9 | v1 范围 | **森林最小闭环**（本设计 §3） | 用户拍板 A |
| D10 | 主题切换验收口径 | **骑现有 Daylight/Dusk 调色板开关**（`CommandId::ToggleMapPalette`）：两内置包各配一套森林规则，差异做在晕色 / 浓度 / 符号密度；不建 M2-lite 主题选择流程，跨风格族（JRPG vs 水墨）随 ADR-0023 M2 | 用户委托（按推荐，可推翻） |
| D11 | 时间演化 UI | v1 **不建时间输入 UI**：圈地界自动写开放区间 span（from = 时间轴当前日、to = None）；追加历史 span 用 YAML 手工 + 时间轴拖动验证 | 用户委托（按推荐，可推翻） |
| D12 | 美术资产来源 | **用户提供图片（路线 D）**：贴图由作者贴入会话 / 放入主题资产目录；代码不生成插画、不联网抓图。我负责资产规格、加载接线、图集打包与缺资产回退；**资产缺失或加载失败一律回退 v1 纯色符号**，绝不崩 | 用户拍板 D |
| D13 | v1 风格归属 | **JRPG 古舆图优先**：v1 的覆盖物语言 = 符号化（远带晕 + 近带树冠符号），与默认主题（ADR-0016）同族；**水墨质感演绎不在 v1**——图2/3 只作为「分带」的动机参考，不是 v1 的美术目标；水墨包可切换后再做 | 用户 2026-10-17 追问后确认 |

## 3. v1 范围与验收画面

v1 只做**森林**一个 feature 类型，跑通「事实 → 分带 → 主题换装」全管线。验收画面：

1. 作者在地图上圈一个多边形 → 命名「黑森林」→ 选类型「森林」→ 落盘；重开工程仍在。
2. 远镜头：地界内一团绿晕 + 名字标注（晕色/浓淡由主题定）。
3. 拉近：地界 ∩ 适合格子内出现树冠符号（贴图或程序生成），避开水面，位置确定性（重开不跳变）。
4. 切主题包：同一份事实换一副观感（**顶栏 Daylight/Dusk 调色板开关**，D10）——两个内置包各有一组森林规则，晕色 / 浓度 / 符号密度可见不同。
5. 时间轴：给地界追加第二条 span（**v1 用 YAML 手工编辑**，D11）→ 拖动时间轴，森林范围随时代变化。
6. 事件引用 `place = 黑森林` → 事件落点渲染与校验行为与现有 Place 一致（回归验证）。

## 4. 非目标（v1 明确不做）

- 山脉全套：`spine` 字段、带状地界派生、沿线符号排布（v2，D6/D7 已定形状）。
- **水墨 / 跨风格族演绎**：湿笔晕染、纸张渗透等质感化画法不在 v1（D13）；v1 只做 JRPG 古舆图的符号化语言。
- 田地 feature（v3 顺位）。
- **作者手动摆单棵树 / 单枚符号**——反目标：长相完全归主题，作者只定事实。
- 地界与高度场联动生成（声明森林不自动改网格；「地界兼作意图包络」记录为待裁 T2）。
- 演绎规则 YAML 化（随 ADR-0023 M3 / README §12-30 一起推迟；v1 规则是代码常量）。
- 跨图同一实体（世界图的森林与区域子图的森林是否同一 Place——P4 跨图引用，待裁 T1）。
- 城市内部细节（ADR-0022 地盘）。

**反目标（做砸了长这样）**：远镜头地图被符号堆满变乱（远带必须克制：晕 + 名）；换主题后森林消失或错位（缺规则必须有回退）；渲染层自己拿 span 做时间比较（必须走求值查询）。

## 5. 约束

- AGENTS.md 全量：P1–P7、依赖方向、§3 渲染层不做时间比较、改世界必经 `EditCommand`。
- ADR-0023 口径：**运行时跟软件、持久化跟工程 YAML、不按元素混搭**主题参数。
- §7.5 V1/V2：校验只提示不阻止；历史（地界 span）可追加不可覆盖。
- 门槛：`cargo test/clippy/fmt --workspace` + `cargo app-test` / `app-clippy` + `bash scripts/smoke-gui.sh`（涉渲染系统与 Plugin 装配）。

## 6. 架构落点

> `[B]` = builder 可自行裁量；`[裁定]` = 本次设计替作者做的 call（用户可推翻）。

### 6.1 wv-core（纯数据 + 求值）

- `Place`（`crates/wv-core/src/map/distance.rs:93`）增字段：
  ```rust
  /// 地理事实类型（theme overlay 词汇，沿用 TerrainKind 的 u8 词汇表编号，
  /// 编号→名称→演绎规则映射由 ThemePack 承载）。None = 普通地点（现状）。
  /// 注意：wv-core 不依赖 wv-terrain（§2.1），故此处是裸 `u8`——语义上的
  /// TerrainKind 编号，词汇对齐表见待裁 T4。
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub feature: Option<u8>,
  ```
  `[裁定]` 字段名用 `feature` 不用 `kind`——避免与 `TerrainCell.kind` 语义混淆，值域同一张词汇表（对齐表见待裁 T4）。零迁移：旧 YAML 不写该字段。
- **锚点 / 范围的维护 `[裁定]`**：创建命令写入 `map` + `x/y`（地界质心）+ `w/h`（地界包围盒）作为**存储值**；顶点编辑命令负责同步（ADR-0022 D2「双字段同步」先例）。地界多边形是几何真值，`x/y/w/h` 是被维护的投影——`EventLocation::resolve`、POI / 标签管线（`place_label_kept_at_tier` 的远档存活判定）今天都读这四个字段，**不改它们的读法**。`PlaceDot` 对 `feature = Some` 的 Place 抑制圆点（§6.4）。
- `World`（`crates/wv-core/src/lib.rs:188` 旁）增并列集合：
  ```rust
  /// Place 的地界几何（GeometrySpan，owner = Place.id）。与 faction_geometry
  /// 同构并列，不泛化共用集合（泛化统一 territory 底座留待将来 ADR）。[裁定]
  pub place_geometry: Vec<map::GeometrySpan>,
  ```
- 求值查询（镜像 `factions_at`，`crates/wv-core/src/model/faction.rs:75` 的全部语义）：
  `place_territory_at(world, map, day) -> Vec<(&Place, &[f32])>`——map 过滤、`span.contains` 半开区间、孤儿 owner 跳过（V1）、仅 Polygon 变体、不排序。**渲染层唯一入口**（§3 纪律）。

### 6.2 wv-io（落盘）

- 加载：`load_dir(&root, "geometry/place")`（目录可缺失 = 空 Vec，零迁移）。
- 保存：`wants("geometry/place")` 分支 → `geometry/place/` 每条一文件，合成文件名沿用 faction 模式 `{map}__{owner}__{from}.yaml`（`crates/wv-io/src/project.rs:845` 现行实现）。
- 测试：`Place.feature` 与 `place_geometry` 的 YAML round-trip（**seas/ 漏写教训：saver 分支与测试同批提交**）。

### 6.3 wv-edit（命令）

- 圈地界手势走 **Region EditCommand 同款模式**（label-layer 已验证的多边形编辑链路）移植为 Place 版：创建带地界的 feature-Place / 修改地界顶点 / 追加地界 span（时间演化）。`[B]` 具体命令集形状与撤销粒度。
- **v1 时间语义（D11）**：创建命令写**开放区间 span**（`from` = 时间轴当前日，`to = None` = 至今有效）；历史 span 追加不做 UI，作者手改 YAML。时间区间不是圈选手势的参数。
- 命名、feature 类型选择是命令参数，不是独立状态。

### 6.4 wv-render + ThemePack（演绎）

- `ThemePack`（`crates/wv-render/src/theme/theme_pack.rs:37`）增静态字段：
  ```rust
  /// 覆盖物演绎规则：feature 词汇 → 远/近带规则（晕、符号集、密度、采样参数）。
  /// v1 为代码常量（内置两包各一套）；YAML 化随 ADR-0023 M3 一起推迟。[裁定]
  pub overlays: FeatureOverlaySet,
  ```
  `[B]` `FeatureOverlaySet` 具体形状（建议：按 `TerrainKind` 键 + 缺省回退规则；v1 只需 `forest` 条目）。**缺省回退是硬要求**：主题没给某 feature 的规则 → 至少画地界描边 + 名字，不消失（验收反目标）。
- **生效主题的取得（D10）**：覆盖物规则从 `BuiltinPacks::for_palette(view.map_palette)` 派生，跟随现有 `CommandId::ToggleMapPalette` 切换——**不新建主题选择 UI**，也不依赖当前为死资源的 `ActiveThemePack`（M2 落地后再统一接线，`[B]` 是否顺带加一个把 `ActiveThemePack` 同步成 `for_palette(...)` 的小系统）。
- `Daylight` 与 `Dusk` 两套 `forest` 规则必须在**晕色 / 浓度 / 符号尺寸或密度**上有可见差异（至少 2 项），否则验收第 4 条不合格。
- 新 `PassId::FeatureOverlay`（recipe 真值表 20→21，走「枚举加项 + PassPlan 默认 false + 真值表加列 + 测试追加行」四步）；`feature = Some` 的 Place 在 Poi 通道**抑制城市圆点、保留标签**。`[B]` 通道挂接方式。
- 分带求值：纯函数 `band_for(world_units_per_pixel) -> Band`，度量与 isoline `lod_bucket` 同源；v1 两带（Far/Near）+ 交叉淡化，中带独立形态不做。`[裁定]` 阈值默认对齐三档 `LodTier` 的中点，标定值 [B] 实测调。
- 近带树冠落位：地界 ∩ 网格采样——`kind = 森林` 的格子优先；作者未刷 kind 时按高度过滤（> 0 陆地）；确定性随机 seed = `hash(Place.id)`（重开不跳变）。`[B]` 采样密度与抖动参数放 `FeatureOverlaySet`。
- **空采样回退 `[裁定]`**：采样产出为空（图无自带栅格、或地界内无适合格子——如全海平面默认网格）时**不降级为消失**：远带晕 + 地界描边 + 名持续可见，与「主题缺规则回退」同一精神。

### 6.5 wv-app（UI + 校验）

- 圈地界工具：Region 面板交互移植 + feature 类型选择（词汇列表来自内置表）+ 命名。经 `EditCommand` 落地（§2.1 纪律）。
- 校验（V1 只提示）：feature=森林但地界内格子 kind 均非森林 → Hint 级诊断（刷不刷合法，D8）；孤儿地界（owner 不在 `distances.places`）→ Error 级提示但不阻止加载（镜像 `drill_into` 悬空处理）。

### 6.6 美术资产契约（D12）

**v1（无资产）**：符号 = 纯色 quad，晕 = 半透明多边形。管线先跑通。

**贴图接入（用户供图后，v1.1）**：

| 项 | 约定 |
|---|---|
| 存放 | `crates/wv-render/assets/overlays/<feature>/`（v1 内置两包共用；ADR-0023 M2 的外部主题资产路径缺口未解决前，外部主题包引用这里的路径） |
| 格式 | PNG，直通 alpha（非预乘），sRGB；透明背景 |
| 近景符号 | **按主题分版本**：Daylight 用深墨稿、Dusk 用浅墨稿（`tree_a.png` / `tree_a_dusk.png`）。**不能只靠 `symbol_color` 提亮**——材质 tint 是乘法，深墨乘浅色仍是深墨；暗主题必须换浅墨资产（2026-10-17 实测 tree_a 发现） |
| 尺寸 | 128×128 px 画布，符号主体占 ~80%；屏幕上以 `symbol_size_px`（约 24–40）显示 |
| 变体 | 3 个（a/b/c）避免重复；不足 3 个时按 `hash(site) % 变体数` 轮换，并对 quad 做确定性旋转 |
| 远景晕纹理（可选） | **单张图章**，按地界包围盒拉伸（不做无缝平铺）；优先提供带林间空地的整幅林地（A5 这类），逐通道去纸底后空地透出地图 |
| 加载 | `AssetServer::load` 句柄缓存在覆盖层 Resource；**加载中/失败 → 回退纯色**（warn 一条，不阻塞） |
| 授权 | 资产由作者提供并自负授权；本项目不联网、不内置第三方素材 |

`FeatureRule` 相应预留：`symbol_texture: Option<&'static str>`、`halo_texture: Option<&'static str>`（v1 恒 `None`）。

**归一化工具**：`scripts/normalize_overlay_asset.py`（白底转 alpha、按行密度切线去地面阴影/水印、118px 内适配到 128 画布、`--light` 出浅墨版）。**彩色稿按原色使用**（渲染时 tint 白）；彩色稿的 `--alpha-gain` 取 1.9（墨线稿用默认 1.35），否则浅色水彩会被当成半透明、在纸底上发灰。2026-10-17 首批入库：`tree_a.png`（Daylight，源图裁 `x 416–1634 / y 433–1372`）+ `tree_a_dusk.png`；`tree_b.png`（彩色单株，裁 `x 216–1843 / y 203–1815`，gain 1.9）+ `tree_b_dusk.png`；`tree_c.png`（针叶三株，裁 `x 480–1574 / y 324–1340`，gain 1.7，切线在地面阴影上沿）+ `tree_c_dusk.png`；`forest_patch.png`（A4 混交林团，裁 `x 264–1808 / y 224–1800`，gain 1.4，**自带林地地面**——不进 tree 轮换，留作远景地界 stamp / 稀疏密林点缀的候选，待裁）；`forest_fill.png`（A5 整幅林地，裁 `0–1900²` 去水印，逐通道去纸底，512×512，**按地界包围盒拉伸的单张图章**）。**横向符号（山脉）例外**：`mountain_a.png` / `mountain_a_dusk.png`（A6，裁 `x 230–1900 / y 620–1340`，512² 画布、内容保持自然宽高比 ~2.3:1；v2 使用）。三个 dusk 版均为派生浅墨占位，正式浅墨稿可替换。

### 6.6.1 生图提示词（外部工具用，2026-10-17 用户要求落档）

**主题归属**：

- **默认主题 = JRPG 古舆图**（ADR-0016：`builtin::DEFAULT`「日间古舆图」；`DUSK`「暮色旧纸」是其暗调变体）。今天编辑器里可选且可得的只有这两包。
- **v1 用下面「A. JRPG 古舆图（v1 正式组）」**；「B. 水墨（保留）」是 `wv-theme-chinese-ink` 将来可切换后的素材，**不参与 v1 验收**（D13）。
- 单色墨稿**按主题分版本**：Daylight 用深墨稿、Dusk 用浅墨稿。乘法 tint 只能压暗、不能提亮——同一份深墨稿在暗主题上换色不会变亮（2026-10-17 实测）。

**通用约束（所有提示词共享）**

- 输出**单色墨稿优先**（深墨 / 深褐灰），主题层用 `symbol_color` 染色；彩色图也接受，按原色用。
- 视角统一：俯视 / 微斜俯视；光源统一**左上**（与 ART_STYLE_SPEC §1.5 同一口径）。
- **不要**投影、地面、文字、边框、水印、说明书式排版。
- 背景：模型若支持 alpha 直接导出透明 PNG；不支持的用**纯白背景**生成，由我裁切 + 去白。
- 尺寸：按模型原生方形输出（1024² 之类）即可，我统一缩到 128×128。
- 变体：同一提示词 + 固定 seed 改形状描述生成 2–4 张；不同变体只改轮廓，不改风格与光线。

**A. JRPG 古舆图（v1 正式组，2026-10-17 重新生成）**

> 风格词保持完全一致，只改「种类 / 组合」词来出变体——三张图放进同一片森林才不打架。

**A1. 阔叶树丛（主用：远看近看都成立）**

```text
Vintage hand-drawn cartography symbol: a small clump of three overlapping broadleaf tree canopies
seen from above with a slight tilt, sepia-brown ink linework, stippled leaf texture and fine hatching,
muted low-saturation antique palette, isolated on a plain white background,
soft consistent light from the upper left, centered, no cast shadow, no ground,
no text, no border, no frame, clean silhouette, square composition
```

**A2. 单株阔叶树（稀疏林地）**

```text
Vintage hand-drawn cartography symbol: a single broadleaf tree canopy seen from above with a slight tilt,
irregular rounded crown, sepia-brown ink linework with stippled foliage and fine hatching,
muted low-saturation antique palette, isolated on a plain white background,
light from the upper left, centered, no cast shadow, no ground,
no text, no border, no frame, clean silhouette, square composition
```

**A3. 针叶树丛（山地森林）**

```text
Vintage hand-drawn cartography symbol: a small cluster of three conifer trees seen from above with a slight tilt,
layered pine canopies, sepia-brown ink linework with fine hatching,
muted low-saturation antique palette, isolated on a plain white background,
light from the upper left, centered, no cast shadow, no ground,
no text, no border, no frame, clean silhouette, square composition
```

**A4. 混交林团（密林边缘）**

```text
Vintage hand-drawn cartography symbol: a compact mixed forest patch with several overlapping
broadleaf and conifer canopies seen from above, sepia-brown ink linework, stippled and hatched foliage,
muted low-saturation antique palette, tidy irregular silhouette, isolated on a plain white background,
light from the upper left, centered, no cast shadow, no ground,
no text, no border, no frame, square composition
```

**A5. 远带晕纹理（可选，256×256 无缝；替代纯色晕）**

```text
Seamless tileable forest texture for a hand-drawn antique map: very light, low-contrast mottled foliage wash
in muted desaturated green-brown, faint sepia ink speckles and tiny leaf marks, subtle parchment grain,
even density across the tile, no distinct objects, no border, no text, no shadow,
seamless edges, tileable 256x256
```

**A6. 山脉符号（v2 预留）**

```text
Vintage hand-drawn cartography symbol: a small mountain range with three to five angular peaks in side profile,
sepia-brown ink linework, fine hatching on the shaded slopes, muted low-saturation antique palette,
isolated on a plain white background, light from the upper left, centered, no cast shadow,
no ground plane, no text, no border, no frame, clean silhouette, square composition
```

**B. 水墨（保留，v2 ink 包可切换后使用；不参与 v1 验收）**

**B1. 水墨树冠**

```text
Chinese ink-wash painting style forest canopy symbol for a hand-drawn fantasy map,
top-down slightly tilted view, monochrome deep ink with soft wet-brush bleed and sparse branch strokes,
subtle paper grain, transparent background, consistent light from upper left,
centered isolated game-map asset, no ground, no cast shadow, no text, no frame,
square 1:1, high detail
```

**B2. 水墨晕纹理（256×256 无缝）**

```text
Seamless tileable forest canopy texture, top-down, Chinese ink-wash style,
very low contrast mottled foliage clusters, monochrome desaturated grey-green ink wash,
tiny ink dots and leaf clumps, subtle paper grain, even density across the tile,
no border, no text, no shadow, seamless edges, tileable 256x256
```

**负面提示词（Negative prompt，各家工具语法不同，按需粘贴）**

```text
photorealistic, 3D render, photograph, isometric game art, anime, mobile game icon, glossy,
multiple trees in one asset, ground patch, grass base, drop shadow, cast shadow,
text, letters, labels, watermark, border, frame, checkerboard pattern,
harsh saturation, neon colors, blurry, low contrast noise, busy background
```

**生成参数建议**

- SD / SDXL：1024×1024，CFG 6–8，steps 28–35，采样 DPM++ 2M Karras；每个提示词 batch 4，选 1–2 张。
- Midjourney：`--ar 1:1 --style raw --s 50`；**不要**用 `--niji`（动漫味，不合古舆图）。
- **透明底陷阱**：很多模型被要求 "transparent" 时会画一格一格的棋盘格。除非工具原生支持 alpha 导出，否则一律用 **plain white background** 生成，去白由我处理。
- 变体一致性：固定 seed 与风格词，只改种类 / 组合词（单株 / 三株丛 / 针叶 / 混交）。

**产出命名**：A1→`tree_a.png`、A2→`tree_b.png`、A3→`tree_c.png`、A4→`tree_d.png`（用几张放几张）；A5→`forest_fill.png`；A6→`mountain_a.png`（v2）。放 `crates/wv-render/assets/overlays/forest/`。若拿到的是白底图或带阴影图，不必返工——我裁切、去白、去投影、统一尺寸后再入库。

## 7. 测试要求（§7.2 场景对照）

- `place_territory_at`：镜像 faction 的 7 个用例——map 过滤、区间半开（首含终不含）、开放区间至今、孤儿跳过、非 Polygon 跳过、切片引用、多 Place 同时生效。
- YAML round-trip：`Place.feature`（None/Some）+ `place_geometry` 集合 `load(save(w)) == w`。
- `band_for` 阈值边界（含 ±2% 滞回，若采纳）。
- 采样确定性：同 Place.id + 同网格 → 同布局（跨进程稳定，断言哈希）。
- 主题缺规则回退：未知 feature → 地界描边 + 名仍渲染。
- recipe 真值表：新列 `FeatureOverlay` × 七模式全行。

## 8. 待裁清单（不阻塞 v1，进 README §12 或后续 ADR）

| # | 问题 | 备注 |
|---|---|---|
| T1 | 跨图同一实体 | 世界图森林 vs 区域子图森林是否同一 Place（P4 跨图引用） |
| T2 | 地界兼作意图包络 | 声明事实时可选联动生成（隆起高度场），打通「手势即命令」与「地界即事实」两套圈选 |
| T3 | 作者侧意向参数 | Place 要不要带密度等参数 vs ADR-0023「不按元素混搭」（v1 裁定：不带，密度全在主题） |
| T4 | feature 词汇对齐表 | 哪些 u8 编号保留给 feature / 与网格 kind 词汇的共享方式 |
| T5 | 演绎规则 YAML 化 | 随 ADR-0023 M3 / §12-30 |
| T6 | 中带独立形态 | v1 只做两带 + 淡化 |
| T7 | 山脉 v2 | spine 编辑器、带状地界派生（D6/D7 已定形状，按本管线加挂） |

## 9. 原则触及声明（AGENTS §1 评审动作）

- **P3**：新增 `geometry/place/` YAML 目录，每条一文件——符合；不开二进制例外。
- **P4**：地界 owner 只存 Place `Id` 引用；跨图引用问题挂 T1。
- **P5**：覆盖物作为可独立开关的渲染通道（`FeatureOverlay` pass + 图层帧绑定）。
- **P6/P7**：地界 = `GeometrySpan` 区间化追加，不存「当前地界」字段；「某时刻的地界」一律经 `place_territory_at` 求值。
- **P1/P2**：未触及。
