//! Region 表单（统一 Inspector 的「属性」页签 · 区域分支）。
//!
//! 从旧 `region_panel` 的 `region_card` 整卡搬入（2026-10-22 统一 Inspector
//! 设计）。**搬的是内容，不是外壳**：旧的卡片自带一个 `CollapsingHeader`
//! （区域列表里每张卡可折叠），Inspector 已有「类型色点 + kind·id」的摘要头，
//! 故这里去掉折叠头，字段直接平铺。
//!
//! ## 与旧面板的分工
//!
//! - **区域面板**（`region_panel`）= 列表 + 「+ 新建区域」按钮 = **选谁**。
//! - **Inspector 的本表单** = 当前选中区域的属性 = **改它**。
//!
//! ## 父区域
//!
//! **本轮不做**（设计稿 §11 Q5）：`Region.parent` 在数据模型里已存在，
//! `SetRegionParentCommand` 也已就位（自带环检测），但**旧 UI 从来没有这个
//! 控件**——它不是「搬现有」，是净新增功能。故留待 v2（届时需在 wv-edit
//! 导出一个「取某区域的后代集」的公共函数，`is_descendant` 目前是私有 fn）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing::EditHistory;
use crate::edit::{nature_region_actions, region_panel_actions};
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{ViewState, WorldProjection};
use crate::theme::ui_tokens::space;
use crate::ui::shell::UiShell;
use crate::ui_shell::ShellNotice;

use wv_core::Region;

/// 区域属性表单。
#[allow(clippy::too_many_arguments)]
pub fn region_form(
    ui: &mut egui::Ui,
    shell: &UiShell,
    region: &Region,
    view: &ViewState,
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    current_loc: &CurrentLocale,
    overlay_keys: &[&'static str],
    cell_size: f32,
    raster_to_map: (f32, f32),
    notice: &mut ShellNotice,
    frame: u64,
) {
    let id = region.id.clone();
    let vertex_count = region.polygon.len() / 2;

    // ── 名称 ────────────────────────────────────────────────────────
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

    // ── 轮廓信息（只读）─────────────────────────────────────────────
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

    // ── 类别互转（ADR-0045 D3）：普通区域 ↔ 自然区 ──────────────────
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
        let _ =
            nature_region_actions::set_region_kind_now(projection, edit, view, id.clone(), kind);
    }

    // ── 自然区专属（ADR-0045 D2 / D6）：覆盖物键 + 重生成边界 ────────
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
}
