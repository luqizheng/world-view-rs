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
        |ui| {
            let chars = &projection.world.characters;
            if chars.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "◎",
                    "暂无已登记人物",
                    "人物是叙事的载体：登记一个，让它走进历史",
                    "登记人物 →",
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
        |ui| {
            let events = &projection.world.events;
            if events.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "○",
                    "暂无已登记事件",
                    "事件是时间轴上的锚点：登记一个，让它在地图上出现",
                    "登记事件 →",
                );
            } else {
                for e in events {
                    let meta = match e.span.to {
                        Some(to) => format!("第 {}–{} 天", e.span.from, to),
                        None => format!("第 {} 天", e.span.from),
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
