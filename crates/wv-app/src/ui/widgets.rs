//! 共享 widget helper：徽标 / 空状态 / 列表项 / 浮动窗口外壳 / 面板头。
//!
//! ## 职责
//! - `BadgeKind` + `badge` —— 药丸型徽标（中性 / 通过 / 警告 / 错误 / 事件）。
//! - `empty_state` —— "图标 + 一句人话 + 一个可点的下一步"占位卡。
//! - `list_item` —— 类型色块 + 主标题 + 等宽元信息。
//! - `terrain_grid` —— 笔刷面板用的 2×5 地形色块网格（被 [`crate::ui::brush_panel`] 调用）。
//! - `float_window` / `panel_header` —— 浮动面板外壳 + 自绘标题栏三段。
//!
//! ## 边界（不做什么）
//! - **不**装配具体业务面板内容 —— 各 `*_panel.rs` 调用 `float_window` 装自己。
//! - **不**改主题/令牌 —— 调 `shell::UiShell` getter 拿颜色。
//!
//! ## 主要导出
//! - `BadgeKind` / `badge`
//! - `empty_state` / `list_item` / `terrain_grid`
//! - `float_window` / `panel_header`

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::state::{Panel, PanelState};
use crate::ui_layout::LayoutFile;
use crate::ui_theme::{color32, mono, proportional};
use crate::ui_tokens::{font_size, radius, space, BadgeTone};

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

/// 空状态卡（§5.2）：图标 + 一句人话 + 一个可点的下一步。
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
        if ui
            .add(egui::Button::new(action).min_size(egui::vec2(120.0, shell.hit)))
            .clicked()
        {
            clicked = true;
        }
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

/// 地形色块网格（§4.5：2 列 × 5 行，色块即地形本色）。
pub fn terrain_grid(ui: &mut egui::Ui, shell: &UiShell, selected: &mut u8) {
    egui::Grid::new("terrain_swatches")
        .num_columns(2)
        .spacing([space::XS, space::XS])
        .show(ui, |ui| {
            for k in 0u8..10 {
                let name = super::shell::TERRAIN_NAMES[k as usize];
                let is_sel = *selected == k;
                let cell_w = (ui.available_width() - space::XS) / 2.0;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(cell_w, 44.0), egui::Sense::click());
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
                // 16px 色块。
                let sw = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + space::SM, rect.min.y + space::SM),
                    egui::vec2(16.0, 16.0),
                );
                painter.rect_filled(sw, radius::BADGE, terrain_swatch(k));
                // 10px 文字。
                painter.text(
                    egui::pos2(
                        rect.min.x + space::SM,
                        rect.min.y + space::SM + 16.0 + space::XS,
                    ),
                    egui::Align2::LEFT_TOP,
                    name,
                    proportional(10.0),
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

/// 浮动面板外壳：类型图标 + 标题 + 徽标 + 折叠，可拖动，可关闭。
///
/// `layouts` 提供**跨会话**的位置/尺寸（`ui-layout.yaml`）；本帧的拖动结果由
/// egui 按窗口 `Id` 自己记住，并由 [`crate::ui_shell::persist_layout_on_change`]
/// 回写磁盘（D1：面板是客人，地图是主体）。
pub fn float_window(
    ctx: &egui::Context,
    shell: &UiShell,
    layouts: &LayoutFile,
    panels: &mut PanelState,
    panel: Panel,
    tag: Option<(BadgeKind, String)>,
    body: impl FnOnce(&mut egui::Ui),
) {
    if !panels.is_visible(panel) {
        return;
    }

    let layout = crate::ui_layout::current_layout(layouts, panels, panel);
    let mut open = true;
    let id = egui::Id::new(("wv_panel", panel));

    // 沉浸模式：整体淡化但**仍可交互**（D3：状态永不抑制）。
    let alpha = if panels.immersive { 0.10 } else { 0.96 };

    let win = egui::Window::new("")
        .id(id)
        .open(&mut open)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .default_open(!layout.collapsed)
        .resizable(true)
        .collapsible(true)
        .title_bar(true)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(crate::ui_theme::margin_all(space::SM))
                .corner_radius(radius::PANEL),
        );

    win.show(ctx, |ui| {
        // 自绘标题栏三段（§4.1）：⠿ 拖动把手 · 图标+标题 · 徽标。
        panel_header(ui, shell, panel, tag);
        ui.add_space(space::XS);
        ui.separator();
        ui.add_space(space::XS);
        body(ui);
    });

    if !open {
        panels.set_visible(panel, false);
    }
}

/// 自绘面板标题栏三段：⠿ 拖动把手 · 图标+标题 · 徽标（§4.1）。
pub fn panel_header(
    ui: &mut egui::Ui,
    shell: &UiShell,
    panel: Panel,
    tag: Option<(BadgeKind, String)>,
) {
    ui.horizontal(|ui| {
        // 拖动把手（视觉上明示可拖）。
        ui.label(
            egui::RichText::new("⠿")
                .size(shell.scale.micro)
                .color(shell.t3().gamma_multiply(0.6)),
        );
        // 类型图标（accent 着色）。
        ui.label(
            egui::RichText::new(panel.icon())
                .size(shell.scale.base)
                .color(shell.accent()),
        );
        // 标题（11px，字距 .05em 近似为大写风格）。
        ui.label(
            egui::RichText::new(panel.title())
                .size(shell.scale.micro)
                .color(shell.t1()),
        );
        if let Some((kind, text)) = tag {
            ui.add_space(space::XS);
            badge(ui, shell, kind, &text);
        }
    });
}
