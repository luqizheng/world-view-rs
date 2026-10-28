//! 应用设置对话框（顶栏最右 `settings-2` 入口，Figma `3:20966`）。
//!
//! ## 职责
//! - [`SettingsDialog`] —— 跨帧开关状态。
//! - [`settings_dialog`] —— 绘制「浮动窗口」分组：逐窗显隐 + 重置位置；
//!   「HTTP 服务」分组（`--features http` 构建才有，2026-10-28）：运行
//!   状态 + 端口 + 启动 / 停止 / 换端口重启（见
//!   [`crate::http_integration::HttpService`]）。
//!
//! ## 边界（不做什么）
//! - **不**自己重置布局——只置 [`PanelState::reset_requested`]，由
//!   [`crate::ui_shell::apply_panel_reset`] 在下一帧统一抹 egui 记忆与
//!   `ui-layout.yaml` 的内存副本（与命令面板 `ResetPanelLayout` 同一出口）。
//! - **不**管工程菜单——工程基本功能（新建 / 打开 / 保存 / 关闭）仍在
//!   [`super::project_menu`]；本对话框只承担「工作区窗口」这一类偏好。
//! - **不**写世界数据（AGENTS §3.3）；HTTP 分组也只动服务生命周期资源，
//!   不碰 `WorldProjection`。
//! - **不**持久化端口——端口是会话级偏好（初始取 CLI `--http-port` 或
//!   默认 8732），落盘属后续待裁项。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::{font_size, space};

#[cfg(feature = "http")]
use crate::http_integration::HttpServiceStatus;

use super::shell::UiShell;
use super::widgets::{button, check_row, ButtonContent, ButtonStyle, DialogWindow};
// HTTP 服务分组专用控件（分组本体仅在 `--features http` 构建编译）。
#[cfg(feature = "http")]
use super::widgets::{divider, numeric_input_salted, numeric_texts};

/// 设置对话框跨帧状态。
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct SettingsDialog {
    /// 是否打开（顶栏 `settings-2` 置 true，关闭按钮 / Esc 置回 false）。
    pub open: bool,
}

/// 应用设置对话框。
///
/// 面板显隐**即时生效**（`PanelState` 是 `panel_*` 系统的唯一真相），
/// 落盘由 [`crate::ui_shell::persist_layout_on_change`] 在松手后统一完成。
pub fn settings_dialog(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<SettingsDialog>,
    mut panels: ResMut<PanelState>,
    current_loc: Res<CurrentLocale>,
    // 仅 `--features http` 构建：该构建下 `HttpServerPlugin` 必已插入资源，
    // `ResMut` 不会缺资源 panic；非 http 构建参数整体编译剔除。
    #[cfg(feature = "http")] mut http_svc: ResMut<crate::http_integration::HttpService>,
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let mut reset = false;

    DialogWindow::new(t(loc, I18nKey::SettingsDialogTitle), &mut dialog.open)
        .width(460.0)
        .show(ctx, &shell, |ui| {
            ui.label(
                egui::RichText::new(t(loc, I18nKey::SettingsPanelsSection))
                    .size(font_size::SMALL)
                    .color(shell.t1()),
            );
            ui.label(
                egui::RichText::new(t(loc, I18nKey::SettingsPanelsHint))
                    .size(font_size::MICRO)
                    .color(shell.t3()),
            );
            ui.add_space(space::SM);

            // 两列铺 11 个面板（`Panel::ALL`，ADR-0070 后含河流和道路；计数
            // 随 ALL 走，2026-10-22 统一 Inspector 后本注释一度停在 12）：
            // 单列会拉到 ~350px 高，对话框超出小屏。
            // `check_row` 吃 `available_width`，放进 `columns` 才得到半宽命中区。
            ui.columns(2, |cols| {
                for (i, panel) in Panel::ALL.iter().enumerate() {
                    let ui = &mut cols[i % 2];
                    let mut on = panels.is_visible(*panel);
                    let before = on;
                    // 不挂尾随图标：`check_row` 把它贴到整行最右，两列并排时
                    // 像第二列无关字形，反而干扰读标签。
                    let _ = check_row(ui, &shell, &mut on, panel.title(loc), None);
                    if on != before {
                        panels.set_visible(*panel, on);
                    }
                }
            });

            ui.add_space(space::MD);
            if button(
                ui,
                &shell,
                ButtonContent::Text(t(loc, I18nKey::SettingsResetPanels)),
                ButtonStyle::Raised,
                Some(egui::vec2(ui.available_width(), shell.hit)),
                true,
            )
            .clicked()
            {
                reset = true;
            }

            #[cfg(feature = "http")]
            http_section(ui, &shell, loc, &mut http_svc);
        });

    if reset {
        // 只发信号：真正抹 egui 窗口记忆发生在下一帧的 `apply_panel_reset`
        // （本帧面板已经按旧位置画完，就地重置会和它抢同一份 memory）。
        panels.reset_requested = true;
        dialog.open = false;
    }
}

