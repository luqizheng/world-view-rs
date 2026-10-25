//! 小地图（Figma `3:20823`）。
//!
//! ## 职责
//! - `panel_minimap` —— 当前地形纹理的宽幅缩略 overview + 视口框 +
//!   footer「内联缩放（− / 百分比 / ＋）+ 适配当前图」（Figma `3:20823`
//!   基础上按 2026-10-25 用户三轮裁定收敛）。
//! - 视口框三种交互：**拖拽** = 平移相机、**点击** = 跳转、**悬停滚轮** =
//!   以指针为锚缩放（每格 ≈×1.12，与主画布滚轮同量级）。
//!
//! ## 边界（不做什么）
//! - **不**画完整大地图 —— 见 [`wv_world_cell_map`] 的 chunk 层。
//! - **不**改 Camera Transform 之外的渲染资源 —— `apply_view_to_camera` 是唯一真源写入。
//! - 图层开关不在本面板 —— 那是图层面板（`panel_layers`）的职责，
//!   Figma `3:20823` 的小地图只有 overview + footer。缩放控件
//!   （Figma `3:21882` 收窄形态：− / 百分比 / ＋）按用户裁定内联在
//!   footer 图名旁；地图边缘的独立 zoom 浮条已随之退役。
//!
//! ## 主要导出
//! - `panel_minimap` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::CurrentLocale;
use crate::state::{DrillState, GridConfig, Panel, PanelState, ViewState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{float_window, rect_stroke};

/// 小地图（Figma `3:20823`）。
#[allow(clippy::too_many_arguments)]
pub fn panel_minimap(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
    grid: Res<GridConfig>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    tex: Res<wv_render::terrain_view::TerrainTexture>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    current_loc: Res<CurrentLocale>,
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
        current_loc.0,
        |ui| {
            // 底图世界尺寸：格宽 × 栅格行/列数。世界原点 = 图左上角，
            // 跨度 [0, w]×[0, h]——与 marker / fit 的 extent 口径一致。
            let map_world = Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size;
            let content_w = ui.available_width().max(64.0);
            let img_h = overview_height(content_w, map_world);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(content_w, img_h), egui::Sense::click_and_drag());
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

            // 世界 ↔ 归一化。**栅格原点必须先减掉**：overview 画的是地形栅格
            // （`TerrainTexture`），栅格在视图空间的位置是 `grid.raster_origin`
            // 而不是零——两套原点混用会让视口框整体错位、被钳到边上看不见
            // （2026-10-25 实机教训，参见 AGENTS §两套原点陷阱）。
            let to_norm = |p: Vec2| (p - grid.raster_origin) / map_world;
            let to_world = |n: Vec2| {
                grid.raster_origin + Vec2::new(n.x.clamp(0.0, 1.0), n.y.clamp(0.0, 1.0)) * map_world
            };
            let norm_of = |p: egui::Pos2| {
                Vec2::new(
                    (p.x - rect.left()) / rect.width(),
                    (p.y - rect.top()) / rect.height(),
                )
            };
            let pos_of = |n: Vec2| {
                egui::pos2(
                    rect.left() + n.x * rect.width(),
                    rect.top() + n.y * rect.height(),
                )
            };

            // 视口矩形（「镜头在哪」）：中心 = 相机中心的归一化位置；
            // 占幅 = 可见世界宽 ÷ 图宽（归一化占比 × overview 实际宽高 =
            // 像素尺寸）。半透明金底 + 2px 金描边——纯描边在花纹理上会淹没。
            // （历史两版各错一刀：`1.0 / view.scale` 量纲反了；归一化占比
            // 忘了乘 overview 尺寸，框恒为 1×1 像素——2026-10-25 实机读数定案。）
            let frac = (Vec2::new(view.window_size.x, view.window_size.y) * view.scale / map_world)
                .clamp(Vec2::splat(0.05), Vec2::ONE);
            let vp = viewport_pixel_rect(rect, to_norm(view.camera_center), frac);
            painter.rect_filled(vp, radius::BADGE, shell.focus().gamma_multiply(0.15));
            rect_stroke(
                painter,
                vp,
                radius::BADGE,
                egui::Stroke::new(2.0_f32, shell.focus()),
            );

            // 当前图（钻取所在级）的缩放区间——与地图右缘工具条同口径。
            let window_size = ctx.input(|i| i.content_rect()).size();
            let level_map_size =
                wv_render::zoom_level::map_size_of(&projection.world, drill.current(), map_world);
            let zoom_band = wv_render::zoom_level::LevelZoom::for_map(
                level_map_size,
                Vec2::new(window_size.x, window_size.y),
            );

            // 拖拽 = 把视口框拖到指针下（与点击跳转同一映射，连续版）。
            // egui 的 click 自带拖拽阈值，两者不会在同一手势里都触发。
            let pointer = response
                .interact_pointer_pos()
                .filter(|_| response.dragged() || response.clicked());
            if let Some(p) = pointer {
                let s = view.scale;
                apply_view(&mut view, &mut cameras, to_world(norm_of(p)), s);
            }

            // 悬停滚轮 = 以指针为锚缩放：锚点世界点在屏幕上不动。
            // 系数 0.002/点 × egui 滚轮一格 ≈ 58 点 → ×0.89，与主画布
            // 滚轮一格 ×1/1.12 同量级（camera_input 的 0.12 步长）。
            let scroll = if response.hovered() {
                ui.input(|i| i.smooth_scroll_delta.y)
            } else {
                0.0
            };
            if scroll != 0.0 && view.scale > 0.0 {
                let anchor = to_world(norm_of(pointer.unwrap_or_else(|| ui.max_rect().center())));
                let next = zoom_band.clamp(view.scale * (-scroll * 0.002).exp());
                if (next - view.scale).abs() > f32::EPSILON {
                    let ratio = next / view.scale;
                    let center = zoom_center_toward(anchor, view.camera_center, ratio);
                    apply_view(&mut view, &mut cameras, center, next);
                }
            }

            // ── Footer（2026-10-25 用户四轮裁定）：一组 = 内联缩放
            // （− / 百分比 / ＋ / 适配，无把手 / 无外圈；适配 = 组件自带的
            // 14px 图标钮，与 − ＋ 同规格同中线）。地图边缘的独立 zoom
            // 浮条同日退役。
            ui.add_space(space::SM);
            ui.horizontal(|ui| {
                // 内联缩放控件（Figma `3:21882` 组件收窄形态）：
                // 100% = scale 1.0（1 世界单位/像素），读数与主画布同源。
                let can_zoom_in = !zoom_band.at_inner_limit(view.scale);
                let can_zoom_out = !zoom_band.at_outer_limit(view.scale);
                // 闭包持三处冲突的 `&mut`，与 `map_controls` 同款处理：三个 bool。
                let mut zoom_in_clicked = false;
                let mut zoom_out_clicked = false;
                let mut fit_clicked = false;
                let mut strip_state = super::widgets::ZoomControlsState {
                    show_grip: false,
                    // footer 行内联：无外圈、行高与 − ＋ 同中线
                    // （2026-10-25 用户裁定「不要外圈」）。
                    show_frame: false,
                    ..Default::default()
                };
                super::widgets::zoom_controls(
                    ui,
                    &shell,
                    &mut strip_state,
                    view.scale,
                    can_zoom_in,
                    can_zoom_out,
                    || zoom_in_clicked = true,
                    || zoom_out_clicked = true,
                    || fit_clicked = true,
                );
                // 单击步长 ×1.6，与地图工具条历史口径一致。
                if zoom_in_clicked || zoom_out_clicked {
                    let center = view.camera_center;
                    let next = if zoom_in_clicked {
                        zoom_band.clamp(view.scale / 1.6)
                    } else {
                        zoom_band.clamp(view.scale * 1.6)
                    };
                    apply_view(&mut view, &mut cameras, center, next);
                } else if fit_clicked {
                    // 适配当前图：中心取**栅格**中心——overview 画的就是
                    // 栅格，适配后视口框应正好铺满全图。
                    apply_view(
                        &mut view,
                        &mut cameras,
                        grid.raster_origin + map_world * 0.5,
                        zoom_band.fit,
                    );
                }
            });
        },
    );
}

