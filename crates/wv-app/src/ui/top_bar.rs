//! 顶栏（38px v0.3）+ 状态栏（22px v0.3）。
//!
//! ## 职责
//! - `top_bar`：身份 / 主控 / 读数 三段式；Figma `5E8BCBCmtGAHqk3ORibBlU` 风格：
//!   左侧项目名 + 面包屑；中段 `Browse`/`Edit` 双段 + 地图主题分段 + `Rebuild`；
//!   右侧 zoom / entities / 脏态徽标 / ⌘K。
//! - `status_bar`：实体计数 / 路径 / zoom / Saved / warnings / 当前文件。
//!
//! ## 边界
//! - **不**画项目管理菜单的展开式内容 —— [`super::project_menu::project_menu`]。
//! - **不**装配 Bevy App / Startup —— [`crate::app_bootstrap`]。
//!
//! ## 关键决策（v0.3 · 2026-10-15 用户拍板）
//! - Figma 用 `Browse` / `Edit` 英文标签；数据层 `ViewMode::label()` 仍输出
//!   中文「矢量地形 / 色块地形」（命令面板/快捷键依赖）。**仅顶栏**改英文：
//!   `Browse` ↔ `ViewMode::VectorFlat`（默认），`Edit` ↔ `ViewMode::Schematic`。
//! - 高度以 **web 实现源码**（`index.css` `.topbar`/`.statusbar`）为准：
//!   38px / 22px（v0.3 初版按渲染图猜的 44/24 已回退）。
//! - `Browse`/`Edit` 选中段是 `bg_active` **灰底**（web `.segmented
//!   .button.active`），蓝底只给主按钮；分段容器底是 `bg_app`。
//! - 脏态：顶栏右侧 warn 徽标 `● Unsaved changes`（web `.badge.warn`）；
//!   状态栏 `Saved locally` 走 ok 徽标、`N warnings` 走 warn 徽标。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{RecentProject, RecentProjects, ViewMode, ViewState, WorldProjection};
use crate::theme::ui_theme::{color32, mono, proportional, UiShellConfig};

use super::project_menu::{project_menu_inner, ProjectMenuState};
use super::shell::UiShell;
use super::widgets::{
    badge, BadgeKind, ICON_ALERT, ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_COMMAND, ICON_REFRESH,
};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui_command_palette::CommandId;

/// 顶栏：身份 / 主控 / 读数。**唯一例外**的 `TopBottomPanel`（v0.3：44px）。
#[allow(clippy::too_many_arguments)]
pub fn top_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    recents_res: Res<RecentProjects>,
    mut menu: ResMut<ProjectMenuState>,
    mut palette: ResMut<crate::ui_command_palette::CommandPalette>,
    mut view: ResMut<ViewState>,
    mut panels: ResMut<crate::state::PanelState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // v0.3 校准：web `.topbar` 高 38px（初版按渲染图猜 44px，已回退）。
    let h = 38.0;
    let recents = recents_res.visible(&projection);
    let mut root = root_ui(ctx);
    egui::Panel::top("wv_top_bar")
        .exact_size(h)
        .frame(
            egui::Frame::NONE
                .fill(color32(shell.palette.bg_panel))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .inner_margin(crate::theme::ui_theme::margin_xy(space::MD, 0.0)),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                // ── 身份：项目名（带 chevron） + 面包屑 ──
                identity_segment(
                    ui,
                    &shell,
                    &projection,
                    &view,
                    &recents,
                    &mut panels,
                    &mut menu,
                );
                ui.add_space(space::LG);

                // ── 主控：Browse / Edit 双段（Figma v0.3）──
                browse_edit_segment(ui, &shell, &mut view);
                ui.add_space(space::SM);
                // ── 地图主题（ADR-0016：日间古舆图 / 暮色旧纸，与技法正交）──
                // v0.3 校准：web 顶栏中段确有主题分段（Dusk Old Paper / Sunlit
                // Ancient Atlas），对应本项目 MapPalette——M1 曾误删，现恢复。
                map_palette_segment(ui, &shell, &mut view);
                ui.add_space(space::SM);
                // ── Rebuild 图标按钮（web 顶栏为带字按钮，此处保留图标+tooltip）──
                if mini_icon_button(ui, &shell, ICON_REFRESH, "强制重画地形（F5）").clicked()
                {
                    palette.pending = Some(CommandId::ForceRebuildTexture);
                }

                // 读数推到右端。
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    readout_segment(ui, &shell, &view, &projection);
                });
            });
        });
}

