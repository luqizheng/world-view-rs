//! 地形装饰（terrain decor，ADR-0031）——按栅格群系采样的程序化符号覆盖层。
//!
//! ## 职责
//! - 从**活动地形栅格**（`active_terrain`）按视口采样格子，把六类群系
//!   （森林 / 平原 / 丘陵 / 山地 / 雪原 / 沼泽）演绎成程序化符号
//!   （树冠团、草点、半圆丘、三角山 + 雪帽、白丘、芦苇）；
//! - 三档 LOD：Nation 档不画（色块承带）、Region 档粗步距、Edit 档细步距；
//! - 确定性：落点抖动用 splitmix64(格坐标 + kind)——同工程重开树不跳；
//! - 渲染走**六桶合批**（每类一个 mesh + 一个实体），可见集 / 尺寸 / Z
//!   变化才经 `Assets::get_mut` 原地改写顶点（ADR-0030 D4 同纪律）。
//!
//! ## 与 `feature_overlays`（主题地界覆盖物）的边界
//!
//! 那边是 **Place.biome 标记地界驱动**（作者声明的标记边界长树，ADR-0032 后唯一词汇）；这边是
//! **地形格驱动**（栅格群系本身长装饰）——不依赖任何 `Place`，纯地形投影。
//!
//! ## 边界（不做什么）
//! - **不写时间比较 / 不写世界**（AGENTS §3）；
//! - **不引入位图瓦片**（JRPG 图集已弃用；全部程序化三角化）；
//! - 符号颜色是 v1 常量（每类一枚「墨色」），主题化随 ThemePack 演绎规则后继
//!   （README §12-30 同款延期）。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{lod_tier, GridConfig, ViewState, WorldProjection};
use crate::terrain_view::terrain_resolve::active_terrain;
use wv_terrain::biome::{
    BIOME_FOREST, BIOME_GRASSLAND, BIOME_HILLS, BIOME_JUNGLE, BIOME_MOUNTAINS, BIOME_OASIS,
    BIOME_PLAINS, BIOME_SAVANNA, BIOME_SNOWFIELD, BIOME_SWAMP, BIOME_TUNDRA, BIOME_WETLAND,
};

/// 装饰 Z：地形族之上（等值线 0.04 / 雾霭 0.06 / 地界符号 0.07）、道路（0.09）之下。
const DECOR_Z: f32 = 0.075;
/// 桶间 Z 微差（六桶按序号错开，防同深度闪烁）。
const BUCKET_Z_STEP: f32 = 0.0004;
/// Edit 档采样步距（屏幕像素）：树冠团间隔的观感基准。
const SPACING_PX_EDIT: f32 = 26.0;
/// Region 档采样步距（屏幕像素）：更疏，避免省级视野糊成噪点。
const SPACING_PX_REGION: f32 = 46.0;
/// 单类单帧符号组数上限（步距本身已把数量压到屏幕量级，此为兜底）。
const MAX_SITES_PER_KIND: usize = 2200;
/// 视口外扩余量（屏幕像素）：拖动相机时边缘符号不闪断。
const VIEWPORT_MARGIN_PX: f32 = 32.0;

/// 装饰类别（六类）。下标即渲染桶序号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorKind {
    Forest = 0,
    Plains = 1,
    Hills = 2,
    Mountain = 3,
    Snow = 4,
    Marsh = 5,
}

impl DecorKind {
    /// 全部类别（按桶序号）。
    pub const ALL: [DecorKind; 6] = [
        DecorKind::Forest,
        DecorKind::Plains,
        DecorKind::Hills,
        DecorKind::Mountain,
        DecorKind::Snow,
        DecorKind::Marsh,
    ];

    /// 类别的符号墨色（sRGB 字节 + alpha）。v1 常量；主题化后继。
    fn ink(self) -> ([u8; 3], f32) {
        match self {
            DecorKind::Forest => ([0x2B, 0x57, 0x31], 0.85),
            DecorKind::Plains => ([0x6B, 0x8F, 0x4E], 0.55),
            DecorKind::Hills => ([0x8A, 0x74, 0x44], 0.60),
            DecorKind::Mountain => ([0x4C, 0x44, 0x3A], 0.65),
            DecorKind::Snow => ([0xFD, 0xFE, 0xFF], 0.90),
            DecorKind::Marsh => ([0x4E, 0x6B, 0x5A], 0.65),
        }
    }
}

