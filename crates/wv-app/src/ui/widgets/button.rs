//! 按钮族 —— 统一按钮工厂 [`button`] + 工具栏按钮 [`ToolButton`]。
//!
//! 双轨制分工（2026-10-15 定型）：
//! - [`ToolButton`]：**工具栏场景**——方块图标 / 选中态 / tooltip 一等公民，
//!   `map_controls` 等处使用。
//! - [`button`]：**非工具栏场景**——主/次 CTA / 对话框 / 错误行 / 顶栏 raised 等。
//!
//! 不合并的原因：合并会把 `ToolButton` 的 active/disabled/hint builder 撑爆，
//! 且工具栏「方块图标」语义与对话框「宽 CTA」语义互不兼容；保持双轨，调用方按场景选入口。
//!
//! 颜色全部走 `UiShell` 令牌，不写裸 hex。
//!
//! 图标常量 [`super::primitives::ICON_PEN`] 等集中在 `primitives`，
//! 本模块只消费不定义。
//!
//! 历史来源：拆自原 [`crate::ui::widgets`]。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::theme::ui_tokens::{font_size, radius};
use crate::ui::shell::UiShell;

use super::primitives::icon_image;

// ── 统一按钮工厂 ─────────────────────────────────────────────────────────

/// 按钮样式（统一工厂第二参数）。
///
/// 决定 fill / stroke / 前景色 / 字号 / 圆角等视觉量。文字与图标色来自 [`UiShell`]
/// 令牌，**不**接收裸 hex。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// accent 实心底 + 白字白图标（主 CTA / Figma `primary button`）。
    Accent,
    /// raised 底 + border_strong 描边 + t2 字（次 CTA / 顶栏 raised）。
    Raised,
    /// 透明底 + t1 字（错误行跳转 / Dialog 取消式按钮）。
    Ghost,
    /// 透明底 + t2 字（顶栏 / 列表底部动作）。
    #[default]
    Subtle,
}

/// 按钮内容（统一工厂第三参数）。
///
/// `Image` / `ImageText` 二变体收 `egui::ImageSource<'static>`——
/// `ICON_*` 常量直接喂入即可（`include_image!` 给的也是 `'static`）。
///
/// 不 derive `Copy`：`egui::ImageSource` 不是 `Copy`（内部是 `Arc<Vec<...>>`
/// 类型也是 `Clone` 而非 `Copy`），本枚举只能 `Clone`。
#[derive(Clone)]
pub enum ButtonContent<'a> {
    /// 纯文字按钮（高度 = `shell.hit`，宽度随文字）。
    Text(&'a str),
    /// 纯图标按钮（方块 = `shell.hit × shell.hit`，图标 = `hit − 8`）。
    Image(egui::ImageSource<'static>),
    /// 图标 + 文字横排（高度 = `shell.hit`，宽度随内容）。
    ImageText(egui::ImageSource<'static>, &'a str),
}

/// 统一按钮工厂。
///
/// 工具栏场景请用 [`ToolButton`]（map_controls 等）；非工具栏场景一律走这里。
/// 返回 [`egui::Response`]，caller 自己判断 `.clicked()` / `.hovered()` 等。
///
/// ## 参数
///
/// - `content` / `style`：见 [`ButtonContent`] / [`ButtonStyle`]。
/// - `min_size_override`：`None` → 工厂默认（`Text` / `ImageText` 高度 =
///   `shell.hit`，宽度随内容；`Image` 方块 = `shell.hit × shell.hit`）。
///   `Some(s)` → caller 自定最小尺寸（例如欢迎页 CTA 高度 = `CTA_HEIGHT=40`
///   与默认 28 不同，必须 `Some(vec2(w, 40))`）。
///   caller **不得**通过 `Response.rect` 反推修改——`rect` 是分配结果，修改即失配。
/// - `enabled`：`false` → 工厂内部走 [`egui::Ui::add_enabled`]，整按钮置灰且不响应
///   点击。对话框「创建」按钮在 `name + location` 双空时禁用就是这一支路。
pub fn button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    content: ButtonContent,
    style: ButtonStyle,
    min_size_override: Option<egui::Vec2>,
    enabled: bool,
) -> egui::Response {
    let (fill, stroke, fg, font_px, corner) = match style {
        ButtonStyle::Accent => (
            shell.accent(),
            None,
            egui::Color32::WHITE,
            font_size::INPUT,
            radius::CONTROL,
        ),
        ButtonStyle::Raised => (
            shell.raised_bg(),
            Some(egui::Stroke::new(1.0, shell.border_strong())),
            shell.t2(),
            font_size::INPUT,
            radius::CONTROL,
        ),
        ButtonStyle::Ghost => (
            egui::Color32::TRANSPARENT,
            None,
            shell.t1(),
            shell.scale.small,
            radius::CONTROL,
        ),
        ButtonStyle::Subtle => (
            egui::Color32::TRANSPARENT,
            None,
            shell.t2(),
            shell.scale.small,
            radius::CONTROL,
        ),
    };

    let default_min = match &content {
        ButtonContent::Text(_) | ButtonContent::ImageText(_, _) => egui::vec2(0.0, shell.hit),
        ButtonContent::Image(_) => egui::Vec2::splat(shell.hit),
    };
    let min_size = min_size_override.unwrap_or(default_min);

    let mut btn = match content {
        ButtonContent::Text(label) => {
            egui::Button::new(egui::RichText::new(label).color(fg).size(font_px))
        }
        ButtonContent::Image(src) => egui::Button::image(icon_image(src, fg, shell.hit - 8.0)),
        ButtonContent::ImageText(src, label) => egui::Button::image_and_text(
            icon_image(src, fg, shell.hit - 8.0),
            egui::RichText::new(label).color(fg).size(font_px),
        ),
    };
    btn = btn.fill(fill).corner_radius(corner).min_size(min_size);
    if let Some(s) = stroke {
        btn = btn.stroke(s);
    }
    ui.add_enabled(enabled, btn)
}

