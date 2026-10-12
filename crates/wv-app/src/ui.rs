//! UI 组件库 —— 设计规格 `UI_DESIGN_SPEC_v0.1.md` 的 Rust 落地。
#![cfg(feature = "app")]

//! ## 布局骨架（规格 §3.1）
//!
//! - 顶栏（[`top_bar`]）：`TopBottomPanel` 是**唯一例外**，固定高 38px / 32px。
//! - 状态栏（[`status_bar`]）：同为 `TopBottomPanel`，固定高 22px。
//! - 其余全部是 [`float_window`] 包出的 `egui::Window` 浮窗，**地图铺满、
//!   面板是客人**（D1：面板不得推挤地图坐标系）。
//! - 时间轴（[`panel_timeline`]）是底部通栏浮窗，四层轨道（§3.4）。
//!
//! ## 设计原则落地
//!
//! | 原则 | 落点 |
//! |---|---|
//! | D1 地图是主体 | 无 SidePanel；面板显隐不改相机 |
//! | D2 世界色/界面色分离 | 本文件只用 [`crate::ui_tokens`]，世界色经 `palette.rs` |
//! | D3 状态永不抑制 | 沉浸模式只降透明度，不隐藏时间轴/日期 |
//! | D4 一切可逆 | 破坏操作二次确认（[`danger_button`]） |
//! | D5 零隐藏功能 | [`crate::ui_cmdk::CommandRegistry`] 收录全部功能 |
//!
//! ## 色值纪律
//!
//! 本文件**不得**出现裸十六进制色值：一律经 [`UiShell`] 持有的
//! [`crate::ui_tokens::ShellPalette`]。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::editing;
use crate::faction_panel_actions;
use crate::state::{
    BrushState, DrillState, GridConfig, Panel, PanelState, PlaybackState, RecentProject,
    RecentProjects, ViewMode, ViewState, WorldProjection,
};
use crate::ui_cmdk::CommandPalette;
use crate::ui_layout::LayoutFile;
use crate::ui_theme::{color32, color32_alpha, mono, proportional, UiShellConfig};
use crate::ui_tokens::{font_size, radius, space, BadgeTone, ShellPalette};
use crate::validation_actions;

/// 地形类型名（供笔刷面板与图例）。
pub const TERRAIN_NAMES: [&str; 10] = [
    "深水", "浅水", "沙滩", "草原", "林地", "丘陵", "山地", "雪原", "沼泽", "道路",
];

/// 地形色块索引 → `palette::CORE_TERRAIN` 同序（§4.5：色块就是地形本色）。
fn terrain_swatch(kind: u8) -> egui::Color32 {
    let c = crate::palette::core_terrain(kind);
    let bevy::prelude::Color::Srgba(s) = c else {
        return egui::Color32::GRAY;
    };
    egui::Color32::from_rgb(
        (s.red * 255.0) as u8,
        (s.green * 255.0) as u8,
        (s.blue * 255.0) as u8,
    )
}

/// 每帧由 UI 系统装配的样式快照（避免到处传参）。
pub struct UiShell {
    /// 当前外壳令牌。
    pub palette: ShellPalette,
    /// 字号阶梯。
    pub scale: crate::ui_theme::FontScale,
    /// 面板头高度（密度）。
    pub head_h: f32,
    /// 命中区（密度）。
    pub hit: f32,
    /// 行高（密度）。
    pub row: f32,
}

impl UiShell {
    /// 由主题配置构建。
    pub fn new(cfg: &UiShellConfig) -> Self {
        Self {
            palette: cfg.palette(),
            scale: crate::ui_theme::font_sizes(cfg.density),
            head_h: cfg.density.panel_head(),
            hit: cfg.density.hit(),
            row: cfg.density.row(),
        }
    }

    /// 主文本色。
    pub fn t1(&self) -> egui::Color32 {
        color32(self.palette.text_1)
    }
    /// 次文本色。
    pub fn t2(&self) -> egui::Color32 {
        color32(self.palette.text_2)
    }
    /// 弱文本色。
    pub fn t3(&self) -> egui::Color32 {
        color32(self.palette.text_3)
    }
    /// 交互主色。
    pub fn accent(&self) -> egui::Color32 {
        color32(self.palette.accent)
    }
    /// 焦点色。
    pub fn focus(&self) -> egui::Color32 {
        color32(self.palette.focus)
    }
    /// 面板底。
    pub fn panel_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_panel)
    }
    /// 抬升块底。
    pub fn raised_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_raised)
    }
    /// hover 底。
    pub fn hover_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_hover)
    }
    /// 选中 / pressed 底。
    pub fn active_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_active)
    }
    /// 分隔线。
    pub fn border_subtle(&self) -> egui::Color32 {
        color32(self.palette.border_subtle)
    }
    /// 外描边。
    pub fn border_strong(&self) -> egui::Color32 {
        color32(self.palette.border_strong)
    }
    /// 浮层底（含标定不透明度）。
    pub fn float_fill(&self) -> egui::Color32 {
        let (c, a) = self.palette.float_fill();
        color32_alpha(c, a)
    }
    /// accent 实心底上的文字色（**不是**白色，见 [`crate::ui_tokens::ShellPalette::text_on_accent`]）。
    pub fn text_on_accent(&self) -> egui::Color32 {
        color32(self.palette.text_on_accent)
    }
    /// 面包屑「上级」文字色（v0.2 §B.3 新增令牌）。
    pub fn crumb_text_3(&self) -> egui::Color32 {
        color32(self.palette.crumb_text_3)
    }
}

// ---------------------------------------------------------------------------
// 项目管理菜单（v0.2 §A）
// ---------------------------------------------------------------------------

/// 项目管理菜单的交互状态。
///
/// **为什么单独一个资源**：菜单要支持 `↑ ↓ ↵`（v0.2 §A「焦点落当前项、↑↓ 选择、
/// ↵ 执行」），需要跨帧记住高亮项；`egui::memory` 只存 popup 的开关，不存选中行。
#[derive(Resource, Debug, Default)]
pub struct ProjectMenuState {
    /// 高亮行索引（对**命令行**编号，跳过分隔线与复选框）。
    pub selected: usize,
    /// 待执行的命令。与 [`CommandPalette::pending`] 一样，交给
    /// [`crate::ui_shell::apply_pending_commands`] 统一分发——**唯一分发器**。
    pub pending: Option<crate::ui_cmdk::CommandId>,
}

/// 菜单里的一行。
///
/// `section` 让「分组标题 + 分隔线」的绘制**跟着行数据走**，而不是靠绘制方数
/// 「第几行该换组」——后者在增删一条命令时会悄悄把分隔线留在错误的位置，
/// 而且空的分组（比如没有最近工程时）会留下一个孤零零的标题。
struct MenuRow {
    section: &'static str,
    label: String,
    keys: Option<&'static str>,
    id: crate::ui_cmdk::CommandId,
}

