//! 右下角浮动工具条（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU` 风格）。
//!
//! ## 职责
//! - `map_controls` —— 右下角工具切换（笔刷 / 折线 / 矩形 / 橡皮 / 缩放 / 全图）。
//! - `set_map_scale` —— 写入缩放并同步相机 Transform（Camera 是渲染真源）。
//! - 块级测试 `map_control_tests` —— `＋/－` 1.6× 步进夹在级夹持区间内。
//!
//! ## 边界（不做什么）
//! - **不**绘制顶栏/状态栏 —— 那是 [`super::top_bar`]。
//! - **不**使用 `colors_google` —— v0.3 起该工具条统一走 `UiShell` 令牌。
//!
//! ## v0.3 变更
//! - **去 Google 色**：旧版用 `GOOGLE_CONTROL_BG/_BORDER/_TEXT/_ACTIVE`
//!   四色直写，现在统一从 [`UiShell`] 取（`panel_bg` / `border_strong` / `t1`
//!   / `active_bg`），与其它面板同源。
//! - 按钮顺序与 Figma 一致：pen / line / rect / eraser · sep · ＋ / − · sep · fullscreen。
//! - 位置：贴底右边 `-16, -50`（`-38` 旧值会让状态条盖住最后 1 个按钮）。
//!
//! ## 主要导出
//! - `map_controls` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{
    BrushState, DrillState, GridConfig, MeasureMode, MeasureState, ViewState, WorldProjection,
};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::radius;

use super::shell::UiShell;
use super::{
    ToolButton, ToolIcon, ICON_ERASER, ICON_LINE, ICON_MAXIMIZE, ICON_PEN, ICON_SQUARE,
    ICON_ZOOM_IN, ICON_ZOOM_OUT,
};

