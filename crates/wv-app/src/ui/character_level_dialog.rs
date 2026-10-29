//! 「人物等级」列表编辑器对话框（2026-10-30）。
//!
//! ## 覆盖面
//!
//! 人物此前在 GUI 里零写侧（UX 审计 2026-10-29：数据侧缺「写」）。
//! 本对话框先接**等级列表**这一刀：`Character.state_spans` 的增 / 删 / 改行
//! （起止日 + 等级值）。人物本体的创建 / 改名（UX-002 剩余实体）不在此框。
//!
//! ## 数据流
//!
//! 打开时把人物当前 `state_spans` 克隆成本地草稿，行编辑只动草稿；
//! 「确定」= 1 条 [`wv_edit::SetCharacterStateSpansCommand`]（整表替换）
//! 进 `EditHistory`，脏路径 `characters`——一次会话一个撤销步。
//! 「取消」/ 标题栏 ×（`DialogWindow` 原生关闭钮）丢弃草稿。
//! 未暴露的字段（place / title）随草稿原样往返，不会误清。
//!
//! **只列带 `power` 的段**：`state_spans` 里还可能有纯头衔 / 地点变迁段
//! （`power: None`）——它们不是「等级」，在此展示会诱导从等级视角改写
//! 其时间语义。这些段在草稿里原样携带，整表提交时不受影响。
//!
//! ## V1 / V2
//!
//! 不做区间重叠 / 顺序校验——作者可改「已发生的历史」（V2），
//! 冲突由诊断面板提示（V1）。唯一硬闸：结束日不得早于起始日
//! （`TimeSpan` 的构造错误面），违反时禁用确定钮。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::i18n::{t, CurrentLocale, I18nKey, Locale};
use crate::state::{ProjectDiagnostics, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;
use crate::ui_shell::ShellNotice;
use wv_core::character::CharacterStateSpan;

/// 对话框跨帧状态。草稿以 `CharacterStateSpan` 原样持有——
/// 未在 UI 暴露的字段天然保真，整表快照命令的前提。
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq)]
pub struct CharacterLevelEditor {
    /// 是否打开。
    pub open: bool,
    /// 正在编辑的人物。
    pub character: Option<wv_core::Id>,
    /// 标题用名称快照（打开时取；编辑期间人物改名不追踪，可接受近似）。
    pub name: String,
    /// 草稿表。
    pub draft: Vec<CharacterStateSpan>,
    /// 新增行的起始日种子（打开时取当前播放日）。
    pub seed_day: i64,
}

impl Default for CharacterLevelEditor {
    fn default() -> Self {
        Self {
            open: false,
            character: None,
            name: String::new(),
            draft: Vec::new(),
            seed_day: 1,
        }
    }
}

impl CharacterLevelEditor {
    /// 打开某人物的等级编辑器（人物面板行点击入口）。
    pub fn open_for(
        &mut self,
        id: wv_core::Id,
        name: String,
        spans: Vec<CharacterStateSpan>,
        playback_day: i64,
    ) {
        self.open = true;
        self.character = Some(id);
        self.name = name;
        self.draft = spans;
        self.seed_day = playback_day;
    }
}

/// 草稿是否可提交：任一行区间倒置（to < from）即不可。
/// （这是 `TimeSpan` 的构造错误面，不是创作内容审查。）
fn draft_submittable(draft: &[CharacterStateSpan]) -> bool {
    draft.iter().all(|s| match s.span.to {
        None => true,
        Some(to) => to >= s.span.from,
    })
}

