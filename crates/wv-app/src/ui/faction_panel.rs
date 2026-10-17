//! 势力层（P-7；ADR-0027 多势力层）。
//!
//! ## 职责
//! - `panel_faction` —— 按势力层分组：层头（名称 / z / 配色 / 可见 / 删除）+
//!   「当前填色层」选择器 + 层内势力卡。
//! - `faction_card` —— 名称 / 颜色 / 层 / 等级提示 / 驻地 / 归属边 / 几何列表 /
//!   focus。
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
use wv_core::{FactionColorMode, FactionLayer, Id, Membership, TimeSpan};
use wv_render::faction_overlays::FactionDisplay;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window};

/// 势力层（P-7）。
#[allow(clippy::too_many_arguments)]
pub fn panel_faction(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
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

            let map_id = Id::new_unchecked(view.current_map.clone());
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
                    for f in &in_layer {
                        faction_card(
                            ui,
                            &shell,
                            f,
                            &layers,
                            &view,
                            &playback,
                            &map_id,
                            &mut projection,
                            &mut edit,
                            &mut display,
                        );
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

/// 一张势力卡：名称/色/层/等级/驻地 + 归属边 + 几何列表 + focus。
#[allow(clippy::too_many_arguments)]
fn faction_card(
    ui: &mut egui::Ui,
    shell: &UiShell,
    faction: &wv_core::Faction,
    layers: &[FactionLayer],
    view: &ViewState,
    playback: &PlaybackState,
    map_id: &Id,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    display: &mut FactionDisplay,
) {
    let id = faction.id.clone();
    egui::CollapsingHeader::new(format!("◆ {} ({})", faction.name, faction.id))
        .id_salt(format!("faction_{}", faction.id))
        .default_open(true)
        .show(ui, |ui| {
            // 名称 / 颜色（用本地副本 commit，避免可变借用互相打架）。
            let mut name_buf = faction.name.clone();
            let mut color_buf = faction.color.as_str().to_string();
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("名称：");
                if ui.text_edit_singleline(&mut name_buf).changed() {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("颜色：");
                if ui.text_edit_singleline(&mut color_buf).changed() {
                    changed = true;
                }
            });
            if changed {
                let color = wv_core::color::Color(color_buf.clone());
                faction_panel_actions::update_faction_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    name_buf,
                    color,
                );
            }

            // 所属层（None = 默认层）。
            let mut selected_layer = faction.layer.clone();
            let before_layer = selected_layer.clone();
            ui.horizontal_wrapped(|ui| {
                ui.label("层：");
                ui.selectable_value(&mut selected_layer, None, "默认");
                for l in layers {
                    ui.selectable_value(&mut selected_layer, Some(l.id.clone()), l.name.as_str());
                }
            });
            if selected_layer != before_layer {
                faction_panel_actions::set_faction_layer_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    selected_layer,
                );
            }

            // 等级提示（自由字符串）与驻地。
            let mut rank_buf = faction.rank.clone().unwrap_or_default();
            let rank_changed = ui
                .horizontal(|ui| {
                    ui.label("等级：");
                    ui.text_edit_singleline(&mut rank_buf).changed()
                })
                .inner;
            if rank_changed {
                let rank = if rank_buf.trim().is_empty() {
                    None
                } else {
                    Some(rank_buf.trim().to_string())
                };
                faction_panel_actions::set_faction_rank_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    rank,
                );
            }
            let mut seat_buf = faction
                .seat
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let seat_changed = ui
                .horizontal(|ui| {
                    ui.label("驻地：");
                    ui.text_edit_singleline(&mut seat_buf).changed()
                })
                .inner;
            if seat_changed {
                let seat = if seat_buf.trim().is_empty() {
                    None
                } else {
                    Some(Id::new_unchecked(seat_buf.trim()))
                };
                faction_panel_actions::set_faction_seat_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    seat,
                );
            }

            // 归属边（ADR-0027：有向、多对多、带时间）。
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!("归属（{}）", faction.memberships.len()))
                    .small()
                    .color(shell.t2()),
            );
            for m in &faction.memberships {
                ui.horizontal(|ui| {
                    let to = m
                        .span
                        .to
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "至今".into());
                    ui.label(format!(
                        "→ {} · {} · [{}, {})",
                        m.target,
                        m.kind.as_deref().unwrap_or("（未命名）"),
                        m.span.from,
                        to
                    ));
                    if ui.small_button("删除归属").clicked() {
                        faction_panel_actions::remove_membership_now(
                            projection,
                            edit,
                            view,
                            id.clone(),
                            m.target.clone(),
                            m.span.from,
                        );
                    }
                });
            }
            // 新增归属：target / kind / from / to。
            ui.push_id(("membership_add", faction.id.as_str()), |ui| {
                let mut target_buf = String::new();
                let mut kind_buf = String::new();
                let mut from_buf = playback.current_day.to_string();
                let mut to_buf = String::new();
                ui.horizontal(|ui| {
                    ui.label("目标：");
                    ui.add(egui::TextEdit::singleline(&mut target_buf).desired_width(110.0));
                    ui.label("类型：");
                    ui.add(egui::TextEdit::singleline(&mut kind_buf).desired_width(80.0));
                    ui.label("起 day：");
                    ui.add(egui::TextEdit::singleline(&mut from_buf).desired_width(60.0));
                    ui.label("止：");
                    ui.add(egui::TextEdit::singleline(&mut to_buf).desired_width(60.0));
                    if ui.button("+ 添加归属").clicked() && !target_buf.trim().is_empty() {
                        let from: i64 = from_buf.trim().parse().unwrap_or(playback.current_day);
                        let to: Option<i64> = if to_buf.trim().is_empty() {
                            None
                        } else {
                            to_buf.trim().parse().ok()
                        };
                        let kind = if kind_buf.trim().is_empty() {
                            None
                        } else {
                            Some(kind_buf.trim().to_string())
                        };
                        faction_panel_actions::add_membership_now(
                            projection,
                            edit,
                            view,
                            id.clone(),
                            Membership {
                                target: Id::new_unchecked(target_buf.trim()),
                                kind,
                                span: TimeSpan { from, to },
                            },
                        );
                    }
                });
            });

            // focus（势力图高亮自身 + 直接下级）。
            ui.horizontal(|ui| {
                if ui.button("在势力图定位").clicked() {
                    display.focus = Some(id.clone());
                }
                if display.focus.as_ref() == Some(&id) && ui.button("清除定位").clicked() {
                    display.focus = None;
                }
            });

            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!("当前地图几何：{}", view.current_map))
                    .small()
                    .color(shell.t2()),
            );
            // 几何列表：仅展示当前地图下的。
            let geos: Vec<wv_core::map::GeometrySpan> = projection
                .world
                .faction_geometry
                .iter()
                .filter(|g| g.map == *map_id && g.owner == id)
                .cloned()
                .collect();
            if geos.is_empty() {
                ui.label(
                    egui::RichText::new("（无几何）")
                        .italics()
                        .small()
                        .color(shell.t2()),
                );
            } else {
                for g in &geos {
                    let from = g.span.from;
                    let relation = g.relation.as_deref().unwrap_or("control");
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "[{}, {}) · {} · {} 顶点",
                            from,
                            g.span.to.unwrap_or(from + 1),
                            relation,
                            g.geometry.point_count()
                        ));
                        if ui.small_button("删除").clicked() {
                            faction_panel_actions::remove_geometry_now(
                                projection,
                                edit,
                                view,
                                id.clone(),
                                from,
                            );
                        }
                    });
                }
            }
            // 新增几何。
            ui.horizontal(|ui| {
                let mut from_buf = playback.current_day.to_string();
                let mut to_buf = String::new();
                ui.label("起 day：");
                ui.text_edit_singleline(&mut from_buf);
                ui.label("止（留空=至今）：");
                ui.text_edit_singleline(&mut to_buf);
                if ui.button("+ 新建疆界").clicked() {
                    let from: i64 = from_buf.trim().parse().unwrap_or(playback.current_day);
                    let to: Option<i64> = if to_buf.trim().is_empty() {
                        None
                    } else {
                        to_buf.trim().parse().ok()
                    };
                    faction_panel_actions::upsert_geometry_now(
                        projection,
                        edit,
                        view,
                        id.clone(),
                        from,
                        to,
                    );
                }
            });

            ui.add_space(space::XS);
            if ui
                .button(egui::RichText::new("删除整个势力").color(egui::Color32::RED))
                .clicked()
            {
                faction_panel_actions::remove_faction_now(projection, edit, view, id.clone());
            }
        });
}
