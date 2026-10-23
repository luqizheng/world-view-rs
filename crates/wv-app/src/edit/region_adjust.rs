//! 区域边界拖拽调整（2026-10-22 验收反馈 · CellMap 专业数据编辑器）。
//!
//! ## 交互（用户裁定）
//!
//! - 选中区域（点标签 / 区域面板行）⇒ CellMap 里出**方格描边**（沿格边的
//!   阶梯描边，非平滑多边形）+ 多边形**顶点手柄**。
//! - 拖动手柄 = 调整区域；**方格为锚点**——顶点吸附到格点（栅格角点），
//!   拖动按整格步进。
//! - 松手 = 1 条 `UpdateRegionCommand`（可撤销）。拖动中的预览不落盘。
//!
//! ## 坐标系
//!
//! 区域多边形 = **图内局部 wu**；手柄吸附的格点（栅格角点）在图内局部 wu
//! 上位于 `k·cs + (map_origin − raster_origin)`——两套原点的差必须参与吸附
//! （ADR-0045 追记三同款换算）。

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::edit::editing::EditHistory;
use crate::state::{GridConfig, PlaybackState, ViewState, WorldProjection};

use wv_core::Id;

/// 区域调整的会话态（CellMap · 选中区域后出现）。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct RegionAdjustState {
    /// 正在调整的区域；`None` = 当前无调整会话。
    pub region: Option<Id>,
    /// 调整中的多边形预览（图内局部 wu，扁平坐标）。
    /// `None` = 未在拖动（展示已落盘轮廓）。
    pub preview_polygon: Option<Vec<f32>>,
    /// 正在拖动的顶点下标（偶数= x，奇数= y）。
    pub drag_vertex: Option<usize>,
    /// 按下点（世界坐标），用于命中判定。
    pub press_world: Option<Vec2>,
}

impl RegionAdjustState {
    /// 切图 / 切模式清空。
    pub fn clear(&mut self) {
        self.region = None;
        self.preview_polygon = None;
        self.drag_vertex = None;
        self.press_world = None;
    }
}

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
///
/// 激活条件：Topographic ∧ 选中了有轮廓的区域。PointerOverUi 门与
/// `commit_edit_cell_on_click` 同口径（面板交互不吃拖拽）。
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
) {
    use crate::state::ViewMode;
    if !matches!(view.mode, ViewMode::Topographic) || playback.playing || ui.wants_pointer {
        return;
    }
    // 当前图：选区多边形是图内局部 wu——只对本图生效。
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
    let Some(region_id) = st.region.clone() else {
        return;
    };
    let Some(cursor) = view.cursor_world else {
        return;
    };
    // 世界 → 图内局部（map-local）wu。
    let local = (cursor - grid.map_origin).into();

    let Some(region) = projection.world.regions.iter().find(|r| r.id == region_id) else {
        st.clear();
        return;
    };
    if region.map != map || region.polygon.len() < 6 {
        return;
    }
    let region_name = region.name.clone();
    let stored_polygon = region.polygon.clone();

    // 拖动中的预览多边形（拖动时优先；未拖动用落盘轮廓）。
    let active_poly = st
        .preview_polygon
        .clone()
        .unwrap_or_else(|| stored_polygon.clone());

    // ── 按下：命中顶点手柄（0.6 格内就近） ──
    if buttons.just_pressed(MouseButton::Left) {
        let near = 0.6 * grid.cell_size;
        let mut best: Option<(usize, f32)> = None;
        for (i, chunk) in active_poly.chunks_exact(2).enumerate() {
            let d = dist_to_segment(local, (chunk[0], chunk[1]), (chunk[0], chunk[1]));
            if d <= near && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        if let Some((vi, _)) = best {
            st.drag_vertex = Some(vi);
            st.press_world = Some(cursor);
            st.preview_polygon = Some(active_poly);
        }
        return;
    }

    // ── 拖动中：顶点吸附格点 ──
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
        // 顶点数不变（拖拽不改形状复杂度），且与落盘轮廓不同才提交。
        if preview == stored_polygon {
            return;
        }
        crate::edit::region_panel_actions::update_region_now(
            &mut projection,
            &mut edit,
            &view,
            region_id,
            region_name.clone(),
            preview,
        );
    }
}
