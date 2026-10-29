//! 时间轴底部通栏（v0.4 · 逐项对位 web 原型 `index.css .timeline`）+ 街道撤销。
//!
//! ## v0.5 停靠化（2026-10-27 用户裁定）
//! - **默认占满宽度、贴状态栏上方**：浮窗几何每帧由视口重推
//!   （[`dock_rect`]），`ui-layout.yaml` 的 x/y/width/height 对播放器不再
//!   生效（`collapsed` 字段仍被消费）——也就没有「落盘坐标把面板丢在
//!   视口外」一类问题，`snap_into_central` 越界回拉随之退役。
//! - **可缩小成播放条**：折叠态只剩「播放钮 + 细进度轨 + 日期药丸」单行
//!   （[`compact_bar`]）；折叠态是独立会话缓存（[`timeline_collapsed`]，
//!   egui persisted bool），跨会话真相在 `ui-layout.yaml` 的 `collapsed`
//!   字段——工程打开时由 `load_layout_on_project_open` 回灌、切换时由
//!   `persist_layout_on_change`（Timeline 特判）落盘。
//!   ⚠️ 不能借用窗口同 Id 的 `"collapsing"` 子位：直连 `egui::Window` 把
//!   它当**窗体内容的闸门**（`show_body_unindented` 在关闭态整个不渲染
//!   body），折叠会变成「窗口消失」。
//!
//! ## 职责
//! - `TimelineLayer` —— Day / Situation / Character / Century 四层枚举；
//!   顶栏四枚徽标与 web `.track-toggles` 一一对应，点击切层。
//! - `panel_timeline` —— 时间轴浮窗；停靠几何见 [`dock_rect`]。
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
use crate::state::{Panel, PanelState, PlaybackState, ViewMode, ViewState, WorldProjection};
use crate::theme::ui_theme::{color32, color32_alpha, mono, proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space, BadgeTone};
use crate::ui_layout::LayoutFile;
use crate::ui_time;

use super::shell::UiShell;
use super::widgets::{
    button, rect_stroke, ButtonContent, ButtonStyle, ICON_CHEVRON_DOWN, ICON_CHEVRON_UP, ICON_PLUS,
};

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
/// 播放头药丸半径（web `span` 药丸的圆角当量；线本身 1px）。
const DOT_R: f32 = 4.0;
/// 播放头线顶与药丸顶（web `.playhead{top:22px}` / `span{top:72px}`，均相对轨道顶）。
const PLAYHEAD_TOP: f32 = 22.0;
const PLAYHEAD_LABEL_TOP: f32 = 72.0;
/// 播放头药丸最小宽（web `span{width:44px}`）。
const PLAYHEAD_LABEL_MIN_W: f32 = 44.0;
/// 展开态（完整播放器）高度——web `.timeline{height:164px}` 同口径。
const EXPANDED_H: f32 = 164.0;
/// 折叠态（播放条）高度：32px 播放钮 + 上下各 6px 呼吸。
const COMPACT_H: f32 = 44.0;

/// 播放速率三档（基线 §1.8）。
const SPEED_STEPS: [f32; 3] = [1.0, 5.0, 30.0];

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

/// 离开 Browse 时自动暂停播放——「播放器只在 Browse」裁定（2026-10-27）的
/// 行为侧兜底。判据本体在 `ViewMode::timeline_shows`（wv-render，与
/// `advance_playback` 共用）；这里补的是**跳变暂停**：播放器不在场时时间
/// 冻结已由推进侧门控保证，但 `playing` 旗标若仍亮着，播放锁（§9.4 Gate-1）
/// 会灰着全部写入口工具——一个用户既看不见也解不开的锁。在 Browse 按下
/// 播放、切到 Map/CellMap 即暂停，切回不自动续播（是否续播由作者再按）。
pub fn pause_playback_off_browse(
    view: Res<ViewState>,
    mut playback: ResMut<PlaybackState>,
    mut prev_mode: Local<Option<ViewMode>>,
) {
    if let Some(prev) = *prev_mode {
        if playback_leaves_browse(prev, view.mode) {
            playback.playing = false;
        }
    }
    *prev_mode = Some(view.mode);
}

