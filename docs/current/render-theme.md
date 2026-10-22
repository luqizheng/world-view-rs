# 渲染架构与主题 · 现行口径

> [!IMPORTANT] 快照 2026-10-21
> 本页是修正链合并后的**最终状态**。权威 source of truth 是仓库 `docs/adr/`；本站是派生投影。
> 本域修正链最密集（色板三次易手），已按最终口径合并。

## 1. 地图浏览体验 ⚠️ 含被修正子句

**现行口径**：大地图默认离线 Google Default Map 式体验——**左键拖动平移**、笔刷**显式开启**、近白底图 + 亚像素边界、道路 casing/fill、**红点白环 POI**、右下角导航控件；**不接在线瓦片**；数据 / 时间求值 / 图层架构不变。

- **⚠️ 被修正**：§2④「底图边界使用**亚像素纹理**渲染」已被 [ADR-0012](/adr/adr-0012-terrain-default-vector-flat-shading) **部分修正**——浏览态地形默认切到**等值线矢量平涂**，亚像素纹理降为可切换模式。
- **其余七项继续有效**：左键平移 / 笔刷显式开启 / 近白陆地色板 / 道路 casing+fill / POI 红点白环 / 右下角控件 / 不接在线瓦片。
- **溯源**：[ADR-0008](/adr/adr-0008-google-default-map-experience)（2026-10-13）
- **关联待决**：§12-27（线宽是否按 LOD 分级）

## 2. 浏览态地形默认底图 ✅

**现行口径**：浏览态地形默认底图 = **等值线矢量平涂**（marching squares → Chaikin → DP → 多边形平涂）。亚像素纹理合成保留为**可切换模式**（plan v0.1 D3 的 C 类），瓦片图集保留为 B 类。

- **溯源**：[ADR-0012](/adr/adr-0012-terrain-default-vector-flat-shading)（2026-10-14）——本文部分修正 ADR-0008 §2④。
- **关联待决**：§12-14(a)(c)（图集素材来源 / 过渡瓦片）· §12-29（地形边界渲染与草图式生成，M0 对账）

## 3. 古舆图主题（默认观感）⚠️ 日间色已被取代

**现行口径**：日间古舆图色板（宣纸底 `#F2EAD8` 一族）为**默认观感**；`MapPalette { Daylight, Dusk }` 与 ViewMode **正交**（暮色可切、`ui-layout.yaml` 持久化）；`ViewMode::JRPG` **移除**；色板载体仍为 Rust 常量（palette.yaml 显式延期）；硬编码色**全部令牌化**；技法层不动。

- **⚠️ 被取代子句**：**日间地形十色 + 日间淡化强度**已被 [ADR-0035](/adr/adr-0035-jrpg-saturated-daylight-palette) 取代；§2② 色板语义被显式取代。**暮色旧纸与其余令牌继续有效**。
- **溯源**：[ADR-0016](/adr/adr-0016-jrpg-antique-map-default-theme)（2026-10-14）→ 承接 [ADR-0035](/adr/adr-0035-jrpg-saturated-daylight-palette) → [ADR-0042](/adr/adr-0042-hand-drawn-nation-style)
- **关联待决**：§12-33（palette.yaml / ThemePack M3 YAML 化）

## 4. 日间地形色板 ✅（三次易手后的最终值）

**现行口径**：日间 `CORE_TERRAIN` 十色为**中高饱和（JRPG A 方案）**——草 `#8CB95F` / 林 `#3F7A46` / 浅水 `#57A0C0` / 深水 `#2C6389` / 沙 `#E8D6A2` / 丘 `#B5A765` / **山 `#9E9070`** / 雪 `#F5F8F9` / 沼 `#6F8564` / 路 `#C8A272`；日间淡化保留 **`JRPG_SOFTEN_KEEP` = 0.85**；`SNOW_HIGHLIGHT` = `#FCFEFF`。暮色旧纸色板 / Google 色板 / 其余令牌**全部不动**。

