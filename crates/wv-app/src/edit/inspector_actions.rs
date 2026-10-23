//! 统一 Inspector 的动作层 —— 事件属性提交。
//!
//! ## 职责
//! - [`update_event_now`] —— 把 Inspector「属性」页签里改好的事件字段提交成
//!   [`wv_edit::command::UpdateEventCommand`]。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界。一切修改经 `wv-edit` 的命令栈（AGENTS §3.3）。
//! - **不**渲染（那是 [`crate::ui::inspector`] 的事）。
//!
//! 与 `region_panel_actions` / `faction_panel_actions` 同构——它们是「面板 →
//! 命令」的桥，本模块是「Inspector 表单 → 命令」的桥。

#![cfg(feature = "app")]

use crate::edit::editing::EditHistory;
use crate::state::{ViewState, WorldProjection};

use wv_core::model::event::EventLocation;
use wv_core::{Id, TimeSpan};

/// 事件字段的一次变更。
///
/// 表单每次只改一个字段，但提交时要带上**全部**字段的当前值
/// （`UpdateEventCommand` 是六字段命令）。用结构体把这组值捆在一起，
/// 比 8 个平铺参数清楚。
#[derive(Debug, Clone)]
pub struct EventFieldPatch {
    /// 目标事件 ID。
    pub id: Id,
    /// 新名称。
    pub name: String,
    /// 新时间跨度。
    pub span: TimeSpan,
    /// 新发生地点。
    pub location: Option<EventLocation>,
    /// 新参与者名单。
    pub participants: Vec<Id>,
    /// 新过程描述。
    pub narrative: Option<String>,
    /// 新结果描述。
    pub outcome: Option<String>,
}

impl EventFieldPatch {
    /// 构造一个「以当前事件为底」的补丁。
    ///
    /// 表单先取当前值填进来，再改要改的那一个——故未动的字段保持原样。
    pub fn from_event(event: &wv_core::model::event::Event) -> Self {
        Self {
            id: event.id.clone(),
            name: event.name.clone(),
            span: event.span,
            location: event.location.clone(),
            participants: event.participants.clone(),
            narrative: event.narrative.clone(),
            outcome: event.outcome.clone(),
        }
    }
}

/// 提交事件属性变更。
///
/// 走 `wv-edit` 的 `UpdateEventCommand`（AGENTS §3.3：System 不直接改世界）。
/// 该命令的 revert 是**逐字段**的——只写回它改过的六个字段，
/// `effects` / `narrations` 不在其列，撤销不会误退它们。
pub fn update_event_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    patch: EventFieldPatch,
) {
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::command::UpdateEventCommand::new(
        patch.id,
        patch.name,
        patch.span,
        patch.location,
        patch.participants,
        patch.narrative,
        patch.outcome,
    )));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            let _ = view;
        }
        Err(e) => {
            bevy::log::warn!("事件编辑失败: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_event() -> wv_core::model::event::Event {
        wv_core::model::event::Event {
            id: Id::new_unchecked("e1"),
            name: "旧名".into(),
            span: TimeSpan {
                from: 100,
                to: Some(200),
            },
            location: None,
            participants: vec![Id::new_unchecked("char_a")],
            narrative: Some("旧叙述".into()),
            outcome: None,
            effects: Vec::new(),
            narrations: Vec::new(),
        }
    }

    #[test]
    fn event_patch_from_event_carries_all_six_fields() {
        let e = sample_event();
        let p = EventFieldPatch::from_event(&e);
        assert_eq!(p.id.as_str(), "e1");
        assert_eq!(p.name, "旧名");
        assert_eq!(p.span.from, 100);
        assert_eq!(p.span.to, Some(200));
        assert_eq!(p.participants, vec![Id::new_unchecked("char_a")]);
        assert_eq!(p.narrative.as_deref(), Some("旧叙述"));
        assert_eq!(p.outcome, None);
        assert_eq!(p.location, None);
    }

    #[test]
    fn event_patch_changing_one_field_keeps_others() {
        // 「只改名称」不能顺手把叙述/参与者清空——补丁从当前事件起底，
        // 改一个字段，其余保持原值。
        let e = sample_event();
        let mut p = EventFieldPatch::from_event(&e);
        p.name = "新名".into();
        assert_eq!(p.narrative.as_deref(), Some("旧叙述"));
        assert_eq!(p.participants, vec![Id::new_unchecked("char_a")]);
        assert_eq!(p.span.from, 100);
    }
}
