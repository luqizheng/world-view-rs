//! 视觉原子 —— 徽标 / 空状态 / 列表项 / 描边 / 图标 tint 中央化 / 最近工程 /
//! 工具图标常量。（`terrain_grid` 域件留 wv-app，ADR-0026 D3。）
//!
//! 单一职责：**没有状态、没有 egui Window 外壳**，只画 widget 自身。
//! 浮层（panel 外壳）、按钮（多 style 工厂）、表单（input/select/...）、
//! SVG 解码器都不在本模块。
//!
//! 历史来源：拆自原 [`crate::widgets`] 顶部块。

use bevy_egui::egui;

use crate::ui_theme::{color32, mono, proportional};
use crate::ui_tokens::{font_size, radius, space, BadgeTone};

use crate::shell::UiShell;

// ── 徽标 ─────────────────────────────────────────────────────────────────

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
    let bg = crate::ui_theme::color32_alpha(colors.fill, colors.fill_alpha);
    let pad = egui::vec2(space::XS * 1.5, 1.0);
    let font = proportional(font_size::MICRO);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, radius::BADGE, bg);
    // egui 0.30 的 `Painter::galley` 需要一个兜底色（字体未命中字形时使用）。
    ui.painter().galley(rect.min + pad, galley, fg);
}

// ── 空状态 / 列表项 ──────────────────────────────────────────────────────

/// 空状态卡（§5.2）：图标 + 一句人话 + 一个可点的下一步。
///
/// CTA 走 [`super::button::ToolButton::text`]（与 Toolbar 同一抽象），最低宽 120 锁定为
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
        let resp = super::button::ToolButton::text(action, "")
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

// ── 身份块 / 分隔线（检视器组件族 · Figma `42:2`）──────────────────────

/// 对象身份块（Figma `42:11` Selected place）：36×36 语义色图标瓦片 + 标题 +
/// 副标题 + 可选右端挂件（如金色图钉）。
///
/// `tile_fill` 由 caller 给**类别语义色**，瓦片上的图标固定白 tint——底色是
/// caller 的语义，白图标在其上恒可读（`ButtonStyle::Accent` 白字同规）。
/// 挂件 tint 固定 [`UiShell::gold`]（Figma `42:18` pin 采样 `#D6B77B` →
/// `gold` 令牌对位）。返回整行 click `Response`，行点击 / 挂件点击的分流
/// 由 caller 自接（挂件几何在行内右缘，行 `Response` 优先）。
pub fn identity_header(
    ui: &mut egui::Ui,
    shell: &UiShell,
    tile_fill: egui::Color32,
    icon: egui::ImageSource<'static>,
    title: &str,
    subtitle: &str,
    trailing: Option<egui::ImageSource<'static>>,
) -> egui::Response {
    const TILE: f32 = 36.0;
    let avail_w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail_w, TILE), egui::Sense::click());
    let painter = ui.painter();
    let tile = egui::Rect::from_min_size(rect.min, egui::vec2(TILE, TILE));
    painter.rect_filled(tile, radius::CONTROL, tile_fill);
    // 标题 + 副标题两行（Figma 行距 2px → 上下各偏 8 居中）。
    let text_x = rect.min.x + TILE + 10.0;
    painter.text(
        egui::pos2(text_x, rect.center().y - 8.0),
        egui::Align2::LEFT_CENTER,
        title,
        proportional(shell.scale.base),
        shell.t1(),
    );
    painter.text(
        egui::pos2(text_x, rect.center().y + 8.0),
        egui::Align2::LEFT_CENTER,
        subtitle,
        proportional(shell.scale.micro),
        shell.t3(),
    );
    let _ = painter; // 释放借用，`ui.put` 落图标。
                     // `ui.put` 左上锚定——20px 图标须落 36px 瓦片的**居中**矩形。
    let icon_rect = egui::Rect::from_center_size(tile.center(), egui::vec2(20.0, 20.0));
    let _ = ui.put(icon_rect, icon_image(icon, egui::Color32::WHITE, 20.0));
    if let Some(src) = trailing {
        let trail = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - 6.0, rect.center().y),
            egui::vec2(12.0, 12.0),
        );
        let _ = ui.put(trail, icon_image(src, shell.gold(), 12.0));
    }
    resp
}

/// 分隔线（Figma `42:23` Divider）：整宽 1px `border_subtle`。
pub fn divider(ui: &mut egui::Ui, shell: &UiShell) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, shell.border_subtle());
}

// ── 描边 + 图标 helper ──────────────────────────────────────────────────

