//! 事实地界覆盖层（theme overlay · Task 6）—— 远带地界晕 + 近带符号。
//!
//! ## 职责
//! - 每帧按 `playback.current_day` 求值当前图上生效的 `Place` 地界
//!   （[`wv_core::place_territory_at`]；渲染层不比较时间，AGENTS §3），为
//!   `feature = Some(_)` 的事实地点画：
//!   - **远带**：一个半透明扇形 halo（地界形状），随 `view.scale` 交叉淡化；
//!   - **近带**：主题 [`crate::theme::overlays::FeatureRule`] 的符号（v1 = 纯色
//!     quad 树冠），由 [`crate::theme::overlays::forest_sites`] 确定性采样落位，
//!     近带满强度、远带退场。
//! - 主题规则取 [`crate::theme::theme_pack::builtin::for_palette`]（D10：规则属于
//!   主题包）。**不读 `ActiveThemePack`**——该资源目前没有同步系统，会永远停在
//!   Daylight。
//!
//! ## 边界（不做什么）
//! - **不写时间比较**：某刻哪些地界生效由 `place_territory_at` 求值；
//! - **不写世界**：只读 `WorldProjection.world`；
//! - **不做资产**：`symbol_texture` / `halo_texture` 留给 Task 9，本期纯色。
//!
//! ## 回退（设计稿 §6.4）
//! - `feature = None`：普通地点不画覆盖层（它的表现是 POI 点 + 标签）；
//! - `feature = Some(f)` 但主题没有 `f` 的规则：仍画中性灰绿晕
//!   （`[0x6A, 0x70, 0x5A]`，alpha 0.12），不画符号——事实不因换主题消失；
//! - 近带采样为空（无地形 / 全水 / 环退化）→ 只画晕，不画符号（空采样回退）。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::faction_overlays::build_polygon_mesh;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::terrain_view::terrain_resolve::active_terrain;
use crate::theme::overlays::{forest_sites, overlay_far_blend};
use crate::theme::theme_pack::builtin;
use wv_core::id::Id;
use wv_core::place_territory_at;

/// 远带地界晕的 Z（地形之上、道路 0.09 之下）。
const HALO_Z: f32 = 0.06;
/// 近带符号的 Z（紧贴晕之上，仍在道路之下）。
const SYMBOL_Z: f32 = 0.07;
/// 主题缺规则时的中性灰绿晕（设计稿 §6.4 回退）。
const FALLBACK_HALO_COLOR: [u8; 3] = [0x6A, 0x70, 0x5A];
/// 主题缺规则时的晕浓度。
const FALLBACK_HALO_ALPHA: f32 = 0.12;
/// 近带符号的 alpha 下限：低于此值不生成 mesh（`1 - blend < 0.05`）。
const SYMBOL_MIN_ALPHA: f32 = 0.05;
/// 近带晕的最低占比：`halo_alpha * (0.4 + 0.6 * blend)`。
const HALO_NEAR_FLOOR: f32 = 0.4;

/// 事实覆盖层的实体集合（每帧 despawn + spawn，不跨帧复用）。
///
/// 与 `FactionOverlayEntities` 同族：资源只持有实体句柄，不持有几何 / 材质
/// 缓存——地界随 (图, 日, 缩放, 主题) 全变，缓存复用得不偿失。
#[derive(Resource, Default)]
pub struct FeatureOverlayEntities {
    /// 本帧全部远带地界晕实体。
    pub halos: Vec<Entity>,
    /// 本帧全部近带符号实体（每个事实地点合并成一个 mesh）。
    pub symbols: Vec<Entity>,
}

impl FeatureOverlayEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for e in self.halos.drain(..) {
            commands.entity(e).despawn();
        }
        for e in self.symbols.drain(..) {
            commands.entity(e).despawn();
        }
    }
}

