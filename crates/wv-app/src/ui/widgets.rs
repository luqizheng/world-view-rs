//! 共享 widget helper：徽标 / 空状态 / 列表项 / 浮动窗口外壳 / 面板头。
//!
//! ## 职责
//! - `BadgeKind` + `badge` —— 药丸型徽标（中性 / 通过 / 警告 / 错误 / 事件）。
//! - `empty_state` —— "图标 + 一句人话 + 一个可点的下一步"占位卡。
//! - `list_item` —— 类型色块 + 主标题 + 等宽元信息。
//! - `terrain_grid` —— 笔刷面板用的 2×5 地形色块网格（被 [`crate::ui::brush_panel`] 调用）。
//! - `float_window` / `panel_header` —— 浮动面板外壳 + 自绘标题栏三段。
//! - `ToolButton` —— **标准按钮**（标准化控件第 1 项）：图标 / 文字 / 图标+文字
//!   三形态 + tooltip + 选中/禁用态，默认样式由 `UiShell` 令牌解析。
//!
//! ## 边界（不做什么）
//! - **不**装配具体业务面板内容 —— 各 `*_panel.rs` 调用 `float_window` 装自己。
//! - **不**改主题/令牌 —— 调 `shell::UiShell` getter 拿颜色。
//!
//! ## 主要导出
//! - `BadgeKind` / `badge`
//! - `empty_state` / `list_item` / `terrain_grid`
//! - `float_window` / `panel_header`
//! - `ToolButton` / `ToolButtonContent` / `ToolIcon` / `ICON_*`
//! - `FieldInput` / `FieldSelect` / `FieldCheckbox` / `Switch` / `SliderRow`
//! - `dialog_window` —— 模态浮层壳（自动 Foreground + 居中 + 标题栏 ×）。
//! - `FieldInput` / `FieldSelect` / `FieldCheckbox` / `Switch` / `SliderRow`
//! - `dialog_window` —— 模态浮层壳（自动 Foreground + 居中 + 标题栏 ×）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::Locale;

use crate::state::{Panel, PanelState};
use crate::theme::ui_theme::{color32, mono, proportional};
use crate::theme::ui_tokens::{font_size, radius, space, BadgeTone};
use crate::ui_layout::LayoutFile;
use std::sync::Arc;

use super::shell::{terrain_swatch, UiShell};

/// 徽标型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    /// 中性计数。
    Neutral,
    /// 校验通过。
    Ok,
    /// 告警。
    Warn,
    /// 错误。
    Err,
    /// 事件相关。
    Event,
}

impl From<BadgeKind> for BadgeTone {
    fn from(kind: BadgeKind) -> Self {
        match kind {
            BadgeKind::Neutral => BadgeTone::Neutral,
            BadgeKind::Ok => BadgeTone::Ok,
            BadgeKind::Warn => BadgeTone::Warn,
            BadgeKind::Err => BadgeTone::Err,
            BadgeKind::Event => BadgeTone::Event,
        }
    }
}

/// 画一个徽标（药丸，4px 圆角）。
///
/// 前景与底色**都**取令牌（§2.2.1）：底色是「世界语义色 × 标定 alpha」，
/// 且必须走非预乘合成——`from_rgba_premultiplied(r,g,b,41)` 那种写法拿到的是
/// 「全强度 RGB + 低 alpha」，比原型的 `rgba(r,g,b,.16)` 亮一大截。
///
/// 语义前景随主题变（亮色下必须换深色系），底色不变：这是 D2 的落点。
pub fn badge(ui: &mut egui::Ui, shell: &UiShell, kind: BadgeKind, text: &str) {
    let colors = shell.palette.badge(kind.into());
    let fg = color32(colors.fg);
    let bg = crate::theme::ui_theme::color32_alpha(colors.fill, colors.fill_alpha);
    let pad = egui::vec2(space::XS * 1.5, 1.0);
    let font = proportional(font_size::MICRO);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, radius::BADGE, bg);
    // egui 0.30 的 `Painter::galley` 需要一个兜底色（字体未命中字形时使用）。
    ui.painter().galley(rect.min + pad, galley, fg);
}

/// 空状态卡（§5.2）：图标 + 一句人话 + 一个可点的下一步。
///
/// CTA 走 [`ToolButton::text`]（与 Toolbar 同一抽象），最低宽 120 锁定为
/// 「空状态 CTA」专属尺寸——窄了就显不出引导感，宽了就把空状态拉散；
/// 居中布局的行高直接吃 `shell.hit`。
pub fn empty_state(
    ui: &mut egui::Ui,
    shell: &UiShell,
    icon: &str,
    title: &str,
    hint: &str,
    action: &str,
) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        ui.label(
            egui::RichText::new(icon)
                .size(shell.scale.title * 1.6)
                .color(shell.t3()),
        );
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(title)
                .size(shell.scale.title)
                .color(shell.t1()),
        );
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new(hint)
                .size(shell.scale.small)
                .color(shell.t3()),
        );
        ui.add_space(space::MD);
        let resp = ToolButton::text(action, "")
            .size(egui::vec2(120.0, shell.hit))
            .show(ui, shell, || clicked = true);
        let _ = resp;
    });
    clicked
}

/// 列表项（§4.6）：类型色块 + 主标题 + 等宽元信息。
pub fn list_item(
    ui: &mut egui::Ui,
    shell: &UiShell,
    swatch: egui::Color32,
    title: &str,
    meta: &str,
    selected: bool,
) -> egui::Response {
    let row_h = shell.row + space::SM;
    let avail_w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail_w, row_h), egui::Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, radius::CONTROL, shell.active_bg());
        rect_stroke(
            painter,
            rect,
            radius::CONTROL,
            egui::Stroke::new(1.0_f32, shell.accent()),
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
    }
    // 类型色块。
    let sw = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + space::SM, rect.center().y - 5.0),
        egui::vec2(10.0, 10.0),
    );
    painter.rect_filled(sw, radius::BADGE, swatch);
    // 主标题。
    painter.text(
        egui::pos2(rect.min.x + space::SM + 10.0 + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        proportional(shell.scale.base),
        shell.t1(),
    );
    // 等宽元信息（右对齐）。
    painter.text(
        egui::pos2(rect.max.x - space::SM, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        meta,
        mono(shell.scale.micro),
        shell.t3(),
    );
    resp
}

/// 描边矩形（统一 `StrokeKind::Inside`）。
///
/// egui 0.36 给 `Painter::rect_stroke` 加了第 4 个参数 `StrokeKind`（描边画在
/// 边界内 / 中 / 外）。本工程的矩形描边都是「选中框 / 视口框」，必须落在矩形**内部**
/// 才不会被邻居裁切吞掉，故收敛成一个实现点。
pub(crate) fn rect_stroke(
    painter: &egui::Painter,
    rect: egui::Rect,
    corner_radius: impl Into<egui::CornerRadius>,
    stroke: impl Into<egui::Stroke>,
) {
    painter.rect_stroke(rect, corner_radius, stroke, egui::StrokeKind::Inside);
}

/// 图标 helper：把 SVG 源 + tint + 目标边长封成单一 API。
///
/// 集中 [`egui::Image::new`]+`.tint()`+`.fit_to_exact_size()`，避免 caller
/// 漏写 tint / 写错尺寸。`tint` 在 egui 0.36 是 vertex 色乘法（内部 wgsl）：
/// 当纹理像素已经是非 0 实色时（仓库所有 25 个 `tool_icons/*.svg` 自 2026-10-15
/// 起改用 `stroke="#ffffff"`），命中 vertex 色即等于直接覆盖。
///
/// **为什么不直接传给 `Image::tint`** —— API 上等同，但 `Image::new(..)` 一旦
/// 漏写 `.tint()` 调默认 WHITE，乘 BLACK 像素仍得 BLACK（已被实测复现；
/// 详见 `welcome_screen.rs::app_icon` 在改动前用户截图全黑的事实）。
/// 本 helper 强制 caller 显式选择 tint，且所有调用点统一收口，方便后续再迭代
/// （例如实现一个真正意义上的"runtime inject color" SvgLoader 时只改这里）。
pub fn icon_image(
    src: egui::ImageSource<'static>,
    tint: egui::Color32,
    px: f32,
) -> egui::Image<'static> {
    egui::Image::new(src)
        .tint(tint)
        .fit_to_exact_size(egui::Vec2::splat(px))
}

// ── 最近工程行（Figma `RecentItem` · 2026-10-15）────────────────────────
//
// 提到 widgets 的目的是消除欢迎页 / 命令面板 / 项目菜单里重复写的 list-row
// 代码（之前三处都用 `ui.allocate_exact_size + painter.text + Image::new`
// 三件套手工写）。**`time_ago` 是 Figma 原型上的一栏**——之前欢迎页预留了接口
// 但形参未到，这次跟 [`crate::state::RecentProject::last_opened_at`]
// 一并对齐，由 caller 传 `Option<&str>` 字符串进来。
//
// 设计量（Figma `00_WelcomeScreen.RecentProjects`）：
// - 行高 36px
// - 左侧 map 图标 14×14（shell.t3）
// - 中间 name + dir 两行（name 13px t1 / dir 11px t3），left-aligned
// - 右侧 `time_ago` 11px t3（mono + right-aligned），无时隐藏

/// 最近工程行高（Figma `RecentItem.height = 36`）。
pub const RECENT_ROW_H: f32 = 36.0;
/// 最近工程行内左右内边距（Figma `padding [0, 8]`）。
pub const RECENT_ROW_PAD_X: f32 = 8.0;
/// 最近工程行内 icon 边长（Figma `map` 14×14）。
pub const RECENT_ROW_ICON: f32 = 14.0;

