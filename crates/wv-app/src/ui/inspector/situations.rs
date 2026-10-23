//! 「事态」页签（统一 Inspector 的第三个页签）。
//!
//! 从旧 `situation_panel` 整体迁入（2026-10-22 统一 Inspector 设计）：
//! 事态列表（带计数徽标 + `Y214 D40 — Y215 D12` 风格元信息）。
//! 列表只读——事态的登记 / 编辑仍走时间轴与登记面板（设计稿 §3 非目标）。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::state::WorldProjection;
use crate::ui::shell::UiShell;

use crate::ui::widgets::{empty_state, list_item};

/// 「事态」页签内容。
pub fn situations_tab(ui: &mut egui::Ui, shell: &UiShell, projection: &WorldProjection) {
    let count = projection.world.situations.len();
    let _ = count;

    let sits = &projection.world.situations;
    if sits.is_empty() {
        empty_state(
            ui,
            shell,
            "▣",
            "暂无已登记事态",
            "事态是叙事的骨架：登记一个，把事件串起来",
            "登记事态 →",
        );
    } else {
        for s in sits {
            let meta = crate::ui::situation_panel::format_span_v03(s.span.from, s.span.to);
            list_item(
                ui,
                shell,
                crate::theme::palette_to_color32(crate::theme::colors::ACCENT_EVENT),
                &s.name,
                &meta,
                false,
            );
        }
    }
}
