//! 右区偏好控件：日夜模式 / 语言 / Rebuild / 设置入口。

use super::*;

/// 偏好控件高度（Figma `EL-bf36dfe9` 26px）。
pub(super) const PREF_HEIGHT: f32 = 26.0;
/// 偏好控件左右内边距（Figma `padding: 0px 8px`）。
pub(super) const PREF_PAD: f32 = 8.0;
/// 偏好控件子项间距（Figma `gap: 6px`）。
pub(super) const PREF_GAP: f32 = 6.0;
/// 偏好控件图标尺寸（Figma 12×12）。
pub(super) const PREF_ICON: f32 = 12.0;

/// 语言短标签（Zh=「简」/ En=「EN」）。
pub(super) fn language_short(loc: crate::i18n::Locale) -> &'static str {
    match loc {
        crate::i18n::Locale::Zh => "简",
        crate::i18n::Locale::En => "EN",
    }
}

/// 日夜控件的（图标, 当前模式标签, 切换提示）——图标表示**当前**模式。
pub(super) fn theme_parts(cfg: &UiShellConfig) -> (egui::ImageSource<'static>, I18nKey, I18nKey) {
    if cfg.theme.is_dark() {
        (
            ICON_MOON,
            I18nKey::TopBarThemeDark,
            I18nKey::TopBarSwitchToDay,
        )
    } else {
        (
            ICON_SUN,
            I18nKey::TopBarThemeLight,
            I18nKey::TopBarSwitchToNight,
        )
    }
}

/// 保存态（Figma `3:20981/3:20982`：5px 圆点 + 11px 弱文本）。
///
/// 设计稿示例带时刻（`Saved 14:32`），但工程目前**不持久化保存时间戳**；
/// 不拿「当前时钟」冒充保存时刻（会骗人），只报已保存 / 未保存。
pub(super) fn save_indicator(
    ui: &mut egui::Ui,
    shell: &UiShell,
    dirty: bool,
    loc: crate::i18n::Locale,
    right: f32,
    cy: f32,
) {
    let (dot, key) = if dirty {
        (color32(shell.palette.badge_warn), I18nKey::TopBarUnsaved)
    } else {
        (shell.success(), I18nKey::TopBarSaved)
    };
    let galley = ui.painter().layout_no_wrap(
        crate::i18n::t(loc, key).to_owned(),
        proportional(font_size::MICRO),
        shell.t3(),
    );
    let text_x = right - galley.size().x;
    ui.painter().galley(
        egui::pos2(text_x, cy - galley.size().y * 0.5),
        galley,
        shell.t3(),
    );
    let dot_center = egui::pos2(text_x - space::XS - 2.5, cy);
    ui.painter().circle_filled(dot_center, 2.5, dot);
}

/// 偏好控件尾随内容（Figma：快捷键文本 / 下翻 chevron）。
#[derive(Clone, Copy)]
pub(super) enum PrefTrailing<'a> {
    Shortcut(&'a str),
    Chevron,
}

/// 偏好控件总宽（含左右内边距）。
pub(super) fn preference_width(ui: &egui::Ui, label: &str, trailing: PrefTrailing<'_>) -> f32 {
    let label_w = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            proportional(font_size::MICRO),
            ui.visuals().text_color(),
        )
        .size()
        .x;
    let trail_w = match trailing {
        PrefTrailing::Shortcut(keys) => {
            ui.painter()
                .layout_no_wrap(keys.to_owned(), mono(10.0), ui.visuals().weak_text_color())
                .size()
                .x
        }
        PrefTrailing::Chevron => PREF_ICON,
    };
    PREF_PAD + PREF_ICON + PREF_GAP + label_w + PREF_GAP + trail_w + PREF_PAD
}

