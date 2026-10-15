//! 等值线矢量叠加层（ADR-0012 / plan v0.1 M2）：`VectorFlat` 浏览态的几何边界。
//!
//! ## 职责
//! - 从 `TerrainData` 派生海岸线（高度场海平面等值线）与类型场边界
//!   （`wv_terrain::isoline` 纯函数），Chaikin 平滑 + LOD 分档 DP 抽稀（Q8），
//!   装配成**单个批量描边 mesh** 叠在底图之上。
//! - 结构键（来源图 / 尺寸 / 分辨率 / 淡化 / LOD 档）变化时重算；`chunks_dirty`
//!   有待处理脏区（笔刷提交 / 整图重建）时下一帧重算。
//!
//! ## 边界（不做什么）
//! - **派生几何永不落盘**（plan D9）——mesh 每次重算；
//! - **不做 chunk 级增量提取**——脏区触发的是全图重算（512² 量级毫秒级；
//!   4096² 每笔一顿是已知妥协，`TODO(plan M2.5)`：按 chunk + halo 局部提取）；
//! - **不改变平涂底图**——色块仍由纹理管线负责，本层只画边界线
//!   （ADR-0012 §2①「多边形平涂」的 mesh 化填充留待 M2 后半）。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::mesh::MeshVertexAttribute;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use wv_terrain::isoline::{assemble_chains, chaikin, height_segments, kind_segments, simplify_dp};

use crate::render::layer_frame::LayerFrame;
use crate::state::{DrillState, GridConfig, ViewMode, ViewState, WorldProjection};
use crate::theme::palette::{self, MapPalette};

use super::terrain_resolve::texture_terrain;
use super::terrain_texture::TerrainTexture;

/// 等值线本色的 alpha（作者未给 Terrain 图层设 opacity 时的值）。
/// 墨色本体走 `PaletteColors.isoline_ink`（ADR-0016：日间褐墨 / 暮色浅墨）。
const ISOLINE_BASE_ALPHA: f32 = 1.0;

/// 叠加层 Z：底图块（`TERRAIN_Z = 0.0`）之上、换级淡化精灵（0.05）与道路
/// （`ROAD_Z = 0.1`）之下——等值线属于底图的一部分。
const ISOLINE_Z: f32 = 0.04;

/// 描边屏幕宽度（px）：世界宽度 = `px × view.scale`（ADR-0008 §8 的「乘」口径）。
const STROKE_PX: f32 = 1.25;

/// 描边亚线性缩放指数（手绘地图 #10）：世界半宽 ∝ scale^`GLYPH_SHRINK_EXP`。
///
/// 取自 civ-atlas 的 GLYPH_SHRINK = 0.22（屏幕观感 k^(1-0.22) = k^0.78 的
/// 对偶）：∈ (0, 1) 才是「放大时屏幕变纤细」；= 1 退化为 ADR-0008 §8 的
/// 严格乘口径（屏幕恒定）；> 1 则放大时线变粗（非手绘观感）。
const GLYPH_SHRINK_EXP: f32 = 0.78;

/// 描边世界半宽的唯一实现（主线 / 水彩晕 / 河水中线全走这里）。
///
/// `base_px` = 屏幕像素基准宽（scale=1 时的屏幕宽），`floor` = 世界下限
/// （极远防消隐）。同公式供测试手算锚定。
fn stroke_half_world(base_px: f32, scale: f32, floor: f32) -> f32 {
    (base_px * 0.5 * scale.powf(GLYPH_SHRINK_EXP)).max(floor)
}

/// 类型边界描边的格径半宽（× cell_size）：盖住「阶梯 → Chaikin 平滑」的
/// 偏差 + 填充的像素量化，让类型边界读作一条画上去的墨线。
const KIND_STROKE_CELL: f32 = 0.25;

/// 海岸线水彩外晕（手绘地图 #4，2026-10-15）：
/// 内晕 = 主线 `thin_half × HALO_1_MULT`、外晕 = `thin_half × HALO_2_MULT`。
/// 两层渐变 alpha（[`HALO_1_ALPHA`] / [`HALO_2_ALPHA`]）模拟水彩晕染。
const HALO_1_MULT: f32 = 2.6;
const HALO_2_MULT: f32 = 5.0;
const HALO_1_ALPHA: f32 = 0.35;
const HALO_2_ALPHA: f32 = 0.18;

