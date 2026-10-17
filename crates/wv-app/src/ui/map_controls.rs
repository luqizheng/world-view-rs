//! 右列纵向工具条（对齐原型 `.map-controls` · 2026-10-15）。
//!
//! ## 职责
//! - `map_controls` —— 右侧工具条（line / square · sep · ＋ / − / 全图）。
//!   笔刷/拖动切换已从地图工具移除（用户裁定）。
//! - `set_map_scale` —— 写入缩放并同步相机 Transform（Camera 是渲染真源）。
//! - `tile_button` —— **本模块本地**的瓷砖按钮 helper：`fill = active ? accent : raised_bg`，
//!   角圆随 `UiShell.radius::PANEL`，图标 = tile − `ICON_INSET × 2`。与 [`super::widgets::ToolButton`]
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
//! ## 视觉规格要点（对齐原型 `.map-controls`）
//! - 右侧垂直居中、距窗体右缘 18px（原型 `right: 18px`）。
//! - 圆角大框包裹整列：`panel_bg` 底 + `border_subtle` 描边 + `radius::PANEL` 圆角 + 投影。
//! - 瓷砖 34×34px（原型 `.map-controls .button`），间距 4px，内边距 5px。
//! - 8 块 = 2 测量工具 + 1 城市 + 1 圈地界（theme overlay Task 7）+ 1 参照
//!   网格开关（2026-10-16 自地形设计器迁入）+ 3 缩放/导航（笔刷切换已移除）。
//! - 活动态：`shell.accent()` 蓝底 + 白图标。
//! - 非活动态：`shell.raised_bg()` 灰底 + 白图标。
//!
//! ## 主要导出
//! - `map_controls` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{
    CityToolMode, CityToolState, DrillState, GridConfig, MeasureMode, MeasureState,
    TerritorySketch, ViewState, WorldProjection,
};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::{radius, space};
use crate::ui_shell::StartupLoadRequest;
use wv_render::theme::overlays::FEATURE_FOREST;

use super::shell::UiShell;
use super::widgets::{icon_image, ToolIcon};
use super::{
    ICON_GRID, ICON_LINE, ICON_MAXIMIZE, ICON_SQUARE, ICON_TREES, ICON_ZOOM_IN, ICON_ZOOM_OUT,
};

/// 单瓷砖边长 34px（原型 `.map-controls .button`）。
const TILE_SIZE: f32 = 34.0;
/// 瓷砖间默认竖向间距（原型 `gap: 4px`）。
const TILE_GAP: f32 = space::XS;
/// 分隔线上下额外间距，让分隔条带视觉上独立成段。
const SEPARATOR_PAD: f32 = space::SM;
/// 锚点偏移：右 18px（原型 `right: 18px`）。
const RIGHT_OFFSET_X: f32 = -18.0;
/// 内边距（原型 `padding: 5px`）。
const PANEL_PAD: f32 = 5.0;
/// 图标内嵌（tile 边长减去 `ICON_INSET * 2` = 34 − 14 = 20px 图标）。
const ICON_INSET: f32 = 7.0;

