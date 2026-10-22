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

/// Inspector 表单的**暂存输入缓冲**。
///
/// egui 的 TextEdit 是**调用方持有文本**——每帧 `String::new()` 的话，
/// 打的字下一帧就没了，按钮的「非空才提交」守卫永远过不去（final review
/// I3；旧势力面板的「添加归属」输入同病，本资源把它一并治了）。
/// 暂存值跨帧存这里；提交成功后清空。
#[derive(Resource, Debug, Default, Clone)]
pub struct InspectorDrafts {
    /// Event 表单：「添加参与者」输入框。
    pub event_participant: String,
    /// Faction 表单：「添加归属」目标 Id。
    pub membership_target: String,
    /// Faction 表单：「添加归属」关系类型。
    pub membership_kind: String,
    /// Faction 表单：「添加归属」起 day。
    pub membership_from: String,
    /// Faction 表单：「添加归属」止 day。
    pub membership_to: String,
    /// Faction 表单：「新建疆界」起 day。
    pub geometry_from: String,
    /// Faction 表单：「新建疆界」止 day。
    pub geometry_to: String,
}

/// 解析「起 day」输入；解析失败（含空串）返回 `fallback`（= 不改）。
fn parse_day_or(text: &str, fallback: i64) -> i64 {
    text.trim().parse::<i64>().unwrap_or(fallback)
}

/// 解析「止 day」输入：**空串 = 开放区间（None）**；非空但解析失败 = 保持
/// `fallback`（= 不改）。解析失败绝不能被当成 None——打「2w」这样一瞬间的
/// 中间态会把事件的结束时间悄悄抹掉（final review I2）。
fn parse_day_open_or(text: &str, fallback: Option<i64>) -> Option<i64> {
    match text.trim() {
        "" => None,
        t => t.parse::<i64>().ok().or(fallback),
    }
}

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
    drafts: &mut InspectorDrafts,
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
    // 解析失败 = 「还没打完」= **不改**；只有空串才表示「开放到永远」。
    // 若把 parse 失败当 None，打「2w」这样一瞬间的中间态就会把事件的
    // 结束时间悄悄抹掉（final review I2）。
    let new_from = parse_day_or(&from_buf, event.span.from);
    let new_to = parse_day_open_or(&to_buf, event.span.to);
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
    ui.horizontal(|ui| {
        ui.label("＋");
        ui.add(egui::TextEdit::singleline(&mut drafts.event_participant).desired_width(140.0));
        if ui.button("添加参与者").clicked() && !drafts.event_participant.trim().is_empty() {
            let mut patch = EventFieldPatch::from_event(event);
            patch
                .participants
                .push(Id::new_unchecked(drafts.event_participant.trim()));
            update_event_now(projection, edit, view, patch);
            drafts.event_participant.clear();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_day_open_or_treats_empty_as_open_end() {
        assert_eq!(parse_day_open_or("", Some(200)), None);
        assert_eq!(parse_day_open_or("  ", Some(200)), None);
    }

    #[test]
    fn parse_day_open_or_keeps_fallback_on_parse_failure() {
        // 「2w」是打字过程中的中间态——绝不能把结束时间抹成开放区间。
        assert_eq!(parse_day_open_or("2w", Some(200)), Some(200));
        assert_eq!(parse_day_open_or("abc", None), None);
    }

    #[test]
    fn parse_day_open_or_parses_valid_number() {
        assert_eq!(parse_day_open_or("300", Some(200)), Some(300));
    }

    #[test]
    fn parse_day_or_falls_back_on_garbage() {
        assert_eq!(parse_day_or("12x", 100), 100);
        assert_eq!(parse_day_or("42", 100), 42);
        assert_eq!(parse_day_or("", 100), 100);
    }
}
