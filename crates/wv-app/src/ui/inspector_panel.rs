//! 检视器面板（v0.3 Figma `5E8bcBCmtGAHqk3ORibBlU` web 实现对齐）。
//!
//! ## 职责
//! - `panel_inspector` —— 未选中对象时显示 web `.empty-inspector` 空态
//!   （虚线圈 + `No object selected` + 提示）；其下恒显 4 条**计数行**
//!   （web `.compact-section`：Situations / Characters / Events / Regions，
//!   右侧计数徽标 + chevron）；Situations 行下列出最多 3 张**列表卡**
//!   （web `.list-card`：3px 色条 + 名称 + 时间跨度）。
//! - 有选中对象时，空态卡替换为选中摘要头（类型色点 + `kind · id`）。
//!
//! ## v0.3-B 变更（web 对齐）
//! - M2 版的 4 个 `CollapsingHeader` 全量列表**退役**：web 设计是紧凑计数行
//!   （`Situations + "2 active" warn 徽标`），不是展开列表；列表卡只给
//!   Situations 段（web 原型同款）。
//!
//! ## 边界
//! - **不**改世界数据 —— 只读 `WorldProjection.selection` 与 `World` 计数。
//! - **不**实现点击行跳转 —— chevron 仅装饰，M3 接选择系统后启用。
//!
//! ## 主要导出
//! - `panel_inspector` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::CurrentLocale;
use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::{color32, proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_command_palette::{CommandId, CommandPalette};
use crate::ui_layout::LayoutFile;
use crate::i18n::{t, I18nKey};

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, BadgeKind, ICON_CHEVRON_RIGHT};
use wv_render::state::EditCellState;
use wv_terrain::biome::{BiomeFamily, family_of};

/// Inspector 面板。
#[allow(clippy::too_many_arguments)]
pub fn panel_inspector(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut projection: ResMut<WorldProjection>,
    mut edit_cell: ResMut<EditCellState>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // 借位记得在闭包前释放 —— render_cell_section 需要 `&mut projection.world`,
    // 而 `projection.selection` 这一行会持续借用整个 `projection` 到方法末尾。
    // 走 clone 释放借用。
    let selection_clone = projection.selection.clone();
    let selected = selection_clone.as_ref();

    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Inspector,
        None,
        current_loc.0,
        |ui| {
            match selected {
                None => render_empty(ui, &shell),
                Some(sel) => render_selection_header(ui, &shell, sel),
            }
            // 4 条计数行（web `.compact-section`）+ Situations 列表卡。
            compact_row(
                ui,
                &shell,
                "Situations",
                (
                    BadgeKind::Warn,
                    format!("{} active", projection.world.situations.len()),
                ),
            );
            situation_cards(ui, &shell, &projection);
            compact_row(
                ui,
                &shell,
                "Characters",
                (
                    BadgeKind::Neutral,
                    format!("{}", projection.world.characters.len()),
                ),
            );
            compact_row(
                ui,
                &shell,
                "Events",
                (
                    BadgeKind::Event,
                    format!("{}", projection.world.events.len()),
                ),
            );
            compact_row(
                ui,
                &shell,
                "Regions",
                (
                    BadgeKind::Neutral,
                    format!("{}", projection.world.distances.places.len()),
                ),
            );

            // 单格编辑折叠区([ADR-0040])。仅 EditCell 工具激活且
            // 选了一格才显示。把 surface/height/biome 三字段做成
            // 真正的可改 widget —— 改完字段 → 直接构造 `EditCellCommand`
            // 走 `History::commit`（经 `commit_stroke_on_release` 同源路径）。
            if edit_cell.tool_active {
                if let Some((gx, gy)) = edit_cell.selected {
                    render_cell_section(
                        ui,
                        &shell,
                        &mut projection,
                        &mut edit_cell,
                        &mut edit,
                        (gx, gy),
                        current_loc.0,
                    );
                }
            }
        },
    );
}

/// web `.empty-inspector`：虚线圆圈 + `No object selected` + 提示。
fn render_empty(ui: &mut egui::Ui, shell: &UiShell) {
    let _ = empty_state(
        ui,
        shell,
        "◎",
        "No object selected",
        "Click a map plot, event, or character\nto inspect properties.",
        "",
    );
}

