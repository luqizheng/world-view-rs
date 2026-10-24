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
//! - **不画 hover**——hover 由本模块的 [`CellHoverEntities`] 单独承载
//!   （[ADR-0052] §9.5 四态视觉分工：hover 与 selected 是两套实体）。
//!
//! ## 几何
//! 格 `(gx, gy)` 的世界矩形 = `raster_origin + (gx·cs, gy·cs)` 起、
//! 边长 `cs = cell_size` 的正方形（与 `cell_pick::pick_at` 反查同一坐标链，
//! 保证「光标压的格」与「高亮的格」逐格一致）。
//!
//! ## 主要导出
//! - `sync_cell_selection` — 每帧同步 5 个 mesh 实体（填充 + 4 条边）。
//! - `cell_world_rect` — 格 → (中心, 尺寸) 纯函数（测试锚）。

use bevy::prelude::*;

use crate::state::GridConfig;

/// Z 偏移：与笔刷光标同层（顶层会话带，[MAP_LAYER_RENDERING_SPEC §7]）。
pub const SELECTION_Z: f32 = 0.9;
/// outline 屏幕像素厚度（与 [`crate::brush_cursor::STROKE_PX`] 同档：2px
/// 在 MSAA4 下不被采样稀释）。世界厚度 = `THICKNESS_PX × view.scale`。
pub const THICKNESS_PX: f32 = 2.0;
/// 半透底色 alpha（[ADR-0040] §决策 4 默认值）。
pub const FILL_ALPHA: f32 = 0.2;

// ---------------------------------------------------------------------------
// hover 高亮（[ADR-0052] D2 / §9.5 四态视觉分工）
// ---------------------------------------------------------------------------

/// hover 内线屏幕像素厚度（白色内线）。
pub const HOVER_INNER_PX: f32 = 1.0;
/// hover 外晕屏幕像素厚度（深墨外晕；比内线宽，在两种底色上都兜得住）。
pub const HOVER_OUTER_PX: f32 = 2.0;
/// hover Z 偏移：比选中（`SELECTION_Z = 0.9`）**更高**——hover 是
/// 「此刻光标在哪」的瞬时反馈，必须画在最上层（[ADR-0052] §9.5 绘制序：
/// region → stroke → selected → hover）。
pub const HOVER_Z: f32 = 0.95;

/// hover 高亮的实体集合。
///
/// **双色描边**：每条边画两层——外层深墨（`LINE_ON_DARK`）宽 2px，
/// 内层白（`Color::WHITE`）宽 1px。深墨在下、白线在上，形成
/// 「白内线 + 深墨外晕」的地图准星观感：无论格底是水面还是陆地，
/// 至少有一层与底色拉开对比（[ADR-0052] §5 M1 验收 ③）。
///
/// **准星**：格中心画十字（横 + 竖两条细线），同样双色。
#[derive(Resource, Default)]
pub struct CellHoverEntities {
    /// 4 条边的**外晕**（深墨，宽 2px）。
    pub outer_edges: [Option<Entity>; 4],
    /// 4 条边的**内线**（白，宽 1px）。
    pub inner_edges: [Option<Entity>; 4],
    /// 准星**外晕**（深墨）横 / 竖。
    pub cross_outer: [Option<Entity>; 2],
    /// 准星**白芯**横 / 竖。
    pub cross_inner: [Option<Entity>; 2],
    /// 外晕 mesh 句柄（横 / 竖）。
    pub outer_h_mesh: Option<Handle<Mesh>>,
    pub outer_v_mesh: Option<Handle<Mesh>>,
    /// 内线 mesh 句柄（横 / 竖）。
    pub inner_h_mesh: Option<Handle<Mesh>>,
    pub inner_v_mesh: Option<Handle<Mesh>>,
    /// 准星外晕 mesh 句柄（横 / 竖）。
    pub cross_h_mesh: Option<Handle<Mesh>>,
    pub cross_v_mesh: Option<Handle<Mesh>>,
    /// 准星白芯 mesh 句柄（横 / 竖）。
    pub core_h_mesh: Option<Handle<Mesh>>,
    pub core_v_mesh: Option<Handle<Mesh>>,
    /// 深墨外晕材质句柄（外晕 + 准星底共用）。
    pub outer_material: Option<Handle<ColorMaterial>>,
    /// 白内线材质句柄（内线 + 准星芯共用）。
    pub inner_material: Option<Handle<ColorMaterial>>,
    /// 上次构建时的格尺寸（世界单位）——变了才重建 mesh。
    pub last_size: Vec2,
}

