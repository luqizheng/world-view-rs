//! 人物（P-9）+ 事件（P-10）登记面板。
//!
//! ## 职责
//! - `panel_character` —— 人物列表（带计数徽标）。
//! - `panel_event` —— 事件列表（带计数徽标；从时间轴"＋登记事件"进入）。
//!
//! ## 边界（不做什么）
//! - **不**画详细属性 —— 仅列表展示。
//!
//! ## 主要导出
//! - `panel_character` / `panel_event` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, list_item, BadgeKind};

/// 人物登记（P-9）。
pub fn panel_character(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let count = projection.world.characters.len();
    let badge = if count == 0 {
        None
    } else {
        Some((BadgeKind::Neutral, count.to_string()))
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Character,
        badge,
        current_loc.0,
        |ui| {
            let chars = &projection.world.characters;
            if chars.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "◎",
                    t_key(&current_loc, I18nKey::CharacterPanelEmptyTitle),
                    t_key(&current_loc, I18nKey::CharacterPanelEmptyHint),
                    t_key(&current_loc, I18nKey::CharacterPanelEmptyAction),
                );
            } else {
                for c in chars {
                    list_item(ui, &shell, shell.accent(), &c.name, "", false);
                }
            }
        },
    );
}

/// 事件登记（P-10，可从时间轴进入）。
pub fn panel_event(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let count = projection.world.events.len();
    let badge = if count == 0 {
        None
    } else {
        Some((BadgeKind::Event, count.to_string()))
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Event,
        badge,
        current_loc.0,
        |ui| {
            let events = &projection.world.events;
            if events.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "○",
                    t_key(&current_loc, I18nKey::EventPanelEmptyTitle),
                    t_key(&current_loc, I18nKey::EventPanelEmptyHint),
                    t_key(&current_loc, I18nKey::EventPanelEmptyAction),
                );
            } else {
                for e in events {
                    let meta = match e.span.to {
                        Some(to) => format!(
                            "{}{}{}{}{}",
                            t_key(&current_loc, I18nKey::SpanDayPrefix),
                            e.span.from,
                            t_key(&current_loc, I18nKey::SpanDayRangeMid),
                            to,
                            t_key(&current_loc, I18nKey::SpanDaySuffix),
                        ),
                        None => format!(
                            "{}{}{}",
                            t_key(&current_loc, I18nKey::SpanDayPrefix),
                            e.span.from,
                            t_key(&current_loc, I18nKey::SpanDaySuffix),
                        ),
                    };
                    list_item(
                        ui,
                        &shell,
                        crate::theme::colors::palette_to_color32(
                            crate::theme::colors::ACCENT_EVENT,
                        ),
                        e.id.as_str(),
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}
