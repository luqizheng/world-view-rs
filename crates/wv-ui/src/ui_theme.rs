//! 界面主题装配 —— 把 [`crate::ui_tokens`] 的令牌投影成 egui 的 `Style` / `Visuals`。
//!
//! ## 职责
//!
//! - 按当前 [`UiShellConfig`] 生成整套 egui 样式（[`crate::shell::UiShell`] 读它）。
//! - 令牌值**只来自** [`crate::ui_tokens`]，本文件不出现任何界面色值常量
//!   （设计规格 §2.2：色值唯一权威是令牌表）。唯一例外是**阴影墨**——
//!   半透明黑对明暗两套都成立，令牌表没有「阴影」语义槽位，故提为
//!   `SHADOW_INK_*` 具名常量（§2.5 的 e1 / e2 高度）。
//! - `ui.bg.map` 的注入：地图底衬色交给 Bevy 清屏色，不在 egui 里画。
//!
//! ## 为什么这里不直接 `egui::Context::set_visuals`
//!
//! bevy_egui 每帧都可能重建/共享上下文，直接改全局 `Visuals` 会与
//! egui 默认样式来回打架。我们把样式算成一个 `Style` 值，由
//! wv-app 在 `UiShellConfig` 变化时调用 [`apply_ui_theme`] 幂等写入——
//! 切换明暗时只是换了令牌来源，幂等写入天然支持热切换（规格 §「双缓冲热交换」的简化版）。
//!
//! ## 为什么必须同时写 `Style::spacing` / `text_styles`
//!
//! 只写 `Visuals` 只能改**颜色**；egui 的控件高度、内边距、行距、字号阶梯
//! 全部住在 `Style` 的另外两个字段里。只改颜色不改它们，等于把令牌表的一半
//! 扔掉：`UiDensity` 对原生控件失效、字号阶梯只对显式 `RichText::size` 生效、
//! 控件高度停留在 egui 默认的 18px（§4.3 要求 24/28/32）。
//!
//! ## 文字层级为什么不设 `override_text_color`
//!
//! egui 的 `visuals.text_color()` 优先返回 `override_text_color`，且所有 widget
//! 的文本都吃它。一旦设成 `text_1`，`widgets.*.fg_stroke` 的四态配置全部变成
//! 死配置，三级文字（t1/t2/t3）与禁用态在同一色上塌平。故此处**保持 `None`**，
//! 层级由各态 `fg_stroke` 与面板侧的 `RichText::color(t2/t3)` 承担。

use bevy::prelude::*;
use bevy_egui::egui;

use crate::ui_tokens::{font_size, motion, radius, space, Rgb, ShellPalette, UiDensity, UiTheme};

