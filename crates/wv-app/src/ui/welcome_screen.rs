//! 欢迎页 / 无工程锁（2026-10-15 用户需求：把无工程欢迎页对齐
//! Figma 设计稿 [`Zw4tonExV5lIY_EJamjPRw`](https://pixso.cn/app/design/Zw4tonExV5lIY_EJamjPRw)
//! 的 `00_WelcomeScreen` 帧）。
//!
//! ## 设计要点
//! - 暗色壳背景 + 居中卡片（520px 宽，14 圆角，32 内边距）。
//! - 卡内三段竖向 auto-layout（间距 24px）：
//!   **Header**（地球图标 + 主副标题）/ **ActionGroup**（主+次 CTA）/
//!   **RecentProjects**（小标题 + 列表 + 时间水印待补）/ **Footer**（版本水印）。
//! - **无标题栏 / 无关闭按钮**：Figma 没有 × 与拖动条；用户只能经由
//!   「新建工程…」「打开工程…」或点击最近工程离开本页。
//! - 颜色全部走 [`crate::theme::ui_tokens::ShellPalette`] 令牌，未写裸 hex
//!   （除了一次性白色 `Color32::WHITE` 用在主 CTA 的图标+文字——
//!   Figma `#FFFFFF` 与暗色 `text_on_accent` `#0B0D11` 在「主 CTA」上对比度
//!   都达标（≥4.5），但 `text_on_accent` 是历史暗色基底，本页统一用白以匹配原型）。
//!
//! ## 本地化（ADR-0018 收尾）
//! - 5 处文案经 `Res<CurrentLocale>` 注入 `t_key(...)` 走 `I18nKey` 表：
//!   `WelcomeTagline` / `WelcomeNewProjectBtn` / `WelcomeOpenProjectBtn` /
//!   `WelcomeRecentHeading` / `WelcomeRecentEmpty`。
//! - 品牌名 `World-View` 与底部 `World-View Alpha` 后缀两语统一不本地化
//!   （产品定位决定——见 `README.md` §1）。
//! - `widgets::format_time_ago` 暂未本地化，待 widgets 模块后续 PR 单独处理。
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

use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{RecentProjects, WorldProjection};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::{font_size, space};
use std::time::Instant;

use crate::ui::widgets::button as widget_button;
use crate::ui::widgets::{
    format_time_ago, icon_image, recent_row, ButtonContent, ButtonStyle, ICON_FOLDER_OPEN,
    ICON_GLOBE, ICON_MAP, ICON_PLUS,
};
use crate::ui::NewProjectDialog;
use crate::ui_command_palette::{CommandId, CommandPalette};
use crate::ui_shell::{DialogPurpose, OpenProjectTask, StartupLoadRequest};

use super::shell::UiShell;

/// 欢迎页上最多列几条最近工程（列表本身存 8 条，首屏别太长）。
const WELCOME_RECENTS_MAX: usize = 5;
/// 卡片宽度（Figma 规定值，`00_WelcomeCard.width = 520`）。
const CARD_WIDTH: f32 = 520.0;
/// 卡片内边距（Figma `padding = 32`，单值对称）。
const CARD_PADDING: f32 = 32.0;
/// 主/次 CTA 高度（Figma `height = 40`）。按钮工厂的 `min_size_override: Some(...)`
/// 走它。
const CTA_HEIGHT: f32 = 40.0;
/// AppIcon 圆角方块尺寸 / 圆角（Figma `AppIcon 56×56, cornerRadius = 12`）。
const APP_ICON_BOX: f32 = 56.0;
const APP_ICON_BOX_RADIUS: u8 = 12;
/// AppIcon 内地球图标尺寸（Figma `globe 28×28`）。
const APP_ICON_GLOBE: f32 = 28.0;

/// 主标题（与 Figma `AppTitle = "World-View"` 一致；品牌名两语统一，不本地化）。
const APP_TITLE: &str = "World-View";
/// 底部版本水印。
///
/// 版本号在**编译期**读 `CARGO_PKG_VERSION`（wv-app 的 `Cargo.toml` 用
/// `version.workspace = true` 指向根 `[workspace.package] version`）：
/// workspace 升 `0.2.0` 时 UI 文字自动同步，不需要手工维护。
const FOOTER_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"), " — World-View Alpha");

