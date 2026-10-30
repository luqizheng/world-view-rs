//! 势力层（P-7；ADR-0027 多势力层）。
//!
//! ## 职责（**已退役的浮窗**）
//!
//! **2026-10-22 统一 Inspector**：本面板整体退役——
//! - 势力的**属性编辑** → Inspector「属性」页签（[`crate::ui::inspector::faction::faction_form`]）；
//! - 层管理 + 势力列表 → Inspector「势力层」页签（[`crate::ui::inspector::layers::layers_tab`]）。
//!
//! `panel_faction` 不再注册（`app_bootstrap`），浮窗不再渲染。文件暂留，
//! 因为 `layer_controls` / `faction_selection` / `faction_is_selected` 三个
//! 公开函数被 Inspector 的新模块引用；待 M5 视觉收尾后一并归位。
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
    mut inspector: ResMut<crate::ui::inspector::InspectorState>,
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
                    wv_core::model::faction::LayerUsage::Political,
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
                    deleted = layer_controls(
                        ui,
                        &shell,
                        current_loc.0,
                        layer,
                        &mut projection,
                        &mut edit,
                        &view,
                    );
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
                            inspector.tab = crate::ui::inspector::InspectorTab::Properties;
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
///
/// 提交粒度（UX 评审 finding-3）：名称**失焦才提交**、z **拖停才提交**
/// （egui 文本框每击键 / DragValue 每拖动刻都会 changed——旧实现逐刻度
/// 提交，改一个名字产生 N 条 Undo 记录）。草稿存 egui memory（键 = 本层
/// Id），跨帧存活、切层互不串扰；配色 / 可见是离散动作，仍即时提交（用
/// **已提交**的名称与 z 组快照，不打劫正在输入的草稿）。
pub fn layer_controls(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: crate::i18n::Locale,
    layer: &FactionLayer,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    view: &ViewState,
) -> bool {
    use crate::i18n::I18nKey;
    let key = egui::Id::new("wv_faction_layer_ctrl").with(layer.id.as_str());
    let mut name_draft = ui
        .ctx()
        .data(|d| d.get_temp::<String>(key.with("name")))
        .unwrap_or_else(|| layer.name.clone());
    let mut z_draft = ui
        .ctx()
        .data(|d| d.get_temp::<Option<i32>>(key.with("z")))
        .unwrap_or(None);
    let mut color_mode = layer.color_mode;
    let mut visible = layer.visible;
    // UX-027：Alt+「可见」= solo 的意图旗标（闭包内置位，出块后执行）。
    let mut solo = false;
    // 提交意图（出块后执行，避免闭包内与世界借用打架）。
    let mut commit_name: Option<String> = None;
    let mut commit_z: Option<i32> = None;
    let mut commit_attrs = false;

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::t(loc, I18nKey::FactionLayerNameLabel))
                .small()
                .color(shell.t2()),
        );
        let name_resp = ui.text_edit_singleline(&mut name_draft);
        if name_resp.changed() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(key.with("name"), name_draft.clone()));
        }
        // 失焦（含回车）= 一次提交；草稿清掉，下一帧回落到已提交值。
        if name_resp.lost_focus() && name_draft != layer.name {
            commit_name = Some(name_draft.clone());
        }
        ui.label(egui::RichText::new("z").small().color(shell.t2()));
        let mut z = z_draft.unwrap_or(layer.z);
        let z_resp = ui.add(egui::DragValue::new(&mut z).speed(0.1).range(-4..=4));
        if z_resp.changed() {
            z_draft = Some(z);
            ui.ctx().data_mut(|d| d.insert_temp(key.with("z"), Some(z)));
        }
        // 拖停 / 单击跳值 = 一次提交（同图层面板 opacity 口径）。
        if (z_resp.drag_stopped() || (z_resp.changed() && !z_resp.dragged())) && z != layer.z {
            commit_z = Some(z);
        }
        // UX-027：Alt+点「可见」= solo（只显示这层，再点恢复全显）——逐层
        // 隐藏是 N 次操作，solo 是 1 次（QGIS/Photoshop 标准动作，即审计的
        // 「isolate-by-visibility」）。solo 是**一个意图**，不走本层的单层
        // 提交（solo 事务统一处理所有层，一次撤销）。
        let alt_held = ui.input(|i| i.modifiers.alt);
        let vis = ui.checkbox(
            &mut visible,
            crate::i18n::t(loc, I18nKey::FactionLayerVisible),
        );
        if vis.clicked() && alt_held {
            solo = true;
            visible = layer.visible; // 撤回本层的半改，交给 solo 事务
        } else if vis.clicked() {
            commit_attrs = true;
        }
        vis.on_hover_text(crate::i18n::t(loc, I18nKey::FactionLayerSoloHint));
    });
    let mut delete = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::t(loc, I18nKey::FactionLayerColorLabel))
                .small()
                .color(shell.t2()),
        );
        if ui
            .selectable_value(
                &mut color_mode,
                FactionColorMode::Own,
                crate::i18n::t(loc, I18nKey::FactionColorOwn),
            )
            .clicked()
            && layer.color_mode != color_mode
        {
            commit_attrs = true; // 点已选中项 = 无变化，不发空命令
        }
        if ui
            .selectable_value(
                &mut color_mode,
                FactionColorMode::Inherit,
                crate::i18n::t(loc, I18nKey::FactionColorInherit),
            )
            .clicked()
            && layer.color_mode != color_mode
        {
            commit_attrs = true;
        }
        // UX-013：两步确认——第一击武装（按钮变确认文案），第二击执行。
        // 武装位存 egui memory（键 = 本层 Id）：无签名传染，切换层自动失效。
        let armed_key = egui::Id::new("wv_faction_layer_delete_armed").with(layer.id.as_str());
        let armed = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(armed_key))
            .unwrap_or(false);
        let label = if armed {
            crate::i18n::t(loc, I18nKey::FactionLayerDeleteArmed)
        } else {
            crate::i18n::t(loc, I18nKey::FactionLayerDelete)
        };
        if ui.button(label).clicked() {
            if armed {
                ui.ctx().data_mut(|d| d.insert_temp(armed_key, false));
                delete = true;
            } else {
                ui.ctx().data_mut(|d| d.insert_temp(armed_key, true));
            }
        }
    });

    if solo {
        faction_panel_actions::solo_layer_now(projection, edit, view, &layer.id.clone());
        return false;
    }
    if delete {
        faction_panel_actions::remove_layer_now(projection, edit, view, layer.id.clone());
        return true;
    }
    if let Some(name) = commit_name {
        // 名称提交：其余字段用已提交值组快照（不打劫未定稿的 z / 属性草稿）。
        ui.ctx().data_mut(|d| d.remove::<String>(key.with("name")));
        faction_panel_actions::update_layer_now(
            projection,
            edit,
            view,
            layer.id.clone(),
            name,
            layer.z,
            layer.color_mode,
            layer.visible,
        );
    }
    if let Some(z) = commit_z {
        ui.ctx()
            .data_mut(|d| d.remove::<Option<i32>>(key.with("z")));
        faction_panel_actions::update_layer_now(
            projection,
            edit,
            view,
            layer.id.clone(),
            layer.name.clone(),
            z,
            layer.color_mode,
            layer.visible,
        );
    }
    if commit_attrs {
        faction_panel_actions::update_layer_now(
            projection,
            edit,
            view,
            layer.id.clone(),
            layer.name.clone(),
            layer.z,
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
