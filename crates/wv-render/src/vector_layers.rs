//! Google 式矢量叠加层：道路 casing/fill + 红点白环 POI。
//!
//! ## 职责
//! - 在地形之上叠画 Google Maps 风格的视觉层：道路外壳（casing）+ 道路主体（fill），
//!   以及 POI 的红点白环 marker。
//! - 用 `PlaceDot` 组件驱动可见性 / LOD：在当前 zoom 下超界的点会随缩放增删，
//!   避免 `wgpu::Batch` 节点数随 POI 总数失控。
//!
//! ## 边界（不做什么）
//! - **不画地形底图**——那是 [`crate::terrain_view`] + [`crate::theme::palette`]。
//! - **不投影地理坐标**——只消费 [`crate::world_projection::ProjectionFrame`]。
//! - **不维护世界数据**——POI 经 [`crate::state::ViewState`] 当前地图与可见层过滤。
//!
//! ## 主要导出
//! - `VectorLayerEntities` — Resource，记录当前帧的 casing / fill / dot 实体。
//! - `setup_place_dots` / `render_place_dots` — POI 红点系统。
//! - `draw_streets_casing` / `draw_streets_fill` — 道路外壳 / 道路主体绘制系统。
//!
//! ## 资产纪律
//!
//! 本系统每帧运行，但 **Mesh / Material 资产不能每帧新增**：
//! - Material 按视图模式缓存，模式切换才重建；
//! - Mesh 只在几何规格变化时重建，旧 Mesh 用延迟命令在实体换绑后移除；
//! - Transform 仍每帧对齐投影，保证视图拉动式刷新。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Indices;
use bevy::render::render_resource::PrimitiveTopology;

use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{lod_tier, GridConfig, ViewState};
use crate::theme::colors_google::{
    GOOGLE_PLACE_DOT, GOOGLE_POI_RING, GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL,
};
use crate::theme::palette::{self, MapPalette, PaletteColors};
use crate::world_projection::{segments_fingerprint, PlaceDot, ProjectionFrame};

/// Google 式道路外缘宽度（屏幕像素）。
const GOOGLE_ROAD_CASING_PX: f32 = 6.0;
/// Google 式道路主体宽度（屏幕像素）。
const GOOGLE_ROAD_FILL_PX: f32 = 3.5;
/// POI 点标在屏幕上的最小可见直径（像素）。
const POI_MIN_SCREEN_PX: f32 = 5.0;

/// 道路 casing 在 Z 轴上的偏移：在 fill 之下、地形之上。
const ROAD_CASING_Z: f32 = 0.09;
/// 道路 fill 在 Z 轴上的偏移。
const ROAD_Z: f32 = 0.1;

/// 河流折线 Z（ADR-0037 D5）：地形装饰（0.075）之上、道路 casing（0.09）之下
/// ——河是底图水系，从地形上长出来，但让路桥压在河上。
const RIVER_Z: f32 = 0.082;
/// 河流描线宽（屏幕像素）：比道路 fill（3.5px）细一档，参考图的细蜿蜒蓝线。
const RIVER_STROKE_PX: f32 = 2.2;
/// 作者手绘河的线宽（屏幕像素）：比派生河细一档——小溪本来就该比大河细。
const AUTHOR_RIVER_STROKE_PX: f32 = 1.6;
/// 作者手绘河的 Z：派生河（0.082）之上、道路 casing（0.09）之下——
/// 作者的声明压在派生资产之上（"我画的"优先显形），又不进道路带。
const AUTHOR_RIVER_Z: f32 = 0.084;
/// 街道主干档（rank ≤ 2）宽度（屏幕像素）。
const STREET_MAJOR_PX: f32 = 4.5;
/// 街道次干 / 支路档宽度（屏幕像素）。
const STREET_MINOR_PX: f32 = 2.75;
/// 街道 casing 相对 fill 的外扩量（屏幕像素，两侧各半）。
/// 与道路（casing 6.0 / fill 3.5）同一套「粗细差即路缘」的口径。
const STREET_CASING_EXTRA_PX: f32 = 2.5;
/// 街道矢量层 Z 偏移：压在 `distance.edges` 行程连线（ROAD_Z=0.1）之上、POI 之下。
const STREET_MAJOR_Z: f32 = 0.11;
const STREET_MINOR_Z: f32 = 0.115;
/// 街道 casing 的 Z：紧贴在本档 fill 之下、上一档 fill 之上。
///
/// 街道必须画 casing：地图（GoogleMap）模式下 fill 是**白色**，而底图土地是
/// `#f8f9fa`——只画白色 fill 等于白线落在近白底图上，肉眼不可见（用户实机反馈
/// 「钻进武昌城看不到街道」的一半原因就是它）。casing 是浅灰 `#dadce0`，
/// 靠它与 fill 的宽度差勾出可见路缘。
const STREET_MAJOR_CASING_Z: f32 = 0.108;
const STREET_MINOR_CASING_Z: f32 = 0.113;

/// Z 序是**编译期**约束：底图 0.0 < 道路 fill < 主干 casing < 主干 fill < 支路 casing
/// < 支路 fill。破了它就会出现「casing 被底图盖掉」或「casing 反压在 fill 上」，
/// 两种表现都是街道在地图模式下不可见——正是这条约束要防的那个 bug。
#[allow(clippy::assertions_on_constants)]
const _: () = assert!(
    ROAD_Z < STREET_MAJOR_CASING_Z
        && STREET_MAJOR_CASING_Z < STREET_MAJOR_Z
        && STREET_MAJOR_Z < STREET_MINOR_CASING_Z
        && STREET_MINOR_CASING_Z < STREET_MINOR_Z,
    "矢量层 Z 序被破坏：道路 fill < 主干 casing < 主干 fill < 支路 casing < 支路 fill"
);
/// 橡皮筋预览线：比街道细、压在最上（仍低于 POI 0.21）。
const STREET_PREVIEW_PX: f32 = 1.75;
const STREET_PREVIEW_Z: f32 = 0.12;
/// POI 外环 / 内芯 Z 偏移。
const POI_OUTER_Z: f32 = 0.21;
const POI_INNER_Z: f32 = 0.22;

/// 合批 stroke mesh 的重建判据（A2）：内容指纹 + 世界线宽 + Z，任一变化才换 mesh。
///
/// `world_thickness` 是「几何赖以烘成的世界尺寸参数」：道路 = 线宽，
/// 地物符号 = 缩放因子 `s`（2026-10-24 教训：符号 key 漏掉 `s`，缩放后
/// mesh 不重建，pin 保持旧世界尺寸随相机放大成巨型墨团）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct StrokeBatchKey {
    fingerprint: u64,
    world_thickness: f32,
    z: f32,
}