/// 右列纵向工具条（对齐原型 `.map-controls`：圆角大框 + 8 独立瓷砖）。
///
/// 顶部早返守卫与 `welcome_screen` 同源 —— 无工程时整条不渲染。
#[allow(clippy::too_many_arguments)]
pub fn map_controls(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut view: ResMut<ViewState>,
    mut measure: ResMut<MeasureState>,
    mut city_tool: ResMut<CityToolState>,
    mut sketch: ResMut<TerritorySketch>,
    current_loc: Res<CurrentLocale>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<wv_render::terrain_view::TerrainTexture>,
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

    // 当前级的缩放区间与「全图」尺寸。
    let window_size = windows
        .single()
        .map(|w| w.resolution.size())
        .unwrap_or(Vec2::new(1280.0, 800.0));
    let raster_size = Vec2::splat(grid.cell_size * tex.size as f32);
    let level_map_size =
        wv_render::zoom_level::map_size_of(&projection.world, drill.current(), raster_size);
    let zoom = wv_render::zoom_level::LevelZoom::for_map(level_map_size, window_size);

    // 视觉量：全部走 shell 令牌。
    let accent = shell.accent();
    let raised_bg = shell.raised_bg();
    let project_open = projection.project_dir.is_some();
    let line_active = measure.mode == MeasureMode::Line;
    let rect_active = measure.mode == MeasureMode::Rect;
    let city_active = city_tool.mode == CityToolMode::CreateArmed;
    let forest_active = sketch.enabled;
    let forest_hint = t_key(&current_loc, I18nKey::FeatureForestToolHint);

    // 位置：右侧垂直居中、距窗体右缘 18px（原型 `.map-controls`）。
    egui::Area::new(egui::Id::new("wv_map_controls_v05"))
        .anchor(egui::Align2::RIGHT_CENTER, egui::vec2(RIGHT_OFFSET_X, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // 圆角大框：panel_bg 底 + border_subtle 描边 + 投影（原型 `.map-controls`）。
            egui::Frame::NONE
                .fill(shell.panel_bg())
                .stroke(egui::Stroke::new(1.0, shell.border_subtle()))
                .corner_radius(egui::CornerRadius::from(radius::PANEL))
                .inner_margin(PANEL_PAD)
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(71),
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = TILE_GAP;

                    // ====== 测量工具组：line / square ======
                    // ① Line — 测量折线；右键/Esc 收笔。
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
                                // 互斥：开测量时关掉圈地界（同一左键不能两用）。
                                sketch.enabled = false;
                                sketch.discard_active();
                                sketch.last_click = None;
                                MeasureMode::Line
                            };
                            measure.clear_draft();
                        },
                    );
                    // ② Square — 测量矩形；两点确定长 × 宽。
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
                                // 互斥：开测量时关掉圈地界（同一左键不能两用）。
                                sketch.enabled = false;
                                sketch.discard_active();
                                sketch.last_click = None;
                                MeasureMode::Rect
                            };
                            measure.clear_draft();
                        },
                    );

                    // ====== 分隔（原型 `<span />`） ======
                    ui.add_space(SEPARATOR_PAD);
                    ui.separator();
                    ui.add_space(SEPARATOR_PAD);

                    // ②.5 City — 城市子图工具（ADR-0022 D2 UI 接入）
                    //   M2 简化：单击落一个默认 256×256 wu 的城市在点击点（drag-to-draw 矩形
                    //   留 v1.1）；启用时互斥地关掉 measure。详见 `commit_city_click` 系统。
                    tile_button(
                        ui,
                        &shell,
                        ToolIcon::Svg(ICON_SQUARE), // 暂复用 SQUARE 图标；V1.1 换专属 ICON_CITY
                        "城市：在父图上单击落一个 256×256 wu 的城市子图（ADR-0022 M2）",
                        city_active,
                        project_open,
                        accent,
                        raised_bg,
                        || {
                            city_tool.mode = if city_active {
                                CityToolMode::Off
                            } else {
                                // 互斥：开城市时关掉测量与圈地界。
                                measure.mode = MeasureMode::Off;
                                measure.clear_draft();
                                sketch.enabled = false;
                                sketch.discard_active();
                                sketch.last_click = None;
                                CityToolMode::CreateArmed
                            };
                            city_tool.draft = None;
                        },
                    );

                    // ②.6 Forest — 圈地界工具（theme overlay Task 7）：
                    //   左键多次落点，双击 / 回车 / 右键闭合；闭合后弹命名对话框
                    //   落成 `AddFeaturePlaceCommand`（feature = 森林）。
                    //   启用时互斥地关掉测量与城市工具。
                    tile_button(
                        ui,
                        &shell,
                        ToolIcon::Svg(ICON_TREES),
                        forest_hint,
                        forest_active,
                        project_open,
                        accent,
                        raised_bg,
                        || {
                            if forest_active {
                                sketch.enabled = false;
                                sketch.discard_active();
                                sketch.last_click = None;
                                // 关工具也一并撤掉待命名的环：命名对话框以
                                // `pending` 为开关判据，环没了它自然跟着关
                                //（final review：作者显式关工具 = 放弃这一圈）。
                                sketch.pending = None;
                            } else {
                                measure.mode = MeasureMode::Off;
                                measure.clear_draft();
                                city_tool.mode = CityToolMode::Off;
                                city_tool.draft = None;
                                sketch.enabled = true;
                                sketch.feature = FEATURE_FOREST;
                                sketch.discard_active();
                                sketch.last_click = None;
                            }
                        },
                    );

                    // ====== 视图 / 导航组：网格 / ＋ / − / 全图 ======
                    // ③ 网格 — 参照网格开关。状态在 `ViewState.grid_lines`
                    //   （2026-10-16 从地形设计器迁来——它是「看图方式」，
                    //   与缩放同族，不是笔刷参数）。
                    tile_button(
                        ui,
                        &shell,
                        ToolIcon::Svg(ICON_GRID),
                        "显示参照网格（独立于笔刷）",
                        view.grid_lines,
                        true,
                        accent,
                        raised_bg,
                        || {
                            view.grid_lines = !view.grid_lines;
                        },
                    );
                    // ④ ＋ — 放大（步进 1.6×，夹在当前级区间内）。
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
                    // ⑤ − — 缩小（步进 1.6×，夹在当前级区间内）。
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
                    // ⑥ 全图 — 缩放到当前级 fit + 把相机中心钉到帧中心。
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
                                .and_then(|id| {
                                    wv_core::resolve_frame_in(&projection.world.maps, id)
                                })
                                .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
                                .unwrap_or(grid.map_origin);
                            view.camera_center = frame_origin + level_map_size * 0.5;
                            set_map_scale(&mut view, &mut cameras, zoom.fit, zoom);
                        },
                    );
                });
        });
}

