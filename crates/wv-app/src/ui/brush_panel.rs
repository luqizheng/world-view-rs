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
use crate::render::streets;
use crate::state::{BrushState, BrushTool, DrillState, Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
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
    // v0.3：标题徽标 `ACTIVE` 当笔刷开着时亮起（金底），Figma 风格。
    let badge = if brush.enabled {
        Some((crate::ui::BadgeKind::Warn, "ACTIVE".into()))
    } else {
        None
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Brush,
        badge,
        current_loc.0,
        |ui| {
            // ── Brush / Grid 双段切换（Figma v0.3 风格）──
            ui.horizontal(|ui| {
                let segs = [("Brush", true), ("Grid", false)];
                let n = segs.len() as f32;
                let seg_w = 120.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(seg_w, shell.hit), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(
                    rect,
                    crate::theme::ui_tokens::radius::CONTROL,
                    shell.raised_bg(),
                );
                let each = rect.width() / n;
                for (i, (label, _)) in segs.iter().enumerate() {
                    let r = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x + each * i as f32, rect.min.y),
                        egui::pos2(rect.min.x + each * (i as f32 + 1.0), rect.max.y),
                    );
                    // Grid 段高亮只跟自己的 grid_lines 走（独立于笔刷，
                    // 2026-10-15 用户新目标：网格能脱离笔刷独立显示）。
                    let sel = (*label == "Brush" && brush.enabled)
                        || (*label == "Grid" && brush.grid_lines);
                    if sel {
                        p.rect_filled(
                            r,
                            crate::theme::ui_tokens::radius::CONTROL,
                            shell.accent_soft(),
                        );
                        p.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            *label,
                            crate::theme::ui_theme::proportional(
                                crate::theme::ui_tokens::font_size::SMALL,
                            ),
                            shell.t1(),
                        );
                    } else {
                        p.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            *label,
                            crate::theme::ui_theme::proportional(
                                crate::theme::ui_tokens::font_size::SMALL,
                            ),
                            shell.t2(),
                        );
                    }
                    // 命中检测（按钮整段可点）
                    let resp =
                        ui.interact(r, egui::Id::new(("wv_brush_seg", i)), egui::Sense::click());
                    if resp.clicked() {
                        match *label {
                            "Brush" => brush.enabled = true,
                            "Grid" => {
                                // 网格独立开关：不再强关笔刷。
                                brush.grid_lines = !brush.grid_lines;
                            }
                            _ => {}
                        }
                    }
                }
            });
            ui.add_space(space::SM);

            // ── TOOL：直接涂类型 vs 意图生成（意图笔刷 plan v0.1）──
            ui.label(
                egui::RichText::new("TOOL")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
            egui::ComboBox::new("wv_brush_tool", "")
                .selected_text(brush.tool.label())
                .show_ui(ui, |ui| {
                    for t in BrushTool::ALL {
                        ui.selectable_value(&mut brush.tool, t, t.label());
                    }
                });
            ui.add_space(space::SM);

            if brush.tool.is_intent() {
                // ── 意图工具：强度 + 半径（河不用半径）+ 交互提示 ──
                slider_row(
                    ui,
                    &shell,
                    "Strength",
                    &format!("×{:.2}", brush.strength),
                    egui::Slider::new(&mut brush.strength, 0.2..=2.0),
                );
                if brush.tool != BrushTool::River {
                    slider_row(
                        ui,
                        &shell,
                        "Radius",
                        &format!(
                            "{} m",
                            (brush.radius / grid.cell_size * 10.0).round() as i32
                        ),
                        egui::Slider::new(&mut brush.radius, grid.cell_size * 2.0..=600.0)
                            .logarithmic(true),
                    );
                }
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new(brush.tool.hint())
                        .size(shell.scale.small)
                        .color(shell.t3()),
                );
            } else {
                // ── TERRAIN MATERIAL 段（Figma 用 8 块色板 4×2；`terrain_grid` 现 10 块 2×5，
                // 视觉差异可接受，M2 再统一）──
                ui.label(
                    egui::RichText::new("TERRAIN MATERIAL")
                        .size(shell.scale.micro)
                        .color(shell.t3()),
                );
                terrain_grid(ui, &shell, &mut brush.kind, current_loc.0);
                ui.add_space(space::SM);

                // ── 半径 / 软度滑条（Figma 风格：标签左、值右、轨在中间）──
                slider_row(
                    ui,
                    &shell,
                    "Radius",
                    &format!(
                        "{} m",
                        (brush.radius / grid.cell_size * 10.0).round() as i32
                    ),
                    egui::Slider::new(&mut brush.radius, grid.cell_size * 2.0..=600.0)
                        .logarithmic(true),
                );
                slider_row(
                    ui,
                    &shell,
                    "Softness",
                    &format!("{}%", (brush.softness * 100.0).round() as i32),
                    egui::Slider::new(&mut brush.softness, 0.0..=1.0),
                );
            }

            ui.add_space(space::SM);
            // 街道区（Figma `Road drawing · 3 segments` 折叠段）
            egui::CollapsingHeader::new(format!("Road drawing · {} segments", streets.drawn))
                .default_open(streets.enabled || streets.drawn > 0)
                .show(ui, |ui| {
                    ui.checkbox(
                        &mut streets.enabled,
                        "Draw mode (left-click line / right-click finish)",
                    );
                    if streets.enabled {
                        egui::ComboBox::new("wv_road_class", "")
                            .selected_text(streets.class.label())
                            .show_ui(ui, |ui| {
                                for c in wv_core::RoadClass::ALL {
                                    ui.selectable_value(&mut streets.class, c, c.label());
                                }
                            });
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ui.text_edit_singleline(&mut streets.name);
                        });
                        // Figma：Imperial Road / North Trade Route 双标签 + Finish line + ↶
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Imperial Road")
                                    .size(shell.scale.small)
                                    .color(shell.t2()),
                            );
                            ui.add_space(space::MD);
                            ui.label(
                                egui::RichText::new("North Trade Route")
                                    .size(shell.scale.small)
                                    .color(shell.t2()),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Finish line")
                                            .size(shell.scale.base)
                                            .color(shell.t1()),
                                    )
                                    .fill(shell.accent())
                                    .min_size(egui::vec2(120.0, shell.hit))
                                    .corner_radius(crate::theme::ui_tokens::radius::CONTROL),
                                )
                                .clicked()
                            {
                                streets.last_click = None;
                            }
                            let deletable = drill
                                .current_node(&projection.world)
                                .and_then(streets::target_road_layer)
                                .and_then(|l| projection.world.street_of(&l.id))
                                .map_or(0, |d| d.len());
                            if ui
                                .add_enabled(deletable > 0, egui::Button::new("↶"))
                                .clicked()
                            {
                                undo_last_street_segment(&mut projection, &drill, &mut edit);
                                streets.drawn = streets.drawn.saturating_sub(1);
                            }
                        });
                    }
                });
        },
    );
}