/// 把整组段合成**一个** Mesh：每段一个世界空间矩形（4 顶点 / 6 索引）。
///
/// A2（2026-10-18）：旧实现每段一个实体 + 一个 `Rectangle` mesh，规格含
/// center 与随缩放的线宽——guangdong 若有 87k 段，相机平移/缩放的每一帧
/// 都会全量 add+remove 87k 个 mesh 资产。合批后平移（纯相机运动，段坐标
/// 不变）零重建；缩放 / 编辑整组换一个 mesh。z 由实体 Transform 承担，
/// 顶点保持在 z=0 平面。
///
/// 线宽口径沿旧实现：屏幕像素 → 世界单位 `world = px * view_scale`
/// （`view.scale` 是世界单位/像素，乘而不是除——历史 bug 见 git blame）。
pub(crate) fn build_stroke_batch_mesh(segments: &[(Vec2, Vec2)], world_thickness: f32) -> Mesh {
    let n = segments.len();
    let half = world_thickness * 0.5;

    // ---- 接头（2026-10-23 修 G3b）：逐段四边形 + butt 端会在交汇处留楔形缺口 ----
    // 两段在一处相接时，外角必然缺一块（角度越尖缺得越大），且线宽是**屏幕常量**，
    // 缺口在任何缩放级别都一样明显。补法：按端点度数统计，度数 ≥2 的位置补一块
    // 半径 = 半线宽的小圆盘（内接 6 边形）。内角的重叠本来无害，外角的缺口被填掉。
    // 成本：只在 mesh 重建时做一次（内容/线宽/Z 变化才重建，见 `sync_road_strokes`
    // 的 `StrokeBatchKey`），稳态零开销；顶点增量 = 接头数 × 7。
    let joins = stroke_join_centers(segments, half);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4 + joins.len() * 7);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6 + joins.len() * 6 * 3);
    for (a, b) in segments {
        let delta = *b - *a;
        let length = delta.length().max(f32::EPSILON);
        let dir = delta / length;
        let side = Vec2::new(-dir.y, dir.x) * half;
        let center = (*a + *b) * 0.5;
        let ext = dir * (length * 0.5);
        let base = positions.len() as u32;
        for offset in [-ext - side, ext - side, ext + side, -ext + side] {
            positions.push((center + offset).extend(0.0).to_array());
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    for center in &joins {
        let base = positions.len() as u32;
        positions.push([center.x, center.y, 0.0]);
        for i in 0..STROKE_JOIN_SEGMENTS {
            let ang = i as f32 / STROKE_JOIN_SEGMENTS as f32 * std::f32::consts::TAU;
            positions.push([
                center.x + half * ang.cos(),
                center.y + half * ang.sin(),
                0.0,
            ]);
        }
        for i in 0..STROKE_JOIN_SEGMENTS as u32 {
            let a = base + 1 + i;
            let b = base + 1 + (i + 1) % STROKE_JOIN_SEGMENTS as u32;
            indices.extend([base, a, b]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 接头圆盘的内接边数。半径 ≤3px 时 6 边的圆度损失不可见（POI 圆周取 12 段，
/// 那是因为点标半径可到 10px；接头比点标小一档，取一半）。
const STROKE_JOIN_SEGMENTS: usize = 6;

/// 找出**有 ≥2 段共用**的端点，返回需要补圆盘的中心点。
///
/// 重合判定用**位型相等**（`f32::to_bits` 当 key），不做 epsilon 合并：
/// - 同一条折线的相邻段由 `windows(2)` 产出，共享的那个顶点是**同一个 f32 克隆**；
/// - 路网边共享的地点坐标来自同一次 `map_to_view` 计算——位型必然相等；
/// - 反过来，两条「几乎但不真的相接」的路若被 epsilon 粘住，会凭空多出一块
///   圆盘（假接头），比缺口更难看。
fn stroke_join_centers(segments: &[(Vec2, Vec2)], half: f32) -> Vec<Vec2> {
    if segments.len() < 2 || half <= 0.0 {
        return Vec::new();
    }
    use std::collections::HashMap;
    let key = |p: Vec2| (p.x.to_bits(), p.y.to_bits());
    let mut degree: HashMap<(u32, u32), u32> = HashMap::with_capacity(segments.len() * 2);
    for (a, b) in segments {
        *degree.entry(key(*a)).or_insert(0) += 1;
        *degree.entry(key(*b)).or_insert(0) += 1;
    }
    degree
        .into_iter()
        .filter(|(_, d)| *d >= 2)
        .map(|((x, y), _)| Vec2::new(f32::from_bits(x), f32::from_bits(y)))
        .collect()
}

/// 屏幕密度预算（ADR-0030 D3）：本帧点标上限。视口只是第一道门，「看到的
/// 地方」在 PRD 这种高密度簇里本身就有上万个——成本预算必须与屏幕挂钩。
/// 超出按优先级裁剪，**纯渲染投影**：不写数据、不阻编辑（V1 无涉）。
const POI_MAX_DOTS: usize = 400;
/// POI 圆周段数：12 段近似（半径 ≤10px 时圆度损失不可见，ADR-0030 D4）。
const POI_CIRCLE_SEGMENTS: usize = 12;

/// POI 合批 mesh 的重建判据（ADR-0030 D4）：集合指纹 + 半径 + Z。
#[derive(Debug, Clone, Copy, PartialEq)]
struct PoiBatchKey {
    fingerprint: u64,
    radius: f32,
    z: f32,
}

/// POI 材质桶池：一个材质一个合批 mesh + 一个实体（ADR-0030 D4）。
///
/// 旧实现每点一对实体 + 逐点 mesh，半径 ∝ `view.scale` 让缩放期间每帧重建
/// 上万个小 mesh 资产（guangdong 实测实体 704 → 35,988、1.8 FPS）。合批后
/// 「每点成本」变「每桶成本」：可见集或半径变化才换 ≤4 个 mesh。
#[derive(Default)]
struct PoiBatchPool {
    batch: Option<PoiBatch>,
}

/// 一个 POI 材质桶的实体与 Mesh 句柄。
struct PoiBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: PoiBatchKey,
}

impl PoiBatchPool {
    /// 整组下线（Place 图层不可见时）。实体与 Mesh 资产一起清。
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

/// 屏幕像素 → 世界线宽：`world = px * view_scale`。`view.scale` 的单位是
/// **世界单位/像素**（见 `ViewState::scale` 的文档与 `terrain_chunks.rs` 的
/// 视口换算），所以恒定的屏幕线宽必须**乘** scale。历史上曾写成
/// `px / view_scale`，屏幕宽度实际变成 `stroke_px / scale²`：远观时细到
/// 几乎消失，深缩时胀成占屏 40% 的巨带——这个纯函数就是防它复发的单点。
pub(crate) fn stroke_world_thickness(stroke_px: f32, view_scale: f32) -> f32 {
    (stroke_px * view_scale).max(f32::EPSILON)
}

/// 线宽量化（ADR-0030 D5b）：相对漂移超过 4% 才算「变了」。
///
/// 屏幕线宽误差 ≤ 4%（6px casing 差 0.24px，不可见）；连续缩放（~1.5%/帧）
/// 下重建频率降约 3 倍。`b` 是已烘焙进几何的线宽，恒 > 0（`max(EPSILON)`）。
pub(crate) fn thickness_drifted(next: f32, baked: f32) -> bool {
    (next - baked).abs() > 0.04 * baked.max(f32::EPSILON)
}

/// POI 门槛 + 密度上限选择（ADR-0030 D2/D3）。
///
/// 输入 = `compute_projection` 已视口裁剪的本帧点标；输出 = 过级别硬门
/// [`crate::state::poi_visible_at_rank`] 后按优先级降序截断到
/// [`POI_MAX_DOTS`] 的幸存者。优先级序：`rank` > `is_city` > 名字长度
/// （更长者信息量更大，与标签碰撞优先级同一惯例）。排序稳定，同优先级
/// 保持原序（确定性好测试）。
fn select_pois(dots: &[PlaceDot], tier: crate::state::LodTier) -> Vec<&PlaceDot> {
    let mut visible: Vec<&PlaceDot> = dots
        .iter()
        .filter(|d| crate::state::poi_visible_at_rank(d.rank, tier))
        .collect();
    visible.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| b.is_city.cmp(&a.is_city))
            .then_with(|| b.name_len.cmp(&a.name_len))
    });
    visible.truncate(POI_MAX_DOTS);
    visible
}

/// 点集内容指纹（ADR-0030 D4）：合批 mesh 的「集合变没变」判据。
/// 只哈希坐标位型——半径 / Z 在 [`PoiBatchKey`] 里单独成字段。
pub(crate) fn dots_fingerprint(dots: &[Vec2]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for p in dots {
        for v in [p.x, p.y] {
            hash ^= u64::from(v.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 把点集合批成**一个**环带 Mesh：每点一个 12 段 annulus（内外圈各 12 顶点
/// 共享，`24N` 顶点 / `72N` 索引），圆心即点位置，z=0（实体 Transform 承担）。
pub(crate) fn build_poi_ring_batch_mesh(
    dots: &[Vec2],
    inner_radius: f32,
    outer_radius: f32,
) -> Mesh {
    let n = dots.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * POI_CIRCLE_SEGMENTS * 2);
    let mut indices: Vec<u32> = Vec::with_capacity(n * POI_CIRCLE_SEGMENTS * 6);
    for pos in dots {
        let base = positions.len() as u32;
        // 内外圈各 12 顶点共享：偶数下标 = 内圈，奇数下标 = 外圈。
        for s in 0..POI_CIRCLE_SEGMENTS {
            let a = std::f32::consts::TAU * s as f32 / POI_CIRCLE_SEGMENTS as f32;
            let (c, s_) = (a.cos(), a.sin());
            positions.push([pos.x + inner_radius * c, pos.y + inner_radius * s_, 0.0]);
            positions.push([pos.x + outer_radius * c, pos.y + outer_radius * s_, 0.0]);
        }
        for s in 0..POI_CIRCLE_SEGMENTS {
            let i0 = base + 2 * s as u32;
            let i1 = base + 2 * ((s + 1) % POI_CIRCLE_SEGMENTS) as u32;
            let (o0, o1) = (i0 + 1, i1 + 1);
            indices.extend([i0, i1, o1, i0, o1, o0]);
        }
    }
    finish_batch_mesh(positions, indices)
}

/// 把点集合批成**一个**扇形圆 Mesh：每点中心 + 12 圈顶点（`13N` 顶点 /
/// `36N` 索引），圆心即点位置。
pub(crate) fn build_poi_dot_batch_mesh(dots: &[Vec2], radius: f32) -> Mesh {
    let n = dots.len();
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * (POI_CIRCLE_SEGMENTS + 1));
    let mut indices: Vec<u32> = Vec::with_capacity(n * POI_CIRCLE_SEGMENTS * 3);
    for pos in dots {
        let base = positions.len() as u32;
        positions.push([pos.x, pos.y, 0.0]);
        for s in 0..POI_CIRCLE_SEGMENTS {
            let a = std::f32::consts::TAU * s as f32 / POI_CIRCLE_SEGMENTS as f32;
            positions.push([pos.x + radius * a.cos(), pos.y + radius * a.sin(), 0.0]);
        }
        for s in 0..POI_CIRCLE_SEGMENTS {
            let q = base + 1 + s as u32;
            let q_next = if s + 1 == POI_CIRCLE_SEGMENTS {
                base + 1
            } else {
                q + 1
            };
            indices.extend([base, q, q_next]);
        }
    }
    finish_batch_mesh(positions, indices)
}

/// 合批 Mesh 的公共收尾：三角形 + 双世界驻留（与 `build_stroke_batch_mesh`
/// 同款资产口径）。
fn finish_batch_mesh(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 按模式缓存的道路材质。
struct RoadMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    casing: Handle<ColorMaterial>,
    fill: Handle<ColorMaterial>,
}

/// 按模式缓存的 POI 材质。
struct PoiMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    /// 非城市 POI：随 `lod_fade` 淡出（远观趋 0）。
    ring: Handle<ColorMaterial>,
    dot: Handle<ColorMaterial>,
    /// 城市 POI：恒全显（Nation 档保留是既有语义）。
    city_ring: Handle<ColorMaterial>,
    city_dot: Handle<ColorMaterial>,
}

/// 按模式 + 主题缓存的街道材质（ADR-0014：主干 / 次干在古舆图里是两种色）。
struct StreetMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    major_casing: Handle<ColorMaterial>,
    major_fill: Handle<ColorMaterial>,
    minor_casing: Handle<ColorMaterial>,
    minor_fill: Handle<ColorMaterial>,
}

/// 道路 stroke 合批池：一个 pass 一个合并 mesh + 一个实体（A2，2026-10-18）。
///
/// 旧实现每段一个实体 + 一个 `Rectangle` mesh，规格含 center 与随缩放的
/// 线宽——guangdong 若有 87k 段，相机平移/缩放的每一帧都全量 add+remove
/// 资产。现在 [`StrokeBatchKey`]（内容指纹 / 线宽 / Z）任一变化才整组换
/// 一个 mesh；平移（纯相机运动，段坐标不变）零重建。
#[derive(Default)]
struct RoadLayerPool {
    batch: Option<StrokeBatch>,
}

/// 一个合批 stroke 的实体与 Mesh 句柄。
struct StrokeBatch {
    entity: Entity,
    mesh: Handle<Mesh>,
    key: StrokeBatchKey,
}

/// 地物符号合批池（每类一个 mesh）。与道路 stroke 同款「单 mesh 单实体」，
/// 改几何走 `Assets::get_mut` 原地改写顶点。
#[derive(Default)]
struct LandmarkPool {
    batch: Option<StrokeBatch>,
}

impl LandmarkPool {
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

impl RoadLayerPool {
    /// 整组下线（图层不可见时）。实体与 Mesh 资产一起清，下次可见重新合批。
    fn despawn(&mut self, commands: &mut Commands) {
        if let Some(batch) = self.batch.take() {
            commands.entity(batch.entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
            });
        }
    }
}

/// 矢量层实体与可复用资产规格缓存。
#[derive(Resource, Default)]
pub struct VectorLayerEntities {
    road_casings: RoadLayerPool,
    roads: RoadLayerPool,
    /// 街道矢量层（ADR-0010）：主干档（rank ≤ 2），画得粗、压在行程连线上。
    street_major: RoadLayerPool,
    /// 街道矢量层：次干/支路档，画得细。
    street_minor: RoadLayerPool,
    /// 街道主干档的 casing（浅灰路缘）：压在本档 fill 之下。
    street_major_casing: RoadLayerPool,
    /// 街道次干/支路档的 casing。
    street_minor_casing: RoadLayerPool,
    /// 街道编辑器的橡皮筋预览（上一落点 → 光标）。
    street_preview: RoadLayerPool,
    /// 河流折线合批（ADR-0037 D5）：模板世界派生水系，一段折线多个连续段。
    rivers: RoadLayerPool,
    /// 河流材质（惰性建一次；固定水蓝，主题化随 README §12-30 后继）。
    river_material: Option<Handle<ColorMaterial>>,
    /// **作者手绘**河合批（ADR-0046 D4 + F3 样式区分）：与派生河**分池分材质**
    /// ——作者河更亮更细，实机可辨「这条是我画的」。
    author_rivers: RoadLayerPool,
    /// 作者河材质（惰性建一次；比派生河亮一档的青蓝）。
    author_river_material: Option<Handle<ColorMaterial>>,
    /// 地物符号合批（每类一个 mesh + 一个实体，共 2 个）；与 decor 同款
    /// 「原地改写」纪律。顶点色已在 mesh 端烘好 → 配白色 ColorMaterial。
    landmarks: [LandmarkPool; 2],
    /// 地物符号材质（白色，顶点色承载全部颜色；惰性建一次防资产累积）。
    landmark_material: Option<Handle<ColorMaterial>>,
    /// POI 外环合批（非城市材质，ADR-0030 D4）。
    poi_ring_plain: PoiBatchPool,
    /// POI 外环合批（城市材质）。
    poi_ring_city: PoiBatchPool,
    /// POI 内芯合批（非城市材质）。
    poi_dot_plain: PoiBatchPool,
    /// POI 内芯合批（城市材质）。
    poi_dot_city: PoiBatchPool,
    road_materials: Vec<RoadMaterialSet>,
    road_material_mode: Option<crate::state::ViewMode>,
    poi_materials: Vec<PoiMaterialSet>,
    poi_material_mode: Option<crate::state::ViewMode>,
    /// 街道分级材质：主干 / 次干各 casing+fill（古舆图双色，ADR-0016）。
    street_materials: Vec<StreetMaterialSet>,
    street_material_key: Option<(crate::state::ViewMode, MapPalette)>,
}

/// 每帧同步道路、街道与 POI。系统只消费 `ProjectionFrame` 与街道缓存，
/// 不修改世界数据。
///
/// 可见 / 透明度 / Z 偏移读 [`LayerFrame`]（ADR-0013）：Road 组与 Place 组
/// 各自独立绑定，组内 casing < fill < 标注的次序不变，整组随 `z_offset` 平移。
#[allow(clippy::too_many_arguments)]
pub fn sync_vector_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    frame: Res<ProjectionFrame>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    streets: Res<crate::streets::StreetLayerCache>,
    editor: Res<crate::streets::StreetEditorState>,
    mut layer: ResMut<VectorLayerEntities>,
) {
    let road_materials_changed = ensure_road_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.road.opacity,
        &mut layer,
    );
    let _poi_materials_changed = ensure_poi_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.place.opacity,
        &mut layer,
    );
    let street_materials_changed = ensure_street_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.road.opacity,
        &mut layer,
    );
    // 绑定变化（作者改 YAML 的 opacity）：缓存材质的 alpha 就地刷新。
    // z 偏移不需要这步——sync_road_strokes / sync_poi 每帧全量重写 Transform。
    if layers.is_changed() {
        apply_road_material_alpha(&mut materials, &layer, layers.road.opacity);
    }
    // POI 的 alpha 只依赖 Place 图层 opacity（级别门槛是硬门，不再有缩放
    // 淡出，ADR-0030 D2）：每帧刷 4 个材质句柄，代价可忽略。
    apply_poi_material_alpha(&mut materials, &layer, layers.place.opacity);
    let road_z = layers.road.z_offset;
    let place_z = layers.place.z_offset;

    if !layers.road.visible {
        layer.road_casings.despawn(&mut commands);
        layer.roads.despawn(&mut commands);
        layer.rivers.despawn(&mut commands);
        layer.river_material = None;
        layer.author_rivers.despawn(&mut commands);
        layer.author_river_material = None;
        for p in &mut layer.landmarks {
            p.despawn(&mut commands);
        }
        layer.street_major.despawn(&mut commands);
        layer.street_minor.despawn(&mut commands);
        layer.street_major_casing.despawn(&mut commands);
        layer.street_minor_casing.despawn(&mut commands);
        layer.street_preview.despawn(&mut commands);
    } else {
        let (casing, fill) = layer
            .road_materials
            .iter()
            .find(|m| m.mode == view.mode)
            .map(|m| (m.casing.clone(), m.fill.clone()))
            .expect("ensure_road_materials 必须已初始化");
        let (major_casing, major_fill, minor_casing, minor_fill) = layer
            .street_materials
            .iter()
            .find(|m| m.mode == view.mode && m.palette == view.map_palette)
            .map(|m| {
                (
                    m.major_casing.clone(),
                    m.major_fill.clone(),
                    m.minor_casing.clone(),
                    m.minor_fill.clone(),
                )
            })
            .expect("ensure_street_materials 必须已初始化");
        // 河流（ADR-0037 D5）：地形族水系，gate = MODE_TABLE(Rivers) ∧ terrain 图层。
        // 与 road_stale 无关——river_polylines 的生命周期由 compute_projection 的
        // river_key 缓存管；这里只做「可见才合批」。
        // 级别门槛（ADR-0047 D5）：派生河与作者河都带 rank（长度占比折算），
        // 过统一门槛——Nation 档只画横贯级主河，Region 档加区域河，Edit 全见。
        if recipe.enabled(PassId::Rivers) && layers.terrain.visible {
            let mat = layer.river_material.get_or_insert_with(|| {
                // 2026-10-22 验收：CellMap 用固定深青（数据视图，不随主题）；
                // 其他模式沿用原水色。
                materials.add(ColorMaterial::from_color(
                    if matches!(view.mode, crate::state::ViewMode::Topographic) {
                        Color::srgb(0.12, 0.32, 0.45)
                    } else {
                        Color::srgb(0.29, 0.53, 0.68)
                    },
                ))
            });
            let river_tier = lod_tier(view.scale);
            let mut river_segments: Vec<(Vec2, Vec2)> = Vec::new();
            for line in &frame.river_polylines {
                if !crate::state::rank_visible_at_tier(line.rank, river_tier) {
                    continue;
                }
                for pair in line.pts.windows(2) {
                    river_segments.push((pair[0], pair[1]));
                }
            }
            let fp = segments_fingerprint(&river_segments);
            sync_road_strokes(
                &mut commands,
                &mut meshes,
                &river_segments,
                fp,
                view.scale,
                mat.clone(),
                RIVER_STROKE_PX,
                RIVER_Z,
                false,
                &mut layer.rivers,
            );
            // 作者手绘河（ADR-0046 D4）：与派生水系**双供给**，且**分池分材质**：
            // 更亮一档的青蓝 + 更细的线——实机可辨「这条是我画的」；派生河是
            // 模板世界的确定性资产，两者视觉职责不同。同一呈现门槛（ADR-0047
            // D5：双供给同一级别语义）。
            let mut author_segments: Vec<(Vec2, Vec2)> = Vec::new();
            for line in &frame.author_river_polylines {
                if !crate::state::rank_visible_at_tier(line.rank, river_tier) {
                    continue;
                }
                for pair in line.pts.windows(2) {
                    author_segments.push((pair[0], pair[1]));
                }
            }
            let author_mat = layer.author_river_material.get_or_insert_with(|| {
                materials.add(ColorMaterial::from_color(Color::srgb(0.42, 0.70, 0.88)))
            });
            let author_fp = segments_fingerprint(&author_segments);
            sync_road_strokes(
                &mut commands,
                &mut meshes,
                &author_segments,
                author_fp,
                view.scale,
                author_mat.clone(),
                AUTHOR_RIVER_STROKE_PX,
                AUTHOR_RIVER_Z,
                false,
                &mut layer.author_rivers,
            );
        } else {
            layer.rivers.despawn(&mut commands);
            layer.author_rivers.despawn(&mut commands);
        }
        // 世界道路（ADR-0047 D4）：按当前视野档位取降序前缀切片——Nation 档
        // 只画城际干线（rank ≥ 3），Region 加区域级，Edit 全量。缩放不重算
        // 段列（缓存门 road_key 不含 scale），跨档只换 mesh（分档指纹）。
        // 2026-10-22 内容矩阵：接 MODE_TABLE(Roads)——Topographic 下不画。
        let road_tier = lod_tier(view.scale);
        let road_segments = frame.road_slice_at(road_tier);
        let road_fingerprint = frame.road_fingerprint_at(road_tier);
        if recipe.enabled(PassId::Roads) {
            sync_road_strokes(
                &mut commands,
                &mut meshes,
                road_segments,
                road_fingerprint,
                view.scale,
                casing.clone(),
                GOOGLE_ROAD_CASING_PX,
                ROAD_CASING_Z + road_z,
                road_materials_changed,
                &mut layer.road_casings,
            );
            sync_road_strokes(
                &mut commands,
                &mut meshes,
                road_segments,
                road_fingerprint,
                view.scale,
                fill.clone(),
                GOOGLE_ROAD_FILL_PX,
                ROAD_Z + road_z,
                road_materials_changed,
                &mut layer.roads,
            );
        } else {
            layer.road_casings.despawn(&mut commands);
            layer.roads.despawn(&mut commands);
        }
        // 橡皮筋预览：画线中的「上一落点 → 光标」（编辑器反馈，与 roads_visible
        // 无关——预览属于编辑会话，不属于图层可见性）。两端都是**视图空间**坐标
        // （光标来自 `view.cursor_world`，落点由 `streets` 存同一空间），
        // 不需要任何换算——历史上这里用 `grid.origin + (世界 − 本图帧原点)` 兜了
        // 一圈，城级（继承底图）时那一圈会把预览推到视野外。
        let preview: Vec<(Vec2, Vec2)> =
            match (editor.enabled, editor.last_click, view.cursor_world) {
                (true, Some(a), Some(b)) => vec![(a, b)],
                _ => Vec::new(),
            };
        let preview_fp = segments_fingerprint(&preview);
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &preview,
            preview_fp,
            view.scale,
            fill.clone(),
            STREET_PREVIEW_PX,
            STREET_PREVIEW_Z + road_z,
            road_materials_changed,
            &mut layer.street_preview,
        );
        // 2026-10-22 内容矩阵：接 MODE_TABLE(Streets)——Topographic 下不画。
        if recipe.enabled(PassId::Streets) {
            sync_street_layers(
                &mut commands,
                &mut meshes,
                &grid,
                &streets,
                view.scale,
                major_casing,
                major_fill,
                minor_casing,
                minor_fill,
                road_z,
                street_materials_changed,
                &mut layer,
            );
        } else {
            layer.street_major.despawn(&mut commands);
            layer.street_minor.despawn(&mut commands);
            layer.street_major_casing.despawn(&mut commands);
            layer.street_minor_casing.despawn(&mut commands);
        }
    }

    // 2026-10-22 内容矩阵：接 MODE_TABLE(Poi)——Topographic 下不画。
    if !layers.place.visible || !recipe.enabled(PassId::Poi) {
        layer.poi_ring_plain.despawn(&mut commands);
        layer.poi_ring_city.despawn(&mut commands);
        layer.poi_dot_plain.despawn(&mut commands);
        layer.poi_dot_city.despawn(&mut commands);
    } else {
        sync_poi(
            &mut commands,
            &mut meshes,
            &frame.place_dots,
            view.scale,
            &grid,
            place_z,
            &mut layer,
        );
    }

    // 地物符号（rank ≥ 2）。2026-10-23 二轮裁定：map-pin **只用于 CellMap**
    // （数据视图的地点标记，独立于 FeatureOverlay——Topographic 行该格是
    // false，主题地界覆盖层不进 CellMap，pin 是唯一例外）；其余
    // FeatureOverlay 开档模式照旧画城堡/小镇插画。place 图层关时都不画。
    // 注意**不得**嵌进上面的 POI 门：Topographic 下 Poi=false，嵌进去 pin
    // 永远画不出（首启实测 mesh_count=0 的根因）。
    let landmarks_want = if view.mode == crate::state::ViewMode::Topographic {
        layers.place.visible
    } else {
        layers.place.visible && recipe.enabled(PassId::FeatureOverlay)
    };
    if landmarks_want {
        let white = layer
            .landmark_material
            .get_or_insert_with(|| materials.add(ColorMaterial::from_color(Color::WHITE)))
            .clone();
        let VectorLayerEntities { landmarks, .. } = &mut *layer;
        sync_landmarks(
            &mut commands,
            &mut meshes,
            landmarks,
            &frame.landmark_dots,
            view.mode == crate::state::ViewMode::Topographic,
            view.scale,
            crate::landmark_symbols::LANDMARK_Z + layers.place.z_offset,
            white,
        );
    } else {
        for p in &mut layer.landmarks {
            p.despawn(&mut commands);
        }
        layer.landmark_material = None;
    }
}

