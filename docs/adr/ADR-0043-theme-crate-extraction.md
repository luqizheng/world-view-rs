# ADR-0043: 主题层抽为独立 crate（wv-theme）——默认主题与 wv-render 彻底分离

- **状态**：已接受（Accepted，2026-10-20，用户裁定「把 theme 抽取来成为一个 crate，把默认的 theme 彻底与 wv_render 分离」）
- **关联**：ADR-0020（wv-render 抽 crate）、ADR-0023（ThemePack 契约与注入）、ADR-0026（wv-ui 抽 crate 的方法论先例）、ADR-0016（地图主题色板）、ADR-0035（JRPG 色板）、ADR-0042（手绘 Nation 表现层）、`civ-atlas` 参考实现（`.ref/civ-atlas`）

## 处境

**用户诉求**：做水彩风格时发现「主题」这一概念被焊在渲染层里——想加/换一套风格就得改 `wv-render` 源码。明确要求：主题抽为一个 crate，默认主题与 `wv-render` 彻底分离。

**现状盘点**（`crates/wv-render/src/theme/`，九件套）：

| 模块 | 性质 | 该去哪 |
|---|---|---|
| `palette.rs` | **数据**（`MapPalette` + 36 槽 `PaletteColors`） | wv-theme |
| `colors.rs` / `colors_dusk.rs` / `colors_google.rs` | **数据**（三套内置色板） | wv-theme |
| `overlays.rs` | **混合**：契约（`FeatureRule` / `FeatureOverlaySet` / `BiomeFamily`）+ 运行时（`overlay_far_blend` / `forest_sites` 采样） | 契约 → wv-theme；运行时留 wv-render |
| `theme_pack.rs` | **契约**（`ThemePack` / `TreatmentFlags` / `TileAtlasSpec` / builtin 包） | wv-theme |
| `registry.rs` | **契约**（`ThemePackRegistry` / `MergedThemeRegistry` / `BuiltinRegistry`） | wv-theme |
| `topographic.rs` | **数据**（Topographic 视图调色） | wv-theme |
| `fonts.rs` | **运行时**（字体装配，消费方是 wv-render 的 `Text2d` 标签 ×7） | 留 wv-render |

**已有基础**：ADR-0023 已定 `ThemePack` 契约，`wv-theme-chinese-ink` 示例 crate 已演示「加新风格 = 加新 crate、零改 wv-render 源码」的注入通路。**但示例 crate 反过来依赖 `wv-render`**（`use wv_render::theme::overlays::…`）——契约在渲染层里，主题 crate 反而离不开渲染层，「插件」是假的。

## 决策

**新建平行叶子 crate `wv-theme`**，持主题的**契约与数据**；`wv-render` 只消费，不持有。

- **D1 · crate 定位**：`wv-theme` 与 `wv-core` / `wv-terrain` / `wv-ui` 平行，是**叶子 crate**。依赖：仅 `bevy`（非 optional，理由同 ADR-0026 D2——`PaletteColors` 的 36 槽是 `bevy::color::Color`；本 crate 的本体就是 Bevy 类型，设门只会把「标准门禁不编译」的旧病带进来）。**不**依赖 wv-core / wv-terrain / wv-render / wv-app。
- **D2 · 依赖方向**：`wv-render → wv-theme`（**optional**，随 `app` feature 一起打开）——wv-render 不带 feature 时仍不拉 Bevy 树（ADR-0020 的红线不动）。`wv-app → wv-theme`（直接依赖，UI 侧重导出）。`wv-theme-chinese-ink → wv-theme`（**不再依赖 wv-render**——这是「彻底分离」的落点）。
- **D3 · 零路径变更**：`wv-render::theme::{colors, colors_dusk, colors_google, palette, overlays, theme_pack, registry, topographic}` 全部改为 `pub use wv_theme::…` **重导出**，既有调用点（`crate::theme::palette::colors(…)` 等 30+ 处）**一行不改**。`wv-app::theme` 的 `pub use wv_render::theme::{…}` 同步改为重导出 `wv_theme`，对外路径不变。
- **D4 · 契约 / 运行时切分**：`overlays.rs` 按性质切两半——`FeatureRule` / `FeatureOverlaySet` / `BiomeFamily`（**数据契约**，外部主题 crate 要构造它们）进 wv-theme；`overlay_far_blend` / `forest_sites`（**渲染采样**，需要高度场与视口）留 wv-render，模块路径 `crate::theme::overlays::{forest_sites, overlay_far_blend}` 不变。
- **D5 · 字体留渲染侧**：`fonts.rs` 不搬——`LoadedFonts` 的消费方是 wv-render 的 `Text2d` 标签系统（ADR-0019 D2 落地注记），字体装配属渲染运行时而非主题数据。
- **D6 · 默认主题 = 水彩**：本 ADR 只定**结构**。默认主题的**内容**（水彩色板取代 JRPG 饱和色板）随 Part B 落地，作为新的 ADR 追记或独立 ADR——它会修正 ADR-0035 的定稿色值，须走 §6.4 流程。
- **D7 · 编号占用登记**：`ADR-0042` 已被本 session 占用（手绘 Nation 表现层）。本 ADR 取 **0043**。

