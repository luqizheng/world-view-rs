//! 浮动面板外壳 + 自绘标题行。
//!
//! 单一职责：装配 `egui::Window` 风格的浮层。两层 API：
//!
//! - [`FloatWindow`] —— **公共浮窗外壳**（自 Figma `42:2` Inspector 面板提取）：
//!   任何浮层给 `id_salt` + 标题即可装配，不耦合 `Panel` 枚举。层序、自绘
//!   标题栏、折叠记忆、拖拽把手、缩放角标都在这里收敛。
//! - `float_window`（wv-app 域件，ADR-0026 D3） —— `Panel` 枚举特化薄包装：读 `PanelState` 可见性与
//!   `ui-layout.yaml` 跨会话布局，其余全部委托 [`FloatWindow`]（既有面板
//!   调用点零改动）。
//!
//! [`panel_head`] 是两者共用的自绘标题行（`pub(crate)` 给同目录复用）。
//!
//! 历史来源：拆自原 [`crate::widgets`] 中段（D1「面板是客人，地图是主体」 +
//! v0.3 Figma 重做后的自绘标题栏）；2026-10-17 按 Figma `42:2` 把外壳提取为
//! 公共组件并补拖拽把手 / 缩放角标。

use std::sync::Arc;

use bevy_egui::egui;

use crate::ui_theme::{color32, proportional};
use crate::ui_tokens::{font_size, radius, space};

use super::primitives::{icon_image, BadgeKind, ICON_GRID};
use crate::shell::UiShell;

/// 浮窗头部图标源：几何字符（**遗留形态**），或 Lucide SVG（设计稿形态）。
///
/// `wv-app` 的 13 个面板已全部改用 [`HeadIcon::Svg`]（2026-10-18）——几何
/// 字符里 ◈/◧/⬡ 在打包字体无字形、会落成豆腐块，`Char` 仅为外部调用兼容保留。
#[derive(Clone)]
pub enum HeadIcon {
    /// 几何字符，着 `t2`。
    Char(&'static str),
    /// Lucide SVG 图标（12×12），着 `t2`（Figma `3:22510` 描边 #A3B1BA）。
    Svg(egui::ImageSource<'static>),
}

/// 通用浮动窗体外壳（公共组件，Figma `42:2`「最外层浮动窗体」的代码对位）。
///
/// 与 `float_window`（wv-app 域件，ADR-0026 D3） 的分工：后者是 `Panel` 枚举特化（可见性 / 跨会话布局
/// 都由 `PanelState` + `ui-layout.yaml` 管），本结构是**解耦版**——未来场景
/// 编辑器、非面板浮窗直接给 `id_salt` + 标题装配，不必往 `Panel` 枚举里塞变体。
///
/// ## 层序约定（与 `float_window`（wv-app 域件，ADR-0026 D3） 共享，2026-10-14 固化）
///
/// 默认 `Order::Middle`：面板互斥点击由 egui「点谁谁置顶」管理，永远压在
/// Background 面板与 Bevy 地图之上。模态弹窗应显式 `.order(Order::Foreground)`
/// —— egui 绘制序 `Background < Middle < Foreground < Tooltip`，故弹窗压
/// 面板、悬浮提示仍在最上。新增浮层按此表对号入座。
///
/// ## 结构规格（Figma `3:20892` Layers / `42:2` Inspector 采样 @1x）
///
/// - 窗体：`radius::PANEL`=10 圆角、`border_strong` 1px 描边、`panel_bg`
///   按 `alpha` 合成（设计稿 `rgba(27,35,42,.96)`，色彩走令牌对位）；
/// - 头行：满宽、左右内边距 12、子项间距 8；顶行两角**跟窗体同半径**——
///   方角会啃掉窗体圆角（2026-10-18 实机截图确认）；
/// - 头行内容：`icon 12 | title | … | ⠿ | ⌄`，chevron 是描边形态（非填充
///   三角）；把手 = 2×3 圆点阵（打包字体缺盲文点字字形，painter 画点）；
/// - 面板体：四周内边距 12（`Panel content` padding），即内容不贴边；
/// - 右下角缩放角标（`resizable(true)` 时）：8×8 两条 45° 短线、距内缘 4 px
///   （`Resize handle` `3:20947`）。交互由 egui Window 边框自带，角标纯指示。
pub struct FloatWindow<'a> {
    id: egui::Id,
    title: &'a str,
    icon: HeadIcon,
    tag: Option<(BadgeKind, String)>,
    default_pos: Option<egui::Pos2>,
    default_size: Option<egui::Vec2>,
    default_open: bool,
    resizable: bool,
    alpha: f32,
    order: egui::Order,
    constrain: Option<egui::Rect>,
    current_pos: Option<egui::Pos2>,
}

/// 面板内容内边距（Figma `3:20892` Panel content `padding: 12`）。
///
/// 头行是满宽的（贴窗体内缘），体内容左右下各缩进 12——两者别混用同一个
/// margin 值，头行的 12 是 `paddingLeft/Right`，体的 12 是容器 padding。
const CONTENT_PAD: f32 = 12.0;
/// 头部左/右内边距（Figma `3:20893` Panel header `paddingLeft/Right: 12`）。
const HEAD_PAD: f32 = 12.0;
/// 头部子项间距（Figma itemSpacing 8）。
const HEAD_GAP: f32 = 8.0;
/// 头部图标尺寸（Figma 12×12）。
const HEAD_ICON: f32 = 12.0;

impl<'a> FloatWindow<'a> {
    /// 最小形态：egui 记忆 `Id`（拖动 / 折叠状态挂它上）+ 标题。
    ///
    /// `id_salt` 走 [`egui::Id::new`] 同款 `AsId` 约束——`&str` 与现成
    /// `egui::Id`（`float_window` 的 `("wv_panel", panel)` 元组）都能直接喂。
    pub fn new(id_salt: impl egui::AsId, title: &'a str) -> Self {
        Self {
            id: egui::Id::new(id_salt),
            title,
            // 缺省用 Lucide `layout-grid`：几何字符（`□`）在打包字体里会缺字形，
            // 落成豆腐块（Figma 头部图标一律 12×12 SVG，见 `3:20893`）。
            icon: HeadIcon::Svg(ICON_GRID),
            tag: None,
            default_pos: None,
            default_size: None,
            default_open: true,
            resizable: true,
            alpha: 0.96,
            order: egui::Order::Middle,
            constrain: None,
            current_pos: None,
        }
    }

