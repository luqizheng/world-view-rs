//! 地图视口比例尺（视口左下角浮层，Google / OSM 式：条 + 端刻度 + 距离标签）。
//!
//! ## 为什么需要它
//! 海岸线平滑、群系边界这类「形态自然度」的验收没有尺度参照就无法下结论：
//! 同样一条平滑曲线，突出部若只有几米，平滑是对的；若有 2 公里，放大后
//! 仍然平滑就是错的。比例尺把「屏幕上这段线 ≈ 多少公里」变成可读数，
//! 作者不必心算世界单位。
//!
//! ## 换算链（零手搓，两因子各只有单一实现点）
//! - 世界单位/像素：`ViewState.scale`（渲染层钳制的相机缩放，语义即
//!   世界单位/屏幕像素）；
//! - 公里/世界单位：当前图 `MapNode::km_per_wu()`——真实底图
//!   走 geographic bbox，虚构图走作者自报的 `unit_per_km`，未声明返回
//!   `None`。
//!
//! `km/px = scale × km_per_wu`，本模块只做这一次乘法与 1-2-5 取整。
//!
//! ## 可见性（V1：缺数据是正常态，提示缺失但不阻拦）
//! 无工程 / 无当前图 / 图未声明 km 换算时**整层静默不画**——不假装有尺度，
//! 同校验面板尺度提示（`land_extent_km`）的口径。要让它出现，在当前图的
//! `index.yaml` 里补一行 `unit_per_km: <每公里多少世界单位>` 即可。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{DrillState, ViewState, WorldProjection};
use crate::theme::ui_theme::{color32, color32_alpha, margin_xy, mono, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui::shell::UiShell;
use crate::ui::top_bar::STATUS_BAR_H;

/// 比例尺字号：10px，与状态栏同档「仪表读数」（比令牌阶梯 MICRO 再小半档）。
const SCALE_FONT: f32 = 10.0;
/// 条长目标窗口下限（px）：候选距离短到画不满这条就换上一档。
const MIN_BAR_PX: f64 = 64.0;
/// 1-2-5 候选距离（米）：1 m 起步、50,000 km 封顶，跨 8 个数量级。
/// 邻档比值 ≤ 5，保证任何缩放下窗口内至少有一档可选中。
const CANDIDATES_M: [f64; 24] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1e3, 2e3, 5e3, 1e4, 2e4, 5e4, 1e5, 2e5,
    5e5, 1e6, 2e6, 5e6, 1e7, 2e7, 5e7,
];

/// 比例尺浮层系统：视口左下角（状态栏上方）画 1-2-5 取整的比例尺条。
///
/// 条长随缩放连续变化，**标签只在换档时跳变**（1-2-5 序列），这正是
/// Google / OSM 的行为。当前图未声明 km 换算时整层不画（见模块文档）。
pub fn map_scale_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Some((_, label, bar_px)) = scale_bar_readout(&view, &projection, &drill) else {
        return;
    };

    let shell = UiShell::new(&shell_cfg);
    egui::Area::new(egui::Id::new("wv_map_scale_bar"))
        .anchor(
            egui::Align2::LEFT_BOTTOM,
            // 视口左下角，避开车状态栏（28px）再留一组距。
            egui::vec2(space::LG, -(STATUS_BAR_H + space::MD)),
        )
        .order(egui::Order::Middle) // 顶栏中段同层；画布之上、模态之下
        .show(ctx, |ui| {
            egui::Frame::NONE
                // 底板跟 chrome 走（同状态栏的 bg_app 语义），任意明暗底图上可读。
                .fill(color32_alpha(shell.palette.bg_app, 0.85))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .corner_radius(egui::CornerRadius::from(radius::FIELD))
                .inner_margin(margin_xy(space::SM, space::XS))
                .show(ui, |ui| {
                    draw_scale_bar(ui, &shell, bar_px, &label);
                });
        });
}

/// 换算三件套：`(距离 km, 标签, 条长 px)`。
///
/// 从 [`ViewState::scale`]（世界单位/像素）与当前图 `km_per_wu`（公里/世界
/// 单位）推出 km/px 后交给 [`pick_scale_distance`] 取整。任一前提缺失
/// （无图 / 无换算 / 非正缩放）返回 `None`。
fn scale_bar_readout(
    view: &ViewState,
    projection: &WorldProjection,
    drill: &DrillState,
) -> Option<(f64, String, f32)> {
    let node = drill.current_node(&projection.world)?;
    let (km_per_wu_x, _) = node.km_per_wu()?;
    let scale = f64::from(view.scale);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let km_per_px = scale * km_per_wu_x;
    let (km, label) = pick_scale_distance(km_per_px)?;
    let bar_px = (km / km_per_px) as f32;
    Some((km, label, bar_px))
}

/// 纯逻辑：按「每像素多少公里」从 1-2-5 序列选比例尺距离。
///
/// 返回 `(距离 km, 标签)`。取**最小的**使条长 ≥ [`MIN_BAR_PX`] 的候选；
/// 极端拉远到连最大候选（50,000 km）都画不满窗时走循环尾回退——返回
/// 最大候选、条短于窗口照画：比例尺第一职责是如实反映换算，凑窗口
/// 长度是第二职责。
pub fn pick_scale_distance(km_per_px: f64) -> Option<(f64, String)> {
    if !km_per_px.is_finite() || km_per_px <= 0.0 {
        return None;
    }
    let px_per_km = 1.0 / km_per_px;
    let mut fallback = None;
    for m in CANDIDATES_M {
        let km = m / 1000.0;
        let candidate = (km, format_distance(km));
        if km * px_per_km >= MIN_BAR_PX {
            return Some(candidate);
        }
        fallback = Some(candidate);
    }
    fallback
}

