//! 屏幕空间地图装饰三件套（罗盘 / 图廓边框 / 图例）——v1.0 定稿实现。
//!
//! ## 唯一视觉出处
//! `docs/reviews/2026-11-12-map-furniture-spec.md` v1.0（用户 2026-11-13 以
//! 「go」拍板采纳）。§1.2 色板、§3.0–3.3 逐项定值、§4.2 白名单、§六 B1–B7
//! 全部按字面落地；本篇不再有骨架占位。
//!
//! ## 链路
//! - 绘制入口 `app_bootstrap/schedules.rs:647-655`（`EguiPrimaryContextPass`，
//!   `.after(ui::status_bar)`）；egui 形状浮在窗口相机之上，是屏幕空间层。
//! - 锚点坐标系 `crate::ui::top_bar::central_area(ctx)`（视口挖去顶栏 38 /
//!   状态栏 28）；单位屏幕 px，固定不随相机缩放 / 世界平移。
//! - 导出 PNG 走离屏 `Camera2d`，本层只挂窗口 → 出图零装饰（B6）。
//! - **不复活** `PassId::MapFrame`：本模块是 egui 屏幕线，与世界空间 mesh 外框
//!   不是同一对象（ADR-0066 全模式退役仍生效）。
//!
//! ## 三层落点（可脱离引擎证明的部分）
//! - 核心逻辑：[`FurnitureGate`] / [`legend_layout`] / [`group_color`] /
//!   [`COMPASS_PLAN`] / [`FRAME_PLAN`] / [`MapFurnitureState`] 全是纯数据，
//!   零 egui / 零 Bevy System 参数，tests 直接断言。
//! - 表现层：`draw_frame` / `draw_compass` / `draw_legend` 只读核心状态，
//!   不反写；唯一交互写回是折叠钮翻 `MapFurnitureState::collapsed`。
//! - 平台层：全部 Resource 读取收口在 [`map_furniture_system`] 一处。
//!
//! ## 色值纪律（§1.2 / AGENTS §8）
//! 五枚色值全部经 `crate::theme::colors_google`（`wv-default-theme` 本体）
//! 的既有令牌，再经 `crate::theme::palette_to_color32` 转 `egui::Color32`；
//! **不走 `UiShell` 暗壳色、不写裸 hex 到 UI、不复制第二份色表**。
//! 群系色块运行时直取冻结表 `BIOME_RAW_COLORS`（只读）。
//!
//! ## 命中范围取舍（§3.0 / `status.rs:31-34`）
//! 罗盘 / 图廓 / 图例面板底与全部行文字都是**纯 painter 形状，零 widget**。
//! 只有两处注册命中区，都严格收在面板矩形内、**绝不注册全视口 widget**：
//! ① 图例 12×12 折叠钮（`ICON_CHEVRON_*` 是 `egui::ImageSource`，纯 Painter
//! 无法贴图，故用一个 `max_rect == 面板矩形` 的最小 `egui::Ui`）；
//! ② 展开态的 `ScrollArea`（规格 §3.3 要求超出 340px 可滚，同样是面板级
//! `Ui`，不是根 Ui）。`status.rs:31-34` 记录的「根 Ui 提层导致 hit_test 把
//! Background / Middle 层命中整体排除」事故不会被重演——本层恒在 Background
//! 层，且最大命中矩形 168×≤340。
//!
//! 开关：`Ctrl+Shift+D`（翻 `enabled`）或程序化写 [`MapFurnitureState`]。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::app_screen::AppScreen;
use crate::state::{PanelState, PlaybackState, ViewMode, ViewState};
use crate::theme::colors_google::{
    GOOGLE_CONTROL_BG, GOOGLE_CONTROL_BORDER, GOOGLE_ISOLINE_INK, GOOGLE_LABEL_FILL,
    GOOGLE_LINE_ON_DARK,
};
use crate::theme::palette_to_color32;
use crate::theme::ui_theme::proportional;
use crate::ui::{ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT};
use wv_world_cell_map::cell_paint::CellPaintSession;
use wv_world_cell_map::terrain_view::biome_raw::{
    BIOME_LEGEND_GROUPS, BIOME_NAMES_ZH, BIOME_RAW_COLORS,
};
use wv_world_cell_map::terrain_view::field::ViewBranch;

// ── 色令牌桥接（唯一出处 = colors_google.rs；禁止裸 hex 写 UI）────────────
// 一级墨 #3C4043  ← GOOGLE_LABEL_FILL:150 / GOOGLE_CONTROL_TEXT:228
// 二级墨 #9AA0A6  ← GOOGLE_ISOLINE_INK:160 / GOOGLE_LINE_ON_MID:179
// 三级墨 #5F6368  ← GOOGLE_LINE_ON_DARK:180
// 极淡线 #DADCE0  ← GOOGLE_CONTROL_BORDER:226 / GOOGLE_LINE_ON_LIGHT:178
// 面板白 #FFFFFF  ← GOOGLE_CONTROL_BG:224 / GOOGLE_LABEL_STROKE:147

/// 令牌 → `egui::Color32`（α=1.0）。
fn token(c: bevy::color::Color) -> egui::Color32 {
    palette_to_color32(c)
}