/// 栅格群系 → 装饰类别。水系 / 荒漠 / 裸地不演绎（`None`）。
///
/// 投影唯一入口；与 `wv-terrain::biome::display_kind` 的词汇表同源。
pub fn decor_kind_for_biome(biome: u8) -> Option<DecorKind> {
    match biome {
        BIOME_FOREST | BIOME_JUNGLE | 11 => Some(DecorKind::Forest), // 11 = TROPICAL_RAINFOREST（wv-terrain 未导出常量，词表锚定）
        BIOME_PLAINS | BIOME_GRASSLAND | BIOME_SAVANNA | BIOME_OASIS => Some(DecorKind::Plains),
        BIOME_HILLS => Some(DecorKind::Hills),
        BIOME_MOUNTAINS => Some(DecorKind::Mountain),
        BIOME_SNOWFIELD | BIOME_TUNDRA => Some(DecorKind::Snow),
        BIOME_SWAMP | BIOME_WETLAND => Some(DecorKind::Marsh),
        // 水系（14/15/16）与荒漠（4/5/19）不画装饰：海面有水彩带、沙漠留纯色。
        _ => None,
    }
}

/// 装饰 LOD 档：Nation 不画（色块承带），Region 粗 / Edit 细。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorBand {
    Region,
    Edit,
}

/// 缩放 → 装饰档。`None` = 本帧不画（Nation 档）。
pub fn decor_band(view_scale: f32) -> Option<DecorBand> {
    match lod_tier(view_scale) {
        crate::state::LodTier::Nation => None,
        crate::state::LodTier::Region => Some(DecorBand::Region),
        crate::state::LodTier::Edit => Some(DecorBand::Edit),
    }
}

impl DecorBand {
    /// 采样步距（屏幕像素）。
    fn spacing_px(self) -> f32 {
        match self {
            DecorBand::Region => SPACING_PX_REGION,
            DecorBand::Edit => SPACING_PX_EDIT,
        }
    }

    /// 符号尺寸系数（Region 档稍小一号，让「放大出现更多细节」有层次）。
    fn size_mul(self) -> f32 {
        match self {
            DecorBand::Region => 0.8,
            DecorBand::Edit => 1.0,
        }
    }
}

