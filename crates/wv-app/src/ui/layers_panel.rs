//! 图层管理 + 世界树（v0.3 Figma `5E8bcBCmtGAHqk3ORibBlU` web 实现对齐）。
//!
//! ## 职责
//! - `panel_layers` —— **2 列 × 8 个可交互 toggle**（web `.layer-grid`：Terrain /
//!   Terrain Smooth / Location / Event / Trail / Road / Building / Faction），
//!   与 [`crate::state::ViewState`] 的 8 个 `*_visible` 字段一一对应，右上 `8` 徽标。
//! - `panel_map_tree` —— 4 行树形（web `.tree`）：行高 27、缩进 18/30/44、
//!   选中行 = accent 18% 底 + 金框方点（`.tree-dot`），非 chevron 蓝竖条。
//!
//! ## 边界
//! - **不**画真实世界树树状结构 —— 4 行是设计稿示意；层级数据对接由后续任务推进。
//!
//! ## 主要导出
//! - `panel_layers` / `panel_map_tree` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::CurrentLocale;
use crate::state::{Panel, PanelState, ViewState};
use crate::theme::ui_theme::{color32, proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{float_window, BadgeKind, ICON_CHEVRON_RIGHT};

/// 世界树（v0.3 · web `.tree` 对齐：4 行、缩进 18/30/44、选中 accent 18% 底）。
pub fn panel_map_tree(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    _view: Res<ViewState>,
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
        Panel::MapTree,
        None,
        current_loc.0,
        |ui| {
            // 4 行：World / Region / City / Interior（web App.tsx 静态树；
            // 真实层级数据对接走后续任务）。(label, 缩进 px, 是否选中)
            let rows: [(&str, f32, bool); 4] = [
                ("World", 0.0, false),
                ("Region · Eastern Reach", 18.0, false),
                ("City · Zunyi", 30.0, false),
                ("Interior · Government Office", 44.0, true),
            ];
            for (label, indent, selected) in rows.iter() {
                tree_row(ui, &shell, label, *indent, *selected);
            }
        },
    );
}

/// 单行树节点（web `.tree > div`：高 27、圆角 5、内边距 6）。
fn tree_row(ui: &mut egui::Ui, shell: &UiShell, label: &str, indent: f32, selected: bool) {
    let row_h = 27.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::click(),
    );
    // 底色：选中 = accent 18%（web `color-mix(accent 18%)`）；hover = hover_bg。
    {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, 5.0, shell.accent().linear_multiply(0.18));
        } else if resp.hovered() {
            painter.rect_filled(rect, 5.0, shell.hover_bg());
        }
    }
    let _ = resp;
    // 行首符号：前三级是折叠 chevron；选中行是金框方点（web `.tree-dot`：
    // 6×6、1px focus 金边、圆角 1）。
    let icon_x = rect.min.x + 6.0 + indent;
    if selected {
        let painter = ui.painter();
        let dot = egui::Rect::from_min_size(
            egui::pos2(icon_x, rect.center().y - 3.0),
            egui::vec2(6.0, 6.0),
        );
        painter.rect_filled(dot, 1.0, color32(shell.palette.bg_panel));
        painter.rect_stroke(
            dot,
            1.0,
            egui::Stroke::new(1.0, shell.gold()),
            egui::StrokeKind::Inside,
        );
    } else {
        let chevron_rect = egui::Rect::from_min_size(
            egui::pos2(icon_x, rect.center().y - 6.0),
            egui::vec2(12.0, 12.0),
        );
        let _ = ui.put(
            chevron_rect,
            egui::Image::new(ICON_CHEVRON_RIGHT)
                .tint(shell.t3())
                .fit_to_exact_size(egui::vec2(12.0, 12.0)),
        );
    }
    // 文本（web：选中行 text_1，其余 text_2）
    let text_x = icon_x + 18.0;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.base),
        if selected { shell.t1() } else { shell.t2() },
    );
}

/// 图层管理（v0.3 · web `.layer-grid` 对齐：2 列 × 8 个可交互 toggle）。
///
/// 8 个开关与 [`crate::state::ViewState`] 的 8 个显隐字段一一对应
/// （面板徽标 `8` = 开关计数）；web `.toggle`：25×14 圆角胶囊，on = accent 底 +
/// 白 knob 右移，off = active 底 + text_3 knob。
pub fn panel_layers(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
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
        Panel::Layers,
        Some((BadgeKind::Neutral, "8".into())),
        current_loc.0,
        |ui| {
            // 一次性借走 8 个互不相同的字段（先降为普通 `&mut ViewState`，
            // borrowck 才允许不相交字段的并列可变借用——ResMut 每次 DerefMut
            // 都是整结构体借用），按 web 顺序排 2 列。
            let view_state: &mut ViewState = &mut view;
            let mut flags = [
                ("Terrain", &mut view_state.terrain_visible),
                ("Terrain Smooth", &mut view_state.terrain_soften),
                ("Location", &mut view_state.places_visible),
                ("Event", &mut view_state.events_visible),
                ("Trail", &mut view_state.tracks_visible),
                ("Road", &mut view_state.roads_visible),
                ("Building", &mut view_state.buildings_visible),
                ("Faction", &mut view_state.factions_visible),
            ];
            egui::Grid::new("wv_layers_grid")
                .num_columns(2)
                .spacing([space::MD, 2.0])
                .show(ui, |ui| {
                    for pair in flags.chunks_mut(2) {
                        for (name, flag) in pair.iter_mut() {
                            layer_cell(ui, &shell, name, flag);
                        }
                        ui.end_row();
                    }
                });
        },
    );
}

/// 单个图层格：名称（micro text_2）+ 右侧 toggle（web `.layer-row` 高 25）。
///
/// 整格作为命中区（点名称也能切，比 web 只点 toggle 更顺手）。
fn layer_cell(ui: &mut egui::Ui, shell: &UiShell, name: &str, flag: &mut bool) {
    let cell_w = 92.0_f32.min(ui.available_width());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell_w, 25.0), egui::Sense::click());
    ui.painter().text(
        egui::pos2(rect.min.x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        proportional(shell.scale.micro),
        shell.t2(),
    );
    // toggle（web `.toggle`：25×14、圆角 10；knob 10px，on 时右移 11px 变白）
    let toggle_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 14.0, rect.center().y),
        egui::vec2(25.0, 14.0),
    );
    let painter = ui.painter();
    painter.rect_filled(
        toggle_rect,
        radius::PILL,
        if *flag {
            shell.accent()
        } else {
            shell.active_bg()
        },
    );
    let knob_x = if *flag {
        toggle_rect.min.x + 2.0 + 11.0
    } else {
        toggle_rect.min.x + 2.0
    };
    painter.circle_filled(
        egui::pos2(knob_x + 5.0, toggle_rect.center().y),
        5.0,
        if *flag {
            egui::Color32::WHITE
        } else {
            shell.t3()
        },
    );
    if resp.clicked() {
        *flag = !*flag;
    }
}
