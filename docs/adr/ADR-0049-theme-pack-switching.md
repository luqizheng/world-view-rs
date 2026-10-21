# ADR-0049: 主题包切换链路（ADR-0023 M2 落地）

- 状态：已接受（Accepted，2026-10-21，用户裁定「go」——接通主题切换链路）
- 关联：ADR-0023（ThemePack 契约 / M2 切换消费点）、ADR-0043（主题层抽 crate）、
  ADR-0048（`SeaRecipe` / `DecorRecipe` 配方）、ADR-0016（`MapPalette` 配色轴）、
  README §12-48

## 处境

ADR-0023 D1 预留了 `ViewState.theme_pack_id`，并写明「M2 把权威迁到本字段、保留
`map_palette` 作为兼容 fallback」。但 M1 阶段它**零消费点**：

- 渲染层 15+ 处一律走 `builtin::for_palette(view.map_palette)`——`MapPalette`
  只有 Daylight / Dusk 两值，**表达不了外部主题包**；
- `ActiveThemePack` Resource 被 `init_resource` 了，但**没有任何渲染系统读它**
  （`feature_overlays.rs` 的注释原话：「该资源目前没有同步系统，会永远停在
  Daylight」）；
- 后果是 ADR-0043 立下的「**加新风格 = 加新 crate、零改渲染层源码**」在端到端
  意义上并不成立：`wv-theme-chinese-ink` 写好了 `SeaRecipe`（无浅滩带、淡墨浪花）
  与 `DecorRecipe`（更疏更沉、步距 40/64/96px），却**永远用不到**。

## 决策

**D1 · 权威从 `map_palette` 迁到 `theme_pack_id`**

`ViewState.theme_pack_id: Option<String>` 成为主题选择的唯一权威；`map_palette`
降级为「**内置包的快捷选择器**」（顶栏日夜按钮仍走它，语义不变）。

**D2 · 解析结果挂在 `ViewState` 上，不新增 Resource 参数**

`ViewState.active_theme: &'static ThemePack` 承载解析结果。

**为什么不用 `ActiveThemePack` Resource**：Bevy 的 system 参数上限是 16，
`wv-app` 的重载 system 已经贴着上限（`poll_open_project_dialog` 原本**正好 16 个**
参数，`apply_pending_commands` 原本 19 个、靠**元组折叠**才压下来）。再加一个
Resource 参数就会超限，而 `ViewState` 是所有消费点**本来就持有**的载体——挂它
零参数成本。（元组折叠是本仓库既有的解法，见 `apply_pending_commands` 的注释。）

**D3 · 同步系统**：`wv_render::theme::switch::sync_active_theme_pack` 挂在
`PreUpdate` **最前**（渲染消费点在 `Update`，PreUpdate 一定先于 Update；唯一同在
PreUpdate 的消费者 `poll_open_project_dialog` 由 app 侧显式 `.after()` 保证）。
**只在 id 变化时写**——`ResMut` 写入会标记 changed，每帧写会让下游「缓存键比对」
误判主题变了而白白重算。

**D4 · 解析优先级**：`theme_pack_id` 命中注册表 → 用它；否则按 `MapPalette::from_key`
兼容回退（存量 YAML 存的是 `daylight` / `dusk`，与内置包 id `antique_default` 不同名）；
否则落回 `map_palette` 对应的内置包。**未知 id 不报错**（V1 纪律）。

**D5 · 消费点全量改读 `view.active_theme`**

wv-render 侧 13 个文件（`cells` / `terrain_decor` / `feature_overlays` /
`atmosphere` / `isoline_overlay` / `region_overlay` / `buildings` / `drill` /
`streets` / `map_frame` / `grid_lines` / `world_projection` / `faction_overlays`）
与 wv-app 侧 4 处，全部改读当前包。`map_palette` 由此**只**剩「内置包快捷选择」
与持久化兼容两个用途。

**D6 · 合成指纹从 `MapPalette` 改 `theme_id`**