/// 令牌 + 规格 α → `egui::Color32`（非预乘，与 `color32_alpha` 同口径）。
fn token_alpha(c: bevy::color::Color, alpha: f32) -> egui::Color32 {
    let [r, g, b, _] = palette_to_color32(c).to_srgba_unmultiplied();
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

// ── §3.1 罗盘定值 ──────────────────────────────────────────────────────────
/// `central_area` 右上角距上 / 距右。
pub const COMPASS_ANCHOR: f32 = 24.0;
/// 底板 64×64px（r=32）。
pub const COMPASS_PLATE_R: f32 = 32.0;
pub const COMPASS_PLATE_ALPHA: f32 = 0.92;
/// 恰好 2 环。
pub const COMPASS_OUTER_R: f32 = 28.0;
pub const COMPASS_INNER_R: f32 = 20.0;
pub const COMPASS_RING_ALPHA: f32 = 0.70;
/// 北 / 南瓣：等腰三角底宽 8px、高 18px。
pub const COMPASS_NEEDLE_HALF_W: f32 = 4.0;
pub const COMPASS_NEEDLE_H: f32 = 18.0;
/// 2 条十字基线各长 36px（半长 18），不出底板。
pub const COMPASS_BASELINE_HALF: f32 = 18.0;
/// 4 个基点刻度，各 4px 长，只在外环内侧。
pub const COMPASS_TICK_LEN: f32 = 4.0;
/// 「N」12px Regular，外环正上方 2px。
pub const COMPASS_N_PX: f32 = 12.0;
pub const COMPASS_N_GAP: f32 = 2.0;
/// 圆心 r=2 实心点。
pub const COMPASS_DOT_R: f32 = 2.0;

/// 罗盘图元预算（可数、可断言；规格 §3.1 / B3）：**恰好 14**。
/// 显式列出每一类，禁 8/16/32 分度盘、星芒、花饰——多一类就过不了测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompassPlan {
    pub plate_filled: u8,
    pub plate_stroke: u8,
    pub rings: u8,
    pub needle_lobes: u8,
    pub cross_baselines: u8,
    pub base_ticks: u8,
    pub north_labels: u8,
    pub center_dots: u8,
}

impl CompassPlan {
    pub const fn total(self) -> u8 {
        self.plate_filled
            + self.plate_stroke
            + self.rings
            + self.needle_lobes
            + self.cross_baselines
            + self.base_ticks
            + self.north_labels
            + self.center_dots
    }
}

pub const COMPASS_PLAN: CompassPlan = CompassPlan {
    plate_filled: 1,
    plate_stroke: 1,
    rings: 2,
    needle_lobes: 2,
    cross_baselines: 2,
    base_ticks: 4,
    north_labels: 1,
    center_dots: 1,
};

// ── §3.2 图廓定值 ──────────────────────────────────────────────────────────
/// 单线 1px、`#9AA0A6`@0.55、距 `central_area` 四边 8px、直角相接不收口。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePlan {
    pub inset: f32,
    pub line_w: f32,
    pub alpha: f32,
    /// §7 指定 4 段 `line_segment`；零双线、零角饰、零填充。
    pub segments: u8,
}

pub const FRAME_PLAN: FramePlan = FramePlan {
    inset: 8.0,
    line_w: 1.0,
    alpha: 0.55,
    segments: 4,
};

// ── §3.3 图例定值 ──────────────────────────────────────────────────────────
/// 右下锚点距下 / 距右。
pub const LEGEND_EDGE: f32 = 32.0;
pub const LEGEND_W: f32 = 168.0;
pub const LEGEND_ALPHA: f32 = 0.94;
pub const LEGEND_CORNER: f32 = 2.0;
pub const LEGEND_PAD: f32 = 8.0;
pub const LEGEND_TITLE_PX: f32 = 11.0;
pub const LEGEND_GROUP_PX: f32 = 11.0;
pub const LEGEND_ITEM_PX: f32 = 10.0;
pub const LEGEND_TITLE_H: f32 = 28.0;
pub const LEGEND_GROUP_ROW_H: f32 = 20.0;
pub const LEGEND_ITEM_ROW_H: f32 = 14.0;
/// 展开高度上限（超出交给面板级 `ScrollArea`）。
pub const LEGEND_MAX_H: f32 = 340.0;
pub const LEGEND_SWATCH: f32 = 12.0;
pub const LEGEND_ITEM_INDENT: f32 = 12.0;
pub const LEGEND_BAR_W: f32 = 4.0;
pub const LEGEND_ICON_PX: f32 = 12.0;

/// 图例排布（纯数据；折叠态与展开态都可脱离 egui 断言）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LegendLayout {
    pub width: f32,
    pub height: f32,
    pub group_rows: u8,
    pub item_rows: u8,
    pub body_height: f32,
}

/// 展开态族内项总数：逐组数非 255 编号，直接读冻结分组表，不另立色表。
pub fn legend_item_rows() -> u8 {
    BIOME_LEGEND_GROUPS
        .iter()
        .flat_map(|(_, members)| members.iter().copied())
        .filter(|&b| b != 255)
        .count() as u8
}

/// 折叠 = 标题 28 + 8×20 = 188px；展开按 340px 封顶。
pub fn legend_layout(expanded: bool) -> LegendLayout {
    let groups = BIOME_LEGEND_GROUPS.len() as u8;
    let items = if expanded { legend_item_rows() } else { 0 };
    let needed = LEGEND_TITLE_H
        + groups as f32 * LEGEND_GROUP_ROW_H
        + items as f32 * LEGEND_ITEM_ROW_H;
    let height = needed.min(LEGEND_MAX_H);
    LegendLayout {
        width: LEGEND_W,
        height,
        group_rows: groups,
        item_rows: items,
        body_height: height - LEGEND_TITLE_H,
    }
}

/// 族代表色：运行时直取 `BIOME_RAW_COLORS[首个非 255 编号]`（单一出处）。
/// 校验锚点：水域族 `#9DC3D8`、道路·无族 `#9E9E9E`。
pub fn group_color(members: [u8; 4]) -> egui::Color32 {
    let first = members
        .iter()
        .copied()
        .find(|&b| b != 255)
        .unwrap_or(0) as usize;
    let rgb = BIOME_RAW_COLORS[first.min(BIOME_RAW_COLORS.len() - 1)];
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

// ── 状态 ────────────────────────────────────────────────────────────────────

/// 三件装饰的绘制单元（纯数据；表现层按此表逐个画，不反向写核心状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FurniturePart {
    Compass,
    Frame,
    Legend,
}

/// 装饰开关（唯一可变状态）。**缺省开**——
/// 用户 2026-11-13 拍板采纳 `docs/reviews/2026-11-12-map-furniture-spec.md`
/// v1.0；`Ctrl+Shift+D`（系统内读 egui input）或程序化写本资源可临时关掉，
/// 关掉后画面回到零装饰基底。
#[derive(Resource, Debug, Clone, Copy)]
pub struct MapFurnitureState {
    pub enabled: bool,
    /// `show_legend`：**默认开**。关掉后只剩罗盘 + 图廓。
    pub show_legend: bool,
    /// `collapsed: bool`，**默认 true**（美术验收清单 L08 v1.2 共同要求）：
    /// 首屏只见 8 族名；点折叠钮才列族内项。
    pub collapsed: bool,
}