/// 道路组两种模式的基础色（casing, fill）。alpha 是「本色 alpha」，
/// 图层 opacity 在其上相乘。
///
/// 非 Google 模式走地图主题色板（ADR-0016）：casing = 纸面色（把路从同族
/// 地形色上「托」出来），fill = 官道棕。
fn road_base_colors(mode: crate::state::ViewMode, pal: &PaletteColors) -> (Color, Color) {
    match mode {
        crate::state::ViewMode::GoogleMap => (GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL),
        _ => (pal.road_casing.with_alpha(0.28), pal.road_stroke),
    }
}

/// POI 组两种模式的基础色（ring, dot）。
fn poi_base_colors(mode: crate::state::ViewMode, pal: &PaletteColors) -> (Color, Color) {
    match mode {
        crate::state::ViewMode::GoogleMap => (GOOGLE_POI_RING, GOOGLE_PLACE_DOT),
        _ => (pal.poi_ring, pal.poi_dot),
    }
}

/// 街道两级的基础色（[(主干 casing, 主干 fill), (次干 casing, 次干 fill)]）。
///
/// 古舆图分级双色（ADR-0016）：主干官道棕粗、次干浅卡其细；casing 同为纸面色。
fn street_base_colors(mode: crate::state::ViewMode, pal: &PaletteColors) -> [(Color, Color); 2] {
    match mode {
        crate::state::ViewMode::GoogleMap => [(GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL); 2],
        _ => [
            (pal.road_casing.with_alpha(0.28), pal.street_major),
            (pal.road_casing.with_alpha(0.28), pal.street_minor),
        ],
    }
}

