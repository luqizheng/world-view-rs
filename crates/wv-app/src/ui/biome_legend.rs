//! 群系图例（CellMap 左上角 · 2026-10-22 验收：专业数据视图必备）。
//!
//! 21 个群系按族分组列出：色块 + 中文名。仅在 Topographic（CellMap）显示，
//! 且随 `ViewState.nature_overlay` 无关——图例本身是「数据视图的读图说明」，
//! 任何时候看 CellMap 都该有。
//!
//! 数据源：[`wv_render::terrain_view::biome_raw`]（色板 + 名 + 族分组同源，
//! 改色板图例自动跟随）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::ViewState;
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;

use wv_render::terrain_view::biome_raw::{BIOME_LEGEND_GROUPS, BIOME_NAMES_ZH, BIOME_RAW_COLORS};

/// 群系图例浮层（左下角，缩放控件上方）。
pub fn biome_legend(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    view: Res<ViewState>,
    screen: Res<crate::app_screen::AppScreen>,
) {
    use crate::state::ViewMode;
    if !matches!(view.mode, ViewMode::Topographic) || !screen.editor_visible() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);

    egui::Area::new(egui::Id::new("wv_biome_legend"))
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(12.0, -44.0))
        .order(egui::Order::Background)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(shell.raised_bg().gamma_multiply(0.94))
                .stroke(egui::Stroke::new(1.0, shell.border_subtle()))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("群系图例")
                            .size(shell.scale.micro)
                            .color(shell.t3()),
                    );
                    ui.add_space(3.0);
                    for (family, ids) in BIOME_LEGEND_GROUPS {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(
                                egui::RichText::new(family)
                                    .size(shell.scale.micro)
                                    .color(shell.t2()),
                            );
                            for id in ids {
                                if id == 255 {
                                    continue;
                                }
                                let rgb = BIOME_RAW_COLORS[id as usize];
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(10.0, 10.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    2.0,
                                    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                                );
                                ui.label(
                                    egui::RichText::new(BIOME_NAMES_ZH[id as usize])
                                        .size(shell.scale.micro)
                                        .color(shell.t1()),
                                );
                                ui.add_space(3.0);
                            }
                        });
                    }
                });
        });
}
