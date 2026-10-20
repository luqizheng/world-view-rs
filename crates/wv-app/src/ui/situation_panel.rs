//! 事态登记面板（P-6 · v0.3 Figma 视觉重做）。
//!
//! ## 职责
//! - `panel_situation` —— 事态列表（带计数徽标 + Figma `Y214 D40 — Y215 D12`
//!   风格元信息）；空时显示空状态卡。
//!
//! ## 边界
//! - **不**改事态数据 —— 列表只读，编辑由 [`crate::editing`] 的命令走。
//!
//! ## 主要导出
//! - `panel_situation` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::i18n::CurrentLocale;
use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, list_item, BadgeKind};

/// 事态登记（P-6 · v0.3）。
///
/// Figma 徽标用 `Warn`（橙）以匹配"事态=待关注"的语义；元信息格式
/// `Y{N} D{M} — Y{N'} D{M'}` 取代旧「第 X 天」，与时间线头部口径一致。
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
        // Figma 中计数徽标是 warn 橙（待关注）。
        Some((BadgeKind::Warn, count.to_string()))
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
                    "暂无已登记事态",
                    "事态是叙事的骨架：登记一个，把事件串起来",
                    "登记事态 →",
                );
            } else {
                for s in sits {
                    let meta = format_span_v03(s.span.from, s.span.to);
                    list_item(
                        ui,
                        &shell,
                        crate::theme::palette_to_color32(crate::theme::colors::ACCENT_EVENT),
                        &s.name,
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}

/// v0.3：Figma 风格 `Y214 D40 — Y215 D12` / `Y214 D82 — D108` 元信息。
///
/// 当前没有 wv_core 历法接口可用时按 360 天 / 年简化（与 timeline.rs
/// `season_of_day` 同口径）；M2 接 `wv_core::Calendar::format` 后替换。
fn format_span_v03(from: i64, to: Option<i64>) -> String {
    let f = day_to_yd(from);
    match to {
        Some(t) => {
            let t_yd = day_to_yd(t);
            format!("{} {} — {} {}", f.0, f.1, t_yd.0, t_yd.1)
        }
        None => format!("{} {} — …", f.0, f.1),
    }
}

fn day_to_yd(day: i64) -> (i32, i32) {
    let year = ((day - 1) / 360 + 1) as i32;
    let d = ((day - 1) % 360 + 1) as i32;
    (year, d)
}
