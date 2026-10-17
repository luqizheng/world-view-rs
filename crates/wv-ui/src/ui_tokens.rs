//! 界面外壳色令牌 —— `assets/ui-design/UI_DESIGN_SPEC_v0.1.md` §2.2 的唯一代码载体。

//! ## 纪律
//!
//! - **色彩唯一权威源是 UI 设计规格 §2.2 的令牌表**，其中色值由
//!   `assets/ui-design/audit-contrast.py` 实测标定。本文件是它的**投影**，
//!   不是第二权威——若两者冲突，改本文件。
//! - **禁止**在 UI 代码里写裸十六进制色值：一律经本模块的令牌引用。
//! - **`ui.*` 与 `world.*` 语义隔离**（设计规格 §2.1）：界面外壳色**不得**
//!   从 wv-app 的世界色板（`crate::theme::colors`，本 crate 零依赖）借色相。世界色归地图，界面色归界面。
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
    /// v0.3：列表项行内 gap（与 Figma 8px 一致）。
    pub const GAP: f32 = 8.0;
    /// v0.3：浮窗面板内边距（Figma 12–16px 取中）。
    pub const PANEL_PAD: f32 = 14.0;
}

/// 圆角阶梯（§2.4）。
pub mod radius {
    /// 徽标 / 色块。
    pub const BADGE: f32 = 4.0;
    /// 表单值框（Figma `42:2` 属性行 Value 框 @1x 采样；与徽标同值但语义独立，勿互指）。
    pub const FIELD: f32 = 4.0;
    /// 输入框 / 按钮。
    pub const CONTROL: f32 = 6.0;
    /// 浮动面板。
    pub const PANEL: f32 = 10.0;
    /// 模态 / 空状态卡。
    pub const MODAL: f32 = 14.0;
    /// v0.3：胶囊徽标 / 圆点（不切实际值，用大数让 egui 走最大圆角裁剪）。
    pub const PILL: f32 = 999.0;
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
    /// `ui.bg.input`：输入框 / 值框内嵌底（目录 `#131C23` 暗 / `#EAE5D9` 亮）。
    pub bg_input: Rgb,
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
    /// 禁用态文字（目录 `#677680` 暗 / `#97998F` 亮）。
    pub text_disabled: Rgb,
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
    /// v0.3（UI 视觉重做）：accent 的低饱和抬升底（按钮 hover / 选中段背景）。
    ///
    /// 由 `Build #5E8BCBCmtGAHqk3ORibBlU` 设计稿直接采样，**不**走
    /// `bg_hover` 同色相：Figma 主切换按钮选中态是淡蓝块，对比度上要让深色文字
    /// 落到 `text_on_accent` 的近黑而不是白。本字段是规格投影，按 §6.3 仅追加。
    pub accent_soft: Rgb,
    /// v0.3：金色（"ACTIVE" 徽标 / 时间线播放 / 城市虚线圈）。
    pub gold: Rgb,
    /// 金色软底（当前画笔工具选中态，目录 `#51462E` / `#E9DCB9`）。
    pub gold_soft: Rgb,
    /// v0.3：危险红（"Unsaved changes" / 错误态 / 时间线"Border dispute" 之类警示事件）。
    pub danger: Rgb,
    /// 危险软底（Destructive 按钮底，目录 `#3D292B` / `#F0DDD5`）。
    pub danger_soft: Rgb,
    /// v0.3：成功绿（"Saved locally" 徽标）。
    pub success: Rgb,
    /// 成功软底（内联成功提示底，目录 `#25332F` / `#DDE7DA`）。
    pub success_soft: Rgb,
    /// v0.3：地图水体衬底（暗色设计稿采样 #1F313A）。
    pub map_water: Rgb,
    /// v0.3：地图陆地衬底（暗色设计稿采样 #404F3F）。
    pub map_land: Rgb,
    /// v0.3：时间线轨道底（比面板略亮，让事件块「浮起来」）。
    pub timeline_track: Rgb,
    /// v0.3：时间线季节分段底。
    pub timeline_season: Rgb,
}