/// 把 Road 图层 opacity 刷进所有已缓存的道路材质（按各模式的基础色重算）。
fn apply_road_material_alpha(
    materials: &mut Assets<ColorMaterial>,
    layer: &VectorLayerEntities,
    opacity: f32,
) {
    for set in &layer.road_materials {
        let (casing, fill) = road_base_colors(set.mode, palette::colors(set.palette));
        if let Some(mut mat) = materials.get_mut(&set.casing) {
            mat.color = casing.with_alpha(casing.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.fill) {
            mat.color = fill.with_alpha(fill.alpha() * opacity);
        }
    }
}

/// 把 Place 图层 opacity 刷进所有已缓存的 POI 材质。
///
/// 级别可见性是档位硬门（ADR-0030 D2），不再有缩放淡出系数——过了门槛的
/// 点标全显，城市/非城市材质只差配色。
fn apply_poi_material_alpha(
    materials: &mut Assets<ColorMaterial>,
    layer: &VectorLayerEntities,
    opacity: f32,
) {
    for set in &layer.poi_materials {
        let (ring, dot) = poi_base_colors(set.mode, palette::colors(set.palette));
        if let Some(mut mat) = materials.get_mut(&set.ring) {
            mat.color = ring.with_alpha(ring.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.dot) {
            mat.color = dot.with_alpha(dot.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.city_ring) {
            mat.color = ring.with_alpha(ring.alpha() * opacity);
        }
        if let Some(mut mat) = materials.get_mut(&set.city_dot) {
            mat.color = dot.with_alpha(dot.alpha() * opacity);
        }
    }
}

fn ensure_road_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    map_palette: MapPalette,
    road_opacity: f32,
    layer: &mut VectorLayerEntities,
) -> bool {
    if layer.road_material_mode == Some(mode) {
        return false;
    }
    if !layer
        .road_materials
        .iter()
        .any(|set| set.mode == mode && set.palette == map_palette)
    {
        let (casing, fill) = road_base_colors(mode, palette::colors(map_palette));
        let with_opacity = |c: Color| c.with_alpha(c.alpha() * road_opacity);
        layer.road_materials.push(RoadMaterialSet {
            mode,
            palette: map_palette,
            casing: materials.add(with_opacity(casing)),
            fill: materials.add(with_opacity(fill)),
        });
    }
    layer.road_material_mode = Some(mode);
    true
}

fn ensure_poi_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    map_palette: MapPalette,
    place_opacity: f32,
    layer: &mut VectorLayerEntities,
) -> bool {
    if layer.poi_material_mode == Some(mode) {
        return false;
    }
    if !layer
        .poi_materials
        .iter()
        .any(|set| set.mode == mode && set.palette == map_palette)
    {
        let (ring, dot) = poi_base_colors(mode, palette::colors(map_palette));
        let with_opacity = |c: Color| c.with_alpha(c.alpha() * place_opacity);
        layer.poi_materials.push(PoiMaterialSet {
            mode,
            palette: map_palette,
            ring: materials.add(with_opacity(ring)),
            dot: materials.add(with_opacity(dot)),
            city_ring: materials.add(with_opacity(ring)),
            city_dot: materials.add(with_opacity(dot)),
        });
    }
    layer.poi_material_mode = Some(mode);
    true
}

/// 确保街道两级材质已按 (mode, palette) 建好；键变了返回 true（mesh 材质句柄要刷）。
fn ensure_street_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    map_palette: MapPalette,
    road_opacity: f32,
    layer: &mut VectorLayerEntities,
) -> bool {
    let key = (mode, map_palette);
    if layer.street_material_key == Some(key) {
        return false;
    }
    if !layer
        .street_materials
        .iter()
        .any(|set| set.mode == mode && set.palette == map_palette)
    {
        let [(mc, mf), (nc, nf)] = street_base_colors(mode, palette::colors(map_palette));
        let with_opacity = |c: Color| c.with_alpha(c.alpha() * road_opacity);
        layer.street_materials.push(StreetMaterialSet {
            mode,
            palette: map_palette,
            major_casing: materials.add(with_opacity(mc)),
            major_fill: materials.add(with_opacity(mf)),
            minor_casing: materials.add(with_opacity(nc)),
            minor_fill: materials.add(with_opacity(nf)),
        });
    }
    layer.street_material_key = Some(key);
    true
}

/// 同步街道矢量层（ADR-0010 待做第 2 项）。
///
/// 路段流按 LOD 档筛过后分两档宽度画出；坐标变换与地点/连线同一条链
/// （[`GridConfig::map_to_view`]，本图局部 → 视图空间），不另立换算。
/// 缓存为空时两池照常走 `sync_road_strokes` 的空列表路径——既清干净又无资产泄漏。
///
/// **每档都画 casing + fill 两层**（与道路同一套口径）：只画 fill 时，地图模式下
/// 白 fill 落在近白底图上等于不可见（casing 的浅灰才是肉眼看到的那条线）。
#[allow(clippy::too_many_arguments)]
fn sync_street_layers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    grid: &GridConfig,
    streets: &crate::streets::StreetLayerCache,
    view_scale: f32,
    major_casing: Handle<ColorMaterial>,
    major_fill: Handle<ColorMaterial>,
    minor_casing: Handle<ColorMaterial>,
    minor_fill: Handle<ColorMaterial>,
    z_offset: f32,
    material_changed: bool,
    layer: &mut VectorLayerEntities,
) {
    let tier = lod_tier(view_scale);
    let mut major: Vec<(Vec2, Vec2)> = Vec::new();
    let mut minor: Vec<(Vec2, Vec2)> = Vec::new();
    for (rank, segments) in crate::streets::street_streams(&streets.sources) {
        if !crate::streets::street_kept_at_tier(rank, tier) {
            continue;
        }
        // 本图局部坐标 → 视图空间（`GridConfig::map_to_view`，与地点同一条链；
        // 城市继承父图底图时栅格原点不是本图帧原点，绝不能拿 `raster_origin` 顶替）。
        let target = if rank <= 2 { &mut major } else { &mut minor };
        target.extend(
            segments
                .iter()
                .map(|(a, b)| (grid.map_to_view(*a), grid.map_to_view(*b))),
        );
    }
    // casing 先画（Z 更低），fill 后画：两层的宽度差就是可见路缘。
    // 组内次序是编译期约束；整组再叠加 Road 图层的 z_offset（ADR-0013）。
    // 内容指纹各算一次，casing / fill 两个 pass 共用。
    let major_fp = segments_fingerprint(&major);
    let minor_fp = segments_fingerprint(&minor);
    sync_road_strokes(
        commands,
        meshes,
        &major,
        major_fp,
        view_scale,
        major_casing,
        STREET_MAJOR_PX + STREET_CASING_EXTRA_PX,
        STREET_MAJOR_CASING_Z + z_offset,
        material_changed,
        &mut layer.street_major_casing,
    );
    sync_road_strokes(
        commands,
        meshes,
        &major,
        major_fp,
        view_scale,
        major_fill,
        STREET_MAJOR_PX,
        STREET_MAJOR_Z + z_offset,
        material_changed,
        &mut layer.street_major,
    );
    sync_road_strokes(
        commands,
        meshes,
        &minor,
        minor_fp,
        view_scale,
        minor_casing,
        STREET_MINOR_PX + STREET_CASING_EXTRA_PX,
        STREET_MINOR_CASING_Z + z_offset,
        material_changed,
        &mut layer.street_minor_casing,
    );
    sync_road_strokes(
        commands,
        meshes,
        &minor,
        minor_fp,
        view_scale,
        minor_fill,
        STREET_MINOR_PX,
        STREET_MINOR_Z + z_offset,
        material_changed,
        &mut layer.street_minor,
    );
}

/// 同步一组道路 stroke 的**合批** mesh：内容指纹 / 线宽 / Z 任一变化才换
/// mesh，材质变化只换绑 `MeshMaterial2d`（A2，2026-10-18）。
///
/// `fingerprint` 由调用方给定（世界道路取档位切片的 `road_fingerprint_at`；
/// 街道 / 预览 / 河流段列就地 [`segments_fingerprint`]）——同一组段在
/// roads 与 casings 两个 pass 间不重复哈希。
#[allow(clippy::too_many_arguments)]
fn sync_road_strokes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    segments: &[(Vec2, Vec2)],
    fingerprint: u64,
    view_scale: f32,
    material: Handle<ColorMaterial>,
    stroke_px: f32,
    z: f32,
    material_changed: bool,
    pool: &mut RoadLayerPool,
) {
    let key = StrokeBatchKey {
        fingerprint,
        world_thickness: stroke_world_thickness(stroke_px, view_scale),
        z,
    };
    match pool.batch.take() {
        None => {
            if segments.is_empty() {
                return;
            }
            let mesh = meshes.add(build_stroke_batch_mesh(segments, key.world_thickness));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material),
                    Transform::from_xyz(0.0, 0.0, key.z),
                ))
                .id();
            pool.batch = Some(StrokeBatch { entity, mesh, key });
        }
        Some(mut batch) => {
            if segments.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            // 线宽量化（ADR-0030 D5b）：内容与 Z 不变、线宽漂移 ≤4% 时不换
            // 几何——连续缩放下把 87k 段 × 2 pass 的重建频率降约 3 倍，屏幕
            // 线宽误差 ≤ 0.24px（6px casing）。`batch.key` 保留**已烘焙**值，
            // 漂移永远相对几何实际线宽累计。
            let rebuilt = batch.key.fingerprint != key.fingerprint
                || batch.key.z != key.z
                || thickness_drifted(key.world_thickness, batch.key.world_thickness);
            if rebuilt {
                batch.key = key;
                let next = meshes.add(build_stroke_batch_mesh(segments, key.world_thickness));
                let old = std::mem::replace(&mut batch.mesh, next.clone());
                commands
                    .entity(batch.entity)
                    .insert((Mesh2d(next), Transform::from_xyz(0.0, 0.0, key.z)));
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&old);
                });
            }
            if material_changed {
                commands
                    .entity(batch.entity)
                    .insert(MeshMaterial2d(material));
            }
            pool.batch = Some(batch);
        }
    }
}

