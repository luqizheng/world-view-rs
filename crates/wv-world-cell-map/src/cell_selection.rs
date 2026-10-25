//! 统一选区（`CellSelection`，ADR-0057）的高亮视觉件。
//!
//! ## 职责
//! - **面**：选中格的金色半透填充（脉动），合并 mesh（[`CellRegionEntities`]）。
//! - **形**：选区外围边界的白色虚线**蚂蚁线**（[`CellAntsEntities`] +
//!   [`build_ants_meshes`]，两相位 mesh 计时互换，ADR-0057 D8）。
//! - **hover 准星**：[`CellHoverEntities`]（瞬时态，双色描边 + 十字）。
//!
//! ## 边界（不做什么）
//! - **不吃输入**——选区由 wv-app 的选择工具（S/R/V/W）写入
//!   `CellSelection`；本模块只画。
//! - **不做世界写入**——纯渲染产物。
//!
//! ## 退役（ADR-0057）
//! - `CellSelectionEntities`（单格金框 outline）——单格 = 1 格选区，由
//!   蚂蚁线 + 填充统一覆盖；
//! - `CellRegionEntities` 的外包矩形四边——由蚂蚁线取代（D8）。
//!
//! ## 几何
//! 格 `(gx, gy)` 的世界矩形 = `raster_origin + (gx·cs, gy·cs)` 起、
//! 边长 `cs = cell_size` 的正方形（与 `cell_pick::pick_at` 反查同一坐标链，
//! 保证「光标压的格」与「高亮的格」逐格一致）。
//!
//! ## 主要导出
//! - `build_region_fill_mesh` / `build_ants_meshes` — mesh 构建纯函数（测试锚）。
//! - `cell_world_rect` — 格 → (中心, 尺寸) 纯函数。

use bevy::prelude::*;

use crate::state::{dash_segments, BoundaryEdge, DashPhase, EdgeOrientation, GridConfig};

/// Z 偏移：与笔刷光标同层（顶层会话带，[MAP_LAYER_RENDERING_SPEC §7]）。
/// ADR-0057 后作为选区填充 / 蚂蚁线的 Z 序锚点（填充 ≈ 0.899，蚂蚁线
/// [`ANTS_Z`] 在其上、hover [`HOVER_Z`] 最顶）。
pub const SELECTION_Z: f32 = 0.9;

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
        ]
        .into_iter()
        .flatten()
        {
            meshes.remove(&h);
        }
        self.last_size = Vec2::ZERO;
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
// 待落笔触色块（[ADR-0052] D3 / §9.5 四态视觉分工）
// ---------------------------------------------------------------------------

/// 待落笔触填充 alpha（§9.5：0.26，不脉动）。
pub const STROKE_FILL_ALPHA: f32 = 0.26;
/// 待落笔触 Z 偏移：低于 selected（`SELECTION_Z = 0.9`）——
/// 笔触是**未提交**的暂态，选中才是已提交态（§9.5 绘制序：
/// region → stroke → selected → hover）。
pub const STROKE_Z: f32 = 0.85;

/// **待落笔触**的实体集合（连续绘制期间的「已采样未写入」格）。
///
/// 与单格选中的高亮（[`CellSelectionEntities`]）是**两套实体**——
/// 笔触可能覆盖几百格（合并成一个 mesh），单格选中只有 1 格（5 个实体）。
#[derive(Resource, Default)]
pub struct CellStrokeEntities {
    /// 合并后的填充 mesh 实体（所有待落格合成一个 quad 网格）。
    pub fill: Option<Entity>,
    /// 填充 mesh 句柄（格集合变化才重建）。
    pub fill_mesh: Option<Handle<Mesh>>,
    /// 填充材质句柄（红橙 `STROKE_PENDING` + alpha 0.26）。
    pub fill_material: Option<Handle<ColorMaterial>>,
    /// 上次构建时的格集合指纹——变了才重建 mesh。
    pub last_fingerprint: u64,
}