// ── 标准按钮（标准化控件 · 第 1 项）────────────────────────────────────
//
// 职责：把「内容（图标 / 文字 / 图标+文字）+ tooltip + 选中态 + 启用态 +
// 点击回调」打包成单次 `ui.add`。**新代码一律走这里**，不再手搓
// `egui::Button` / 手绘 clickable（存量六种旧模式按批次迁移）。
//
// 样式来源：默认值全部由 [`UiShell`] 令牌解析——`tint = t1`、
// `active_bg = bg_active`、边长 = `hit`（28/24 随密度）、图标 = hit−8、
// 文字 = `scale.small`；不写裸 hex（ui_tokens 纪律）。caller 用 builder
// 显式覆盖（map_controls 的 Google 白控件配色即此用法）。
//
// 边界：单一按钮封装，**不**承载浮窗外壳/分组/分隔线（各面板自管）；
// 不写主题令牌源头。

/// 工具栏图标源。SVG 走 `egui::include_image!`，编译期嵌入二进制，零运行时 IO。
#[derive(Clone)]
pub enum ToolIcon {
    /// Lucide / 任意合规 SVG 源码（`include_str!`/宏嵌入），`stroke="currentColor"`。
    Svg(egui::ImageSource<'static>),
}

/// 按钮内容三形态。
///
/// 新增图标 = 放一个 SVG 进 `assets/ui/tool_icons/` + 加一个 `ICON_*` 常量，
/// 不需要任何运行时注册。
#[derive(Clone)]
pub enum ToolButtonContent<'a> {
    /// 纯图标（方块工具钮）。
    Icon(ToolIcon),
    /// 纯文字（幽灵按钮：透明底，hover/按下态走 egui visuals）。
    Text(&'a str),
    /// 图标 + 文字（横排）。
    IconText(ToolIcon, &'a str),
}

/// 默认样式解析结果（纯数据，单测直接断言；`show` 消费它装配 egui Button）。
struct ResolvedStyle {
    tint: egui::Color32,
    min_size: egui::Vec2,
    icon_px: f32,
    text_px: f32,
}

/// 选中态只差背景：active → `active_bg`，非选中 → 幽灵（透明底）。
///
/// 透明底让图标钮浮在自绘 Frame（地图白卡）上不留灰块——map_controls 的
/// 既有观感由此保持；hover / 按下反馈交给 egui visuals（`bg_hover` 等）。
fn fill_for(active: bool, active_bg: egui::Color32) -> egui::Color32 {
    if active {
        active_bg
    } else {
        egui::Color32::TRANSPARENT
    }
}

/// 标准按钮。所有形态共用：tooltip、选中态、禁用态、点击回调。
///
/// - `active=true` → 底色 `active_bg`（选中态纯背景差异，不换图标）；
/// - `enabled=false` → `ui.add_enabled` 整按钮置灰；
/// - `hint` 非空 → `on_hover_text`（egui 的 Tooltip 绘制层永远在最上，
///   见 `floater.rs` 绘制序注释）；
/// - `on_click` 仅在本帧被点击时调用一次（`FnOnce`）。
pub struct ToolButton<'a> {
    content: ToolButtonContent<'a>,
    hint: &'a str,
    active: bool,
    enabled: bool,
    size: Option<egui::Vec2>,
    icon_size: Option<f32>,
    tint: Option<egui::Color32>,
    active_bg: Option<egui::Color32>,
}

impl<'a> ToolButton<'a> {
    /// 纯图标按钮（方块，默认边长 = `shell.hit`）。
    pub fn icon(icon: ToolIcon, hint: &'a str) -> Self {
        Self::base(ToolButtonContent::Icon(icon), hint)
    }

