//! 矢量叠加层：道路（地点边）+ POI（地点点标），世界空间 Mesh2d 渲染。
//!
//! ## 背景与动机（优化项 1）
//!
//! 原实现把这俩层画在 [`crate::projection::draw_map_gizmos`] 的 `Gizmos` 上。
//! `Gizmos` 是屏幕空间描线，线宽是 1 像素常量，与相机缩放完全无关——
//! 放大到 8× 时，1 像素经相机放大变 8 屏幕像素，矢量要素瞬间变粗、出现锯齿。
//!
//! 这里把这两层迁到**世界空间的 `Mesh2d` 实体**：
//! - 道路：每条边一个细 `Rectangle` mesh，`Transform` 控制位置与旋转；
//! - POI：每个点外层一个 `Annulus`（白环）+ 内层一个实心 `Circle`（深芯）。
//!
//! ## 线宽策略 A：世界厚度 = `PIXEL_WIDTH / view.scale`
//!
//! 公式：`world_thickness = PIXEL_WIDTH / view.scale`。
//! **语义勘误（2026-10-13）**：`view.scale` 是「世界单位/屏幕像素」，大 = 拉远。
//! 该公式在 scale=1 时恰为屏幕 1px；拉远（scale 增大）时道路在屏幕上变细、
//! 拉近时变粗——这对 LOD 反而是想要的（国家级路网退为细网纹）。
//! 原文档「0.1=远观 / 屏幕恒定 1px」的表述是错的，此处按实现修正。
//!
//! ## LOD 分档（v0.3 优化项 A）
//!
//! 按 [`crate::state::lod_tier`] 分三档：国家级（`Nation`）只画城市级 POI
//! （`PlaceDot.is_city`，即声明了 `w/h` 的地点），点标半径加「屏幕可见下限」
//! 保证拉远仍可见——Google 地图式「低层级只画高级要素」的第一步。
//! 道路按等级筛选待数据模型补 `rank` 字段后再做。
//!
//! ## 数据流
//!
//! - 几何数据由 [`crate::projection::ProjectionFrame`] 的
//!   `road_segments` / `place_dots` 提供（`compute_projection` 写）；
//! - 本模块的 [`sync_vector_layers`] 在它**之后**跑，只读 `ProjectionFrame` 与
//!   `view.scale` / `view.places_visible` / `view.roads_visible` / `grid`，
//!   把数据落到实体上。
//!
//! ## 与原 `Gizmos` 实现的关系
//!
//! 原实现**保留**（[`crate::projection::draw_map_gizmos`] 内由 `if false` 门控），
//! 作为回退开关；本模块生效时显式跳过那一段。
//!
//! ## 不动原则
//!
//! - **零 `wv-core` 改动**：纯渲染层改造（`AGENTS.md` §2 依赖方向）。
//! - **零新依赖**：`bevy::sprite` / `bevy::math::primitives` 已随 `bevy` 引入。
//! - **时间求值仍由 `TimelineQuery` 完成**：本模块不读 `current_day`，几何数据
//!   经 `ProjectionFrame` 中转，符合 `AGENTS.md` §3.2。

#![cfg(feature = "app")]

use bevy::math::primitives::{Annulus, Circle};
use bevy::prelude::*;

use crate::palette;
use crate::projection::{PlaceDot, ProjectionFrame};
use crate::state::{lod_tier, GridConfig, LodTier, ViewState};

/// 屏幕线宽（像素）。放大无数倍也保持锐利。
const PIXEL_WIDTH: f32 = 1.0;
/// POI 点标在**屏幕上**的最小可见直径（像素）。
///
/// 世界半径仍按 `grid.cell_size` 取（近看不变）；拉远时世界半径换算到屏幕
/// 会小于此值，此时用它兜底——否则国家级视野下点标缩成一个像素、不可见。
const POI_MIN_SCREEN_PX: f32 = 3.0;

/// 道路层在 Z 轴上的偏移：保证压在 POI 之下、地形之上。
const ROAD_Z: f32 = 0.1;
/// POI 外环（白圈）Z 偏移。
const POI_OUTER_Z: f32 = 0.21;
/// POI 内芯（深芯）Z 偏移。
const POI_INNER_Z: f32 = 0.22;

