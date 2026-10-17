# ADR-0026：界面控件层独立成 crate（`wv-ui`）—— egui 控件 + 界面令牌的编译级隔离

**状态**：已接受（Accepted，2026-10-17，用户对话拍板）
**日期**：2026-10-17
**决策者**：用户（依赖实况核查 + 方案评审后拍板「开 worktree 执行」）
**关联**：AGENTS §2（分层与依赖方向）/ §2.2（树断言）/ §7.1（双闸门）/ §8（图标落盘路径）、ADR-0019（字体）/ 0020（wv-render 抽取先例，本 ADR 的同款方法论）/ 0023（复用动机先例）
**约束**：本 ADR 一旦「已接受」，正文不再修改；若被推翻，新开一份并在索引标注。

---

## 1. 处境（Context）

ADR-0020 把地图渲染（`render/` + `theme/{palette,colors*,fonts}`）抽成了 `wv-render`，
但 **egui 控件层与界面令牌**仍留在 `wv-app`（`ui/widgets/` + `theme/{ui_tokens,ui_theme}` +
`ui/shell.rs`，约 4900 行）。这留下两个结构性问题：

1. **标准门禁看不见控件层**。wv-app 的全部 UI 模块挂在 `#[cfg(feature = "app")]`
   之后，而该 feature 不在默认 feature 里——`cargo test --workspace` **不编译任何
   UI 代码**（§7.1 实测 `cargo test -p wv-app` 0 个测试目标）。为此被迫发明了
   `app-test` / `app-clippy` 两条补充闸门，且仍有「改坏控件一路绿灯」的残留风险：
   闸门是**纪律**，不是**编译器保证**。
2. **控件层与业务层同 crate，隔离靠自觉**。控件能否读到 `World` / `EditHistory` /
   `WorldProjection`？没有客观兜底——与 ADR-0020 抽取前「render 反向依赖全凭
   review 者记忆」是同一个病。

2026-10-17 按 Figma `42:2` 完成 `FloatWindow` / 检视器组件族提取（commit `3d64a1a`）
后核查依赖实况：`ui/widgets/` 对 wv-app 内部的依赖仅 6 个入口，其中 3 个是纯令牌层
（`ui_tokens` / `ui_theme` / `UiShell`），业务耦合只剩 `float_window`（Panel 枚举
包装）、`terrain_grid` + 地形两 helper（域色彩与 i18n）三处——**抽取的耦合面已经
被此前的组件化工作清理干净**。

## 2. 决策（Decision）

1. **新建叶子 crate `wv-ui`**，与 `wv-terrain` / `wv-overture` / `wv-render` 平行。
   迁入：`theme/ui_tokens.rs`、`theme/ui_theme.rs`、`ui/shell.rs` 的 `UiShell`、
   `ui/widgets/` 全部控件（button / controls / floater / primitives /
   segmented_control / svg_loader）与 `widgets.rs` 根，以及 `assets/ui/tool_icons/`
   （`include_image!` 相对路径深度恰好不变：两侧都是 `src/widgets/` → crate 根 3 层）。
2. **依赖白名单**：`wv-ui` 只依赖 `bevy + bevy_egui + egui_extras`（均**非 optional**
   ——控件层的本体就是 egui，不再设 feature 门；标准 `cargo test --workspace`
   从此天然全量编译控件层）。**禁止**依赖 `wv-core` / `wv-terrain` / `wv-io` /
   `wv-edit` / `wv-render` / `wv-app`，禁止文件系统与网络（§2.1 同款红线，
   §2.2 增列 `cargo tree -p wv-ui` 断言）。
3. **域件留在 wv-app**（它们依赖 i18n / `Panel` / 世界色板，不属公共控件）：
   - `float_window`（`Panel` 特化包装，委托 `wv_ui::FloatWindow`）；
   - `terrain_grid` + `terrain_name_at` + `terrain_swatch`（依赖 `crate::i18n` 与
     `crate::theme::colors`——后者是 `wv_render` 的 re-export）。
   二者落在 app 侧 shim 模块（`ui/widgets/local.rs` / `ui/shell.rs`），**旧路径
   全部零改动**（`crate::ui::widgets::X` / `crate::ui::shell::UiShell` /
   `crate::theme::ui_tokens::X` 经 re-export 继续可达，ADR-0020 §2.6 同款手法）。
