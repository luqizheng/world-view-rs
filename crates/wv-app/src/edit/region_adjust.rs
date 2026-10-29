//! 区域边界拖拽调整（2026-10-22 验收反馈 · CellMap 专业数据编辑器）。
//!
//! ## 交互（用户裁定）
//!
//! - 选中区域（点标签 / 区域面板行）⇒ CellMap 里出**方格描边**（沿格边的
//!   阶梯描边）+ 边界**锚点手柄**。
//! - **锚点数量随缩放自适应**（用户裁定）：抽稀间距 = `view.scale × 24px`
//!   的世界长度——zoom out 锚点少（粗调）、zoom in 锚点多（细调）。
//! - 拖锚点 = 调整区域；**方格为锚点**——锚点吸附到格点，按整格步进。
//! - 松手 = 1 条 `UpdateRegionCommand`（可撤销）。
//!
//! ## 坐标系
//!
//! 区域多边形 = **图内局部 wu**；格点在图内局部 wu 上位于
//! `k·cs + (map_origin − raster_origin)`——两套原点的差必须参与吸附。

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::edit::editing::EditHistory;
use crate::state::{GridConfig, PlaybackState, ViewState, WorldProjection};

/// 点到线段的距离（wu）。
fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt();
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// 图内局部 wu → 就近格点（栅格角点）的图内局部 wu。
fn snap_to_corner(local: (f32, f32), cs: f32, corner0: (f32, f32)) -> (f32, f32) {
    let kx = ((local.0 - corner0.0) / cs).round();
    let ky = ((local.1 - corner0.1) / cs).round();
    (corner0.0 + kx * cs, corner0.1 + ky * cs)
}

/// 区域边界拖拽输入系统。
#[allow(clippy::too_many_arguments)]
pub fn region_adjust_input(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    drill: Res<wv_render::state::DrillState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut st: ResMut<wv_render::state::RegionAdjustState>,
    mut nature: ResMut<crate::state::NatureRegionUiState>,
) {
    use crate::state::ViewMode;
    if !matches!(view.mode, ViewMode::Topographic) || playback.playing || ui.wants_pointer {
        return;
    }
    let Some(map) = drill.current().cloned() else {
        return;
    };
    // 选中目标变化（切选/删区）⇒ 会话失效。
    let sel_region = match projection.selection.as_ref() {
        Some(sel) if sel.kind == wv_core::EntityKind::Region => Some(sel.id.clone()),
        _ => None,
    };
    if st.region != sel_region {
        st.clear();
        st.region = sel_region;
    }
    // 候选态（2026-10-22 验收）：selection 里没有候选（它不是 Region 实体），
    // 但作者点选候选后要能拉锚点——走独立分支，拖的是候选多边形。
    if st.region.is_none() {
        let Some(cell) = nature.pending_candidate else {
            return;
        };
        let Some(cursor) = view.cursor_world else {
            return;
        };
        let local: (f32, f32) = (cursor - grid.map_origin).into();
        // 候选多边形：编辑中优先，否则按格派生。
        let poly = nature
            .candidate_polygon
            .clone()
            .unwrap_or_else(|| derive_candidate_polygon(&projection, &map, cell, &grid));
        if poly.len() < 6 {
            return;
        }
        let mut preview = st.preview_polygon.clone();
        let mut drag_vertex = st.drag_vertex;
        let mut press_world = st.press_world;
        drag_anchors(
            &buttons,
            &view,
            &grid,
            local,
            cursor,
            &poly,
            &mut preview,
            &mut drag_vertex,
            &mut press_world,
            |edited| {
                // 候选态：改候选多边形（物化时用）。
                nature.candidate_polygon = Some(edited);
            },
        );
        st.preview_polygon = preview;
        st.drag_vertex = drag_vertex;
        st.press_world = press_world;
        return;
    }
    let Some(region_id) = st.region.clone() else {
        return;
    };
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let local: (f32, f32) = (cursor - grid.map_origin).into();

    let Some(region) = projection.world.regions.iter().find(|r| r.id == region_id) else {
        st.clear();
        return;
    };
    if region.map != map || region.polygon.len() < 6 {
        return;
    }
    let stored_polygon = region.polygon.clone();
    let region_name = region.name.clone();

    // ── 按下：冻结当前缩放的锚点集，命中就近锚点 ──
    if buttons.just_pressed(MouseButton::Left) {
        let spacing = wv_render::biome_boundary::anchor_spacing(view.scale, grid.cell_size, 24.0);
        let poly_pairs: Vec<(f32, f32)> = stored_polygon
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| (c[0], c[1]))
            .collect();
        let anchors = wv_render::biome_boundary::decimate_poly(&poly_pairs, spacing);
        let near = spacing * 0.35;
        let mut best: Option<(usize, f32)> = None;
        for (i, a) in anchors.iter().enumerate() {
            let d = dist_to_segment(local, *a, *a);
            if d <= near && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        if best.is_some() {
            st.preview_polygon = Some(anchors.iter().flat_map(|p| [p.0, p.1]).collect());
            st.drag_vertex = best.map(|(i, _)| i);
            st.press_world = Some(cursor);
        }
        return;
    }

    // ── 拖动中：锚点吸附格点 ──
    if buttons.pressed(MouseButton::Left) {
        let Some(vi) = st.drag_vertex else {
            return;
        };
        let Some(mut poly) = st.preview_polygon.clone() else {
            return;
        };
        if vi * 2 + 1 >= poly.len() {
            return;
        }
        let corner0 = (
            grid.map_origin.x - grid.raster_origin.x,
            grid.map_origin.y - grid.raster_origin.y,
        );
        let snapped = snap_to_corner(local, grid.cell_size, corner0);
        poly[vi * 2] = snapped.0;
        poly[vi * 2 + 1] = snapped.1;
        st.preview_polygon = Some(poly);
        return;
    }

    // ── 松手：提交（1 条命令，可撤销）──
    if buttons.just_released(MouseButton::Left) {
        let Some(preview) = st.preview_polygon.take() else {
            return;
        };
        st.drag_vertex = None;
        st.press_world = None;
        if preview == stored_polygon || preview.len() < 6 {
            return;
        }
        crate::edit::region_panel_actions::update_region_now(
            &mut projection,
            &mut edit,
            &view,
            region_id,
            region_name,
            preview,
        );
    }
}