/// 纯判定：从 Browse 切往其他模式的那一跳才暂停（进入/逗留 Browse 不动）。
fn playback_leaves_browse(prev: ViewMode, next: ViewMode) -> bool {
    prev.timeline_shows() && !next.timeline_shows()
}

/// 折叠态会话缓存的存储位（egui persisted bool，进程内跨帧）。
///
/// 刻意**避开**窗口同 Id 的 `"collapsing"` 子位——那是直连 `egui::Window`
/// 自己的窗体内容闸门（借它会「折叠 = 整窗消失」，见模块文档）。跨会话
/// 真相在 `ui-layout.yaml` 的 `collapsed`：工程打开时
/// `load_layout_on_project_open` 回灌本缓存，切换时
/// `persist_layout_on_change` 对 Timeline 特判读取并落盘。
pub(crate) fn timeline_collapse_id() -> egui::Id {
    egui::Id::new(("wv_panel", Panel::Timeline)).with("playbar_collapsed")
}

/// 读播放条折叠态；`None` = 本会话尚未初始化（调用方回退落盘值）。
pub(crate) fn timeline_collapsed(ctx: &egui::Context) -> Option<bool> {
    ctx.data_mut(|d| d.get_persisted::<bool>(timeline_collapse_id()))
}

/// 播放器停靠矩形（2026-10-27 用户裁定：默认占满宽度、贴状态栏上方）。
///
/// 横向 = 整个视口；纵向 = 底边贴在状态栏上沿（`vp.bottom − STATUS_BAR_H`），
/// 高度按折叠态取档（展开 [`EXPANDED_H`] / 播放条 [`COMPACT_H`]）。每帧从
/// 视口重推：窗口跟随主窗缩放，落盘的 x/y/width/height 对播放器不再生效
/// （`ui-layout.yaml` 仍记录几何，但只有 `collapsed` 被消费）。
///
/// 抽出为纯函数供单测；唯一消费方是 [`panel_timeline`]。
fn dock_rect(vp: egui::Rect, collapsed: bool) -> (egui::Pos2, egui::Vec2) {
    let h = if collapsed { COMPACT_H } else { EXPANDED_H };
    (
        egui::pos2(
            vp.left(),
            vp.bottom() - crate::ui::top_bar::STATUS_BAR_H - h,
        ),
        egui::vec2(vp.width(), h),
    )
}