impl CellHoverEntities {
    /// 全部 despawn + 清空实体句柄（gate 不满足时调用）。
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        for slot in &mut self.cross_outer {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
        for slot in &mut self.cross_inner {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
        for slot in &mut self.outer_edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
        for slot in &mut self.inner_edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }

    /// 释放已缓存的 mesh 资产（实体消失后调用，避免 `Assets` 堆积）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        for h in [
            self.outer_h_mesh.take(),
            self.outer_v_mesh.take(),
            self.inner_h_mesh.take(),
            self.inner_v_mesh.take(),
            self.cross_h_mesh.take(),
            self.cross_v_mesh.take(),
            self.core_h_mesh.take(),
            self.core_v_mesh.take(),
        ] {
            if let Some(h) = h {
                meshes.remove(&h);
            }
        }
        self.last_size = Vec2::ZERO;
    }
}


/// 选中格的 mesh 实体集合（复用：稳态 5 实体 = 填充 + 4 边）。
///
/// **资产句柄一并缓存**（2026-10-20 实机卡顿修）：本模块初版每帧
/// `meshes.add()` + `materials.add()` 新建资产再 `insert` 替换——旧句柄
/// 永不回收，`Assets` 无限堆积 + 每帧 GPU 重传，选中一片就明显变卡。
/// 现在 mesh/material **只建一次**，之后只改 Transform（位置）与材质
/// alpha（脉动）。
#[derive(Resource, Default)]
pub struct CellSelectionEntities {
    /// 半透填充块。
    pub fill: Option<Entity>,
    /// 四条边（下 / 上 / 左 / 右）。
    pub edges: [Option<Entity>; 4],
    /// 填充 mesh 句柄（复用）。
    pub fill_mesh: Option<Handle<Mesh>>,
    /// 填充材质句柄（复用；alpha 每帧改）。
    pub fill_material: Option<Handle<ColorMaterial>>,
    /// 四条边的 mesh 句柄（复用）。
    pub edge_meshes: [Option<Handle<Mesh>>; 4],
    /// 四条边的材质句柄（复用）。
    pub edge_materials: [Option<Handle<ColorMaterial>>; 4],
    /// 上次构建时的格尺寸（世界单位）——变了才重建 mesh。
    pub last_size: Vec2,
}

impl CellSelectionEntities {
    /// 全部 despawn + 清空（gate 不满足时调用）。
    ///
    /// **资产句柄保留**：下次重建时 `meshes.remove()` 旧的即可——句柄本身
    /// 在 `Assets` 里，实体 despawn 不会连带回收，所以要显式 remove。
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
        for slot in &mut self.edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }

    /// 释放已缓存的 mesh 资产（实体消失后调用，避免 `Assets` 堆积）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        if let Some(h) = self.fill_mesh.take() {
            meshes.remove(&h);
        }
        for slot in &mut self.edge_meshes {
            if let Some(h) = slot.take() {
                meshes.remove(&h);
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
    closed_local: &[(f32, f32)],
    content_origin: Vec2,
    grid: &GridConfig,
    terrain_w: u32,
    terrain_h: u32,
) -> Vec<(i32, i32)> {
    if closed_local.len() < 3 {
        return Vec::new();
    }
    let cs = grid.cell_size;
    if cs <= 0.0 {
        return Vec::new();
    }
    // **坐标链（关键）**：选区锚点存的是**本图局部坐标**
    // （`SelectionState.draft` ← `cursor_world − map_origin`，见
    // `selection::commit_selection_click`），而地形栅格在**世界坐标**
    // `raster_origin + cell·cs`（见 `terrain_chunks::sync_terrain_chunks`）。
    // 两套原点不同（`map_origin` = 本图图帧原点，`raster_origin` = 栅格原点，
    // 城市子图继承父图栅格时二者不等）——必须先补 `content_origin` 再减
    // `raster_origin`，否则选区会整体错位（本轮实机症状：三角形只圈住
    // 一个小岛，却选出 21974 格）。
    //
    // 矩形体（`cell_world_rect`）与点拾取（`cell_pick::pick_at`）都直接用
    // 世界坐标，与此处同源。
    let poly_world: Vec<(f32, f32)> = closed_local
        .iter()
        .map(|(x, y)| (x + content_origin.x, y + content_origin.y))
        .collect();
    // 多边形包围盒（世界坐标）。
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for (x, y) in &poly_world {
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
    let mut cells = Vec::new();
    for gy in gy0..gy1 {
        for gx in gx0..gx1 {
            let cx = grid.raster_origin.x + (gx as f32 + 0.5) * cs;
            let cy = grid.raster_origin.y + (gy as f32 + 0.5) * cs;
            if point_in_polygon(cx, cy, &poly_world) {
                cells.push((gx, gy));
            }
        }
    }
    cells
}

/// 区域选区的 batch 高亮实体（复用：稳态 1 填充 mesh + 4 条包边 mesh）。
///
/// **资产句柄一并缓存**（2026-10-20 实机卡顿修）：本模块初版每帧
/// `meshes.add()` + `materials.add()` 新建资产再 `insert` 替换——旧句柄
/// 永不回收，`Assets` 无限堆积 + 每帧 GPU 重传，圈选一片就明显变卡。
/// 现在 mesh 只在**格集合变化**时重建，材质只改 alpha（脉动）。
#[derive(Resource, Default)]
pub struct CellRegionEntities {
    /// 半透金色填充（所有选中格合成一个 mesh）。
    pub fill: Option<Entity>,
    /// 外包矩形 4 条边（下/上/左/右，屏幕恒定 2px 世界厚度）。
    pub edges: [Option<Entity>; 4],
    /// 填充 mesh 句柄（复用）。
    pub fill_mesh: Option<Handle<Mesh>>,
    /// 填充材质句柄（复用；alpha 每帧改）。
    pub fill_material: Option<Handle<ColorMaterial>>,
    /// 四条边的 mesh 句柄（复用）。
    pub edge_meshes: [Option<Handle<Mesh>>; 4],
    /// 四条边的材质句柄（复用）。
    pub edge_materials: [Option<Handle<ColorMaterial>>; 4],
    /// 上次构建时的格集合指纹——变了才重建 mesh。
    pub last_fingerprint: u64,
}

impl CellRegionEntities {
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
        for slot in &mut self.edges {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }

    /// 释放已缓存的 mesh 资产（`Assets` 不会随实体 despawn 回收）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        if let Some(h) = self.fill_mesh.take() {
            meshes.remove(&h);
        }
        for slot in &mut self.edge_meshes {
            if let Some(h) = slot.take() {
                meshes.remove(&h);
            }
        }
        self.last_fingerprint = 0;
    }
}