/// 同步红点白环 POI（ADR-0030 D4 合批版）。
///
/// 门槛 + 密度上限（[`select_pois`]）幸存的点标按 4 个材质桶各合批成
/// **一个** mesh（外环/内芯 × 城市/非城市）；半径或集合变化才换该桶 mesh
/// （旧资产延迟移除，A2 同款）。`z_offset` 是 Place 图层的槽位偏移；
/// 外环/内芯各占一个 Z 槽，桶内共享。
#[allow(clippy::too_many_arguments)]
fn sync_poi(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[PlaceDot],
    view_scale: f32,
    grid: &GridConfig,
    z_offset: f32,
    layer: &mut VectorLayerEntities,
) {
    let tier = crate::state::lod_tier(view_scale);
    let selected = select_pois(dots, tier);
    let radius_outer = (grid.cell_size * 0.45).max(POI_MIN_SCREEN_PX * view_scale * 0.5);
    let radius_inner = radius_outer * 0.52;
    let ring_thickness = (1.5 * view_scale).max(f32::EPSILON);
    let ring_inner = (radius_outer - ring_thickness).max(f32::EPSILON);
    let mode = layer
        .poi_material_mode
        .expect("ensure_poi_materials 必须已初始化");
    let (ring_plain, dot_plain, city_ring, city_dot) = layer
        .poi_materials
        .iter()
        .find(|m| m.mode == mode)
        .map(|m| {
            (
                m.ring.clone(),
                m.dot.clone(),
                m.city_ring.clone(),
                m.city_dot.clone(),
            )
        })
        .expect("ensure_poi_materials 必须已初始化");

    // 按材质分桶取坐标（桶内几何只依赖坐标 + 统一半径）。
    let mut plain: Vec<Vec2> = Vec::new();
    let mut city: Vec<Vec2> = Vec::new();
    for d in &selected {
        if d.is_city {
            city.push(d.pos);
        } else {
            plain.push(d.pos);
        }
    }
    let plain_fp = dots_fingerprint(&plain);
    let city_fp = dots_fingerprint(&city);
    let z_outer = POI_OUTER_Z + z_offset;
    let z_inner = POI_INNER_Z + z_offset;

    sync_poi_batch(
        commands,
        meshes,
        &plain,
        plain_fp,
        (ring_inner, radius_outer),
        z_outer,
        ring_plain,
        &mut layer.poi_ring_plain,
        build_poi_ring_batch_mesh,
    );
    sync_poi_batch(
        commands,
        meshes,
        &city,
        city_fp,
        (ring_inner, radius_outer),
        z_outer,
        city_ring,
        &mut layer.poi_ring_city,
        build_poi_ring_batch_mesh,
    );
    sync_poi_batch(
        commands,
        meshes,
        &plain,
        plain_fp,
        (radius_inner, radius_inner),
        z_inner,
        dot_plain,
        &mut layer.poi_dot_plain,
        |dots, r, _| build_poi_dot_batch_mesh(dots, r),
    );
    sync_poi_batch(
        commands,
        meshes,
        &city,
        city_fp,
        (radius_inner, radius_inner),
        z_inner,
        city_dot,
        &mut layer.poi_dot_city,
        |dots, r, _| build_poi_dot_batch_mesh(dots, r),
    );
}

/// 同步一个 POI 材质桶的合批 mesh：指纹/半径/Z 任一变化才换 mesh。
///
/// `build` 由调用方给定（外环要 (inner, outer) 双半径，内芯只用单半径——
/// 传 `(r, r)` 复用同一条同步路径，两个桶字段不必各写一份）。
#[allow(clippy::too_many_arguments)]
fn sync_poi_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[Vec2],
    fingerprint: u64,
    radii: (f32, f32),
    z: f32,
    material: Handle<ColorMaterial>,
    pool: &mut PoiBatchPool,
    build: impl Fn(&[Vec2], f32, f32) -> Mesh,
) {
    let key = PoiBatchKey {
        fingerprint,
        radius: radii.1,
        z,
    };
    match pool.batch.take() {
        None => {
            if dots.is_empty() {
                return;
            }
            spawn_poi_batch(commands, meshes, dots, key, radii, material, pool, build);
        }
        Some(mut batch) => {
            if dots.is_empty() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
                return;
            }
            if batch.key != key {
                let next = meshes.add(build(dots, radii.0, radii.1));
                let old = std::mem::replace(&mut batch.mesh, next.clone());
                commands
                    .entity(batch.entity)
                    .insert((Mesh2d(next), Transform::from_xyz(0.0, 0.0, key.z)));
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&old);
                });
                batch.key = key;
            }
            pool.batch = Some(batch);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_landmarks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    landmark: &mut [LandmarkPool; 2],
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    pin: bool,
    view_scale: f32,
    z: f32,
    material: Handle<ColorMaterial>,
) {
    use crate::world_projection::LANDMARK_KINDS;
    // 形状分流（2026-10-23 二轮裁定）：CellMap 用 map-pin；其余模式画
    // 城堡/小镇插画。PNG 贴图路径已退役，全部走顶点色 mesh。
    for (i, kind) in LANDMARK_KINDS.iter().copied().enumerate() {
        sync_landmark_mesh_batches(
            commands,
            meshes,
            &mut landmark[i],
            frame_landmarks,
            kind,
            pin,
            view_scale,
            z,
            material.clone(),
        );
    }
}

/// 地物符号的 LOD 尺寸微调：档位越近稍大（Nation 1.0 / Region 1.25 /
/// Edit 1.5），贴近 Google Maps marker 的恒定手感。独立成函数供回归测试
/// 推导「世界高 ∝ tier × scale」的精确期望值。
fn landmark_size_tier(view_scale: f32) -> f32 {
    match lod_tier(view_scale) {
        crate::state::LodTier::Nation => 1.0,
        crate::state::LodTier::Region => 1.25,
        crate::state::LodTier::Edit => 1.5,
    }
}

/// 地物符号合批的复用 key（纯函数，测试锚定）。
///
/// 几何 = f(位置, kind, 形态, s)，故 key 必须吃全四样：
/// - `s` 进 `world_thickness`——否则缩放后位置指纹不变、mesh 不重建，
///   符号保持旧世界尺寸随相机缩放（2026-10-24 CellMap 实测：455% 下
///   pin 放大 ~4.5 倍）。与 POI 的 `radius` 进 `PoiBatchKey` 同一条纪律。
/// - **形态（pin）必须混进指纹**——切 Browse ↔ Topographic 翻转 pin，
///   位置与 `s` 都不变时旧 key 仍命中池子，mesh 维持旧形态：实机表现
///   即「进 CellMap 还是城堡，缩放一下才变 pin」（2026-10-25 验收发现）。
///   混入用黄金比例奇常数：pin=false 不变，pin=true 加一个充分离散的
///   非零 delta，避免只翻指纹低 1 位的意外相撞。
fn landmark_batch_key(
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    kind: crate::world_projection::LandmarkKind,
    pin: bool,
    s: f32,
    z: f32,
) -> StrokeBatchKey {
    const PIN_TAG: u64 = 0x9E37_79B9_7F4A_7C15;
    let fingerprint =
        points_fingerprint_dots(frame_landmarks, kind) ^ if pin { PIN_TAG } else { 0 };
    StrokeBatchKey {
        fingerprint,
        world_thickness: s,
        z,
    }
}

/// 单类符号的顶点色 mesh 合批（从 sync_landmarks 拆出的唯一绘制路径）。
#[allow(clippy::too_many_arguments)]
fn sync_landmark_mesh_batches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    pool: &mut LandmarkPool,
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    kind: crate::world_projection::LandmarkKind,
    pin: bool,
    view_scale: f32,
    z: f32,
    material: Handle<ColorMaterial>,
) {
    use crate::landmark_symbols::{build_landmark_mesh, LandmarkBatch};
    // 尺寸口径：**屏幕像素恒定**，按 LOD 档微调（档位越近稍大，贴近
    // Google Maps marker 的恒定手感）。符号局部坐标单位就是屏幕像素，
    // 缩放系数只乘 `view_scale`——2026-10-23 实测教训：曾在本地坐标外
    // 再乘一档画布基准（26），符号被放大 26 倍成盖住半个大陆的墨团。
    let s = landmark_size_tier(view_scale) * view_scale;
    let mut batch = LandmarkBatch::new();
    for d in frame_landmarks {
        if d.kind == kind {
            if pin {
                batch.push_map_pin(d.pos, s);
            } else {
                match kind {
                    crate::world_projection::LandmarkKind::Citadel => batch.push_citadel(d.pos, s),
                    crate::world_projection::LandmarkKind::Town => batch.push_town(d.pos, s),
                }
            }
        }
    }
    let key = landmark_batch_key(frame_landmarks, kind, pin, s, z);
    match pool.batch.take() {
        None => {
            let Some(mesh) = build_landmark_mesh(batch) else {
                return;
            };
            let mesh_handle = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Mesh2d(mesh_handle.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(0.0, 0.0, z),
                ))
                .id();
            pool.batch = Some(StrokeBatch {
                entity,
                mesh: mesh_handle,
                key,
            });
        }
        Some(mut existing) => {
            let Some(new_mesh) = build_landmark_mesh(batch) else {
                commands.entity(existing.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&existing.mesh);
                });
                return;
            };
            if existing.key != key {
                if let Some(mut m) = meshes.get_mut(&existing.mesh) {
                    *m = new_mesh;
                }
                commands
                    .entity(existing.entity)
                    .insert(Transform::from_xyz(0.0, 0.0, z));
                existing.key = key;
            }
            pool.batch = Some(existing);
        }
    }
}

/// 与 `segments_fingerprint` 同款，但用 `LandmarkDot` 字段做内容指纹。
fn points_fingerprint_dots(
    dots: &[crate::world_projection::LandmarkDot],
    kind: crate::world_projection::LandmarkKind,
) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for d in dots {
        if d.kind != kind {
            continue;
        }
        for v in [d.pos.x, d.pos.y] {
            hash ^= u64::from(v.to_bits());
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

/// 首次生成一个 POI 桶的合批实体。
#[allow(clippy::too_many_arguments)]
fn spawn_poi_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[Vec2],
    key: PoiBatchKey,
    radii: (f32, f32),
    material: Handle<ColorMaterial>,
    pool: &mut PoiBatchPool,
    build: impl Fn(&[Vec2], f32, f32) -> Mesh,
) {
    let mesh = meshes.add(build(dots, radii.0, radii.1));
    let entity = commands
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, key.z),
        ))
        .id();
    pool.batch = Some(PoiBatch { entity, mesh, key });
}

#[cfg(test)]
mod tests {
    /// **形态翻转必须换 key**（2026-10-25 实机验收发现）：Browse ↔
    /// Topographic 切换翻转 pin，位置指纹与尺寸 `s` 都不变时 key 也要
    /// 不同——否则池子复用旧形态 mesh，实机表现即「进 CellMap 还是
    /// 城堡，缩放一下才变 pin」。
    #[test]
    fn landmark_batch_key_changes_when_pin_flips() {
        use crate::world_projection::{LandmarkDot, LandmarkKind};
        let dots = vec![
            LandmarkDot {
                pos: Vec2::new(10.0, 20.0),
                kind: LandmarkKind::Citadel,
            },
            LandmarkDot {
                pos: Vec2::new(30.0, 40.0),
                kind: LandmarkKind::Town,
            },
        ];
        let plain = landmark_batch_key(&dots, LandmarkKind::Citadel, false, 1.0, 5.0);
        let pin = landmark_batch_key(&dots, LandmarkKind::Citadel, true, 1.0, 5.0);
        assert_ne!(
            plain.fingerprint, pin.fingerprint,
            "形态翻转必须改变指纹，否则切视图模式复用旧符号 mesh"
        );
        // 位置 / 尺寸 / z 的既有纪律不回归：任一变化都换 key。
        let mut moved = dots.clone();
        moved[0].pos = Vec2::new(11.0, 20.0);
        assert_ne!(
            plain.fingerprint,
            landmark_batch_key(&moved, LandmarkKind::Citadel, false, 1.0, 5.0).fingerprint
        );
        assert_ne!(
            plain.world_thickness,
            landmark_batch_key(&dots, LandmarkKind::Citadel, false, 2.0, 5.0).world_thickness
        );
    }

