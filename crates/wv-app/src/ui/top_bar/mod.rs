//! 顶栏（38px v0.3 · web `.topbar` 对齐）+ 状态栏（28px · Figma #3:21001）。
//!
//! ## 职责
//! - `top_bar` —— Figma `3:20966` 三区：左 = 项目按钮（蓝 logo +
//!   粗体名 + 下翻箭头，见 [`super::project_menu`]）+ 面包屑；**中 = Browse /
//!   Edit 分段（独立 Area 锚视口顶部居中）**；右 = 保存态 / 分隔线 /
//!   **日夜模式控件** / **语言控件** / **Rebuild** / **设置入口（`settings-2`）**。
//!
//! ## 右区对齐（2026-10-19 用户裁定 → 2026-10-25 增补地图主题）
//! - Figma 的 `Cozy`（密度）控件**不建**：密度走命令面板（`Ctrl+K`）。
//! - **地图主题控件**（JRPG 水彩 / 中国水墨）已加回顶栏右区，与日夜控件
//!   并列——快捷键 `Ctrl+M`，命令面板仍可切。主题名是主题 crate 的
//!   `display_name(_en)` 常量（2026-10-25 用户裁定），不走 i18n。
//! - `settings-2` 是应用设置入口：浮动窗口显隐 + 重置位置（见
//!   [`super::settings_dialog`]）；工程菜单因此只剩工程基本操作。
//! - zoom / entities / ⌘K 不再占顶栏：对象计数在状态栏、缩放控件在地图工具条、
//!   命令面板仍是 `Ctrl+K`。
//! - `status_bar`（2026-10-16 按 Figma `Untitled` #3:21001 重做）——
//!   左 = `WORLD–VIEW` + 版本 + 工程目录名；中 = 光标世界坐标 X/Y +
//!   经纬度（真实地理图才有）+ EPSG；右 = 对象计数 + 选中数 + 警告徽标 +
//!   FPS。
//!
//! ## web 对齐要点（`index.css` / `App.tsx`，v0.3 校准）
//! - 高度 38px（`.topbar`）；底色 `bg_app` 96% + 底边框 + `0 4px 20px` 投影。
//! - 分段控件（`.segmented`）：容器 `bg_app` 底 + border + 圆角 7、内边距 2px；
//!   选中段 `bg_active` 底 + `text_1`，**内嵌**于容器而非顶满。
//! - 中区段宽 64/段（Figma Mode selector）。`Rebuild` 2026-10-19 起移到右区
//!   语言控件之后，与日夜控件同款 26px 偏好控件（图标 + 文字 + `F5`）。
//! - ⌘K 按钮已退出顶栏（`Ctrl+K` 仍可开命令面板）。
//! - 面包屑末级 `text_2`（web `b` 标签色），非末级 `text_3`。
//!
//! ## 边界
//! - **不**画项目管理菜单的展开式内容 —— [`super::project_menu::project_menu`]。
//! - **不**装配 Bevy App / Startup —— [`crate::app_bootstrap`]。
//!
//! ## 子模块（2026-10-24 由单文件 1147 行拆出）
//! - [`bar`]：顶栏三区（身份 / 主控 / 读数）。
//! - [`prefs`]：右区偏好控件（日夜 / 地图主题 / 语言 / Rebuild / 设置）。
//! - [`status`]：状态栏与各读数段。
//! - [`window_controls`]：无边框窗窗控三键（2026-10-25，去原生标题栏）。
//! - `tests`：原 `mod tests` 整段平移。
//!
//! ## 主要导出
//! - [`top_bar`] / [`status_bar`] —— 两个 Bevy 系统。
//! - [`ValidationCount`] —— 状态栏徽标读的警告计数资源。
#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{CurrentLocale, I18nKey};
use crate::state::{
    DrillState, RecentProject, RecentProjects, ViewMode, ViewState, WorldProjection,
};
use crate::theme::ui_theme::{color32, color32_alpha, mono, proportional, UiShellConfig};

use super::project_menu::{project_menu_inner, ProjectMenuState};
use super::settings_dialog::SettingsDialog;
use super::shell::UiShell;
use super::widgets::{
    badge, icon_image, BadgeKind, ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_FOLDER_OPEN,
    ICON_LANGUAGES, ICON_MAP, ICON_MOON, ICON_REFRESH, ICON_SETTINGS, ICON_SUN,
};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui_command_palette::CommandId;

