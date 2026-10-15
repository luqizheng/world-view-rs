//! 时间轴底部通栏（v0.4 · 逐项对位 web 原型 `index.css .timeline`）+ 街道撤销。
//!
//! ## 职责
//! - `TimelineLayer` —— Day / Situation / Character / Century 四层枚举；
//!   顶栏四枚徽标与 web `.track-toggles` 一一对应，点击切层。
//! - `panel_timeline` —— 时间轴浮窗；通栏位置由 [`crate::ui_layout`] 持久化。
//! - `undo_last_street_segment` —— 撤销街道段（UI → `wv-edit::History`，见 §3.3）。
//!
//! ## v0.4 对位表（web `index.css` → 本文件常量）
//!
//! | web | 本文件 |
//! |---|---|
//! | `.timeline{height:164px}`（含边框） | 面板默认尺寸 `[1256,164]`（state.rs），内容内边距 0 |
//! | `.timeline-top{height:50px;padding:0 10px;border-bottom}` | [`TOP_ROW_H`] / [`TOP_PAD_X`] |
//! | `.timeline-top .play`（32px 圆形 accent 底 + 白 glyph） | [`play_button`]（自绘三角 / 双竖条，不依赖字体 glyph） |
//! | `.date{min-width:110px}` | [`DATE_MIN_W`]；主行 = 权威日期（§5.3 唯一入口） |
//! | `.timeline-top .select{width:108px;height:27px}` | [`speed_select`]（bg_app 底 + border_strong 描边） |
//! | `.track-toggles` 四枚徽标 | [`track_chip`]（语义色徽标，选中加浓） |
//! | `.button.primary`（Register event） | 标准工厂 [`button`]（`Accent` + `ICON_PLUS`） |
//! | `.timeline-track` 10% 竖网格 | [`timeline_track`] 首段循环 |
//! | `.years{height:26px;border-bottom}` | [`YEARS_ROW_H`] |
//! | `.event-block{top:38px;height:20px;padding:3px 7px}` | [`BLOCK_TOP`] / [`BLOCK_H`] / [`BLOCK_PAD_X`] |
//! | `.event-dot{top:69px;8px + 25% 光环}` | [`DOT_TOP`] / `DOT_R` |
//! | `.playhead{top:22px}` + 下三角 + `top:72px` 药丸 | [`PLAYHEAD_TOP`] / [`PLAYHEAD_LABEL_TOP`] |
//!
//! ## 边界（不做什么）
//! - **不**改时间语义 —— 日期显示统一入口 [`crate::ui_time`]：顶排主行取
//!   `current_date`（有历法即作者历法），播放头药丸取 `format_date`（紧凑日计）。
//! - **不**直接改世界 —— UI 只生成 `EditCommand`，交给 [`crate::edit::editing::EditHistory`]。
//! - 季节段文案（`Y213 · Winter` …）与无数据时的事件块 mock（`Border dispute` …）
//!   仍是 web 原型的 Figma mock（逐字保留）；M2 接 `Calendar` 季节派生、
//!   M3 接真实事件后替换。
//!
//! ## 主要导出
//! - `TimelineLayer` / `panel_timeline` / `undo_last_street_segment`

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, I18nKey, Locale};
use crate::state::{Panel, PanelState, PlaybackState, WorldProjection};
use crate::theme::ui_theme::{color32, color32_alpha, mono, proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space, BadgeTone};
use crate::ui_layout::LayoutFile;
use crate::ui_time;

use super::shell::UiShell;
use super::widgets::{button, rect_stroke, ButtonContent, ButtonStyle, ICON_PLUS};

// ── 设计量（web `index.css` 常量，1 CSS px = 1 egui pt）─────────────────

