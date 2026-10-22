//! 地形森林的**团块层**（§七 的第二尺度，2026-10-22 追记）。
//!
//! ## 为什么要有这一层
//!
//! 单株树（[`crate::forest_canopy`]）解决的是「近看有树」，但**地图尺度**上
//! 手绘地图的森林是**一整片**——参考图 `100+ Hand-drawn Trees and Plants` 的
//! 上半部分就是这种读法：一块有机形状的密林，边缘是连续的树冠轮廓。
//! 只有单株时，森林读作「草地底色 + 零星几棵树」，与参考图差一层。
//!
//! ## 两段式
//!
//! 1. **填充**（`fill_*`）：在密度达标的格位逐块铺可平铺的俯视森林底纹。
//!    UV **世界锚定**（`世界坐标 / 贴图边长`），相邻块共用同一映射 →
//!    看起来是一整张连续的底纹，不是格子拼贴。
//! 2. **边缘**（`patch_*`）：密度**边界**处的格位盖「团块」贴图
//!    （一小丛林，自带不规则边缘）——把填充层的格状边界打散成树冠轮廓。
//!
//! ## 与其它森林层的关系
//!
//! | 层 | 尺度 | 角色 |
//! |---|---|---|
//! | 林底蒙版（`cells.rs`，烘焙） | 格子 | 底色，跟着水彩渗色晕开 |
//! | **团块层（本模块）** | 区域 | 远 / 中景主力：森林读作一片 |
//! | 单株树（`forest_canopy`） | 个体 | 近景点缀，落在团块之上 |
//!
//! 三层都由**同一个密度场**驱动（[`crate::forest_density`]），故「哪里是林」
//! 一致；三层都是**派生量、不落盘**（P7）。
//!
//! ## 边界（不做什么）
//!
//! - **不做区域多边形**（不跑连通分区 / 三角化）：森林是逐格判定的，边界
//!   天然跟随密度；切多边形再铺贴图会引入「区域版本」这第二套真相；
//! - **不写世界**、**不落盘**；
//! - **不做逐块脏区**：地形变了就整层重建（重建只发生在视口内）。

#![cfg(feature = "app")]

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, MeshVertexAttribute};
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};

use crate::forest_density::ForestDensityCache;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{GridConfig, ViewState, WorldProjection};
use crate::terrain_decor::{decor_band, hash01, points_fingerprint, DecorBand};
use crate::terrain_view::terrain_resolve::active_terrain;

/// 填充层 Z：地形装饰（0.075）之上、边缘团块与单株树之下。
const FILL_Z: f32 = 0.0765;
/// 边缘团块 Z：填充之上、单株树（0.078）之下。
const PATCH_Z: f32 = 0.0775;
/// 视口外扩余量（屏幕像素）：拖动相机时边缘不闪断。
const VIEWPORT_MARGIN_PX: f32 = 32.0;
/// 单帧填充块上限（兜底）。
///
/// 正常量级 = 视口 ÷ 步距（与缩放无关，两者都乘 `view.scale`）：
/// 1600×1000 屏、步距 22px ⇒ 约 73×46 ≈ 3300 块。上限只是防病态主题参数。
const MAX_FILL_QUADS: usize = 20_000;
/// 单帧边缘团块上限（兜底）：边界长度 ÷ 步距，千级。
const MAX_PATCHES: usize = 4_000;

/// 一层的运行时状态（实体 / mesh / 材质 / 贴图 / 指纹）。
///
/// **为什么拆子结构**：系统里要跨函数传「这一层的全部可变状态」，把它们打包
/// 才能一次借出（`&mut pool.fill`），否则 Rust 不允许同时借多个字段。
#[derive(Default)]
pub(crate) struct LayerPool {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<ColorMaterial>>,
    /// 材质 alpha 快照（变了才重建材质）。
    material_alpha: f32,
    /// 贴图键 / 句柄 / 失败标记（按 key 缓存，换 key 才重解码）。
    texture_key: Option<&'static str>,
    texture: Option<Handle<Image>>,
    texture_failed: bool,
    /// 几何指纹（变了才原地改写 mesh）。
    fingerprint: u64,
}

impl LayerPool {
    /// 当前 mesh 句柄（诊断 / 单测用）。
    pub(crate) fn mesh(&self) -> Option<&Handle<Mesh>> {
        self.mesh.as_ref()
    }