/// 工程名下拉菜单（v0.2 §A）。
///
/// 规格明确要求「**不用固定菜单栏**」——那会破坏顶栏的三段式密度（§3.2）。
/// 于是入口就是身份段的工程名本身：点它展开，焦点落在当前项上。
///
/// 菜单内容顺序（与原型一致）：最近工程 → 工程操作 → 面板显隐。
/// **保存 / 撤销不进这个菜单**：它们已有独立位置（§A）。
fn project_menu(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    recents: &[RecentProject],
    panels: &mut PanelState,
    state: &mut ProjectMenuState,
) {
    let name = projection
        .world
        .meta
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "（未命名工程）".to_owned());
    let dirty = projection.dirty;
    let popup_id = egui::Id::new("wv_project_menu");
    let opened = ui.memory(|m| m.is_popup_open(popup_id));

    // ── 触发器 ──
    let text = format!("◈ {name}{}", if dirty { " ●" } else { "" });
    let galley = ui
        .painter()
        .layout_no_wrap(text, proportional(shell.scale.base), shell.t1());
    let size = egui::vec2(galley.size().x + space::SM * 2.0, shell.hit);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    // 自绘而不是用 `Button`：菜单在顶栏里的 hover 底必须是 `bg.hover` 令牌（§4.2）。
    if resp.hovered() || opened {
        ui.painter()
            .rect_filled(rect, radius::CONTROL, shell.hover_bg());
    }
    ui.painter().galley(
        egui::pos2(
            rect.min.x + space::SM,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        shell.t1(),
    );
    if resp.clicked() {
        state.selected = 0;
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    // ── 菜单内容 ──
    // 命令行清单先建好：键盘导航要知道一共几行，绘制时才知道哪行高亮。
    use crate::ui_cmdk::CommandId;
    let mut rows: Vec<MenuRow> = Vec::new();
    for (i, p) in recents.iter().enumerate() {
        rows.push(MenuRow {
            section: "最近工程",
            label: format!("打开最近工程 · {}", p.name),
            keys: None,
            id: CommandId::OpenRecentProject(i as u8),
        });
    }
    for (label, keys, id) in [
        ("新建工程", Some("Ctrl+Shift+N"), CommandId::NewProject),
        ("打开工程…", Some("Ctrl+O"), CommandId::OpenProject),
        ("另存为…", Some("Ctrl+Shift+S"), CommandId::SaveProjectAs),
        ("导出…", Some("Ctrl+E"), CommandId::ExportProject),
        ("关闭工程", Some("Ctrl+W"), CommandId::CloseProject),
        ("项目设置", None, CommandId::ProjectSettings),
    ] {
        rows.push(MenuRow {
            section: "工程",
            label: label.to_owned(),
            keys,
            id,
        });
    }

    // 键盘先于绘制处理：绘制时才知道 hover，键盘不该依赖它。
    let (down, up, enter, esc) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if opened {
        let last = rows.len().saturating_sub(1);
        if down {
            state.selected = (state.selected + 1).min(last);
        }
        if up {
            state.selected = state.selected.saturating_sub(1);
        }
        if enter {
            if let Some(r) = rows.get(state.selected) {
                state.pending = Some(r.id);
            }
            ui.memory_mut(|m| m.close_popup());
        }
        if esc {
            ui.memory_mut(|m| m.close_popup());
        }
    }
    state.selected = state.selected.min(rows.len().saturating_sub(1));

    let mut clicked_id: Option<CommandId> = None;
    egui::popup::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::popup::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(268.0);
            // 分组标题与分隔线跟着行数据走：空分组不会留下孤立标题，
            // 增删命令也不需要同步改这里的计数（见 [`MenuRow::section`]）。
            let mut section = "";
            for (i, r) in rows.iter().enumerate() {
                if r.section != section {
                    if !section.is_empty() {
                        ui.separator();
                    }
                    menu_section(ui, shell, r.section);
                    section = r.section;
                }
                let highlighted = state.selected == i;
                if menu_row(ui, shell, &r.label, r.keys, highlighted, true).clicked() {
                    clicked_id = Some(r.id);
                }
            }
            // 面板显隐 + 重置位置：v0.1 的「窗口菜单」内容保留在这里，
            // 它是三级可发现性的第 3 级，不能删（§6.2）。
            ui.separator();
            menu_section(ui, shell, "面板");
            for p in Panel::ALL {
                let mut on = panels.is_visible(p);
                if ui.checkbox(&mut on, p.title()).changed() {
                    panels.set_visible(p, on);
                }
            }
            ui.separator();
            if ui.button("重置所有面板位置").clicked() {
                panels.reset_requested = true;
                ui.memory_mut(|m| m.close_popup());
            }
        },
    );
    if let Some(id) = clicked_id {
        state.pending = Some(id);
        ui.memory_mut(|m| m.close_popup());
    }
}

/// 菜单里的分组小标题（11px、`text.3`、字距放宽）。
fn menu_section(ui: &mut egui::Ui, shell: &UiShell, title: &str) {
    ui.add_space(space::XS);
    ui.label(
        egui::RichText::new(title)
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(space::XS * 0.5);
}

/// 菜单里的一行（§4.2）。
///
/// `highlighted` 是键盘高亮（↑↓），与 hover 分开：键盘用户没有指针，
/// 只靠 hover 的话完全看不出焦点在哪（§7 非颜色依赖的同类问题）。
fn menu_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    keys: Option<&str>,
    highlighted: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), shell.row),
        egui::Sense::click(),
    );
    let fg = if !enabled {
        shell.t3()
    } else if highlighted || resp.hovered() {
        shell.t1()
    } else {
        shell.t2()
    };
    if highlighted || resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius::BADGE, shell.hover_bg());
    }
    ui.painter().text(
        egui::pos2(rect.min.x + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.small),
        fg,
    );
    if let Some(k) = keys {
        ui.painter().text(
            egui::pos2(rect.max.x - space::SM, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            k,
            mono(shell.scale.micro),
            shell.t3(),
        );
    }
    resp
}

// ---------------------------------------------------------------------------
// 面包屑（v0.2 §B.3）
// ---------------------------------------------------------------------------

