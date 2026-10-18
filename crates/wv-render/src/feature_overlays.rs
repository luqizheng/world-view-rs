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
//! ## 贴图（Task 9 / D12）
//! - 资产**编译期嵌入**（`include_bytes!`，主题规则里的相对路径 → PNG 字节），
//!   运行期用 `Image::from_buffer` 解码进 `Assets<Image>`，句柄按规则键缓存在
//!   [`FeatureOverlayEntities`]——**不用 `AssetServer`**（本仓库零 `AssetServer`
//!   用法，且资产不在 Bevy 资产根下）；
//! - 符号：每地点一个带 UV 的合批 mesh，UV 取 3 格横向图集的第
//!   `hash(地点种子, site 序号) % 3` 格；
//! - 晕：地界扇形补 UV（包围盒拉伸单张图章）；规则没给贴图 → 纯色。
//!
//! ## 边界（不做什么）
//! - **不写时间比较**：某刻哪些地界生效由 `place_territory_at` 求值；
//! - **不写世界**：只读 `WorldProjection.world`；
//! - **不重复解码**：贴图句柄跨帧缓存；解码失败仅 `warn!` 一次，之后静默回退。
//!
//! ## 回退（设计稿 §6.4）
//! - `feature = None`：普通地点不画覆盖层（它的表现是 POI 点 + 标签）；
//! - `feature = Some(f)` 但主题没有 `f` 的规则：仍画中性灰绿晕
//!   （`[0x6A, 0x70, 0x5A]`，alpha 0.12），不画符号——事实不因换主题消失；
//! - 近带采样为空（无地形 / 全水 / 环退化）→ 只画晕，不画符号（空采样回退）；
//! - 贴图键未知 / 解码失败 → 走纯色路径，不 panic、不空帧（解码同步完成，
//!   不存在「句柄未就绪」的首帧状态）。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::faction_overlays::{
    build_polygon_mesh, build_polygon_with_holes_mesh, earcut_with_holes,
};
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::terrain_view::terrain_resolve::active_terrain;
use crate::theme::overlays::{forest_sites, overlay_far_blend};
use crate::theme::theme_pack::builtin;
use wv_core::id::Id;
use wv_core::{place_territory_at, TerritoryShape};

/// 远带地界晕的 Z。
///
/// 邻居槽位：等值线 0.04 → 外框 / 换级淡出 0.05 → **纸面雾霭
/// （`atmosphere::VEIL_Z`）与 1/4 参考网格（`map_frame::GRID_Z`）同在 0.06**
/// → 本晕 0.065 → 符号 0.07 → 道路 casing 0.09。晕必须与 0.06 的雾霭 /
/// 网格**错开**：半透明同 Z 的先后序是未定义的，而覆盖层实体每帧重生成
/// （全新 entity id），雾与晕谁压谁会跨运行漂移——哪怕 0.005 的空隙也是
/// 确定序（final review 修复；雾 / 网格自身的 0.06 不动）。
const HALO_Z: f32 = 0.065;
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
/// 符号图集的横向格数（3 变体：阔叶丛 / 单株阔叶 / 针叶丛）。
const SYMBOL_ATLAS_VARIANTS: usize = 3;

/// 编译期嵌入的覆盖物贴图（规则里的相对路径 → PNG 字节）。
///
/// **为什么不用 `AssetServer`**：本仓库零 `AssetServer` 用法，且资产位于
/// `crates/wv-render/assets`（Bevy 默认资产根之外）。`include_bytes!` 让二进制
/// 自带贴图、无运行期 IO 失败分支；解码失败仅是纯色回退（不 panic）。
///
/// 只登记运行期真正消费的 3 张：图集（日 / 暮）+ 地界晕图章。单株 `tree_a/b/c`
/// 与 `forest_patch` 是归一化中间产物，不参与运行时轮换。
const EMBEDDED_TEXTURES: &[(&str, &[u8])] = &[
    (
        "overlays/forest/tree_atlas_day.png",
        include_bytes!("../assets/overlays/forest/tree_atlas_day.png"),
    ),
    (
        "overlays/forest/tree_atlas_dusk.png",
        include_bytes!("../assets/overlays/forest/tree_atlas_dusk.png"),
    ),
    (
        "overlays/forest/forest_fill.png",
        include_bytes!("../assets/overlays/forest/forest_fill.png"),
    ),
];

