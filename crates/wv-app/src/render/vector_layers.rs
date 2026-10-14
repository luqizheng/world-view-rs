//! Google 式矢量叠加层：道路 casing/fill + 红点白环 POI。
//!
//! ## 职责
//! - 在地形之上叠画 Google Maps 风格的视觉层：道路外壳（casing）+ 道路主体（fill），
//!   以及 POI 的红点白环 marker。
//! - 用 `PlaceDot` 组件驱动可见性 / LOD：在当前 zoom 下超界的点会随缩放增删，
//!   避免 `wgpu::Batch` 节点数随 POI 总数失控。
//!
//! ## 边界（不做什么）
//! - **不画地形底图**——那是 [`crate::render::terrain_view`] + [`crate::theme::palette`]。
//! - **不投影地理坐标**——只消费 [`crate::render::world_projection::ProjectionFrame`]。
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

use bevy::math::primitives::{Annulus, Circle};
use bevy::prelude::*;

use crate::render::layer_frame::LayerFrame;
use crate::render::world_projection::{PlaceDot, ProjectionFrame};
use crate::state::{lod_tier, GridConfig, LodTier, ViewState};
use crate::theme::colors_google::{
    GOOGLE_PLACE_DOT, GOOGLE_POI_RING, GOOGLE_ROAD_CASING, GOOGLE_ROAD_FILL,
};
use crate::theme::palette::{self, MapPalette, PaletteColors};

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
        // 屏幕像素 → 世界单位：`world = px * view_scale`。`view.scale` 的单位是
        // **世界单位/像素**（见 `ViewState::scale` 的文档与 `terrain_chunks.rs` 的视口换算），
        // 所以恒定的屏幕线宽必须**乘** scale。历史上这里写成 `px / view_scale`，
        // 屏幕宽度实际变成 `stroke_px / scale²`：远观时细到几乎消失，深缩时胀成
        // 占屏 40% 的巨带。
        let world_thickness = (stroke_px * view_scale).max(f32::EPSILON);
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
    palette: MapPalette,
    casing: Handle<ColorMaterial>,
    fill: Handle<ColorMaterial>,
}

/// 按模式缓存的 POI 材质。
struct PoiMaterialSet {
    mode: crate::state::ViewMode,
    palette: MapPalette,
    ring: Handle<ColorMaterial>,
    dot: Handle<ColorMaterial>,
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
    poi_outer: PoiOuterPool,
    poi_inner: PoiInnerPool,
    road_materials: Vec<RoadMaterialSet>,
    road_material_mode: Option<crate::state::ViewMode>,
    poi_materials: Vec<PoiMaterialSet>,
    poi_material_mode: Option<crate::state::ViewMode>,
    /// 街道分级材质：主干 / 次干各 casing+fill（古舆图双色，ADR-0014）。
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
    streets: Res<crate::render::streets::StreetLayerCache>,
    editor: Res<crate::render::streets::StreetEditorState>,
    mut layer: ResMut<VectorLayerEntities>,
) {
    let road_materials_changed = ensure_road_materials(
        &mut materials,
        view.mode,
        view.map_palette,
        layers.road.opacity,
        &mut layer,
    );
    let poi_materials_changed = ensure_poi_materials(
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
        apply_poi_material_alpha(&mut materials, &layer, layers.place.opacity);
    }
    let road_z = layers.road.z_offset;
    let place_z = layers.place.z_offset;

    if !layers.road.visible {
        layer.road_casings.mesh.despawn(&mut commands);
        layer.roads.mesh.despawn(&mut commands);
        layer.road_casings.specs.clear();
        layer.roads.specs.clear();
        layer.street_major.mesh.despawn(&mut commands);
        layer.street_minor.mesh.despawn(&mut commands);
        layer.street_major_casing.mesh.despawn(&mut commands);
        layer.street_minor_casing.mesh.despawn(&mut commands);
        layer.street_major.specs.clear();
        layer.street_minor.specs.clear();
        layer.street_major_casing.specs.clear();
        layer.street_minor_casing.specs.clear();
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
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &frame.road_segments,
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
        sync_road_strokes(
            &mut commands,
            &mut meshes,
            &preview,
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
            place_z,
            poi_materials_changed,
            &mut layer,
        );
    }
}

/// 道路组两种模式的基础色（casing, fill）。alpha 是「本色 alpha」，
/// 图层 opacity 在其上相乘。
///
/// 非 Google 模式走地图主题色板（ADR-0014）：casing = 纸面色（把路从同族
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
/// 古舆图分级双色（ADR-0014）：主干官道棕粗、次干浅卡其细；casing 同为纸面色。
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
    }
}

