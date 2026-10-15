//! 共享 widget helper：徽标 / 空状态 / 列表项 / 浮动窗口外壳 / 面板头。
//!
//! ## 职责
//! - `BadgeKind` + `badge` —— 药丸型徽标（中性 / 通过 / 警告 / 错误 / 事件）。
//! - `empty_state` —— "图标 + 一句人话 + 一个可点的下一步"占位卡。
//! - `list_item` —— 类型色块 + 主标题 + 等宽元信息。
//! - `terrain_grid` —— 笔刷面板用的 2×5 地形色块网格（被 [`crate::ui::brush_panel`] 调用）。
//! - `float_window` / `panel_header` —— 浮动面板外壳 + 自绘标题栏三段。
//! - `ToolButton` —— **标准按钮**（标准化控件第 1 项）：图标 / 文字 / 图标+文字
//!   三形态 + tooltip + 选中/禁用态，默认样式由 `UiShell` 令牌解析。
//!
//! ## 边界（不做什么）
//! - **不**装配具体业务面板内容 —— 各 `*_panel.rs` 调用 `float_window` 装自己。
//! - **不**改主题/令牌 —— 调 `shell::UiShell` getter 拿颜色。
//!
//! ## 主要导出
//! - `BadgeKind` / `badge`
//! - `empty_state` / `list_item` / `terrain_grid`
//! - `float_window` / `panel_header`
//! - `ToolButton` / `ToolButtonContent` / `ToolIcon` / `ICON_*`

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{Panel, PanelState};
use crate::theme::ui_theme::{color32, mono, proportional};
use crate::theme::ui_tokens::{font_size, radius, space, BadgeTone};
use crate::ui_layout::LayoutFile;

use super::shell::{terrain_swatch, UiShell};

/// 徽标型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    /// 中性计数。
    Neutral,
    /// 校验通过。
    Ok,
    /// 告警。
    Warn,
    /// 错误。
    Err,
    /// 事件相关。
    Event,
}

impl From<BadgeKind> for BadgeTone {
    fn from(kind: BadgeKind) -> Self {
        match kind {
            BadgeKind::Neutral => BadgeTone::Neutral,
            BadgeKind::Ok => BadgeTone::Ok,
            BadgeKind::Warn => BadgeTone::Warn,
            BadgeKind::Err => BadgeTone::Err,
            BadgeKind::Event => BadgeTone::Event,
        }
    }
}

/// 画一个徽标（药丸，4px 圆角）。
///
/// 前景与底色**都**取令牌（§2.2.1）：底色是「世界语义色 × 标定 alpha」，
/// 且必须走非预乘合成——`from_rgba_premultiplied(r,g,b,41)` 那种写法拿到的是
/// 「全强度 RGB + 低 alpha」，比原型的 `rgba(r,g,b,.16)` 亮一大截。
///
/// 语义前景随主题变（亮色下必须换深色系），底色不变：这是 D2 的落点。
pub fn badge(ui: &mut egui::Ui, shell: &UiShell, kind: BadgeKind, text: &str) {
    let colors = shell.palette.badge(kind.into());
    let fg = color32(colors.fg);
    let bg = crate::theme::ui_theme::color32_alpha(colors.fill, colors.fill_alpha);
    let pad = egui::vec2(space::XS * 1.5, 1.0);
    let font = proportional(font_size::MICRO);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, radius::BADGE, bg);
    // egui 0.30 的 `Painter::galley` 需要一个兜底色（字体未命中字形时使用）。
    ui.painter().galley(rect.min + pad, galley, fg);
}

/// 空状态卡（§5.2）：图标 + 一句人话 + 一个可点的下一步。
pub fn empty_state(
    ui: &mut egui::Ui,
    shell: &UiShell,
    icon: &str,
    title: &str,
    hint: &str,
    action: &str,
) -> bool {
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        ui.label(
            egui::RichText::new(icon)
                .size(shell.scale.title * 1.6)
                .color(shell.t3()),
        );
        ui.add_space(space::SM);
        ui.label(
            egui::RichText::new(title)
                .size(shell.scale.title)
                .color(shell.t1()),
        );
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new(hint)
                .size(shell.scale.small)
                .color(shell.t3()),
        );
        ui.add_space(space::MD);
        if ui
            .add(egui::Button::new(action).min_size(egui::vec2(120.0, shell.hit)))
            .clicked()
        {
            clicked = true;
        }
    });
    clicked
}

