//! 无边框窗的窗控按钮（2026-10-25 用户裁定去原生标题栏）。
//!
//! 原生标题栏是系统黑底，主题切亮色 / 水墨时极突兀。窗控三键
//! （最小化 / 最大化 / 关闭）移入顶栏**最左上角**（用户指定方位）；
//! 顶栏空白背的拖拽移窗 / 双击最大化在 [`super::bar::top_bar`] 里接
//! （拖拽区先注册垫底，本模块的按钮压在其上，互不误触）。
//!
//! 图标走 Lucide（AGENTS §8）：`minus` / `square` ↔ `copy`（还原态）/ `x`。
//!
//! ## 最大化态为何自带镜像资源
//!
//! bevy 0.19 的 `bevy_winit` 只**消费** `set_maximized` 请求、不回写
//! `Window::maximized` 字段，也没有 `is_maximized` 查询 API——按钮的
//! square/copy 图标切换只能读 [`WindowChromeState`] 镜像。已知盲区：
//! Win+方向键等系统级最大化不经过本资源，图标可能滞后一拍；再点一次
//! 即对齐（`set_maximized` 对同值幂等，不会反向跳动）。

use bevy::prelude::*;
use bevy_egui::egui;

use crate::theme::ui_theme::color32;
use crate::ui::shell::UiShell;
use crate::ui::widgets::{ICON_COPY, ICON_MINUS, ICON_SQUARE, ICON_X};

/// 窗控按钮宽（高 = 顶栏内容全高，命中区饱满）。
const BTN_W: f32 = 30.0;
/// 窗控图标边长。
const ICON: f32 = 13.0;

/// 窗口最大化态镜像（见模块文档「最大化态为何自带镜像资源」）。
///
/// `pub`：`top_bar` 系统签名引用本类型（`private_interfaces` 门槛），
/// 与同模块 [`super::ValidationCount`] 同款处置。
#[derive(Default, Resource, Debug, Clone, Copy)]
pub struct WindowChromeState {
    pub maximized: bool,
}

/// 顶栏最左的窗控三键：最小化 / 最大化（还原）/ 关闭。
///
/// 关闭键 hover 走 `badge_err` 红底 + 亮色图标——Windows 关闭键惯例的
/// 令牌化表达（UI 禁写裸 hex，AGENTS §8）；其余两键 `hover_bg`。
/// 三键不挂 tooltip：原生窗控无提示是桌面惯例，也省去三枚 i18n Key。
pub(super) fn window_controls_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    window: &mut Window,
    chrome: &mut WindowChromeState,
    exit: &mut MessageWriter<AppExit>,
) {
    let h = ui.available_height().min(super::TOP_BAR_H);
    if control_button(ui, shell, h, ICON_MINUS, false).clicked() {
        window.set_minimized(true);
    }
    // 图标随镜像态切换：square（最大化）↔ copy（还原）。
    let max_icon = if chrome.maximized {
        ICON_COPY
    } else {
        ICON_SQUARE
    };
    if control_button(ui, shell, h, max_icon, false).clicked() {
        chrome.maximized = !chrome.maximized;
        window.set_maximized(chrome.maximized);
    }
    if control_button(ui, shell, h, ICON_X, true).clicked() {
        // 关闭主窗口 = 退出应用（单窗口编辑器，无「其余窗口存活」语义）。
        exit.write(AppExit::Success);
    }
}

/// 单个窗控键：全高命中格 + 居中图标 + hover 底色。`danger` = 关闭键红底。
fn control_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    h: f32,
    icon: egui::ImageSource<'static>,
    danger: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(BTN_W, h), egui::Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        let fill = if danger {
            color32(shell.palette.badge_err)
        } else {
            shell.hover_bg()
        };
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(ICON, ICON));
    // 红底 hover 上换主文本色保对比；常态一律次文本色（与设置键同款）。
    let tint = if danger && hovered {
        shell.t1()
    } else {
        shell.t2()
    };
    let _ = ui.put(
        icon_rect,
        egui::Image::new(icon)
            .tint(tint)
            .fit_to_exact_size(egui::vec2(ICON, ICON)),
    );
    resp
}
