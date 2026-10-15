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
use crate::edit::region_panel_actions;
use crate::state::{Panel, PanelState, ViewState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

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
        |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ 新建区域").clicked() {
                    region_panel_actions::add_region_now(
                        &mut projection,
                        &mut edit,
                        &view,
                        "新区域".into(),
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
                    "当前地图尚无区域",
                    "点击上方「+ 新建区域」创建，或在工程目录下编辑 regions/*.yaml",
                    "查看区域说明 →",
                );
                return;
            }

            for r in &regions {
                region_card(ui, &shell, r, &view, &mut projection, &mut edit);
                ui.add_space(space::XS);
            }
        },
    );
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
) {
    let id = region.id.clone();
    let vertex_count = region.polygon.len() / 2;
    egui::CollapsingHeader::new(format!(
        "⬡ {} ({}) — {} 顶点",
        region.name, region.id, vertex_count
    ))
    .default_open(true)
    .show(ui, |ui| {
        // 名称编辑。
        let mut name_buf = region.name.clone();
        ui.horizontal(|ui| {
            ui.label("名称：");
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
                egui::RichText::new(format!("轮廓：{} 个顶点", vertex_count))
                    .small()
                    .color(shell.t2()),
            );
            if region.polygon.is_empty() {
                ui.label(
                    egui::RichText::new("（未绘制）")
                        .italics()
                        .small()
                        .color(shell.t2()),
                );
            }
        });

        ui.add_space(space::XS);
        if ui
            .button(egui::RichText::new("删除区域").color(egui::Color32::RED))
            .clicked()
        {
            region_panel_actions::remove_region_now(projection, edit, view, id.clone());
        }
    });
}