/// 绘制对话框；确定时把 `(人物 ID, 草稿)` 交给 `on_commit`（ui / apply 拆分，
/// `event_create_dialog` 同款模式）。回调返回 `false`（提交失败）时框不关。
pub fn character_level_dialog(
    ctx: &egui::Context,
    shell: &UiShell,
    dialog: &mut CharacterLevelEditor,
    loc: Locale,
    on_commit: impl FnOnce(wv_core::Id, Vec<CharacterStateSpan>) -> bool,
) -> bool {
    if !dialog.open {
        return false;
    }
    let mut commit = false;
    let mut cancel = false;
    let title = format!("{} — {}", t(loc, I18nKey::CharacterLevelTitle), dialog.name);
    DialogWindow::new(&title, &mut dialog.open)
        .width(420.0)
        .show(ctx, shell, |ui| {
            // 行区可滚：等级段数量不设上限。
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .show(ui, |ui| {
                    let mut remove: Option<usize> = None;
                    for (i, s) in dialog.draft.iter_mut().enumerate() {
                        // 只显示带等级的段：不带 power 的状态段（纯头衔/地点变迁）
                        // 不属于「等级列表」，在此展示反而会诱导改写它的时间语义；
                        // 草稿整表持有，它们原样往返不受影响。
                        if s.power.is_none() {
                            continue;
                        }
                        ui.push_id(i, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(t(loc, I18nKey::CharacterLevelFrom));
                                ui.add(
                                    egui::DragValue::new(&mut s.span.from)
                                        .range(i64::MIN..=i64::MAX),
                                );
                                let mut open = s.span.to.is_none();
                                if ui
                                    .checkbox(&mut open, t(loc, I18nKey::CharacterLevelOngoing))
                                    .changed()
                                {
                                    // 勾「至今」清 to；取消勾选以起始日兜底（可再改）。
                                    s.span.to = if open { None } else { Some(s.span.from) };
                                }
                                if s.span.to.is_some() {
                                    ui.label(t(loc, I18nKey::CharacterLevelTo));
                                    let mut to_val = s.span.to.unwrap_or(s.span.from);
                                    ui.add(
                                        egui::DragValue::new(&mut to_val)
                                            .range(i64::MIN..=i64::MAX),
                                    );
                                    s.span.to = Some(to_val);
                                }
                                ui.separator();
                                ui.label(t(loc, I18nKey::CharacterLevelPower));
                                let mut power = s.power.unwrap_or(0.0);
                                ui.add(
                                    egui::DragValue::new(&mut power)
                                        .speed(1.0)
                                        .range(0.0..=f32::MAX),
                                );
                                s.power = Some(power);
                                if ui.small_button("✕").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    }
                    if let Some(i) = remove {
                        dialog.draft.remove(i);
                    }
                });
            if ui.button(t(loc, I18nKey::CharacterLevelAddRow)).clicked() {
                dialog.draft.push(CharacterStateSpan {
                    span: wv_core::time::TimeSpan {
                        from: dialog.seed_day,
                        to: None,
                    },
                    place: None,
                    title: None,
                    power: Some(1.0),
                });
            }
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t(loc, I18nKey::CharacterLevelHint))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                    // 窗口持有 `&mut dialog.open`——闭包里只记旗标，出闭包再落。
                    cancel = true;
                }
                let ready = draft_submittable(&dialog.draft);
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(egui::RichText::new(t(loc, I18nKey::CommonOk)).strong()),
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
        let id = dialog.character.clone();
        let draft = dialog.draft.clone();
        if let Some(id) = id {
            // 提交失败不关框，作者改了重试。
            dialog.open = !on_commit(id, draft);
        }
    }
    dialog.open
}

/// Bevy 系统：绘制对话框 + 提交 `SetCharacterStateSpansCommand`（宽签名是框架惯例）。
#[allow(clippy::too_many_arguments)]
pub fn character_level_dialog_system(
    mut contexts: bevy_egui::EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<CharacterLevelEditor>,
    current_loc: Res<CurrentLocale>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
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

    character_level_dialog(ctx, &shell, &mut dialog, loc, |id, draft| {
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(wv_edit::SetCharacterStateSpansCommand::new(
            id.clone(),
            draft,
        )));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                diagnostics.needs_diagnose = true;
                let name = projection
                    .world
                    .characters
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                notice.info(
                    format!(
                        "{}{}",
                        t(loc, I18nKey::NoticeCharacterLevelSavedPrefix),
                        name
                    ),
                    frame,
                );
                true
            }
            Err(e) => {
                notice.error(
                    format!("{}: {e}", t(loc, I18nKey::NoticeCharacterLevelSaveFailed)),
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

    fn span(from: i64, to: Option<i64>, power: f32) -> CharacterStateSpan {
        CharacterStateSpan {
            span: wv_core::time::TimeSpan { from, to },
            place: None,
            title: None,
            power: Some(power),
        }
    }

    /// 打开即克隆整表草稿 + 记录播放日种子。
    #[test]
    fn open_for_clones_spans_and_seeds_playback_day() {
        let mut d = CharacterLevelEditor::default();
        let spans = vec![span(100, Some(200), 10.0)];
        d.open_for(
            wv_core::Id::new_unchecked("char_a"),
            "沈砚".into(),
            spans,
            42,
        );
        assert!(d.open);
        assert_eq!(d.name, "沈砚");
        assert_eq!(d.draft.len(), 1);
        assert_eq!(d.draft[0].power, Some(10.0));
        assert_eq!(d.seed_day, 42);
    }

    /// 区间倒置禁提交；开放区间（至今）恒可提交。
    #[test]
    fn draft_with_inverted_span_is_not_submittable() {
        let ok = vec![span(100, Some(200), 1.0), span(200, None, 2.0)];
        assert!(draft_submittable(&ok), "正序 + 至今应可提交");

        let inverted = vec![span(300, Some(200), 1.0)];
        assert!(!draft_submittable(&inverted), "to < from 应禁提交");

        let zero_len = vec![span(200, Some(200), 1.0)];
        assert!(
            draft_submittable(&zero_len),
            "零长度区间合法（半开语义），应可提交"
        );

        assert!(draft_submittable(&[]), "清空整表也是合法编辑");
    }
}