/// 面包屑：`世界 › 遵义城 › 府衙`。
///
/// 浮在地图区左上角（避顶栏与时间轴），**不推挤地图坐标系**（D1）。层级状态是
/// [`DrillState`] 的 id 链，名字现查 `world.maps`——那才是唯一真源。
///
/// 当前级 700 字重 / 14px / `text.1` 且**不可点**；上级 400 字重 / 13px /
/// 新增令牌 `crumb.text_3`；分隔符 `›`(U+203A)。沉浸模式降 60% 透明度但**保留可见**
/// （§B.3：沉浸只隐面板与演示控制，导航痕迹必须留着，否则用户不知道自己身在何处）。
///
/// **偏离记录**：v0.2 把 LOD 标签（`四渡赤水 · 正在发生`）画在地图上，但那需要
/// Bevy `Text2d` 的字体资产，而本工程不打包字体（§2.3 用系统字体栈，且只有 egui 侧
/// 接了 `fonts.rs`）。故 LOD 文案并入面包屑行尾——信息不丢，代价是位置不同。
pub fn breadcrumb(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    mut drill: ResMut<DrillState>,
    panels: Res<PanelState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    if drill.current().is_none() {
        return;
    }
    let alpha = if panels.immersive { 0.4 } else { 1.0 };
    let top = shell_cfg.density.top_bar_height() + space::MD;

    // 先把要画的内容收集出来（借 `world` 的不可变借用不能跨进闭包）。
    let mut segments: Vec<(String, bool)> = Vec::new();
    let trail = drill.trail().to_vec();
    for (i, id) in trail.iter().enumerate() {
        let name = drill
            .node(&projection.world, id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| id.as_str().to_owned());
        segments.push((name, i + 1 == trail.len()));
    }
    let lod = drill
        .current_node(&projection.world)
        .map(|n| format!("{} · {}", n.name, crate::state::map_level_label(n.level)))
        .unwrap_or_default();

    let mut jump: Option<usize> = None;
    egui::Area::new(egui::Id::new("wv_breadcrumb"))
        .fixed_pos(egui::pos2(space::MD, top))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(shell.float_fill().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_subtle()))
                .inner_margin(egui::Margin::symmetric(space::SM, space::XS))
                .rounding(radius::CONTROL)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, (name, is_current)) in segments.iter().enumerate() {
                            if i > 0 {
                                ui.label(
                                    egui::RichText::new("›")
                                        .size(shell.scale.small)
                                        .color(shell.t3().gamma_multiply(alpha)),
                                );
                            }
                            if *is_current {
                                // 当前级：不可点（§B.3）。
                                ui.label(
                                    egui::RichText::new(name)
                                        .size(font_size::INPUT)
                                        .strong()
                                        .color(shell.t1().gamma_multiply(alpha)),
                                );
                            } else {
                                let resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(name)
                                            .size(shell.scale.base)
                                            .color(shell.crumb_text_3().gamma_multiply(alpha)),
                                    )
                                    .sense(egui::Sense::click()),
                                );
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.clicked() {
                                    jump = Some(i);
                                }
                            }
                        }
                        ui.add_space(space::SM);
                        ui.label(
                            egui::RichText::new(&lod)
                                .size(shell.scale.micro)
                                .color(shell.t3().gamma_multiply(alpha)),
                        );
                    });
                });
        });

    if let Some(i) = jump {
        drill.jump_to(i);
    }
}

// ---------------------------------------------------------------------------
// 徽标（§4.7）
// ---------------------------------------------------------------------------

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
    let bg = color32_alpha(colors.fill, colors.fill_alpha);
    let pad = egui::vec2(space::XS * 1.5, 1.0);
    let font = proportional(font_size::MICRO);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, fg);
    let size = galley.size() + pad * 2.0;
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, radius::BADGE, bg);
    // egui 0.30 的 `Painter::galley` 需要一个兜底色（字体未命中字形时使用）。
    ui.painter().galley(rect.min + pad, galley, fg);
}

// ---------------------------------------------------------------------------
// 顶栏（§3.2 三段式）
// ---------------------------------------------------------------------------

/// 顶栏：身份 → 主控 → 读数。**唯一例外**的 `TopBottomPanel`（38px / 32px）。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn top_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    recents_res: Res<RecentProjects>,
    mut menu: ResMut<ProjectMenuState>,
    mut view: ResMut<ViewState>,
    playback: Res<PlaybackState>,
    mut panels: ResMut<PanelState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let h = shell_cfg.density.top_bar_height();
    let recents = recents_res.visible(&projection);
    egui::TopBottomPanel::top("wv_top_bar")
        .exact_height(h)
        .frame(
            egui::Frame::none()
                .fill(color32(shell.palette.bg_panel))
                .inner_margin(egui::Margin::symmetric(space::SM, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // ── 身份 ──
                identity_segment(ui, &shell, &projection, &recents, &mut panels, &mut menu);
                ui.add_space(space::MD);

                // ── 主控（视图模式分段控件，权重最高）──
                view_mode_segment(ui, &shell, &mut view);

                // 读数推到右端。
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    readout_segment(ui, &shell, &view, &playback, &projection);
                });
            });
        });
}

fn identity_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    recents: &[RecentProject],
    panels: &mut PanelState,
    menu: &mut ProjectMenuState,
) {
    // 项目管理菜单（v0.2 §A）。入口就是工程名本身——不用固定菜单栏（§3.2 三段式密度）。
    project_menu(ui, shell, projection, recents, panels, menu);

    // 脏标记 / 保存状态（§A：切换工程后徽标变「已加载」）。
    let label = if projection.dirty {
        BadgeKind::Warn
    } else {
        BadgeKind::Ok
    };
    let text = if projection.dirty {
        "未保存"
    } else {
        "已保存"
    };
    ui.add_space(space::XS);
    badge(ui, shell, label, text);

    // 工程未加载时的醒目错误（§5.1）。
    if projection.load_error.is_some() {
        ui.add_space(space::SM);
        badge(ui, shell, BadgeKind::Err, "工程未加载");
    }
}

fn view_mode_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    let total = ui.available_width();
    // 分段控件：4 等分，窄窗下也不截断（§8 断点）。
    let seg_w = (total * 0.30).clamp(220.0, 340.0);
    let seg_h = shell.hit;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CONTROL, shell.raised_bg());
    let seg_w_each = rect.width() / 4.0;
    for (i, mode) in ViewMode::ALL.iter().enumerate() {
        let r = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + seg_w_each * i as f32, rect.min.y),
            egui::pos2(rect.min.x + seg_w_each * (i as f32 + 1.0), rect.max.y),
        );
        let selected = view.mode == *mode;
        let resp = ui.interact(r, egui::Id::new(("wv_view_seg", i)), egui::Sense::click());
        if selected {
            painter.rect_filled(r, radius::CONTROL, shell.accent());
        } else if resp.hovered() {
            painter.rect_filled(r, radius::CONTROL, shell.hover_bg());
        }
        // 选中段用 accent 实心底，文字取 `ui.tx.on_accent`——
        // 暗色 accent 上的白字只有 3.26:1（§7 正文门槛 4.5），必须用令牌给的近黑色。
        let fg = if selected {
            shell.text_on_accent()
        } else {
            shell.t2()
        };
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            mode.label(),
            proportional(font_size::SMALL),
            fg,
        );
        if resp.clicked() {
            view.mode = *mode;
        }
    }
}