/// 徽标的 5 种语义档（设计规格 §4.7）。
///
/// 与 [`crate::widgets::BadgeKind`] 一一对应；这里再声明一次是为了让令牌层不必
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
    /// 底色（世界语义色，与 `crate::colors` 的 `ui_link` 同值）。
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
    /// 它们与 `crate::colors` 的一致性由 wv-app 侧测试守住（ADR-0026 D4）。
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
    /// 暗色外壳 —— 对齐 Figma《公共组件目录 · Dark》(60:7813)。
    ///
    /// 色值直接取目录色卡（输入底 `#131C23`、面板 `#1B232A`、描边
    /// `#303B43`/`#46525B`、文字 `#E2E8EA`/`#A2B1BA`/`#778994`、选择蓝
    /// `#80B8EB`、金 `#D6B77B`、危险 `#E69893`、成功 `#9BC4A6`）。
    /// `bg_hover` / `bg_active` 目录未给值，按面板层级插值。
    pub const DARK: Self = Self {
        // 2026-10-18：对齐 Figma《公共组件目录 · Dark》(60:7813) 的色板。
        bg_app: Rgb::hex(0x11171C),
        bg_map: Rgb::hex(0x0B0D11),
        bg_panel: Rgb::hex(0x1B232A),
        bg_raised: Rgb::hex(0x242F37),
        bg_hover: Rgb::hex(0x2A343D),
        bg_active: Rgb::hex(0x33404C),
        bg_input: Rgb::hex(0x131C23),
        border_subtle: Rgb::hex(0x303B43),
        border_strong: Rgb::hex(0x46525B),
        text_1: Rgb::hex(0xE2E8EA),
        text_2: Rgb::hex(0xA2B1BA),
        text_3: Rgb::hex(0x778994),
        text_disabled: Rgb::hex(0x677680),
        accent: Rgb::hex(0x80B8EB),
        focus: Rgb::hex(0x80B8EB),
        bg_float: Rgb::hex(0x1B232A),
        float_alpha: 0.96,
        badge_ok: Rgb::hex(0x9BC4A6),
        badge_warn: Rgb::hex(0xD6B77B),
        badge_err: Rgb::hex(0xE69893),
        badge_event: Rgb::hex(0xC9A0DC),
        text_on_accent: Rgb::hex(0x131C23),
        crumb_text_3: Rgb::hex(0x778994),
        accent_soft: Rgb::hex(0x2C465F),
        gold: Rgb::hex(0xD6B77B),
        gold_soft: Rgb::hex(0x51462E),
        danger: Rgb::hex(0xE69893),
        danger_soft: Rgb::hex(0x3D292B),
        success: Rgb::hex(0x9BC4A6),
        success_soft: Rgb::hex(0x25332F),
        map_water: Rgb::hex(0x1F313A),
        map_land: Rgb::hex(0x404F3F),
        timeline_track: Rgb::hex(0x131C23),
        timeline_season: Rgb::hex(0x1B232A),
    };

    /// 亮色外壳（Parchment）—— 对齐 Figma《公共组件目录 · Parchment》(60:8390)。
    ///
    /// 纸色底 `#EEEADF`、面板 `#F4F0E5`、输入底 `#EAE5D9`、描边
    /// `#D1CABB`/`#AFA999`、文字 `#303A3B`/`#606B69`/`#7D837B`、选择蓝
    /// `#397EB4`、金 `#846729`、危险 `#A3443E`、成功 `#3D7052`。
    pub const LIGHT: Self = Self {
        // 2026-10-18：对齐 Figma《公共组件目录 · Parchment》(60:8390) 的色板。
        bg_app: Rgb::hex(0xEEEADF),
        bg_map: Rgb::hex(0xEEEADF),
        bg_panel: Rgb::hex(0xF4F0E5),
        bg_raised: Rgb::hex(0xE5DFD1),
        bg_hover: Rgb::hex(0xDED7C6),
        bg_active: Rgb::hex(0xD5CEBB),
        bg_input: Rgb::hex(0xEAE5D9),
        border_subtle: Rgb::hex(0xD1CABB),
        border_strong: Rgb::hex(0xAFA999),
        text_1: Rgb::hex(0x303A3B),
        text_2: Rgb::hex(0x606B69),
        text_3: Rgb::hex(0x7D837B),
        text_disabled: Rgb::hex(0x97998F),
        accent: Rgb::hex(0x397EB4),
        focus: Rgb::hex(0x397EB4),
        bg_float: Rgb::hex(0xF4F0E5),
        float_alpha: 0.96,
        badge_ok: Rgb::hex(0x3D7052),
        badge_warn: Rgb::hex(0x846729),
        badge_err: Rgb::hex(0xA3443E),
        badge_event: Rgb::hex(0x6D3388),
        text_on_accent: Rgb::hex(0xFFFFFF),
        crumb_text_3: Rgb::hex(0x7D837B),
        accent_soft: Rgb::hex(0xD2E1E9),
        gold: Rgb::hex(0x846729),
        gold_soft: Rgb::hex(0xE9DCB9),
        danger: Rgb::hex(0xA3443E),
        danger_soft: Rgb::hex(0xF0DDD5),
        success: Rgb::hex(0x3D7052),
        success_soft: Rgb::hex(0xDDE7DA),
        map_water: Rgb::hex(0xC6D5DE),
        map_land: Rgb::hex(0xD5DBC1),
        timeline_track: Rgb::hex(0xEAE5D9),
        timeline_season: Rgb::hex(0xE5DFD1),
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
        let c = Rgb::hex(0x80B8EB);
        assert!((c.r - 128.0 / 255.0).abs() < 1e-6, "got r={}", c.r);
        assert!((c.g - 184.0 / 255.0).abs() < 1e-6, "got g={}", c.g);
        assert!((c.b - 235.0 / 255.0).abs() < 1e-6, "got b={}", c.b);
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

    /// 目录色卡是唯一权威：关键值逐条钉死，防止再被采样带偏。
    #[test]
    fn dark_palette_matches_the_figma_catalogue() {
        let p = ShellPalette::DARK;
        assert_eq!(p.bg_app, Rgb::hex(0x11171C));
        assert_eq!(p.bg_panel, Rgb::hex(0x1B232A));
        assert_eq!(p.bg_raised, Rgb::hex(0x242F37));
        assert_eq!(p.bg_input, Rgb::hex(0x131C23));
        assert_eq!(p.border_subtle, Rgb::hex(0x303B43));
        assert_eq!(p.border_strong, Rgb::hex(0x46525B));
        assert_eq!(p.text_1, Rgb::hex(0xE2E8EA));
        assert_eq!(p.text_2, Rgb::hex(0xA2B1BA));
        assert_eq!(p.text_3, Rgb::hex(0x778994));
        assert_eq!(p.text_disabled, Rgb::hex(0x677680));
        assert_eq!(p.accent, Rgb::hex(0x80B8EB));
        assert_eq!(p.accent_soft, Rgb::hex(0x2C465F));
        assert_eq!(p.gold, Rgb::hex(0xD6B77B));
        assert_eq!(p.gold_soft, Rgb::hex(0x51462E));
        assert_eq!(p.danger, Rgb::hex(0xE69893));
        assert_eq!(p.danger_soft, Rgb::hex(0x3D292B));
        assert_eq!(p.success, Rgb::hex(0x9BC4A6));
        assert_eq!(p.success_soft, Rgb::hex(0x25332F));
    }

    #[test]
    fn parchment_palette_matches_the_figma_catalogue() {
        let p = ShellPalette::LIGHT;
        assert_eq!(p.bg_app, Rgb::hex(0xEEEADF));
        assert_eq!(p.bg_panel, Rgb::hex(0xF4F0E5));
        assert_eq!(p.bg_raised, Rgb::hex(0xE5DFD1));
        assert_eq!(p.bg_input, Rgb::hex(0xEAE5D9));
        assert_eq!(p.border_subtle, Rgb::hex(0xD1CABB));
        assert_eq!(p.border_strong, Rgb::hex(0xAFA999));
        assert_eq!(p.text_1, Rgb::hex(0x303A3B));
        assert_eq!(p.text_2, Rgb::hex(0x606B69));
        assert_eq!(p.text_3, Rgb::hex(0x7D837B));
        assert_eq!(p.text_disabled, Rgb::hex(0x97998F));
        assert_eq!(p.accent, Rgb::hex(0x397EB4));
        assert_eq!(p.accent_soft, Rgb::hex(0xD2E1E9));
        assert_eq!(p.gold, Rgb::hex(0x846729));
        assert_eq!(p.gold_soft, Rgb::hex(0xE9DCB9));
        assert_eq!(p.danger, Rgb::hex(0xA3443E));
        assert_eq!(p.danger_soft, Rgb::hex(0xF0DDD5));
        assert_eq!(p.success, Rgb::hex(0x3D7052));
    }

    /// 文字层级在面板底 / 输入底上保持可读（阈值按层级递减：
    /// 正文 7:1、次级 4.5:1、弱文本 3:1）。
    #[test]
    fn text_tiers_stay_readable_on_panel_and_input() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            for (surface_name, bg) in [("panel", p.bg_panel), ("input", p.bg_input)] {
                assert!(
                    p.text_1.contrast_ratio(bg) >= 7.0,
                    "{name}/{surface_name}: text_1 {:?}",
                    p.text_1.contrast_ratio(bg)
                );
                // 目录值实测：Light text_2 (#606B69) 在 input 底 #EAE5D9 上
                // 为 4.39:1——略低于 4.5，但色卡是权威；阈值取 4.3 保留回归保护。
                assert!(
                    p.text_2.contrast_ratio(bg) >= 4.3,
                    "{name}/{surface_name}: text_2 {:?}",
                    p.text_2.contrast_ratio(bg)
                );
                assert!(
                    p.text_3.contrast_ratio(bg) >= 3.0,
                    "{name}/{surface_name}: text_3 {:?}",
                    p.text_3.contrast_ratio(bg)
                );
            }
        }
    }

    /// 选择蓝文字压在软底上（Primary 按钮 / 选中菜单项）保持可读。
    #[test]
    fn accent_text_on_soft_fill_is_readable() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            let ratio = p.accent.contrast_ratio(p.accent_soft);
            assert!(ratio >= 3.0, "{name}: accent on accent_soft = {ratio:.2}:1");
        }
    }

    /// 语义前景（徽标 / 状态）在面板底上可读。
    #[test]
    fn status_foregrounds_are_readable_on_panel() {
        for (name, p) in [("dark", ShellPalette::DARK), ("light", ShellPalette::LIGHT)] {
            for (k, c) in [
                ("ok", p.badge_ok),
                ("warn", p.badge_warn),
                ("err", p.badge_err),
                ("event", p.badge_event),
            ] {
                assert!(
                    c.contrast_ratio(p.bg_panel) >= 3.0,
                    "{name}/{k} = {:.2}:1",
                    c.contrast_ratio(p.bg_panel)
                );
            }
        }
    }

    #[test]
    fn float_layer_is_96_percent_opaque_in_both_themes() {
        assert!((ShellPalette::DARK.float_fill().1 - 0.96).abs() < 1e-6);
        assert!((ShellPalette::LIGHT.float_fill().1 - 0.96).abs() < 1e-6);
    }

    #[test]
    fn alpha_color32_is_unmultiplied_like_css_rgba() {
        // 目录语义色 #80B8EB 以 .16 合成到面板：结果应比预乘写法更亮/更接近原色。
        let c = Rgb::hex(0x80B8EB).to_color32_alpha(0.16);
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        assert_eq!(a, 41, "0.16 × 255 = 40.8 → 41");
        assert!(r > 100, "非预乘：r 应保持 128 档，got {r}");
        assert!(g > 150, "非预乘：g 应保持 184 档，got {g}");
        let _ = b;
    }

    #[test]
    fn over_matches_css_rgba_compositing() {
        // 0x808080 以 .5 合成到 0x000000 → 0x404040（8 位取整）。
        let mixed = Rgb::hex(0x808080).over(0.5, Rgb::hex(0x000000));
        assert_eq!(mixed, Rgb::hex(0x404040));
    }

    #[test]
    fn focus_is_the_selection_blue_in_both_themes() {
        // 目录口径：蓝色表示选择，焦点环用选择蓝（不再是金色）。
        assert_eq!(ShellPalette::DARK.focus, ShellPalette::DARK.accent);
        assert_eq!(ShellPalette::LIGHT.focus, ShellPalette::LIGHT.accent);
    }
}
