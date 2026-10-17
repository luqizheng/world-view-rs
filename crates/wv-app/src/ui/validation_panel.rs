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

// ---------------------------------------------------------------------------
// 世界图尺度提示的参考载具（README §12-39）
// ---------------------------------------------------------------------------

/// 参考载具的施工模板：只填所需字段，其余默认。
///
/// 为什么是函数而不是 `const`：`Vehicle` 含 `Id` / `String` / `HashMap`，
/// 无法 const 构造（计划原文写 `const REF_WALK: Vehicle`，见 ledger Ruling）。
#[allow(dead_code)] // Task 3 面板行消费后移除
fn ref_vehicle(
    id: &str,
    name: &str,
    speed_kmh: f32,
    daily_hours: f32,
) -> wv_core::map::distance::Vehicle {
    wv_core::map::distance::Vehicle {
        id: wv_core::Id::new_unchecked(id),
        name: name.to_owned(),
        instant: false,
        speed_kmh: Some(speed_kmh),
        daily_hours: Some(daily_hours),
        // 空 modifiers = 任何地形 ×1.0（平原 / 海面默认）。
        terrain_modifiers: std::collections::HashMap::new(),
        requires_endpoints: false,
        requirements: Vec::new(),
    }
}

/// 参考：步行（5 km/h × 8 h/日）。
#[allow(dead_code)] // Task 3 面板行消费后移除
pub(crate) fn ref_walk() -> wv_core::map::distance::Vehicle {
    ref_vehicle("ref_walk", "步行", 5.0, 8.0)
}

/// 参考：骑马（8 km/h × 8 h/日）。
#[allow(dead_code)] // Task 3 面板行消费后移除
pub(crate) fn ref_horse() -> wv_core::map::distance::Vehicle {
    ref_vehicle("ref_horse", "骑马", 8.0, 8.0)
}

/// 参考：帆船（10 km/h × 12 h/日）。
#[allow(dead_code)] // Task 3 面板行消费后移除
pub(crate) fn ref_sail() -> wv_core::map::distance::Vehicle {
    ref_vehicle("ref_sail", "帆船", 10.0, 12.0)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考载具的日速（手算，AGENTS §7.2/§7.4）。
    ///
    /// 唯一换算入口 `Vehicle::effective_daily_speed` 的现行公式是
    /// `speed_kmh × (daily_hours / 24)`。因此 5 km/h·8h = 1.667「日速」，
    /// 而不是直觉上的 40 km/日——公式单位问题见本文件的 Ruling 记录，
    /// 改动它需要单独 ADR（会改变所有既有行程天数）。
    #[test]
    fn reference_vehicles_daily_speeds_match_hand_computed_values() {
        let walk = ref_walk()
            .effective_daily_speed("plain")
            .expect("步行应有日速");
        let horse = ref_horse()
            .effective_daily_speed("plain")
            .expect("骑马应有日速");
        let sail = ref_sail()
            .effective_daily_speed("sea")
            .expect("帆船应有日速");
        assert!((walk - 5.0 * 8.0 / 24.0).abs() < 1e-6, "步行得 {walk}");
        assert!((horse - 8.0 * 8.0 / 24.0).abs() < 1e-6, "骑马得 {horse}");
        assert!((sail - 10.0 * 12.0 / 24.0).abs() < 1e-6, "帆船得 {sail}");
    }

    /// 三种参考载具都可用（非 instant、参数齐全）。
    #[test]
    fn reference_vehicles_are_not_instant_and_have_params() {
        for v in [ref_walk(), ref_horse(), ref_sail()] {
            assert!(!v.instant, "参考载具不得是瞬时的");
            assert!(v.speed_kmh.is_some(), "{} 缺 speed_kmh", v.id);
            assert!(v.daily_hours.is_some(), "{} 缺 daily_hours", v.id);
        }
    }
}