    /// 头部图标（默认几何字符占位）。
    pub fn icon(mut self, icon: HeadIcon) -> Self {
        self.icon = icon;
        self
    }

    /// 标题栏右侧徽标（如 `ACTIVE`）。
    pub fn tag(mut self, tag: Option<(BadgeKind, String)>) -> Self {
        self.tag = tag;
        self
    }

    /// 首帧缺省位置（此后由 egui 按 `Id` 记忆拖动结果）。
    pub fn default_pos(mut self, p: egui::Pos2) -> Self {
        self.default_pos = Some(p);
        self
    }

    /// 首帧缺省尺寸。
    pub fn default_size(mut self, s: egui::Vec2) -> Self {
        self.default_size = Some(s);
        self
    }

    /// 首帧缺省折叠态（默认展开；此后由 egui 按 `Id` 记忆）。
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// 是否可拖边缩放（默认 true，同时控制右下角角标）。
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// 整窗不透明度（沉浸模式淡化用；默认 0.96）。
    pub fn alpha(mut self, a: f32) -> Self {
        self.alpha = a;
        self
    }

    /// 层序（默认 `Middle`；模态浮层显式给 `Foreground`，见结构文档层序表）。
    pub fn order(mut self, order: egui::Order) -> Self {
        self.order = order;
        self
    }

    /// 拖动 / 缩放的活动范围钳制（缺省 None = egui 默认整视口）。
    ///
    /// 无边框窗（2026-10-25）后应用自带常驻顶栏与状态栏：调用方把浮窗
    /// 钳进「中央工作区」，既拖不进两端 chrome、也拖不出视口——按大窗口
    /// 保存的 `ui-layout.yaml` 坐标在小窗口下会被逐帧钳回可见区。
    pub fn constrain_to(mut self, constrain_rect: egui::Rect) -> Self {
        self.constrain = Some(constrain_rect);
        self
    }

    /// 本帧强制位置（越界回拉用，正常帧传 None）。
    ///
    /// ⚠️ egui 0.36 的 `constrain_to` **不钳制位置**——只钳尺寸 / 锚点 /
    /// move 命中区（`area.rs` 通读无 position clamp），存下的位置照样可以
    /// 压进 chrome。调用方每帧读 `area_rect` 对活动范围做越界检测，越界才
    /// 传本值（egui 用它覆写本帧位置，正常帧不传则不干扰用户拖拽）。
    pub fn current_pos(mut self, pos: egui::Pos2) -> Self {
        self.current_pos = Some(pos);
        self
    }