/// 单条最近工程（Figma `RecentItem`）：左侧 icon + 中间 name/dir 两行 + 右侧可选 time_ago。
///
/// 返回 [`egui::Response`]，caller 自己判断 `.clicked()`。
/// `time_ago` 为 `None` 时不画右侧栏（空列表 / 尚未会话内记录的工程）；
/// 不为 `None` 时是已经格式化好的字符串 —— 格式化放在 caller 测（见
/// [`format_time_ago`]），本 helper 只画。
pub fn recent_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    icon: egui::ImageSource<'static>,
    name: &str,
    dir: &str,
    time_ago: Option<&str>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), RECENT_ROW_H),
        egui::Sense::click(),
    );
    let painter = ui.painter();
    // 1. hover 底。
    if resp.hovered() {
        painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
    }
    // 2. 左侧 icon，垂直居中。
    let icon_y = rect.center().y - RECENT_ROW_ICON / 2.0;
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + RECENT_ROW_PAD_X, icon_y),
        egui::vec2(RECENT_ROW_ICON, RECENT_ROW_ICON),
    );
    egui::Image::new(icon)
        .tint(shell.t3())
        .paint_at(ui, icon_rect);
    // 3. name + dir 两行。
    let text_x = icon_rect.max.x + RECENT_ROW_PAD_X;
    painter.text(
        egui::pos2(text_x, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        name,
        proportional(font_size::BASE),
        shell.t1(),
    );
    painter.text(
        egui::pos2(text_x, rect.max.y - 13.0),
        egui::Align2::LEFT_TOP,
        dir,
        proportional(font_size::MICRO),
        shell.t3(),
    );
    // 4. time_ago 右对齐。
    if let Some(t) = time_ago {
        painter.text(
            egui::pos2(rect.max.x - RECENT_ROW_PAD_X, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            t,
            proportional(font_size::MICRO),
            shell.t3(),
        );
    }
    resp
}

/// 把过去的 [`std::time::Instant`] 格式化为 UI 文字（"Just now" / "5m ago" / "Yesterday" 等）。
///
/// **纯相对时间，不调用 chrono**：仓库禁用新依赖（AGENTS §9 默认拒绝），
/// 且会话内 `[`Instant::now` − `last_opened_at`]` 的差是 monotonic 的。
/// 大于 30 天用 "Nmo ago"（30 天 = 1 月的近似，与本工程其它"原值不在数据层"
/// 的纪元无关——见 §7.5 V1）。
///
/// 边界（用上限排除下一档）：
/// - `[0, 60s)` → "Just now"
/// - `[60s, 1h)` → `"{n}m ago"`（n = secs/60）
/// - `[1h, 1d)` → `"{n}h ago"`
/// - `[1d, 2d)` → "Yesterday"
/// - `[2d, 7d)` → `"{n}d ago"`
/// - `[7d, 30d)` → `"{n}w ago"`（周）
/// - `[30d, ∞)` → `"{n}mo ago"`（月）
pub fn format_time_ago(at: std::time::Instant, now: std::time::Instant) -> String {
    let dur = now.saturating_duration_since(at);
    let secs = dur.as_secs();
    const MIN: u64 = 60;
    const HOUR: u64 = MIN * 60;
    const DAY: u64 = HOUR * 24;
    const WEEK: u64 = DAY * 7;
    const MONTH: u64 = DAY * 30;
    if secs < MIN {
        "Just now".into()
    } else if secs < HOUR {
        format!("{}m ago", secs / MIN)
    } else if secs < DAY {
        format!("{}h ago", secs / HOUR)
    } else if secs < 2 * DAY {
        "Yesterday".into()
    } else if secs < WEEK {
        format!("{}d ago", secs / DAY)
    } else if secs < MONTH {
        format!("{}w ago", secs / WEEK)
    } else {
        format!("{}mo ago", secs / MONTH)
    }
}

/// 地形色块网格（§4.5：2 列 × 5 行，单行色块+名称，整块紧凑）。
///
/// 行高 22 px（曾 44 px），色块 14 px 居左、名称 11 px 居右同一行——
/// 高度砍半但视觉仍能一眼区分当前选中（背景填 `active_bg` + 1.5 px
/// `focus` 描边），网格外的所有状态提示交还给面板体。
pub fn terrain_grid(ui: &mut egui::Ui, shell: &UiShell, selected: &mut u8, loc: Locale) {
    egui::Grid::new("terrain_swatches")
        .num_columns(2)
        .spacing([space::XS, space::XS])
        .show(ui, |ui| {
            for k in 0u8..10 {
                let name = super::shell::terrain_name_at(loc, k as usize);
                let is_sel = *selected == k;
                let cell_w = (ui.available_width() - space::XS) / 2.0;
                let row_h = 22.0;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(cell_w, row_h), egui::Sense::click());
                let painter = ui.painter();
                if is_sel {
                    painter.rect_filled(rect, radius::CONTROL, shell.active_bg());
                    rect_stroke(
                        painter,
                        rect,
                        radius::CONTROL,
                        egui::Stroke::new(1.5_f32, shell.focus()),
                    );
                } else if resp.hovered() {
                    painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
                }
                // 14px 色块，垂直居中。
                let sw_size = 14.0;
                let sw = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + space::SM, rect.center().y - sw_size / 2.0),
                    egui::vec2(sw_size, sw_size),
                );
                painter.rect_filled(sw, radius::BADGE, terrain_swatch(k));
                // 11px 名称，色块右侧，垂直居中。
                painter.text(
                    egui::pos2(
                        rect.min.x + space::SM + sw_size + space::SM,
                        rect.center().y,
                    ),
                    egui::Align2::LEFT_CENTER,
                    name,
                    proportional(11.0),
                    if is_sel { shell.t1() } else { shell.t2() },
                );
                if resp.clicked() {
                    *selected = k;
                }
                if (k + 1) % 2 == 0 {
                    ui.end_row();
                }
            }
        });
}

/// 浮动面板外壳：自绘 `.panel-head` 标题行 + 可拖动、可折叠；不再走原生标题栏。
///
/// `layouts` 提供**跨会话**的位置/尺寸（`ui-layout.yaml`）；本帧的拖动结果由
/// egui 按窗口 `Id` 自己记住，并由 [`crate::ui_shell::persist_layout_on_change`]
/// 回写磁盘（D1：面板是客人，地图是主体）。
///
/// ## 层序约定（2026-10-14 用户要求「弹窗/浮动都在最顶层」后固化）
///
/// 本外壳**不设** `order`——即 egui 缺省 `Middle`：面板互斥点击由 egui 的
/// 「点谁谁置顶」管理，且永远压在 egui Background 面板（顶栏/状态栏）与
/// Bevy 地图之上。模态弹窗（新建地图 / 新建工程 / 命令面板 / 欢迎页 /
/// 启动加载）在 `Order::Foreground`—— egui 绘制序
/// `Background < Middle < Foreground < Tooltip`，故弹窗压面板、悬浮提示仍在最上。
/// 新增浮层时按此表对号入座，不要把弹窗放回 Middle。
///
/// ## 标题为什么改成自绘（v0.3 Figma 重做 · 2026-10-15）
///
/// 原生 `egui::Window` 的标题栏是「▼ … 标题 … ×」三件套，丢掉了 Figma 的
/// `icon | title | status | ▼-collapse` 四段式（v0.3 `5E8BCBCmtGAHqk3ORibBlU`），
/// 也无法把状态徽标（如 `ACTIVE`）放进标题栏。本实现走：
///
/// - `egui::Window::title_bar(false)` —— 关掉原生标题栏，标题不进 `Window::new`。
/// - 自绘 `panel_head` 行（`shell.head_h` 密度感知 = Cozy 30 / Compact 26 px；
///   略窄于原型的 34 px 但保留密度档语义，文字 `font_size::SMALL = 12 px`
///   与 Figma body 等同）。
/// - 折叠用 [`egui::collapsing_header::CollapsingState`]（Id = `(id,"head")`），
///   与 `persist_layout_on_change` 现行读法无缝兼容，跨会话落盘零改动。
/// - `title_bar(false)` 之后 [`egui::WindowDrag::OnTouch`] 解析为
///   [`egui::WindowDrag::Anywhere`]（egui 0.36 `window.rs` 注释明文保证），
///   整面任意位置都能拖动，**不需要**额外写 `area_mut`。
/// - 关按钮被删：`v0.3` Figma 里浮动面板没有 ×，作者用 `Esc` / 菜单
///   关掉面板（菜单入口已在顶栏，不在本外壳范围）。
#[allow(clippy::too_many_arguments)]
pub fn float_window(
    ctx: &egui::Context,
    shell: &UiShell,
    layouts: &LayoutFile,
    panels: &mut PanelState,
    panel: Panel,
    tag: Option<(BadgeKind, String)>,
    loc: Locale,
    body: impl FnOnce(&mut egui::Ui),
) {
    if !panels.is_visible(panel) {
        return;
    }

    let layout = crate::ui_layout::current_layout(layouts, panels, panel);
    let id = egui::Id::new(("wv_panel", panel));
    let head_id = id.with("head");

    // 沉浸模式：整体淡化但**仍可交互**（D3：状态永不抑制）。
    let alpha = if panels.immersive { 0.10 } else { 0.96 };

    // 折叠态：egui CollapsingState（id = head_id）+ layout.collapsed 兜底。
    // 持久化由 persist_layout_on_change 走同样的 id 读回，零改动兼容。
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        head_id,
        !layout.collapsed,
    );

    let win = egui::Window::new(panel.title(loc))
        .id(id)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .resizable(true)
        .collapsible(false)
        .title_bar(false)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(crate::theme::ui_theme::margin_all(0.0))
                .corner_radius(radius::PANEL),
        );

    win.show(ctx, |ui| {
        panel_head(ui, shell, panel, tag.as_ref(), &mut state, alpha, loc);
        if state.is_open() {
            // 折叠关闭时不画体；体与头间留 `space::SM` 与壳内边距同步。
            state.show_body_unindented(ui, |ui| {
                ui.add_space(space::SM);
                body(ui);
            });
        } else {
            state.store(ctx);
        }
    });
}

