//! CellMap 桥接层（ADR-0054 D5 的落地形态）：**读取 wv-render 呈现态的系统**
//! 与 **`ThemePack → CellPack` 换算**都收在此处。
//!
//! ## 为什么系统在 wv-render
//! 这些系统的入参是 `ViewState` / `WorldProjection` / `DrillState` / `BrushState`
//! / `LayerFrame` / `RenderRecipe`——全是 wv-render 的呈现态类型；而
//! `wv-world-cell-map` 禁依赖 wv-render（红线），且 `&World` 无法作为平面
//! 数据跨资源传递（8MB~1.5GB，克隆不可接受）。故桥接系统留此，重活
//! （合成 / 网格 / mip / 派生几何）全部在新 crate 的纯函数与资源上。
//!
//! ## 换算纪律
//! `conv_*` 系列是 wv-theme 类型 → 新 crate 平面镜像的**唯一**通道；
//! `shade_mode_of` 是 `ViewMode → ShadeMode` 的唯一映射点（ADR-0054 D4）。

#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use bevy::math::Affine2;
use bevy::prelude::*;

use wv_world_cell_map::palette::{
    CellPack, CellPalette, HatchSpec, RippleSpec, SeaRecipe, TreatmentFlags,
};
use wv_world_cell_map::shade::ShadeMode;
use wv_world_cell_map::state::{
    CellSelection, DashPhase, EditCellState, GridConfig, PointSelectState,
};
use wv_world_cell_map::terrain_view::rebuild::{PendingTerrainSynth, SynthCtx};
// 桥接系统消费的引擎件（含 crate 内部件）——原文件级 use 随函数迁居此处（ADR-0054 D5）。
use wv_terrain::isoline::{assemble_chains, chaikin, height_segments, kind_segments, simplify_dp};
use wv_world_cell_map::cell_flash::*;
use wv_world_cell_map::cell_selection::*;
use wv_world_cell_map::region_overlay::*;
use wv_world_cell_map::terrain_chunks::*;
use wv_world_cell_map::terrain_view::atmosphere::*;
use wv_world_cell_map::terrain_view::cells::*;
use wv_world_cell_map::terrain_view::grid_lines::*;
use wv_world_cell_map::terrain_view::isoline_overlay::*;
use wv_world_cell_map::terrain_view::rebuild::*;
use wv_world_cell_map::terrain_view::setup::*;
use wv_world_cell_map::terrain_view::terrain_resolve::*;
use wv_world_cell_map::terrain_view::terrain_texture::*;
use wv_world_cell_map::terrain_view::tile_atlas::*;

use crate::layer_frame::LayerFrame;
use crate::recipe::{PassId, RenderRecipe};
use crate::state::{BrushState, DrillState, ViewState, WorldProjection};

// ── ADR-0053：GPU 场渲染（`sync_terrain_field`，Phase 1 接线 2026-10-24）──
//
// **逐条列出而不是 glob**：`field` 与 `terrain_chunks` 各自导出一个 `TERRAIN_Z`
// （`field::paper::TERRAIN_Z` = 纸面层 Z，与本层无关），glob 会把名字撞在一起。
use wv_world_cell_map::terrain_view::field::{
    apply_hypsometric, apply_strip_mask, build_material, default_params, patch_material_images,
    set_coast_encoding, strip_veil_on, warp_amp, BlockFingerprint, CellRect, CoastEncoding,
    FieldPack, FieldPalette, TerrainFieldMaterial, ViewBranch, H_MAX, H_MIN,
};
use wv_world_cell_map::terrain_view::hypsometric;
// ADR-0077：晕渲法线的米制换算唯一入口（格距 + 比例尺 → 米/格）。
use wv_world_cell_map::shading::meters_per_cell_of;

// ── 子模块（P1-1 拆分，2026-10-29）：外部路径经下方 pub use 零改动 ──
mod atmosphere;
mod chunks;
mod edit_overlays;
mod field;
mod grid_config;
mod grid_lines;
mod isoline;
mod overlays;
mod terrain_texture;
mod theme_conv;

pub use atmosphere::*;
pub use chunks::*;
pub use edit_overlays::*;
pub use field::*;
pub use grid_config::*;
pub use grid_lines::*;
pub use isoline::*;
pub use overlays::*;
pub use terrain_texture::*;
pub use theme_conv::*;

// ── 测试（原样保留；`use super::*` 经重导出仍可见全部符号）──
#[cfg(test)]
mod visibility_tests {
    use super::*;
    use crate::state::ViewMode;
    // 桥接测试：region_layer_visible 的判定矩阵随函数迁居 bridge（ADR-0054 D5）。
    use wv_core::model::region::RegionKind;

    #[test]
    fn region_layer_in_cellmap_shows_nature_only_when_overlay_on() {
        let kinds = [RegionKind::Nature, RegionKind::Area];
        // 开关开：Nature ✓、Area ✗
        assert!(region_layer_visible(
            ViewMode::Topographic,
            true,
            kinds[0],
            false,
            true
        ));
        assert!(!region_layer_visible(
            ViewMode::Topographic,
            true,
            kinds[1],
            false,
            true
        ));
        // 开关关：全隐（recipe 格本就 false）
        assert!(!region_layer_visible(
            ViewMode::Topographic,
            false,
            kinds[0],
            false,
            true
        ));
    }

