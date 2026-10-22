//! 区域轮廓渲染（ADR-0045 追记四）——轮廓线框 + 底色覆盖。
//!
//! ## 为什么要有这个模块
//!
//! 区域实体（`Region`）此前**只有标签、没有轮廓**：作者物化一个自然区之后，
//! 地图上看不出它到底圈了哪块地——「改覆盖物」也就无从下手（改的是哪一片？）。
//! 本模块补上两条：
//!
//! 1. **轮廓线框**：区域多边形（外环 + 洞环）画成线，
//!    Z `REGION_OUTLINE_Z = 0.076`（地形装饰 0.075 之上 / 道路 0.09 之下）——
//!    作者划的边界要压在底图族之上才醒目，又不能压过道路。
//! 2. **底色覆盖**：区域**声明了覆盖物键** → 用该键的主题规则色画一层
//!    半透明色块（Z `REGION_FILL_Z = 0.064`，底图族内、道路之下）。
//!
//! ## 为什么底色覆盖走覆盖层而不是改烘焙
//!
//! 烘焙（`terrain_pixels`）是**全图重算或增量 patch** 两条路；若底色覆盖进
//! 烘焙，则「改一个区域的覆盖物键」要触发纹理重建 + 增量 patch 的失效传播
//! （脏区要按覆盖范围膨胀）。走覆盖层则：**改键即生效**（下一帧重画色块），
//! 纹理烘焙契约（局部重画 = 全图重画，逐字节一致）完全不受影响。
//!
//! ## 边界
//!
//! - **不写世界**：纯渲染投影（P7，AGENTS §3）；
//! - **不改数据**：区域多边形只读；洞环的扣减由三角化（earcut）负责；
//! - **不消费主题之外的规则**：色值来自 `ThemePack::region_overlays`
//!   （经 [`crate::region_overlay::RegionOverlayIndex`] 解析），主题没定义的键
//!   走缺失覆盖物标记（白底红斜线，ADR-0045 追记一），**不在这里**兜底。

#![cfg(feature = "app")]

use bevy::prelude::*;

use wv_core::model::region::Region;
use wv_core::Id;

use crate::faction_overlays::earcut_with_holes;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::region_overlay::RegionOverlayIndex;
use crate::state::{DrillState, GridConfig, ViewState, WorldProjection};
use crate::vector_layers::build_stroke_batch_mesh;

/// 区域轮廓线框 Z：地形装饰（0.075）之上、道路（0.09）之下。
pub const REGION_OUTLINE_Z: f32 = 0.076;

/// 区域底色覆盖 Z：雾霭（0.06）附近、地界晕（0.065）之下——属底图族。
pub const REGION_FILL_Z: f32 = 0.064;

/// 轮廓线宽（屏幕像素）。
const OUTLINE_STROKE_PX: f32 = 1.6;

/// 底色覆盖的不透明度（0..1）——**半透明**：底图的地形层次要透出来，
/// 覆盖层是「给这片地定个性」，不是「刷一层漆」。
const FILL_ALPHA: f32 = 0.28;

/// 候选区预览的**帧间输入**（ADR-0045 追记四）。
///
/// app 层每帧刷新（读光标格 → 查 biome 连通区 → 换算成视空间环），本模块的
/// 系统消费。独立 Resource：渲染层不做 UI 拾取（`cell_pick` 在 app 层），
/// 两边的职责靠这个小结构交接。
#[derive(Resource, Debug, Default, Clone)]
pub struct CandidateRegionInput {
    /// 候选区轮廓（**视空间**点列，闭合环；`None` = 光标不在地形上 / 无候选）。
    pub ring: Option<Vec<Vec2>>,
}

/// 区域轮廓的运行时资源（合批 mesh + 实体 + 材质句柄）。
#[derive(Resource, Default)]
pub struct RegionOutlinePool {
    /// 轮廓线框（所有区域的线段合批成一个 mesh）。
    outline: Option<OutlineBatch>,
    /// 底色覆盖（每个有色区域一个 mesh + 实体）。
    fills: Vec<FillBatch>,
    /// 轮廓线材质（固定色，**不随主题色板**——它是「作者划的边界」这一语义）。
    outline_material: Option<Handle<ColorMaterial>>,
    /// **候选区预览**（物化前光标所在 biome 连通区的高亮轮廓）。
    preview: Option<OutlineBatch>,
    /// 候选区预览材质（金色高亮，与选区/意图像一个语义族）。
    preview_material: Option<Handle<ColorMaterial>>,
    /// 底色覆盖材质缓存（色 → 句柄）：同一色的区域共用一个材质。
    fill_materials: Vec<([u8; 3], Handle<ColorMaterial>)>,
}

