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
    // 目录 StatusPill：h20 / 左右 6 内边距 / radius 3 / 9px 字；
    // 底色走语义软底，前景走语义色（不再用 alpha 叠色）。
    let (bg, fg) = match kind {
        BadgeKind::Neutral => (shell.raised_bg(), shell.t2()),
        BadgeKind::Ok => (shell.success_soft(), shell.success()),
        BadgeKind::Warn => (shell.gold_soft(), shell.gold()),
        BadgeKind::Err => (shell.danger_soft(), shell.danger()),
        BadgeKind::Event => (shell.active_bg(), color32(shell.palette.badge_event)),
    };
    let font = proportional(font_size::MICRO - 2.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = galley.size() + egui::vec2(12.0, 4.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, bg);
    p.galley(rect.min + egui::vec2(6.0, 2.0), galley, fg);
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
    let (rect, resp) = row_rect(ui, 28.0, egui::Sense::click());
    let painter = ui.painter();
    if selected {
        paint_selected_row(painter, rect, shell, radius::FIELD, true);
    } else if resp.hovered() {
        painter.rect_filled(rect, radius::FIELD, shell.hover_bg());
    }
    let sw = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + space::SM + 4.0, rect.center().y),
        egui::Vec2::splat(8.0),
    );
    painter.rect_filled(sw, radius::BADGE, swatch);
    painter.text(
        egui::pos2(sw.max.x + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        proportional(font_size::SMALL),
        if selected { shell.accent() } else { shell.t1() },
    );
    painter.text(
        egui::pos2(rect.max.x - space::SM, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        meta,
        mono(font_size::MICRO - 2.0),
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

// ── 目录新增：Keycap / InlineAlert / MenuItem ─────────────────────────

/// 键帽（目录 `UI/Keycap`）：h20 / 左右 6 / radius 4 / raised 底 +
/// border_subtle 描边 / mono 9 字。
pub fn keycap(ui: &mut egui::Ui, shell: &UiShell, label: &str) {
    let font = mono(font_size::MICRO - 2.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, shell.t2());
    let size = galley.size() + egui::vec2(12.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, radius::FIELD, shell.raised_bg());
    p.rect_stroke(
        rect,
        radius::FIELD,
        egui::Stroke::new(1.0, shell.border_subtle()),
        egui::StrokeKind::Inside,
    );
    p.galley(rect.min + egui::vec2(6.0, 3.0), galley, shell.t2());
}

/// 内联提示种类（目录 `UI/InlineAlert`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// 成功：success_soft 底 + success 描边/字。
    Success,
    /// 错误：danger_soft 底 + danger 描边/字。
    Error,
}

/// 内联提示条：软底 + 同色描边 + 12px 图标 + 11px 正文，padding 8，radius 4。
pub fn inline_alert(ui: &mut egui::Ui, shell: &UiShell, kind: AlertKind, text: &str) {
    let (bg, fg, icon) = match kind {
        AlertKind::Success => (shell.success_soft(), shell.success(), ICON_CHECK),
        AlertKind::Error => (shell.danger_soft(), shell.danger(), ICON_ALERT),
    };
    egui::Frame::default()
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, fg))
        .corner_radius(radius::FIELD)
        .inner_margin(crate::ui_theme::margin_all(8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                ui.add(icon_image(icon, fg, 12.0));
                ui.label(egui::RichText::new(text).size(font_size::MICRO).color(fg));
            });
        });
}

/// 菜单项状态（目录 `UI/MenuItem` 四变体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuState {
    /// 默认：文字 t1，hover 走 hover_bg。
    Default,
    /// 选中：accent_soft 底 + accent 字。
    Selected,
    /// 带勾：右侧 check 图标。
    Checked,
    /// 禁用：text_disabled。
    Disabled,
}

