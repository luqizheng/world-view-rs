//! 街区参数表单（2026-11-06 城市街区工具设计 §4.1 的表单部分）。
//!
//! **上下文浮窗**（不是常驻侧面板）：作者在城市图上圈完一块地，这个窗
//! 立刻弹出——调密度 / 楼层带 / 肌理带，按「生成」落一条
//! `UpsertDistrictCommand`（D3 单事务），按「取消」什么也不发生。
//!
//! 走 `egui::Window` 而非 `float_window` 面板体系：街区表单是**一次性**
//! 的上下文窗（作者操作完就关），不进 `Panel` 枚举 / 布局文件 / 图标轮
//! （那三处是常驻侧面板的持久化机制）。
//!
//! ## 种子
//!
//! 表单里的种子可改（改 = 换布局）——D2「留种」的核心：同一块地、不同
//! 种子 = 不同街区方案。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::district_tool::{district_from_form, DistrictToolState};
use crate::edit::editing::EditHistory;
use crate::i18n::{t_key, CurrentLocale};
use crate::state::{DrillState, WorldProjection};
use wv_edit::UpsertDistrictCommand;

/// 街区表单窗口 + 「生成」提交。
///
/// 守卫：表单开着才画；提交成功后关窗并清工具（生成完毕，工具回到点选）。
pub fn district_form_window(
    mut contexts: EguiContexts,
    mut tool: ResMut<DistrictToolState>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    mut edit: ResMut<EditHistory>,
    current_loc: Res<CurrentLocale>,
) {
    let Some(mut form) = tool.form.clone() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let mut generate = false;
    let mut cancel = false;
    let mut seed_text = format!("{:x}", form.seed);

    egui::Window::new(t_key(&current_loc, crate::i18n::I18nKey::DistrictFormTitle))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(t_key(
                &current_loc,
                crate::i18n::I18nKey::DistrictFormDensity,
            ));
            ui.add(egui::Slider::new(&mut form.params.density, 0.0..=1.0).text("0.0 – 1.0"));
            ui.label(t_key(
                &current_loc,
                crate::i18n::I18nKey::DistrictFormHeight,
            ));
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut form.params.height_min_m, 3.0..=40.0).text("min"));
                ui.add(egui::Slider::new(&mut form.params.height_max_m, 3.0..=60.0).text("max"));
            });
            ui.label(t_key(&current_loc, crate::i18n::I18nKey::DistrictFormBlock));
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut form.params.block_min_m, 8.0..=60.0).text("min"));
                ui.add(egui::Slider::new(&mut form.params.block_max_m, 8.0..=80.0).text("max"));
            });
            ui.horizontal(|ui| {
                ui.label(t_key(&current_loc, crate::i18n::I18nKey::DistrictFormSeed));
                ui.text_edit_singleline(&mut seed_text);
                // 十六进制校验：非 hex 字符 → 种子保持上一个有效值（不弹错，
                // 作者打字中途的非法字符不该打断输入）。
                if seed_text.chars().all(|c| c.is_ascii_hexdigit()) && !seed_text.is_empty() {
                    if let Ok(v) = u64::from_str_radix(&seed_text, 16) {
                        form.seed = v;
                    }
                }
                if ui.button("🎲").clicked() {
                    form.seed = crate::edit::district_tool::random_seed();
                    seed_text = format!("{:x}", form.seed);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(t_key(
                        &current_loc,
                        if form.existing {
                            crate::i18n::I18nKey::DistrictFormRegenerate
                        } else {
                            crate::i18n::I18nKey::DistrictFormGenerate
                        },
                    ))
                    .clicked()
                {
                    generate = true;
                }
                if ui
                    .button(t_key(
                        &current_loc,
                        crate::i18n::I18nKey::DistrictFormCancel,
                    ))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });

    if cancel {
        tool.form = None;
        tool.pending_ring = None;
        tool.active = false;
        return;
    }
    if !generate {
        // 参数改动即回写（滑条拖动实时存进表单状态）。
        tool.form = Some(form);
        return;
    }

    // 提交：补建图层 → 构造命令 → 单事务提交（一条 undo）。
    let map_id = drill.current().cloned();
    let Some(map_id) = map_id else {
        tool.form = None;
        return;
    };
    let (district_layer, building_layer) =
        wv_edit::ensure_district_layers(&mut projection.world, &map_id);
    let (district, streets) = district_from_form(&form, &projection, &drill);
    let cmd = UpsertDistrictCommand::new(district_layer, building_layer, streets, district);
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // 街区选中态：让 overlay 画出金色高亮（作者看得见自己刚生成的地块）。
            tool.selected = Some(form.id.clone());
            tool.form = None;
            tool.pending_ring = None;
            tool.active = false;
        }
        Err(e) => bevy::log::warn!("街区生成命令失败: {e}"),
    }
}