    /// 装配并落地。`body` 在折叠态不执行。
    pub fn show<F: FnOnce(&mut egui::Ui)>(self, ctx: &egui::Context, shell: &UiShell, body: F) {
        let head_id = self.id.with("head");
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            ctx,
            head_id,
            self.default_open,
        );

        let mut win = egui::Window::new(self.title)
            .id(self.id)
            .resizable(self.resizable)
            .collapsible(false)
            .title_bar(false)
            .order(self.order)
            .frame(
                // 跟随应用当前日夜模式（亮色下面板阴影走亮色样式）。
                egui::Frame::window(&ctx.style_of(ctx.theme()))
                    .fill(shell.panel_bg().gamma_multiply(self.alpha))
                    .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                    .inner_margin(crate::ui_theme::margin_all(0.0))
                    .corner_radius(radius::PANEL),
            );
        if let Some(p) = self.default_pos {
            win = win.default_pos(p);
        }
        if let Some(s) = self.default_size {
            win = win.default_size(s);
        }
        if let Some(r) = self.constrain {
            win = win.constrain_to(r);
        }
        if let Some(p) = self.current_pos {
            win = win.current_pos(p);
        }

        let icon = self.icon;
        let tag = self.tag;
        let alpha = self.alpha;
        // egui Window 自带 3 线缩放角标（`resize_corner_size` 缺省 12）；设计稿
        // `Resize handle` 是 8×8 两笔。临时关掉内置角标、由本文件自绘，画完
        // 立刻恢复（只影响本次 show 的绘制样式）。
        let theme = ctx.theme();
        let prev_corner = ctx.style_of(theme).visuals.resize_corner_size;
        if self.resizable {
            ctx.style_mut_of(theme, |s| s.visuals.resize_corner_size = 0.0);
        }
        win.show(ctx, |ui| {
            panel_head(
                ui,
                shell,
                &icon,
                self.title,
                tag.as_ref(),
                &mut state,
                alpha,
            );
            if state.is_open() {
                // 折叠关闭时不画体。内容四周各留 12（Figma `Panel content`
                // padding），头行贴边、体不贴边。
                let body = egui::Frame::new()
                    .inner_margin(crate::ui_theme::margin_all(CONTENT_PAD))
                    .show(ui, body);
                if self.resizable {
                    paint_resize_grip(ui, shell, body.response.rect);
                }
            } else {
                state.store(ctx);
            }
        });
        if self.resizable {
            ctx.style_mut_of(theme, |s| s.visuals.resize_corner_size = prev_corner);
        }
    }
}

/// 右下角缩放角标：两条 45° 短线（Figma `3:20947` / `42:55`，8×8 双斜线）。
///
/// 纯视觉指示——缩放交互由 egui Window 可拖边框自带。`body_rect` 是内容
/// 外框（已含 12 padding），角标再外扩 8 → 距窗体右下内缘 4 px，与设计稿
/// `Resize handle` 同位（折叠态不画）。
fn paint_resize_grip(ui: &mut egui::Ui, shell: &UiShell, body_rect: egui::Rect) {
    let br = body_rect.right_bottom() + egui::vec2(8.0, 8.0);
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.0, shell.t3());
    // 长臂 8（对角整条），短臂 8−5.33≈2.67（Figma path 第二段 5.333→8）。
    p.line_segment(
        [egui::pos2(br.x - 8.0, br.y), egui::pos2(br.x, br.y - 8.0)],
        stroke,
    );
    p.line_segment(
        [
            egui::pos2(br.x - 8.0 + 5.333, br.y),
            egui::pos2(br.x, br.y - 8.0 + 5.333),
        ],
        stroke,
    );
}

