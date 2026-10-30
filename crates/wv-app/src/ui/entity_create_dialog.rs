//! 「登记人物 / 事态」对话框（UX-002 batch6：事件样板之后补齐 P0 的
//! 另外两个实体——人物与事态此前同样只能手编 YAML 创建）。
//!
//! ## 样板最小面
//!
//! 与 `event_create_dialog` 同族：人物 = 名称；事态 = 名称 + 起/止日
//! （默认当前播放日）。别名 / 生卒 / 等级段（人物）与 steps / units
//! （事态）登记后经既有编辑面补填——创建不重复造轮子。
//!
//! ## 数据流
//!
//! 提交 = 1 条 `AddCharacterCommand` / `AddSituationCommand`（wv-edit）
//! 进 `EditHistory`；可撤销、记脏（`characters` / `timeline/situations`）、
//! 触发诊断。ID 生成 `char_N` / `situation_N` 首空闲。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{Panel, PlaybackState, ProjectDiagnostics, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;
use crate::ui_shell::ShellNotice;
use wv_core::time::TimeSpan;

// ── 人物 ──────────────────────────────────────────────────────────────

/// 人物创建对话框跨帧状态。
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq, Default)]
pub struct CharacterCreateDialog {
    pub open: bool,
    pub name: String,
}

/// 第一个空闲的 `char_N` 形 ID（纯函数供测试）。
pub fn next_free_character_id(existing: &[wv_core::Id]) -> wv_core::Id {
    next_free_id("char", existing)
}

// ── 事态 ──────────────────────────────────────────────────────────────

/// 事态创建对话框跨帧状态（跨度默认当前播放日 ±1）。
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq)]
pub struct SituationCreateDialog {
    pub open: bool,
    pub name: String,
    pub from_day: i64,
    pub to_day: i64,
}

impl Default for SituationCreateDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: String::new(),
            from_day: 1,
            to_day: 2,
        }
    }
}

/// 打开并按当前播放日预填跨度。
pub fn open_situation_with_playback_defaults(
    dialog: &mut SituationCreateDialog,
    playback: &PlaybackState,
) {
    dialog.open = true;
    dialog.from_day = playback.current_day;
    dialog.to_day = playback.current_day + 1;
}

/// 第一个空闲的 `situation_N` 形 ID（纯函数供测试）。
pub fn next_free_situation_id(existing: &[wv_core::Id]) -> wv_core::Id {
    next_free_id("situation", existing)
}

/// `前缀_N` 首空闲（与 event 版同算法，抽公共纯函数）。
fn next_free_id(prefix: &str, existing: &[wv_core::Id]) -> wv_core::Id {
    let mut n = 1usize;
    loop {
        let candidate = wv_core::Id::new_unchecked(format!("{prefix}_{n}"));
        if !existing.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

// ── Bevy 系统 ───────────────────────────────────────────────────────—

/// 人物创建对话框（名称单一字段）+ 提交。
/// 人物登记的五个写侧资源（折成一层元组，避开 clippy 参数上限与
/// type_complexity 两条同时报）。
type CharacterCreateWrites<'w> = (
    ResMut<'w, WorldProjection>,
    ResMut<'w, EditHistory>,
    ResMut<'w, crate::state::PanelState>,
    ResMut<'w, ProjectDiagnostics>,
    ResMut<'w, ShellNotice>,
);

pub fn character_create_dialog_system(
    mut contexts: bevy_egui::EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<CharacterCreateDialog>,
    current_loc: Res<CurrentLocale>,
    (mut projection, mut edit, mut panels, mut diagnostics, mut notice): CharacterCreateWrites,
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
    let mut commit = false;
    let mut cancel = false;
    // 闭包不得再借 `dialog`（`&mut dialog.open` 已被 DialogWindow 持有）——
    // 用局部副本编辑，出闭包写回（event_create_dialog 同款教训）。
    let mut name = dialog.name.clone();
    DialogWindow::new(t(loc, I18nKey::CharacterCreateTitle), &mut dialog.open)
        .width(320.0)
        .show(ctx, &shell, |ui| {
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::FeaturePlaceNameLabel));
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(180.0)
                        .hint_text(t(loc, I18nKey::CharacterCreateNameHint)),
                );
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t(loc, I18nKey::CharacterCreateHint))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                    cancel = true;
                }
                let ready = !name.trim().is_empty();
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(
                            egui::RichText::new(t(loc, I18nKey::CharacterCreateTitle)).strong(),
                        ),
                    )
                    .clicked()
                {
                    commit = true;
                }
            });
        });
    dialog.name = name;
    if cancel {
        dialog.open = false;
        return;
    }
    if commit {
        let taken: Vec<wv_core::Id> = projection
            .world
            .characters
            .iter()
            .map(|c| c.id.clone())
            .collect();
        let character = wv_core::model::character::Character {
            id: next_free_character_id(&taken),
            name: dialog.name.trim().to_owned(),
            alias: None,
            traits: Vec::new(),
            attributes: std::collections::HashMap::new(),
            born_at: None,
            died_at: None,
            state_spans: Vec::new(),
        };
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(wv_edit::AddCharacterCommand::new(
            character.clone(),
        )));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                diagnostics.needs_diagnose = true;
                notice.info(
                    format!(
                        "{}{}",
                        t(loc, I18nKey::NoticeCharacterCreatedPrefix),
                        character.name
                    ),
                    frame,
                );
                panels.set_visible(Panel::Character, true);
                dialog.open = false;
            }
            Err(e) => {
                notice.error(
                    format!("{}: {e}", t(loc, I18nKey::NoticeCharacterCreateFailed)),
                    frame,
                );
            }
        }
    }
}

