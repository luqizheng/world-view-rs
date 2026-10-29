//! 「登记事件」对话框（UX-002 / round-2 审计 P0：事件此前 GUI 无法创建，
//! 时间轴「+ 登记事件」只开一个只读列表面板——登记的写侧从未建成）。
//!
//! ## 样板最小面
//!
//! 名称 + 时间跨度（起/止 day，默认当前播放日）。地点 / 参与者 / 叙述 /
//! 结果**不在本对话框**——登记后由统一 Inspector 的「属性」页签补填
//! （`UpdateEventCommand` 编辑面已存在，不为创建重复造轮子）。
//!
//! ## 数据流
//!
//! 提交 = 1 条 `AddEventCommand`（wv-edit）进 `EditHistory`，脏路径
//! `timeline/events`；登记是普通编辑事务——可撤销、记脏、触发诊断，
//! 与面板编辑同一套纪律（§3.3）。ID 由系统生成（`event_N` 首个空闲）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::i18n::{t, CurrentLocale, I18nKey, Locale};
use crate::state::{Panel, PlaybackState, ProjectDiagnostics, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;
use crate::ui_shell::ShellNotice;
use wv_core::time::TimeSpan;

/// 对话框跨帧状态。
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq)]
pub struct EventCreateDialog {
    /// 是否打开。
    pub open: bool,
    /// 事件名称（提交时 trim，空 = 登记钮禁用）。
    pub name: String,
    /// 起始日（含）。
    pub from_day: i64,
    /// 结束日（**不含**，与 [`TimeSpan`] 半开语义一致）。
    pub to_day: i64,
}

impl Default for EventCreateDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: String::new(),
            from_day: 1,
            to_day: 2,
        }
    }
}

/// 打开对话框并按当前播放日预填跨度（时间轴按钮与命令面板共用的入口）。
pub fn open_with_playback_defaults(dialog: &mut EventCreateDialog, playback: &PlaybackState) {
    dialog.open = true;
    dialog.from_day = playback.current_day;
    dialog.to_day = playback.current_day + 1;
}

/// 第一个空闲的 `event_N` 形 ID（纯函数供测试）：从 1 起跳过已占用的。
pub fn next_free_event_id(existing: &[wv_core::Id]) -> wv_core::Id {
    let mut n = 1usize;
    loop {
        let candidate = wv_core::Id::new_unchecked(format!("event_{n}"));
        if !existing.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 绘制对话框；确认时把 `(名称, 跨度)` 交给 `on_commit`（ui / apply 拆分，
/// `continent_dialog` 同款模式）。回调返回 `false`（提交失败）时框不关。
pub fn event_create_dialog(
    ctx: &egui::Context,
    shell: &UiShell,
    dialog: &mut EventCreateDialog,
    loc: Locale,
    on_commit: impl FnOnce(String, TimeSpan) -> bool,
) -> bool {
    if !dialog.open {
        return false;
    }
    let mut commit = false;
    let mut cancel = false;
    let title = t(loc, I18nKey::EventCreateTitle);
    DialogWindow::new(title, &mut dialog.open)
        .width(360.0)
        .show(ctx, shell, |ui| {
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::FeaturePlaceNameLabel));
                ui.add(
                    egui::TextEdit::singleline(&mut dialog.name)
                        .desired_width(200.0)
                        .hint_text(t(loc, I18nKey::EventCreateNameHint)),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::EventCreateFromDay));
                ui.add(egui::DragValue::new(&mut dialog.from_day).range(0..=i64::MAX));
                ui.label(t(loc, I18nKey::EventCreateToDay));
                ui.add(egui::DragValue::new(&mut dialog.to_day).range(0..=i64::MAX));
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t(loc, I18nKey::EventCreateHint))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                    // 窗口持有 `&mut dialog.open`——闭包里只记旗标，出闭包再落。
                    cancel = true;
                }
                // 名称必填 + 跨度不得倒置（`TimeSpan::new` 的唯一错误面）。
                let ready = !dialog.name.trim().is_empty() && dialog.to_day >= dialog.from_day;
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(
                            egui::RichText::new(t(loc, I18nKey::TimelineRegisterEventBtn)).strong(),
                        ),
                    )
                    .clicked()
                {
                    commit = true;
                }
            });
        });

    if cancel {
        dialog.open = false;
        return false;
    }
    if commit {
        let span =
            TimeSpan::new(dialog.from_day, dialog.to_day).expect("ready 守卫已保证 from <= to");
        let name = dialog.name.trim().to_owned();
        // 提交失败不关框，作者改了重试。
        dialog.open = !on_commit(name, span);
    }
    dialog.open
}

