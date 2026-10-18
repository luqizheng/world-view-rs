//! 公共控件预览浮窗 + 截图探针（**验收工具，非业务面板**）。
//!
//! 双环境变量门控，缺省（都不设）时本模块零开销、零画面：
//!
//! - `WV_WIDGET_PREVIEW=1` —— 每帧画一个「Inspector 样」浮窗
//!   （Figma `42:2` 全套公共控件按设计稿装配：[`FloatWindow`] +
//!   `identity_header` + `PropertyRow`×3 + `check_row` + `button` Raised 行），
//!   供人工/截图验收组件观感。
//! - `WV_SHOT=<png 路径>` —— 第 120 帧自动发一次
//!   `bevy::render::view::screenshot::Screenshot` 落盘（既有实机取图法，
//!   见 2026-10-15 截图验收先例）；不自动退出，由外部脚本杀进程。
//!
//! ## 为什么存在
//!
//! 公共控件**没有业务调用点**（等接线），全门禁（含 smoke-gui）只能证明
//! 「不崩」，证明不了「长什么样」。本浮窗是控件的验收载体——接线新面板前
//! 先在这里看效果。
//!
//! ## 占位说明（验收时须知的刻意偏差）
//!
//! - 头部图标用 `ICON_SLIDERS`（Lucide `sliders-horizontal`，Figma `42:4`）；
//! - 身份块瓦片底 = `gold × 0.35` 顶替设计稿类别色 `#4B422E`；
//! - 按钮图标用 `ICON_MAXIMIZE` / `ICON_PENCIL` 顶替 `scan` / `square-pen`；
//! - 挂件用 `ICON_MAP_PIN`（金 tint）/ `ICON_CHECK` 顶替 `pin` / `lock`。
//! - 色彩一律走令牌（控件纪律），与 42-2 色板的既有偏差见 floater/controls 注释。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::theme::ui_theme::{mono, proportional, UiShellConfig};
use crate::ui::shell::UiShell;
use crate::ui::widgets::{
    button, check_row, divider, identity_header, ButtonContent, ButtonStyle, FloatWindow, HeadIcon,
    PropertyRow, ICON_CHECK, ICON_GLOBE, ICON_MAP_PIN, ICON_MAXIMIZE, ICON_PENCIL, ICON_SLIDERS,
};

/// 预览浮窗系统（`EguiPrimaryContextPass` 调度，环境变量不设时直接返回）。
fn widget_preview(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut visible: Local<bool>,
) {
    if std::env::var_os("WV_WIDGET_PREVIEW").is_none() {
        return;
    }
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    FloatWindow::new("wv_widget_preview", "Inspector")
        .icon(HeadIcon::Svg(ICON_SLIDERS))
        .default_pos(egui::pos2(700.0, 60.0))
        .default_size(egui::vec2(288.0, 340.0))
        .show(ctx, &shell, |ui| {
            // Figma `42:10` Panel content：padding 12、行距 12。
            egui::Frame::default()
                .inner_margin(crate::theme::ui_theme::margin_all(12.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    // 身份块（瓦片底 = gold 压暗顶替类别色）。
                    let _ = identity_header(
                        ui,
                        &shell,
                        shell.gold().gamma_multiply(0.35),
                        ICON_GLOBE,
                        "Government Office",
                        "Point of interest · Civic",
                        Some(ICON_MAP_PIN),
                    );
                    // mono id + 金色状态（Figma `42:20` Object identifiers）。
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("poi_government_001")
                                .font(mono(11.0))
                                .color(shell.t3()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new("Selection pinned")
                                    .font(proportional(11.0))
                                    .color(shell.gold()),
                            );
                        });
                    });
                    divider(ui, &shell);
                    // 属性行三形态：下拉 / 纯文本 / 带单位。
                    PropertyRow::new("Type", "Government")
                        .dropdown(true)
                        .show(ui, &shell);
                    PropertyRow::new("Position", "1142.00 / 496.00").show(ui, &shell);
                    PropertyRow::new("Elevation", "842")
                        .suffix("m")
                        .show(ui, &shell);
                    // 可见性复选行（挂件用 ICON_CHECK 顶替 lock-keyhole-open）。
                    check_row(ui, &shell, &mut visible, "Visible on map", Some(ICON_CHECK));
                    // 操作按钮行（Raised = Figma 42:47/42:51 Button 对位）。
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 8.0) / 2.0;
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::ImageText(ICON_MAXIMIZE, "Focus  F"),
                            ButtonStyle::Raised,
                            Some(egui::vec2(w, 28.0)),
                            true,
                        );
                        let _ = button(
                            ui,
                            &shell,
                            ButtonContent::ImageText(ICON_PENCIL, "Edit details"),
                            ButtonStyle::Raised,
                            Some(egui::vec2(w, 28.0)),
                            true,
                        );
                    });
                });
        });
}

/// 截图探针（`WV_SHOT=<路径>` 时第 120 帧落盘一张，不自动退出）。
fn screenshot_probe(mut frames: Local<u32>, mut sent: Local<bool>, mut commands: Commands) {
    let Some(path) = std::env::var_os("WV_SHOT") else {
        return;
    };
    if *sent {
        return;
    }
    *frames += 1;
    if *frames >= 120 {
        *sent = true;
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(path));
        info!("[wv_shot] screenshot requested at frame {}", *frames);
    }
}

/// 注册预览与探针系统（由 [`crate::ui_shell::register`] 尾部调用）。
pub fn register(app: &mut App) {
    app.add_systems(bevy_egui::EguiPrimaryContextPass, widget_preview)
        .add_systems(Update, screenshot_probe);
}