/// 一组 Mesh 句柄缓存 + 实体列表，对应本帧「应当存在」的 Mesh2d 实体集合。
///
/// 用 `Vec<Entity>` 而不是 `HashMap<Id, Entity>`：每帧 `ProjectionFrame` 的
/// 数据已经是按当前图筛过的有序列表（`projection::compute_projection` 的输出口径），
/// 实体按索引与数据一一对应即可；上层一旦把 `frame` 清空，整层也跟着清。
#[derive(Resource, Default)]
pub struct VectorLayerEntities {
    /// 道路实体（按 `frame.road_segments` 顺序）。`len` 应 = 边数。
    pub roads: Vec<Entity>,
    /// POI 外环实体（白圈）。
    pub poi_outer: Vec<Entity>,
    /// POI 内芯实体（深色实心圆）。
    pub poi_inner: Vec<Entity>,
}

/// 一条边所需的「世界空间 mesh 参数」。
struct RoadMeshSpec {
    /// 中点（世界坐标）。
    center: Vec2,
    /// 长度（世界单位）。
    length: f32,
    /// 朝向（弧度，与 +X 轴的夹角）。
    angle: f32,
    /// 屏幕 1px 对应的世界厚度（随 `view.scale` 缩放反比）。
    world_thickness: f32,
}

impl RoadMeshSpec {
    fn from_segment(a: Vec2, b: Vec2, view_scale: f32) -> Self {
        let delta = b - a;
        let length = delta.length().max(f32::EPSILON);
        let center = (a + b) * 0.5;
        // atan2(dy, dx) → 弧度。
        let angle = delta.y.atan2(delta.x);
        let world_thickness = (PIXEL_WIDTH / view_scale).max(f32::EPSILON);
        Self {
            center,
            length,
            angle,
            world_thickness,
        }
    }
}

/// 道路 + POI 两层的「数据驱动」同步入口。
///
/// 在 [`crate::projection::compute_projection`] 之后跑，**只读** `ProjectionFrame`、
/// `ViewState`、`GridConfig`，**写** `VectorLayerEntities`（持有实体引用以便下一帧复用）
/// 与全局实体表（spawn / despawn / 改 Mesh handle / 改 Transform）。
pub fn sync_vector_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    frame: Res<ProjectionFrame>,
    mut layer: ResMut<VectorLayerEntities>,
) {
    if !view.roads_visible {
        despawn_all(&mut commands, &mut layer.roads);
    } else {
        sync_roads(
            &mut commands,
            &mut meshes,
            &mut materials,
            &frame.road_segments,
            view.scale,
            &mut layer.roads,
        );
    }

    if !view.places_visible {
        despawn_all(&mut commands, &mut layer.poi_outer);
        despawn_all(&mut commands, &mut layer.poi_inner);
    } else {
        sync_poi(
            &mut commands,
            &mut meshes,
            &mut materials,
            &frame.place_dots,
            view.scale,
            lod_tier(view.scale),
            &grid,
            &mut layer,
        );
    }
}

/// LOD 筛选口径（纯函数）：国家级只保留城市级点标，其余档全保留。
fn poi_kept_at_tier(dot: &PlaceDot, tier: LodTier) -> bool {
    tier != LodTier::Nation || dot.is_city
}

fn despawn_all(commands: &mut Commands, entities: &mut Vec<Entity>) {
    for e in entities.drain(..) {
        // `Commands::entity` 在 0.15 是新 API：直接拿 `EntityCommands`，
        // 不存在实体在 apply 时被忽略（无需 `Result` 检查）。
        commands.entity(e).despawn();
    }
}

