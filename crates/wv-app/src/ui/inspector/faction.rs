//! Faction 表单（统一 Inspector 的「属性」页签 · 势力分支）。
//!
//! 从旧 `faction_panel` 的 `faction_card` 整卡搬入（2026-10-22 统一 Inspector
//! 设计）。**搬的是内容，不是外壳**：旧的卡片自带 `CollapsingHeader`
//! （势力列表里每张卡可折叠），Inspector 已有「类型色点 + kind·id」的摘要头，
//! 故这里去掉折叠头，字段直接平铺。
//!
//! ## 与旧面板的分工
//!
//! - **势力面板**（`faction_panel`）= 层的增删改 + 势力列表 = **选谁**。
//!   （层的增删改在 M4 迁入 Inspector 的「势力层」页签。）
//! - **Inspector 的本表单** = 当前选中势力的属性 = **改它**。
//!
//! ## 驻地与归属目标用自由文本
//!
//! 沿用旧面板的形态（`TextEdit` 输 Id），不改成下拉——人物 Id 在 UI 里根本
//! 列不出来（人物无标签、无列表联动），做下拉只会做出半成品
//! （设计稿 §11 Q6）。悬空引用靠校验面板提示（AGENTS §7.5 V1：只提示不阻止）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing;
use crate::edit::faction_panel_actions;
use crate::state::{PlaybackState, ViewState, WorldProjection};
use crate::theme::ui_tokens::space;
use crate::ui::shell::UiShell;

use wv_core::{FactionLayer, Id, Membership, TimeSpan};
use wv_render::faction_overlays::FactionDisplay;

#[allow(clippy::too_many_arguments)]
pub fn faction_form(
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
    drafts: &mut super::event::InspectorDrafts,
) {
    let id = faction.id.clone();
    {
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
            // 取色器（wv-ui ColorPicker，2026-10-25 替换 egui 内置）：
            // 色板 + hex 双向同步都在浮层内完成；非法 hex 宽容处理
            // （§7.5 V1）——输入框危险色提示，色板保持上一个合法值。
            // 势力色是 #RRGGBB 不透明，alpha 条关掉。
            if let Some(rgb) = wv_core::color::Color(color_buf.clone()).to_rgb() {
                let mut rgba = egui::Rgba::from_srgba_unmultiplied(rgb[0], rgb[1], rgb[2], 255);
                let resp = wv_ui::widgets::ColorPicker::new(
                    ("faction_color", faction.id.as_str()),
                    &mut rgba,
                )
                .with_alpha(false)
                .show(ui, shell);
                if resp.changed() {
                    color_buf = wv_ui::widgets::rgba_to_hex(rgba);
                    changed = true;
                }
            } else if let Some(rgba) = wv_ui::widgets::hex_to_rgba(&color_buf) {
                // 文本是 8 位等可解析形式：同样走取色器（wv-core 的 to_rgb
                // 只吃 6/8 位；这里兜住 8 位带 alpha 的情形，提交时仍取 RGB）。
                let mut rgba = rgba;
                let resp = wv_ui::widgets::ColorPicker::new(
                    ("faction_color", faction.id.as_str()),
                    &mut rgba,
                )
                .with_alpha(false)
                .show(ui, shell);
                if resp.changed() {
                    color_buf = wv_ui::widgets::rgba_to_hex(rgba);
                    changed = true;
                }
            } else {
                // 当前文本解析不出颜色：画一块「无效色」占位（危险色系），
                // 点它不弹取色器——先把文本修成合法 #RRGGBB。
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 4.0, shell.danger().gamma_multiply(0.35));
                ui.painter().rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.0, shell.danger()),
                    egui::StrokeKind::Inside,
                );
            }
            if ui
                .add(egui::TextEdit::singleline(&mut color_buf).desired_width(90.0))
                .changed()
            {
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
            faction_panel_actions::set_faction_rank_now(projection, edit, view, id.clone(), rank);
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
            faction_panel_actions::set_faction_seat_now(projection, edit, view, id.clone(), seat);
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
        // 输入缓冲持久化（egui TextEdit 是调用方持有文本——每帧 new 的
        // 缓冲打不进字，final review I3）。
        ui.push_id(("membership_add", faction.id.as_str()), |ui| {
            if drafts.membership_from.is_empty() {
                drafts.membership_from = playback.current_day.to_string();
            }
            ui.horizontal(|ui| {
                ui.label("目标：");
                ui.add(
                    egui::TextEdit::singleline(&mut drafts.membership_target).desired_width(110.0),
                );
                ui.label("类型：");
                ui.add(egui::TextEdit::singleline(&mut drafts.membership_kind).desired_width(80.0));
                ui.label("起 day：");
                ui.add(egui::TextEdit::singleline(&mut drafts.membership_from).desired_width(60.0));
                ui.label("止：");
                ui.add(egui::TextEdit::singleline(&mut drafts.membership_to).desired_width(60.0));
                if ui.button("+ 添加归属").clicked() && !drafts.membership_target.trim().is_empty()
                {
                    let from: i64 = drafts
                        .membership_from
                        .trim()
                        .parse()
                        .unwrap_or(playback.current_day);
                    let to: Option<i64> = if drafts.membership_to.trim().is_empty() {
                        None
                    } else {
                        drafts.membership_to.trim().parse().ok()
                    };
                    let kind = if drafts.membership_kind.trim().is_empty() {
                        None
                    } else {
                        Some(drafts.membership_kind.trim().to_string())
                    };
                    faction_panel_actions::add_membership_now(
                        projection,
                        edit,
                        view,
                        id.clone(),
                        Membership {
                            target: Id::new_unchecked(drafts.membership_target.trim()),
                            kind,
                            span: TimeSpan { from, to },
                        },
                    );
                    // 提交成功清空目标（保留 day 输入，连续添加同区间更顺手）。
                    drafts.membership_target.clear();
                    drafts.membership_kind.clear();
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
        // 新增几何。（day 输入同样走持久缓冲——同 I3。）
        ui.horizontal(|ui| {
            if drafts.geometry_from.is_empty() {
                drafts.geometry_from = playback.current_day.to_string();
            }
            ui.label("起 day：");
            ui.text_edit_singleline(&mut drafts.geometry_from);
            ui.label("止（留空=至今）：");
            ui.text_edit_singleline(&mut drafts.geometry_to);
            if ui.button("+ 新建疆界").clicked() {
                let from: i64 = drafts
                    .geometry_from
                    .trim()
                    .parse()
                    .unwrap_or(playback.current_day);
                let to: Option<i64> = if drafts.geometry_to.trim().is_empty() {
                    None
                } else {
                    drafts.geometry_to.trim().parse().ok()
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
        // UX-013：两步确认（第一击武装、第二击执行；切选中势力自动解除）。
        let armed = drafts.faction_delete_armed.as_ref() == Some(&id.clone());
        let label = if armed {
            "确认删除？（此操作不可从列表撤销找回）"
        } else {
            "删除整个势力"
        };
        if ui
            .button(egui::RichText::new(label).color(egui::Color32::RED))
            .clicked()
        {
            if armed {
                drafts.faction_delete_armed = None;
                faction_panel_actions::remove_faction_now(projection, edit, view, id.clone());
            } else {
                drafts.faction_delete_armed = Some(id.clone());
            }
        }
    }
}