/// 候选格的 biome 连通区 → 图内局部 wu 多边形（与物化同源）。
fn derive_candidate_polygon(
    projection: &WorldProjection,
    map: &wv_core::Id,
    cell: (i32, i32),
    grid: &GridConfig,
) -> Vec<f32> {
    let Some(terrain) = projection.world.terrain_of(map) else {
        return Vec::new();
    };
    let cells: Vec<wv_terrain::TerrainCell> = terrain
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let Some(g) = wv_terrain::TerrainGrid::from_cells(terrain.width, terrain.height, cells) else {
        return Vec::new();
    };
    let Some(region) = wv_terrain::region::biome_region_at(&g, cell.0, cell.1 as i64) else {
        return Vec::new();
    };
    let corner = (
        grid.map_origin.x - grid.raster_origin.x,
        grid.map_origin.y - grid.raster_origin.y,
    );
    region
        .outer
        .iter()
        .flat_map(|&(x, y)| [x * grid.cell_size + corner.0, y * grid.cell_size + corner.1])
        .collect()
}

/// 锚点拖动的共享实现（候选态与区域态同款：按下命中 → 拖动吸附 → 松手回调）。
#[allow(clippy::too_many_arguments)]
fn drag_anchors(
    buttons: &ButtonInput<MouseButton>,
    view: &ViewState,
    grid: &GridConfig,
    local: (f32, f32),
    cursor: Vec2,
    poly: &[f32],
    preview: &mut Option<Vec<f32>>,
    drag_vertex: &mut Option<usize>,
    press_world: &mut Option<Vec2>,
    mut on_release: impl FnMut(Vec<f32>),
) {
    let poly_pairs: Vec<(f32, f32)> = poly
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect();
    let spacing = wv_render::biome_boundary::anchor_spacing(view.scale, grid.cell_size, 24.0);
    let anchors = wv_render::biome_boundary::decimate_poly(&poly_pairs, spacing);
    let near = spacing * 0.35;

    if buttons.just_pressed(MouseButton::Left) {
        let mut best: Option<(usize, f32)> = None;
        for (i, a) in anchors.iter().enumerate() {
            let d = dist_to_segment(local, *a, *a);
            if d <= near && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        if let Some((vi, _)) = best {
            *preview = Some(anchors.iter().flat_map(|p| [p.0, p.1]).collect());
            *drag_vertex = Some(vi);
            *press_world = Some(cursor);
        }
        return;
    }
    if buttons.pressed(MouseButton::Left) {
        let Some(vi) = *drag_vertex else {
            return;
        };
        let Some(mut p) = preview.clone() else {
            return;
        };
        if vi * 2 + 1 >= p.len() {
            return;
        }
        let corner0 = (
            grid.map_origin.x - grid.raster_origin.x,
            grid.map_origin.y - grid.raster_origin.y,
        );
        let snapped = snap_to_corner(local, grid.cell_size, corner0);
        p[vi * 2] = snapped.0;
        p[vi * 2 + 1] = snapped.1;
        *preview = Some(p);
        return;
    }
    if buttons.just_released(MouseButton::Left) {
        let Some(p) = preview.take() else {
            return;
        };
        *drag_vertex = None;
        *press_world = None;
        if p.len() >= 6 {
            on_release(p);
        }
    }
}