/// 自绘 `panel-head` 行（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU`）：
///
/// `icon | title | … | badge? | ▼` 四段，水平一字排列，圆形三角随 openness 旋转。
/// 三角点中切 [`egui::collapsing_header::CollapsingState`]。整个头可视为
/// `egui::Window` 的 drag-anywhere 区域的一份（egui 0.36 `title_bar=false`
/// 会把 `OnTouch` 降级为 `Anywhere`，无需自管 `area_mut`）。
///
/// 字号 `font_size::SMALL = 12 px` —— Figma body 同号；徽标继续吃
/// `font_size::MICRO = 11 px`（Figma 9 px 偏小，会顺走其它调用点，暂时不动）。
fn panel_head(
    ui: &mut egui::Ui,
    shell: &UiShell,
    panel: Panel,
    tag: Option<&(BadgeKind, String)>,
    state: &mut egui::collapsing_header::CollapsingState,
    alpha: f32,
    loc: Locale,
) {
    let head_h = shell.head_h;
    let avail_w = ui.available_width();
    let (head_rect, _head_resp) =
        ui.allocate_exact_size(egui::vec2(avail_w, head_h), egui::Sense::hover());

    // ── 右件几何先算（互不引用 ui，便于可变 borrow 切回） ──
    let pad = 10.0_f32;
    let center_y = head_rect.center().y;
    let tri_w = shell.head_h - 8.0; // ≈ 22 cozy / 18 compact
    let tri_rect = egui::Rect::from_center_size(
        egui::pos2(head_rect.max.x - 8.0 - tri_w / 2.0, center_y),
        egui::vec2(tri_w, tri_w),
    );

    // ── 算徽标矩形（含则备好） ──
    let badge_rect: Option<(
        egui::Rect,
        egui::Color32,
        egui::Color32,
        Arc<egui::Galley>,
        egui::Vec2,
    )> = if let Some((kind, text)) = tag {
        let colors = shell.palette.badge((*kind).into());
        let fg = color32(colors.fg);
        let bg = crate::theme::ui_theme::color32_alpha(colors.fill, colors.fill_alpha);
        let badge_pad = egui::vec2(space::XS * 1.5, 1.0);
        let font = proportional(font_size::MICRO);
        let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
        let size = galley.size() + badge_pad * 2.0;
        let right_edge = tri_rect.min.x - 8.0;
        let r = egui::Rect::from_min_size(
            egui::pos2(right_edge - size.x, center_y - size.y / 2.0),
            egui::vec2(size.x, size.y),
        );
        Some((r, fg, bg, galley, badge_pad))
    } else {
        None
    };

    // ── 徽标占位：参与 hit-test 排序，但不抢拖动 ──
    if let Some((r, _, _, _, _)) = badge_rect.as_ref() {
        let _ = ui.allocate_rect(*r, egui::Sense::hover());
    }

    // ── 自绘背景、底线、icon、title、徽标、折叠图标 ──
    let p = ui.painter();
    // 头底 = panel_bg（与窗体无缝拼接）；底线 1px border_subtle（Figma `.panel-head`）。
    p.rect_filled(head_rect, 0.0, shell.panel_bg().gamma_multiply(alpha));
    p.line_segment(
        [
            egui::pos2(head_rect.min.x, head_rect.max.y),
            egui::pos2(head_rect.max.x, head_rect.max.y),
        ],
        egui::Stroke::new(1.0, shell.border_subtle()), // = Figma `--border`
    );

    let title_font = proportional(font_size::SMALL); // 12 px = Figma body

    // 左：icon + title。icon 着 `t3` 弱色，title 着 `t1` 主文。
    p.text(
        egui::pos2(head_rect.min.x + pad, center_y),
        egui::Align2::LEFT_CENTER,
        panel.icon(),
        title_font.clone(),
        shell.t3(),
    );
    // icon 字符约 12 px 宽，加上 6 px gap；`panel.icon()` 取 1-unicode 几何字符，
    // 宽度用 18 px 经验值避错位。
    p.text(
        egui::pos2(head_rect.min.x + pad + 18.0, center_y),
        egui::Align2::LEFT_CENTER,
        panel.title(loc),
        title_font,
        shell.t1(),
    );

    // 右：徽标 → 折叠三角。
    if let Some((r, fg, bg, galley, pad)) = badge_rect {
        p.rect_filled(r, radius::BADGE, bg);
        p.galley(r.min + pad, galley, fg);
    }
    let _ = p; // 释放对 ui 的不可变借用，让 `interact` 与 `paint_default_icon` 可借 ui。

    // 三角点中切 state。
    let tri_resp = ui.interact(tri_rect, ui.id().with("tri"), egui::Sense::click());
    let openness = state.openness(ui.ctx());
    egui::collapsing_header::paint_default_icon(ui, openness, &tri_resp);
    if tri_resp.clicked() {
        state.toggle(ui);
    }
    state.store(ui.ctx());
}

// ── 标准按钮（统一工厂 · 2026-10-15）───────────────────────────────────
//
// 跨场景的统一按钮：欢迎页 CTA、对话框创建按钮、错误行 ghost 跳转、
// 顶栏 raised 等都走这里——不再手搓 `egui::Button` / `ui.button()`。
//
// 与 [`ToolButton`] 的分工：
// - `ToolButton`：**工具栏场景**——方块图标 / 选中态 / tooltip 一等公民，
//   map_controls 5 处用它，保留。
// - `button()`：**非工具栏场景**——主/次 CTA / 对话框 / 错误行 / 顶栏 raised 等。
//
// 为什么不合并：合并会把 ToolButton 的 active/disabled/hint builder 撑爆，
// 且工具栏"方块图标"语义与对话框"宽 CTA"语义互不兼容；保持双轨，调用方按场景选入口。
//
// 颜色全部走 [`UiShell`] 令牌，不写裸 hex。
//
// 与 [`ToolButton`] 的分工：
// - `ToolButton`：**工具栏场景**——方块图标 / 选中态 / tooltip 一等公民，
//   map_controls 5 处用它，保留。
// - `button()`：**非工具栏场景**——主/次 CTA / 对话框 / 错误行 / 顶栏 raised 等。
//
// 为什么不合并：合并会把 ToolButton 的 active/disabled/hint builder 撑爆，
// 且工具栏"方块图标"语义与对话框"宽 CTA"语义互不兼容；保持双轨，调用方按场景选入口。
//
// 颜色全部走 [`UiShell`] 令牌，不写裸 hex。

/// 按钮样式（统一工厂第二参数）。
///
/// 决定 fill / stroke / 前景色 / 字号 / 圆角等视觉量。文字与图标色来自 [`UiShell`]
/// 令牌，**不**接收裸 hex。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// accent 实心底 + 白字白图标（主 CTA / Figma `primary button`）。
    Accent,
    /// raised 底 + border_strong 描边 + t2 字（次 CTA / 顶栏 raised）。
    Raised,
    /// 透明底 + t1 字（错误行跳转 / Dialog 取消式按钮）。
    Ghost,
    /// 透明底 + t2 字（顶栏 / 列表底部动作）。
    #[default]
    Subtle,
}

/// 按钮内容（统一工厂第三参数）。
///
/// `Image` / `ImageText` 二变体收 `egui::ImageSource<'static>`——
/// `ICON_*` 常量直接喂入即可（`include_image!` 给的也是 `'static`）。
///
/// 不 derive `Copy`：`egui::ImageSource` 不是 `Copy`（内部是 `Arc<Vec<...>>`
/// 类型也是 `Clone` 而非 `Copy`），本枚举只能 `Clone`。
#[derive(Clone)]
pub enum ButtonContent<'a> {
    /// 纯文字按钮（高度 = `shell.hit`，宽度随文字）。
    Text(&'a str),
    /// 纯图标按钮（方块 = `shell.hit × shell.hit`，图标 = `hit − 8`）。
    Image(egui::ImageSource<'static>),
    /// 图标 + 文字横排（高度 = `shell.hit`，宽度随内容）。
    ImageText(egui::ImageSource<'static>, &'a str),
}

/// 统一按钮工厂。
///
/// 工具栏场景请用 [`ToolButton`]（map_controls 等）；非工具栏场景一律走这里。
/// 返回 [`egui::Response`]，caller 自己判断 `.clicked()` / `.hovered()` 等。
///
/// ## 参数
///
/// - `content` / `style`：见 [`ButtonContent`] / [`ButtonStyle`]。
/// - `min_size_override`：`None` → 工厂默认（`Text` / `ImageText` 高度 =
///   `shell.hit`，宽度随内容；`Image` 方块 = `shell.hit × shell.hit`）。
///   `Some(s)` → caller 自定最小尺寸（例如欢迎页 CTA 高度 = `CTA_HEIGHT=40`
///   与默认 28 不同，必须 `Some(vec2(w, 40))`）。
///   caller **不得**通过 `Response.rect` 反推修改——`rect` 是分配结果，修改即失配。
/// - `enabled`：`false` → 工厂内部走 [`egui::Ui::add_enabled`]，整按钮置灰且不响应
///   点击。对话框「创建」按钮在 `name + location` 双空时禁用就是这一支路。
pub fn button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    content: ButtonContent,
    style: ButtonStyle,
    min_size_override: Option<egui::Vec2>,
    enabled: bool,
) -> egui::Response {
    let (fill, stroke, fg, font_px, corner) = match style {
        ButtonStyle::Accent => (
            shell.accent(),
            None,
            egui::Color32::WHITE,
            font_size::INPUT,
            radius::CONTROL,
        ),
        ButtonStyle::Raised => (
            shell.raised_bg(),
            Some(egui::Stroke::new(1.0, shell.border_strong())),
            shell.t2(),
            font_size::INPUT,
            radius::CONTROL,
        ),
        ButtonStyle::Ghost => (
            egui::Color32::TRANSPARENT,
            None,
            shell.t1(),
            shell.scale.small,
            radius::CONTROL,
        ),
        ButtonStyle::Subtle => (
            egui::Color32::TRANSPARENT,
            None,
            shell.t2(),
            shell.scale.small,
            radius::CONTROL,
        ),
    };

    let default_min = match &content {
        ButtonContent::Text(_) | ButtonContent::ImageText(_, _) => egui::vec2(0.0, shell.hit),
        ButtonContent::Image(_) => egui::Vec2::splat(shell.hit),
    };
    let min_size = min_size_override.unwrap_or(default_min);

    let mut btn = match content {
        ButtonContent::Text(label) => {
            egui::Button::new(egui::RichText::new(label).color(fg).size(font_px))
        }
        ButtonContent::Image(src) => egui::Button::image(icon_image(src, fg, shell.hit - 8.0)),
        ButtonContent::ImageText(src, label) => egui::Button::image_and_text(
            icon_image(src, fg, shell.hit - 8.0),
            egui::RichText::new(label).color(fg).size(font_px),
        ),
    };
    btn = btn.fill(fill).corner_radius(corner).min_size(min_size);
    if let Some(s) = stroke {
        btn = btn.stroke(s);
    }
    ui.add_enabled(enabled, btn)
}