fn identity_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    view: &ViewState,
    recents: &[RecentProject],
    panels: &mut crate::state::PanelState,
    menu: &mut ProjectMenuState,
) {
    // 项目管理菜单：图标（蓝书）+ 工程名 + chevron
    project_menu_inner(ui, shell, projection, recents, panels, menu);
    ui.add_space(space::MD);

    // 面包屑：World › Zunyi › Government Office
    //  末级 = `t1`（深），上级 = `t3`（弱）；与 `crumb_text_3` 同口径。
    let crumb = crate::ui_time::format_breadcrumb(projection, &view.current_map);
    for (i, seg) in crumb.iter().enumerate() {
        if i > 0 {
            // 弱色 chevron 分隔
            ui.add(
                egui::Image::new(ICON_CHEVRON_RIGHT)
                    .tint(shell.t3())
                    .fit_to_exact_size(egui::vec2(12.0, 12.0)),
            );
            ui.add_space(space::XS);
        }
        let color = if i + 1 == crumb.len() {
            shell.t1()
        } else {
            shell.t3()
        };
        ui.label(
            egui::RichText::new(seg.as_str())
                .size(shell.scale.base)
                .color(color),
        );
        ui.add_space(space::XS);
    }

    // 工程未加载时的醒目错误（§5.1）。脏态徽标在右侧读数段（web `.top-right`）。
    if projection.load_error.is_some() {
        ui.add_space(space::SM);
        badge(ui, shell, BadgeKind::Err, "工程未加载");
    }
}

/// Figma v0.3：Browse / Edit 双段。
///
/// 与旧 `view_mode_segment` 同款手绘（密度敏感），但标签英文、内部映射：
/// - `Browse` → `ViewMode::VectorFlat`（默认浏览）
/// - `Edit`   → `ViewMode::Schematic`（色块格子，更适合编辑）
///
/// 数据层 `view.mode` 仍按原枚举流转；`CommandId::SetViewMode` 与命令面板不
/// 受影响（命令面板的 label 来自 `ViewMode::label()`，保持中文「矢量地形/色块地形」）。
///
/// 样式以 web `.segmented` 为准：容器 `bg_app` 底 + `border` 描边，选中段
/// `bg_active` 灰底 + `text_1`（蓝底只给主按钮，v0.3 初版的 accent_soft 已回退）。
fn browse_edit_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    let segs: [(&str, ViewMode); 2] = [
        ("Browse", ViewMode::VectorFlat),
        ("Edit", ViewMode::Schematic),
    ];
    let n = segs.len() as f32;
    let seg_w = 140.0;
    let seg_h = shell.hit;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CONTROL, color32(shell.palette.bg_app));
    rect_stroke_app(
        painter,
        rect,
        radius::CONTROL,
        color32(shell.palette.border_subtle),
    );
    let seg_w_each = rect.width() / n;
    for (i, (label, mode)) in segs.iter().enumerate() {
        let r = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + seg_w_each * i as f32, rect.min.y),
            egui::pos2(rect.min.x + seg_w_each * (i as f32 + 1.0), rect.max.y),
        );
        let selected = view.mode == *mode;
        let resp = ui.interact(
            r,
            egui::Id::new(("wv_browse_edit", i)),
            egui::Sense::click(),
        );
        if selected {
            painter.rect_filled(r, radius::CONTROL, shell.active_bg());
        } else if resp.hovered() {
            painter.rect_filled(r, radius::CONTROL, shell.hover_bg());
        }
        let fg = if selected { shell.t1() } else { shell.t2() };
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            proportional(font_size::SMALL),
            fg,
        );
        if resp.clicked() {
            view.mode = *mode;
        }
    }
}

