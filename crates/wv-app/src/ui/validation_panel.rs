//! 校验面板（P-8）。
//!
//! ## 职责
//! - `panel_validation` —— 顶部状态条（错误/提示计数）+ 错误列表 + 提示列表，
//!   每行带 anchor 可点击跳转。
//!
//! ## 边界（不做什么）
//! - **不**重新派生 diagnostics —— 见 [`crate::diagnostics_bridge`] 与 [`crate::state::ProjectDiagnostics`]。
//! - **不**阻止保存 —— 校验只提示（AGENTS §7.5 V1）。
//!
//! ## 主要导出
//! - `panel_validation` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::validation_actions;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{
    Panel, PanelState, PlaybackState, ProjectDiagnostics, ScaleHintCache, ViewState,
    WorldProjection,
};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{badge, float_window, BadgeKind};

// ---------------------------------------------------------------------------
// 世界图尺度提示的参考载具（README §12-39）
// ---------------------------------------------------------------------------

/// 参考载具的施工模板：只填所需字段，其余默认。
///
/// 为什么是函数而不是 `const`：`Vehicle` 含 `Id` / `String` / `HashMap`，
/// 无法 const 构造（计划原文写 `const REF_WALK: Vehicle`，见 ledger Ruling）。
fn ref_vehicle(
    id: &str,
    name: &str,
    speed_kmh: f32,
    daily_hours: f32,
) -> wv_core::map::distance::Vehicle {
    wv_core::map::distance::Vehicle {
        id: wv_core::Id::new_unchecked(id),
        name: name.to_owned(),
        instant: false,
        speed_kmh: Some(speed_kmh),
        daily_hours: Some(daily_hours),
        // 空 modifiers = 任何地形 ×1.0（平原 / 海面默认）。
        terrain_modifiers: std::collections::HashMap::new(),
        requires_endpoints: false,
        requirements: Vec::new(),
    }
}

/// 参考：步行（5 km/h × 8 h/日）。
pub(crate) fn ref_walk() -> wv_core::map::distance::Vehicle {
    ref_vehicle("ref_walk", "步行", 5.0, 8.0)
}

/// 参考：骑马（8 km/h × 8 h/日）。
pub(crate) fn ref_horse() -> wv_core::map::distance::Vehicle {
    ref_vehicle("ref_horse", "骑马", 8.0, 8.0)
}

/// 参考：帆船（10 km/h × 12 h/日）。
pub(crate) fn ref_sail() -> wv_core::map::distance::Vehicle {
    ref_vehicle("ref_sail", "帆船", 10.0, 12.0)
}

/// 天数显示：< 10 天保留两位小数，较大时取整（「约」字已说明是粗估）。
fn format_days(v: &wv_core::map::distance::Vehicle, km: f32, terrain: &str) -> String {
    match v.segment_days(km, terrain) {
        Some(d) if d < 10.0 => format!("{d:.2}"),
        Some(d) => format!("{d:.0}"),
        None => "—".to_string(),
    }
}

/// 世界图尺度只读提示的三行文案（README §12-39）。
///
/// 返回 `None` 的条件：无当前图 / 无 `unit_per_km` / 无地形 / 无陆地——
/// 缺数据是正常态，面板静默（V1 只提示）。
///
/// 图幅优先取 `MapNode.extent`；未声明时退回栅格尺寸。
fn scale_hint_texts(
    world: &wv_core::World,
    map_id: &wv_core::Id,
    cell_size: f32,
    loc: crate::i18n::Locale,
) -> Option<[String; 3]> {
    let node = world.maps.iter().find(|m| &m.id == map_id)?;
    let per_km = node.unit_per_km.filter(|p| *p > 0.0)?;
    let data = world.terrain_of(map_id)?;
    let (extent_w, extent_h) = node.extent.map(|e| (e.width, e.height)).unwrap_or((
        data.width as f32 * cell_size,
        data.height as f32 * cell_size,
    ));
    let extent_w_km = extent_w / per_km;
    let extent_h_km = extent_h / per_km;

    let land = wv_core::map::scale_hint::land_extent_km(world, map_id, cell_size)?;
    let walk = format_days(&ref_walk(), land.diagonal_km, "plain");
    let horse = format_days(&ref_horse(), land.diagonal_km, "plain");
    let sail = format_days(&ref_sail(), land.diagonal_km, "sea");

    let extent_line = crate::i18n::t(loc, I18nKey::ScaleHintExtent)
        .replace("{w}", &format!("{extent_w_km:.1}"))
        .replace("{h}", &format!("{extent_h_km:.1}"));
    let land_line = crate::i18n::t(loc, I18nKey::ScaleHintLand)
        .replace("{w}", &format!("{:.1}", land.width_km))
        .replace("{h}", &format!("{:.1}", land.height_km));
    let crossing_line = crate::i18n::t(loc, I18nKey::ScaleHintCrossing)
        .replace("{a}", &walk)
        .replace("{b}", &horse)
        .replace("{c}", &sail);
    Some([extent_line, land_line, crossing_line])
}

