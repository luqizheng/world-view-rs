//! Cell 编辑器面板（[ADR-0040] · 2026-10-20 独立面板化）。
//!
//! ## 职责
//! - `panel_cell_editor` —— CellMap 工作区的核心编辑面板：选中格的
//!   **坐标（只读）/ 地表（水陆分段）/ 海拔（数值输入）/ 群系（下拉）**。
//! - 任一字段变更即提交 `EditCellCommand`（每格 1 command 进撤销栈，
//!   [ADR-0040] §决策 8；stale 快照校验防叠加冲突）。
//!
//! ## 控件口径（用户 2026-10-20 要求：空间尽量用 wv-ui）
//! - 壳：[`float_window`]（与 Region / Faction 面板同款浮动壳）；
//! - 头：[`identity_header`]（色点 + 标题 + 副标题）；
//! - 地表：[`segmented`]（水 / 陆两段，与顶栏 Browse/Edit 同款分段控件）；
//! - 海拔：[`numeric_input`]（等宽字体 + 单位后缀，步长 1 m）；
//! - 群系：[`option_menu`]（NONE + 20 编号平铺，冻结表顺序）；
//! - 只读坐标：[`PropertyRow`]（label + value_box）。
//!
//! ## 与 Inspector 的关系
//! Inspector 的 Cell 折叠区**退役**（本面板取代）——CellMap 工作区编排改为
//! 只开本面板。Inspector 回归纯「世界实体」检查器。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::font_size;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::float_window;
use wv_render::state::EditCellState;
use wv_ui::widgets::controls::{numeric_input, option_menu};
use wv_ui::widgets::primitives::{divider, identity_header};
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
            // ── 头：选中态 / 空态 ──
            match edit_cell.selected {
                Some((gx, gy)) => {
                    identity_header(
                        ui,
                        &shell,
                        shell.accent(),
                        super::widgets::ICON_MAP,
                        &format!("Cell ({gx}, {gy})"),
                        &t(current_loc.0, I18nKey::CellInspectorCoord),
                        None,
                    );
                    divider(ui, &shell);
                    // 借位隔离：读 before → 放掉不可变借用 → 控件闭包内可
                    // `&mut projection.world` 提交（与 Inspector 版同款手法）。
                    let before = read_cell(&projection, &edit_cell, gx, gy);
                    let Some(before) = before else {
                        return;
                    };
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
                }
                None => {
                    super::widgets::empty_state(
                        ui,
                        &shell,
                        "▣",
                        &t(current_loc.0, I18nKey::CellEditorEmptyTitle),
                        &t(current_loc.0, I18nKey::CellEditorEmptyHint),
                        "",
                    );
                }
            }
        },
    );
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

/// 四字段渲染 + 即改即提交。
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

    // ── 地表：水 / 陆 分段 ──
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellInspectorSurface))
            .size(12.0)
            .color(shell.t2()),
    );
    let mut surface_seg = if before.surface == wv_terrain::SURFACE_WATER {
        0
    } else {
        1
    };
    segmented(
        ui,
        shell,
        "cell_editor_surface",
        56.0,
        &[
            t(loc, I18nKey::CellEditorWater),
            t(loc, I18nKey::CellEditorLand),
        ],
        surface_seg,
        font_size::SMALL,
        |i| surface_seg = i,
    );
    next.surface = if surface_seg == 0 {
        wv_terrain::SURFACE_WATER
    } else {
        wv_terrain::SURFACE_LAND
    };
    ui.add_space(6.0);

    // ── 海拔：数值输入（米，步长 1）──
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellInspectorHeight))
            .size(12.0)
            .color(shell.t2()),
    );
    let mut height_f = before.height as f32;
    numeric_input(ui, shell, &mut height_f, "m", 1.0);
    next.height = height_f.clamp(i16::MIN as f32, i16::MAX as f32).round() as i16;
    ui.add_space(6.0);

    // ── 群系：下拉（NONE + 冻结表 20 编号）──
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellInspectorBiome))
            .size(12.0)
            .color(shell.t2()),
    );
    let labels: Vec<&str> = biome_menu_labels();
    let selected = biome_menu_index(before.biome);
    if let Some(picked) = option_menu(ui, shell, "cell_editor_biome", &labels, selected, &[]) {
        next.biome = menu_index_to_biome(picked);
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

/// 提交一格编辑（stale 快照校验 + History 落盘；从 Inspector 版原样迁入）。
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
    // Stale 校验：Inspector/面板打开期间值被别的系统改过 → 拒收（防叠加冲突）。
    let current = world.terrain_of(&map).and_then(|ter| {
        let w = ter.width as i32;
        let h = ter.height as i32;
        if gx < 0 || gy < 0 || gx >= w || gy >= h {
            return None;
        }
        ter.cells
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

// ---------------------------------------------------------------------------
// 冻结表下拉数据（NONE + 20 编号，顺序 = wv_terrain::biome 冻结表）
// ---------------------------------------------------------------------------

/// 下拉项标签（下标 0 = NONE，其后按冻结编号 1..=20 顺序）。
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
        .map(|(i, n)| match i {
            0 => "NONE",
            _ => n,
        })
        .collect()
}

/// biome 编号 → 下拉下标（编号即下标：冻结表 0..=20 与菜单顺序一致；
/// 开放集未登记编号回落 NONE 项——显示近似，不崩）。
fn biome_menu_index(biome: u8) -> usize {
    (biome as usize).min(20)
}

/// 下拉下标 → biome 编号。
fn menu_index_to_biome(index: usize) -> u8 {
    index as u8
}