/// 弹层阴影墨的不透明度（§2.5 e2：`0 4px 12px`）。
///
/// 阴影不是界面色，不占令牌槽位；两套主题共用同一墨色，仅底色不同。
const SHADOW_INK_POPUP: u8 = 90;
/// 浮动面板阴影墨的不透明度（§2.5 e1：`0 1px 2px`）。
const SHADOW_INK_WINDOW: u8 = 107;

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
///
/// 基底随主题选 `Visuals::dark()` / `Visuals::light()`：基底决定未显式覆写槽位
/// （超链接色、文本颜色变换等）的默认值。此前一律从暗色基底起步，亮色主题会
/// 残留暗色默认值。
///
/// **四态 + open 全部显式配置**：按钮 / 下拉 / 折叠头的底色读 `weak_bg_fill`
/// （egui `WidgetStyle` 的 `frame.fill`），只设 `bg_fill` 会让这些控件继续用
/// egui 默认灰，令牌等于没接上。
pub fn build_visuals(palette: ShellPalette) -> egui::Visuals {
    let mut v = if palette.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    // ── 底色层级：app < panel < raised < hover < active ──────────────────
    v.panel_fill = color32(palette.bg_panel);
    v.window_fill = color32(palette.bg_panel);
    v.extreme_bg_color = color32(palette.bg_input);
    v.faint_bg_color = color32(palette.bg_raised);
    v.code_bg_color = color32(palette.bg_raised);
    // 面板描边：§2.4「1px ui.border.strong」。
    v.window_stroke = egui::Stroke::new(1.0_f32, color32(palette.border_strong));

    // ── 选中态 ───────────────────────────────────────────────────────────
    // `selection.stroke` 在 egui 里是**选中文字色**（`paint_text_selection` 用它
    // 重绘选中字形，`button_style` 也用它当 selected 态前景），**不是**焦点环。
    //
    // 底色取 `accent_soft`（令牌定义即「accent 的低饱和抬升底，用于按钮 hover /
    // 选中段背景」）而不是 accent 实心：accent 实心在亮色下压白字仅 4.36:1，
    // 低于 §4.1 P1 的正文门槛。软底 + text_1 明暗分别 7.9:1 / 8.9:1。
    v.selection.bg_fill = color32(palette.accent_soft);
    v.selection.stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));

    // ── 文字 ─────────────────────────────────────────────────────────────
    // 不设全局覆盖：层级由下面四态 `fg_stroke` 与面板侧显式 `RichText::color` 承担。
    v.override_text_color = None;
    // `weak()` 文本走 text_3，与面板手写的弱文本同色。
    v.weak_text_color = Some(color32(palette.text_3));
    v.hyperlink_color = color32(palette.accent);
    v.warn_fg_color = color32(palette.badge_warn);
    v.error_fg_color = color32(palette.badge_err);

    // ── 控件四态 ─────────────────────────────────────────────────────────
    let control_radius = egui::CornerRadius::same(radius::CONTROL as u8);
    let subtle = egui::Stroke::new(1.0_f32, color32(palette.border_subtle));
    let strong = egui::Stroke::new(1.0_f32, color32(palette.border_strong));

    // 不可交互：`fg_stroke` 在 egui 里就是**正常文字色**（不是禁用色），
    // 故取 text_1；禁用态由 `disabled_alpha` 单独压暗（§4.4 的 0.4）。
    v.widgets.noninteractive.bg_fill = color32(palette.bg_panel);
    v.widgets.noninteractive.weak_bg_fill = color32(palette.bg_raised);
    v.widgets.noninteractive.bg_stroke = subtle;
    v.widgets.noninteractive.corner_radius = control_radius;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));
    v.widgets.noninteractive.expansion = 0.0;

    v.widgets.inactive.bg_fill = color32(palette.bg_raised);
    v.widgets.inactive.weak_bg_fill = color32(palette.bg_raised);
    v.widgets.inactive.bg_stroke = strong;
    v.widgets.inactive.corner_radius = control_radius;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));
    v.widgets.inactive.expansion = 0.0;

    // hover：底色 +6% 明度（§4.4），**不加阴影、不长大**（expansion = 0）。
    v.widgets.hovered.bg_fill = color32(palette.bg_hover);
    v.widgets.hovered.weak_bg_fill = color32(palette.bg_hover);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, color32(palette.accent));
    v.widgets.hovered.corner_radius = control_radius;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));
    v.widgets.hovered.expansion = 0.0;

    // pressed / selected：底色 −8% 明度（§4.4），描边加粗到 1.5px 做按下暗示。
    v.widgets.active.bg_fill = color32(palette.bg_active);
    v.widgets.active.weak_bg_fill = color32(palette.bg_active);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.5_f32, color32(palette.accent));
    v.widgets.active.corner_radius = control_radius;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, color32(palette.text_1));
    v.widgets.active.expansion = 0.0;

    // 展开态（combo 展开、菜单按钮按下）：与 hover 同观感。
    v.widgets.open = v.widgets.hovered;

    // ── 圆角阶梯 ─────────────────────────────────────────────────────────
    v.window_corner_radius = egui::CornerRadius::same(radius::PANEL as u8);
    v.menu_corner_radius = egui::CornerRadius::same(radius::CONTROL as u8);

    // ── 光标 / 阴影 ──────────────────────────────────────────────────────
    // 文本光标默认是 egui 的亮蓝 #C0DEFF，不属于任何一套壳配色。
    v.text_cursor.stroke = egui::Stroke::new(1.5_f32, color32(palette.accent));
    // 禁用态整体 alpha（§4.4：0.4；egui 默认 0.5）。
    v.disabled_alpha = 0.4;

    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 12,
        spread: 0,
        color: egui::Color32::from_black_alpha(SHADOW_INK_POPUP),
    };
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 1],
        blur: 2,
        spread: 0,
        color: egui::Color32::from_black_alpha(SHADOW_INK_WINDOW),
    };

    v
}