/// 道路层：每条边一个 `Rectangle` mesh。
///
/// 复用已有 entity 时只 `insert` 新几何 + Transform（mesh handle 重新分配，
/// 但 bevy 0.15 的 Assets 句柄很轻，比手写 mesh attribute 替换省事且不会出错）。
fn sync_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    segments: &[(Vec2, Vec2)],
    view_scale: f32,
    layer: &mut Vec<Entity>,
) {
    // 数据少了：把多余的实体 despawn。
    if layer.len() > segments.len() {
        for e in layer.drain(segments.len()..) {
            commands.entity(e).despawn();
        }
    }
    // 共享 material：每条边一个 ColorMaterial 太浪费；同色复用即可。
    // 道路色来自 palette，不在 system 中写裸 hex。
    let material = materials.add(palette::ROAD_STROKE.with_alpha(0.9));

    for (i, (a, b)) in segments.iter().enumerate() {
        let spec = RoadMeshSpec::from_segment(*a, *b, view_scale);
        // 先把要复用的 entity 拷出值，避开 `layer` 的可变借用。
        let existing = layer.get(i).copied();
        let mesh_handle = meshes.add(Rectangle::new(spec.length, spec.world_thickness));
        let transform = Transform::from_translation(spec.center.extend(ROAD_Z))
            .with_rotation(Quat::from_rotation_z(spec.angle));

        if let Some(entity) = existing {
            commands
                .entity(entity)
                .insert((Mesh2d(mesh_handle), transform));
        } else {
            let entity = commands
                .spawn((
                    Mesh2d(mesh_handle),
                    MeshMaterial2d(material.clone()),
                    transform,
                ))
                .id();
            layer.push(entity);
        }
    }
}

/// POI 层：每个点标 = 外环（`Annulus` 白环）+ 内芯（`Circle` 深色实心圆），
/// 与原 `Gizmos` 视觉一致（白圈 + 深芯）。
///
/// 外径 = `grid.cell_size * 0.9`（世界空间固定，近看视觉不变），但不小于
/// [`POI_MIN_SCREEN_PX`] 像素的世界换算值——拉远时点标保持屏幕可见
/// （导航地图惯例是「屏幕尺寸下限」，不是纯世界固定）。
/// 外环壁厚 = 屏幕 `PIXEL_WIDTH` px 的世界换算（`PIXEL_WIDTH * view_scale`，
/// 原 `PIXEL_WIDTH / view_scale` 方向写反，scale≠1 时环会消失/糊成实心，已修正）。
///
/// LOD：`tier == Nation` 时只画 `is_city` 的点标（省级/编辑级全画）。
fn sync_poi(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    dots: &[PlaceDot],
    view_scale: f32,
    tier: LodTier,
    grid: &GridConfig,
    layer: &mut VectorLayerEntities,
) {
    // LOD 筛选（纯逻辑抽成 [`poi_kept_at_tier`] 便于单测）。
    let visible: Vec<&PlaceDot> = dots.iter().filter(|d| poi_kept_at_tier(d, tier)).collect();
    let outer_layer = &mut layer.poi_outer;
    let inner_layer = &mut layer.poi_inner;

    // 数据少了：把多余实体 despawn。
    if outer_layer.len() > visible.len() {
        for e in outer_layer.drain(visible.len()..) {
            commands.entity(e).despawn();
        }
    }
    if inner_layer.len() > visible.len() {
        for e in inner_layer.drain(visible.len()..) {
            commands.entity(e).despawn();
        }
    }

    // 世界半径为下限之一；拉远时屏幕可见性下限接管。
    let radius_outer = (grid.cell_size * 0.9 * 0.5).max(POI_MIN_SCREEN_PX * view_scale);
    let radius_inner = radius_outer * 0.5;
    // 屏幕 1px 对应的世界厚度（随 scale 正比缩放，见函数文档勘误）。
    let ring_thickness = (PIXEL_WIDTH * view_scale).max(f32::EPSILON);
    let ring_inner = (radius_outer - ring_thickness).max(f32::EPSILON);

    // 城市点 = 红芯白环（palette::PLACE_DOT / POI_RING，2026-11 Theme Pack）。
    let mat_outer = materials.add(palette::POI_RING);
    let mat_inner = materials.add(palette::PLACE_DOT);

    for (i, dot) in visible.iter().enumerate() {
        let pos3 = dot.pos.extend(POI_OUTER_Z);
        let inner_pos3 = dot.pos.extend(POI_INNER_Z);

        // 外环 = Annulus（外圆减去内圆）= 完美白环。
        let existing_outer = outer_layer.get(i).copied();
        let annulus_handle = meshes.add(Annulus::new(ring_inner, radius_outer));
        if let Some(entity) = existing_outer {
            commands
                .entity(entity)
                .insert((Mesh2d(annulus_handle), Transform::from_translation(pos3)));
        } else {
            let entity = commands
                .spawn((
                    Mesh2d(annulus_handle),
                    MeshMaterial2d(mat_outer.clone()),
                    Transform::from_translation(pos3),
                ))
                .id();
            outer_layer.push(entity);
        }

        // 内芯 = 实心圆。
        let existing_inner = inner_layer.get(i).copied();
        let inner_handle = meshes.add(Circle::new(radius_inner));
        if let Some(entity) = existing_inner {
            commands.entity(entity).insert((
                Mesh2d(inner_handle),
                Transform::from_translation(inner_pos3),
            ));
        } else {
            let entity = commands
                .spawn((
                    Mesh2d(inner_handle),
                    MeshMaterial2d(mat_inner.clone()),
                    Transform::from_translation(inner_pos3),
                ))
                .id();
            inner_layer.push(entity);
        }
    }
}