/// 菜单项（目录 `UI/MenuItem`）：h28 / 左右 8 / radius 4。
pub fn menu_item(
    ui: &mut egui::Ui,
    shell: &UiShell,
    icon: Option<egui::ImageSource<'static>>,
    label: &str,
    state: MenuState,
) -> egui::Response {
    let sense = if state == MenuState::Disabled {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let (rect, resp) = row_rect(ui, 28.0, sense);
    let p = ui.painter();
    if state == MenuState::Selected {
        paint_selected_row(p, rect, shell, radius::FIELD, false);
    } else if resp.hovered() && state != MenuState::Disabled {
        p.rect_filled(rect, radius::FIELD, shell.hover_bg());
    }
    let fg = match state {
        MenuState::Selected => shell.accent(),
        MenuState::Disabled => shell.text_disabled(),
        _ => shell.t1(),
    };
    let text_x = if icon.is_some() {
        rect.min.x + 28.0
    } else {
        rect.min.x + 8.0
    };
    p.text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(font_size::SMALL),
        fg,
    );
    let _ = p; // 释放 painter 借用，`ui.put` 落图标。
    if let Some(src) = icon {
        let _ = ui.put(
            egui::Rect::from_center_size(
                egui::pos2(rect.min.x + 14.0, rect.center().y),
                egui::vec2(12.0, 12.0),
            ),
            icon_image(src, fg, 12.0),
        );
    }
    if state == MenuState::Checked {
        let _ = ui.put(
            egui::Rect::from_center_size(
                egui::pos2(rect.max.x - 14.0, rect.center().y),
                egui::vec2(12.0, 12.0),
            ),
            icon_image(ICON_CHECK, shell.accent(), 12.0),
        );
    }
    resp
}

// ── 目录新增：TreeRow / LayerRow / Breadcrumb / Loading ───────────────

/// 树行（目录 `UI/TreeRow`）：h24 / 缩进 / 展开三角 + 名称；选中行
/// `accent_soft` 底 + 左侧 2px 蓝条 + accent 字。
pub fn tree_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    depth: usize,
    label: &str,
    expanded: bool,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = row_rect(ui, 24.0, egui::Sense::click());
    let p = ui.painter();
    if selected {
        paint_selected_row(p, rect, shell, radius::FIELD, true);
    } else if resp.hovered() {
        p.rect_filled(rect, radius::FIELD, shell.hover_bg());
    }
    let cx = rect.min.x + 12.0 + depth as f32 * 12.0;
    let cy = rect.center().y;
    let tri = if expanded {
        egui::Shape::convex_polygon(
            vec![
                egui::pos2(cx - 3.0, cy - 2.0),
                egui::pos2(cx + 3.0, cy - 2.0),
                egui::pos2(cx, cy + 2.0),
            ],
            shell.t3(),
            egui::Stroke::NONE,
        )
    } else {
        egui::Shape::convex_polygon(
            vec![
                egui::pos2(cx - 2.0, cy - 3.0),
                egui::pos2(cx - 2.0, cy + 3.0),
                egui::pos2(cx + 2.0, cy),
            ],
            shell.t3(),
            egui::Stroke::NONE,
        )
    };
    p.add(tri);
    p.text(
        egui::pos2(cx + 8.0, cy),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(font_size::SMALL),
        if selected { shell.accent() } else { shell.t1() },
    );
    resp
}

/// 图层行（目录 `UI/LayerRow`）：h24 / 名称 + 右端可见性（圆环）与锁定（方框）状态。
pub fn layer_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    visible: bool,
    locked: bool,
) -> egui::Response {
    let (rect, resp) = row_rect(ui, 24.0, egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, radius::FIELD, shell.hover_bg());
    }
    p.text(
        egui::pos2(rect.min.x + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(font_size::SMALL),
        if visible {
            shell.t1()
        } else {
            shell.text_disabled()
        },
    );
    p.circle_stroke(
        egui::pos2(rect.max.x - 30.0, rect.center().y),
        4.0,
        egui::Stroke::new(
            1.0,
            if visible {
                shell.t2()
            } else {
                shell.text_disabled()
            },
        ),
    );
    p.rect_stroke(
        egui::Rect::from_center_size(
            egui::pos2(rect.max.x - 14.0, rect.center().y),
            egui::vec2(8.0, 8.0),
        ),
        1.0,
        egui::Stroke::new(
            1.0,
            if locked {
                shell.gold()
            } else {
                shell.border_subtle()
            },
        ),
        egui::StrokeKind::Inside,
    );
    resp
}

