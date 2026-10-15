//! 顶栏（38px v0.3 · web `.topbar` 对齐）+ 状态栏（22px）。
//!
//! ## 职责
//! - `top_bar` —— web `grid: 1fr auto 1fr` 三区：左 = 项目按钮（蓝 logo +
//!   粗体名 + 下翻箭头，见 [`super::project_menu`]）+ 面包屑；**中 = Browse /
//!   Edit 分段 + 地图主题分段 + Rebuild 按钮（独立 Area 锚视口顶部居中）**；
//!   右 = zoom / entities / 脏态徽标 / ⌘K 按钮。
//! - `status_bar` —— 实体计数 / 路径 / zoom / Saved / warnings / 当前文件。
//!
//! ## web 对齐要点（`index.css` / `App.tsx`，v0.3 校准）
//! - 高度 38px（`.topbar`）；底色 `bg_app` 96% + 底边框 + `0 4px 20px` 投影。
//! - 分段控件（`.segmented`）：容器 `bg_app` 底 + border + 圆角 7、内边距 2px；
//!   选中段 `bg_active` 底 + `text_1`，**内嵌**于容器而非顶满。
//! - 主题分段按 web 顺序与文案：`Dusk Old Paper` / `Sunlit Ancient Atlas`
//!   （10px 字）；数据层仍映射 [`crate::theme::palette::MapPalette`]。
//! - `Rebuild` / `⌘K` 是**图标+文字**的 raised 按钮（web `.button`），非裸图标。
//! - 面包屑末级 `text_2`（web `b` 标签色），非末级 `text_3`。
//!
//! ## 边界
//! - **不**画项目管理菜单的展开式内容 —— [`super::project_menu::project_menu`]。
//! - **不**装配 Bevy App / Startup —— [`crate::app_bootstrap`]。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{RecentProject, RecentProjects, ViewMode, ViewState, WorldProjection};
use crate::theme::palette::MapPalette;
use crate::theme::ui_theme::{color32, mono, proportional, UiShellConfig};

use super::project_menu::{project_menu_inner, ProjectMenuState};
use super::shell::UiShell;
use super::widgets::{
    badge, icon_image, BadgeKind, ICON_CHEVRON_RIGHT, ICON_COMMAND, ICON_REFRESH,
};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui_command_palette::CommandId;

/// 顶栏：身份 / 主控（居中独立 Area）/ 读数。**唯一例外**的 `TopBottomPanel`。
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
    // web `.topbar` 精确高度 38px。
    let h = 38.0;
    let recents = recents_res.visible(&projection);
    let mut root = root_ui(ctx);
    egui::Panel::top("wv_top_bar")
        .exact_size(h)
        .frame(
            egui::Frame::NONE
                // web：color-mix(app 96%) + border-bottom + 0 4px 20px 阴影
                .fill(color32(shell.palette.bg_app).gamma_multiply(0.96))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 20,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(56),
                })
                .inner_margin(crate::theme::ui_theme::margin_xy(space::SM, 0.0)),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                // ── 左区：项目按钮 + 面包屑（web .top-left）──
                identity_segment(
                    ui,
                    &shell,
                    &projection,
                    &view,
                    &recents,
                    &mut panels,
                    &mut menu,
                );
                // ── 右区：zoom / entities / 脏态徽标 / ⌘K（web .top-right，
                //    L→R 次序 zoom→entities→badge→⌘K，故 RTL 添加序相反）──
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    readout_segment(ui, &shell, &view, &projection);
                });
            });
        });

    // ── 中区：web grid 的 `auto` 列——独立 Area 锚视口顶部居中 ──
    //（TopBottomPanel 里做不了「剩余宽度居中」，锚定 Area 是 egui 等价物；
    //  高 38 内垂直居中：内容 shell.hit=28 → 偏移 (38-28)/2 = 5px。）
    egui::Area::new(egui::Id::new("wv_topbar_middle"))
        .anchor(
            egui::Align2::CENTER_TOP,
            egui::vec2(0.0, (h - shell.hit) * 0.5),
        )
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                browse_edit_segment(ui, &shell, &mut view);
                ui.add_space(6.0);
                theme_segment(ui, &shell, &mut view);
                ui.add_space(6.0);
                rebuild_button(ui, &shell, &mut palette);
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
    // 项目按钮（logo + 粗体名 + chevron，见 project_menu）+ 组间距。
    project_menu_inner(ui, shell, projection, recents, panels, menu);
    ui.add_space(space::MD);

    // 面包屑（web .breadcrumbs：gap 4、上级 text_3、**末级 text_2**）。
    let crumb = crate::ui_time::format_breadcrumb(projection, &view.current_map);
    for (i, seg) in crumb.iter().enumerate() {
        if i > 0 {
            ui.add(
                egui::Image::new(ICON_CHEVRON_RIGHT)
                    .tint(shell.t3())
                    .fit_to_exact_size(egui::vec2(12.0, 12.0)),
            );
            ui.add_space(space::XS);
        }
        let color = if i + 1 == crumb.len() {
            shell.t2()
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

    // 工程未加载时的醒目错误（§5.1）。脏态徽标在右侧读数段。
    if projection.load_error.is_some() {
        ui.add_space(space::SM);
        badge(ui, shell, BadgeKind::Err, "工程未加载");
    }
}

/// web `.segmented`：容器 `bg_app` 底 + border + 圆角 7 + 内边距 2px；
/// 选中段 `bg_active` 底 + `text_1`（内嵌于容器，不顶边）+ 1px 投影。
#[allow(clippy::too_many_arguments)]
fn segmented(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: &str,
    seg_w_each: f32,
    labels: &[&str],
    active_index: usize,
    font_px: f32,
    mut on_pick: impl FnMut(usize),
) {
    let total_w = seg_w_each * labels.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, shell.hit), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0, color32(shell.palette.bg_app));
    painter.rect_stroke(
        rect,
        7.0,
        egui::Stroke::new(1.0, color32(shell.palette.border_subtle)),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(2.0);
    let each = inner.width() / labels.len().max(1) as f32;
    for (i, label) in labels.iter().enumerate() {
        let r = egui::Rect::from_min_max(
            egui::pos2(inner.min.x + each * i as f32, inner.min.y),
            egui::pos2(inner.min.x + each * (i as f32 + 1.0), inner.max.y),
        );
        let resp = ui.interact(r, egui::Id::new((id, i)), egui::Sense::click());
        if i == active_index {
            // web `.segmented .button.active`：active 底 + 0 1px 3px 阴影 + text_1
            painter.rect_filled(r, 5.0, shell.active_bg());
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                proportional(font_px),
                shell.t1(),
            );
        } else {
            if resp.hovered() {
                painter.rect_filled(r, 5.0, shell.hover_bg());
            }
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                proportional(font_px),
                shell.t2(),
            );
        }
        if resp.clicked() {
            on_pick(i);
        }
    }
}

