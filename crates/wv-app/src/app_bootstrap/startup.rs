//! `Startup` 阶段的世界兜底：正交相机与地形纹理。

use bevy::prelude::*;

use crate::state::{DrillState, ViewState, WorldProjection};
use wv_render::cellmap_bridge;
use wv_render::terrain_view;

/// 2D 正交相机。若启动前已装配好 `ViewState`（fit 缩放 + 根图中心），
/// 直接把它落到 Camera 的 transform / projection——**Camera 的 Transform 才是
/// 相机主真源**，`ViewState.camera_center` 只是 pan/zoom 回写的镜像，
/// 光写 ViewState 不动 transform 相机就还停在原点。
pub(super) fn setup_camera(mut commands: Commands, view: Option<Res<ViewState>>) {
    let (center, scale) = view
        .as_deref()
        .map(|v| (v.camera_center, v.scale))
        .unwrap_or((Vec2::ZERO, 1.0));
    commands.spawn((
        Camera2d,
        Transform::from_xyz(center.x, center.y, 0.0),
        // 方案 C（矢量覆盖层抗锯齿）：4x MSAA。POI 圆环 / 道路条 / 地名底框
        // 都是 Mesh2d，多边形边缘靠 MSAA 平滑——纹理内部的地形边界不受益
        // （那由 terrain_view 的边界扰动负责），两者互补。
        Msaa::Sample4,
        // Bevy 0.16 起 `Camera2d` require 的是 `Projection` **枚举**组件
        // （`OrthographicProjection` 自己不再是 Component）：要覆盖默认 scale=1.0
        // 必须包一层 `Projection::Orthographic(..)`。改动出处见 ADR-0011。
        Projection::Orthographic(OrthographicProjection {
            scale,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// 地形纹理的初始化必须在 `WorldProjection` 就位之后。
///
/// 保证地形数据存在：若**当前图及其祖先**都没带栅格，用占位地形填进根图，
/// 这样笔刷编辑与纹理重建都有统一的数据源（Phase 2：地形按图存，见
/// [`terrain_view::active_terrain`]）。
pub(super) fn setup_terrain_texture_after_world(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
) {
    // 瓦片图集（README §12-14）：加载失败静默回退纯色平涂。
    let atlas = terrain_view::ensure_atlas(&mut commands);
    // 原型默认 512×512 占位（基线 §2.19：1024² 亦轻松 60 FPS）。
    let (map, data) =
        terrain_view::active_terrain_or_placeholder(&mut projection.world, drill.current(), 512);
    // ── 跨度接线（ADR-0092，取代旧米/格接线）：本图高度实际跨度现场算，
    // 晕渲观感与格距/比例尺脱钩。本函数只负责启动首帧，此后每次重建都走
    // `flush_terrain_texture` 的同款接线。
    let image = terrain_view::image_from_terrain_with_scale(
        &data,
        cellmap_bridge::shade_mode_of(view.mode),
        atlas.as_ref(),
        view.terrain_soften,
        &cellmap_bridge::conv_pack_with(view.active_theme, view.hypsometric_tint, view.mode),
        0.0,
        terrain_view::bake_height_span(&data),
    );
    commands.insert_resource(terrain_view::new_terrain_texture(
        &mut images,
        image,
        &data,
        map,
        cellmap_bridge::shade_mode_of(view.mode),
        view.terrain_soften,
        view.active_theme.id,
        view.hypsometric_tint,
    ));
}