/// 把相机（中心 + 缩放）写回视图态与相机 Transform。
///
/// 视图态与 Transform 必须同帧同步：只写 ViewState 不动相机，chunk 拣选
/// 会按新视口走、画面却按旧相机裁（抖动重建）。
fn apply_view(
    view: &mut ViewState,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    center: Vec2,
    scale: f32,
) {
    view.camera_center = center;
    view.scale = scale;
    wv_render::camera::apply_view_to_camera(center, scale, cameras);
}

/// overview 高度：宽随面板内容宽，高按地图纵横比推导后夹进 `[90, 240]`。
///
/// Figma `3:20823` 的 264×108 只是占位——真实地图多为 16:9，等比放进
/// 264 宽应是 148；夹取是为了极端纵横比（细长条图）不把面板撑爆。
/// 图尺寸退化（0 / NaN）时回退设计稿的 108。
fn overview_height(content_w: f32, map_world: Vec2) -> f32 {
    if content_w <= 0.0 || !map_world.is_finite() || map_world.x <= 0.0 || map_world.y <= 0.0 {
        return 108.0;
    }
    (content_w * map_world.y / map_world.x).clamp(90.0, 240.0)
}

/// 以 `world_under` 为锚的缩放后相机中心：锚点在屏幕上的位置不动。
///
/// 推导：屏幕偏移 =（世界点 − 相机中心）/ scale；缩放后要求同一偏移，
/// 解得 `center' = anchor − (anchor − center) × (scale'/scale)`。
/// `ratio < 1`（放大）时中心向锚点收拢，`ratio > 1`（缩小）时远离。
fn zoom_center_toward(world_under: Vec2, center: Vec2, ratio: f32) -> Vec2 {
    world_under - (world_under - center) * ratio
}