/// 选中摘要头：类型色点 + `kind · id`（替换空态卡的位置）。
fn render_selection_header(ui: &mut egui::Ui, shell: &UiShell, sel: &wv_core::SelectedEntity) {
    ui.horizontal(|ui| {
        let dot_color = match sel.kind {
            wv_core::EntityKind::Place => shell.accent(),
            wv_core::EntityKind::Region => shell.gold(),
            wv_core::EntityKind::Faction => shell.danger(),
            wv_core::EntityKind::Event => shell.success(),
            wv_core::EntityKind::StreetSegment => shell.t3(),
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, dot_color);
        ui.label(
            egui::RichText::new(format!("{} · {}", kind_label(sel.kind), sel.id.as_str()))
                .size(shell.scale.small)
                .color(shell.t1())
                .strong(),
        );
    });
    ui.add_space(space::SM);
}

/// web `.compact-section`：高 29、raised 底、圆角 5：名称 + 计数徽标 + chevron。
fn compact_row(ui: &mut egui::Ui, shell: &UiShell, label: &str, count: (BadgeKind, String)) {
    let row_h = 29.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, 5.0, shell.raised_bg());
    // 名称（web `.compact-section > span:first-child`：600 weight）
    painter.text(
        egui::pos2(rect.min.x + 7.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.base),
        shell.t1(),
    );
    // 徽标与 chevron 的几何先算好（painter 借用随作用域结束，再 ui.put 图标）
    let (kind, text) = count;
    let bc = shell.palette.badge(match kind {
        BadgeKind::Neutral => crate::theme::ui_tokens::BadgeTone::Neutral,
        BadgeKind::Warn => crate::theme::ui_tokens::BadgeTone::Warn,
        BadgeKind::Event => crate::theme::ui_tokens::BadgeTone::Event,
        BadgeKind::Ok => crate::theme::ui_tokens::BadgeTone::Ok,
        BadgeKind::Err => crate::theme::ui_tokens::BadgeTone::Err,
    });
    let pill_w = 30.0 + text.chars().count() as f32 * 6.0;
    let pill = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 25.0 - pill_w * 0.5, rect.center().y),
        egui::vec2(pill_w, 16.0),
    );
    painter.rect_filled(
        pill,
        radius::BADGE,
        color32(bc.background_on(shell.palette.bg_panel)),
    );
    painter.text(
        pill.center(),
        egui::Align2::CENTER_CENTER,
        text,
        proportional(shell.scale.micro),
        color32(bc.fg),
    );
    // chevron（最右，t3）—— painter 借用已随上文最后使用结束，可安全 ui.put
    let chevron_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 13.0, rect.center().y),
        egui::vec2(12.0, 12.0),
    );
    let _ = ui.put(
        chevron_rect,
        egui::Image::new(ICON_CHEVRON_RIGHT)
            .tint(shell.t3())
            .fit_to_exact_size(egui::vec2(12.0, 12.0)),
    );
}

/// Situations 列表卡（web `.list-card`：3px 色条 + 名称 + 跨度 meta）。
/// 最多 3 张（计数行已给总数，卡是"最近关注"预览）。
fn situation_cards(ui: &mut egui::Ui, shell: &UiShell, projection: &WorldProjection) {
    for s in projection.world.situations.iter().take(3) {
        let card_h = 42.0;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), card_h),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        // 卡底：raised 60% + border 描边（web `color-mix(raised 60%)`）
        painter.rect_filled(rect, 6.0, shell.raised_bg().linear_multiply(0.6));
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, shell.border_subtle()),
            egui::StrokeKind::Inside,
        );
        // 3px 事件色条（web `.color-bar`：stretch、圆角 4）
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 6.0, rect.min.y + 6.0),
            egui::pos2(rect.min.x + 9.0, rect.max.y - 6.0),
        );
        painter.rect_filled(bar, 4.0, shell.danger());
        // 名称 + meta
        painter.text(
            egui::pos2(rect.min.x + 17.0, rect.center().y - 7.0),
            egui::Align2::LEFT_CENTER,
            &s.name,
            proportional(shell.scale.micro),
            shell.t1(),
        );
        painter.text(
            egui::pos2(rect.min.x + 17.0, rect.center().y + 7.0),
            egui::Align2::LEFT_CENTER,
            format_span(s.span.from, s.span.to),
            proportional(shell.scale.micro),
            shell.t3(),
        );
    }
}