/// 一个合批线框。
struct OutlineBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: OutlineKey,
}

/// 线框的重建判据（内容指纹 + 世界线宽 + Z）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct OutlineKey {
    fingerprint: u64,
    world_thickness: f32,
    z: f32,
}

/// 一个底色覆盖块。
struct FillBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    fingerprint: u64,
    color: [u8; 3],
    region_id: Id,
}

impl RegionOutlinePool {
    /// 清空所有实体（图层关闭 / 换图时用）。
    fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(b) = self.outline.take() {
            commands.entity(b.entity).despawn();
        }
        if let Some(b) = self.preview.take() {
            commands.entity(b.entity).despawn();
        }
        for f in self.fills.drain(..) {
            commands.entity(f.entity).despawn();
        }
    }
}

/// 区域多边形 → **视空间**线段（外环闭合 + 每个洞环闭合）。
///
/// 坐标链：区域多边形是**图内局部 wu** → `GridConfig::map_to_view` 转视空间
/// （与 POI / 街道 / 标注同一条链）。
pub fn region_segments(region: &Region, grid: &GridConfig) -> Vec<(Vec2, Vec2)> {
    let mut segs = Vec::new();
    push_loop(&region.polygon, grid, &mut segs);
    for hole in &region.holes {
        push_loop(hole, grid, &mut segs);
    }
    segs
}

/// 一条闭合环 → 线段列。
fn push_loop(ring: &[f32], grid: &GridConfig, out: &mut Vec<(Vec2, Vec2)>) {
    let pts: Vec<Vec2> = ring
        .chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| grid.map_to_view(Vec2::new(p[0], p[1])))
        .collect();
    if pts.len() < 2 {
        return;
    }
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        if a != b {
            out.push((a, b));
        }
    }
}

/// 区域多边形 → **底色覆盖 mesh**（外环 − 洞，三角化）。
///
/// 顶点用**视空间坐标**（mesh 直接摆在原点，实体只带 Z）——与线框同坐标系，
/// 避免「mesh 局部坐标 + 实体位移」两套换算出错。
pub fn region_fill_mesh(region: &Region, grid: &GridConfig) -> Option<Mesh> {
    let outer: Vec<f32> = region
        .polygon
        .chunks(2)
        .filter(|p| p.len() == 2)
        .flat_map(|p| {
            let v = grid.map_to_view(Vec2::new(p[0], p[1]));
            [v.x, v.y]
        })
        .collect();
    if outer.len() < 6 {
        return None;
    }
    let holes: Vec<Vec<f32>> = region
        .holes
        .iter()
        .map(|h: &Vec<f32>| {
            h.chunks(2)
                .filter(|p| p.len() == 2)
                .flat_map(|p| {
                    let v = grid.map_to_view(Vec2::new(p[0], p[1]));
                    [v.x, v.y]
                })
                .collect()
        })
        .filter(|h: &Vec<f32>| h.len() >= 6)
        .collect();
    let hole_refs: Vec<&[f32]> = holes.iter().map(|h| h.as_slice()).collect();
    let tris = earcut_with_holes(&outer, &hole_refs);
    if tris.is_empty() {
        return None;
    }
    let positions: Vec<[f32; 3]> = tris.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    Some(mesh)
}

