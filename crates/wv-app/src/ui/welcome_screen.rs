//! 欢迎页 / 无工程锁（2026-10-14 用户需求：程序刚打开时不能画图，
//! 必须先新建或打开一个工程）。
//!
//! ## 职责
//! - 没有打开任何工程时，全屏压暗 + 居中卡片：「新建工程…」/「打开工程…」/
//!   最近工程。压暗层**吃掉点击**，地图与浮动面板都不可交互——
//!   「什么都不能做，先立工程」。
//! - 落笔入口（笔刷 / 街道）另有一道同口径的硬门禁
//!   （`collect_brush_stroke` / `commit_street_click`），这里挡 UI、那里挡数据，
//!   双保险防穿透。
//!
//! ## 边界（不做什么）
//! - **不**加载 / 创建——按钮只排队意图：打开 → [`OpenProjectTask`]，
//!   新建 → [`NewProjectDialog`]，最近工程 → `CommandPalette::pending`
//!   （走唯一分发器，与菜单点「打开最近工程」同一条路）。
//! - **不**在加载中画——`StartupLoadRequest` 有值时让位给启动加载遮罩，
//!   避免两层压暗叠一起；命令面板 / 新建工程对话框打开时也让位
//!   （它们与本页同为 Foreground 层，抢顶层）。
//!
//! ## 主要导出
//! - `welcome_screen` —— Bevy 系统（注册在 `ui_shell::register`）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{RecentProjects, WorldProjection};
use crate::ui::NewProjectDialog;
use crate::ui_command_palette::{CommandId, CommandPalette};
use crate::ui_shell::{DialogPurpose, OpenProjectTask, StartupLoadRequest};

use crate::i18n::CurrentLocale;

/// 欢迎页上最多列几条最近工程（列表本身存 8 条，首屏别太长）。
const WELCOME_RECENTS_MAX: usize = 5;

/// 无工程时的欢迎页：全屏压暗 + 居中的「新建 / 打开 / 最近」入口。
#[allow(clippy::too_many_arguments)]
pub fn welcome_screen(
    mut ctxs: EguiContexts,
    projection: Res<WorldProjection>,
    recents: Res<RecentProjects>,
    mut open_task: ResMut<OpenProjectTask>,
    mut palette: ResMut<CommandPalette>,
    mut new_project: ResMut<NewProjectDialog>,
    startup_req: Res<StartupLoadRequest>,
    current_loc: Res<CurrentLocale>,
) {
    // 有工程在手（或正在做启动加载）：不画。加载失败时工程仍未打开，
    // 本页照常出现，失败原因卡片（同为 Foreground、注册在后）压在其上。
    if projection.has_open_project() || startup_req.0.is_some() {
        return;
    }
    // 命令面板 / 新建工程对话框是更聚焦的临时浮层，让位。
    if palette.open || new_project.open {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let screen = ctx.viewport_rect();

    // 全屏压暗 + 吃点击：`interactable(true)` 让 Area 接住指针，
    // 下面的地图与面板（Middle 层）点不到。
    egui::Area::new(egui::Id::new("wv_welcome_dim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(180));
        });

    egui::Window::new(crate::i18n::t_key(
        &current_loc,
        crate::i18n::I18nKey::WelcomeHeading,
    ))
    .order(egui::Order::Foreground)
    .collapsible(false)
    .resizable(false)
    .default_width(440.0)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ctx, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
            ui.heading(crate::i18n::t_key(
                &current_loc,
                crate::i18n::I18nKey::WelcomeHeading,
            ));
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(crate::i18n::t_key(
                    &current_loc,
                    crate::i18n::I18nKey::WelcomeTagline,
                ))
                .text_style(egui::TextStyle::Small)
                .color(ui.style().visuals.weak_text_color()),
            );
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new(crate::i18n::t_key(
                    &current_loc,
                    crate::i18n::I18nKey::WelcomeHelpText,
                ))
                .color(ui.style().visuals.warn_fg_color),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .button(crate::i18n::t_key(
                        &current_loc,
                        crate::i18n::I18nKey::WelcomeNewProjectBtn,
                    ))
                    .clicked()
                {
                    new_project.open();
                }
                if ui
                    .button(crate::i18n::t_key(
                        &current_loc,
                        crate::i18n::I18nKey::WelcomeOpenProjectBtn,
                    ))
                    .clicked()
                {
                    // 只排队意图；原生目录窗口由 poll_open_project_dialog 发起。
                    open_task.request(DialogPurpose::Open);
                }
            });
            let entries = recents.entries();
            if !entries.is_empty() {
                ui.add_space(10.0);
                ui.separator();
                ui.label(
                    egui::RichText::new(crate::i18n::t_key(
                        &current_loc,
                        crate::i18n::I18nKey::WelcomeRecentHeading,
                    ))
                    .text_style(egui::TextStyle::Small),
                );
                for (i, e) in entries.iter().take(WELCOME_RECENTS_MAX).enumerate() {
                    // 走唯一分发器（与菜单同一条路），本页不做加载。
                    // 「—」分隔符与项目名是用户输入（跨语种天然混合），
                    // 不引入 i18n Key。
                    if ui
                        .button(egui::RichText::new(format!("{} — {}", e.name, e.dir)).small())
                        .clicked()
                    {
                        palette.pending = Some(CommandId::OpenRecentProject(i as u8));
                    }
                }
            }
        });
    });
}