    /// 下线：实体与 mesh / 材质回收（贴图句柄保留，换回来时还能用）。
    pub(crate) fn clear(&mut self, commands: &mut Commands) {
        if let Some(e) = self.entity.take() {
            commands.entity(e).despawn();
        }
        if let Some(m) = self.mesh.take() {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&m);
            });
        }
        if let Some(m) = self.material.take() {
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<ColorMaterial>>().remove(&m);
            });
        }
        self.fingerprint = 0;
    }

    /// 取内嵌贴图（按 key 缓存；换 key 才重解码）。
    pub(crate) fn ensure_texture(
        &mut self,
        images: &mut Assets<Image>,
        commands: &mut Commands,
        key: &'static str,
        what: &str,
    ) -> Option<Handle<Image>> {
        if self.texture_key != Some(key) {
            self.texture_key = Some(key);
            self.texture_failed = false;
            if let Some(old) = self.texture.take() {
                commands.queue(move |world: &mut World| {
                    world.resource_mut::<Assets<Image>>().remove(&old);
                });
            }
            self.material = None;
        }
        if self.texture.is_none() && !self.texture_failed {
            match crate::feature_overlays::embedded_texture(key)
                .and_then(crate::feature_overlays::decode_texture)
            {
                Some(image) => self.texture = Some(images.add(image)),
                None => {
                    // 缺资产：告警一次，该层不画（**不回退**到别的形态——
                    // 静默换形态会让人以为「换了主题没生效」）。
                    warn!("森林团块贴图键不在内嵌表或解码失败（{what}）：{key}");
                    self.texture_failed = true;
                }
            }
        }
        self.texture.clone()
    }

    /// 几何 → 实体（空则下线；非空则新建或原地改写）。
    pub(crate) fn sync_geometry(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<ColorMaterial>,
        positions: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        indices: Vec<u32>,
        color: Color,
        z: f32,
    ) {
        if positions.is_empty() {
            self.clear(commands);
            return;
        }
        let Some(texture) = self.texture.clone() else {
            // 贴图缺失：该层不画（已告警一次）。
            self.clear(commands);
            return;
        };
        if self.material.is_none() || self.material_alpha != color.alpha() {
            self.material = Some(materials.add(ColorMaterial {
                color,
                texture: Some(texture),
                alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
                ..default()
            }));
            self.material_alpha = color.alpha();
        }
        let material = self.material.clone().expect("材质已建");
        let fingerprint = points_fingerprint(&positions);
        match (self.entity, self.mesh.clone()) {
            (None, _) => {
                let mesh = meshes.add(build_layer_mesh(positions, uvs, indices));
                let e = commands
                    .spawn((
                        Mesh2d(mesh.clone()),
                        MeshMaterial2d(material),
                        Transform::from_xyz(0.0, 0.0, z),
                    ))
                    .id();
                self.entity = Some(e);
                self.mesh = Some(mesh);
                self.fingerprint = fingerprint;
            }
            (Some(_), Some(mesh)) => {
                if self.fingerprint != fingerprint {
                    if let Some(mut m) = meshes.get_mut(&mesh) {
                        *m = build_layer_mesh(positions, uvs, indices);
                    }
                    self.fingerprint = fingerprint;
                }
            }
            (Some(_), None) => {
                self.clear(commands);
            }
        }
    }
}

/// 团块层的两层状态（填充 / 边缘团块）。
#[derive(Resource, Default)]
pub struct ForestMassPool {
    fill: LayerPool,
    patch: LayerPool,
}

