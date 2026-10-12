//! 界面外壳色令牌 —— `assets/ui-design/UI_DESIGN_SPEC_v0.1.md` §2.2 的唯一代码载体。
#![cfg(feature = "app")]

//! ## 纪律
//!
//! - **色彩唯一权威源是 UI 设计规格 §2.2 的令牌表**，其中色值由
//!   `assets/ui-design/audit-contrast.py` 实测标定。本文件是它的**投影**，
//!   不是第二权威——若两者冲突，改本文件。
//! - **禁止**在 UI 代码里写裸十六进制色值：一律经本模块的令牌引用。
//! - **`ui.*` 与 `world.*` 语义隔离**（设计规格 §2.1）：界面外壳色**不得**
//!   从 [`crate::palette`]（世界色板）借色相。世界色归地图，界面色归界面。
//!   唯一例外是 [`ShellPalette::focus`] 的暗色值复用 `world.accent.select`（金），
//!   因为「选中」语义本来就是世界状态（§4.4）。
//!
//! ## 为什么是 12 色 × 明暗双值
//!
//! 明暗两套外壳**不是色相反转**（§7）：亮色的焦点环、浮层不透明度都单独标定过
//! （金 `#F2C14E` 在白底仅 1.62:1，绝不能直接当亮色焦点环）。因此令牌以
//! [`UiTheme`] 为索引成对持有，运行时由主题系统选择。

/// 界面明暗主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiTheme {
    /// 暗色外壳（默认）。
    #[default]
    Dark,
    /// 亮色外壳。
    Light,
}

impl UiTheme {
    /// 切换明暗（`Ctrl+J`）。
    pub fn toggled(self) -> Self {
        match self {
            UiTheme::Dark => UiTheme::Light,
            UiTheme::Light => UiTheme::Dark,
        }
    }
}

/// 界面密度档（设计规格 §8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiDensity {
    /// 标准：顶栏 38px / 命中区 28px / 面板头 30px / 树行 24px。
    #[default]
    Cozy,
    /// 紧凑：顶栏 32px / 命中区 24px / 面板头 26px / 树行 20px。
    Compact,
}

impl UiDensity {
    /// 切换密度档。
    pub fn toggled(self) -> Self {
        match self {
            UiDensity::Cozy => UiDensity::Compact,
            UiDensity::Compact => UiDensity::Cozy,
        }
    }

    /// 顶栏高度（px）。
    pub fn top_bar_height(self) -> f32 {
        match self {
            UiDensity::Cozy => 38.0,
            UiDensity::Compact => 32.0,
        }
    }

    /// 命中区最小边长（px）。
    pub fn hit(self) -> f32 {
        match self {
            UiDensity::Cozy => 28.0,
            UiDensity::Compact => 24.0,
        }
    }

    /// 面板标题栏高度（px）。
    pub fn panel_head(self) -> f32 {
        match self {
            UiDensity::Cozy => 30.0,
            UiDensity::Compact => 26.0,
        }
    }

    /// 列表 / 树行高（px）。
    pub fn row(self) -> f32 {
        match self {
            UiDensity::Cozy => 24.0,
            UiDensity::Compact => 20.0,
        }
    }
}

/// 间距阶梯（§2.4：4px 基准）。
pub mod space {
    /// 同组控件间距。
    pub const XS: f32 = 4.0;
    /// 面板内边距 / 常规间距。
    pub const SM: f32 = 8.0;
    /// 跨组控件间距。
    pub const MD: f32 = 12.0;
    /// 段落间距。
    pub const LG: f32 = 16.0;
    /// 区块间距。
    pub const XL: f32 = 20.0;
    /// 大区块间距。
    pub const XXL: f32 = 24.0;
}

/// 圆角阶梯（§2.4）。
pub mod radius {
    /// 徽标 / 色块。
    pub const BADGE: f32 = 4.0;
    /// 输入框 / 按钮。
    pub const CONTROL: f32 = 6.0;
    /// 浮动面板。
    pub const PANEL: f32 = 10.0;
    /// 模态 / 空状态卡。
    pub const MODAL: f32 = 14.0;
}

/// 字号阶梯（§2.3：7 级）。
pub mod font_size {
    /// 面板标题 / 徽标 / 弱标签。
    pub const MICRO: f32 = 11.0;
    /// 正文 / 控件。
    pub const SMALL: f32 = 12.0;
    /// 默认。
    pub const BASE: f32 = 13.0;
    /// 输入框 / 命令面板输入。
    pub const INPUT: f32 = 14.0;
    /// 空状态标题。
    pub const TITLE: f32 = 16.0;
    /// 次级标题。
    pub const SUBTITLE: f32 = 19.0;
    /// 大标题（预留）。
    pub const DISPLAY: f32 = 24.0;
}