fn readout_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    playback: &PlaybackState,
    projection: &WorldProjection,
) {
    // 比例尺（<1400px 隐藏）。
    if ui.available_width() > 400.0 {
        ui.label(
            egui::RichText::new(format!("1:{:.0}", 1.0 / view.scale.max(1e-4) * 1000.0))
                .size(shell.scale.small)
                .color(shell.t2())
                .font(mono(shell.scale.small)),
        );
        ui.add_space(space::MD);
    }
    // 坐标（<1100px 隐藏）。
    if ui.available_width() > 240.0 {
        let coord = match view.cursor_world {
            Some(c) => format!("({:.0}, {:.0})", c.x, c.y),
            None => "(—, —)".into(),
        };
        ui.label(
            egui::RichText::new(coord)
                .size(shell.scale.small)
                .color(shell.t2())
                .font(mono(shell.scale.small)),
        );
        ui.add_space(space::MD);
    }
    // 日期（只读镜像，§5.3 唯一真源是 playback.current_day）。
    let date = crate::ui_time::format_date_for(projection, playback.current_day);
    ui.label(
        egui::RichText::new(date)
            .size(shell.scale.small)
            .color(shell.t1())
            .font(mono(shell.scale.small)),
    );
}

// ---------------------------------------------------------------------------
// 状态栏（§3.1 底部 22px）
// ---------------------------------------------------------------------------

/// 状态栏：实体计数 / 已选 / 缩放 / 操作回执。
pub fn status_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    notice: Res<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
) {
    let shell = UiShell::new(&shell_cfg);
    let fresh = notice.is_fresh((time.elapsed_secs() * 60.0) as u64);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    egui::TopBottomPanel::bottom("wv_status_bar")
        .exact_height(22.0)
        .frame(
            egui::Frame::none()
                .fill(color32(shell.palette.bg_panel))
                .inner_margin(egui::Margin::symmetric(space::SM, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let w = &projection.world;
                let summary = format!(
                    "Unit {} · 人物 {} · 事态 {} · 事件 {}",
                    w.units.len(),
                    w.characters.len(),
                    w.situations.len(),
                    w.events.len()
                );
                ui.label(
                    egui::RichText::new(summary)
                        .size(shell.scale.micro)
                        .color(shell.t3())
                        .font(mono(shell.scale.micro)),
                );
                ui.add_space(space::LG);
                ui.label(
                    egui::RichText::new(format!("已选：{}", view.current_map))
                        .size(shell.scale.micro)
                        .color(shell.t2()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("缩放 {:.2}×", view.scale))
                            .size(shell.scale.micro)
                            .color(shell.t2())
                            .font(mono(shell.scale.micro)),
                    );
                    // 操作回执（§5.3：成功与失败都要有人话说明）。
                    if let Some(text) = notice.text.as_deref().filter(|_| fresh) {
                        ui.add_space(space::LG);
                        badge(
                            ui,
                            &shell,
                            if notice.is_error {
                                BadgeKind::Err
                            } else {
                                BadgeKind::Ok
                            },
                            text,
                        );
                    }
                });
            });
        });
}

// ---------------------------------------------------------------------------
// 面板外壳（§4.1）
// ---------------------------------------------------------------------------

/// 浮动面板外壳：类型图标 + 标题 + 徽标 + 折叠，可拖动、可关闭。
///
/// `layouts` 提供**跨会话**的位置/尺寸（`ui-layout.yaml`）；本帧的拖动结果由
/// egui 按窗口 `Id` 自己记住，并由 [`crate::ui_shell::persist_layout_on_change`]
/// 回写磁盘（D1：面板是客人，地图是主体）。
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

    let win = egui::Window::new("")
        .id(id)
        .open(&mut open)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .default_open(!layout.collapsed)
        .resizable(true)
        .collapsible(true)
        .title_bar(true)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(egui::Margin::same(space::SM))
                .rounding(radius::PANEL),
        );

    win.show(ctx, |ui| {
        // 自绘标题栏三段（§4.1）：⠿ 拖动把手 · 图标+标题 · 徽标。
        panel_header(ui, shell, panel, tag);
        ui.add_space(space::XS);
        ui.separator();
        ui.add_space(space::XS);
        body(ui);
    });

    if !open {
        panels.set_visible(panel, false);
    }
}

fn panel_header(
    ui: &mut egui::Ui,
    shell: &UiShell,
    panel: Panel,
    tag: Option<(BadgeKind, String)>,
) {
    ui.horizontal(|ui| {
        // 拖动把手（视觉上明示可拖）。
        ui.label(
            egui::RichText::new("⠿")
                .size(shell.scale.micro)
                .color(shell.t3().gamma_multiply(0.6)),
        );
        // 类型图标（accent 着色）。
        ui.label(
            egui::RichText::new(panel.icon())
                .size(shell.scale.base)
                .color(shell.accent()),
        );
        // 标题（11px，字距 .05em 近似为大写风格）。
        ui.label(
            egui::RichText::new(panel.title())
                .size(shell.scale.micro)
                .color(shell.t1()),
        );
        if let Some((kind, text)) = tag {
            ui.add_space(space::XS);
            badge(ui, shell, kind, &text);
        }
    });
}

