//! 地形森林的**林下植被层**（§七 第三尺度，2026-10-22 追记三）。
//!
//! ## 为什么需要这一层
//!
//! 委托设计的素材表里，行 5 是「灌木草丛」（8 变体）——贴地的矮植被。
//! 单株树与树簇解决的是「树」，但近景看下来林地**地面**是空的：
//! 树与树之间只有平涂的草色。参考图的地面有灌木、蕨、碎石，才有「走进林子」
//! 的厚度。
//!
//! ## 尺寸与档位
//!
//! **只在 Edit 档出现**（`view.scale < LOD_REGION_SCALE`）——它是最小尺度的
//! 一层：Region 档一屏看几座山，几像素高的灌木只会变成噪点；Edit 档一屏看
//! 几片林，它才读得出来。这与单株 / 树簇「Nation 不画」是同一条尺度纪律。
//!
//! ## 摆放
//!
//! - 位置：视口内**屏幕恒定步距**的世界锚定格（同单株/树簇，平移不滑动）；
//! - 资格：**森林群系** + 密度 ≥ `min_density`；
//! - 疏密：`keep_prob` 为基准，**边界处 ×`edge_boost`**（林缘的灌丛比林心密，
//!   因为林心地面上有树冠遮荫）；
//! - 变体 / 大小 / 旋转：逐块哈希（确定性）。
//!
//! ## 边界（不做什么）
//!
//! - **不落盘**（P7）、**不写世界**；
//! - **不做碰撞 / 交互**：纯视觉层；
//! - 与「覆盖物（`feature_overlays`）」的区别：那是**作者声明的地界**驱动的，
//!   本层是**地形密度**驱动的。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::forest_density::ForestDensityCache;
use crate::forest_mass::LayerPool;
use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{GridConfig, ViewState, WorldProjection};
use crate::terrain_decor::{decor_band, hash01, DecorBand};
use crate::terrain_view::terrain_resolve::active_terrain;
use wv_terrain::biome::is_forest_family;

/// 林下植被 Z：树簇（0.0775）之上、单株树（0.078）之下。
///
/// **为什么在树簇之上**：灌木长在林缘、会挡住团块的底边，压上去才有「厚度」；
/// **为什么在单株之下**：树是最高的自然物，永远压住地面层。
const UNDERSTORY_Z: f32 = 0.0776;
/// 视口外扩余量（屏幕像素）。
const VIEWPORT_MARGIN_PX: f32 = 32.0;
/// 单帧上限（兜底）：Edit 档整屏最多几千丛。
const MAX_CLUMPS: usize = 4_000;

/// 林下植被层的运行时状态（一层）。
#[derive(Resource, Default)]
pub struct ForestUnderstoryPool {
    layer: LayerPool,
}

impl ForestUnderstoryPool {
    /// 当前 mesh 句柄（无则 `None`）——诊断 / 单测用。
    pub fn mesh(&self) -> Option<&Handle<Mesh>> {
        self.layer.mesh()
    }
}

