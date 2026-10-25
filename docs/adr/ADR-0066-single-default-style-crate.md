# ADR-0066: 单一默认风格层——`wv-default-theme` 取代多主题机制

- 状态：**已接受**（2026-10-25，随 `refactor/default-theme-crate` 落地）
- 关联：ADR-0016（配色轴 / 技法轴正交）、ADR-0023（主题包机制）、ADR-0043
  （主题层抽 crate）、ADR-0048（数值配方住主题层）、ADR-0061（专业制图范式）、
  ADR-0062 / 0063 / 0064（群系面 / 国境线 / POI 分类）
- 取代：`wv-theme` crate、`wv-theme-chinese-ink` crate、多主题注册表机制

## 1. 处境（Context）

**主理人裁定（2026-10-25 19:48 起一贯）**：「这个软件不需要做 theme 的贴图这个
功能，我自己走向了错误道路。我只需要一个专业地图 UI 就可以。」

裁定之后，仓库里的风格层仍是「多主题」形状，且越滚越大：

| 组成 | 规模 | 问题 |
|---|---|---|
| `wv-theme`（11 个模块） | 3405 行 | 混着「专业制图色板」「风格契约」「艺术主题」三类 |
| `wv-theme-chinese-ink` | ~480 行 | 演示用的外部风格包，只为证明「加新风格 = 加新 crate」 |
| `ThemePackRegistry` / `MergedThemeRegistry` | 228 行 | 多包注册与合并机制——**只有一个包时是纯负担** |
| `colors_dusk`（暮色） | 147 行 | 暮色包从未开放为独立持久化键（ADR-0023 §6.2 M2 未做） |
| `MapPalette`（两值） | — | 日间 / 暮色二选一，暮色档实际不可达 |
| `theme_pack_id` + 顶栏主题控件 + 命令面板条目 + 2 组 i18n | — | 一整套「切风格」交互面，切了也没得切 |

同时存在一个**语义分裂**：专业制图的观感由 `ViewMode::GoogleMap` 这个**模式**
在渲染层分派（`conv_palette_terrain` 换十色、`conv_pack_with` 强制 EMPTY
treatment、`feature_symbol_alpha` 归零），而「风格数据」住在 `wv-theme` 的
主题包里。两处各管一半——换色板要动渲染层，动渲染层要懂主题包。

## 2. 决策（Decision）

**D1 · 新 crate `wv-default-theme`：风格数据的唯一家**

承接 `wv-theme` 的**全部有用内容**，并按新定位重组：

| 模块 | 内容 | 去向 |
|---|---|---|
| `colors_google` | **专业制图色板**（十色地形 / 道路三档 / 12 类 POI / 注记 / 海岸线 / 边界） | 升为主色表，补齐 36 槽所需的中性灰阶 |
| `colors` | 语义色与编辑器色（accent / ui_link / 派系 / 描线三档） | 保留（风格**不变**项） |
| `palette` | `PaletteColors`（36 槽结构）+ `MapPalette` | 收成**单值** |
| `theme_pack` | `ThemePack` / `TreatmentFlags` 契约 | 保留，**只有一个包** |
| `decor` / `sea` | 画法配方（高度阈值 / 气候带 / 海面笔触） | 保留，加**专业制图版**配方 |
| `overlays` | `FeatureRule` / `RegionOverlaySet` 契约 | 保留 |
| `topographic` | Topographic 调色数据 | 保留 |
| ~~`colors_dusk`~~ | 暮色色表 | **删** |
| ~~`registry`~~ | 多包注册表 | **删** |

**D2 · 默认风格 = 专业制图**（不是水彩）

`builtin::DEFAULT` 的四件事同时改：

- `palette` = `PaletteColors::DEFAULT`：风格色取专业制图十色（近白陆地 + 淡水 +
  公园绿 + 橙黄分级道路 + 分类 POI + 中性灰描线）；
- `treatment` = `EMPTY`：海岸晕 / 纸纹 / 水彩渐变 / 晕开 / 颗粒——手绘语汇整体退场；
- `sea` = `PROFESSIONAL`：无波纹 / 碎浪 / 排线 / 纸纹 / 浅滩带 / 积墨，
  **只留水深渐变**（深度是数据不是画法）；