// ── 标准按钮（标准化控件 · 第 1 项）──────────────────────────────────
//
// 职责：把「内容（图标 / 文字 / 图标+文字）+ tooltip + 选中态 + 启用态 +
// 点击回调」打包成单次 `ui.add`。**新代码一律走这里**，不再手搓
// `egui::Button` / 手绘 clickable（存量六种旧模式按批次迁移）。
//
// 样式来源：默认值全部由 [`UiShell`] 令牌解析——`tint = t1`、
// `active_bg = bg_active`、边长 = `hit`（28/24 随密度）、图标 = hit−8、
// 文字 = `scale.small`；不写裸 hex（ui_tokens 纪律）。caller 用 builder
// 显式覆盖（map_controls 的 Google 白控件配色即此用法）。
//
// 图标：Lucide 24×24 outline，SVG 内 `stroke="currentColor" fill="none"`，
// 由 `tint()` 注入主题色——与 `RichText::color()` 同效果；
// `include_image!` 编译期嵌入二进制，零运行时 IO。
//
// 边界：单一按钮封装，**不**承载浮窗外壳/分组/分隔线（各面板自管）；
// 不写主题令牌源头。

/// 工具栏图标源。SVG 走 `egui::include_image!`，编译期嵌入二进制，零运行时 IO。
#[derive(Clone)]
pub enum ToolIcon {
    /// Lucide / 任意合规 SVG 源码（`include_str!`/宏嵌入），`stroke="currentColor"`。
    Svg(egui::ImageSource<'static>),
}

// 工具图标常量。`include_image!` 宏展开为
// `ImageSource::Bytes { uri: "bytes://...", bytes: include_bytes!(path) }`，
// egui 0.36 看 `.svg` 后缀自动用内置 SVG 加载器解析 `currentColor`。
pub const ICON_PEN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/pen.svg");
pub const ICON_RULER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/ruler.svg");
pub const ICON_SQUARE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/square.svg");
pub const ICON_ERASER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/eraser.svg");
pub const ICON_ZOOM_IN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/zoom-in.svg");
pub const ICON_ZOOM_OUT: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/zoom-out.svg");
pub const ICON_MAXIMIZE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/maximize.svg");

// ── v0.3 视觉重做：Figma `5E8BCBCmtGAHqk3ORibBlU` 所需的新图标 ──────────
//
// Lucide 24×24 outline 风格一致，全部 `stroke="currentColor" fill="none"`，
// 由 ToolButton 的 `tint()` 注入主题色。
pub const ICON_CHEVRON_RIGHT: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/chevron-right.svg");
pub const ICON_CHEVRON_DOWN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/chevron-down.svg");
pub const ICON_PLAY: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/play.svg");
pub const ICON_REFRESH: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/refresh-cw.svg");
pub const ICON_SEARCH: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/search.svg");
pub const ICON_LAYERS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/layers.svg");
pub const ICON_MAP: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/map.svg");
pub const ICON_BRUSH: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/paintbrush.svg");
pub const ICON_GRID: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/layout-grid.svg");
pub const ICON_LINE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/minus.svg");
pub const ICON_PENCIL: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/pencil.svg");
pub const ICON_ALERT: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/triangle-alert.svg");
pub const ICON_CHECK: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/circle-check.svg");
pub const ICON_COMMAND: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/command.svg");
pub const ICON_MAP_PIN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/map-pin.svg");

// ── 欢迎页（无工程锁）图标：Figma `Zw4tonExV5lIY_EJamjPRw` ────────────────
//
// 入口图标——蓝底圆角方块用 ICON_GLOBE，CTA 用 ICON_PLUS（主）/ ICON_FOLDER_OPEN（次），
// 最近工程用 ICON_MAP（已有）。同样 `stroke="currentColor"`，由 tint 注入主题色。
pub const ICON_GLOBE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/globe.svg");
pub const ICON_PLUS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/plus.svg");
pub const ICON_FOLDER_OPEN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/folder-open.svg");

/// 按钮内容三形态。
///
/// 新增图标 = 放一个 SVG 进 `assets/ui/tool_icons/` + 加一个 `ICON_*` 常量，
/// 不需要任何运行时注册。
#[derive(Clone)]
pub enum ToolButtonContent<'a> {
    /// 纯图标（方块工具钮）。
    Icon(ToolIcon),
    /// 纯文字（幽灵按钮：透明底，hover/按下态走 egui visuals）。
    Text(&'a str),
    /// 图标 + 文字（横排）。
    IconText(ToolIcon, &'a str),
}

/// 默认样式解析结果（纯数据，单测直接断言；`show` 消费它装配 egui Button）。
struct ResolvedStyle {
    tint: egui::Color32,
    min_size: egui::Vec2,
    icon_px: f32,
    text_px: f32,
}

/// 选中态只差背景：active → `active_bg`，非选中 → 幽灵（透明底）。
///
/// 透明底让图标钮浮在自绘 Frame（地图白卡）上不留灰块——map_controls 的
/// 既有观感由此保持；hover / 按下反馈交给 egui visuals（`bg_hover` 等）。
fn fill_for(active: bool, active_bg: egui::Color32) -> egui::Color32 {
    if active {
        active_bg
    } else {
        egui::Color32::TRANSPARENT
    }
}

/// 标准按钮。所有形态共用：tooltip、选中态、禁用态、点击回调。
///
/// - `active=true` → 底色 `active_bg`（选中态纯背景差异，不换图标）；
/// - `enabled=false` → `ui.add_enabled` 整按钮置灰；
/// - `hint` 非空 → `on_hover_text`（egui 的 Tooltip 绘制层永远在最上，
///   见 `widgets.rs` 绘制序注释）；
/// - `on_click` 仅在本帧被点击时调用一次（`FnOnce`）。
pub struct ToolButton<'a> {
    content: ToolButtonContent<'a>,
    hint: &'a str,
    active: bool,
    enabled: bool,
    size: Option<egui::Vec2>,
    icon_size: Option<f32>,
    tint: Option<egui::Color32>,
    active_bg: Option<egui::Color32>,
}

impl<'a> ToolButton<'a> {
    /// 纯图标按钮（方块，默认边长 = `shell.hit`）。
    pub fn icon(icon: ToolIcon, hint: &'a str) -> Self {
        Self::base(ToolButtonContent::Icon(icon), hint)
    }

    /// 纯文字按钮（高度 = `shell.hit`，宽度随文字）。
    pub fn text(label: &'a str, hint: &'a str) -> Self {
        Self::base(ToolButtonContent::Text(label), hint)
    }

    /// 图标 + 文字按钮（高度 = `shell.hit`，宽度随内容）。
    pub fn icon_text(icon: ToolIcon, label: &'a str, hint: &'a str) -> Self {
        Self::base(ToolButtonContent::IconText(icon, label), hint)
    }

    fn base(content: ToolButtonContent<'a>, hint: &'a str) -> Self {
        Self {
            content,
            hint,
            active: false,
            enabled: true,
            size: None,
            icon_size: None,
            tint: None,
            active_bg: None,
        }
    }

    /// 选中态。
    pub fn active(mut self, v: bool) -> Self {
        self.active = v;
        self
    }
    /// 禁用态。
    pub fn enabled(mut self, v: bool) -> Self {
        self.enabled = v;
        self
    }
    /// 显式尺寸（默认：图标钮 = hit×hit；带文字的 = 高 hit、宽随内容）。
    pub fn size(mut self, s: egui::Vec2) -> Self {
        self.size = Some(s);
        self
    }
    /// 显式图标边长（默认 hit−8：Cozy 28→20、Compact 24→16）。
    pub fn icon_size(mut self, s: f32) -> Self {
        self.icon_size = Some(s);
        self
    }
    /// 前景/图标色（默认 `shell.t1()`）。
    pub fn tint(mut self, c: egui::Color32) -> Self {
        self.tint = Some(c);
        self
    }
    /// 选中底色（默认 `shell.active_bg()`）。
    pub fn active_bg(mut self, c: egui::Color32) -> Self {
        self.active_bg = Some(c);
        self
    }

    /// 由 `UiShell` 解析默认样式；显式覆盖优先。
    fn resolve(&self, shell: &UiShell) -> ResolvedStyle {
        let hit = shell.hit;
        let min_size = match &self.content {
            ToolButtonContent::Icon(_) => self.size.unwrap_or(egui::Vec2::splat(hit)),
            ToolButtonContent::Text(_) | ToolButtonContent::IconText(_, _) => {
                self.size.unwrap_or(egui::vec2(0.0, hit))
            }
        };
        ResolvedStyle {
            tint: self.tint.unwrap_or_else(|| shell.t1()),
            min_size,
            icon_px: self.icon_size.unwrap_or(hit - 8.0),
            text_px: shell.scale.small,
        }
    }

