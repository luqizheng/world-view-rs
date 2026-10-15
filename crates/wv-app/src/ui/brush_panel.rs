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
use super::widgets::{float_window, slider_row, terrain_grid, FieldInput, FieldSelect, ToolButton};

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
            // 逐格编辑网格（plan v0.1 D4）：笔刷开着恒显；这里手动补一个
            // 「不开笔刷也想看格子」的覆盖开关（相或）。
            ui.checkbox(&mut brush.grid_lines, "显示网格");
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("地形")
                    .size(shell.scale.micro)
                    .color(shell.t2()),
            );
            terrain_grid(ui, &shell, &mut brush.kind);
            ui.add_space(space::XS);
            // 半径滑条：标签合并进 show_value，单位用「wu」。
            slider_row(
                ui,
                &shell,
                "半径",
                &mut brush.radius,
                grid.cell_size * 2.0..=600.0,
                |n| format!("半径 {:.0} wu", n),
                true,
            );
            slider_row(
                ui,
                &shell,
                "衰减软度",
                &mut brush.softness,
                0.0..=1.0,
                |n| format!("衰减软度 {:.2}", n),
                false,
            );
            ui.add_space(space::XS);
            // 街道区：默认折叠，作者真在画街道时（`enabled` 或已有段数）才展开。
            egui::CollapsingHeader::new("街道")
                .default_open(streets.enabled || streets.drawn > 0)
                .show(ui, |ui| {
                    ui.checkbox(&mut streets.enabled, "画街道模式（左键逐点连线，右键结束）");
                    if streets.enabled {
                        FieldSelect::new(
                            &mut streets.class,
                            &wv_core::RoadClass::ALL,
                            "wv_road_class",
                        )
                        .show(ui, &shell, |c| c.label().to_owned());
                        ui.horizontal(|ui| {
                            ui.label("街道名");
                            // 留空 = 下一笔无名（不产生标注）。
                            FieldInput::new(&mut streets.name, "").show(ui, &shell);
                        });
                        ui.label(
                            egui::RichText::new(format!("已画 {} 段", streets.drawn))
                                .size(shell.scale.small)
                                .color(shell.t3())
                                .font(mono(shell.scale.small)),
                        );
                        ui.horizontal(|ui| {
                            ToolButton::text("结束连线", "").show(ui, &shell, || {
                                streets.last_click = None;
                            });
                            // 撤销上一段：UI 只生成 `DeleteStreetSegmentCommand` 交给
                            // History（§3.3）；可删性在这里判断，命令层兜底越界。
                            let deletable = drill
                                .current_node(&projection.world)
                                .and_then(streets::target_road_layer)
                                .and_then(|l| projection.world.street_of(&l.id))
                                .map_or(0, |d| d.len());
                            ToolButton::text("撤销上一段", "")
                                .enabled(deletable > 0)
                                .show(ui, &shell, || {
                                    undo_last_street_segment(&mut projection, &drill, &mut edit);
                                    streets.drawn = streets.drawn.saturating_sub(1);
                                });
                        });
                    }
                });
            // 底部一行：仅保留「Ctrl+滚轮改半径」（其他提示与笔刷复选框标签重复）。
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("Ctrl+滚轮改半径")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
        },
    );
}