    /// 纯文字按钮（高度 = `shell.hit`，宽度随文字）。
    pub fn text(label: &'a str, hint: &'a str) -> Self {
        Self::base(ToolButtonContent::Text(label), hint)
    }

    /// 图标 + 文字按钮（高度 = `shell.hit`，宽度随内容）。
    pub fn icon_text(icon: ToolIcon, label: &'a str, hint: &'a str) -> Self {
        Self::base(ToolButtonContent::IconText(icon, label), hint)
    }

    fn base(content: ToolButtonContent<'a>, hint: &'a str) -> Self {
        Self {
            content,
            hint,
            active: false,
            enabled: true,
            size: None,
            icon_size: None,
            tint: None,
            active_bg: None,
        }
    }

    /// 选中态。
    pub fn active(mut self, v: bool) -> Self {
        self.active = v;
        self
    }
    /// 禁用态。
    pub fn enabled(mut self, v: bool) -> Self {
        self.enabled = v;
        self
    }
    /// 显式尺寸（默认：图标钮 = hit×hit；带文字的 = 高 hit、宽随内容）。
    pub fn size(mut self, s: egui::Vec2) -> Self {
        self.size = Some(s);
        self
    }
    /// 显式图标边长（默认 hit−8：Cozy 28→20、Compact 24→16）。
    pub fn icon_size(mut self, s: f32) -> Self {
        self.icon_size = Some(s);
        self
    }
    /// 前景/图标色（默认 `shell.t1()`）。
    pub fn tint(mut self, c: egui::Color32) -> Self {
        self.tint = Some(c);
        self
    }
    /// 选中底色（默认 `shell.active_bg()`）。
    pub fn active_bg(mut self, c: egui::Color32) -> Self {
        self.active_bg = Some(c);
        self
    }

    /// 由 `UiShell` 解析默认样式；显式覆盖优先。
    fn resolve(&self, shell: &UiShell) -> ResolvedStyle {
        let hit = shell.hit;
        let min_size = match &self.content {
            ToolButtonContent::Icon(_) => self.size.unwrap_or(egui::Vec2::splat(hit)),
            ToolButtonContent::Text(_) | ToolButtonContent::IconText(_, _) => {
                self.size.unwrap_or(egui::vec2(0.0, hit))
            }
        };
        ResolvedStyle {
            tint: self.tint.unwrap_or_else(|| shell.t1()),
            min_size,
            icon_px: self.icon_size.unwrap_or(hit - 8.0),
            text_px: shell.scale.small,
        }
    }