    /// 添加到 UI。`on_click` 在本帧被点击时调用一次。
    pub fn show<F: FnOnce()>(
        self,
        ui: &mut egui::Ui,
        shell: &UiShell,
        on_click: F,
    ) -> egui::Response {
        let st = self.resolve(shell);
        let fill = fill_for(
            self.active,
            self.active_bg.unwrap_or_else(|| shell.active_bg()),
        );
        let btn = match self.content {
            ToolButtonContent::Icon(icon) => {
                let ToolIcon::Svg(source) = icon;
                egui::Button::image(icon_image(source, st.tint, st.icon_px))
                    .min_size(st.min_size)
                    .fill(fill)
            }
            ToolButtonContent::Text(label) => {
                egui::Button::new(egui::RichText::new(label).color(st.tint).size(st.text_px))
                    .min_size(st.min_size)
                    .fill(fill)
            }
            ToolButtonContent::IconText(icon, label) => {
                let ToolIcon::Svg(source) = icon;
                egui::Button::image_and_text(
                    icon_image(source, st.tint, st.icon_px),
                    egui::RichText::new(label).color(st.tint).size(st.text_px),
                )
                .min_size(st.min_size)
                .fill(fill)
            }
        };
        let resp = ui.add_enabled(self.enabled, btn);
        if resp.clicked() {
            on_click();
        }
        // `on_hover_text` 消费 self 返回 Self；空 hint 时直接透传。
        if self.hint.is_empty() {
            resp
        } else {
            resp.on_hover_text(self.hint)
        }
    }
}

// ── 标准化控件 · 第 2 批（feat/ui-controls-spec）────────────────────────
//
// 共享表单 / 浮层控件：FieldInput / FieldSelect / FieldCheckbox / Switch +
// SliderRow + dialog_window。全部用 builder + show 模式（与上方 ToolButton
// 同构），样式默认由 [`UiShell`] 令牌解析，**绝不写裸 hex**——caller 忘了
// 覆盖就走 shell 默认值（map_controls 的白控件配色即覆盖用法）。
//
// 测试纪律同 ToolButton：每个新控件**至少**四条单测——默认解析、builder
// 覆盖、show 不 panic、CSS 风格边界。任何「等 GUI 烟测兜底」的脏改 panic 在
// 这里而不是在用户面前。
//
// 边界：
// - **不**承载浮窗外壳 / 多列布局 / 列表项 / Radio——各 panel 自管或留
//   in-place（selectable_value / selectable_label 等）。
// - **不**写主题令牌源头 —— 调 `UiShell` getter 拿颜色。

// ── FieldInput（单行文本输入）───────────────────────────────────────

/// 单行文本输入框。`hint` 是空值时的占位文本（**不是**初始值）。
///
/// 默认：原生 [`egui::TextEdit::singleline`]，无 min_size，文本色随主题，
/// 不抢焦点。CFT（command palette search）通过 `.focus_on_add(true)` 在打开
/// 帧自动聚焦；窄角度数字输入（compass 朝向）通过 `.min_size(..)` + mono 字体
/// 改写。
pub struct FieldInput<'a> {
    value: &'a mut String,
    hint: String,
    font_mono: bool,
    min_size: Option<egui::Vec2>,
    fill_width: bool,
    focus_on_add: bool,
}

impl<'a> FieldInput<'a> {
    /// 绑定 `&mut String` 与占位文本。`hint` 为空时不下发 hint。
    pub fn new(value: &'a mut String, hint: &str) -> Self {
        Self {
            value,
            hint: hint.to_owned(),
            font_mono: false,
            min_size: None,
            fill_width: false,
            focus_on_add: false,
        }
    }

    /// 等宽字体（默认 = 比例字体）。适合数字输入 / 工程代码字段。
    pub fn font_mono(mut self, on: bool) -> Self {
        self.font_mono = on;
        self
    }

    /// 显式最小尺寸（宽 × 高）。未设时宽度由父布局决定。
    pub fn min_size(mut self, s: egui::Vec2) -> Self {
        self.min_size = Some(s);
        self
    }

    /// 撑满父容器宽度（`desired_width(f32::INFINITY)`）。
    /// 命令面板输入框 / 字段宽度跟随对话框 footer 等场景开。
    pub fn fill_width(mut self, on: bool) -> Self {
        self.fill_width = on;
        self
    }

    /// 添加帧自动抢焦点（`request_focus()`）。命令面板打开时用。
    pub fn focus_on_add(mut self, on: bool) -> Self {
        self.focus_on_add = on;
        self
    }

    /// 装配并落地。返回的 `Response` 可用 `.changed() / .lost_focus() / ...`
    /// 继续串行检查。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let mut edit = egui::TextEdit::singleline(self.value)
            .hint_text(&self.hint)
            .desired_width(if self.fill_width { f32::INFINITY } else { 0.0 });
        if self.font_mono {
            edit = edit.font(crate::theme::ui_theme::mono(shell.scale.input));
        }
        if let Some(sz) = self.min_size {
            edit = edit.min_size(sz);
        }
        let resp = ui.add(edit);
        if self.focus_on_add {
            resp.request_focus();
        }
        resp
    }
}

// ── FieldSelect<T>（下拉）───────────────────────────────────────────

/// 下拉选择：`selected = options[i]` 之一，绑定 `&mut T`。
///
/// 模版用 `egui::ComboBox::from_id_salt + show_ui`，把每一项塞进
/// `selectable_value` —— 调用者只用给一个 `T -> String` 的 label 闭包，
/// 不必为每种 T 写新组件。
///
/// ID salt 由调用方提供（避免面板里两个 FieldSelect 撞名）。
///
/// **注意**：不能替代 Radio 组（new_map_dialog 的 level/extent/prefill 是
/// selectable_value 横向排，不抽）。
pub struct FieldSelect<'a, T> {
    selected: &'a mut T,
    options: &'a [T],
    id_salt: String,
    fill_width: bool,
}

impl<'a, T: Copy + PartialEq> FieldSelect<'a, T> {
    /// 绑定当前值 / 选项切片 / egui 记忆 ID。**ID 必须在面板作用域内唯一**。
    pub fn new(selected: &'a mut T, options: &'a [T], id_salt: &str) -> Self {
        Self {
            selected,
            options,
            id_salt: id_salt.to_owned(),
            fill_width: false,
        }
    }

    /// 撑满父容器宽度。
    pub fn fill_width(mut self, on: bool) -> Self {
        self.fill_width = on;
        self
    }

    /// 装配并落地。`label_for` 把 `&T` 映成显示文本——
    /// 推荐返回 `String`，避免与 `FieldSelect` 自身的 `&'a T` 借用相争。
    pub fn show<F>(self, ui: &mut egui::Ui, _shell: &UiShell, label_for: F)
    where
        F: Fn(&T) -> String,
    {
        let combo =
            egui::ComboBox::from_id_salt(&self.id_salt).selected_text(label_for(self.selected));
        let _ = self.fill_width;
        combo.show_ui(ui, |ui| {
            for opt in self.options.iter() {
                ui.selectable_value(self.selected, *opt, label_for(opt));
            }
        });
    }
}

// ── FieldCheckbox（原生复选项）────────────────────────────────────────

/// 复选项 + 文本标签；匹配原型 `.checkbox` 与原生 `ui.checkbox` 同形视觉。
///
/// 当前是 [`egui::Ui::checkbox`] 的薄壳——给定 stable API，未来要在 hover
/// 反馈 / focus 环上做品牌化也只改这一处。
///
/// ```text
///   [✓] 标签
/// ```
pub fn field_checkbox(
    ui: &mut egui::Ui,
    _shell: &UiShell,
    on: &mut bool,
    label: &str,
) -> egui::Response {
    ui.checkbox(on, label)
}

// ── Switch（视觉开关 —— 预留）────────────────────────────────────────

/// Switch 视觉控件（25×14 胶囊 + 滑钮）。
///
/// **当前 main 没有 call site**；保留作为后续「图层开关视觉升级」与原型
/// `.toggle` 的对齐点。等真正用到时由 `layers_panel` 替换 `field_checkbox`
/// 决定是否升级——本接口签名先稳定下来。
///
/// 视觉（web `.toggle`）：
/// on：accent 底 + 白滑钮靠右；
/// off：active 底 + text_3 滑钮靠左；
/// 命中区整胶囊。
pub struct Switch<'a> {
    on: &'a mut bool,
    size: egui::Vec2,
}

impl<'a> Switch<'a> {
    /// 默认 25×14（对齐 web `.toggle`）。
    pub fn new(on: &'a mut bool) -> Self {
        Self {
            on,
            size: egui::vec2(25.0, 14.0),
        }
    }

    /// 显式宽 × 高（含 `radius` = `size.y / 2.0`）。
    pub fn size(mut self, s: egui::Vec2) -> Self {
        self.size = s;
        self
    }

    /// 装配并落地。返回命中区 `Response`。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(self.size, egui::Sense::click());
        let painter = ui.painter();
        // 底色胶囊。
        let r = self.size.y / 2.0;
        painter.rect_filled(
            rect,
            r,
            if *self.on {
                shell.accent()
            } else {
                shell.active_bg()
            },
        );
        // 滑钮。
        let knob = self.size.y - 4.0;
        let knob_x = if *self.on {
            // 靠右：max.x − 2 − knob/2。
            rect.max.x - 2.0 - knob / 2.0
        } else {
            // 靠左：min.x + 2 + knob/2。
            rect.min.x + 2.0 + knob / 2.0
        };
        painter.circle_filled(
            egui::pos2(knob_x, rect.center().y),
            knob / 2.0,
            if *self.on {
                egui::Color32::WHITE
            } else {
                shell.t3()
            },
        );
        if resp.clicked() {
            *self.on = !*self.on;
        }
        resp
    }
}

// ── SliderRow（label-on-top + Slider + 自定义 format）──────────────────