/// 视口框的像素矩形：归一化中心 + 归一化占幅 × overview 实际宽高，
/// 最后裁进 overview 矩形（视野超出全图时框贴满整幅）。
fn viewport_pixel_rect(overview: egui::Rect, center_norm: Vec2, frac: Vec2) -> egui::Rect {
    let center = egui::pos2(
        overview.left() + center_norm.x.clamp(0.0, 1.0) * overview.width(),
        overview.top() + center_norm.y.clamp(0.0, 1.0) * overview.height(),
    );
    egui::Rect::from_center_size(
        center,
        egui::vec2(frac.x * overview.width(), frac.y * overview.height()),
    )
    .intersect(overview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overview_height_follows_map_aspect() {
        // 16:9 地图放进 264 宽 = 148.5（Figma 264×108 是占位，真实地图等比）。
        let h = overview_height(264.0, Vec2::new(2048.0, 1152.0));
        assert!((h - 148.5).abs() < 1e-3, "got {h}");
    }

    #[test]
    fn overview_height_clamps_extreme_aspect_ratios() {
        // 细长条图不许把面板撑爆：高夹在 [90, 240]。
        assert_eq!(overview_height(264.0, Vec2::new(4096.0, 256.0)), 90.0);
        assert_eq!(overview_height(264.0, Vec2::new(256.0, 4096.0)), 240.0);
    }

    #[test]
    fn overview_height_degenerate_map_falls_back_to_figma_placeholder() {
        assert_eq!(overview_height(264.0, Vec2::ZERO), 108.0);
        assert_eq!(overview_height(0.0, Vec2::new(100.0, 100.0)), 108.0);
    }

    #[test]
    fn zoom_center_toward_keeps_anchor_screen_offset_proportional() {
        // 放大一半（ratio 0.5）：锚点 (900,500) 相对画面中心的屏幕偏移
        // 应从 50px 收拢为 25px（scale 2→1，世界/像素减半）。
        let center = zoom_center_toward(Vec2::new(900.0, 500.0), Vec2::new(800.0, 400.0), 0.5);
        assert_eq!(center, Vec2::new(850.0, 450.0), "got {center:?}");
    }

    #[test]
    fn viewport_rect_scales_with_normalized_fraction() {
        // 回归（2026-10-25）：归一化占比曾被直接当像素尺寸，框恒为 1×1 px。
        // 262×240 的 overview、中心 (0.5,0.5)、占幅 (0.5,0.25)
        // → 131×60 的框，中心对齐。
        let overview =
            egui::Rect::from_min_size(egui::pos2(345.0, 442.0), egui::vec2(262.0, 240.0));
        let vp = viewport_pixel_rect(overview, Vec2::new(0.5, 0.5), Vec2::new(0.5, 0.25));
        assert!((vp.width() - 131.0).abs() < 1e-3, "w={}", vp.width());
        assert!((vp.height() - 60.0).abs() < 1e-3, "h={}", vp.height());
        assert!((vp.center().x - 476.0).abs() < 1e-3, "cx={}", vp.center().x);
    }

    #[test]
    fn viewport_rect_covers_whole_overview_when_view_exceeds_map() {
        // 视野 ≥ 全图（frac 钳到 1）：框 = 整幅 overview。
        let overview = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        let vp = viewport_pixel_rect(overview, Vec2::new(0.5, 0.5), Vec2::ONE);
        assert!((vp.width() - 200.0).abs() < 1e-3, "w={}", vp.width());
        assert!((vp.height() - 100.0).abs() < 1e-3, "h={}", vp.height());
    }

    #[test]
    fn viewport_rect_clamps_center_into_overview() {
        // 相机中心落在图外（钻取子图等）：框钳到边上且整体裁进 overview，
        // 不许飞出去。中心 (2,-1) → 钳到右上角 (200,0)，20×10 的框裁成
        // 右上 10×5 的一角。
        let overview = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        let vp = viewport_pixel_rect(overview, Vec2::new(2.0, -1.0), Vec2::new(0.1, 0.1));
        assert!((vp.right() - 200.0).abs() < 1e-3, "right={}", vp.right());
        assert!((vp.top() - 0.0).abs() < 1e-3, "top={}", vp.top());
        assert!((vp.width() - 10.0).abs() < 1e-3, "w={}", vp.width());
        assert!((vp.height() - 5.0).abs() < 1e-3, "h={}", vp.height());
    }

    #[test]
    fn zoom_center_toward_zoom_out_moves_center_away_from_anchor() {
        let center = zoom_center_toward(Vec2::new(900.0, 500.0), Vec2::new(800.0, 400.0), 2.0);
        assert_eq!(center, Vec2::new(700.0, 300.0), "got {center:?}");
    }

    #[test]
    fn map_center_world_coord_reads_as_half_of_extent() {
        // 回归（2026-10-25）：旧实现按「原点居中」口径写跳转坐标，
        // 点小地图中心会跳到图左上角。世界原点在图左上角、跨度 [0,w]×[0,h]，
        // 图心 (1024, 576) 必须读出归一化 (0.5, 0.5)。
        let map_world = Vec2::new(2048.0, 1152.0);
        let n = Vec2::new(1024.0 / map_world.x, 576.0 / map_world.y);
        assert!(
            (n.x - 0.5).abs() < 1e-4 && (n.y - 0.5).abs() < 1e-4,
            "got {n:?}"
        );
    }
}