/// Browse / Edit 双段：`Browse` → `ViewMode::VectorFlat`（默认浏览）、
/// `Edit` → `ViewMode::Schematic`（色块格子）。
///
/// 数据层 `view.mode` 仍按原枚举流转；命令面板 label 来自 `ViewMode::label()`
/// （中文），不受影响（v0.3 用户拍板：仅顶栏用英文）。
fn browse_edit_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    let labels = ["Browse", "Edit"];
    let modes = [ViewMode::VectorFlat, ViewMode::Schematic];
    let active = modes.iter().position(|m| *m == view.mode).unwrap_or(0);
    segmented(
        ui,
        shell,
        "wv_browse_edit",
        64.0,
        &labels,
        active,
        font_size::SMALL,
        |i| {
            view.mode = modes[i];
        },
    );
}

/// 地图主题分段（web `.theme-toggle`：`Dusk Old Paper / Sunlit Ancient Atlas`，
/// 10px 字，Dusk 在前）。数据层仍映射 [`MapPalette`]（Daylight/Dusk）。
fn theme_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    let order = [MapPalette::Dusk, MapPalette::Daylight];
    let labels = ["Dusk Old Paper", "Sunlit Ancient Atlas"];
    let active = order
        .iter()
        .position(|p| *p == view.map_palette)
        .unwrap_or(0);
    segmented(
        ui,
        shell,
        "wv_theme_seg",
        96.0,
        &labels,
        active,
        10.0,
        |i| {
            view.map_palette = order[i];
        },
    );
}

/// Rebuild 按钮（web：`<Button><Icon redo/> Rebuild</Button>`——raised 底 +
/// border 的标准按钮，非裸图标）。
///
/// **走 [`icon_image`] helper 而不是 [`crate::ui::widgets::button`] 工厂**：因为
/// 本按钮 stroke 是 `border_subtle`（与工厂默认 `border_strong` 不一致——顶栏设计稿
/// 用浅边，与对话/CTA 的强边区分）。工厂升级时若统一改 stroke，可以并入。
fn rebuild_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    palette: &mut crate::ui_command_palette::CommandPalette,
) {
    let resp = ui.add(
        egui::Button::image_and_text(
            icon_image(ICON_REFRESH, shell.t2(), shell.hit - 8.0),
            egui::RichText::new("Rebuild").size(shell.scale.small),
        )
        .fill(shell.raised_bg())
        .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
        .corner_radius(radius::CONTROL)
        .min_size(egui::vec2(0.0, shell.hit)),
    );
    if resp.clicked() {
        palette.pending = Some(CommandId::ForceRebuildTexture);
    }
}

fn readout_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &WorldProjection,
) {
    // 右至左排布；web `.top-right` 从左到右是 zoom / entities / 脏态徽标 / ⌘K，
    // 故本段添加顺序：⌘K → 脏态 → entities → zoom，间距 12px（web gap）。
    // ⌘K 按钮（web：command 图标 + K 的 raised 按钮；tooltip 提示快捷键）
    let cmdk = ui.add(
        egui::Button::image_and_text(
            icon_image(ICON_COMMAND, shell.t2(), shell.hit - 8.0),
            egui::RichText::new("K").size(shell.scale.small),
        )
        .fill(shell.raised_bg())
        .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
        .corner_radius(radius::CONTROL)
        .min_size(egui::vec2(0.0, shell.hit)),
    );
    let _ = cmdk.on_hover_text("命令面板（Ctrl+K / ⌘K）");
    ui.add_space(space::LG);

    // 脏态徽标（web `.badge.warn`：橙底 + 圆点；已保存时不画，状态栏负责）。
    if projection.dirty {
        badge(ui, shell, BadgeKind::Warn, "● Unsaved changes");
        ui.add_space(space::LG);
    }

    // 实体计数（"128 entities"）
    let entity_count = count_entities(projection);
    ui.label(
        egui::RichText::new(format!("{entity_count} entities"))
            .size(shell.scale.small)
            .color(shell.t2())
            .font(mono(shell.scale.small)),
    );
    ui.add_space(space::LG);

    // 缩放百分比（"86%"）
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
                .fill(color32(shell.palette.bg_app))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .inner_margin(crate::theme::ui_theme::margin_xy(space::SM, 0.0)),
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

                    // 警告徽标（web `.badge.warn`）
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
