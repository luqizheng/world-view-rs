//! 界面主题装配 —— 把 [`crate::ui_tokens`] 的令牌投影成 egui 的 `Visuals`。

//! ## 职责
//!
//! - 按当前 [`UiTheme`] 生成整套 egui 视觉样式（[`crate::shell::UiShell`] 读它）。
//! - 令牌值**只来自** [`crate::ui_tokens`]，本文件不出现任何色值常量
//!   （设计规格 §2.2：色值唯一权威是令牌表）。
//! - `ui.bg.map` 的注入：地图底衬色交给 Bevy 清屏色，不在 egui 里画。
//!
//! ## 为什么这里不直接 `egui::Context::set_visuals`
//!
//! bevy_egui 每帧都可能重建/共享上下文，直接改全局 `Visuals` 会与
//! egui 默认样式来回打架。我们把样式算成一个 `Visuals` 值，由
//! wv-app 每帧 `ctx.set_visuals(...)` 幂等写入——切换明暗时
//! 只是换了令牌来源，幂等写入天然支持热切换（规格 §「双缓冲热交换」的简化版）。

use bevy::prelude::*;
use bevy_egui::egui;

use crate::ui_tokens::{font_size, radius, Rgb, ShellPalette, UiDensity, UiTheme};

/// 界面主题状态：明暗 + 密度。
///
/// 作为独立 Resource 而不是塞进 `ViewState`：
/// 它是**界面自身**的状态，与世界视图无关（D2 世界色归世界、界面色归界面）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiShellConfig {
    /// 明暗主题。
    pub theme: UiTheme,
    /// 密度档。
    pub density: UiDensity,
}

impl Default for UiShellConfig {
    fn default() -> Self {
        Self {
            theme: UiTheme::Dark,
            density: UiDensity::Cozy,
        }
    }
}

impl UiShellConfig {
    /// 当前主题的令牌集。
    pub fn palette(&self) -> ShellPalette {
        ShellPalette::for_theme(self.theme)
    }
}

/// 把 [`Rgb`] 转成 egui 颜色。
pub fn color32(c: Rgb) -> egui::Color32 {
    c.to_color32()
}

/// 带不透明度的 egui 颜色。
pub fn color32_alpha(c: Rgb, alpha: f32) -> egui::Color32 {
    c.to_color32_alpha(alpha)
}

/// 令牌是 `f32`，而 egui 0.36 的 `Margin` 用 `i8` 像素——内边距换算统一在这里收敛。
pub fn margin_all(v: f32) -> egui::Margin {
    egui::Margin::same(v.round() as i8)
}

/// 横纵内边距（同上）。
pub fn margin_xy(x: f32, y: f32) -> egui::Margin {
    egui::Margin::symmetric(x.round() as i8, y.round() as i8)
}
/// 由令牌生成 egui `Visuals`。
///
/// 只配置**界面外壳**会用到的槽位；地图、世界高亮一律不经过 egui
/// （D2：世界色归世界）。
pub fn build_visuals(palette: ShellPalette) -> egui::Visuals {
    let mut v = egui::Visuals::dark();

    // 底色层级：app < panel < raised < hover < active。
    v.panel_fill = color32(palette.bg_panel);
    v.window_fill = color32(palette.bg_panel);
    v.extreme_bg_color = color32(palette.bg_app);
    v.faint_bg_color = color32(palette.bg_raised);
    v.code_bg_color = color32(palette.bg_raised);
    v.selection.bg_fill = color32(palette.accent);
    v.selection.stroke = egui::Stroke::new(1.0_f32, color32(palette.accent));

    // 文字三级。
    v.override_text_color = Some(color32(palette.text_1));
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_2));
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));

    // 抬升块（输入框 / 列表项 / 按钮）。
    v.widgets.noninteractive.bg_fill = color32(palette.bg_raised);
    v.widgets.inactive.bg_fill = color32(palette.bg_raised);
    v.widgets.hovered.bg_fill = color32(palette.bg_hover);
    v.widgets.active.bg_fill = color32(palette.bg_active);

    // 描边：面板内分隔线用 subtle，控件描边用 strong。
    let subtle = egui::Stroke::new(1.0_f32, color32(palette.border_subtle));
    let strong = egui::Stroke::new(1.0_f32, color32(palette.border_strong));
    v.widgets.noninteractive.bg_stroke = subtle;
    v.widgets.inactive.bg_stroke = strong;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, color32(palette.accent));
    v.widgets.active.bg_stroke = egui::Stroke::new(1.5_f32, color32(palette.accent));

    // 圆角阶梯。
    v.window_corner_radius = egui::CornerRadius::from(radius::PANEL);
    v.menu_corner_radius = egui::CornerRadius::from(radius::CONTROL);

    // 焦点环：亮色下必须是深琥珀而非金色（§7）。
    v.selection.stroke = egui::Stroke::new(1.0_f32, color32(palette.focus));
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: egui::Color32::from_black_alpha(90),
    };
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 1],
        blur: 2,
        spread: 0,
        color: egui::Color32::from_black_alpha(107),
    };

    v
}

