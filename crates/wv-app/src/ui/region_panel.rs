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
use crate::edit::nature_region_actions;
use crate::edit::region_panel_actions;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{
    NatureRegionUiState, Panel, PanelState, PlaybackState, ViewState, WorldProjection,
};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use crate::ui::cell_pick;
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
    grid: Res<wv_render::state::GridConfig>,
    mut nature_state: ResMut<NatureRegionUiState>,
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

            // 主题支持的覆盖物键（词表由主题拥有，ADR-0045 D10）。
            let theme = &view.active_theme;
            let overlay_keys: Vec<&'static str> = theme.region_overlays.keys().collect();
            // 两套原点的差（区域多边形 = 图内局部 wu；格子属于栅格所在的图）。
            let raster_to_map = (
                grid.map_origin.x - grid.raster_origin.x,
                grid.map_origin.y - grid.raster_origin.y,
            );

            for r in &regions {
                region_card(
                    ui,
                    &shell,
                    r,
                    &view,
                    &mut projection,
                    &mut edit,
                    &current_loc,
                    &overlay_keys,
                    grid.cell_size,
                    raster_to_map,
                    &mut notice,
                    time.elapsed_secs() as u64,
                );
                ui.add_space(space::XS);
            }

            // 自然区域（ADR-0045 D4）：**命名即物化**——自动分区候选不落盘，
            // 作者在光标处点一个候选区、起了名，才生成 regions/<id>.yaml。
            ui.add_space(space::SM);
            ui.separator();
            ui.add_space(space::SM);
            egui::CollapsingHeader::new("自然区域")
                .default_open(true)
                .show(ui, |ui| {
                    let frame = (time.elapsed_secs() * 60.0) as u64;
                    ui.horizontal(|ui| {
                        ui.label("名称");
                        ui.text_edit_singleline(&mut nature_state.name);
                    });
                    // 光标所在格（按 `cell_pick` 口径：栅格原点）。
                    let cursor_cell = view.cursor_world.and_then(|c| {
                        let terrain = projection.world.terrain_of(&map_id)?;
                        cell_pick::pick_at((c.x, c.y), &grid, terrain).map(|p| (p.gx, p.gy))
                    });
                    ui.horizontal(|ui| {
                        match cursor_cell {
                            Some((gx, gy)) => {
                                // 顺带告诉作者「这里是什么」——物化出来的是
                                // 这一整片同群系连通区（不是只有一格）。
                                let what = nature_region_actions::candidate_at(
                                    &projection,
                                    &map_id,
                                    (gx, gy),
                                )
                                .map(|(biome, cells)| format!("群系 {} · {cells} 格", biome))
                                .unwrap_or_else(|| "无候选区".to_string());
                                ui.label(
                                    egui::RichText::new(format!("光标格 ({gx}, {gy}) · {what}"))
                                        .small()
                                        .color(shell.t2()),
                                );
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new("光标不在地形上")
                                        .small()
                                        .color(shell.t2()),
                                );
                            }
                        }
                        let can_materialize =
                            cursor_cell.is_some() && !nature_state.name.trim().is_empty();
                        let btn = ui.add_enabled(can_materialize, egui::Button::new("物化自然区"));
                        if btn.clicked() {
                            let Some((gx, gy)) = cursor_cell else {
                                return;
                            };
                            let name = nature_state.name.trim().to_string();
                            match nature_region_actions::materialize_nature_region_now(
                                &mut projection,
                                &mut edit,
                                &view,
                                name,
                                (gx, gy),
                                grid.cell_size,
                                raster_to_map,
                            ) {
                                Ok(_) => nature_state.name.clear(),
                                Err(msg) => notice.error(msg, frame),
                            }
                        }
                    });
                    ui.label(
                        egui::RichText::new(
                            "自动分区候选不落盘——命名后才生成 regions/<id>.yaml（ADR-0045 D4）。",
                        )
                        .small()
                        .color(shell.t2()),
                    );
                });

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
                            // 不加装饰字形：AGENTS §8 图标一律 Lucide（emoji 跨平台字形不稳）。
                            ui.label(format!("{name} · {biome}"));
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
    overlay_keys: &[&'static str],
    cell_size: f32,
    raster_to_map: (f32, f32),
    notice: &mut ShellNotice,
    frame: u64,
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

        // 类别互转（ADR-0045 D3）：普通区域 ↔ 自然区。
        // 「普通区域」不承载覆盖物（`declared_overlay()` 对 area 一律 None）——
        // 作者想加覆盖物声明，得先转成自然区。
        if ui
            .button(if region.is_nature() {
                "转为普通区域"
            } else {
                "转为自然区（可加覆盖物）"
            })
            .clicked()
        {
            let kind = if region.is_nature() {
                wv_core::model::region::RegionKind::Area
            } else {
                wv_core::model::region::RegionKind::Nature
            };
            let _ = nature_region_actions::set_region_kind_now(
                projection,
                edit,
                view,
                id.clone(),
                kind,
            );
        }

        // 自然区专属（ADR-0045 D2 / D6）：类别 + 覆盖物键 + 重生成边界。
        if region.is_nature() {
            ui.add_space(space::XS);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "自然区 · 覆盖物：{}",
                        region.declared_overlay().unwrap_or("（无声明）")
                    ))
                    .small()
                    .color(shell.t2()),
                );
            });
            // 覆盖物键下拉：词表由主题拥有（ADR-0045 D10）；未登记的键照样能写
            // （开放词表）——但地图上会画成**缺失覆盖物标记**（白底红斜线）。
            let current_key = region.declared_overlay().unwrap_or("").to_string();
            egui::ComboBox::from_id_salt(("region_overlay", id.as_str()))
                .selected_text(if current_key.is_empty() {
                    "（无声明）".to_string()
                } else {
                    current_key.clone()
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(current_key.is_empty(), "（无声明）")
                        .clicked()
                    {
                        let _ = nature_region_actions::set_region_overlay_now(
                            projection,
                            edit,
                            view,
                            id.clone(),
                            None,
                        );
                    }
                    for key in overlay_keys {
                        if ui.selectable_label(current_key == *key, *key).clicked() {
                            let _ = nature_region_actions::set_region_overlay_now(
                                projection,
                                edit,
                                view,
                                id.clone(),
                                Some((*key).to_string()),
                            );
                        }
                    }
                });
            // 键在当前主题无规则 → 提示（地图上以缺失标记标出）。
            if let Some(key) = region.declared_overlay() {
                if !overlay_keys.contains(&key) {
                    ui.label(
                        egui::RichText::new(format!(
                            "键「{key}」在当前主题无规则——地图上以白底红斜线标出。"
                        ))
                        .small()
                        .color(egui::Color32::from_rgb(214, 69, 69)),
                    );
                }
            }
            if ui.button("重生成边界（按当前地形）").clicked() {
                if let Err(msg) = nature_region_actions::regenerate_nature_region_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    cell_size,
                    raster_to_map,
                ) {
                    notice.error(msg, frame);
                }
            }
        }

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
            line: None,
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
