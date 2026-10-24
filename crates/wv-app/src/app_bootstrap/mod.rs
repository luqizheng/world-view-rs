//! GUI 装配（S3–S9）：Bevy + bevy_egui，把各系统按顺序接入。
//!
//! ## 职责
//! - `run(config)`：构造 `App`，依次装上插件、启动资源与全部系统，进入事件循环。
//! - 装配清单按职责分在子模块里（2026-10-24 由单文件 1045 行拆出）：
//!   [`plugins`]（插件）/ [`resources`]（启动资源）/ [`schedules`]（系统注册）/
//!   [`startup`]（相机与地形兜底）/ [`window`]（隐藏起步揭示与标题）/
//!   [`view_fit`]（初始取景）。
//!
//! ## 边界（不做什么）
//! - **feature 门控**——整个模块只在 `--features app` 下编译，让数据层（`wv-core` /
//!   `wv-io` / `wv-edit` / `wv-terrain`）开发不受 Bevy 全量编译影响（AGENTS §7.1）。
//! - **不画 UI**——只把面板注册进 schedule；面板绘制在 `ui.rs`，快捷键在 `ui_shell`。
//! - **不加载工程**——`startup_project` 为 `Some(_)` 时，由 `crate::ui_shell::drive_project_load`
//!   在窗口内做异步加载并显示进度 / 失败原因。
//!
//! ## 主要导出
//! - `run(config: AppConfig)` — 启动 GUI 的唯一入口。
//! - `fit_view_for_world(world)` — 初始取景；Startup 与运行期加载共用。

use bevy::prelude::*;

use crate::AppConfig;

mod plugins;
mod resources;
mod schedules;
mod startup;
mod view_fit;
mod window;

pub(crate) use view_fit::fit_view_for_world;

/// 启动 GUI。`startup_project` 为 `None` 时进空编辑器（占位地形），
/// `Some(dir)` 时启动后由 `crate::ui_shell::drive_project_load` 在后台线程加载，
/// 窗口内显示进度 / 失败原因。
pub fn run(config: AppConfig) {
    // 注意：这里**不做任何工程加载**（用户裁定：启动不主动加载地图）。
    // App 以空世界起步立刻开窗——首帧即是可交互的编辑器；
    // `-m` 的加载请求经 [`crate::ui_shell::StartupLoadRequest`] 在运行期执行，
    // 遮罩先画再阻塞加载，避免「白屏无反馈」。

    // 初始相机对准占位地形中心（根图 extent 缺省 4096²）。
    // 真正按工程内容 fit 的逻辑在 [`fit_view_for_world`]，
    // 运行期加载成功后会再次调用（相机 transform 才是主真源）。
    let (center, fit_scale) = fit_view_for_world(&wv_core::World::default());

    let mut app = App::new();
    plugins::configure(&mut app);
    resources::insert_all(&mut app, &config, center, fit_scale);
    // LLM 可控编辑（ADR-0014 / 0016）：仅 `--features http` 时启用。
    // 配置（`bind` / `port`）由 CLI `--http-bind` / `--http-port` 注入
    // （v0.2 增量）；默认 `127.0.0.1:8732` 走 `HttpServerConfig::default()`。
    #[cfg(feature = "http")]
    app.add_plugins(crate::http_integration::HttpServerPlugin {
        config: config.http_server_config.clone(),
    });
    schedules::register(&mut app);

    app.run();
}
