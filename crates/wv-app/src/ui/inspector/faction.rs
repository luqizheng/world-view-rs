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
        // ── 草稿初始化（finding-3）──
        // 名称 / 颜色 / 等级 / 驻地全部走草稿 + 失焦提交：egui 文本框每击键
        // 都 changed，旧实现逐键提交命令 → 改一个名字产生 N 条 Undo 记录。
        // 草稿挂势力 Id：切换选中势力自动重初始化。
        if drafts.faction_drafts_for.as_ref() != Some(&id) {
            drafts.faction_drafts_for = Some(id.clone());
            drafts.faction_name_draft = faction.name.clone();
            drafts.faction_color_draft = faction.color.as_str().to_string();
            drafts.faction_color_open = false;
            drafts.faction_rank_draft = faction.rank.clone().unwrap_or_default();
            drafts.faction_seat_draft = faction
                .seat
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_default();
        }
        ui.horizontal(|ui| {
            ui.label("名称：");
            let name_resp = ui.text_edit_singleline(&mut drafts.faction_name_draft);
            // 失焦（含回车）= 一次提交；颜色用已提交值组快照。
            if name_resp.lost_focus() && drafts.faction_name_draft != faction.name {
                faction_panel_actions::update_faction_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    drafts.faction_name_draft.clone(),
                    faction.color.clone(),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.label("颜色：");
            // 取色器（wv-ui ColorPicker，2026-10-25 替换 egui 内置）：
            // 色板 + hex 双向同步都在浮层内完成；非法 hex 宽容处理
            // （§7.5 V1）——输入框危险色提示，色板保持上一个合法值。
            // 势力色是 #RRGGBB 不透明，alpha 条关掉。
            // finding-3：拖动/输入期间只写**草稿**，浮层关闭（或 hex 失焦）
            // 才一次提交——旧实现逐帧提交，拖一次色相条产生几十条 Undo。
            let picker_id = egui::Id::new(("faction_color", faction.id.as_str()));
            let draft_valid = wv_core::color::Color(drafts.faction_color_draft.clone())
                .to_rgb()
                .map(|rgb| egui::Rgba::from_srgba_unmultiplied(rgb[0], rgb[1], rgb[2], 255))
                .or_else(|| wv_ui::widgets::hex_to_rgba(&drafts.faction_color_draft));
            if let Some(mut rgba) = draft_valid {
                let resp = wv_ui::widgets::ColorPicker::new(
                    ("faction_color", faction.id.as_str()),
                    &mut rgba,
                )
                .with_alpha(false)
                .show(ui, shell);
                if resp.changed() {
                    drafts.faction_color_draft = wv_ui::widgets::rgba_to_hex(rgba);
                }
                // 浮层关闭沿 = 提交沿（关闭帧 is_id_open 已翻转）。
                let open_now = egui::Popup::is_id_open(ui.ctx(), picker_id);
                let was_open = drafts.faction_color_open;
                drafts.faction_color_open = open_now;
                if was_open && !open_now && drafts.faction_color_draft != faction.color.as_str() {
                    faction_panel_actions::update_faction_now(
                        projection,
                        edit,
                        view,
                        id.clone(),
                        faction.name.clone(),
                        wv_core::Color(drafts.faction_color_draft.clone()),
                    );
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
            // hex 文本框（与取色器同一草稿）：失焦提交。
            let hex_resp = ui
                .add(
                    egui::TextEdit::singleline(&mut drafts.faction_color_draft).desired_width(90.0),
                )
                .lost_focus();
            if hex_resp
                && wv_core::color::Color(drafts.faction_color_draft.clone())
                    .to_rgb()
                    .is_some()
                && drafts.faction_color_draft != faction.color.as_str()
            {
                faction_panel_actions::update_faction_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    faction.name.clone(),
                    wv_core::Color(drafts.faction_color_draft.clone()),
                );
            }
        });
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

        // 等级提示（自由字符串）与驻地——草稿 + 失焦提交（finding-3）。
        let rank_resp = ui
            .horizontal(|ui| {
                ui.label("等级：");
                ui.text_edit_singleline(&mut drafts.faction_rank_draft)
                    .lost_focus()
            })
            .inner;
        if rank_resp {
            let rank = drafts.faction_rank_draft.trim();
            let new_rank = if rank.is_empty() {
                None
            } else {
                Some(rank.to_string())
            };
            if new_rank != faction.rank {
                faction_panel_actions::set_faction_rank_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    new_rank,
                );
            }
        }
        let seat_resp = ui
            .horizontal(|ui| {
                ui.label("驻地：");
                ui.text_edit_singleline(&mut drafts.faction_seat_draft)
                    .lost_focus()
            })
            .inner;
        if seat_resp {
            let seat_text = drafts.faction_seat_draft.trim();
            let new_seat = if seat_text.is_empty() {
                None
            } else {
                Some(Id::new_unchecked(seat_text))
            };
            if new_seat != faction.seat {
                faction_panel_actions::set_faction_seat_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    new_seat,
                );
            }
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