/// splitmix64（与 `theme::overlays` 同族）：落点抖动 / 变体选择的确定性来源。
fn splitmix64(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn hash01(x: i64, y: i64, seed: u64) -> f32 {
    let h = splitmix64(
        (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ seed,
    );
    (h >> 11) as f32 / (1u64 << 53) as f32
}

/// 一个采样点的装饰符号集合：`(|dx|, |dy|, r|半宽, 半高|, 桶)` 三角形列表。
///
/// 坐标是**相对格中心的屏幕像素**（调用方乘 `view.scale` 转世界单位）。
/// 输出 `(dx, dy, half_w, half_h)` 四元组——都是轴对齐缩放后的三角形顶点
/// 由 [`build_bucket_mesh`] 统一三角化；这里只产出**符号图元参数**。
///
/// v1 用「相对偏移 + 半径」的圆 / 半圆 / 三角参数化，避免在每个采样点手写
/// 顶点循环：三角化集中在 mesh 构建一处（改形状只动一处）。
pub fn site_primitives(
    kind: DecorKind,
    band: DecorBand,
    cx: i64,
    cy: i64,
    height_m: i16,
) -> Vec<SitePrim> {
    let mut out = Vec::new();
    let m = band.size_mul();
    let r1 = hash01(cx, cy, 1);
    let r2 = hash01(cx, cy, 2);
    let r3 = hash01(cx, cy, 3);
    match kind {
        DecorKind::Forest => {
            // 树冠团：3 颗小圆（径 2.6px），确定性散布在 ±5px 内。
            for s in 0..3u64 {
                let jx = (hash01(cx, cy, 10 + s) - 0.5) * 10.0;
                let jy = (hash01(cx, cy, 20 + s) - 0.5) * 8.0;
                out.push(SitePrim::circle(kind, jx * m, jy * m, 2.6 * m));
            }
        }
        DecorKind::Plains => {
            // 草点：单颗 1.2px 微圆，稀疏感靠步距。
            out.push(SitePrim::circle(
                kind,
                (r1 - 0.5) * 6.0 * m,
                (r2 - 0.5) * 6.0 * m,
                1.2 * m,
            ));
        }
        DecorKind::Hills => {
            // 半圆丘：宽 9px 高 5px 的半圆（底边贴格中心下方）。
            out.push(SitePrim::half_disc(
                kind,
                (r1 - 0.5) * 4.0 * m,
                1.0 * m,
                4.5 * m,
                5.0 * m,
            ));
        }
        DecorKind::Mountain => {
            // 三角山：宽 13px 高 11px；海拔 ≥ 1800m 顶部叠雪帽（小三角，雪桶）。
            out.push(SitePrim::triangle(
                kind,
                (r1 - 0.5) * 5.0 * m,
                2.0 * m,
                6.5 * m,
                11.0 * m,
            ));
            if height_m >= 1800 {
                out.push(SitePrim::triangle(
                    DecorKind::Snow,
                    0.0,
                    4.5 * m,
                    2.6 * m,
                    3.5 * m,
                ));
            }
        }
        DecorKind::Snow => {
            // 雪原白丘：宽 10px 高 4px 的扁半圆。
            out.push(SitePrim::half_disc(
                kind,
                (r1 - 0.5) * 5.0 * m,
                1.0 * m,
                5.0 * m,
                4.0 * m,
            ));
        }
        DecorKind::Marsh => {
            // 芦苇：2 根细三角（高 6px）+ 底部水线意象由半圆替代。
            let ox = (r1 - 0.5) * 6.0 * m;
            out.push(SitePrim::triangle(
                kind,
                ox - 2.0 * m,
                0.0,
                0.6 * m,
                6.0 * m,
            ));
            out.push(SitePrim::triangle(
                kind,
                ox + 2.0 * m,
                0.0,
                0.6 * m,
                5.0 * m,
            ));
            let _ = (r2, r3);
        }
    }
    out
}

/// 一个符号图元：形状 + 相对格中心的屏幕像素偏移 / 半径（调用方统一换算）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SitePrim {
    pub kind: PrimShape,
    pub dx: f32,
    pub dy: f32,
    pub half_w: f32,
    pub half_h: f32,
    /// 专属桶（山雪帽跨桶进 Snow）。
    pub bucket: DecorKind,
}

impl SitePrim {
    fn circle(bucket: DecorKind, dx: f32, dy: f32, r: f32) -> Self {
        Self {
            kind: PrimShape::Circle,
            dx,
            dy,
            half_w: r,
            half_h: r,
            bucket,
        }
    }

    fn half_disc(bucket: DecorKind, dx: f32, dy: f32, half_w: f32, half_h: f32) -> Self {
        Self {
            kind: PrimShape::HalfDisc,
            dx,
            dy,
            half_w,
            half_h,
            bucket,
        }
    }

    fn triangle(bucket: DecorKind, dx: f32, dy: f32, half_w: f32, half_h: f32) -> Self {
        Self {
            kind: PrimShape::Triangle,
            dx,
            dy,
            half_w,
            half_h,
            bucket,
        }
    }
}

/// 符号形状（三角化口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimShape {
    Circle,
    HalfDisc,
    Triangle,
}