// ---------------------------------------------------------------------------
// 面板内容组件
// ---------------------------------------------------------------------------

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
    let avail = ui.available_size();
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
        let _ = avail;
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
        painter.rect_stroke(
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

/// 地形色块网格（§4.5：2 列 × 5 行，色块即地形本色）。
pub fn terrain_grid(ui: &mut egui::Ui, shell: &UiShell, selected: &mut u8) {
    egui::Grid::new("terrain_swatches")
        .num_columns(2)
        .spacing([space::XS, space::XS])
        .show(ui, |ui| {
            for k in 0u8..10 {
                let name = TERRAIN_NAMES[k as usize];
                let is_sel = *selected == k;
                let cell_w = (ui.available_width() - space::XS) / 2.0;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(cell_w, 44.0), egui::Sense::click());
                let painter = ui.painter();
                if is_sel {
                    painter.rect_filled(rect, radius::CONTROL, shell.active_bg());
                    painter.rect_stroke(
                        rect,
                        radius::CONTROL,
                        egui::Stroke::new(1.5_f32, shell.focus()),
                    );
                } else if resp.hovered() {
                    painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
                }
                // 16px 色块。
                let sw = egui::Rect::from_min_size(
                    egui::pos2(rect.min.x + space::SM, rect.min.y + space::SM),
                    egui::vec2(16.0, 16.0),
                );
                painter.rect_filled(sw, radius::BADGE, terrain_swatch(k));
                // 10px 文字。
                painter.text(
                    egui::pos2(
                        rect.min.x + space::SM,
                        rect.min.y + space::SM + 16.0 + space::XS,
                    ),
                    egui::Align2::LEFT_TOP,
                    name,
                    proportional(10.0),
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

// ---------------------------------------------------------------------------
// 时间轴四层（§3.4）
// ---------------------------------------------------------------------------

/// 时间轴轨道层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineLayer {
    /// L1 日：细刻度尺 + 播放头。
    Day,
    /// L2 事态：事态条 + 轨迹条（默认层）。
    #[default]
    Situation,
    /// L3 人物：生卒条。
    Character,
    /// L4 世纪：按世纪聚合鸟瞰。
    Century,
}

impl TimelineLayer {
    /// 层名。
    pub fn label(self) -> &'static str {
        match self {
            TimelineLayer::Day => "日",
            TimelineLayer::Situation => "事态",
            TimelineLayer::Character => "人物",
            TimelineLayer::Century => "世纪",
        }
    }
    /// 全部层。
    pub const ALL: [TimelineLayer; 4] = [
        TimelineLayer::Day,
        TimelineLayer::Situation,
        TimelineLayer::Character,
        TimelineLayer::Century,
    ];
}

/// 时间轴底部通栏浮窗 + 四层轨道。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn panel_timeline(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut playback: ResMut<PlaybackState>,
    projection: Res<WorldProjection>,
    mut layer: Local<TimelineLayer>,
    mut open_event: Local<bool>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    if !panels.is_visible(Panel::Timeline) {
        return;
    }

    // 沉浸模式下时间轴降到 55% 透明但**不隐藏**（D3）。
    let alpha = if panels.immersive { 0.55 } else { 1.0 };

    // 通栏也走布局表（否则拖动结果不落盘，`ui-layout.yaml` 里的 timeline 段是死数据）。
    let layout = crate::ui_layout::current_layout(&layouts, &panels, Panel::Timeline);

    let mut open = true;
    egui::Window::new("")
        .id(egui::Id::new(("wv_panel", Panel::Timeline)))
        .open(&mut open)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .resizable(true)
        .title_bar(false)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(shell.panel_bg().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(egui::Margin::same(space::SM))
                .rounding(radius::PANEL),
        )
        .show(ctx, |ui| {
            // ── 播放控件 + 日期 + 层切换 ──
            ui.horizontal(|ui| {
                let play_label = if playback.playing { "⏸" } else { "▶" };
                if ui
                    .add(egui::Button::new(play_label).min_size(egui::vec2(shell.hit, shell.hit)))
                    .clicked()
                {
                    playback.playing = !playback.playing;
                }
                for s in [1.0f32, 5.0, 30.0] {
                    let sel = (playback.speed - s).abs() < f32::EPSILON;
                    if ui.selectable_label(sel, format!("{s:.0}天/秒")).clicked() {
                        playback.speed = s;
                    }
                }
                ui.add_space(space::MD);
                // 日期（时间轴是唯一时间权威，§5.3）。
                ui.label(
                    egui::RichText::new(crate::ui_time::format_date_for(
                        &projection,
                        playback.current_day,
                    ))
                    .size(shell.scale.base)
                    .color(shell.t1())
                    .font(mono(shell.scale.base)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("＋ 登记事件").clicked() {
                        *open_event = true;
                    }
                    ui.add_space(space::SM);
                    for l in TimelineLayer::ALL {
                        let sel = *layer == l;
                        if ui.selectable_label(sel, l.label()).clicked() {
                            *layer = l;
                        }
                    }
                });
            });

            ui.add_space(space::SM);

            // ── 轨道区（按层渲染）──
            let track_h = 84.0;
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), track_h),
                egui::Sense::click_and_drag(),
            );
            draw_timeline_tracks(ui, &shell, rect, *layer, &playback, &projection);

            // 播放头拖拽 / 点击定位。
            if resp.dragged() || resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let frac = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                    let span = (playback.range_to - playback.range_from).max(1);
                    playback.current_day = playback.range_from + (frac * span as f32) as i64;
                }
            }
        });

    if !open {
        panels.set_visible(Panel::Timeline, false);
    }
    if *open_event {
        panels.set_visible(Panel::Event, true);
        *open_event = false;
    }
}

fn draw_timeline_tracks(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    layer: TimelineLayer,
    playback: &PlaybackState,
    projection: &WorldProjection,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CONTROL, shell.raised_bg());

    let span = (playback.range_to - playback.range_from).max(1) as f32;
    let x_of = |day: i64| {
        rect.min.x + ((day - playback.range_from) as f32 / span).clamp(0.0, 1.0) * rect.width()
    };

    match layer {
        TimelineLayer::Day => {
            // 细刻度尺 + 播放头。
            for i in 0..=20 {
                let x = rect.min.x + rect.width() * i as f32 / 20.0;
                painter.line_segment(
                    [egui::pos2(x, rect.min.y), egui::pos2(x, rect.min.y + 8.0)],
                    egui::Stroke::new(1.0_f32, shell.border_strong()),
                );
            }
        }
        TimelineLayer::Situation => {
            // 事态条（世界色：事件紫）。
            for (i, s) in projection.world.situations.iter().enumerate() {
                let y = rect.min.y + space::SM + i as f32 * 18.0;
                let x0 = x_of(s.span.from);
                let x1 = x_of(s.span.to.unwrap_or(playback.range_to));
                let bar = egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + 12.0));
                painter.rect_filled(
                    bar,
                    radius::BADGE,
                    crate::palette::palette_to_color32(crate::palette::ACCENT_EVENT),
                );
                painter.text(
                    egui::pos2(x0 + space::XS, y + 6.0),
                    egui::Align2::LEFT_CENTER,
                    &s.name,
                    proportional(shell.scale.micro),
                    shell.t1(),
                );
            }
        }
        TimelineLayer::Character => {
            // 人物生卒条。
            for (i, c) in projection.world.characters.iter().enumerate() {
                let y = rect.min.y + space::SM + i as f32 * 18.0;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.min.x + space::MD, y),
                    egui::pos2(rect.max.x - space::MD, y + 10.0),
                );
                painter.rect_filled(bar, radius::BADGE, shell.accent().gamma_multiply(0.5));
                painter.text(
                    egui::pos2(bar.min.x + space::XS, y + 5.0),
                    egui::Align2::LEFT_CENTER,
                    &c.name,
                    proportional(shell.scale.micro),
                    shell.t2(),
                );
            }
        }
        TimelineLayer::Century => {
            // 世纪聚合鸟瞰（10 万年跨度下 L1 画不出来）。
            let buckets = 12;
            for i in 0..buckets {
                let x = rect.min.x + rect.width() * i as f32 / buckets as f32;
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x + 2.0, rect.max.y - 30.0),
                    egui::pos2(
                        x + rect.width() / buckets as f32 - 2.0,
                        rect.max.y - space::SM,
                    ),
                );
                painter.rect_filled(bar, radius::BADGE, shell.accent().gamma_multiply(0.35));
            }
        }
    }

    // 播放头（游标色 world.accent.cursor，共用令牌）。
    let cx = x_of(playback.current_day);
    painter.line_segment(
        [egui::pos2(cx, rect.min.y), egui::pos2(cx, rect.max.y)],
        egui::Stroke::new(
            2.0_f32,
            crate::palette::palette_to_color32(crate::palette::ACCENT_CURSOR),
        ),
    );
}