/// 列表项（§4.6）：类型色块 + 主标题 + 等宽元信息。
pub fn list_item(
    ui: &mut egui::Ui,
    shell: &UiShell,
    swatch: egui::Color32,
    title: &str,
    meta: &str,
    selected: bool,
) -> egui::Response {
    let row_h = shell.row + space::SM;
    let avail_w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail_w, row_h), egui::Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, radius::CONTROL, shell.active_bg());
        rect_stroke(
            painter,
            rect,
            radius::CONTROL,
            egui::Stroke::new(1.0_f32, shell.accent()),
        );
    } else if resp.hovered() {
        painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
    }
    // 类型色块。
    let sw = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + space::SM, rect.center().y - 5.0),
        egui::vec2(10.0, 10.0),
    );
    painter.rect_filled(sw, radius::BADGE, swatch);
    // 主标题。
    painter.text(
        egui::pos2(rect.min.x + space::SM + 10.0 + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        proportional(shell.scale.base),
        shell.t1(),
    );
    // 等宽元信息（右对齐）。
    painter.text(
        egui::pos2(rect.max.x - space::SM, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        meta,
        mono(shell.scale.micro),
        shell.t3(),
    );
    resp
}

/// 描边矩形（统一 `StrokeKind::Inside`）。
///
/// egui 0.36 给 `Painter::rect_stroke` 加了第 4 个参数 `StrokeKind`（描边画在
/// 边界内 / 中 / 外）。本工程的矩形描边都是「选中框 / 视口框」，必须落在矩形**内部**
/// 才不会被邻居裁切吞掉，故收敛成一个实现点。
pub(crate) fn rect_stroke(
    painter: &egui::Painter,
    rect: egui::Rect,
    corner_radius: impl Into<egui::CornerRadius>,
    stroke: impl Into<egui::Stroke>,
) {
    painter.rect_stroke(rect, corner_radius, stroke, egui::StrokeKind::Inside);
}

/// 地形色块网格（§4.5：2 列 × 5 行，单行色块+名称，整块紧凑）。
///
/// 行高 22 px（曾 44 px），色块 14 px 居左、名称 11 px 居右同一行——
/// 高度砍半但视觉仍能一眼区分当前选中（背景填 `active_bg` + 1.5 px
/// `focus` 描边），网格外的所有状态提示交还给面板体。
pub fn terrain_grid(ui: &mut egui::Ui, shell: &UiShell, selected: &mut u8) {
    egui::Grid::new("terrain_swatches")
        .num_columns(2)
        .spacing([space::XS, space::XS])
        .show(ui, |ui| {
            for k in 0u8..10 {
                let name = super::shell::TERRAIN_NAMES[k as usize];
                let is_sel = *selected == k;
                let cell_w = (ui.available_width() - space::XS) / 2.0;
                let row_h = 22.0;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(cell_w, row_h), egui::Sense::click());
                let painter = ui.painter();
                if is_sel {
                    painter.rect_filled(rect, radius::CONTROL, shell.active_bg());
                    rect_stroke(
                        painter,
                        rect,
                        radius::CONTROL,
                        egui::Stroke::new(1.5_f32, shell.focus()),
                    );
                } else if resp.hovered() {
                    painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
                }
                // 14px 色块，垂直居中。
                let sw_size = 14.0;
                let sw = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + space::SM, rect.center().y - sw_size / 2.0),
                    egui::vec2(sw_size, sw_size),
                );
                painter.rect_filled(sw, radius::BADGE, terrain_swatch(k));
                // 11px 名称，色块右侧，垂直居中。
                painter.text(
                    egui::pos2(
                        rect.min.x + space::SM + sw_size + space::SM,
                        rect.center().y,
                    ),
                    egui::Align2::LEFT_CENTER,
                    name,
                    proportional(11.0),
                    if is_sel { shell.t1() } else { shell.t2() },
                );
                if resp.clicked() {
                    *selected = k;
                }
                if (k + 1) % 2 == 0 {
                    ui.end_row();
                }
            }
        });
}

