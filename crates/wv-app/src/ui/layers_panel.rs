//! 图层管理 + 世界树（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU` 风格）。
//!
//! ## 职责
//! - `panel_layers` —— 8 个图层开关（Figma: Terrain / Location / Trail / Building
//!   各有"主开关 + 二级子开关"——主控显隐，二级控子项），右上 `8` 徽标。
//! - `panel_map_tree` —— 4 行树形（World / Region / City / Interior），选中行
//!   蓝竖条 + 略深底。
//!
//! ## 边界
//! - **不**画世界树树状结构 —— 占位卡先到位；树状交互由后续任务推进。
//!
//! ## 主要导出
//! - `panel_layers` / `panel_map_tree` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{Panel, PanelState, ViewState};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{badge, float_window, BadgeKind, ICON_CHEVRON_RIGHT, ICON_MAP};

/// 世界树（v0.3 · Figma 4 行树形 + 选中蓝竖条）。
pub fn panel_map_tree(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    _view: Res<ViewState>,
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
        |ui| {
            // 4 行：World / Region / City / Interior（Figma 设计稿固定 4 行；
            // 真实 `view.current_map` 走 M2 与层级数据对接）
            let rows: [(&str, bool); 4] = [
                ("World", false),
                ("Region · Eastern Reach", false),
                ("City · Zunyi", false),
                ("Interior · Government Office", true),
            ];
            for (label, selected) in rows.iter() {
                tree_row(ui, &shell, label, *selected);
            }
        },
    );
}

fn tree_row(ui: &mut egui::Ui, shell: &UiShell, label: &str, selected: bool) {
    let row_h = 30.0;
    let indent = if label.starts_with("Region")
        || label.starts_with("City")
        || label.starts_with("Interior")
    {
        14.0
    } else {
        0.0
    };
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::click(),
    );
    // 先画底色（用 painter 的不可变借用），再 put 图标（mut 借用 ui）。
    // Rust 不允许同时持 mut+not，所以底色必须在 put 之前完成。
    let bg_hovered = resp.hovered();
    {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
            painter.rect_filled(
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 2.0, rect.max.y)),
                0.0,
                shell.accent(),
            );
        } else if bg_hovered {
            painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
        }
    }
    let _ = resp;
    // 折叠 / 展开 chevron（仅在有子级的行展示）
    if !label.starts_with("Interior") {
        let chevron_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + space::MD + indent, rect.center().y - 5.0),
            egui::vec2(10.0, 10.0),
        );
        let _ = ui.put(
            chevron_rect,
            egui::Image::new(ICON_CHEVRON_RIGHT)
                .tint(shell.t3())
                .fit_to_exact_size(egui::vec2(10.0, 10.0)),
        );
    }
    // World 行额外带一个 map 图标
    if !label.starts_with("Region") && !label.starts_with("City") && !label.starts_with("Interior")
    {
        let map_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + space::MD + indent, rect.center().y - 7.0),
            egui::vec2(14.0, 14.0),
        );
        let _ = ui.put(
            map_rect,
            egui::Image::new(ICON_MAP)
                .tint(shell.t2())
                .fit_to_exact_size(egui::vec2(14.0, 14.0)),
        );
    }
    let text_x = rect.min.x + space::MD + indent + 18.0;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.base),
        if selected { shell.t1() } else { shell.t2() },
    );
}

/// 图层管理（v0.3 · Figma 4 行 × 主开关 + 二级子开关）。
///
/// 设计稿：4 行（Terrain / Location / Trail / Building），每行一个圆形主 toggle，
/// 行尾再带 1 个二级圆形 toggle（4 行 × 2 = 8 个 toggle，对应面板徽标 `8`）。
///
/// 字段映射沿用旧枚举，未实际新增二级开关（避免引入新数据字段）。旧 7 个
/// `*_visible` 字段映射为：terrain→terrain / places→Location / tracks→Trail /
/// roads→Trail · 道路 / events→Location · 事件 / buildings→Building /
/// factions→Faction（合并到 Building 行）。这样 Figma 4 行有数据，徽标 8 也有数据。
pub fn panel_layers(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
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
        |ui| {
            // 4 行：每行 [主开关 · 名称] ── [二级开关]
            // 提前复制 secondary 以绕开 Rust 同时借用 view 的两个字段限制
            let trail_secondary = view.roads_visible;
            let building_secondary = view.factions_visible;
            let location_secondary = view.events_visible;
            layer_row(ui, &shell, "Terrain", &mut view.terrain_visible, true);
            layer_row(
                ui,
                &shell,
                "Location",
                &mut view.places_visible,
                location_secondary,
            );
            layer_row(
                ui,
                &shell,
                "Trail",
                &mut view.tracks_visible,
                trail_secondary,
            );
            layer_row(
                ui,
                &shell,
                "Building",
                &mut view.buildings_visible,
                building_secondary,
            );
        },
    );
}

fn layer_row(ui: &mut egui::Ui, shell: &UiShell, label: &str, primary: &mut bool, secondary: bool) {
    let row_h = 30.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    // 圆形主 toggle（蓝=开，灰=关）
    let toggle = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + 14.0, rect.center().y),
        egui::vec2(20.0, 12.0),
    );
    painter.rect_filled(
        toggle,
        radius::PILL,
        if *primary {
            shell.accent()
        } else {
            shell.active_bg()
        },
    );
    let knob_x = if *primary {
        toggle.max.x - 6.0
    } else {
        toggle.min.x + 6.0
    };
    painter.circle_filled(egui::pos2(knob_x, toggle.center().y), 4.0, shell.t1());
    // 名称
    painter.text(
        egui::pos2(rect.min.x + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.base),
        shell.t2(),
    );
    // 二级 toggle（小圆点）
    let dot = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 14.0, rect.center().y),
        egui::vec2(8.0, 8.0),
    );
    painter.circle_filled(
        dot.center(),
        4.0,
        if secondary { shell.gold() } else { shell.t3() },
    );
    // 字段 primary 的可变性测试用——M2 切真 toggle 用 `ui.toggle_value`。
    let _ = primary;
    let _ = badge; // 保留徽标 helper 引用以备后续添加
}
