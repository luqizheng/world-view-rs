//! 势力层（P-7）。
//!
//! ## 职责
//! - `panel_faction` —— 势力列表（按 ID 升序）；每张卡走 `faction_card`。
//! - `faction_card` —— 名称 / 颜色编辑 + 当前地图几何列表（增 / 删）。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界 —— 改动经 [`crate::faction_panel_actions`] 提交 `EditCommand`。
//!
//! ## 主要导出
//! - `panel_faction` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::edit::faction_panel_actions;
use crate::i18n::CurrentLocale;
use crate::state::{Panel, PanelState, PlaybackState, ViewState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window};

/// 势力层（P-7）。
#[allow(clippy::too_many_arguments)]
pub fn panel_faction(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<editing::EditHistory>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let map_id = wv_core::id::Id::new_unchecked(view.current_map.clone());
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Faction,
        None,
        current_loc.0,
        |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ 新建势力").clicked() {
                    faction_panel_actions::add_faction_now(
                        &mut projection,
                        &mut edit,
                        &view,
                        "新势力".into(),
                        wv_core::color::Color("#1E6FCC".into()),
                    );
                }
                ui.add_space(space::SM);
                ui.label(
                    egui::RichText::new(format!("当前地图：{}", view.current_map))
                        .small()
                        .color(shell.t2()),
                );
            });
            ui.add_space(space::SM);
            ui.separator();
            ui.add_space(space::SM);

            // 复制出 Vec 以便可变性借出（按 ID 升序展示，便于稳定定位）。
            let mut factions: Vec<wv_core::Faction> = projection.world.factions.clone();
            factions.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

            if factions.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "◆",
                    "尚未创建势力",
                    "在工程目录下编辑 factions/*.yaml，或点击上方「+ 新建势力」",
                    "查看势力层说明 →",
                );
                return;
            }

            for f in &factions {
                faction_card(
                    ui,
                    &shell,
                    f,
                    &view,
                    &playback,
                    &map_id,
                    &mut projection,
                    &mut edit,
                );
                ui.add_space(space::XS);
            }
        },
    );
}

/// 一张势力卡：名称/色编辑 + 当前地图下的几何列表（增/删）。
#[allow(clippy::too_many_arguments)]
fn faction_card(
    ui: &mut egui::Ui,
    shell: &UiShell,
    faction: &wv_core::Faction,
    view: &ViewState,
    playback: &PlaybackState,
    map_id: &wv_core::id::Id,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
) {
    let id = faction.id.clone();
    egui::CollapsingHeader::new(format!("◆ {} ({})", faction.name, faction.id))
        .default_open(true)
        .show(ui, |ui| {
            // 名称 / 颜色（用本地副本 commit，避免可变借用互相打架）。
            let mut name_buf = faction.name.clone();
            let mut color_buf = faction.color.as_str().to_string();
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("名称：");
                if ui.text_edit_singleline(&mut name_buf).changed() {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("颜色：");
                if ui.text_edit_singleline(&mut color_buf).changed() {
                    changed = true;
                }
            });
            if changed {
                let color = wv_core::color::Color(color_buf.clone());
                faction_panel_actions::update_faction_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    name_buf,
                    color,
                );
            }

            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!("当前地图几何：{}", view.current_map))
                    .small()
                    .color(shell.t2()),
            );
            // 几何列表：仅展示当前地图下的。
            let geos: Vec<wv_core::map::GeometrySpan> = projection
                .world
                .faction_geometry
                .iter()
                .filter(|g| g.map == *map_id && g.owner == id)
                .cloned()
                .collect();
            if geos.is_empty() {
                ui.label(
                    egui::RichText::new("（无几何）")
                        .italics()
                        .small()
                        .color(shell.t2()),
                );
            } else {
                for g in &geos {
                    let from = g.span.from;
                    ui.horizontal(|ui| {
                        ui.label(format!("[{}, {})", from, g.span.to.unwrap_or(from + 1)));
                        ui.label(format!("{} 顶点", g.geometry.point_count()));
                        if ui.small_button("删除").clicked() {
                            faction_panel_actions::remove_geometry_now(
                                projection,
                                edit,
                                view,
                                id.clone(),
                                from,
                            );
                        }
                    });
                }
            }
            // 新增几何。
            ui.horizontal(|ui| {
                let mut from_buf = playback.current_day.to_string();
                let mut to_buf = "".to_string();
                ui.label("起 day：");
                if ui.text_edit_singleline(&mut from_buf).changed() {
                    // 改了起日就停掉按钮的焦点闪烁问题——面板每帧重画，自动 commit。
                }
                ui.label("止（留空=至今）：");
                ui.text_edit_singleline(&mut to_buf);
                if ui.button("+ 新建疆界").clicked() {
                    let from: i64 = from_buf.trim().parse().unwrap_or(playback.current_day);
                    let to: Option<i64> = if to_buf.trim().is_empty() {
                        None
                    } else {
                        to_buf.trim().parse().ok()
                    };
                    faction_panel_actions::upsert_geometry_now(
                        projection,
                        edit,
                        view,
                        id.clone(),
                        from,
                        to,
                    );
                }
            });

            ui.add_space(space::XS);
            if ui
                .button(egui::RichText::new("删除整个势力").color(egui::Color32::RED))
                .clicked()
            {
                faction_panel_actions::remove_faction_now(projection, edit, view, id.clone());
            }
        });
}
