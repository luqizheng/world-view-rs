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
        |ui| {
            ui.checkbox(&mut brush.enabled, "笔刷模式（关闭时左键拖动地图）");
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("地形")
                    .size(shell.scale.micro)
                    .color(shell.t2()),
            );
            terrain_grid(ui, &shell, &mut brush.kind);
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(format!("半径 {:.0} wu", brush.radius))
                    .size(shell.scale.small)
                    .color(shell.t2())
                    .font(mono(shell.scale.small)),
            );
            ui.add(
                egui::Slider::new(&mut brush.radius, grid.cell_size * 2.0..=600.0)
                    .show_value(false)
                    .logarithmic(true),
            );
            ui.label(
                egui::RichText::new(format!("衰减软度 {:.2}", brush.softness))
                    .size(shell.scale.small)
                    .color(shell.t2())
                    .font(mono(shell.scale.small)),
            );
            ui.add(egui::Slider::new(&mut brush.softness, 0.0..=1.0).show_value(false));
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("街道")
                    .size(shell.scale.micro)
                    .color(shell.t2()),
            );
            ui.checkbox(&mut streets.enabled, "画街道模式（左键逐点连线，右键结束）");
            if streets.enabled {
                egui::ComboBox::from_label("类别")
                    .selected_text(streets.class.label())
                    .show_ui(ui, |ui| {
                        for c in wv_core::RoadClass::ALL {
                            ui.selectable_value(&mut streets.class, c, c.label());
                        }
                    });
                ui.horizontal(|ui| {
                    ui.label("街道名");
                    // 留空 = 下一笔无名（不产生标注）。
                    ui.text_edit_singleline(&mut streets.name);
                });
                ui.label(
                    egui::RichText::new(format!("已画 {} 段", streets.drawn))
                        .size(shell.scale.small)
                        .color(shell.t3())
                        .font(mono(shell.scale.small)),
                );
                ui.horizontal(|ui| {
                    if ui.button("结束连线").clicked() {
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
                        .add_enabled(deletable > 0, egui::Button::new("撤销上一段"))
                        .clicked()
                    {
                        undo_last_street_segment(&mut projection, &drill, &mut edit);
                        streets.drawn = streets.drawn.saturating_sub(1);
                    }
                });
            }
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("拖动模式左键平移 · 笔刷模式左键涂抹 · Ctrl+滚轮改半径")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
        },
    );
}
