//! Google 式地图控件 + 辅助函数（root_ui / rect_stroke / set_map_scale）。
//!
//! ## 职责
//! - `map_controls` —— 右下角拖动/笔刷工具切换、+/− 缩放、缩放到全图；
//!   测量工具三按钮（折线 / 矩形 / 橡皮，2026-10-14）。
//! - `set_map_scale` —— 写入缩放并同步相机 Transform（Camera 是渲染真源）。
//! - 块级测试 `map_control_tests` —— `＋/－` 1.6× 步进夹在级夹持区间内。
//!
//! ## 边界（不做什么）
//! - **不**绘制顶栏/状态栏 —— 那是 [`super::top_bar`]。
//! - **不**改 `crate::colors_google` 的色板 —— 只读。
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

use super::shell::UiShell;

/// 右下角地图控件：拖动 / 笔刷工具、+/- 缩放、全图复位。
///
/// 这组控件属于地图体验，不进浮动面板体系：它永远贴边、永远小体量，像 Google
/// Maps 的导航控件一样不遮挡地图主体。写相机必须经 `apply_view_to_camera`，
/// 因为 Camera Transform 才是渲染真源。
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
    // 当前级的缩放区间与「全图」尺寸。**级由 drill 决定**（当前在哪张图），
    // 图没声明 extent 时以实际显示的栅格（格子尺寸 × 纹理边长）兜底。
    let window_size = windows
        .single()
        .map(|w| w.resolution.size())
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let raster_size = Vec2::splat(grid.cell_size * tex.size as f32);
    let level_map_size =
        crate::render::zoom_level::map_size_of(&projection.world, drill.current(), raster_size);
    let zoom = crate::render::zoom_level::LevelZoom::for_map(level_map_size, window_size);
    let control_bg =
        crate::theme::colors::palette_to_color32(crate::theme::colors_google::GOOGLE_CONTROL_BG);
    let control_border = crate::theme::colors::palette_to_color32(
        crate::theme::colors_google::GOOGLE_CONTROL_BORDER,
    );
    let control_text =
        crate::theme::colors::palette_to_color32(crate::theme::colors_google::GOOGLE_CONTROL_TEXT);
    let active_bg = crate::theme::colors::palette_to_color32(
        crate::theme::colors_google::GOOGLE_CONTROL_ACTIVE,
    );

    egui::Area::new(egui::Id::new("wv_google_map_controls"))
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -38.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_min_width(48.0);
            egui::Frame::NONE
                .fill(control_bg)
                .stroke(egui::Stroke::new(1.0_f32, control_border))
                .corner_radius(egui::CornerRadius::from(10.0))
                .inner_margin(crate::theme::ui_theme::margin_xy(5.0, 5.0))
                .show(ui, |ui| {
                    let tool_label = if brush.enabled { "✏" } else { "✋" };
                    let mut tool_button = egui::Button::new(
                        egui::RichText::new(tool_label)
                            .size(shell.scale.base)
                            .color(control_text),
                    );
                    if brush.enabled {
                        tool_button = tool_button.fill(active_bg);
                    }
                    let tool_response = ui.add_enabled(
                        brush_available,
                        tool_button.min_size(egui::vec2(38.0, shell.hit)),
                    );
                    if tool_response.clicked() {
                        brush.enabled = !brush.enabled;
                    }
                    let tool_hint = if brush_available {
                        format!(
                            "切换工具（当前：{}）",
                            if brush.enabled { "笔刷" } else { "拖动" }
                        )
                    } else {
                        "地形笔刷仅在世界图可用；当前为拖动".to_owned()
                    };
                    tool_response.on_hover_text(tool_hint);
                    ui.add_space(2.0);
                    ui.separator();
                    // ---- 测量工具三按钮（2026-10-14）----
                    // 互斥：任一测量模式激活 → 笔刷顶下。模式 ≠ Off 时左键被
                    // `commit_measurement_click` 截走，地图拖动失效——这是预期的。
                    // 无工程锁与笔刷同口径：没打开工程不允许启用。
                    let project_open = projection.project_dir.is_some();
                    let mut pick = |target: MeasureMode, label: &str, hint: &str| {
                        let active = measure.mode == target;
                        let mut btn = egui::Button::new(
                            egui::RichText::new(label)
                                .size(shell.scale.base)
                                .color(control_text),
                        )
                        .min_size(egui::vec2(38.0, shell.hit));
                        if active {
                            btn = btn.fill(active_bg);
                        }
                        let resp = ui.add_enabled(project_open, btn);
                        if resp.clicked() {
                            // 二态切换：再点一次同一模式 → 关闭。
                            measure.mode = if active { MeasureMode::Off } else { target };
                            // 与笔刷互斥：开任一测量模式 → 关闭笔刷。
                            if measure.mode.is_active() {
                                brush.enabled = false;
                            }
                            // 切模式时丢弃草稿。
                            measure.clear_draft();
                        }
                        resp.on_hover_text(hint);
                    };
                    pick(MeasureMode::Line, "📏", "测量：折线（多段；右键/Esc 收笔）");
                    pick(MeasureMode::Rect, "▭", "测量：矩形（两点确定长×宽）");
                    pick(MeasureMode::Eraser, "🧹", "测量：橡皮（点选删除）");
                    ui.add_space(2.0);
                    ui.separator();
                    let zoom_in = ui.add_sized(
                        egui::vec2(38.0, shell.hit),
                        egui::Button::new(
                            egui::RichText::new("＋")
                                .size(shell.scale.base)
                                .color(control_text),
                        ),
                    );
                    if zoom_in.clicked() {
                        let next = view.scale / 1.6;
                        set_map_scale(&mut view, &mut cameras, next, zoom);
                    }
                    zoom_in.on_hover_text("放大");
                    let zoom_out = ui.add_sized(
                        egui::vec2(38.0, shell.hit),
                        egui::Button::new(
                            egui::RichText::new("－")
                                .size(shell.scale.base)
                                .color(control_text),
                        ),
                    );
                    if zoom_out.clicked() {
                        let next = view.scale * 1.6;
                        set_map_scale(&mut view, &mut cameras, next, zoom);
                    }
                    zoom_out.on_hover_text("缩小");
                    ui.separator();
                    let fit = ui.add_sized(
                        egui::vec2(38.0, shell.hit),
                        egui::Button::new(
                            egui::RichText::new("⛶")
                                .size(shell.scale.base)
                                .color(control_text),
                        ),
                    );
                    let fit = fit.on_hover_text("缩放到全图");
                    if fit.clicked() {
                        // 「全图」= **当前级**那张图的全幅（不是底层栅格的全幅）：
                        // 中心取该图的图帧原点 + 半幅，缩放取它的 fit。
                        // 图帧原点与 `grid.map_origin` 同源（`drill::map_origin` 是唯一
                        // 实现点），但这里要的是**当帧**的值，直接解析一次比读资源更稳。
                        let frame_origin = drill
                            .current()
                            .and_then(|id| wv_core::resolve_frame_in(&projection.world.maps, id))
                            .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
                            .unwrap_or(grid.map_origin);
                        view.camera_center = frame_origin + level_map_size * 0.5;
                        set_map_scale(&mut view, &mut cameras, zoom.fit, zoom);
                    }
                });
        });
}

/// 写入缩放并同步相机，避免 ViewState 与 Camera 变成两个真相。
///
/// 夹取区间来自**当前级**（`crate::zoom_level`，ADR-0009 §2 决策 ⑤）；
/// 级之间的越界换级不在这里做——那样 `＋/－` 按钮会意外换图。
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
    /// **当前级**区间内：连按到底应停在内界/外界，而不是穿过去（穿过去会让
    /// 城市视图缩到世界级的尺度，画面瞬间变成一块糊）。
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