/// 自绘 `panel-head` 行（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU`，2026-10-17 按
/// Figma `42:2` 补拖拽把手）：
///
/// `icon | title | … | ⠿ | ⌄` 水平一字排列，`⌄` 随 openness 在「右尖」与
/// 「下尖」之间插值（Lucide chevron 形态，非填充三角）。chevron 点中切
/// [`egui::collapsing_header::CollapsingState`]。整个头可视为
/// `egui::Window` 的 drag-anywhere 区域的一份（egui 0.36 `title_bar=false`
/// 会把 `OnTouch` 降级为 `Anywhere`，无需自管 `area_mut`）。
///
/// 字号 `font_size::SMALL = 12 px` —— Figma body 同号；徽标继续吃
/// `font_size::MICRO = 11 px`（Figma 9 px 偏小，会顺走其它调用点，暂时不动）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel_head(
    ui: &mut egui::Ui,
    shell: &UiShell,
    icon: &HeadIcon,
    title: &str,
    tag: Option<&(BadgeKind, String)>,
    state: &mut egui::collapsing_header::CollapsingState,
    alpha: f32,
) {
    let head_h = shell.head_h;
    let avail_w = ui.available_width();
    let (head_rect, _head_resp) =
        ui.allocate_exact_size(egui::vec2(avail_w, head_h), egui::Sense::hover());

    // ── 右件几何先算（互不引用 ui，便于可变 borrow 切回） ──
    let center_y = head_rect.center().y;
    // 折叠 chevron：12×12，右缘距头右缘 12（Figma `3:22512` 12×12 @ 右 12）。
    let chev_rect = egui::Rect::from_min_size(
        egui::pos2(
            head_rect.max.x - HEAD_PAD - HEAD_ICON,
            center_y - HEAD_ICON / 2.0,
        ),
        egui::vec2(HEAD_ICON, HEAD_ICON),
    );
    // 拖拽把手（Figma `⠿`）：2×3 圆点阵，紧贴 chevron 左侧 8 px。打包字体
    // （Inter + Noto SC）缺盲文点字字形，画点比画字稳。
    let dots_gap = 3.0_f32;
    let dots_right = chev_rect.min.x - HEAD_GAP;
    let dots_cx = [dots_right - 2.0, dots_right - 5.0];

    // ── 算徽标矩形（含则备好） ──
    let badge_rect: Option<(
        egui::Rect,
        egui::Color32,
        egui::Color32,
        Arc<egui::Galley>,
        egui::Vec2,
    )> = if let Some((kind, text)) = tag {
        let colors = shell.palette.badge((*kind).into());
        let fg = color32(colors.fg);
        let bg = crate::ui_theme::color32_alpha(colors.fill, colors.fill_alpha);
        let badge_pad = egui::vec2(space::XS * 1.5, 1.0);
        let font = proportional(font_size::MICRO);
        let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
        let size = galley.size() + badge_pad * 2.0;
        let right_edge = dots_cx[1] - 1.5 - HEAD_GAP;
        let r = egui::Rect::from_min_size(
            egui::pos2(right_edge - size.x, center_y - size.y / 2.0),
            egui::vec2(size.x, size.y),
        );
        Some((r, fg, bg, galley, badge_pad))
    } else {
        None
    };

    // ── 徽标占位：参与 hit-test 排序，但不抢拖动 ──
    if let Some((r, _, _, _, _)) = badge_rect.as_ref() {
        let _ = ui.allocate_rect(*r, egui::Sense::hover());
    }

    // ── 自绘背景、底线、icon、title、徽标、拖拽把手、折叠图标 ──
    let openness = state.openness(ui.ctx());
    let p = ui.painter();
    // 头底 = panel_bg（与窗体无缝拼接）。顶部两角必须跟窗体同半径：若按方角
    // 铺满，会把窗体圆角描边「啃」掉一块（2026-10-18 实机截图确认）。
    // 折叠态头就是整个窗体，底部两角同样要跟随窗体圆角。
    let r = radius::PANEL as u8;
    let bottom_r = (((1.0 - openness.clamp(0.0, 1.0)) * radius::PANEL).round()) as u8;
    let head_radius = egui::CornerRadius {
        nw: r,
        ne: r,
        sw: bottom_r,
        se: bottom_r,
    };
    p.rect_filled(
        head_rect,
        head_radius,
        shell.panel_bg().gamma_multiply(alpha),
    );
    // 底线 1px border_subtle（Figma Panel header 只描底边，`3:20893`）。
    p.line_segment(
        [
            egui::pos2(head_rect.min.x, head_rect.max.y),
            egui::pos2(head_rect.max.x, head_rect.max.y),
        ],
        egui::Stroke::new(1.0, shell.border_subtle()),
    );

    let title_font = proportional(font_size::SMALL); // 12 px = Figma body

    // 左：icon + title。Figma `3:20893`：12 内边距 + 12 图标 + 8 间距 → 标题 x=32。
    let title_x = match icon {
        HeadIcon::Char(ch) => {
            p.text(
                egui::pos2(head_rect.min.x + HEAD_PAD, center_y),
                egui::Align2::LEFT_CENTER,
                *ch,
                title_font.clone(),
                shell.t2(),
            );
            head_rect.min.x + HEAD_PAD + 18.0
        }
        HeadIcon::Svg(_) => head_rect.min.x + HEAD_PAD + HEAD_ICON + HEAD_GAP,
    };
    p.text(
        egui::pos2(title_x, center_y),
        egui::Align2::LEFT_CENTER,
        title,
        title_font,
        shell.t1(),
    );

    // 右：徽标 → 拖拽把手 → 折叠 chevron。
    if let Some((r, fg, bg, galley, pad)) = badge_rect {
        p.rect_filled(r, radius::BADGE, bg);
        p.galley(r.min + pad, galley, fg);
    }
    // ⠿ 点阵：设计稿 t3（#778994）。
    for cx in dots_cx {
        for row in 0..3 {
            let cy = center_y + (row as f32 - 1.0) * dots_gap;
            p.circle_filled(egui::pos2(cx, cy), 1.0, shell.t3());
        }
    }
    let _ = p; // 释放对 ui 的不可变借用，让 `paint_at` / `interact` 可借 ui。

    // SVG 头图标：t2（Figma `3:22510` 描边 #A3B1BA），12×12 居左内边距。
    if let HeadIcon::Svg(src) = icon {
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(head_rect.min.x + HEAD_PAD, center_y - HEAD_ICON / 2.0),
            egui::vec2(HEAD_ICON, HEAD_ICON),
        );
        // `paint_at` 而非 `put`：put 会把光标推进到图标矩形，后面的面板体
        // 会从头上沿开始（压住头行）。
        icon_image(src.clone(), shell.t2(), HEAD_ICON).paint_at(ui, icon_rect);
    }

    // chevron 点中切 state（12×12 命中区比 6×3 的笔画大，好点）。
    let tri_resp = ui.interact(chev_rect, ui.id().with("tri"), egui::Sense::click());
    paint_chevron(ui, shell, chev_rect.center(), openness);
    if tri_resp.clicked() {
        state.toggle(ui);
    }
    state.store(ui.ctx());
}

