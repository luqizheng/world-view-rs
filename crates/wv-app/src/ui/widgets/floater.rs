//! 浮动面板外壳 + 自绘标题行。
//!
//! 单一职责：装配 `egui::Window` 风格的浮层，对外只暴露 `float_window` 这一个入口；
//! `panel_head` 是被它内部消费的私有 helper（也通过 `pub(crate)` 给同目录其它文件
//! 复用），标题栏、徽标、折叠三角都画在自己的 painter 调用里。
//!
//! 历史来源：拆自原 [`crate::ui::widgets`] 中段（D1「面板是客人，地图是主体」 +
//! v0.3 Figma 重做后的自绘标题栏）。

#![cfg(feature = "app")]

use std::sync::Arc;

use bevy_egui::egui;

use crate::i18n::Locale;
use crate::state::{Panel, PanelState};
use crate::theme::ui_theme::{color32, proportional};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui_layout::LayoutFile;

use super::primitives::BadgeKind;
use crate::ui::shell::UiShell;

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
pub(crate) fn panel_head(
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