/// 时间轴底部停靠条（v0.5 停靠化；展开态 = v0.4 web 对位的完整播放器）。
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
    view: Res<ViewState>,
    mut layer: Local<TimelineLayer>,
    mut open_event: Local<bool>,
    // UX-002：「+ 登记事件」从「只开只读列表」升级为打开创建对话框。
    mut event_create: ResMut<crate::ui::event_create_dialog::EventCreateDialog>,
    current_loc: Res<crate::i18n::CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if !panels.is_visible(Panel::Timeline) || !view.mode.timeline_shows() {
        return;
    }

    let loc = current_loc.0;
    // 沉浸模式：整体淡化但**仍可交互**（D3：状态永不抑制）。
    let alpha = if panels.immersive { 0.55 } else { 1.0 };
    let layout = crate::ui_layout::current_layout(&layouts, &panels, Panel::Timeline);
    let win_id = egui::Id::new(("wv_panel", Panel::Timeline));

    // 折叠态（完整播放器 ↔ 播放条，2026-10-27）：会话缓存优先
    //（[`timeline_collapsed`]），缓存缺失（本会话首帧）回退落盘值——
    // 工程打开时 `load_layout_on_project_open` 会把落盘值回灌进缓存，
    // 这里只是兜底时序。
    let mut collapsed = timeline_collapsed(ctx).unwrap_or(layout.collapsed);

    // 底部停靠（2026-10-27 用户裁定）：几何每帧由视口重推（[`dock_rect`]），
    // `fixed_rect` 同时关掉拖动与拖角缩放——「缩小」由折叠钮承担。旧的自由
    // 浮窗 + 越界回拉（`snap_into_central`）随停靠化退役：位置不再来自记忆，
    // 也就不存在拖丢 / 盖住状态栏的问题。
    let (pos, size) = dock_rect(ctx.viewport_rect(), collapsed);
    egui::Window::new("")
        .id(win_id)
        .fixed_rect(egui::Rect::from_min_size(pos, size))
        .title_bar(false)
        .frame(
            // 跟随应用当前日夜模式（面板底仍由 shell 令牌决定）。
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                // web 的顶排底线 / 轨道网格都是通栏的：面板内容零内边距，
                // 间隔由各段自管（下方 `item_spacing = 0`）。
                .inner_margin(crate::theme::ui_theme::margin_all(0.0))
                .corner_radius(radius::PANEL),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if collapsed {
                compact_bar(ui, &shell, &mut playback, loc, &mut collapsed);
            } else {
                timeline_top_row(
                    ui,
                    &shell,
                    &mut playback,
                    &mut layer,
                    &projection,
                    loc,
                    &mut open_event,
                    &mut collapsed,
                    &mut event_create,
                );
                timeline_track(ui, &shell, &mut playback, &projection, *layer, loc);
            }
        });

    // 会话缓存写回：本帧 chevron 翻转过才写（persist 对 Timeline 特判读本
    // 缓存并落盘 `ui-layout.yaml` 的 `collapsed` 字段）。
    if timeline_collapsed(ctx) != Some(collapsed) {
        ctx.data_mut(|d| d.insert_persisted(timeline_collapse_id(), collapsed));
    }

    if *open_event {
        panels.set_visible(Panel::Event, true);
        *open_event = false;
    }
}

/// 顶排（web `.timeline-top`，50px）：播放圆钮 + 日期块 + 速度下拉 + 层徽标 + 登记事件。
///
/// 底线画在整排之下、**通栏到面板两缘**（web 是 `.timeline-top` 的 border-bottom）。
#[allow(clippy::too_many_arguments)]
fn timeline_top_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    playback: &mut PlaybackState,
    layer: &mut TimelineLayer,
    projection: &WorldProjection,
    loc: Locale,
    open_event: &mut bool,
    collapsed: &mut bool,
    event_create: &mut crate::ui::event_create_dialog::EventCreateDialog,
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
            // 右端：折叠钮（最右缘，收起 = 缩成播放条）+ Register event
            //（web `.button.primary`：accent 实底 + 白字 + plus 图标）。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(TOP_PAD_X);
                collapse_chevron(ui, shell, collapsed, loc);
                ui.add_space(space::XS);
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
                    // UX-002：打开创建对话框（按当前播放日预填跨度）；
                    // 只读列表照旧亮出，登记完成后也能立刻看到结果。
                    crate::ui::event_create_dialog::open_with_playback_defaults(
                        event_create,
                        playback,
                    );
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

/// 折叠态播放条（2026-10-27「可以缩小，变为只有播放条」）：单行 44px——
/// 播放钮 + 细进度轨 + 展开钮。
///
/// 日期药丸随播放头悬浮在轨上方；层徽标 / 速度 / 登记事件 / 年份行 / 层
/// 内容都只属于展开态（完整播放器）。
fn compact_bar(
    ui: &mut egui::Ui,
    shell: &UiShell,
    playback: &mut PlaybackState,
    loc: Locale,
    collapsed: &mut bool,
) {
    let row_rect =
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), COMPACT_H));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(row_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add_space(TOP_PAD_X);
            play_button(ui, shell, playback);
            ui.add_space(space::SM);
            compact_track(ui, shell, playback, loc);
            ui.add_space(space::SM);
            collapse_chevron(ui, shell, collapsed, loc);
            ui.add_space(TOP_PAD_X);
        },
    );
}