    /// 空符号集（该 kind 无点）时 pin 两态的指纹仍可区分（pin=true 的
    /// 空集不该命中 pin=false 的空集 mesh——虽然都是空几何不落 mesh，
    /// key 语义上必须分开）。
    #[test]
    fn landmark_batch_key_pin_tag_survives_empty_set() {
        use crate::world_projection::{LandmarkDot, LandmarkKind};
        let dots = vec![LandmarkDot {
            pos: Vec2::new(1.0, 1.0),
            kind: LandmarkKind::Town,
        }];
        let a = landmark_batch_key(&dots, LandmarkKind::Citadel, false, 1.0, 0.0);
        let b = landmark_batch_key(&dots, LandmarkKind::Citadel, true, 1.0, 0.0);
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    use super::*;
    use bevy::render::mesh::VertexAttributeValues;

    /// Google 式道路必须有灰 casing + 白 fill 双层，粗细差即路缘。
    #[test]
    fn google_road_casing_is_thicker_than_fill() {
        let casing = stroke_world_thickness(GOOGLE_ROAD_CASING_PX, 1.0);
        let fill = stroke_world_thickness(GOOGLE_ROAD_FILL_PX, 1.0);
        assert!(
            casing > fill,
            "casing 应厚于 fill：casing={} fill={}，期望 {}px > {}px",
            casing,
            fill,
            GOOGLE_ROAD_CASING_PX,
            GOOGLE_ROAD_FILL_PX
        );
    }

    /// 街道与道路同口径：每档都必须有 casing，粗细差即路缘；且 Z 序要保证 casing
    /// 既不被底图（Z=0.0）盖掉、也不越过自己那档 fill——地图模式下 fill 是白色，
    /// 落在近白底图上唯一看得见的就是这层浅灰 casing。
    #[test]
    fn street_casing_is_thicker_than_its_fill_and_keeps_the_z_order() {
        for (tier, fill_px, fill_z, casing_z) in [
            (
                "主干",
                STREET_MAJOR_PX,
                STREET_MAJOR_Z,
                STREET_MAJOR_CASING_Z,
            ),
            (
                "支路",
                STREET_MINOR_PX,
                STREET_MINOR_Z,
                STREET_MINOR_CASING_Z,
            ),
        ] {
            let casing = stroke_world_thickness(fill_px + STREET_CASING_EXTRA_PX, 1.0);
            let fill = stroke_world_thickness(fill_px, 1.0);
            assert!(
                casing > fill,
                "{tier} 街道 casing 应厚于 fill：casing={} fill={}，期望 {}px > {}px",
                casing,
                fill,
                fill_px + STREET_CASING_EXTRA_PX,
                fill_px
            );
            assert!(
                casing_z < fill_z,
                "{tier} 街道 casing 的 Z（{casing_z}）必须低于自己那档 fill（{fill_z}）"
            );
        }
        // 全序（底图 0.0 < 道路 fill < 主干 casing < …）由模块顶部的
        // `const _: () = assert!(…)` 在**编译期**把关，这里不再重复。
    }

    /// 合批 mesh 的四个角必须以线段两端为「端帽中点」：p0+p3 == 2a，p1+p2 == 2b。
    /// 水平段与斜段各验一遍（斜段防「忘了旋转矩形」的回归）。
    #[test]
    fn batch_mesh_caps_center_on_the_segment_endpoints() {
        let cases = [
            (Vec2::new(100.0, 200.0), Vec2::new(300.0, 200.0)), // 水平
            (Vec2::ZERO, Vec2::new(3.0, 4.0)),                  // 3-4-5 斜段
        ];
        for (a, b) in cases {
            let mesh = build_stroke_batch_mesh(&[(a, b)], 2.0);
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                panic!("合批 mesh 必有 Float32x3 POSITION");
            };
            assert_eq!(positions.len(), 4, "一段 = 4 顶点");
            let v = |i: usize| Vec2::new(positions[i][0], positions[i][1]);
            assert!(
                (v(0) + v(3) - a * 2.0).length() < 1e-4,
                "起端帽中点应落在 a={a:?}，实得 {:?}",
                (v(0) + v(3)) * 0.5
            );
            assert!(
                (v(1) + v(2) - b * 2.0).length() < 1e-4,
                "末端帽中点应落在 b={b:?}，实得 {:?}",
                (v(1) + v(2)) * 0.5
            );
        }
    }

    // ---- 接头（G3b，2026-10-23）：交汇处补圆盘，填掉 butt 端的外角缺口 ----

    fn batch_positions(mesh: &Mesh) -> Vec<Vec2> {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        positions.iter().map(|p| Vec2::new(p[0], p[1])).collect()
    }