/// 顶排高度（web `.timeline-top{height:50px}`）。
const TOP_ROW_H: f32 = 50.0;
/// 顶排左右内边距（web `padding:0 10px`）。
const TOP_PAD_X: f32 = 10.0;
/// 日期块最小宽（web `.date{min-width:110px}`）。
const DATE_MIN_W: f32 = 110.0;
/// 主行与次行间距（web `.date span{margin-top:2px}`）。
const DATE_LINE_GAP: f32 = 2.0;
/// 速度下拉宽（web `.select{width:108px}`）。
const SPEED_SELECT_W: f32 = 108.0;
/// 年份行高度（web `.years{height:26px}`）。
const YEARS_ROW_H: f32 = 26.0;
/// 事件块顶距与高（web `.event-block{top:38px;height:20px}`）。
const BLOCK_TOP: f32 = 38.0;
const BLOCK_H: f32 = 20.0;
/// 事件块左右内边距（web `padding:3px 7px`）。
const BLOCK_PAD_X: f32 = 7.0;
/// 真实事态多行堆叠的行距。
const BLOCK_ROW_GAP: f32 = 6.0;
/// 事件点顶距（web `.event-dot{top:69px}`；圆直径 8 → 半径 4）。
const DOT_TOP: f32 = 69.0;
const DOT_R: f32 = 4.0;
/// 播放头线顶与药丸顶（web `.playhead{top:22px}` / `span{top:72px}`，均相对轨道顶）。
const PLAYHEAD_TOP: f32 = 22.0;
const PLAYHEAD_LABEL_TOP: f32 = 72.0;
/// 播放头药丸最小宽（web `span{width:44px}`）。
const PLAYHEAD_LABEL_MIN_W: f32 = 44.0;

/// 播放速率三档（基线 §1.8）。
const SPEED_STEPS: [f32; 3] = [1.0, 5.0, 30.0];

/// 季节段文案（Figma mock，逐字保留；M2 接 `Calendar` 后按 range 派生）。
const YEAR_SEGMENTS: [&str; 4] = [
    "Y213 · Winter",
    "Y214 · Spring",
    "Y214 · Summer",
    "Y214 · Autumn",
];

/// 无真实事态时的两块 mock（web `App.tsx` `.event-a/.event-b`；M3 接真实数据后删除）。
/// 元组 =（起点 frac，宽度 frac，标题，语义色）。
const MOCK_BLOCKS: [(f32, f32, &str, BadgeTone); 2] = [
    (0.12, 0.25, "Border dispute", BadgeTone::Warn),
    (0.57, 0.16, "Market opens", BadgeTone::Event),
];
/// 事件点横位置（web `.event-dot{left:45%}`）。
const MOCK_DOT_AT: f32 = 0.45;

/// 时间轴轨道层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineLayer {
    /// L1 日：细刻度尺 + 播放头（Figma 段 `DAY TICKS`）。
    Day,
    /// L2 事态：真实事态块（无数据时画 Figma mock），Figma 段 `SITUATIONS`。
    #[default]
    Situation,
    /// L3 人物生卒条（Figma 段 `CHARACTERS`，本条仍是占位轨）。
    Character,
    /// L4 世纪聚合鸟瞰（Figma 段 `CENTURY VIEW`，本条仍是占位轨）。
    Century,
}

impl TimelineLayer {
    /// 层名（ADR-0018：locale-dependent 文案查表）。
    pub fn label(self, loc: Locale) -> &'static str {
        match self {
            TimelineLayer::Day => t(loc, I18nKey::TimelineLayerDay),
            TimelineLayer::Situation => t(loc, I18nKey::TimelineLayerSituation),
            TimelineLayer::Character => t(loc, I18nKey::TimelineLayerCharacter),
            TimelineLayer::Century => t(loc, I18nKey::TimelineLayerCentury),
        }
    }
    /// 徽标语义色（web `.track-toggles`：DAY TICKS 中性 / SITUATIONS warn /
    /// CHARACTERS event / CENTURY VIEW 中性）。
    pub fn chip_tone(self) -> BadgeTone {
        match self {
            TimelineLayer::Day => BadgeTone::Neutral,
            TimelineLayer::Situation => BadgeTone::Warn,
            TimelineLayer::Character => BadgeTone::Event,
            TimelineLayer::Century => BadgeTone::Neutral,
        }
    }
    /// 顶栏 chip 展示的全部层（web `.track-toggles` 有 4 个徽标）。
    pub const VISIBLE: [TimelineLayer; 4] = [
        TimelineLayer::Day,
        TimelineLayer::Situation,
        TimelineLayer::Character,
        TimelineLayer::Century,
    ];
    /// 与 [`Self::VISIBLE`] 同表（命令面板 / 测试用名保留）。
    pub const ALL: [TimelineLayer; 4] = TimelineLayer::VISIBLE;
}

