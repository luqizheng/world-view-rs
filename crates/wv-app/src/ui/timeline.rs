//! 时间轴底部通栏（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU` 风格）+ 街道撤销。
//!
//! ## 职责
//! - `TimelineLayer` —— Day / Situation 二段（v0.3 砍掉 Character/Century 两段，
//!   Figma 上只显示 `DAY TICKS` / `SITUATIONS` 两个 chip；旧四段保留
//!   在枚举里以备恢复）。
//! - `panel_timeline` —— 时间轴浮窗；通栏位置由 [`crate::ui_layout`] 持久化。
//! - `undo_last_street_segment` —— 撤销街道段（UI → `wv-edit::History`，见 §3.3）。
//!
//! ## v0.3 视觉重做要点
//! - 顶头：圆形播放按钮 + `Year N` / `Day M · Season` + 速度下拉 + 二段 chip
//! - 主轨道：4 季节段（程序化几何，Figma `Y213·Winter / Y214·Spring ...`）
//! - 事件块：金色"Border dispute" + 紫色"Market opens"（来自 Figma mock）
//! - 播放头：金三角 + 金线 + `Day 38` 文字标签
//! - 右：`+ Register event` 蓝按钮
//!
//! ## 边界
//! - **不**改时间语义 —— 时间显示统一入口 [`crate::ui_time::format_date_for`]。
//! - **不**直接改世界 —— UI 只生成 `EditCommand`，交给 [`crate::edit::editing::EditHistory`]。
//!
//! ## 主要导出
//! - `TimelineLayer` / `panel_timeline` / `undo_last_street_segment`

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{Panel, PanelState, PlaybackState, WorldProjection};
use crate::theme::ui_theme::{mono, proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{ICON_PLAY, ICON_SEARCH};

/// 时间轴轨道层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineLayer {
    /// L1 日：细刻度尺 + 播放头（Figma 段 `DAY TICKS`）。
    Day,
    /// L2 事态：事态条 + 轨迹条（默认层，Figma 段 `SITUATIONS`）。
    #[default]
    Situation,
    /// 保留：人物生卒条。Figma 不再展示，但枚举保留供 M3 重新开放。
    #[allow(dead_code)]
    Character,
    /// 保留：世纪聚合鸟瞰。同上。
    #[allow(dead_code)]
    Century,
}

impl TimelineLayer {
    /// 顶栏 chip 标签（Figma 段是大写英文）。
    pub fn chip_label(self) -> &'static str {
        match self {
            TimelineLayer::Day => "DAY TICKS",
            TimelineLayer::Situation => "SITUATIONS",
            TimelineLayer::Character => "CHARACTERS",
            TimelineLayer::Century => "CENTURIES",
        }
    }
    /// 旧内部层名（命令面板 / 日志 / 测试仍识别）。
    pub fn label(self) -> &'static str {
        match self {
            TimelineLayer::Day => "日",
            TimelineLayer::Situation => "事态",
            TimelineLayer::Character => "人物",
            TimelineLayer::Century => "世纪",
        }
    }
    /// 顶栏二段 chip 用的两段（Figma v0.3）。
    pub const VISIBLE: [TimelineLayer; 2] = [TimelineLayer::Day, TimelineLayer::Situation];
    /// 旧四段保留，命令面板 / 测试不破。
    pub const ALL: [TimelineLayer; 4] = [
        TimelineLayer::Day,
        TimelineLayer::Situation,
        TimelineLayer::Character,
        TimelineLayer::Century,
    ];
}

