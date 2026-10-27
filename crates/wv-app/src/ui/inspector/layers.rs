//! 「势力层」页签（统一 Inspector 的第二个页签）。
//!
//! 从旧 `faction_panel` 整体迁入（2026-10-22 统一 Inspector 设计）：
//! 层头管理（名称 / z / 配色策略 / 可见 / 删除）+「当前填色层」选择器 +
//! 势力列表（点行 = 选中 →「属性」页签）。旧面板随之退役。
//!
//! ## 与「属性」页签的分工
//!
//! - 本页签 = **集合层**：有哪几个层、每层有哪几个势力、当前填色层。
//! - 「属性」页签 = **单体层**：选中势力的名称 / 颜色 / 归属边 / 几何。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing;
use crate::edit::faction_panel_actions;
use crate::state::{ViewState, WorldProjection};
use crate::theme::ui_tokens::space;
use crate::ui::shell::UiShell;
use crate::ui::widgets::icon_image;
use crate::ui::ICON_PENCIL;

use crate::ui::faction_panel::{faction_is_selected, faction_selection};
use crate::ui::widgets::{empty_state, list_item};
use wv_render::faction_overlays::FactionDisplay;

/// 「势力层」页签内容。
#[allow(clippy::too_many_arguments)]
pub fn layers_tab(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    display: &mut FactionDisplay,
    inspector: &mut super::InspectorState,
    pen_dialog: &mut crate::ui::partition_pen::PartitionPenDialog,
    playback_day: i64,
) {
    let layers = wv_core::resolved_faction_layers(&projection.world);
    let pen_gated = crate::ui::partition_pen::pen_available_for(
        &projection.world,
        &wv_core::Id::new_unchecked(view.current_map.clone()),
    );
    let mut factions: Vec<wv_core::Faction> = projection.world.factions.clone();
    factions.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    ui.horizontal(|ui| {
        if ui.button("⋯").clicked() {
            // TODO(M2): 弹出势力批量操作菜单（导入 / 导出 / 重命名）
        }
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(format!("Map: {}", view.current_map))
                .size(shell.scale.small)
                .color(shell.t3()),
        );
    });
    ui.add_space(space::MD);

    // 新建势力层。
    let add_w = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(add_w, shell.hit), egui::Sense::click());
    ui.painter().rect_stroke(
        rect,
        crate::theme::ui_tokens::radius::CONTROL,
        egui::Stroke::new(1.0, shell.border_strong()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+ Add faction layer",
        crate::theme::ui_theme::proportional(shell.scale.base),
        shell.t2(),
    );
    if r.clicked() {
        faction_panel_actions::add_layer_now(
            projection,
            edit,
            view,
            "新势力层".into(),
            layers.len() as i32,
        );
    }
    ui.add_space(space::MD);

    // 当前填色层选择器（势力图一次只填一个层）。
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("势力图当前层：")
                .small()
                .color(shell.t2()),
        );
        if ui
            .selectable_label(display.active_layer.is_none(), "自动")
            .clicked()
        {
            display.active_layer = None;
        }
        let selected = display.active_layer.clone();
        for l in &layers {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(selected.as_ref() == Some(&l.id), l.name.as_str())
                    .clicked()
                {
                    display.active_layer = Some(l.id.clone());
                }
                // pen 按钮（ADR-0074 D3/T11）：动刀入口。门控 = 该图有自有
                // 栅格（图级 World/Region 由 Inspector 语境保证）。
                // ⚠️ 不用 with_layout(right_to_left) 右对齐——收缩 Ui 里它把
                // 行 min_rect 撑到整屏宽，Inspector 窗被顶成全屏宽（实测）。
                // ⚠️ 图标用 Lucide pencil.svg——"✎" 字符在打包字体无字形，
                // 落豆腐块（AGENTS §8 图标纪律）。
                if pen_gated {
                    let pen = ui.add(
                        egui::Button::image(icon_image(
                            ICON_PENCIL,
                            egui::Color32::WHITE,
                            12.0,
                        ))
                        .fill(egui::Color32::TRANSPARENT),
                    );
                    if pen.on_hover_text("画这张势力图").clicked() {
                        pen_dialog.open(l.id.clone(), playback_day);
                    }
                }
            });
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut display.show_other_borders, "其他层边界");
        ui.checkbox(&mut display.show_claims, "声称");
    });
    ui.add_space(space::SM);

    if factions.is_empty() && layers.is_empty() {
        empty_state(
            ui,
            shell,
            "◆",
            "尚未创建势力",
            "在工程目录下编辑 factions/*.yaml，或点击上方「+ 新建势力」",
            "查看势力层说明 →",
        );
        return;
    }

    for layer in &layers {
        let layer_id = layer.id.clone();
        let in_layer: Vec<wv_core::Faction> = factions
            .iter()
            .filter(|f| wv_core::resolve_faction_layer_id(&projection.world, f) == layer_id)
            .cloned()
            .collect();

        let mut deleted = false;
        egui::CollapsingHeader::new(format!(
            "▤ {} ({}) · {}",
            layer.name,
            layer.id,
            in_layer.len()
        ))
        .id_salt(format!("faction_layer_{}", layer.id))
        .default_open(true)
        .show(ui, |ui| {
            deleted =
                crate::ui::faction_panel::layer_controls(ui, shell, layer, projection, edit, view);
            if in_layer.is_empty() {
                ui.label(
                    egui::RichText::new("（本层暂无势力）")
                        .italics()
                        .small()
                        .color(shell.t2()),
                );
            }
            // 列表项：点击 = 选中（驱动 Inspector 的「属性」页签）。
            // 2026-10-22 统一 Inspector：势力的属性编辑已迁入
            // Inspector，本面板只当选择器 + 层管理。
            for f in &in_layer {
                let is_selected = faction_is_selected(projection.selection.as_ref(), &f.id);
                let resp = list_item(
                    ui,
                    shell,
                    shell.danger(),
                    &f.name,
                    f.id.as_str(),
                    is_selected,
                );
                if resp.clicked() {
                    projection.selection = Some(faction_selection(&f.id));
                    // 切回「属性」页签（同 region/faction 列表）。
                    inspector.tab = super::InspectorTab::Properties;
                }
                ui.add_space(space::XS);
            }
        });
        if deleted && display.active_layer.as_ref() == Some(&layer.id) {
            display.active_layer = None;
        }
        ui.add_space(space::XS);
    }
}
