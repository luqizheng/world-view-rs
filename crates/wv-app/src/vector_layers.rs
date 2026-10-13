//! Google 式矢量叠加层：道路 casing/fill + 红点白环 POI。
//!
//! ## 资产纪律
//!
//! 本系统每帧运行，但 **Mesh / Material 资产不能每帧新增**：
//! - Material 按视图模式缓存，模式切换才重建；
//! - Mesh 只在几何规格变化时重建，旧 Mesh 用延迟命令在实体换绑后移除；
//! - Transform 仍每帧对齐投影，保证视图拉动式刷新。
//!
//! 这样 Google 式双层道路虽然实体数翻倍，但不会带来无界资产泄漏。

#![cfg(feature = "app")]

use bevy::math::primitives::{Annulus, Circle};
use bevy::prelude::*;

use crate::palette;
use crate::palette_google::{
    GOOGLE_PLACE_DOT, GOOGLE_POI_RING, GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL,
};
use crate::projection::{PlaceDot, ProjectionFrame};
use crate::state::{lod_tier, GridConfig, LodTier, ViewState};

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
/// POI 外环 / 内芯 Z 偏移。
const POI_OUTER_Z: f32 = 0.21;
const POI_INNER_Z: f32 = 0.22;

/// 一条道路 stroke 的几何规格；只有字段变化时才重建 Mesh。
#[derive(Debug, Clone, Copy, PartialEq)]
struct RoadMeshSpec {
    center: Vec2,
    length: f32,
    angle: f32,
    world_thickness: f32,
}

impl RoadMeshSpec {
    fn from_segment(a: Vec2, b: Vec2, view_scale: f32, stroke_px: f32) -> Self {
        let delta = b - a;
        let length = delta.length().max(f32::EPSILON);
        let center = (a + b) * 0.5;
        let angle = delta.y.atan2(delta.x);
        let world_thickness = (stroke_px / view_scale).max(f32::EPSILON);
        Self {
            center,
            length,
            angle,
            world_thickness,
        }
    }
}

/// POI 外环规格。
#[derive(Debug, Clone, Copy, PartialEq)]
struct PoiOuterSpec {
    pos: Vec2,
    inner_radius: f32,
    outer_radius: f32,
}

/// POI 内芯规格。
#[derive(Debug, Clone, Copy, PartialEq)]
struct PoiInnerSpec {
    pos: Vec2,
    radius: f32,
}

/// 按模式缓存的道路材质。
struct RoadMaterialSet {
    mode: crate::state::ViewMode,
    casing: Handle<ColorMaterial>,
    fill: Handle<ColorMaterial>,
}

/// 按模式缓存的 POI 材质。
struct PoiMaterialSet {
    mode: crate::state::ViewMode,
    ring: Handle<ColorMaterial>,
    dot: Handle<ColorMaterial>,
}

/// 实体与 Mesh 句柄的一一对应缓存。
#[derive(Default)]
struct MeshPool {
    entities: Vec<Entity>,
    meshes: Vec<Handle<Mesh>>,
}

impl MeshPool {
    fn despawn(&mut self, commands: &mut Commands) {
        for (entity, handle) in self.entities.drain(..).zip(self.meshes.drain(..)) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&handle);
            });
        }
    }
}

/// 道路 stroke 池。
#[derive(Default)]
struct RoadLayerPool {
    mesh: MeshPool,
    specs: Vec<RoadMeshSpec>,
}

/// POI 外环池。
#[derive(Default)]
struct PoiOuterPool {
    mesh: MeshPool,
    specs: Vec<PoiOuterSpec>,
}

/// POI 内芯池。
#[derive(Default)]
struct PoiInnerPool {
    mesh: MeshPool,
    specs: Vec<PoiInnerSpec>,
}

/// 矢量层实体与可复用资产规格缓存。
#[derive(Resource, Default)]
pub struct VectorLayerEntities {
    road_casings: RoadLayerPool,
    roads: RoadLayerPool,
    poi_outer: PoiOuterPool,
    poi_inner: PoiInnerPool,
    road_materials: Vec<RoadMaterialSet>,
    road_material_mode: Option<crate::state::ViewMode>,
    poi_materials: Vec<PoiMaterialSet>,
    poi_material_mode: Option<crate::state::ViewMode>,
}

