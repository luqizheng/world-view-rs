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
//! ## 模块导航表（按职责分组；想找什么先翻这里）
//!
//! | 模块 | 一句话职责 |
//! |---|---|
//! | [`app_bootstrap`] | `run(AppConfig)` 入口；装 Bevy + bevy_egui 系统。 |
//! | [`state`] | 编辑器工作区所需的全部 Bevy Resource。 |
//! | [`camera`] | 缩放 / 平移 + 笔刷拾取 / 笔触收集。 |
//! | [`zoom_level`] | 分级缩放区间与 `clamp_to_level`。 |
//! | [`map_frame`] | 地图外框 + 1/4 幅面参考网格。 |
//! | [`world_projection`] | 时间投影 + 轨迹逐段生长 + 笔刷/网格高亮。 |
//! | [`terrain_view`] | 地形底图（色块 / 真实地形） + LOD tile atlas。 |
//! | [`terrain_chunks`] | 地形分块 / mip / 视口拣选 / 脏矩形驱动。 |
//! | [`vector_layers`] | Google 式道路 casing / fill + POI 红点白环。 |
//! | [`streets`] | 街道矢量：从 `World.streets` 取数投到 `vector_layers`。 |
//! | [`faction_overlays`] | 势力多边形叠加层（替代 `shade_faded`）。 |
//! | [`brush_cursor`] | 编辑光标（屏幕恒定粗细）。 |
//! | [`editing`] | 笔刷 → `EditCommand` → `wv-edit::History` 的桥。 |
//! | [`drill`] | 地图三级钻取的导航层（标记布局 / 命中 / 过渡）。 |
//! | [`diagnostics_bridge`] | 内存 `World` 的派生 `Diagnostics`（V1 守约）。 |
//! | [`overture_cli`] | `wv overture ...` 子命令（ADR-0007）。 |
//! | [`fonts`] | 系统字体栈探测 + CJK 字体注册。 |
//! | [`colors`] | 28 色板令牌（基底）；权威来自 `ART_STYLE_SPEC_v0.1` §1.4。 |
//! | [`colors_google`] | Google Default Map 色板（地图默认视图）。 |
//! | [`colors_jrpg`] | JRPG 风格色板（真实地形视图）。 |
//! | [`ui_tokens`] | 界面外壳色令牌（权威来自 `UI_DESIGN_SPEC` §2.2）。 |
//! | [`ui_theme`] | 把 `ui_tokens` 投影成 egui `Visuals`。 |
//! | [`ui_time`] | 时间显示统一入口（`format_date`，§5.3）。 |
//! | [`ui_layout`] | 面板布局持久化（`ui-layout.yaml`，§10 Q5）。 |
//! | [`ui_command_palette`] | 命令注册表 + 模糊搜索；零隐藏功能（D5）。 |
//! | [`ui_shell`] | 装配 + **唯一**命令分发器 + 启动期工程接管。 |
//! | [`ui`] | UI 组件库（顶栏 / 状态栏 / 菜单 / 面板 / 共享 widget）。 |
//! | [`validation_actions`] | 校验面板 → 跳转定位的动作层。 |
//! | [`faction_panel_actions`] | 势力面板 → 提交 `EditCommand` 的动作层。 |
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
}

#[cfg(feature = "app")]
pub mod app_bootstrap;
#[cfg(feature = "app")]
pub mod brush_cursor;
#[cfg(feature = "app")]
pub mod camera;
#[cfg(feature = "app")]
pub mod colors;
#[cfg(feature = "app")]
pub mod colors_google;
#[cfg(feature = "app")]
pub mod colors_jrpg;
#[cfg(feature = "app")]
pub mod diagnostics_bridge;
#[cfg(feature = "app")]
pub mod drill;
#[cfg(feature = "app")]
pub mod editing;
#[cfg(feature = "app")]
pub mod faction_overlays;
#[cfg(feature = "app")]
pub mod faction_panel_actions;
#[cfg(feature = "app")]
pub mod fonts;
#[cfg(feature = "app")]
pub mod map_frame;
#[cfg(feature = "app")]
pub mod state;
#[cfg(feature = "app")]
pub mod streets;
#[cfg(feature = "app")]
pub mod terrain_chunks;
#[cfg(feature = "app")]
pub mod terrain_view;
#[cfg(feature = "app")]
pub mod ui;
#[cfg(feature = "app")]
pub mod ui_command_palette;
#[cfg(feature = "app")]
pub mod ui_layout;
#[cfg(feature = "app")]
pub mod ui_shell;
#[cfg(feature = "app")]
pub mod ui_theme;
#[cfg(feature = "app")]
pub mod ui_time;
#[cfg(feature = "app")]
pub mod ui_tokens;
#[cfg(feature = "app")]
pub mod validation_actions;
#[cfg(feature = "app")]
pub mod vector_layers;
#[cfg(feature = "app")]
pub mod world_projection;
#[cfg(feature = "app")]
pub mod zoom_level;
