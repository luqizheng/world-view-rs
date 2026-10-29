//! 界面外壳装配 + 唯一命令分发器。
//!
//! ## 职责
//! - 注册 [`crate::ui_theme`] / [`crate::ui_command_palette`] / [`crate::ui_layout`] 等 `crate::ui_*` 资产。
//! - 装配快捷键（设计规格 §6.1，19 条）与命令面板（§6.3）。
//! - 布局持久化（§10 Q5）：面板被拖动 / 折叠 / 显隐时写回 `ui-layout.yaml`。
//! - **`apply_pending_commands` 是命令**唯一**分发器**（见 [§9.3 验证项 8]）：
//!   从 `CommandPalette::pending` 取 1 条、枚举派发、各类命中的具体动作都在这里。
//! - `StartupLoadRequest` 排队、文件 / 目录对话框任务、面板显隐 Toggle 注册。
//!
//! ## 边界（不做什么）
//! - **不持有权威世界数据**——经 `WorldProjection.world`；改动必须经 `wv-edit`。
//! - **不画地形**——`crate::world_projection` / `crate::terrain_view` 负责。
//! - **不画单个面板**——所有 `crate::ui::panel_*` 在 `ui.rs`。
//!
//! ## 子模块（2026-10-24 由单文件按职责段拆出）
//! - [`shell`]：快捷键、日夜主题投影、面板位置重置、窗口清屏色。
//! - [`notice`]：状态栏回执资源 [`ShellNotice`]。
//! - [`drill_state`]：`DrillState` 根图钉定与镜像同步。
//! - [`project_io`]：目录对话框任务、工程落盘、重生成前地形备份。
//! - [`project_load`]：后台线程加载驱动、加载遮罩 / 失败卡片、换世界接管。
//! - [`dispatch`]：**唯一**命令分发器与工程 / 导航子派发。
//! - [`layout`]：`ui-layout.yaml` 读回与写盘。
//! - `tests`：原 `mod tests` 整段平移（`#[cfg(test)]`）。
//!
//! ## 主要导出
//! - `register(app: &mut App)` — Bevy App 装配入口。
//! - `apply_pending_commands` / `dispatch_project_command` — **唯一**命令分发器与子派发器。
//! - `drive_project_load` / `load_error_card` — 工程加载驱动（后台线程）与失败卡片；
//!   `adopt_world_snapshot` — 运行期换世界（加载 / 新建 / 关闭）的视图接管。
//! - `save_world_to_dir` / `poll_open_project_dialog` — 工程加载与落盘收口。
//! - `persist_layout_on_change` — 布局写盘系统。
#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::app_screen::AppScreen;
use crate::i18n::CurrentLocale;
use crate::project_loader::{PendingProjectLoad, ProjectLoadTask};
use crate::state::{DrillState, RecentProjects};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_command_palette::CommandPalette;
use crate::ui_layout::LayoutFile;

pub(crate) mod close_confirm;
mod dispatch;
mod drill_state;
mod layout;
mod notice;
mod project_io;
mod project_load;
mod shell;
#[cfg(test)]
mod tests;

pub use close_confirm::{CloseAction, CloseConfirm};
pub use notice::ShellNotice;
pub use project_io::{DialogPurpose, OpenProjectTask};
pub use project_load::StartupLoadRequest;

// `ui/*` / `app_bootstrap` 等模块的文档以 `crate::ui_shell::X` 链接这些系统。
// 拆模块是实现细节，保持既有入口路径不变，避免文档链接与调用方路径失效。
pub(crate) use dispatch::apply_pending_commands;
pub(crate) use drill_state::sync_drill_mirror;
pub(crate) use layout::{load_layout_on_project_open, persist_layout_on_change};
pub(crate) use project_io::poll_open_project_dialog;
#[allow(unused_imports)] // `apply_loaded_world` 只被其它模块的文档链接引用
pub(crate) use project_load::{apply_loaded_world, drive_project_load, load_error_card};
pub(crate) use shell::apply_panel_reset;

use drill_state::seed_drill_state;
use project_load::startup_load_overlay;
use shell::{apply_ui_theme, hotkeys, sync_window_clear_color};

