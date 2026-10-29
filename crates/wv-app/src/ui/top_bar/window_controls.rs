//! 无边框窗的窗控按钮（2026-10-25 用户裁定去原生标题栏）。
//!
//! 原生标题栏是系统黑底，主题切亮色 / 水墨时极突兀。窗控三键
//! （最小化 / 最大化 / 关闭）移入顶栏**右上角**——首轮裁定最左上角，
//! 同日二轮改裁右上（Windows 惯例位，关闭键贴右缘）；顶栏空白背的
//! 拖拽移窗 / 双击最大化在 [`super::bar::top_bar`] 里接（拖拽区先注册
//! 垫底，本模块的按钮压在其上，互不误触）。
//!
//! 几何与右区读数段（`bar::readout_segment`）同款「按右缘绝对反推」：
//! 三键占 [`CONTROLS_W`]，读数段右缘让出同宽，两段永不相叠。
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
/// 窗控三键总宽——右区读数段按此让位（见模块文档几何段）。
pub(super) const CONTROLS_W: f32 = BTN_W * 3.0;
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

/// 顶栏最右的窗控三键：最小化 / 最大化（还原）/ 关闭（关闭键贴右缘）。
///
/// 关闭键 hover 走 `badge_err` 红底 + 亮色图标——Windows 关闭键惯例的
/// 令牌化表达（UI 禁写裸 hex，AGENTS §8）；其余两键 `hover_bg`。
/// 三键不挂 tooltip：原生窗控无提示是桌面惯例，也省去三枚 i18n Key。
pub(super) fn window_controls_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    window: &mut Window,
    chrome: &mut WindowChromeState,
    dirty: bool,
    close_confirm: &mut crate::ui_shell::CloseConfirm,
    exit: &mut MessageWriter<AppExit>,
) {
    let bar = ui.max_rect();
    let rect_at = |x0: f32| {
        egui::Rect::from_min_max(
            egui::pos2(x0, bar.top()),
            egui::pos2(x0 + BTN_W, bar.bottom()),
        )
    };
    let right = bar.right();
    if control_button(ui, shell, rect_at(right - CONTROLS_W), ICON_MINUS, false).clicked() {
        window.set_minimized(true);
    }
    // 图标随镜像态切换：square（最大化）↔ copy（还原）。
    let max_icon = if chrome.maximized {
        ICON_COPY
    } else {
        ICON_SQUARE
    };
    if control_button(ui, shell, rect_at(right - BTN_W * 2.0), max_icon, false).clicked() {
        chrome.maximized = !chrome.maximized;
        window.set_maximized(chrome.maximized);
    }
    if control_button(ui, shell, rect_at(right - BTN_W), ICON_X, true).clicked() {
        // 关闭主窗口 = 退出应用（单窗口编辑器，无「其余窗口存活」语义）。
        // UX-001 最小版：dirty 时先过关闭确认框，不直接丢工作。
        if dirty {
            close_confirm.pending = Some(crate::ui_shell::CloseAction::ExitApp);
        } else {
            exit.write(AppExit::Success);
        }
    }
}

/// 关闭键单键（`ICON_X` + danger 红底 hover），rect 由 caller 算。
///
/// 顶栏走 [`window_controls_segment`] 三键整段；本页之外的场景（欢迎页等
/// 无顶栏界面）经此复用**同一个视觉出处**——应用的关闭键长什么样，只在这一
/// 处定义，避免两份 hover 红底配方各自漂移。
pub(crate) fn close_button(ui: &mut egui::Ui, shell: &UiShell, rect: egui::Rect) -> egui::Response {
    control_button(ui, shell, rect, ICON_X, true)
}

/// 单个窗控键：全高命中格 + 居中图标 + hover 底色。`danger` = 关闭键红底。
fn control_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    icon: egui::ImageSource<'static>,
    danger: bool,
) -> egui::Response {
    let resp = ui.allocate_rect(rect, egui::Sense::click());
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