4. **世界色一致性测试随域走**：`ui_tokens` 测试中唯一引用世界色板的
   `badge_fills_mirror_the_world_link_palette`（§4.7「同色相」守卫）移到
   wv-app 的 `theme/mod.rs`，改为断言 `wv_ui::ui_tokens` × `crate::theme::colors`
   （wv_render re-export）——`wv-ui` 保持零世界色板依赖，守卫不丢。
5. **wv-render 与 wv-ui 的边界**：**egui 控件与界面令牌 = wv-ui；地图渲染、GPU
   主题、打包字体（`LoadedFonts` 是 Text2d 资源）= wv-render**。二者互不依赖，
   由 wv-app 同时装配。
6. **闸门口径过渡**：`app-test` / `app-clippy` **保留**——wv-app 内的 panel /
   dialog 层（读 `WorldProjection` 等业务态）仍在 `app` feature 门后，双闸门
   继续覆盖它们；但控件层（约 4600 行、约 50 条单测）自此由标准三闸门覆盖，
   §7.1 的盲区收窄到 panel 层。

## 3. 理由（Rationale）

- **为什么是 crate 而不是模块纪律**：只有 crate 边界能给出编译级保证——
  `wv-ui` 想引用 `wv-app::state` 在依赖图上就不成立。这是 ADR-0020 §2.2
  「客观兜底，不必依赖 review 者的记忆」的直接推广。
- **为什么现在抽**：`FloatWindow` 去 Panel 化（`3d64a1a`）让业务耦合从
  「控件层遍布 Panel 类型」降到「一个 20 行包装函数」，抽取成本到达历史最低点；
  再晚做，Figma 42-2 新面板接线会继续往控件里塞业务态，耦合重新长大。
- **为什么 bevy 非 optional**：与 wv-render 相反——wv-render 的数据路径
  （AtlasConfig 等）要服务不装 Bevy 的消费方；wv-ui 没有这种消费方，
  它存在的意义就是 egui。设 optional 只会把「标准门禁不编译 UI」的病带进新 crate。
- **复用方向**：与 ADR-0023 同构——未来独立工具（wv-render-cli 的调试面板、
  组件画廊、游戏内 egui UI）直接以 `wv-ui` 为依赖，不必拖上整个编辑器。

## 4. 代价（Costs）+ 被否决的备选

**代价**：
- 约 4600 行搬家 + 三处 shim（机械活，一次完成；ADR-0020 全套先例）；
- 新增一个成员 crate 的维护面（Cargo.toml / 树断言 / 文档同步）；
- wv-render 与 wv-ui 的「口头边界」（本 ADR §2.5）需要评审时人肉守——
  无法用 cargo tree 表达（两个都是叶子，互相不依赖即是合规）；
- 图标资产迁到 `crates/wv-ui/assets/ui/tool_icons/`，AGENTS §8 的落盘路径
  随之更新（外部贡献者的肌肉记忆成本）。

**被否决的备选**：
- **并入 wv-render**：模糊「地图渲染」与「egui 控件」两个复用方向；wv-render
  的 headless 目标（未来 CLI）会被 egui 初始化拖累；否。
- **不抽 crate、只加 clippy lint / 模块约定**：给不了编译级隔离，§7.1 盲区
  原样保留；否。
- **连同 panel 层一起抽**：panel 读 `WorldProjection` / `PanelState` / 派发
  EditCommand，是业务层本体，抽走只会造出 `RenderInputs` 式的 trait 打洞
  （ADR-0020 的教训：能不 trait 化就不 trait 化）；否。

## 5. 落地注记（执行期追记区，只增不改）

- M1（2026-10-17，本 ADR 同 commit）：crate 建立 + 搬家 + shim + 树断言 +
  AGENTS §2/§2.2/§7.1/§8 修订；`cargo test --workspace` 首次纳入控件层全部单测。
- 遗留：panel 层何时脱离 `app` feature 门（依赖 app 状态，需先解耦状态注入）
  ——挂 README §12 待裁（编号以合入时为准，本分支登记为 §12-39）。