/// 格集合指纹（用于「集合变了才重建 mesh」）。
pub fn cells_fingerprint(cells: &[(i32, i32)]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (gx, gy) in cells {
        for v in [*gx as u64, *gy as u64] {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 由格集合构建**合并四边形 mesh**（单个 Mesh，N 格 = 4N 顶点）。
///
/// 顶点坐标 = 相对 `origin` 的局部偏移；mesh 实体 Transform 平移到 `origin`。
/// 返回 `(mesh, origin, bbox_size)`；空集返 `None`。
pub fn build_region_fill_mesh(
    cells: &[(i32, i32)],
    grid: &GridConfig,
) -> Option<(Mesh, Vec2, Vec2)> {
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
        let cells = rasterize_polygon_cells(&poly, Vec2::ZERO, &g, 8, 8);
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
        let cells = rasterize_polygon_cells(&poly, Vec2::ZERO, &g, 4, 4);
        assert_eq!(cells.len(), 16, "4×4 全选中");
    }

    /// **content_origin ≠ 0 的坐标链回归**（2026-10-20 实机 bug）：
    /// 选区锚点存本图局部坐标，而栅格在 raster_origin 处——两者原点不同。
    /// 若漏补 `content_origin`，三角形会整体错位（实测：圈住一个小岛却
    /// 选出 21974 格）。本例：栅格在 (100, 200)，多边形局部坐标围住栅格的
    /// (2,2)-(4,4) → 世界坐标 = content_origin + local。
    #[test]
    fn rasterize_polygon_respects_content_origin_offset() {
        // 栅格原点 (100, 200)，cell 10；多边形局部坐标围住栅格格 (2,2)-(4,4)
        // 的世界区域 [120,150)² → 局部 = 世界 − content_origin(50, 60)
        let g = grid(10.0, 100.0, 200.0);
        let content_origin = Vec2::new(50.0, 60.0);
        // 栅格格 (2..4, 2..4) 的世界区域 = x[120,150) y[220,250)
        //   → 局部 = 世界 − content_origin(50,60) = x[70,100) y[160,190)
        let poly = [(71.0, 161.0), (99.0, 161.0), (99.0, 189.0), (71.0, 189.0)];
        let cells = rasterize_polygon_cells(&poly, content_origin, &g, 8, 8);
        assert_eq!(cells.len(), 9, "3×3 方阵(栅格格 2..4)");
        for gy in 2..=4 {
            for gx in 2..=4 {
                assert!(cells.contains(&(gx, gy)), "缺格 ({gx},{gy})");
            }
        }
    }

    /// 退化输入（<3 顶点 / cell_size=0）返空，不 panic。
    #[test]
    fn rasterize_polygon_rejects_degenerate_input() {
        let g = grid(0.0, 0.0, 0.0);
        assert!(
            rasterize_polygon_cells(&[(0.0, 0.0), (1.0, 1.0)], Vec2::ZERO, &g, 4, 4).is_empty()
        );
        let g = grid(10.0, 0.0, 0.0);
        assert!(
            rasterize_polygon_cells(&[(0.0, 0.0), (1.0, 1.0)], Vec2::ZERO, &g, 4, 4).is_empty()
        );
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