impl CellStrokeEntities {
    /// 全部 despawn（笔触结束 / 工具关闭时调用）。
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
    }

    /// 释放已缓存的 mesh 资产（`Assets` 不会随实体 despawn 回收）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        if let Some(h) = self.fill_mesh.take() {
            meshes.remove(&h);
        }
        self.last_fingerprint = 0;
    }
}

/// 由格集合构建**合并四边形 mesh**（单个 Mesh，N 格 = 4N 顶点）。
///
/// 与 [`build_region_fill_mesh`] 同款结构（复用同一套顶点拼装逻辑），
/// 差别只在用途：region 画金底（已提交选区），stroke 画红橙底（待落笔触）。
pub fn build_stroke_fill_mesh(
    cells: &[(i32, i32)],
    grid: &GridConfig,
) -> Option<(Mesh, Vec2, Vec2)> {
    build_region_fill_mesh(cells, grid)
}

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

/// 统一选区的**面**高亮实体（ADR-0057 D8：稳态 1 填充 mesh；外包矩形四边
/// 已退役，边界视觉 = 蚂蚁线 [`CellAntsEntities`]）。
///
/// **资产句柄一并缓存**（2026-10-20 实机卡顿修）：本模块初版每帧
/// `meshes.add()` + `materials.add()` 新建资产再 `insert` 替换——旧句柄
/// 永不回收，`Assets` 无限堆积 + 每帧 GPU 重传，圈选一片就明显变卡。
/// 现在 mesh 只在**格集合变化**时重建，材质只改 alpha（脉动）。
#[derive(Resource, Default)]
pub struct CellRegionEntities {
    /// 半透金色填充（所有选中格合成一个 mesh）。
    pub fill: Option<Entity>,
    /// 填充 mesh 句柄（复用）。
    pub fill_mesh: Option<Handle<Mesh>>,
    /// 填充材质句柄（复用；alpha 每帧改）。
    pub fill_material: Option<Handle<ColorMaterial>>,
    /// 上次构建时的格集合指纹——变了才重建 mesh。
    pub last_fingerprint: u64,
}

impl CellRegionEntities {
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.fill.take() {
            commands.entity(e).despawn();
        }
    }

    /// 释放已缓存的 mesh 资产（`Assets` 不会随实体 despawn 回收）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        if let Some(h) = self.fill_mesh.take() {
            meshes.remove(&h);
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

// ---------------------------------------------------------------------------
// 蚂蚁线（[ADR-0057] D8：统一选区的边界视觉，两相位互换）
// ---------------------------------------------------------------------------

/// 虚线节距（屏幕像素）——实段 + 空段各半个周期，段长 = 节距。
pub const ANTS_PITCH_PX: f32 = 6.0;
/// 蚂蚁线线宽（屏幕像素；世界厚度 = `ANTS_THICKNESS_PX × view.scale`——
/// 屏幕像素恒定策略 A，与笔刷光标同口径）。
pub const ANTS_THICKNESS_PX: f32 = 2.0;
/// 相位翻转周期（秒）——用户裁定 ≈0.3 s 整组互换。
pub const ANTS_FLIP_SECS: f32 = 0.3;
/// 蚂蚁线 Z：在选区填充（`SELECTION_Z` = 0.9）之上、hover（0.95）之下。
pub const ANTS_Z: f32 = 0.903;

/// **蚂蚁线相位钟**：计时到点整组翻转（相位 A/B 互换可见性）。
///
/// 纯数据 + 纯函数，不碰 ECS——翻转动作（切 `Visibility`）由桥接系统执行。
/// 「选区不变时每帧零重建」的另一半（重建键）在 [`CellAntsEntities`]。
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct AntsPhaseClock {
    /// 距上次翻转累计的秒数。
    elapsed: f32,
    /// 当前可见的是否相位 A（true = A 实段可见 / B 隐藏）。
    show_a: bool,
}

impl Default for AntsPhaseClock {
    fn default() -> Self {
        Self {
            elapsed: 0.0,
            show_a: true,
        }
    }
}