// ---------------------------------------------------------------------------
// 其余面板
// ---------------------------------------------------------------------------

/// 世界树（P-1）。
pub fn panel_map_tree(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::MapTree,
        None,
        |ui| {
            let resp = list_item(
                ui,
                &shell,
                terrain_swatch(3),
                &view.current_map,
                "世界地图",
                true,
            );
            let _ = resp;
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("World → Region → City → Interior")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
        },
    );
}

/// 图层管理（P-2）。
pub fn panel_layers(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Layers,
        None,
        |ui| {
            ui.checkbox(&mut view.terrain_visible, "地形");
            ui.checkbox(&mut view.places_visible, "地点");
            ui.checkbox(&mut view.events_visible, "事件点");
            ui.checkbox(&mut view.tracks_visible, "轨迹");
            ui.checkbox(&mut view.roads_visible, "道路");
            ui.checkbox(&mut view.factions_visible, "势力");
        },
    );
}

/// 地形笔刷（P-3）。
pub fn panel_brush(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut brush: ResMut<BrushState>,
    grid: Res<GridConfig>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Brush,
        None,
        |ui| {
            ui.checkbox(&mut brush.enabled, "启用笔刷");
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("地形")
                    .size(shell.scale.micro)
                    .color(shell.t2()),
            );
            terrain_grid(ui, &shell, &mut brush.kind);
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new(format!("半径 {:.0} wu", brush.radius))
                    .size(shell.scale.small)
                    .color(shell.t2())
                    .font(mono(shell.scale.small)),
            );
            ui.add(
                egui::Slider::new(&mut brush.radius, grid.cell_size * 2.0..=600.0)
                    .show_value(false)
                    .logarithmic(true),
            );
            ui.label(
                egui::RichText::new(format!("衰减软度 {:.2}", brush.softness))
                    .size(shell.scale.small)
                    .color(shell.t2())
                    .font(mono(shell.scale.small)),
            );
            ui.add(egui::Slider::new(&mut brush.softness, 0.0..=1.0).show_value(false));
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("左键涂抹 · Ctrl+滚轮改半径 · [ ] 微调")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
        },
    );
}

/// 小地图 + 图层开关（P-4）。
pub fn panel_minimap(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<crate::terrain_view::TerrainTexture>,
) {
    let shell = UiShell::new(&shell_cfg);
    // 小地图缩略图直接复用大地图的地形纹理（同一 Handle，零额外上传）。
    // 注意先取 tex_id 再借 ctx（两者都要 &mut contexts，顺序反了过不了借用检查）。
    let tex_id = contexts.add_image(tex.handle.clone());
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Minimap,
        None,
        |ui| {
            let side = 168.0;
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
            let painter = ui.painter();
            painter.rect_filled(rect, radius::CONTROL, shell.raised_bg());
            // UV 取整张纹理：egui 没有 `Rect::RECT` 常量，用 [0,1]² 单位矩形。
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(tex_id, rect, uv, egui::Color32::WHITE);
            painter.rect_stroke(
                rect,
                radius::CONTROL,
                egui::Stroke::new(1.0_f32, shell.border_strong()),
            );
            // 视口矩形。
            let half = 1.0 / view.scale.max(1e-4);
            let cx = 0.5 + (view.camera_center.x / (grid.cell_size * 512.0)).clamp(-0.5, 0.5);
            let cy = 0.5 + (view.camera_center.y / (grid.cell_size * 512.0)).clamp(-0.5, 0.5);
            let vw = (half * 0.5).clamp(0.05, 1.0);
            let vp = egui::Rect::from_center_size(
                egui::pos2(
                    rect.left() + rect.width() * cx,
                    rect.top() + rect.height() * cy,
                ),
                egui::vec2(rect.width() * vw, rect.height() * vw),
            )
            .intersect(rect); // §C6：裁剪，不许画出边界
            painter.rect_stroke(vp, radius::BADGE, egui::Stroke::new(1.5_f32, shell.focus()));
            if response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    let nx = (p.x - rect.left()) / rect.width() - 0.5;
                    let ny = (p.y - rect.top()) / rect.height() - 0.5;
                    view.camera_center =
                        Vec2::new(nx * grid.cell_size * 512.0, ny * grid.cell_size * 512.0);
                }
            }
            // 图层开关排到地形缩略图**两侧**：左侧 = 地点 / 轨迹，右侧 = 事件 / 道路；
            // "地形" 与 "图层" 标题共用顶部行，保持视觉对齐。
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                ui.checkbox(&mut view.terrain_visible, "地形");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("图层")
                            .size(shell.scale.micro)
                            .color(shell.t2()),
                    );
                });
            });
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.checkbox(&mut view.places_visible, "地点");
                    ui.checkbox(&mut view.tracks_visible, "轨迹");
                });
                ui.add_space(space::SM);
                ui.vertical(|ui| {
                    ui.checkbox(&mut view.events_visible, "事件");
                    ui.checkbox(&mut view.roads_visible, "道路");
                });
            });
        },
    );
}

/// 检视器（P-5）。
pub fn panel_inspector(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Inspector,
        None,
        |ui| {
            if empty_state(
                ui,
                &shell,
                "◎",
                "未选中对象",
                "在图上点选地块 / 事件 / 人物后，此处显示其全部属性",
                "用命令面板查找 →",
            ) {
                // 空状态的引导按钮：无副作用的占位（后续接选择系统）。
            }
        },
    );
}

