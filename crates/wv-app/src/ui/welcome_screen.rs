//! 欢迎页 / 无工程闸门（2026-10-15 用户需求：把无工程欢迎页对齐
//! Figma 设计稿 [`Zw4tonExV5lIY_EJamjPRw`](https://pixso.cn/app/design/Zw4tonExV5lIY_EJamjPRw)
//! 的 `00_WelcomeScreen` 帧）。
//!
//! ## 术语（2026-10-22 规范）
//! - **欢迎页** = 本页。**无工程时接管全屏的着陆页**，是一个**终点态**：
//!   用户在此新建 / 打开工程，抑或就此离开。
//! - **无工程闸门** = 判据 `WorldProjection::has_open_project()`
//!   （= `project_dir.is_some()`）。**不是本页的名字**，而是一条恒定规则：
//!   UI 侧决定本页是否接管，渲染侧禁止落笔 / 框选 / 测量。
//!   （旧称「无工程锁」——2026-10-22 改名：它不是一把可开可锁的锁。）
//! - **启动屏** = 进程启动到世界数据就绪之间的过场屏（`ui_shell` 的
//!   `startup_load_overlay`）。与本页是**两种角色**，不可合并：启动屏必然
//!   一闪而过，欢迎页是用户停留的界面。
//! - **场景** 在本项目专指「地点图」（城市 / 地下城 / 森林 / 村落，见
//!   场景编辑器设计），与上述三者都不是一回事。
//!
//! ## 设计要点
//! - 暗色壳背景 + 居中卡片（520px 宽，14 圆角，32 内边距）。
//! - 卡内三段竖向 auto-layout（间距 24px）：
//!   **Header**（地球图标 + 主副标题）/ **ActionGroup**（主+次 CTA）/
//!   **RecentProjects**（小标题 + 列表 + 时间水印待补）/ **Footer**（状态
//!   说明 + 版本水印）。
//! - **无标题栏**：Figma 没有拖动条，本页不画。
//! - **关闭按钮（2026-10-26 追加，偏离 Figma）**：原结论「无关闭按钮，用户
//!   只能经由新建/打开/最近工程离开本页」被修正——无边框窗
//!   （`decorations: false`，2026-10-25 裁定）之下本页没有任何原生窗控，
//!   无工程时唯一关闭途径只剩 Alt+F4。经用户反馈补**视口右上角**窗控
//!   关闭键，视觉与顶栏窗控同源（[`crate::ui::top_bar`] 的
//!   `window_controls::close_button`）；离开本页仍以新建/打开/最近工程
//!   为主路径，× 只兜「就此离开」。
//! - 颜色全部走 [`crate::theme::ui_tokens::ShellPalette`] 令牌，未写裸 hex
//!   （除了一次性白色 `Color32::WHITE` 用在主 CTA 的图标+文字——
//!   Figma `#FFFFFF` 与暗色 `text_on_accent` `#0B0D11` 在「主 CTA」上对比度
//!   都达标（≥4.5），但 `text_on_accent` 是历史暗色基底，本页统一用白以匹配原型）。
//!
//! ## 本地化（ADR-0018 收尾）
//! - 6 处文案经 `Res<CurrentLocale>` 注入 `t_key(...)` 走 `I18nKey` 表：
//!   `WelcomeTagline` / `WelcomeHelpText` / `WelcomeNewProjectBtn` /
//!   `WelcomeOpenProjectBtn` / `WelcomeRecentHeading` / `WelcomeRecentEmpty`。
//!   （`WelcomeHeading` 不在本页——它是**窗口标题**，由
//!   `app_bootstrap::sync_window_title_with_locale` 消费。）
//! - 品牌名 `World-View` 与底部 `World-View Alpha` 后缀两语统一不本地化
//!   （产品定位决定——见 `README.md` §1）。
//! - `widgets::format_time_ago` 暂未本地化，待 widgets 模块后续 PR 单独处理。
//!
//! ## 边界（不做什么）
//! - **不**加载 / 创建——按钮只排队意图：打开 → [`OpenProjectTask`]，
//!   新建 → [`NewProjectDialog`]，最近工程 → `CommandPalette::pending`
//!   （走唯一分发器，与菜单点「打开最近工程」同一条路）。
//! - **不**在加载中画——加载期间让位给启动加载遮罩，
//!   避免两层压暗叠一起；命令面板 / 新建工程对话框打开时也让位
//!   （它们与本页同为 Foreground 层，抢顶层）。
//!
//! ## 主要导出
//! - `welcome_screen` —— Bevy 系统（注册在 `ui_shell::register`）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::app_screen::AppScreen;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::RecentProjects;
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
use crate::ui_shell::{DialogPurpose, OpenProjectTask};