/// 面包屑（目录 `UI/Breadcrumb`）：11px 文字 + `›` 分隔，末段 t1、祖先 t3。
pub fn breadcrumb(ui: &mut egui::Ui, shell: &UiShell, parts: &[&str]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::XS;
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            ui.label(
                egui::RichText::new(*part)
                    .size(font_size::SMALL)
                    .color(if last { shell.t1() } else { shell.t3() }),
            );
            if !last {
                ui.label(
                    egui::RichText::new("›")
                        .size(font_size::SMALL)
                        .color(shell.text_disabled()),
                );
            }
        }
    });
}

/// 不确定进度（目录 `UI/Loading`）：三个圆点（静态帧；动画由调用方驱动）。
pub fn loading(ui: &mut egui::Ui, shell: &UiShell) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 16.0), egui::Sense::hover());
    let p = ui.painter();
    for i in 0..3 {
        p.circle_filled(
            egui::pos2(rect.min.x + 6.0 + i as f32 * 10.0, rect.center().y),
            2.5,
            if i == 0 {
                shell.accent()
            } else {
                shell.text_disabled()
            },
        );
    }
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

// ── 行控件公共骨架 ───────────────────────────────────────────────────────

/// 分配「整宽 × `height`」的行矩形 + 交互 `Response`。
///
/// 收敛 `list_item` / `tree_row` / `layer_row` / `menu_item` / `recent_row` /
/// `option_menu` 各处反复手写的
/// `ui.allocate_exact_size(egui::vec2(ui.available_width(), H), …)`。底色 / 文字 /
/// 挂件由 caller 自绘——各行选中态细节不同，不在此一并统一。
pub(crate) fn row_rect(
    ui: &mut egui::Ui,
    height: f32,
    sense: egui::Sense,
) -> (egui::Rect, egui::Response) {
    ui.allocate_exact_size(egui::vec2(ui.available_width(), height), sense)
}

/// 选中行底：`accent_soft` 整块 +（`left_bar` 时）左缘 2px `accent` 选中条。
///
/// `list_item` / `tree_row`（带左缘条）与 `menu_item` / `option_menu`（无左缘条，
/// 传 `left_bar = false`）共用同一实现，消除两处逐行重抄的选中底代码。
pub(crate) fn paint_selected_row(
    painter: &egui::Painter,
    rect: egui::Rect,
    shell: &UiShell,
    corner: f32,
    left_bar: bool,
) {
    painter.rect_filled(rect, corner, shell.accent_soft());
    if left_bar {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
            0.0,
            shell.accent(),
        );
    }
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
    let (rect, resp) = row_rect(ui, RECENT_ROW_H, egui::Sense::click());
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
// 不是 `CARGO_MANIFEST_DIR`。ADR-0026 后本模块随控件层迁至
// crates/wv-ui/src/widgets/，回到 crate 根（crates/wv-ui/）需 2 层 `../../`
// ——图标资产落在 crates/wv-ui/assets/ui/tool_icons/。

pub const ICON_PEN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/pen.svg");
/// 抓手（平移）工具 —— Lucide `hand`（地图工具条 H）。
pub const ICON_HAND: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/hand.svg");
/// 撤销 —— Lucide `undo-2`（地图工具条）。
pub const ICON_UNDO: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/undo-2.svg");
/// 重做 —— Lucide `redo-2`（地图工具条）。
pub const ICON_REDO: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/redo-2.svg");
/// 吸附开关 —— Lucide `magnet`（地图工具条）。
pub const ICON_MAGNET: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/magnet.svg");
/// 工具条切换为纵向 —— Lucide `move-vertical`。
pub const ICON_MOVE_VERTICAL: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/move-vertical.svg");
/// 工具条切换为横向 —— Lucide `move-horizontal`。
pub const ICON_MOVE_HORIZONTAL: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/move-horizontal.svg");
/// 只读指示（Cell 编辑器「位置」段，Lucide `lock-keyhole`）。
pub const ICON_LOCK_KEYHOLE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/lock-keyhole.svg");
/// 选区范围指示（Cell 编辑器尾注，Lucide `focus`）。
pub const ICON_FOCUS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/focus.svg");
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
// 方框勾（Figma 42:2 `square-check`；check_row 勾选块用）。
pub const ICON_TRASH: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/trash-2.svg");
pub const ICON_MOUSE_POINTER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/mouse-pointer-2.svg");
pub const ICON_SQUARE_CHECK: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/square-check.svg");
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
/// 日间模式（Lucide `sun`）：暗色下点击切到亮色。
pub const ICON_SUN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/sun.svg");
/// 夜间模式（Lucide `moon`）：亮色下点击切到暗色。
pub const ICON_MOON: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/moon.svg");
/// 设置入口（Lucide `settings-2`）：顶栏最右，打开应用设置对话框。
pub const ICON_SETTINGS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/settings-2.svg");
/// 语言选择器（Lucide `languages`）：顶栏「简 / EN」控件的行首图标。
pub const ICON_LANGUAGES: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/languages.svg");