/// 每帧同步道路与 POI。系统只消费 `ProjectionFrame`，不修改世界数据。
pub fn sync_vector_layers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    frame: Res<ProjectionFrame>,
    mut layer: ResMut<VectorLayerEntities>,
) {
    let road_materials_changed = ensure_road_materials(&mut materials, view.mode, &mut layer);
    let poi_materials_changed = ensure_poi_materials(&mut materials, view.mode, &mut layer);

    if !view.roads_visible {
        layer.road_casings.mesh.despawn(&mut commands);
        layer.roads.mesh.despawn(&mut commands);
        layer.road_casings.specs.clear();
        layer.roads.specs.clear();
    } else {
        let (casing, fill) = layer
            .road_materials
            .iter()
            .find(|m| m.mode == view.mode)
            .map(|m| (m.casing.clone(), m.fill.clone()))
            .expect("ensure_road_materials 必须已初始化");
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &frame.road_segments,
            view.scale,
            casing,
            GOOGLE_ROAD_CASING_PX,
            ROAD_CASING_Z,
            road_materials_changed,
            &mut layer.road_casings,
        );
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &frame.road_segments,
            view.scale,
            fill,
            GOOGLE_ROAD_FILL_PX,
            ROAD_Z,
            road_materials_changed,
            &mut layer.roads,
        );
    }

    if !view.places_visible {
        layer.poi_outer.mesh.despawn(&mut commands);
        layer.poi_inner.mesh.despawn(&mut commands);
        layer.poi_outer.specs.clear();
        layer.poi_inner.specs.clear();
    } else {
        sync_poi(
            &mut commands,
            &mut meshes,
            &frame.place_dots,
            view.scale,
            lod_tier(view.scale),
            &grid,
            poi_materials_changed,
            &mut layer,
        );
    }
}

/// LOD 口径：国家级只保留城市级 POI，省级 / 编辑级全保留。
fn poi_kept_at_tier(dot: &PlaceDot, tier: LodTier) -> bool {
    tier != LodTier::Nation || dot.is_city
}

fn ensure_road_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    layer: &mut VectorLayerEntities,
) -> bool {
    if layer.road_material_mode == Some(mode) {
        return false;
    }
    if !layer.road_materials.iter().any(|set| set.mode == mode) {
        let (casing_color, fill_color) = match mode {
            crate::state::ViewMode::GoogleMap => (GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL),
            _ => (
                palette::LINE_ON_MID.with_alpha(0.28),
                palette::ROAD_STROKE,
            ),
        };
        layer.road_materials.push(RoadMaterialSet {
            mode,
            casing: materials.add(casing_color),
            fill: materials.add(fill_color),
        });
    }
    layer.road_material_mode = Some(mode);
    true
}

fn ensure_poi_materials(
    materials: &mut Assets<ColorMaterial>,
    mode: crate::state::ViewMode,
    layer: &mut VectorLayerEntities,
) -> bool {
    if layer.poi_material_mode == Some(mode) {
        return false;
    }
    if !layer.poi_materials.iter().any(|set| set.mode == mode) {
        let (ring_color, dot_color) = match mode {
            crate::state::ViewMode::GoogleMap => (GOOGLE_POI_RING, GOOGLE_PLACE_DOT),
            _ => (palette::POI_RING, palette::PLACE_DOT),
        };
        layer.poi_materials.push(PoiMaterialSet {
            mode,
            ring: materials.add(ring_color),
            dot: materials.add(dot_color),
        });
    }
    layer.poi_material_mode = Some(mode);
    true
}

/// 同步一层道路 stroke。规格不变时只更新 Transform，不新增 Mesh 资产。
#[allow(clippy::too_many_arguments)]
fn sync_road_strokes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    segments: &[(Vec2, Vec2)],
    view_scale: f32,
    material: Handle<ColorMaterial>,
    stroke_px: f32,
    z: f32,
    material_changed: bool,
    pool: &mut RoadLayerPool,
) {
    if pool.mesh.entities.len() > segments.len() {
        let keep = segments.len();
        let removed_entities = pool.mesh.entities.split_off(keep);
        let removed_meshes = pool.mesh.meshes.split_off(keep);
        pool.specs.truncate(keep);
        for (entity, handle) in removed_entities.into_iter().zip(removed_meshes) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&handle);
            });
        }
    }

    for (i, (a, b)) in segments.iter().enumerate() {
        let spec = RoadMeshSpec::from_segment(*a, *b, view_scale, stroke_px);
        let transform = Transform::from_translation(spec.center.extend(z))
            .with_rotation(Quat::from_rotation_z(spec.angle));

        if i == pool.mesh.entities.len() {
            let mesh = meshes.add(Rectangle::new(spec.length, spec.world_thickness));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    transform,
                ))
                .id();
            pool.mesh.entities.push(entity);
            pool.mesh.meshes.push(mesh);
            pool.specs.push(spec);
            continue;
        }

        let entity = pool.mesh.entities[i];
        let spec_changed = pool.specs.get(i) != Some(&spec);
        if spec_changed {
            let next = meshes.add(Rectangle::new(spec.length, spec.world_thickness));
            let old = pool.mesh.meshes[i].clone();
            pool.mesh.meshes[i] = next.clone();
            commands.entity(entity).insert(Mesh2d(next));
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&old);
            });
            pool.specs[i] = spec;
        }
        if material_changed {
            commands
                .entity(entity)
                .insert(MeshMaterial2d(material.clone()));
        }
        commands.entity(entity).insert(transform);
    }
}