/// `Y214 D40 — Y215 D12` / `Y214 D82 — …`（360 天/年简化口径，M3 接历法）。
fn format_span(from: i64, to: Option<i64>) -> String {
    let (fy, fd) = year_day(from);
    match to {
        Some(t) => {
            let (ty, td) = year_day(t);
            format!("Y{fy} D{fd} — Y{ty} D{td}")
        }
        None => format!("Y{fy} D{fd} — …"),
    }
}

fn year_day(day: i64) -> (i32, i32) {
    (((day - 1) / 360 + 1) as i32, ((day - 1) % 360 + 1) as i32)
}

fn kind_label(k: wv_core::EntityKind) -> &'static str {
    match k {
        wv_core::EntityKind::Place => "Place",
        wv_core::EntityKind::Region => "Region",
        wv_core::EntityKind::Faction => "Faction",
        wv_core::EntityKind::StreetSegment => "Segment",
        wv_core::EntityKind::Event => "Event",
    }
}

// ── 测试 ────────────────────────────────────────────────────────────
//
// `panel_inspector` 是 Bevy 系统 + egui 渲染，集成测试要起 App 太重；
// 单元测试只覆盖纯函数，渲染覆盖由 smoke-gui + 人工验收承担。

/// 单格编辑折叠区([ADR-0040] §决策 6 Inspector Panel)。
#[allow(clippy::too_many_arguments)]
fn render_cell_section(
    ui: &mut egui::Ui,
    _shell: &UiShell,
    projection: &mut WorldProjection,
    edit_cell: &mut EditCellState,
    edit: &mut crate::edit::editing::EditHistory,
    (gx, gy): (i32, i32),
    loc: crate::i18n::Locale,
) {
    let _ = _shell;
    let Some(target_map) = edit_cell.last_map_id.clone() else {
        return;
    };
    // 借用 `'1`:`&WorldProjection`(投影 `world` 的 `terrain_of`) + 索引 → before;
    // borrow 在本块结束就放掉,闭包内才能 `&mut projection.world` 提交。
    let before = {
        let Some(terrain) = projection.world.terrain_of(&target_map) else {
            return;
        };
        let w = terrain.width as i32;
        let h = terrain.height as i32;
        if gx < 0 || gy < 0 || gx >= w || gy >= h {
            return;
        }
        let idx = (gy as usize) * (w as usize) + (gx as usize);
        terrain.cells.get(idx).copied()
    };
    let Some(before) = before else {
        return;
    };

    ui.collapsing(t(loc, I18nKey::CellInspectorSection), |ui| {
        ui.label(format!("{} ({}, {})", t(loc, I18nKey::CellInspectorCoord), gx, gy));

        let mut new_surface = before.surface;
        ui.horizontal(|ui| {
            ui.label(t(loc, I18nKey::CellInspectorSurface));
            if ui
                .selectable_label(new_surface == wv_terrain::SURFACE_WATER, "水 (0)")
                .clicked()
            {
                new_surface = wv_terrain::SURFACE_WATER;
            }
            if ui
                .selectable_label(new_surface == wv_terrain::SURFACE_LAND, "陆 (1)")
                .clicked()
            {
                new_surface = wv_terrain::SURFACE_LAND;
            }
        });

        let mut new_height = before.height;
        ui.horizontal(|ui| {
            ui.label(t(loc, I18nKey::CellInspectorHeight));
            ui.add(egui::DragValue::new(&mut new_height).speed(1.0).suffix(" m"));
        });

        let mut new_biome = before.biome;
        ui.horizontal(|ui| {
            ui.label(t(loc, I18nKey::CellInspectorBiome));
            egui::ComboBox::new("cell-biome", "")
                .selected_text(biome_label(before.biome))
                .show_ui(ui, |ui| {
                    if ui.selectable_label(new_biome == 0, "<NONE>").clicked() {
                        new_biome = 0;
                    }
                    for family in [
                        BiomeFamily::Forest,
                        BiomeFamily::Grassland,
                        BiomeFamily::Desert,
                        BiomeFamily::Wetland,
                        BiomeFamily::Cold,
                        BiomeFamily::Highland,
                        BiomeFamily::Water,
                        BiomeFamily::Road,
                    ] {
                        ui.separator();
                        ui.label(format!("{:?}", family));
                        for &(num, name) in biomes_in_family(family) {
                            if ui
                                .selectable_label(new_biome == num, format!("{name} ({num})"))
                                .clicked()
                            {
                                new_biome = num;
                            }
                        }
                    }
                });
        });

        let after = wv_core::terrain::TerrainCellData {
            height: new_height,
            surface: new_surface,
            biome: new_biome,
        };
        if after != before {
            push_cell_edit(edit, &mut projection.world, &target_map, (gx, gy), before, after);
        }
    });
}