/// 描边 chevron（Figma `3:22513` 6×3 圆头两笔）：openness 0 = 右尖 `›`，
/// 1 = 下尖 `˅`，中间线性插值。
///
/// 不用 `egui::collapsing_header::paint_default_icon`——它画的是**填充三角**，
/// 与设计稿的细线 chevron 形态不同（实机截图对比确认）。
pub(crate) fn paint_chevron(ui: &egui::Ui, shell: &UiShell, center: egui::Pos2, openness: f32) {
    let t = openness.clamp(0.0, 1.0);
    let lerp = |a: egui::Vec2, b: egui::Vec2| a + (b - a) * t;
    let a = lerp(egui::vec2(-1.5, -3.0), egui::vec2(-3.0, -1.5));
    let tip = lerp(egui::vec2(1.5, 0.0), egui::vec2(0.0, 1.5));
    let b = lerp(egui::vec2(-1.5, 3.0), egui::vec2(3.0, -1.5));
    ui.painter().add(egui::Shape::line(
        vec![center + a, center + tip, center + b],
        egui::Stroke::new(1.5_f32, shell.t2()),
    ));
}

// ── 测试 ────────────────────────────────────────────────────────────
//
// `FloatWindow::show` 会装配 `egui::Window`——注册的 texture delta 在 unit
// 上下文无 GPU surface，drop 时必撞「Dropped TexturesDelta」（DialogWindow
// 同款教训），因此**不**在单测里跑 `show`；外壳真实装配由
// `scripts/smoke-gui.sh` 兜底。`panel_head` 是纯 `Ui` 级绘制，可以 headless 跑。

#[cfg(test)]
mod tests {
    use super::super::primitives::ICON_CHEVRON_DOWN;
    use super::*;
    use crate::widgets::test_support::shell;

    /// 构造一个 `egui::Ui` 跑在 `400×300` 矩形里，供 `panel_head` 落地用。
    fn make_ui() -> (egui::Context, egui::Ui) {
        crate::widgets::test_support::make_ui("floater_tests", 400.0, 300.0)
    }

