//! 区域面板（ADR-0017 §2.2）。
//!
//! ## 职责
//! - `panel_region` —— 区域列表（按 ID 升序）；每张卡走 `region_card`。
//! - `region_card` —— 名称编辑 + 当前地图轮廓顶点数 + 删除。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界 —— 改动经 [`crate::edit::region_panel_actions`] 提交 `EditCommand`。
//! - **不**处理几何绘制交互——那是未来「区域绘制工具」的职责（笔刷式 or 点击式）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::edit::feature_place_actions::regenerate_marker_now;
use crate::edit::region_panel_actions;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState, PlaybackState, ViewState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use crate::ui::feature_place_dialog::marker_biome_name;
use crate::ui_shell::ShellNotice;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window};

/// 区域面板。
#[allow(clippy::too_many_arguments)]
pub fn panel_region(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<editing::EditHistory>,
    playback: Res<PlaybackState>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    mut notice: ResMut<ShellNotice>,
    time: Res<Time>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Region,
        None,
        current_loc.0,
        |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(t_key(&current_loc, I18nKey::RegionPanelAddBtn))
                    .clicked()
                {
                    region_panel_actions::add_region_now(
                        &mut projection,
                        &mut edit,
                        &view,
                        t_key(&current_loc, I18nKey::RegionPanelDefaultName).to_owned(),
                    );
                }
                ui.add_space(space::SM);
                ui.label(
                    egui::RichText::new(format!(
                        "{}{}",
                        t_key(&current_loc, I18nKey::RegionPanelCurrentMapPrefix),
                        view.current_map
                    ))
                    .small()
                    .color(shell.t2()),
                );
            });
            ui.add_space(space::SM);
            ui.separator();
            ui.add_space(space::SM);

            // 按 ID 升序展示当前地图下的区域。
            let mut regions: Vec<wv_core::Region> = projection
                .world
                .regions
                .iter()
                .filter(|r| r.map == map_id)
                .cloned()
                .collect();
            regions.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

            if regions.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "⬡",
                    t_key(&current_loc, I18nKey::RegionPanelEmptyTitle),
                    t_key(&current_loc, I18nKey::RegionPanelEmptyHint),
                    t_key(&current_loc, I18nKey::RegionPanelEmptyAction),
                );
                return;
            }

            for r in &regions {
                region_card(
                    ui,
                    &shell,
                    r,
                    &view,
                    &mut projection,
                    &mut edit,
                    &current_loc,
                );
                ui.add_space(space::XS);
            }

            // 生物群系标记（ADR-0029 M2）：列当前图标记 + 逐个重生成边界。
            ui.add_space(space::SM);
            ui.separator();
            ui.add_space(space::SM);
            egui::CollapsingHeader::new(t_key(&current_loc, I18nKey::RegionPanelMarkerSection))
                .default_open(false)
                .show(ui, |ui| {
                    // 先拍 owned 快照：迭代中要 &mut projection 提交命令，
                    // 不能让 &Place 引用跨按钮点击存活。
                    let markers: Vec<(wv_core::Id, String, u8)> =
                        markers_on_map(&projection.world, &map_id)
                            .into_iter()
                            .map(|m| {
                                (
                                    m.id.clone(),
                                    m.name.clone(),
                                    m.biome.unwrap_or(wv_terrain::biome::BIOME_NONE),
                                )
                            })
                            .collect();
                    if markers.is_empty() {
                        ui.label(
                            egui::RichText::new(t_key(
                                &current_loc,
                                I18nKey::RegionPanelMarkerEmpty,
                            ))
                            .small()
                            .color(shell.t2()),
                        );
                        return;
                    }
                    for (id, name, biome_id) in markers {
                        ui.horizontal(|ui| {
                            let biome = marker_biome_name(current_loc.0, biome_id);
                            ui.label(format!("🏷 {name} · {biome}"));
                            if ui
                                .button(t_key(&current_loc, I18nKey::RegionPanelMarkerRegenBtn))
                                .clicked()
                            {
                                let frame = (time.elapsed_secs() * 60.0) as u64;
                                match regenerate_marker_now(
                                    &mut projection,
                                    &mut edit,
                                    &playback,
                                    &mut tex,
                                    id.clone(),
                                ) {
                                    Ok(()) => {}
                                    Err(msg) => notice.error(msg, frame),
                                }
                            }
                        });
                    }
                });
        },
    );
}