/// 把界面外壳的资源与系统注册进 `App`。
///
/// 调用顺序：在 `EguiPlugin` 之后、UI 面板系统之前。
pub fn register(app: &mut App) {
    // 应用设置对话框（顶栏 settings-2）：只存开关，关闭即无开销。
    // `WV_SETTINGS_OPEN=1` 启动即打开——截图验收探针，与 `WV_SHOT` 同族
    //（GUI 点击注入在本机拿不到前台焦点，视觉验收靠它）。
    let mut settings_dialog = crate::ui::SettingsDialog::default();
    if std::env::var_os("WV_SETTINGS_OPEN").is_some() {
        settings_dialog.open = true;
    }
    app.insert_resource(settings_dialog)
        .init_resource::<UiShellConfig>()
        .init_resource::<CommandPalette>()
        .init_resource::<crate::ui::ProjectMenuState>()
        .init_resource::<ShellNotice>()
        // UX-001 最小版：dirty 关闭确认状态机（dispatch 守卫 + 本模块确认框共用）。
        .init_resource::<close_confirm::CloseConfirm>()
        // UX-002：「登记事件」对话框跨帧状态（时间轴按钮 / 命令面板入口）。
        .init_resource::<crate::ui::event_create_dialog::EventCreateDialog>()
        .init_resource::<DrillState>()
        .init_resource::<RecentProjects>()
        .init_resource::<LayoutFile>()
        // 无边框窗（2026-10-25）：最大化态镜像（bevy 0.19 无 is_maximized 查询）。
        .init_resource::<crate::ui::top_bar::WindowChromeState>()
        // 地图工具条朝向 + 吸附（2026-10-19）：由 `ui-layout.yaml` 的
        // `map_dock` / `map_snap` 读写；`map_controls` 与分发器共享同一份状态。
        .init_resource::<crate::ui::map_controls::MapDockState>()
        // ADR-0018：UI 文案语言；默认 `Locale::Zh`。`load_layout_on_project_open`
        // 在切工程时回灌；`persist_layout_on_change` 在用户切换时落盘。
        .init_resource::<CurrentLocale>()
        .init_resource::<OpenProjectTask>()
        // 「新建工程」对话框状态（欢迎页 / 分发器 / 目录轮询三方共用）。
        .init_resource::<crate::ui::NewProjectDialog>()
        // CLI `-m` 的启动加载请求（`gui::run` 已注入值，这里只兜底默认）。
        .init_resource::<StartupLoadRequest>()
        // 工程加载排队 + 后台任务（2026-10-22 C 档）：加载派发与结果应用
        // 都走 `drive_project_load`，这里是它的两个资源。
        .init_resource::<PendingProjectLoad>()
        .init_resource::<ProjectLoadTask>()
        // 应用屏状态（2026-10-22 B 档）：每帧由 `sync_app_screen` 求值。
        // 初始值由 `app_bootstrap` 插入（按「有没有 CLI 加载请求」给）——
        // `init_resource` 不覆盖已存在的资源。
        .init_resource::<AppScreen>()
        // 根图落位必须早于所有读 `DrillState` 的 UI 系统（同一 `Update` 里靠顺序保证）。
        .add_systems(
            PreUpdate,
            (
                seed_drill_state,
                hotkeys.after(bevy_egui::EguiPreUpdateSet::InitContexts),
                // 主题投影必须晚于 hotkeys（Ctrl+J 同帧生效）、早于所有 UI 绘制。
                // 只在 `UiShellConfig` 变化时写 egui，避免每帧重建样式。
                apply_ui_theme.after(hotkeys),
                // 窗口位置重置：必须早于本帧面板绘制，否则旧位置会先画一遍。
                apply_panel_reset.after(apply_ui_theme),
                // 必须在主题包同步之后：本 system 会用当前主题包烘焙底图。
                poll_open_project_dialog.after(wv_render::theme::switch::sync_active_theme_pack),
            ),
        )
        // 工程加载驱动 + 屏状态求值（2026-10-22）。
        //
        // 顺序：`drive_project_load` 先跑（它会改 `has_project` / `load_error`，
        // 即屏状态的两个来源），`sync_app_screen` 再把结果写进资源——UI 系统
        // （在 egui pass 里）读到的必须是本帧最新的屏。
        .add_systems(
            PreUpdate,
            (
                drive_project_load.after(poll_open_project_dialog),
                crate::app_screen::sync_app_screen.after(drive_project_load),
            )
                .chain(),
        )
        .add_systems(
            Update,
            // 世界侧派生状态：每帧先于 pass 里的 UI 刷新。
            (
                apply_pending_commands,
                sync_drill_mirror,
                // **必须先读后写**：两者都拿 `LayoutFile`，无序时 persist 可能抢先
                // 用「默认 Dark」把磁盘上的 `theme: light` 覆盖掉，随后 load 读到的
                // 就是它自己刚写坏的默认值（打开工程永远回不到用户上次的日夜模式）。
                (load_layout_on_project_open, persist_layout_on_change).chain(),
                sync_window_clear_color,
                // 撤销栈逐出回执（UX-034）：与命令分发无依赖，进同组即可。
                notice::notify_history_trimmed,
            ),
        )
        // bevy_egui 0.42：UI 系统进 pass 调度（见 gui.rs 同款注释）。
        //
        // 2026-10-22：加载本身已移出本链（`drive_project_load` 在 PreUpdate，
        // IO 走后台线程），这里只留**画**的部分。
        //
        // `welcome_screen`（无工程闸门）排在遮罩之后、失败卡片之前：
        // 加载期间它自己会让位；失败卡片画在其后压在其上。
        .add_systems(
            bevy_egui::EguiPrimaryContextPass,
            (
                crate::ui::command_palette,
                startup_load_overlay,
                crate::ui::welcome_screen,
                load_error_card,
            )
                .chain(),
        );

    // 公共控件预览浮窗 + 截图探针（WV_WIDGET_PREVIEW / WV_SHOT 门控，缺省零开销）。
    crate::ui::widget_preview::register(app);
}