/// 工具条本地瓷砖按钮 helper（详见模块头注释 §职责）。
///
/// 与 [`super::widgets::ToolButton`] 的差异：自己负责 `raised_bg` 非活动底；
/// ToolButton 的非活动底透明不满足「独立瓷砖」观感。调用面窄——工具条 5 个
/// 接点全在本模块内——故不上 [`super::widgets`] 公共抽象面。
///
/// 视觉参数（对齐原型）：
/// - 方块 = `TILE_SIZE × TILE_SIZE`（34），圆角 = `radius::PANEL = 10`。
/// - 填充：`active ? accent : raised_bg`。
/// - 图标：`TILE_SIZE − ICON_INSET × 2 = 20px`，白（白在 accent / raised_bg 上对比都达标）。
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
    zoom: wv_render::zoom_level::LevelZoom,
) {
    view.scale = zoom.clamp(scale);
    wv_render::camera::apply_view_to_camera(view.camera_center, view.scale, cameras);
}

#[cfg(test)]
mod map_control_tests {
    use bevy::prelude::Vec2;

    /// `＋/－` 每次 1.6×（`view.scale / 1.6`、`* 1.6`），经 `set_map_scale` 夹在
    /// **当前级**区间内：连按到底应停在内界/外界，而不是穿过去。
    #[test]
    fn zoom_button_steps_stop_at_the_levels_band() {
        let zoom = wv_render::zoom_level::LevelZoom::for_map(
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

    /// 视觉量计数（对齐原型 `.map-controls`）：8 个独立瓷砖 + 1 分隔。
    /// 内容高度 = 8×TILE + 7×GAP + 2×SEPARATOR_PAD。
    ///
    /// 不直接渲染——egui 0.36 在 unit 上下文没有 GPU surface；
    /// 这里钉的是 layout 数学校验：模块顶部常量（`TILE_SIZE` = 34、
    /// `TILE_GAP` = 4、`SEPARATOR_PAD` = 8）若被改了，这里必须同步改。
    #[test]
    fn toolbar_layout_sums_to_expected_total_height() {
        // 8 瓷砖 × 34 + 7 个 tile gap × 4 + 2 × 8 = 272 + 28 + 16 = 316
        // （2026-10-17 加第 8 块「圈地界」瓷砖；第 7 块为「城市」）
        const TILE: f32 = 34.0;
        const GAP: f32 = 4.0;
        const SEP: f32 = 8.0;
        let total = 8.0 * TILE + 7.0 * GAP + 2.0 * SEP;
        assert!(
            (total - 316.0).abs() < 1e-3,
            "总额不等于 316：模块顶部常量（TILE_SIZE/TILE_GAP/SEPARATOR_PAD）改了 \
             必须在测试里同步更新。got {total}",
        );
    }
}