- `decor` = `PROFESSIONAL`：`mountain_style` / `forest_style` 均为 `None`
  （不画山符号、不画林团），但**高度阈值与气候带仍取地形事实**（阈值是地形
  数据，不是画法）；
- `overlays.forest`：**只留地界晕**（中性灰），去掉树图集贴图与符号尺寸。

**D3 · 两份地形色：风格色 `core` + 逐格技术色 `kind_core`**

`PaletteColors` 新增 `kind_core: [Color; 10]`：

- `core` = **风格色**——专业制图把多数陆上类折叠为近白（`shade_google` 的规则），
  只在水 / 森林 / 沼泽三档上真正有区分度；
- `kind_core` = **逐格技术色**——旧的十色高饱和表，专供**色块地形 / 编辑视图**
  的「一格一色」对照。

**为什么必须分开**：色块视图（`ViewMode::Schematic` + CellMap 编辑）的存在意义
就是让作者一眼看出每格是什么地形。若它也用风格色，10 档全变成近白，编辑视图
就废了。而「换风格不该影响色块视图的可读性」正是拆两轴的收益。

**D4 · 多主题机制整族退役**

| 退役物 | 处置 |
|---|---|
| `wv-theme` crate | **删除**（内容全部迁入新 crate） |
| `wv-theme-chinese-ink` crate | **删除** |
| `ThemePackRegistry` / `MergedThemeRegistry` / `BuiltinRegistry` | **删除** |
| `ViewState.theme_pack_id` | **删除**（多包选择才需要它） |
| `CommandId::ToggleMapPalette` + 顶栏地图主题控件 + 2 组 i18n | **删除**（单风格没有可切的） |
| `MapPalette` | **收成单值**（枚举保留：它同时是「配色轴」概念的位置标记 + 持久化键载体） |
| `builtin::DUSK` / `PaletteColors::DUSK` / `WATERCOLOR_DUSK` | **删除** |

**D5 · 保留 `ViewState.active_theme: &'static ThemePack`**

渲染层有 40+ 处读 `view.active_theme.palette` / `.treatment` / `.sea`。拆掉这个
字段会波及整个渲染管线，收益不抵。保留它的代价只是「单值恒等」，
收益是**风格数据仍只有一个家**（渲染层不持有任何风格副本）。

`switch::resolve_pack` 简化为恒返回 `builtin::DEFAULT`；
`sync_active_theme_pack` 保留（它是 `PreUpdate` 最前的风格锚点）。

**D6 · 持久化零迁移**

`ui-layout.yaml` 的 `map_theme` 字段**保留**（不删字段 = 旧文件照读），
但值一律映射回默认档：`MapPalette::from_key` 接受 `"daylight"` **与**
`"dusk"`（暮色已退役但存量工程可能存着），都回落到唯一档；写入时固定写
`"daylight"`。

**D7 · 模式差异只剩「技法」，不再有「风格」**

`ViewMode` 之间的差异全部是**画什么**（哪些 pass 画、群系是否折叠、是否出边界
线、是否出分类 POI），不再有**长什么样**。`MODE_TABLE` 的 GoogleMap 行保留
（它是技法差异：Faction / BiomeFill / 无装饰符号）。

## 3. 理由（Why This）

- **风格与技法彻底分家**：ADR-0016 定的两轴正交，此前只做了一半——风格数据在
  `wv-theme`，但「专业制图长什么样」实际由 `ViewMode::GoogleMap` 在渲染层分派。
  本 ADR 把「长什么样」整体收进风格层，`ViewMode` 只管「画什么」。
- **删机制 > 加机制**：多主题机制（注册表 / 外部包 / id 选择 / 切换 UI）服务的
  场景已被裁定取消。留着它 = 留着一套永远走不到的分支。
- **单风格 = 风格数据的唯一真相源**：ADR-0061 追记一刚修完「两份同名色表」
  的病（`shade_google` 读渲染侧抄录表，主题侧覆盖对像素无效）。单一风格让
  这种「两份」无处藏身。
- **两份地形色是必要的**：风格色服务于「像不像地图」，技术色服务于「看不看得
  懂编辑对象」。合成一份必然废掉其中一个。

