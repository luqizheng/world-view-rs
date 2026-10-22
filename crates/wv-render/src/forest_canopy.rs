//! 地形森林的**位图树冠层**（设计文档 §七 Sprite 形态，2026-10-22）。
//!
//! ## 职责
//! - 主题给了 `DecorRecipe::forest_symbol` → 在**地形事实**（森林群系 + 密度场）
//!   驱动下撒手绘树：密度决定**疏密**与**树冠大小**，密度分级决定 tint；
//! - 主题没给 → 本层**不画**（程序化林冠点由 [`crate::terrain_decor`] 承担）。
//!
//! ## 形态二选一（§七 / ADR-0050）
//!
//! 位图树与程序化林冠点**互斥**：主题声明了位图形态时，`terrain_decor` 的森林
//! 桶同步停摆（否则「树上长树点」）。形态是主题的一次性选择，不是运行时开关。
//!
//! ## 与 `terrain_decor` 的分工
//!
//! - [`crate::terrain_decor`]：**程序化符号**（三角山 / 草点 / 林冠团…），
//!   位图形态缺席；
//! - 本模块：**位图树**（地形驱动的森林）。
//!
//! ## 边界（不做什么）
//!
//! - **不写世界 / 不做时间比较**（AGENTS §3）；
//! - **不引入位图瓦片**——位图只用于**符号**，底图仍是 CPU 烘焙的 RGBA 纹理
//!   （ADR-0031 D5 的「位图瓦片已弃用」结论不受本模块影响）；
//! - 贴图走渲染层内嵌资产表（`include_bytes!`），无运行期 IO 失败分支。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::Mesh;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use crate::forest_density::ForestDensityCache;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{GridConfig, ViewState, WorldProjection};
use crate::terrain_decor::{decor_band, forest_keep_prob, hash01, points_fingerprint, DecorBand};
use crate::terrain_view::terrain_resolve::active_terrain;
use crate::theme::decor::ForestSymbolSpec;
use wv_terrain::biome::is_forest_family;
use wv_terrain::forest::{classify_forest, ForestType};

/// 树冠层 Z：地形装饰（0.075）之上、道路（0.09）之下。
///
/// **为什么在装饰之上**：树是「林冠」，画在林冠点（装饰）之下会被几何符号压住；
/// **为什么在道路之下**：路网是信息层，必须压在一切自然物之上。
const CANOPY_Z: f32 = 0.078;
/// 视口外扩余量（屏幕像素）：拖动相机时边缘树不闪断。
const VIEWPORT_MARGIN_PX: f32 = 32.0;
/// 单帧树数上限（兜底；步距本身已把数量压到屏幕量级）。
const MAX_TREES: usize = 4000;
/// 单帧候选格数上限（兜底）。
///
/// **为什么要有**：采样量 = 视口 ÷ 步距，正常情况下与缩放无关（两者都乘
/// `view.scale`）；但主题若给出极小步距、或视口被外部放大，候选数会按平方
/// 增长。宁可这一帧不画，也不要在渲染线程里跑百万次哈希。
const MAX_CANDIDATES: i64 = 40_000;
/// 树根锚点的下沉量（相对符号边长）。
///
/// **为什么树根要下沉 15%**：树图集每格 128×128，树冠图的实际着墨范围
/// 约在格高的 6%–95%（实测 `tree_atlas_day.png` 三个变体的 alpha 包围盒
/// 底边在 85% / 95% / 93%）。若把图集格底边直接压在落点上，树的树干底
/// 会浮在落点上方约 15%——**树会「浮」在地面上**。故把图集格底边压到
/// 落点下方 15%，让三个变体的树干底都落在落点附近（略埋入林底比浮空好）。
/// 现在这套是**委托设计素材**（192×260 单株格），构图本身就是「底边贴地」，
/// 着墨底边离格底 2–6%，故下沉量收到 3%。
const TREE_BASE_SINK: f32 = 0.03;

