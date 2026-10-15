//! 命令面板弹窗（§6.3）：Ctrl+K 打开 / Esc 关闭 / 模糊搜索 / 键盘完全可操作。
//!
//! ## 职责
//! - `command_palette` —— 弹窗绘制（遮罩 + 输入 + 结果列表 + 键盘）。
//! - 通过 `palette.pending` 把"用户选的命令"排队，由 [`crate::ui_shell::apply_pending_commands`] 派发。
//!
//! ## 边界（不做什么）
//! - **不**持有命令注册表 —— 那是 [`crate::ui_command_palette`]（后端）。
//! - **不**分发命令 —— 唯一分发器在 [`crate::ui_shell`]。
//!
//! ## 主要导出
//! - `command_palette` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::CurrentLocale;
use crate::state::{RecentProjects, WorldProjection};
use crate::theme::ui_theme::{mono, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_command_palette::CommandPalette;

use super::shell::UiShell;
use super::widgets::FieldInput;

/// 命令面板：`Ctrl+K` 打开，`Esc` 关闭，模糊搜索，键盘完全可操作。
pub fn command_palette(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    recents_res: Res<RecentProjects>,
    mut palette: ResMut<CommandPalette>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    // 「最近工程」是动态命令，每次绘制时按当前工程展开（v0.2 §C）。
    let recents = recents_res.visible(&projection);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !palette.open {
        return;
    }

    // 遮罩。
    egui::Area::new(egui::Id::new("wv_cmdk_scrim"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let rect = ctx.viewport_rect();
            ui.allocate_rect(rect, egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
        });

    let screen = ctx.viewport_rect();
    let w = (screen.width() * 0.45).clamp(360.0, 560.0);
    let x = (screen.width() - w) * 0.5;
    let y = screen.height() * 0.22;
    egui::Window::new("")
        .id(egui::Id::new("wv_cmdk"))
        // 与上面 scrim 同层（Foreground）：同层内按声明顺序绘制，窗口在遮罩
        // 之后创建 → 压在遮罩上、点得到。此前窗口缺省是 Middle——**低于**
        // Foreground 的遮罩，鼠标点击全被遮罩吃掉，面板只剩键盘输入可用
        //（2026-10-14 用户要求「弹窗/浮动都在最顶层」时一并修正）。
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, y))
        .fixed_size(egui::vec2(w, 380.0))
        .title_bar(false)
        .resizable(false)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(shell.panel_bg())
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(crate::theme::ui_theme::margin_all(space::MD))
                .corner_radius(radius::MODAL),
        )
        .show(ctx, |ui| {
            // 输入框（14px）：撑满宽度 + 打开帧抢焦点。
            FieldInput::new(&mut palette.query, "搜索命令、面板、对象…  例：笔刷 / 撤销")
                .fill_width(true)
                .focus_on_add(true)
                .show(ui, &shell);
            ui.add_space(space::SM);

            let results = palette.results_with_locale(&recents, current_loc.0);
            if results.is_empty() {
                ui.label(
                    egui::RichText::new("无匹配命令")
                        .size(shell.scale.small)
                        .color(shell.t3()),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for (i, cmd) in results.iter().enumerate() {
                            let sel = palette.selected == i;
                            let row = ui.add_sized(
                                egui::vec2(ui.available_width(), shell.row + space::XS),
                                egui::Button::selectable(sel, &cmd.label),
                            );
                            if row.clicked() {
                                palette.selected = i;
                                palette.run_selected(&recents);
                            }
                            // 快捷键（右侧等宽小字）。
                            if let Some(k) = cmd.keys {
                                let rect = row.rect;
                                ui.painter().text(
                                    egui::pos2(rect.max.x - space::SM, rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    k,
                                    mono(shell.scale.micro),
                                    shell.t3(),
                                );
                            }
                        }
                    });
            }

            // 键盘：↑ ↓ ↵ Esc。
            let input = ui.input(|i| i.clone());
            if input.key_pressed(egui::Key::Escape) {
                palette.close();
            } else if input.key_pressed(egui::Key::ArrowDown) {
                // 旧写法是 `.max(len-1)`，那会把光标永远钉在最后一项；
                // 正确语义是「向下走一格，到底停住」。
                palette.selected = (palette.selected + 1).min(results.len().saturating_sub(1));
            } else if input.key_pressed(egui::Key::ArrowUp) {
                palette.selected = palette.selected.saturating_sub(1);
            } else if input.key_pressed(egui::Key::Enter) {
                palette.run_selected(&recents);
            }
        });
}