/// 界面动效时长（§2.6，单位秒；egui 动画以秒计）。
pub mod motion {
    /// hover / 按下 / 开关滑动。
    pub const FAST: f32 = 0.110;
    /// 面板折叠展开 / 进场 / Toast。
    pub const BASE: f32 = 0.180;
    /// 模态淡入、视野归位。
    pub const SLOW: f32 = 0.320;
}

/// 一个 sRGB 颜色（0..=1 每通道）。
///
/// 不直接用 `bevy::prelude::Color` / `egui::Color32` 承载令牌值：
/// 令牌层是**规格投影**，应当对渲染后端中立；两个后端的转换各自一行，
/// 放在使用处（[`crate::ui_theme`]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    /// 红通道 0..=1。
    pub r: f32,
    /// 绿通道 0..=1。
    pub g: f32,
    /// 蓝通道 0..=1。
    pub b: f32,
}

impl Rgb {
    /// 从 `0xRRGGBB` 构造。
    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
            g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
            b: (rgb & 0xFF) as f32 / 255.0,
        }
    }

    /// 转为 `egui::Color32`（不透明）。
    pub fn to_color32(self) -> bevy_egui::egui::Color32 {
        bevy_egui::egui::Color32::from_rgb(
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
        )
    }

    /// 转为带 alpha 的 `egui::Color32`（`alpha` 取 0..=1）。
    ///
    /// **必须走 `from_rgba_unmultiplied`**：`Color32` 是预乘格式，把不透明的
    /// `Color32` 的第四字节改小会得到「全强度 RGB + 低 alpha」的过亮结果，
    /// 与 CSS `rgba(r,g,b,.16)` 不是一回事。浮层底色（`bg.float`）与徽标底色
    /// 都靠这条与原型对齐。
    pub fn to_color32_alpha(self, alpha: f32) -> bevy_egui::egui::Color32 {
        bevy_egui::egui::Color32::from_rgba_unmultiplied(
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8,
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        )
    }

    /// 把本色以 `alpha` 合成在 `backdrop` 上（CSS `rgba` 的合成语义，8 位取整）。
    ///
    /// 存在的理由：§7 的门槛约束的是**合成后的实际底色**，不是色卡上的原始色值。
    /// 只看色卡算对比度会算错（§2.2.1 徽标底色、REVIEW_v0.1 的教训），
    /// 所以令牌层必须能自己算合成色，才能在无渲染器处做审计。
    pub fn over(self, alpha: f32, backdrop: Rgb) -> Rgb {
        let a = alpha.clamp(0.0, 1.0);
        let mix = |fg: f32, bg: f32| ((fg * a + bg * (1.0 - a)) * 255.0).round() / 255.0;
        Rgb {
            r: mix(self.r, backdrop.r),
            g: mix(self.g, backdrop.g),
            b: mix(self.b, backdrop.b),
        }
    }

    /// sRGB 相对亮度（WCAG 2.1 定义）。
    pub fn relative_luminance(self) -> f32 {
        fn channel(c: f32) -> f32 {
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// 与另一色的对比度（WCAG 2.1 定义，返回 1.0..=21.0）。
    ///
    /// 这是**唯一**的对比度判定入口：`assets/ui-design/audit-contrast.py` 用的是
    /// 同一算式，所以 Rust 侧可以复现原型实测出来的数字（见本文件测试）。
    pub fn contrast_ratio(self, other: Rgb) -> f32 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// 一套界面外壳令牌（§2.2 的 12 色 + 状态前景 + 文字on-accent + v0.2 面包屑色）。
///
/// 字段名与设计规格 §2.2 表格一一对应，顺序亦同，便于逐行核对。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellPalette {
    /// `ui.bg.app`：窗口底。
    pub bg_app: Rgb,
    /// `ui.bg.map`：地图底衬（比面板更暗，让地图「沉下去」）。
    pub bg_map: Rgb,
    /// `ui.bg.panel`：面板底。
    pub bg_panel: Rgb,
    /// `ui.bg.raised`：面板内抬升块（输入框 / 列表项 / 按钮）。
    pub bg_raised: Rgb,
    /// `ui.bg.hover`：hover 底。
    pub bg_hover: Rgb,
    /// `ui.bg.active`：pressed / 选中底。
    pub bg_active: Rgb,
    /// `ui.border.subtle`：面板内分隔线。
    pub border_subtle: Rgb,
    /// `ui.border.strong`：面板外描边 / 输入框。
    pub border_strong: Rgb,
    /// `ui.text.1`：主文本（暗 14.40:1 / 亮 16.12:1）。
    pub text_1: Rgb,
    /// `ui.text.2`：次文本（暗 7.37:1 / 亮 6.72:1）。
    pub text_2: Rgb,
    /// `ui.text.3`：弱文本（暗 5.10:1 / 亮 4.61:1）。
    pub text_3: Rgb,
    /// `ui.accent`：交互主色（暗 5.23:1 / 亮 4.70:1）。
    pub accent: Rgb,
    /// `ui.focus`：焦点环（暗 10.15:1 / 亮 5.73:1）。
    pub focus: Rgb,
    /// `ui.bg.float`：浮在地图上的层（HUD chip / 缩放栈）。
    bg_float: Rgb,
    /// 浮层不透明度（明暗分别标定：暗 .94 / 亮 .93）。
    float_alpha: f32,
    /// `.bdg.ok` 前景。
    pub badge_ok: Rgb,
    /// `.bdg.warn` 前景。
    pub badge_warn: Rgb,
    /// `.bdg.err` 前景。
    pub badge_err: Rgb,
    /// `.bdg.ev` 前景（事态徽标）。
    pub badge_event: Rgb,
    /// `ui.tx.on_accent`：accent 实心底上的文字。
    ///
    /// 不是白色：暗色 accent `#4C8DF6` 上的白字只有 3.26:1（§7 正文门槛 4.5），
    /// 必须用近黑的 `#0B0D11`（5.97:1）；亮色 accent 反过来才用白。
    pub text_on_accent: Rgb,
    /// `--crumb-text-3`：面包屑「上级」文字（v0.2 §B.3 新增令牌）。
    ///
    /// 它**不是** `text_3` 的别名：面包屑浮在 `bg.float` 上，而浮层合成后的底色
    /// 既不是 `bg.panel` 也不是 `bg.map`，所以这枚弱文本必须单独标定。
    pub crumb_text_3: Rgb,
}

/// 徽标的 5 种语义档（设计规格 §4.7）。
///
/// 与 [`crate::ui::BadgeKind`] 一一对应；这里再声明一次是为了让令牌层不必
/// 依赖 UI 组件层（令牌是规格投影，组件是它的使用者，方向不能反过来）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    /// 中性（已保存 / 计数）：无语义色，用抬升底 + 次文本色。
    Neutral,
    /// 通过 / 可达。
    Ok,
    /// 提示 / 迟到 / 未保存。
    Warn,
    /// 错误 / 不可达 / 悬空引用。
    Err,
    /// 事态。
    Event,
}