/// 一棵候选树：位置 / 边长 / 图集变体 / 顶点 tint。
#[derive(Debug, Clone, Copy)]
struct Tree {
    /// 树根锚点的世界 x（底边中点）。
    x: f32,
    /// 树根锚点的世界 y（底边中点）。
    y: f32,
    /// 符号边长（世界单位）。
    size: f32,
    /// 图集变体号（横向格号）。
    variant: u32,
    /// 顶点 tint（0..=1 的 RGB；与贴图相乘）。
    tint: [f32; 3],
}

/// 树冠层的运行时资源：实体 + mesh / 材质 / 贴图句柄（跨帧复用）。
///
/// **为什么池化**：贴图解码成本远高于重建 mesh（PNG 解码 ~ms 级），
/// 故贴图按键缓存；mesh 则是「内容变了才原地改写」（ADR-0030 D4 同纪律）。
#[derive(Resource, Default)]
pub struct ForestCanopyPool {
    /// 当前树冠实体（`None` = 当前无树）。
    entity: Option<Entity>,
    /// 当前树冠 mesh 句柄。
    mesh: Option<Handle<Mesh>>,
    /// 当前材质句柄。
    material: Option<Handle<ColorMaterial>>,
    /// 当前贴图句柄。
    texture: Option<Handle<Image>>,
    /// 当前贴图键（换主题 / 换日暮 → 换图）。
    texture_key: Option<&'static str>,
    /// 当前材质的不透明度（变了要重建材质）。
    material_alpha: f32,
    /// 贴图解码失败标记（只告警一次，之后静默不画）。
    texture_failed: bool,
    /// 上帧几何指纹（变了才改写 mesh）。
    fingerprint: u64,
}