## 理由

- **主题是「内容」不是「机制」**：色板、装饰旗标、覆盖物规则是作者与美术要改的东西；把机制（采样、绘制、视口裁剪）留在渲染层、把内容抽出去，边界与 AGENTS §2「数据 / 渲染分离」同构。
- **既有 ADR-0023 已经画好了蓝图**：契约 + 注册表 + 外部 crate 三件套都在，缺的只是「契约自己的家」。补上这一步，`wv-theme-chinese-ink` 才真正从「示例」变成「可用的第三方包」。
- **不推倒 ThemePack 抽象**：M1/M2 已经把 `ThemePack` / `TreatmentFlags` / `ThemePackRegistry` 定型并有测试覆盖；本 ADR 是**搬房子**，不是重设计。

## 代价

- **`cargo test --workspace` 会拉起 Bevy 树**（wv-theme 非 optional 依赖 bevy）——与 wv-ui 已有行为同源（ADR-0026 D2 已接受该代价：数据层跑门禁时一次性编译缓存）。
- **`wv-render` 的 `app` feature 多一个 optional 依赖**——不带 feature 的构建仍零 Bevy，红线保持。
- **`wv-theme-chinese-ink` 的 `app` feature 失效**（它不再依赖 wv-render）——改为无条件编译；其 `#![cfg(feature = "app")]` 门一并撤掉。
- 依赖表（AGENTS §2.1）需增一行 `wv-theme`；§2.2 的 `cargo tree` 断言需增一条（`wv-theme` 不依赖任何兄弟 crate）。

## 被否决的备选

- **B · 把主题留在 wv-render，只加 `ThemePack` 抽象**：ADR-0023 已做，仍不解决「换风格要改渲染层源码」——用户的原话诉求。
- **C · 主题 crate 不依赖 bevy（用 `[f32;3]` 自定义色类型）**：与 Bevy 的 `Color` 互转要写 36 槽 × 全部调用点的适配层，收益仅是「门禁不拉 Bevy」——而 wv-ui 已经拉了，收益重复而成本巨大。
- **D · 主题 crate 依赖 wv-core**：主题数据不承载世界数据（P4），引入即多一条不必要的耦合。

## 验证

- `cargo tree -p wv-theme` 输出应只有 `bevy` 族（无任何兄弟 crate）。
- `cargo tree -p wv-render | grep wv-theme` 在不带 feature 时为空、带 `--features app` 时有值。
- `cargo tree -p wv-theme-chinese-ink | grep wv-render` 应为空（彻底分离的判据）。
- `cargo test --workspace` / `cargo clippy --workspace --all-targets -- -D warnings` / `cargo fmt --all -- --check` 全绿 + `cargo app-test` / `cargo app-clippy` + `bash scripts/smoke-gui.sh`。

## 影响文件

| 文件 | 改动 |
|---|---|
| `crates/wv-theme/**` | **新建**：契约 + 数据 + 内置色板 |
| `crates/wv-render/src/theme/**` | 八个模块改 `pub use wv_theme::…` 重导出；`overlays.rs` 只留运行时；`fonts.rs` 不动 |
| `crates/wv-render/Cargo.toml` | 加 `wv-theme = { workspace = true, optional = true }`；`app` feature 增 `dep:wv-theme` |
| `crates/wv-app/Cargo.toml` | 加 `wv-theme = { workspace = true }` |
| `crates/wv-app/src/theme/mod.rs` | 重导出改指 `wv_theme` |
| `crates/wv-theme-chinese-ink/**` | 依赖改 `wv-theme`；`use` 路径改；撤 `app` feature 门 |
| `Cargo.toml` | workspace members + dependencies 增 `wv-theme` |
| `AGENTS.md` | §2.1 依赖表增行 + §2.2 断言增条 |