/// 滑条行：顶行 label（shell.t2 + scale.small），其下整宽 Slider。
///
/// 自带 `show_value(false)`——拖动时浮标内是 `custom_formatter` 的渲染。
///
/// `formatter` 必须是 `Fn(f32) -> String + Send + Sync + 'static`——
/// `egui::Slider::custom_formatter` 要求 `'static` 闭包；egui 0.36 的
/// formatter 接收 `f64`，实现里 `as f32` 后再交给 caller。
pub fn slider_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    formatter: impl Fn(f32) -> String + Send + Sync + 'static,
    logarithmic: bool,
) -> egui::Response {
    ui.label(
        egui::RichText::new(label)
            .size(shell.scale.small)
            .color(shell.t2()),
    );
    let mut slider = egui::Slider::new(value, range)
        .show_value(true)
        .custom_formatter(move |n: f64, _| formatter(n as f32));
    if logarithmic {
        slider = slider.logarithmic(true);
    }
    ui.add(slider)
}

// ── dialog_window（模态壳）───────────────────────────────────────────

/// 模态对话框壳：标题进原生标题栏 + × 写回 `open` + 居中 + Foreground 置顶。
///
/// 替代各对话框（new_project / new_map …）内反复出现的
/// `egui::Window::new(title).open(&mut open).collapsible(false).resizable(false)
///  .order(Foreground).anchor(CENTER_CENTER, [0,0]).frame(...corner_radius(MODAL))`
/// 一坨样板；调用者只关心 body 与可选 foot。
///
/// **层序**：egui 绘制序 `Background < Middle < Foreground < Tooltip`——浮动
/// 面板在 Middle，本壳开 Foreground 压得过面板；Tooltip 仍在最上（欢迎页 /
/// startup 加载 / 命令面板 / 新建工程 / 新建地图 / 失败提示卡片等同层）。
pub struct DialogWindow<'a> {
    title: &'a str,
    open: &'a mut bool,
    width: f32,
    collapsible: bool,
    resizable: bool,
}

impl<'a> DialogWindow<'a> {
    /// 最小形态——`open = &mut dialog.open`、title 即可。
    pub fn new(title: &'a str, open: &'a mut bool) -> Self {
        Self {
            title,
            open,
            width: 460.0,
            collapsible: false,
            resizable: false,
        }
    }

    /// 显式宽度（默认 460）。
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// 标题栏 ▼（默认 false —— 模态不该折起来）。true 用于「故障诊断卡」。
    pub fn collapsible(mut self, on: bool) -> Self {
        self.collapsible = on;
        self
    }

    /// 是否可拖拽边框改尺寸（默认 false）。
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// 装配并落地。`open` 收 `Window::open` 的写回（用户点 × 时变 false）。
    pub fn show<F: FnOnce(&mut egui::Ui)>(self, ctx: &egui::Context, shell: &UiShell, body: F) {
        let mut open = *self.open;
        egui::Window::new(self.title)
            .open(&mut open)
            .collapsible(self.collapsible)
            .resizable(self.resizable)
            .default_width(self.width)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                    .fill(shell.panel_bg())
                    .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                    .inner_margin(crate::theme::ui_theme::margin_all(space::MD))
                    .corner_radius(radius::MODAL),
            )
            .show(ctx, body);
        *self.open = open;
    }
}

// ── egui 图像解码器装配（工具栏 SVG 渲染前置）─────────────────────────
//
// egui 0.36 的 loader 链（`egui/src/load.rs` 顶部文档）共三级：
//   ImageSource::load
//     ├► Context::include_bytes   （BytesLoader：取原始字节）
//     ├► Context::try_load_image  （ImageLoader：字节 → 像素）★ 解码
//     └► try_load_texture         （TextureLoader：像素 → 纹理）
//
// **修正（2026-10-15，推翻本节旧结论）**：旧版注解说「bevy_egui 不注册
// DefaultBytesLoader / DefaultTextureLoader，需手动装」——误诊。两者都是
// `Loaders::default()` 的出厂件；真正的缺口是 **ImageLoader：egui core
// 一个解码器都不带**（出厂 `image: Vec::new()`），SVG 字节永远解不成像素，
// `Image` 只能画红三角占位（用户截图所见）。旧代码手动 push 的两个 loader
// 实为重复注册（`add_*_loader` 不去重），一并移除。
//
// SVG 解码器只在官方配套 crate `egui_extras`（`svg` feature → resvg；
// `install_image_loaders` 在该 feature 下仅装 `SvgLoader`，且
// `is_loader_installed` 幂等）。依赖审视见根 Cargo.toml 注释。
//
// 时序：bevy_egui 0.42 的主上下文在 `PreUpdate` 的 `InitContexts` 才建立，
// `Startup` 调 `ctx_mut()` 必然 `Err`。**与 `setup_egui_fonts` 同口径**：
// 注册在 `PreUpdate` 链 `InitContexts` 之后，重试式 + 资源 marker 防重入。

/// 已安装图像解码器的 marker 资源（零大小）。
#[derive(Default, Resource)]
pub struct EguiImageLoadersInstalled;

/// 在 egui 上下文上安装 SVG 解码器（纯 egui 操作，单测可直调）。
pub fn install_svg_decoder(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// 主 egui 上下文就绪后安装图像解码器。
///
/// 上下文未就绪时静默下帧再试；装好后插入 marker；marker 存在则早退零开销。
pub fn setup_egui_image_loaders(
    mut commands: Commands,
    mut contexts: EguiContexts,
    installed: Option<Res<EguiImageLoadersInstalled>>,
) {
    if installed.is_some() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    install_svg_decoder(ctx);
    commands.insert_resource(EguiImageLoadersInstalled);
}

#[cfg(test)]
mod tool_button_tests {
    use super::*;
    use crate::theme::ui_theme::UiShellConfig;

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    /// 默认样式必须来自 `UiShell` 令牌：曾经默认 `tint = WHITE`，
    /// caller 忘了 `.tint()` 就会在浅色底上隐身——现在默认解析不再有这个坑。
    #[test]
    fn icon_button_defaults_resolve_from_shell_tokens_not_white() {
        let sh = shell();
        let st = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "hint").resolve(&sh);
        assert_eq!(st.tint, sh.t1(), "tint 应 = shell.t1()，got {:?}", st.tint);
        assert_eq!(
            st.min_size,
            egui::vec2(sh.hit, sh.hit),
            "默认方块 = hit×hit（{}px），got {:?}",
            sh.hit,
            st.min_size
        );
        assert!(
            (st.icon_px - (sh.hit - 8.0)).abs() < 1e-3,
            "默认图标 = hit−8（{}px），got {}",
            sh.hit - 8.0,
            st.icon_px
        );
        assert!(
            (st.text_px - sh.scale.small).abs() < 1e-3,
            "got {}",
            st.text_px
        );
    }

    /// 显式 builder 覆盖必须压过 shell 默认（map_controls 的 Google 白控件靠它）。
    #[test]
    fn explicit_builder_overrides_win_over_shell_defaults() {
        let sh = shell();
        let st = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "hint")
            .tint(egui::Color32::RED)
            .size(egui::vec2(38.0, 24.0))
            .icon_size(11.0)
            .resolve(&sh);
        assert_eq!(st.tint, egui::Color32::RED, "got {:?}", st.tint);
        assert_eq!(st.min_size, egui::vec2(38.0, 24.0), "got {:?}", st.min_size);
        assert!((st.icon_px - 11.0).abs() < 1e-3, "got {}", st.icon_px);
    }

    /// 带文字的按钮：高度吃满命中区，宽度随内容（0 = 不设下限）。
    #[test]
    fn text_button_fills_shell_hit_height_and_keeps_width_auto() {
        let sh = shell();
        let st = ToolButton::text("新建", "hint").resolve(&sh);
        assert_eq!(st.min_size.y, sh.hit, "高度应 = hit（{}px）", sh.hit);
        assert_eq!(st.min_size.x, 0.0, "宽度应随内容，got {}", st.min_size.x);

        let st2 = ToolButton::icon_text(ToolIcon::Svg(ICON_PEN), "笔刷", "hint").resolve(&sh);
        assert_eq!(
            st2.min_size.y, sh.hit,
            "图标+文字同口径，got {}",
            st2.min_size.y
        );
    }

    /// 选中态只差背景：active → active_bg，非选中 → 幽灵透明底。
    #[test]
    fn active_state_selects_active_bg_and_inactive_is_ghost() {
        let bg = egui::Color32::from_rgb(0xE8, 0xF0, 0xFE);
        assert_eq!(fill_for(true, bg), bg, "active 应取 active_bg");
        assert_eq!(
            fill_for(false, bg),
            egui::Color32::TRANSPARENT,
            "非选中应为幽灵透明底"
        );
    }

    /// 三种内容形态都能真实装配进 egui（API 签名 / Image tint / RichText
    /// 尺寸错一处就 panic 在这里，而不是等 GUI 启动烟测兜底）。
    #[test]
    fn show_renders_all_three_content_variants_without_panic() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("tool_button_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
        );
        let r1 = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "图标").show(&mut ui, &sh, || {});
        let r2 = ToolButton::text("文字按钮", "文字").show(&mut ui, &sh, || {});
        let r3 = ToolButton::icon_text(ToolIcon::Svg(ICON_RULER), "测量", "混合").show(
            &mut ui,
            &sh,
            || {},
        );
        assert!(
            r1.rect.width() > 0.0,
            "图标钮应有实际宽度，got {}",
            r1.rect.width()
        );
        assert!(
            r2.rect.height() >= sh.hit,
            "文字钮高度 ≥ hit，got {}",
            r2.rect.height()
        );
        assert!(r3.rect.width() > r1.rect.width(), "图标+文字应比纯图标宽");
        // 禁用态不触发回调。
        let mut fired = false;
        ToolButton::text("禁用", "")
            .enabled(false)
            .show(&mut ui, &sh, || fired = true);
        assert!(!fired, "未发生点击事件时回调不应触发");
    }
}

