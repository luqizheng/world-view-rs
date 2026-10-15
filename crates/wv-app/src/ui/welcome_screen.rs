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
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::{font_size, space};
use crate::ui::widgets::{ICON_FOLDER_OPEN, ICON_GLOBE, ICON_MAP, ICON_PLUS};
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
/// 主/次 CTA 高度（Figma `height = 40`）。
const CTA_HEIGHT: f32 = 40.0;
/// CTA 圆角（Figma `cornerRadius = 6`）。
const CTA_CORNER: u8 = 6;
/// AppIcon 圆角方块尺寸 / 圆角（Figma `AppIcon 56×56, cornerRadius = 12`）。
const APP_ICON_BOX: f32 = 56.0;
const APP_ICON_BOX_RADIUS: u8 = 12;
/// AppIcon 内地球图标尺寸（Figma `globe 28×28`）。
const APP_ICON_GLOBE: f32 = 28.0;
/// 最近工程行高（Figma `RecentItem height = 36`）。
const RECENT_ROW_H: f32 = 36.0;
/// 最近工程列表项内左右内边距（Figma `padding [0, 8]`）。
const RECENT_ROW_PAD_X: f32 = 8.0;
/// 最近工程列表项内 icon 边长（Figma `map 14×14`）。
const RECENT_ROW_ICON: f32 = 14.0;
/// CTA 内 icon 边长（Figma `plus/folder-open 16×16`）。
const CTA_ICON: f32 = 16.0;

/// 主标题（与 Figma `AppTitle = "World-View"` 一致）。
const APP_TITLE: &str = "World-View";
/// 副标题（与 Figma `AppSubtitle` 一致）。
const APP_SUBTITLE: &str = "World-Building & Cartography Editor";
/// 主 CTA 文案（与 Figma `BtnNewProject` 一致）。
const NEW_PROJECT_LABEL: &str = "New Project…";
/// 次 CTA 文案（与 Figma `BtnOpenProject` 一致）。
const OPEN_PROJECT_LABEL: &str = "Open Project…";
/// 最近工程小节标题（与 Figma `Recent Projects` 一致）。
const RECENTS_HEADER: &str = "Recent Projects";
/// 底部版本水印（Figma 直接给出；版本号同步是后续 TODO，见 README §12 待裁）。
const FOOTER_VERSION: &str = "v0.9.1 — World-View Alpha";

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
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
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
                        egui::RichText::new(APP_SUBTITLE)
                            .font(proportional(font_size::BASE))
                            .color(shell.t3()),
                    );
                });

                // ── Divider ─────────────────────────────────────
                divider(ui, &shell);

                // ── ActionGroup（主/次 CTA，gap = 8） ───────────
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = space::SM;
                    if cta_primary(ui, &shell, NEW_PROJECT_LABEL) {
                        new_project.open();
                    }
                    if cta_secondary(ui, &shell, OPEN_PROJECT_LABEL) {
                        open_task.request(DialogPurpose::Open);
                    }
                });

                // ── Divider ─────────────────────────────────────
                divider(ui, &shell);

                // ── RecentProjects ──────────────────────────────
                recents_section(ui, &shell, &recents, &mut palette);

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
/// 用 [`egui::Button::image`] 把图标裹进可悬浮控件——`fill` 设成 accent，
/// `corner_radius` 设成 12，整体 `min_size` 锁 56×56。不挂 on_click（装饰用）。
fn app_icon(ui: &mut egui::Ui, shell: &UiShell) {
    let image = egui::Image::new(ICON_GLOBE)
        .tint(egui::Color32::WHITE)
        .fit_to_exact_size(egui::vec2(APP_ICON_GLOBE, APP_ICON_GLOBE));
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
/// 返回 `true` 当用户点击——caller 据此排队意图。
fn cta_primary(ui: &mut egui::Ui, shell: &UiShell, label: &str) -> bool {
    let image = egui::Image::new(ICON_PLUS)
        .tint(egui::Color32::WHITE)
        .fit_to_exact_size(egui::vec2(CTA_ICON, CTA_ICON));
    let resp = ui.add(
        egui::Button::image_and_text(
            image,
            egui::RichText::new(label)
                .font(proportional(font_size::INPUT))
                .strong()
                .color(egui::Color32::WHITE),
        )
        .fill(shell.accent())
        .min_size(egui::vec2(ui.available_width(), CTA_HEIGHT))
        .corner_radius(egui::CornerRadius::same(CTA_CORNER)),
    );
    resp.clicked()
}

/// 次 CTA：raised_bg 底 + border_strong 描边，t2 文字 + 文件夹图标。
fn cta_secondary(ui: &mut egui::Ui, shell: &UiShell, label: &str) -> bool {
    let image = egui::Image::new(ICON_FOLDER_OPEN)
        .tint(shell.t2())
        .fit_to_exact_size(egui::vec2(CTA_ICON, CTA_ICON));
    let resp = ui.add(
        egui::Button::image_and_text(
            image,
            egui::RichText::new(label)
                .font(proportional(font_size::INPUT))
                .color(shell.t2()),
        )
        .fill(shell.raised_bg())
        .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
        .min_size(egui::vec2(ui.available_width(), CTA_HEIGHT))
        .corner_radius(egui::CornerRadius::same(CTA_CORNER)),
    );
    resp.clicked()
}

/// 最近工程小节（Figma `RecentProjects`）。
///
/// `palette.pending` 通过唯一分发器出口，避免本页重复实现"打开最近工程"的
/// 路径——与菜单点同一行项走同一条命令。
fn recents_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    recents: &RecentProjects,
    palette: &mut CommandPalette,
) {
    let entries = recents.entries();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = space::XS;
        ui.label(
            egui::RichText::new(RECENTS_HEADER)
                .font(proportional(font_size::MICRO))
                .strong()
                .color(shell.t3())
                .extra_letter_spacing(0.8),
        );
        if entries.is_empty() {
            ui.label(
                egui::RichText::new("尚无最近工程——新建后会自动出现在这里。")
                    .font(proportional(font_size::MICRO))
                    .color(shell.t3()),
            );
            return;
        }
        for (i, e) in entries.iter().take(WELCOME_RECENTS_MAX).enumerate() {
            if recent_row(ui, shell, &e.name, &e.dir) {
                palette.pending = Some(CommandId::OpenRecentProject(i as u8));
            }
        }
    });
}

/// 单条最近工程（Figma `RecentItem`）：map 图标 + 名/路径。
///
/// 暂未实现时间戳（`last_opened` 字段与持久化待后续补；详见
/// `state::RecentProject` 注释）。Figma 中那一栏留作 `time_ago: &str` 接口，
/// 现阶段传 `""`，由 [`recent_row`] 在文字层做"空就隐藏"的处理。
fn recent_row(ui: &mut egui::Ui, shell: &UiShell, name: &str, dir: &str) -> bool {
    let resp = ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), RECENT_ROW_H),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add_space(RECENT_ROW_PAD_X);
            ui.add(
                egui::Image::new(ICON_MAP)
                    .tint(shell.t3())
                    .fit_to_exact_size(egui::vec2(RECENT_ROW_ICON, RECENT_ROW_ICON)),
            );
            ui.add_space(RECENT_ROW_PAD_X);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    egui::RichText::new(name)
                        .font(proportional(font_size::BASE))
                        .strong()
                        .color(shell.t1()),
                );
                ui.label(
                    egui::RichText::new(dir)
                        .font(proportional(font_size::MICRO))
                        .color(shell.t3()),
                );
            });
        },
    );
    resp.response.clicked()
}