impl Default for MapFurnitureState {
    fn default() -> Self {
        // 用户 2026-11-13 拍板采纳 v1.0 → 缺省开、默认开图例、默认折叠。
        Self {
            enabled: true,
            show_legend: true,
            collapsed: true,
        }
    }
}

impl MapFurnitureState {
    /// `Ctrl+Shift+D` 的纯数据效果（系统只调它，便于脱离 egui 单测）。
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
    }

    /// 折叠钮的纯数据效果。
    pub fn toggle_legend(&mut self) {
        self.collapsed = !self.collapsed;
    }

    /// 面板级二次裁切：`draw_plan` 放行的是「装饰层」，本开关只裁图例。
    pub fn legend_visible(self) -> bool {
        self.enabled && self.show_legend
    }

    pub fn legend_layout(self) -> LegendLayout {
        legend_layout(!self.collapsed)
    }
}

/// 可见性判据快照（纯数据，零 egui / 零 Bevy System 参数，可脱离引擎单测）。
/// §4.2 白名单五项全部显式成布尔，**不**用 `ViewBranch` / `p.shape.y` 推断 mode。
#[derive(Debug, Clone, Copy)]
pub struct FurnitureGate {
    pub enabled: bool,
    pub editor_visible: bool,
    pub immersive: bool,
    pub mode: ViewMode,
    pub branch: ViewBranch,
    pub field_render: bool,
    pub terrain_visible: bool,
    /// `AppScreen::shows_loading_overlay()`
    pub loading_overlay: bool,
    /// `PlaybackState::playing`
    pub playback_playing: bool,
    /// `CellPaintSession::is_stroking()`
    pub stroking: bool,
}

impl FurnitureGate {
    /// 目标画布本身是否成立（与用户开关无关）。系统据此整层早退。
    ///
    /// `VectorFlat` ∧ `Themed` ∧ `terrain_visible` ∧ `field_render`
    /// ∧ `!loading_overlay` ∧ `!playback_playing` ∧ `!stroking`
    /// ∧ `!immersive` ∧ `editor_visible`。
    pub fn target_canvas(self) -> bool {
        self.editor_visible
            && !self.immersive
            && self.mode == ViewMode::VectorFlat
            && self.branch == ViewBranch::Themed
            && self.terrain_visible
            && self.field_render
            && !self.loading_overlay
            && !self.playback_playing
            && !self.stroking
    }

    /// 三件装饰的绘制表。`None` = 本帧一件都不画。
    ///
    /// 固定长度数组而非 `Vec`——每帧零堆分配。
    pub fn draw_plan(self) -> Option<[FurniturePart; 3]> {
        if !self.enabled || !self.target_canvas() {
            return None;
        }
        Some([
            FurniturePart::Compass,
            FurniturePart::Frame,
            FurniturePart::Legend,
        ])
    }
}

/// 每帧系统（注册点在 `app_bootstrap/schedules.rs:647-655`）。
#[allow(clippy::too_many_arguments)]
pub fn map_furniture_system(
    mut contexts: EguiContexts,
    view: Res<ViewState>,
    screen: Res<AppScreen>,
    panels: Res<PanelState>,
    playback: Res<PlaybackState>,
    paint: Res<CellPaintSession>,
    mut state: ResMut<MapFurnitureState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let ctx: &egui::Context = ctx;

    // Ctrl+Shift+D：开关（不与 hotkeys 的 Ctrl+Shift+I/N/S/Z 冲突）。
    if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::D)) {
        state.toggle();
    }

    // 门控字段**每帧从 Res 重算、绝不缓存**：切模式 / 结束播放 / 松笔 /
    // 遮罩消失后立刻反映当帧状态，不留旧值残留（§4.2 实现约束）。
    // branch 由渲染层唯一选择点求值，本层不另写一套布尔式（防门控漂移）。
    let branch = wv_render::cellmap_bridge::field_branch_of(
        view.mode,
        view.terrain_visible,
        view.cell_raw,
    );
    let gate = FurnitureGate {
        enabled: state.enabled,
        editor_visible: screen.editor_visible(),
        immersive: panels.immersive,
        mode: view.mode,
        branch,
        field_render: view.field_render,
        terrain_visible: view.terrain_visible,
        loading_overlay: screen.shows_loading_overlay(),
        playback_playing: playback.playing,
        stroking: paint.is_stroking(),
    };
    // 非目标画布 / 开关关：整层早退，零 shape、零 widget。
    let Some(plan) = gate.draw_plan() else {
        return;
    };

    let central = crate::ui::top_bar::central_area(ctx);
    if central.width() <= 0.0 || central.height() <= 0.0 {
        return;
    }

    // 罗盘 / 图廓：纯 painter 形状，零 widget（不注册任何命中区）。
    let canvas = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("wv_map_furniture_canvas"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(central),
    );
    for part in plan {
        match part {
            FurniturePart::Frame => draw_frame(canvas.painter(), central),
            FurniturePart::Compass => draw_compass(canvas.painter(), central),
            // 图例要走面板级最小 `Ui`（折叠钮 + 展开态 ScrollArea），见模块文档。
            FurniturePart::Legend => {}
        }
    }
    if state.legend_visible() {
        draw_legend(ctx, central, &mut state);
    }
}

/// §3.2 图廓：**单线 1px**、`#9AA0A6`@0.55、内收 8px、4 段 `line_segment`
/// 直角直接相接不收口；零双线 / 零角饰 / 零填充 / 零刻度尺。
fn draw_frame(p: &egui::Painter, central: egui::Rect) {
    let r = central.shrink(FRAME_PLAN.inset);
    let stroke = egui::Stroke::new(
        FRAME_PLAN.line_w,
        token_alpha(GOOGLE_ISOLINE_INK, FRAME_PLAN.alpha),
    );
    p.line_segment(
        [egui::pos2(r.min.x, r.min.y), egui::pos2(r.max.x, r.min.y)],
        stroke,
    );
    p.line_segment(
        [egui::pos2(r.max.x, r.min.y), egui::pos2(r.max.x, r.max.y)],
        stroke,
    );
    p.line_segment(
        [egui::pos2(r.max.x, r.max.y), egui::pos2(r.min.x, r.max.y)],
        stroke,
    );
    p.line_segment(
        [egui::pos2(r.min.x, r.max.y), egui::pos2(r.min.x, r.min.y)],
        stroke,
    );
}