/// 事实覆盖层的实体集合（每帧 despawn + spawn，不跨帧复用）。
///
/// 与 `FactionOverlayEntities` 同族：资源只持有实体句柄；地界随
/// (图, 日, 缩放, 主题) 全变，几何 / 材质缓存复用得不偿失。**例外是贴图**：
/// 解码成本远高于重建 mesh，句柄按规则键跨帧缓存。
#[derive(Resource, Default)]
pub struct FeatureOverlayEntities {
    /// 本帧全部远带地界晕实体。
    pub halos: Vec<Entity>,
    /// 本帧全部近带符号实体（每个事实地点合并成一个 mesh）。
    pub symbols: Vec<Entity>,
    /// 解码后的贴图句柄：规则路径 → `Assets<Image>` 句柄（跨帧复用）。
    texture_cache: HashMap<&'static str, Handle<Image>>,
    /// 解码失败 / 未知的键（`warn!` 只发一次，之后静默走纯色回退）。
    failed_textures: HashSet<&'static str>,
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

    /// 规则里的贴图键 → 已解码句柄；未知键 / 解码失败返回 `None`（纯色回退）。
    ///
    /// **首帧语义**：解码是同步的，第一帧要么拿到句柄要么确定失败——不存在
    /// 「句柄未就绪」的空白帧。失败按键记忆，避免每帧重复解码与重复告警。
    fn texture(&mut self, key: &'static str, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if let Some(handle) = self.texture_cache.get(key) {
            return Some(handle.clone());
        }
        if self.failed_textures.contains(key) {
            return None;
        }
        let Some(bytes) = embedded_texture(key) else {
            warn!("覆盖物贴图键不在内嵌表：{key}；回退纯色");
            self.failed_textures.insert(key);
            return None;
        };
        match decode_texture(bytes) {
            Some(image) => {
                let handle = images.add(image);
                self.texture_cache.insert(key, handle.clone());
                Some(handle)
            }
            None => {
                warn!("覆盖物贴图解码失败：{key}；回退纯色");
                self.failed_textures.insert(key);
                None
            }
        }
    }
}

/// 贴图键 → 编译期嵌入的 PNG 字节。
///
/// **纯函数**（零 ECS / 零 `Assets` 访问）：未知键返回 `None`，渲染侧据此
/// 回退纯色——这是「缺资产不崩」的可单测决策点。
pub fn embedded_texture(key: &str) -> Option<&'static [u8]> {
    EMBEDDED_TEXTURES
        .iter()
        .find_map(|(candidate, bytes)| (*candidate == key).then_some(*bytes))
}