/// 事态登记（P-6）。
pub fn panel_situation(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let count = projection.world.situations.len();
    let badge = if count == 0 {
        None
    } else {
        Some((BadgeKind::Neutral, count.to_string()))
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Situation,
        badge,
        |ui| {
            let sits = &projection.world.situations;
            if sits.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "▣",
                    "暂无已登记事态",
                    "事态是叙事的骨架：登记一个，把事件串起来",
                    "登记事态 →",
                );
            } else {
                for s in sits {
                    let meta = match s.span.to {
                        Some(to) => format!("第 {}–{} 天", s.span.from, to),
                        None => format!("第 {} 天 –", s.span.from),
                    };
                    list_item(
                        ui,
                        &shell,
                        crate::palette::palette_to_color32(crate::palette::ACCENT_EVENT),
                        &s.name,
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}

/// 势力层（P-7）。
pub fn panel_faction(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<editing::EditHistory>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let map_id = wv_core::id::Id::new_unchecked(view.current_map.clone());
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Faction,
        None,
        |ui| {
            ui.horizontal(|ui| {
                if ui.button("+ 新建势力").clicked() {
                    faction_panel_actions::add_faction_now(
                        &mut projection,
                        &mut edit,
                        &view,
                        "新势力".into(),
                        wv_core::color::Color("#1E6FCC".into()),
                    );
                }
                ui.add_space(space::SM);
                ui.label(
                    egui::RichText::new(format!("当前地图：{}", view.current_map))
                        .small()
                        .color(shell.t2()),
                );
            });
            ui.add_space(space::SM);
            ui.separator();
            ui.add_space(space::SM);

            // 复制出 Vec 以便可变性借出（按 ID 升序展示，便于稳定定位）。
            let mut factions: Vec<wv_core::Faction> = projection.world.factions.clone();
            factions.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

            if factions.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "◆",
                    "尚未创建势力",
                    "在工程目录下编辑 factions/*.yaml，或点击上方「+ 新建势力」",
                    "查看势力层说明 →",
                );
                return;
            }

            for f in &factions {
                faction_card(
                    ui,
                    &shell,
                    &f,
                    &view,
                    &playback,
                    &map_id,
                    &mut projection,
                    &mut edit,
                );
                ui.add_space(space::XS);
            }
        },
    );
}

/// 一张势力卡：名称/色编辑 + 当前地图下的几何列表（增/删）。
#[allow(clippy::too_many_arguments)]
fn faction_card(
    ui: &mut egui::Ui,
    shell: &UiShell,
    faction: &wv_core::Faction,
    view: &ViewState,
    playback: &PlaybackState,
    map_id: &wv_core::id::Id,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
) {
    let id = faction.id.clone();
    egui::CollapsingHeader::new(format!("◆ {} ({})", faction.name, faction.id))
        .default_open(true)
        .show(ui, |ui| {
            // 名称 / 颜色（用本地副本 commit，避免可变借用互相打架）。
            let mut name_buf = faction.name.clone();
            let mut color_buf = faction.color.as_str().to_string();
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("名称：");
                if ui.text_edit_singleline(&mut name_buf).changed() {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("颜色：");
                if ui.text_edit_singleline(&mut color_buf).changed() {
                    changed = true;
                }
            });
            if changed {
                let color = wv_core::color::Color(color_buf.clone());
                faction_panel_actions::update_faction_now(
                    projection,
                    edit,
                    view,
                    id.clone(),
                    name_buf,
                    color,
                );
            }

            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new(format!("当前地图几何：{}", view.current_map))
                    .small()
                    .color(shell.t2()),
            );
            // 几何列表：仅展示当前地图下的。
            let geos: Vec<wv_core::map::GeometrySpan> = projection
                .world
                .faction_geometry
                .iter()
                .filter(|g| g.map == *map_id && g.owner == id)
                .cloned()
                .collect();
            if geos.is_empty() {
                ui.label(
                    egui::RichText::new("（无几何）")
                        .italics()
                        .small()
                        .color(shell.t2()),
                );
            } else {
                for g in &geos {
                    let from = g.span.from;
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "[{}, {})",
                            from,
                            g.span.to.map(|t| t).unwrap_or(from + 1)
                        ));
                        ui.label(format!("{} 顶点", g.geometry.point_count()));
                        if ui.small_button("删除").clicked() {
                            faction_panel_actions::remove_geometry_now(
                                projection,
                                edit,
                                view,
                                id.clone(),
                                from,
                            );
                        }
                    });
                }
            }
            // 新增几何。
            ui.horizontal(|ui| {
                let mut from_buf = playback.current_day.to_string();
                let mut to_buf = "".to_string();
                ui.label("起 day：");
                if ui.text_edit_singleline(&mut from_buf).changed() {
                    // 改了起日就停掉按钮的焦点闪烁问题——面板每帧重画，自动 commit。
                }
                ui.label("止（留空=至今）：");
                ui.text_edit_singleline(&mut to_buf);
                if ui.button("+ 新建疆界").clicked() {
                    let from: i64 = from_buf.trim().parse().unwrap_or(playback.current_day);
                    let to: Option<i64> = if to_buf.trim().is_empty() {
                        None
                    } else {
                        to_buf.trim().parse().ok()
                    };
                    faction_panel_actions::upsert_geometry_now(
                        projection,
                        edit,
                        view,
                        id.clone(),
                        from,
                        to,
                    );
                }
            });

            ui.add_space(space::XS);
            if ui
                .button(egui::RichText::new("删除整个势力").color(egui::Color32::RED))
                .clicked()
            {
                faction_panel_actions::remove_faction_now(projection, edit, view, id.clone());
            }
        });
}

/// 校验（P-8）。
pub fn panel_validation(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    diagnostics: Res<crate::state::ProjectDiagnostics>,
    projection: Res<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut view: ResMut<ViewState>,
    mut playback: ResMut<crate::state::PlaybackState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let d = &diagnostics.diagnostics;
    let n_errors = d.errors().len();
    let n_hints = d.hints().len();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Validation,
        None,
        |ui| {
            // 顶部状态条：错误用 Err 徽标，提示用 Warn 徽标。
            if n_errors > 0 {
                badge(ui, &shell, BadgeKind::Err, &format!("错误 {n_errors}"));
            } else {
                badge(ui, &shell, BadgeKind::Ok, "无错误");
            }
            ui.add_space(space::SM);
            badge(ui, &shell, BadgeKind::Warn, &format!("提示 {n_hints}"));
            ui.add_space(space::SM);
            ui.label(
                egui::RichText::new("四维校验：时间 / 引用 / 几何 / 一致性")
                    .size(shell.scale.small)
                    .color(shell.t2()),
            );
            ui.add_space(space::XS);
            ui.label(
                egui::RichText::new("校验只提示，不阻止保存（AGENTS §7.5）")
                    .size(shell.scale.micro)
                    .color(shell.t3()),
            );
            ui.add_space(space::SM);
            // 错误列表（按指纹 dedup 后仍可能有 N 条同类）。
            if n_errors > 0 {
                ui.label(
                    egui::RichText::new("错误")
                        .size(shell.scale.small)
                        .strong()
                        .color(shell.t1()),
                );
                ui.add_space(space::XS);
                egui::ScrollArea::vertical()
                    .max_height(shell.row * 6.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for err in d.errors() {
                            ui.horizontal(|ui| {
                                badge(ui, &shell, BadgeKind::Err, "E");
                                ui.add_space(space::XS);
                                ui.label(
                                    egui::RichText::new(&err.message)
                                        .size(shell.scale.small)
                                        .color(shell.t1()),
                                );
                            });
                        }
                    });
                ui.add_space(space::SM);
            }
            // 提示列表（相遇提示 / 事件位移可行性等）。
            if n_hints > 0 {
                ui.label(
                    egui::RichText::new("提示")
                        .size(shell.scale.small)
                        .strong()
                        .color(shell.t1()),
                );
                ui.add_space(space::XS);
                egui::ScrollArea::vertical()
                    .max_height(shell.row * 8.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for h in d.hints() {
                            ui.horizontal(|ui| {
                                badge(ui, &shell, BadgeKind::Warn, "H");
                                ui.add_space(space::XS);
                                // 阶段八 §A：hint 行变可点。点击触发跳转
                                // (drill_to + camera + day)。无 anchor 的提示
                                // （如 `fingerprint` 派生但还没几何）退化为普通 label。
                                if let Some(anchor) = &h.anchor {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(&h.message)
                                                    .size(shell.scale.small)
                                                    .color(shell.t1()),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        validation_actions::jump_to_anchor_now(
                                            &projection,
                                            &mut drill,
                                            &mut view,
                                            &mut playback,
                                            anchor,
                                        );
                                    }
                                } else {
                                    ui.label(
                                        egui::RichText::new(&h.message)
                                            .size(shell.scale.small)
                                            .color(shell.t1()),
                                    );
                                }
                            });
                        }
                    });
            } else if n_errors == 0 {
                ui.add_space(space::MD);
                ui.label(
                    egui::RichText::new("一切正常")
                        .size(shell.scale.small)
                        .color(shell.t3()),
                );
            }
        },
    );
}

