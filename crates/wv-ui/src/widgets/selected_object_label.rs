//! 「Selected object label」组件——地图信息标签的**统一面板**。
//!
//! ## 规格来源（Figma `world view rs`）
//!
//! | 变体 | 节点 |
//! |---|---|
//! | 暗色 | `3:21165` |
//! | 浅色 | `3:21704` |
//!
//! 两档同名同构，只换配色。实测规格：
//!
//! - 容器：高 30 / 圆角 5 / 左右 padding 10 / **1px 金描边**；
//! - Selection indicator：5 px 金点，位于 padding 内、垂直居中；
//! - 文字：Inter 600 / 11 px / 行高 16，与指示点间距 8。
//!
//! ## 谁在用
//!
//! - **egui 侧**：本模块的 [`paint`] 直接可画（浮窗里的地图信息条）。
//! - **世界空间侧**（测量读数等 `Text2d` 标签）：渲染层**不持 UI 令牌源**
//!   （ADR-0020 §2.6 同「钻入标记」先例）——应用层每帧把本组件的**配色**
//!   灌进渲染侧资源，渲染只读（`wv-app::render_bridge`）。
//!
//! ## 边界
//!
//! 本模块只管**度量与配色**（纯数据）+ egui 绘制助手；世界空间的 mesh /
//! Text2d 绘制在渲染层，两边共用同一份度量与配色值。

use bevy_egui::egui;

use crate::ui_tokens::{Rgb, UiTheme};

/// 面板底不透明度（Figma `opacity` 0.949 ≈ 0.95）。
pub const PANEL_ALPHA: f32 = 0.95;

/// 组件度量（Figma 实测值）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectedObjectLabelMetrics {
    /// 面板高（px）。
    pub height: f32,
    /// 圆角半径（px）。
    pub radius: f32,
    /// 左右内边距（px）。
    pub padding_h: f32,
    /// 指示点与文字的间距（px）。
    pub gap: f32,
    /// 指示点直径（px）。
    pub indicator: f32,
    /// 文字字号（px）。
    pub font_size: f32,
    /// 描边宽（px）。
    pub border: f32,
}

impl Default for SelectedObjectLabelMetrics {
    fn default() -> Self {
        Self {
            height: 30.0,
            radius: 5.0,
            padding_h: 10.0,
            gap: 8.0,
            indicator: 5.0,
            font_size: 11.0,
            border: 1.0,
        }
    }
}

/// 组件配色（日 / 夜两档，Figma 同名组件的两份配色）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectedObjectLabelColors {
    /// 面板底色（叠 [`PANEL_ALPHA`]）。
    pub panel: Rgb,
    /// 面板描边色（金）。
    pub border: Rgb,
    /// 指示点色（金）。
    pub indicator: Rgb,
    /// 文字色。
    pub text: Rgb,
}

impl SelectedObjectLabelColors {
    /// 按外壳明暗取配色。
    ///
    /// 暗色 = Figma `3:21165`（面板 `#17252D` / 描边 `#D6B87A` / 文字 `#E3E8EB`）；
    /// 浅色 = `3:21704`（面板 `#F3EDDE` / 描边 `#D6B87A` / 文字 `#303B3B`）。
    pub fn for_theme(theme: UiTheme) -> Self {
        match theme {
            UiTheme::Dark => Self {
                panel: Rgb::hex(0x0017_252D),
                border: Rgb::hex(0x00D6_B87A),
                indicator: Rgb::hex(0x00D6_B87A),
                text: Rgb::hex(0x00E3_E8EB),
            },
            UiTheme::Light => Self {
                panel: Rgb::hex(0x00F3_EDDE),
                border: Rgb::hex(0x00D6_B87A),
                indicator: Rgb::hex(0x00D6_B87A),
                text: Rgb::hex(0x0030_3B3B),
            },
        }
    }
}

/// 在 egui 里画一个「Selected object label」面板。
///
/// `rect` 是面板的外框（调用方给定尺寸）；文字左对齐排在指示点之后
/// （Figma 布局：padding → 指示点 → 间距 → 文字）。
pub fn paint(
    ui: &egui::Ui,
    rect: egui::Rect,
    colors: &SelectedObjectLabelColors,
    metrics: &SelectedObjectLabelMetrics,
    text: &str,
) {
    let p = ui.painter();
    p.rect_filled(
        rect,
        metrics.radius,
        colors.panel.to_color32_alpha(PANEL_ALPHA),
    );
    p.rect_stroke(
        rect,
        metrics.radius,
        egui::Stroke::new(metrics.border, colors.border.to_color32()),
        egui::StrokeKind::Inside,
    );
    let dot_c = egui::pos2(
        rect.min.x + metrics.padding_h + metrics.indicator / 2.0,
        rect.center().y,
    );
    p.circle_filled(
        dot_c,
        metrics.indicator / 2.0,
        colors.indicator.to_color32(),
    );
    p.text(
        egui::pos2(
            rect.min.x + metrics.padding_h + metrics.indicator + metrics.gap,
            rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(metrics.font_size),
        colors.text.to_color32(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日 / 夜两档必须**不同**（Figma 是两份配色，不是一份加滤镜）。
    #[test]
    fn day_and_night_colors_differ() {
        let dark = SelectedObjectLabelColors::for_theme(UiTheme::Dark);
        let light = SelectedObjectLabelColors::for_theme(UiTheme::Light);
        assert_ne!(dark.panel, light.panel, "面板底色应随明暗切换");
        assert_ne!(dark.text, light.text, "文字色应随明暗切换");
        // 金描边 / 金指示点两档同色（Figma 两份设计一致）。
        assert_eq!(dark.border, light.border, "描边金两档一致");
        assert_eq!(dark.indicator, light.indicator, "指示点金两档一致");
    }

    /// 度量必须与 Figma 实测值一致（改这里等于改设计）。
    #[test]
    fn metrics_match_figma_spec() {
        let m = SelectedObjectLabelMetrics::default();
        assert_eq!(m.height, 30.0);
        assert_eq!(m.radius, 5.0);
        assert_eq!(m.padding_h, 10.0);
        assert_eq!(m.gap, 8.0);
        assert_eq!(m.indicator, 5.0);
        assert_eq!(m.font_size, 11.0);
        assert_eq!(m.border, 1.0);
    }

    /// 配色值与 Figma 节点一致（暗 `#17252D` / 浅 `#F3EDDE`，金 `#D6B87A`）。
    #[test]
    fn colors_match_figma_nodes() {
        let dark = SelectedObjectLabelColors::for_theme(UiTheme::Dark);
        assert_eq!(dark.panel, Rgb::hex(0x0017_252D), "暗色面板底 #17252D");
        assert_eq!(dark.text, Rgb::hex(0x00E3_E8EB), "暗色文字 #E3E8EB");
        let light = SelectedObjectLabelColors::for_theme(UiTheme::Light);
        assert_eq!(light.panel, Rgb::hex(0x00F3_EDDE), "浅色面板底 #F3EDDE");
        assert_eq!(light.text, Rgb::hex(0x0030_3B3B), "浅色文字 #303B3B");
        assert_eq!(light.border, Rgb::hex(0x00D6_B87A), "金描边 #D6B87A");
    }
}
