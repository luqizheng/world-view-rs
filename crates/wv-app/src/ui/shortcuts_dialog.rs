//! 快捷键速查浮层（UX-015：审计「无快捷键速查表」——`?` 键弹出）。
//!
//! 内容是**六组静态清单**（工程 / 工具 / 视图 / 时间轴 / 笔刷 / 面板），
//! 文案走 i18n 双语表；键位名（Ctrl+S 等）与语言无关，硬编码在行内。
//! 表的真源仍是 [`crate::ui_shell::shell::hotkeys`]——本浮层只是把既有
//! 键位写出来，新增键位时两处要同步（无单一真源的静态帮助文本的既有
//! 妥协，模块顶部注释即同步提醒）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;

/// 速查浮层的跨帧状态（`?` 开关；× / Esc / 「知道了」关）。
#[derive(bevy::prelude::Resource, Debug, Default)]
pub struct ShortcutsDialog {
    pub open: bool,
}

/// 速查浮层（egui pass，居中模态窗）。
pub fn shortcuts_dialog_system(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<ShortcutsDialog>,
    current_loc: Res<CurrentLocale>,
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let mut open = true;
    let mut dismissed = false;
    DialogWindow::new(t(loc, I18nKey::CheatSheetTitle), &mut open)
        .width(520.0)
        .show(ctx, &shell, |ui| {
            let rows = [
                (I18nKey::CheatSheetProject, I18nKey::CheatSheetProjectBody),
                (I18nKey::CheatSheetTools, I18nKey::CheatSheetToolsBody),
                (I18nKey::CheatSheetView, I18nKey::CheatSheetViewBody),
                (I18nKey::CheatSheetTimeline, I18nKey::CheatSheetTimelineBody),
                (I18nKey::CheatSheetBrush, I18nKey::CheatSheetBrushBody),
                (I18nKey::CheatSheetHelp, I18nKey::CheatSheetHelpBody),
            ];
            for (title, body) in rows {
                ui.label(egui::RichText::new(t(loc, title)).strong());
                ui.label(egui::RichText::new(t(loc, body)).small().color(shell.t2()));
                ui.add_space(6.0);
            }
            ui.add_space(2.0);
            if ui.button(t(loc, I18nKey::LoadErrorDismiss)).clicked() {
                dismissed = true;
            }
        });
    if !open {
        // × / Esc 关框 = 知道了。
        dismissed = true;
    }
    if dismissed {
        dialog.open = false;
    }
}