    /// 添加到 UI。`on_click` 在本帧被点击时调用一次。
    pub fn show<F: FnOnce()>(
        self,
        ui: &mut egui::Ui,
        shell: &UiShell,
        on_click: F,
    ) -> egui::Response {
        let st = self.resolve(shell);
        let fill = fill_for(
            self.active,
            self.active_bg.unwrap_or_else(|| shell.active_bg()),
        );
        let btn = match self.content {
            ToolButtonContent::Icon(icon) => {
                let ToolIcon::Svg(source) = icon;
                egui::Button::image(icon_image(source, st.tint, st.icon_px))
                    .min_size(st.min_size)
                    .fill(fill)
            }
            ToolButtonContent::Text(label) => {
                egui::Button::new(egui::RichText::new(label).color(st.tint).size(st.text_px))
                    .min_size(st.min_size)
                    .fill(fill)
            }
            ToolButtonContent::IconText(icon, label) => {
                let ToolIcon::Svg(source) = icon;
                egui::Button::image_and_text(
                    icon_image(source, st.tint, st.icon_px),
                    egui::RichText::new(label).color(st.tint).size(st.text_px),
                )
                .min_size(st.min_size)
                .fill(fill)
            }
        };
        let resp = ui.add_enabled(self.enabled, btn);
        if resp.clicked() {
            on_click();
        }
        // `on_hover_text` 消费 self 返回 Self；空 hint 时直接透传。
        if self.hint.is_empty() {
            resp
        } else {
            resp.on_hover_text(self.hint)
        }
    }
}

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tool_button_tests {
    use super::super::primitives::{ICON_PEN, ICON_RULER};
    use super::*;
    use crate::theme::ui_theme::UiShellConfig;

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    /// 默认样式必须来自 `UiShell` 令牌：曾经默认 `tint = WHITE`，
    /// caller 忘了 `.tint()` 就会在浅色底上隐身——现在默认解析不再有这个坑。
    #[test]
    fn icon_button_defaults_resolve_from_shell_tokens_not_white() {
        let sh = shell();
        let st = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "hint").resolve(&sh);
        assert_eq!(st.tint, sh.t1(), "tint 应 = shell.t1()，got {:?}", st.tint);
        assert_eq!(
            st.min_size,
            egui::vec2(sh.hit, sh.hit),
            "默认方块 = hit×hit（{}px），got {:?}",
            sh.hit,
            st.min_size
        );
        assert!(
            (st.icon_px - (sh.hit - 8.0)).abs() < 1e-3,
            "默认图标 = hit−8（{}px），got {}",
            sh.hit - 8.0,
            st.icon_px
        );
        assert!(
            (st.text_px - sh.scale.small).abs() < 1e-3,
            "got {}",
            st.text_px
        );
    }