mod bar;
mod prefs;
mod status;
#[cfg(test)]
mod tests;
mod window_controls;

pub use bar::top_bar;
pub use status::status_bar;
pub(crate) use window_controls::WindowChromeState;

/// 顶栏高度（web `.topbar` 38px）。
pub(crate) const TOP_BAR_H: f32 = 38.0;
/// 状态栏高度（Figma #3:21001 designedHeight 28px）。
pub(crate) const STATUS_BAR_H: f32 = 28.0;

/// 验证警告数（v0.3：状态条右侧徽标需要）。
///
/// wv-app 当前没有现成的 Resource 暴露「总验证警告数」——它由
/// `validation_actions` 模块现算。为不引入新依赖，本 Resource 由
/// `app_bootstrap` 在 Update 链上每帧 `insert_resource(ValidationCount{warnings:...})`。
/// 未来 M3 真接 `validation` Resource 时可直接替换。
#[derive(Default, Resource, Debug, Clone, Copy)]
pub struct ValidationCount {
    pub warnings: usize,
}

/// 构造一个「视口根 `Ui`」。
///
/// ⚠️ **必须留在 Background 层**（探针实测 2026-10-25）：状态栏曾为「不被
/// 浮窗盖住」把根 Ui 提到 Foreground——但根 Ui 自身会注册一个全视口
/// widget，egui `hit_test` 的层过滤（`hit_test.rs` 的 included_layers
/// break 循环）在覆盖搜索圆的更高层 widget 处提前收口，Background /
/// Middle 层的命中被整体排除——**全界面输入失灵**（顶栏拖拽区
/// `contains=false`、按钮点不动）。防遮挡由 [`central_area`] 的浮窗
/// 钳制保证，不需要抬层。
pub(crate) fn root_ui(ctx: &egui::Context) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        "wv_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    )
}

/// 浮动面板的活动范围：视口挖去顶栏与状态栏（无边框窗 2026-10-25）。
///
/// 由 [`crate::ui::widgets::float_window`] 统一喂给
/// `FloatWindow::constrain_to`——浮窗拖不进两端 chrome、也拖不出视口；
/// 按大窗口保存的 `ui-layout.yaml` 坐标（如 timeline 默认 y=880）在小窗
/// 口下会被逐帧钳回可见区，不再出现「面板丢在视口外」。
pub(crate) fn central_area(ctx: &egui::Context) -> egui::Rect {
    central_area_of(ctx.viewport_rect())
}

/// [`central_area`] 的纯几何核（供单测）：`vp` 挖去顶栏高与状态栏高。
pub(crate) fn central_area_of(vp: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(vp.min.x, vp.min.y + TOP_BAR_H),
        egui::pos2(vp.max.x, vp.max.y - STATUS_BAR_H),
    )
}

/// 越界回拉（2026-10-25）：`id` 对应浮窗上一帧的位置若越出
/// [`central_area`]，返回修正后的左上角（喂给 `egui::Window::current_pos`
/// 强制落位）；在界内返回 None（不干预用户拖拽）。
///
/// ⚠️ 为什么必须自己做：egui 0.36 的 `constrain_to` **不钳制位置**——只钳
/// 尺寸 / 锚点 / move 命中区（`area.rs` 通读无 position clamp），上一会话
/// 拖进 chrome 的 `ui-layout.yaml` 坐标会原样复现（实测：时间轴拖到贴视口
/// 底、盖住状态栏）。拖进 chrome 的那一帧松手即回弹。所有**游离浮窗**
/// 都要走这把钳子——`float_window`（`ui::widgets::local`）与直连
/// `egui::Window` 的 timeline；居中模态窗（各 dialog）不适用。
pub(crate) fn snap_into_central(ctx: &egui::Context, id: egui::Id) -> Option<egui::Pos2> {
    let central = central_area(ctx);
    ctx.memory(|m| m.area_rect(id)).and_then(|r| {
        if central.contains_rect(r) {
            return None;
        }
        Some(egui::pos2(
            r.left().clamp(
                central.left(),
                (central.right() - r.width()).max(central.left()),
            ),
            r.top().clamp(
                central.top(),
                (central.bottom() - r.height()).max(central.top()),
            ),
        ))
    })
}