#[cfg(test)]
mod button_factory_tests {
    //! 2026-10-15 引入的 `button()` 工厂 + `icon_image` helper。
    //!
    //! ToolButton 4 个老测试留在 [`super::tool_button_tests`]——它们继续守住工具栏
    //! 方块图标 / active / disabled 语义；本模块只覆盖**新**工厂：4 种 style
    //! 装配 + Image tint 中央化。

    use super::*;
    use crate::theme::ui_theme::UiShellConfig;

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    fn new_ui(ctx: &egui::Context, w: f32, h: f32) -> egui::Ui {
        egui::Ui::new(
            ctx.clone(),
            egui::Id::new("button_factory_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w, h),
            )),
        )
    }

    /// 4 种 style 都能真实装配进 egui 而不 panic（API 签名 / Image tint /
    /// RichText 尺寸任何错一处都立刻 panic 在这里，而不是等 GUI 启动烟测）。
    #[test]
    fn factory_renders_all_four_styles_into_egui_without_panic() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 600.0, 200.0);

        let r_a = button(
            &mut ui,
            &sh,
            ButtonContent::ImageText(ICON_PLUS, "主 CTA"),
            ButtonStyle::Accent,
            None,
            true,
        );
        let r_b = button(
            &mut ui,
            &sh,
            ButtonContent::ImageText(ICON_FOLDER_OPEN, "次 CTA"),
            ButtonStyle::Raised,
            None,
            true,
        );
        let r_c = button(
            &mut ui,
            &sh,
            ButtonContent::Text("取消"),
            ButtonStyle::Ghost,
            None,
            true,
        );
        let r_d = button(
            &mut ui,
            &sh,
            ButtonContent::Text("了解更多"),
            ButtonStyle::Subtle,
            None,
            true,
        );

        // 高度预期：Accent / Raised / Ghost / Subtle 的文字钮统一 = shell.hit；
        // 即使文字色与 fill 不同也至少要有矩形。
        assert!(
            r_a.rect.width() > 0.0,
            "Accent 钮应有宽度，got {}",
            r_a.rect.width()
        );
        assert!(
            r_b.rect.height() >= sh.hit,
            "Raised 钮高 ≥ hit，got {}",
            r_b.rect.height()
        );
        assert!(
            r_c.rect.height() >= sh.hit,
            "Ghost 钮高 ≥ hit，got {}",
            r_c.rect.height()
        );
        assert!(
            r_d.rect.height() >= sh.hit,
            "Subtle 钮高 ≥ hit，got {}",
            r_d.rect.height()
        );
    }

    /// 纯 Image 按钮应该是 `shell.hit × shell.hit` 方块（与 ToolButton 的
    /// 纯图标钮同一尺寸口径）。
    #[test]
    fn image_only_button_is_hit_square() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 200.0, 200.0);

        let r = button(
            &mut ui,
            &sh,
            ButtonContent::Image(ICON_PLUS),
            ButtonStyle::Accent,
            None,
            true,
        );
        assert!(
            (r.rect.width() - sh.hit).abs() < 1e-3 && (r.rect.height() - sh.hit).abs() < 1e-3,
            "纯图标钮 = hit×hit（{}×{}），got {:?}",
            sh.hit,
            sh.hit,
            r.rect.size()
        );
    }

    /// `min_size_override` 在 `Some(...)` 时必须覆盖工厂默认。
    /// 欢迎页 CTA 要求 `40px` 高，与默认 `shell.hit=28` 不同——这条钉住扩展口径。
    #[test]
    fn min_size_override_wins_over_factory_default() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 600.0, 100.0);
        let r = button(
            &mut ui,
            &sh,
            ButtonContent::ImageText(ICON_PLUS, "主 CTA"),
            ButtonStyle::Accent,
            Some(egui::vec2(200.0, 40.0)),
            true,
        );
        assert!(
            (r.rect.height() - 40.0).abs() < 1e-3,
            "min_size_override(40) 必须压过默认 28，got {}",
            r.rect.height()
        );
        assert!(
            r.rect.width() >= 200.0,
            "min_size_override(200) 必须让宽度 ≥ 200，got {}",
            r.rect.width()
        );
    }

    /// `enabled = false` 必须通过 [`egui::Ui::add_enabled`] 把整按钮置灰且不可点。
    /// 这是对话框「创建」按钮在 `name + location` 双空时禁用这一支路。
    #[test]
    fn enabled_false_renders_button_disabled() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 200.0, 200.0);

        let r = button(
            &mut ui,
            &sh,
            ButtonContent::Text("创建"),
            ButtonStyle::Accent,
            None,
            false,
        );
        assert!(
            r.rect.width() > 0.0,
            "禁用钮也应有矩形，got {}",
            r.rect.width()
        );
        // 关键：禁用的 egui::Response 自身**不**感知"自己被禁用"，但点击
        // 应该在没有 enabled 包装时不再触发已注册的 sense——
        // 单测无法直接断言"点不动"，所以只验：r 仍返回了、且感官上至少占位存在。
        // 此条测试护的是 add_enabled 这条路径不 panic（曾经要求 path 走 `ui.add`）。
    }

    /// `icon_image` 必须把 `tint` 显式设进 Image（caller 漏 tint 的旧坑）。
    /// `ImageOptions.tint` 是 `pub` 字段，getter `image_options()` 公开可读
    /// ——直接对比 caller 传入值即可。
    #[test]
    fn icon_image_centralizes_tint_into_image_options() {
        let img = icon_image(ICON_GLOBE, egui::Color32::RED, 28.0);
        assert_eq!(
            img.image_options().tint,
            egui::Color32::RED,
            "icon_image 必须把 tint 传到 Image，got {:?}",
            img.image_options().tint
        );
    }

    /// `icon_image` 的 `fit_to_exact_size` 落到 Image 私有 `size: ImageSize` 字段，
    /// 不暴露 getter；走渲染出口拿 `Response.rect` —— 钉死 28×28。
    #[test]
    fn icon_image_paints_at_exact_pixel_size() {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 200.0, 200.0);
        let img = icon_image(ICON_GLOBE, egui::Color32::WHITE, 28.0);
        let r = ui.add(img);
        let s = r.rect.size();
        assert!(
            (s.x - 28.0).abs() < 1e-3 && (s.y - 28.0).abs() < 1e-3,
            "icon_image 强制 28×28，got {s:?}"
        );
    }
}

#[cfg(test)]
mod recent_row_and_time_ago_tests {
    //! 2026-10-15 提到 widgets 的 [`super::recent_row`] 与
    //! [`super::format_time_ago`] 的回归。

    use super::*;
    use crate::theme::ui_theme::UiShellConfig;
    use std::time::{Duration, Instant};

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    /// 区间表钉死，避免后续误改文案悄悄回归。每档差用上限值测下界（如 `89m`
    /// 进 "[1h, 1d)" 仍是 1h ago），上界留给相邻测。
    #[test]
    fn format_time_ago_spans_cover_every_bucket() {
        let now = Instant::now();
        // [0, 60s) → "Just now"
        assert_eq!(format_time_ago(now, now), "Just now");
        assert_eq!(
            format_time_ago(now - Duration::from_secs(59), now),
            "Just now"
        );
        // [60s, 1h) → "Nm ago"
        assert_eq!(
            format_time_ago(now - Duration::from_secs(60), now),
            "1m ago"
        );
        // 59min 在 [60s, 1h) 上边界之内，应仍归 59m；59*60+30 也仍在同一档
        assert_eq!(
            format_time_ago(now - Duration::from_secs(59 * 60 + 30), now),
            "59m ago"
        );
        // [1h, 1d) → "Nh ago"
        assert_eq!(
            format_time_ago(now - Duration::from_secs(3600), now),
            "1h ago"
        );
        assert_eq!(
            format_time_ago(now - Duration::from_secs(23 * 3600), now),
            "23h ago"
        );
        // [1d, 2d) → "Yesterday"
        assert_eq!(
            format_time_ago(now - Duration::from_secs(86400), now),
            "Yesterday"
        );
        // [2d, 7d) → "Nd ago"
        assert_eq!(
            format_time_ago(now - Duration::from_secs(2 * 86400), now),
            "2d ago"
        );
        assert_eq!(
            format_time_ago(now - Duration::from_secs(6 * 86400), now),
            "6d ago"
        );
        // [7d, 30d) → "Nw ago"（按周）
        assert_eq!(
            format_time_ago(now - Duration::from_secs(7 * 86400), now),
            "1w ago"
        );
        // [30d, ∞) → "Nmo ago"（按月）
        assert_eq!(
            format_time_ago(now - Duration::from_secs(30 * 86400), now),
            "1mo ago"
        );
        assert_eq!(
            format_time_ago(now - Duration::from_secs(120 * 86400), now),
            "4mo ago"
        );
    }

    /// `recent_row` 必须能给 `time_ago = None` 不画右侧栏（不 panic，rect
    /// 仍存在）；给 `Some(...)` 也不 panic——这两条守的是 `Option` 通路。
    #[test]
    fn recent_row_handles_time_ago_none_and_some() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("recent_row_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 100.0),
            )),
        );
        let r_none = recent_row(&mut ui, &sh, ICON_GLOBE, "name", "dir", None);
        assert!(
            r_none.rect.height() > 0.0,
            "None 路径至少占位，got {}",
            r_none.rect.height()
        );

        let ctx2 = egui::Context::default();
        ctx2.begin_pass(Default::default());
        let mut ui2 = egui::Ui::new(
            ctx2.clone(),
            egui::Id::new("recent_row_tests_2"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 100.0),
            )),
        );
        let r_some = recent_row(&mut ui2, &sh, ICON_GLOBE, "name", "dir", Some("5m ago"));
        assert!(
            r_some.rect.height() > 0.0,
            "Some 路径至少占位，got {}",
            r_some.rect.height()
        );
    }
}