    #[test]
    fn float_window_builder_defaults_match_panel_spec() {
        let fw = FloatWindow::new("test_float", "标题");
        assert_eq!(fw.title, "标题");
        assert!(fw.default_open, "默认展开");
        assert!(fw.resizable, "默认可缩放（角标随行）");
        assert!(
            (fw.alpha - 0.96).abs() < 1e-3,
            "默认 alpha = 0.96（Figma rgba(27,35,42,.96) 对位），got {}",
            fw.alpha
        );
        assert_eq!(fw.order, egui::Order::Middle, "浮层缺省 Middle 层序");
        assert!(fw.default_pos.is_none(), "位置缺省交给 egui 级联");
        assert!(fw.default_size.is_none(), "尺寸缺省交给 egui 级联");
        assert!(fw.constrain.is_none(), "活动范围缺省交给 egui 整视口");
    }

    #[test]
    fn float_window_builder_overrides_win_over_defaults() {
        let fw = FloatWindow::new("test_float2", "标题")
            .icon(HeadIcon::Svg(ICON_CHEVRON_DOWN))
            .default_pos(egui::pos2(10.0, 20.0))
            .default_size(egui::vec2(288.0, 328.0))
            .default_open(false)
            .resizable(false)
            .alpha(0.10)
            .order(egui::Order::Foreground)
            .constrain_to(egui::Rect::from_min_size(
                egui::pos2(10.0, 20.0),
                egui::vec2(200.0, 100.0),
            ));
        assert!(matches!(fw.icon, HeadIcon::Svg(_)));
        assert_eq!(fw.default_pos, Some(egui::pos2(10.0, 20.0)));
        assert_eq!(fw.default_size, Some(egui::vec2(288.0, 328.0)));
        assert!(!fw.default_open);
        assert!(!fw.resizable);
        assert!((fw.alpha - 0.10).abs() < 1e-3, "got {}", fw.alpha);
        assert_eq!(fw.order, egui::Order::Foreground);
        assert_eq!(
            fw.constrain,
            Some(egui::Rect::from_min_size(
                egui::pos2(10.0, 20.0),
                egui::vec2(200.0, 100.0)
            ))
        );
    }

    #[test]
    fn float_window_accepts_plain_str_and_egui_id_alike() {
        // `impl Hash` 入参：&str 与现成 Id 都能喂（float_window 走元组 Id）。
        let _a = FloatWindow::new("plain_str", "a");
        let _b = FloatWindow::new(egui::Id::new(("wv_panel", 1)), "b");
    }

    #[test]
    fn panel_head_renders_char_icon_without_panic() {
        let sh = shell();
        let (ctx, mut ui) = make_ui();
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            &ctx,
            egui::Id::new("head_char"),
            true,
        );
        panel_head(
            &mut ui,
            &sh,
            &HeadIcon::Char("◎"),
            "Inspector",
            None,
            &mut state,
            0.96,
        );
        // 头行高度 = shell.head_h（密度感知），绘制即占位。
        assert!(
            (ui.min_rect().height() - sh.head_h).abs() < 1e-3,
            "头行高应 = head_h（{}），got {}",
            sh.head_h,
            ui.min_rect().height()
        );
    }

    #[test]
    fn chevron_paints_at_both_extremes_without_panic() {
        let sh = shell();
        let (_ctx, ui) = make_ui();
        for t in [0.0, 0.5, 1.0] {
            paint_chevron(&ui, &sh, egui::pos2(50.0, 50.0), t);
        }
    }

    #[test]
    fn content_padding_matches_figma_12() {
        assert!(
            (CONTENT_PAD - 12.0).abs() < 1e-6,
            "Figma Panel content padding 12"
        );
        assert!(
            (HEAD_PAD - 12.0).abs() < 1e-6,
            "Figma Panel header padding 12"
        );
        assert!((HEAD_ICON - 12.0).abs() < 1e-6, "Figma 头图标 12×12");
    }

    #[test]
    fn panel_head_renders_svg_icon_and_badge_without_panic() {
        let sh = shell();
        let (ctx, mut ui) = make_ui();
        let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
            &ctx,
            egui::Id::new("head_svg"),
            true,
        );
        panel_head(
            &mut ui,
            &sh,
            &HeadIcon::Svg(ICON_CHEVRON_DOWN),
            "Inspector",
            Some(&(BadgeKind::Ok, "ACTIVE".to_string())),
            &mut state,
            0.96,
        );
        assert!(ui.min_rect().width() > 0.0);
    }
}