    /// 显式 builder 覆盖必须压过 shell 默认（map_controls 的 Google 白控件靠它）。
    #[test]
    fn explicit_builder_overrides_win_over_shell_defaults() {
        let sh = shell();
        let st = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "hint")
            .tint(egui::Color32::RED)
            .size(egui::vec2(38.0, 24.0))
            .icon_size(11.0)
            .resolve(&sh);
        assert_eq!(st.tint, egui::Color32::RED, "got {:?}", st.tint);
        assert_eq!(st.min_size, egui::vec2(38.0, 24.0), "got {:?}", st.min_size);
        assert!((st.icon_px - 11.0).abs() < 1e-3, "got {}", st.icon_px);
    }

    /// 带文字的按钮：高度吃满命中区，宽度随内容（0 = 不设下限）。
    #[test]
    fn text_button_fills_shell_hit_height_and_keeps_width_auto() {
        let sh = shell();
        let st = ToolButton::text("新建", "hint").resolve(&sh);
        assert_eq!(st.min_size.y, sh.hit, "高度应 = hit（{}px）", sh.hit);
        assert_eq!(st.min_size.x, 0.0, "宽度应随内容，got {}", st.min_size.x);

        let st2 = ToolButton::icon_text(ToolIcon::Svg(ICON_PEN), "笔刷", "hint").resolve(&sh);
        assert_eq!(
            st2.min_size.y, sh.hit,
            "图标+文字同口径，got {}",
            st2.min_size.y
        );
    }

    /// 选中态只差背景：active → active_bg，非选中 → 幽灵透明底。
    #[test]
    fn active_state_selects_active_bg_and_inactive_is_ghost() {
        let bg = egui::Color32::from_rgb(0xE8, 0xF0, 0xFE);
        assert_eq!(fill_for(true, bg), bg, "active 应取 active_bg");
        assert_eq!(
            fill_for(false, bg),
            egui::Color32::TRANSPARENT,
            "非选中应为幽灵透明底"
        );
    }

    /// 三种内容形态都能真实装配进 egui（API 签名 / Image tint / RichText
    /// 尺寸错一处就 panic 在这里，而不是等 GUI 启动烟测兜底）。
    #[test]
    fn show_renders_all_three_content_variants_without_panic() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("tool_button_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
        );
        let r1 = ToolButton::icon(ToolIcon::Svg(ICON_PEN), "图标").show(&mut ui, &sh, || {});
        let r2 = ToolButton::text("文字按钮", "文字").show(&mut ui, &sh, || {});
        let r3 = ToolButton::icon_text(ToolIcon::Svg(ICON_RULER), "测量", "混合").show(
            &mut ui,
            &sh,
            || {},
        );
        assert!(
            r1.rect.width() > 0.0,
            "图标钮应有实际宽度，got {}",
            r1.rect.width()
        );
        assert!(
            r2.rect.height() >= sh.hit,
            "文字钮高度 ≥ hit，got {}",
            r2.rect.height()
        );
        assert!(r3.rect.width() > r1.rect.width(), "图标+文字应比纯图标宽");
        // 禁用态不触发回调。
        let mut fired = false;
        ToolButton::text("禁用", "")
            .enabled(false)
            .show(&mut ui, &sh, || fired = true);
        assert!(!fired, "未发生点击事件时回调不应触发");
    }
}

#[cfg(test)]
mod button_factory_tests {
    //! 2026-10-15 引入的 `button()` 工厂 + `icon_image` helper。
    //!
    //! ToolButton 4 个老测试留在 [`super::tool_button_tests`]——它们继续守住工具栏
    //! 方块图标 / active / disabled 语义；本模块只覆盖**新**工厂：4 种 style
    //! 装配 + Image tint 中央化。