/// 26px 偏好控件（Figma `EL-bf36dfe9`）：raised 底 + 1px 描边 + 圆角 4，
/// 内边距左右 8、子项间距 6。整块可点（含 padding 区）。
///
/// 内容全部在**给定矩形内**左起绝对定位——不参与父布局方向。
pub(super) fn preference_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    icon: egui::ImageSource<'static>,
    label: &str,
    trailing: PrefTrailing<'_>,
) -> egui::Response {
    ui.painter().rect(
        rect,
        radius::BADGE,
        shell.raised_bg(),
        egui::Stroke::new(1.0, shell.border_subtle()),
        egui::StrokeKind::Inside,
    );

    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), proportional(font_size::MICRO), shell.t1());
    let trail_galley = match trailing {
        PrefTrailing::Shortcut(keys) => Some(ui.painter().layout_no_wrap(
            keys.to_owned(),
            mono(10.0),
            shell.t3(),
        )),
        PrefTrailing::Chevron => None,
    };

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + PREF_PAD + PREF_ICON * 0.5, rect.center().y),
        egui::vec2(PREF_ICON, PREF_ICON),
    );
    let _ = ui.put(icon_rect, icon_image(icon, shell.t2(), PREF_ICON));

    let label_x = icon_rect.max.x + PREF_GAP;
    let trail_x = label_x + label_galley.size().x + PREF_GAP;
    ui.painter().galley(
        egui::pos2(label_x, rect.center().y - label_galley.size().y * 0.5),
        label_galley,
        shell.t1(),
    );
    match trail_galley {
        Some(galley) => {
            ui.painter().galley(
                egui::pos2(trail_x, rect.center().y - galley.size().y * 0.5),
                galley,
                shell.t3(),
            );
        }
        None => {
            let chevron_rect = egui::Rect::from_center_size(
                egui::pos2(trail_x + PREF_ICON * 0.5, rect.center().y),
                egui::vec2(PREF_ICON, PREF_ICON),
            );
            let _ = ui.put(
                chevron_rect,
                egui::Image::new(ICON_CHEVRON_DOWN)
                    .tint(shell.t3())
                    .fit_to_exact_size(egui::vec2(PREF_ICON, PREF_ICON)),
            );
        }
    }

    // 交互矩形**最后注册**：图标 `put` 进来的 hover 区不会挡住整块点击。
    let resp = ui.allocate_rect(rect, egui::Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// 日夜模式控件：点击只写 [`UiShellConfig::theme`]；真正投影到 egui
/// 由 [`crate::ui_shell`] 的 `apply_ui_theme` 在 `PreUpdate` 统一完成。
#[allow(clippy::too_many_arguments)]
pub(super) fn theme_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    icon: egui::ImageSource<'static>,
    label: &str,
    hint: I18nKey,
    cfg: &mut UiShellConfig,
    current_loc: &CurrentLocale,
) {
    let resp = preference_control(
        ui,
        shell,
        rect,
        icon,
        label,
        PrefTrailing::Shortcut("Ctrl+J"),
    )
    .on_hover_text(crate::i18n::t(current_loc.0, hint));
    if resp.clicked() {
        cfg.theme = cfg.theme.toggled();
    }
}

/// Rebuild 控件（2026-10-19 用户裁定：位置移到语言之后、样式与日夜控件同款）。
///
/// 只排队命令，不直接碰渲染资源——`ForceRebuildTexture` 由
/// [`crate::ui_shell::apply_pending_commands`] 统一执行。
pub(super) fn rebuild_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    palette: &mut crate::ui_command_palette::CommandPalette,
    current_loc: &CurrentLocale,
) {
    let resp = preference_control(
        ui,
        shell,
        rect,
        ICON_REFRESH,
        "Rebuild",
        PrefTrailing::Shortcut("F5"),
    )
    .on_hover_text(crate::i18n::t(
        current_loc.0,
        I18nKey::TopBarForceRedrawTooltip,
    ));
    if resp.clicked() {
        palette.pending = Some(CommandId::ForceRebuildTexture);
    }
}

/// 语言控件（ADR-0018）：图标 + 短标签 + chevron，点击弹出全名菜单。
///
/// 用 [`egui::Popup::from_toggle_button_response`]（memory 驱动）而不是
/// `from_response`：后者默认常开（egui 0.36 迁移坑，详见
/// [`super::project_menu`] 同款注释）。
pub(super) fn language_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    short: &str,
    current_loc: &mut CurrentLocale,
) {
    let resp = preference_control(
        ui,
        shell,
        rect,
        ICON_LANGUAGES,
        short,
        PrefTrailing::Chevron,
    );

    egui::Popup::from_toggle_button_response(&resp)
        .id(egui::Id::new("wv_language_menu"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(140.0);
            for (label, loc) in [
                ("简体中文", crate::i18n::Locale::Zh),
                ("English", crate::i18n::Locale::En),
            ] {
                let selected = current_loc.0 == loc;
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new(label)
                            .size(shell.scale.small)
                            .color(if selected { shell.t1() } else { shell.t2() }),
                    ))
                    .clicked()
                {
                    current_loc.0 = loc;
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });
}

/// 设置入口（Figma `3:22599` `settings-2`，裸图标 14×14，悬停才有底色）。
pub(super) fn settings_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    settings: &mut SettingsDialog,
    current_loc: &CurrentLocale,
) {
    if ui.rect_contains_pointer(rect) {
        ui.painter()
            .rect_filled(rect, radius::CONTROL, shell.hover_bg());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
    let _ = ui.put(
        icon_rect,
        egui::Image::new(ICON_SETTINGS)
            .tint(shell.t2())
            .fit_to_exact_size(egui::vec2(14.0, 14.0)),
    );
    let resp = ui
        .allocate_rect(rect, egui::Sense::click())
        .on_hover_text(crate::i18n::t(
            current_loc.0,
            I18nKey::TopBarSettingsTooltip,
        ));
    if resp.clicked() {
        settings.open = true;
    }
}

pub(super) fn count_entities(projection: &WorldProjection) -> usize {
    let w = &projection.world;
    w.units.len() + w.characters.len() + w.situations.len() + w.events.len()
}