/// 右下角地图控件：拖动 / 笔刷工具、+/- 缩放、全图复位。
///
/// v0.3 起视觉与 Figma 一致（深色面板 + shell 令牌）；写相机仍经
/// `apply_view_to_camera`，因为 Camera Transform 才是渲染真源。
#[allow(clippy::too_many_arguments)]
pub fn map_controls(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut view: ResMut<ViewState>,
    mut brush: ResMut<BrushState>,
    mut measure: ResMut<MeasureState>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<crate::render::terrain_view::TerrainTexture>,
    projection: Res<WorldProjection>,
    windows: Query<&Window>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // 地形笔刷只作用于世界图；钻入城市 / 建筑后强制回到拖动，避免"铅笔亮着却不能画"。
    let brush_available = drill.depth() == 0;
    if !brush_available {
        brush.enabled = false;
    }
    // 当前级的缩放区间与「全图」尺寸。
    let window_size = windows
        .single()
        .map(|w| w.resolution.size())
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let raster_size = Vec2::splat(grid.cell_size * tex.size as f32);
    let level_map_size =
        crate::render::zoom_level::map_size_of(&projection.world, drill.current(), raster_size);
    let zoom = crate::render::zoom_level::LevelZoom::for_map(level_map_size, window_size);

    // v0.3：统一走 shell 令牌（深色面板 + shell 边框 + 文字色 + 选中底）。
    // `panel_bg()` 已经包含 `float_alpha`（暗 .94 / 亮 .93），与旧 GOOGLE 同款观感。
    let panel_bg = shell.panel_bg();
    let panel_border = shell.border_strong();
    let text_color = shell.t1();
    let active_bg = shell.active_bg();

    // v0.3：位置上移 12px（-38 → -50），让出底部 24px 状态条。
    egui::Area::new(egui::Id::new("wv_map_controls_v03"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -50.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_min_width(48.0);
            egui::Frame::NONE
                .fill(panel_bg)
                .stroke(egui::Stroke::new(1.0_f32, panel_border))
                .corner_radius(egui::CornerRadius::from(radius::PANEL))
                .inner_margin(crate::theme::ui_theme::margin_xy(4.0, 4.0))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        let button_size = egui::vec2(38.0, shell.hit);

                        // ---- 笔刷 / 平移 toggle ----
                        ToolButton::icon(ToolIcon::Svg(ICON_PEN), "切换工具（当前：笔刷 / 拖动）")
                            .active(brush.enabled)
                            .enabled(brush_available)
                            .size(button_size)
                            .tint(text_color)
                            .active_bg(active_bg)
                            .show(ui, &shell, || {
                                brush.enabled = !brush.enabled;
                            });

                        // ---- 测量工具三按钮（Figma：line / square / eraser）----
                        // 用 v0.3 新加的 `ICON_LINE`（minus.svg）替代 ruler 的折线感。
                        let project_open = projection.project_dir.is_some();
                        let line_active = measure.mode == MeasureMode::Line;
                        let rect_active = measure.mode == MeasureMode::Rect;
                        let eraser_active = measure.mode == MeasureMode::Eraser;
                        let mut measure_pick =
                            |target: MeasureMode,
                             icon: egui::ImageSource<'static>,
                             hint: &str,
                             active: bool| {
                                ToolButton::icon(ToolIcon::Svg(icon), hint)
                                    .active(active)
                                    .enabled(project_open)
                                    .size(button_size)
                                    .tint(text_color)
                                    .active_bg(active_bg)
                                    .show(ui, &shell, || {
                                        // 二态切换：再点一次同一模式 → 关闭。
                                        measure.mode =
                                            if active { MeasureMode::Off } else { target };
                                        // 与笔刷互斥：开任一测量模式 → 关闭笔刷。
                                        if measure.mode.is_active() {
                                            brush.enabled = false;
                                        }
                                        // 切模式时丢弃草稿。
                                        measure.clear_draft();
                                    });
                            };
                        measure_pick(
                            MeasureMode::Line,
                            ICON_LINE,
                            "测量：折线（多段；右键/Esc 收笔）",
                            line_active,
                        );
                        measure_pick(
                            MeasureMode::Rect,
                            ICON_SQUARE,
                            "测量：矩形（两点确定长×宽）",
                            rect_active,
                        );
                        measure_pick(
                            MeasureMode::Eraser,
                            ICON_ERASER,
                            "测量：橡皮（点选删除）",
                            eraser_active,
                        );

                        ui.add_space(2.0);
                        ui.separator();

                        // ---- 缩放 / 全图 ----
                        ToolButton::icon(ToolIcon::Svg(ICON_ZOOM_IN), "放大")
                            .size(button_size)
                            .tint(text_color)
                            .show(ui, &shell, || {
                                let next = view.scale / 1.6;
                                set_map_scale(&mut view, &mut cameras, next, zoom);
                            });
                        ToolButton::icon(ToolIcon::Svg(ICON_ZOOM_OUT), "缩小")
                            .size(button_size)
                            .tint(text_color)
                            .show(ui, &shell, || {
                                let next = view.scale * 1.6;
                                set_map_scale(&mut view, &mut cameras, next, zoom);
                            });
                        ui.separator();
                        ToolButton::icon(ToolIcon::Svg(ICON_MAXIMIZE), "缩放到全图")
                            .size(button_size)
                            .tint(text_color)
                            .show(ui, &shell, || {
                                let frame_origin = drill
                                    .current()
                                    .and_then(|id| {
                                        wv_core::resolve_frame_in(&projection.world.maps, id)
                                    })
                                    .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
                                    .unwrap_or(grid.map_origin);
                                view.camera_center = frame_origin + level_map_size * 0.5;
                                set_map_scale(&mut view, &mut cameras, zoom.fit, zoom);
                            });
                    });
                });
        });
}

/// 写入缩放并同步相机，避免 ViewState 与 Camera 变成两个真相。
fn set_map_scale(
    view: &mut ViewState,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    scale: f32,
    zoom: crate::render::zoom_level::LevelZoom,
) {
    view.scale = zoom.clamp(scale);
    crate::render::camera::apply_view_to_camera(view.camera_center, view.scale, cameras);
}

#[cfg(test)]
mod map_control_tests {
    use bevy::prelude::Vec2;

    /// `＋/－` 每次 1.6×（`view.scale / 1.6`、`* 1.6`），经 `set_map_scale` 夹在
    /// **当前级**区间内：连按到底应停在内界/外界，而不是穿过去。
    #[test]
    fn zoom_button_steps_stop_at_the_levels_band() {
        let zoom = crate::render::zoom_level::LevelZoom::for_map(
            Vec2::splat(1024.0),
            Vec2::new(1280.0, 800.0),
        );
        let mut s = zoom.fit;
        for _ in 0..40 {
            s = zoom.clamp(s / 1.6);
        }
        assert!((s - zoom.min).abs() < 1e-6, "got {s}, min {}", zoom.min);
        let mut s = zoom.fit;
        for _ in 0..40 {
            s = zoom.clamp(s * 1.6);
        }
        assert!((s - zoom.max).abs() < 1e-6, "got {s}, max {}", zoom.max);
    }
}