/// 时间轴底部通栏浮窗（v0.3 视觉重做）。
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

    let alpha = if panels.immersive { 0.55 } else { 1.0 };
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
                .inner_margin(crate::theme::ui_theme::margin_all(space::MD))
                .corner_radius(radius::PANEL),
        )
        .show(ctx, |ui| {
            // ── 顶头：播放 + 年 + 日/季节 + 速度下拉 + 二段 chip ──
            ui.horizontal(|ui| {
                // 圆形播放按钮（Figma 44×44 蓝底）
                let play_size = 44.0;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(play_size, play_size), egui::Sense::click());
                let painter = ui.painter();
                painter.circle_filled(rect.center(), play_size * 0.5, shell.accent());
                let glyph = if playback.playing { "⏸" } else { "▶" };
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    proportional(18.0),
                    shell.t1(),
                );
                if resp.clicked() {
                    playback.playing = !playback.playing;
                }
                let _ = ICON_PLAY; // 保留引用：M2 替换 ⏸/▶ 为 SVG
                let _ = ICON_SEARCH;

                ui.add_space(space::MD);
                // 年份 + 当日 + 季节
                ui.vertical(|ui| {
                    let year = playback.current_day.max(1) as i32;
                    ui.label(
                        egui::RichText::new(format!("Year {year}"))
                            .size(shell.scale.title)
                            .color(shell.t1())
                            .strong(),
                    );
                    let day = playback.current_day;
                    let season = season_of_day(day);
                    ui.label(
                        egui::RichText::new(format!("Day {day} · {season}"))
                            .size(shell.scale.small)
                            .color(shell.t3())
                            .font(mono(shell.scale.small)),
                    );
                });
                ui.add_space(space::LG);

                // 速度下拉（v0.3：Figma 是 `1 day / sec` 风格）
                let speed_label = format!(
                    "{:.0} day{} / sec",
                    playback.speed,
                    if playback.speed >= 2.0 { "s" } else { "" }
                );
                egui::ComboBox::new("wv_timeline_speed", "")
                    .selected_text(speed_label)
                    .show_ui(ui, |ui| {
                        for s in [1.0f32, 5.0, 30.0] {
                            if ui
                                .selectable_label(
                                    (playback.speed - s).abs() < f32::EPSILON,
                                    format!("{s:.0} day / sec"),
                                )
                                .clicked()
                            {
                                playback.speed = s;
                            }
                        }
                    });

                ui.add_space(space::MD);
                // 二段 chip（Figma `DAY TICKS` 浅描边 / `SITUATIONS` 金激活）
                ui.horizontal(|ui| {
                    for l in TimelineLayer::VISIBLE {
                        let sel = *layer == l;
                        let chip_w = 100.0;
                        let chip_h = shell.hit;
                        let (rect, r) = ui
                            .allocate_exact_size(egui::vec2(chip_w, chip_h), egui::Sense::click());
                        let p = ui.painter();
                        if sel {
                            // 选中：金底黑字（Figma v0.3）
                            p.rect_filled(rect, radius::CONTROL, shell.gold());
                            p.text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                l.chip_label(),
                                proportional(shell.scale.micro),
                                shell.text_on_accent(),
                            );
                        } else {
                            p.rect_stroke(
                                rect,
                                radius::CONTROL,
                                egui::Stroke::new(1.0, shell.border_strong()),
                                egui::StrokeKind::Inside,
                            );
                            p.text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                l.chip_label(),
                                proportional(shell.scale.micro),
                                shell.t2(),
                            );
                        }
                        if r.clicked() {
                            *layer = l;
                        }
                    }
                });

                // 右端：Register event 蓝按钮
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let resp = ui.add(
                        egui::Button::new(
                            egui::RichText::new("+ Register event")
                                .size(shell.scale.base)
                                .color(shell.t1()),
                        )
                        .fill(shell.accent())
                        .min_size(egui::vec2(150.0, shell.hit))
                        .corner_radius(radius::CONTROL),
                    );
                    if resp.clicked() {
                        *open_event = true;
                    }
                });
            });

            ui.add_space(space::MD);

            // ── 主轨道（按层渲染）──
            let track_h = 130.0;
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), track_h),
                egui::Sense::click_and_drag(),
            );
            draw_timeline_tracks(ui, &shell, rect, *layer, &playback, &projection);

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