/// 事态创建对话框（名称 + 起/止日）+ 提交。
#[allow(clippy::too_many_arguments)]
pub fn situation_create_dialog_system(
    mut contexts: bevy_egui::EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<SituationCreateDialog>,
    current_loc: Res<CurrentLocale>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    playback: Res<PlaybackState>,
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
    let mut commit = false;
    let mut cancel = false;
    // 同人物的借用纪律：闭包内只用局部副本。
    let mut name = dialog.name.clone();
    let (mut from_day, mut to_day) = (dialog.from_day, dialog.to_day);
    DialogWindow::new(t(loc, I18nKey::SituationCreateTitle), &mut dialog.open)
        .width(360.0)
        .show(ctx, &shell, |ui| {
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::FeaturePlaceNameLabel));
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(200.0)
                        .hint_text(t(loc, I18nKey::SituationCreateNameHint)),
                );
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::EventCreateFromDay));
                ui.add(egui::DragValue::new(&mut from_day).range(0..=i64::MAX));
                ui.label(t(loc, I18nKey::EventCreateToDay));
                ui.add(egui::DragValue::new(&mut to_day).range(0..=i64::MAX));
            });
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t(loc, I18nKey::SituationCreateHint))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // 闭包里 dialog.open 被 DialogWindow 持可变借用——只记旗标。
                if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                    cancel = true;
                }
                let ready = !name.trim().is_empty() && to_day >= from_day;
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(
                            egui::RichText::new(t(loc, I18nKey::SituationCreateTitle)).strong(),
                        ),
                    )
                    .clicked()
                {
                    commit = true;
                }
            });
        });
    dialog.name = name;
    dialog.from_day = from_day;
    dialog.to_day = to_day;
    if cancel {
        dialog.open = false;
        return;
    }
    if commit {
        let span = TimeSpan::new(from_day, to_day).expect("ready 守卫已保证 from <= to");
        let taken: Vec<wv_core::Id> = projection
            .world
            .situations
            .iter()
            .map(|x| x.id.clone())
            .collect();
        let situation = wv_core::derived::situation::Situation {
            id: next_free_situation_id(&taken),
            name: dialog.name.trim().to_owned(),
            summary: None,
            steps: Vec::new(),
            span,
            units: Vec::new(),
            narrative_order: None,
            maps: Vec::new(),
            protagonist: None,
        };
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(wv_edit::AddSituationCommand::new(
            situation.clone(),
        )));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                diagnostics.needs_diagnose = true;
                notice.info(
                    format!(
                        "{}{}",
                        t(loc, I18nKey::NoticeSituationCreatedPrefix),
                        situation.name
                    ),
                    frame,
                );
                // 事态登记完亮出 Inspector（「事态」页签在那里）。
                panels.set_visible(Panel::Inspector, true);
                dialog.open = false;
            }
            Err(e) => {
                notice.error(
                    format!("{}: {e}", t(loc, I18nKey::NoticeSituationCreateFailed)),
                    frame,
                );
            }
        }
    }
    let _ = playback; // 预填只在打开入口用（open_situation_with_playback_defaults）
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ID 生成：前缀_N 跳过已占用（人物与事态共用同一算法）。
    #[test]
    fn next_free_ids_skip_taken_numbers_per_prefix() {
        let taken_c = vec![wv_core::Id::new_unchecked("char_1")];
        assert_eq!(
            next_free_character_id(&taken_c),
            wv_core::Id::new_unchecked("char_2")
        );
        assert_eq!(
            next_free_character_id(&[]),
            wv_core::Id::new_unchecked("char_1")
        );
        let taken_s = vec![
            wv_core::Id::new_unchecked("situation_1"),
            wv_core::Id::new_unchecked("situation_2"),
        ];
        assert_eq!(
            next_free_situation_id(&taken_s),
            wv_core::Id::new_unchecked("situation_3")
        );
        // 前缀互不干扰：char_1 占用不影响 situation_1。
        assert_eq!(
            next_free_situation_id(&taken_c),
            wv_core::Id::new_unchecked("situation_1")
        );
    }

    /// 事态打开即按当前播放日预填。
    #[test]
    fn situation_open_seeds_span_from_playback_day() {
        let playback = PlaybackState::default();
        let mut d = SituationCreateDialog::default();
        open_situation_with_playback_defaults(&mut d, &playback);
        assert!(d.open);
        assert_eq!(d.from_day, playback.current_day);
        assert_eq!(d.to_day, playback.current_day + 1);
    }
}