/// 「HTTP 服务」分组（仅 `--features http` 构建）。
///
/// 状态行 + 端口输入 + 单个动作按钮：`Stopped/Failed` → 启动；
/// `Starting` → 停止；`Running` 且端口未变 → 停止；`Running` 且端口
/// 已改 → 换端口重启。动作直接调 [`crate::http_integration`] 的启停
/// 函数——它们只动服务生命周期（线程 + 停机信号），不碰世界数据。
#[cfg(feature = "http")]
fn http_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: crate::i18n::Locale,
    svc: &mut crate::http_integration::HttpService,
) {
    use crate::http_integration::{restart_http_service, start_http_service, stop_http_service};

    ui.add_space(space::MD);
    divider(ui, shell);
    ui.add_space(space::SM);
    ui.label(
        egui::RichText::new(t(loc, I18nKey::SettingsHttpSection))
            .size(font_size::SMALL)
            .color(shell.t1()),
    );
    ui.label(
        egui::RichText::new(t(loc, I18nKey::SettingsHttpHint))
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(space::SM);

    // 状态行：失败用 danger 红字，其余按层级淡出。
    let (status_line, status_color) = match &svc.status {
        HttpServiceStatus::Stopped => (
            t(loc, I18nKey::SettingsHttpStatusStopped).to_owned(),
            shell.t3(),
        ),
        HttpServiceStatus::Starting => (
            t(loc, I18nKey::SettingsHttpStatusStarting).to_owned(),
            shell.t2(),
        ),
        HttpServiceStatus::Running { addr, .. } => (
            format!("{}{addr}", t(loc, I18nKey::SettingsHttpStatusRunning)),
            shell.t2(),
        ),
        HttpServiceStatus::Failed { message } => (
            format!("{}{message}", t(loc, I18nKey::SettingsHttpStatusFailed)),
            shell.danger(),
        ),
    };
    ui.label(
        egui::RichText::new(status_line)
            .size(font_size::SMALL)
            .color(status_color),
    );

    // 端口行：有界数值输入（1..=65535），越界 / 非法由控件钳制 + 红闪提示，
    // 不存在「无效端口」态——`desired` 永远合法。
    ui.horizontal(|ui| {
        ui.label(t(loc, I18nKey::SettingsHttpPortLabel));
        numeric_input_salted(
            ui,
            shell,
            "http_port",
            numeric_texts(loc),
            &mut svc.port_edit,
            "",
            1.0,
            1.0,
            65535.0,
        );
    });
    ui.add_space(space::SM);

    let desired = svc.port_edit.round().clamp(1.0, 65535.0) as u16;
    let (label, action) = match svc.status {
        HttpServiceStatus::Stopped | HttpServiceStatus::Failed { .. } => {
            (t(loc, I18nKey::SettingsHttpStart), HttpAction::Start)
        }
        HttpServiceStatus::Starting => (t(loc, I18nKey::SettingsHttpStop), HttpAction::Stop),
        HttpServiceStatus::Running { port, .. } if port == desired => {
            (t(loc, I18nKey::SettingsHttpStop), HttpAction::Stop)
        }
        HttpServiceStatus::Running { .. } => {
            (t(loc, I18nKey::SettingsHttpRestart), HttpAction::Restart)
        }
    };
    if button(
        ui,
        shell,
        ButtonContent::Text(label),
        ButtonStyle::Raised,
        Some(egui::vec2(ui.available_width(), shell.hit)),
        true,
    )
    .clicked()
    {
        match action {
            HttpAction::Start => start_http_service(svc, desired),
            HttpAction::Stop => stop_http_service(svc),
            HttpAction::Restart => restart_http_service(svc, desired),
        }
    }

    // 「复制 agent skill」按钮（ADR-0085 标配入口）：把驱动指南全文塞进
    // 剪贴板，用户粘给自己的 AI 助手即得 15 工具直驱能力——与 `wv skill`
    // 子命令同源（crate::http::AGENT_SKILL_MD）。
    ui.add_space(space::SM);
    if button(
        ui,
        shell,
        ButtonContent::Text(t(loc, I18nKey::SettingsHttpSkillCopy)),
        ButtonStyle::Raised,
        Some(egui::vec2(ui.available_width(), shell.hit)),
        true,
    )
    .clicked()
    {
        ui.ctx().copy_text(crate::http::AGENT_SKILL_MD.to_owned());
        // 瞬时反馈：记时间戳，2s 内在按钮下显示「已复制」——不需要常驻
        // 状态字段（对话框没有可变的本地状态槽，temp 存储随 egui memory 走）。
        ui.data_mut(|d| d.insert_temp(ui.id(), std::time::Instant::now()));
    }
    let copied_at: Option<std::time::Instant> = ui.data_mut(|d| d.get_temp(ui.id()));
    if let Some(t0) = copied_at {
        if t0.elapsed().as_secs() < 2 {
            ui.label(
                egui::RichText::new(t(loc, I18nKey::SettingsHttpSkillCopied))
                    .size(font_size::MICRO)
                    .color(shell.t3()),
            );
        }
    }
}

/// [`http_section`] 的动作枚举（局部；文本已定，动作随状态判定）。
#[cfg(feature = "http")]
enum HttpAction {
    Start,
    Stop,
    Restart,
}
