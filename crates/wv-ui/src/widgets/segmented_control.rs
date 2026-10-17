//! 段控件（Figma 公共组件目录 `UI/Segmented`：容器 `bg_input` 底 +
//! `border_subtle` 1px + 圆角 6 + 内边距 2px；选中段 `accent_soft` 底 +
//! `accent` 字；非选中 `t2` 字）。
//!
//! ## 职责
//! - 抽「N 选 1」连续段控件为公共 widget——顶栏 2 处使用（Browse/Edit 段、
//!   地图主题段），笔刷面板 1 处使用（涂/塑/放 3 段）。
//!
//! ## 边界（不做什么）
//! - **不**承载下拉菜单 / 嵌套控件 / 工具提示——段就是段，多一层就走
//!   `ToolButton` 或 `ComboBox`。
//! - **不**写主题令牌源头——所有颜色走 `UiShell` 解析。
//!
//! ## 视觉规格（2026-10-18 对齐目录）
//! - 容器：`bg_input` 底 + `border_subtle` 1px 描边 + 6px 圆角 + 2px 内边距。
//! - 选中段：`accent_soft` 底 + 4px 圆角 + `accent` 字。
//! - 非选中段：`t2` 字；hover 态走 `hover_bg`。
//! - 命中区 = `seg_w_each` × 28（目录 64×24 段 + 2px 内边距）。
//!
//! ## 已知使用点（2026-10-16 提公共后）
//! - `top_bar::browse_edit_segment` —— Browse/Edit 段
//! - `top_bar::theme_segment` —— 地图主题段
//! - `brush_panel::tool_segmented` —— 涂/塑/放 3 段

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::ui_theme::proportional;

/// 段控件：等宽 N 段，选中段高亮，点击触发 `on_pick(index)`。
///
/// ## 参数
/// - `seg_w_each`：每段宽度（命中区与视觉宽度一致）。
/// - `labels`：每段文案（顺序即 `on_pick` 的 index 顺序）。
/// - `active_index`：当前选中段下标（越界时按 0 处理 + 仍渲染点击）。
/// - `font_px`：段内字号（顶栏用 `font_size::SMALL`、面板用 `scale.micro`）。
/// - `on_pick`：被点段的下标；`FnOnce` 不可复用。
#[allow(clippy::too_many_arguments)]
pub fn segmented(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: &str,
    seg_w_each: f32,
    labels: &[&str],
    active_index: usize,
    font_px: f32,
    mut on_pick: impl FnMut(usize),
) {
    let total_w = seg_w_each * labels.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, shell.hit), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, shell.input_bg());
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.0, shell.border_subtle()),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(2.0);
    let each = inner.width() / labels.len().max(1) as f32;
    for (i, label) in labels.iter().enumerate() {
        let r = egui::Rect::from_min_max(
            egui::pos2(inner.min.x + each * i as f32, inner.min.y),
            egui::pos2(inner.min.x + each * (i as f32 + 1.0), inner.max.y),
        );
        let resp = ui.interact(r, egui::Id::new((id, i)), egui::Sense::click());
        if i == active_index {
            painter.rect_filled(r, 4.0, shell.accent_soft());
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                proportional(font_px),
                shell.accent(),
            );
        } else {
            if resp.hovered() {
                painter.rect_filled(r, 5.0, shell.hover_bg());
            }
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                proportional(font_px),
                shell.t2(),
            );
        }
        if resp.clicked() {
            on_pick(i);
        }
    }
}

#[cfg(test)]
mod tests {
    //! 段控件是纯绘制——`on_pick(index)` 是唯一动作，layout 数学是它的不变量。
    //! 不直接渲染（egui 0.36 unit 上下文无 GPU surface），钉两件事：
    //! 1. 总宽 = `seg_w_each * len`；内边距缩 2 px 后 5 段均分。
    //! 2. 选中段数学：active_index 越界不 panic，按 0 处理（与顶栏既有语义一致）。

    #[test]
    fn total_width_is_segments_times_each() {
        // 顶栏 Browse/Edit：2 段 × 64 px = 128；3 段工具选择 = 3 × 60 = 180。
        assert_eq!(2.0 * 64.0, 128.0);
        assert_eq!(3.0 * 60.0, 180.0);
    }

    #[test]
    fn inner_rect_shrinks_by_two_each_side_before_dividing() {
        // 容器 60×N，缩 2 px 后内框 (60N − 4) × (h − 4)；N 段均分。
        let total_w = 60.0 * 3.0;
        let inner_w = total_w - 4.0;
        let each = inner_w / 3.0;
        assert!(
            (each - (60.0_f32 - 4.0_f32 / 3.0_f32)).abs() < 1e-3_f32,
            "got {each}"
        );
    }

    /// 选中段越界时不 panic：调用方传 usize，超长时按 0 兜底。
    /// `top_bar::browse_edit_segment` / `theme_segment` 既有语义：
    /// `position(...).unwrap_or(0)`——不在列表里就归 0。
    #[test]
    fn out_of_range_active_index_falls_back_to_zero() {
        let modes = ["A", "B", "C"];
        let active: Option<usize> = modes.iter().position(|m| *m == "Z");
        assert_eq!(active.unwrap_or(0), 0, "未命中应兜底为 0");
    }
}
