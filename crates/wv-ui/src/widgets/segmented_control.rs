//! 段控件（Figma 公共组件目录 `UI/Segmented`：容器 `bg_input` 底 +
//! `border_subtle` 1px + 圆角 6 + 内边距 2px；选中段 `accent_soft` 底 +
//! `accent` 字；非选中 `t2` 字）。
//!
//! ## 职责
//! - 抽「N 选 1」连续段控件为公共 widget——顶栏 1 处使用（Browse/Edit 段），
//!   笔刷面板 1 处使用（涂/塑/放 3 段）。
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
//! - 命中区 = `seg_w_each` × `shell.hit`（目录 64×24 段 + 2px 内边距）。
//!
//! ## 已知使用点（2026-10-16 提公共后）
//! - `top_bar::browse_edit_segment` —— Browse/Edit 段（2026-10-19 起 Faction
//!   与地图主题段已移除，地图主题改走命令面板）
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
    on_pick: impl FnMut(usize),
) {
    segmented_with_disabled(
        ui,
        shell,
        id,
        seg_w_each,
        labels,
        active_index,
        font_px,
        &[],
        on_pick,
    );
}
/// 段控件（**可按段禁用**版）。
///
/// `disabled[i] == true` 的段：文字走 `text_disabled`、不响应 hover / 点击
/// ——用于「某一段当前不可进」的场景（[ADR-0052] §9.4 Gate-2：无地图时顶栏
/// 的 `CellMap` 段灰化不可点，但 `Browse` / `Edit` 照常）。
///
/// `disabled` 短于 `labels` 时缺省视为全可用。
#[allow(clippy::too_many_arguments)]
pub fn segmented_with_disabled(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: &str,
    seg_w_each: f32,
    labels: &[&str],
    active_index: usize,
    font_px: f32,
    disabled: &[bool],
    mut on_pick: impl FnMut(usize),
) -> egui::Response {
    let total_w = seg_w_each * labels.len() as f32;
    let (rect, whole_resp) =
        ui.allocate_exact_size(egui::vec2(total_w, shell.hit), egui::Sense::hover());
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
        let is_disabled = disabled.get(i).copied().unwrap_or(false);
        if is_disabled {
            // 禁用段：不注册点击命中（`Sense::hover` 只够出 tooltip），
            // 文字走禁用色——视觉与行为一致，不会「看着能点但没反应」。
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                proportional(font_px),
                shell.text_disabled(),
            );
            continue;
        }
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
    whole_resp
}

#[cfg(test)]
mod tests {
    //! 段控件是纯绘制 + 交互：`on_pick(index)` 是唯一动作。
    //!
    //! 用**真实 headless `egui::Ui`** 装配 [`segmented`]（不再用与实现同款的
    //! 算术式自证那类空转断言——那种写法从不调用被测函数，等于没测）：
    //! 1. 分配矩形宽 = `seg_w_each * len`、高 = `shell.hit`。
    //! 2. 无输入时不触发 `on_pick`。
    //! 3. 越界 `active_index` 不 panic。
    //! 4. 三帧驱动真实指针事件 → 命中段触发 `on_pick(index)`（覆盖
    //!    `resp.clicked() → on_pick` 路径）。

    use super::*;
    use crate::widgets::test_support::{make_ui, shell};

    #[test]
    fn segmented_occupies_seg_w_times_len_by_hit() {
        let sh = shell();
        let (_c, mut ui) = make_ui("segmented_geom", 400.0, 60.0);
        let mut picks = Vec::new();
        segmented(
            &mut ui,
            &sh,
            "seg_geom",
            64.0,
            &["Browse", "Edit"],
            1,
            12.0,
            |i| picks.push(i),
        );
        let size = ui.min_rect().size();
        assert!(
            (size.x - 128.0).abs() < 1e-3,
            "2 × 64 = 128 宽，got {}",
            size.x
        );
        assert!(
            (size.y - sh.hit).abs() < 1e-3,
            "高应 = shell.hit({})，got {}",
            sh.hit,
            size.y
        );
        assert!(
            picks.is_empty(),
            "headless 无点击不应触发 on_pick，got {picks:?}"
        );
    }