/// 当前地图上的生物群系标记（`biome` 非空且锚定本图），按 id 升序。
///
/// 纯函数（只读 `World`）——面板渲染与单测共用；普通地点 / 他图标记不列。
pub(crate) fn markers_on_map<'w>(
    world: &'w wv_core::World,
    map: &wv_core::Id,
) -> Vec<&'w wv_core::distance::Place> {
    let mut markers: Vec<&wv_core::distance::Place> = world
        .distances
        .places
        .iter()
        .filter(|p| p.biome.is_some() && p.map.as_ref() == Some(map))
        .collect();
    markers.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    markers
}

/// 一张区域卡：名称编辑 + 顶点数 + 删除。
#[allow(clippy::too_many_arguments)]
fn region_card(
    ui: &mut egui::Ui,
    shell: &UiShell,
    region: &wv_core::Region,
    view: &ViewState,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    current_loc: &CurrentLocale,
) {
    let id = region.id.clone();
    let vertex_count = region.polygon.len() / 2;
    egui::CollapsingHeader::new(format!(
        "⬡ {} ({}) — {}{}",
        region.name,
        region.id,
        vertex_count,
        t_key(current_loc, I18nKey::FactionPanelPointCountSuffix)
    ))
    .default_open(true)
    .show(ui, |ui| {
        // 名称编辑。
        let mut name_buf = region.name.clone();
        ui.horizontal(|ui| {
            ui.label(t_key(current_loc, I18nKey::RegionPanelNameLabel));
            if ui.text_edit_singleline(&mut name_buf).changed() {
                region_panel_actions::update_region_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    name_buf,
                    region.polygon.clone(),
                );
            }
        });

        // 轮廓信息。
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{}{}{}",
                    t_key(current_loc, I18nKey::RegionPanelOutlinePrefix),
                    vertex_count,
                    t_key(current_loc, I18nKey::RegionPanelOutlineSuffix)
                ))
                .small()
                .color(shell.t2()),
            );
            if region.polygon.is_empty() {
                ui.label(
                    egui::RichText::new(t_key(current_loc, I18nKey::RegionPanelOutlinePlaceholder))
                        .italics()
                        .small()
                        .color(shell.t2()),
                );
            }
        });

        ui.add_space(space::XS);
        if ui
            .button(
                egui::RichText::new(t_key(current_loc, I18nKey::RegionPanelDeleteBtn))
                    .color(egui::Color32::RED),
            )
            .clicked()
        {
            region_panel_actions::remove_region_now(projection, edit, view, id.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::markers_on_map;
    use wv_core::distance::Place;
    use wv_core::Id;

    fn place(id: &str, biome: Option<u8>, map: Option<&str>) -> Place {
        Place {
            id: Id::new_unchecked(id),
            name: id.to_string(),
            feature: None,
            biome,
            envelope: None,
            map: map.map(Id::new_unchecked),
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
        }
    }

    /// 只列「本图 + biome 非空」的标记，按 id 升序；普通地点与他图标记不列。
    #[test]
    fn markers_on_map_lists_only_biome_places_of_current_map() {
        let mut world = wv_core::World::default();
        world.distances.places = vec![
            place("place_zz", Some(1), Some("map_a")),    // 本图标记
            place("place_aa", Some(1), Some("map_a")),    // 本图标记（排序应在前）
            place("place_plain", None, Some("map_a")),    // 普通地点（无 biome）
            place("place_other", Some(1), Some("map_b")), // 他图标记
            place("place_drift", Some(1), None),          // 未锚定图
        ];
        let map = Id::new_unchecked("map_a");
        let got = markers_on_map(&world, &map);
        let ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["place_aa", "place_zz"],
            "应只列本图标记并按 id 升序，实得 {ids:?}"
        );
    }
}