/// LOD 口径：国家级只保留城市级 POI，省级 / 编辑级全保留。
fn poi_kept_at_tier(dot: &PlaceDot, tier: LodTier) -> bool {
    tier != LodTier::Nation || dot.is_city
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
    streets: &crate::render::streets::StreetLayerCache,
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
    for (rank, segments) in crate::render::streets::street_streams(&streets.sources) {
        if !crate::render::streets::street_kept_at_tier(rank, tier) {
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
    sync_road_strokes(
        commands,
        meshes,
        &major,
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
        view_scale,
        minor_fill,
        STREET_MINOR_PX,
        STREET_MINOR_Z + z_offset,
        material_changed,
        &mut layer.street_minor,
    );
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

/// 同步红点白环 POI；Mesh 只在半径变化时重建。`z_offset` 是 Place 图层的槽位偏移。
#[allow(clippy::too_many_arguments)]
fn sync_poi(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    dots: &[PlaceDot],
    view_scale: f32,
    tier: LodTier,
    grid: &GridConfig,
    z_offset: f32,
    material_changed: bool,
    layer: &mut VectorLayerEntities,
) {
    let visible: Vec<&PlaceDot> = dots.iter().filter(|d| poi_kept_at_tier(d, tier)).collect();
    let radius_outer = (grid.cell_size * 0.45).max(POI_MIN_SCREEN_PX * view_scale * 0.5);
    let radius_inner = radius_outer * 0.52;
    let ring_thickness = (1.5 * view_scale).max(f32::EPSILON);
    let ring_inner = (radius_outer - ring_thickness).max(f32::EPSILON);
    let mode = layer
        .poi_material_mode
        .expect("ensure_poi_materials 必须已初始化");
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
                    Transform::from_translation(dot.pos.extend(POI_OUTER_Z + z_offset)),
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
                z_offset,
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
                    Transform::from_translation(dot.pos.extend(POI_INNER_Z + z_offset)),
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
                z_offset,
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
    z_offset: f32,
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
    commands.entity(entity).insert(Transform::from_translation(
        spec.pos.extend(POI_OUTER_Z + z_offset),
    ));
}

#[allow(clippy::too_many_arguments)]
fn update_poi_inner(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    entity: Entity,
    spec: PoiInnerSpec,
    z_offset: f32,
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
    commands.entity(entity).insert(Transform::from_translation(
        spec.pos.extend(POI_INNER_Z + z_offset),
    ));
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

    /// 街道与道路同口径：每档都必须有 casing，粗细差即路缘；且 Z 序要保证 casing
    /// 既不被底图（Z=0.0）盖掉、也不越过自己那档 fill——地图模式下 fill 是白色，
    /// 落在近白底图上唯一看得见的就是这层浅灰 casing。
    #[test]
    fn street_casing_is_thicker_than_its_fill_and_keeps_the_z_order() {
        let (a, b) = (Vec2::ZERO, Vec2::X * 100.0);
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
            let casing = RoadMeshSpec::from_segment(a, b, 1.0, fill_px + STREET_CASING_EXTRA_PX);
            let fill = RoadMeshSpec::from_segment(a, b, 1.0, fill_px);
            assert!(
                casing.world_thickness > fill.world_thickness,
                "{tier} 街道 casing 应厚于 fill：casing={} fill={}，期望 {}px > {}px",
                casing.world_thickness,
                fill.world_thickness,
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

    /// 线宽公式：`world_thickness = stroke_px * view.scale`，屏幕宽度恒定
    /// （`world_thickness / view.scale == stroke_px`），不随缩放变化。
    #[test]
    fn road_spec_screen_width_stays_constant_across_zoom() {
        for scale in [0.1, 1.0, 8.0] {
            let spec = RoadMeshSpec::from_segment(Vec2::ZERO, Vec2::X, scale, 3.0);
            let screen_px = spec.world_thickness / scale;
            assert!(
                (screen_px - 3.0).abs() < 1e-4,
                "scale={scale}: 屏幕宽度应恒为 3px，实得 {screen_px}px"
            );
        }
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
        world.init_resource::<crate::render::streets::StreetLayerCache>();
        world.init_resource::<crate::render::streets::StreetEditorState>();
        world.init_resource::<crate::render::layer_frame::LayerFrame>();

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
        world.insert_resource(ViewState::default());
        world.insert_resource(GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::ZERO,
            map_origin,
            map_size: 1024.0,
        });
        world.insert_resource(ProjectionFrame::default());
        world.init_resource::<VectorLayerEntities>();
        world.insert_resource(crate::render::streets::StreetLayerCache {
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
        world.init_resource::<crate::render::streets::StreetEditorState>();
        world.init_resource::<crate::render::layer_frame::LayerFrame>();

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_vector_layers);
        schedule.run(&mut world);

        let layer = world.resource::<VectorLayerEntities>();
        assert_eq!(layer.street_major.specs.len(), 1, "应画出 1 段主干道");
        let got = layer.street_major.specs[0].center;
        let expect = map_origin + Vec2::new(200.0, 200.0);
        assert!(
            got.distance(expect) < 1e-3,
            "街道中点应落在城市帧 {expect}；实得 {got}（若误用栅格原点会得到 (200,200)）"
        );
    }
}
