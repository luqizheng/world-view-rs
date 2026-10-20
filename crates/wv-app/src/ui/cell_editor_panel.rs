//! Cell 编辑器面板（[ADR-0040] · 2026-10-20 Figma `99-293` 对齐版）。
//!
//! ## 设计对照（Figma `world view rs` node `99-293` "Cell 编辑器 · 选中属性"）
//!
//! 288px 暗色浮动面板，自上而下：
//! 1. **Selected cell 头**：`float_window`（图标/标题/把手/折叠 chevron 已由壳画）
//!    + identity 两行（`Cell · X, Y` 主标 + `网格单元 · 已选中` 金色副标）；
//! 2. **位置**：heading「位置」+ 右侧 `lock-keyhole 只读坐标`；X/Y 两列
//!    Mono 标签 + [`value_box`] 只读值；
//! 3. **地表**：heading + 右侧 [`segmented`]（水 / 陆地，段宽 64）；
//! 4. **海拔**：heading + [`numeric_input_bounded`]（i16 米全域钳制，stepper
//!    步长 1）+ 下方范围提示；
//! 5. **群系**：heading「群系 / 互斥单选」+ 设计稿 7 组 [`radio`]
//!    （森林系/干旱系/湿寒系/高地草原系/水系/道路河流/无群系），选中即改；
//! 6. 尾注：`focus` 图标 + 「仅编辑当前选中的 cell」。
//!
//! 批量编辑区（多边形选区）保留在面板底部（divider 之下），设计稿未画、
//! 为 2026-10-20 批量需求的延续。
//!
//! 控件全部出自 wv-ui（用户 2026-10-20 要求）；缺的
//! [`numeric_input_bounded`] 已新建为公共组件。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState, SelectionState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::font_size;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{
    divider, empty_state, float_window, value_box, ButtonContent, ButtonStyle, ICON_FOCUS,
    ICON_LOCK_KEYHOLE,
};
use wv_render::state::EditCellState;
use wv_ui::widgets::controls::{numeric_input_bounded, option_menu, radio};
use wv_ui::widgets::segmented_control::segmented;

/// Cell 编辑器面板。
#[allow(clippy::too_many_arguments)]
pub fn panel_cell_editor(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut projection: ResMut<WorldProjection>,
    edit_cell: Res<EditCellState>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut selection: ResMut<SelectionState>,
    current_loc: Res<CurrentLocale>,
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
        Panel::CellEditor,
        None,
        current_loc.0,
        |ui| {
            // ── 批量模式：多边形选区贴合的方格集合非空 ──
            if !edit_cell.region_cells.is_empty() {
                render_batch_section(
                    ui,
                    &shell,
                    &mut projection,
                    &edit_cell,
                    &mut edit,
                    &mut selection,
                    current_loc.0,
                );
                divider(ui, &shell);
                ui.add_space(6.0);
            }
            // ── 单格：选中态 / 空态 ──
            match edit_cell.selected {
                Some((gx, gy)) => {
                    let Some(before) = read_cell(&projection, &edit_cell, gx, gy) else {
                        return;
                    };
                    render_identity(ui, &shell, gx, gy);
                    divider(ui, &shell);
                    render_fields(
                        ui,
                        &shell,
                        &mut projection,
                        &edit_cell,
                        &mut edit,
                        (gx, gy),
                        before,
                        current_loc.0,
                    );
                    divider(ui, &shell);
                    // 尾注（Figma "Selection scope"）：focus 图标 + 提示。
                    ui.horizontal(|ui| {
                        let y = ui.cursor().center().y;
                        let icon_x = ui.cursor().left() + 6.0;
                        ui.put(
                            egui::Rect::from_min_size(
                                egui::pos2(icon_x, y - 6.0),
                                egui::vec2(12.0, 12.0),
                            ),
                            egui::Image::new(ICON_FOCUS).tint(shell.t3()),
                        );
                        ui.label(
                            egui::RichText::new(t(current_loc.0, I18nKey::CellEditorScopeHint))
                                .size(font_size::MICRO)
                                .color(shell.t3()),
                        );
                    });
                }
                None => {
                    empty_state(
                        ui,
                        &shell,
                        "▣",
                        t(current_loc.0, I18nKey::CellEditorEmptyTitle),
                        t(current_loc.0, I18nKey::CellEditorEmptyHint),
                        "",
                    );
                }
            }
        },
    );
}