/// §3.1 罗盘：右上角 24px、64×64 白底板 @0.92、2 环、1 根双瓣针、
/// 2 条十字基线、4 刻度、「N」、圆心点 —— 恰好 [`COMPASS_PLAN`] 的 14 个图元。
/// N 恒定朝屏幕正上方，**不绑相机旋转**（世界→屏幕反向换算缺失，不兜底）。
fn draw_compass(p: &egui::Painter, central: egui::Rect) {
    let c = egui::pos2(
        central.max.x - COMPASS_ANCHOR - COMPASS_PLATE_R,
        central.min.y + COMPASS_ANCHOR + COMPASS_PLATE_R,
    );
    let ink1 = token(GOOGLE_LABEL_FILL);
    let ink2 = token(GOOGLE_ISOLINE_INK);
    let line = token(GOOGLE_CONTROL_BORDER);
    let white = token(GOOGLE_CONTROL_BG);

    // 1 底板 + 1 描边：`#FFFFFF`@0.92 + 1px `#DADCE0`。
    p.circle_filled(
        c,
        COMPASS_PLATE_R,
        token_alpha(GOOGLE_CONTROL_BG, COMPASS_PLATE_ALPHA),
    );
    p.circle_stroke(c, COMPASS_PLATE_R, egui::Stroke::new(1.0, line));

    // 2 环（封顶）：外 r=28 1px `#9AA0A6`@0.70 / 内 r=20 1px `#DADCE0`。
    p.circle_stroke(
        c,
        COMPASS_OUTER_R,
        egui::Stroke::new(1.0, token_alpha(GOOGLE_ISOLINE_INK, COMPASS_RING_ALPHA)),
    );
    p.circle_stroke(c, COMPASS_INNER_R, egui::Stroke::new(1.0, line));

    // 2 条十字基线：各长 36px、1px `#DADCE0`、过圆心、不出底板。
    let h = COMPASS_BASELINE_HALF;
    p.line_segment(
        [egui::pos2(c.x - h, c.y), egui::pos2(c.x + h, c.y)],
        egui::Stroke::new(1.0, line),
    );
    p.line_segment(
        [egui::pos2(c.x, c.y - h), egui::pos2(c.x, c.y + h)],
        egui::Stroke::new(1.0, line),
    );

    // 4 个基点刻度：4px 长、1px `#9AA0A6`，只在外环内侧（r=28 → r=24）。
    let tick = egui::Stroke::new(1.0, ink2);
    let outer = COMPASS_OUTER_R;
    let inner = COMPASS_OUTER_R - COMPASS_TICK_LEN;
    for (a, b) in [
        (egui::pos2(c.x, c.y - outer), egui::pos2(c.x, c.y - inner)),
        (egui::pos2(c.x + outer, c.y), egui::pos2(c.x + inner, c.y)),
        (egui::pos2(c.x, c.y + outer), egui::pos2(c.x, c.y + inner)),
        (egui::pos2(c.x - outer, c.y), egui::pos2(c.x - inner, c.y)),
    ] {
        p.line_segment([a, b], tick);
    }

    // 1 根双瓣针：北瓣等腰三角（底宽 8 / 高 18）`#3C4043` 实心；
    // 南瓣同形倒置 `#FFFFFF` 空心 + 1px `#9AA0A6` 描边。
    let w = COMPASS_NEEDLE_HALF_W;
    let nh = COMPASS_NEEDLE_H;
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - w, c.y),
            egui::pos2(c.x + w, c.y),
            egui::pos2(c.x, c.y - nh),
        ],
        ink1,
        egui::Stroke::NONE,
    ));
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - w, c.y),
            egui::pos2(c.x + w, c.y),
            egui::pos2(c.x, c.y + nh),
        ],
        white,
        egui::Stroke::new(1.0, ink2),
    ));

    // 1 个「N」：12px Regular `#3C4043`，锚在外环正上方 2px（cy-30），
    // 禁三角旗 / 星芒 / 百合花饰 / 装饰字母。
    p.text(
        egui::pos2(c.x, c.y - outer - COMPASS_N_GAP),
        egui::Align2::CENTER_CENTER,
        "N",
        proportional(COMPASS_N_PX),
        ink1,
    );

    // 1 个圆心 r=2 `#3C4043` 实心点，收住两瓣针接缝。
    p.circle_filled(c, COMPASS_DOT_R, ink1);
}