/// 距离标签：< 1 km 出米（整数），≥ 1 km 出公里（候选序列保证整数）。
fn format_distance(km: f64) -> String {
    if km < 1.0 {
        format!("{} m", (km * 1000.0).round() as u64)
    } else {
        format!("{} km", km as u64)
    }
}

/// 画条 + 端刻度 + 居中标签：条宽 `bar_px`，标签横跨条上方居中。
fn draw_scale_bar(ui: &mut egui::Ui, shell: &UiShell, bar_px: f32, label: &str) {
    let ink = color32(shell.palette.text_2);
    let font = mono(SCALE_FONT);
    let galley = ui
        .painter()
        .layout(label.to_owned(), font, ink, f32::INFINITY);
    // 条与标签取宽者作占位：条极短时标签不外溢底板。
    let w = bar_px.max(galley.size().x);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::hover());
    let painter = ui.painter();
    let line_y = rect.max.y - 5.0;
    let x0 = rect.center().x - bar_px * 0.5;
    let x1 = rect.center().x + bar_px * 0.5;
    let stroke = egui::Stroke::new(1.0, ink);
    painter.line_segment([egui::pos2(x0, line_y), egui::pos2(x1, line_y)], stroke);
    // 两端 6px 端刻度（地图学惯例：标明条的两个端点才可量）。
    painter.line_segment(
        [egui::pos2(x0, line_y - 3.0), egui::pos2(x0, line_y + 3.0)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(x1, line_y - 3.0), egui::pos2(x1, line_y + 3.0)],
        stroke,
    );
    painter.galley(
        egui::pos2(rect.center().x - galley.size().x * 0.5, rect.min.y + 1.0),
        galley,
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 条长结构上界：相邻候选最大比值 2.5（2→5 档），故「首个入窗候选」
    /// 的条长必 < `MIN_BAR_PX × 2.5`——上界是序列结构的推论，不是算法输入。
    const MAX_BAR_PX: f64 = MIN_BAR_PX * 2.5;

    #[test]
    fn one_km_is_picked_when_it_lands_inside_the_window() {
        // 1 px = 10 m（km_per_px = 0.01）：1 km 条 = 100 px，落在 [64, 160]。
        let (km, label) = pick_scale_distance(0.01).expect("正常缩放应有候选");
        assert!((km - 1.0).abs() < 1e-9, "应选 1 km，得 {km}");
        assert_eq!(label, "1 km");
    }

    #[test]
    fn mid_scale_picks_five_meters() {
        // 1 px = 5 cm：1 m → 20 px、2 m → 40 px 都短于窗，5 m → 100 px 首个入窗。
        let (km, label) = pick_scale_distance(5e-5).expect("正常缩放应有候选");
        assert!((km - 0.005).abs() < 1e-9, "应选 5 m，得 {km}");
        assert_eq!(label, "5 m");
    }

    #[test]
    fn zoomed_out_picks_hundred_km() {
        // 1 px = 1 km：50 km 条 = 50 px 短于窗，100 km 条 = 100 px 首个入窗。
        let (km, label) = pick_scale_distance(1.0).expect("正常缩放应有候选");
        assert!((km - 100.0).abs() < 1e-9, "应选 100 km，得 {km}");
        assert_eq!(label, "100 km");
    }

    #[test]
    fn extreme_zoom_in_still_picks_the_smallest_sensible_candidate() {
        // 1 px = 10 cm：1 m → 10 px、2 m → 20 px、5 m → 50 px 都短于窗，
        // 10 m → 100 px 首个入窗——再拉近就是最小候选直接超窗的诚实回退。
        let (km, label) = pick_scale_distance(1e-4).expect("正常缩放应有候选");
        assert!((km - 0.01).abs() < 1e-9, "应选 10 m，得 {km}");
        assert_eq!(label, "10 m");
    }

    #[test]
    fn extreme_zoom_out_falls_back_to_largest_candidate_honestly() {
        // 1 px = 1,000 km：50,000 km 条也只有 0.05 px——返回最大候选。
        let (km, label) = pick_scale_distance(1e6).expect("应有防御性候选");
        assert!((km - 5e4).abs() < 1e-9, "应退到最大候选 50,000 km，得 {km}");
        assert_eq!(label, "50000 km");
    }

    #[test]
    fn non_positive_or_nan_scale_returns_none() {
        assert_eq!(pick_scale_distance(0.0), None, "零换算应返回 None");
        assert_eq!(pick_scale_distance(-1.0), None, "负换算应返回 None");
        assert_eq!(pick_scale_distance(f64::NAN), None, "NaN 应返回 None");
    }

    #[test]
    fn picked_bar_stays_inside_window_across_eight_decades() {
        // 性质扫：除两端防御性回退外，任意数量级的条长都应落回 [64, 160] 窗口。
        for exp in -6..=3 {
            for mantissa in [1.0, 2.0, 5.0] {
                let km_per_px = mantissa * 10f64.powi(exp);
                let Some((km, _)) = pick_scale_distance(km_per_px) else {
                    panic!("km_per_px={km_per_px} 应有候选");
                };
                let width = km / km_per_px;
                let at_edge = km <= CANDIDATES_M[0] / 1000.0 + 1e-12
                    || km >= CANDIDATES_M[CANDIDATES_M.len() - 1] / 1000.0 - 1e-12;
                assert!(
                    at_edge || (MIN_BAR_PX..=MAX_BAR_PX).contains(&width),
                    "km_per_px={km_per_px} 选 {km} km 条长 {width} px 越窗",
                );
            }
        }
    }
}
