//! 图层管理 + 世界树。
//!
//! ## 职责
//! - `panel_layers` —— 地形 / 底图淡化 / 地点 / 事件点 / 轨迹 / 道路 / 势力 开关。
//! - `panel_map_tree` —— 当前地图占位卡 + 层级提示（P-1）。
//!
//! ## 边界（不做什么）
//! - **不**画世界树树状结构 —— 占位卡先到位；树状交互由后续任务推进。
//!
//! ## 主要导出
//! - `panel_layers` / `panel_map_tree` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{Panel, PanelState, ViewState};
use crate::ui_layout::LayoutFile;
use crate::ui_theme::UiShellConfig;
use crate::ui_tokens::space;

use super::shell::{terrain_swatch, UiShell};
use super::widgets::{float_window, list_item};

/// 世界树（P-1）。
pub fn panel_map_tree(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
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
        Panel::MapTree,
        None,
        |ui| {
            let resp = list_item(
                ui,
                &shell,
                terrain_swatch(3),
                &view.current_map,
                "世界地图",
                true,
            );
            let _ = resp;
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("World → Region → City → Interior")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
        },
    );
}

/// 图层管理（P-2）。
pub fn panel_layers(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
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
        Panel::Layers,
        None,
        |ui| {
            ui.checkbox(&mut view.terrain_visible, "地形");
            // 底图淡化：把瓦片图集向纸面色拉，让路线/地标压得住底图。
            ui.checkbox(&mut view.terrain_soften, "底图淡化");
            ui.checkbox(&mut view.places_visible, "地点");
            ui.checkbox(&mut view.events_visible, "事件点");
            ui.checkbox(&mut view.tracks_visible, "轨迹");
            ui.checkbox(&mut view.roads_visible, "道路");
            ui.checkbox(&mut view.factions_visible, "势力");
        },
    );
}
