//! 时间轴底部通栏 + 四层轨道 + 街道撤销。
//!
//! ## 职责
//! - `TimelineLayer` —— L1 日 / L2 事态 / L3 人物 / L4 世纪 四层轨道枚举。
//! - `panel_timeline` —— 时间轴浮窗；通栏位置由 [`crate::ui_layout`] 持久化。
//! - `undo_last_street_segment` —— 撤销街道段（UI → `wv-edit::History`，见 §3.3）。
//!
//! ## 边界（不做什么）
//! - **不**改时间语义 —— 时间显示统一入口 [`crate::ui_time::format_date_for`]。
//! - **不**直接改世界 —— UI 只生成 `EditCommand`，交给 [`crate::editing::EditHistory`]。
//!
//! ## 主要导出
//! - `TimelineLayer` / `panel_timeline` / `undo_last_street_segment`

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{Panel, PanelState, PlaybackState, WorldProjection};
use crate::ui_layout::LayoutFile;
use crate::ui_theme::{proportional, UiShellConfig};
use crate::ui_tokens::{radius, space};

use super::shell::UiShell;

/// 时间轴轨道层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineLayer {
    /// L1 日：细刻度尺 + 播放头。
    Day,
    /// L2 事态：事态条 + 轨迹条（默认层）。
    #[default]
    Situation,
    /// L3 人物：生卒条。
    Character,
    /// L4 世纪：按世纪聚合鸟瞰。
    Century,
}

impl TimelineLayer {
    /// 层名。
    pub fn label(self) -> &'static str {
        match self {
            TimelineLayer::Day => "日",
            TimelineLayer::Situation => "事态",
            TimelineLayer::Character => "人物",
            TimelineLayer::Century => "世纪",
        }
    }
    /// 全部层。
    pub const ALL: [TimelineLayer; 4] = [
        TimelineLayer::Day,
        TimelineLayer::Situation,
        TimelineLayer::Character,
        TimelineLayer::Century,
    ];
}

/// 时间轴底部通栏浮窗 + 四层轨道。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn panel_timeline(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut playback: ResMut<PlaybackState>,
    projection: Res<WorldProjection>,
    mut layer: Local<TimelineLayer>,
    mut open_event: Local<bool>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !panels.is_visible(Panel::Timeline) {
        return;
    }

    // 沉浸模式下时间轴降到 55% 透明但**不隐藏**（D3）。
    let alpha = if panels.immersive { 0.55 } else { 1.0 };

    // 通栏也走布局表（否则拖动结果不落盘，`ui-layout.yaml` 里的 timeline 段是死数据）。
    let layout = crate::ui_layout::current_layout(&layouts, &panels, Panel::Timeline);

    let mut open = true;
    egui::Window::new("")
        .id(egui::Id::new(("wv_panel", Panel::Timeline)))
        .open(&mut open)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .resizable(true)
        .title_bar(false)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(crate::ui_theme::margin_all(space::SM))
                .corner_radius(radius::PANEL),
        )
        .show(ctx, |ui| {
            // ── 播放控件 + 日期 + 层切换 ──
            ui.horizontal(|ui| {
                let play_label = if playback.playing { "⏸" } else { "▶" };
                if ui
                    .add(egui::Button::new(play_label).min_size(egui::vec2(shell.hit, shell.hit)))
                    .clicked()
                {
                    playback.playing = !playback.playing;
                }
                for s in [1.0f32, 5.0, 30.0] {
                    let sel = (playback.speed - s).abs() < f32::EPSILON;
                    if ui.selectable_label(sel, format!("{s:.0}天/秒")).clicked() {
                        playback.speed = s;
                    }
                }
                ui.add_space(space::MD);
                // 日期（时间轴是唯一时间权威，§5.3）。
                ui.label(
                    egui::RichText::new(crate::ui_time::format_date_for(
                        &projection,
                        playback.current_day,
                    ))
                    .size(shell.scale.base)
                    .color(shell.t1())
                    .font(crate::ui_theme::mono(shell.scale.base)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("＋ 登记事件").clicked() {
                        *open_event = true;
                    }
                    ui.add_space(space::SM);
                    for l in TimelineLayer::ALL {
                        let sel = *layer == l;
                        if ui.selectable_label(sel, l.label()).clicked() {
                            *layer = l;
                        }
                    }
                });
            });

            ui.add_space(space::SM);

            // ── 轨道区（按层渲染）──
            let track_h = 84.0;
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), track_h),
                egui::Sense::click_and_drag(),
            );
            draw_timeline_tracks(ui, &shell, rect, *layer, &playback, &projection);

            // 播放头拖拽 / 点击定位。
            if resp.dragged() || resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let frac = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                    let span = (playback.range_to - playback.range_from).max(1);
                    playback.current_day = playback.range_from + (frac * span as f32) as i64;
                }
            }
        });

    if !open {
        panels.set_visible(Panel::Timeline, false);
    }
    if *open_event {
        panels.set_visible(Panel::Event, true);
        *open_event = false;
    }
}