/// 区域轮廓 + 底色覆盖系统。
///
/// **图层可见性**：跟随 `terrain` 层（区域是底图族的地理事实）。
#[allow(clippy::too_many_arguments)]
pub fn sync_region_outlines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    index: Res<RegionOverlayIndex>,
    input: Res<crate::region_outline::CandidateRegionInput>,
    mut pool: ResMut<RegionOutlinePool>,
) {
    // 2026-10-22 内容矩阵：区域层从 TerrainDecor 拆到独立 pass——Schematic
    // 下装饰 ✓ 而区域 ✗，共享一格做不到（ADR-0051）。
    // CellMap（Topographic）：nature_overlay 放行 Nature 类（判定入口唯一）。
    let enabled = crate::region_overlay::region_layer_visible(
        view.mode,
        view.nature_overlay,
        wv_core::model::region::RegionKind::Nature,
        recipe.enabled(PassId::RegionLayers),
        layers.terrain.visible,
    );
    let Some(map_id) = drill.current().cloned() else {
        pool.despawn_all(&mut commands);
        return;
    };
    if !enabled {
        pool.despawn_all(&mut commands);
        return;
    }
    let in_cellmap = matches!(view.mode, crate::state::ViewMode::Topographic);
    let regions: Vec<&Region> = projection
        .world
        .regions
        .iter()
        .filter(|r| r.map == map_id && !r.polygon.is_empty())
        // CellMap 只显示 Nature 类（Area 属内容，永远藏）
        .filter(|r| !in_cellmap || matches!(r.kind, wv_core::model::region::RegionKind::Nature))
        .collect();
    if regions.is_empty() {
        pool.despawn_all(&mut commands);
        return;
    }

    // ---- 轮廓线框（所有区域合批）----
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for r in &regions {
        segs.extend(region_segments(r, &grid));
    }
    let fp = segments_fingerprint(&segs);
    let world_thickness = OUTLINE_STROKE_PX * view.scale;
    let key = OutlineKey {
        fingerprint: fp,
        world_thickness,
        z: REGION_OUTLINE_Z,
    };
    if pool.outline_material.is_none() {
        pool.outline_material = Some(materials.add(ColorMaterial::from_color(Color::srgba(
            0.21, 0.35, 0.48, 0.85,
        ))));
    }
    let outline_mat = pool.outline_material.clone().expect("刚建");
    match pool.outline.take() {
        None => {
            if !segs.is_empty() {
                let mesh = meshes.add(build_stroke_batch_mesh(&segs, world_thickness));
                let entity = commands
                    .spawn((
                        Mesh2d(mesh.clone()),
                        MeshMaterial2d(outline_mat),
                        Transform::from_xyz(0.0, 0.0, REGION_OUTLINE_Z),
                    ))
                    .id();
                pool.outline = Some(OutlineBatch { entity, mesh, key });
            }
        }
        Some(mut b) => {
            if b.key != key {
                let fresh = build_stroke_batch_mesh(&segs, world_thickness);
                if let Some(mut old) = meshes.get_mut(&b.mesh) {
                    *old = fresh;
                }
                b.key = key;
            }
            pool.outline = Some(b);
        }
    }

    // ---- 底色覆盖（每个有规则色的区域一块）----
    let wanted: Vec<(&Region, [u8; 3])> = regions
        .iter()
        .filter_map(|r| {
            let entry = crate::region_overlay::entry_of_region(&index, &r.id)?;
            let rule = entry.rule.as_ref()?;
            Some((*r, rule.color))
        })
        .collect();
    // 旧的覆盖块先回收，多余的再补（按 region id + 颜色比对）。
    let mut keep: Vec<FillBatch> = Vec::new();
    for f in pool.fills.drain(..) {
        if wanted
            .iter()
            .any(|(r, c)| r.id == f.region_id && *c == f.color)
        {
            keep.push(f);
        } else {
            commands.entity(f.entity).despawn();
        }
    }
    pool.fills = keep;
    for (r, color) in &wanted {
        let fp = region_fill_fingerprint(r, color);
        if let Some(existing) = pool
            .fills
            .iter_mut()
            .find(|f| f.region_id == r.id && f.color == *color)
        {
            if existing.fingerprint != fp {
                if let Some(mesh) = region_fill_mesh(r, &grid) {
                    if let Some(mut old) = meshes.get_mut(&existing.mesh) {
                        *old = mesh;
                    }
                }
                existing.fingerprint = fp;
            }
            continue;
        }
        let Some(mesh) = region_fill_mesh(r, &grid) else {
            continue;
        };
        let handle = meshes.add(mesh);
        let mat = fill_material(&mut pool.fill_materials, &mut materials, *color);
        let entity = commands
            .spawn((
                Mesh2d(handle.clone()),
                MeshMaterial2d(mat),
                Transform::from_xyz(0.0, 0.0, REGION_FILL_Z),
            ))
            .id();
        pool.fills.push(FillBatch {
            entity,
            mesh: handle,
            fingerprint: fp,
            color: *color,
            region_id: r.id.clone(),
        });
    }

    // ---- 候选区预览（物化前光标所在的连通区）----
    sync_candidate_preview(
        commands,
        meshes,
        materials,
        grid,
        view,
        input.ring.as_deref(),
        &mut pool,
    );
}