/// 播放条细轨：基线 + 已播进度（focus 金）+ 播放头圆点 + 悬浮日期药丸。
///
/// 点 / 拖定位与主轨同语义（[`seek_to_pointer`]）；药丸横向夹进轨道
///（[`pill_center_x`]，与主轨同款）。
fn compact_track(ui: &mut egui::Ui, shell: &UiShell, playback: &mut PlaybackState, loc: Locale) {
    // 右侧让位：展开钮（`shell.hit` 方块）+ 间距 + 右缘内边距。
    let reserve = shell.hit + space::SM + TOP_PAD_X;
    let w = (ui.available_width() - reserve).max(120.0);
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(w, COMPACT_H), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let frac = day_to_frac(playback.current_day, playback.range_from, playback.range_to);
    let cx = rect.min.x + frac * rect.width();
    let line_y = rect.center().y + 6.0; // 基线略低，给药丸留出上方空间
                                        // 基线（暗）+ 已播段（金）+ 播放头圆点。
    painter.line_segment(
        [
            egui::pos2(rect.min.x, line_y),
            egui::pos2(rect.max.x, line_y),
        ],
        egui::Stroke::new(2.0_f32, shell.border_subtle()),
    );
    painter.line_segment(
        [egui::pos2(rect.min.x, line_y), egui::pos2(cx, line_y)],
        egui::Stroke::new(3.0_f32, shell.focus()),
    );
    painter.circle_filled(egui::pos2(cx, line_y), DOT_R, shell.focus());
    // 日期药丸：悬浮在基线上方（紧凑日计，与主轨播放头同一入口）。
    let on_gold = shell.text_on_accent();
    let galley = painter.layout_no_wrap(
        ui_time::format_date(playback.current_day, loc),
        proportional(shell.scale.micro),
        on_gold,
    );
    let pill_w = (galley.size().x + space::SM).max(PLAYHEAD_LABEL_MIN_W);
    let pill_h = galley.size().y + space::XS;
    let pill = egui::Rect::from_min_size(
        egui::pos2(
            pill_center_x(cx, pill_w, rect.min.x, rect.max.x) - pill_w / 2.0,
            (line_y - 6.0 - pill_h).max(rect.min.y + DATE_LINE_GAP),
        ),
        egui::vec2(pill_w, pill_h),
    );
    painter.rect_filled(pill, 3.0, shell.focus());
    painter.galley(
        egui::pos2(pill.min.x + space::XS, pill.min.y + DATE_LINE_GAP),
        galley,
        on_gold,
    );
    seek_to_pointer(&resp, rect, playback);
}

/// 折叠 / 展开切换钮（Lucide chevron 对偶：展开态显 `chevron-down` =
/// 收成播放条；播放条态显 `chevron-up` = 展回完整播放器）。返回是否被点击。
fn collapse_chevron(ui: &mut egui::Ui, shell: &UiShell, collapsed: &mut bool, loc: Locale) -> bool {
    let (icon, hint) = if *collapsed {
        (ICON_CHEVRON_UP, I18nKey::TimelineExpandHint)
    } else {
        (ICON_CHEVRON_DOWN, I18nKey::TimelineCollapseHint)
    };
    let resp = button(
        ui,
        shell,
        ButtonContent::Image(icon),
        ButtonStyle::Ghost,
        None,
        true,
    )
    .on_hover_text(t(loc, hint));
    if resp.clicked() {
        *collapsed = !*collapsed;
    }
    resp.clicked()
}