/// identity 两行（Figma "Object identity"）：主标 `Cell · X, Y` + 金副标。
fn render_identity(ui: &mut egui::Ui, shell: &UiShell, gx: i32, gy: i32) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(format!("Cell · {gx}, {gy}"))
            .size(13.0)
            .color(shell.t1()),
    );
    ui.label(
        egui::RichText::new(t(crate::i18n::Locale::Zh, I18nKey::CellEditorSubtitle))
            .size(font_size::MICRO)
            .color(shell.accent()),
    );
    ui.add_space(4.0);
}

/// 设计稿单格四段（位置 / 地表 / 海拔 / 群系）。
#[allow(clippy::too_many_arguments)]
fn render_fields(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    edit_cell: &EditCellState,
    edit: &mut crate::edit::editing::EditHistory,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    loc: crate::i18n::Locale,
) {
    let mut next = before;
    ui.add_space(6.0);

    // ── 位置（只读）：heading + 🔒 只读坐标；X / Y 两列 ──
    section_heading(ui, shell, t(loc, I18nKey::CellInspectorPosition), true);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for (label, v) in [("X", gx), ("Y", gy)] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    egui::RichText::new(label)
                        .monospace()
                        .size(11.0)
                        .color(shell.t3()),
                );
                value_box(ui, shell, &v.to_string(), None, false);
            });
        }
    });
    ui.add_space(8.0);

    // ── 地表：heading + 分段（水 / 陆地）──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorSurface))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        segmented(
            ui,
            shell,
            "cell_editor_surface",
            64.0,
            &[
                t(loc, I18nKey::CellEditorWater),
                t(loc, I18nKey::CellEditorLand),
            ],
            if before.surface == wv_terrain::SURFACE_WATER {
                0
            } else {
                1
            },
            font_size::SMALL,
            |i| {
                next.surface = if i == 0 {
                    wv_terrain::SURFACE_WATER
                } else {
                    wv_terrain::SURFACE_LAND
                };
            },
        );
    });
    ui.add_space(8.0);

    // ── 海拔：heading + stepper 数值（i16 米全域钳制）+ 范围提示 ──
    let mut height_f = before.height as f32;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorHeight))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.allocate_ui(egui::vec2(168.0, 28.0), |ui| {
            numeric_input_bounded(
                ui,
                shell,
                &mut height_f,
                "m",
                1.0,
                i16::MIN as f32,
                i16::MAX as f32,
            );
        });
    });
    next.height = height_f.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellEditorHeightRange))
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(8.0);
    divider(ui, &shell);
    ui.add_space(6.0);

    // ── 群系：heading（互斥单选）+ 设计稿 7 组 radio ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorBiome))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellEditorBiomeExclusive))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
    });
    ui.add_space(4.0);
    for group in biome_groups() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (num, name) in group.iter() {
                if radio(ui, shell, next.biome == *num, name).clicked() {
                    next.biome = *num;
                }
            }
        });
        ui.add_space(4.0);
    }

    // ── 即改即提交（每格 1 command，[ADR-0040] §决策 8）──
    if next != before {
        push_cell_edit(
            edit,
            &mut projection.world,
            edit_cell,
            (gx, gy),
            before,
            next,
        );
    }
}

/// 段落 heading 行：左标题 + 右侧可选「🔒 只读」徽标（Figma Position heading）。
fn section_heading(ui: &mut egui::Ui, shell: &UiShell, title: &str, readonly_badge: bool) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        if readonly_badge {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let text = t(crate::i18n::Locale::Zh, I18nKey::CellEditorReadonly);
                ui.label(
                    egui::RichText::new(text)
                        .size(font_size::MICRO)
                        .color(shell.t3()),
                );
                let y = ui.cursor().center().y;
                let icon_x = ui.cursor().left() + 5.0;
                ui.put(
                    egui::Rect::from_min_size(egui::pos2(icon_x, y - 5.0), egui::vec2(10.0, 10.0)),
                    egui::Image::new(ICON_LOCK_KEYHOLE).tint(shell.t3()),
                );
            });
        }
    });
}