    /// 两段共用端点 → 该端点补一块圆盘（1 中心 + 6 边），外角缺口由此填平。
    /// 断言之眼：顶点数 = 2 段 × 4 + 7，且圆心精确落在共用端点上。
    #[test]
    fn shared_endpoint_gets_a_join_disc() {
        let corner = Vec2::new(10.0, 0.0);
        let segs = [
            (Vec2::new(0.0, 0.0), corner),
            (corner, Vec2::new(10.0, 10.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        let pos = batch_positions(&mesh);
        assert_eq!(
            pos.len(),
            2 * 4 + 1 + STROKE_JOIN_SEGMENTS,
            "共用端点应补一块 {STROKE_JOIN_SEGMENTS} 边形圆盘"
        );
        assert!(
            pos.iter().any(|p| (*p - corner).length() < 1e-5),
            "圆盘中心必须是共用端点 {corner:?}"
        );
        // 半径 = 半线宽：只看圆盘自己的顶点（前 8 个是两段的四边形角点，
        // 它们的端帽角到 corner 的距离也正好是半线宽，不能混进来数）。
        let disc = &pos[2 * 4..];
        assert_eq!(disc.len(), 1 + STROKE_JOIN_SEGMENTS, "圆盘 = 圆心 + 环");
        assert!((disc[0] - corner).length() < 1e-5, "disc[0] 是圆心");
        let ring = disc[1..]
            .iter()
            .filter(|p| ((**p - corner).length() - 1.0).abs() < 1e-5)
            .count();
        assert_eq!(ring, STROKE_JOIN_SEGMENTS, "环半径应等于半线宽 1.0");
    }

    /// 互不相接的两段**不得**长接头：否则每个端点都白多一块圆盘。
    #[test]
    fn disjoint_segments_get_no_join_disc() {
        let segs = [
            (Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)),
            (Vec2::new(20.0, 0.0), Vec2::new(30.0, 0.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        assert_eq!(batch_positions(&mesh).len(), 2 * 4, "无共用端点 → 无圆盘");
    }

    /// 折线（3 点 2 段）只有一个拐点 → 只补一块圆盘；顺带钉死「不按点数平方补」。
    #[test]
    fn polyline_corner_gets_exactly_one_join_disc() {
        let p = [
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(20.0, 5.0),
        ];
        let segs = [(p[0], p[1]), (p[1], p[2])];
        let mesh = build_stroke_batch_mesh(&segs, 4.0);
        assert_eq!(
            batch_positions(&mesh).len(),
            2 * 4 + 1 + STROKE_JOIN_SEGMENTS,
            "唯一拐点只补一块圆盘"
        );
    }

    /// **行为断言**（不只是数顶点）：直角拐点的**外角**必须在网格覆盖范围内。
    ///
    /// 几何事实：厚 2 的两段在 (10,0) 直角相接，水平段的 butt 端止于 x=10，
    /// 竖直段止于 y=0 —— 右下外角 `(10.6, -0.6)` 落在两段四边形之外（旧实现的缺口）。
    /// 补圆盘（半径 1）后该点距圆心 0.849 < 1，必须被覆盖。
    #[test]
    fn join_disc_covers_the_outer_notch_at_a_right_angle() {
        let corner = Vec2::new(10.0, 0.0);
        let segs = [
            (Vec2::new(0.0, 0.0), corner),
            (corner, Vec2::new(10.0, 10.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        assert!(
            mesh_covers(&mesh, Vec2::new(10.6, -0.6)),
            "直角外角的楔形缺口必须被接头圆盘填掉"
        );
        // 对照：圆盘半径外的点不该被覆盖（防止「整块糊上去」式修法）。
        assert!(
            !mesh_covers(&mesh, Vec2::new(11.6, -1.6)),
            "半径之外不应有几何——否则是过度填充"
        );
    }

    /// 点在网格（三角形并集）内判定：逐三角形重心坐标同号。
    fn mesh_covers(mesh: &Mesh, p: Vec2) -> bool {
        let pos = batch_positions(mesh);
        let Some(Indices::U32(idx)) = mesh.indices() else {
            panic!("合批 mesh 必有 U32 索引");
        };
        idx.chunks(3).any(|t| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            let s = |u: Vec2, v: Vec2| (v.x - u.x) * (p.y - u.y) - (v.y - u.y) * (p.x - u.x);
            let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
            let has_neg = d1 < -1e-6 || d2 < -1e-6 || d3 < -1e-6;
            let has_pos = d1 > 1e-6 || d2 > 1e-6 || d3 > 1e-6;
            !(has_neg && has_pos)
        })
    }

    /// 接头是 **O(n) 增量、只在重建时发生**：guangdong 规模（17.4 万段网格路网，
    /// 共享端点极密）补完接头后，顶点数严格等于 `4n + 7×接头数`，构建仍在亚秒级。
    /// 稳态每帧不重建（`StrokeBatchKey` 用指纹/线宽/Z 判变），所以这条预算守的是
    /// 「重建一次的代价」，不是帧时间。
    #[test]
    fn join_fill_stays_linear_at_guangdong_scale() {
        let mut segs: Vec<(Vec2, Vec2)> = Vec::with_capacity(295 * 295 * 2);
        for i in 0..295u32 {
            for j in 0..295u32 {
                let a = Vec2::new(i as f32 * 8.0, j as f32 * 8.0);
                segs.push((a, a + Vec2::new(8.0, 0.0)));
                segs.push((a, a + Vec2::new(0.0, 8.0)));
            }
        }
        let joins = stroke_join_centers(&segs, 1.0).len();
        assert!(
            joins * 2 >= segs.len(),
            "网格路网应有大量共享端点，实得 {joins} / {} 段",
            segs.len()
        );
        let t0 = std::time::Instant::now();
        let mesh = build_stroke_batch_mesh(&segs, 2.0);
        let dt = t0.elapsed();
        assert_eq!(
            batch_positions(&mesh).len(),
            segs.len() * 4 + joins * (1 + STROKE_JOIN_SEGMENTS),
            "顶点数必须严格 = 4n + 7×接头数（无隐性重复）"
        );
        // 预算 1s：debug 构建实测百毫秒级；守的是「不退化到秒级」。
        assert!(
            dt.as_secs_f32() < 1.0,
            "17.4 万段补接头应在亚秒级，实得 {dt:?}"
        );
    }

    /// 线宽公式：`world_thickness = stroke_px * view.scale`，屏幕宽度恒定
    /// （`world_thickness / view.scale == stroke_px`），不随缩放变化。
    #[test]
    fn stroke_screen_width_stays_constant_across_zoom() {
        for scale in [0.1, 1.0, 8.0] {
            let world_thickness = stroke_world_thickness(3.0, scale);
            let screen_px = world_thickness / scale;
            assert!(
                (screen_px - 3.0).abs() < 1e-4,
                "scale={scale}: 屏幕宽度应恒为 3px，实得 {screen_px}px"
            );
        }
    }

    #[test]
    fn batch_mesh_handles_zero_length_segment_without_nan() {
        let mesh = build_stroke_batch_mesh(
            &[(Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0))],
            GOOGLE_ROAD_FILL_PX,
        );
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        assert!(positions.iter().all(|p| p.iter().all(|c| c.is_finite())));
    }

    /// N 段 = 4N 顶点 / 6N 索引：合批不是丢段。
    #[test]
    fn batch_mesh_carries_one_quad_per_segment_and_one_disc_per_join() {
        let segs = [
            (Vec2::ZERO, Vec2::X * 10.0),
            (Vec2::X * 10.0, Vec2::new(15.0, 3.0)),
            (Vec2::new(15.0, 3.0), Vec2::new(-4.0, 8.0)),
        ];
        let mesh = build_stroke_batch_mesh(&segs, 1.5);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        // G3b 之后：每段一个四边形（4 顶点 / 6 索引），每处交汇（度数 ≥2 的端点）
        // 再加一块圆盘（1 + 6 顶点 / 18 索引）。这条折线有 2 处交汇。
        let joins = 2;
        assert_eq!(
            positions.len(),
            segs.len() * 4 + joins * (1 + STROKE_JOIN_SEGMENTS),
            "{} 段 = 12 顶点 + {joins} 处接头 × {}",
            segs.len(),
            1 + STROKE_JOIN_SEGMENTS
        );
        match mesh.indices() {
            Some(Indices::U32(idx)) => assert_eq!(
                idx.len(),
                segs.len() * 6 + joins * STROKE_JOIN_SEGMENTS * 3,
                "{} 段 = 18 索引 + {joins} 处接头 × {}",
                segs.len(),
                STROKE_JOIN_SEGMENTS * 3
            ),
            other => panic!("应为 U32 索引，实得 {other:?}"),
        }
    }

    /// 呈现门槛端到端（ADR-0030 D2/D3）：Nation 档只放行 rank 3，密度上限
    /// 截断后保的是高优先级（rank > is_city > 名字长）。
    #[test]
    fn select_pois_gates_by_rank_then_caps_by_priority() {
        use crate::state::LodTier;
        let dot = |rank: u8, is_city: bool, name_len: u32| PlaceDot {
            pos: Vec2::ZERO,
            is_city,
            rank,
            name_len,
        };
        let dots = vec![
            dot(1, false, 4),  // 普通点：Nation 档出局
            dot(3, false, 6),  // 国家级
            dot(2, false, 99), // 市镇级
            dot(3, true, 2),   // 城市级材质的国家级
        ];
        let nation: Vec<&PlaceDot> = select_pois(&dots, LodTier::Nation);
        assert_eq!(nation.len(), 2, "Nation 档只放行 rank 3，实得 {nation:?}");
        assert!(nation.iter().all(|d| d.rank == 3));
        // 上限截断：rank 优先保 3，其次 is_city，再次名字长。
        let many: Vec<PlaceDot> = (0..(POI_MAX_DOTS + 50))
            .map(|i| dot(2, false, i as u32))
            .collect();
        let capped = select_pois(&many, LodTier::Edit);
        assert_eq!(capped.len(), POI_MAX_DOTS, "超预算必须截断到上限");
        let min_kept = capped.iter().map(|d| d.name_len).min();
        assert_eq!(
            min_kept,
            Some(50),
            "450 个候选截到 400，保的是名字最长的 400 个（最短 50）"
        );
    }

    /// POI 点集指纹：坐标位型变化才变，顺序无关性不要求（合批列表本身有序）。
    #[test]
    fn dots_fingerprint_changes_with_positions_only() {
        let a = vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.0)];
        let b = vec![Vec2::new(1.0, 2.0), Vec2::new(3.0, 4.5)];
        assert_ne!(dots_fingerprint(&a), dots_fingerprint(&b), "坐标变指纹必变");
        assert_eq!(
            dots_fingerprint(&a),
            dots_fingerprint(&a.clone()),
            "同集合指纹稳定"
        );
    }

    /// 环带合批的顶点账：N 点 = 24N 顶点 / 72N 索引（12 段共享内外圈）。
    #[test]
    fn poi_ring_batch_carries_twelve_segments_per_dot() {
        let dots = [Vec2::ZERO, Vec2::new(50.0, 20.0), Vec2::new(-30.0, 8.0)];
        let mesh = build_poi_ring_batch_mesh(&dots, 3.0, 5.0);
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("环带合批 mesh 必有 Float32x3 POSITION");
        };
        assert_eq!(positions.len(), dots.len() * 24, "每点 12 段 × 2 圈顶点");
        match mesh.indices() {
            Some(bevy::render::mesh::Indices::U32(idx)) => {
                assert_eq!(idx.len(), dots.len() * 72, "每点 12 段 × 6 索引")
            }
            other => panic!("应为 U32 索引，实得 {other:?}"),
        }
        // 外圈顶点半径必须是 outer_radius（角 0 处的外圈顶点 = 下标 1）。
        assert!(
            (positions[1][0] - 5.0).abs() < 1e-5 && positions[1][1].abs() < 1e-5,
            "外圈顶点应落在 outer_radius 上，实得 {:?}",
            positions[1]
        );
    }

    /// 内芯合批的顶点账：N 点 = 13N 顶点 / 36N 索引（中心 + 12 圈）。
    #[test]
    fn poi_dot_batch_carries_center_fan_per_dot() {
        let dots = [Vec2::ZERO, Vec2::new(10.0, 10.0)];
        let mesh = build_poi_dot_batch_mesh(&dots, 2.5);
        let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("内芯合批 mesh 必有 Float32x3 POSITION");
        };
        assert_eq!(positions.len(), dots.len() * 13, "每点中心 + 12 圈顶点");
        match mesh.indices() {
            Some(bevy::render::mesh::Indices::U32(idx)) => {
                assert_eq!(idx.len(), dots.len() * 36, "每点 12 扇区 × 3 索引")
            }
            other => panic!("应为 U32 索引，实得 {other:?}"),
        }
    }

    /// 线宽量化（ADR-0030 D5b）：漂移 ≤4% 视为未变，>4% 才触发重建。
    #[test]
    fn thickness_quantization_tolerates_four_percent_drift() {
        assert!(!thickness_drifted(6.0, 6.0));
        assert!(!thickness_drifted(6.2, 6.0), "3.3% 漂移在容差内");
        assert!(thickness_drifted(6.3, 6.0), "5% 漂移必须重建");
        assert!(thickness_drifted(5.0, 6.0), "缩小的漂移同样计入");
    }

    /// 同一投影连续两帧不应继续新增 Mesh / Material 资产。
    #[test]
    fn vector_sync_does_not_accumulate_assets_when_projection_is_unchanged() {
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            vec![(3, Vec2::ZERO, Vec2::X * 100.0)],
            vec![PlaceDot {
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
        ));
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.insert_resource(schematic_recipe());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        let material_count = world.resource::<Assets<ColorMaterial>>().iter().count();

        schedule.run(&mut world);
        let next_mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        let next_material_count = world.resource::<Assets<ColorMaterial>>().iter().count();
        assert_eq!(
            next_mesh_count, mesh_count,
            "第二帧 Mesh 资产应稳定：first={mesh_count} second={next_mesh_count}"
        );
        assert_eq!(
            next_material_count, material_count,
            "第二帧 Material 资产应稳定：first={material_count} second={next_material_count}"
        );
    }

    /// 门控（2026-10-22 内容矩阵 T4）：Topographic 下 Roads/Streets/Poi 三 pass
    /// 全 false ⇒ 街网/道路/POI **不产任何 mesh**（含既有池被清）。
    #[test]
    fn topographic_recipe_builds_no_road_street_or_poi_meshes() {
        use crate::recipe::resolve_render_recipe;
        use crate::state::ViewMode;

        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        let view = ViewState {
            mode: ViewMode::Topographic,
            ..Default::default()
        };
        let layers = crate::layer_frame::LayerFrame::default();
        // 真实 recipe：按 Topographic 行解析（非全关 init_resource——那测不出
        // 「接了线」还是「恰好没画」）。view 已在上面对象化，直接借用。
        let recipe = resolve_render_recipe(
            &view,
            &layers,
            &crate::state::WorldProjection::default(),
            &crate::state::DrillState::default(),
            &crate::terrain_view::terrain_texture::TerrainTexture {
                handle: bevy::asset::Handle::default(),
                size: 4,
                height: 4,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "",
                last_hypsometric: 0.0,
                last_map: None,
            },
            None,
        );
        world.insert_resource(view);
        world.insert_resource(layers);
        world.insert_resource(recipe);
        world.insert_resource(GridConfig::default());
        let mut frame = projection_frame_with_roads(
            vec![(3, Vec2::ZERO, Vec2::X * 100.0)],
            vec![PlaceDot {
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
        );
        // 地物符号给了数据：CellMap 里以 **map-pin** 形式在场（2026-10-23
        // 二轮裁定：pin 只用于 CellMap），道路/街网/POI 仍不得产 mesh。
        frame
            .landmark_dots
            .push(crate::world_projection::LandmarkDot {
                pos: Vec2::new(30.0, 30.0),
                kind: crate::world_projection::LandmarkKind::Citadel,
            });
        world.insert_resource(frame);
        // 街道缓存**有数据**——门控必须让它在 Topographic 下画不出来。
        world.insert_resource(crate::streets::StreetLayerCache {
            sources: vec![(
                "test".to_string(),
                wv_core::StreetLayer {
                    segments: vec![wv_core::StreetSegment::straight(
                        wv_core::source::street::RoadClass::Residential,
                        0.0,
                        0.0,
                        50.0,
                        50.0,
                    )],
                },
            )],
            ..Default::default()
        });
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetEditorState>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            mesh_count, 1,
            "Topographic 下道路/街网/POI 不得产 mesh；地物符号只留 1 个 map-pin 合批"
        );
    }

    /// 对照组（防「全关假绿」）：同一夹具切 Schematic（三 pass 全 true）
    /// ⇒ mesh 必须产出来。证明上一条的 0 是门控起效，不是夹具坏了。
    #[test]
    fn schematic_recipe_still_builds_road_street_and_poi_meshes() {
        use crate::recipe::resolve_render_recipe;
        use crate::state::ViewMode;

        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        let view = ViewState {
            mode: ViewMode::Schematic,
            ..Default::default()
        };
        let layers = crate::layer_frame::LayerFrame::default();
        let recipe = resolve_render_recipe(
            &view,
            &layers,
            &crate::state::WorldProjection::default(),
            &crate::state::DrillState::default(),
            &crate::terrain_view::terrain_texture::TerrainTexture {
                handle: bevy::asset::Handle::default(),
                size: 4,
                height: 4,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "",
                last_hypsometric: 0.0,
                last_map: None,
            },
            None,
        );
        world.insert_resource(view);
        world.insert_resource(layers);
        world.insert_resource(recipe);
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            vec![(3, Vec2::ZERO, Vec2::X * 100.0)],
            vec![PlaceDot {
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
        ));
        let cache = crate::streets::StreetLayerCache {
            map_id: None,
            sources: vec![(
                "test".to_string(),
                wv_core::StreetLayer {
                    segments: vec![wv_core::StreetSegment::straight(
                        wv_core::source::street::RoadClass::Residential,
                        0.0,
                        0.0,
                        50.0,
                        50.0,
                    )],
                },
            )],
        };
        world.insert_resource(cache);
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetEditorState>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert!(
            mesh_count > 0,
            "Schematic 下道路/街网/POI 必须照画（对照组）"
        );
    }

    /// 回归（用户报告：钻入武昌城「看不到武昌」）：城市**继承**父图底图时，
    /// 栅格原点仍是世界图原点 (0,0)，而街道坐标是**本图局部**坐标，必须按
    /// **本图图帧原点**换算。曾用 `raster_origin` 换算，1024² 城市的街道被画到
    /// (0..1024)——即世界图左上角，钻进去之后的视野之外，于是「一条街都看不到」。
    #[test]
    fn street_strokes_use_the_map_frame_origin_when_the_raster_is_inherited() {
        // 城市图帧原点（由父图钻入图标派生）= (1536,1536)；显示的栅格仍是世界图 → (0,0)。
        let map_origin = Vec2::new(1536.0, 1536.0);
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::ZERO,
            map_origin,
            map_size: Vec2::splat(1024.0),
        });
        world.insert_resource(ProjectionFrame::default());
        world.init_resource::<VectorLayerEntities>();
        world.insert_resource(crate::streets::StreetLayerCache {
            map_id: Some(wv_core::Id::new_unchecked("map_city")),
            sources: vec![(
                "maps/city_wuchang/roads.yaml".to_owned(),
                wv_core::StreetLayer {
                    segments: vec![wv_core::StreetSegment::straight(
                        wv_core::RoadClass::Primary,
                        100.0,
                        200.0,
                        300.0,
                        200.0,
                    )],
                },
            )],
        });
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.insert_resource(schematic_recipe());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        // 断言走合批 mesh 的顶点：1 段 = 4 顶点，端帽中点必须落在
        // 「本图局部坐标 + 城市帧原点」换算后的端点上（若误用栅格原点，
        // 会落在 (200,200)/(400,200)——即世界图左上角，视野之外）。
        let layer = world.resource::<VectorLayerEntities>();
        let batch = layer
            .street_major
            .batch
            .as_ref()
            .expect("应画出 1 段主干道（合批）");
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&batch.mesh)
            .expect("主干道合批 mesh 应在资产表中");
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("合批 mesh 必有 Float32x3 POSITION");
        };
        assert_eq!(positions.len(), 4, "1 段 = 4 顶点");
        let v = |i: usize| Vec2::new(positions[i][0], positions[i][1]);
        let a = map_origin + Vec2::new(100.0, 200.0);
        let b = map_origin + Vec2::new(300.0, 200.0);
        assert!(
            (v(0) + v(3) - a * 2.0).length() < 1e-3,
            "起端帽中点应落在城市帧 {a:?}；实得 {:?}（若误用栅格原点会得到 (200,200)）",
            (v(0) + v(3)) * 0.5
        );
        assert!(
            (v(1) + v(2) - b * 2.0).length() < 1e-3,
            "末端帽中点应落在城市帧 {b:?}；实得 {:?}",
            (v(1) + v(2)) * 0.5
        );
    }

    /// 合批世界（A2 测试基建）：路网段列 + 全默认资源，跑一次 sync 前的最小装配。
    /// 测试夹具专用：Schematic 行的 recipe（Roads/Streets/Poi 全开）——
    /// 合批/档位/原点换算类测试的意图是「画法」，不是门控。
    fn schematic_recipe() -> crate::recipe::RenderRecipe {
        crate::recipe::resolve_render_recipe(
            &ViewState::default(),
            &crate::layer_frame::LayerFrame::default(),
            &crate::state::WorldProjection::default(),
            &crate::state::DrillState::default(),
            &crate::terrain_view::terrain_texture::TerrainTexture {
                handle: bevy::asset::Handle::default(),
                size: 4,
                height: 4,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "",
                last_hypsometric: 0.0,
                last_map: None,
            },
            None,
        )
    }

    fn world_with_roads(segments: Vec<(Vec2, Vec2)>) -> World {
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            segments.into_iter().map(|(a, b)| (3u8, a, b)).collect(),
            Vec::new(),
        ));
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        // 2026-10-22 内容矩阵后 sync_vector_layers 消费 recipe——夹具按
        // Schematic 行解析（Roads/Streets/Poi 全 true），保住「合批 / 档位」
        // 类测试的意图；门控行为由本模块新加的两条专测覆盖。
        world.insert_resource(schematic_recipe());
        world
    }

    /// 带道路的投影帧（ADR-0047 D4 测试装配）：带级别段列经
    /// `build_road_tier_view` 算齐切片位与三档指纹——与 `compute_projection`
    /// 同一构建入口，测试数据不会绕过档位切片语义。
    fn projection_frame_with_roads(
        ranked: Vec<(u8, Vec2, Vec2)>,
        place_dots: Vec<PlaceDot>,
    ) -> ProjectionFrame {
        let (segments, cuts, fingerprints) = crate::world_projection::build_road_tier_view(ranked);
        ProjectionFrame {
            road_segments: segments,
            road_rank_cuts: cuts,
            road_tier_fingerprints: fingerprints,
            road_fingerprint: fingerprints[0],
            place_dots,
            ..Default::default()
        }
    }

    /// 3 段路网只产生 2 个 Mesh（casing + fill 各一个合批），不是每段 2 个。
    /// 旧实现（每段一个 Rectangle mesh）在同装配下是 6 个——本测试钉死合批语义。
    #[test]
    fn road_batch_uses_one_mesh_per_pass_not_one_per_segment() {
        let mut world = world_with_roads(vec![
            (Vec2::ZERO, Vec2::X * 100.0),
            (Vec2::X * 100.0, Vec2::new(140.0, 30.0)),
            (Vec2::new(140.0, 30.0), Vec2::new(-20.0, 80.0)),
        ]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            mesh_count, 2,
            "3 段应合批成 casing + fill 两个 mesh，实得 {mesh_count}"
        );
    }

    /// 档位门槛端到端（ADR-0047 D4）：同一份混合级别路网（1 干线 + 2 局部），
    /// Nation 档 fill 合批只含 rank ≥ 3 前缀——顶点数必须比 Edit 档少。
    #[test]
    fn nation_tier_draws_only_high_rank_road_prefix() {
        let fill_vertices = |world: &World| -> usize {
            let layer = world.resource::<VectorLayerEntities>();
            let handle = &layer
                .roads
                .batch
                .as_ref()
                .expect("路网非空，应有 fill 合批")
                .mesh;
            world
                .resource::<Assets<Mesh>>()
                .get(handle)
                .expect("fill mesh 在册")
                .count_vertices()
        };

        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(projection_frame_with_roads(
            vec![
                (3u8, Vec2::ZERO, Vec2::X * 100.0),
                (1u8, Vec2::X * 100.0, Vec2::new(140.0, 30.0)),
                (1u8, Vec2::new(140.0, 30.0), Vec2::new(-20.0, 80.0)),
            ],
            Vec::new(),
        ));
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.insert_resource(schematic_recipe());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        // ViewState::default() 的 scale=1.0 恰落在 Region 档（阈值含闭区间），
        // 先显式拉到 Edit 档再对比。
        world.resource_mut::<ViewState>().scale = 0.5;
        schedule.run(&mut world);
        let edit_vertices = fill_vertices(&world);

        world.resource_mut::<ViewState>().scale = 4.0; // Nation 档
        schedule.run(&mut world);
        let nation_vertices = fill_vertices(&world);

        assert!(
            nation_vertices < edit_vertices,
            "Nation 档只画 rank≥3 前缀（1/3 段），顶点应更少：edit={edit_vertices} nation={nation_vertices}"
        );
    }

    /// 段列内容变化：整组换 mesh（旧的移除、新的加入），资产数不累积。
    #[test]
    fn road_batch_replaces_its_mesh_when_segments_change() {
        let mut world = world_with_roads(vec![(Vec2::ZERO, Vec2::X * 100.0)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let old_handle = world
            .resource::<VectorLayerEntities>()
            .roads
            .batch
            .as_ref()
            .expect("首帧应有 fill 合批")
            .mesh
            .clone();

        let next = [(Vec2::ZERO, Vec2::new(50.0, 60.0))];
        *world.resource_mut::<ProjectionFrame>() =
            projection_frame_with_roads(vec![(3u8, next[0].0, next[0].1)], Vec::new());
        schedule.run(&mut world);

        let layer = world.resource::<VectorLayerEntities>();
        let new_handle = &layer
            .roads
            .batch
            .as_ref()
            .expect("次帧仍应有 fill 合批")
            .mesh;
        assert_ne!(
            new_handle, &old_handle,
            "内容变了必须换 mesh（否则画的是旧几何）"
        );
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(mesh_count, 2, "换 mesh 是替换不是累积，实得 {mesh_count}");
    }

    /// 缩放一步（线宽 key 变化）：整组换 mesh，资产数不累积。
    #[test]
    fn road_batch_swaps_mesh_on_zoom_without_accumulating() {
        let mut world = world_with_roads(vec![(Vec2::ZERO, Vec2::X * 100.0)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let old_handle = world
            .resource::<VectorLayerEntities>()
            .roads
            .batch
            .as_ref()
            .expect("首帧应有 fill 合批")
            .mesh
            .clone();

        world.resource_mut::<ViewState>().scale = 2.0;
        schedule.run(&mut world);

        let layer = world.resource::<VectorLayerEntities>();
        let new_handle = &layer
            .roads
            .batch
            .as_ref()
            .expect("缩放后仍应有 fill 合批")
            .mesh;
        assert_ne!(new_handle, &old_handle, "线宽变了必须换 mesh");
        let mesh_count = world.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            mesh_count, 2,
            "缩放换 mesh 是替换不是累积，实得 {mesh_count}"
        );
    }

    /// 线宽量化（ADR-0030 D5b）：缩放漂移 ≤4% 时**不**换 mesh（几何保留），
    /// 超过 4% 的下一步才换——连续缩放下重建频率因此降约 3 倍。
    #[test]
    fn road_batch_keeps_mesh_within_four_percent_thickness_drift() {
        let mut world = world_with_roads(vec![(Vec2::ZERO, Vec2::X * 100.0)]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let baked = world
            .resource::<VectorLayerEntities>()
            .roads
            .batch
            .as_ref()
            .expect("首帧应有 fill 合批")
            .mesh
            .clone();

        world.resource_mut::<ViewState>().scale = 1.02;
        schedule.run(&mut world);
        let after_small = world
            .resource::<VectorLayerEntities>()
            .roads
            .batch
            .as_ref()
            .expect("2% 缩放后仍应有 fill 合批")
            .mesh
            .clone();
        assert_eq!(after_small, baked, "2% 线宽漂移在容差内，不得换 mesh");

        world.resource_mut::<ViewState>().scale = 1.10;
        schedule.run(&mut world);
        let after_big = world
            .resource::<VectorLayerEntities>()
            .roads
            .batch
            .as_ref()
            .expect("10% 缩放后仍应有 fill 合批")
            .mesh
            .clone();
        assert_ne!(after_big, baked, "10% 线宽漂移必须换 mesh");
    }

    /// 带地物符号的夹具：复用道路夹具的资源装配，只把 `landmark_dots`
    /// 填上（道路留空——本组测试只关心符号 mesh）。
    fn world_with_landmarks(dots: Vec<crate::world_projection::LandmarkDot>) -> World {
        let mut world = world_with_roads(Vec::new());
        let mut frame = world
            .remove_resource::<ProjectionFrame>()
            .expect("道路夹具必已插入投影帧");
        frame.landmark_dots = dots;
        world.insert_resource(frame);
        world
    }

    /// Citadel 桶（`landmarks[0]`）当前 mesh 的世界高度（顶点 y 的最大值）。
    fn landmark_mesh_top_y(world: &World) -> f32 {
        let handle = world.resource::<VectorLayerEntities>().landmarks[0]
            .batch
            .as_ref()
            .expect("符号合批应已生成")
            .mesh
            .clone();
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&handle)
            .expect("mesh 资产应存在");
        batch_positions(mesh)
            .iter()
            .fold(f32::MIN, |m, p| m.max(p.y))
    }

    /// 缩放必须换符号 mesh（屏幕恒定口径的落地）：符号几何烘在世界单位
    /// `s = tier × view_scale`，位置不变而缩放变化时 key 必须变——否则
    /// mesh 沿用旧世界尺寸随相机缩放（2026-10-24 CellMap 实测：455% 下
    /// pin 放大 ~4.5 倍成巨型墨团）。
    #[test]
    fn landmark_batch_rebuilds_mesh_when_zoom_changes_symbol_scale() {
        // 锚点贴 y=0：符号向上生长（y+），顶点最大 y = 局部高 × s，不含锚点偏移。
        let mut world = world_with_landmarks(vec![crate::world_projection::LandmarkDot {
            pos: Vec2::ZERO,
            kind: crate::world_projection::LandmarkKind::Citadel,
        }]);
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);
        let baked_height = landmark_mesh_top_y(&world);

        // 455% ⇒ scale ≈ 0.22（百分比 = 1 / scale × 100）。
        world.resource_mut::<ViewState>().scale = 0.22;
        schedule.run(&mut world);
        let zoomed_height = landmark_mesh_top_y(&world);

        // 屏幕恒定 ⇒ 世界高度严格 ∝ tier × scale：Region(1.0) → Edit(0.22)。
        let expected_ratio = (landmark_size_tier(0.22) * 0.22) / (landmark_size_tier(1.0) * 1.0);
        let actual_ratio = zoomed_height / baked_height;
        assert!(
            (actual_ratio - expected_ratio).abs() < 1e-4,
            "缩放后符号 mesh 必须按 tier×scale 重烘：期望比例 {expected_ratio:.4}，\
             实得 {actual_ratio:.4}（首帧高 {baked_height:.2} → 缩放后高 {zoomed_height:.2}；\
             比例≈1 说明 mesh 未重建，即本 bug 的症状）"
        );
    }
}