/// 时间轴底部通栏浮窗（v0.4 web 对位）。
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
    current_loc: Res<crate::i18n::CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !panels.is_visible(Panel::Timeline) {
        return;
    }

    let loc = current_loc.0;
    // 沉浸模式：整体淡化但**仍可交互**（D3：状态永不抑制）。
    let alpha = if panels.immersive { 0.55 } else { 1.0 };
    let layout = crate::ui_layout::current_layout(&layouts, &panels, Panel::Timeline);

    egui::Window::new("")
        .id(egui::Id::new(("wv_panel", Panel::Timeline)))
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .resizable(true)
        .title_bar(false)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                // web 的顶排底线 / 轨道网格都是通栏的：面板内容零内边距，
                // 间隔由各段自管（下方 `item_spacing = 0`）。
                .inner_margin(crate::theme::ui_theme::margin_all(0.0))
                .corner_radius(radius::PANEL),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            timeline_top_row(
                ui,
                &shell,
                &mut playback,
                &mut layer,
                &projection,
                loc,
                &mut open_event,
            );
            timeline_track(ui, &shell, &mut playback, &projection, *layer, loc);
        });

    if *open_event {
        panels.set_visible(Panel::Event, true);
        *open_event = false;
    }
}

/// 顶排（web `.timeline-top`，50px）：播放圆钮 + 日期块 + 速度下拉 + 层徽标 + 登记事件。
///
/// 底线画在整排之下、**通栏到面板两缘**（web 是 `.timeline-top` 的 border-bottom）。
fn timeline_top_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    playback: &mut PlaybackState,
    layer: &mut TimelineLayer,
    projection: &WorldProjection,
    loc: Locale,
    open_event: &mut bool,
) {
    let row_rect =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), TOP_ROW_H));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add_space(TOP_PAD_X);
            play_button(ui, shell, playback);
            ui.add_space(space::SM);
            date_block(ui, shell, playback, projection, loc);
            ui.add_space(space::SM);
            speed_select(ui, shell, playback, loc);
            ui.add_space(space::SM);
            // 层徽标 ×4（web `.track-toggles{gap:4px}`，占据弹性区、靠左排列）。
            for l in TimelineLayer::VISIBLE {
                if track_chip(ui, shell, l, *layer == l, loc) {
                    *layer = l;
                }
                ui.add_space(space::XS);
            }
            // 右端：Register event（web `.button.primary`：accent 实底 + 白字 + plus 图标）。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(TOP_PAD_X);
                let label = t(loc, I18nKey::TimelineRegisterEventBtn);
                let resp = button(
                    ui,
                    shell,
                    ButtonContent::ImageText(ICON_PLUS, label),
                    ButtonStyle::Accent,
                    None,
                    true,
                );
                if resp.clicked() {
                    *open_event = true;
                }
            });
        },
    );
    // 底线：通栏到面板两缘（web `.timeline-top{border-bottom}`）。
    let y = row_rect.max.y - 0.5;
    ui.painter().line_segment(
        [egui::pos2(row_rect.min.x, y), egui::pos2(row_rect.max.x, y)],
        egui::Stroke::new(1.0_f32, shell.border_subtle()),
    );
}