/// 生成一个格采样点的三角形顶点 / 索引，写入对应桶（世界坐标）。
pub fn push_site_tris(
    site: &SitePrim,
    cx: f32,
    cy: f32,
    px_to_wu: f32,
    verts: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
) {
    // 顶点基址。
    let base = verts.len() as u32;
    let x = cx + site.dx * px_to_wu;
    let y = cy + site.dy * px_to_wu;
    let hw = site.half_w * px_to_wu;
    let hh = site.half_h * px_to_wu;
    match site.kind {
        PrimShape::Circle => {
            verts.push([x, y, 0.0]);
            for s in 0..POI_LIKE_SEGMENTS {
                let a = s as f32 / POI_LIKE_SEGMENTS as f32 * std::f32::consts::TAU;
                verts.push([x + hw * a.cos(), y + hh * a.sin(), 0.0]);
            }
            for s in 1..=POI_LIKE_SEGMENTS {
                let a = base + s as u32;
                let b = base + (s % POI_LIKE_SEGMENTS) as u32 + 1;
                indices.extend([base, a, b]);
            }
        }
        PrimShape::HalfDisc => {
            // 半圆：直径边贴 (x, y-hh)（底边在下，丘顶朝上）。
            verts.push([x - hw, y - hh, 0.0]);
            verts.push([x + hw, y - hh, 0.0]);
            for s in 0..=POI_LIKE_SEGMENTS {
                let a = std::f32::consts::PI * (s as f32 / POI_LIKE_SEGMENTS as f32);
                verts.push([x - hw * a.cos(), y - hh + hh * a.sin(), 0.0]);
            }
            for s in 0..POI_LIKE_SEGMENTS {
                let a = base;
                let b = base + 2 + s as u32;
                let c = base + 3 + s as u32;
                indices.extend([a, b, c]);
            }
        }
        PrimShape::Triangle => {
            verts.push([x - hw, y - hh, 0.0]);
            verts.push([x + hw, y - hh, 0.0]);
            verts.push([x, y + hh, 0.0]);
            indices.extend([base, base + 1, base + 2]);
        }
    }
}

/// 圆 / 扇区的离散段数（装饰符号 ≤14px，12 段与 POI 同观感）。
const POI_LIKE_SEGMENTS: usize = 12;

// ---------------------------------------------------------------------------
// 渲染侧：六桶合批池
// ---------------------------------------------------------------------------

/// 桶合批的重建判据：可见集指纹 + 符号尺寸 + Z。
#[derive(Debug, Clone, Copy, PartialEq)]
struct DecorBatchKey {
    fingerprint: u64,
    size: f32,
    z: f32,
}

/// 一个桶的实体与 Mesh 句柄。
struct DecorBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: DecorBatchKey,
}

/// 一个桶的池。
#[derive(Default)]
struct DecorBatchPool {
    batch: Option<DecorBatch>,
}

impl DecorBatchPool {
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

/// 六桶池 + 每桶材质句柄。
#[derive(Resource, Default)]
pub struct TerrainDecorPools {
    pools: [DecorBatchPool; 6],
    materials: Option<[Handle<ColorMaterial>; 6]>,
}

impl TerrainDecorPools {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for p in &mut self.pools {
            p.despawn(commands);
        }
    }
}

/// 保证六桶材质存在（惰性创建一次；颜色来自 [`DecorKind::ink`]）。
fn ensure_materials(
    pools: &mut TerrainDecorPools,
    materials: &mut Assets<ColorMaterial>,
) -> [Handle<ColorMaterial>; 6] {
    if let Some(handles) = &pools.materials {
        return handles.clone();
    }
    let handles = DecorKind::ALL.map(|k| {
        let (rgb, a) = k.ink();
        materials.add(ColorMaterial {
            color: Color::srgba(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
                a,
            ),
            ..default()
        })
    });
    pools.materials = Some(handles.clone());
    handles
}