/// 人物登记（P-9）。
pub fn panel_character(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let count = projection.world.characters.len();
    let badge = if count == 0 {
        None
    } else {
        Some((BadgeKind::Neutral, count.to_string()))
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Character,
        badge,
        |ui| {
            let chars = &projection.world.characters;
            if chars.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "◎",
                    "暂无已登记人物",
                    "人物是叙事的载体：登记一个，让它走进历史",
                    "登记人物 →",
                );
            } else {
                for c in chars {
                    list_item(ui, &shell, shell.accent(), &c.name, "", false);
                }
            }
        },
    );
}

/// 事件登记（P-10，可从时间轴进入）。
pub fn panel_event(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let shell = UiShell::new(&shell_cfg);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let count = projection.world.events.len();
    let badge = if count == 0 {
        None
    } else {
        Some((BadgeKind::Event, count.to_string()))
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Event,
        badge,
        |ui| {
            let events = &projection.world.events;
            if events.is_empty() {
                empty_state(
                    ui,
                    &shell,
                    "○",
                    "暂无已登记事件",
                    "事件是时间轴上的锚点：登记一个，让它在地图上出现",
                    "登记事件 →",
                );
            } else {
                for e in events {
                    let meta = match e.span.to {
                        Some(to) => format!("第 {}–{} 天", e.span.from, to),
                        None => format!("第 {} 天", e.span.from),
                    };
                    list_item(
                        ui,
                        &shell,
                        crate::palette::palette_to_color32(crate::palette::ACCENT_EVENT),
                        e.id.as_str(),
                        &meta,
                        false,
                    );
                }
            }
        },
    );
}

// ---------------------------------------------------------------------------
// 命令面板（§6.3）
// ---------------------------------------------------------------------------

/// 命令面板：`Ctrl+K` 打开，`Esc` 关闭，模糊搜索，键盘完全可操作。
pub fn command_palette(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    recents_res: Res<RecentProjects>,
    mut palette: ResMut<CommandPalette>,
) {
    let shell = UiShell::new(&shell_cfg);
    // 「最近工程」是动态命令，每次绘制时按当前工程展开（v0.2 §C）。
    let recents = recents_res.visible(&projection);
    let ctx: &mut egui::Context = contexts.ctx_mut();
    if !palette.open {
        return;
    }

    // 遮罩。
    egui::Area::new(egui::Id::new("wv_cmdk_scrim"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let rect = ctx.screen_rect();
            ui.allocate_rect(rect, egui::Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(120));
        });

    let screen = ctx.screen_rect();
    let w = (screen.width() * 0.45).clamp(360.0, 560.0);
    let x = (screen.width() - w) * 0.5;
    let y = screen.height() * 0.22;
    egui::Window::new("")
        .id(egui::Id::new("wv_cmdk"))
        .fixed_pos(egui::pos2(x, y))
        .fixed_size(egui::vec2(w, 380.0))
        .title_bar(false)
        .resizable(false)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(shell.panel_bg())
                .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                .inner_margin(egui::Margin::same(space::MD))
                .rounding(radius::MODAL),
        )
        .show(ctx, |ui| {
            // 输入框（14px）。
            let resp = ui.add(
                egui::TextEdit::singleline(&mut palette.query)
                    .hint_text("搜索命令、面板、对象…  例：笔刷 / 撤销")
                    .font(proportional(font_size::INPUT))
                    .desired_width(f32::INFINITY),
            );
            resp.request_focus();
            ui.add_space(space::SM);

            let results = palette.results(&recents);
            if results.is_empty() {
                ui.label(
                    egui::RichText::new("无匹配命令")
                        .size(shell.scale.small)
                        .color(shell.t3()),
                );
            } else {
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for (i, cmd) in results.iter().enumerate() {
                            let sel = palette.selected == i;
                            let row = ui.add_sized(
                                egui::vec2(ui.available_width(), shell.row + space::XS),
                                egui::SelectableLabel::new(sel, &cmd.label),
                            );
                            if row.clicked() {
                                palette.selected = i;
                                palette.run_selected(&recents);
                            }
                            // 快捷键（右侧等宽小字）。
                            if let Some(k) = cmd.keys {
                                let rect = row.rect;
                                ui.painter().text(
                                    egui::pos2(rect.max.x - space::SM, rect.center().y),
                                    egui::Align2::RIGHT_CENTER,
                                    k,
                                    mono(shell.scale.micro),
                                    shell.t3(),
                                );
                            }
                        }
                    });
            }

            // 键盘：↑ ↓ ↵ Esc。
            let input = ui.input(|i| i.clone());
            if input.key_pressed(egui::Key::Escape) {
                palette.close();
            } else if input.key_pressed(egui::Key::ArrowDown) {
                // 旧写法是 `.max(len-1)`，那会把光标永远钉在最后一项；
                // 正确语义是「向下走一格，到底停住」。
                palette.selected = (palette.selected + 1).min(results.len().saturating_sub(1));
            } else if input.key_pressed(egui::Key::ArrowUp) {
                palette.selected = palette.selected.saturating_sub(1);
            } else if input.key_pressed(egui::Key::Enter) {
                palette.run_selected(&recents);
            }
        });
}