/// 团块层主 system。
#[allow(clippy::too_many_arguments)]
pub fn sync_forest_mass(
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
    mut pool: ResMut<ForestMassPool>,
    mut density_cache: ResMut<ForestDensityCache>,
) {
    // 形态：主题没声明团块规格 → 本层退场（水墨包就是这种情况）。
    let Some(spec) = view.active_theme.decor.forest_mass else {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    };
    if !recipe.enabled(PassId::TerrainDecor) || !layers.terrain.visible {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    }
    // **档位门控**（ADR-0052 追记二）：世界级视距不画树簇（同单株，靠林底色调）。
    if decor_band(view.scale) == Some(DecorBand::Nation) && !spec.nation_visible {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    }
    let Some((_, data)) = active_terrain(&projection.world, drill.current()) else {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    };
    if grid.cell_size <= 0.0 || data.width == 0 || data.height == 0 {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    }
    // 帧首同步一次（§九-3 的调用纪律），帧内逐格取值零成本。
    density_cache.sync(data);

    let step_wu = spec.fill_step_px * view.scale;
    let tile_wu = spec.fill_tile_px * view.scale;
    if step_wu <= 0.0 || tile_wu <= 0.0 {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    }

    let half = view.window_size * view.scale * 0.5 + Vec2::splat(VIEWPORT_MARGIN_PX * view.scale);
    let rect_min = view.camera_center - half;
    let rect_max = view.camera_center + half;
    // **整图可见门控**（同 forest_canopy，ADR-0052 追记四）。
    if grid.whole_map_visible(rect_max - rect_min) {
        pool.fill.clear(&mut commands);
        pool.patch.clear(&mut commands);
        return;
    }

    let origin = grid.raster_origin;
    let cell = grid.cell_size;
    // 边缘判定的邻格距离（格）：一个采样步距折算成格，至少 1。
    let probe_cells = (step_wu / cell).round().max(1.0) as i64;

    // 采样格**世界锚定**（同 `forest_canopy` 的纪律）：平移相机时填充不滑动。
    let gx0 = (rect_min.x / step_wu).floor() as i64;
    let gy0 = (rect_min.y / step_wu).floor() as i64;
    let gx1 = (rect_max.x / step_wu).ceil() as i64;
    let gy1 = (rect_max.y / step_wu).ceil() as i64;

    let mut fill_pos: Vec<[f32; 3]> = Vec::new();
    let mut fill_uv: Vec<[f32; 2]> = Vec::new();
    let mut fill_idx: Vec<u32> = Vec::new();
    let mut patch_pos: Vec<[f32; 3]> = Vec::new();
    let mut patch_uv: Vec<[f32; 2]> = Vec::new();
    let mut patch_idx: Vec<u32> = Vec::new();

    // 填充块之间留一点重叠，否则步距取整会在缝里露出底色。
    let quad_half = step_wu * 0.62;

    for gy_lat in gy0..=gy1 {
        for gx_lat in gx0..=gx1 {
            let wx = gx_lat as f32 * step_wu;
            let wy = gy_lat as f32 * step_wu;
            let cell_x = ((wx - origin.x) / cell).floor();
            let cell_y = ((wy - origin.y) / cell).floor();
            if cell_x < 0.0 || cell_y < 0.0 {
                continue;
            }
            let (cx, cy) = (cell_x as u32, cell_y as u32);
            if cx >= data.width || cy >= data.height {
                continue;
            }
            let d = density_cache.density_at(data, cx, cy);
            if d < spec.fill_min_density {
                continue;
            }

            // 内 / 边判定：四邻（一个步距外）是否都在阈值上。
            //
            // **为什么先腐蚀**：只按「本格达标」就填，会在阈值附近留下**孤立
            // 单块**（画面上的绿色小方块 / 条纹）。故把「内部」定义为四邻都达标
            // ——填充只铺内部，边界带交给团块（团块自带不规则边缘，正好吃掉
            // 台阶）。这就是形态学腐蚀，一步到位。
            let mut interior = true;
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let nx = cx as i64 + dx * probe_cells;
                let ny = cy as i64 + dy * probe_cells;
                let nd = if nx < 0 || ny < 0 {
                    0.0
                } else {
                    density_cache.density_at(data, nx as u32, ny as u32)
                };
                if nd < spec.fill_min_density {
                    interior = false;
                    break;
                }
            }

            // 填充块（仅内部）：世界锚定的 UV（相邻块共用映射 → 看不出格子）。
            if interior && fill_pos.len() / 4 < MAX_FILL_QUADS {
                let u0 = wx / tile_wu;
                let v0 = wy / tile_wu;
                let du = (quad_half * 2.0) / tile_wu;
                let base = fill_pos.len() as u32;
                fill_pos.push([wx - quad_half, wy - quad_half, 0.0]);
                fill_pos.push([wx + quad_half, wy - quad_half, 0.0]);
                fill_pos.push([wx + quad_half, wy + quad_half, 0.0]);
                fill_pos.push([wx - quad_half, wy + quad_half, 0.0]);
                fill_uv.push([u0, v0]);
                fill_uv.push([u0 + du, v0]);
                fill_uv.push([u0 + du, v0 + du]);
                fill_uv.push([u0, v0 + du]);
                fill_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }

            // 边缘带（达标但非内部）盖团块。
            //
            // **稀疏化**：边界上每个步距点都盖会重叠加糊（首版 132px 团块配
            // 22px 步距 = 一块盖住几十个邻点，整片糊成一团）。故按哈希只留约
            // 四成——团块本身远大于步距，四成仍连得成树冠轮廓。
            if !interior
                && spec.patch_texture.is_some()
                && patch_pos.len() / 4 < MAX_PATCHES
                && hash01(gx_lat, gy_lat, 0x95) < 0.42
            {
                // 团块：逐块抖动位置 / 大小 / 旋转 / **图集变体**
                // （否则边界是一排等距同款，密集林区还会明显重复同一团）。
                let jx = (hash01(gx_lat, gy_lat, 0x91) - 0.5) * step_wu;
                let jy = (hash01(gx_lat, gy_lat, 0x92) - 0.5) * step_wu;
                let size =
                    spec.patch_px * view.scale * (0.82 + 0.42 * hash01(gx_lat, gy_lat, 0x93));
                let angle = hash01(gx_lat, gy_lat, 0x94) * std::f32::consts::TAU;
                let (s, c) = angle.sin_cos();
                let cxp = wx + jx;
                let cyp = wy + jy;
                // 非方格：`patch_aspect` = 格宽 ÷ 格高（192/225），否则树冠被压扁。
                let hh = size * 0.5;
                let hw = hh * spec.patch_aspect.max(0.05);
                let variants = spec.patch_variants.max(1);
                let cell = 1.0 / variants as f32;
                let variant = ((hash01(gx_lat, gy_lat, 0x96) * variants as f32) as u32) % variants;
                let u0 = variant as f32 * cell;
                let u1 = u0 + cell;
                let base = patch_pos.len() as u32;
                for (ox, oy) in [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)] {
                    // 四角绕块心旋转：贴图跟着转（UV 不转，整张精灵一起转）。
                    patch_pos.push([cxp + ox * c - oy * s, cyp + ox * s + oy * c, 0.0]);
                }
                patch_uv.push([u0, 1.0]);
                patch_uv.push([u1, 1.0]);
                patch_uv.push([u1, 0.0]);
                patch_uv.push([u0, 0.0]);
                patch_idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    // ---- 填充层（可选）---------------------------------------------------
    // 委托设计的用法是「树簇堆叠成片」，不铺连续底纹；底纹仍留作可选手段。
    match spec.fill_texture {
        Some(fill_key) => {
            let _ = pool
                .fill
                .ensure_texture(&mut images, &mut commands, fill_key, "填充");
            pool.fill.sync_geometry(
                &mut commands,
                &mut meshes,
                &mut materials,
                fill_pos,
                fill_uv,
                fill_idx,
                Color::WHITE.with_alpha(spec.fill_alpha),
                FILL_Z,
            );
        }
        None => pool.fill.clear(&mut commands),
    }

    // ---- 边缘团块层 ------------------------------------------------------
    match spec.patch_texture {
        Some(patch_key) => {
            let _ = pool
                .patch
                .ensure_texture(&mut images, &mut commands, patch_key, "边缘团块");
            pool.patch.sync_geometry(
                &mut commands,
                &mut meshes,
                &mut materials,
                patch_pos,
                patch_uv,
                patch_idx,
                Color::WHITE.with_alpha(spec.patch_alpha),
                PATCH_Z,
            );
        }
        None => pool.patch.clear(&mut commands),
    }
}

/// 位置 + UV + 索引 → mesh（无顶点色：tint 走材质色）。
fn build_layer_mesh(positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        positions,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::PassPlan;
    use crate::state::DrillState;
    use bevy::ecs::schedule::Schedule;
    use wv_core::terrain::{TerrainCellData, TerrainData};
    use wv_terrain::biome::{BIOME_DESERT, BIOME_FOREST};

    /// 主题夹具：团块给 / 不给（`ViewState.active_theme` 持 `&'static`）。
    fn theme_with_mass(with_mass: bool) -> &'static crate::theme::theme_pack::ThemePack {
        static WITH: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        static WITHOUT: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        let slot = if with_mass { &WITH } else { &WITHOUT };
        slot.get_or_init(|| crate::theme::theme_pack::ThemePack {
            decor: crate::theme::decor::DecorRecipe {
                forest_mass: if with_mass {
                    crate::theme::decor::WATERCOLOR.forest_mass
                } else {
                    None
                },
                ..crate::theme::decor::WATERCOLOR
            },
            ..*crate::theme::theme_pack::builtin::for_palette(Default::default())
        })
    }

    /// 主题夹具：**显式打开底纹填充**（默认配方不铺底纹，见 ADR-0052 追记）。
    ///
    /// 底纹与树簇是两段独立开关：默认只用树簇堆叠成片；底纹留给
    /// 「大块密林还要再压暗一层」的场合，故它的测试要自己把开关打开。
    fn theme_with_fill() -> &'static crate::theme::theme_pack::ThemePack {
        static PACK: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        PACK.get_or_init(|| {
            let base = crate::theme::decor::WATERCOLOR
                .forest_mass
                .expect("默认配方应有团块规格");
            crate::theme::theme_pack::ThemePack {
                decor: crate::theme::decor::DecorRecipe {
                    forest_mass: Some(crate::theme::decor::ForestMassSpec {
                        fill_texture: Some("overlays/forest/forest_fill_gen.png"),
                        ..base
                    }),
                    ..crate::theme::decor::WATERCOLOR
                },
                ..*crate::theme::theme_pack::builtin::for_palette(Default::default())
            }
        })
    }

    /// 装配：左半森林 / 右半荒漠的 64² 世界 + Edit 档视口。
    fn mass_app(biome_right: u8) -> (World, Schedule) {
        let mut world = wv_core::World::default();
        let map_id = wv_core::Id::new_unchecked("map_t");
        world.maps.push(wv_core::map::MapNode {
            id: map_id.clone(),
            ..Default::default()
        });
        let n = 64u32;
        let cells = (0..n * n)
            .map(|i| {
                let x = i % n;
                TerrainCellData {
                    height: 500,
                    surface: 1,
                    biome: if x < n / 2 { BIOME_FOREST } else { biome_right },
                }
            })
            .collect();
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
            scale: 0.5,
            camera_center: Vec2::splat(n as f32 * 4.0 / 2.0),
            window_size: Vec2::new(1280.0, 800.0),
            active_theme: theme_with_mass(true),
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
        app.init_resource::<ForestMassPool>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Image>>();
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_forest_mass);
        (app, schedule)
    }

    /// 一层 mesh 的 quad 数（4 顶点 = 1 块）。
    fn quad_count(app: &World, mesh: &Handle<Mesh>) -> usize {
        app.resource::<Assets<Mesh>>()
            .get(mesh)
            .and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION).map(|a| a.len()))
            .unwrap_or(0)
            / 4
    }

    /// 把主题换成「显式打开底纹」的夹具（默认配方不铺底纹）。
    fn use_fill_theme(app: &mut World) {
        app.resource_mut::<ViewState>().active_theme = theme_with_fill();
    }

    /// **档位门控**（ADR-0052 追记二）：世界级视距（Nation）不画树簇。
    #[test]
    fn clusters_are_hidden_at_the_nation_band() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        app.resource_mut::<ViewState>().scale = 5.0;
        schedule.run(&mut app);
        let pool = app.resource::<ForestMassPool>();
        assert!(pool.patch.entity.is_none(), "Nation 档不该贴树簇");
        assert!(pool.fill.entity.is_none(), "Nation 档也不该铺填充");
    }

    /// 同一条门控的正面：Region 档照画。
    #[test]
    fn clusters_appear_at_the_region_band() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        app.resource_mut::<ViewState>().scale = 1.5;
        schedule.run(&mut app);
        assert!(
            app.resource::<ForestMassPool>().patch.entity.is_some(),
            "Region 档应有树簇"
        );
    }

    /// **整图可见门控**：一屏能看完整张图时，树簇是「贴纸」——不画
    /// （夹具把图幅缩到 800，Edit 档视口已 ≥ 图幅 60%）。
    #[test]
    fn clusters_hidden_when_the_whole_map_is_visible() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        app.resource_mut::<GridConfig>().map_size = Vec2::splat(800.0);
        schedule.run(&mut app);
        let pool = app.resource::<ForestMassPool>();
        assert!(pool.patch.entity.is_none(), "整图可见时不该贴树簇");
        assert!(pool.fill.entity.is_none(), "整图可见时也不该铺填充");
    }

    /// 默认配方（树簇堆叠成片、不铺底纹）→ 只有树簇层，没有填充层。
    #[test]
    fn mass_default_has_clusters_but_no_fill() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        schedule.run(&mut app);
        let pool = app.resource::<ForestMassPool>();
        assert!(
            pool.patch.mesh.is_some(),
            "默认配方应靠树簇堆叠成片（树簇层必须在）"
        );
        assert!(
            pool.fill.mesh.is_none(),
            "默认配方不铺底纹（`fill_texture: None`）"
        );
    }

    /// 主题显式给底纹 → 森林侧铺出填充块。
    #[test]
    fn mass_fill_covers_forest_only() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        use_fill_theme(&mut app);
        schedule.run(&mut app);
        let mesh = app
            .resource::<ForestMassPool>()
            .fill
            .mesh
            .clone()
            .expect("森林侧应铺出填充 mesh");
        let quads = quad_count(&app, &mesh);
        assert!(quads > 0, "填充块数应 > 0，got {quads}");
        // 视口内步距 22px × scale 0.5 = 11 wu；视口 640×400 wu ⇒ 约 58×36 ≈ 2000 块，
        // 森林只占左半 + 密度阈值 ⇒ 数量应明显小于全屏上限。
        assert!(quads < MAX_FILL_QUADS, "不该触顶，got {quads}");
    }

    /// 主题没给团块 → 整层不画（形态由主题决定，渲染层不自作主张）。
    #[test]
    fn mass_layer_is_inert_without_the_spec() {
        let (mut app, mut schedule) = mass_app(BIOME_FOREST);
        {
            let mut view = app.resource_mut::<ViewState>();
            view.active_theme = theme_with_mass(false);
        }
        schedule.run(&mut app);
        let pool = app.resource::<ForestMassPool>();
        assert!(pool.fill.entity.is_none(), "无规格时填充层应退场");
        assert!(pool.patch.entity.is_none(), "无规格时边缘层应退场");
    }

    /// 边缘团块只出现在密度边界（数量远少于填充块）。
    #[test]
    fn mass_patches_only_sit_on_the_boundary() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        use_fill_theme(&mut app);
        schedule.run(&mut app);
        let pool = app.resource::<ForestMassPool>();
        let fill = pool.fill.mesh.clone().expect("填充 mesh");
        let patch = pool.patch.mesh.clone().expect("边缘 mesh");
        let f = quad_count(&app, &fill);
        let p = quad_count(&app, &patch);
        assert!(p > 0, "边界上应盖团块，got {p}");
        assert!(p < f, "边缘块数应远少于填充块数（got patch={p} fill={f}）");
    }

    /// 二帧稳定：同一视口不新增资产（原地改写纪律）。
    #[test]
    fn mass_layer_does_not_accumulate_assets_across_frames() {
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        schedule.run(&mut app);
        let first = app.resource::<Assets<Mesh>>().iter().count();
        schedule.run(&mut app);
        let second = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(
            second, first,
            "第二帧不应新增 mesh（got {first} → {second}）"
        );
    }

    /// 平移相机后，仍在视口内的填充块世界坐标不变（世界锚定采样格）。
    #[test]
    fn mass_fill_positions_stay_pinned_when_the_camera_pans() {
        use bevy::render::mesh::VertexAttributeValues;
        let (mut app, mut schedule) = mass_app(BIOME_DESERT);
        use_fill_theme(&mut app);
        schedule.run(&mut app);
        let before: Vec<[f32; 3]> = {
            let mesh = app.resource::<ForestMassPool>().fill.mesh.clone().unwrap();
            let m = app.resource::<Assets<Mesh>>();
            match m.get(&mesh).unwrap().attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
                _ => panic!("位置属性应为 Float32x3"),
            }
        };
        let (center, half) = {
            let v = app.resource::<ViewState>();
            (v.camera_center, v.window_size * v.scale * 0.5)
        };
        let pan = Vec2::new(23.0, 17.0);
        let safe_min = center - half + Vec2::splat(60.0);
        let safe_max = center + half - pan - Vec2::splat(60.0);
        app.resource_mut::<ViewState>().camera_center = center + pan;
        schedule.run(&mut app);
        let after: Vec<[f32; 3]> = {
            let mesh = app.resource::<ForestMassPool>().fill.mesh.clone().unwrap();
            let m = app.resource::<Assets<Mesh>>();
            match m.get(&mesh).unwrap().attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
                _ => panic!("位置属性应为 Float32x3"),
            }
        };
        let pinned = before.iter().filter(|p| {
            p[0] >= safe_min.x && p[0] <= safe_max.x && p[1] >= safe_min.y && p[1] <= safe_max.y
        });
        let mut compared = 0;
        for p in pinned {
            assert!(after.contains(p), "平移后填充块应先世界坐标不变：{p:?}");
            compared += 1;
        }
        assert!(compared > 0, "交集区内应至少有一个填充块可比对");
    }
}