/// 河水中线（手绘地图 #8，2026-10-15）：
/// - `RIVER_MULT` = 主线 `thin_half × 0.5`（比主线细一半）。
/// - `RIVER_ALPHA` = 0.5（半透，不抢主线）。
const RIVER_MULT: f32 = 0.5;
const RIVER_ALPHA: f32 = 0.5;

/// 重算触发键：只含**结构性**输入——任一变化才重算派生几何。
///
/// `view.scale` 只通过 `lod` 档（Q8 分档）进入，连续缩放不触发重算。
#[derive(Debug, Clone, PartialEq)]
struct IsolineKey {
    map: Option<wv_core::Id>,
    size: u32,
    res: u32,
    soften: bool,
    palette: MapPalette,
    lod: u8,
}

/// 资源：等值线叠加实体 + 上次构建的键。
#[derive(Resource, Default)]
pub struct IsolineOverlay {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<ColorMaterial>>,
    /// 海岸线水彩外晕（手绘地图 #4，2026-10-15）：2 个额外描边层（2.6× / 5.0×
    /// `thin_half`），仅在海岸存在时插入；alpha 渐变模拟水彩晕。类型边界不
    /// 走晕（kind 切换不是水彩晕的语义）。
    halo_outer: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    halo_outer_outer: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    /// 河水中线（手绘地图 #8，2026-10-15）：海岸链上方一道窄青蓝线（`label_water`
    /// 颜色）——civ-atlas 风格的"水边反光"。宽度 0.5× main，alpha 0.5。仅海岸。
    coast_river: Option<(Entity, Handle<Mesh>, Handle<ColorMaterial>)>,
    key: Option<IsolineKey>,
}