/// 由密度档生成 egui 字体尺寸阶梯（§2.3：7 级）。
///
/// egui 的字体缩放是整体的，这里把 7 级阶梯映射到 egui 的
/// `FontId::proportional/monospace` 尺寸，供 UI 组件按语义取用。
pub fn font_sizes(_density: UiDensity) -> FontScale {
    // 密度不改字号，只改行高/命中区（§8）：字号阶梯是固定的。
    FontScale {
        micro: font_size::MICRO,
        small: font_size::SMALL,
        base: font_size::BASE,
        input: font_size::INPUT,
        title: font_size::TITLE,
        subtitle: font_size::SUBTITLE,
        display: font_size::DISPLAY,
    }
}

/// 字号阶梯（§2.3）。
#[derive(Debug, Clone, Copy)]
pub struct FontScale {
    /// 面板标题 / 徽标 / 弱标签。
    pub micro: f32,
    /// 正文 / 控件。
    pub small: f32,
    /// 默认。
    pub base: f32,
    /// 输入框 / 命令面板输入。
    pub input: f32,
    /// 空状态标题。
    pub title: f32,
    /// 次级标题。
    pub subtitle: f32,
    /// 大标题（预留）。
    pub display: f32,
}

/// 界面数字统一用等宽字体（§2.3：所有会跳动的数字必须等宽）。
pub fn mono(size: f32) -> egui::FontId {
    egui::FontId::monospace(size)
}

/// 界面文字统一用比例字体。
pub fn proportional(size: f32) -> egui::FontId {
    egui::FontId::proportional(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visuals_panel_fill_tracks_palette_panel_color() {
        let p = ShellPalette::DARK;
        let v = build_visuals(p);
        assert_eq!(v.panel_fill, p.bg_panel.to_color32());
        assert_eq!(v.window_fill, p.bg_panel.to_color32());
    }

    #[test]
    fn light_theme_focus_stroke_is_dark_amber_not_gold() {
        let light = build_visuals(ShellPalette::LIGHT);
        assert_eq!(
            light.selection.stroke.color,
            ShellPalette::LIGHT.focus.to_color32()
        );
        assert_ne!(
            light.selection.stroke.color,
            ShellPalette::DARK.focus.to_color32(),
            "亮色焦点环不得复用暗色金色（白底仅 1.62:1）"
        );
    }

    #[test]
    fn font_scale_matches_seven_step_ladder() {
        let s = font_sizes(UiDensity::Cozy);
        assert_eq!(s.micro, 11.0);
        assert_eq!(s.small, 12.0);
        assert_eq!(s.base, 13.0);
        assert_eq!(s.input, 14.0);
        assert_eq!(s.title, 16.0);
        assert_eq!(s.subtitle, 19.0);
        assert_eq!(s.display, 24.0);
    }

    #[test]
    fn density_does_not_change_font_scale() {
        // §8：密度只改行高/命中区，字号阶梯固定。
        let a = font_sizes(UiDensity::Cozy);
        let b = font_sizes(UiDensity::Compact);
        assert_eq!(a.base, b.base);
    }
}
