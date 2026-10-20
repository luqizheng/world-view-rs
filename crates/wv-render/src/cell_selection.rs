//! 单格编辑工具的选中高亮（[ADR-0040] §决策 4 · `PassId::CellSelection`）。
//!
//! ## 职责
//! - `EditCellState.selected = Some((gx, gy))` 时，在该格上画
//!   **1 格 outline + 半透底色**（金色 `ACCENT_SELECT`，底色 alpha 0.2）。
//! - `RenderRecipe.enabled(PassId::CellSelection)` 是总闸（[ADR-0021] 模式轴）；
//!   会话轴由 `EditCellState.selected` 承载——两者合取，任一不满足即 despawn。
//!
//! ## 边界（不做什么）
//! - **不吃输入**——selected 由 `wv-app::edit::commit_edit_cell_on_click` 写入；
//!   本模块只画（与 [`crate::brush_cursor`] 同款分工）。
//! - **不画 hover**——hover 是即时状态，画出来只会闪烁；只画 committed 的格。
//!
//! ## 几何
//! 格 `(gx, gy)` 的世界矩形 = `raster_origin + (gx·cs, gy·cs)` 起、
//! 边长 `cs = cell_size` 的正方形（与 `cell_pick::pick_at` 反查同一坐标链，
//! 保证「光标压的格」与「高亮的格」逐格一致）。
//!
//! ## 主要导出
//! - `sync_cell_selection` — 每帧同步 5 个 mesh 实体（填充 + 4 条边）。
//! - `cell_world_rect` — 格 → (中心, 尺寸) 纯函数（测试锚）。

#![cfg(feature = "app")]

use bevy::math::primitives::Rectangle;
use bevy::prelude::*;

use crate::recipe::PassId;
use crate::state::{EditCellState, GridConfig, ViewState};
use crate::theme::colors;

/// Z 偏移：与笔刷光标同层（顶层会话带，[MAP_LAYER_RENDERING_SPEC §7]）。
const SELECTION_Z: f32 = 0.9;
/// outline 屏幕像素厚度（与 [`crate::brush_cursor::STROKE_PX`] 同档：2px
/// 在 MSAA4 下不被采样稀释）。世界厚度 = `THICKNESS_PX × view.scale`。
const THICKNESS_PX: f32 = 2.0;
/// 半透底色 alpha（[ADR-0040] §决策 4 默认值）。
const FILL_ALPHA: f32 = 0.2;

/// 选中格的 mesh 实体集合（复用：稳态 5 实体 = 填充 + 4 边）。
#[derive(Resource, Default)]
pub struct CellSelectionEntities {
    /// 半透填充块。
    fill: Option<Entity>,
    /// 四条边（下 / 上 / 左 / 右）。
    edges: [Option<Entity>; 4],
}

impl CellSelectionEntities {
    /// 全部 despawn + 清空（gate 不满足时调用）。
    fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
        for slot in &mut self.edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }
}

/// 格 `(gx, gy)` → 世界空间 (中心, 尺寸)。
///
/// 纯函数——`cell_pick` 的反查（`floor((cursor − origin) / cs)`）与本函数
/// 互为逆运算，坐标链一致性由两者的同构保证。
pub fn cell_world_rect(grid: &GridConfig, gx: i32, gy: i32) -> (Vec2, Vec2) {
    let cs = grid.cell_size;
    let min = grid.raster_origin + Vec2::new(gx as f32 * cs, gy as f32 * cs);
    (min + Vec2::splat(cs) * 0.5, Vec2::splat(cs))
}