/// Bevy 系统：绘制对话框 + 提交 `AddEventCommand`（参数宽签名是框架惯例）。
#[allow(clippy::too_many_arguments)]
pub fn event_create_dialog_system(
    mut contexts: bevy_egui::EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<EventCreateDialog>,
    current_loc: Res<CurrentLocale>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut panels: ResMut<crate::state::PanelState>,
    mut diagnostics: ResMut<ProjectDiagnostics>,
    mut notice: ResMut<ShellNotice>,
    time: Res<Time>,
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let frame = (time.elapsed_secs() * 60.0) as u64;

    event_create_dialog(ctx, &shell, &mut dialog, loc, |name, span| {
        let taken: Vec<wv_core::Id> = projection
            .world
            .events
            .iter()
            .map(|e| e.id.clone())
            .collect();
        let event = wv_core::model::event::Event {
            id: next_free_event_id(&taken),
            name,
            span,
            location: None,
            participants: Vec::new(),
            narrative: None,
            outcome: None,
            effects: Vec::new(),
            narrations: Vec::new(),
        };
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(wv_edit::AddEventCommand::new(event.clone())));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                diagnostics.needs_diagnose = true;
                notice.info(
                    format!(
                        "{}{}",
                        t(loc, I18nKey::NoticeEventCreatedPrefix),
                        event.name
                    ),
                    frame,
                );
                // 登记完顺带亮出事件列表（旧按钮行为保留，作者立刻看到结果）。
                panels.set_visible(Panel::Event, true);
                true
            }
            Err(e) => {
                notice.error(
                    format!("{}: {e}", t(loc, I18nKey::NoticeEventCreateFailed)),
                    frame,
                );
                false
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;

    /// ID 生成：从 event_1 起跳过已占用。
    #[test]
    fn next_free_event_id_skips_taken_numbers() {
        let taken = vec![
            wv_core::Id::new_unchecked("event_1"),
            wv_core::Id::new_unchecked("event_2"),
        ];
        assert_eq!(
            next_free_event_id(&taken),
            wv_core::Id::new_unchecked("event_3")
        );
        assert_eq!(
            next_free_event_id(&[]),
            wv_core::Id::new_unchecked("event_1")
        );
    }

    /// 打开即按当前播放日预填跨度。
    #[test]
    fn open_with_playback_defaults_seeds_span_from_current_day() {
        let playback = PlaybackState::default();
        let mut d = EventCreateDialog::default();
        open_with_playback_defaults(&mut d, &playback);
        assert!(d.open);
        assert_eq!(d.from_day, playback.current_day);
        assert_eq!(d.to_day, playback.current_day + 1);
    }

    /// 文案键齐备（双语非空）：对话框文本全走 i18n。
    #[test]
    fn event_create_keys_resolve_in_both_locales() {
        for key in [
            I18nKey::EventCreateTitle,
            I18nKey::EventCreateNameHint,
            I18nKey::EventCreateFromDay,
            I18nKey::EventCreateToDay,
            I18nKey::EventCreateHint,
            I18nKey::NoticeEventCreatedPrefix,
            I18nKey::NoticeEventCreateFailed,
        ] {
            for loc in [Locale::Zh, Locale::En] {
                assert!(!t(loc, key).is_empty(), "{key:?} {loc:?} 缺文案");
            }
        }
    }
}
