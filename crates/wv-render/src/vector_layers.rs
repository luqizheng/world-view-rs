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
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(n * 6);
    let half = world_thickness * 0.5;
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
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
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
    /// Sprite 路径的实体池（PNG 加载成功后启用；长度 = 当前可见符号数）。
    sprites: Vec<Entity>,
}

impl LandmarkPool {
    fn despawn(&mut self, commands: &mut Commands) {
        for e in self.sprites.drain(..) {
            commands.entity(e).despawn();
        }
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
    /// 地物符号合批（每类一个 mesh + 一个实体，共 2 个）；与 decor 同款
    /// 「原地改写」纪律。顶点色已在 mesh 端烘好 → 配白色 ColorMaterial。
    landmarks: [LandmarkPool; 2],
    /// 地物符号材质（白色，顶点色承载全部颜色；惰性建一次防资产累积）。
    landmark_material: Option<Handle<ColorMaterial>>,
    /// 地物符号 PNG（`symbols/*.png`，豆包生成手绘水彩）。`None` = 尚未发起加载。
    /// 某类 PNG 加载完成后该类走 Sprite 路径，mesh 版自动退位（兜底保留）。
    landmark_png: Option<[Handle<Image>; 2]>,
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
    images: Res<Assets<Image>>,
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
        if recipe.enabled(PassId::Rivers) && layers.terrain.visible {
            let mat = layer.river_material.get_or_insert_with(|| {
                materials.add(ColorMaterial::from_color(Color::srgb(0.29, 0.53, 0.68)))
            });
            let mut river_segments: Vec<(Vec2, Vec2)> = Vec::new();
            for pts in &frame.river_polylines {
                for pair in pts.windows(2) {
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
        } else {
            layer.rivers.despawn(&mut commands);
        }
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &frame.road_segments,
            frame.road_fingerprint,
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
            &frame.road_segments,
            frame.road_fingerprint,
            view.scale,
            fill.clone(),
            GOOGLE_ROAD_FILL_PX,
            ROAD_Z + road_z,
            road_materials_changed,
            &mut layer.roads,
        );
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
    }

    if !layers.place.visible {
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

        // 地物符号（rank ≥ 2 程序化插画）。
        // Gate：与 POI 同一图层（place）控制，recipe 路径放 MODE_TABLE 后续接。
        if layers.place.visible {
            let white = layer
                .landmark_material
                .get_or_insert_with(|| materials.add(ColorMaterial::from_color(Color::WHITE)))
                .clone();
            let VectorLayerEntities {
                landmarks,
                landmark_png,
                ..
            } = &mut *layer;
            sync_landmarks(
                &mut commands,
                &mut meshes,
                &images,
                landmarks,
                landmark_png,
                &frame.landmark_dots,
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
/// `fingerprint` 由调用方给定（路段列从 `ProjectionFrame.road_fingerprint`
/// 现成拿来；街道 / 预览段列就地 [`segments_fingerprint`]）——同一组段在
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
    images: &Assets<Image>,
    landmark: &mut [LandmarkPool; 2],
    png: &Option<[Handle<Image>; 2]>,
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    view_scale: f32,
    z: f32,
    material: Handle<ColorMaterial>,
) {
    use crate::landmark_symbols::BASE_H_PX;
    use crate::state::lod_tier;
    use crate::world_projection::LANDMARK_KINDS;
    // 尺寸口径（ADR-0042 修正）：**屏幕像素恒定**，按 LOD 档微调——旧公式
    // `BASE_H_PX * 0.04 * scale` 折算屏幕恒 1.04px，城堡图标从未真正显示过
    //（实机截图确认）。参考图全球档城堡 ~30px；档位越近稍大（贴近
    // Google Maps marker 的恒定手感）。
    let side_px = match lod_tier(view_scale) {
        crate::state::LodTier::Nation => BASE_H_PX,
        crate::state::LodTier::Region => BASE_H_PX * 1.25,
        crate::state::LodTier::Edit => BASE_H_PX * 1.5,
    };
    // 符号画布边长（世界单位）= 屏幕像素 × 每像素世界单位。
    let side_wu = side_px * view_scale;

    // PNG 句柄由 `ensure_landmark_png_loaded` 发起；`None`（测试夹具 / ensure
    // 未注册）→ 走顶点色 mesh 兜底路径。
    let Some(handles) = png.as_ref() else {
        for (i, kind) in LANDMARK_KINDS.iter().copied().enumerate() {
            sync_landmark_mesh_batches(
                commands,
                meshes,
                &mut landmark[i],
                frame_landmarks,
                kind,
                view_scale,
                z,
                material.clone(),
            );
        }
        return;
    };

    for (i, kind) in LANDMARK_KINDS.iter().copied().enumerate() {
        let pool = &mut landmark[i];
        let want: Vec<Vec2> = frame_landmarks
            .iter()
            .filter(|d| d.kind == kind)
            .map(|d| d.pos)
            .collect();

        // PNG 已加载 → Sprite 路径（每符号一个实体，同纹理由渲染后端合批）。
        if images.contains(&handles[i]) {
            // mesh 兜底实体下线。
            if let Some(batch) = pool.batch.take() {
                commands.entity(batch.entity).despawn();
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Mesh>>().remove(&batch.mesh);
                });
            }
            // 长度对齐：少的 spawn，多的 despawn。
            while pool.sprites.len() > want.len() {
                if let Some(e) = pool.sprites.pop() {
                    commands.entity(e).despawn();
                }
            }
            for (j, pos) in want.iter().enumerate() {
                if j < pool.sprites.len() {
                    commands.entity(pool.sprites[j]).insert((
                        Sprite {
                            image: handles[i].clone(),
                            custom_size: Some(Vec2::splat(side_wu)),
                            ..default()
                        },
                        bevy::sprite::Anchor::BOTTOM_CENTER,
                        Transform::from_xyz(pos.x, pos.y, z),
                    ));
                } else {
                    let e = commands
                        .spawn((
                            Sprite {
                                image: handles[i].clone(),
                                custom_size: Some(Vec2::splat(side_wu)),
                                ..default()
                            },
                            bevy::sprite::Anchor::BOTTOM_CENTER,
                            Transform::from_xyz(pos.x, pos.y, z),
                        ))
                        .id();
                    pool.sprites.push(e);
                }
            }
            continue;
        }

        sync_landmark_mesh_batches(
            commands,
            meshes,
            pool,
            frame_landmarks,
            kind,
            view_scale,
            z,
            material.clone(),
        );
    }
}

/// 单类符号的顶点色 mesh 兜底合批（从 sync_landmarks 拆出；PNG 未就绪时走此路）。
#[allow(clippy::too_many_arguments)]
fn sync_landmark_mesh_batches(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    pool: &mut LandmarkPool,
    frame_landmarks: &[crate::world_projection::LandmarkDot],
    kind: crate::world_projection::LandmarkKind,
    view_scale: f32,
    z: f32,
    material: Handle<ColorMaterial>,
) {
    use crate::landmark_symbols::{build_landmark_mesh, LandmarkBatch, BASE_H_PX};
    use crate::state::lod_tier;
    for e in pool.sprites.drain(..) {
        commands.entity(e).despawn();
    }
    // 尺寸口径与 Sprite 路径一致（ADR-0042）：屏幕像素恒定按档位微调。
    let side_px = match lod_tier(view_scale) {
        crate::state::LodTier::Nation => BASE_H_PX,
        crate::state::LodTier::Region => BASE_H_PX * 1.25,
        crate::state::LodTier::Edit => BASE_H_PX * 1.5,
    };
    let mut batch = LandmarkBatch::new();
    for d in frame_landmarks {
        if d.kind == kind {
            let s = side_px * view_scale;
            match d.kind {
                crate::world_projection::LandmarkKind::Citadel => batch.push_citadel(d.pos, s),
                crate::world_projection::LandmarkKind::Town => batch.push_town(d.pos, s),
            }
        }
    }
    let fingerprint = points_fingerprint_dots(frame_landmarks, kind);
    let key = StrokeBatchKey {
        fingerprint,
        world_thickness: 1.0,
        z,
    };
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

/// 地物符号 PNG（豆包手绘水彩，256² 透底）。编译期嵌入——与 ADR-0019 打包
/// 字体同款纪律：Bevy 0.19 的 asset root 在 exe 旁 `target/debug/assets/`，
/// 文件加载对「从任意目录启动」不可靠（P1 单机可运行优先），且测试夹具
/// 无法构造 AssetServer。句柄由 [`ensure_landmark_png_loaded`] 首帧解码插入。
pub const CITADEL_PNG: &[u8] = include_bytes!("../assets/symbols/citadel.png");
pub const TOWN_PNG: &[u8] = include_bytes!("../assets/symbols/town.png");

/// 解码嵌入 PNG → `Assets<Image>`（一次），句柄存入 `landmark_png`。
///
/// 与 [`sync_vector_layers`] 拆开：裸测试 World 不注册本 system →
/// `landmark_png` 保持 `None` → 符号走 mesh 兜底，向量层测试无需图像资源。
pub fn ensure_landmark_png_loaded(
    mut images: ResMut<Assets<Image>>,
    mut layer: ResMut<VectorLayerEntities>,
) {
    if layer.landmark_png.is_some() {
        return;
    }
    let decode = |bytes: &[u8]| {
        Image::from_buffer(
            bytes,
            bevy::image::ImageType::Extension("png"),
            bevy::image::CompressedImageFormats::NONE,
            true,
            bevy::image::ImageSampler::Default,
            bevy::asset::RenderAssetUsages::RENDER_WORLD
                | bevy::asset::RenderAssetUsages::MAIN_WORLD,
        )
        .ok()
    };
    if let (Some(a), Some(t)) = (decode(CITADEL_PNG), decode(TOWN_PNG)) {
        layer.landmark_png = Some([images.add(a), images.add(t)]);
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
    fn batch_mesh_carries_one_quad_per_segment() {
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
        assert_eq!(
            positions.len(),
            segs.len() * 4,
            "{} 段 = 12 顶点",
            segs.len()
        );
        match mesh.indices() {
            Some(Indices::U32(idx)) => {
                assert_eq!(idx.len(), segs.len() * 6, "{} 段 = 18 索引", segs.len())
            }
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
        world.insert_resource(ProjectionFrame {
            road_segments: vec![(Vec2::ZERO, Vec2::X * 100.0)],
            place_dots: vec![PlaceDot {
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
                rank: 3,
                name_len: 3,
            }],
            ..Default::default()
        });
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.init_resource::<crate::recipe::RenderRecipe>();

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
        world.init_resource::<crate::recipe::RenderRecipe>();

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
    fn world_with_roads(segments: Vec<(Vec2, Vec2)>) -> World {
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(ProjectionFrame {
            road_fingerprint: segments_fingerprint(&segments),
            road_segments: segments,
            ..Default::default()
        });
        world.init_resource::<VectorLayerEntities>();
        world.init_resource::<crate::streets::StreetLayerCache>();
        world.init_resource::<crate::streets::StreetEditorState>();
        world.init_resource::<crate::layer_frame::LayerFrame>();
        world.init_resource::<crate::recipe::RenderRecipe>();
        world
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

        let next = vec![(Vec2::ZERO, Vec2::new(50.0, 60.0))];
        world.resource_mut::<ProjectionFrame>().road_segments = next.clone();
        world.resource_mut::<ProjectionFrame>().road_fingerprint = segments_fingerprint(&next);
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
}
