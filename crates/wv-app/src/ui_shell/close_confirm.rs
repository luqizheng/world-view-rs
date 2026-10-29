//! 关闭确认（UX-001 最小版 / round-2 审计 P0）。
//!
//! 此前 dirty 状态下关闭工程 / 关窗**零确认**直接丢弃——数小时手绘因误点
//! 一个像素宽的 × 蒸发。本模块给三条出口（Ctrl+W / 工程菜单「关闭工程」、
//! 顶栏窗控 ×）装同一道闸：dirty 时弹「保存并关闭 / 不保存 / 取消」。
//!
//! ## 状态机
//!
//! - `pending`：有值 = 确认框在屏上，等待作者三选一；取消/× = 清空。
//! - 保存并关闭 → 排队 `Save` 并记 `action_after_save`：保存成功（dirty
//!   落假）由本系统的后续帧收尾执行动作；保存失败则超时放弃（保存路径
//!   自己已弹错误回执，作者可重试）。
//! - 不保存 → `CloseProject` 走 `force_close` 单次放行（守卫在 dispatch）；
//!   `ExitApp` 直接写 `AppExit`。
//!
//! 欢迎页的 × 不经此闸：无工程态 `dirty` 恒假，没有可丢的东西。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;
use crate::ui_command_palette::CommandId;
use crate::ui_shell::CommandPalette;

/// 待确认的关闭动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// 关闭工程（回欢迎页）。
    CloseProject,
    /// 退出应用（顶栏窗控 ×）。
    ExitApp,
}

/// 关闭确认状态机（ui_shell 资源）。
#[derive(bevy::prelude::Resource, Debug, Default)]
pub struct CloseConfirm {
    /// 有值 = 确认框在屏上。
    pub pending: Option<CloseAction>,
    /// 「保存并关闭」已排队 `Save`：保存成功后收尾执行该动作。
    pub action_after_save: Option<CloseAction>,
    /// 「不保存」的单次放行：下一个 `CloseProject` 绕过 dirty 守卫一次。
    pub force_close: bool,
    /// `action_after_save` 的排队帧号：超时而 dirty 仍在 = 保存失败，放弃。
    pub save_queued_at: Option<u64>,
}

/// 给出窗口帧号，判定「保存并关闭」是否已失败应放弃。
///
/// 保存是 dispatch 内的**同步**操作：排队后一帧内必见分晓。超时仍 dirty
/// = 保存失败（错误回执由保存路径弹过），继续挂着会让之后某次不相干的
/// 成功保存意外触发关闭——放弃并交还作者。
fn save_then_close_timed_out(now: u64, queued_at: u64) -> bool {
    now.saturating_sub(queued_at) > 10
}

/// 确认框系统（egui pass，压在状态栏之后 = 所有面板之上）。
#[allow(clippy::too_many_arguments)]
pub fn close_confirm_dialog(
    mut contexts: bevy_egui::EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut confirm: ResMut<CloseConfirm>,
    projection: Res<crate::state::WorldProjection>,
    mut palette: ResMut<CommandPalette>,
    mut exit: MessageWriter<AppExit>,
    current_loc: Res<CurrentLocale>,
    time: Res<Time>,
) {
    let frame = (time.elapsed_secs() * 60.0) as u64;
    // ── 「保存并关闭」收尾 / 超时放弃 ──
    if let Some(action) = confirm.action_after_save {
        if !projection.dirty {
            confirm.action_after_save = None;
            confirm.save_queued_at = None;
            match action {
                // dirty 已落假：守卫自然放行。
                CloseAction::CloseProject => palette.pending = Some(CommandId::CloseProject),
                CloseAction::ExitApp => {
                    exit.write(AppExit::Success);
                }
            }
        } else if let Some(queued_at) = confirm.save_queued_at {
            if save_then_close_timed_out(frame, queued_at) {
                confirm.action_after_save = None;
                confirm.save_queued_at = None;
            }
        }
    }

    let Some(action) = confirm.pending else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let mut open = true;
    DialogWindow::new(t(loc, I18nKey::CloseConfirmTitle), &mut open)
        .width(340.0)
        .show(ctx, &shell, |ui| {
            ui.label(t(loc, I18nKey::CloseConfirmMessage));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // 无工程目录时「保存并关闭」不可选（保存会拐进另存为对话框，
                // 与「关闭」的意图错位）——只给 不保存 / 取消。
                if projection.project_dir.is_some()
                    && ui
                        .button(
                            egui::RichText::new(t(loc, I18nKey::CloseConfirmSaveAndClose)).strong(),
                        )
                        .clicked()
                {
                    palette.pending = Some(CommandId::Save);
                    confirm.action_after_save = Some(action);
                    confirm.save_queued_at = Some(frame);
                    confirm.pending = None;
                }
                if ui.button(t(loc, I18nKey::CloseConfirmDiscard)).clicked() {
                    match action {
                        CloseAction::CloseProject => {
                            confirm.force_close = true;
                            palette.pending = Some(CommandId::CloseProject);
                        }
                        CloseAction::ExitApp => {
                            exit.write(AppExit::Success);
                        }
                    }
                    confirm.pending = None;
                }
                if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                    confirm.pending = None;
                }
            });
        });
    if !open {
        // × / Esc 关框 = 取消。
        confirm.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 超时判定：10 帧内不算超时，11 帧起算。
    #[test]
    fn save_then_close_times_out_after_ten_frames() {
        assert!(!save_then_close_timed_out(10, 0));
        assert!(!save_then_close_timed_out(10 + 10, 10));
        assert!(save_then_close_timed_out(10 + 11, 10));
    }

    /// 默认状态机：全空。
    #[test]
    fn close_confirm_default_is_idle() {
        let c = CloseConfirm::default();
        assert!(c.pending.is_none());
        assert!(c.action_after_save.is_none());
        assert!(!c.force_close);
    }
}