/// 无工程时的欢迎页（Figma `Zw4tonExV5lIY_EJamjPRw` · `00_WelcomeScreen`）。
///
/// 渲染条件：没有打开工程、且不在启动加载中、且命令面板 / 新建工程对话框
/// 都未打开——这三层"让位"避免与启动加载遮罩或更聚焦的浮层叠绘。
#[allow(clippy::too_many_arguments)]
pub fn welcome_screen(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    recents: Res<RecentProjects>,
    current_loc: Res<CurrentLocale>,
    mut open_task: ResMut<OpenProjectTask>,
    mut palette: ResMut<CommandPalette>,
    mut new_project: ResMut<NewProjectDialog>,
    startup_req: Res<StartupLoadRequest>,
) {
    if projection.has_open_project() || startup_req.0.is_some() {
        return;
    }
    if palette.open || new_project.open {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let screen = ctx.viewport_rect();

    // 全屏点击吞吃层 + `bg_app` 底色（Figma `00_WelcomeScreen` 的整体底色）：
    // 视觉上与空场景一致，功能上仍挡住下面所有 UI（地图 + 浮窗都点不到——
    // 「什么都不能做，先立工程」）。
    egui::Area::new(egui::Id::new("wv_welcome_dim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, shell.palette.bg_app.to_color32());
        });

    // 居中卡片——`title_bar(false)` 拿掉 egui 默认标题栏与 × 按钮；自定义
    // `Frame` 复写 fill / stroke / 内边距 / 圆角以贴近 Figma。
    egui::Window::new("wv_welcome_card")
        .order(egui::Order::Foreground)
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(CARD_WIDTH)
        .frame(
            // 跟随应用当前日夜模式（亮色下阴影/圆角取亮色样式）。
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .fill(shell.panel_bg())
                .stroke(egui::Stroke::new(1.0_f32, shell.border_subtle()))
                .inner_margin(egui::Margin::same(CARD_PADDING as i8))
                .corner_radius(egui::CornerRadius::same(14)),
        )
        .show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::XXL;

                // ── Header ─────────────────────────────────────
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = space::SM;
                    app_icon(ui, &shell);
                    ui.label(
                        egui::RichText::new(APP_TITLE)
                            .font(proportional(24.0))
                            .strong()
                            .color(shell.t1()),
                    );
                    ui.label(
                        egui::RichText::new(t_key(&current_loc, I18nKey::WelcomeTagline))
                            .font(proportional(font_size::BASE))
                            .color(shell.t3()),
                    );
                });

                // ── Divider ─────────────────────────────────────
                divider(ui, &shell);

                // ── ActionGroup（主/次 CTA，gap = 8） ───────────
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::SM;
                    if cta_primary(
                        ui,
                        &shell,
                        t_key(&current_loc, I18nKey::WelcomeNewProjectBtn),
                    ) {
                        new_project.open();
                    }
                    if cta_secondary(
                        ui,
                        &shell,
                        t_key(&current_loc, I18nKey::WelcomeOpenProjectBtn),
                    ) {
                        open_task.request(DialogPurpose::Open);
                    }
                });

                // ── Divider ─────────────────────────────────────
                divider(ui, &shell);

                // ── RecentProjects ──────────────────────────────
                recents_section(ui, &shell, &recents, &mut palette, &current_loc);

                // ── Footer version ──────────────────────────────
                ui.add_space(space::SM);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(FOOTER_VERSION)
                            .font(proportional(font_size::MICRO))
                            .color(shell.t3()),
                    );
                });
            });
        });
}