/// 圆形播放 / 暂停钮（web `.timeline-top .play`：32×32 圆、accent 底、白 glyph）。
///
/// glyph 自绘（实心三角 / 两条竖条）而不走字体字符：web 图标是 SVG 路径，
/// 用 `▶`/`⏸` 字符会随字体走形（Emoji 字体还会染色）。
fn play_button(ui: &mut egui::Ui, shell: &UiShell, playback: &mut PlaybackState) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 16.0, shell.accent());
    let white = egui::Color32::WHITE;
    if playback.playing {
        // web `pause` 图标：`M9 5v14M15 5v14`（16px 下条高 ≈ 9.3、条宽 ≈ 1.1）。
        let (bar_w, bar_h, gap) = (2.5_f32, 10.0_f32, 4.0_f32);
        for side in [-1.0_f32, 1.0] {
            let cx = rect.center().x + side * gap / 2.0;
            painter.rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(cx, rect.center().y),
                    egui::vec2(bar_w, bar_h),
                ),
                1.0,
                white,
            );
        }
    } else {
        // web `play` 图标：`m8 5 11 7-11 7Z`——右偏 1px 的实心三角（16px 下 ≈7.3×9.3）。
        let c = rect.center() + egui::vec2(1.0, 0.0);
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(c.x - 3.5, c.y - 5.0),
                egui::pos2(c.x - 3.5, c.y + 5.0),
                egui::pos2(c.x + 4.5, c.y),
            ],
            white,
            egui::Stroke::NONE,
        ));
    }
    if resp.clicked() {
        playback.playing = !playback.playing;
    }
}

/// 日期块（web `.date`）：主行 = 权威日期（§5.3 唯一入口），次行 = `Day N · 季节` mock。
///
/// 主行长度随历法变（`公元 1911 年 1 月 1 日` 会比 mock 宽），取两行最宽 + 下限 110，
/// 保证右侧速度下拉与徽标的起点不随日期跳变。
fn date_block(
    ui: &mut egui::Ui,
    shell: &UiShell,
    playback: &PlaybackState,
    projection: &WorldProjection,
    loc: Locale,
) {
    let main = ui_time::current_date(projection, playback, loc);
    let sub = format!(
        "Day {} · {}",
        playback.current_day,
        season_of_day(playback.current_day)
    );
    let painter = ui.painter();
    let g_main = painter.layout_no_wrap(main, proportional(shell.scale.small), shell.t1());
    let g_sub = painter.layout_no_wrap(sub, mono(shell.scale.micro), shell.t3());
    let w = g_main.size().x.max(g_sub.size().x).max(DATE_MIN_W);
    let h = g_main.size().y + DATE_LINE_GAP + g_sub.size().y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter();
    let main_h = g_main.size().y;
    p.galley(rect.min, g_main, shell.t1());
    p.galley(
        egui::pos2(rect.min.x, rect.min.y + main_h + DATE_LINE_GAP),
        g_sub,
        shell.t3(),
    );
}

/// 速度下拉（web `.timeline-top .select`：108×27、bg_app 底 + border_strong 描边 + 6 圆角）。
fn speed_select(ui: &mut egui::Ui, shell: &UiShell, playback: &mut PlaybackState, loc: Locale) {
    let label = |s: f32| {
        let suffix = if s <= 1.0 {
            t(loc, I18nKey::TimelineSpeedPillSuffix)
        } else {
            t(loc, I18nKey::TimelineSpeedPillSuffixPlural)
        };
        format!("{s:.0}{suffix}")
    };
    ui.scope(|ui| {
        let style = ui.style_mut();
        let w = &mut style.visuals.widgets;
        // web `.field`：底色是窗口底（比面板更暗），描边用外描边色。
        w.inactive.bg_fill = color32(shell.palette.bg_app);
        w.inactive.bg_stroke = egui::Stroke::new(1.0, shell.border_strong());
        w.inactive.corner_radius = radius::CONTROL.into();
        w.hovered.bg_fill = color32(shell.palette.bg_app);
        w.hovered.bg_stroke = egui::Stroke::new(1.0, shell.accent());
        w.hovered.corner_radius = radius::CONTROL.into();
        w.active.corner_radius = radius::CONTROL.into();
        style.spacing.button_padding = egui::vec2(space::SM, 6.0);
        egui::ComboBox::new("wv_timeline_speed", "")
            // 宽含右侧 chevron 与描边，≈ web 的 108。
            .width(SPEED_SELECT_W - 4.0)
            .selected_text(egui::RichText::new(label(playback.speed)).size(shell.scale.small))
            .show_ui(ui, |ui| {
                for s in SPEED_STEPS {
                    if ui
                        .selectable_label((playback.speed - s).abs() < f32::EPSILON, label(s))
                        .clicked()
                    {
                        playback.speed = s;
                    }
                }
            });
    });
}