/// 设计稿的群系 7 组（顺序、分组与 Figma `99-356` "Biome options" 一致）。
/// 每项 = (冻结编号, 显示名)；编号全部引用 `wv_terrain` 冻结常量，防手滑。
fn biome_groups() -> &'static [&'static [(u8, &'static str)]] {
    use wv_terrain::*;
    &[
        &[
            (BIOME_FOREST, "森林"),
            (BIOME_JUNGLE, "丛林"),
            (BIOME_TROPICAL_RAINFOREST, "热带雨林"),
        ],
        &[
            (BIOME_GOBI, "戈壁"),
            (BIOME_DESERT, "沙漠"),
            (BIOME_SALT_FLAT, "盐碱地"),
            (BIOME_OASIS, "绿洲"),
        ],
        &[
            (BIOME_SWAMP, "沼泽"),
            (BIOME_WETLAND, "湿地"),
            (BIOME_TUNDRA, "苔原"),
            (BIOME_SNOWFIELD, "雪原"),
        ],
        &[
            (BIOME_HILLS, "丘陵"),
            (BIOME_SAVANNA, "稀树草原"),
            (BIOME_PLAINS, "平原"),
            (BIOME_GRASSLAND, "草原"),
        ],
        &[(BIOME_OCEAN, "海洋"), (BIOME_LAKE, "湖泊")],
        &[(BIOME_ROAD, "道路"), (BIOME_RIVER, "河流")],
        &[(BIOME_NONE, "无群系")],
    ]
}

/// 批量编辑区（多边形选区贴合 N 格）：海拔增减 / 设水陆 / 设群系，
/// 每次应用 = 1 条 `BatchEditCellsCommand`（N 格 1 个撤销单位）。
#[allow(clippy::too_many_arguments)]
fn render_batch_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &mut WorldProjection,
    edit_cell: &EditCellState,
    edit: &mut crate::edit::editing::EditHistory,
    selection: &mut SelectionState,
    loc: crate::i18n::Locale,
) {
    let n = edit_cell.region_cells.len();
    // ── 头：选区规模 + 取消按钮 ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{} {}", n, t(loc, I18nKey::BatchCellsCount)))
                .size(font_size::SMALL)
                .color(shell.accent()),
        );
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::BatchCellsClear)),
            ButtonStyle::Ghost,
            None,
            true,
        )
        .clicked()
        {
            selection.cancel();
        }
    });
    ui.add_space(4.0);

    let Some(map) = edit_cell.last_map_id.clone() else {
        return;
    };

    // ── 海拔增减 ──
    ui.label(
        egui::RichText::new(t(loc, I18nKey::BatchCellsHeightDelta))
            .size(font_size::SMALL)
            .color(shell.t2()),
    );
    let mut delta = 0.0f32;
    numeric_input_bounded(ui, shell, &mut delta, "m", 10.0, -32767.0, 32767.0);
    if delta != 0.0
        && super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::BatchCellsApply)),
            ButtonStyle::Accent,
            None,
            true,
        )
        .clicked()
    {
        apply_batch(projection, edit, &map, edit_cell, |c| {
            wv_core::terrain::TerrainCellData {
                height: (c.height as i32 + delta as i32).clamp(i16::MIN as i32, i16::MAX as i32)
                    as i16,
                ..c
            }
        });
    }
    ui.add_space(4.0);

    // ── 设水陆 ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellInspectorSurface))
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::CellEditorWater)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            apply_batch(projection, edit, &map, edit_cell, |c| {
                wv_core::terrain::TerrainCellData {
                    surface: wv_terrain::SURFACE_WATER,
                    ..c
                }
            });
        }
        if super::widgets::button(
            ui,
            shell,
            ButtonContent::Text(t(loc, I18nKey::CellEditorLand)),
            ButtonStyle::Raised,
            None,
            true,
        )
        .clicked()
        {
            apply_batch(projection, edit, &map, edit_cell, |c| {
                wv_core::terrain::TerrainCellData {
                    surface: wv_terrain::SURFACE_LAND,
                    ..c
                }
            });
        }
    });
    ui.add_space(4.0);

    // ── 设群系 ──
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellInspectorBiome))
            .size(font_size::SMALL)
            .color(shell.t2()),
    );
    let labels = biome_menu_labels();
    if let Some(picked) = option_menu(ui, shell, "cell_batch_biome", &labels, 0, &[]) {
        let biome = menu_index_to_biome(picked);
        apply_batch(projection, edit, &map, edit_cell, |c| {
            wv_core::terrain::TerrainCellData { biome, ..c }
        });
    }
}