/// 浮动面板外壳：标题进原生标题栏行 + 可拖动，可折叠，可关闭。
///
/// `layouts` 提供**跨会话**的位置/尺寸（`ui-layout.yaml`）；本帧的拖动结果由
/// egui 按窗口 `Id` 自己记住，并由 [`crate::ui_shell::persist_layout_on_change`]
/// 回写磁盘（D1：面板是客人，地图是主体）。
///
/// ## 层序约定（2026-10-14 用户要求「弹窗/浮动都在最顶层」后固化）
///
/// 本外壳**不设** `order`——即 egui 缺省 `Middle`：面板互斥点击由 egui 的
/// 「点谁谁置顶」管理，且永远压在 egui Background 面板（顶栏/状态栏）与
/// Bevy 地图之上。模态弹窗（新建地图 / 新建工程 / 命令面板 / 欢迎页 /
/// 启动加载）在 `Order::Foreground`—— egui 绘制序
/// `Background < Middle < Foreground < Tooltip`，故弹窗压面板、悬浮提示仍在最上。
/// 新增浮层时按此表对号入座，不要把弹窗放回 Middle。
///
/// ## 标题为什么在原生标题栏里（2026-10-14 用户裁定）
///
/// 旧实现用空标题窗口（原生栏只剩 ▼ / × 两个按钮）+ 体内自绘标题行，代价是：
/// ① 窗体多出一整行高度；② 折叠后只剩空标题栏，看不出折起来的是哪个面板。
/// 把「图标 + 标题」直接交给 `Window::new`（egui 0.36 的标题栏布局是
/// 「▼ … 标题 … ×」），两个问题同时消失。`panel_header` 自绘行随之退役。
pub fn float_window(
    ctx: &egui::Context,
    shell: &UiShell,
    layouts: &LayoutFile,
    panels: &mut PanelState,
    panel: Panel,
    tag: Option<(BadgeKind, String)>,
    body: impl FnOnce(&mut egui::Ui),
) {
    if !panels.is_visible(panel) {
        return;
    }

    let layout = crate::ui_layout::current_layout(layouts, panels, panel);
    let mut open = true;
    let id = egui::Id::new(("wv_panel", panel));

    // 沉浸模式：整体淡化但**仍可交互**（D3：状态永不抑制）。
    let alpha = if panels.immersive { 0.10 } else { 0.96 };

    let win = egui::Window::new(format!("{} {}", panel.icon(), panel.title()))
        .id(id)
        .open(&mut open)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .default_open(!layout.collapsed)
        .resizable(true)
        .collapsible(true)
        .title_bar(true)
        .frame(
            egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(crate::theme::ui_theme::margin_all(space::SM))
                .corner_radius(radius::PANEL),
        );

    win.show(ctx, |ui| {
        // 徽标（当前无调用方传入，保留入口）：画在体首，不占标题栏。
        if let Some((kind, text)) = tag {
            badge(ui, shell, kind, &text);
            ui.add_space(space::XS);
        }
        body(ui);
    });

    if !open {
        panels.set_visible(panel, false);
    }
}

// ── 标准按钮（标准化控件 · 第 1 项）──────────────────────────────────
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
// 图标：Lucide 24×24 outline，SVG 内 `stroke="currentColor" fill="none"`，
// 由 `tint()` 注入主题色——与 `RichText::color()` 同效果；
// `include_image!` 编译期嵌入二进制，零运行时 IO。
//
// 边界：单一按钮封装，**不**承载浮窗外壳/分组/分隔线（各面板自管）；
// 不写主题令牌源头。

