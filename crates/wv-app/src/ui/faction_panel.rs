//! 势力层（P-7；ADR-0027 多势力层）。
//!
//! ## 职责
//! - `panel_faction` —— 按势力层分组：层头（名称 / z / 配色 / 可见 / 删除）+
//!   「当前填色层」选择器 + 层内势力**列表**。
//!
//! **2026-10-22 统一 Inspector**：单个势力的**属性编辑**（名称 / 颜色 / 层 /
//! 等级 / 驻地 / 归属边 / 几何列表 / focus）已迁入 Inspector 的「属性」页签
//! （[`crate::ui::inspector::faction::faction_form`］）。本面板现在只当
//! **选择器**用——列出「有哪些势力」+ 点行选中。层头的增删改在 M4 迁入
//! Inspector 的「势力层」页签。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界 —— 改动经 [`crate::edit::faction_panel_actions`] 提交
//!   `EditCommand`。
//! - 面板文案沿用现有硬编码中文（本面板尚未接 i18n；新增键留给后续收口）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::edit::faction_panel_actions;
use crate::i18n::CurrentLocale;
use crate::state::{Panel, PanelState, PlaybackState, ViewState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;
use wv_core::{FactionColorMode, FactionLayer, Id};
use wv_render::faction_overlays::FactionDisplay;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, list_item};

/// 势力层（P-7）。
#[allow(clippy::too_many_arguments)]
pub fn panel_faction(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
    _playback: Res<PlaybackState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<editing::EditHistory>,
    mut display: ResMut<FactionDisplay>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let layers = wv_core::resolved_faction_layers(&projection.world);
    let mut factions: Vec<wv_core::Faction> = projection.world.factions.clone();
    factions.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Faction,
        None,
        current_loc.0,
        |ui| {
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
            let (rect, r) =
                ui.allocate_exact_size(egui::vec2(add_w, shell.hit), egui::Sense::click());
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
                    &mut projection,
                    &mut edit,
                    &view,
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
                    if ui
                        .selectable_label(selected.as_ref() == Some(&l.id), l.name.as_str())
                        .clicked()
                    {
                        display.active_layer = Some(l.id.clone());
                    }
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
                    &shell,
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
                    deleted = layer_controls(ui, &shell, layer, &mut projection, &mut edit, &view);
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
                            &shell,
                            shell.danger(),
                            &f.name,
                            f.id.as_str(),
                            is_selected,
                        );
                        if resp.clicked() {
                            projection.selection = Some(faction_selection(&f.id));
                        }
                        ui.add_space(space::XS);
                    }
                });
                if deleted && display.active_layer.as_ref() == Some(&layer.id) {
                    display.active_layer = None;
                }
                ui.add_space(space::XS);
            }
        },
    );
}

/// 构造「选中这个势力」的会话态引用。
///
/// 选谁是**会话态**（`WorldProjection.selection`）——不落盘、不经
/// `EditCommand`、不进撤销栈（ADR-0017 §2.7）。
pub fn faction_selection(id: &Id) -> wv_core::SelectedEntity {
    wv_core::SelectedEntity::new(wv_core::EntityKind::Faction, id.clone())
}

/// 当前选中的是否就是这一个势力（用于列表行高亮）。
pub fn faction_is_selected(selection: Option<&wv_core::SelectedEntity>, id: &Id) -> bool {
    selection.is_some_and(|s| s.kind == wv_core::EntityKind::Faction && &s.id == id)
}

/// 层头：名称 / z / 配色策略 / 可见性 / 删除。返回 true 表示本帧点了删除。
fn layer_controls(
    ui: &mut egui::Ui,
    shell: &UiShell,
    layer: &FactionLayer,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    view: &ViewState,
) -> bool {
    let mut name_buf = layer.name.clone();
    let mut z = layer.z;
    let mut color_mode = layer.color_mode;
    let mut visible = layer.visible;
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("层名：").small().color(shell.t2()));
        if ui.text_edit_singleline(&mut name_buf).changed() {
            changed = true;
        }
        ui.label(egui::RichText::new("z：").small().color(shell.t2()));
        if ui
            .add(egui::DragValue::new(&mut z).speed(0.1).range(-4..=4))
            .changed()
        {
            changed = true;
        }
        if ui.checkbox(&mut visible, "可见").changed() {
            changed = true;
        }
    });
    let mut delete = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("配色：").small().color(shell.t2()));
        if ui
            .selectable_value(&mut color_mode, FactionColorMode::Own, "自身")
            .clicked()
        {
            changed = true;
        }
        if ui
            .selectable_value(&mut color_mode, FactionColorMode::Inherit, "继承上级")
            .clicked()
        {
            changed = true;
        }
        if ui.button("删除层").clicked() {
            delete = true;
        }
    });

    if delete {
        faction_panel_actions::remove_layer_now(projection, edit, view, layer.id.clone());
        return true;
    }
    if changed {
        faction_panel_actions::update_layer_now(
            projection,
            edit,
            view,
            layer.id.clone(),
            name_buf,
            z,
            color_mode,
            visible,
        );
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{faction_is_selected, faction_selection};
    use wv_core::Id;

    #[test]
    fn faction_selection_targets_the_faction_kind_and_id() {
        let sel = faction_selection(&Id::new_unchecked("f1"));
        assert_eq!(sel.kind, wv_core::EntityKind::Faction);
        assert_eq!(sel.id.as_str(), "f1");
    }

    #[test]
    fn faction_is_selected_true_only_for_same_faction() {
        let sel = faction_selection(&Id::new_unchecked("f1"));
        assert!(faction_is_selected(Some(&sel), &Id::new_unchecked("f1")));
        assert!(!faction_is_selected(Some(&sel), &Id::new_unchecked("f2")));
    }

    #[test]
    fn faction_is_selected_false_for_other_kind_or_none() {
        let sel = wv_core::SelectedEntity::new(wv_core::EntityKind::Place, Id::new_unchecked("f1"));
        assert!(!faction_is_selected(Some(&sel), &Id::new_unchecked("f1")));
        assert!(!faction_is_selected(None, &Id::new_unchecked("f1")));
    }
}