/// 一整套徽标配色：语义前景 + 底色（世界语义色 × 标定 alpha，§2.2.1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BadgeColors {
    /// 文字 / 图标色（`ui.*-fg`）——**明暗两套不同**，浅底上的浅色文字必然失效。
    pub fg: Rgb,
    /// 底色（世界语义色，与 `crate::palette` 的 `ui_link` 同值）。
    pub fill: Rgb,
    /// 底色 alpha（§2.2.1：ok / warn / ev 用 .16，err 用 .18）。
    pub fill_alpha: f32,
}

impl BadgeColors {
    /// 底色合成在实际背板上的结果——审计对比度时要拿它当底，不能用色卡原值。
    pub fn background_on(self, backdrop: Rgb) -> Rgb {
        self.fill.over(self.fill_alpha, backdrop)
    }
}

impl ShellPalette {
    /// 取某一档徽标的完整配色。
    ///
    /// 底色取自 `world.ui_link.*`（`#3F9B4E` 可达 / `#C9971F` 迟到 / `#B5453A`
    /// 不可达）与 `world.accent.event`（`#C08BD8`），**同色相**要求见 §4.7；
    /// 它们与 `crate::palette` 的一致性由本文件测试守住。
    pub fn badge(self, tone: BadgeTone) -> BadgeColors {
        let (fg, fill, fill_alpha) = match tone {
            BadgeTone::Neutral => (self.text_2, self.bg_active, 1.0),
            BadgeTone::Ok => (self.badge_ok, Rgb::hex(0x3F9B4E), 0.16),
            BadgeTone::Warn => (self.badge_warn, Rgb::hex(0xC9971F), 0.16),
            BadgeTone::Err => (self.badge_err, Rgb::hex(0xB5453A), 0.18),
            BadgeTone::Event => (self.badge_event, Rgb::hex(0xC08BD8), 0.16),
        };
        BadgeColors {
            fg,
            fill,
            fill_alpha,
        }
    }
}

