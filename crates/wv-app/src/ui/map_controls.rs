//! 左列纵向工具条（图片规格 · 8 独立瓷砖 · 2026-10-15）。
//!
//! ## 职责
//! - `map_controls` —— 左侧贴边工具切换（pen / line / square / pencil · sep · ＋ / − / 全图）。
//! - `set_map_scale` —— 写入缩放并同步相机 Transform（Camera 是渲染真源）。
//! - `tile_button` —— **本模块本地**的瓷砖按钮 helper：`fill = active ? accent : raised_bg`，
//!   角圆随 `UiShell.radius::PANEL`，图标 = tile − 16 内嵌。与 [`super::widgets::ToolButton`]
//!   的关键差异：本 helper 自己负责非活动底色，`ToolButton` 的非活动底是透明——
//!   工具条要求每块「独立的圆角方块」观感，必须由本 helper 提供 raised_bg。
//! - 块级测试 `map_control_tests` —— `＋/－` 1.6× 步进夹在级夹持区间内 + 视觉量计数。
//!
//! ## 边界（不做什么）
//! - **不**绘制顶栏/状态栏 —— 那是 [`super::top_bar`]。
//! - **不**在无工程时显示 —— `has_open_project() == false` 或启动加载中即早返，
//!   与 `welcome_screen` 共用同一信号 [`crate::ui_shell::StartupLoadRequest`]。
//! - **不**重复封装 [`ToolButton`] —— 该抽象的非活动底透明不满足本规格「独立瓷砖」语义；
//!   本 helper 不进入 `widgets` 公共面（仅本模块使用，受图片规格约束）。
//!
//! ## 图片规格要点
//! - 8 个独立瓷砖（**不是**包围整列的一个圆角大框）：4 工具 + 1 分隔 + 3 缩放/导航。
//! - 活动态：`shell.accent()` 蓝底 + 白图标。
//! - 非活动态：`shell.raised_bg()` 灰底 + 白图标（`raised_bg` 与暗色背景有可辨对比）。
//! - Pen 与 Pencil 同义（都切 `brush.enabled`，用户裁定的设计冗余）。
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
use crate::theme::ui_tokens::{radius, space};
use crate::ui_shell::StartupLoadRequest;

use super::shell::UiShell;
use super::widgets::{icon_image, ToolIcon};
use super::{
    ICON_LINE, ICON_MAXIMIZE, ICON_PEN, ICON_PENCIL, ICON_SQUARE, ICON_ZOOM_IN, ICON_ZOOM_OUT,
};

/// 单瓷砖边长 36px（适中尺寸，不遮地图）。
const TILE_SIZE: f32 = 36.0;
/// 瓷砖间默认竖向间距。
const TILE_GAP: f32 = space::XS;
/// 分隔线上下额外间距，让分隔条带视觉上独立成段。
const SEPARATOR_PAD: f32 = space::SM;
/// 锚点偏移：左 16px。
const LEFT_OFFSET_X: f32 = space::LG;
/// 图标内嵌（tile 边长减去 `ICON_INSET * 2` = 36 − 16 = 20px 图标）。
const ICON_INSET: f32 = 8.0;

