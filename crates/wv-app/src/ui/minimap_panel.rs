//! 小地图 + 图层开关（P-4）。
//!
//! ## 职责
//! - `panel_minimap` —— 168×168 地形缩略图 + 视口框（点击跳转）+ 图层开关行。
//!
//! ## 边界（不做什么）
//! - **不**画完整大地图 —— 见 [`crate::terrain_chunks`]。
//! - **不**改 Camera Transform 之外的渲染资源 —— `apply_view_to_camera` 是唯一真源写入。
//!
//! ## 主要导出
//! - `panel_minimap` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{GridConfig, Panel, PanelState, ViewState};
use crate::ui_layout::LayoutFile;
use crate::ui_theme::UiShellConfig;
use crate::ui_tokens::{radius, space};

use super::shell::UiShell;
use super::widgets::{float_window, rect_stroke};

/// 小地图 + 图层开关（P-4）。
#[allow(clippy::too_many_arguments)]
pub fn panel_minimap(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<crate::terrain_view::TerrainTexture>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let shell = UiShell::new(&shell_cfg);
    // 小地图缩略图直接复用大地图的地形纹理（同一 Handle，零额外上传）。
    // 注意先取 tex_id 再借 ctx（两者都要 &mut contexts，顺序反了过不了借用检查）。
    let tex_id = contexts.add_image(bevy_egui::EguiTextureHandle::Strong(tex.handle.clone()));
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
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
            rect_stroke(
                painter,
                rect,
                radius::CONTROL,
                egui::Stroke::new(1.0_f32, shell.border_strong()),
            );
            // 视口矩形。底图世界宽度 = 格宽 × 栅格列数；可见世界宽度 = 窗口宽 ×
            // view.scale（`view.scale` 的单位是世界单位/像素），两者相除即框占幅
            // 的比例。历史上写成 `1.0 / view.scale`，量纲反了：越深缩框越大，
            // 深缩时框占满整幅小地图，完全失去「我在哪」的指示作用。
            let map_world = (grid.cell_size * tex.size as f32).max(f32::EPSILON);
            let cx = 0.5 + ((view.camera_center.x / map_world) - 0.5).clamp(-0.5, 0.5);
            let cy = 0.5 + ((view.camera_center.y / map_world) - 0.5).clamp(-0.5, 0.5);
            let frac_x = ((view.window_size.x * view.scale) / map_world).clamp(0.05, 1.0);
            let frac_y = ((view.window_size.y * view.scale) / map_world).clamp(0.05, 1.0);
            let vp = egui::Rect::from_center_size(
                egui::pos2(
                    rect.left() + rect.width() * cx,
                    rect.top() + rect.height() * cy,
                ),
                egui::vec2(rect.width() * frac_x, rect.height() * frac_y),
            )
            .intersect(rect); // §C6：裁剪，不许画出边界
            rect_stroke(
                painter,
                vp,
                radius::BADGE,
                egui::Stroke::new(1.5_f32, shell.focus()),
            );
            if response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    let nx = (p.x - rect.left()) / rect.width() - 0.5;
                    let ny = (p.y - rect.top()) / rect.height() - 0.5;
                    view.camera_center =
                        Vec2::new(nx * grid.cell_size * 512.0, ny * grid.cell_size * 512.0);
                    // 相机 Transform 是主真源：只写 ViewState 相机不动，
                    // chunk 拣选却按新视口走 → 画面按假视口重裁（抖动重建）。
                    crate::camera::apply_view_to_camera(
                        view.camera_center,
                        view.scale,
                        &mut cameras,
                    );
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
                ui.add_space(space::SM);
                ui.vertical(|ui| {
                    // 底图淡化：把瓦片图集向纸面色拉，让路线/地标压得住底图。
                    ui.checkbox(&mut view.terrain_soften, "底图淡化");
                });
            });
        },
    );
}