impl AntsPhaseClock {
    /// 推进计时；到点翻转相位并返回 `true`（调用方据此切实体可见性）。
    ///
    /// 一次调用可能跨过多个周期（卡顿后 dt 很大）——按周期数取奇偶，
    /// 保证长时间挂起后相位仍确定，不错乱。
    pub fn tick(&mut self, dt: f32) -> bool {
        self.elapsed += dt.max(0.0);
        if self.elapsed < ANTS_FLIP_SECS {
            return false;
        }
        let periods = (self.elapsed / ANTS_FLIP_SECS).floor() as u64;
        self.elapsed -= periods as f32 * ANTS_FLIP_SECS;
        if periods % 2 == 1 {
            self.show_a = !self.show_a;
        }
        true
    }

    /// 当前应可见的相位。
    pub fn visible_phase(&self) -> DashPhase {
        if self.show_a {
            DashPhase::A
        } else {
            DashPhase::B
        }
    }
}

/// 蚂蚁线的实体集合（两套 mesh 相位 + 共用白材质）。
///
/// **重建键 = (选区指纹, 缩放档)**：选区内容变（指纹）或相机越过缩放档
/// （屏幕恒定节距/线宽按档心烘焙）才重建；其余帧零成本，只有
/// [`AntsPhaseClock`] 翻转可见性。
#[derive(Resource, Default)]
pub struct CellAntsEntities {
    /// 相位 A mesh 实体。
    pub phase_a: Option<Entity>,
    /// 相位 B mesh 实体。
    pub phase_b: Option<Entity>,
    /// 相位 A mesh 句柄。
    pub mesh_a: Option<Handle<Mesh>>,
    /// 相位 B mesh 句柄。
    pub mesh_b: Option<Handle<Mesh>>,
    /// 白色材质句柄（两相位共用；蚂蚁线 = 白虚线，ADR-0057 D8）。
    pub material: Option<Handle<ColorMaterial>>,
    /// 上次重建的键（选区指纹, 缩放档）。
    pub last_key: Option<(u64, i16)>,
    /// 相位钟。
    pub clock: AntsPhaseClock,
}