fn draw_timeline_tracks(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    layer: TimelineLayer,
    playback: &PlaybackState,
    projection: &WorldProjection,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CONTROL, shell.raised_bg());

    let span = (playback.range_to - playback.range_from).max(1) as f32;
    let x_of = |day: i64| {
        rect.min.x + ((day - playback.range_from) as f32 / span).clamp(0.0, 1.0) * rect.width()
    };

    match layer {
        TimelineLayer::Day => {
            // 细刻度尺 + 播放头。
            for i in 0..=20 {
                let x = rect.min.x + rect.width() * i as f32 / 20.0;
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 8.0)],
                    egui::Stroke::new(1.0_f32, shell.border_strong()),
                );
            }
        }
        TimelineLayer::Situation => {
            // 事态条（世界色：事件紫）。
            for (i, s) in projection.world.situations.iter().enumerate() {
                let y = rect.min.y + space::SM + i as f32 * 18.0;
                let x0 = x_of(s.span.from);
                let x1 = x_of(s.span.to.unwrap_or(playback.range_to));
                let bar = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + 12.0));
                painter.rect_filled(
                    bar,
                    radius::BADGE,
                    crate::colors::palette_to_color32(crate::colors::ACCENT_EVENT),
                );
                painter.text(
                    egui::pos2(x0 + space::XS, y + 6.0),
                    egui::Align2::LEFT_CENTER,
                    &s.name,
                    proportional(shell.scale.micro),
                    shell.t1(),
                );
            }
        }
        TimelineLayer::Character => {
            // 人物生卒条。
            for (i, c) in projection.world.characters.iter().enumerate() {
                let y = rect.min.y + space::SM + i as f32 * 18.0;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x + space::MD, y),
                    egui::pos2(rect.max.x - space::MD, y + 10.0),
                );
                painter.rect_filled(bar, radius::BADGE, shell.accent().gamma_multiply(0.5));
                painter.text(
                    egui::pos2(bar.min.x + space::XS, y + 5.0),
                    egui::Align2::LEFT_CENTER,
                    &c.name,
                    proportional(shell.scale.micro),
                    shell.t2(),
                );
            }
        }
        TimelineLayer::Century => {
            // 世纪聚合鸟瞰（10 万年跨度下 L1 画不出来）。
            let buckets = 12;
            for i in 0..buckets {
                let x = rect.min.x + rect.width() * i as f32 / buckets as f32;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x + 2.0, rect.max.y - 30.0),
                    egui::pos2(
                        x + rect.width() / buckets as f32 - 2.0,
                        rect.max.y - space::SM,
                    ),
                );
                painter.rect_filled(bar, radius::BADGE, shell.accent().gamma_multiply(0.35));
            }
        }
    }

    // 播放头（游标色 world.accent.cursor，共用令牌）。
    let cx = x_of(playback.current_day);
    painter.line_segment(
        [egui::pos2(cx, rect.min.y), egui::pos2(cx, rect.max.y)],
        egui::Stroke::new(
            2.0_f32,
            crate::colors::palette_to_color32(crate::colors::ACCENT_CURSOR),
        ),
    );
}

/// 撤销目标图层最后一段街道（面板按钮的落点；UI 不直接改世界）。
pub fn undo_last_street_segment(
    projection: &mut crate::state::WorldProjection,
    drill: &crate::state::DrillState,
    edit: &mut crate::editing::EditHistory,
) {
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let Some(layer) = crate::streets::target_road_layer(node) else {
        return;
    };
    // 先读段数并结束借用，再把命令交给 History（commit 要 &mut World）。
    let len = projection.world.street_of(&layer.id).map_or(0, |d| d.len());
    if len == 0 {
        return;
    }
    let cmd = wv_edit::DeleteStreetSegmentCommand::new(layer.id.clone(), len - 1);
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
        }
        Err(e) => bevy::log::warn!("删除街道段失败: {e}"),
    }
}
