//! 缩放控件条（Figma `3:21882` · Zoom controls，2026-10-19）。
//!
//! ## 职责
//! - `ZoomControlsState` —— 拖动手柄位置 + 显隐缩放百分比。
//! - `zoom_controls` —— 自绘 32 高横条（grip + − + 百分比 + ＋ + 分隔条 + 适配）。
//!   整条**可拖**：grip 是显式把柄（光标 Grab / Grabbing），其余区间也跟着走。
//!
//! ## 使用方
//! - `map_controls` —— 替代原先 `icon_tile(zoom_in/zoom_out/maximize)` 三块。
//! - `panel_minimap` —— 缩略图下方的同一控件条（小地图用户常在这里微调）。
//!
//! ## 视觉规格（Figma `3:21882`）
//! - 大框：`float_fill` 底 + `border_strong` 描边 + 圆角 6。
//! - 高 32，水平 padding 12，grip 后三项按 Figma 行布局：
//!   `[grip 12]  − 12   百分比（IBM Plex Mono 11）  ＋ 12   │ 1×16   ⌐⌐ 14`。
//! - 工具图标 12 / 14 px，色 `t3` 默认、`t1` hover。
//!
//! ## 边界
//! - **不**拥有 `ViewState` / `LevelZoom` 之类的领域资源——caller 算好 `scale`
//!   与三件事的回调，本函数只画 + 处理拖动。
//! - **不**做交互互斥——开 / 关工具是分发器的事。
//! - 持久化是 caller 的事：`ZoomControlsState::pos` 用谁的 key 由 caller
//!   决定（地图工具条 / 小地图 / 未来其它位点可以各持一份）。
//!
//! ## 主要导出
//! - `ZoomControlsState` —— 拖动状态资源。
//! - `zoom_controls` —— UI 函数。

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::ui_theme::mono;

/// 缩放控件条状态。
///
/// **设计要点**：本结构只记「与领域无关的几何」——grip 拖动后的位置偏移
/// （`pos: Option<[f32; 2]>`）。它**不**存 `scale`：缩放跟随领域状态，条
/// 本身只是观察者。
///
/// 持久化由 caller 决定，序列化时整结构可走 `serde`：
/// `pos` 为 `None` 时用默认锚点。
#[derive(Debug, Clone, PartialEq)]
pub struct ZoomControlsState {
    /// 用户拖动 grip 后的锚点偏移：`[x, y]`。
    /// `None` = caller 提供的默认锚点（参考 `map_controls` 的 `default_offset`）。
    pub pos: Option<[f32; 2]>,
    /// 是否在条中显示「百分比」文字。
    ///
    /// 小地图场景里有时希望只保留 − / ＋ / 适配 三块（更紧凑），由 caller 关闭。
    /// 默认开。
    pub show_scale_text: bool,
}

impl Default for ZoomControlsState {
    fn default() -> Self {
        Self {
            pos: None,
            show_scale_text: true,
        }
    }
}

