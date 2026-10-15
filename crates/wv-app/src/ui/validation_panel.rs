//! 校验面板（P-8）。
//!
//! ## 职责
//! - `panel_validation` —— 顶部状态条（错误/提示计数）+ 错误列表 + 提示列表，
//!   每行带 anchor 可点击跳转。
//!
//! ## 边界（不做什么）
//! - **不**重新派生 diagnostics —— 见 [`crate::diagnostics_bridge`] 与 [`crate::state::ProjectDiagnostics`]。
//! - **不**阻止保存 —— 校验只提示（AGENTS §7.5 V1）。
//!
//! ## 主要导出
//! - `panel_validation` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::validation_actions;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{
    Panel, PanelState, PlaybackState, ProjectDiagnostics, ViewState, WorldProjection,
};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{badge, float_window, BadgeKind};

/// 校验（P-8）。
#[allow(clippy::too_many_arguments)]
pub fn panel_validation(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    diagnostics: Res<ProjectDiagnostics>,
    projection: Res<WorldProjection>,
    mut drill: ResMut<crate::state::DrillState>,
    mut view: ResMut<ViewState>,
    mut playback: ResMut<PlaybackState>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let d = &diagnostics.diagnostics;
    let n_errors = d.errors().len();
    let n_hints = d.hints().len();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Validation,
        None,
        current_loc.0,
        |ui| {
            // 顶部状态条：错误用 Err 徽标，提示用 Warn 徽标。
            if n_errors > 0 {
                badge(
                    ui,
                    &shell,
                    BadgeKind::Err,
                    &format!(
                        "{}{n_errors}",
                        t_key(&current_loc, I18nKey::ValidationBadgeErrCountPrefix)
                    ),
                );
            } else {
                badge(
                    ui,
                    &shell,
                    BadgeKind::Ok,
                    t_key(&current_loc, I18nKey::ValidationBadgeErrNone),
                );
            }
            ui.add_space(space::SM);
            badge(
                ui,
                &shell,
                BadgeKind::Warn,
                &format!(
                    "{}{n_hints}",
                    t_key(&current_loc, I18nKey::ValidationBadgeWarnCountPrefix)
                ),
            );
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(t_key(&current_loc, I18nKey::ValidationSubtitle))
                    .size(shell.scale.small)
                    .color(shell.t2()),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(t_key(&current_loc, I18nKey::ValidationFootnote))
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
            ui.add_space(space::SM);
            // 错误列表（按指纹 dedup 后仍可能有 N 条同类）。
            if n_errors > 0 {
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::ValidationSectionErrors))
                        .size(shell.scale.small)
                        .strong()
                        .color(shell.t1()),
                );
                ui.add_space(space::XS);
                egui::ScrollArea::vertical()
                    .max_height(shell.row * 6.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for err in d.errors() {
                            ui.horizontal(|ui| {
                                badge(ui, &shell, BadgeKind::Err, "E");
                                ui.add_space(space::XS);
                                // 阶段八 §A 延展：错误行与提示行**同形**——
                                // 有 anchor 就可点。错误**不**阻止保存（V1 守约），
                                // 但「让作者能跳过去看一眼」是与提示同级的体面。
                                if let Some(anchor) = &err.anchor {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(&err.message)
                                                    .size(shell.scale.small)
                                                    .color(shell.t1()),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        validation_actions::jump_to_anchor_now(
                                            &projection,
                                            &mut drill,
                                            &mut view,
                                            &mut playback,
                                            &mut cameras,
                                            anchor,
                                        );
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&err.message)
                                            .size(shell.scale.small)
                                            .color(shell.t1()),
                                    );
                                }
                            });
                        }
                    });
                ui.add_space(space::SM);
            }
            // 提示列表（相遇提示 / 事件位移可行性等）。
            if n_hints > 0 {
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::ValidationSectionHints))
                        .size(shell.scale.small)
                        .strong()
                        .color(shell.t1()),
                );
                ui.add_space(space::XS);
                egui::ScrollArea::vertical()
                    .max_height(shell.row * 8.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for h in d.hints() {
                            ui.horizontal(|ui| {
                                badge(ui, &shell, BadgeKind::Warn, "H");
                                ui.add_space(space::XS);
                                // 阶段八 §A：hint 行变可点。点击触发跳转
                                // (drill_to + camera + day)。无 anchor 的提示
                                // （如 `fingerprint` 派生但还没几何）退化为普通 label。
                                if let Some(anchor) = &h.anchor {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(&h.message)
                                                    .size(shell.scale.small)
                                                    .color(shell.t1()),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        validation_actions::jump_to_anchor_now(
                                            &projection,
                                            &mut drill,
                                            &mut view,
                                            &mut playback,
                                            &mut cameras,
                                            anchor,
                                        );
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&h.message)
                                            .size(shell.scale.small)
                                            .color(shell.t1()),
                                    );
                                }
                            });
                        }
                    });
            } else if n_errors == 0 {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::ValidationOkAll))
                        .size(shell.scale.small)
                        .color(shell.t3()),
                );
            }
        },
    );
}