impl ShellPalette {
    /// 暗色外壳（设计规格 §2.2「暗色」列）。
    pub const DARK: Self = Self {
        bg_app: Rgb::hex(0x0F1116),
        bg_map: Rgb::hex(0x0B0D11),
        bg_panel: Rgb::hex(0x191C24),
        bg_raised: Rgb::hex(0x212530),
        bg_hover: Rgb::hex(0x2A2F3C),
        bg_active: Rgb::hex(0x323847),
        border_subtle: Rgb::hex(0x272C37),
        border_strong: Rgb::hex(0x3A4152),
        text_1: Rgb::hex(0xE9ECF3),
        text_2: Rgb::hex(0xA3ABB9),
        text_3: Rgb::hex(0x858D9C),
        accent: Rgb::hex(0x4C8DF6),
        focus: Rgb::hex(0xF2C14E),
        bg_float: Rgb::hex(0x14171D),
        float_alpha: 0.94,
        badge_ok: Rgb::hex(0x7FD593),
        badge_warn: Rgb::hex(0xE8C065),
        badge_err: Rgb::hex(0xF09A91),
        badge_event: Rgb::hex(0xC9A0DC),
        text_on_accent: Rgb::hex(0x0B0D11),
        crumb_text_3: Rgb::hex(0x8A93A6),
    };

    /// 亮色外壳（设计规格 §2.2「亮色」列）。
    ///
    /// 注意焦点环用深琥珀 `#8A5A00` 而非金色：金 `#F2C14E` 在白底仅 1.62:1（§7）。
    pub const LIGHT: Self = Self {
        bg_app: Rgb::hex(0xEDEEF2),
        bg_map: Rgb::hex(0xE4E6EB),
        bg_panel: Rgb::hex(0xFBFBFD),
        bg_raised: Rgb::hex(0xF1F2F6),
        bg_hover: Rgb::hex(0xE7E9F0),
        bg_active: Rgb::hex(0xDBDEE8),
        border_subtle: Rgb::hex(0xE1E4EC),
        border_strong: Rgb::hex(0xC6CBD8),
        text_1: Rgb::hex(0x1B1E26),
        text_2: Rgb::hex(0x525A69),
        text_3: Rgb::hex(0x6B7383),
        accent: Rgb::hex(0x2A6FD4),
        focus: Rgb::hex(0x8A5A00),
        bg_float: Rgb::hex(0xF8F8FB),
        float_alpha: 0.93,
        // 状态色前景**必须换深色系**：浅底上浅色前景必然失效。
        // 原型上一轮只测了暗色，结果亮色下 `.bdg.ok` 实测仅 0.88:1
        // （浅绿 `#7FD593` 画在浅底合成出的 `#C2D3CF` 上）——
        // 见 `assets/ui-design/index.html` L125–L130 的亮色覆盖块。
        badge_ok: Rgb::hex(0x186B33),
        badge_warn: Rgb::hex(0x7A4E00),
        badge_err: Rgb::hex(0x992B22),
        badge_event: Rgb::hex(0x6D3388),
        text_on_accent: Rgb::hex(0xFFFFFF),
        crumb_text_3: Rgb::hex(0x5B6373),
    };

    /// 按主题取整套令牌。
    pub fn for_theme(theme: UiTheme) -> Self {
        match theme {
            UiTheme::Dark => Self::DARK,
            UiTheme::Light => Self::LIGHT,
        }
    }