/// §3.3 图例：右下 32px、168px 白卡片 @0.94 + 1px `#DADCE0` + 圆角 2px。
/// 默认折叠 = 8 族行；展开才列族内项，高度封顶 340px 后交 `ScrollArea`。
///
/// 见模块文档「命中范围取舍」：这个子 `Ui` 的 `max_rect` 恒等于面板矩形，
/// 折叠钮只占 12×12 命中区；**不是全视口 widget**。
fn draw_legend(ctx: &egui::Context, central: egui::Rect, state: &mut MapFurnitureState) {
    let expanded = !state.collapsed;
    let layout = legend_layout(expanded);
    let panel = egui::Rect::from_min_size(
        egui::pos2(
            central.max.x - LEGEND_EDGE - layout.width,
            central.max.y - LEGEND_EDGE - layout.height,
        ),
        egui::vec2(layout.width, layout.height),
    );
    let mut ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("wv_map_furniture_legend"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(panel),
    );

    // 面板底 / 描边 / 标题：纯 painter 形状，不注册 widget。
    {
        let p = ui.painter();
        p.rect_filled(
            panel,
            LEGEND_CORNER,
            token_alpha(GOOGLE_CONTROL_BG, LEGEND_ALPHA),
        );
        p.rect_stroke(
            panel,
            LEGEND_CORNER,
            egui::Stroke::new(1.0, token(GOOGLE_CONTROL_BORDER)),
            egui::StrokeKind::Inside,
        );
        p.text(
            egui::pos2(
                panel.min.x + LEGEND_PAD,
                panel.min.y + LEGEND_TITLE_H * 0.5,
            ),
            egui::Align2::LEFT_CENTER,
            "群系图例",
            proportional(LEGEND_TITLE_PX),
            token(GOOGLE_LABEL_FILL),
        );
    }

    // 折叠钮：12×12 命中区，用 `ui/mod.rs:119-120` 现成 `ICON_CHEVRON_*`。
    // L08 v1.2 走**路径 ①（现成图标）**：`ICON_CHEVRON_*` 是
    // `egui::ImageSource<'static>`，纯 `Painter` 贴不了，但经这个面板级
    // `egui::Ui` 的 `Image::paint_at` 在本工程可编译可接线（无 API / 纹理
    // 阻塞），故**未**降级到路径 ② 的 2 段折线，也没有两套指示符并存。
    // `ui.interact` 不移动布局游标，绝不会撑出面板矩形；命中区保持
    // 12×12，不因图标形态缩小。
    let icon = if expanded {
        ICON_CHEVRON_DOWN
    } else {
        ICON_CHEVRON_RIGHT
    };
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(
            panel.max.x - LEGEND_PAD - LEGEND_ICON_PX,
            panel.min.y + (LEGEND_TITLE_H - LEGEND_ICON_PX) * 0.5,
        ),
        egui::vec2(LEGEND_ICON_PX, LEGEND_ICON_PX),
    );
    let resp = ui.interact(icon_rect, ui.id().with("legend_toggle"), egui::Sense::click());
    if resp.clicked() {
        state.toggle_legend();
    }
    egui::Image::new(icon)
        .tint(token(GOOGLE_LABEL_FILL))
        .fit_to_exact_size(egui::Vec2::splat(LEGEND_ICON_PX))
        .paint_at(&ui, icon_rect);

    // 游标下移到标题之下；展开态超高时 `ScrollArea` 负责滚动（不占满画布）。
    ui.add_space(LEGEND_TITLE_H);
    egui::ScrollArea::vertical()
        .max_height(layout.body_height)
        .auto_shrink([false, false])
        .show(&mut ui, |ui| {
            // 展开项 `BIOME_NAMES_ZH` 只在 `collapsed == false` 分支绘制。
            legend_rows(ui, expanded);
        });
}

/// 图例行：折叠态只 8 族；展开态在族行后列族内项（10px + 缩进 12px）。
/// 每行用 `allocate_exact_size` 取矩形，形状仍由 `ui.painter()` 画——
/// 这样 `ScrollArea` 的滚动偏移天然作用到行矩形上，无需手搓换算。
fn legend_rows(ui: &mut egui::Ui, expanded: bool) {
    let row_w = LEGEND_W - 2.0 * LEGEND_PAD;
    for (group, members) in &BIOME_LEGEND_GROUPS {
        let (row, _) = ui.allocate_exact_size(
            egui::vec2(row_w, LEGEND_GROUP_ROW_H),
            egui::Sense::hover(),
        );
        draw_legend_group_row(ui.painter(), row, group, *members);
        if !expanded {
            continue;
        }
        for &b in members.iter() {
            if b == 255 {
                continue;
            }
            let (item, _) = ui.allocate_exact_size(
                egui::vec2(row_w, LEGEND_ITEM_ROW_H),
                egui::Sense::hover(),
            );
            let name = BIOME_NAMES_ZH[(b as usize).min(BIOME_NAMES_ZH.len() - 1)];
            ui.painter().text(
                egui::pos2(
                    item.min.x + LEGEND_SWATCH + LEGEND_ITEM_INDENT,
                    item.center().y,
                ),
                egui::Align2::LEFT_CENTER,
                name,
                proportional(LEGEND_ITEM_PX),
                token(GOOGLE_LINE_ON_DARK),
            );
        }
    }
}

