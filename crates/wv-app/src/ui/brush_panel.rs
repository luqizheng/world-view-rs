//! 地形笔刷 + 街道编辑器面板（P-3 / ADR-0010 编辑器 UI）。
//!
//! ## 职责
//! - `panel_brush` —— 笔刷开关 / 地形色块 / 半径 / 软度 / 街道模式 / 撤销段。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界 —— 撤销段生成 `DeleteStreetSegmentCommand` 交给 [`crate::edit::editing::EditHistory`]。
//! - **不**持有街道矢量数据 —— 那是 [`crate::streets`]。
//!
//! ## 主要导出
//! - `panel_brush` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::i18n::CurrentLocale;
use crate::i18n::{t_key, I18nKey};
use crate::render::streets;
use crate::state::{BrushState, DrillState, Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::{mono, UiShellConfig};
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::timeline::undo_last_street_segment;
use super::widgets::{float_window, terrain_grid};

/// 地形笔刷 + 街道编辑器面板（P-3 / ADR-0010 编辑器 UI）。
#[allow(clippy::too_many_arguments)]
pub fn panel_brush(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut brush: ResMut<BrushState>,
    mut streets: ResMut<streets::StreetEditorState>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    mut edit: ResMut<editing::EditHistory>,
    grid: Res<crate::state::GridConfig>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Brush,
        None,
        current_loc.0,
        |ui| {
            ui.checkbox(
                &mut brush.enabled,
                t_key(&current_loc, I18nKey::BrushModeToggle),
            );
            // 逐格编辑网格（plan v0.1 D4）：笔刷开着恒显；这里手动补一个
            // 「不开笔刷也想看格子」的覆盖开关（相或）。
            ui.checkbox(
                &mut brush.grid_lines,
                t_key(&current_loc, I18nKey::BrushShowGrid),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(t_key(&current_loc, I18nKey::BrushTerrainHeader))
                    .size(shell.scale.micro)
                    .color(shell.t2()),
            );
            terrain_grid(ui, &shell, &mut brush.kind, current_loc.0);
            ui.add_space(space::XS);
            // 半径滑条：标签合并进 show_value，单位用「wu」。
            // 数值由 custom_formatter 拼，「半径」「wu」字串走 i18n。
            let radius_prefix = t_key(&current_loc, I18nKey::BrushRadiusPrefix).to_owned();
            let radius_unit = t_key(&current_loc, I18nKey::BrushRadiusUnitWu).to_owned();
            ui.add(
                egui::Slider::new(&mut brush.radius, grid.cell_size * 2.0..=600.0)
                    .logarithmic(true)
                    .show_value(true)
                    .custom_formatter(move |n, _| format!("{radius_prefix}{:.0}{radius_unit}", n)),
            );
            let soft_prefix = t_key(&current_loc, I18nKey::BrushSoftnessPrefix).to_owned();
            ui.add(
                egui::Slider::new(&mut brush.softness, 0.0..=1.0)
                    .show_value(true)
                    .custom_formatter(move |n, _| format!("{soft_prefix}{:.2}", n)),
            );
            ui.add_space(space::XS);
            // 街道区：默认折叠，作者真在画街道时（`enabled` 或已有段数）才展开。
            egui::CollapsingHeader::new(t_key(&current_loc, I18nKey::BrushStreetsHeader))
                .default_open(streets.enabled || streets.drawn > 0)
                .show(ui, |ui| {
                    ui.checkbox(
                        &mut streets.enabled,
                        t_key(&current_loc, I18nKey::BrushStreetsToggle),
                    );
                    if streets.enabled {
                        egui::ComboBox::from_label(t_key(&current_loc, I18nKey::BrushClassLabel))
                            .selected_text(streets.class.label())
                            .show_ui(ui, |ui| {
                                for c in wv_core::RoadClass::ALL {
                                    ui.selectable_value(&mut streets.class, c, c.label());
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label(t_key(&current_loc, I18nKey::BrushStreetNameLabel));
                            // 留空 = 下一笔无名（不产生标注）。
                            ui.text_edit_singleline(&mut streets.name);
                        });
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{}{}",
                                t_key(&current_loc, I18nKey::BrushStreetsDrawnPrefix),
                                streets.drawn,
                                t_key(&current_loc, I18nKey::BrushStreetsDrawnUnit),
                            ))
                            .size(shell.scale.small)
                            .color(shell.t3())
                            .font(mono(shell.scale.small)),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .button(t_key(&current_loc, I18nKey::BrushFinishLineBtn))
                                .clicked()
                            {
                                streets.last_click = None;
                            }
                            // 撤销上一段：UI 只生成 `DeleteStreetSegmentCommand` 交给
                            // History（§3.3）；可删性在这里判断，命令层兜底越界。
                            let deletable = drill
                                .current_node(&projection.world)
                                .and_then(streets::target_road_layer)
                                .and_then(|l| projection.world.street_of(&l.id))
                                .map_or(0, |d| d.len());
                            if ui
                                .add_enabled(
                                    deletable > 0,
                                    egui::Button::new(t_key(
                                        &current_loc,
                                        I18nKey::BrushUndoLastSegmentBtn,
                                    )),
                                )
                                .clicked()
                            {
                                undo_last_street_segment(&mut projection, &drill, &mut edit);
                                streets.drawn = streets.drawn.saturating_sub(1);
                            }
                        });
                    }
                });
            // 底部一行：仅保留「Ctrl+滚轮改半径」（其他提示与笔刷复选框标签重复）。
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(t_key(&current_loc, I18nKey::BrushHintRadiusShortcut))
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
        },
    );
}
