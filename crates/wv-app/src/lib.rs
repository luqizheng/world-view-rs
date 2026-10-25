//! # wv-app —— World View 应用层
//!
//! Bevy 渲染与 UI。**默认不编译**，需 `--features app`。
//!
//! 职责边界（`AGENTS.md` §3 数据与渲染分离）：
//! - 本层**不持有**权威世界数据。世界数据是 `wv-core` 的类型，
//!   本层读取它、把投影结果写入 Bevy 资源。
//! - 本层**不直接改**世界。所有修改经 `wv-edit` 的命令。
//! - 本层**不直接读盘**。加载经 `wv-io`。
//! - 本层**不做时间比较**。任何「某时刻的状态」经 `wv_core::time::TimelineQuery::at`。
//!
//! ## 模块导航表（按职责域分组；想找什么先翻这里）
//!
//! 顶层（横切：装配 / 工作区状态 / 跨面板服务 / CLI）：
//!
//! | 模块 | 一句话职责 |
//! |---|---|
//! | [`app_bootstrap`] | `run(AppConfig)` 入口；装 Bevy + bevy_egui 系统。 |
//! | [`state`] | 编辑器工作区所需的全部 Bevy Resource。 |
//! | [`ui_shell`] | 装配 + **唯一**命令分发器 + 启动期工程接管。 |
//! | [`ui_command_palette`] | 命令注册表 + 模糊搜索；零隐藏功能（D5）。 |
//! | [`ui_layout`] | 面板布局持久化（`ui-layout.yaml`，§10 Q5）。 |
//! | [`ui_time`] | 时间显示统一入口（`format_date`，§5.3）。 |
//! | [`overture_cli`] | `wv overture ...` 子命令（ADR-0007）。 |
//!
//! [`render`] —— 地图渲染与视图：
//!
//! | 模块 | 一句话职责 |
//! |---|---|
//! | [`render::camera`] | 缩放 / 平移 + 笔刷拾取 / 笔触收集。 |
//! | [`render::zoom_level`] | 分级缩放区间与 `clamp_to_level`。 |
//! | [`render::map_frame`] | 地图外框 + 1/4 幅面参考网格。 |
//! | [`render::world_projection`] | 时间投影 + 轨迹逐段生长 + 笔刷/网格高亮。 |
//! | [`render::terrain_view`] | 地形底图（色块 / 真实地形） + LOD tile atlas。 |
//! | [`render::terrain_chunks`] | 地形分块 / mip / 视口拣选 / 脏矩形驱动。 |
//! | [`render::vector_layers`] | Google 式道路 casing / fill + POI 红点白环。 |
//! | [`render::streets`] | 街道矢量：从 `World.streets` 取数投到 `vector_layers`。 |
//! | [`render::faction_overlays`] | 势力多边形叠加层（替代 `shade_faded`）。 |
//! | [`render::brush_cursor`] | 编辑光标（屏幕恒定粗细）。 |
//! | [`render::drill`] | 地图三级钻取的导航层（标记布局 / 命中 / 过渡）。 |
//!
//! [`edit`] —— UI 动作 → `EditCommand` 链路：
//!
//! | 模块 | 一句话职责 |
//! |---|---|
//! | [`edit::editing`] | 笔刷 → `EditCommand` → `wv-edit::History` 的桥。 |
//! | [`edit::faction_panel_actions`] | 势力面板 → 提交 `EditCommand` 的动作层。 |
//! | [`edit::validation_actions`] | 校验面板 → 跳转定位的动作层。 |
//! | [`edit::diagnostics_bridge`] | 内存 `World` 的派生 `Diagnostics`（V1 守约）。 |
//!
//! [`ui`] —— 面板/组件库（顶栏 / 状态栏 / 菜单 / 面板 / 共享 widget）。
//!
//! [`theme`] —— 外观令牌与字体：
//!
//! | 模块 | 一句话职责 |
//! |---|---|
//! | [`theme::colors`] | 28 色板令牌（基底）；权威来自 `ART_STYLE_SPEC_v0.1` §1.4。 |
//! | [`theme::colors_google`] | Google Default Map 色板（地图默认视图）。 |
//! | [`theme::palette`] | 地图主题分发：日间古舆图（默认）/ 暮色旧纸（ADR-0016）。 |
//! | [`theme::ui_tokens`] | 界面外壳色令牌（权威来自 `UI_DESIGN_SPEC` §2.2）。 |
//! | [`theme::ui_theme`] | 把 `ui_tokens` 投影成 egui `Visuals`。 |
//! | [`theme::fonts`] | 打包字体装配（Inter 拉丁 + Noto Sans SC 中文，ADR-0019）。 |
//!
//! [`i18n`] —— UI 文案 zh/en 翻译（ADR-0018）；`CurrentLocale` Resource
//! 与 `t(locale, key)` 查表入口；偏好随工程落 `ui-layout.yaml#language`。
//!
//! 应用层持有的一次性配置。
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    /// 启动后要加载的工程目录。
    ///
    /// `None`（默认）= **空编辑器**启动——不主动加载任何地图（用户裁定），
    /// 用户随后可经「顶栏工程菜单 → 打开工程…」（Ctrl+O）或 CLI `-m <目录>` 显式加载。
    /// `Some(dir)` = 启动后立即加载该工程：加载期间窗口内显示进度遮罩，
    /// 失败原因在窗口内展示（不依赖控制台）。
    pub startup_project: Option<String>,
    /// MCP server 配置（ADR-0014 v0.2 增量）。仅在 `--features http` 时被
    /// `app_bootstrap` 接线到 `HttpServerPlugin`；`--features app` 但不带 http
    /// 时**不**启 server（无 server 进程就是 v0.1 默认行为）。
    ///
    /// 默认值见 [`HttpServerConfig::default`]：`127.0.0.1:8732`。
    /// `Default::default()` 走字段自身的 `Default`（`HttpServerConfig::default()`），
    /// 与手写 `impl Default` 等价——`#[cfg]` 跨字段类型对 derive 透明。
    #[cfg(feature = "http")]
    pub http_server_config: crate::http_integration::HttpServerConfig,
}

#[cfg(feature = "app")]
pub mod app_bootstrap;
#[cfg(feature = "app")]
pub mod app_screen;
#[cfg(feature = "app")]
pub mod debug_probe;
#[cfg(feature = "app")]
pub mod edit;
#[cfg(feature = "app")]
pub mod i18n;
#[cfg(feature = "app")]
pub mod overture_cli;
#[cfg(feature = "app")]
pub mod project_loader;
#[cfg(feature = "app")]
pub mod render_bridge;
#[cfg(feature = "app")]
pub mod state;
#[cfg(feature = "app")]
pub mod theme;
#[cfg(feature = "app")]
pub mod ui;
#[cfg(feature = "app")]
pub mod ui_command_palette;
#[cfg(feature = "app")]
pub mod ui_gallery;
#[cfg(feature = "app")]
pub mod ui_layout;
#[cfg(feature = "app")]
pub mod ui_shell;
#[cfg(feature = "app")]
pub mod ui_time;

// LLM 可控编辑（ADR-0016）。仅 `--features http` 时编译；与 `app` 同源
// （HTTP handler 持 GUI 主帧的 `WorldProjection` + `History` 句柄）。
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "http")]
pub mod http_integration;