/// 校验（P-8）。
#[allow(clippy::too_many_arguments)]
pub fn panel_validation(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    diagnostics: Res<ProjectDiagnostics>,
    projection: Res<WorldProjection>,
    mut scale_hint: ResMut<ScaleHintCache>,
    mut drill: ResMut<crate::state::DrillState>,
    mut view: ResMut<ViewState>,
    mut playback: ResMut<PlaybackState>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    current_loc: Res<CurrentLocale>,
    grid: Res<crate::state::GridConfig>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let d = &diagnostics.diagnostics;
    let n_errors = d.errors().len();
    let n_hints = d.hints().len();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Validation,
        None,
        current_loc.0,
        |ui| {
            // 顶部状态条：错误用 Err 徽标，提示用 Warn 徽标。
            if n_errors > 0 {
                badge(
                    ui,
                    &shell,
                    BadgeKind::Err,
                    &format!(
                        "{}{n_errors}",
                        t_key(&current_loc, I18nKey::ValidationBadgeErrCountPrefix)
                    ),
                );
            } else {
                badge(
                    ui,
                    &shell,
                    BadgeKind::Ok,
                    t_key(&current_loc, I18nKey::ValidationBadgeErrNone),
                );
            }
            ui.add_space(space::SM);
            badge(
                ui,
                &shell,
                BadgeKind::Warn,
                &format!(
                    "{}{n_hints}",
                    t_key(&current_loc, I18nKey::ValidationBadgeWarnCountPrefix)
                ),
            );
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(t_key(&current_loc, I18nKey::ValidationSubtitle))
                    .size(shell.scale.small)
                    .color(shell.t2()),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(t_key(&current_loc, I18nKey::ValidationFootnote))
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
            // 世界图尺度只读提示（README §12-39）：有 unit_per_km 且有陆地才出现。
            //
            // 缓存 + 变更检测（重构 P0-3）：`scale_hint_texts` 会全量扫描地形栅格
            // 求陆地包围盒（512×288 ≈ 14.7 万格）。只在「换图 / 改 cell_size」
            // 或世界被写过（`is_changed`）时重算——面板常开不再每帧白扫。
            if let Some(map_id) = drill.current().cloned() {
                let key = (map_id.clone(), grid.cell_size.to_bits());
                if scale_hint.key.as_ref() != Some(&key) || projection.is_changed() {
                    scale_hint.lines =
                        scale_hint_texts(&projection.world, &map_id, grid.cell_size, current_loc.0);
                    scale_hint.key = Some(key);
                }
                if let Some(lines) = &scale_hint.lines {
                    ui.add_space(space::SM);
                    for line in lines {
                        ui.label(
                            egui::RichText::new(line)
                                .size(shell.scale.small)
                                .color(shell.t2()),
                        );
                    }
                }
            }
            ui.add_space(space::SM);
            // 错误列表（按指纹 dedup 后仍可能有 N 条同类）。
            if n_errors > 0 {
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::ValidationSectionErrors))
                        .size(shell.scale.small)
                        .strong()
                        .color(shell.t1()),
                );
                ui.add_space(space::XS);
                egui::ScrollArea::vertical()
                    .max_height(shell.row * 6.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for err in d.errors() {
                            ui.horizontal(|ui| {
                                badge(ui, &shell, BadgeKind::Err, "E");
                                ui.add_space(space::XS);
                                // 阶段八 §A 延展：错误行与提示行**同形**——
                                // 有 anchor 就可点。错误**不**阻止保存（V1 守约），
                                // 但「让作者能跳过去看一眼」是与提示同级的体面。
                                if let Some(anchor) = &err.anchor {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(&err.message)
                                                    .size(shell.scale.small)
                                                    .color(shell.t1()),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        validation_actions::jump_to_anchor_now(
                                            &projection,
                                            &mut drill,
                                            &mut view,
                                            &mut playback,
                                            &mut cameras,
                                            anchor,
                                        );
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&err.message)
                                            .size(shell.scale.small)
                                            .color(shell.t1()),
                                    );
                                }
                            });
                        }
                    });
                ui.add_space(space::SM);
            }
            // 提示列表（相遇提示 / 事件位移可行性等）。
            if n_hints > 0 {
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::ValidationSectionHints))
                        .size(shell.scale.small)
                        .strong()
                        .color(shell.t1()),
                );
                ui.add_space(space::XS);
                egui::ScrollArea::vertical()
                    .max_height(shell.row * 8.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for h in d.hints() {
                            ui.horizontal(|ui| {
                                badge(ui, &shell, BadgeKind::Warn, "H");
                                ui.add_space(space::XS);
                                // 阶段八 §A：hint 行变可点。点击触发跳转
                                // (drill_to + camera + day)。无 anchor 的提示
                                // （如 `fingerprint` 派生但还没几何）退化为普通 label。
                                if let Some(anchor) = &h.anchor {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(&h.message)
                                                    .size(shell.scale.small)
                                                    .color(shell.t1()),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        validation_actions::jump_to_anchor_now(
                                            &projection,
                                            &mut drill,
                                            &mut view,
                                            &mut playback,
                                            &mut cameras,
                                            anchor,
                                        );
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&h.message)
                                            .size(shell.scale.small)
                                            .color(shell.t1()),
                                    );
                                }
                            });
                        }
                    });
            } else if n_errors == 0 {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::ValidationOkAll))
                        .size(shell.scale.small)
                        .color(shell.t3()),
                );
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考载具的日速（手算，AGENTS §7.2/§7.4）。
    ///
    /// `Vehicle::effective_daily_speed` = `speed_kmh × daily_hours`（ADR-0024
    /// 单位修正后）。5 km/h × 8 h = 40 km/日，与计划期望一致。
    #[test]
    fn reference_vehicles_daily_speeds_match_hand_computed_values() {
        let walk = ref_walk()
            .effective_daily_speed("plain")
            .expect("步行应有日速");
        let horse = ref_horse()
            .effective_daily_speed("plain")
            .expect("骑马应有日速");
        let sail = ref_sail()
            .effective_daily_speed("sea")
            .expect("帆船应有日速");
        assert!((walk - 40.0).abs() < 1e-6, "步行得 {walk}");
        assert!((horse - 64.0).abs() < 1e-6, "骑马得 {horse}");
        assert!((sail - 120.0).abs() < 1e-6, "帆船得 {sail}");
    }

    /// 三种参考载具都可用（非 instant、参数齐全）。
    #[test]
    fn reference_vehicles_are_not_instant_and_have_params() {
        for v in [ref_walk(), ref_horse(), ref_sail()] {
            assert!(!v.instant, "参考载具不得是瞬时的");
            assert!(v.speed_kmh.is_some(), "{} 缺 speed_kmh", v.id);
            assert!(v.daily_hours.is_some(), "{} 缺 daily_hours", v.id);
        }
    }

    // -----------------------------------------------------------------------
    // 世界图尺度只读提示（README §12-39）
    // -----------------------------------------------------------------------

    fn scale_world(unit_per_km: Option<f32>, with_land: bool) -> (wv_core::World, wv_core::Id) {
        let map = wv_core::Id::new_unchecked("map_world");
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            name: "世界".into(),
            level: wv_core::map::MapLevel::World,
            extent: Some(wv_core::map::Extent::sized(320.0, 320.0)),
            unit_per_km,
            ..Default::default()
        });
        let mut cells = vec![
            wv_core::terrain::TerrainCellData {
                height: -3500,
                surface: 0,
                biome: 0,
            };
            40 * 40
        ];
        if with_land {
            for y in 0..10 {
                for x in 0..20 {
                    cells[y * 40 + x] = wv_core::terrain::TerrainCellData {
                        height: 500,
                        surface: 1,
                        biome: 3,
                    };
                }
            }
        }
        world.set_terrain(
            map.clone(),
            wv_core::terrain::TerrainData {
                width: 40,
                height: 40,
                cells,
            },
        );
        (world, map)
    }

    /// 缺 `unit_per_km` / 无陆地 → 整段静默（V1：只提示，缺数据是正常态）。
    #[test]
    fn scale_hint_rows_hidden_without_unit_per_km_or_land() {
        let loc = crate::i18n::Locale::Zh;
        let (world, map) = scale_world(None, true);
        assert_eq!(
            scale_hint_texts(&world, &map, 8.0, loc),
            None,
            "缺 upk 应静默"
        );
        let (world, map) = scale_world(Some(100.0), false);
        assert_eq!(
            scale_hint_texts(&world, &map, 8.0, loc),
            None,
            "无陆地应静默"
        );
    }

    /// 有数据时三行文案含手算值（20×10 陆格、cell 8、upk 100）。
    #[test]
    fn scale_hint_rows_show_hand_computed_km_and_days() {
        let loc = crate::i18n::Locale::Zh;
        let (world, map) = scale_world(Some(100.0), true);
        let lines = scale_hint_texts(&world, &map, 8.0, loc).expect("应有提示");
        // 图幅 320×320 wu / 100 = 3.2×3.2 km。
        assert!(
            lines[0].contains("3.2×3.2 km"),
            "图幅行应含 3.2×3.2 km，得 {}",
            lines[0]
        );
        // 陆地 20×8/100=1.6 km × 10×8/100=0.8 km。
        assert!(
            lines[1].contains("1.6×0.8 km"),
            "陆地行应含 1.6×0.8 km，得 {}",
            lines[1]
        );
        // 对角 √(1.6²+0.8²)=1.7889 km；日速 40/64/120 km/日（ADR-0024）
        // → 0.04 / 0.03 / 0.01 天。
        assert!(
            lines[2].contains("0.04") && lines[2].contains("0.03") && lines[2].contains("0.01"),
            "横穿行应含手算天数 0.04/0.03/0.01，得 {}",
            lines[2]
        );
    }
}