## 4. 代价与风险（Cost）

- **风格不可换**：这是裁定的直接后果。若将来要恢复多风格，需要重做注册表机制
  （`registry` 的 228 行已删，但 `switch.rs` 的「解析到 `&'static ThemePack`」
  骨架还在，扩展点是现成的）。
- **顶栏少一个控件**：地图主题切换器删除后，顶栏右区少一项（`Ctrl+M` 失效）。
- **水彩 / 暮色 / 水墨观感无法找回**：代码与资产（树图集等）仍在 git 历史里，
  但**当前树里没有**。这是裁定的意图。
- **DUSK 键仍被接受**：`from_key("dusk")` 回落默认档而非报错——V1 纪律
  （不阻止、不报错），但意味着存量工程「选过暮色」的用户会看到观感变化。

## 5. 被否决的备选（Alternatives）

- **保留 `wv-theme` 名字**：它已经不再是「主题层」而是「默认风格层」，
  `wv-theme` 会让人以为多主题仍在。改名成本 = 一次机械替换。
- **保留 `wv-theme-chinese-ink` 作为「外部风格包」的样板**：它证明的机制
  （注册表注入）已退役，留着就是一个编译不过的孤儿。
- **把 `ViewMode::GoogleMap` 也删掉**（既然单风格）：**否决**。模式轴是
  **技法**轴（哪些图层画 / 群系折不折 / 出不出边界线），与风格轴正交
  （ADR-0016）——专业制图技法仍是主力浏览模式（顶栏 `Map` 段）。
- **`MapPalette` 枚举整个删掉**：它是持久化键 `map_theme` 的载体，也是
  「配色轴」概念的位置标记。删了要动 YAML 契约（破坏零迁移）。
- **把 `active_theme` 拆成散字段**：40+ 处调用点，且风格契约（palette /
  treatment / sea / decor / overlays）天然是一个整体。

## 6. 验收（Acceptance）

1. `wv-default-theme` 存在且**自包含**（依赖仅 `bevy` + `wv-terrain`）；
   `wv-theme` / `wv-theme-chinese-ink` **不存在**；
2. `cargo tree -p wv-default-theme` 不含 `wv-render` / `wv-app` / `wv-core`；
3. `PaletteColors::DEFAULT` 的十槽 = `GOOGLE_CORE_TERRAIN`（单测锚定）；
   `kind_core` 十档互不相同（单测锚定）；
4. `builtin::DEFAULT.treatment == EMPTY`（单测锚定）；
   `PROFESSIONAL` 海配方无笔触但保留深度量程（单测锚定）；
   `PROFESSIONAL` 装饰配方 `mountain_style` / `forest_style` 均为 `None`
   而阈值仍取地形事实（单测锚定）；
5. `MapPalette::from_key("daylight" | "dusk")` 均回默认档；
   `toggled()` 是恒等映射；
6. 四类实体色（place / region / faction / event）互不相同（`hover_tooltip`
   的既有锚点）；
7. 门禁：`cargo test --workspace`（除 `wv-io` 的本机写盘环境约束）、
   三套 clippy `-D warnings`、`cargo fmt --check`、`smoke-gui.sh` 全绿。

## 7. 遗留

- **`wv-world-cell-map` 的固定色镜像**：它按 ADR-0054 D4 禁依赖任何 theme
  crate，自带一份 `CellPalette` 平面镜像。本 ADR 后那份镜像与新 crate 的
  `DEFAULT` 是**两份数据**（各为各的用途：前者是数据视图的固定色，后者是
  地图风格）。两者的漂移风险需在下次动配色时留意。
- **`MapPalette` 的持久化键仍是 `daylight`**：语义上现在是「默认档」而非
  「日间古舆图」。改名会破坏存量 YAML，故保留；文档已注明。
- **风格可换性的回归路径**：若主理人改主意要恢复多风格，落点是重建
  `registry`（`ThemePackRegistry` trait + `MergedThemeRegistry::merge`）
  并把 `builtin::ALL` 从单元素扩回多元素——`switch::resolve_pack` 的签名
  已经为此预留（参数 `_view` 保留）。