    /// 浮层底色（`ui.bg.float`）连同其标定过的不透明度。
    pub fn float_fill(self) -> (Rgb, f32) {
        (self.bg_float, self.float_alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_decodes_to_expected_channels() {
        // 0x4C8DF6 → r=76/255, g=141/255, b=246/255
        let c = Rgb::hex(0x4C8DF6);
        assert!((c.r - 76.0 / 255.0).abs() < 1e-6, "got r={}", c.r);
        assert!((c.g - 141.0 / 255.0).abs() < 1e-6, "got g={}", c.g);
        assert!((c.b - 246.0 / 255.0).abs() < 1e-6, "got b={}", c.b);
    }

    #[test]
    fn theme_toggle_round_trips() {
        assert_eq!(UiTheme::Dark.toggled(), UiTheme::Light);
        assert_eq!(UiTheme::Light.toggled(), UiTheme::Dark);
    }

    #[test]
    fn density_metrics_follow_spec_table() {
        assert_eq!(UiDensity::Cozy.top_bar_height(), 38.0);
        assert_eq!(UiDensity::Compact.top_bar_height(), 32.0);
        assert_eq!(UiDensity::Cozy.hit(), 28.0);
        assert_eq!(UiDensity::Compact.hit(), 24.0);
        assert_eq!(UiDensity::Cozy.panel_head(), 30.0);
        assert_eq!(UiDensity::Compact.panel_head(), 26.0);
        assert_eq!(UiDensity::Cozy.row(), 24.0);
        assert_eq!(UiDensity::Compact.row(), 20.0);
    }

    #[test]
    fn light_focus_is_not_the_gold_that_fails_on_white() {
        // §7：world.accent.select 金 #F2C14E 在白底仅 1.62:1，亮色焦点环必须是深琥珀。
        assert_ne!(ShellPalette::LIGHT.focus, Rgb::hex(0xF2C14E));
        assert_eq!(ShellPalette::LIGHT.focus, Rgb::hex(0x8A5A00));
        // 暗色焦点环才是金。
        assert_eq!(ShellPalette::DARK.focus, Rgb::hex(0xF2C14E));
    }

    #[test]
    fn float_alpha_is_calibrated_separately_per_theme() {
        // §7：明暗浮层不透明度必须分别标定，不能只做色相反转。
        let (dark_fill, dark_a) = ShellPalette::DARK.float_fill();
        let (light_fill, light_a) = ShellPalette::LIGHT.float_fill();
        assert!((dark_a - 0.94).abs() < 1e-6, "got {dark_a}");
        assert!((light_a - 0.93).abs() < 1e-6, "got {light_a}");
        assert_ne!(dark_fill, light_fill);
    }

    // ── 对比度审计（§7）────────────────────────────────────────────────────
    //
    // 门槛约束的是**合成后的实际底色**，不是色卡原值；而且「先确认用的是哪条
    // 门槛」比数字本身更重要（REVIEW_v0.1 的教训）。所以审计不是把令牌两两
    // 相乘扫一遍，而是逐条列出**设计上真的会出现的配对**并给它该有的门槛。

    /// 正文 / 弱文本 / 强调文字的门槛（§7：正文 ≥ 4.5:1）。
    const TEXT_MIN: f32 = 4.5;
    /// 非文本（焦点环、图标、描边）的门槛（§7：非文本 ≥ 3:1）。
    const NON_TEXT_MIN: f32 = 3.0;

    /// 一条审计用例。
    struct AuditCase {
        theme: UiTheme,
        role: &'static str,
        surface: &'static str,
        ratio: f32,
        min: f32,
    }

    impl AuditCase {
        fn key(&self) -> String {
            format!("{:?}/{}/{}", self.theme, self.role, self.surface)
        }
    }

    /// 设计上真的会出现的「令牌 × 背板」配对。
    ///
    /// 背板取该令牌**实际落脚**的底：面板内的文字落在 `bg.panel` /
    /// `bg.raised` / `bg.hover` / `bg.active` 上；浮层上的文字落在 `bg.float`
    /// 合成后的底上（**不是** `bg.map`——浮层不透明 .93/.94，底衬只占 6–7%）；
    /// 面包屑只落在浮层上。
    fn audit_cases(theme: UiTheme) -> Vec<AuditCase> {
        let p = ShellPalette::for_theme(theme);
        let (float_fill, float_alpha) = p.float_fill();
        let float_bg = float_fill.over(float_alpha, p.bg_map);
        let mut out: Vec<AuditCase> = Vec::new();
        let mut add = |role: &'static str, fg: Rgb, surface: &'static str, bg: Rgb, min: f32| {
            out.push(AuditCase {
                theme,
                role,
                surface,
                ratio: fg.contrast_ratio(bg),
                min,
            });
        };

        for (surface, bg) in [
            ("bg.panel", p.bg_panel),
            ("bg.raised", p.bg_raised),
            ("bg.hover", p.bg_hover),
            ("bg.active", p.bg_active),
            ("bg.float", float_bg),
        ] {
            add("text.1", p.text_1, surface, bg, TEXT_MIN);
            add("text.2", p.text_2, surface, bg, TEXT_MIN);
            add("text.3", p.text_3, surface, bg, TEXT_MIN);
        }
        // accent 作**文字**（链接、次要按钮标签）只出现在面板内；作图标时
        // 按非文本门槛，这枚令牌在两种角色下都要能用。
        for (surface, bg) in [
            ("bg.panel", p.bg_panel),
            ("bg.raised", p.bg_raised),
            ("bg.hover", p.bg_hover),
        ] {
            add("accent(as text)", p.accent, surface, bg, TEXT_MIN);
        }
        // 焦点环是非文本：2px 实心环 + 2px 偏移（§7）。
        for (surface, bg) in [
            ("bg.panel", p.bg_panel),
            ("bg.raised", p.bg_raised),
            ("bg.active", p.bg_active),
            ("bg.map", p.bg_map),
        ] {
            add("focus(ring)", p.focus, surface, bg, NON_TEXT_MIN);
        }
        add(
            "crumb.text.3",
            p.crumb_text_3,
            "bg.float",
            float_bg,
            TEXT_MIN,
        );
        add(
            "text.on_accent",
            p.text_on_accent,
            "accent",
            p.accent,
            TEXT_MIN,
        );
        for tone in [
            BadgeTone::Ok,
            BadgeTone::Warn,
            BadgeTone::Err,
            BadgeTone::Event,
        ] {
            let badge = p.badge(tone);
            for (surface, backdrop) in [
                ("badge on bg.panel", p.bg_panel),
                ("badge on bg.raised", p.bg_raised),
                ("badge on bg.float", float_bg),
            ] {
                add(
                    badge_role(tone),
                    badge.fg,
                    surface,
                    badge.background_on(backdrop),
                    TEXT_MIN,
                );
            }
        }
        out
    }

    fn badge_role(tone: BadgeTone) -> &'static str {
        match tone {
            BadgeTone::Neutral => "badge.neutral",
            BadgeTone::Ok => "badge.ok",
            BadgeTone::Warn => "badge.warn",
            BadgeTone::Err => "badge.err",
            BadgeTone::Event => "badge.ev",
        }
    }

    /// 已登记的**真实缺口**：设计上会出现的配对里，实测低于门槛的那些。
    ///
    /// 根因只有一条：**整套令牌是按 `bg.panel` 标定的**，弱文本与强调色一旦落到
    /// 更暗 / 更亮的辅助底（`hover` / `active` / `raised` / 浮层）就会掉到 4.5 以下。
    /// 暗色掉在 `hover` / `active`（底比 panel 亮），亮色几乎处处都掉
    /// （底色越亮，浅色文字越失效）。
    ///
    /// 这不是「算了」：候选修正值已在
    /// [`tests::candidate_tokens_close_every_registered_gap`] 里验证通过，换上去
    /// 即可全部达标。先只登记不改，是因为令牌值属 `UI_DESIGN_SPEC_v0.1.md` §2.2
    /// 的既有结论，改它要走 §6.3（追加说明不改结论）与 §10 决议，不由实现单方面决定。
    const KNOWN_GAPS: &[(UiTheme, &str, &str)] = &[
        (UiTheme::Dark, "text.3", "bg.hover"),            // 实测 4.00
        (UiTheme::Dark, "text.3", "bg.active"),           // 实测 3.51
        (UiTheme::Dark, "accent(as text)", "bg.hover"),   // 实测 4.11
        (UiTheme::Light, "text.3", "bg.raised"),          // 实测 4.26
        (UiTheme::Light, "text.3", "bg.hover"),           // 实测 3.93
        (UiTheme::Light, "text.3", "bg.active"),          // 实测 3.55
        (UiTheme::Light, "text.3", "bg.float"),           // 实测 4.46
        (UiTheme::Light, "accent(as text)", "bg.raised"), // 实测 4.34
        (UiTheme::Light, "accent(as text)", "bg.hover"),  // 实测 4.00
    ];

    /// 审计结果必须与登记表**完全一致**：表外的必须达标，表内的必须确实不达标。
    ///
    /// 双向断言的意义在「防腐烂」：修好一个缺口却忘了删表，测试同样会红。
    #[test]
    fn contrast_audit_matches_the_registered_gap_table() {
        let mut failing = Vec::new();
        let mut passing = 0usize;
        for theme in [UiTheme::Dark, UiTheme::Light] {
            for case in audit_cases(theme) {
                assert!(
                    (1.0..=21.0).contains(&case.ratio),
                    "{} 的对比度 {:.2} 不在 WCAG 值域内",
                    case.key(),
                    case.ratio
                );
                if case.ratio < case.min {
                    failing.push(case.key());
                } else {
                    passing += 1;
                }
            }
        }
        let mut expected: Vec<String> = KNOWN_GAPS
            .iter()
            .map(|(theme, role, surface)| format!("{theme:?}/{role}/{surface}"))
            .collect();
        failing.sort();
        expected.sort();
        assert_eq!(
            failing,
            expected,
            "审计与登记表不一致。\n实测不达标（{}）{:?}\n已登记（{}）{:?}\n\
             未登记的缺口必须修掉；已修好的缺口必须从 KNOWN_GAPS 删行。",
            failing.len(),
            failing,
            expected.len(),
            expected
        );
        // 钉住配对表的规模：防止有人把审计面悄悄改瘦来让测试变绿。
        assert!(
            passing >= 60,
            "被审计且达标的配对只剩 {passing} 条，配对表是不是被改瘦了？"
        );
    }

    /// 已登记的缺口都有**已验证的候选修正值**：换上去即可全部达标。
    ///
    /// 交付物不只是「哪里不合格」，还包括「改成什么能合格」。
    #[test]
    fn candidate_tokens_close_every_registered_gap() {
        let dark_text3 = Rgb::hex(0x9BA3B3);
        let light_text3 = Rgb::hex(0x59606E);
        let dark_accent = Rgb::hex(0x6BA1F8);
        let light_accent = Rgb::hex(0x2263C0);
        let dp = ShellPalette::DARK;
        let lp = ShellPalette::LIGHT;
        let (lf, la) = lp.float_fill();
        let light_float = lf.over(la, lp.bg_map);
        for (name, fg, bg) in [
            ("暗色 text.3 / bg.hover", dark_text3, dp.bg_hover),
            ("暗色 text.3 / bg.active", dark_text3, dp.bg_active),
            ("暗色 accent / bg.hover", dark_accent, dp.bg_hover),
            ("亮色 text.3 / bg.raised", light_text3, lp.bg_raised),
            ("亮色 text.3 / bg.hover", light_text3, lp.bg_hover),
            ("亮色 text.3 / bg.active", light_text3, lp.bg_active),
            ("亮色 text.3 / bg.float", light_text3, light_float),
            ("亮色 accent / bg.raised", light_accent, lp.bg_raised),
            ("亮色 accent / bg.hover", light_accent, lp.bg_hover),
        ] {
            let ratio = fg.contrast_ratio(bg);
            assert!(ratio >= TEXT_MIN, "候选值在「{name}」上只有 {ratio:.2}:1");
        }
    }

    /// 复现原型的实测数字：亮色面包屑 5.65:1（`assets/ui-design/CHANGES_v0.2` §B.3）。
    ///
    /// 这条测试真正的价值不在 5.65 这个数，而在于**证明 Rust 侧的算式与
    /// `assets/ui-design/audit-contrast.py` 是同一条**——否则 Rust 里的「达标」
    /// 只是自说自话。
    #[test]
    fn light_crumb_reproduces_the_html_contrast_audit() {
        let p = ShellPalette::LIGHT;
        let (fill, alpha) = p.float_fill();
        let float_bg = fill.over(alpha, p.bg_map);
        assert_eq!(float_bg, Rgb::hex(0xF7F7FA), "浮层合成底色与原型不一致");
        let ratio = p.crumb_text_3.contrast_ratio(float_bg);
        assert!((ratio - 5.65).abs() < 0.02, "got {ratio:.3}");
    }

    /// 暗色浮层同理：压暗色地图得 `#13161C`，面包屑 5.87:1。
    #[test]
    fn dark_float_layer_composites_to_the_audited_backdrop() {
        let p = ShellPalette::DARK;
        let (fill, alpha) = p.float_fill();
        let float_bg = fill.over(alpha, p.bg_map);
        assert_eq!(float_bg, Rgb::hex(0x13161C));
        let ratio = p.crumb_text_3.contrast_ratio(float_bg);
        assert!(ratio >= TEXT_MIN, "暗色面包屑只有 {ratio:.2}:1");
    }

    /// accent 实心底上的文字**不能一律用白色**——这条测试是 `text_on_accent` 的由来。
    #[test]
    fn accent_fill_text_must_not_be_white_in_dark_theme() {
        let white = Rgb::hex(0xFFFFFF);
        let dark = ShellPalette::DARK;
        assert!(
            white.contrast_ratio(dark.accent) < TEXT_MIN,
            "白字在暗色 accent 上意外达标了，text_on_accent 存在的前提不成立"
        );
        assert!(dark.text_on_accent.contrast_ratio(dark.accent) >= TEXT_MIN);
        // 亮色 accent 反过来：白字达标，近黑色反而不行。
        let light = ShellPalette::LIGHT;
        assert_eq!(light.text_on_accent, white);
        assert!(light.text_on_accent.contrast_ratio(light.accent) >= TEXT_MIN);
    }

    /// 亮色语义前景必须是深色系：原型上一轮只测暗色，亮色下 `.bdg.ok` 实测仅 0.88:1
    /// （浅绿 `#7FD593` 画在浅底合成出的 `#C2D3CF` 上）。
    #[test]
    fn light_semantic_foregrounds_are_dark_not_pastel() {
        let dark = ShellPalette::DARK;
        let light = ShellPalette::LIGHT;
        for (name, df, lf) in [
            ("badge.ok", dark.badge_ok, light.badge_ok),
            ("badge.warn", dark.badge_warn, light.badge_warn),
            ("badge.err", dark.badge_err, light.badge_err),
            ("badge.ev", dark.badge_event, light.badge_event),
        ] {
            assert_ne!(df, lf, "{name} 的明暗前景不该相同");
            assert!(
                lf.relative_luminance() < df.relative_luminance(),
                "{name} 的亮色前景必须比暗色前景更暗"
            );
        }
        // 钉住原型给出的亮色值（`index.html` L127–L130）。
        assert_eq!(light.badge_ok, Rgb::hex(0x186B33));
        assert_eq!(light.badge_warn, Rgb::hex(0x7A4E00));
        assert_eq!(light.badge_err, Rgb::hex(0x992B22));
        assert_eq!(light.badge_event, Rgb::hex(0x6D3388));
    }

    /// 徽标底色取的是世界语义色，必须与 `crate::palette` 同值（§4.7「同色相」）。
    ///
    /// 令牌层刻意不依赖世界色板（方向是「色板 → 令牌」，不是反过来），
    /// 所以这层一致性由测试守住，而不是靠 `use`。
    #[test]
    fn badge_fills_mirror_the_world_link_palette() {
        let p = ShellPalette::DARK;
        assert_eq!(
            p.badge(BadgeTone::Ok).fill,
            world_rgb(crate::palette::UI_REACHABLE)
        );
        assert_eq!(
            p.badge(BadgeTone::Warn).fill,
            world_rgb(crate::palette::UI_LATE)
        );
        assert_eq!(
            p.badge(BadgeTone::Err).fill,
            world_rgb(crate::palette::UI_UNREACHABLE)
        );
        assert_eq!(
            p.badge(BadgeTone::Event).fill,
            world_rgb(crate::palette::ACCENT_EVENT)
        );
        // 底色恒定，只有前景随主题走。
        assert_eq!(
            p.badge(BadgeTone::Ok).fill,
            ShellPalette::LIGHT.badge(BadgeTone::Ok).fill
        );
    }

    /// `bevy::Color` → 本模块的 `Rgb`（测试专用）。
    fn world_rgb(c: bevy::prelude::Color) -> Rgb {
        let [r, g, b, _] = crate::palette::palette_to_color32(c).to_srgba_unmultiplied();
        Rgb {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
        }
    }

    /// `to_color32_alpha` 必须是**非预乘**语义，否则浮层底会过亮。
    #[test]
    fn alpha_color32_is_unmultiplied_like_css_rgba() {
        let half = Rgb::hex(0xFFFFFF).to_color32_alpha(0.5);
        // CSS 里「白色 .5」是半透明白；若照预乘写法只改第四字节，
        // 这里会读出 255（即全白），浮层就会盖住地图。
        assert_eq!(half.to_srgba_unmultiplied(), [255, 255, 255, 128]);
    }

    /// `over` 必须与 CSS `rgba` 的合成结果一致。
    #[test]
    fn over_matches_css_rgba_compositing() {
        assert_eq!(
            Rgb::hex(0xFFFFFF).over(0.5, Rgb::hex(0x000000)),
            Rgb::hex(0x808080)
        );
        let some = Rgb::hex(0x4C8DF6);
        assert_eq!(some.over(1.0, Rgb::hex(0x000000)), some, "不透明时应为本色");
        assert_eq!(
            some.over(0.0, Rgb::hex(0x191C24)),
            Rgb::hex(0x191C24),
            "全透明时应为背板色"
        );
    }
}