/// 单格编辑提交到 `wv_edit::History`(`EditCellCommand::apply`)——
/// 走 `region_panel_actions::push_cmd` 同款 4 步模式(snapshot-diff-revert)。
#[allow(clippy::too_many_arguments)]
fn push_cell_edit(
    edit: &mut crate::edit::editing::EditHistory,
    world: &mut wv_core::World,
    map: &wv_core::id::Id,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    after: wv_core::terrain::TerrainCellData,
) {
    // Stale-snapshot 校验。
    let current = world.terrain_of(map).and_then(|t| {
        let w = t.width as i32;
        let h = t.height as i32;
        if gx < 0 || gy < 0 || gx >= w || gy >= h {
            return None;
        }
        t.cells
            .get((gy as usize) * (w as usize) + (gx as usize))
            .copied()
    });
    if current != Some(before) {
        bevy::log::warn!(
            "EditCell commit 拒收: cell ({gx}, {gy}) 当前值已不是你 Inspector 上看到的值"
        );
        return;
    }
    let cmd = wv_edit::command::EditCellCommand::new(map.clone(), gx, gy, before, after);
    use wv_edit::EditCommand;
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
        }
        Err(e) => {
            bevy::log::warn!("EditCell commit 失败: {e}");
        }
    }
}

/// 静态映射(`BiomeFamily` 内全部编号 + 名称,冻结表)。
fn biomes_in_family(family: BiomeFamily) -> &'static [(u8, &'static str)] {
    match family {
        BiomeFamily::Forest => &[(1, "Forest"), (10, "Jungle"), (11, "Tropical Rainforest")],
        BiomeFamily::Grassland => &[
            (2, "Plains"),
            (3, "Grassland"),
            (17, "Oasis"),
            (18, "Savanna"),
        ],
        BiomeFamily::Desert => &[(4, "Gobi"), (5, "Desert"), (19, "Salt Flat")],
        BiomeFamily::Wetland => &[(6, "Swamp"), (7, "Wetland")],
        BiomeFamily::Cold => &[(8, "Tundra"), (9, "Snowfield")],
        BiomeFamily::Highland => &[(12, "Hills"), (13, "Mountains")],
        BiomeFamily::Water => &[(14, "Ocean"), (15, "Lake"), (16, "River")],
        BiomeFamily::Road => &[(20, "Road")],
    }
}

fn biome_label(num: u8) -> String {
    if let Some(family) = family_of(num) {
        for &(n, name) in biomes_in_family(family) {
            if n == num {
                return format!("{name} ({num})");
            }
        }
    }
    if num == 0 {
        return "<NONE>".into();
    }
    format!("#{num}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_label_maps_all_five_kinds() {
        assert_eq!(kind_label(wv_core::EntityKind::Place), "Place");
        assert_eq!(kind_label(wv_core::EntityKind::Region), "Region");
        assert_eq!(kind_label(wv_core::EntityKind::Faction), "Faction");
        assert_eq!(kind_label(wv_core::EntityKind::Event), "Event");
        assert_eq!(kind_label(wv_core::EntityKind::StreetSegment), "Segment");
    }

    #[test]
    fn year_day_matches_timeline_360_day_year() {
        // 第 1 天 → Y1 D1
        assert_eq!(year_day(1), (1, 1));
        // 第 360 天 → Y1 D360
        assert_eq!(year_day(360), (1, 360));
        // 第 361 天 → Y2 D1
        assert_eq!(year_day(361), (2, 1));
    }

    #[test]
    fn format_span_renders_both_bounds_or_open_end() {
        assert_eq!(format_span(14441, Some(14445)), "Y41 D41 — Y41 D45");
        assert_eq!(format_span(14441, None), "Y41 D41 — …");
    }

    #[test]
    fn header_label_includes_kind_and_id() {
        let sel = wv_core::SelectedEntity::new(
            wv_core::EntityKind::Place,
            wv_core::id::Id::new_unchecked("city_zunyi"),
        );
        assert_eq!(sel.kind, wv_core::EntityKind::Place);
        assert_eq!(sel.id.as_str(), "city_zunyi");
        assert_eq!(
            format!("{} · {}", kind_label(sel.kind), sel.id.as_str()),
            "Place · city_zunyi"
        );
    }
}