// ── 面板头部 + 图层面板（Figma `3:20892` Layers / `42:2` Inspector） ──
//
// 图层面板行内图标与头部图标同源（Lucide 12×12、`t2` 着色），见
// [`crate::widgets::layer_panel`]。

/// 面板头部：检视器 / 属性（Lucide `sliders-horizontal`，Figma `42:4`）。
pub const ICON_SLIDERS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/sliders-horizontal.svg");
/// 图层可见（Lucide `eye`）。
pub const ICON_EYE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/eye.svg");
/// 图层隐藏（Lucide `eye-off`）。
pub const ICON_EYE_OFF: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/eye-off.svg");
/// 图层锁定（Lucide `lock-keyhole`）。
pub const ICON_LOCK: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/lock-keyhole.svg");
/// 新建图层组（Lucide `folder-plus`）。
pub const ICON_FOLDER_PLUS: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/folder-plus.svg");
/// 图层类型：参考图（Lucide `image`）。
pub const ICON_IMAGE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/image.svg");
/// 图层类型：道路（Lucide `route`）。
pub const ICON_ROUTE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/route.svg");
/// 图层类型：水系（Lucide `waves`）。
pub const ICON_WAVES: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/waves.svg");
/// 图层类型：地形（Lucide `mountain`）。
pub const ICON_MOUNTAIN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/mountain.svg");
/// 图层类型：聚落 / 势力驻地（Lucide `landmark`）。
pub const ICON_LANDMARK: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/landmark.svg");
/// 面板头部：势力（Lucide `flag`）。
pub const ICON_FLAG: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/flag.svg");
/// 面板头部：态势列表（Lucide `list`）。
pub const ICON_LIST: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/list.svg");
/// 面板头部：事件 / 时间轴（Lucide `calendar` / `clock`）。
pub const ICON_CALENDAR: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/calendar.svg");
pub const ICON_CLOCK: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/clock.svg");
/// 面板头部：角色（Lucide `user`）。
pub const ICON_USER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/user.svg");
/// 图层类型：文字 / 标注（Lucide `type`）。
pub const ICON_TYPE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/type.svg");

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod recent_row_and_time_ago_tests {
    //! 2026-10-15 提到 widgets 的 [`super::recent_row`] 与
    //! [`super::format_time_ago`] 的回归。

    use super::*;
    use crate::widgets::test_support::{make_ui, shell};
    use std::time::{Duration, Instant};

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
        let (_c, mut ui) = make_ui("recent_row_tests", 400.0, 100.0);
        let r_none = recent_row(&mut ui, &sh, ICON_GLOBE, "name", "dir", None);
        assert!(
            r_none.rect.height() > 0.0,
            "None 路径至少占位，got {}",
            r_none.rect.height()
        );

        let (_c2, mut ui2) = make_ui("recent_row_tests_2", 400.0, 100.0);
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
    use crate::widgets::test_support::shell;

    /// 瓦片宽度按 Figma 采样 288 铺满画布。
    fn make_ui(name: &str) -> (egui::Context, egui::Ui) {
        crate::widgets::test_support::make_ui(name, 288.0, 100.0)
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