/// 每帧同步选中高亮的 5 个实体。
///
/// gate = `recipe.enabled(CellSelection) ∧ edit_cell.selected.is_some()`；
/// 不满足时全 despawn（与笔刷光标的「切走即清」同口径，无残留）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn sync_cell_selection(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    recipe: Res<crate::recipe::RenderRecipe>,
    edit_cell: Res<EditCellState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut entities: ResMut<CellSelectionEntities>,
) {
    let Some((gx, gy)) = edit_cell
        .selected
        .filter(|_| recipe.enabled(PassId::CellSelection))
    else {
        entities.despawn_all(&mut commands);
        return;
    };
    let (center, size) = cell_world_rect(&grid, gx, gy);
    let accent = colors::ACCENT_SELECT;
    // 世界厚度 = 屏幕恒定 2px（策略 A，与笔刷光标一致）。
    let thickness = THICKNESS_PX * view.scale;

    // ── 填充 ──
    let fill_mesh = meshes.add(Rectangle::new(size.x, size.y));
    let fill_mat = materials.add(ColorMaterial::from_color(accent.with_alpha(FILL_ALPHA)));
    match entities.fill {
        Some(e) => {
            commands.entity(e).insert((
                Transform::from_translation(center.extend(SELECTION_Z)),
                Mesh2d(fill_mesh),
                MeshMaterial2d(fill_mat),
            ));
        }
        None => {
            entities.fill = Some(
                commands
                    .spawn((
                        Transform::from_translation(center.extend(SELECTION_Z)),
                        Mesh2d(fill_mesh),
                        MeshMaterial2d(fill_mat),
                    ))
                    .id(),
            );
        }
    }

    // ── 四条边：下 (0) / 上 (1) / 左 (2) / 右 (3) ──
    // 每条边 = 细长 Rectangle,中心贴格边线,厚度覆盖向格内（视觉描边）。
    let half = size * 0.5;
    let t2 = thickness * 0.5;
    let edge_defs: [(Vec2, Vec2); 4] = [
        // (中心偏移, 尺寸)
        (Vec2::new(0.0, -half.y + t2), Vec2::new(size.x, thickness)), // 下
        (Vec2::new(0.0, half.y - t2), Vec2::new(size.x, thickness)),  // 上
        (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, size.y)), // 左
        (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, size.y)),  // 右
    ];
    for (i, (offset, edge_size)) in edge_defs.into_iter().enumerate() {
        let mesh = meshes.add(Rectangle::new(edge_size.x, edge_size.y));
        let mat = materials.add(ColorMaterial::from_color(accent));
        let tf = Transform::from_translation((center + offset).extend(SELECTION_Z + 0.001));
        match entities.edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(mat)));
            }
            None => {
                entities.edges[i] =
                    Some(commands.spawn((tf, Mesh2d(mesh), MeshMaterial2d(mat))).id());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cs: f32, ox: f32, oy: f32) -> GridConfig {
        GridConfig {
            cell_size: cs,
            raster_origin: Vec2::new(ox, oy),
            map_origin: Vec2::ZERO,
            map_size: Vec2::new(1024.0, 512.0),
        }
    }

    /// `cell_world_rect` 是 `cell_pick::pick_at` 的逆运算：矩形中心反查
    /// 必须落回同一格。
    #[test]
    fn cell_world_rect_inverts_cell_pick() {
        let g = grid(8.0, 100.0, 200.0);
        // pick: (123, 234) → (2, 3)（见 cell_pick 测试）。
        let (center, size) = cell_world_rect(&g, 2, 3);
        assert_eq!(size, Vec2::splat(8.0));
        // 中心 = origin + (2.5·8, 3.5·8) = (120, 228)。
        assert_eq!(center, Vec2::new(100.0 + 2.5 * 8.0, 200.0 + 3.5 * 8.0));
    }

    /// 负坐标格同样成立（栅格外的格不该 panic——防御性）。
    #[test]
    fn cell_world_rect_handles_negative_grid_coords() {
        let g = grid(10.0, 0.0, 0.0);
        let (center, size) = cell_world_rect(&g, -1, -1);
        assert_eq!(center, Vec2::splat(-5.0));
        assert_eq!(size, Vec2::splat(10.0));
    }
}