/// 缩放控件条（Figma `3:21882`）。
///
/// 绘制一条 32 高的横条：grip + − + 百分比（可选） + ＋ + 分隔 + 适配。
/// 整条由 `grip_drag` 累计到 `state.pos`，松手后 caller 落盘。
///
/// **参数：**
/// - `scale` —— 当前 `ViewState.scale`；用于显示百分比与判定「到位」。
/// - `can_zoom_in` / `can_zoom_out` —— 上 / 下限钳制（来自 `LevelZoom`）。
///   `false` 时对应按钮用 `text_disabled` 灰，**不可点**。
/// - `on_zoom_in` / `on_zoom_out` / `on_fit` —— 三个按钮的回调；本函数
///   只在「按钮确实被点击」时调一次，**不**做实际写状态（caller 改
///   `ViewState.scale`，下一帧回灌）。
///
/// **返回值**：条整体的 `egui::Response`（含 drag），caller 用来观察
/// `dragged()` 之外的额外状态（一般不用）。
#[allow(clippy::too_many_arguments)]
pub fn zoom_controls(
    ui: &mut egui::Ui,
    shell: &UiShell,
    state: &mut ZoomControlsState,
    scale: f32,
    can_zoom_in: bool,
    can_zoom_out: bool,
    on_zoom_in: impl FnOnce(),
    on_zoom_out: impl FnOnce(),
    on_fit: impl FnOnce(),
) {
    // 大框：Figma `3:21882` —— float_fill + border_strong + radius 6。
    //
    // 高度 32 = 行高 16 + 上下内边距 8×2。**不能**用 `set_min_height(32)`
    // 撑高父容器——横向子行会贴在容器顶部，图标与文字全部偏上（2026-10-19
    // 实测截图）。用上下内边距让 `ui.horizontal` 的 Align::Center 真正生效。
    let bg = shell.float_fill();
    let stroke_color = shell.border_strong();
    let frame = egui::Frame::NONE
        .fill(bg)
        .stroke(egui::Stroke::new(1.0, stroke_color))
        .corner_radius(egui::CornerRadius::from(6.0))
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 8,
        });
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 0.0);

            // ── Grip（把柄） ───────────────────────────────────────────
            let (grip_rect, grip_resp) =
                ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::drag());
            if grip_resp.hovered() || grip_resp.dragged() {
                ui.painter().rect_filled(grip_rect, 2.0, shell.hover_bg());
            }
            // 6 个小点（3×2）模拟 Figma grip-vertical 的纹理。
            let dot_r = 1.0;
            let dot_gap = 2.0;
            for col in 0..2 {
                for row in 0..3 {
                    let c = egui::pos2(
                        grip_rect.left() + 2.0 + col as f32 * (dot_r * 2.0 + dot_gap),
                        grip_rect.top() + 2.0 + row as f32 * (dot_r * 2.0 + dot_gap),
                    );
                    ui.painter().circle_filled(c, dot_r, shell.t3());
                }
            }
            // 在消耗 `grip_resp` 之前先读两个状态量；`on_hover_*` 会 move。
            let dragged = grip_resp.dragged();
            if dragged {
                let delta = grip_resp.drag_delta();
                let p = state.pos.get_or_insert([0.0, 0.0]);
                p[0] += delta.x;
                p[1] += delta.y;
            }
            let cursor = if dragged {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            };
            grip_resp
                .on_hover_cursor(cursor)
                .on_hover_text("拖动移动 / − ／ ＋ 缩放 / 适配全图");

            // ── −（缩小） ─────────────────────────────────────────────
            if zoom_button(ui, shell, ZOOM_OUT_SVG, can_zoom_out, "缩小").clicked() {
                on_zoom_out();
            }

            // ── 百分比（Figma `125%`） ────────────────────────────────
            //
            // 与 icon 同高：分配 14×14 矩形（同 `zoom_button`），用
            // `Align2::CENTER_CENTER` 把文字**整体**（galley 行框）对准矩形
            // 中心。不要手算「行高 × 系数」——那依赖字体 ascent/descent，
            // 换字体就漂；`CENTER_CENTER` 是 egui 全局标签的对齐口径，
            // 与图标 `fit_to_exact_size` 的视觉中心同源。
            if state.show_scale_text {
                let pct = scale_to_percent(scale);
                let galley = ui
                    .painter()
                    .layout_no_wrap(format!("{pct}%"), mono(11.0), shell.t1());
                let (text_rect, _) = ui.allocate_exact_size(
                    egui::vec2(galley.size().x.max(14.0), 14.0),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    text_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{pct}%"),
                    mono(11.0),
                    shell.t1(),
                );
            }

            // ── ＋（放大） ─────────────────────────────────────────────
            if zoom_button(ui, shell, ZOOM_IN_SVG, can_zoom_in, "放大").clicked() {
                on_zoom_in();
            }

            // ── 分隔条 ────────────────────────────────────────────────
            let sep_h = 16.0;
            let (sep_rect, _) =
                ui.allocate_exact_size(egui::vec2(1.0, sep_h), egui::Sense::hover());
            let stroke = egui::Stroke::new(1.0, shell.border_subtle());
            ui.painter().line_segment(
                [
                    egui::pos2(sep_rect.left_center().x, sep_rect.top()),
                    egui::pos2(sep_rect.left_center().x, sep_rect.bottom()),
                ],
                stroke,
            );

            // ── 适配（maximize = 四角括号） ──────────────────────────
            if zoom_button(ui, shell, FIT_SVG, true, "缩放到全图 (F)").clicked() {
                on_fit();
            }
        });
    });
}

/// 缩放条内的图标按钮（− / ＋ / 适配）。统一尺寸 14×14，hover 显 `t1`。
/// 返回 `Response` —— caller 自己读 `clicked()`。
fn zoom_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    svg: egui::ImageSource<'static>,
    enabled: bool,
    hint: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(14.0, 14.0),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hovered = enabled && resp.hovered();
    let tint = if !enabled {
        shell.text_disabled()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    crate::widgets::primitives::icon_image(svg, tint, 14.0).paint_at(
        ui,
        egui::Rect::from_min_size(rect.min, egui::vec2(14.0, 14.0)),
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(hint)
    } else {
        resp
    }
}

/// `ViewState.scale` 语义：值 = 世界单位 / 屏幕像素（Figma 1.0 = 100%）。
/// 百分比 = 1 / scale × 100。
pub fn scale_to_percent(scale: f32) -> i32 {
    if scale <= 0.0 {
        100
    } else {
        (100.0 / scale).round() as i32
    }
}

// 注：图标常量走 crate 顶层的 `ICON_ZOOM_IN` / `ICON_ZOOM_OUT` /
// `ICON_MAXIMIZE` —— `ZoomControls` 在 crate 根定义；本子模块按 SVG
// 字面量引用即可（避免再加三层重导出）。
use crate::widgets::primitives::ICON_MAXIMIZE as FIT_SVG;
use crate::widgets::primitives::ICON_ZOOM_IN as ZOOM_IN_SVG;
use crate::widgets::primitives::ICON_ZOOM_OUT as ZOOM_OUT_SVG;

#[cfg(test)]
mod tests {
    use super::*;

    /// `scale_to_percent` 是「世界单位 / 像素」的倒数（100% = scale 1.0）。
    /// 钳制 0 边界，避免 divide-by-zero 崩。
    #[test]
    fn scale_to_percent_basic_values() {
        assert_eq!(scale_to_percent(1.0), 100);
        assert_eq!(scale_to_percent(0.5), 200);
        assert_eq!(scale_to_percent(2.0), 50);
        assert_eq!(scale_to_percent(0.0), 100);
        assert_eq!(scale_to_percent(-1.0), 100);
    }

    /// `ZoomControlsState` 默认 = 默认位置 + 显示百分比。
    #[test]
    fn state_defaults_are_visible() {
        let s = ZoomControlsState::default();
        assert!(s.pos.is_none());
        assert!(s.show_scale_text);
    }
}