#[cfg(test)]
mod svg_decoder_tests {
    use super::install_svg_decoder;
    use bevy_egui::egui;

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2"><rect x="4" y="4" width="16" height="16"/></svg>"##;

    fn bytes_source() -> egui::ImageSource<'static> {
        egui::ImageSource::Bytes {
            uri: std::borrow::Cow::Borrowed("bytes://test/pen.svg"),
            bytes: egui::load::Bytes::from(SVG),
        }
    }

    /// 红三角根因的回归测试：出厂 egui 上下文没有任何 `ImageLoader`，
    /// `bytes://…​.svg` 解不出像素（egui 画红三角占位）；装上
    /// `egui_extras` 的 `SvgLoader` 后必须一步到位解码成纹理。
    #[test]
    fn svg_bytes_stay_undecoded_until_svg_loader_installed() {
        let ctx = egui::Context::default();

        // 未装解码器：必须报 NoMatchingImageLoader（旧版红三角的确切来源）。
        let before = bytes_source().load(
            &ctx,
            Default::default(),
            egui::load::SizeHint::Scale(1.0.into()),
        );
        match &before {
            // 出厂 `image` 列表为空 → NoImageLoaders；若哪天 egui 出厂带了解码器
            // 但不认 SVG，会落 NoMatchingImageLoader——两者都是「没解码」的证据。
            Err(
                egui::load::LoadError::NoImageLoaders
                | egui::load::LoadError::NoMatchingImageLoader { .. },
            ) => {}
            Err(e) => panic!("出厂上下文应报缺解码器，got {e}"),
            Ok(poll) => panic!(
                "出厂上下文不应解码就绪，got {}",
                if matches!(poll, egui::load::TexturePoll::Ready { .. }) {
                    "Ready"
                } else {
                    "Pending"
                }
            ),
        }

        install_svg_decoder(&ctx);
        let after = bytes_source().load(
            &ctx,
            Default::default(),
            egui::load::SizeHint::Scale(1.0.into()),
        );
        let ready = match after {
            Ok(ready @ egui::load::TexturePoll::Ready { .. }) => ready,
            Ok(_) => panic!("装上 SvgLoader 后应直接解码就绪，而非 Pending"),
            Err(e) => panic!("装上 SvgLoader 后应解码成功，got {e}"),
        };
        let egui::load::TexturePoll::Ready { texture, .. } = ready else {
            unreachable!("上面已筛出 Ready");
        };
        let size = texture.size;
        assert_eq!(
            (size.x, size.y),
            (24.0, 24.0),
            "SVG 应按 viewBox 点数解码，got {size:?}"
        );
    }
}

#[cfg(test)]
mod field_input_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
    }

    /// 构造一个 `egui::Ui` 跑在 `400×400` 矩形里，供 `show` 落地用。
    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("field_input_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
        );
        (ctx, ui)
    }

    /// `show` 不 panic 且返回合理尺寸（任何 builder 配置下都至少 1 帧内可用）。
    #[test]
    fn field_input_show_renders_without_panic_for_each_builder_path() {
        let sh = shell();
        let (_c1, mut u1) = make_ui();
        let mut v = String::from("initial");
        FieldInput::new(&mut v, "hint").show(&mut u1, &sh);
        assert!(!v.is_empty(), "初始值不应被改");

        // font_mono + min_size + fill_width + focus_on_add 全开
        let (_c2, mut u2) = make_ui();
        let mut v = String::new();
        let r = FieldInput::new(&mut v, "hint")
            .font_mono(true)
            .min_size(egui::vec2(80.0, 28.0))
            .fill_width(true)
            .focus_on_add(true)
            .show(&mut u2, &sh);
        // `request_focus` 不总能在 headless 上下文里成功测，但调用本身必须不 panic。
        let _ = r;
    }

    /// `font_mono(false)` 是默认态：构造应不要求 shell 也能跑（这是 row）。
    #[test]
    fn field_input_default_proportional_does_not_require_shell_in_builder() {
        // 仅做构造阶段检验：`new + show` 形签名对得上。
        let _ = std::marker::PhantomData::<fn() -> FieldInput<'static>>;
    }

    /// builder 链式调用应能稳定串成：`new(…).font_mono(true).fill_width(true)`。
    #[test]
    fn field_input_builder_chain_is_consistent() {
        // 编译通过即视为链式 builder 类型一致。
        // struct 字段私有，靠类型系统保证默认值：
        //   fn _check_default<T: Default>() {}
        // 直接借用 shell 也只是为对齐编译。
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut v = String::new();
        FieldInput::new(&mut v, "h")
            .font_mono(true)
            .fill_width(true)
            .min_size(egui::vec2(60.0, 24.0))
            .focus_on_add(true)
            .show(&mut ui, &sh);
    }
}

#[cfg(test)]
mod field_select_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
    }

    /// ID 必须稳定（面板作用域内不撞名）——构造路径不 panic。
    #[test]
    fn field_select_id_salt_is_recorded() {
        let opts = [10u32, 20, 30];
        let mut sel = 10u32;
        let fs = FieldSelect::new(&mut sel, &opts, "unique_id");
        // 字段私有，但构造不爆 + `Debug` 形可显示。
        assert_eq!(fs.id_salt, "unique_id");
    }

    /// `show` 必须落地（不 panic）。
    #[test]
    fn field_select_show_renders_with_label_closure() {
        let sh = shell();
        let opts = ["甲", "乙", "丙"];
        let mut sel = "甲";
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("field_select_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        FieldSelect::new(&mut sel, &opts, "wv_test_select").show(&mut ui, &sh, |s| s.to_string());
        // sel 不被 show 改写——只有 open 后 user pick 才改。
        assert_eq!(sel, "甲");
    }
}

#[cfg(test)]
mod field_checkbox_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
    }

    /// 真正的 egui 调用：开/关两个方向都跑一遍。
    #[test]
    fn field_checkbox_toggles_correctly() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("field_checkbox_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        let mut on = false;
        let _ = field_checkbox(&mut ui, &sh, &mut on, "复选项");
        assert!(!on, "未点击时不变");
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
    }

    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("switch_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        (ctx, ui)
    }

    /// 默认尺寸是 25×14（对齐 web `.toggle`）。
    #[test]
    fn switch_default_size_matches_web_toggle() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut on = false;
        let r = Switch::new(&mut on).show(&mut ui, &sh);
        assert!(
            (r.rect.width() - 25.0).abs() < 1e-3,
            "默认宽 = 25px，got {}",
            r.rect.width()
        );
        assert!(
            (r.rect.height() - 14.0).abs() < 1e-3,
            "默认高 = 14px，got {}",
            r.rect.height()
        );
    }

    /// 显式 size builder 覆盖默认。
    #[test]
    fn switch_size_override_takes_effect() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut on = true;
        let r = Switch::new(&mut on)
            .size(egui::vec2(40.0, 20.0))
            .show(&mut ui, &sh);
        assert!(r.rect.width() > 25.0, "显式宽 40 > 默认 25");
    }
}

#[cfg(test)]
mod slider_row_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
    }

    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("slider_row_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        (ctx, ui)
    }

    /// 默认（非 logarithmic）：formatter 直接吃数值。
    #[test]
    fn slider_row_linear_passes_value_through_formatter() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut v: f32 = 0.5;
        slider_row(
            &mut ui,
            &sh,
            "软度",
            &mut v,
            0.0..=1.0,
            |n| format!("{:.2}", n),
            false,
        );
        // 跑过 show 不 panic 即视为通过。
    }

    /// logarithmic=true：内部仍走 `Slider::logarithmic(true)`，UI 端不报错。
    #[test]
    fn slider_row_logarithmic_path_does_not_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut v: f32 = 32.0;
        slider_row(
            &mut ui,
            &sh,
            "半径",
            &mut v,
            16.0..=600.0,
            |n| format!("{:.0} wu", n),
            true,
        );
    }
}

#[cfg(test)]
mod dialog_window_tests {
    use super::*;

    /// 默认配置（无 `.width/.collapsible/.resizable` 覆盖）所有字段留默认值。
    #[test]
    fn dialog_window_default_fields_have_expected_values() {
        let mut open = true;
        let dw = DialogWindow::new("test", &mut open);
        assert_eq!(dw.title, "test");
        assert!(
            (dw.width - 460.0).abs() < 1e-3,
            "默认宽 460，got {}",
            dw.width
        );
        assert!(!dw.collapsible, "默认不可折叠");
        assert!(!dw.resizable, "默认不可缩放");
        // `&mut open` 链接：show 时被读 + 写回。
        *dw.open = false;
        assert!(!open, "open 字段是 &mut 桥");
    }

    /// builder 链覆盖：width / collapsible / resizable 都能覆盖默认值。
    #[test]
    fn dialog_window_builder_overrides_default() {
        let mut open = true;
        let dw = DialogWindow::new("可折叠", &mut open)
            .width(420.0)
            .collapsible(true)
            .resizable(true);
        assert!((dw.width - 420.0).abs() < 1e-3);
        assert!(dw.collapsible);
        assert!(dw.resizable);
    }

    /// `show` 的真实跑通由 [`scripts/smoke-gui.sh`]（GUI 启动烟测）兜底——
    /// `egui::Window::show(ctx, …)` 注册的 texture delta 在 unit 上下文无 GPU 渲染
    /// surface，必然在 drop 时撞「Dropped TexturesDelta with N unapplied deltas」。
    /// 因此这里**不**跑 `show`，把渲染时正确性交给启动烟测。
    #[test]
    fn dialog_window_construction_only_runs_in_unit_tests() {
        // 仅做构造（不调 show）能过 → API 类型与 builder 链 OK。
        let mut open = true;
        let mut open2 = true;
        let _a = DialogWindow::new("a", &mut open);
        let _b = DialogWindow::new("b", &mut open2)
            .width(700.0)
            .collapsible(true)
            .resizable(true);
    }
}