/// 点 / 拨轨道 → 当前时刻（主轨与播放条共用；web 用底部隐形 slider，
/// 本实现把命中区扩到全轨，可视结果一致、操作更容易）。
fn seek_to_pointer(resp: &egui::Response, rect: egui::Rect, playback: &mut PlaybackState) {
    if resp.dragged() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let frac = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
            let days = (playback.range_to - playback.range_from).max(1);
            playback.current_day = playback.range_from + (frac * days as f32) as i64;
        }
    }
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
    // UX-011：季节从历法真实月份派生（此前 90 天等分是 mock，与真实历法月份错位）。
    let sub = format!(
        "Day {} · {}",
        playback.current_day,
        season_of_day(
            crate::ui_time::calendar_of(projection),
            playback.current_day
        )
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
/// 整轨可点可拖（[`seek_to_pointer`]：位置 → `current_day`）。
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

    // ── 年份行（web `.years`：26px + 底线）──
    // UX-011：刻度按播放区间与历法**真实派生**（此前是 Y213·Winter 等 Figma
    // mock 串，与「无仙 1 年」的真实日期同屏自相矛盾）。
    let years = year_segments(
        playback.range_from,
        playback.range_to,
        crate::ui_time::calendar_of(projection),
        loc,
    );
    for (label, frac) in &years {
        painter.text(
            egui::pos2(x_at(*frac), rect.min.y + YEARS_ROW_H / 2.0),
            egui::Align2::CENTER_CENTER,
            label,
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
                // UX-011：空态引导替代 Figma mock（此前画假块，作者会当成
                // 真实数据做创作决策）。
                let cx = rect.center();
                painter.text(
                    egui::pos2(cx.x, cx.y - 8.0),
                    egui::Align2::CENTER_CENTER,
                    t(loc, I18nKey::SituationPanelEmptyTitle),
                    proportional(shell.scale.small),
                    shell.t2(),
                );
                painter.text(
                    egui::pos2(cx.x, cx.y + 12.0),
                    egui::Align2::CENTER_CENTER,
                    t(loc, I18nKey::SituationPanelEmptyHint),
                    proportional(shell.scale.micro),
                    shell.t3(),
                );
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
    seek_to_pointer(&resp, rect, playback);
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

/// UX-011：年份行刻度——按播放区间与历法**真实派生**。
///
/// 返回 `(标签, 区间内中点 frac)`。有历法时逐「年」出段（年首日经
/// `Calendar::to_day` 求逆，跨年段与区间端点取交）；年份多于 10 个时按
/// 步长抽稀（`ceil(n/10)`），保证 26px 行内放得下。无历法回退四等分
/// 「第 N 天」标签（与 `Calendar::format` 的无历法输出同语言）。
///
/// 纯函数：单测钉住「简单历法跨 2 年出 2 段 / 抽稀 / 无历法回退」三个面。
fn year_segments(
    from: i64,
    to: i64,
    calendar: Option<&wv_core::Calendar>,
    locale: Locale,
) -> Vec<(String, f32)> {
    let span = (to - from).max(1);
    let Some(cal) = calendar else {
        return [0.125_f32, 0.375, 0.625, 0.875]
            .iter()
            .map(|f| {
                let day = from + (span as f32 * f) as i64;
                (ui_time::format_date(day, locale), *f)
            })
            .collect();
    };
    let y0 = cal.to_ymd(from).0;
    let y1 = cal.to_ymd((to - 1).max(from)).0;
    let years = (y1 - y0 + 1).max(1);
    let step = (((years as f32) / 10.0).ceil() as i32).max(1);
    let mut out = Vec::new();
    for y in (y0..=y1).step_by(step as usize) {
        let d_start = cal.to_day(y, 1, 1).unwrap_or(from).max(from);
        let d_end = cal.to_day(y + 1, 1, 1).unwrap_or(to).min(to);
        if d_end <= d_start {
            continue;
        }
        let mid = (d_start + d_end) as f32 / 2.0;
        let frac = ((mid - from as f32) / span as f32).clamp(0.0, 1.0);
        let label = match &cal.prefix {
            Some(p) if !p.is_empty() => format!("{p} {y} 年"),
            _ => format!("{y}"),
        };
        out.push((label, frac));
    }
    out
}

/// 播放头药丸的横向中心：默认与线同 x，贴近两端时整体夹进轨道内。
///
/// web 的 day 从 1 起、播放头永远到不了 0%（药丸不会出界）；本应用
/// `range_from` 恰在 0%，不夹就会有一半被 `painter_at` 裁掉。
fn pill_center_x(line_x: f32, pill_w: f32, track_min_x: f32, track_max_x: f32) -> f32 {
    line_x.clamp(track_min_x + pill_w / 2.0, track_max_x - pill_w / 2.0)
}

/// `Day N · 季节` 的季节段（UX-011：从历法**真实月份**派生——此前 90 天
/// 等分是 mock，与历法月长/闰月都错位）。月份取 `Calendar::to_ymd`；
/// 无历法回退按 360 天整除历法的月份（等价于 Simple 缺省口径）。
fn season_of_day(calendar: Option<&wv_core::Calendar>, day: i64) -> &'static str {
    let month = match calendar {
        Some(cal) => cal.to_ymd(day).1,
        None => {
            let n = (day - 1).max(0);
            n % 360 / 30 + 1
        }
    };
    match month {
        3..=5 => "Spring",
        6..=8 => "Summer",
        9..=11 => "Autumn",
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
    let Some(layer) = wv_render::streets::target_road_layer(node) else {
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

    /// 季节划分（UX-011）：按历法真实月份——3-5 春 / 6-8 夏 / 9-11 秋 / 其余冬；
    /// 无历法回退 360 天整除历法的月份口径。
    #[test]
    fn season_of_day_follows_calendar_months() {
        let cal = wv_core::Calendar::default();
        // Simple 缺省：360 天/年、30 天/月 → 月 = ((day-1)%360)/30+1。
        assert_eq!(season_of_day(Some(&cal), 1), "Winter", "day 1 = 1 月");
        assert_eq!(season_of_day(Some(&cal), 31), "Winter", "day 31 = 2 月");
        assert_eq!(season_of_day(Some(&cal), 61), "Spring", "day 61 = 3 月");
        assert_eq!(season_of_day(Some(&cal), 149), "Spring", "day 149 = 5 月");
        assert_eq!(
            season_of_day(Some(&cal), 150),
            "Spring",
            "day 150 = 5 月（n=149）"
        );
        assert_eq!(
            season_of_day(Some(&cal), 151),
            "Summer",
            "day 151 = 6 月（n=150）"
        );
        assert_eq!(season_of_day(Some(&cal), 181), "Summer", "day 181 = 6 月");
        assert_eq!(season_of_day(Some(&cal), 271), "Autumn", "day 271 = 9 月");
        assert_eq!(season_of_day(Some(&cal), 331), "Winter", "day 331 = 11 月");
        // 无历法回退 = 同一 Simple 缺省口径。
        assert_eq!(season_of_day(None, 61), season_of_day(Some(&cal), 61));
    }

    /// 年份行刻度（UX-011）：按区间与历法真实派生。
    #[test]
    fn year_segments_derive_from_calendar_and_thin_out() {
        use crate::i18n::Locale;
        let cal = wv_core::Calendar::default();
        // 跨 2 个历年（Simple：360 天/年，start_year 缺省 1）：360+1 天跨 Y1..Y2。
        let segs = year_segments(1, 362, Some(&cal), Locale::Zh);
        assert_eq!(segs.len(), 2, "跨 2 年应出 2 段，got {segs:?}");
        assert!(
            segs[0].0.contains('1') && segs[1].0.contains('2'),
            "标签含真实年号 {segs:?}"
        );
        // 第二段中点落在区间后半（frac > 0.5）。
        assert!(
            segs[1].1 > 0.5,
            "第二段中点应在区间后半，got {:?}",
            segs[1].1
        );
        // 区间内同一年 → 1 段。
        let segs1 = year_segments(1, 180, Some(&cal), Locale::Zh);
        assert_eq!(segs1.len(), 1, "同年区间应出 1 段");
        // 抽稀：300 年跨度的 301 个段被压到 ≤10。
        let segs_many = year_segments(1, 360 * 300 + 1, Some(&cal), Locale::Zh);
        assert!(
            segs_many.len() <= 10,
            "抽稀后 ≤10 段，got {}",
            segs_many.len()
        );
        // 无历法回退：4 段天数值标签。
        let segs_plain = year_segments(1, 100, None, Locale::Zh);
        assert_eq!(segs_plain.len(), 4, "无历法回退 4 段");
        assert!(
            segs_plain[0].0.starts_with("第"),
            "回退标签应是「第 N 天」口径"
        );
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

    /// 播放器是 Browse 专属（2026-10-27 用户裁定）：VectorFlat 在场，
    /// Map / CellMap / Faction 等一切段外模式不在场。判据本体在
    /// `ViewMode::timeline_shows`（wv-render），这里钉应用侧消费口径。
    #[test]
    fn timeline_shows_only_in_browse_mode() {
        assert!(ViewMode::VectorFlat.timeline_shows());
        assert!(!ViewMode::GoogleMap.timeline_shows(), "Map 段不需要播放器");
        assert!(
            !ViewMode::Topographic.timeline_shows(),
            "CellMap 段不应有播放器"
        );
        assert!(!ViewMode::Faction.timeline_shows(), "段外模式同口径");
    }

    /// 自动暂停只发生在「离开 Browse」的那一跳：段间互切、逗留、进入
    /// Browse 都不动播放态——否则在 Browse 点播放会被误停。
    #[test]
    fn playback_pauses_only_when_leaving_browse() {
        assert!(playback_leaves_browse(
            ViewMode::VectorFlat,
            ViewMode::GoogleMap
        ));
        assert!(playback_leaves_browse(
            ViewMode::VectorFlat,
            ViewMode::Topographic
        ));
        assert!(
            !playback_leaves_browse(ViewMode::GoogleMap, ViewMode::Topographic),
            "非 Browse 之间互切不碰播放态（已因离开而暂停）"
        );
        assert!(!playback_leaves_browse(
            ViewMode::VectorFlat,
            ViewMode::VectorFlat
        ));
        assert!(
            !playback_leaves_browse(ViewMode::GoogleMap, ViewMode::VectorFlat),
            "切回 Browse 是恢复播放的场景，不是暂停"
        );
    }

    /// 停靠几何（2026-10-27 裁定）：展开态占满视口宽、底边贴状态栏上沿。
    #[test]
    fn dock_rect_spans_viewport_and_sits_above_status_bar() {
        let vp = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let (pos, size) = dock_rect(vp, false);
        assert_eq!(
            (pos.x, pos.y),
            (0.0, 800.0 - crate::ui::top_bar::STATUS_BAR_H - EXPANDED_H),
            "展开态左上角应贴视口左缘、高度让出状态栏: got {pos:?}"
        );
        assert_eq!(
            (size.x, size.y),
            (1280.0, EXPANDED_H),
            "展开态应占满视口宽: got {size:?}"
        );
        assert!(
            (pos.y + size.y - (vp.bottom() - crate::ui::top_bar::STATUS_BAR_H)).abs() < 1e-3,
            "底边必须贴状态栏上沿"
        );
    }

    /// 播放条档：高度收成 44、仍占满宽度、仍贴状态栏（收起不改停靠语义）。
    #[test]
    fn dock_rect_collapsed_is_full_width_playbar() {
        let vp = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0));
        let (pos, size) = dock_rect(vp, true);
        assert_eq!(
            (size.x, size.y),
            (1920.0, COMPACT_H),
            "播放条仍应占满宽度、高度为播放条档: got {size:?}"
        );
        assert!(
            (pos.y + size.y - (vp.bottom() - crate::ui::top_bar::STATUS_BAR_H)).abs() < 1e-3,
            "播放条底边同样贴状态栏上沿"
        );
    }
}
