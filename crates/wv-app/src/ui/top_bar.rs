//! 顶栏（§3.2 三段式）+ 状态栏（§3.1 底部 22px）。
//!
//! ## 职责
//! - `top_bar`：身份 → 主控 → 读数，三段式布局；身份段含项目管理菜单。
//! - `status_bar`：实体计数 / 已选 / 缩放 / 操作回执。
//!
//! ## 边界（不做什么）
//! - **不**画项目管理菜单的展开式内容 —— [`super::project_menu::project_menu`]。
//! - **不**装配 Bevy App / Startup —— 那是 [`crate::app_bootstrap`]。
//!
//! ## 主要导出
//! - `top_bar` —— 顶部固定栏 Bevy 系统。
//! - `status_bar` —— 底部固定栏 Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{
    PlaybackState, RecentProject, RecentProjects, ViewMode, ViewState, WorldProjection,
};
use crate::ui_theme::{color32, mono, proportional, UiShellConfig};

use super::project_menu::{project_menu_inner, ProjectMenuState};
use super::shell::UiShell;
use super::widgets::{badge, BadgeKind};

use crate::ui_tokens::{font_size, radius, space};

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
    mut panels: ResMut<crate::state::PanelState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let h = shell_cfg.density.top_bar_height();
    let recents = recents_res.visible(&projection);
    let mut root = root_ui(ctx);
    egui::Panel::top("wv_top_bar")
        .exact_size(h)
        .frame(
            egui::Frame::NONE
                .fill(color32(shell.palette.bg_panel))
                .inner_margin(crate::ui_theme::margin_xy(space::SM, 0.0)),
        )
        .show(&mut root, |ui| {
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
    panels: &mut crate::state::PanelState,
    menu: &mut ProjectMenuState,
) {
    // 项目管理菜单（v0.2 §A）。入口就是工程名本身——不用固定菜单栏（§3.2 三段式密度）。
    project_menu_inner(ui, shell, projection, recents, panels, menu);

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
    // 分段控件：等分 `ViewMode::ALL`（当前 6 个模式，窄窗下不截断，§8 断点）。
    // 宽度按分段数缩放，clamp 上下限保证每段仍有 ≥ 55px 可点区域。
    let n = ViewMode::ALL.len() as f32;
    let seg_w = (total * 0.40).clamp(n * 55.0, n * 85.0);
    let seg_h = shell.hit;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(seg_w, seg_h), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, radius::CONTROL, shell.raised_bg());
    let seg_w_each = rect.width() / n;
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
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let mut root = root_ui(ctx);
    egui::Panel::bottom("wv_status_bar")
        .exact_size(22.0)
        .frame(
            egui::Frame::NONE
                .fill(color32(shell.palette.bg_panel))
                .inner_margin(crate::ui_theme::margin_xy(space::SM, 0.0)),
        )
        .show(&mut root, |ui| {
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

/// 构造一个「视口根 `Ui`」。
///
/// egui 0.36 起，`Panel` / `CentralPanel` 收 `&mut Ui` 而不是 `&Context`
/// （框架改成「一次 `Context::run_ui` 产生根 Ui，面板从根 Ui 里切条」）。
/// bevy_egui 0.42 在 `run_ui` 里把根 Ui 丢弃了，所以顶栏/状态栏各自造一个：
/// 两者的 `max_rect` 都是整个视口，各自从自己的 `PanelState` 取边条，
/// 因此互不干扰（中间整块永远留给世界渲染，egui 不占）。
pub(crate) fn root_ui(ctx: &egui::Context) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        "wv_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    )
}