/// 层徽标（web `.badge`：9px 粗体 + 2px/6px 内边距 + 4 圆角 + 语义色描边与浅底）。
///
/// 选中态 = 底色加浓 + 描边实色（web 徽标是纯展示，没有选中态；本应用把它做成
/// 切层的开关，故在 web 基线之上叠加一档强调）。返回是否被点击。
fn track_chip(
    ui: &mut egui::Ui,
    shell: &UiShell,
    layer: TimelineLayer,
    selected: bool,
    loc: Locale,
) -> bool {
    let tone = layer.chip_tone();
    let colors = shell.palette.badge(tone);
    let fg = color32(colors.fg);
    let galley = ui.painter().layout_no_wrap(
        layer.label(loc).to_owned(),
        proportional(shell.scale.micro),
        fg,
    );
    let pad = egui::vec2(space::SM, 3.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + pad * 2.0, egui::Sense::click());
    let painter = ui.painter();
    let (bg, border) = match tone {
        BadgeTone::Neutral => (
            if selected {
                shell.active_bg()
            } else {
                shell.raised_bg()
            },
            shell.border_strong(),
        ),
        _ => {
            let alpha = if selected {
                0.32
            } else if resp.hovered() {
                0.22
            } else {
                0.15
            };
            (
                color32(colors.fill.over(alpha, shell.palette.bg_panel)),
                color32_alpha(colors.fill, if selected { 1.0 } else { 0.55 }),
            )
        }
    };
    painter.rect_filled(rect, radius::BADGE, bg);
    rect_stroke(
        painter,
        rect,
        radius::BADGE,
        egui::Stroke::new(1.0_f32, border),
    );
    painter.galley(rect.min + pad, galley, fg);
    resp.clicked()
}