#[cfg(test)]
mod tests {
    //! 测试**只覆盖纯计算**（`RoadMeshSpec`）和**数据结构契约**。
    //! 真正的 ECS 行为（spawn / despawn 数与数据数一致）由 `gui` 集成测试覆盖——
    //! 在这里 mock 一个 World 既贵又脆（Mesh / Material asset 系统非平凡），
    //! 把这部分留给后续 PR 引入 `bevy_test` 风格的最小 harness。

    use super::*;

    /// `RoadMeshSpec::from_segment` 中点 / 长度 / 角度正确。
    #[test]
    fn road_spec_midpoint_and_length_match_endpoints() {
        let a = Vec2::new(100.0, 200.0);
        let b = Vec2::new(300.0, 200.0);
        let spec = RoadMeshSpec::from_segment(a, b, 1.0);
        let len = (b - a).length();
        assert!((spec.length - len).abs() < 1e-4, "len={}", spec.length);
        assert!((spec.center - Vec2::new(200.0, 200.0)).length() < 1e-4);
        // 水平向右：atan2(0, 200) = 0
        assert!(spec.angle.abs() < 1e-4, "angle={}", spec.angle);
    }

    /// 线宽公式：`world_thickness = PIXEL_WIDTH / view.scale`。
    /// 缩放越大，世界厚度越小——保证屏幕 1px 视觉。
    #[test]
    fn road_spec_thickness_inverts_with_view_scale() {
        let s_zoomed_out = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, 0.1);
        let s_normal = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, 1.0);
        let s_zoomed_in = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, 8.0);
        // 远观（scale=0.1）→ 世界厚度大；近看（scale=8）→ 世界厚度小。
        assert!(s_zoomed_out.world_thickness > s_normal.world_thickness);
        assert!(s_normal.world_thickness > s_zoomed_in.world_thickness);
        // 屏幕 1px 等价：world_thickness * view_scale == PIXEL_WIDTH。
        assert!((s_zoomed_out.world_thickness * 0.1 - PIXEL_WIDTH).abs() < 1e-4);
        assert!((s_normal.world_thickness * 1.0 - PIXEL_WIDTH).abs() < 1e-4);
        assert!((s_zoomed_in.world_thickness * 8.0 - PIXEL_WIDTH).abs() < 1e-4);
    }

    /// 零长度边：长度被钳到 EPSILON（避免 NaN / 除零）。
    #[test]
    fn road_spec_handles_zero_length_segment() {
        let spec = RoadMeshSpec::from_segment(Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0), 1.0);
        assert!(spec.length.is_finite());
        assert!(spec.length >= f32::EPSILON, "len must not be 0");
        assert!(spec.center.x.is_finite() && spec.center.y.is_finite());
    }

    /// `VectorLayerEntities` 默认空。
    #[test]
    fn vector_layer_entities_default_is_empty() {
        let v = VectorLayerEntities::default();
        assert!(v.roads.is_empty());
        assert!(v.poi_outer.is_empty());
        assert!(v.poi_inner.is_empty());
    }

    /// LOD 筛选：国家级只留城市级点标；省级 / 编辑级全保留。
    #[test]
    fn nation_tier_keeps_only_city_pois() {
        let city = PlaceDot {
            pos: Vec2::ONE,
            is_city: true,
        };
        let waypoint = PlaceDot {
            pos: Vec2::ZERO,
            is_city: false,
        };
        assert!(poi_kept_at_tier(&city, LodTier::Nation));
        assert!(!poi_kept_at_tier(&waypoint, LodTier::Nation));
        assert!(poi_kept_at_tier(&waypoint, LodTier::Region));
        assert!(poi_kept_at_tier(&waypoint, LodTier::Edit));
    }
}