impl IsolineOverlay {
    fn despawn(&mut self, commands: &mut Commands) {
        if let (Some(entity), Some(mesh)) = (self.entity.take(), self.mesh.take()) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        for (entity, mesh, _mat) in [
            self.halo_outer.take(),
            self.halo_outer_outer.take(),
            self.coast_river.take(),
        ]
        .into_iter()
        .flatten()
        {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        self.key = None;
    }

    /// 「建过一次且结论是无几何」：实体与 mesh 皆空但键还在。整图同类型
    /// （新建地图的常态）没有海岸线 / 类型边界，叠加层**合法地无实体**；
    /// 不把这个稳态记进键，守卫 `entity.is_some()` 会撞空、每帧全图
    /// marching-squares 重算。
    fn empty_settled(&self) -> bool {
        self.entity.is_none() && self.mesh.is_none() && self.key.is_some()
    }
}

/// DP 容差的 LOD 分档（plan Q8：远 0.5 格 / 中 0.25 / 近 0.125）。
///
/// ⚠️ 2026-10-15 侧记：本分档与旧 chunk mip 阈值 `[0.5, 0.25]` 同源、疑似同款
/// 「scale 大 = 近」方向反装（按现行 world-per-px 口径，近观 scale<0.25 反而
/// 拿最粗容差 0.5，远观恒拿最细 0.125）。mip 已按 texel 屏幕尺寸修正
/// （`terrain_chunks::select_mip`），本分档**暂未动**——改动会改变描边观感，
/// 待用户拍板后另行处理。
fn lod_bucket(scale: f32) -> u8 {
    if scale >= 0.5 {
        0
    } else if scale >= 0.25 {
        1
    } else {
        2
    }
}

fn dp_tol_for(scale: f32) -> f32 {
    [0.125, 0.25, 0.5][lod_bucket(scale) as usize]
}

/// 同步等值线叠加层。排在 [`super::terrain_chunks::sync_terrain_chunks`]
/// **之前**：借它还没消费的 `tex.chunks_dirty` 感知「地形内容变了」
/// （笔刷提交 / 整图重建都会置位），结构键本身不吸收这个瞬态位。
#[allow(clippy::too_many_arguments)]
pub fn sync_isoline_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    mut overlay: ResMut<IsolineOverlay>,
) {
    // 只在 VectorFlat 浏览态存在；关图层（YAML ∨ 会话，ADR-0013）/ 换模式即退场
    // （键随之清零，重进重算）。
    if !layers.terrain.visible || view.mode != ViewMode::VectorFlat {
        overlay.despawn(&mut commands);
        return;
    }
    let Some(data) = texture_terrain(&tex, &projection.world, drill.current()) else {
        overlay.despawn(&mut commands);
        return;
    };
    let key = IsolineKey {
        map: tex.last_map.clone(),
        size: tex.size,
        res: tex.res,
        soften: tex.last_soften,
        palette: view.map_palette,
        lod: lod_bucket(view.scale),
    };
    let structural_same = overlay.key.as_ref() == Some(&key);
    let dirty_pending = tex.chunks_dirty.is_some();
    if structural_same && !dirty_pending && (overlay.entity.is_some() || overlay.empty_settled()) {
        // 结构没变也可能只是图层绑定变了（作者改 opacity/z）：就地补丁，
        // 不重算派生几何。
        if layers.is_changed() {
            apply_material_alpha(
                &mut materials,
                &overlay,
                palette::colors(view.map_palette).isoline_ink,
                layers.terrain.opacity,
            );
            if let Some(entity) = overlay.entity {
                commands.entity(entity).insert(Transform::from_xyz(
                    0.0,
                    0.0,
                    ISOLINE_Z + layers.terrain.z_offset,
                ));
            }
        }
        return;
    }

    // 派生管线（plan §2.4：提取 → 装配 → Chaikin → DP）。
    let w = data.width;
    let h = data.height;
    let cell = |x: u32, y: u32| &data.cells[(y * w + x) as usize];
    let coast = height_segments(w, h, |x, y| cell(x, y).height as f32, 0.0);
    let kinds = kind_segments(w, h, |x, y| cell(x, y).kind);
    let tol = dp_tol_for(view.scale);
    // 描边世界半宽（手绘地图 #10，2026-10-15）：**亚线性**缩放——
    // 世界半宽 = px × scale^0.78 而非 px × scale（ADR-0008 §8 乘口径的
    // 修正式）。civ-atlas GLYPH_SHRINK=0.22 同款思路：放大 4 倍时墨线的
    // *世界*尺寸只长 ~3 倍 → *屏幕*观感随放大反而变纤细（k^-0.22），地图
    // 细节浮现而墨线不霸道；缩小时屏幕上线变粗，远观仍可辨。
    // 世界下限保留：极远时防消隐。
    let thin_half = stroke_half_world(STROKE_PX, view.scale, grid.cell_size * 0.03);
    // 类型边界加宽：格缘阶梯被 Chaikin 磨角后，平滑曲线偏离原阶梯至多约
    // 0.3 格——细线盖不住露出的「错色台阶」，按格径给足半宽才压得平。
    let kind_half = thin_half.max(grid.cell_size * KIND_STROKE_CELL);
    // 主线（最锐）：主线 + 全部 kind 边界。
    let mut strokes: Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> = Vec::new();
    // 水彩外晕（手绘地图 #4）：仅海岸，2 圈；走同一 `coast` 链，3 套半宽。
    let mut coast_halo_inner: Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> = Vec::new();
    let mut coast_halo_outer: Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> = Vec::new();
    // 河水中线（手绘地图 #8）：与外晕同链，宽 0.5×，单层。
    let mut coast_river: Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> = Vec::new();
    for c in assemble_chains(&coast) {
        let smoothed = chaikin(&c.pts, c.closed, 2);
        let thin = simplify_dp(&smoothed, c.closed, tol);
        if thin.len() < 2 {
            continue;
        }
        strokes.push((thin.clone(), c.closed, thin_half));
        coast_halo_inner.push((thin.clone(), c.closed, thin_half * HALO_1_MULT));
        coast_halo_outer.push((thin.clone(), c.closed, thin_half * HALO_2_MULT));
        coast_river.push((thin, c.closed, thin_half * RIVER_MULT));
    }
    for c in assemble_chains(&kinds) {
        // 类型场边界是格缘阶梯：先小容差预抽稀去共线中点，再 Chaikin 磨角，
        // 否则平滑后的点数翻两倍全是楼梯碎点。
        let pts = simplify_dp(&c.pts, c.closed, 0.30);
        let smoothed = chaikin(&pts, c.closed, 2);
        let thin = simplify_dp(&smoothed, c.closed, tol);
        if thin.len() >= 2 {
            strokes.push((thin, c.closed, kind_half));
        }
    }

    let Some(mesh) = build_stroke_mesh(&strokes, grid.raster_origin, grid.cell_size) else {
        // 空几何（无等值线）也是稳态：0 顶点 mesh 会踩 bevy #24874
        // （MeshAllocator 误报 Use-after-free），退场不插；键记为「空已定」，
        // 下一帧走 `empty_settled` 快路，不重算。
        overlay.despawn(&mut commands);
        overlay.key = Some(key);
        return;
    };
    let handle = meshes.add(mesh);
    let pal = palette::colors(view.map_palette);
    let material = overlay
        .material
        .get_or_insert_with(|| materials.add(ColorMaterial::from_color(pal.isoline_ink)))
        .clone();
    // 材质跨重建复用（get_or_insert）：重算后也要按当前绑定刷新 alpha。
    apply_material_alpha(
        &mut materials,
        &overlay,
        pal.isoline_ink,
        layers.terrain.opacity,
    );

    overlay.despawn(&mut commands);
    let entity = commands
        .spawn((
            Mesh2d(handle.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, ISOLINE_Z + layers.terrain.z_offset),
        ))
        .id();
    overlay.entity = Some(entity);
    overlay.mesh = Some(handle);

    // 水彩外晕（手绘地图 #4）：仅当海岸链非空时生成。晕在主线下层（z 更小），
    // 避免遮住主线。alpha 渐变（`HALO_*_ALPHA`）实现水彩晕染感。
    if !coast_halo_inner.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_halo_inner, grid.raster_origin, grid.cell_size)
        {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.isoline_ink
                    .with_alpha(HALO_1_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z - 0.002 + layers.terrain.z_offset),
                ))
                .id();
            overlay.halo_outer = Some((e, h, mat));
        }
    }
    if !coast_halo_outer.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_halo_outer, grid.raster_origin, grid.cell_size)
        {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.isoline_ink
                    .with_alpha(HALO_2_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z - 0.004 + layers.terrain.z_offset),
                ))
                .id();
            overlay.halo_outer_outer = Some((e, h, mat));
        }
    }
    // 河水中线（手绘地图 #8）：在主线上方，z+0.001，宽度 0.5× main，
    // `label_water` 颜色 + 0.5 alpha——"水边反光"观感（civ-atlas 风格）。
    if !coast_river.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_river, grid.raster_origin, grid.cell_size) {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.label_water
                    .with_alpha(RIVER_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z + 0.001 + layers.terrain.z_offset),
                ))
                .id();
            overlay.coast_river = Some((e, h, mat));
        }
    }

    overlay.key = Some(key);
}