/// 候选区预览：光标所在 biome 连通区的**金色高亮轮廓**。
///
/// 物化前先看到「要圈的是哪块地」（ADR-0045 D4 的可视化面）。轮廓由调用方
/// 传入（渲染层不做连通区计算——那是 `wv_terrain::region::biome_region_at`
/// 的事，app 层按光标格查一次）。
fn sync_candidate_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    preview_ring: Option<&[Vec2]>,
    pool: &mut RegionOutlinePool,
) {
    let Some(ring) = preview_ring else {
        if let Some(b) = pool.preview.take() {
            commands.entity(b.entity).despawn();
        }
        return;
    };
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for i in 0..ring.len() {
        let a = ring[i];
        let b = ring[(i + 1) % ring.len()];
        if a != b {
            segs.push((a, b));
        }
    }
    let fp = segments_fingerprint(&segs);
    let thickness = CANDIDATE_STROKE_PX * view.scale;
    let key = OutlineKey {
        fingerprint: fp,
        world_thickness: thickness,
        z: CANDIDATE_Z,
    };
    if pool.preview_material.is_none() {
        pool.preview_material = Some(materials.add(ColorMaterial::from_color(Color::srgba(
            0.96, 0.78, 0.26, 0.9,
        ))));
    }
    let mat = pool.preview_material.clone().expect("刚建");
    match pool.preview.take() {
        None => {
            let mesh = meshes.add(build_stroke_batch_mesh(&segs, thickness));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(mat),
                    Transform::from_xyz(0.0, 0.0, CANDIDATE_Z),
                ))
                .id();
            pool.preview = Some(OutlineBatch { entity, mesh, key });
        }
        Some(mut b) => {
            if b.key != key {
                let fresh = build_stroke_batch_mesh(&segs, thickness);
                if let Some(mut old) = meshes.get_mut(&b.mesh) {
                    *old = fresh;
                }
                b.key = key;
            }
            pool.preview = Some(b);
        }
    }
    let _ = grid;
}

/// 候选区预览线宽（屏幕像素）——比已物化的轮廓粗一点、更醒目。
const CANDIDATE_STROKE_PX: f32 = 2.4;
/// 候选区预览 Z：与已物化轮廓同带（0.076），金色已足够区分「还没物化」。
const CANDIDATE_Z: f32 = 0.077;