/// 林下植被层主 system。
#[allow(clippy::too_many_arguments)]
pub fn sync_forest_understory(
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
    mut pool: ResMut<ForestUnderstoryPool>,
    mut density_cache: ResMut<ForestDensityCache>,
) {
    let Some(spec) = view.active_theme.decor.forest_understory else {
        pool.layer.clear(&mut commands);
        return;
    };
    if !recipe.enabled(PassId::TerrainDecor) || !layers.terrain.visible {
        pool.layer.clear(&mut commands);
        return;
    }
    // **档位门控**：只在 Edit 档出现（最小尺度的一层，见模块文档）。
    if decor_band(view.scale) != Some(DecorBand::Edit) {
        pool.layer.clear(&mut commands);
        return;
    }
    let Some((_, data)) = active_terrain(&projection.world, drill.current()) else {
        pool.layer.clear(&mut commands);
        return;
    };
    if grid.cell_size <= 0.0 || data.width == 0 || data.height == 0 {
        pool.layer.clear(&mut commands);
        return;
    }
    density_cache.sync(data);

    let step_wu = spec.spacing_px * view.scale;
    let size_wu = spec.size_px * view.scale;
    if step_wu <= 0.0 || size_wu <= 0.0 {
        pool.layer.clear(&mut commands);
        return;
    }

    let half = view.window_size * view.scale * 0.5 + Vec2::splat(VIEWPORT_MARGIN_PX * view.scale);
    let rect_min = view.camera_center - half;
    let rect_max = view.camera_center + half;

    let origin = grid.raster_origin;
    let cell = grid.cell_size;
    let probe_cells = (step_wu / cell).round().max(1.0) as i64;
    let variants = spec.variants.max(1);
    let cell_uv = 1.0 / variants as f32;
    let aspect = spec.cell_aspect.max(0.05);

    // 世界锚定采样格（平移不滑动）。
    let gx0 = (rect_min.x / step_wu).floor() as i64;
    let gy0 = (rect_min.y / step_wu).floor() as i64;
    let gx1 = (rect_max.x / step_wu).ceil() as i64;
    let gy1 = (rect_max.y / step_wu).ceil() as i64;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for gy_lat in gy0..=gy1 {
        for gx_lat in gx0..=gx1 {
            if positions.len() / 4 >= MAX_CLUMPS {
                break;
            }
            // 抖动（±0.35 步距）：规则网格会读成「花点阵」。
            let jx = (hash01(gx_lat, gy_lat, 0xA1) - 0.5) * step_wu * 0.7;
            let jy = (hash01(gx_lat, gy_lat, 0xA2) - 0.5) * step_wu * 0.7;
            let wx = gx_lat as f32 * step_wu + jx;
            let wy = gy_lat as f32 * step_wu + jy;
            let cell_x = ((wx - origin.x) / cell).floor();
            let cell_y = ((wy - origin.y) / cell).floor();
            if cell_x < 0.0 || cell_y < 0.0 {
                continue;
            }
            let (cx, cy) = (cell_x as u32, cell_y as u32);
            if cx >= data.width || cy >= data.height {
                continue;
            }
            let Some(cell_data) = data.cells.get((cy * data.width + cx) as usize) else {
                continue;
            };
            // 只在林地上长：非森林群系（水面 / 荒漠 / 裸岩）不长灌丛。
            if !is_forest_family(cell_data.biome) {
                continue;
            }
            let d = density_cache.density_at(data, cx, cy);
            if d < spec.min_density {
                continue;
            }
            // 林缘更密：四邻有跌破阈值（或不是林地）的 → 边界，
            // 保留率乘 `edge_boost`（林心地面上有树冠遮荫，灌丛更稀）。
            let mut boundary = false;
            for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let nx = cx as i64 + dx * probe_cells;
                let ny = cy as i64 + dy * probe_cells;
                if nx < 0 || ny < 0 {
                    boundary = true;
                    break;
                }
                let Some(neighbor) = data
                    .cells
                    .get((ny as usize) * data.width as usize + nx as usize)
                else {
                    boundary = true;
                    break;
                };
                if !is_forest_family(neighbor.biome)
                    || density_cache.density_at(data, nx as u32, ny as u32) < spec.min_density
                {
                    boundary = true;
                    break;
                }
            }
            let keep = if boundary {
                (spec.keep_prob * spec.edge_boost).clamp(0.0, 1.0)
            } else {
                spec.keep_prob
            };
            if hash01(gx_lat, gy_lat, 0xA3) > keep {
                continue;
            }

            let variant = ((hash01(gx_lat, gy_lat, 0xA4) * variants as f32) as u32) % variants;
            let u0 = variant as f32 * cell_uv;
            let u1 = u0 + cell_uv;
            let hh = size_wu * 0.5 * (0.82 + 0.36 * hash01(gx_lat, gy_lat, 0xA5));
            let hw = hh * aspect;
            let base = positions.len() as u32;
            // 底对齐：灌丛的根部落在落点上。
            positions.push([wx - hw, wy, 0.0]);
            positions.push([wx + hw, wy, 0.0]);
            positions.push([wx + hw, wy + hh * 2.0, 0.0]);
            positions.push([wx - hw, wy + hh * 2.0, 0.0]);
            uvs.push([u0, 1.0]);
            uvs.push([u1, 1.0]);
            uvs.push([u1, 0.0]);
            uvs.push([u0, 0.0]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    pool.layer
        .ensure_texture(&mut images, &mut commands, spec.texture, "林下植被");
    pool.layer.sync_geometry(
        &mut commands,
        &mut meshes,
        &mut materials,
        positions,
        uvs,
        indices,
        Color::WHITE.with_alpha(spec.alpha),
        UNDERSTORY_Z,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::PassPlan;
    use crate::state::DrillState;
    use bevy::ecs::schedule::Schedule;
    use wv_core::terrain::{TerrainCellData, TerrainData};
    use wv_terrain::biome::{BIOME_DESERT, BIOME_FOREST};

    /// 主题夹具：林下植被给 / 不给。
    fn theme_with_understory(with: bool) -> &'static crate::theme::theme_pack::ThemePack {
        static WITH: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        static WITHOUT: std::sync::OnceLock<crate::theme::theme_pack::ThemePack> =
            std::sync::OnceLock::new();
        let slot = if with { &WITH } else { &WITHOUT };
        slot.get_or_init(|| crate::theme::theme_pack::ThemePack {
            decor: crate::theme::decor::DecorRecipe {
                forest_understory: if with {
                    crate::theme::decor::WATERCOLOR.forest_understory
                } else {
                    None
                },
                ..crate::theme::decor::WATERCOLOR
            },
            ..*crate::theme::theme_pack::builtin::for_palette(Default::default())
        })
    }

    /// 装配：左半森林 / 右半荒漠的 64² 世界（scale 由调用方给）。
    fn app_with(scale: f32, with_spec: bool) -> (World, Schedule) {
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
                    biome: if x < n / 2 {
                        BIOME_FOREST
                    } else {
                        BIOME_DESERT
                    },
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
            scale,
            camera_center: Vec2::splat(n as f32 * 4.0 / 2.0),
            window_size: Vec2::new(1280.0, 800.0),
            active_theme: theme_with_understory(with_spec),
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
        app.init_resource::<ForestUnderstoryPool>();
        app.init_resource::<Assets<Mesh>>();
        app.init_resource::<Assets<ColorMaterial>>();
        app.init_resource::<Assets<Image>>();
        let mut schedule = Schedule::default();
        schedule.add_systems(sync_forest_understory);
        (app, schedule)
    }

    fn quad_count(app: &World) -> usize {
        let Some(mesh) = app.resource::<ForestUnderstoryPool>().mesh().cloned() else {
            return 0;
        };
        app.resource::<Assets<Mesh>>()
            .get(&mesh)
            .and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION).map(|a| a.len()))
            .unwrap_or(0)
            / 4
    }

    /// Edit 档 + 有规格 → 林地里长出灌丛。
    #[test]
    fn understory_grows_in_forest_at_edit_band() {
        let (mut app, mut schedule) = app_with(0.5, true);
        schedule.run(&mut app);
        let n = quad_count(&app);
        assert!(n > 0, "Edit 档林地上应有灌丛，got {n}");
    }

    /// **只在 Edit 档**：Region / Nation 都不画（最小尺度的一层）。
    #[test]
    fn understory_is_edit_band_only() {
        for scale in [1.5_f32, 5.0] {
            let (mut app, mut schedule) = app_with(scale, true);
            schedule.run(&mut app);
            assert_eq!(
                quad_count(&app),
                0,
                "scale={scale} 不该有林下植被（只在 Edit 档）"
            );
        }
    }

    /// 主题没给规格 → 整层不画。
    #[test]
    fn understory_is_inert_without_the_spec() {
        let (mut app, mut schedule) = app_with(0.5, false);
        schedule.run(&mut app);
        assert_eq!(quad_count(&app), 0, "无规格时不该画");
    }

    /// 二帧稳定：不新增资产（原地改写纪律）。
    #[test]
    fn understory_does_not_accumulate_assets_across_frames() {
        let (mut app, mut schedule) = app_with(0.5, true);
        schedule.run(&mut app);
        let first = app.resource::<Assets<Mesh>>().iter().count();
        schedule.run(&mut app);
        let second = app.resource::<Assets<Mesh>>().iter().count();
        assert_eq!(second, first, "第二帧不应新增 mesh");
    }

    /// 确定性：同视口两帧的块数一致（哈希驱动，无随机）。
    #[test]
    fn understory_layout_is_deterministic() {
        let (mut app1, mut s1) = app_with(0.5, true);
        s1.run(&mut app1);
        let a = quad_count(&app1);
        let (mut app2, mut s2) = app_with(0.5, true);
        s2.run(&mut app2);
        let b = quad_count(&app2);
        assert_eq!(a, b, "同输入应得同布局（{a} vs {b}）");
    }
}