/// 批量应用：从 world 读每格当前值 → 构造 changes → 1 条
/// `BatchEditCellsCommand`（N 格 1 个撤销单位）→ History 落盘。
fn apply_batch(
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    map: &wv_core::id::Id,
    edit_cell: &EditCellState,
    transform: impl Fn(wv_core::terrain::TerrainCellData) -> wv_core::terrain::TerrainCellData,
) {
    use wv_edit::command::{BatchEditCellsCommand, CellChange};
    let Some(terrain) = projection.world.terrain_of(map) else {
        return;
    };
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    let mut changes = Vec::new();
    for (gx, gy) in &edit_cell.region_cells {
        if *gx < 0 || *gy < 0 || *gx >= w || *gy >= h {
            continue;
        }
        let idx = (*gy as usize) * (w as usize) + (*gx as usize);
        let before = terrain.cells[idx];
        let after = transform(before);
        if after != before {
            changes.push(CellChange {
                gx: *gx,
                gy: *gy,
                before,
                after,
            });
        }
    }
    if changes.is_empty() {
        return;
    }
    let cmd = BatchEditCellsCommand::new(map.clone(), changes);
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => edit.record_dirty(dirty),
        Err(e) => bevy::log::warn!("批量改 cell 失败: {e}"),
    }
}

/// 单格提交（stale 快照校验 + History 落盘；每格 1 command）。
fn push_cell_edit(
    edit: &mut crate::edit::editing::EditHistory,
    world: &mut wv_core::World,
    edit_cell: &EditCellState,
    (gx, gy): (i32, i32),
    before: wv_core::terrain::TerrainCellData,
    after: wv_core::terrain::TerrainCellData,
) {
    let Some(map) = edit_cell.last_map_id.clone() else {
        return;
    };
    let current = world.terrain_of(&map).and_then(|t| {
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
        bevy::log::warn!("EditCell commit 拒收: cell ({gx}, {gy}) 当前值已不是面板上看到的值");
        return;
    }
    let cmd = wv_edit::command::EditCellCommand::new(map, gx, gy, before, after);
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(world, tx) {
        Ok(dirty) => edit.record_dirty(dirty),
        Err(e) => bevy::log::warn!("EditCell commit 失败: {e}"),
    }
}

/// 读选中格当前值（`EditCellState.last_map_id` 定图）。
fn read_cell(
    projection: &WorldProjection,
    edit_cell: &EditCellState,
    gx: i32,
    gy: i32,
) -> Option<wv_core::terrain::TerrainCellData> {
    let terrain = projection
        .world
        .terrain_of(edit_cell.last_map_id.as_ref()?)?;
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    if gx < 0 || gy < 0 || gx >= w || gy >= h {
        return None;
    }
    terrain
        .cells
        .get((gy as usize) * (w as usize) + (gx as usize))
        .copied()
}

/// 批量下拉项标签（NONE + 冻结表 20 编号；下标即编号）。
fn biome_menu_labels() -> Vec<&'static str> {
    const NAMES: [&str; 21] = [
        "NONE",
        "森林",
        "平原",
        "草原",
        "戈壁",
        "沙漠",
        "沼泽",
        "湿地",
        "苔原",
        "雪原",
        "丛林",
        "热带雨林",
        "丘陵",
        "山地",
        "海洋",
        "湖泊",
        "河流",
        "绿洲",
        "稀树草原",
        "盐碱地",
        "道路",
    ];
    NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| if i == 0 { "NONE" } else { *n })
        .collect()
}

/// 下拉下标 → biome 编号。
fn menu_index_to_biome(index: usize) -> u8 {
    index as u8
}
