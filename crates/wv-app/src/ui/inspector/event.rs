//! Event 表单（统一 Inspector 的「属性」页签 · 事件分支）。
//!
//! 事件此前**只读**（`Panel::Event` 是个列表）——本表单是它第一次有属性编辑面。
//!
//! ## 时间跨度是本体
//!
//! `Event.span` 是事件自身的时间区间 `[from, to)`——事件**就是**一段带区间
//! 的实体，不是「当前值」字段（AGENTS §4.1 的 P7 禁令针对的是「随时间变化的
//! 量被存成当前值」，而 span 恰恰是区间化的正确形态）。改 `span` = 改事件
//! 发生的时间。
//!
//! ## effects / narrations 本轮只读
//!
//! 设计稿 §11 Q1：这两个字段不在本轮编辑面内。`UpdateEventCommand` 的 revert
//! 是**逐字段**的，只写回它改过的六个字段——所以它们不在表单里，撤销也
//! **不会**误退它们（这是「窄快照」的意义，见 `wv-edit` 里 `EventFields` 的
//! 文档）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::edit::inspector_actions::{update_event_now, EventFieldPatch};
use crate::i18n::{t_key, I18nKey};
use crate::state::{ViewState, WorldProjection};
use crate::theme::ui_tokens::space;
use crate::ui::shell::UiShell;
use crate::ui::widgets::value_box;

use wv_core::model::event::{Event, EventLocation};
use wv_core::{Id, TimeSpan};

/// 事件属性表单。
#[allow(clippy::too_many_arguments)]
pub fn event_form(
    ui: &mut egui::Ui,
    shell: &UiShell,
    event: &Event,
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    current_loc: &crate::i18n::CurrentLocale,
) {
    // ── 名称 ────────────────────────────────────────────────────────
    let mut name_buf = event.name.clone();
    ui.horizontal(|ui| {
        ui.label("名称：");
        if ui.text_edit_singleline(&mut name_buf).changed() {
            let mut patch = EventFieldPatch::from_event(event);
            patch.name = name_buf.clone();
            update_event_now(projection, edit, view, patch);
        }
    });

    // ── 时间跨度 [from, to) ─────────────────────────────────────────
    let mut from_buf = event.span.from.to_string();
    let mut to_buf = event.span.to.map(|t| t.to_string()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(format!(
            "{}：",
            t_key(current_loc, I18nKey::InspectorEventSpan)
        ));
        ui.add(egui::TextEdit::singleline(&mut from_buf).desired_width(90.0));
        ui.label("→");
        ui.add(egui::TextEdit::singleline(&mut to_buf).desired_width(90.0));
        ui.label("（留空 = 至今）");
    });
    let new_from = from_buf.trim().parse::<i64>().unwrap_or(event.span.from);
    let new_to = if to_buf.trim().is_empty() {
        None
    } else {
        to_buf.trim().parse::<i64>().ok()
    };
    if new_from != event.span.from || new_to != event.span.to {
        let mut patch = EventFieldPatch::from_event(event);
        patch.span = TimeSpan {
            from: new_from,
            to: new_to,
        };
        update_event_now(projection, edit, view, patch);
    }

    // ── 发生地点 ────────────────────────────────────────────────────
    // 地点 = Place 的引用（可选带自己的 x/y/w/h 覆写）。用自由文本输 Place
    // 的 Id——人物/地点 Id 在 UI 里列不全（人物无标签），做下拉只会是半成品
    // （设计稿 §11 Q6 / Q8）。悬空引用靠校验面板提示（V1：只提示不阻止）。
    let mut loc_place = event
        .location
        .as_ref()
        .map(|l| l.place.to_string())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("地点：");
        if ui.text_edit_singleline(&mut loc_place).changed() {
            let mut patch = EventFieldPatch::from_event(event);
            patch.location = if loc_place.trim().is_empty() {
                None
            } else {
                Some(EventLocation {
                    place: Id::new_unchecked(loc_place.trim()),
                    x: event.location.as_ref().and_then(|l| l.x),
                    y: event.location.as_ref().and_then(|l| l.y),
                    w: event.location.as_ref().and_then(|l| l.w),
                    h: event.location.as_ref().and_then(|l| l.h),
                })
            };
            update_event_now(projection, edit, view, patch);
        }
    });

    // ── 参与者 ──────────────────────────────────────────────────────
    ui.add_space(space::XS);
    ui.label(
        egui::RichText::new(format!(
            "{}（{}）",
            t_key(current_loc, I18nKey::InspectorEventParticipants),
            event.participants.len()
        ))
        .small()
        .color(shell.t2()),
    );
    for p in &event.participants {
        ui.horizontal(|ui| {
            ui.label(p.as_str());
            if ui.small_button("移除").clicked() {
                let mut patch = EventFieldPatch::from_event(event);
                patch.participants.retain(|x| x != p);
                update_event_now(projection, edit, view, patch);
            }
        });
    }
    let mut add_buf = String::new();
    ui.horizontal(|ui| {
        ui.label("＋");
        ui.add(egui::TextEdit::singleline(&mut add_buf).desired_width(140.0));
        if ui.button("添加参与者").clicked() && !add_buf.trim().is_empty() {
            let mut patch = EventFieldPatch::from_event(event);
            patch.participants.push(Id::new_unchecked(add_buf.trim()));
            update_event_now(projection, edit, view, patch);
        }
    });

    // ── 叙述 / 结果 ─────────────────────────────────────────────────
    let mut narrative_buf = event.narrative.clone().unwrap_or_default();
    ui.add_space(space::XS);
    ui.label(format!(
        "{}：",
        t_key(current_loc, I18nKey::InspectorEventNarrative)
    ));
    if ui
        .add(
            egui::TextEdit::multiline(&mut narrative_buf)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        let mut patch = EventFieldPatch::from_event(event);
        patch.narrative = if narrative_buf.trim().is_empty() {
            None
        } else {
            Some(narrative_buf.clone())
        };
        update_event_now(projection, edit, view, patch);
    }

    let mut outcome_buf = event.outcome.clone().unwrap_or_default();
    ui.label(format!(
        "{}：",
        t_key(current_loc, I18nKey::InspectorEventOutcome)
    ));
    if ui
        .add(
            egui::TextEdit::multiline(&mut outcome_buf)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        let mut patch = EventFieldPatch::from_event(event);
        patch.outcome = if outcome_buf.trim().is_empty() {
            None
        } else {
            Some(outcome_buf.clone())
        };
        update_event_now(projection, edit, view, patch);
    }

    // ── effects / narrations（只读）─────────────────────────────────
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.label("效果（只读）：");
        value_box(ui, shell, &event.effects.len().to_string(), None, false);
    });
    ui.horizontal(|ui| {
        ui.label("旁白（只读）：");
        value_box(ui, shell, &event.narrations.len().to_string(), None, false);
    });

    // ── 稳定 ID（只读）──────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.label("ID：");
        value_box(ui, shell, event.id.as_str(), None, false);
    });
}