/// 底色覆盖块的指纹（几何 + 颜色）。
fn region_fill_fingerprint(r: &Region, color: &[u8; 3]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in r.polygon.iter().map(|f| f.to_bits()) {
        h ^= u64::from(v);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for hole in &r.holes {
        for v in hole.iter().map(|f| f.to_bits()) {
            h ^= u64::from(v);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    for v in color {
        h ^= u64::from(*v);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 线段列指纹（与道路同款 FNV）。
fn segments_fingerprint(segments: &[(Vec2, Vec2)]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (a, b) in segments {
        for v in [a.x, a.y, b.x, b.y] {
            h ^= u64::from(v.to_bits());
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 底色覆盖材质（按色缓存——同色的区域共用一个材质句柄）。
fn fill_material(
    cache: &mut Vec<([u8; 3], Handle<ColorMaterial>)>,
    materials: &mut Assets<ColorMaterial>,
    color: [u8; 3],
) -> Handle<ColorMaterial> {
    if let Some((_, h)) = cache.iter().find(|(c, _)| *c == color) {
        return h.clone();
    }
    let handle = materials.add(ColorMaterial::from_color(Color::srgba(
        color[0] as f32 / 255.0,
        color[1] as f32 / 255.0,
        color[2] as f32 / 255.0,
        FILL_ALPHA,
    )));
    cache.push((color, handle.clone()));
    handle
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::model::region::RegionKind;

    fn region_with(polygon: Vec<f32>, holes: Vec<Vec<f32>>) -> Region {
        Region {
            id: Id::new_unchecked("nr_a"),
            name: "甲".into(),
            map: Id::new_unchecked("map_world"),
            polygon,
            holes,
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        }
    }

    /// 无位移的栅格配置（map_origin = 0）——视空间 = 局部 wu。
    fn grid() -> GridConfig {
        GridConfig {
            cell_size: 8.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        }
    }

    #[test]
    fn region_segments_close_the_outer_loop() {
        let r = region_with(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0], vec![]);
        let segs = region_segments(&r, &grid());
        assert_eq!(segs.len(), 4, "方形外环应出 4 段");
        // 闭合：每段的终点 = 下一段的起点。
        for i in 0..segs.len() {
            let end = segs[i].1;
            let next = segs[(i + 1) % segs.len()].0;
            assert_eq!(end, next, "第 {i} 段未闭合");
        }
    }

    #[test]
    fn region_segments_include_hole_loops() {
        let r = region_with(
            vec![0.0, 0.0, 6.0, 0.0, 6.0, 6.0, 0.0, 6.0],
            vec![vec![1.0, 1.0, 3.0, 1.0, 3.0, 3.0, 1.0, 3.0]],
        );
        let segs = region_segments(&r, &grid());
        assert_eq!(segs.len(), 8, "外环 4 段 + 洞环 4 段");
    }

    #[test]
    fn region_fill_mesh_area_excludes_the_hole() {
        let r = region_with(
            vec![0.0, 0.0, 6.0, 0.0, 6.0, 6.0, 0.0, 6.0],
            vec![vec![1.0, 1.0, 3.0, 1.0, 3.0, 3.0, 1.0, 3.0]],
        );
        let mesh = region_fill_mesh(&r, &grid()).expect("应能三角化");
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("位置属性");
        let area = triangle_signed_area(pos);
        // 外环 6×6 = 36，洞 2×2 = 4 → 净面积 32。
        assert!(
            (area - 32.0).abs() < 0.01,
            "净面积应为 36−4=32，实得 {area}"
        );
    }

    #[test]
    fn region_fill_mesh_without_hole_covers_the_whole_polygon() {
        let r = region_with(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0], vec![]);
        let mesh = region_fill_mesh(&r, &grid()).expect("应能三角化");
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).expect("位置属性");
        let area = triangle_signed_area(pos);
        assert!((area - 16.0).abs() < 0.01, "面积应为 16，实得 {area}");
    }

    #[test]
    fn degenerate_polygon_yields_no_fill_mesh() {
        let r = region_with(vec![0.0, 0.0, 1.0, 0.0], vec![]);
        assert!(
            region_fill_mesh(&r, &grid()).is_none(),
            "退化多边形不该出面"
        );
    }

    #[test]
    fn fill_fingerprint_changes_with_color_and_geometry() {
        let r = region_with(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0], vec![]);
        let base = region_fill_fingerprint(&r, &[1, 2, 3]);
        assert_ne!(
            base,
            region_fill_fingerprint(&r, &[9, 9, 9]),
            "换色应改指纹"
        );
        let moved = region_with(vec![0.0, 0.0, 5.0, 0.0, 5.0, 4.0], vec![]);
        assert_ne!(
            base,
            region_fill_fingerprint(&moved, &[1, 2, 3]),
            "改几何应改指纹"
        );
    }

    /// 三角网**有符号**面积（按三角形绕向求和）。
    ///
    /// 用有符号而非绝对值：洞桥接会产生**绕向相反**的零宽走廊三角形，
    /// 绝对值求和会把它们的面积也算进去（实测 32 → 44）。有符号求和是
    /// 三角网覆盖面积的正确度量。
    fn triangle_signed_area(pos: &bevy::mesh::VertexAttributeValues) -> f32 {
        let bevy::mesh::VertexAttributeValues::Float32x3(v) = pos else {
            panic!("应为 Float32x3");
        };
        let mut area = 0.0f32;
        for tri in v.chunks(3) {
            let (a, b, c) = (
                (tri[0][0], tri[0][1]),
                (tri[1][0], tri[1][1]),
                (tri[2][0], tri[2][1]),
            );
            area += ((b.0 - a.0) * (c.1 - a.1) - (c.0 - a.0) * (b.1 - a.1)) / 2.0;
        }
        area.abs()
    }
}
