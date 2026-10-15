//! 指南针面板（临时替换小地图，显示地图朝向）。
//!
//! ## 职责
//! - `panel_compass` —— 圆形罗盘 + 可拖拽指针 + 数值输入 + N/E/S/W 快捷按钮。
//! - `push_orientation_cmd` —— 朝向变化包成 `SetMapOrientationCommand` 提交。
//!
//! ## 边界（不做什么）
//! - **不**直接改 `projection.world` —— 走 `wv_edit::History`（§3.3）。
//! - **不**重定义朝向语义 —— `0° = 正北朝上，正值 = 顺时针偏转`。
//!
//! ## 主要导出
//! - `panel_compass` —— Bevy 系统。
//! - `push_orientation_cmd` —— 内部辅助（`pub(crate)`，原 `push_cmd` 私有名迁出）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing::EditHistory;
use crate::state::{DrillState, Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, FieldInput, ToolButton};

/// 指南针（临时替换小地图，显示地图朝向）。
///
/// 圆形罗盘 + 可拖拽指针 + 数值输入。朝向 = 地图屏幕旋转角（度）：
/// `0.0` = 正北朝上，正值 = 顺时针偏转。
pub fn panel_compass(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    drill: Res<DrillState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
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
        Panel::Compass,
        None,
        |ui| {
            // 取当前地图朝向；无地图时显示空状态。
            let Some(node) = drill.current_node(&projection.world) else {
                empty_state(ui, &shell, "◐", "指南针", "尚未选择地图", "→ 地图树");
                return;
            };
            let map_id = node.id.clone();
            let mut orientation = node.orientation;

            // ── 罗盘 ──
            ui.add_space(space::SM);
            let side = ui.available_width().min(168.0);
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
            let painter = ui.painter();
            let center = rect.center();
            let r = side / 2.0 - 4.0;

            // 底盘
            painter.circle_filled(center, r, shell.raised_bg());
            painter.circle_stroke(center, r, egui::Stroke::new(1.5, shell.border_strong()));

            // 刻度环
            for i in 0..72 {
                let angle = (i as f32 * 5.0 - 90.0) * std::f32::consts::PI / 180.0;
                let is_major = i % 18 == 0;
                let len = if is_major { 8.0 } else { 4.0 };
                let dir = egui::vec2(angle.sin(), -angle.cos());
                let p0 = center + dir * (r - 2.0);
                let p1 = center + dir * (r - 2.0 - len);
                let col = if is_major { shell.t1() } else { shell.t3() };
                painter.line_segment([p0, p1], egui::Stroke::new(1.0, col));
            }

            // N/E/S/W 标签
            let label_r = r - 16.0;
            for (label, deg) in [("N", 0.0), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
                let angle = (deg - 90.0) * std::f32::consts::PI / 180.0;
                let pos = center + egui::vec2(angle.sin(), -angle.cos()) * label_r;
                let galley = painter.layout_no_wrap(
                    label.to_string(),
                    proportional(shell.scale.micro),
                    shell.t2(),
                );
                painter.galley(pos - galley.size() / 2.0, galley, shell.t2());
            }

            // 指针（从中心指向朝向方向）
            let theta = orientation * std::f32::consts::PI / 180.0;
            let needle_len = r - 20.0;
            let tip = center + egui::vec2(theta.sin(), -theta.cos()) * needle_len;
            let tail = center + egui::vec2(-theta.sin(), theta.cos()) * (needle_len * 0.3);

            // 指针杆
            painter.line_segment([center, tip], egui::Stroke::new(2.5, shell.accent()));
            // 指针尾
            painter.line_segment([center, tail], egui::Stroke::new(2.0, shell.t2()));
            // 指针尖（三角形用三条线段模拟）
            let perp = egui::vec2(theta.cos(), theta.sin());
            let tri_w = 7.0;
            let tip_w = tip + perp * tri_w;
            let tip_s = tip - perp * tri_w;
            painter.line_segment([tip_w, tip], egui::Stroke::new(2.0, shell.accent()));
            painter.line_segment([tip_s, tip], egui::Stroke::new(2.0, shell.accent()));

            // 中心点
            painter.circle_filled(center, 3.0, shell.accent());

            // ── 拖拽逻辑 ──
            if resp.dragged() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let dx = pos.x - center.x;
                    let dy = pos.y - center.y;
                    let deg = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                    if (deg - orientation).abs() > 0.5 {
                        orientation = deg;
                        push_orientation_cmd(&mut projection, &mut edit, &map_id, deg);
                    }
                }
            }

            // ── 数值输入 + 标签 ──
            ui.add_space(space::SM);
            ui.horizontal_centered(|ui| {
                ui.label(
                    egui::RichText::new("朝向")
                        .size(shell.scale.small)
                        .color(shell.t2()),
                );
                let mut val_text = format!("{:.1}", orientation);
                if FieldInput::new(&mut val_text, "")
                    .font_mono(true)
                    .min_size(egui::vec2(60.0, shell.hit))
                    .show(ui, &shell)
                    .changed()
                {
                    if let Ok(v) = val_text.parse::<f32>() {
                        let v = v.rem_euclid(360.0);
                        if v != orientation {
                            orientation = v;
                            push_orientation_cmd(&mut projection, &mut edit, &map_id, v);
                        }
                    }
                }
                ui.label(
                    egui::RichText::new("°")
                        .size(shell.scale.small)
                        .color(shell.t2()),
                );
            });

            // ── 快捷按钮 ──
            ui.add_space(space::XS);
            ui.horizontal_centered(|ui| {
                for (label, val) in [
                    ("N 0°", 0.0),
                    ("E 90°", 90.0),
                    ("S 180°", 180.0),
                    ("W 270°", 270.0),
                ] {
                    ToolButton::text(label, "").show(ui, &shell, || {
                        if val != orientation {
                            orientation = val;
                            push_orientation_cmd(&mut projection, &mut edit, &map_id, val);
                        }
                    });
                }
            });
        },
    );
}

/// 指南针内部：把朝向变化包成事务提交。
pub(crate) fn push_orientation_cmd(
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    map_id: &wv_core::Id,
    orientation: f32,
) {
    let cmd = wv_edit::SetMapOrientationCommand::new(map_id.clone(), orientation);
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
        }
        Err(e) => {
            bevy::log::warn!("设置地图朝向失败: {e}");
        }
    }
}