/// Figma 风格滑条：标签左、值右、滑条在中。
///
/// 对齐原型 `World-View UI Prototype/src/index.css` 的 `.range-label` /
/// `input[type="range]`：
///
/// - 整行单条 record：label 与值各占固定底端宽度，两端对齐；滑条吃掉剩余空间。
/// - 字号 11px（最贴原型 10px；§2.3 阶梯 MICRO=11，BASE=13 不用于滑条行）。
/// - value 不走 monospace（原型用 `font-weight: 500` Proportional，本处用 t1）。
/// - 滑条内嵌 `show_value(false)`——值已显示在右侧，外侧不再叠气泡。
/// - 强制 `trailing_fill(true)` + `HandleShape::Circle`：
///   egui 0.36 全局默认 `slider_trailing_fill=false`、`handle_shape=Rect`，
///   不开这两项就是空心方框 thumb + 无蓝色选中轨。选中轨颜色由
///   `ui.visuals().selection.bg_fill` 决定（见 `build_visuals`，已把这一项
///   路由到 `palette.accent`）。
fn slider_row(ui: &mut egui::Ui, shell: &UiShell, label: &str, value: &str, slider: egui::Slider) {
    // label / value 锁宽：让中间滑条吃光剩余宽度（最贴近原型 `justify-content:
    // space-between` 两端对齐）。具体宽度由字号 * 经验上限推得，留 2px 余量。
    let label_w = shell.scale.micro * 5.0;
    let value_w = shell.scale.micro * 4.0;
    ui.horizontal(|ui| {
        ui.allocate_exact_size(
            egui::vec2(label_w, ui.spacing().interact_size.y),
            egui::Sense::hover(),
        );
        ui.label(
            egui::RichText::new(label)
                .size(shell.scale.micro)
                .color(shell.t2()),
        );
        ui.add_space(space::SM);
        // 滑条宽度 = 当前行剩余宽度 - value 宽度 - 两侧 spacing。
        let remaining = ui.available_width() - value_w - space::SM;
        ui.allocate_ui(
            egui::vec2(remaining.max(64.0), ui.spacing().interact_size.y),
            |ui| {
                ui.add(
                    slider
                        .show_value(false)
                        .trailing_fill(true)
                        .handle_shape(egui::style::HandleShape::Circle),
                );
            },
        );
        ui.add_space(space::SM);
        ui.allocate_exact_size(
            egui::vec2(value_w, ui.spacing().interact_size.y),
            egui::Sense::hover(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value)
                    .size(shell.scale.micro)
                    .color(shell.t1()),
            );
        });
    });
}