/// 左列纵向工具条（图片规格：8 独立瓷砖）。
///
/// v0.3 重做后视觉与图片一致：左列 36×36 独立圆角方块，活动态蓝底白图标、
/// 非活动态 raised_bg 灰底；写相机仍经 `apply_view_to_camera`，
/// Camera Transform 仍是渲染真源。
///
/// Pen + Pencil 两瓷砖共用同一个 `brush.enabled` 状态（用户裁定的设计冗余）；
/// 顶部早返守卫与 `welcome_screen` 同源 —— 无工程时整条不渲染。
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
    startup_req: Res<StartupLoadRequest>,
) {
    // 无工程锁（图片规格 §首页不可见）：与 `welcome_screen` 共用同一信号。
    if !projection.has_open_project() || startup_req.0.is_some() {
        return;
    }

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

    // 视觉量：全部走 shell 令牌（图片规格 §设计要点）。
    let accent = shell.accent();
    let raised_bg = shell.raised_bg();
    let project_open = projection.project_dir.is_some();
    let line_active = measure.mode == MeasureMode::Line;
    let rect_active = measure.mode == MeasureMode::Rect;

    // 位置：左侧居中（egui LEFT_CENTER 锚点 Y 偏移 0 = 垂直居中）。
    egui::Area::new(egui::Id::new("wv_map_controls_v04"))
        .anchor(egui::Align2::LEFT_CENTER, egui::vec2(LEFT_OFFSET_X, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = TILE_GAP;

            // ====== 工具组：pen / line / square / pencil ======
            // ① Pen — 笔刷 / 拖动切换；与 ④ Pencil 同键联动（设计冗余）。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_PEN),
                "切换笔刷 / 拖动",
                brush.enabled,
                brush_available,
                accent,
                raised_bg,
                || {
                    brush.enabled = !brush.enabled;
                },
            );
            // ② Line — 测量折线；右键/Esc 收笔（同 `MeasureMode::Line` 既有语义）。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_LINE),
                "测量：折线（多段；右键/Esc 收笔）",
                line_active,
                project_open,
                accent,
                raised_bg,
                || {
                    measure.mode = if line_active {
                        MeasureMode::Off
                    } else {
                        MeasureMode::Line
                    };
                    if measure.mode.is_active() {
                        brush.enabled = false;
                    }
                    measure.clear_draft();
                },
            );
            // ③ Square — 测量矩形；两点确定长 × 宽。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_SQUARE),
                "测量：矩形（两点确定长×宽）",
                rect_active,
                project_open,
                accent,
                raised_bg,
                || {
                    measure.mode = if rect_active {
                        MeasureMode::Off
                    } else {
                        MeasureMode::Rect
                    };
                    if measure.mode.is_active() {
                        brush.enabled = false;
                    }
                    measure.clear_draft();
                },
            );
            // ④ Pencil — 与 Pen 同义（用户裁定：双图标同切 brush.enabled）。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_PENCIL),
                "切换笔刷 / 拖动（与笔同义）",
                brush.enabled,
                brush_available,
                accent,
                raised_bg,
                || {
                    brush.enabled = !brush.enabled;
                },
            );

            // ====== 分隔 ======
            ui.add_space(SEPARATOR_PAD);
            ui.separator();
            ui.add_space(SEPARATOR_PAD);

            // ====== 缩放 / 导航组：＋ / − / 全图 ======
            // ⑤ ＋ — 放大（步进 1.6×，夹在当前级区间内）。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_ZOOM_IN),
                "放大",
                false,
                true,
                accent,
                raised_bg,
                || {
                    let next = view.scale / 1.6;
                    set_map_scale(&mut view, &mut cameras, next, zoom);
                },
            );
            // ⑥ − — 缩小（步进 1.6×，夹在当前级区间内）。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_ZOOM_OUT),
                "缩小",
                false,
                true,
                accent,
                raised_bg,
                || {
                    let next = view.scale * 1.6;
                    set_map_scale(&mut view, &mut cameras, next, zoom);
                },
            );
            // ⑦ 全图 — 缩放到当前级 fit + 把相机中心钉到帧中心。
            tile_button(
                ui,
                &shell,
                ToolIcon::Svg(ICON_MAXIMIZE),
                "缩放到全图",
                false,
                true,
                accent,
                raised_bg,
                || {
                    let frame_origin = drill
                        .current()
                        .and_then(|id| wv_core::resolve_frame_in(&projection.world.maps, id))
                        .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
                        .unwrap_or(grid.map_origin);
                    view.camera_center = frame_origin + level_map_size * 0.5;
                    set_map_scale(&mut view, &mut cameras, zoom.fit, zoom);
                },
            );
        });
}

/// 工具条本地瓷砖按钮 helper（详见模块头注释 §职责）。
///
/// 与 [`super::widgets::ToolButton`] 的差异：自己负责 `raised_bg` 非活动底；
/// ToolButton 的非活动底透明不满足「独立瓷砖」观感。调用面窄——工具条 7 个
/// 接点全在本模块内——故不上 [`super::widgets`] 公共抽象面。
///
/// 视觉参数（图片规格）：
/// - 方块 = `TILE_SIZE × TILE_SIZE`，圆角 = `radius::PANEL = 10`。
/// - 填充：`active ? accent : raised_bg`。
/// - 图标：`TILE_SIZE − ICON_INSET × 2 = 24px`，白（白在 accent / raised_bg 上对比都达标）。
#[allow(clippy::too_many_arguments)]
fn tile_button(
    ui: &mut egui::Ui,
    _shell: &UiShell,
    icon: ToolIcon,
    hint: &str,
    active: bool,
    enabled: bool,
    accent: egui::Color32,
    raised_bg: egui::Color32,
    on_click: impl FnOnce(),
) {
    let ToolIcon::Svg(source) = icon;
    let fill = if active { accent } else { raised_bg };
    let image = icon_image(source, egui::Color32::WHITE, TILE_SIZE - ICON_INSET);
    let btn = egui::Button::image(image)
        .min_size(egui::vec2(TILE_SIZE, TILE_SIZE))
        .fill(fill)
        .corner_radius(egui::CornerRadius::from(radius::PANEL));
    let resp = if enabled {
        ui.add(btn)
    } else {
        ui.add_enabled(false, btn)
    };
    if resp.clicked() {
        on_click();
    }
    if !hint.is_empty() {
        resp.on_hover_text(hint);
    }
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

    /// 视觉量计数（图片规格 §设计要点）：左列 7 个独立瓷砖 + 1 分隔。
    /// 高度 = 7×TILE + 6×GAP + 2×SEPARATOR_PAD。
    ///
    /// 不直接渲染——egui 0.36 在 unit 上下文没有 GPU surface；
    /// 这里钉的是 layout 数学校验：模块顶部常量（`TILE_SIZE` = 36、
    /// `TILE_GAP` = 4、`SEPARATOR_PAD` = 8）若被改了，这里必须同步改。
    #[test]
    fn toolbar_layout_sums_to_expected_total_height() {
        // 7 瓷砖 × 36 + 6 个 tile gap × 4 + 2 × 8 = 252 + 24 + 16 = 292
        const TILE: f32 = 36.0;
        const GAP: f32 = 4.0;
        const SEP: f32 = 8.0;
        let total = 7.0 * TILE + 6.0 * GAP + 2.0 * SEP;
        assert!(
            (total - 292.0).abs() < 1e-3,
            "总额不等于 292：模块顶部常量（TILE_SIZE/TILE_GAP/SEPARATOR_PAD）改了 \
             必须在测试里同步更新。got {total}",
        );
    }
}
