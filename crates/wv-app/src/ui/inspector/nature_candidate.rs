//! 候选命名表单（统一 Inspector 的「属性」页签 · CellMap 分支）。
//!
//! 2026-10-22 内容矩阵 §2.6-4：CellMap 开 Nature 开关后点选候选格 →
//! `NatureRegionUiState.pending_candidate` 记下该格 → 本表单接管属性页签
//! （优先级在 Cell 表单之上——上下文存在即作者正在命名，编辑完格子的事
//! 稍后再说）。
//!
//! **命名即物化**（ADR-0045 D4）：「物化」= 走既有
//! `materialize_nature_region_now` 落盘成 `kind = Nature` 的区域实体；
//! 「丢弃」= 清上下文，什么都不写。未命名候选本身**不落盘**。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::edit::nature_region_actions;
use crate::state::{ViewState, WorldProjection};
use crate::ui::shell::UiShell;
use crate::ui::widgets::value_box;

/// 候选命名表单。
#[allow(clippy::too_many_arguments)]
pub fn candidate_naming_form(
    ui: &mut egui::Ui,
    shell: &UiShell,
    cell: (i32, i32),
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    grid: &wv_render::state::GridConfig,
    notice: &mut crate::ui_shell::ShellNotice,
    time: &bevy::prelude::Time,
    nature: &mut crate::state::NatureRegionUiState,
) {
    ui.label("未命名自然区");
    ui.label(
        egui::RichText::new(format!("选中格：({}, {})", cell.0, cell.1))
            .small()
            .color(shell.t3()),
    );
    ui.add_space(4.0);

    // 候选信息（群系 + 格数）——candidate_at 只回 (biome, cells)。
    if let Some((biome, cells)) = nature_region_actions::candidate_at(
        projection,
        &wv_core::Id::new_unchecked(&view.current_map),
        cell,
    ) {
        ui.horizontal(|ui| {
            ui.label("群系：");
            value_box(ui, shell, &biome.to_string(), None, false);
        });
        ui.horizontal(|ui| {
            ui.label("格数：");
            value_box(ui, shell, &cells.to_string(), None, false);
        });
    }

    ui.add_space(6.0);
    ui.label("名称：");
    ui.text_edit_singleline(&mut nature.name);

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("物化（落盘）").clicked() && !nature.name.trim().is_empty() {
            let offset = (
                grid.map_origin.x - grid.raster_origin.x,
                grid.map_origin.y - grid.raster_origin.y,
            );
            match nature_region_actions::materialize_nature_region_now(
                projection,
                edit,
                view,
                nature.name.trim().to_owned(),
                cell,
                grid.cell_size,
                offset,
            ) {
                Ok(id) => {
                    notice.info(
                        format!("已物化自然区「{}」({id})", nature.name.trim()),
                        (time.elapsed_secs() * 60.0) as u64,
                    );
                    nature.name.clear();
                    nature.pending_candidate = None;
                }
                Err(msg) => notice.error(msg, (time.elapsed_secs() * 60.0) as u64),
            }
        }
        if ui.button("丢弃").clicked() {
            nature.pending_candidate = None;
        }
    });
}
