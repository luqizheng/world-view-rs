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

/// 应用层持有的一次性配置。
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    /// 要打开的工程目录。`None` 时用内置占位地形。
    pub project_dir: Option<String>,
}

#[cfg(feature = "app")]
pub mod brush_cursor;
#[cfg(feature = "app")]
pub mod camera;
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
pub mod gui;
#[cfg(feature = "app")]
pub mod map_frame;
#[cfg(feature = "app")]
pub mod palette;
#[cfg(feature = "app")]
pub mod projection;
#[cfg(feature = "app")]
pub mod state;
#[cfg(feature = "app")]
pub mod terrain_chunks;
#[cfg(feature = "app")]
pub mod terrain_view;
#[cfg(feature = "app")]
pub mod ui;
#[cfg(feature = "app")]
pub mod ui_cmdk;
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
