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

// ---------------------------------------------------------------------------
// 多边形 → 方格贴合（2026-10-20 批量编辑）
// ---------------------------------------------------------------------------

/// 射线法点在多边形内（纯函数；顶点/边上的点按「内」处理不了——v1 接受
/// 半格误差，贴合语义是「cell 中心落入即选中」，不是精确几何剖分）。
fn point_in_polygon(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        let crosses = (yi > py) != (yj > py);
        if crosses {
            let x_at = (xj - xi) * (py - yi) / (yj - yi) + xi;
            if px < x_at {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// 闭合多边形 → 覆盖到的方格集合（**自动贴合方格**）。
///
/// 判定口径：**cell 中心点在多边形内即选中**（与 `cell_pick` 的
/// `floor` 采样同一坐标链——`raster_origin + (gx+0.5)·cs`）。
/// 扫描范围钳在多边形包围盒 ∩ 栅格范围，O(bbox_cells × 多边形边数)。
/// 结果按扫描序返回（行优先，天然稳定，测试可断言）。
pub fn rasterize_polygon_cells(
    closed: &[(f32, f32)],
    grid: &GridConfig,
    terrain_w: u32,
    terrain_h: u32,
) -> Vec<(i32, i32)> {
    if closed.len() < 3 {
        return Vec::new();
    }
    let cs = grid.cell_size;
    if cs <= 0.0 {
        return Vec::new();
    }
    // 多边形包围盒（世界坐标）。
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for (x, y) in closed {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    // 世界 → 栅格范围（钳到栅格内）。
    let gx0 = (((min_x - grid.raster_origin.x) / cs).floor() as i32).clamp(0, terrain_w as i32 - 1);
    let gy0 = (((min_y - grid.raster_origin.y) / cs).floor() as i32).clamp(0, terrain_h as i32 - 1);
    let gx1 = (((max_x - grid.raster_origin.x) / cs).ceil() as i32).clamp(0, terrain_w as i32);
    let gy1 = (((max_y - grid.raster_origin.y) / cs).ceil() as i32).clamp(0, terrain_h as i32);
    let poly: Vec<(f32, f32)> = closed.to_vec();
    let mut cells = Vec::new();
    for gy in gy0..gy1 {
        for gx in gx0..gx1 {
            let cx = grid.raster_origin.x + (gx as f32 + 0.5) * cs;
            let cy = grid.raster_origin.y + (gy as f32 + 0.5) * cs;
            if point_in_polygon(cx, cy, &poly) {
                cells.push((gx, gy));
            }
        }
    }
    cells
}

/// 区域选区的 batch 高亮实体（复用：稳态 1 填充 mesh + 4 条包边 mesh）。
#[derive(Resource, Default)]
pub struct CellRegionEntities {
    /// 半透金色填充（所有选中格合成一个 mesh）。
    fill: Option<Entity>,
    /// 外包矩形 4 条边（下/上/左/右，屏幕恒定 2px 世界厚度）。
    edges: [Option<Entity>; 4],
}

impl CellRegionEntities {
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

/// 由格集合构建**合并四边形 mesh**（单个 Mesh，N 格 = 4N 顶点）。
///
/// 顶点坐标 = 相对 `origin` 的局部偏移；mesh 实体 Transform 平移到 `origin`。
/// 返回 `(mesh, origin, bbox_size)`；空集返 `None`。
fn build_region_fill_mesh(cells: &[(i32, i32)], grid: &GridConfig) -> Option<(Mesh, Vec2, Vec2)> {
    if cells.is_empty() {
        return None;
    }
    let cs = grid.cell_size;
    // 包围盒（格坐标）。
    let (mut gx0, mut gy0) = (i32::MAX, i32::MAX);
    let (mut gx1, mut gy1) = (i32::MIN, i32::MIN);
    for (gx, gy) in cells {
        gx0 = gx0.min(*gx);
        gy0 = gy0.min(*gy);
        gx1 = gx1.max(*gx);
        gy1 = gy1.max(*gy);
    }
    let origin = grid.raster_origin + Vec2::new(gx0 as f32 * cs, gy0 as f32 * cs);
    let bbox = Vec2::new((gx1 - gx0 + 1) as f32 * cs, (gy1 - gy0 + 1) as f32 * cs);
    // 手拼三角形列表：每格两三角。顶点 = 相对 origin 的局部坐标。
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(cells.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(cells.len() * 6);
    for (gx, gy) in cells {
        let min = Vec2::new((gx - gx0) as f32 * cs, (gy - gy0) as f32 * cs);
        let base = positions.len() as u32;
        let (x0, y0) = (min.x, min.y);
        let (x1, y1) = (min.x + cs, min.y + cs);
        positions.push([x0, y0, 0.0]);
        positions.push([x1, y0, 0.0]);
        positions.push([x1, y1, 0.0]);
        positions.push([x0, y1, 0.0]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    // ColorMaterial 需要 UV/法线？不需要——纯色材质仅位置即可（Bevy 0.19
    // ColorMaterial 走 mesh pipeline 的 position + 可选 uv；无 uv 时纯色可渲染）。
    mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
    Some((mesh, origin, bbox))
}

/// 区域选区高亮（明显视觉 = 金色半透填充 + **alpha 脉动**）。
///
/// gate = `recipe.enabled(CellSelection) ∧ !edit_cell.region_cells.is_empty()`；
/// 与单格高亮（`sync_cell_selection`）并存——区域优先视觉（单格 outline
/// 画在 Z+0.001，区域填充在其下）。
#[allow(clippy::too_many_arguments)]
pub fn sync_cell_region(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    recipe: Res<crate::recipe::RenderRecipe>,
    edit_cell: Res<EditCellState>,
    grid: Res<GridConfig>,
    time: Res<Time>,
    mut entities: ResMut<CellRegionEntities>,
) {
    let cells = edit_cell.region_cells.as_slice();
    let active = recipe.enabled(PassId::CellSelection) && !cells.is_empty();
    if !active {
        entities.despawn_all(&mut commands);
        return;
    }
    let Some((mesh, origin, bbox)) = build_region_fill_mesh(cells, &grid) else {
        entities.despawn_all(&mut commands);
        return;
    };
    // 脉动：0.22 + 0.12·sin(3.5t) → alpha ∈ [0.10, 0.34]，肉眼可辨的呼吸感。
    let alpha = 0.22 + 0.12 * (time.elapsed_secs() * 3.5).sin();
    let fill_mesh = meshes.add(mesh);
    let fill_mat = materials.add(ColorMaterial::from_color(
        crate::theme::colors::ACCENT_SELECT.with_alpha(alpha),
    ));
    let tf = Transform::from_translation(origin.extend(0.899)); // 单格高亮(0.9)之下
    match entities.fill {
        Some(e) => {
            commands
                .entity(e)
                .insert((tf, Mesh2d(fill_mesh), MeshMaterial2d(fill_mat)));
        }
        None => {
            entities.fill = Some(
                commands
                    .spawn((tf, Mesh2d(fill_mesh), MeshMaterial2d(fill_mat)))
                    .id(),
            );
        }
    }
    // 外包矩形 4 条边（明显视觉的第二层，屏幕恒定 2px）。
    let thickness = 2.0 * 1.0; // 视口 scale 未传——固定 2 世界单位，批量区域通常近观
    let t2 = thickness * 0.5;
    let half = bbox * 0.5;
    let edge_defs: [(Vec2, Vec2); 4] = [
        (Vec2::new(0.0, -half.y + t2), Vec2::new(bbox.x, thickness)),
        (Vec2::new(0.0, half.y - t2), Vec2::new(bbox.x, thickness)),
        (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, bbox.y)),
        (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, bbox.y)),
    ];
    let center = origin + bbox * 0.5;
    for (i, (offset, edge_size)) in edge_defs.into_iter().enumerate() {
        let mesh = meshes.add(Rectangle::new(edge_size.x, edge_size.y));
        let mat = materials.add(ColorMaterial::from_color(
            crate::theme::colors::ACCENT_SELECT.with_alpha((alpha * 2.5).min(1.0)),
        ));
        let tf = Transform::from_translation((center + offset).extend(0.8995));
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

    // -------------------------------------------------------------------
    // 多边形贴合方格（批量编辑）
    // -------------------------------------------------------------------

    /// 矩形多边形覆盖的格全部选中，且**只**选中中心落入的格。
    /// 8×8 栅格、cell 10；多边形盖 (2,2)..(4,4) 三格方阵。
    #[test]
    fn rasterize_polygon_snaps_rectangle_to_cells() {
        let g = grid(10.0, 0.0, 0.0);
        // 覆盖格 (2,2)-(4,4) 中心：世界 [20,50)²；取多边形 (21,21)-(49,49)。
        let poly = [(21.0, 21.0), (49.0, 21.0), (49.0, 49.0), (21.0, 49.0)];
        let cells = rasterize_polygon_cells(&poly, &g, 8, 8);
        assert_eq!(cells.len(), 9, "3×3 方阵");
        for gy in 2..=4 {
            for gx in 2..=4 {
                assert!(cells.contains(&(gx, gy)), "缺格 ({gx},{gy})");
            }
        }
    }

    /// 多边形出界 → 只返回栅格内部分，不 panic。
    #[test]
    fn rasterize_polygon_clamps_to_grid() {
        let g = grid(10.0, 0.0, 0.0);
        let poly = [
            (-100.0, -100.0),
            (100.0, -100.0),
            (100.0, 100.0),
            (-100.0, 100.0),
        ];
        let cells = rasterize_polygon_cells(&poly, &g, 4, 4);
        assert_eq!(cells.len(), 16, "4×4 全选中");
    }

    /// 退化输入（<3 顶点 / cell_size=0）返空，不 panic。
    #[test]
    fn rasterize_polygon_rejects_degenerate_input() {
        let g = grid(0.0, 0.0, 0.0);
        assert!(rasterize_polygon_cells(&[(0.0, 0.0), (1.0, 1.0)], &g, 4, 4).is_empty());
        let g = grid(10.0, 0.0, 0.0);
        assert!(rasterize_polygon_cells(&[(0.0, 0.0), (1.0, 1.0)], &g, 4, 4).is_empty());
    }

    /// 填充 mesh：9 格 → 36 顶点 54 索引，包围盒 = 3 格见方。
    #[test]
    fn region_fill_mesh_batches_all_cells_into_one_mesh() {
        let g = grid(10.0, 0.0, 0.0);
        let cells: Vec<(i32, i32)> = (2..=4)
            .flat_map(|gx| (2..=4).map(move |gy| (gx, gy)))
            .collect();
        let (mesh, origin, bbox) = build_region_fill_mesh(&cells, &g).expect("非空");
        let count = mesh.count_vertices();
        assert_eq!(count, 36);
        assert_eq!(origin, Vec2::new(20.0, 20.0));
        assert_eq!(bbox, Vec2::new(30.0, 30.0));
    }
}