/// 地图主题分段控件（日间古舆图 / 暮色旧纸，ADR-0016）。
///
/// v0.3 校准恢复：web 顶栏中段的 `Dusk Old Paper / Sunlit Ancient Atlas`
/// 分段即本项目的 `MapPalette` 轴（M1 重写时误删）。样式与
/// [`browse_edit_segment`] 同款（web `.segmented`），两段定宽 64px。
fn map_palette_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    use crate::theme::palette::MapPalette;
    let n = MapPalette::ALL.len() as f32;
    let seg_w = n * 64.0;
    let seg_h = shell.hit;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CONTROL, color32(shell.palette.bg_app));
    rect_stroke_app(
        painter,
        rect,
        radius::CONTROL,
        color32(shell.palette.border_subtle),
    );
    let seg_w_each = rect.width() / n;
    for (i, p) in MapPalette::ALL.iter().enumerate() {
        let r = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + seg_w_each * i as f32, rect.min.y),
            egui::pos2(rect.min.x + seg_w_each * (i as f32 + 1.0), rect.max.y),
        );
        let selected = view.map_palette == *p;
        let resp = ui.interact(
            r,
            egui::Id::new(("wv_palette_seg", i)),
            egui::Sense::click(),
        );
        if selected {
            painter.rect_filled(r, radius::CONTROL, shell.active_bg());
        } else if resp.hovered() {
            painter.rect_filled(r, radius::CONTROL, shell.hover_bg());
        }
        let fg = if selected { shell.t1() } else { shell.t2() };
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            p.label(),
            proportional(font_size::SMALL),
            fg,
        );
        if resp.clicked() {
            view.map_palette = *p;
        }
    }
}

/// 顶栏分段容器的内描边（统一 `StrokeKind::Inside`，与 widgets::rect_stroke
/// 同口径；本文件自持一份以免引 pub(crate) 跨模块）。
fn rect_stroke_app(
    painter: &egui::Painter,
    rect: egui::Rect,
    corner_radius: impl Into<egui::CornerRadius>,
    color: egui::Color32,
) {
    painter.rect_stroke(
        rect,
        corner_radius,
        egui::Stroke::new(1.0, color),
        egui::StrokeKind::Inside,
    );
}

fn readout_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &WorldProjection,
) {
    // 右至左排布；web `.top-right` 从左到右是 zoom / entities / 脏态徽标 / ⌘K，
    // 故本段添加顺序：⌘K → 脏态 → entities → zoom。
    // ⌘K 提示（点击触发命令面板）
    let cmdk = ui.add(
        egui::Image::new(ICON_COMMAND)
            .tint(shell.t3())
            .fit_to_exact_size(egui::vec2(14.0, 14.0)),
    );
    let _ = cmdk.on_hover_text("命令面板（Ctrl+K / ⌘K）");
    ui.label(
        egui::RichText::new("K")
            .size(shell.scale.small)
            .color(shell.t3())
            .font(mono(shell.scale.small)),
    );
    ui.add_space(space::MD);

    // 脏态徽标（web `.badge.warn`：橙底 + 圆点；已保存时不画，状态栏负责）。
    if projection.dirty {
        badge(ui, shell, BadgeKind::Warn, "● Unsaved changes");
        ui.add_space(space::MD);
    }

    // 实体计数（"128 entities"）
    let entity_count = count_entities(projection);
    ui.label(
        egui::RichText::new(format!("{entity_count} entities"))
            .size(shell.scale.small)
            .color(shell.t2())
            .font(mono(shell.scale.small)),
    );
    ui.add_space(space::MD);

    // 缩放百分比（"86%"）—— Figma 是无小数百分比
    let pct = (view.scale * 100.0).round() as i32;
    ui.label(
        egui::RichText::new(format!("{pct}%"))
            .size(shell.scale.small)
            .color(shell.t2())
            .font(mono(shell.scale.small)),
    );
}

fn count_entities(projection: &WorldProjection) -> usize {
    let w = &projection.world;
    w.units.len() + w.characters.len() + w.situations.len() + w.events.len()
}

/// 顶栏用的小图标按钮（统一 `hit` 高、hover 走 `hover_bg`、点中走 `active_bg`）。
///
/// v0.3：图标从单字符 `↻` 升级为 Lucide SVG（`ICON_REFRESH` 等），由
/// `tint` 注入主题色；保持 hover 走 `hover_bg`、透明底语义。
fn mini_icon_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    icon: egui::ImageSource<'static>,
    tooltip: &str,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(shell.hit, shell.hit), egui::Sense::click());
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
    } else {
        painter.rect_filled(rect, 0.0, egui::Color32::TRANSPARENT);
    }
    // 直接用 egui::Image widget（与 widgets.rs L539 Button::image 同口径），
    // 避免手工 painter.image() 需要 TextureId 的绕路。
    let icon_size = shell.hit - 10.0;
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size));
    // 复用 ui 的子作用域画 image，egui 自动把 child 添加到当前 layout。
    let _ = ui.put(
        icon_rect,
        egui::Image::new(icon)
            .tint(shell.t1())
            .fit_to_exact_size(egui::vec2(icon_size, icon_size)),
    );
    resp.on_hover_text(tooltip)
}

