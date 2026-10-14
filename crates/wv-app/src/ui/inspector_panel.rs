//! 检视器面板（占位）。
//!
//! ## 职责
//! - `panel_inspector` —— 未选中对象时空状态卡（§5.2）。
//!
//! ## 边界（不做什么）
//! - **不**实现真正的属性编辑 —— 选区系统尚未落地（占位卡先到位）。
//!
//! ## 主要导出
//! - `panel_inspector` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::state::{Panel, PanelState};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window};

pub fn panel_inspector(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Inspector,
        None,
        |ui| {
            if empty_state(
                ui,
                &shell,
                "◎",
                "未选中对象",
                "在图上点选地块 / 事件 / 人物后，此处显示其全部属性",
                "用命令面板查找 →",
            ) {
                // 空状态的引导按钮：无副作用的占位（后续接选择系统）。
            }
        },
    );
}