/// 单个族行：4px 族代表色竖条 + 12×12 直角色块（1px `#DADCE0` 内描边）
/// + 11px `#3C4043` 族名。色块运行时直取冻结表，不复制第二份色表。
fn draw_legend_group_row(p: &egui::Painter, row: egui::Rect, group: &str, members: [u8; 4]) {
    let color = group_color(members);
    let bar = egui::Rect::from_min_size(
        egui::pos2(row.min.x, row.min.y + 2.0),
        egui::vec2(LEGEND_BAR_W, LEGEND_GROUP_ROW_H - 4.0),
    );
    p.rect_filled(bar, 0.0, color);
    let sw = egui::Rect::from_min_size(
        egui::pos2(
            row.min.x + LEGEND_BAR_W + 4.0,
            row.center().y - LEGEND_SWATCH * 0.5,
        ),
        egui::vec2(LEGEND_SWATCH, LEGEND_SWATCH),
    );
    p.rect_filled(sw, 0.0, color);
    p.rect_stroke(
        sw,
        0.0,
        egui::Stroke::new(1.0, token(GOOGLE_CONTROL_BORDER)),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(sw.max.x + 6.0, row.center().y),
        egui::Align2::LEFT_CENTER,
        group,
        proportional(LEGEND_GROUP_PX),
        token(GOOGLE_LABEL_FILL),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正例的唯一基准：Browse / VectorFlat / Themed / 有工程 / 非沉浸 /
    /// 地形可见 / 场渲染 / 无遮罩 / 未播放 / 未拖绘 / 开。
    fn themed_browse() -> FurnitureGate {
        FurnitureGate {
            enabled: true,
            editor_visible: true,
            immersive: false,
            mode: ViewMode::VectorFlat,
            branch: ViewBranch::Themed,
            field_render: true,
            terrain_visible: true,
            loading_overlay: false,
            playback_playing: false,
            stroking: false,
        }
    }

    #[test]
    fn themed_browse_canvas_draws_all_three_furniture_pieces() {
        let plan = themed_browse()
            .draw_plan()
            .expect("Browse / Themed 画布应产出装饰绘制表");
        assert_eq!(plan.len(), 3, "三件套应恰好三件，实得 {}", plan.len());
        assert!(plan.contains(&FurniturePart::Compass), "罗盘缺席");
        assert!(plan.contains(&FurniturePart::Frame), "图廓缺席");
        assert!(plan.contains(&FurniturePart::Legend), "图例缺席");
    }

    /// 用户 2026-11-13 拍板采纳 v1.0：缺省开；`Ctrl+Shift+D` 或写资源可关。
    /// 负向断言路径 `gate.draw_plan() == None` 在此保留。
    #[test]
    fn adopted_spec_defaults_to_on_and_can_be_switched_off() {
        let d = MapFurnitureState::default();
        assert!(d.enabled, "拍板后缺省必须开");
        assert!(d.show_legend, "show_legend 默认开");
        assert!(d.collapsed, "默认折叠（collapsed 缺省 true）");

        let gate = FurnitureGate {
            enabled: d.enabled,
            ..themed_browse()
        };
        assert!(gate.target_canvas(), "目标画布本身成立");
        let plan = gate.draw_plan().expect("缺省帧应放行三件套");
        assert_eq!(plan.len(), 3);

        // Ctrl+Shift+D 语义 = 翻开关（纯数据，可脱离 egui 断言）。
        let mut off = d;
        off.toggle();
        assert!(!off.enabled, "Ctrl+Shift+D 应把缺省开翻成关");
        let gate_off = FurnitureGate {
            enabled: off.enabled,
            ..themed_browse()
        };
        assert_eq!(gate_off.draw_plan(), None, "翻关后必须一件都不画");

        // 程序化写资源关掉，同样零绘制。
        let written = MapFurnitureState {
            enabled: false,
            ..MapFurnitureState::default()
        };
        let gate_written = FurnitureGate {
            enabled: written.enabled,
            ..themed_browse()
        };
        assert_eq!(gate_written.draw_plan(), None, "写资源关掉后必须一件都不画");
        assert!(
            gate_written.target_canvas(),
            "关开关不改变目标画布本身，零绘制是开关的功劳"
        );
    }

    #[test]
    fn google_map_professional_canvas_draws_nothing() {
        let mut gate = themed_browse();
        gate.mode = ViewMode::GoogleMap;
        assert_eq!(gate.draw_plan(), None, "普通地图保持干净专业制图，不挂装饰");
        assert!(!gate.target_canvas(), "非目标画布系统应整层早退");
    }

    #[test]
    fn cellmap_grid_data_view_draws_nothing() {
        let mut gate = themed_browse();
        gate.mode = ViewMode::Topographic;
        gate.branch = ViewBranch::CellFlat;
        assert_eq!(gate.draw_plan(), None, "CellMap 方格数据视图不得绘制");
        assert!(!gate.target_canvas(), "方格平涂分支不是目标画布");
    }

    #[test]
    fn biome_raw_comparison_view_draws_nothing() {
        let mut gate = themed_browse();
        gate.branch = ViewBranch::BiomeRaw;
        assert_eq!(gate.draw_plan(), None, "WV_CELLRAW 对照档不得绘制");
    }

    #[test]
    fn hidden_terrain_political_flat_base_draws_nothing() {
        let mut gate = themed_browse();
        gate.branch = ViewBranch::Flat;
        assert_eq!(gate.draw_plan(), None, "terrain_visible=false 的政治素底页不得绘制");
        assert!(!gate.target_canvas(), "Flat 分支不是目标画布");
    }

    #[test]
    fn no_project_immersive_and_toggle_off_all_draw_nothing() {
        let mut no_project = themed_browse();
        no_project.editor_visible = false;
        assert_eq!(no_project.draw_plan(), None, "无工程（欢迎页）不得绘制");
        assert!(!no_project.target_canvas());

        let mut immersive = themed_browse();
        immersive.immersive = true;
        assert_eq!(immersive.draw_plan(), None, "沉浸 / 全屏态不得绘制");

        let mut off = themed_browse();
        off.enabled = false;
        assert_eq!(off.draw_plan(), None, "开关关闭时不得绘制");
        assert!(off.target_canvas(), "关开关不改变目标画布本身");
    }

    #[test]
    fn every_other_view_mode_draws_nothing() {
        for mode in [
            ViewMode::RealTerrain,
            ViewMode::Faction,
            ViewMode::Event,
            ViewMode::Coastline,
        ] {
            let mut gate = themed_browse();
            gate.mode = mode;
            assert_eq!(gate.draw_plan(), None, "{mode:?} 不得绘制装饰");
        }
    }

    /// 门控读到的 branch 必须来自渲染层唯一选择点（独立来源，不许本模块自造）。
    #[test]
    fn gate_branches_match_the_render_layers_single_choice_point() {
        assert_eq!(
            wv_render::cellmap_bridge::field_branch_of(ViewMode::VectorFlat, true, false),
            ViewBranch::Themed,
            "Browse + 地形可见 = Themed"
        );
        assert_eq!(
            wv_render::cellmap_bridge::field_branch_of(ViewMode::VectorFlat, false, false),
            ViewBranch::Flat,
            "地形隐藏 = 政治素底页"
        );
        assert_eq!(
            wv_render::cellmap_bridge::field_branch_of(ViewMode::Topographic, true, false),
            ViewBranch::CellFlat,
            "CellMap 默认方格平涂"
        );
        assert_eq!(
            wv_render::cellmap_bridge::field_branch_of(ViewMode::Topographic, true, true),
            ViewBranch::BiomeRaw,
            "WV_CELLRAW 对照档"
        );
        // GoogleMap 同样是 Themed 分支——故不能只靠 branch 判据，
        // 必须有 mode 白名单把它排除（测试钉住这一点，防回归）。
        assert_eq!(
            wv_render::cellmap_bridge::field_branch_of(ViewMode::GoogleMap, true, false),
            ViewBranch::Themed,
            "普通地图也是 Themed，靠 mode 白名单排除"
        );
    }

    #[test]
    fn legend_group_data_is_read_without_mutating_the_frozen_table() {
        assert_eq!(BIOME_LEGEND_GROUPS.len(), 8, "既有图例分 8 组");
        assert_eq!(BIOME_NAMES_ZH.len(), 21, "冻结群系名表 21 项");
        assert_eq!(BIOME_RAW_COLORS.len(), 21, "冻结色表 21 项");
        for (_, members) in &BIOME_LEGEND_GROUPS {
            for &b in members {
                if b != 255 {
                    assert!(
                        (b as usize) < BIOME_NAMES_ZH.len(),
                        "图例编号 {b} 越出名称表"
                    );
                }
            }
        }
    }

    // ── §4.2 白名单五条新增负向断言（逐条独立，各自 draw_plan() == None）──

    #[test]
    fn loading_overlay_hides_every_furniture_piece() {
        let mut gate = themed_browse();
        gate.loading_overlay = true;
        assert!(!gate.target_canvas(), "加载遮罩期间不是目标画布");
        assert_eq!(gate.draw_plan(), None, "加载遮罩盖住装饰，不得绘制");
    }

    #[test]
    fn playback_playing_hides_every_furniture_piece() {
        let mut gate = themed_browse();
        gate.playback_playing = true;
        assert!(!gate.target_canvas(), "时间轴播放中不是目标画布");
        assert_eq!(gate.draw_plan(), None, "播放中装饰不得盖住进度反馈");
    }

    #[test]
    fn stroking_hides_every_furniture_piece() {
        let mut gate = themed_browse();
        gate.stroking = true;
        assert!(!gate.target_canvas(), "笔刷 stroke 期间不是目标画布");
        assert_eq!(gate.draw_plan(), None, "拖绘期间装饰不得干扰笔触读数");
    }

    #[test]
    fn field_render_off_hides_every_furniture_piece() {
        let mut gate = themed_browse();
        gate.field_render = false;
        assert!(!gate.target_canvas(), "CPU 烘焙留守档不是目标画布");
        assert_eq!(gate.draw_plan(), None, "field_render=false 不得绘制");
    }

    #[test]
    fn terrain_visible_off_hides_every_furniture_piece() {
        let mut gate = themed_browse();
        gate.terrain_visible = false;
        assert!(!gate.target_canvas(), "地形层隐藏不是目标画布");
        assert_eq!(gate.draw_plan(), None, "terrain_visible=false 不得绘制");
    }

    /// 白名单是「全部条件同时成立」，缺一即整层早退——切模式不留旧值残留的
    /// 纯数据版本：把每个条件单独翻成假，逐条断言。
    #[test]
    fn target_canvas_requires_every_whitelist_condition_simultaneously() {
        assert!(themed_browse().target_canvas(), "基准必须放行");
        let mut g = themed_browse();
        g.editor_visible = false;
        assert!(!g.target_canvas(), "editor_visible");
        let mut g = themed_browse();
        g.immersive = true;
        assert!(!g.target_canvas(), "immersive");
        let mut g = themed_browse();
        g.mode = ViewMode::GoogleMap;
        assert!(!g.target_canvas(), "mode");
        let mut g = themed_browse();
        g.branch = ViewBranch::CellFlat;
        assert!(!g.target_canvas(), "branch");
        let mut g = themed_browse();
        g.terrain_visible = false;
        assert!(!g.target_canvas(), "terrain_visible");
        let mut g = themed_browse();
        g.field_render = false;
        assert!(!g.target_canvas(), "field_render");
        let mut g = themed_browse();
        g.loading_overlay = true;
        assert!(!g.target_canvas(), "loading_overlay");
        let mut g = themed_browse();
        g.playback_playing = true;
        assert!(!g.target_canvas(), "playback_playing");
        let mut g = themed_browse();
        g.stroking = true;
        assert!(!g.target_canvas(), "stroking");
    }

    // ── §六 B1：色板饱和度上限 + 零暖色 ──────────────────────────────────

    /// 期望值**全部手抄自规格 §5.1 / B1 数值表**（独立来源），不引用本模块
    /// 任一常量当两边：先钉死实现令牌的 RGB，再由 RGB 独立算 S 与 |R−B|。
    #[test]
    fn furniture_palette_is_neutral_cold_and_matches_the_spec_table() {
        // (名称, RGB, S, |R−B|) —— 规格 §5.1 表，8-bit sRGB 直算。
        let spec: [(&str, [u8; 3], f32, i32); 5] = [
            ("一级墨 #3C4043", [60, 64, 67], 0.104, 7),
            ("三级墨 #5F6368", [95, 99, 104], 0.087, 9),
            ("二级墨 #9AA0A6", [154, 160, 166], 0.072, 12),
            ("极淡线 #DADCE0", [218, 220, 224], 0.027, 6),
            ("面板白 #FFFFFF", [255, 255, 255], 0.000, 0),
        ];
        let tokens = [
            GOOGLE_LABEL_FILL,
            GOOGLE_LINE_ON_DARK,
            GOOGLE_ISOLINE_INK,
            GOOGLE_CONTROL_BORDER,
            GOOGLE_CONTROL_BG,
        ];
        for ((name, want_rgb, want_s, want_rb), t) in spec.iter().zip(tokens.iter()) {
            let [r, g, b, _] = palette_to_color32(*t).to_srgba_unmultiplied();
            assert_eq!([r, g, b], *want_rgb, "{name}: 实现令牌偏离规格表");
            let max = r.max(g).max(b) as f32;
            let min = r.min(g).min(b) as f32;
            let s = (max - min) / max;
            let rb = (r as i32 - b as i32).abs();
            assert!(s <= 0.12, "{name}: S={s} > 0.12");
            assert!(rb <= 12, "{name}: |R−B|={rb} > 12");
            assert!((s - want_s).abs() < 0.002, "{name}: S={s}，规格 {want_s}");
            assert_eq!(rb, *want_rb, "{name}: |R−B| 与规格表不符");
            assert!(
                !(r > g && g > b),
                "{name}: 出现 R>G>B 暖序色，古舆图墨混入"
            );
        }
    }

    /// C03（美术验收清单 v1.2）：α 合成必须走 `from_rgba_unmultiplied`，
    /// **禁用 `.gamma_multiply`**（`status.rs:73` 与 `ui_tokens.rs:239-250`
    /// 两处工程注释为证）。期望值来自独立手算 (α×255).round()，不是
    /// 引用实现常量：0.92→235 / 0.94→240 / 0.70→179 / 0.55→140。
    /// RGB 通道必须保持令牌原值（预乘近似会把亮片洗成全强度 RGB + 低 α）。
    #[test]
    fn alpha_composition_is_unmultiplied_and_never_gamma_multiplied() {
        for (alpha, want_a) in [(0.92_f32, 235_u8), (0.94, 240), (0.70, 179), (0.55, 140)] {
            let got = token_alpha(GOOGLE_CONTROL_BG, alpha);
            let [r, g, b, a] = got.to_srgba_unmultiplied();
            assert_eq!(a, want_a, "α={alpha} 应得 {want_a}，实得 {a}");
            assert_eq!([r, g, b], [255, 255, 255], "RGB 不得被 α 预乘改写");
        }
        // 二级墨 @0.70：α 必须精确 179。RGB 因 `Color32` 内部预乘 / 反预乘
        // 的整数舍入有 ±1 往返误差（`ui_tokens.rs:239-250` 已记录「低 alpha
        // 往返会丢精度」），故只断言 ≤1；这不是 `.gamma_multiply` 那种
        // 「全强度 RGB + 低 α」的过亮白片，RGB 仍牢牢贴着 #9AA0A6。
        let [r, g, b, a] =
            token_alpha(GOOGLE_ISOLINE_INK, COMPASS_RING_ALPHA).to_srgba_unmultiplied();
        assert_eq!(a, 179, "α 必须精确 179");
        for (got, want) in [(r, 154), (g, 160), (b, 166)] {
            assert!(
                (got as i32 - want).abs() <= 1,
                "RGB 通道 {got} 偏离令牌值 {want} 超过 ±1（预乘舍入）"
            );
        }
    }

    // ── §3.1 / §3.2 / §3.3 定值可断言 ────────────────────────────────────

    #[test]
    fn compass_primitive_budget_is_exactly_fourteen_and_forbids_division_dials() {
        assert_eq!(COMPASS_PLAN.total(), 14, "罗盘图元总数必须恰好 14");
        assert!(COMPASS_PLAN.total() <= 14, "图元上限 ≤14");
        assert_eq!(COMPASS_PLAN.rings, 2, "恰好 2 环，禁 3+ 环玫瑰盘");
        assert_eq!(COMPASS_PLAN.needle_lobes, 2, "恰好 1 根双瓣针");
        assert_eq!(COMPASS_PLAN.cross_baselines, 2, "2 条十字基线");
        assert_eq!(COMPASS_PLAN.base_ticks, 4, "4 个基点刻度，禁 8/16/32 分度盘");
        assert_eq!(COMPASS_PLAN.north_labels, 1, "仅 1 个「N」，禁星芒 / 花饰");
        assert_eq!(COMPASS_PLAN.center_dots, 1, "1 个圆心点");
        assert_eq!(COMPASS_PLATE_R, 32.0, "底板 64×64px");
        assert_eq!(COMPASS_ANCHOR, 24.0, "右上锚点距上 / 右 24px");
        assert_eq!(COMPASS_NEEDLE_HALF_W * 2.0, 8.0, "针底宽 8px");
        assert_eq!(COMPASS_NEEDLE_H, 18.0, "针高 18px");
        assert_eq!(COMPASS_BASELINE_HALF * 2.0, 36.0, "十字基线各长 36px");
        assert_eq!(COMPASS_TICK_LEN, 4.0, "刻度 4px");
        assert_eq!(COMPASS_N_PX, 12.0, "「N」12px");
        assert_eq!(COMPASS_DOT_R, 2.0, "圆心 r=2");
    }

    #[test]
    fn frame_is_a_single_line_of_four_segments_at_eight_px_inset() {
        assert_eq!(FRAME_PLAN.segments, 4, "§7 指定 4 段 line_segment");
        assert_eq!(FRAME_PLAN.inset, 8.0, "距 central_area 四边 8px");
        assert_eq!(FRAME_PLAN.line_w, 1.0, "单线 1px，零双线");
        assert_eq!(FRAME_PLAN.alpha, 0.55, "α ≤ 0.55 硬上限");
    }

    #[test]
    fn legend_state_defaults_to_on_collapsed_and_expands_with_a_340px_cap() {
        let s = MapFurnitureState::default();
        assert!(s.show_legend, "show_legend 默认开");
        assert!(s.collapsed, "默认折叠（collapsed 缺省 true）");
        assert!(s.legend_visible(), "缺省三件套应含图例");

        let collapsed = legend_layout(false);
        assert_eq!(collapsed.group_rows, 8, "折叠态仅 8 族行");
        assert_eq!(collapsed.item_rows, 0, "折叠态零族内项");
        assert_eq!(collapsed.height, 188.0, "标题 28 + 8×20 = 188px");
        assert_eq!(collapsed.width, 168.0, "面板宽 168px");

        let expanded = legend_layout(true);
        assert_eq!(expanded.item_rows, 21, "展开态列出全部 21 个族内项");
        assert_eq!(expanded.height, LEGEND_MAX_H, "展开态封顶 340px");
        assert!(expanded.height <= 340.0, "不得占满画布");

        let mut toggled = s;
        toggled.toggle_legend();
        assert!(!toggled.collapsed, "点折叠钮应展开（collapsed → false）");
        assert_eq!(toggled.legend_layout().item_rows, 21);

        let off = MapFurnitureState {
            show_legend: false,
            ..MapFurnitureState::default()
        };
        assert!(!off.legend_visible(), "关掉 show_legend 后图例不画");
    }

    /// 族色块只读冻结表，不复制第二份色表（规格 §3.3 校验锚点）。
    #[test]
    fn legend_group_swatch_colors_are_read_straight_from_the_frozen_table() {
        assert_eq!(
            group_color([14, 15, 16, 255]),
            egui::Color32::from_rgb(0x9D, 0xC3, 0xD8),
            "水域族首色 #9DC3D8（biome_raw.rs:92）"
        );
        assert_eq!(
            group_color([20, 0, 255, 255]),
            egui::Color32::from_rgb(0x9E, 0x9E, 0x9E),
            "道路·无族首色 #9E9E9E（biome_raw.rs:98）"
        );
    }
}