/// Figma 中 `Day N · Early Spring` 用的季节名（v0.3 占位）；
/// M2 接 `wv_core::Calendar::season_of` 后替换。
fn season_of_day(day: i64) -> &'static str {
    // 90 天一个季节，从 1 起的 day index 决定。
    let bucket = (day - 1).rem_euclid(360) / 90;
    match bucket {
        0 => "Early Spring",
        1 => "Summer",
        2 => "Autumn",
        _ => "Winter",
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
    // 轨道底（v0.3 新令牌 `timeline_track`）
    painter.rect_filled(rect, radius::CONTROL, shell.timeline_track());

    let span = (playback.range_to - playback.range_from).max(1) as f32;
    let x_of = |day: i64| {
        rect.min.x + ((day - playback.range_from) as f32 / span).clamp(0.0, 1.0) * rect.width()
    };

    // ── 季节段（4 等分，与 Figma 一致）──
    let season_h = 20.0;
    let season_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + season_h));
    let seg_w = season_rect.width() / 4.0;
    let seasons = [
        "Y213 · Winter",
        "Y214 · Spring",
        "Y214 · Summer",
        "Y214 · Autumn",
    ];
    for (i, s) in seasons.iter().enumerate() {
        let r = egui::Rect::from_min_max(
            egui::pos2(season_rect.min.x + seg_w * i as f32, season_rect.min.y),
            egui::pos2(
                season_rect.min.x + seg_w * (i as f32 + 1.0),
                season_rect.max.y,
            ),
        );
        painter.rect_filled(r, 0.0, shell.timeline_season());
        // 段分隔线
        if i > 0 {
            painter.line_segment(
                [r.min, egui::pos2(r.min.x, r.max.y)],
                egui::Stroke::new(1.0, shell.border_subtle()),
            );
        }
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *s,
            proportional(shell.scale.micro),
            shell.t3(),
        );
    }

    let track_y = rect.min.y + season_h + 8.0;
    let track_h = rect.height() - season_h - 16.0;

    match layer {
        TimelineLayer::Day => {
            // 细刻度尺
            for i in 0..=20 {
                let x = rect.min.x + rect.width() * i as f32 / 20.0;
                painter.line_segment(
                    [egui::pos2(x, track_y), egui::pos2(x, track_y + 8.0)],
                    egui::Stroke::new(1.0, shell.border_strong()),
                );
            }
        }
        TimelineLayer::Situation => {
            // 真实事态条（来自 world.situations）
            for (i, s) in projection.world.situations.iter().enumerate() {
                let y = track_y + i as f32 * 28.0;
                let x0 = x_of(s.span.from);
                let x1 = x_of(s.span.to.unwrap_or(playback.range_to));
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x0, y),
                    egui::pos2(x1.max(x0 + 60.0), y + 18.0),
                );
                painter.rect_filled(
                    bar,
                    radius::BADGE,
                    crate::theme::colors::palette_to_color32(crate::theme::colors::ACCENT_EVENT),
                );
                painter.text(
                    egui::pos2(bar.min.x + space::SM, bar.center().y),
                    egui::Align2::LEFT_CENTER,
                    &s.name,
                    proportional(shell.scale.micro),
                    shell.t1(),
                );
            }
            // v0.3 mock 两条事件块（Figma 视觉对照；M2 切到真实数据）
            draw_mock_event(
                painter,
                shell,
                track_y + 56.0,
                x_of(playback.range_from + (span * 0.20) as i64),
                x_of(playback.range_from + (span * 0.40) as i64),
                "Border dispute",
                shell.gold(),
            );
            draw_mock_event(
                painter,
                shell,
                track_y + 84.0,
                x_of(playback.range_from + (span * 0.55) as i64),
                x_of(playback.range_from + (span * 0.75) as i64),
                "Market opens",
                shell.danger(),
            );
        }
        TimelineLayer::Character | TimelineLayer::Century => {
            // v0.3 顶栏 chip 不再展示这两层，但旧 enum 仍可达，保留简易占位
            // 以免切回时空白。
            for i in 0..3 {
                let y = track_y + i as f32 * 24.0;
                painter.line_segment(
                    [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                    egui::Stroke::new(1.0, shell.border_subtle()),
                );
            }
        }
    }

    // ── 播放头：金三角 + 金线 + "Day N" 标签（v0.3）──
    let cx = x_of(playback.current_day);
    let _ = track_h;
    painter.line_segment(
        [egui::pos2(cx, rect.min.y), egui::pos2(cx, rect.max.y)],
        egui::Stroke::new(2.0_f32, shell.gold()),
    );
    // 三角
    let tri = [
        egui::pos2(cx, rect.min.y),
        egui::pos2(cx - 6.0, rect.min.y - 8.0),
        egui::pos2(cx + 6.0, rect.min.y - 8.0),
    ];
    painter.add(egui::Shape::convex_polygon(
        tri.to_vec(),
        shell.gold(),
        egui::Stroke::NONE,
    ));
    // Day N 标签
    let day = playback.current_day;
    let label = format!("Day {day}");
    let label_size = painter
        .layout(
            label.clone(),
            proportional(shell.scale.micro),
            shell.text_on_accent(),
            100.0,
        )
        .size();
    let label_rect = egui::Rect::from_min_size(
        egui::pos2(cx - label_size.x * 0.5 - 4.0, rect.max.y + 2.0),
        egui::vec2(label_size.x + 8.0, label_size.y + 2.0),
    );
    painter.rect_filled(label_rect, radius::BADGE, shell.gold());
    painter.text(
        label_rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        proportional(shell.scale.micro),
        shell.text_on_accent(),
    );
}

fn draw_mock_event(
    painter: &egui::Painter,
    shell: &UiShell,
    y: f32,
    x0: f32,
    x1: f32,
    name: &str,
    color: egui::Color32,
) {
    let bar = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1.max(x0 + 60.0), y + 18.0));
    painter.rect_filled(bar, radius::BADGE, color.linear_multiply(0.35));
    painter.rect_stroke(
        bar,
        radius::BADGE,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        egui::pos2(bar.min.x + space::SM, bar.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        proportional(shell.scale.micro),
        shell.t1(),
    );
}

/// 撤销目标图层最后一段街道（面板按钮的落点；UI 不直接改世界）。
pub fn undo_last_street_segment(
    projection: &mut crate::state::WorldProjection,
    drill: &crate::state::DrillState,
    edit: &mut crate::edit::editing::EditHistory,
) {
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let Some(layer) = crate::render::streets::target_road_layer(node) else {
        return;
    };
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