/// 事实地界覆盖层主 system。
///
/// 每帧先 despawn 全部旧实体再重建：地界随 `(current_map, current_day, scale,
/// map_palette, 图层透明度)` 变化，跨帧缓存的重建条件比直接重建更贵。典型工程
/// 事实地点 < 20 个，重建成本可忽略。
///
/// **地图 / 日**：`place_territory_at(&world, &current_map, current_day)`——时间
/// 求值在 `wv-core`，本系统不做任何 `day > …` 比较（AGENTS §3）。
///
/// **坐标口径**：地界顶点与近带采样点都是**本图局部坐标**，mesh 顶点保持局部，
/// 由 `Transform` 平移到 `grid.map_origin`（等价于逐点 `GridConfig::map_to_view`）。
/// 近带采样换到**栅格**格时用 `map_origin - raster_origin` 作偏移——子图继承
/// 父图底图时两套原点不同（见 `GridConfig` 的两套原点表）。
#[allow(clippy::too_many_arguments)]
pub fn sync_feature_overlays(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    projection: Res<WorldProjection>,
    grid: Res<GridConfig>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    mut layer: ResMut<FeatureOverlayEntities>,
) {
    // 门：recipe 已把 `!layers.place.visible` 级联进本 pass（ADR-0013 / ADR-0021）。
    // 无论开不开，都先清旧实体——关图层的那一帧必须立刻消失。
    layer.despawn_all(&mut commands);
    if !recipe.enabled(PassId::FeatureOverlay) {
        return;
    }

    let map_id = Id::new_unchecked(&view.current_map);
    let territories = place_territory_at(&projection.world, &map_id, playback.current_day);

    let blend = overlay_far_blend(view.scale);
    // 远带晕：近带保留 40% 浓度，随 scale 增长到 100%。
    let halo_band = HALO_NEAR_FLOOR + (1.0 - HALO_NEAR_FLOOR) * blend;
    // 近带符号：近带满强度，远带退场。
    let symbol_alpha = (1.0 - blend) * layers.place.opacity;
    let opacity = layers.place.opacity;

    // 主题规则取内置包（D10）——不读 `ActiveThemePack`（尚无同步系统）。
    let pack = builtin::for_palette(view.map_palette);
    let map_origin = Vec3::new(grid.map_origin.x, grid.map_origin.y, 0.0);
    let raster_offset = (
        grid.map_origin.x - grid.raster_origin.x,
        grid.map_origin.y - grid.raster_origin.y,
    );
    // 地形只用于近带采样；拿不到 = 空采样回退（只画晕）。
    let terrain = active_terrain(&projection.world, Some(&map_id)).map(|(_, data)| data);

    for (place, ring) in territories {
        // 普通地点不发覆盖层：它的表现是 POI 点 + 标签（feature_overlays 与
        // `world_projection::place_dot_for` 的抑制规则互为镜像）。
        let Some(feature) = place.feature else {
            continue;
        };
        // 顶点不足 3 个（< 6 个 float）无法成面——`build_polygon_mesh` 的扇形
        // 三角化要求 n ≥ 3。
        if ring.len() < 6 {
            continue;
        }

        let rule = pack.overlays.rule_for(feature);
        let (halo_color, halo_alpha) = match rule {
            Some(rule) => (rule.halo_color, rule.halo_alpha * halo_band * opacity),
            // 主题缺规则的回退：中性灰绿晕，事实不因换主题消失。
            None => (FALLBACK_HALO_COLOR, FALLBACK_HALO_ALPHA * opacity),
        };
        if halo_alpha > 0.0 {
            layer.halos.push(spawn_colored_mesh(
                &mut commands,
                &mut meshes,
                &mut materials,
                build_polygon_mesh(ring),
                halo_color,
                halo_alpha,
                map_origin + Vec3::Z * HALO_Z,
            ));
        }

        // 近带符号：只有拿到规则的事实才画；远带（alpha < 0.05）不画。
        let Some(rule) = rule else {
            continue;
        };
        if symbol_alpha < SYMBOL_MIN_ALPHA {
            continue;
        }
        let sites = forest_sites(
            ring,
            terrain,
            grid.cell_size,
            raster_offset,
            stable_hash(place.id.as_str()),
            rule.symbol_spacing_wu,
        );
        // 空采样回退：晕 + 地界仍可见，不画树（设计稿 §6.4）。
        if sites.is_empty() {
            continue;
        }
        layer.symbols.push(spawn_colored_mesh(
            &mut commands,
            &mut meshes,
            &mut materials,
            build_symbol_mesh(&sites, rule.symbol_size_px * view.scale),
            rule.symbol_color,
            symbol_alpha,
            map_origin + Vec3::Z * SYMBOL_Z,
        ));
    }
}

/// spawn 一个「单色 mesh」实体（`Mesh2d` + `ColorMaterial`），返回实体句柄。
fn spawn_colored_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    mesh: Mesh,
    rgb: [u8; 3],
    alpha: f32,
    translation: Vec3,
) -> Entity {
    let mesh = meshes.add(mesh);
    let material = materials.add(ColorMaterial {
        color: Color::srgba(
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            alpha,
        ),
        ..default()
    });
    commands
        .spawn((
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform::from_translation(translation),
        ))
        .id()
}

/// 把一批局部坐标落位合并成**单个** mesh：每个符号一个轴对齐 quad
/// （4 顶点 / 6 索引）。
///
/// `world_size` 是符号的世界边长（渲染侧传 `symbol_size_px * view.scale`，
/// 即屏幕尺寸恒定的像素值换成世界单位）。合批成一个 mesh 而不是每符号一个
/// 实体：一个森林地界可能有几十个采样点，单实体省掉同数量 draw call。
fn build_symbol_mesh(sites: &[(f32, f32)], world_size: f32) -> Mesh {
    let half = world_size * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(sites.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(sites.len() * 6);
    for &(x, y) in sites {
        let base = positions.len() as u32;
        positions.push([x - half, y - half, 0.0]);
        positions.push([x + half, y - half, 0.0]);
        positions.push([x + half, y + half, 0.0]);
        positions.push([x - half, y + half, 0.0]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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
    mesh
}

/// 字符串 id → 确定性种子（近带采样抖动的熵源）。
///
/// `DefaultHasher::new()` 的键固定（SipHash-1-3，keys `(0, 0)`），同一构建内
/// 跨进程 / 跨调用稳定——每帧对同一地点重算采样时树冠布局不跳变。**不承诺**
/// 跨 Rust 版本的数值稳定；v1 的树冠布局只要求会话内一致（无持久化依赖）。
pub fn stable_hash(id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `stable_hash` 是纯函数：同 id 每次调用同值，不同 id 不撞。
    #[test]
    fn stable_hash_is_stable_across_calls() {
        assert_eq!(
            stable_hash("place_forest"),
            stable_hash("place_forest"),
            "同一 id 两次调用必须同值"
        );
        assert_ne!(
            stable_hash("place_forest"),
            stable_hash("place_meadow"),
            "不同 id 应给出不同种子"
        );
    }

    /// 每个采样点一个 quad：4 顶点 / 6 索引。
    #[test]
    fn symbol_mesh_has_four_vertices_and_six_indices_per_site() {
        let mesh = build_symbol_mesh(&[(1.0, 2.0), (3.0, 4.0)], 2.0);
        let positions: &[[f32; 3]] = mesh
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在")
            .as_float3()
            .expect("应是 f32x3");
        assert_eq!(positions.len(), 8, "2 个符号 × 4 顶点");
        assert_eq!(
            mesh.indices().expect("索引应存在").len(),
            12,
            "2 个符号 × 6 索引"
        );
    }
}