/// 解码内嵌 PNG 为 Bevy `Image`；失败返回 `None`（调用方回退纯色）。
///
/// `is_srgb = true`：墨稿 / 彩色稿都是 sRGB 创作；`ImageSampler::Default`
/// 走 `ImagePlugin` 的线性过滤，缩小图集符号时不出硬边。
fn decode_texture(bytes: &[u8]) -> Option<Image> {
    Image::from_buffer(
        bytes,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .ok()
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
    mut images: ResMut<Assets<Image>>,
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
    // ADR-0013 槽位偏移：晕与符号同属 Place 渲染组，作者 `z` 平移整组
    // （与 `faction_overlays` 的 `FACTION_Z + layers.influence.z_offset` 同款）。
    let halo_z = HALO_Z + layers.place.z_offset;
    let symbol_z = SYMBOL_Z + layers.place.z_offset;

    // 主题规则取内置包（D10）——不读 `ActiveThemePack`（尚无同步系统）。
    let pack = builtin::for_palette(view.map_palette);
    let map_origin = Vec3::new(grid.map_origin.x, grid.map_origin.y, 0.0);
    let raster_offset = (
        grid.map_origin.x - grid.raster_origin.x,
        grid.map_origin.y - grid.raster_origin.y,
    );
    // 地形只用于近带采样；拿不到 = 空采样回退（只画晕）。
    let terrain = active_terrain(&projection.world, Some(&map_id)).map(|(_, data)| data);

    for (place, shape) in territories {
        // 普通地点不发覆盖层：它的表现是 POI 点 + 标签（feature_overlays 与
        // `world_projection::place_dot_for` 的抑制规则互为镜像）。
        let Some(feature) = overlay_feature_for_place(place) else {
            continue;
        };
        // 带洞地界（ADR-0029 M2）：晕按外环 + 洞三角化，符号采样避开洞。
        let (outer, holes): (&[f32], Vec<&[f32]>) = match shape {
            TerritoryShape::Solid(ring) => (ring, Vec::new()),
            TerritoryShape::WithHoles { outer, holes } => (outer, holes),
        };
        // 顶点不足 3 个（< 6 个 float）无法成面——扇形 / earcut 都要求 n ≥ 3。
        if outer.len() < 6 {
            continue;
        }

        let rule = pack.overlays.rule_for(feature);
        let (halo_color, halo_alpha) = match rule {
            Some(rule) => (rule.halo_color, rule.halo_alpha * halo_band * opacity),
            // 主题缺规则的回退：中性灰绿晕，事实不因换主题消失。
            None => (FALLBACK_HALO_COLOR, FALLBACK_HALO_ALPHA * opacity),
        };
        // 晕贴图解析：只有规则给了键且键在内嵌表里才走贴图。
        // 未知键 / 解码失败 → `None` → 下面的纯色路径（不 panic、不空帧）。
        let halo_texture = rule
            .and_then(|rule| rule.halo_texture)
            .and_then(|key| layer.texture(key, &mut images));
        if halo_alpha > 0.0 {
            let entity = match halo_texture {
                Some(texture) => spawn_textured_mesh(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    if holes.is_empty() {
                        build_polygon_mesh_uv(outer)
                    } else {
                        build_polygon_mesh_uv_with_holes(outer, &holes)
                    },
                    texture,
                    halo_alpha,
                    map_origin + Vec3::Z * halo_z,
                ),
                None => spawn_colored_mesh(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    if holes.is_empty() {
                        build_polygon_mesh(outer)
                    } else {
                        build_polygon_with_holes_mesh(outer, &holes)
                    },
                    halo_color,
                    halo_alpha,
                    map_origin + Vec3::Z * halo_z,
                ),
            };
            layer.halos.push(entity);
        }

        // 近带符号：只有拿到规则的事实才画；远带（alpha < 0.05）不画。
        let Some(rule) = rule else {
            continue;
        };
        if symbol_alpha < SYMBOL_MIN_ALPHA {
            continue;
        }
        let seed = stable_hash(place.id.as_str());
        let sites = forest_sites(
            outer,
            &holes,
            terrain,
            grid.cell_size,
            raster_offset,
            seed,
            rule.symbol_spacing_wu,
        );
        // 空采样回退：晕 + 地界仍可见，不画树（设计稿 §6.4）。
        if sites.is_empty() {
            continue;
        }
        let world_size = rule.symbol_size_px * view.scale;
        let symbol_texture = rule
            .symbol_texture
            .and_then(|key| layer.texture(key, &mut images));
        let entity = match symbol_texture {
            Some(texture) => {
                // 每地点一个合批 mesh：变体由 (地点种子, site 序号) 决定。
                let (mesh, _variants) =
                    build_symbol_mesh_uv_seeded(&sites, world_size, seed, SYMBOL_ATLAS_VARIANTS);
                spawn_textured_mesh(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    mesh,
                    texture,
                    symbol_alpha,
                    map_origin + Vec3::Z * symbol_z,
                )
            }
            // 缺资产回退：纯色 quad（Task 6 路径），树形退化为色块但不空帧。
            None => spawn_colored_mesh(
                &mut commands,
                &mut meshes,
                &mut materials,
                build_symbol_mesh(&sites, world_size),
                rule.symbol_color,
                symbol_alpha,
                map_origin + Vec3::Z * symbol_z,
            ),
        };
        layer.symbols.push(entity);
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

/// spawn 一个「带贴图 mesh」实体（`ColorMaterial { color: WHITE, texture }`），
/// 透明度落在材质 color 的 alpha 上。
///
/// **`alpha_mode: Blend`**：贴图自带 alpha（树图集透明背景），不能走 Mask——
/// 否则 MSAA 边缘会被阈值切成硬边。`color = WHITE` 不染色，保留原画配色。
fn spawn_textured_mesh(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    mesh: Mesh,
    texture: Handle<Image>,
    alpha: f32,
    translation: Vec3,
) -> Entity {
    let mesh = meshes.add(mesh);
    let material = materials.add(ColorMaterial {
        color: Color::WHITE.with_alpha(alpha),
        texture: Some(texture),
        alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
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

/// 地界多边形的**带 UV** 扇形 mesh（晕贴图专用）。
///
/// 与 [`build_polygon_mesh`] 同一三角化口径（中心 = 首顶点），但给每个顶点补
/// `uv = ((x - min_x) / w, (y - min_y) / h)`：单张 `forest_fill` 图章按地界
/// 包围盒**拉伸**铺满，不随地界形状裁切。不修改 faction 的位置-only mesh——
/// 势力多边形不需要贴图，补 UV 只是白付顶点带宽。
///
/// UV 的 v 随本图局部 +Y 增长。世界 +Y 在屏幕上朝上（`Camera2d` 默认朝向），
/// `forest_fill` 是俯视森林盖层（无上下之分），故与本文件符号的「树冠朝上」
/// 映射不冲突。
fn build_polygon_mesh_uv(coords: &[f32]) -> Mesh {
    let n = coords.len() / 2;
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for i in 0..n {
        let (x, y) = (coords[i * 2], coords[i * 2 + 1]);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    // 退化轴（零宽 / 零高环）避免除零；UV 全部压到 0。
    let w = (max_x - min_x).max(f32::EPSILON);
    let h = (max_y - min_y).max(f32::EPSILON);

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(n);
    for i in 0..n {
        let (x, y) = (coords[i * 2], coords[i * 2 + 1]);
        positions.push([x, y, 0.0]);
        uvs.push([(x - min_x) / w, (y - min_y) / h]);
    }
    let mut indices: Vec<u32> = Vec::with_capacity((n - 2) * 3);
    for i in 1..(n as u32 - 1) {
        indices.push(0);
        indices.push(i);
        indices.push(i + 1);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 带洞地界的**带 UV** 晕 mesh（ADR-0029 M2）：外环 − 洞经 [`earcut_with_holes`]
/// 三角化，每个顶点补外环包围盒归一化 UV——单张 `forest_fill` 图章按包围盒
/// 拉伸铺面，与 [`build_polygon_mesh_uv`] 同口径；洞内不产生三角形（晕不盖
/// 湖面 / 草地芯）。
fn build_polygon_mesh_uv_with_holes(outer: &[f32], holes: &[&[f32]]) -> Mesh {
    let tris = earcut_with_holes(outer, holes);
    let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
    let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for point in outer.chunks_exact(2) {
        min_x = min_x.min(point[0]);
        min_y = min_y.min(point[1]);
        max_x = max_x.max(point[0]);
        max_y = max_y.max(point[1]);
    }
    // 退化轴（零宽 / 零高环）避免除零；UV 全部压到 0。
    let w = (max_x - min_x).max(f32::EPSILON);
    let h = (max_y - min_y).max(f32::EPSILON);
    let positions: Vec<[f32; 3]> = tris.iter().map(|p| [p[0], p[1], 0.0]).collect();
    let uvs: Vec<[f32; 2]> = tris
        .iter()
        .map(|p| [(p[0] - min_x) / w, (p[1] - min_y) / h])
        .collect();
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
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

/// 带图集 UV 的符号 mesh（3 格横向图集）。
///
/// 每个 site 一个 quad（4 顶点 / 6 索引 / 4 UV），格号 = 该 site 的变体。
/// 返回的 `Vec<u32>` 是逐 site 的变体索引，供测试断言（渲染侧不使用）。
///
/// `world_size` 是符号的世界边长；无地点种子（`seed = 0`）——系统内走
/// [`build_symbol_mesh_uv_seeded`]，本函数是接口锁定下的纯函数入口。
pub fn build_symbol_mesh_uv(
    sites: &[(f32, f32)],
    world_size: f32,
    variants: usize,
) -> (Mesh, Vec<u32>) {
    build_symbol_mesh_uv_seeded(sites, world_size, 0, variants)
}

/// [`build_symbol_mesh_uv`] 的带种子版本：变体熵源 = (地点种子, site 序号)。
///
/// **为什么种子不进 `build_symbol_mesh_uv` 形参**：对外接口已按设计稿锁定；
/// 系统层另有地点 id 种子，故保留带种子版本供渲染调用，避免把种子塞进
/// site 坐标（那会污染近带采样）。
fn build_symbol_mesh_uv_seeded(
    sites: &[(f32, f32)],
    world_size: f32,
    seed: u64,
    variants: usize,
) -> (Mesh, Vec<u32>) {
    let variants = variants.max(1);
    let cell = 1.0 / variants as f32;
    let half = world_size * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(sites.len() * 4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(sites.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(sites.len() * 6);
    let mut variant_indices: Vec<u32> = Vec::with_capacity(sites.len());

    for (index, &(x, y)) in sites.iter().enumerate() {
        let variant = symbol_variant(seed, index, variants);
        variant_indices.push(variant);
        let u0 = variant as f32 * cell;
        let u1 = u0 + cell;
        let base = positions.len() as u32;
        // 顶点顺序 = [左下, 右下, 右上, 左上]（与 `build_symbol_mesh` 同序）。
        // 世界 +Y 朝屏幕上方：图 v=0 是树冠顶（PNG 第 0 行），故 v=0 绑 +Y 顶点，
        // v=1（树根）绑 -Y 顶点——树在屏幕上正立。
        positions.push([x - half, y - half, 0.0]);
        positions.push([x + half, y - half, 0.0]);
        positions.push([x + half, y + half, 0.0]);
        positions.push([x - half, y + half, 0.0]);
        uvs.push([u0, 1.0]);
        uvs.push([u1, 1.0]);
        uvs.push([u1, 0.0]);
        uvs.push([u0, 0.0]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, variant_indices)
}

/// 站点 → 图集格号：`hash(地点种子, site 序号) % variants`。
///
/// 确定性：同一构建内 `DefaultHasher` 键固定，同一 (seed, index) 每帧同值
/// （树的重开不跳变）；不承诺跨 Rust 版本的数值稳定（与 [`stable_hash`] 同口径，
/// v1 树冠布局只要求会话内一致）。
fn symbol_variant(seed: u64, index: usize, variants: usize) -> u32 {
    if variants <= 1 {
        return 0;
    }
    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    index.hash(&mut hasher);
    (hasher.finish() % variants as u64) as u32
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

/// 标记的群系 → 主题覆盖物词汇（ADR-0029 D5 的过渡桥）。
///
/// 主题 pack 的规则仍以旧 `FEATURE_FOREST = 4` 为键（D18 词汇统一延后）。
/// 有 `biome` 时：森林族（森林/丛林/热带雨林）映射到 4，其余群系无覆盖物
/// （名字与格子色仍然可见）；无 `biome` 的旧工程回退到 `place.feature`。
pub(crate) fn overlay_feature_for_place(place: &wv_core::distance::Place) -> Option<u8> {
    match place.biome {
        Some(b) if wv_terrain::biome::is_forest_family(b) => {
            Some(crate::theme::overlays::FEATURE_FOREST)
        }
        Some(_) => None,
        None => place.feature,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `stable_hash` 是纯函数：同 id 每次调用同值，不同 id 不撞。
    #[test]
    fn forest_biome_maps_to_legacy_feature_four() {
        let mut p = wv_core::distance::Place {
            id: wv_core::Id::new_unchecked("p"),
            name: "p".into(),
            feature: None,
            biome: Some(wv_terrain::biome::BIOME_JUNGLE),
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
        };
        assert_eq!(
            overlay_feature_for_place(&p),
            Some(crate::theme::overlays::FEATURE_FOREST),
            "森林族群系应映射到旧 feature 4"
        );
        p.biome = Some(wv_terrain::biome::BIOME_GRASSLAND);
        assert_eq!(overlay_feature_for_place(&p), None, "非森林群系无覆盖物");
        p.biome = None;
        p.feature = Some(4);
        assert_eq!(
            overlay_feature_for_place(&p),
            Some(4),
            "旧工程只有 feature 时走回退"
        );
    }

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

    /// 带 UV 的符号 mesh：3 个 site → 12 顶点、18 索引、12 个 UV；每个 site
    /// 的 UV 落在图集 1/3 格内（`variants = 3` 的横向图集）。
    #[test]
    fn symbol_mesh_has_uvs_and_four_vertices_per_site() {
        let sites = [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0)];
        let (mesh, variants) = build_symbol_mesh_uv(&sites, 2.0, 3);
        let positions = mesh
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在")
            .as_float3()
            .expect("应是 f32x3");
        let uvs = mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("UV 属性应存在");
        assert_eq!(positions.len(), 12, "3 个 site × 4 顶点");
        assert_eq!(
            mesh.indices().expect("索引应存在").len(),
            18,
            "3 个 site × 6 索引"
        );
        let bevy::render::mesh::VertexAttributeValues::Float32x2(uvs) = uvs else {
            panic!("UV 应为 Float32x2");
        };
        assert_eq!(uvs.len(), 12, "3 个 site × 4 UV");
        assert_eq!(variants.len(), 3, "每个 site 一个变体索引");
        for (i, (site, &variant)) in sites.iter().zip(&variants).enumerate() {
            assert!(variant < 3, "site {site:?} 变体越界：{variant}");
            let cell = 1.0 / 3.0;
            let u0 = variant as f32 * cell;
            let u1 = u0 + cell;
            // quad 顶点顺序 = [左下, 右下, 右上, 左上]；UV 必须同序。
            let quad = &uvs[i * 4..i * 4 + 4];
            for (i, uv) in quad.iter().enumerate() {
                assert!(
                    uv[0] >= u0 - 1e-6 && uv[0] <= u1 + 1e-6,
                    "uv {i} 的 u={} 应落在格 [{u0}, {u1}]",
                    uv[0]
                );
                assert!(
                    uv[1] == 0.0 || uv[1] == 1.0,
                    "uv {i} 的 v={} 应为 0 或 1（整格取样）",
                    uv[1]
                );
            }
        }
    }

    /// 同一 site 多次选择同 variant；变体数降为 1 时全部归 0。
    #[test]
    fn symbol_variant_selection_is_stable_per_site() {
        let sites = [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0), (7.0, 8.0), (9.0, 10.0)];
        let (_, first) = build_symbol_mesh_uv(&sites, 2.0, 3);
        let (_, second) = build_symbol_mesh_uv(&sites, 2.0, 3);
        assert_eq!(first, second, "同一 site 多次选择必须同 variant");
        assert_eq!(first.len(), sites.len());
        for (i, &variant) in first.iter().enumerate() {
            assert!(variant < 3, "site {i} 变体越界：{variant}");
        }
        let (_, single) = build_symbol_mesh_uv(&sites, 2.0, 1);
        assert!(
            single.iter().all(|&v| v == 0),
            "variants=1 时全部归 0，got {single:?}"
        );
    }

    /// 变体熵源含地点种子：同一批 site 位置、不同种子 → 变体序列不同。
    #[test]
    fn symbol_variants_change_with_place_seed() {
        let sites = [(1.0, 2.0), (3.0, 4.0), (5.0, 6.0), (7.0, 8.0)];
        let (_, a) = build_symbol_mesh_uv_seeded(&sites, 2.0, 0x5EED_0001, 3);
        let (_, b) = build_symbol_mesh_uv_seeded(&sites, 2.0, 0x5EED_0002, 3);
        assert_ne!(a, b, "不同地点种子的变体序列应不同");
    }

    /// 带洞晕 mesh：任一三角形质心不得落进洞；总面积 ≈ 外环 − 洞；UV 归一化。
    #[test]
    fn halo_mesh_with_holes_leaves_hole_interior_uncovered() {
        let outer = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let hole = vec![4.0, 4.0, 6.0, 4.0, 6.0, 6.0, 4.0, 6.0];
        let mesh = build_polygon_mesh_uv_with_holes(&outer, &[&hole]);
        let positions: &[[f32; 3]] = mesh
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在")
            .as_float3()
            .expect("应是 f32x3");
        assert!(
            positions.len().is_multiple_of(3),
            "三角 soup 顶点数应为 3 的倍数，实得 {}",
            positions.len()
        );
        let mut area = 0.0f32;
        for tri in positions.chunks(3) {
            let (a, b, c) = (tri[0], tri[1], tri[2]);
            let centroid = ((a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0);
            assert!(
                !(centroid.0 > 4.0 && centroid.0 < 6.0 && centroid.1 > 4.0 && centroid.1 < 6.0),
                "三角形质心 {centroid:?} 不得落在洞 (4..6)² 内"
            );
            area += ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0;
        }
        assert!(
            (area - 96.0).abs() < 1.0,
            "带洞晕面积应 ≈ 100−4=96，实得 {area}"
        );
        let bevy::render::mesh::VertexAttributeValues::Float32x2(uvs) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("UV 应存在")
        else {
            panic!("UV 应为 Float32x2");
        };
        assert_eq!(uvs.len(), positions.len(), "每顶点一个 UV");
        for (i, uv) in uvs.iter().enumerate() {
            assert!(
                uv[0] >= -1e-6 && uv[0] <= 1.0 + 1e-6 && uv[1] >= -1e-6 && uv[1] <= 1.0 + 1e-6,
                "UV {i} = {uv:?} 应归一化到外环包围盒 [0,1]²"
            );
        }
    }

    /// 晕多边形补 UV：顶点 UV = ((x-min_x)/w, (y-min_y)/h)（拉伸单张图章）。
    #[test]
    fn halo_mesh_uvs_span_the_ring_bbox() {
        let ring = [10.0, 20.0, 30.0, 20.0, 30.0, 60.0, 10.0, 60.0];
        let mesh = build_polygon_mesh_uv(&ring);
        let bevy::render::mesh::VertexAttributeValues::Float32x2(uvs) =
            mesh.attribute(Mesh::ATTRIBUTE_UV_0).expect("UV 应存在")
        else {
            panic!("UV 应为 Float32x2");
        };
        assert_eq!(uvs.len(), 4, "4 个环顶点 → 4 个 UV");
        assert_eq!(uvs[0], [0.0, 0.0], "左下 → (0,0)");
        assert_eq!(uvs[1], [1.0, 0.0], "右下 → (1,0)");
        assert_eq!(uvs[2], [1.0, 1.0], "右上 → (1,1)");
        assert_eq!(uvs[3], [0.0, 1.0], "左上 → (0,1)");
    }

    /// 已知贴图键 → 编译期嵌入的 PNG 字节（三条运行时资产全在册）。
    #[test]
    fn known_texture_keys_map_to_embedded_bytes() {
        for key in [
            "overlays/forest/tree_atlas_day.png",
            "overlays/forest/tree_atlas_dusk.png",
            "overlays/forest/forest_fill.png",
        ] {
            let bytes = embedded_texture(key).unwrap_or_else(|| panic!("{key} 应有内嵌字节"));
            assert_eq!(&bytes[..4], b"\x89PNG", "{key} 应是 PNG 魔数开头");
        }
    }

    /// 未知贴图键 → `None`（渲染侧回退纯色，不 panic、不空帧）。
    #[test]
    fn unknown_texture_keys_have_no_embedded_bytes() {
        assert!(
            embedded_texture("overlays/forest/does_not_exist.png").is_none(),
            "未知键不得命中内嵌表"
        );
    }

    /// 非 PNG 字节 → 解码返回 `None`（调用方回退纯色，不 panic）。
    #[test]
    fn invalid_png_bytes_decode_to_none() {
        assert!(
            decode_texture(b"not a png").is_none(),
            "非 PNG 字节必须解码失败并回退纯色"
        );
    }

    /// 缺失贴图的纯决策：规则键未知 → 纯色回退；纯色 mesh 仍可生成（不空帧）。
    #[test]
    fn missing_texture_falls_back_to_flat_color() {
        let mut rule = *builtin::DEFAULT
            .overlays
            .forest
            .as_ref()
            .expect("默认包必须有森林规则");
        rule.symbol_texture = Some("overlays/forest/does_not_exist.png");
        assert!(
            rule.symbol_texture.and_then(embedded_texture).is_none(),
            "未知贴图键必须回退纯色路径"
        );
        let mesh = build_symbol_mesh(&[(1.0, 2.0)], 4.0);
        assert_eq!(mesh.count_vertices(), 4, "回退纯色路径仍产出 4 顶点 quad");
    }

    /// 内嵌图集解码为 384×128（3 格 × 128）；解码失败才走纯色回退。
    #[test]
    fn embedded_tree_atlas_decodes_to_three_cells() {
        let bytes = embedded_texture("overlays/forest/tree_atlas_day.png").expect("日间图集应内嵌");
        let image = decode_texture(bytes).expect("内嵌 PNG 应可解码");
        assert_eq!(image.width(), 384, "横向 3 格 × 128");
        assert_eq!(image.height(), 128, "单格 128");
    }
}