/// 地形装饰主 system。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_decor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    mut pools: ResMut<TerrainDecorPools>,
) {
    let enabled = recipe.enabled(PassId::TerrainDecor) && layers.terrain.visible;
    if !enabled {
        pools.despawn_all(&mut commands);
        return;
    }
    // Nation 档不画：色块承带（ADR-0031 D3）。
    let Some(band) = decor_band(view.scale) else {
        pools.despawn_all(&mut commands);
        return;
    };
    let Some((_, data)) = active_terrain(&projection.world, drill.current()) else {
        pools.despawn_all(&mut commands);
        return;
    };
    if grid.cell_size <= 0.0 || data.width == 0 || data.height == 0 {
        return;
    }

    let handles = ensure_materials(&mut pools, &mut materials);

    // 视口矩形（视图空间）+ 外扩余量。
    let half = view.window_size * view.scale * 0.5 + Vec2::splat(VIEWPORT_MARGIN_PX * view.scale);
    let rect_min = view.camera_center - half;
    let rect_max = view.camera_center + half;

    // 栅格 → 视图：raster_origin + 格中心 × cell_size（与 terrain_chunks 同链）。
    let origin = grid.raster_origin;
    let cell = grid.cell_size;
    let px_to_wu = view.scale;

    // 步距（格）：由屏幕步距换算，至少 1。
    let stride = ((band.spacing_px() * view.scale) / cell).ceil().max(1.0) as i64;
    // 覆盖视口的格范围（钳到栅格）。
    let cx0 = (((rect_min.x - origin.x) / cell).floor() as i64 / stride) * stride;
    let cy0 = (((rect_min.y - origin.y) / cell).floor() as i64 / stride) * stride;
    let cx1 = (((rect_max.x - origin.x) / cell).ceil() as i64 + stride) / stride * stride;
    let cy1 = (((rect_max.y - origin.y) / cell).ceil() as i64 + stride) / stride * stride;

    // 六桶顶点 / 索引。
    let mut buckets: Vec<(Vec<[f32; 3]>, Vec<u32>)> =
        (0..6).map(|_| (Vec::new(), Vec::new())).collect();
    let mut site_counts = [0usize; 6];

    let mut cy = cy0;
    while cy <= cy1 {
        let gy = cy;
        let mut cx = cx0;
        while cx <= cx1 {
            if cx < 0 || gy < 0 || cx >= data.width as i64 || gy >= data.height as i64 {
                cx += stride;
                continue;
            }
            let idx = (gy as u32) * data.width + cx as u32;
            let Some(c) = data.cells.get(idx as usize) else {
                cx += stride;
                continue;
            };
            // 阈值抖动：部分格跳过，密度再降一档（森林 100%、平原 35%）。
            let kind = match decor_kind_for_biome(c.biome) {
                Some(k) => k,
                None => {
                    cx += stride;
                    continue;
                }
            };
            let keep_prob = match kind {
                DecorKind::Forest | DecorKind::Mountain | DecorKind::Snow | DecorKind::Marsh => 1.0,
                DecorKind::Plains | DecorKind::Hills => 0.4,
            };
            if hash01(cx, gy, 77) > keep_prob {
                cx += stride;
                continue;
            }
            let bucket_idx = kind as usize;
            if site_counts[bucket_idx] >= MAX_SITES_PER_KIND {
                cx += stride;
                continue;
            }
            site_counts[bucket_idx] += 1;
            let wx = origin.x + (cx as f32 + 0.5) * cell;
            let wy = origin.y + (gy as f32 + 0.5) * cell;
            for prim in site_primitives(kind, band, cx, gy, c.height) {
                let b = prim.bucket as usize;
                let (bv, bi) = &mut buckets[b];
                push_site_tris(&prim, wx, wy, px_to_wu, bv, bi);
            }
            cx += stride;
        }
        cy += stride;
    }
    // 同步六桶。
    let size_q = band.spacing_px() * view.scale * band.size_mul();
    for i in 0..6 {
        let (verts, indices) = &mut buckets[i];
        let geom = DecorGeom {
            verts: core::mem::take(verts),
            indices: core::mem::take(indices),
            size: size_q,
            z: DECOR_Z + i as f32 * BUCKET_Z_STEP,
        };
        sync_decor_bucket(
            &mut commands,
            &mut meshes,
            geom,
            &handles[i],
            &mut pools.pools[i],
        );
    }
}

/// 一帧的桶几何（顶点 / 索引 + 重建判据的尺寸 / Z 分量）。
struct DecorGeom {
    verts: Vec<[f32; 3]>,
    indices: Vec<u32>,
    size: f32,
    z: f32,
}