/// 主轨道（web `.timeline-track`）：10% 竖网格 + 年份行 + 层内容 + 播放头。
///
/// 整轨可点可拖：位置 → `current_day`（web 用底部隐形 slider，本实现把命中区
/// 扩到全轨，可视结果一致、操作更容易）。
fn timeline_track(
    ui: &mut egui::Ui,
    shell: &UiShell,
    playback: &mut PlaybackState,
    projection: &WorldProjection,
    layer: TimelineLayer,
    loc: Locale,
) {
    let size = egui::vec2(ui.available_width(), ui.available_height().max(60.0));
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let x_at = |frac: f32| rect.min.x + frac.clamp(0.0, 1.0) * rect.width();
    let x_of = |day: i64| x_at(day_to_frac(day, playback.range_from, playback.range_to));

    // ── 刻度网格：每 10% 一条竖线（web `repeating-linear-gradient(90deg …)`）──
    for i in 1..10 {
        let x = x_at(i as f32 / 10.0);
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0_f32, shell.border_subtle()),
        );
    }

    // ── 年份行（web `.years`：26px、四段均分、底线；文字是 Figma mock）──
    let seg_w = rect.width() / YEAR_SEGMENTS.len() as f32;
    for (i, s) in YEAR_SEGMENTS.iter().enumerate() {
        painter.text(
            egui::pos2(
                rect.min.x + seg_w * (i as f32 + 0.5),
                rect.min.y + YEARS_ROW_H / 2.0,
            ),
            egui::Align2::CENTER_CENTER,
            *s,
            proportional(shell.scale.micro),
            shell.t3(),
        );
    }
    let years_line_y = rect.min.y + YEARS_ROW_H - 0.5;
    painter.line_segment(
        [
            egui::pos2(rect.min.x, years_line_y),
            egui::pos2(rect.max.x, years_line_y),
        ],
        egui::Stroke::new(1.0_f32, shell.border_subtle()),
    );

    // ── 层内容 ──
    match layer {
        TimelineLayer::Day => {
            // 细刻度尺（每 5% 一格）。
            for i in 0..=20 {
                let x = x_at(i as f32 / 20.0);
                painter.line_segment(
                    [
                        egui::pos2(x, rect.min.y + YEARS_ROW_H),
                        egui::pos2(x, rect.min.y + YEARS_ROW_H + 8.0),
                    ],
                    egui::Stroke::new(1.0_f32, shell.border_strong()),
                );
            }
        }
        TimelineLayer::Situation => {
            let block_y = rect.min.y + BLOCK_TOP;
            if projection.world.situations.is_empty() {
                // Figma mock（web `App.tsx` `.event-a/.event-b/.event-dot`）。
                for (start, width, name, tone) in MOCK_BLOCKS {
                    event_block(
                        &painter,
                        shell,
                        x_at(start),
                        x_at(start + width),
                        block_y,
                        name,
                        tone,
                    );
                }
                let dot = shell.palette.badge(BadgeTone::Event).fill;
                let dot_p = egui::pos2(x_at(MOCK_DOT_AT), rect.min.y + DOT_TOP + DOT_R);
                painter.circle_filled(dot_p, DOT_R + 3.0, color32_alpha(dot, 0.25));
                painter.circle_filled(dot_p, DOT_R, color32(dot));
            } else {
                // 真实事态：与 mock 同一视觉语言（Event 语义色），按 span 定位、逐行堆叠。
                for (i, s) in projection.world.situations.iter().enumerate() {
                    let y = block_y + i as f32 * (BLOCK_H + BLOCK_ROW_GAP);
                    let x0 = x_of(s.span.from);
                    let x1 = x_of(s.span.to.unwrap_or(playback.range_to)).max(x0 + 60.0);
                    event_block(&painter, shell, x0, x1, y, &s.name, BadgeTone::Event);
                }
            }
        }
        TimelineLayer::Character | TimelineLayer::Century => {
            // 占位轨：等距细线（切到这两层时不至于空白；真实数据 M3 再接）。
            for i in 0..3 {
                let y = rect.min.y + BLOCK_TOP + i as f32 * 24.0;
                painter.line_segment(
                    [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                    egui::Stroke::new(1.0_f32, shell.border_subtle()),
                );
            }
        }
    }

    // ── 播放头（web `.playhead`：1px 金线 + 顶部**向下**三角 + 底部药丸）──
    let cx = x_of(playback.current_day);
    painter.line_segment(
        [
            egui::pos2(cx, rect.min.y + PLAYHEAD_TOP),
            egui::pos2(cx, rect.max.y),
        ],
        egui::Stroke::new(1.0_f32, shell.focus()),
    );
    let tri_y = rect.min.y + PLAYHEAD_TOP;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(cx - 4.0, tri_y),
            egui::pos2(cx + 4.0, tri_y),
            egui::pos2(cx, tri_y + 7.0),
        ],
        shell.focus(),
        egui::Stroke::NONE,
    ));
    // 药丸：紧凑日计（ui_time 同一入口；金底 + on-accent 深字，随主题翻转）。
    let on_gold = shell.text_on_accent();
    let galley = painter.layout_no_wrap(
        ui_time::format_date(playback.current_day, loc),
        proportional(shell.scale.micro),
        on_gold,
    );
    let pill_w = (galley.size().x + space::SM).max(PLAYHEAD_LABEL_MIN_W);
    let pill_h = galley.size().y + space::XS;
    let pill_top =
        (rect.min.y + PLAYHEAD_TOP + PLAYHEAD_LABEL_TOP).min(rect.max.y - pill_h - DATE_LINE_GAP);
    let pill_cx = pill_center_x(cx, pill_w, rect.min.x, rect.max.x);
    let pill = egui::Rect::from_min_size(
        egui::pos2(pill_cx - pill_w / 2.0, pill_top),
        egui::vec2(pill_w, pill_h),
    );
    painter.rect_filled(pill, 3.0, shell.focus());
    painter.galley(
        egui::pos2(pill.min.x + space::XS, pill.min.y + DATE_LINE_GAP),
        galley,
        on_gold,
    );

    // ── 整轨定位（点 / 拖 → 当前时刻）──
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let frac = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            let days = (playback.range_to - playback.range_from).max(1);
            playback.current_day = playback.range_from + (frac * days as f32) as i64;
        }
    }
}