impl ForestCanopyPool {
    /// 清空：实体下线、mesh / 材质资产回收。
    ///
    /// 贴图句柄**保留**（解码成本高，下次启用直接复用）。
    fn clear(&mut self, commands: &mut Commands) {
        if let Some(entity) = self.entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some(mesh) = self.mesh.take() {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        if let Some(material) = self.material.take() {
            commands.queue(move |world: &mut World| {
                world
                    .resource_mut::<Assets<ColorMaterial>>()
                    .remove(&material);
            });
        }
        self.fingerprint = 0;
    }
}

/// 档位 → （步距像素，符号边长像素）。
fn band_params(spec: &ForestSymbolSpec, band: DecorBand) -> (f32, f32) {
    match band {
        DecorBand::Nation => (spec.band_nation.spacing_px, spec.band_nation.size_px),
        DecorBand::Region => (spec.band_region.spacing_px, spec.band_region.size_px),
        DecorBand::Edit => (spec.band_edit.spacing_px, spec.band_edit.size_px),
    }
}

/// 地形森林位图树冠主 system。
///
/// 每帧按视口重采样（与 `terrain_decor` 同口径：屏幕像素恒定的步距），
/// 几何变化才原地改写 mesh。
#[allow(clippy::too_many_arguments)]
pub fn sync_forest_canopy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<crate::state::DrillState>,
    grid: Res<GridConfig>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    mut pool: ResMut<ForestCanopyPool>,
    mut density_cache: ResMut<ForestDensityCache>,
) {
    // 形态二选一（§七）：主题没声明位图森林 → 本层退场（程序化林冠点归
    // `terrain_decor`）。主题换包 / 换日暮即刻换形态。
    let Some(spec) = view.active_theme.decor.forest_symbol else {
        pool.clear(&mut commands);
        return;
    };
    // 与装饰同口径：装饰 pass 关 / 地形层不可见 → 树也不画。
    if !recipe.enabled(PassId::TerrainDecor) || !layers.terrain.visible {
        pool.clear(&mut commands);
        return;
    }
    let Some(band) = decor_band(view.scale) else {
        pool.clear(&mut commands);
        return;
    };
    let Some((_, data)) = active_terrain(&projection.world, drill.current()) else {
        pool.clear(&mut commands);
        return;
    };
    if grid.cell_size <= 0.0 || data.width == 0 || data.height == 0 {
        pool.clear(&mut commands);
        return;
    }

    // 森林密度场（地形事实：群系 + 林线 + 聚集噪声）。
    // §九-3：帧首同步一次（地形变了才清块），帧内逐格惰性取值。
    density_cache.sync(data);
    let gain = view.active_theme.decor.forest_density_gain;

    let (spacing_px, size_px) = band_params(&spec, band);
    let spacing_wu = spacing_px * view.scale;
    let size_wu = size_px * view.scale;
    if spacing_wu <= 0.0 || size_wu <= 0.0 {
        pool.clear(&mut commands);
        return;
    }

    // 视口矩形（世界坐标）+ 外扩余量。
    let half = view.window_size * view.scale * 0.5 + Vec2::splat(VIEWPORT_MARGIN_PX * view.scale);
    let rect_min = view.camera_center - half;
    let rect_max = view.camera_center + half;
    // **「太远不画」**（ADR-0052 追记五）：物理口径优先——地图声明了
    // 「1 格 = 多少 km」就按 km 判定（树是几十米的东西，视野几十公里宽时
    // 单株不足一像素）；未声明尺度才退回几何口径（Nation 档 / 整图可见）。
    let span = rect_max - rect_min;
    let km_per_wu = drill
        .current_node(&projection.world)
        .and_then(|n| n.km_per_wu())
        .map(|(kx, ky)| (kx + ky) * 0.5);
    if grid.forest_sprites_too_far(
        span,
        km_per_wu,
        band == DecorBand::Nation,
        spec.nation_visible,
    ) {
        pool.clear(&mut commands);
        return;
    }

    let origin = grid.raster_origin;
    let cell = grid.cell_size;
    let variants = spec.variants.max(1);

    // 采样格**世界锚定**（§九-3 的「Chunk 级稳定生成」）：格点落在
    // `世界坐标 = 格号 × 步距`上，抖动由**格号**哈希给出。这样平移相机时
    // 同一棵树的世界位置不变（只会在视口边缘进出），不会整片重洗——
    // 视口锚定的写法会让「拖一下地图，树全换位置」。
    let gx0 = (rect_min.x / spacing_wu).floor() as i64;
    let gy0 = (rect_min.y / spacing_wu).floor() as i64;
    let gx1 = (rect_max.x / spacing_wu).ceil() as i64;
    let gy1 = (rect_max.y / spacing_wu).ceil() as i64;
    // 兜底：极端缩放（视口 ≫ 步距）下候选量会爆炸，宁可这一帧不画也不卡死。
    let candidate_count = (gx1 - gx0 + 1).saturating_mul(gy1 - gy0 + 1);
    if candidate_count > MAX_CANDIDATES {
        pool.clear(&mut commands);
        return;
    }

    let mut trees: Vec<Tree> = Vec::new();
    for gy_lattice in gy0..=gy1 {
        if trees.len() >= MAX_TREES {
            break;
        }
        for gx_lattice in gx0..=gx1 {
            if trees.len() >= MAX_TREES {
                break;
            }
            // 落点抖动（±0.35 步距）：规则网格会读成「棋盘」——
            // 同一理由见 `terrain_decor` 的站点抖动（ADR-0042 追记 D13）。
            let jx = (hash01(gx_lattice, gy_lattice, 0x51) - 0.5) * spacing_wu * 0.7;
            let jy = (hash01(gx_lattice, gy_lattice, 0x52) - 0.5) * spacing_wu * 0.7;
            let px = gx_lattice as f32 * spacing_wu + jx;
            let py = gy_lattice as f32 * spacing_wu + jy;

            // 抖动后的落点 → 栅格格（归属按**落点**所在格，不按网格点）。
            let gx = ((px - origin.x) / cell).floor();
            let gy = ((py - origin.y) / cell).floor();
            if gx < 0.0 || gy < 0.0 {
                continue;
            }
            let (cx, cy) = (gx as u32, gy as u32);
            if cx >= data.width || cy >= data.height {
                continue;
            }
            let idx = (cy * data.width + cx) as usize;
            let Some(cell_data) = data.cells.get(idx) else {
                continue;
            };
            // 只有**森林群系**才长树：密度场对非森林群系本就趋零，但群系判定
            // 更直白——「林地上的水」「河岸的草」不该长树。
            if !is_forest_family(cell_data.biome) {
                continue;
            }
            let d = density_cache.density_at(data, cx, cy);
            // 密度分类：`< 0.18` 不算林（不成片）。
            let Some(class) = classify_forest(d) else {
                continue;
            };
            // 保留率 = 密度场 × 主题增益（疏密是主题的参数，见 ADR-0031 追记一）。
            if hash01(cx as i64, cy as i64, 0x7A) > forest_keep_prob(d, gain) {
                continue;
            }
            // 变体：哈希取图集格（确定性——同一地形每次打开树形一致）。
            let variant =
                ((hash01(cx as i64, cy as i64, 0x7B) * variants as f32) as u32) % variants;
            // 树冠大小随密度（§九-2：密林树冠更大）+ 逐树微抖动（避免整齐）。
            let size =
                size_wu * (0.72 + 0.56 * d) * (0.88 + 0.24 * hash01(cx as i64, cy as i64, 0x7C));
            let tint = match class {
                ForestType::Sparse => spec.tint_sparse,
                ForestType::Normal => spec.tint_normal,
                ForestType::Dense => spec.tint_dense,
            };
            trees.push(Tree {
                x: px,
                y: py,
                size,
                variant,
                tint: [
                    tint[0] as f32 / 255.0,
                    tint[1] as f32 / 255.0,
                    tint[2] as f32 / 255.0,
                ],
            });
        }
    }

    // 画家算法：按 y 升序排（屏幕下方者后画 → 盖在靠上者之前），
    // 树与树的前后关系因此正确（否则会随机穿插）。
    trees.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));

    // 贴图：按键缓存；换键（换主题 / 换日暮）才重解码。
    if pool.texture_key != Some(spec.texture) {
        pool.texture_key = Some(spec.texture);
        // 旧贴图资产回收（换主题 / 换日暮时）：留在 Assets 里就是白占显存。
        if let Some(old) = pool.texture.take() {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Image>>().remove(&old);
            });
        }
        pool.texture_failed = false;
        pool.material = None;
    }
    if pool.texture.is_none() && !pool.texture_failed {
        match crate::feature_overlays::embedded_texture(spec.texture)
            .and_then(crate::feature_overlays::decode_texture)
        {
            Some(image) => {
                pool.texture = Some(images.add(image));
            }
            None => {
                // 缺资产 = 主题声明了位图森林但贴图不在内嵌表：告警一次，
                // 本层不画。**不回退程序化形态**——形态是主题的一次性选择，
                // 静默换形态会让人以为「改了主题没生效」。
                warn!("森林位图贴图键不在内嵌表或解码失败：{}", spec.texture);
                pool.texture_failed = true;
            }
        }
    }
    let Some(texture) = pool.texture.clone() else {
        pool.clear(&mut commands);
        return;
    };

    // 材质：不透明度在材质 color 的 alpha 上（与覆盖物路径同口径）。
    // **只在需要时重建**：材质变了才重挂实体——每帧 `insert` 同一句柄会被
    // Bevy 记成「组件变更」，白白触发一次材质重提取。
    let mut material_changed = false;
    if pool.material.is_none() || pool.material_alpha != spec.alpha {
        pool.material = Some(materials.add(ColorMaterial {
            color: Color::WHITE.with_alpha(spec.alpha),
            texture: Some(texture),
            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
            ..default()
        }));
        pool.material_alpha = spec.alpha;
        material_changed = true;
    }
    let material = pool.material.clone().expect("材质已建");

    // 几何 → mesh（一个 quad 一棵树：4 顶点 / 6 索引 / 4 UV / 4 顶点色）。
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(trees.len() * 4);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(trees.len() * 4);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(trees.len() * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(trees.len() * 6);
    let cell_uv = 1.0 / variants as f32;
    let aspect = spec.cell_aspect;
    for t in &trees {
        let q = tree_quad_bounds(t.x, t.y, t.size, aspect);
        let u0 = t.variant as f32 * cell_uv;
        let u1 = u0 + cell_uv;
        let base = positions.len() as u32;
        // 锚点 = 树根（底边中点）：树在屏幕上正立——冠在上、根在下。
        for (px, py) in q {
            positions.push([px, py, 0.0]);
        }
        // PNG 第 0 行 = 树冠顶 → v=0 绑 +Y 顶点（与覆盖物路径同口径）。
        uvs.push([u0, 1.0]);
        uvs.push([u1, 1.0]);
        uvs.push([u1, 0.0]);
        uvs.push([u0, 0.0]);
        let c = [t.tint[0], t.tint[1], t.tint[2], 1.0];
        colors.extend_from_slice(&[c, c, c, c]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    if trees.is_empty() {
        pool.clear(&mut commands);
        return;
    }

    let fingerprint = points_fingerprint(&positions);
    match (pool.entity, pool.mesh.clone()) {
        (None, _) => {
            let mesh = meshes.add(build_canopy_mesh(positions, uvs, colors, indices));
            let entity = commands
                .spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material),
                    Transform::from_xyz(0.0, 0.0, CANOPY_Z),
                ))
                .id();
            pool.entity = Some(entity);
            pool.mesh = Some(mesh);
            pool.fingerprint = fingerprint;
        }
        (Some(entity), Some(mesh)) => {
            if pool.fingerprint != fingerprint {
                if let Some(mut m) = meshes.get_mut(&mesh) {
                    *m = build_canopy_mesh(positions, uvs, colors, indices);
                }
                pool.fingerprint = fingerprint;
            }
            // 材质换了（贴图 / 不透明度变）→ 重挂；没换就别碰
            // （每帧重挂 = 每帧触发材质重提取）。
            if material_changed {
                commands.entity(entity).insert(MeshMaterial2d(material));
            }
        }
        (Some(entity), None) => {
            // 池状态不自洽（mesh 句柄丢了）→ 重建实体。
            commands.entity(entity).despawn();
            pool.entity = None;
            pool.clear(&mut commands);
        }
    }
}