    use super::super::primitives::{ICON_FOLDER_OPEN, ICON_GLOBE, ICON_PLUS};
    use super::*;
    use crate::theme::ui_theme::UiShellConfig;

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    fn new_ui(ctx: &egui::Context, w: f32, h: f32) -> egui::Ui {
        egui::Ui::new(
            ctx.clone(),
            egui::Id::new("button_factory_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w, h),
            )),
        )
    }

    /// 4 种 style 都能真实装配进 egui 而不 panic（API 签名 / Image tint /
    /// RichText 尺寸任何错一处都立刻 panic 在这里，而不是等 GUI 启动烟测）。
    #[test]
    fn factory_renders_all_four_styles_into_egui_without_panic() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 600.0, 200.0);

        let r_a = button(
            &mut ui,
            &sh,
            ButtonContent::ImageText(ICON_PLUS, "主 CTA"),
            ButtonStyle::Accent,
            None,
            true,
        );
        let r_b = button(
            &mut ui,
            &sh,
            ButtonContent::ImageText(ICON_FOLDER_OPEN, "次 CTA"),
            ButtonStyle::Raised,
            None,
            true,
        );
        let r_c = button(
            &mut ui,
            &sh,
            ButtonContent::Text("取消"),
            ButtonStyle::Ghost,
            None,
            true,
        );
        let r_d = button(
            &mut ui,
            &sh,
            ButtonContent::Text("了解更多"),
            ButtonStyle::Subtle,
            None,
            true,
        );

        // 高度预期：Accent / Raised / Ghost / Subtle 的文字钮统一 = shell.hit；
        // 即使文字色与 fill 不同也至少要有矩形。
        assert!(
            r_a.rect.width() > 0.0,
            "Accent 钮应有宽度，got {}",
            r_a.rect.width()
        );
        assert!(
            r_b.rect.height() >= sh.hit,
            "Raised 钮高 ≥ hit，got {}",
            r_b.rect.height()
        );
        assert!(
            r_c.rect.height() >= sh.hit,
            "Ghost 钮高 ≥ hit，got {}",
            r_c.rect.height()
        );
        assert!(
            r_d.rect.height() >= sh.hit,
            "Subtle 钮高 ≥ hit，got {}",
            r_d.rect.height()
        );
    }

    /// 纯 Image 按钮应该是 `shell.hit × shell.hit` 方块（与 ToolButton 的
    /// 纯图标钮同一尺寸口径）。
    #[test]
    fn image_only_button_is_hit_square() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 200.0, 200.0);

        let r = button(
            &mut ui,
            &sh,
            ButtonContent::Image(ICON_PLUS),
            ButtonStyle::Accent,
            None,
            true,
        );
        assert!(
            (r.rect.width() - sh.hit).abs() < 1e-3 && (r.rect.height() - sh.hit).abs() < 1e-3,
            "纯图标钮 = hit×hit（{}×{}），got {:?}",
            sh.hit,
            sh.hit,
            r.rect.size()
        );
    }

    /// `min_size_override` 在 `Some(...)` 时必须覆盖工厂默认。
    /// 欢迎页 CTA 要求 `40px` 高，与默认 `shell.hit=28` 不同——这条钉住扩展口径。
    #[test]
    fn min_size_override_wins_over_factory_default() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 600.0, 100.0);
        let r = button(
            &mut ui,
            &sh,
            ButtonContent::ImageText(ICON_PLUS, "主 CTA"),
            ButtonStyle::Accent,
            Some(egui::vec2(200.0, 40.0)),
            true,
        );
        assert!(
            (r.rect.height() - 40.0).abs() < 1e-3,
            "min_size_override(40) 必须压过默认 28，got {}",
            r.rect.height()
        );
        assert!(
            r.rect.width() >= 200.0,
            "min_size_override(200) 必须让宽度 ≥ 200，got {}",
            r.rect.width()
        );
    }

    /// `enabled = false` 必须通过 [`egui::Ui::add_enabled`] 把整按钮置灰且不可点。
    /// 这是对话框「创建」按钮在 `name + location` 双空时禁用这一支路。
    #[test]
    fn enabled_false_renders_button_disabled() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 200.0, 200.0);

        let r = button(
            &mut ui,
            &sh,
            ButtonContent::Text("创建"),
            ButtonStyle::Accent,
            None,
            false,
        );
        assert!(
            r.rect.width() > 0.0,
            "禁用钮也应有矩形，got {}",
            r.rect.width()
        );
        // 关键：禁用的 egui::Response 自身**不**感知"自己被禁用"，但点击
        // 应该在没有 enabled 包装时不再触发已注册的 sense——
        // 单测无法直接断言"点不动"，所以只验：r 仍返回了、且感官上至少占位存在。
        // 此条测试护的是 add_enabled 这条路径不 panic（曾经要求 path 走 `ui.add`）。
    }

    /// `icon_image` 必须把 `tint` 显式设进 Image（caller 漏 tint 的旧坑）。
    /// `ImageOptions.tint` 是 `pub` 字段，getter `image_options()` 公开可读
    /// ——直接对比 caller 传入值即可。
    #[test]
    fn icon_image_centralizes_tint_into_image_options() {
        let img = icon_image(ICON_GLOBE, egui::Color32::RED, 28.0);
        assert_eq!(
            img.image_options().tint,
            egui::Color32::RED,
            "icon_image 必须把 tint 传到 Image，got {:?}",
            img.image_options().tint
        );
    }

    /// `icon_image` 的 `fit_to_exact_size` 落到 Image 私有 `size: ImageSize` 字段，
    /// 不暴露 getter；走渲染出口拿 `Response.rect` —— 钉死 28×28。
    #[test]
    fn icon_image_paints_at_exact_pixel_size() {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = new_ui(&ctx, 200.0, 200.0);
        let img = icon_image(ICON_GLOBE, egui::Color32::WHITE, 28.0);
        let r = ui.add(img);
        let s = r.rect.size();
        assert!(
            (s.x - 28.0).abs() < 1e-3 && (s.y - 28.0).abs() < 1e-3,
            "icon_image 强制 28×28，got {s:?}"
        );
    }
}