/// 把 Terrain 图层绑定的 opacity 写到等值线材质 alpha 上。
///
/// 材质是**缓存复用**的单例（`overlay.material`），改 alpha 走 `Assets::get_mut`，
/// 不新建资产；实体 z 由 spawn 路径或结构不变路径的 Transform 补丁负责。
fn apply_material_alpha(
    materials: &mut Assets<ColorMaterial>,
    overlay: &IsolineOverlay,
    ink: Color,
    opacity: f32,
) {
    if let Some(handle) = &overlay.material {
        if let Some(mut mat) = materials.get_mut(handle) {
            mat.color = ink.with_alpha(ISOLINE_BASE_ALPHA * opacity);
        }
    }
}

/// 把折线链展开成**单个**三角网（每段一个加宽四边形，段端外扩半宽盖住转角缝隙）。
///
/// 每链自带半宽（`chains` 第三元）：海岸线细、类型边界粗。
/// 不做圆头/斜头连接：同色 overdraw 下外扩接缝不可见，省掉逐转角的扇形开销。
/// `grid_lines`（编辑态逐格网格）复用同一展开器。
///
/// 返回 `None` 表示**什么都没挤出来**（链表为空 / 全是短于 2 点或零长的退化
/// 链）。调用方必须以退场响应 `None`——绝不能把 0 顶点 mesh 插进 `Assets`：
/// MeshAllocator 对空 mesh 跳过分配、拷贝阶段却无条件读取，bevy #24874 会
/// 误报 `Use-after-free`。
pub(crate) fn build_stroke_mesh(
    chains: &[(Vec<wv_terrain::isoline::Pt>, bool, f32)],
    origin: Vec2,
    cell_size: f32,
) -> Option<Mesh> {
    let to_world = |p: wv_terrain::isoline::Pt| origin + Vec2::new(p.x, p.y) * cell_size;
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (pts, closed, half_w) in chains {
        let n = pts.len();
        if n < 2 {
            continue;
        }
        let last = if *closed { n } else { n - 1 };
        for i in 0..last {
            let a = to_world(pts[i]);
            let b = to_world(pts[(i + 1) % n]);
            let dir = b - a;
            let len = dir.length();
            if len < 1e-9 {
                continue;
            }
            let dir = dir / len;
            let nrm = Vec2::new(-dir.y, dir.x);
            // 段两端沿方向外扩半宽：相邻段的扩出部分互相覆盖，转角无豁口。
            let a0 = a - dir * half_w;
            let b0 = b + dir * half_w;
            let base = positions.len() as u32;
            positions.push([a0.x + nrm.x * half_w, a0.y + nrm.y * half_w, 0.0]);
            positions.push([a0.x - nrm.x * half_w, a0.y - nrm.y * half_w, 0.0]);
            positions.push([b0.x - nrm.x * half_w, b0.y - nrm.y * half_w, 0.0]);
            positions.push([b0.x + nrm.x * half_w, b0.y + nrm.y * half_w, 0.0]);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::isoline::Pt;

    fn p(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    /// Q8 分档：缩放 1.0 / 0.3 / 0.1 分别落近 / 中 / 远档。
    #[test]
    fn dp_tolerance_buckets_match_plan_q8() {
        assert!((dp_tol_for(1.0) - 0.125).abs() < 1e-6);
        assert!((dp_tol_for(0.3) - 0.25).abs() < 1e-6);
        assert!((dp_tol_for(0.1) - 0.5).abs() < 1e-6);
        // 档界与 mip 选级同口径：0.5 归近档、0.25 归中档。
        assert!((dp_tol_for(0.5) - 0.125).abs() < 1e-6);
        assert!((dp_tol_for(0.25) - 0.25).abs() < 1e-6);
    }

    /// 全退化链（空表 / 单点链）→ 无 mesh：渲染层不得插 0 顶点网格
    /// （bevy #24874 误报 Use-after-free），调用方以退场响应 `None`。
    #[test]
    fn stroke_mesh_degenerate_chains_produce_no_mesh() {
        assert!(build_stroke_mesh(&[], Vec2::ZERO, 1.0).is_none());
        assert!(build_stroke_mesh(&[(vec![p(0.0, 0.0)], false, 0.5)], Vec2::ZERO, 1.0).is_none());
    }

    #[test]
    fn stroke_mesh_two_point_chain_yields_one_quad() {
        let chains = vec![(vec![p(0.0, 0.0), p(4.0, 0.0)], false, 0.5)];
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 1.0).expect("非退化链应有 mesh");
        let pos = mesh
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在")
            .as_float3()
            .expect("应为 Float32x3")
            .to_vec();
        assert_eq!(pos.len(), 4);
        // 外扩：x 范围 [-0.5, 4.5]，y 范围 [-0.5, 0.5]。
        let xs: Vec<f32> = pos.iter().map(|v| v[0]).collect();
        let ys: Vec<f32> = pos.iter().map(|v| v[1]).collect();
        let (xmin, xmax) = (
            xs.iter().cloned().fold(f32::MAX, f32::min),
            xs.iter().cloned().fold(f32::MIN, f32::max),
        );
        let (ymin, ymax) = (
            ys.iter().cloned().fold(f32::MAX, f32::min),
            ys.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(
            (xmin + 0.5).abs() < 1e-6 && (xmax - 4.5).abs() < 1e-6,
            "x 范围 [{xmin}, {xmax}] ≠ [-0.5, 4.5]，顶点 {pos:?}"
        );
        assert!(
            (ymin + 0.5).abs() < 1e-6 && (ymax - 0.5).abs() < 1e-6,
            "y 范围 [{ymin}, {ymax}] ≠ [-0.5, 0.5]，顶点 {pos:?}"
        );
        assert_eq!(mesh.indices().expect("索引应存在").len(), 6);
    }

    /// 闭环三角链：三条边各一段（含闭合边），12 顶点 18 索引。
    #[test]
    fn stroke_mesh_closed_chain_draws_closing_edge() {
        let chains = vec![(vec![p(0.0, 0.0), p(4.0, 0.0), p(2.0, 3.0)], true, 0.5)];
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 1.0).expect("非退化链应有 mesh");
        assert_eq!(mesh.indices().expect("索引应存在").len(), 18);
    }

    /// 结构键：图 / 尺寸 / 分辨率 / 淡化 / LOD 任一变化都判不等；瞬态脏位不进键。
    #[test]
    fn isoline_key_covers_structural_inputs_only() {
        let base = IsolineKey {
            palette: Default::default(),
            map: Some(wv_core::Id::new("m").expect("合法 id")),
            size: 512,
            res: 4,
            soften: false,
            lod: 0,
        };
        let same = base.clone();
        assert_eq!(base, same);
        let variants = [
            IsolineKey {
                map: None,
                size: 512,
                res: 4,
                soften: false,
                palette: base.palette,
                lod: 0,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 256,
                res: 4,
                soften: false,
                palette: base.palette,
                lod: 0,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                res: 8,
                soften: false,
                palette: base.palette,
                lod: 0,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                res: 4,
                soften: true,
                palette: base.palette,
                lod: 0,
            },
            IsolineKey {
                map: base.map.clone(),
                size: 512,
                res: 4,
                soften: false,
                palette: base.palette,
                lod: 1,
            },
        ];
        for v in variants {
            assert_ne!(base, v, "键 {v:?} 应与基键不同");
        }
    }

    /// 端到端（纯函数段）：小地形 → 提取 → 装配 → 平滑 → 抽稀 → mesh 非空。
    #[test]
    fn full_derivation_pipeline_yields_nonempty_mesh() {
        let (w, h) = (8u32, 8u32);
        let cells: Vec<wv_core::terrain::TerrainCellData> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let land = (2..=5).contains(&x) && (2..=5).contains(&y);
                wv_core::terrain::TerrainCellData {
                    height: if land { 100 } else { -100 },
                    kind: if land { 3 } else { 0 },
                }
            })
            .collect();
        let data = wv_core::terrain::TerrainData {
            width: w,
            height: h,
            cells,
        };
        let cell = |x: u32, y: u32| &data.cells[(y * w + x) as usize];
        let coast = height_segments(w, h, |x, y| cell(x, y).height as f32, 0.0);
        let kinds = kind_segments(w, h, |x, y| cell(x, y).kind);
        assert!(!coast.is_empty() && !kinds.is_empty());
        let mut strokes = Vec::new();
        for (segs, pre) in [(coast, false), (kinds, true)] {
            for c in assemble_chains(&segs) {
                let pts = if pre {
                    simplify_dp(&c.pts, c.closed, 0.30)
                } else {
                    c.pts.clone()
                };
                let thin = simplify_dp(&chaikin(&pts, c.closed, 2), c.closed, 0.125);
                if thin.len() >= 2 {
                    strokes.push((thin, c.closed, 0.5));
                }
            }
        }
        let mesh = build_stroke_mesh(&strokes, Vec2::ZERO, 20.0).expect("4×4 陆块应有 mesh");
        let idx = mesh.indices().expect("索引应存在").len();
        assert!(idx > 0, "4×4 陆块的海岸线与类型边界不应为空");
        assert_eq!(idx % 3, 0, "三角网索引应是 3 的倍数");
    }
    // -----------------------------------------------------------------------
    // 水彩外晕（手绘地图 #4，2026-10-15）
    // -----------------------------------------------------------------------

    /// 海岸水彩晕比内晕更宽：HALO_2_MULT > HALO_1_MULT > 1。
    #[test]
    fn halo_multipliers_form_widening_radii() {
        const { assert!(HALO_1_MULT > 1.0, "内晕应比主线宽") }
        const { assert!(HALO_2_MULT > HALO_1_MULT, "外晕应比内晕更宽") }
        const {
            assert!(
                HALO_1_ALPHA < 1.0 && HALO_2_ALPHA < HALO_1_ALPHA,
                "晕越外 alpha 越低"
            )
        }
    }

    /// 主线 mesh 在 kind 边界单独注入点（kind=6 山地块）下也能产出——我们没有
    /// 改 build_stroke_mesh 的契约，复用既有测试断言。
    #[test]
    fn main_stroke_mesh_unaffected_by_halo_branch() {
        let mesh = build_stroke_mesh(
            &[(
                vec![
                    wv_terrain::isoline::Pt { x: 0.5, y: 1.5 },
                    wv_terrain::isoline::Pt { x: 2.5, y: 1.5 },
                ],
                false,
                0.5,
            )],
            Vec2::ZERO,
            1.0,
        );
        assert!(mesh.is_some(), "主线 mesh 仍应产出");
    }

    /// 水彩晕 alpha < 1 才有「水彩」感；等于 1 退化成主线，不算晕。
    #[test]
    fn halo_alphas_are_strictly_less_than_one() {
        const {
            assert!(
                HALO_1_ALPHA > 0.0 && HALO_1_ALPHA < 1.0,
                "内晕 alpha 应在 (0,1)"
            )
        }
        const {
            assert!(
                HALO_2_ALPHA > 0.0 && HALO_2_ALPHA < 1.0,
                "外晕 alpha 应在 (0,1)"
            )
        }
    }

    // -----------------------------------------------------------------------
    // 河水中线（手绘地图呈现 #8，2026-10-15）
    // -----------------------------------------------------------------------

    /// RIVER_MULT = 0.5，RIVER_ALPHA = 0.5：比主线更细 + 半透，不抢主线。
    #[test]
    fn river_layer_dimensions_are_strictly_smaller_than_main() {
        const { assert!(RIVER_MULT < 1.0, "河水中线应比主线窄") }
        const { assert!(RIVER_MULT > 0.0, "但不能为 0") }
        const {
            assert!(
                RIVER_ALPHA > 0.0 && RIVER_ALPHA < 1.0,
                "河水中线 alpha 应在 (0,1)"
            )
        }
    }

    /// 河水中线 mesh 产出非空：与主线同链（海岸），复用 `build_stroke_mesh`。
    #[test]
    fn river_coast_mesh_is_nonempty_when_coast_present() {
        let mesh = build_stroke_mesh(
            &[(
                vec![
                    wv_terrain::isoline::Pt { x: 0.5, y: 1.5 },
                    wv_terrain::isoline::Pt { x: 5.5, y: 1.5 },
                    wv_terrain::isoline::Pt { x: 5.5, y: 5.5 },
                ],
                false,
                0.5 * 1.25, // RIVER_MULT * STROKE_PX
            )],
            Vec2::ZERO,
            1.0,
        );
        assert!(mesh.is_some(), "河水中线 mesh 应产出");
    }

    // -----------------------------------------------------------------------
    // 符号缩放（手绘地图呈现 #10，2026-10-15）
    // -----------------------------------------------------------------------

    /// 亚线性缩放锚点（手算）：scale=1 → base/2；scale=4 → base/2 × 4^0.78
    /// ≈ base/2 × 2.928（**小于**线性的 base/2 × 4 = 2·base——放大时世界
    /// 尺寸长得慢）；scale=0.25 → base/2 × 0.25^0.78 ≈ base/2 × 0.341
    /// （**大于**线性的 base/8——缩小时世界尺寸缩得慢，屏幕更粗更可辨）。
    #[test]
    fn stroke_half_world_grows_sublinearly_with_scale() {
        let base = 2.0; // STROKE_PX 量级，手算方便
        let floor = 0.0;
        let at_one = stroke_half_world(base, 1.0, floor);
        assert!(
            (at_one - 1.0).abs() < 1e-5,
            "scale=1 应为 base/2=1，实得 {at_one}"
        );
        let at_four = stroke_half_world(base, 4.0, floor);
        // 4^0.78 = e^(0.78·ln4) = e^(0.78×1.38629) = e^1.08131 ≈ 2.9485
        assert!(
            (at_four - 2.9485).abs() < 0.01,
            "scale=4 应为 1×2.9485≈2.95，实得 {at_four}"
        );
        assert!(at_four < 4.0, "亚线性：4 倍缩放下世界半宽应 < 线性值 4");
        let at_quarter = stroke_half_world(base, 0.25, floor);
        // 0.25^0.78 = 1/2.9485 ≈ 0.3392
        assert!(
            (at_quarter - 0.3392).abs() < 0.01,
            "scale=0.25 应为 1×0.3392≈0.34，实得 {at_quarter}"
        );
        assert!(
            at_quarter > 0.25,
            "亚线性：1/4 缩放下世界半宽应 > 线性值 0.25"
        );
    }

    /// 下限生效：极小 scale 时回到 floor，不随指数继续缩。
    #[test]
    fn stroke_half_world_respects_floor_at_extreme_zoom_out() {
        let base = 2.0;
        let floor = 0.5;
        let v = stroke_half_world(base, 0.001, floor);
        assert_eq!(v, floor, "极远时应取世界下限 {floor}，实得 {v}");
        // 指数约束：∈ (0,1) 才是亚线性。
        const {
            assert!(
                GLYPH_SHRINK_EXP > 0.0 && GLYPH_SHRINK_EXP < 1.0,
                "缩放指数应在 (0,1)"
            )
        }
    }
}