/// 树图集四边 → 世界坐标（左下 / 右下 / 右上 / 左上）。
///
/// **底边下沉 `TREE_BASE_SINK`**：素材格里树干底边离格底 2–6%，
/// 下沉 3% 让树根压在地面落点上（见 [`TREE_BASE_SINK`]）。
///
/// **宽度按格比**（`aspect` = 格宽 ÷ 格高）：委托设计的单株格是 192×260，
/// 用正方形 quad 会把树横向拉扁——`size` 记的是**高度**。
fn tree_quad_bounds(x: f32, y: f32, size: f32, aspect: f32) -> [(f32, f32); 4] {
    let hw = size * 0.5 * aspect.max(0.05);
    let y_bottom = y - size * TREE_BASE_SINK;
    let y_top = y_bottom + size;
    [
        (x - hw, y_bottom),
        (x + hw, y_bottom),
        (x + hw, y_top),
        (x - hw, y_top),
    ]
}

/// 树冠 mesh（位置 + UV + 顶点色 + 索引）。
///
/// **顶点色承载密度分级 tint**：Bevy 的 `ColorMaterial` 着色器按
/// `material.color × 顶点色 × 贴图` 相乘，故一棵树的 tint 走顶点色即可——
/// 单 mesh 单材质，不必按密度分级拆批（拆批会破坏画家算法的前后关系）。
fn build_canopy_mesh(
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{PassId, PassPlan, RenderRecipe};
    use crate::state::DrillState;
    use bevy::ecs::schedule::Schedule;
    use wv_core::terrain::{TerrainCellData, TerrainData};
    use wv_terrain::biome::{BIOME_DESERT, BIOME_FOREST};

    /// 主题夹具：位图形态 / 程序化形态各一份（静态——`ViewState.active_theme`
    /// 持 `&'static ThemePack`）。
    fn theme_with_form(with_bitmap: bool) -> &'static crate::theme::theme_pack::ThemePack {
        static BITMAP: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        static PROCEDURAL: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        let slot = if with_bitmap { &BITMAP } else { &PROCEDURAL };
        slot.get_or_init(|| crate::theme::theme_pack::ThemePack {
            decor: crate::theme::decor::DecorRecipe {
                forest_symbol: if with_bitmap {
                    crate::theme::decor::WATERCOLOR.forest_symbol
                } else {
                    None
                },
                ..crate::theme::decor::WATERCOLOR
            },
            ..*crate::theme::theme_pack::builtin::for_palette(Default::default())
        })
    }

    /// 装配一个最小 App（照 `terrain_decor` 的系统级测试口径）。
    fn app_with(biome: u8, with_bitmap: bool) -> (World, Schedule) {
        let mut world = wv_core::World::default();
        let map_id = wv_core::Id::new_unchecked("map_t");
        world.maps.push(wv_core::map::MapNode {
            id: map_id.clone(),
            ..Default::default()
        });
        let n = 32u32;
        let cells = vec![
            TerrainCellData {
                height: 500,
                surface: 1,
                biome,
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
            active_theme: theme_with_form(with_bitmap),
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
        app.init_resource::<ForestDensityCache>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Image>>();
        app.init_resource::<ForestCanopyPool>();
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_forest_canopy);
        (app, schedule)
    }

    /// 位图形态 + 森林群系 → 画出树（实体存在、mesh 有顶点）。
    #[test]
    fn canopy_draws_trees_on_forest_terrain() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        schedule.run(&mut app);
        let pool = app.resource::<ForestCanopyPool>();
        let entity = pool.entity.expect("位图形态 + 森林群系应画出树冠实体");
        let mesh = pool.mesh.clone().expect("应有 mesh 句柄");
        let verts = app
            .resource::<Assets<Mesh>>()
            .get(&mesh)
            .expect("mesh 应存在")
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(
            verts >= 4,
            "应至少有一棵树（4 顶点 / 树），got {verts} 顶点"
        );
        assert_eq!(verts % 4, 0, "顶点数应是 4 的倍数（每树一个 quad）");
        // 实体确实挂着 mesh 与材质。
        assert!(
            app.entity(entity).get::<Mesh2d>().is_some(),
            "实体应挂 Mesh2d"
        );
        assert!(
            app.entity(entity)
                .get::<MeshMaterial2d<ColorMaterial>>()
                .is_some(),
            "实体应挂 MeshMaterial2d"
        );
    }

    /// 程序化形态（主题没给位图规格）→ 本层不画（形态归 `terrain_decor`）。
    #[test]
    fn canopy_is_inert_for_procedural_form_themes() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, false);
        schedule.run(&mut app);
        let pool = app.resource::<ForestCanopyPool>();
        assert!(
            pool.entity.is_none(),
            "程序化形态下树冠层不应画树（形态二选一）"
        );
    }

    /// 非森林群系 → 不长树（哪怕主题声明了位图形态）。
    #[test]
    fn canopy_draws_nothing_on_non_forest_biome() {
        let (mut app, mut schedule) = app_with(BIOME_DESERT, true);
        schedule.run(&mut app);
        let pool = app.resource::<ForestCanopyPool>();
        assert!(pool.entity.is_none(), "荒漠不该长树（密度场对荒漠给零）");
    }

    /// 二帧稳定：几何未变 → 不新增 mesh 资产（原地改写纪律）。
    #[test]
    fn canopy_does_not_accumulate_assets_across_frames() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        schedule.run(&mut app);
        let first = app.resource::<Assets<Mesh>>().iter().count();
        schedule.run(&mut app);
        let second = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            second, first,
            "第二帧不应新增 mesh（got {first} → {second}）"
        );
    }

    /// 关掉装饰 pass → 树冠层整组下线（与装饰同口径）。
    #[test]
    fn canopy_clears_when_decor_pass_is_disabled() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_some(),
            "先确认有树，再验证下线"
        );
        let mut recipe = app.resource_mut::<RenderRecipe>();
        recipe.passes[PassId::TerrainDecor as usize] = PassPlan::disabled();
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_none(),
            "pass 关闭后树冠应全部清除"
        );
    }

    /// 树根锚点：图集格底边下沉 15% → 格内 85% 处（树干的实际底边）落在落点上，
    /// 树**不浮空**。
    #[test]
    fn tree_quad_sinks_base_so_trunk_lands_on_anchor() {
        let size = 10.0;
        let q = tree_quad_bounds(3.0, 20.0, size, 1.0);
        let bottom = q[0].1;
        let top = q[2].1;
        assert!((top - bottom - size).abs() < 1e-5, "格高应等于符号边长");
        assert!((q[0].1 - q[1].1).abs() < 1e-5, "底边两点应等高");
        // 委托设计素材的树干底边在格高 ~97% 处（v=0 在格顶）→
        // 世界 y = 格顶 - 0.97 × 边长 = 锚点（下沉量 3%）。
        let trunk_base = top - 0.97 * size;
        assert!(
            (trunk_base - 20.0).abs() < 1e-4,
            "树干底边应落在锚点 20.0 附近，got {trunk_base}"
        );
    }

    /// 树根世界锚点（底边中点，已补偿下沉量）——平移比较用。
    fn tree_anchors(app: &World) -> Vec<(f32, f32)> {
        use bevy::render::mesh::VertexAttributeValues;
        let Some(mesh) = app.resource::<ForestCanopyPool>().mesh.clone() else {
            return Vec::new();
        };
        let Some(mesh) = app.resource::<Assets<Mesh>>().get(&mesh) else {
            return Vec::new();
        };
        let Some(VertexAttributeValues::Float32x3(v)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Vec::new();
        };
        v.chunks(4)
            .map(|q| {
                let x = (q[0][0] + q[1][0]) * 0.5;
                let size = q[2][1] - q[0][1];
                // 反解锚点：底边 = 锚点 - 边长 × 下沉量。
                let y = q[0][1] + size * TREE_BASE_SINK;
                (x, y)
            })
            .collect()
    }

    /// **采样格世界锚定**（§九-3 的「Chunk 级稳定生成」）：平移相机后，
    /// 仍在视口里的树必须落在**同一个世界坐标**上（不是「拖一下地图树全换位」）。
    ///
    /// 反证：视口锚定的写法里抖动取 `(col, row)`（从视口左上角数起），
    /// 平移会把整张采样格错位，所有树同时改变位置——本测试因此会失败。
    #[test]
    fn tree_positions_stay_pinned_when_the_camera_pans() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        schedule.run(&mut app);
        let before = tree_anchors(&app);
        assert!(!before.is_empty(), "夹具应至少画出一棵树");

        // 取「两个视口交集」再内缩一截，落在这里的树必须在平移后原样存在。
        let (center, half) = {
            let view = app.resource::<ViewState>();
            (view.camera_center, view.window_size * view.scale * 0.5)
        };
        let margin = Vec2::splat(80.0);
        let pan = Vec2::new(37.0, 23.0);
        let safe_min = center - half + margin;
        let safe_max = center + half - pan - margin;

        app.resource_mut::<ViewState>().camera_center = center + pan;
        schedule.run(&mut app);
        let after = tree_anchors(&app);

        let pinned: Vec<_> = before
            .iter()
            .filter(|(x, y)| {
                *x >= safe_min.x && *x <= safe_max.x && *y >= safe_min.y && *y <= safe_max.y
            })
            .collect();
        assert!(
            !pinned.is_empty(),
            "交集区内应至少有一棵树可供比较（before={} 棵）",
            before.len()
        );
        for p in pinned {
            assert!(
                after.contains(p),
                "平移后这棵树应先世界坐标不变：{p:?}（after={} 棵）",
                after.len()
            );
        }
    }

    /// **档位门控**（ADR-0052 追记二）：世界级视距（Nation）不画单株。
    #[test]
    fn trees_are_hidden_at_the_nation_band() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        app.resource_mut::<ViewState>().scale = 5.0; // >= LOD_NATION_SCALE
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_none(),
            "世界级视距不该贴单株（那尺度由林底色调承担）"
        );
    }

    /// 同一条门控的正面：Region 档照画（缩放进来树才出现）。
    #[test]
    fn trees_appear_at_the_region_band() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        app.resource_mut::<ViewState>().scale = 1.5; // Region
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_some(),
            "Region 档应有单株"
        );
    }

    /// **物理口径**：地图声明「1 格 = 多少 km」（`unit_per_km`）→ 按 km 判定，
    /// 与绝对缩放值无关——同一视口，大陆图（视野几百 km）不画，区域图（视野
    /// 十几 km）照画。
    #[test]
    fn trees_follow_the_declared_km_scale() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        // 1 wu = 1 km：Edit 档视口 640 wu = 640 km ⇒ 太远，不画。
        {
            let mut proj = app.resource_mut::<WorldProjection>();
            if let Some(m) = proj.world.maps.first_mut() {
                m.unit_per_km = Some(1.0);
            }
        }
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_none(),
            "640 km 视野不该画单株（树在真实世界只有几十米）"
        );

        // 同一视口，改成 1 km = 64 wu：视野只有 10 km ⇒ 画。
        {
            let mut proj = app.resource_mut::<WorldProjection>();
            if let Some(m) = proj.world.maps.first_mut() {
                m.unit_per_km = Some(64.0);
            }
        }
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_some(),
            "10 km 视野应画单株"
        );
    }

    /// **整图可见门控**：一屏能看完整张图时，树是「贴纸」——不画。
    ///
    /// 夹具把图幅缩到 800：Edit 档视口（640×400 wu）已 ≥ 图幅 60% ⇒ 整图可见。
    #[test]
    fn trees_hidden_when_the_whole_map_is_visible() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        app.resource_mut::<GridConfig>().map_size = Vec2::splat(800.0);
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestCanopyPool>().entity.is_none(),
            "整图可见时不该贴树"
        );
    }

    /// 密度场按块惰性计算（§九-3）：一帧只算**扫到的块**，不整场预算 32² 块。
    #[test]
    fn canopy_only_forces_the_chunks_it_samples() {
        let (mut app, mut schedule) = app_with(BIOME_FOREST, true);
        schedule.run(&mut app);
        let computed = app.resource::<ForestDensityCache>().computed_chunks();
        // 32² 地图在 scale 0.5 下视口约 640×400 wu、格 4 wu ⇒ 160×100 格，
        // 128 格一块 ⇒ 至多 4 块；整图预算是 1 块（32 格 < 128）。
        assert!(computed <= 4, "只该算视口扫到的块，got {computed} 块");
        assert!(computed >= 1, "至少要算一块");
    }
}