- **⚠️ 修正注记**：`core.mountain` 的最终值 **`#9E9070`**——ADR-0035 原定 `#8A7A66`，被 [ADR-0042](/adr/adr-0042-hand-drawn-nation-style) 修正（手绘参考图对比后）。
- **溯源**：[ADR-0016](/adr/adr-0016-jrpg-antique-map-default-theme) → [ADR-0035](/adr/adr-0035-jrpg-saturated-daylight-palette)（取代十色与淡化强度）→ [ADR-0042](/adr/adr-0042-hand-drawn-nation-style)（修正 mountain 色值）
- **关联待决**：暮色是否跟进 JRPG 色系（ADR-0035 §待裁）

## 5. 手绘 Nation 表现层 ✅

**现行口径**：生成端——山脊高斯 22→10 格 + 边缘噪声化、雪线 2250→2750 / 山地 1150→1500、气候纬度边界噪声化、深海五档渐变至 -3200。渲染端——陆格 **±4.5% 确定性颗粒**（全路径同源）、山形图标加背光面、**Nation 步距 96→72 / 符号 1.6×**、波浪提亮、纸纹叠加（程序化噪声）。

- **追记三（civ-atlas 方法论落地）**：`TreatmentFlags.watercolor_bleed` 水彩晕开（5×5 盒式模糊纯函数）；`gen_demo` 删 MOUNTAINS/SNOWFIELD 分带（D13 的 `rock_break`/`ridge_break` **一并撤销**）；装饰层高度优先出山形（≥1500 山 / ≥550 丘）+ **脊点吸附**（步距块内最高格）。
- **溯源**：[ADR-0042](/adr/adr-0042-hand-drawn-nation-style)（2026-10-20）——修正 ADR-0035 色值、调整 ADR-0036 步距
- **注**：色板仍守 `ART_STYLE_SPEC`（只改实现方式，不换颜色体系）

## 6. 地图渲染独立 crate（`wv-render`）✅

**现行口径**：render 模块独立为**平行 leaf crate** `wv-render`；`wv-edit` / `wv-render` **互不依赖**（双子 crate，由 `wv-app` 装配）；应用状态访问一律走 `wv_render::RenderInputs` trait；theme 配色由 wv-render 重导出。**禁止** wv-render 反向依赖 wv-edit / wv-app；**禁止**直接构造 `EditCommand`（走 intent dispatcher）。

- **溯源**：[ADR-0020](/adr/adr-0020-wv-render-crate)（2026-10-16）

## 7. 界面控件独立 crate（`wv-ui`）✅

**现行口径**：新建平行叶子 crate `wv-ui`（egui 控件 + 界面令牌 + `UiShell` + `tool_icons` 资产）；依赖白名单 `bevy` + `bevy_egui` + `egui_extras` **均非 optional**——标准 `cargo test --workspace` 从此天然编译控件层；域件（`float_window` / `terrain_grid`）留 wv-app，旧路径经 shim 零改动。

- **溯源**：[ADR-0026](/adr/adr-0026-wv-ui-ui-widget-crate)（2026-10-17）
- **关联待决**：§12-39 第一组（panel 层何时脱离 `app` feature 门）

## 8. 主题层独立 crate（`wv-theme`）✅

**现行口径**：平行叶子 crate `wv-theme`（依赖仅 `bevy` 非 optional + `wv-terrain`）：持色板 / 装饰旗标 / 覆盖物数据契约 / `ThemePack` 契约 / 注册表 / topographic 调色；`wv-render` 侧只留**重导出**（既有路径零改动）+ 渲染运行时（字体装配、树冠采样、分带）；`wv-render → wv-theme` 为 **optional**（随 `app` feature）。外部主题 crate（如 `wv-theme-chinese-ink`）**只依赖 `wv-theme`**——「**加新风格 = 加新 crate、零改渲染层源码**」成立。