/// 描边矩形（统一 `StrokeKind::Inside`）。
///
/// egui 0.36 给 `Painter::rect_stroke` 加了第 4 个参数 `StrokeKind`（描边画在
/// 边界内 / 中 / 外）。本工程的矩形描边都是「选中框 / 视口框」，必须落在矩形**内部**
/// 才不会被邻居裁切吞掉，故收敛成一个实现点。
pub fn rect_stroke(
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

// ── 最近工程行 + 相对时间格式化 ─────────────────────────────────────────

// ── 最近工程行（Figma `RecentItem` · 2026-10-15）────────────────────────
//
// 提到 widgets 的目的是消除欢迎页 / 命令面板 / 项目菜单里重复写的 list-row
// 代码（之前三处都用 `ui.allocate_exact_size + painter.text + Image::new`
// 三件套手工写）。**`time_ago` 是 Figma 原型上的一栏**——之前欢迎页预留了接口
// 但形参未到，这次跟 `crate::state::RecentProject::last_opened_at`（wv-app）
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

// ── 工具图标常量 ─────────────────────────────────────────────────────────
//
// `include_image!` 内部走 `include_bytes!`，路径**相对于调用它的源文件**，
// 不是 `CARGO_MANIFEST_DIR`。本模块位于 crates/wv-app/src/ui/widgets/，
// 回到 crates/wv-app/ 需 3 层 `../../../`。原 widgets.rs 在
// crates/wv-app/src/ui/，离根只需 2 层，故那里写的是 `../../`。

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
pub const ICON_GLOBE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/globe.svg");
pub const ICON_PLUS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/plus.svg");
pub const ICON_FOLDER_OPEN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/folder-open.svg");
/// 森林意图图标（Lucide `trees`，theme overlay v1 / 选择工具 M4）。
pub const ICON_TREES: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/trees.svg");
/// 笔刷意图工具图标（地形设计器三行行内瓷砖）。形态两两互异是硬要求——
/// 瓷砖纯图标，识别全靠形：尖峰(山脉)/平顶(高原)/圆丘(丘陵)/开口锥+射线(火山)/
/// 水滴(湖)/波线(河)。
pub const ICON_MOUNTAIN_RANGE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/mountain-range.svg");
pub const ICON_PLATEAU: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/plateau.svg");
pub const ICON_HILLS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/hills.svg");
pub const ICON_VOLCANO: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/volcano.svg");
pub const ICON_LAKE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/lake.svg");
pub const ICON_RIVER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/river.svg");

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod recent_row_and_time_ago_tests {
    //! 2026-10-15 提到 widgets 的 [`super::recent_row`] 与
    //! [`super::format_time_ago`] 的回归。

    use super::*;
    use crate::ui_theme::UiShellConfig;
    use std::time::{Duration, Instant};

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

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
mod identity_header_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::ui_theme::UiShellConfig::default())
    }

    fn make_ui(name: &str) -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new(name),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(288.0, 100.0),
            )),
        );
        (ctx, ui)
    }

    #[test]
    fn identity_header_is_figma_36_tile_high_and_full_width() {
        let sh = shell();
        let (_c, mut ui) = make_ui("identity_header_geom");
        let r = identity_header(
            &mut ui,
            &sh,
            egui::Color32::from_rgb(0x4B, 0x42, 0x2E),
            ICON_GLOBE,
            "Government Office",
            "Point of interest · Civic",
            Some(ICON_MAP_PIN),
        );
        assert!(
            (r.rect.height() - 36.0).abs() < 1e-3,
            "瓦片行高 = Figma 采样 36，got {}",
            r.rect.height()
        );
        assert!(
            (r.rect.width() - 288.0).abs() < 1e-3,
            "整行可点（288），got {}",
            r.rect.width()
        );
    }

    #[test]
    fn identity_header_without_trailing_renders_without_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui("identity_header_no_trailing");
        let r = identity_header(
            &mut ui,
            &sh,
            egui::Color32::GRAY,
            ICON_GLOBE,
            "t",
            "s",
            None,
        );
        assert!(r.rect.width() > 0.0);
    }

    #[test]
    fn divider_is_one_pixel_and_full_width() {
        let sh = shell();
        let (_c, mut ui) = make_ui("divider_geom");
        divider(&mut ui, &sh);
        let last = ui.min_rect();
        assert!(
            (last.height() - 1.0).abs() < 1e-3,
            "分隔线高 = 1px，got {}",
            last.height()
        );
        assert!(
            (last.width() - 288.0).abs() < 1e-3,
            "整宽（288），got {}",
            last.width()
        );
    }
}
