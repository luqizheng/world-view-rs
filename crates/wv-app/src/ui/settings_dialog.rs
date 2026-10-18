//! 应用设置对话框（顶栏最右 `settings-2` 入口，Figma `3:20966`）。
//!
//! ## 职责
//! - [`SettingsDialog`] —— 跨帧开关状态。
//! - [`settings_dialog`] —— 绘制「浮动窗口」分组：逐窗显隐 + 重置位置。
//!
//! ## 边界（不做什么）
//! - **不**自己重置布局——只置 [`PanelState::reset_requested`]，由
//!   [`crate::ui_shell::apply_panel_reset`] 在下一帧统一抹 egui 记忆与
//!   `ui-layout.yaml` 的内存副本（与命令面板 `ResetPanelLayout` 同一出口）。
//! - **不**管工程菜单——工程基本功能（新建 / 打开 / 保存 / 关闭）仍在
//!   [`super::project_menu`]；本对话框只承担「工作区窗口」这一类偏好。
//! - **不**写世界数据（AGENTS §3.3）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::{font_size, space};

use super::shell::UiShell;
use super::widgets::{button, check_row, ButtonContent, ButtonStyle, DialogWindow};

/// 设置对话框跨帧状态。
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct SettingsDialog {
    /// 是否打开（顶栏 `settings-2` 置 true，关闭按钮 / Esc 置回 false）。
    pub open: bool,
}

/// 应用设置对话框。
///
/// 面板显隐**即时生效**（`PanelState` 是 `panel_*` 系统的唯一真相），
/// 落盘由 [`crate::ui_shell::persist_layout_on_change`] 在松手后统一完成。
pub fn settings_dialog(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<SettingsDialog>,
    mut panels: ResMut<PanelState>,
    current_loc: Res<CurrentLocale>,
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let mut reset = false;

    DialogWindow::new(t(loc, I18nKey::SettingsDialogTitle), &mut dialog.open)
        .width(460.0)
        .show(ctx, &shell, |ui| {
            ui.label(
                egui::RichText::new(t(loc, I18nKey::SettingsPanelsSection))
                    .size(font_size::SMALL)
                    .color(shell.t1()),
            );
            ui.label(
                egui::RichText::new(t(loc, I18nKey::SettingsPanelsHint))
                    .size(font_size::MICRO)
                    .color(shell.t3()),
            );
            ui.add_space(space::SM);

            // 两列铺 12 个面板：单列会拉到 ~350px 高，对话框超出小屏。
            // `check_row` 吃 `available_width`，放进 `columns` 才得到半宽命中区。
            ui.columns(2, |cols| {
                for (i, panel) in Panel::ALL.iter().enumerate() {
                    let ui = &mut cols[i % 2];
                    let mut on = panels.is_visible(*panel);
                    let before = on;
                    // 不挂尾随图标：`check_row` 把它贴到整行最右，两列并排时
                    // 像第二列无关字形，反而干扰读标签。
                    let _ = check_row(ui, &shell, &mut on, panel.title(loc), None);
                    if on != before {
                        panels.set_visible(*panel, on);
                    }
                }
            });

            ui.add_space(space::MD);
            if button(
                ui,
                &shell,
                ButtonContent::Text(t(loc, I18nKey::SettingsResetPanels)),
                ButtonStyle::Raised,
                Some(egui::vec2(ui.available_width(), shell.hit)),
                true,
            )
            .clicked()
            {
                reset = true;
            }
        });

    if reset {
        // 只发信号：真正抹 egui 窗口记忆发生在下一帧的 `apply_panel_reset`
        // （本帧面板已经按旧位置画完，就地重置会和它抢同一份 memory）。
        panels.reset_requested = true;
        dialog.open = false;
    }
}
