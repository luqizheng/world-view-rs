//! 事态登记面板（P-6）。
//!
//! ## 职责
//! - `panel_situation` —— 事态列表（带计数徽标）；空时显示空状态卡。
//!
//! ## 边界（不做什么）
//! - **不**改事态数据 —— 列表只读，编辑由 [`crate::editing`] 的命令走。
//!
//! ## 主要导出
//! - `panel_situation` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, BadgeKind};

/// 事态登记（P-6）。
pub fn panel_situation(
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
    let count = projection.world.situations.len();
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
        Panel::Situation,
        badge,
        current_loc.0,
        |ui| {
            let sits = &projection.world.situations;
            if sits.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "▣",
                    t_key(&current_loc, I18nKey::SituationPanelEmptyTitle),
                    t_key(&current_loc, I18nKey::SituationPanelEmptyHint),
                    t_key(&current_loc, I18nKey::SituationPanelEmptyAction),
                );
            } else {
                for s in sits {
                    let meta = match s.span.to {
                        Some(to) => format!(
                            "{}{}{}{}{}",
                            t_key(&current_loc, I18nKey::SpanDayPrefix),
                            s.span.from,
                            t_key(&current_loc, I18nKey::SpanDayRangeMid),
                            to,
                            t_key(&current_loc, I18nKey::SpanDaySuffix),
                        ),
                        None => format!(
                            "{}{}{}",
                            t_key(&current_loc, I18nKey::SpanDayPrefix),
                            s.span.from,
                            t_key(&current_loc, I18nKey::SpanDayOpenSuffix),
                        ),
                    };
                    super::widgets::list_item(
                        ui,
                        &shell,
                        crate::theme::colors::palette_to_color32(
                            crate::theme::colors::ACCENT_EVENT,
                        ),
                        &s.name,
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}