/// 同步红点白环 POI；Mesh 只在半径变化时重建。
#[allow(clippy::too_many_arguments)]
fn sync_poi(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[PlaceDot],
    view_scale: f32,
    tier: LodTier,
    grid: &GridConfig,
    material_changed: bool,
    layer: &mut VectorLayerEntities,
) {
    let visible: Vec<&PlaceDot> = dots.iter().filter(|d| poi_kept_at_tier(d, tier)).collect();
    let radius_outer = (grid.cell_size * 0.45).max(POI_MIN_SCREEN_PX * view_scale * 0.5);
    let radius_inner = radius_outer * 0.52;
    let ring_thickness = (1.5 * view_scale).max(f32::EPSILON);
    let ring_inner = (radius_outer - ring_thickness).max(f32::EPSILON);
    let mode = layer.poi_material_mode.expect("ensure_poi_materials 必须已初始化");
    let materials = layer
        .poi_materials
        .iter()
        .find(|m| m.mode == mode)
        .map(|m| (m.ring.clone(), m.dot.clone()))
        .expect("ensure_poi_materials 必须已初始化");

    if layer.poi_outer.mesh.entities.len() > visible.len() {
        let keep = visible.len();
        let removed_entities = layer.poi_outer.mesh.entities.split_off(keep);
        let removed_meshes = layer.poi_outer.mesh.meshes.split_off(keep);
        layer.poi_outer.specs.truncate(keep);
        for (entity, handle) in removed_entities.into_iter().zip(removed_meshes) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&handle);
            });
        }
    }
    if layer.poi_inner.mesh.entities.len() > visible.len() {
        let keep = visible.len();
        let removed_entities = layer.poi_inner.mesh.entities.split_off(keep);
        let removed_meshes = layer.poi_inner.mesh.meshes.split_off(keep);
        layer.poi_inner.specs.truncate(keep);
        for (entity, handle) in removed_entities.into_iter().zip(removed_meshes) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&handle);
            });
        }
    }

    for (i, dot) in visible.iter().enumerate() {
        let outer_spec = PoiOuterSpec {
            pos: dot.pos,
            inner_radius: ring_inner,
            outer_radius: radius_outer,
        };
        let inner_spec = PoiInnerSpec {
            pos: dot.pos,
            radius: radius_inner,
        };

        if i == layer.poi_outer.mesh.entities.len() {
            let mesh = meshes.add(Annulus::new(ring_inner, radius_outer));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(materials.0.clone()),
                    Transform::from_translation(dot.pos.extend(POI_OUTER_Z)),
                ))
                .id();
            layer.poi_outer.mesh.entities.push(entity);
            layer.poi_outer.mesh.meshes.push(mesh);
            layer.poi_outer.specs.push(outer_spec);
        } else {
            update_poi_outer(
                commands,
                meshes,
                layer.poi_outer.mesh.entities[i],
                outer_spec,
                material_changed,
                &mut layer.poi_outer.mesh.meshes[i],
                &mut layer.poi_outer.specs[i],
                &materials.0,
            );
        }

        if i == layer.poi_inner.mesh.entities.len() {
            let mesh = meshes.add(Circle::new(radius_inner));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(materials.1.clone()),
                    Transform::from_translation(dot.pos.extend(POI_INNER_Z)),
                ))
                .id();
            layer.poi_inner.mesh.entities.push(entity);
            layer.poi_inner.mesh.meshes.push(mesh);
            layer.poi_inner.specs.push(inner_spec);
        } else {
            update_poi_inner(
                commands,
                meshes,
                layer.poi_inner.mesh.entities[i],
                inner_spec,
                material_changed,
                &mut layer.poi_inner.mesh.meshes[i],
                &mut layer.poi_inner.specs[i],
                &materials.1,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_poi_outer(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    entity: Entity,
    spec: PoiOuterSpec,
    material_changed: bool,
    mesh_handle: &mut Handle<Mesh>,
    cached: &mut PoiOuterSpec,
    material: &Handle<ColorMaterial>,
) {
    if *cached != spec {
        let next = meshes.add(Annulus::new(spec.inner_radius, spec.outer_radius));
        let old = mesh_handle.clone();
        *mesh_handle = next.clone();
        commands.entity(entity).insert(Mesh2d(next));
        commands.queue(move |world: &mut World| {
            world.resource_mut::<Assets<Mesh>>().remove(&old);
        });
        *cached = spec;
    }
    if material_changed {
        commands
            .entity(entity)
            .insert(MeshMaterial2d(material.clone()));
    }
    commands
        .entity(entity)
        .insert(Transform::from_translation(spec.pos.extend(POI_OUTER_Z)));
}

#[allow(clippy::too_many_arguments)]
fn update_poi_inner(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    entity: Entity,
    spec: PoiInnerSpec,
    material_changed: bool,
    mesh_handle: &mut Handle<Mesh>,
    cached: &mut PoiInnerSpec,
    material: &Handle<ColorMaterial>,
) {
    if *cached != spec {
        let next = meshes.add(Circle::new(spec.radius));
        let old = mesh_handle.clone();
        *mesh_handle = next.clone();
        commands.entity(entity).insert(Mesh2d(next));
        commands.queue(move |world: &mut World| {
            world.resource_mut::<Assets<Mesh>>().remove(&old);
        });
        *cached = spec;
    }
    if material_changed {
        commands
            .entity(entity)
            .insert(MeshMaterial2d(material.clone()));
    }
    commands
        .entity(entity)
        .insert(Transform::from_translation(spec.pos.extend(POI_INNER_Z)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Google 式道路必须有灰 casing + 白 fill 双层，粗细差即路缘。
    #[test]
    fn google_road_casing_is_thicker_than_fill() {
        let a = Vec2::ZERO;
        let b = Vec2::X * 100.0;
        let casing = RoadMeshSpec::from_segment(a, b, 1.0, GOOGLE_ROAD_CASING_PX);
        let fill = RoadMeshSpec::from_segment(a, b, 1.0, GOOGLE_ROAD_FILL_PX);
        assert!(
            casing.world_thickness > fill.world_thickness,
            "casing 应厚于 fill：casing={} fill={}，期望 {}px > {}px",
            casing.world_thickness,
            fill.world_thickness,
            GOOGLE_ROAD_CASING_PX,
            GOOGLE_ROAD_FILL_PX
        );
    }

    #[test]
    fn road_spec_midpoint_and_length_match_endpoints() {
        let a = Vec2::new(100.0, 200.0);
        let b = Vec2::new(300.0, 200.0);
        let spec = RoadMeshSpec::from_segment(a, b, 1.0, GOOGLE_ROAD_FILL_PX);
        let len = (b - a).length();
        assert!((spec.length - len).abs() < 1e-4, "len={}", len);
        assert!((spec.center - Vec2::new(200.0, 200.0)).length() < 1e-4);
        assert!(spec.angle.abs() < 1e-4, "angle={}", spec.angle);
    }

    /// 线宽公式：`world_thickness = stroke_px / view.scale`。
    #[test]
    fn road_spec_thickness_inverts_with_view_scale() {
        let s_zoomed_out = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, 0.1, 3.0);
        let s_normal = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, 1.0, 3.0);
        let s_zoomed_in = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, 8.0, 3.0);
        assert!(s_zoomed_out.world_thickness > s_normal.world_thickness);
        assert!(s_normal.world_thickness > s_zoomed_in.world_thickness);
        assert!((s_zoomed_out.world_thickness * 0.1 - 3.0).abs() < 1e-4);
        assert!((s_normal.world_thickness * 1.0 - 3.0).abs() < 1e-4);
        assert!((s_zoomed_in.world_thickness * 8.0 - 3.0).abs() < 1e-4);
    }

    #[test]
    fn road_spec_handles_zero_length_segment() {
        let spec = RoadMeshSpec::from_segment(
            Vec2::new(5.0, 5.0),
            Vec2::new(5.0, 5.0),
            1.0,
            GOOGLE_ROAD_FILL_PX,
        );
        assert!(spec.length.is_finite());
        assert!(spec.length >= f32::EPSILON, "len must not be 0");
    }

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

    /// 同一投影连续两帧不应继续新增 Mesh / Material 资产。
    #[test]
    fn vector_sync_does_not_accumulate_assets_when_projection_is_unchanged() {
        let mut world = World::default();
        world.insert_resource(Assets::<Mesh>::default());
        world.insert_resource(Assets::<ColorMaterial>::default());
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig::default());
        world.insert_resource(ProjectionFrame {
            road_segments: vec![(Vec2::ZERO, Vec2::X * 100.0)],
            place_dots: vec![PlaceDot {
                pos: Vec2::new(20.0, 20.0),
                is_city: true,
            }],
            ..Default::default()
        });
        world.init_resource::<VectorLayerEntities>();

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
}