// 抑制未使用 import 警告 —— `ICON_ALERT` 暂未在顶栏直接用，但保留作为未来 v0.4 入口。
#[allow(dead_code)]
const _ICON_ALERT_PRESENT_FOR_FUTURE_USE: egui::ImageSource<'static> = ICON_ALERT;
#[allow(dead_code)]
const _ICON_CHEVRON_DOWN_PRESENT_FOR_FUTURE_USE: egui::ImageSource<'static> = ICON_CHEVRON_DOWN;

/// 状态栏（v0.3：22px，web `.statusbar` 精确值）。
///
/// - 左：实体计数 `6 units · 12 characters · 2 situations · 28 events`
/// - 中：当前路径 `Zunyi / Government Office` + zoom `Zoom 86%`
/// - 右：`Saved locally`（ok 徽标）/ `N warnings`（warn 徽标）/ `ui-layout.yaml`
pub fn status_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    validation_count: Res<ValidationCount>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let mut root = root_ui(ctx);
    egui::Panel::bottom("wv_status_bar")
        .exact_size(22.0)
        .frame(
            egui::Frame::NONE
                .fill(color32(shell.palette.bg_panel))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .inner_margin(crate::theme::ui_theme::margin_xy(space::MD, 0.0)),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                // ── 左：实体计数（mono 数字）──
                let w = &projection.world;
                ui.label(
                    egui::RichText::new(format!(
                        "{} units · {} characters · {} situations · {} events",
                        w.units.len(),
                        w.characters.len(),
                        w.situations.len(),
                        w.events.len()
                    ))
                    .size(shell.scale.micro)
                    .color(shell.t3())
                    .font(mono(shell.scale.micro)),
                );

                // ── 中 + 右：右对齐的路径 / zoom / 状态徽标 / 文件名 ──
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 文件名（最右 · 弱文本）
                    ui.label(
                        egui::RichText::new("ui-layout.yaml")
                            .size(shell.scale.micro)
                            .color(shell.t3())
                            .font(mono(shell.scale.micro)),
                    );
                    ui.add_space(space::MD);

                    // 警告徽标（web `.badge.warn`：warn 语义底 + 前景）
                    if validation_count.warnings > 0 {
                        badge(
                            ui,
                            &shell,
                            BadgeKind::Warn,
                            &format!(
                                "{} warning{}",
                                validation_count.warnings,
                                if validation_count.warnings == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ),
                        );
                        ui.add_space(space::MD);
                    }

                    // Saved locally 徽标（web `.badge.ok`；脏态时顶栏右侧已有
                    // Unsaved changes，此处不画）
                    if !projection.dirty {
                        badge(ui, &shell, BadgeKind::Ok, "Saved locally");
                        ui.add_space(space::MD);
                    }

                    // 当前路径（mono）
                    let path = crate::ui_time::format_breadcrumb(&projection, &view.current_map)
                        .join(" / ");
                    ui.label(
                        egui::RichText::new(path)
                            .size(shell.scale.micro)
                            .color(shell.t2())
                            .font(mono(shell.scale.micro)),
                    );
                    ui.add_space(space::SM);
                    ui.label(
                        egui::RichText::new("|")
                            .size(shell.scale.micro)
                            .color(shell.t3()),
                    );
                    ui.add_space(space::SM);

                    // Zoom 百分比
                    let pct = (view.scale * 100.0).round() as i32;
                    ui.label(
                        egui::RichText::new(format!("Zoom {pct}%"))
                            .size(shell.scale.micro)
                            .color(shell.t2())
                            .font(mono(shell.scale.micro)),
                    );
                });
            });
        });
}

/// 验证警告数（v0.3：状态条右侧徽标需要）。
///
/// wv-app 当前没有现成的 Resource 暴露「总验证警告数」——它由
/// `validation_actions` 模块现算。为不引入新依赖，本 Resource 由
/// `app_bootstrap` 在 Update 链上每帧 `insert_resource(ValidationCount{warnings:...})`。
/// 未来 M3 真接 `validation` Resource 时可直接替换。
#[derive(Default, Resource, Debug, Clone, Copy)]
pub struct ValidationCount {
    pub warnings: usize,
}

/// 构造一个「视口根 `Ui`」。
pub(crate) fn root_ui(ctx: &egui::Context) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        "wv_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    )
}