/// 由配置生成整套 egui `Style`（视觉 + 间距 + 字号阶梯）。
///
/// 分开暴露（而不是只给 `apply_ui_theme` 用）是为了让单测能直接断言
/// 「令牌 → egui 字段」的映射，不必起 GUI。
pub fn build_style(cfg: &UiShellConfig) -> egui::Style {
    let mut style = egui::Style {
        visuals: build_visuals(cfg.palette()),
        // 动效：hover/pressed 用 fast，面板折叠用 base（§2.6）。egui 只有一个全局值，
        // 取 base（0.18s）——比默认 0.2s 略快，且与令牌表同源。
        animation_time: motion::BASE,
        ..egui::Style::default()
    };

    // 间距：4px 基准栅格（§2.4）。密度档**只改命中区高度**，不改间距刻度
    // （§8 的顶栏/命中区/面板头/树行四值），故这里只有 `interact_size.y` 随档变化。
    style.spacing.item_spacing = egui::vec2(space::SM, space::XS);
    style.spacing.button_padding = egui::vec2(space::SM, space::XS);
    style.spacing.window_margin = margin_all(space::SM);
    style.spacing.menu_margin = margin_all(space::SM);
    style.spacing.interact_size = egui::vec2(40.0, cfg.density.hit());
    // 树缩进 = 图标宽 14 + 间隙 4（egui 的 checkbox/radio 对齐基准，无令牌槽位）。
    style.spacing.indent = 18.0;

    // 字号阶梯（§2.3 七级）：egui 自带的命名档位映射到令牌值。
    // - Small → micro（徽标/弱标签，11）
    // - Body  → base（默认正文，13）
    // - Button→ small（控件文字，12）
    // - Heading → title（空状态/分组标题，16）
    // - Monospace → base，且**必须是等宽族**（§4.2 硬约束 1：数字等宽）。
    style.text_styles = [
        (egui::TextStyle::Small, proportional(font_size::MICRO)),
        (egui::TextStyle::Body, proportional(font_size::BASE)),
        (egui::TextStyle::Button, proportional(font_size::SMALL)),
        (egui::TextStyle::Heading, proportional(font_size::TITLE)),
        (egui::TextStyle::Monospace, mono(font_size::BASE)),
    ]
    .into();

    style
}

/// 把当前主题写入 egui 上下文（**幂等**，只在 `UiShellConfig` 变化时调用）。
///
/// 做三件事：
/// 1. `ctx.set_theme(...)` —— 让 egui 自己知道当前是暗/亮，后续新建的菜单、
///    弹窗、滚动条、输入框等系统控件都按对应风格绘制；
/// 2. `ctx.set_style_of(...)` —— 把**壳层令牌**投影成整套 `Style`
///    （视觉 + 间距 + 字号阶梯），覆盖 egui 默认值；
/// 3. 两套主题都写：`set_style_of` 是按主题归档的，这样任何 `style_of(Theme::X)`
///    取值处都不会拿到 egui 默认样式（避免亮色模式下某个回调仍按暗色绘制）。
pub fn apply_ui_theme(ctx: &egui::Context, cfg: &UiShellConfig) {
    ctx.set_theme(cfg.theme.egui_theme());
    for theme in [UiTheme::Dark, UiTheme::Light] {
        let mut themed = *cfg;
        themed.theme = theme;
        ctx.set_style_of(theme.egui_theme(), build_style(&themed));
    }
}