use super::shell::UiShell;
use super::top_bar::close_button;

/// 卡片窗口 id（也是 egui 里它的 Area/Layer id）。
///
/// 抽成常量是为了在 [`welcome_screen`] 尾部能把卡片层显式钉回顶层——
/// 见那里关于「暗层点击上浮盖住卡片」的回归说明。
const CARD_ID: &str = "wv_welcome_card";
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
/// 视口右上角窗控关闭键边长（与顶栏窗控键同宽 30，见
/// `top_bar::window_controls::BTN_W`；命中区饱满，Windows 关闭键惯例位）。
const CLOSE_BTN: f32 = 30.0;

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
/// 渲染条件（2026-10-22 B 档起由 [`AppScreen`] 一句话裁决）：
/// 没有打开工程、且不在加载中、且没有加载失败——这三层「让位」避免与
/// 加载遮罩或错误卡片叠绘。命令面板 / 新建工程对话框打开时也额外让位。
///
/// **加载失败时也让位**：加载失败后屏状态转为「错误」时，
/// 下一帧起「无工程」条件成立——若还画欢迎页，就会把居中的
/// [`crate::ui_shell::load_error_card`] 盖住（全屏底色 + 卡片压在同为
/// `Order::Foreground` 的错误窗之上），用户只看到欢迎页、看不到失败原因。
/// 让位后错误卡片是唯一可见的浮层，作者能立刻读到路径或 YAML 错误。
#[allow(clippy::too_many_arguments)]
pub fn welcome_screen(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    recents: Res<RecentProjects>,
    current_loc: Res<CurrentLocale>,
    mut open_task: ResMut<OpenProjectTask>,
    mut palette: ResMut<CommandPalette>,
    mut new_project: ResMut<NewProjectDialog>,
    // 应用屏状态（2026-10-22 B 档）：「该不该显示欢迎页」由它一句话决定，
    // 不再在本页拼 `has_open_project || 加载中 || 出错` 三个条件。
    screen: Res<AppScreen>,
    // 无边框窗之下本页是「无工程」时唯一的 UI，关闭键直写退出
    // （与顶栏窗控同语义：单窗口编辑器，关窗即退出）。
    mut exit: MessageWriter<AppExit>,
) {
    if !screen.shows_welcome() {
        return;
    }
    if palette.open || new_project.open {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let viewport = ctx.viewport_rect();

    // 全屏点击吞吃层 + `bg_app` 底色（Figma `00_WelcomeScreen` 的整体底色）：
    // 视觉上与空场景一致，功能上仍挡住下面所有 UI（地图 + 浮窗都点不到——
    // 「什么都不能做，先立工程」）。
    egui::Area::new(egui::Id::new("wv_welcome_dim"))
        .order(egui::Order::Foreground)
        .fixed_pos(viewport.min)
        .interactable(true)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(viewport.size(), egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, shell.palette.bg_app.to_color32());
        });

    // ── 窗控关闭键（视口右上角；理由见模块文档「关闭按钮」段）─────────
    // 注册在暗层之后，同 Foreground order 内初始即在暗层之上；层级随暗层
    // 点击上浮的问题在函数尾部随卡片一起钉顶解决。
    egui::Area::new(egui::Id::new("wv_welcome_close"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_TOP, [0.0, 0.0])
        .interactable(true)
        .show(ctx, |ui| {
            let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(CLOSE_BTN, CLOSE_BTN));
            if close_button(ui, &shell, rect).clicked() {
                exit.write(AppExit::Success);
            }
        });

    // 居中卡片——`title_bar(false)` 拿掉 egui 默认标题栏与 × 按钮；自定义
    // `Frame` 复写 fill / stroke / 内边距 / 圆角以贴近 Figma。
    egui::Window::new(CARD_ID)
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

                // ── Footer：无工程提示 + 版本水印 ────────────────
                ui.add_space(space::SM);
                ui.vertical_centered(|ui| {
                    // 一句白话说明「现在为什么什么都不能画」——「无工程闸门」
                    // 的日常版。此前这条文案（`WelcomeHelpText`）登记在 i18n
                    // 表里却没有调用点，页面只说了「这是什么产品」，没说
                    // 「你现在处在哪个状态、要先做什么」。
                    ui.label(
                        egui::RichText::new(t_key(&current_loc, I18nKey::WelcomeHelpText))
                            .font(proportional(font_size::MICRO))
                            .color(shell.t3()),
                    );
                    ui.label(
                        egui::RichText::new(FOOTER_VERSION)
                            .font(proportional(font_size::MICRO))
                            .color(shell.t3()),
                    );
                });
            });
        });

    // 卡片钉回 Foreground 顶层（2026-10-19 回归修复）。
    //
    // 上面的暗层是**全屏可交互** Area（要吞掉地图/浮窗的点击）。egui 在
    // `Area::begin` 里对「按在自己身上」的 Area 调 `move_to_top`——同 order 内
    // 暗层会浮到卡片之上；而暗层是全屏**不透明**填充，于是「关闭工程回到欢迎页
    // 后点一下空白处，整页只剩暗底、卡片消失」（实机复现：Bevy `Screenshot`
    // 也拍到只剩底色）。层级顺序在帧末统一结算，所以这里在画完卡片后显式把它
    // 钉回顶层：同帧即生效，下一帧的命中测试也仍从卡片开始。
    pin_card_on_top(ctx);
    // 关闭键最后钉：置于卡片之上（视口极窄与卡片重叠时，窗控键恒在最上——
    // Windows 惯例）。不钉的话，暗层被按一下就浮到它之上——× 被不透明的
    // 暗层盖住，既看不见也点不到（同 `pin_card_on_top` 的回归机理）。
    ctx.move_to_top(close_button_layer());
}

