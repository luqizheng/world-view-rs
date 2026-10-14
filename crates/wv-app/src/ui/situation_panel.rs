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

use crate::state::{Panel, PanelState, WorldProjection};
use crate::ui_layout::LayoutFile;
use crate::ui_theme::UiShellConfig;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, BadgeKind};

/// 事态登记（P-6）。
pub fn panel_situation(
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
                    let meta = match s.span.to {
                        Some(to) => format!("第 {}–{} 天", s.span.from, to),
                        None => format!("第 {} 天 –", s.span.from),
                    };
                    super::widgets::list_item(
                        ui,
                        &shell,
                        crate::colors::palette_to_color32(crate::colors::ACCENT_EVENT),
                        &s.name,
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}