/// 单桶合批同步：指纹 / 尺寸 / Z 变化才原地改写顶点。
fn sync_decor_bucket(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    geom: DecorGeom,
    material: &Handle<ColorMaterial>,
    pool: &mut DecorBatchPool,
) {
    let DecorGeom {
        verts,
        indices,
        size,
        z,
    } = geom;
    let fingerprint = points_fingerprint(&verts);
    let key = DecorBatchKey {
        fingerprint,
        size,
        z,
    };
    match pool.batch.take() {
        None => {
            if verts.is_empty() {
                return;
            }
            let mesh = meshes.add(build_mesh(verts, indices));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0.0, 0.0, key.z),
                ))
                .id();
            pool.batch = Some(DecorBatch { entity, mesh, key });
        }
        Some(mut batch) => {
            if verts.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            if batch.key != key {
                if let Some(mut m) = meshes.get_mut(&batch.mesh) {
                    *m = build_mesh(verts, indices);
                }
                if batch.key.z != key.z {
                    commands
                        .entity(batch.entity)
                        .insert(Transform::from_xyz(0.0, 0.0, key.z));
                }
                batch.key = key;
            }
            commands
                .entity(batch.entity)
                .insert(MeshMaterial2d(material.clone()));
            pool.batch = Some(batch);
        }
    }
}