    #[test]
    fn segmented_tolerates_out_of_range_active_index() {
        let sh = shell();
        let (_c, mut ui) = make_ui("segmented_oob", 200.0, 60.0);
        // active_index = 99 越界（与顶栏 `unwrap_or(0)` 语义一致）不应 panic。
        segmented(&mut ui, &sh, "seg_oob", 50.0, &["A", "B"], 99, 12.0, |_| {});
    }

    /// 真点击：三帧驱动（hover → 按下 → 抬起），指针落在第 1 段中心，
    /// 断言 `on_pick(1)` 被触发——这是唯一真正覆盖点击链路的断言。
    #[test]
    fn segmented_reports_clicked_segment_index() {
        let sh = shell();
        let seg_w = 60.0;
        let labels = ["A", "B", "C"];
        // 容器从 (0,0) 起（见下 `Frame::default()` 零边距），内缩 2 → 第 1 段
        // 覆盖 x ≈ [60.67, 119.33]；指针取其中心 (90, hit/2)。
        let pos = egui::pos2(seg_w * 1.5, sh.hit * 0.5);
        let mut picks: Vec<usize> = Vec::new();
        let ctx = egui::Context::default();
        for frame in 0..3 {
            let events = match frame {
                0 => vec![egui::Event::PointerMoved(pos)],
                1 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                _ => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default())
                    .show(ctx, |ui| {
                        segmented(ui, &sh, "seg_click", seg_w, &labels, 0, 12.0, |i| {
                            picks.push(i)
                        });
                    });
            });
            // headless 无 GPU surface：丢弃未应用的纹理增量，避免 epaint
            // 「Dropped TexturesDelta with unapplied deltas」panic。
            output.textures_delta.clear();
        }
        assert!(
            picks.contains(&1),
            "第 1 段被点击应触发 on_pick(1)，got {picks:?}"
        );
    }

    /// **禁用段不响应点击**（[ADR-0052] §9.4 Gate-2：无地图时 CellMap 段灰化）。
    ///
    /// 与上一条同款装配，唯一区别是 `disabled` 把被点段标禁用——
    /// 命中区根本不注册，`on_pick` 一次都不该被调用。
    #[test]
    fn segmented_disabled_segment_ignores_clicks() {
        let sh = shell();
        let seg_w = 60.0;
        let labels = ["A", "B", "C"];
        let pos = egui::pos2(seg_w * 1.5, sh.hit * 0.5);
        let mut picks: Vec<usize> = Vec::new();
        let ctx = egui::Context::default();
        for frame in 0..3 {
            let events = match frame {
                0 => vec![egui::Event::PointerMoved(pos)],
                1 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                _ => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default())
                    .show(ctx, |ui| {
                        segmented_with_disabled(
                            ui,
                            &sh,
                            "seg_disabled",
                            seg_w,
                            &labels,
                            0,
                            12.0,
                            // 第 1 段（被点的那个）禁用。
                            &[false, true, false],
                            |i| picks.push(i),
                        );
                    });
            });
            output.textures_delta.clear();
        }
        assert!(picks.is_empty(), "禁用段不该触发 on_pick，got {picks:?}");
    }

    /// 禁用第 2 段时，第 1 段仍然可点（禁用不外溢到邻段）。
    #[test]
    fn segmented_disabling_one_segment_leaves_neighbours_clickable() {
        let sh = shell();
        let seg_w = 60.0;
        let labels = ["A", "B", "C"];
        // 第 2 段（x ≈ [120.67, 179.33]）中心。
        let pos = egui::pos2(seg_w * 2.5, sh.hit * 0.5);
        let mut picks: Vec<usize> = Vec::new();
        let ctx = egui::Context::default();
        for frame in 0..3 {
            let events = match frame {
                0 => vec![egui::Event::PointerMoved(pos)],
                1 => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                _ => vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 200.0),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::default())
                    .show(ctx, |ui| {
                        segmented_with_disabled(
                            ui,
                            &sh,
                            "seg_partial",
                            seg_w,
                            &labels,
                            0,
                            12.0,
                            // 只禁第 1 段（索引 1），点的是索引 2。
                            &[false, true, false],
                            |i| picks.push(i),
                        );
                    });
            });
            output.textures_delta.clear();
        }
        assert!(picks.contains(&2), "未禁用的第 2 段应可点，got {picks:?}");
    }
}