/// 单个事件块（web `.event-block`：深底 + 语义色描边 + 语义色文字 + 4 圆角）。
///
/// 底色用「语义 fill × 25% 合成在面板底」逼近 web 的既定深色（如 `#4a3b21`），
/// 不写裸 hex；文字过长时超出块宽（与 web 一致，靠轨道裁切兜底）。
fn event_block(
    painter: &egui::Painter,
    shell: &UiShell,
    x0: f32,
    x1: f32,
    y: f32,
    name: &str,
    tone: BadgeTone,
) {
    let colors = shell.palette.badge(tone);
    let fg = color32(colors.fg);
    let bg = color32(colors.fill.over(0.25, shell.palette.bg_panel));
    let border = color32_alpha(colors.fill, 0.55);
    let galley = painter.layout_no_wrap(name.to_owned(), proportional(shell.scale.micro), fg);
    let w = (x1 - x0).max(galley.size().x + BLOCK_PAD_X * 2.0);
    let bar = egui::Rect::from_min_size(egui::pos2(x0, y), egui::vec2(w, BLOCK_H));
    painter.rect_filled(bar, radius::BADGE, bg);
    rect_stroke(
        painter,
        bar,
        radius::BADGE,
        egui::Stroke::new(1.0_f32, border),
    );
    painter.galley(
        egui::pos2(
            bar.min.x + BLOCK_PAD_X,
            bar.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
}

/// 天数 → 轨道横向比例（0..1，越界夹取）。
///
/// 抽出为纯函数：拖动定位 / 事态块 / 播放头共用同一映射，也是单测的落点。
fn day_to_frac(day: i64, from: i64, to: i64) -> f32 {
    let span = (to - from).max(1) as f32;
    ((day - from) as f32 / span).clamp(0.0, 1.0)
}

/// 播放头药丸的横向中心：默认与线同 x，贴近两端时整体夹进轨道内。
///
/// web 的 day 从 1 起、播放头永远到不了 0%（药丸不会出界）；本应用
/// `range_from` 恰在 0%，不夹就会有一半被 `painter_at` 裁掉。
fn pill_center_x(line_x: f32, pill_w: f32, track_min_x: f32, track_max_x: f32) -> f32 {
    line_x.clamp(track_min_x + pill_w / 2.0, track_max_x - pill_w / 2.0)
}

/// `Day N · 季节` 的次行 mock（v0.3 占位）；M2 接 `wv_core::Calendar` 后替换。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 范围端点必须映射到轨道两端：start → 0、end → 1（web `day/1.2%` 的 0.83..100
    /// 是本应用 `range_from` 基准下的等价形）。
    #[test]
    fn day_to_frac_maps_range_endpoints_to_track_edges() {
        assert!(
            (day_to_frac(1, 1, 120) - 0.0).abs() < 1e-6,
            "start should be 0"
        );
        assert!(
            (day_to_frac(120, 1, 120) - 1.0).abs() < 1e-6,
            "end should be 1"
        );
        // 中点：range 1..=120 的几何中点 ≈ 60.5 → 0.5。
        assert!(
            (day_to_frac(60, 1, 120) - 0.495_798_3).abs() < 1e-4,
            "got {}",
            day_to_frac(60, 1, 120)
        );
    }

    /// 越界天数夹取到 [0,1]，不允许画到轨道外。
    #[test]
    fn day_to_frac_clamps_out_of_range_days() {
        assert!(
            (day_to_frac(0, 1, 120) - 0.0).abs() < 1e-6,
            "below start clamps to 0"
        );
        assert!(
            (day_to_frac(10_000, 1, 120) - 1.0).abs() < 1e-6,
            "past end clamps to 1"
        );
        // 零长区间（from == to）不得除零：span 兜底为 1。
        let f = day_to_frac(5, 5, 5);
        assert!(f.is_finite(), "degenerate range must stay finite, got {f}");
    }

    /// 季节划分的边界（90 天一格，360 天回卷）。
    #[test]
    fn season_of_day_spans_ninety_day_buckets() {
        assert_eq!(season_of_day(1), "Early Spring", "day 1");
        assert_eq!(season_of_day(90), "Early Spring", "last day of bucket 0");
        assert_eq!(season_of_day(91), "Summer", "first day of bucket 1");
        assert_eq!(season_of_day(180), "Summer");
        assert_eq!(season_of_day(181), "Autumn");
        assert_eq!(season_of_day(270), "Autumn");
        assert_eq!(season_of_day(271), "Winter");
        assert_eq!(season_of_day(360), "Winter");
        assert_eq!(season_of_day(361), "Early Spring", "wraps into next year");
    }

    /// 层名走 i18n 查表：zh / en 两表都得有值。
    #[test]
    fn timeline_layer_labels_follow_locale() {
        assert_eq!(TimelineLayer::Day.label(Locale::Zh), "日");
        assert_eq!(TimelineLayer::Situation.label(Locale::En), "Situation");
        assert_eq!(TimelineLayer::Character.label(Locale::Zh), "人物");
        assert_eq!(TimelineLayer::Century.label(Locale::En), "Century");
    }

    /// 徽标语义色与 web `.track-toggles` 对位：SITUATIONS warn、CHARACTERS event、其余中性。
    #[test]
    fn timeline_layer_chip_tones_match_web_badges() {
        assert_eq!(TimelineLayer::Day.chip_tone(), BadgeTone::Neutral);
        assert_eq!(TimelineLayer::Situation.chip_tone(), BadgeTone::Warn);
        assert_eq!(TimelineLayer::Character.chip_tone(), BadgeTone::Event);
        assert_eq!(TimelineLayer::Century.chip_tone(), BadgeTone::Neutral);
    }

    /// mock 事件块必须完整落在轨道内（web `.event-a{left:12%;width:25%}` 等）。
    #[test]
    fn mock_blocks_sit_inside_track_bounds() {
        for (start, width, name, _) in MOCK_BLOCKS {
            assert!(
                start >= 0.0 && start + width <= 1.0,
                "mock block {name} out of track: {start}+{width}"
            );
        }
        assert!(
            (0.0..=1.0).contains(&MOCK_DOT_AT),
            "dot at {MOCK_DOT_AT} outside track"
        );
    }

    /// 播放头药丸的底边永不越过轨道下沿（保证 114 轨道内可读）。
    #[test]
    fn playhead_pill_fits_within_track_height() {
        let track_h = 114.0;
        let pill_h = 11.0 * 1.4 + space::XS; // micro 字号行高上界 + 上下内边距
        let top = (PLAYHEAD_TOP + PLAYHEAD_LABEL_TOP).min(track_h - pill_h - DATE_LINE_GAP);
        assert!(
            top + pill_h <= track_h,
            "pill bottom {} exceeds track {track_h}",
            top + pill_h
        );
        assert!(
            top >= PLAYHEAD_TOP,
            "pill must stay below the playhead triangle"
        );
    }

    /// 药丸横向不许越轨：两端各夹半个药丸宽，中间保持与线同 x。
    #[test]
    fn playhead_pill_center_stays_inside_track() {
        let (min_x, max_x, w) = (0.0_f32, 1000.0_f32, 44.0_f32);
        // 左端：day = range_from → 线在 0 → 药丸中心夹到 22。
        assert!((pill_center_x(0.0, w, min_x, max_x) - 22.0).abs() < 1e-3);
        // 右端：线贴右沿 → 中心夹到 978。
        assert!((pill_center_x(1000.0, w, min_x, max_x) - 978.0).abs() < 1e-3);
        // 中间不动。
        assert!((pill_center_x(500.0, w, min_x, max_x) - 500.0).abs() < 1e-3);
    }
}
