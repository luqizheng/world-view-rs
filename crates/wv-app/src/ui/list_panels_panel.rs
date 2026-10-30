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
use crate::state::{Panel, PanelState, PlaybackState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, list_item, BadgeKind};

/// 人物登记（P-9）。
#[allow(clippy::too_many_arguments)]
pub fn panel_character(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
    current_loc: Res<CurrentLocale>,
    playback: Res<PlaybackState>,
    mut level_editor: ResMut<crate::ui::character_level_dialog::CharacterLevelEditor>,
    // UX-002 batch6：「登记人物」对话框（空态动作 + 列表尾按钮双入口）。
    mut character_create: ResMut<crate::ui::entity_create_dialog::CharacterCreateDialog>,
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
                // UX-002：空态动作可点——直接打开「登记人物」对话框。
                if empty_state(
                    ui,
                    &shell,
                    "◎",
                    t_key(&current_loc, I18nKey::CharacterPanelEmptyTitle),
                    t_key(&current_loc, I18nKey::CharacterPanelEmptyHint),
                    t_key(&current_loc, I18nKey::CharacterPanelEmptyAction),
                ) {
                    character_create.open = true;
                }
            } else {
                for c in chars {
                    // 元信息 = 等级段数（存储计数，非时刻投影，不经 TimelineQuery）。
                    let meta = format!(
                        "{} {}",
                        c.state_spans.len(),
                        t_key(&current_loc, I18nKey::CharacterLevelSpanCountUnit)
                    );
                    let resp = list_item(ui, &shell, shell.accent(), &c.name, &meta, false);
                    // 行点击 = 打开该人物的等级列表编辑器（草稿从当前表克隆）。
                    if resp.clicked() {
                        level_editor.open_for(
                            c.id.clone(),
                            c.name.clone(),
                            c.state_spans.clone(),
                            playback.current_day,
                        );
                    }
                }
            }
            ui.add_space(space::XS);
            // UX-002：常驻登记入口（非空列表也要能加人）。
            if ui
                .button(t_key(&current_loc, I18nKey::CharacterCreateTitle))
                .clicked()
            {
                character_create.open = true;
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
                        crate::theme::palette_to_color32(crate::theme::colors::ACCENT_EVENT),
                        e.id.as_str(),
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}