impl CellAntsEntities {
    /// 全部 despawn（gate 不满足时调用）。
    pub fn despawn_all(&mut self, commands: &mut Commands) {
        for slot in [&mut self.phase_a, &mut self.phase_b] {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }

    /// 释放已缓存的 mesh 资产，并**复位重建键**（否则「同键重来」会跳过
    /// 重建而句柄已空——2026-10-24 `CellSelectionEntities::release_assets`
    /// 同款教训）。
    pub fn release_assets(&mut self, meshes: &mut Assets<Mesh>) {
        for h in [self.mesh_a.take(), self.mesh_b.take()]
            .into_iter()
            .flatten()
        {
            meshes.remove(&h);
        }
        self.last_key = None;
    }
}

/// 由边界边集合构建**两相位蚂蚁线 mesh**（相位 A 实段一 mesh、相位 B 一 mesh）。
///
/// - 输入边为格坐标（[`crate::state::CellSelection::boundary_edges`] 产出）；
///   顶点直接烘成**世界坐标**（`raster_origin + 格坐标·cs`），实体 Transform
///   只抬 Z。
/// - 每条边独立按 `pitch_world` 切段（[`dash_segments`]），段索引奇偶分相位——
///   A/B 互补由构造保证，无需全局相位对齐。
/// - `thickness_world` 为线宽（沿边的法向两侧各半）。
///
/// 空边集 / 非正节距 / 非正线宽 → `None`（调用方整条视觉链退场）。
pub fn build_ants_meshes(
    edges: &[BoundaryEdge],
    grid: &GridConfig,
    pitch_world: f32,
    thickness_world: f32,
) -> Option<(Mesh, Mesh)> {
    if edges.is_empty() || pitch_world <= 0.0 || thickness_world <= 0.0 {
        return None;
    }
    let cs = grid.cell_size;
    if cs <= 0.0 {
        return None;
    }
    let o = grid.raster_origin;
    let half_t = thickness_world * 0.5;
    let mut pos_a: Vec<[f32; 3]> = Vec::new();
    let mut idx_a: Vec<u32> = Vec::new();
    let mut pos_b: Vec<[f32; 3]> = Vec::new();
    let mut idx_b: Vec<u32> = Vec::new();
    for e in edges {
        // 边起点（世界）与走向单位向量；格边世界长度恒 = cs。
        let (start, dir) = match e.orientation {
            EdgeOrientation::Horizontal => {
                (o + Vec2::new(e.gx as f32 * cs, e.gy as f32 * cs), Vec2::X)
            }
            EdgeOrientation::Vertical => {
                (o + Vec2::new(e.gx as f32 * cs, e.gy as f32 * cs), Vec2::Y)
            }
        };
        // 法向（左法线）× 半线宽——实段 quad 沿边两侧各让半宽。
        let normal = Vec2::new(-dir.y, dir.x) * half_t;
        for (seg_i, (s0, s1)) in dash_segments(cs, pitch_world).into_iter().enumerate() {
            let (positions, indices) = if DashPhase::A.is_solid(seg_i) {
                (&mut pos_a, &mut idx_a)
            } else {
                (&mut pos_b, &mut idx_b)
            };
            let p0 = start + dir * s0;
            let p1 = start + dir * s1;
            let base = positions.len() as u32;
            positions.push([p0.x + normal.x, p0.y + normal.y, 0.0]);
            positions.push([p1.x + normal.x, p1.y + normal.y, 0.0]);
            positions.push([p1.x - normal.x, p1.y - normal.y, 0.0]);
            positions.push([p0.x - normal.x, p0.y - normal.y, 0.0]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    if pos_a.is_empty() && pos_b.is_empty() {
        return None;
    }
    let build = |positions: Vec<[f32; 3]>, indices: Vec<u32>| {
        let mut mesh = Mesh::new(
            bevy::render::render_resource::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD
                | bevy::asset::RenderAssetUsages::MAIN_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
        mesh
    };
    Some((build(pos_a, idx_a), build(pos_b, idx_b)))
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

    // ── release_assets 必复位重建键（2026-10-24 真机 panic 回归同款教训）──────

    /// 对照：`CellHoverEntities` 的不变式（防止将来被「优化」掉）。
    #[test]
    fn hover_release_assets_also_resets_last_size() {
        let mut e = CellHoverEntities {
            last_size: Vec2::splat(8.0),
            ..Default::default()
        };
        let mut meshes = Assets::<Mesh>::default();
        e.release_assets(&mut meshes);
        assert_eq!(e.last_size, Vec2::ZERO);
    }

    /// **`CellAntsEntities::release_assets` 必须复位 `last_key`**——
    /// 否则「同选区同缩放档重来」会跳过重建而 mesh 句柄已空（2026-10-24
    /// `CellSelectionEntities` 同款 panic 的教训迁移）。
    #[test]
    fn ants_release_assets_resets_rebuild_key() {
        let mut e = CellAntsEntities {
            last_key: Some((42, 3)),
            ..Default::default()
        };
        let mut meshes = Assets::<Mesh>::default();
        e.release_assets(&mut meshes);
        assert_eq!(e.last_key, None, "release 必须复位重建键");
        assert!(e.mesh_a.is_none() && e.mesh_b.is_none(), "句柄应已释放");
    }

    // ── 蚂蚁线 mesh 构建（ADR-0057 D8 / A3）──────────────────────────

    /// 单格 4 条边 → 两相位 mesh 都非空；**A 实段 + B 实段 = 全部段**
    /// （quad 数互补），且两 mesh 顶点不相等（几何上确实分家了）。
    #[test]
    fn ants_meshes_split_segments_complementarily() {
        let g = grid(10.0, 0.0, 0.0);
        let edges = [
            BoundaryEdge {
                orientation: EdgeOrientation::Horizontal,
                gx: 0,
                gy: 0,
            },
            BoundaryEdge {
                orientation: EdgeOrientation::Horizontal,
                gx: 0,
                gy: 1,
            },
            BoundaryEdge {
                orientation: EdgeOrientation::Vertical,
                gx: 0,
                gy: 0,
            },
            BoundaryEdge {
                orientation: EdgeOrientation::Vertical,
                gx: 1,
                gy: 0,
            },
        ];
        // pitch 2.5：cs=10 → 4 段/边（偶奇偶奇），A 2 段、B 2 段 × 4 边。
        let (ma, mb) = build_ants_meshes(&edges, &g, 2.5, 0.4).expect("非空");
        let qa = ma.count_vertices() / 4;
        let qb = mb.count_vertices() / 4;
        assert_eq!(qa, 8, "A 应得 8 个实段 quad，got {qa}");
        assert_eq!(qb, 8, "B 应得 8 个实段 quad，got {qb}");
        assert_eq!(qa + qb, 16, "A+B = 全部段（4 边 × 4 段）");
    }

    /// 段长不收在节距上时，末段截短——几何仍覆盖整条边（周长守恒抽样：
    /// A/B 实段长度之和 = 边长 × 边数）。
    #[test]
    fn ants_segments_cover_full_edge_length() {
        let g = grid(10.0, 0.0, 0.0);
        let edges = [BoundaryEdge {
            orientation: EdgeOrientation::Horizontal,
            gx: 0,
            gy: 0,
        }];
        // pitch 3：10/3 → 4 段，[0,3) [3,6) [6,9) [9,10)。
        let (ma, mb) = build_ants_meshes(&edges, &g, 3.0, 0.4).expect("非空");
        let len_of = |m: &Mesh| -> f32 {
            let pos = match m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
                bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v,
                other => panic!("unexpected vertex format: {other:?}"),
            };
            // 每个 quad 的第 0、1 顶点沿边方向——x 差即段长。
            pos.chunks_exact(4).map(|q| q[1][0] - q[0][0]).sum()
        };
        let total = len_of(&ma) + len_of(&mb);
        assert!(
            (total - 10.0).abs() < 1e-4,
            "实段总长应 = 边长 10，got {total}"
        );
    }

    /// 退化输入（空边 / 非正节距 / 非正线宽 / cs=0）→ None，不 panic。
    #[test]
    fn ants_meshes_reject_degenerate_input() {
        let g = grid(10.0, 0.0, 0.0);
        let edges = [BoundaryEdge {
            orientation: EdgeOrientation::Horizontal,
            gx: 0,
            gy: 0,
        }];
        assert!(build_ants_meshes(&[], &g, 2.5, 0.4).is_none());
        assert!(build_ants_meshes(&edges, &g, 0.0, 0.4).is_none());
        assert!(build_ants_meshes(&edges, &g, 2.5, 0.0).is_none());
        let g0 = grid(0.0, 0.0, 0.0);
        assert!(build_ants_meshes(&edges, &g0, 2.5, 0.4).is_none());
    }

    /// **A3 · 相位钟**：到点整组翻转；跨多周期按奇偶落定；负 dt 防御。
    #[test]
    fn ants_clock_flips_on_period_and_handles_long_frames() {
        let mut c = AntsPhaseClock::default();
        assert_eq!(c.visible_phase(), DashPhase::A, "初始相位 A");
        assert!(!c.tick(ANTS_FLIP_SECS - 0.01), "未到点不翻");
        assert!(c.tick(0.02), "到点翻转");
        assert_eq!(c.visible_phase(), DashPhase::B);
        // 一次跨 3 个周期（卡顿）：3 是奇数 → 再翻一次回 A。
        assert!(c.tick(ANTS_FLIP_SECS * 3.0));
        assert_eq!(c.visible_phase(), DashPhase::A, "奇数周期 → 净翻一次");
        // 跨 2 个周期：偶数 → 相位不变（但仍算到点，调用方重挂可见性）。
        assert!(c.tick(ANTS_FLIP_SECS * 2.0));
        assert_eq!(c.visible_phase(), DashPhase::A, "偶数周期 → 相位不变");
        assert!(!c.tick(-1.0), "负 dt 不得翻转");
    }
}