/// 56×56 accent 圆角方块 + 28×28 地球图标（Figma `AppIcon`）。
///
/// 装饰用：不经按钮工厂（[`crate::ui::widgets::button`]）而直接构造
/// [`egui::Button::image`]，因为这里需要**自定义 `corner_radius`=12**（Figma）
/// 且**高度 = 56**（与默认 `shell.hit=28` 不同），又不响应点击。
/// 图标仍然走 [`icon_image`] helper（强制 tint + 显式尺寸），与按钮工厂同一约定。
fn app_icon(ui: &mut egui::Ui, shell: &UiShell) {
    let image = icon_image(ICON_GLOBE, egui::Color32::WHITE, APP_ICON_GLOBE);
    let _ = ui.add(
        egui::Button::image(image)
            .fill(shell.accent())
            .min_size(egui::vec2(APP_ICON_BOX, APP_ICON_BOX))
            .corner_radius(egui::CornerRadius::same(APP_ICON_BOX_RADIUS)),
    );
}

/// 1px 分隔线（Figma `Divider`/`Divider2`）：用 `painter.hline` 在可用区
/// 顶沿画一条 `border_subtle`，再 `add_space(1)` 占位。
fn divider(ui: &mut egui::Ui, shell: &UiShell) {
    let painter = ui.painter();
    let rect = ui.available_rect_before_wrap();
    painter.hline(
        rect.x_range(),
        rect.min.y,
        egui::Stroke::new(1.0_f32, shell.border_subtle()),
    );
    ui.add_space(1.0);
}

/// 主 CTA：accent 实心底，白字 + 加号图标。
///
/// 经 [`widget_button`] 工厂统一装配（fill/stroke/corner_radius/font 全部从
/// `UiShell` 令牌解析），高度用 [`CTA_HEIGHT`]=40 覆盖默认 `shell.hit=28`
/// —— `widget_button` 的 `min_size_override: Some(...)` 机制负责这件事。
/// 返回 `true` 当用户点击——caller 据此排队意图。
fn cta_primary(ui: &mut egui::Ui, shell: &UiShell, label: &str) -> bool {
    widget_button(
        ui,
        shell,
        ButtonContent::ImageText(ICON_PLUS, label),
        ButtonStyle::Accent,
        Some(egui::vec2(ui.available_width(), CTA_HEIGHT)),
        true,
    )
    .clicked()
}

/// 次 CTA：raised_bg 底 + border_strong 描边，t2 文字 + 文件夹图标。
///
/// 同 [`cta_primary`] 的工厂化路径，区别在 [`ButtonStyle::Raised`]（自动写
/// raised_bg / border_strong / t2 字）。
fn cta_secondary(ui: &mut egui::Ui, shell: &UiShell, label: &str) -> bool {
    widget_button(
        ui,
        shell,
        ButtonContent::ImageText(ICON_FOLDER_OPEN, label),
        ButtonStyle::Raised,
        Some(egui::vec2(ui.available_width(), CTA_HEIGHT)),
        true,
    )
    .clicked()
}

/// 最近工程小节（Figma `RecentProjects`）。
///
/// `palette.pending` 通过唯一分发器出口，避免本页重复实现"打开最近工程"的
/// 路径——与菜单点同一行项走同一条命令。右侧 `time_ago` 由
/// [`crate::ui::widgets::format_time_ago`] 算（Figma 原型设计）；`now` 在循环
/// 外取一次，保证本帧所有行的"现在"一致。
fn recents_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    recents: &RecentProjects,
    palette: &mut CommandPalette,
    current_loc: &CurrentLocale,
) {
    let entries = recents.entries();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        ui.label(
            egui::RichText::new(t_key(current_loc, I18nKey::WelcomeRecentHeading))
                .font(proportional(font_size::MICRO))
                .strong()
                .color(shell.t3())
                .extra_letter_spacing(0.8),
        );
        if entries.is_empty() {
            ui.label(
                egui::RichText::new(t_key(current_loc, I18nKey::WelcomeRecentEmpty))
                    .font(proportional(font_size::MICRO))
                    .color(shell.t3()),
            );
            return;
        }
        let now = Instant::now();
        for (i, e) in entries.iter().take(WELCOME_RECENTS_MAX).enumerate() {
            let time_ago = format_time_ago(e.last_opened_at, now);
            if recent_row(ui, shell, ICON_MAP, &e.name, &e.dir, Some(&time_ago)).clicked() {
                palette.pending = Some(CommandId::OpenRecentProject(i as u8));
            }
        }
    });
}