- **溯源**：[ADR-0043](/adr/adr-0043-theme-crate-extraction)（2026-10-20，`ADR-0043-theme-crate-extraction.md`）
- **方法论先例**：[ADR-0020](/adr/adr-0020-wv-render-crate) / [ADR-0026](/adr/adr-0026-wv-ui-ui-widget-crate)

## 9. 表达配方（RenderRecipe）⚠️ 含被修订子句

**现行口径**：`RenderRecipe` Resource（**20 个 PassId**）+ `MODE_TABLE`（模式 × pass 真值表）+ PreUpdate **单点求值** + LayerFrame 叠加；「该不该画」的 mode 散点（buildings / faction / atmosphere / isoline）迁移收口。其余 ViewMode match 属**选色逻辑**，归 ThemePack 方向。

- **⚠️ 被修订**：**D2「装饰族独立轴」** 被 [ADR-0023](/adr/adr-0023-theme-pack-injection-contract) D4 修订——`TreatmentFlags` **归入 ThemePack**（HandDrawn / EMPTY 两极 const，9 旗标已接线），不执行「独立轴」。
- **溯源**：[ADR-0021](/adr/adr-0021-render-recipe-and-treatment)（2026-10-17）→ 承接 [ADR-0023](/adr/adr-0023-theme-pack-injection-contract) D4

## 10. 主题包（ThemePack）注入契约 ✅

**现行口径**（ADR-0023 五项决定）：
- **D1** `ThemePack` 聚合 `PaletteColors` + `TreatmentFlags` + `Option<TileAtlasSpec>`，挂为 `ActiveThemePack` Resource
- **D2** 开放注册用 **cargo 多 crate + 静态切片**（`&'static ThemePack`）+ `ThemePackRegistry` trait，**不用** `Box<dyn>` 动态分发
- **D3** 持久化键沿用 `ui-layout.yaml::map_theme`（零迁移），语义升格为 ThemePack id；未知 id V1 兜底 + 状态栏告警；**运行时跟软件 / 持久化跟工程 / 不按元素混搭**
- **D4** 重新定位 ADR-0021 D2 的 Treatment **归入** ThemePack（而非独立轴）
- **D5** Plugin 拆分 `MapRenderPlugin` / `EditorOverlayPlugin` / `MapShellPlugin`

- **溯源**：[ADR-0023](/adr/adr-0023-theme-pack-injection-contract)（2026-10-16）→ M2 落地 [ADR-0049](/adr/adr-0049-theme-pack-switching)
- **M3（YAML 化）仍推迟**：见 §12-33

## 11. 图层帧（LayerFrame）✅

**现行口径**：`LayerFrame`（`render/layer_frame.rs`）是图层声明的**唯一求值点**：kind → 渲染组映射；**可见 = YAML 声明 ∧ 会话开关**；opacity 落材质 alpha；**z 语义 v1 = 槽位偏移**（每格 0.05，clamp ±4）；标注跟随要素 kind。Border/Custom 暂无消费者、EventPoint 的 z 在 Gizmos 路径不生效（待裁）。

- **溯源**：[ADR-0013](/adr/adr-0013-layer-frame-binding)（2026-10-14）

## 12. 主题包切换链路 ✅

**现行口径**：接通 `ViewState.theme_pack_id` 权威字段——渲染层从「15+ 处一律走 `builtin::for_palette`」改为消费 `ActiveThemePack`；`MapPalette` 仅保留为**兼容 fallback**。端到端落实「加新风格 = 加新 crate、零改渲染层源码」（`wv-theme-chinese-ink` 的 `SeaRecipe` / `DecorRecipe` 从此可用）。

- **溯源**：[ADR-0049](/adr/adr-0049-theme-pack-switching)（2026-10-21）——ADR-0023 M2 的落地实现
- **配套**：[ADR-0048](/adr/adr-0048-ocean-shore-presentation)（`SeaRecipe` / `DecorRecipe` 配方）