    #[test]
    fn region_layer_outside_cellmap_follows_recipe() {
        let nature = RegionKind::Nature;
        assert!(region_layer_visible(
            ViewMode::VectorFlat,
            false,
            nature,
            true,
            true
        ));
        assert!(!region_layer_visible(
            ViewMode::VectorFlat,
            false,
            nature,
            false,
            true
        ));
        // GoogleMap：recipe 格=false ⇒ 无论开关都藏（开关只在 CellMap 有意义）
        assert!(!region_layer_visible(
            ViewMode::GoogleMap,
            true,
            nature,
            false,
            true
        ));
    }

    #[test]
    fn region_layer_respects_layer_visibility_everywhere() {
        assert!(!region_layer_visible(
            ViewMode::VectorFlat,
            false,
            RegionKind::Nature,
            true,
            false
        ));
        assert!(!region_layer_visible(
            ViewMode::Topographic,
            true,
            RegionKind::Nature,
            false,
            false
        ));
    }
}

// ══════ 自新 crate 迁来的呈现态测试（它们测桥接系统） ══════

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use wv_world_cell_map::palette::CellPack;

    fn map_node(
        id: &str,
        parent: Option<&str>,
        extent: Option<wv_core::map::Extent>,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked(id),
            name: id.to_owned(),
            level: wv_core::map::MapLevel::City,
            parent: parent.map(wv_core::Id::new_unchecked),
            children: Vec::new(),
            icons: Vec::new(),
            layers: Vec::new(),
            extent,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        }
    }

    fn small_terrain(biome: u8) -> TerrainData {
        TerrainData {
            width: 4,
            height: 4,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 7,
                    surface: 1,
                    biome,
                };
                16
            ],
        }
    }

    fn two_level_world() -> wv_core::World {
        let mut world = wv_core::World::default();
        world.maps.push(map_node(
            "map_world",
            None,
            Some(wv_core::map::Extent::sized(4096.0, 4096.0)),
        ));
        world.maps.push(map_node(
            "map_city",
            Some("map_world"),
            Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
        ));
        world
    }
    use wv_core::terrain::TerrainData;
    use wv_world_cell_map::terrain_view::{
        cells::image_from_terrain,
        terrain_resolve::{active_terrain_or_placeholder, texture_terrain},
        terrain_texture::TerrainTexture,
    };

    #[test]
    fn placeholder_terrain_lands_on_the_root_map_when_nothing_has_a_raster() {
        let mut world = two_level_world();
        let city = wv_core::Id::new_unchecked("map_city");
        let (map, data) = active_terrain_or_placeholder(&mut world, Some(&city), 32);
        assert_eq!(map.map(|m| m.as_str().to_owned()), Some("map_world".into()));
        assert_eq!(data.width, 32, "占位地形按调用方给的格数生成");
        assert!(
            world
                .terrain_of(&wv_core::Id::new_unchecked("map_world"))
                .is_some(),
            "占位地形应写进根图，而不是只存在于纹理里"
        );
    }

    #[test]
    fn unknown_current_map_does_not_overwrite_existing_terrain() {
        let mut world = two_level_world();
        let root = wv_core::Id::new_unchecked("map_world");
        world.set_terrain(root.clone(), small_terrain(3));
        let (map, data) = active_terrain_or_placeholder(&mut world, None, 32);
        assert_eq!(map.map(|m| m.as_str().to_owned()), Some("map_world".into()));
        assert_eq!(data.cells[0].biome, 3, "应取回已有真实地形，而不是占位");
        assert_eq!(
            world.terrain_of(&root).map(|t| t.width),
            Some(4),
            "世界里的地形不能被占位覆盖"
        );
    }

    #[test]
    fn texture_terrain_follows_the_map_the_texture_holds() {
        let mut world = two_level_world();
        world.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        world.set_terrain(wv_core::Id::new_unchecked("map_city"), small_terrain(5));
        let city = wv_core::Id::new_unchecked("map_city");
        let mut tex = TerrainTexture {
            handle: Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            field_dirty: None,
            last_mode: None,
            last_soften: false,
            last_theme_id: "fixture",
            last_hypsometric: 0.0,
            last_map: Some(wv_core::Id::new_unchecked("map_world")),
        };
        let data = texture_terrain(&tex, &world, Some(&city)).expect("应取世界图地形");
        assert_eq!(
            data.cells[0].biome, 3,
            "纹理装了世界图，就不能拿城市格去 patch"
        );
        // 纹理换到城市后，同一调用应跟着换成城市的数据。
        tex.last_map = Some(city.clone());
        let data = texture_terrain(&tex, &world, Some(&city)).expect("应取城市地形");
        assert_eq!(data.cells[0].biome, 5);
    }

    #[test]
    fn texture_terrain_falls_back_to_the_current_map_when_last_map_is_unknown() {
        let mut world = two_level_world();
        world.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        let city = wv_core::Id::new_unchecked("map_city");
        let tex = TerrainTexture {
            handle: Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            field_dirty: None,
            last_mode: None,
            last_soften: false,
            last_theme_id: "fixture",
            last_hypsometric: 0.0,
            last_map: None,
        };
        let data = texture_terrain(&tex, &world, Some(&city)).expect("应退回继承口径");
        assert_eq!(data.cells[0].biome, 3);
    }

    #[test]
    fn terrain_texture_records_which_map_it_holds() {
        let data = small_terrain(3);
        let image = image_from_terrain(
            &data,
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let mut images = Assets::<Image>::default();
        let held = wv_core::Id::new_unchecked("map_city");
        let tex = new_terrain_texture(
            &mut images,
            image,
            &data,
            Some(held.clone()),
            ShadeMode::GoogleMap,
            false,
            Default::default(),
            0.0,
        );
        assert_eq!(tex.last_map.as_ref(), Some(&held));
    }

    #[test]
    fn grid_map_origin_follows_the_drilled_map_when_the_raster_is_inherited() {
        let mut core = two_level_world();
        // 助手把两张图都造 `City` 级；`ensure_root` 要认根图，标回 `World`。
        core.maps[0].level = wv_core::map::MapLevel::World;
        // 只有世界图有地形：城市继承它 → 纹理两帧都不变。
        core.set_terrain(wv_core::Id::new_unchecked("map_world"), small_terrain(3));
        {
            let root = &mut core.maps[0];
            root.children.push(wv_core::Id::new_unchecked("map_city"));
            root.icons.push(wv_core::map::IconSpec {
                id: wv_core::Id::new_unchecked("icon_city"),
                anchor: Some([0.5, 0.5]),
                shape: Default::default(),
                size: None,
                color: None,
                drill_into: Some(wv_core::Id::new_unchecked("map_city")),
            });
        }
        let city = wv_core::Id::new_unchecked("map_city");

        let mut drill = DrillState::default();
        drill.ensure_root(&core);
        assert_eq!(
            drill.current().map(|i| i.as_str().to_owned()),
            Some("map_world".to_owned()),
            "起始应落在根图"
        );

        let mut app = World::default();
        app.insert_resource(TerrainTexture {
            handle: Handle::default(),
            size: 4,
            height: 4,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            field_dirty: None,
            last_mode: None,
            last_soften: false,
            last_theme_id: "fixture",
            last_hypsometric: 0.0,
            // 纹理装的是世界图底图；钻入城市后**不会**重建（继承同一个来源）。
            last_map: Some(wv_core::Id::new_unchecked("map_world")),
        });
        app.insert_resource(WorldProjection {
            world: core.clone(),
            ..Default::default()
        });
        app.insert_resource(drill);
        app.insert_resource(GridConfig::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(sync_grid_config);
        schedule.run(&mut app);
        assert_eq!(
            app.resource::<GridConfig>().map_origin,
            Vec2::ZERO,
            "世界图上图帧原点与栅格原点重合于 (0,0)"
        );

        // 第二帧：只换当前图（钻入城市），纹理一个字都不动。
        {
            let mut d = app.resource_mut::<DrillState>();
            assert!(d.enter(&core, &city), "城市应是根图的直接子图");
        }
        schedule.run(&mut app);

        let grid = app.resource::<GridConfig>();
        assert_eq!(grid.raster_origin, Vec2::ZERO, "继承的底图仍是世界图原点");
        assert_eq!(
            grid.map_origin,
            Vec2::new(1536.0, 1536.0),
            "城市图帧原点应由父图图标派生：2048 − 512 = 1536；实得 {}",
            grid.map_origin
        );
        assert_eq!(
            grid.map_size,
            Vec2::splat(1024.0),
            "幅面应取当前图（城市）的 extent，而不是世界图的 4096"
        );
    }

    #[test]
    fn fadeout_spawn_clones_pixels_and_covers_the_old_raster() {
        let data = small_terrain(3);
        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            &data,
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let tex = new_terrain_texture(
            &mut images,
            image,
            &data,
            None,
            ShadeMode::GoogleMap,
            false,
            Default::default(),
            0.0,
        );
        let grid = GridConfig {
            cell_size: 2.0,
            raster_origin: Vec2::new(100.0, 200.0),
            ..GridConfig::default()
        };

        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            spawn_basemap_fadeout(&mut commands, &mut images, &tex, &grid);
        }
        queue.apply(&mut world);

        let mut q = world.query::<(&BasemapFadeOut, &Sprite, &Transform)>();
        let (fade, sprite, tf) = q.single(&world).expect("应生成一个淡出精灵");
        assert_eq!(fade.alpha, 1.0, "初始不透明");
        assert_eq!(
            sprite.custom_size,
            Some(Vec2::splat(8.0)),
            "幅面 = 4 格 × cell_size 2.0"
        );
        assert_eq!(
            tf.translation.truncate(),
            Vec2::new(104.0, 204.0),
            "中心 = 栅格原点 + 半幅"
        );
        assert_eq!(tf.translation.z, FADEOUT_Z);
        // 克隆：句柄不同、内容一致，且旧句柄的像素未被动过。
        assert_ne!(sprite.image, tex.handle, "淡出必须用克隆出的新句柄");
        let old = images.get(&tex.handle).expect("旧资产还在");
        let new = images.get(&sprite.image).expect("克隆资产已入库");
        assert_eq!(old.data, new.data, "克隆像素应与旧底图逐字节一致");
    }

    #[test]
    fn map_change_spawns_fadeout_but_mode_change_does_not() {
        let mut core = two_level_world();
        // `ensure_root` 只认 `level == World`（fixture 默认 City）：把根图升回来。
        core.maps[0].level = wv_core::map::MapLevel::World;
        let world_id = wv_core::Id::new_unchecked("map_world");
        let city_id = wv_core::Id::new_unchecked("map_city");
        core.set_terrain(world_id.clone(), small_terrain(3));
        core.set_terrain(city_id.clone(), small_terrain(5));

        let mut drill = DrillState::default();
        drill.ensure_root(&core);
        let (node, data) = active_terrain(&core, drill.current()).expect("世界图有地形");
        assert_eq!(node.id, world_id);
        let mut images = Assets::<Image>::default();
        let image = image_from_terrain(
            data,
            ShadeMode::GoogleMap,
            None,
            false,
            &CellPack::fixture(),
            0.0,
        );
        let tex = new_terrain_texture(
            &mut images,
            image,
            data,
            Some(world_id),
            ShadeMode::GoogleMap,
            false,
            Default::default(),
            0.0,
        );

        let mut world = World::new();
        world.insert_resource(ViewState::default());
        world.insert_resource(WorldProjection {
            world: core.clone(),
            ..Default::default()
        });
        world.insert_resource(drill);
        world.insert_resource(GridConfig::default());
        world.insert_resource(tex);
        world.insert_resource(images);
        world.init_resource::<crate::terrain_view::rebuild::PendingTerrainSynth>();

        let count_fades = |world: &mut World| -> usize {
            let mut q = world.query::<&BasemapFadeOut>();
            q.iter(world).count()
        };
        let mut schedule = Schedule::default();
        schedule.add_systems(rebuild_texture_on_mode_change);

        // 换图：钻入自带地形的城市 → 应生成恰好一个淡出精灵。
        // fixture 没接 children（`enter` 只认直接子图），这里补上引用。
        core.maps[0].children.push(city_id.clone());
        {
            let mut d = world.resource_mut::<DrillState>();
            assert!(d.enter(&core, &city_id), "城市应是根图的直接子图");
        }
        schedule.run(&mut world);
        assert_eq!(count_fades(&mut world), 1, "换图应生成一个淡出精灵");

        // 换模式：即时重建是既有行为，不再叠加淡出。
        world.resource_mut::<ViewState>().mode = crate::state::ViewMode::GoogleMap;
        schedule.run(&mut world);
        assert_eq!(
            count_fades(&mut world),
            1,
            "换模式（换眼镜）不应生成第二个淡出精灵"
        );
    }

    /// 桥接的米/格填充（ADR-0077 D2）：比例尺精确换算 + 未声明回落。
    ///
    /// `sync_terrain_field` 把「来源图比例尺 + 格距」经
    /// [`bridge_meters_per_cell`] 算出的米/格写进 uniform `light_dir.w`
    /// （WGSL 侧 `mpc` 槽）。本测试锁两个口径：
    /// 声明比例尺走精确换算；未声明回落「1 wu = 1 m」（README §12-53 待决，
    /// 口径若静默更改，此测试应当先红）。
    #[test]
    fn bridge_fills_meters_per_cell_from_map_scale() {
        // `Some(8.0)` = 「1 km = 8 wu」→ 1 wu = 125 m → 100 wu 格 = 12500 m/格。
        let scaled = bridge_meters_per_cell(100.0, Some(8.0));
        assert!((scaled - 12500.0).abs() < 1e-6, "got {scaled}");
        // 未声明比例尺 → 回落：100 wu 格 = 100 m/格（「1 wu = 1 m」假设）。
        let fallback = bridge_meters_per_cell(100.0, None);
        assert!((fallback - 100.0).abs() < 1e-6, "got {fallback}");
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bevy::render::mesh::MeshVertexAttribute;

        // ── 政治图集页素底分支选择（ADR-0079 T5）──────────────────────
        //
        // 「自然图关」= 素底政治页：不退场（地图全空则势力填色无处可压），
        // 改走 `ViewBranch::Flat` 分支——平面海陆两色 + 海岸线。
        //
        // 判据顺序：CellMap（Topographic）恒走 BiomeRaw（那是方格数据视图，
        // 自然图开关不介入）；其余模式按「地形层可见否」选 Themed / Flat。

        #[test]
        fn branch_is_flat_when_natural_map_hidden_outside_topographic() {
            assert_eq!(
                field_branch_of(crate::state::ViewMode::VectorFlat, false),
                ViewBranch::Flat,
                "自然图关 → 素底政治页分支"
            );
            assert_eq!(
                field_branch_of(crate::state::ViewMode::GoogleMap, false),
                ViewBranch::Flat,
                "任何非 Topographic 模式都走素底"
            );
        }

        #[test]
        fn branch_is_themed_when_natural_map_visible() {
            assert_eq!(
                field_branch_of(crate::state::ViewMode::VectorFlat, true),
                ViewBranch::Themed,
                "自然图开 → 主题色板"
            );
        }

        #[test]
        fn topographic_mode_always_uses_biome_raw() {
            // CellMap 是方格数据视图——自然图开关不介入（ADR-0051 §5）。
            assert_eq!(
                field_branch_of(crate::state::ViewMode::Topographic, true),
                ViewBranch::BiomeRaw
            );
            assert_eq!(
                field_branch_of(crate::state::ViewMode::Topographic, false),
                ViewBranch::BiomeRaw
            );
        }

        fn view(grid_lines: bool) -> ViewState {
            ViewState {
                grid_lines,
                ..ViewState::default()
            }
        }

        /// 可见性只看 `view.grid_lines` 一个旗标：默认关；置真即显。
        /// （2026-10-16 字段搬出 `BrushState` 后，笔刷不再参与本判定。）
        #[test]
        fn grid_lines_visible_follows_the_view_flag() {
            assert!(!grid_lines_visible(&view(false)), "默认关");
            assert!(grid_lines_visible(&view(true)), "旗标开 → 显");
        }

        /// 地形 10 槽必是 `GOOGLE_CORE_TERRAIN`（专业制图色板）。
        ///
        /// **防回归**：主题侧这份色板自 2026-10-13 定义以来零消费点；渲染侧
        /// （`wv-world-cell-map`）另有一份同名抄录表，`shade_google` 一度读的
        /// 是后者——于是本测试曾经**只验证结构体字段**、对像素无影响（假绿）。
        /// 2026-10-25 修正：`shade_google` 改读 `CellPalette.core`，本函数才
        /// 真正成为「普通地图的地形色」的唯一入口。像素级锚点在
        /// `wv-world-cell-map::cells::tests::google_shade_reads_palette_core…`。
        ///
        /// **ADR-0066 更新**：单风格后「普通地图」与「默认风格」是**同一个**
        /// ——不再存在「主题色板 ≠ 专业色板」的对照，故那条 `assert_ne` 撤掉，
        /// 改为断言**两个模式取到同一份**（单风格的直接后果）。
        #[test]
        fn google_mode_terrain_core_is_the_google_palette() {
            let pack = &wv_default_theme::theme_pack::builtin::DEFAULT;
            let google = conv_pack_with(pack, 0.0, crate::state::ViewMode::GoogleMap);
            let themed = conv_pack_with(pack, 0.0, crate::state::ViewMode::VectorFlat);
            assert_eq!(
                google.palette.core,
                wv_default_theme::colors_google::GOOGLE_CORE_TERRAIN,
                "普通地图的地形基色应是 Google 色板（近白陆地 / 淡水 / 公园绿）"
            );
            assert_eq!(
                google.palette.core, themed.palette.core,
                "单风格（ADR-0066）：两个模式取到同一份风格色板"
            );
            // 处理旗标：海岸晕 / 纸纹是手绘语汇，普通地图要干净平涂。
            assert_eq!(
                google.treatment,
                conv_treatment(&wv_default_theme::theme_pack::TreatmentFlags::EMPTY),
                "普通地图的处理旗标应为 EMPTY（无水彩晕染 / 纸纹）"
            );
            assert_eq!(
                google.treatment, themed.treatment,
                "单风格：处理旗标也是同一份（EMPTY）"
            );
        }

        /// 专业制图的两处「主题泄漏」必须同时收口（2026-10-25）。
        ///
        /// **海配方**与 **treatment 是两套独立开关**：旗标管晕 / 纸纹，海配方
        /// 管水面笔触（水纹颗粒 / 波纹 / 碎浪 / 排线 / 浅滩积墨）。只清前者、
        /// 透传后者，专业地图的水面上仍会带回水彩笔触。**分层设色**同理：
        /// 它是地形分析的表达，普通地图陆地单色，滑条非零也不许染色。
        #[test]
        fn google_mode_flattens_sea_recipe_and_hypsometric() {
            let pack = &wv_default_theme::theme_pack::builtin::DEFAULT;
            let google = conv_pack_with(pack, 0.8, crate::state::ViewMode::GoogleMap);
            assert!(google.sea.ripples.is_empty(), "普通地图不画离岸墨线波纹");
            assert!(google.sea.foam.is_empty(), "普通地图不画近岸碎浪");
            assert!(google.sea.shore_hatch.is_none(), "普通地图不画近岸排线");
            assert_eq!(google.sea.water_grain, 0.0, "普通地图水面无纸纹颗粒");
            assert_eq!(
                google.sea.shore_edge_strength, 0.0,
                "普通地图无浅滩外缘积墨"
            );
            assert_eq!(
                google.hypsometric, 0.0,
                "分层设色是地形分析表达，普通地图强制归零"
            );
            // 深度渐变是数据不是画法：保留。
            assert!(google.sea.depth_scale_m > 0.0, "水深量程应保留");
            // **ADR-0066**：单风格后不存在「艺术主题模式」——手绘海配方整体退役，
            // 两个模式取到同一份（专业制图）配方。分层设色仍按模式分派
            // （它是地形分析的表达，不是风格）。
            let themed = conv_pack_with(pack, 0.8, crate::state::ViewMode::VectorFlat);
            assert!(
                themed.sea.ripples.is_empty(),
                "单风格：手绘海配方已整体退役，两模式取同一份"
            );
            assert_eq!(themed.hypsometric, 0.8, "非专业模式照常吃分层设色强度");
        }

        /// 远观淡出锚点：≥4px 全显、≤2px 消失、中点线性一半、端点外钳制。
        ///
        /// 2026-10-23：本函数语义收窄为「**细档**淡出系数」——`=0` 不再意味着
        /// 整层退场（粗档接手）。整层语义的断言见 `grid_alpha_stays_positive_at_fit`。
        #[test]
        fn grid_line_fade_is_linear_between_two_and_four_pixel_cells() {
            assert_eq!(grid_line_fade(8.0), 1.0, "宽格全显");
            assert_eq!(grid_line_fade(GRID_FINE_FULL_PX), 1.0, "起点全显");
            let mid = grid_line_fade(3.0);
            assert!((mid - 0.5).abs() < 1e-6, "中点应线性一半：got {mid}");
            assert_eq!(grid_line_fade(GRID_FINE_MIN_PX), 0.0, "终点消失");
            assert_eq!(grid_line_fade(1.0), 0.0, "更密仍钳在 0");
        }

        /// ── 网格系统的断言装置 ────────────────────────────────────────────
        ///
        /// 造一个只装 `sync_grid_lines` 的最小 `World`：资源注入按系统的实际
        /// 参数表列全（少一个就是 panic，不是「静默不画」）。`scale` 由调用方给，
        /// 因为本轮的整个论点是「**scale 不该影响任何东西**」。
        fn grid_world(scale: f32, grid_lines: bool, terrain_visible: bool) -> World {
            let mut world = World::new();
            world.insert_resource(Assets::<Mesh>::default());
            world.insert_resource(Assets::<GridLinesMaterial>::default());
            world.insert_resource(ViewState {
                grid_lines,
                scale,
                ..ViewState::default()
            });
            world.insert_resource(GridConfig {
                cell_size: 4.0,
                raster_origin: Vec2::new(100.0, 200.0),
                map_origin: Vec2::ZERO,
                map_size: Vec2::splat(4096.0),
            });
            world.insert_resource(TerrainTexture {
                handle: Handle::default(),
                size: 1024,
                height: 1024,
                res: 4,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "daylight",
                last_hypsometric: 0.0,
                last_map: None,
            });
            let mut layers = LayerFrame::default();
            layers.terrain.visible = terrain_visible;
            world.insert_resource(layers);
            let mut recipe = RenderRecipe::default();
            recipe.passes[PassId::GridLines as usize].enabled = true;
            world.insert_resource(recipe);
            world.init_resource::<GridLinesOverlay>();
            world
        }

        fn run_grid(world: &mut World) {
            let mut schedule = Schedule::default();
            schedule.add_systems(sync_grid_lines);
            schedule.run(world);
        }

        fn grid_entity_count(world: &mut World) -> usize {
            let mut q = world.query::<&Mesh2d>();
            q.iter(world).count()
        }

        /// 验收断言 1：世界级 fit（`scale 5.12`、`cell_size 4.0` → 格距 0.78px）
        /// 时网格**实际可见**——不只是「没 despawn」。
        ///
        /// 三重证据：① 实体存在；② 屏上 alpha > 0（纯函数复算）；③ 粗档倍率
        /// 把定位格顶到 ≥4px（0.78125 × 8 = 6.25px），即「看见的确实是一张网格
        /// 而不是一条线」。
        #[test]
        fn grid_alpha_stays_positive_at_fit() {
            let cell_px = grid_cell_px(4.0, 5.12);
            assert!((cell_px - 0.78125).abs() < 1e-9, "手算 4.0/5.12");
            let alpha = grid_line_alpha_with_opacity(cell_px, 1.0);
            assert!(alpha > 0.0, "fit 下 alpha 必须 > 0，实得 {alpha}");
            assert_eq!(grid_coarse_stride(cell_px), 8, "0.78px 需 8 倍才到 4px");
            let locator = grid_coarse_px(cell_px);
            assert!(locator >= GRID_COARSE_MIN_PX, "定位格 {locator}px 应 ≥ 4px");

            let mut world = grid_world(5.12, true, true);
            run_grid(&mut world);
            assert_eq!(grid_entity_count(&mut world), 1, "fit 下应有 1 个网格实体");
            let overlay = world.resource::<GridLinesOverlay>();
            assert_eq!(overlay.rebuilds, 1, "首帧建一次");
            // 材质里真的烧进了 > 0 的 alpha（不只是 CPU 函数算得好看）
            let mat_handle = overlay.material.clone().expect("材质应存在");
            let materials = world.resource::<Assets<GridLinesMaterial>>();
            let mat = materials.get(&mat_handle).expect("材质资产在库里");
            assert!(
                mat.params.ink.w > 0.0,
                "uniform 基色 alpha 必须 > 0，实得 {}",
                mat.params.ink.w
            );
            assert_eq!(
                Vec2::new(mat.params.geo.z, mat.params.geo.w),
                Vec2::new(100.0, 200.0),
                "栅格原点进 uniform（shader 用它反推格坐标）"
            );
        }

        /// 验收断言 2：连续 12 档缩放（×1.12 单调推 12 次）触发的 mesh 重建
        /// 次数 **= 0**（需求上限 ≤2）。
        ///
        /// 判据是公开计数字段 `GridLinesOverlay::rebuilds`，不看墙钟。
        #[test]
        fn twelve_zoom_notches_trigger_zero_rebuilds() {
            let mut world = grid_world(1.0, true, true);
            run_grid(&mut world);
            assert_eq!(world.resource::<GridLinesOverlay>().rebuilds, 1, "首帧一次");
            let handle_before = world
                .resource::<GridLinesOverlay>()
                .mesh
                .clone()
                .expect("mesh 应存在");
            for i in 0..12 {
                world.resource_mut::<ViewState>().scale *= 1.12;
                run_grid(&mut world);
                assert_eq!(grid_entity_count(&mut world), 1, "第 {i} 档仍应有 1 个实体");
            }
            let overlay = world.resource::<GridLinesOverlay>();
            assert_eq!(
                overlay.rebuilds, 1,
                "12 档缩放**不得**重建 mesh（旧实现每档一次 = 13 次）"
            );
            assert_eq!(
                overlay.mesh.as_ref(),
                Some(&handle_before),
                "mesh handle 应逐帧不变"
            );
            assert_eq!(
                world.resource::<Assets<Mesh>>().len(),
                1,
                "mesh 资产数应恒为 1（无泄漏、无堆积）"
            );
        }

        // ── ADR-0057 A3：选区不变 → 面 / 蚂蚁线两套 mesh 都不重建 ─────────

        /// 只装 `sync_cell_region` 的最小世界（面 + 两相位蚂蚁线）。
        fn cell_region_world(cells: &[(i32, i32)]) -> World {
            let mut world = World::new();
            world.insert_resource(Assets::<Mesh>::default());
            world.insert_resource(Assets::<ColorMaterial>::default());
            let mut recipe = RenderRecipe::default();
            recipe.passes[PassId::CellSelection as usize].enabled = true;
            world.insert_resource(recipe);
            // 图绑定：drill 为空 → 系统不重绑，故这里预先绑好 8×8 图幅。
            let map = wv_core::Id::new_unchecked("world_main");
            let mut sel = CellSelection::default();
            sel.bind_map(&map, 8, 8);
            sel.replace(cells.iter().copied());
            world.insert_resource(sel);
            world.insert_resource(DrillState::default());
            world.insert_resource(TerrainTexture {
                handle: Handle::default(),
                size: 8,
                height: 8,
                res: 1,
                dirty: None,
                chunks_dirty: None,
                field_dirty: None,
                last_mode: None,
                last_soften: false,
                last_theme_id: "daylight",
                last_hypsometric: 0.0,
                last_map: None,
            });
            world.insert_resource(GridConfig {
                cell_size: 10.0,
                raster_origin: Vec2::ZERO,
                map_origin: Vec2::ZERO,
                map_size: Vec2::splat(80.0),
            });
            world.insert_resource(ViewState::default());
            world.insert_resource(Time::<()>::default());
            world.init_resource::<CellRegionEntities>();
            world.init_resource::<CellAntsEntities>();
            world
        }

        fn run_cell_region(world: &mut World) {
            let mut schedule = Schedule::default();
            schedule.add_systems(sync_cell_region);
            schedule.run(world);
        }

        /// **A3**：选区不变（同指纹、同缩放档）→ 面与蚂蚁线的 mesh 句柄逐帧不变；
        /// 选区一变则两套都重建（新句柄）。
        #[test]
        fn unchanged_selection_does_not_rebuild_cell_region_meshes() {
            let mut world = cell_region_world(&[(0, 0), (1, 0)]);
            run_cell_region(&mut world);
            let fill_before = world
                .resource::<CellRegionEntities>()
                .fill_mesh
                .clone()
                .expect("填充 mesh 应已建");
            let ants_before = world
                .resource::<CellAntsEntities>()
                .mesh_a
                .clone()
                .expect("蚂蚁线 mesh 应已建");
            let assets_after_first = world.resource::<Assets<Mesh>>().len();

            // 同选区、同缩放档再跑一帧：不得重建（句柄与资产数都不变）。
            run_cell_region(&mut world);
            assert_eq!(
                world.resource::<CellRegionEntities>().fill_mesh,
                Some(fill_before.clone()),
                "选区不变 → 填充 mesh 不得重建"
            );
            assert_eq!(
                world.resource::<CellAntsEntities>().mesh_a,
                Some(ants_before.clone()),
                "选区不变 → 蚂蚁线 mesh 不得重建"
            );
            assert_eq!(
                world.resource::<Assets<Mesh>>().len(),
                assets_after_first,
                "选区不变不得新增 mesh 资产（句柄复用）"
            );

            // 选区 +1 格 → 指纹变 → 两套都重建（句柄必须换新）。
            world.resource_mut::<CellSelection>().union([(2, 0)]);
            run_cell_region(&mut world);
            assert_ne!(
                world.resource::<CellRegionEntities>().fill_mesh,
                Some(fill_before),
                "选区变化必须重建填充 mesh"
            );
            assert_ne!(
                world.resource::<CellAntsEntities>().mesh_a,
                Some(ants_before),
                "选区变化必须重建蚂蚁线 mesh"
            );
        }

        /// 验收断言 3（负向）：开关关 / 图层隐藏 / recipe 关 → 实体数 0，
        /// 且 mesh 与 material **都**被回收（旧实现只删 mesh，material 每轮泄漏）。
        #[test]
        fn grid_despawns_and_reclaims_both_assets() {
            for (grid_lines, terrain_visible, recipe_on) in [
                (false, true, true),
                (true, false, true),
                (true, true, false),
            ] {
                let mut world = grid_world(1.0, grid_lines, terrain_visible);
                if !recipe_on {
                    let mut recipe = world.resource_mut::<RenderRecipe>();
                    recipe.passes[PassId::GridLines as usize].enabled = false;
                }
                run_grid(&mut world);
                assert_eq!(
                    grid_entity_count(&mut world),
                    0,
                    "负向用例（grid={grid_lines}, terrain={terrain_visible}, recipe={recipe_on}）不得残留实体"
                );
                assert_eq!(world.resource::<Assets<Mesh>>().len(), 0, "mesh 资产应回收");
                assert_eq!(
                    world.resource::<Assets<GridLinesMaterial>>().len(),
                    0,
                    "material 资产应回收"
                );
            }
            // 再验证「建过之后关掉」也回收干净
            let mut world = grid_world(1.0, true, true);
            run_grid(&mut world);
            assert_eq!(grid_entity_count(&mut world), 1);
            world.resource_mut::<ViewState>().grid_lines = false;
            run_grid(&mut world);
            assert_eq!(grid_entity_count(&mut world), 0, "关开关 → 实体 0");
            assert_eq!(world.resource::<Assets<Mesh>>().len(), 0, "mesh 回收");
            assert_eq!(
                world.resource::<Assets<GridLinesMaterial>>().len(),
                0,
                "material 回收"
            );
        }

        /// 表驱动一致性：`RenderRecipe` 说 GridLines 开 ⇔ 系统真画（2026-10-23
        /// 的收口——旧系统**完全不读** recipe，表在骗人）。
        ///
        /// 对全部 8 个 `ViewMode` 跑一遍 `resolve_render_recipe`：表说开就必须
        /// 出实体、说关就必须 0 实体。「说一套做一套」在这里立刻红。
        #[test]
        fn recipe_table_says_on_iff_grid_actually_draws() {
            use crate::state::ViewMode;
            let modes = [
                ViewMode::VectorFlat,
                ViewMode::RealTerrain,
                ViewMode::GoogleMap,
                ViewMode::Faction,
                ViewMode::Event,
                ViewMode::Coastline,
                ViewMode::Topographic,
            ];
            for mode in modes {
                let mut world = grid_world(1.0, true, true);
                world.resource_mut::<ViewState>().mode = mode;
                // `ViewState` / `TerrainTexture` 不实现 Clone（大资源），
                // 故 recipe 求值在借用作用域内完成后再落库。
                let recipe = {
                    let projection = WorldProjection::default();
                    let drill = DrillState::default();
                    crate::recipe::resolve_render_recipe(
                        world.resource::<ViewState>(),
                        world.resource::<LayerFrame>(),
                        &projection,
                        &drill,
                        world.resource::<TerrainTexture>(),
                        None,
                    )
                };
                let table_on = recipe.enabled(PassId::GridLines);
                world.insert_resource(recipe);
                run_grid(&mut world);
                let drawn = grid_entity_count(&mut world) > 0;
                assert_eq!(
                    table_on, drawn,
                    "{mode:?}：表说 {table_on}，实际画 {drawn} —— 表与行为必须一致"
                );
            }
        }

        /// 2×2 幅面 → 3 竖 + 3 横 = 6 条链，每条 2 点 1 段：24 顶点 36 索引。
        #[test]
        fn grid_chains_cover_full_raster_extent() {
            let chains = grid_chains(2, 2, 0.5);
            assert_eq!(chains.len(), 6, "3 竖 + 3 横");
            let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0).expect("规则格链应有 mesh");
            let idx = mesh.indices().expect("索引应存在").len();
            assert_eq!(idx, 6 * 6, "每段一个四边形 = 6 索引");
        }

        /// 网格线落在格缘整数位：2×2 幅面、格径 10 → 最外缘 x = 0 与 x = 20。
        #[test]
        fn grid_chains_span_raster_bounds_exactly() {
            let chains = grid_chains(2, 2, 0.0);
            let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0).expect("规则格链应有 mesh");
            let pos = mesh
                .attribute(MeshVertexAttribute::new(
                    "Vertex_Position",
                    0,
                    bevy::render::render_resource::VertexFormat::Float32x3,
                ))
                .expect("位置属性应存在")
                .as_float3()
                .expect("应为 Float32x3")
                .to_vec();
            // half_w = 0：顶点即格缘本身；端点外扩 0 不越界。
            let xs: Vec<f32> = pos.iter().map(|v| v[0]).collect();
            let ys: Vec<f32> = pos.iter().map(|v| v[1]).collect();
            assert!(
                xs.iter().all(|x| (0.0..=20.0).contains(x)),
                "xs 越界 {xs:?}"
            );
            assert!(
                ys.iter().all(|y| (0.0..=20.0).contains(y)),
                "ys 越界 {ys:?}"
            );
        }

        /// 非方形栅格：竖线条数按宽、横线按高，跨度各自到边。
        #[test]
        fn grid_chains_cover_non_square_raster() {
            let chains = grid_chains(4, 2, 0.5);
            assert_eq!(chains.len(), 5 + 3, "5 竖 + 3 横");
            let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0).expect("应成网格");
            assert!(mesh.indices().is_some());
        }

        /// 资源默认值：未构建过。
        #[test]
        fn grid_lines_overlay_default_is_empty() {
            let o = GridLinesOverlay::default();
            assert!(o.entity.is_none() && o.key.is_none());
        }
    }
}