/// 顶点指纹（FNV-1a over f32 bits）——可见集或符号尺寸变化即换 key。
fn points_fingerprint(verts: &[[f32; 3]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for v in verts {
        for c in v {
            hash ^= u64::from(c.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn build_mesh(verts: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        verts,
    );
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::biome::{BIOME_DESERT, BIOME_GOBI, BIOME_OCEAN, BIOME_SALT_FLAT};

    #[test]
    fn decor_kind_maps_forest_and_rejects_water() {
        assert_eq!(decor_kind_for_biome(BIOME_FOREST), Some(DecorKind::Forest));
        assert_eq!(decor_kind_for_biome(BIOME_HILLS), Some(DecorKind::Hills));
        assert_eq!(
            decor_kind_for_biome(BIOME_MOUNTAINS),
            Some(DecorKind::Mountain)
        );
        assert_eq!(decor_kind_for_biome(BIOME_SNOWFIELD), Some(DecorKind::Snow));
        assert_eq!(decor_kind_for_biome(BIOME_SWAMP), Some(DecorKind::Marsh));
        assert_eq!(
            decor_kind_for_biome(BIOME_GRASSLAND),
            Some(DecorKind::Plains)
        );
        assert_eq!(decor_kind_for_biome(BIOME_OCEAN), None, "海面不画装饰");
        assert_eq!(decor_kind_for_biome(BIOME_DESERT), None, "沙漠留纯色");
        assert_eq!(decor_kind_for_biome(BIOME_GOBI), None);
        let _ = BIOME_SALT_FLAT;
    }

    #[test]
    fn nation_band_draws_nothing_region_and_edit_do() {
        assert_eq!(decor_band(5.0), None, "Nation 档不画");
        assert_eq!(decor_band(2.0), Some(DecorBand::Region));
        assert_eq!(decor_band(0.5), Some(DecorBand::Edit));
    }

    #[test]
    fn region_band_spacing_is_coarser_than_edit() {
        assert!(DecorBand::Region.spacing_px() > DecorBand::Edit.spacing_px());
        assert!(DecorBand::Region.size_mul() < DecorBand::Edit.size_mul());
    }

    #[test]
    fn site_primitives_are_deterministic() {
        let a = site_primitives(DecorKind::Forest, DecorBand::Edit, 100, 200, 500);
        let b = site_primitives(DecorKind::Forest, DecorBand::Edit, 100, 200, 500);
        assert_eq!(a, b, "同输入必须同符号（确定性）");
        let c = site_primitives(DecorKind::Forest, DecorBand::Edit, 101, 200, 500);
        assert_ne!(a, c, "不同格应有不同抖动");
    }

    #[test]
    fn mountains_above_snow_line_get_a_snow_cap_prim() {
        let high = site_primitives(DecorKind::Mountain, DecorBand::Edit, 5, 5, 2200);
        assert!(
            high.iter().any(|p| p.bucket == DecorKind::Snow),
            "2200m 山应有雪帽图元"
        );
        let low = site_primitives(DecorKind::Mountain, DecorBand::Edit, 5, 5, 800);
        assert!(
            low.iter().all(|p| p.bucket != DecorKind::Snow),
            "800m 山不应有雪帽"
        );
    }

    #[test]
    fn push_site_tris_circle_yields_fan_topology() {
        let prim = SitePrim::circle(DecorKind::Forest, 0.0, 0.0, 3.0);
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        push_site_tris(&prim, 0.0, 0.0, 1.0, &mut verts, &mut indices);
        assert_eq!(verts.len(), POI_LIKE_SEGMENTS + 1, "圆心 + 12 边");
        assert_eq!(indices.len(), POI_LIKE_SEGMENTS * 3, "12 扇区");
    }

    #[test]
    fn push_site_tris_half_disc_and_triangle_shapes() {
        let half = SitePrim::half_disc(DecorKind::Hills, 0.0, 0.0, 4.0, 5.0);
        let mut verts = Vec::new();
        let mut indices = Vec::new();
        push_site_tris(&half, 0.0, 0.0, 1.0, &mut verts, &mut indices);
        assert_eq!(verts.len(), 2 + POI_LIKE_SEGMENTS + 1);

        let tri = SitePrim::triangle(DecorKind::Mountain, 0.0, 0.0, 6.0, 11.0);
        let mut v2 = Vec::new();
        let mut i2 = Vec::new();
        push_site_tris(&tri, 0.0, 0.0, 1.0, &mut v2, &mut i2);
        assert_eq!(v2.len(), 3);
        assert_eq!(i2.len(), 3);
    }

    #[test]
    fn points_fingerprint_changes_with_content() {
        let a = vec![[0.0, 0.0, 0.0]];
        let b = vec![[1.0, 0.0, 0.0]];
        assert_ne!(points_fingerprint(&a), points_fingerprint(&b));
        assert_eq!(points_fingerprint(&a), points_fingerprint(&a));
    }

    /// 系统级集成：Edit 档生成装饰桶 mesh（森林 64² 全林网格）、二帧资产稳定、
    /// 拉到 Nation 档整组下线——ADR-0031 D2/D4 的端到端契约。
    #[test]
    fn decor_system_spawns_at_edit_and_clears_at_nation() {
        use crate::layer_frame::LayerFrame;
        use crate::recipe::{PassId, PassPlan, RenderRecipe};
        use crate::state::DrillState;
        use bevy::ecs::schedule::Schedule;
        use wv_core::terrain::{TerrainCellData, TerrainData};

        let mut world = wv_core::World::default();
        let map_id = wv_core::Id::new_unchecked("map_t");
        world.maps.push(wv_core::map::MapNode {
            id: map_id.clone(),
            ..Default::default()
        });
        // 64² 全林格（height 500 → 无雪帽）。
        let n = 64u32;
        let cells = vec![
            TerrainCellData {
                height: 500,
                surface: 1,
                biome: BIOME_FOREST,
            };
            (n * n) as usize
        ];
        world.terrains.insert(
            map_id.clone(),
            TerrainData {
                width: n,
                height: n,
                cells,
            },
        );

        let mut app = World::default();
        app.insert_resource(ViewState {
            scale: 0.5, // Edit 档
            camera_center: Vec2::splat(n as f32 * 4.0 / 2.0),
            window_size: Vec2::new(1280.0, 800.0),
            ..Default::default()
        });
        app.insert_resource(GridConfig {
            cell_size: 4.0,
            ..Default::default()
        });
        let mut drill = DrillState::default();
        drill.ensure_root(&world);
        app.insert_resource(drill);
        app.insert_resource(WorldProjection {
            world,
            ..Default::default()
        });
        app.insert_resource(LayerFrame::default());
        let mut recipe = RenderRecipe::default();
        recipe.passes[PassId::TerrainDecor as usize] = PassPlan::enabled();
        app.insert_resource(recipe);
        app.init_resource::<TerrainDecorPools>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_terrain_decor);

        schedule.run(&mut app);
        let meshes = app.resource::<Assets<Mesh>>();
        let first = meshes.iter().count();
        assert_eq!(
            first, 1,
            "Edit 档 64² 全林应生成 1 个森林桶 mesh，got {first}"
        );

        // 二帧：无变化 → 零新增资产（原地改写纪律）。
        schedule.run(&mut app);
        let second = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(second, first, "第二帧不应新增 mesh");

        // Nation 档：整组下线。
        app.resource_mut::<ViewState>().scale = 5.0;
        schedule.run(&mut app);
        let nation = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(nation, 0, "Nation 档装饰应全部清除，got {nation}");
    }
}