`SynthCtx` / `TerrainTexture` 的「当前纹理用的什么主题」指纹改记**主题包 id**
（`&'static str`）——`MapPalette` 表达不了外部包。`SynthCtx` 手写 `PartialEq`
（`ThemePack` 含 f32 字段不能 derive；`pack` 字段由 `theme_id` 唯一决定，不参与比较）。

**D7 · 切换入口：命令面板「切换地图主题」循环注册表所有包**

不新增 UI——现有命令（`CommandId::ToggleMapPalette`）的语义从「日夜两档轮换」扩成
「遍历注册表里的全部包」。文案同步改为「日间古舆图 / 暮色旧纸 / 水墨…」。

**D8 · `wv-theme-chinese-ink` 从 dev-dep 提升为 wv-app 运行时依赖**

否则编辑器选不到它。**依赖方向仍合规**：它只依赖 `wv-theme`，不依赖 `wv-render`
（ADR-0043 的注入契约）。

## 理由

- **为什么不在渲染层做 `if theme == ink` 分支**：那是 ADR-0043 明确禁止的形态——
  换风格要改渲染源码。配方化（ADR-0048）+ 权威接通后，水墨包的差异**全部**由
  `wv-theme-chinese-ink` 自己声明，渲染层零改动。
- **为什么保留 `map_palette`**：顶栏日夜按钮、`ui-layout.yaml` 的存量键都依赖它；
  降级为「内置包快捷选择器」是零成本的，且不破坏任何现有交互。
- **为什么主题包 id 存进 `ui-layout.yaml::map_theme`**：D3 早已规定该字段存主题包
  id，存量工程存的是 `MapPalette::key()`——由 D4 的兼容回退解析，**零迁移**。

## 代价

- `ViewState` 多了一个 `&'static ThemePack` 字段（引用，Copy，无额外内存开销）。
- `ThemePack` 需要 `Copy`（已是）且被 `ViewState` 持有 → **主题包必须是 `static`**
  （与 ADR-0023 D2 的 `&'static` 切片契约一致，外部包天然满足）。
- 主题切换后**底图需要重烘焙**（`last_theme_id` 变了 → 走全图重建，1024² 图约 2s），
  与既有的「切调色板」路径同口径，不是新增开销。
- `wv-app` 多一个运行时依赖（`wv-theme-chinese-ink`）。

## 被否决的备选

- **给每个渲染 system 加 `Res<ActiveThemePack>` 参数**：Bevy 16 参数上限直接撞墙
  （`poll_open_project_dialog` 已经正好 16 个）。元组折叠能救 `apply_pending_commands`，
  但救不了所有 system——挂 `ViewState` 是一劳永逸的解法。
- **在渲染层按 theme id 分支**：违反 ADR-0043。
- **把 `ThemePack` 整个搬进 `ViewState`**（而非只放引用）：`ViewState` 会被序列化
  （持久化路径），`ThemePack` 不该进工程 YAML。存 `&'static` 引用则两者皆不误。
- **运行时加载主题包（YAML / 文件路径）**：ADR-0023 M3 挂账（README §12-30），不在本轮。

## 验证

- 单元测试 9 条（`wv_render::theme::switch`）：无 id 回退、存量键兼容回退、未知 id
  不报错、注册表命中优先、合并拒绝重复 id、外部包按 id 解析到它自己。
- 全门禁：fmt / workspace test / 双 clippy（含 wv-render app feature）/ app-test 227 /
  wv-render 419 / smoke-gui。
- **实机端到端**：把 `examples/work-view-test/ui-layout.yaml` 的 `map_theme` 改成
  `chinese_ink` → 启动 → 海的三圈墨线波纹与白浪花消失、浅滩带关闭、山三角变疏变淡
  （`wv-theme-chinese-ink` 的 `SeaRecipe` / `DecorRecipe` 逐项生效）——证明
  「持久化 → `theme_pack_id` → 同步 → 渲染消费」整条链路通。