/// 工具栏图标源。SVG 走 `egui::include_image!`，编译期嵌入二进制，零运行时 IO。
#[derive(Clone)]
pub enum ToolIcon {
    /// Lucide / 任意合规 SVG 源码（`include_str!`/宏嵌入），`stroke="currentColor"`。
    Svg(egui::ImageSource<'static>),
}

// 工具图标常量。`include_image!` 宏展开为
// `ImageSource::Bytes { uri: "bytes://...", bytes: include_bytes!(path) }`，
// egui 0.36 看 `.svg` 后缀自动用内置 SVG 加载器解析 `currentColor`。
pub const ICON_PEN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/pen.svg");
pub const ICON_RULER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/ruler.svg");
pub const ICON_SQUARE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/square.svg");
pub const ICON_ERASER: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/eraser.svg");
pub const ICON_ZOOM_IN: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/zoom-in.svg");
pub const ICON_ZOOM_OUT: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/zoom-out.svg");
pub const ICON_MAXIMIZE: egui::ImageSource<'static> =
    egui::include_image!("../../assets/ui/tool_icons/maximize.svg");

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
///   见 `widgets.rs` 绘制序注释）；
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
                let image = egui::Image::new(source)
                    .tint(st.tint)
                    .fit_to_exact_size(egui::Vec2::splat(st.icon_px));
                egui::Button::image(image).min_size(st.min_size).fill(fill)
            }
            ToolButtonContent::Text(label) => {
                egui::Button::new(egui::RichText::new(label).color(st.tint).size(st.text_px))
                    .min_size(st.min_size)
                    .fill(fill)
            }
            ToolButtonContent::IconText(icon, label) => {
                let ToolIcon::Svg(source) = icon;
                let image = egui::Image::new(source)
                    .tint(st.tint)
                    .fit_to_exact_size(egui::Vec2::splat(st.icon_px));
                egui::Button::image_and_text(
                    image,
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

// ── egui 图像解码器装配（工具栏 SVG 渲染前置）─────────────────────────
//
// egui 0.36 的 loader 链（`egui/src/load.rs` 顶部文档）共三级：
//   ImageSource::load
//     ├► Context::include_bytes   （BytesLoader：取原始字节）
//     ├► Context::try_load_image  （ImageLoader：字节 → 像素）★ 解码
//     └► try_load_texture         （TextureLoader：像素 → 纹理）
//
// **修正（2026-10-15，推翻本节旧结论）**：旧版注解说「bevy_egui 不注册
// DefaultBytesLoader / DefaultTextureLoader，需手动装」——误诊。两者都是
// `Loaders::default()` 的出厂件；真正的缺口是 **ImageLoader：egui core
// 一个解码器都不带**（出厂 `image: Vec::new()`），SVG 字节永远解不成像素，
// `Image` 只能画红三角占位（用户截图所见）。旧代码手动 push 的两个 loader
// 实为重复注册（`add_*_loader` 不去重），一并移除。
//
// SVG 解码器只在官方配套 crate `egui_extras`（`svg` feature → resvg；
// `install_image_loaders` 在该 feature 下仅装 `SvgLoader`，且
// `is_loader_installed` 幂等）。依赖审视见根 Cargo.toml 注释。
//
// 时序：bevy_egui 0.42 的主上下文在 `PreUpdate` 的 `InitContexts` 才建立，
// `Startup` 调 `ctx_mut()` 必然 `Err`。**与 `setup_egui_fonts` 同口径**：
// 注册在 `PreUpdate` 链 `InitContexts` 之后，重试式 + 资源 marker 防重入。

/// 已安装图像解码器的 marker 资源（零大小）。
#[derive(Default, Resource)]
pub struct EguiImageLoadersInstalled;

/// 在 egui 上下文上安装 SVG 解码器（纯 egui 操作，单测可直调）。
pub fn install_svg_decoder(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// 主 egui 上下文就绪后安装图像解码器。
///
/// 上下文未就绪时静默下帧再试；装好后插入 marker；marker 存在则早退零开销。
pub fn setup_egui_image_loaders(
    mut commands: Commands,
    mut contexts: EguiContexts,
    installed: Option<Res<EguiImageLoadersInstalled>>,
) {
    if installed.is_some() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    install_svg_decoder(ctx);
    commands.insert_resource(EguiImageLoadersInstalled);
}

#[cfg(test)]
mod tool_button_tests {
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
mod svg_decoder_tests {
    use super::install_svg_decoder;

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2"><rect x="4" y="4" width="16" height="16"/></svg>"##;

    fn bytes_source() -> egui::ImageSource<'static> {
        egui::ImageSource::Bytes {
            uri: std::borrow::Cow::Borrowed("bytes://test/pen.svg"),
            bytes: std::borrow::Cow::Borrowed(SVG),
        }
    }

    /// 红三角根因的回归测试：出厂 egui 上下文没有任何 `ImageLoader`，
    /// `bytes://…​.svg` 解不出像素（egui 画红三角占位）；装上
    /// `egui_extras` 的 `SvgLoader` 后必须一步到位解码成纹理。
    #[test]
    fn svg_bytes_stay_undecoded_until_svg_loader_installed() {
        let ctx = egui::Context::default();

        // 未装解码器：必须报 NoMatchingImageLoader（旧版红三角的确切来源）。
        let before = bytes_source().load(
            &ctx,
            Default::default(),
            egui::load::SizeHint::Scale(1.0.into()),
        );
        assert!(
            matches!(
                &before,
                Err(egui::load::LoadError::NoMatchingImageLoader { .. })
            ),
            "出厂上下文应缺 SVG 解码器，got {before:?}"
        );

        install_svg_decoder(&ctx);
        let after = bytes_source().load(
            &ctx,
            Default::default(),
            egui::load::SizeHint::Scale(1.0.into()),
        );
        match after {
            Ok(egui::load::TexturePoll::Ready { texture, .. }) => {
                let size = texture.size;
                assert_eq!(
                    (size.x, size.y),
                    (24, 24),
                    "SVG 应按 viewBox 点数解码，got {size:?}"
                );
            }
            other => panic!("装上 SvgLoader 后应直接解码就绪，got {other:?}"),
        }
    }
}