/// 由密度档生成 egui 字体尺寸阶梯（§2.3：7 级）。
///
/// egui 的字体缩放是整体的，这里把 7 级阶梯映射到 egui 的
/// `FontId::proportional/monospace` 尺寸，供 UI 组件按语义取用。
///
/// `_density` 形参保留：字号阶梯**不随密度变化**（§8：密度只改行高/命中区），
/// 但调用点（`UiShell::new`）按「配置 → 全部样式」一处取用，签名稳定省一次改面。
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

    /// 把 egui 颜色转回令牌层的 `Rgb`，复用 [`Rgb::contrast_ratio`] 做验收。
    fn rgb(c: egui::Color32) -> Rgb {
        Rgb {
            r: c.r() as f32 / 255.0,
            g: c.g() as f32 / 255.0,
            b: c.b() as f32 / 255.0,
        }
    }

    fn cozy(theme: UiTheme) -> UiShellConfig {
        UiShellConfig {
            theme,
            density: UiDensity::Cozy,
        }
    }

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
            ShellPalette::LIGHT.text_1.to_color32()
        );
        assert_ne!(
            light.selection.stroke.color,
            ShellPalette::DARK.focus.to_color32(),
            "亮色选中文字不得复用暗色金色（白底仅 1.62:1）"
        );
    }

    #[test]
    fn visuals_start_from_the_base_matching_the_palette() {
        // 基底决定未覆写槽位（超链接 / 文本变换）的默认值，不能一律暗色起步。
        assert!(build_visuals(ShellPalette::DARK).dark_mode);
        assert!(!build_visuals(ShellPalette::LIGHT).dark_mode);
    }

    /// 选中文字必须压在 accent 底上可读——egui 用它重绘选中字形
    /// （`paint_text_selection`）与 selected 态按钮文字（`button_style`）。
    /// 本项目的焦点色与 accent 同值，拿焦点色当选中文字色会让文字消失。
    #[test]
    fn selected_text_is_readable_over_the_selection_fill() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            let v = build_visuals(p);
            let ratio = rgb(v.selection.stroke.color).contrast_ratio(rgb(v.selection.bg_fill));
            assert!(
                ratio >= 4.5,
                "{name}: 选中文字 {ratio:.2}:1（需 ≥4.5），色 {:?} 压 {:?}",
                v.selection.stroke.color,
                v.selection.bg_fill
            );
        }
    }

    /// `noninteractive.fg_stroke` 是 egui 的「正常文字色」，不是禁用色。
    #[test]
    fn labels_use_primary_text_not_the_disabled_tone() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            let v = build_visuals(p);
            assert_eq!(
                v.widgets.noninteractive.fg_stroke.color,
                color32(p.text_1),
                "{name}: label 文字取 text_1"
            );
            assert_ne!(
                v.widgets.noninteractive.fg_stroke.color,
                color32(p.text_disabled),
                "{name}: label 不得取禁用色"
            );
            let ratio = rgb(v.widgets.noninteractive.fg_stroke.color)
                .contrast_ratio(rgb(v.widgets.noninteractive.bg_fill));
            assert!(ratio >= 7.0, "{name}: 正文对比 {ratio:.2}:1（需 ≥7）");
            assert_eq!(
                v.widgets.noninteractive.expansion, 0.0,
                "{name}: 不随状态长大"
            );
        }
    }

    /// 全局文字覆盖会把三级文字与四态文字塌成一种颜色。
    #[test]
    fn text_hierarchy_is_not_flattened_by_a_global_override() {
        assert!(build_visuals(ShellPalette::DARK)
            .override_text_color
            .is_none());
        assert!(build_visuals(ShellPalette::LIGHT)
            .override_text_color
            .is_none());
    }

    /// 按钮 / 下拉 / 折叠头的底色读 `weak_bg_fill`——它必须来自令牌，
    /// 只设 `bg_fill` 会让这些控件继续用 egui 默认灰。
    #[test]
    fn widget_fills_come_from_the_shell_palette() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            let v = build_visuals(p);
            assert_eq!(
                v.widgets.inactive.weak_bg_fill,
                color32(p.bg_raised),
                "{name}/inactive"
            );
            assert_eq!(
                v.widgets.inactive.bg_fill,
                color32(p.bg_raised),
                "{name}/inactive bg"
            );
            assert_eq!(
                v.widgets.hovered.weak_bg_fill,
                color32(p.bg_hover),
                "{name}/hovered"
            );
            assert_eq!(
                v.widgets.active.weak_bg_fill,
                color32(p.bg_active),
                "{name}/active"
            );
            assert_eq!(
                v.widgets.open.weak_bg_fill,
                color32(p.bg_hover),
                "{name}/open"
            );
        }
    }

    /// 圆角阶梯：控件 6px（§2.4），不得停在 egui 默认的 2px。
    #[test]
    fn control_corner_radius_matches_the_token_ladder() {
        let v = build_visuals(ShellPalette::DARK);
        for (state, w) in [
            ("noninteractive", v.widgets.noninteractive),
            ("inactive", v.widgets.inactive),
            ("hovered", v.widgets.hovered),
            ("active", v.widgets.active),
        ] {
            assert_eq!(
                w.corner_radius,
                egui::CornerRadius::same(radius::CONTROL as u8),
                "{state} 圆角应为 CONTROL"
            );
        }
        assert_eq!(
            v.window_corner_radius,
            egui::CornerRadius::same(radius::PANEL as u8)
        );
    }

    /// hover / pressed 不得改变控件尺寸（egui 默认 expansion=1 会外扩 1px）。
    #[test]
    fn hover_and_press_do_not_resize_widgets() {
        let v = build_visuals(ShellPalette::DARK);
        assert_eq!(v.widgets.hovered.expansion, 0.0);
        assert_eq!(v.widgets.active.expansion, 0.0);
    }

    /// 文本光标默认是 egui 的亮蓝，必须换成壳层 accent。
    #[test]
    fn text_cursor_uses_the_shell_accent_not_egui_blue() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            let v = build_visuals(p);
            assert_eq!(v.text_cursor.stroke.color, color32(p.accent), "{name}");
        }
    }

    #[test]
    fn spacing_follows_the_four_px_grid_and_density_heights() {
        let cozy = build_style(&cozy(UiTheme::Dark));
        assert_eq!(cozy.spacing.interact_size.y, 28.0, "舒适档命中区 28px");
        assert_eq!(cozy.spacing.button_padding, egui::vec2(8.0, 4.0));
        assert_eq!(cozy.spacing.item_spacing, egui::vec2(8.0, 4.0));
        assert_eq!(cozy.spacing.window_margin, egui::Margin::same(8));

        let compact = build_style(&UiShellConfig {
            theme: UiTheme::Dark,
            density: UiDensity::Compact,
        });
        assert_eq!(compact.spacing.interact_size.y, 24.0, "紧凑档命中区 24px");
        assert_eq!(
            compact.spacing.button_padding, cozy.spacing.button_padding,
            "密度只改高度，不改间距刻度（§8）"
        );
    }

    /// 字号阶梯必须接进 egui 的命名档位，否则 `ui.heading()` 仍是 18px、
    /// `TextStyle::Small` 仍是 9px（中文不可读，§2.3 硬约束 3）。
    #[test]
    fn text_styles_follow_the_seven_step_ladder() {
        let style = build_style(&cozy(UiTheme::Dark));
        let size = |t: egui::TextStyle| style.text_styles[&t].size;
        assert_eq!(size(egui::TextStyle::Small), 11.0);
        assert_eq!(size(egui::TextStyle::Body), 13.0);
        assert_eq!(size(egui::TextStyle::Button), 12.0);
        assert_eq!(size(egui::TextStyle::Heading), 16.0);
        assert_eq!(
            style.text_styles[&egui::TextStyle::Monospace].family,
            egui::FontFamily::Monospace,
            "数字必须等宽（§4.2 硬约束 1）"
        );
    }

    #[test]
    fn apply_ui_theme_drives_context_theme_and_shell_visuals() {
        let ctx = egui::Context::default();
        apply_ui_theme(&ctx, &cozy(UiTheme::Light));
        assert_eq!(ctx.theme(), egui::Theme::Light);
        assert_eq!(
            ctx.style_of(egui::Theme::Light).visuals.panel_fill,
            ShellPalette::LIGHT.bg_panel.to_color32(),
            "亮色上下文的面板底必须来自 LIGHT 令牌"
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals.panel_fill,
            ShellPalette::DARK.bg_panel.to_color32(),
            "两套样式都要写入，避免任何 style_of 取值处拿到 egui 默认色"
        );
        assert_eq!(
            ctx.style_of(egui::Theme::Light).spacing.interact_size.y,
            28.0,
            "间距/字号也必须按主题写入"
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