/// 把欢迎卡片钉回 Foreground 顶层（见 [`welcome_screen`] 尾部回归说明）。
fn pin_card_on_top(ctx: &egui::Context) {
    ctx.move_to_top(card_layer());
}

/// 卡片在 egui 里的层 id。
///
/// `Window::new` 走 `Area::new(Id::new(title.text()))`，而 `Atoms::text()`
/// 返回 `Option<&str>`——所以它的 id 是 `Id::new(Some(name))`，不是
/// `Id::new(name)`。两者哈希不同，钉错了层就会「看起来改完还是盖住」。
fn card_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new(Some(CARD_ID)))
}

/// 关闭键在 egui 里的层 id。
///
/// 与 [`card_layer`] 不同：`Area::new(Id::new(name))`（非 `egui::Window`）
/// 的层 id 就是传入的 Id 本身，没有 `Some(...)` 包装。
fn close_button_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("wv_welcome_close"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 回归（2026-10-19）：欢迎页的全屏暗层是可交互 Area（吞点击）。egui 在
    /// 按下时会把它 `move_to_top`，同 order 内浮到卡片之上——暗层是**不透明**
    /// 全屏填充，于是用户看到的是「关闭工程回到欢迎页后点一下空白，卡片消失、
    /// 整页只剩暗底」。修复靠画完卡片后 `pin_card_on_top` 把它钉回顶层。
    ///
    /// 用 headless `Context` 驱动真实点击链：hover → 按下暗层（卡片之外）→ 抬起，
    /// 断言卡片中心的顶层 layer 仍是卡片。没有 `pin_card_on_top` 时这里会
    /// 返回 `wv_welcome_dim`（实机/headless 双向验证过）。
    #[test]
    fn clicking_the_dim_keeps_the_welcome_card_on_top() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let center = screen.center();
        // 卡片 520 宽且居中 → x∈[380,900]；取左下角空白处，确保点在暗层上。
        let dim_pos = egui::pos2(100.0, 700.0);

        let draw = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::Area::new(egui::Id::new("wv_welcome_dim"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(screen.min)
                    .interactable(true)
                    .show(ctx, |ui| {
                        let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
                    });
                egui::Window::new(CARD_ID)
                    .order(egui::Order::Foreground)
                    .title_bar(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("card");
                    });
                pin_card_on_top(ctx);
            });
            // headless 无 GPU surface：丢弃未应用的纹理增量，避免 epaint
            // 「Dropped TexturesDelta with unapplied deltas」panic。
            output.textures_delta.clear();
        };

        // 暖机三帧：Area 首帧是 sizing pass（不可交互、也不计入层级），
        // 直接点会被忽略；这与真实运行（欢迎页已稳定显示数帧）一致。
        for _ in 0..3 {
            draw(vec![]);
        }
        draw(vec![egui::Event::PointerMoved(dim_pos)]);
        draw(vec![egui::Event::PointerButton {
            pos: dim_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }]);

        assert_eq!(
            ctx.layer_id_at(center),
            Some(card_layer()),
            "点击全屏暗层后，卡片必须仍在 Foreground 顶层（否则暗底会盖住卡片）"
        );
    }

    /// 回归（2026-10-26）：视口右上角的窗控关闭键与暗层同在 Foreground
    /// order——暗层被按下会 `move_to_top` 浮到其上（上一测试同款机理）。
    /// 关闭键若不随卡片一起钉回顶层，用户点一下空白 × 就被不透明的暗层
    /// 盖住：看不见也点不到，无边框窗下无从关闭。
    ///
    /// headless 驱动真实点击链：按下暗层（卡片与关闭键之外）→ 抬起，断言
    /// 关闭键中心的顶层 layer 仍是关闭键层。
    #[test]
    fn clicking_the_dim_keeps_the_close_button_on_top() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let close_pos = egui::pos2(screen.max.x - CLOSE_BTN / 2.0, CLOSE_BTN / 2.0);
        // 卡片 520 宽居中 → x∈[380,900]；取左下角空白处，确保点在暗层上。
        let dim_pos = egui::pos2(100.0, 700.0);

        let draw = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::Area::new(egui::Id::new("wv_welcome_dim"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(screen.min)
                    .interactable(true)
                    .show(ctx, |ui| {
                        let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
                    });
                // 与 welcome_screen 同构：右上角 30×30 命中格（视觉略去——
                // 本测试只关心层级，不关心笔刷）。
                egui::Area::new(egui::Id::new("wv_welcome_close"))
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::RIGHT_TOP, [0.0, 0.0])
                    .interactable(true)
                    .show(ctx, |ui| {
                        let rect = egui::Rect::from_min_size(
                            ui.cursor().min,
                            egui::vec2(CLOSE_BTN, CLOSE_BTN),
                        );
                        let _ = ui.allocate_rect(rect, egui::Sense::click());
                    });
                egui::Window::new(CARD_ID)
                    .order(egui::Order::Foreground)
                    .title_bar(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("card");
                    });
                pin_card_on_top(ctx);
                ctx.move_to_top(close_button_layer());
            });
            output.textures_delta.clear();
        };

        // 暖机三帧：Area 首帧是 sizing pass（不可交互、也不计入层级），
        // 与真实运行（欢迎页已稳定显示数帧）一致。
        for _ in 0..3 {
            draw(vec![]);
        }
        draw(vec![egui::Event::PointerMoved(dim_pos)]);
        draw(vec![egui::Event::PointerButton {
            pos: dim_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        }]);

        assert_eq!(
            ctx.layer_id_at(close_pos),
            Some(close_button_layer()),
            "点击全屏暗层后，视口右上角的关闭键必须仍是顶层（否则暗层盖住 ×，无从关闭）"
        );
    }
}
