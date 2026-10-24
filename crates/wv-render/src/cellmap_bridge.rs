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
use wv_world_cell_map::state::{EditCellState, GridConfig};
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
    build_material, default_params, patch_material_images, set_coast_encoding, warp_amp, CellRect,
    FieldPack, FieldPalette, TerrainFieldMaterial, ViewBranch,
};

/// `ViewMode → ShadeMode` 唯一映射点。
pub fn shade_mode_of(mode: crate::state::ViewMode) -> ShadeMode {
    match mode {
        crate::state::ViewMode::VectorFlat => ShadeMode::VectorFlat,
        crate::state::ViewMode::GoogleMap => ShadeMode::GoogleMap,
        crate::state::ViewMode::RealTerrain => ShadeMode::RealTerrain,
        crate::state::ViewMode::Schematic => ShadeMode::Schematic,
        crate::state::ViewMode::Faction => ShadeMode::Faction,
        crate::state::ViewMode::Event => ShadeMode::Event,
        crate::state::ViewMode::Coastline => ShadeMode::Coastline,
        crate::state::ViewMode::Topographic => ShadeMode::Topographic,
    }
}

/// 色板换算（字段 1:1）。
pub fn conv_palette(p: &wv_theme::palette::PaletteColors) -> CellPalette {
    CellPalette {
        core: p.core,
        soften_keep: p.soften_keep,
        soften_toward: p.soften_toward,
        poi_dot: p.poi_dot,
        poi_ring: p.poi_ring,
        road_stroke: p.road_stroke,
        road_casing: p.road_casing,
        street_major: p.street_major,
        street_minor: p.street_minor,
        label_fill: p.label_fill,
        label_region: p.label_region,
        label_place: p.label_place,
        label_water: p.label_water,
        dot_place: p.dot_place,
        dot_region: p.dot_region,
        dot_faction: p.dot_faction,
        dot_event: p.dot_event,
        text_place: p.text_place,
        text_region: p.text_region,
        text_faction: p.text_faction,
        text_event: p.text_event,
        snow_highlight: p.snow_highlight,
        snow_shadow: p.snow_shadow,
        ice_paper: p.ice_paper,
        ice_ink: p.ice_ink,
        ice_shadow: p.ice_shadow,
        halo_fill: p.halo_fill,
        isoline_ink: p.isoline_ink,
        grid_line: p.grid_line,
        building_top: p.building_top,
        building_side: p.building_side,
        event_backdrop: p.event_backdrop,
        line_on_light: p.line_on_light,
        line_on_mid: p.line_on_mid,
        line_on_dark: p.line_on_dark,
        deep_ocean_blue: p.deep_ocean_blue,
        water_shore: p.water_shore,
    }
}

/// 旗标换算（字段 1:1）。
pub fn conv_treatment(t: &wv_theme::theme_pack::TreatmentFlags) -> TreatmentFlags {
    TreatmentFlags {
        coast_halo: t.coast_halo,
        water_gradient: t.water_gradient,
        snow_shade: t.snow_shade,
        sea_ice: t.sea_ice,
        paper_grain: t.paper_grain,
        river_centerline: t.river_centerline,
        boundary_wobble: t.boundary_wobble,
        ink_sublinear: t.ink_sublinear,
        paper_fog: t.paper_fog,
        land_grain: t.land_grain,
        watercolor_bleed: t.watercolor_bleed,
        shore_shallow: t.shore_shallow,
    }
}

/// 海配方换算（波纹 / 碎浪转 owned `Vec`；镜像字段 1:1）。
pub fn conv_sea(s: &wv_theme::sea::SeaRecipe) -> SeaRecipe {
    let conv = |specs: &'static [wv_theme::sea::RippleSpec]| {
        specs
            .iter()
            .map(|r| RippleSpec {
                dist_cells: r.dist_cells,
                sigma_cells: r.sigma_cells,
                alpha: r.alpha,
            })
            .collect()
    };
    SeaRecipe {
        depth_scale_m: s.depth_scale_m,
        depth_exponent: s.depth_exponent,
        shore_band_cells: s.shore_band_cells,
        shore_peak: s.shore_peak,
        water_grain: s.water_grain,
        ripple_warp_cells: s.ripple_warp_cells,
        ripples: conv(s.ripples),
        ink: s.ink,
        foam: conv(s.foam),
        foam_ink: s.foam_ink,
        foam_break: s.foam_break,
        foam_warp_cells: s.foam_warp_cells,
        shore_hatch: s.shore_hatch.map(|h| HatchSpec {
            dist_cells: h.dist_cells,
            row_spacing_px: h.row_spacing_px,
            alpha: h.alpha,
        }),
        shore_edge_strength: s.shore_edge_strength,
        shore_edge_width_cells: s.shore_edge_width_cells,
        shore_edge_ink: s.shore_edge_ink,
        foam_break_freq: s.foam_break_freq,
    }
}

/// 区域覆盖物规则集换算（键 `&'static str` 保主题字面量；规则 1:1）。
pub fn conv_region_set(
    set: &wv_theme::overlays::RegionOverlaySet,
) -> wv_world_cell_map::region_overlay::RegionOverlaySet {
    use wv_world_cell_map::region_overlay::{RegionOverlayRule, RegionOverlaySet};
    RegionOverlaySet {
        rules: set
            .keys()
            .filter_map(|k| {
                let r = set.rule_for(k)?;
                Some((
                    k,
                    RegionOverlayRule {
                        variant: r.variant,
                        color: r.color,
                        alpha: r.alpha,
                        size_scale: r.size_scale,
                        spacing_scale: r.spacing_scale,
                    },
                ))
            })
            .collect(),
    }
}

/// 整包换算（`ThemePack` 三字段 → `CellPack`）。
pub fn conv_pack(pack: &wv_theme::theme_pack::ThemePack) -> CellPack {
    CellPack {
        palette: conv_palette(&pack.palette),
        treatment: conv_treatment(&pack.treatment),
        sea: conv_sea(&pack.sea),
    }
}

// ══════ from src/terrain_view/setup.rs :: setup_terrain_texture ══════
/// 启动时由工程的地形数据建纹理。若无地形，建一张占位（避免空句柄）。
///
/// Phase 2 起底图**属于某张图**：这里取「当前图向上的第一张自带栅格」，
/// 并把来源图 id 记进 [`TerrainTexture::last_map`]。
pub fn setup_terrain_texture(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
) {
    let atlas = wv_world_cell_map::terrain_view::tile_atlas::ensure_atlas(&mut commands);
    let (map, data) = active_terrain_or_placeholder(&mut projection.world, drill.current(), 64);
    let image = image_from_terrain(
        &data,
        shade_mode_of(view.mode),
        atlas.as_ref(),
        view.terrain_soften,
        &conv_pack(view.active_theme),
        0.0,
    );
    commands.insert_resource(new_terrain_texture(
        &mut images,
        image,
        &data,
        map,
        shade_mode_of(view.mode),
        view.terrain_soften,
        view.active_theme.id,
    ));
}

// ══════ from src/terrain_view/rebuild.rs :: rebuild_texture_on_mode_change ══════
/// 当**视图模式真的变化**时重建整张纹理（换的是眼镜，不是世界，基线 §1.10）。
///
/// ## 为什么不能监听 `Res<ViewState>::is_changed()`（缺陷修复）
///
/// `ViewState` 是「大杂烩资源」：既有低频的 `mode`，也有**每帧无条件写入**的
/// `cursor_world`（见 [`crate::camera::track_cursor_world`]）。对 `ResMut<T>` 的
/// `deref_mut` 写入会标记资源 changed，于是**只要鼠标在地图上移动，
/// `view.is_changed()` 每帧为真**，整张 512×512 纹理就被逐格重着色并重传 GPU。
///
/// 这违反基线 §2.11「不要每帧重建整图」，也违反 `AGENTS.md` §3.4 的拉动式刷新
/// 原则（刷新应由「模式」这一语义输入驱动，而不是「整个 ViewState 资源」变化）。
///
/// 因此改为比较 `tex.last_mode` 与当前 `view.mode`，**只有模式字段真的变了才重建**。
///
/// Phase 2 补一条同类比较：`tex.last_map` 与「当前应有的来源图」。换图（钻入一座
/// 自带地形的城市）与换模式一样是**语义输入变化**，必须重建；否则会继续画着
/// 世界图的底图、而 `grid` 已按城市帧对齐，地形与标记错开整幅。
///
/// ## 边界：首次地形可用
///
/// 若地形数据尚不可用（`terrain` 为 `None`），直接返回，**不写 `last_mode`**——
/// 否则地形稍后加载完时，`last_mode` 已等于当前 mode，会误判「没变」而永不重建。
/// `last_mode` 仅在**真正生成了纹理之后**才更新。
/// 系统参数即函数参数（Bevy 注入），7 个都是必要资源——与 sync_street_layers 同理由允许超参。
#[allow(clippy::too_many_arguments)]
pub fn rebuild_texture_on_mode_change(
    mut commands: Commands,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    atlas: Option<Res<TileAtlas>>,
    tex: Res<TerrainTexture>,
    mut images: ResMut<Assets<Image>>,
    mut pending: ResMut<PendingTerrainSynth>,
) {
    // 模式、淡化开关、主题、来源图任一真的变了才需要重建（不看整个 ViewState 资源）。
    let map_changed = match active_terrain(&projection.world, drill.current()) {
        Some((node, _)) => tex.last_map.as_ref() != Some(&node.id),
        None => false,
    };
    if !map_changed
        && tex.last_mode == Some(shade_mode_of(view.mode))
        && tex.last_soften == view.terrain_soften
        && tex.last_theme_id == view.active_theme.id
    {
        return;
    }
    let Some((node, data)) = active_terrain(&projection.world, drill.current()) else {
        // 地形尚未可用：不重建，也不置位 last_mode，等地形就绪后再走这里。
        return;
    };
    let ctx = SynthCtx {
        mode: shade_mode_of(view.mode),
        soften: view.terrain_soften,
        theme_id: view.active_theme.id,
        pack: conv_pack(view.active_theme),
        map: Some(node.id.clone()),
        data_hash: terrain_data_hash(data),
        topographic_thresholds: (0, 0), // PR-1 占位；PR-1b 由 `topographic_thresholds_for` 填真值。
    };
    // 已有同指纹任务在跑：等它落地，不重复排。
    if pending.task.is_some() && pending.ctx.as_ref() == Some(&ctx) {
        return;
    }
    // 指纹不同：drop 旧任务（`Task::Drop` 取消），按新条件重排。
    pending.task = None;
    pending.ctx = None;
    // 换级淡化（ADR-0009 §2⑦ / README §12-25）：只有**换图**才淡出旧底图；
    // 换模式/换淡化开关是「换眼镜」，保持既有的瞬时切换（淡化是 12-25 的收敛项）。
    // 注意此刻 `grid` 仍是旧图的栅格口径（sync_grid_config 在本系统之后运行），
    // 淡出精灵恰好盖住旧底图原本的位置与幅面。
    if map_changed && tex.last_map.is_some() && pending.fade_for_map.as_ref() != Some(&node.id) {
        spawn_basemap_fadeout(&mut commands, &mut images, &tex, &grid);
        pending.fade_for_map = Some(node.id.clone());
    }

    // 整图合成移出主线程：**闭包内零 ECS / 零 Assets 触碰**（`Assets<Image>`
    // 非线程安全，像素回主线程后由 `land_pending_terrain_synth` 建资产）。
    let data_clone = data.clone();
    let atlas_clone = atlas.as_deref().cloned();
    let task_ctx = ctx.clone();
    let task = bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default)
        .spawn(async move {
            // 主题包由闭包捕获（`&'static ThemePack` 是 Copy）——异步任务里
            // 拿不到 registry，只能在提交时捕获。
            let pack = task_ctx.pack.clone();
            let (pixels, res) = terrain_pixels(
                &data_clone,
                task_ctx.mode,
                atlas_clone.as_ref(),
                task_ctx.soften,
                &pack,
            );
            SynthResult {
                pixels,
                res,
                ctx: task_ctx,
            }
        });
    pending.task = Some(task);
    pending.ctx = Some(ctx);
}

// ══════ from src/terrain_view/rebuild.rs :: land_pending_terrain_synth ══════
/// 主线程落地：任务完成且世界未变 → 建 `Image` 资产并替换纹理。
///
/// 校验失败（快速连切 palette / 完成前换图）直接丢弃；`last_*` 未置位，
/// 下一帧 `rebuild_texture_on_mode_change` 会按新条件重新触发。
pub fn land_pending_terrain_synth(
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut tex: ResMut<TerrainTexture>,
    mut images: ResMut<Assets<Image>>,
    mut pending: ResMut<PendingTerrainSynth>,
) {
    let Some(task) = pending.task.as_ref() else {
        return;
    };
    if !task.is_finished() {
        return;
    }
    let Some(task) = pending.task.take() else {
        return;
    };
    let result = bevy::tasks::block_on(task);
    pending.ctx = None;
    if !ctx_matches_view(&result.ctx, &view)
        || !ctx_matches_world(&result.ctx, &projection.world, drill.current())
    {
        // 世界已变：丢弃旧像素，重建条件会在下一帧重新触发。
        return;
    }
    let Some((node, data)) = active_terrain(&projection.world, drill.current()) else {
        return;
    };
    let (w, h) = (data.width.max(1), data.height.max(1));
    let image = Image::new(
        bevy::render::render_resource::Extent3d {
            width: w * result.res,
            height: h * result.res,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        result.pixels,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    replace_terrain_texture(
        &mut tex,
        &mut images,
        image,
        data,
        Some(node.id.clone()),
        result.ctx.mode,
        result.ctx.soften,
        result.ctx.theme_id,
    );
    // 整图内容已整体替换：chunk 层需要全量重贴。
    tex.chunks_dirty = Some([0, 0, tex.size, tex.height]);
    tex.last_mode = Some(result.ctx.mode);
    tex.last_soften = result.ctx.soften;
    tex.last_theme_id = result.ctx.theme_id;
    tex.last_map = Some(node.id.clone());
    // 这次换图的淡出精灵已经就位；下一次换图再重新判定。
    pending.fade_for_map = None;
}

// ══════ from src/terrain_view/rebuild.rs :: ctx_matches_view ══════
/// 落地校验：当前 `active_terrain` 的来源图与内容指纹都必须与任务上下文一致。
///
/// 快速连切 palette / 合成完成前换图 → 旧像素不得贴进新世界；丢弃后由
/// `rebuild_texture_on_mode_change` 在后续帧按新条件重新触发。
/// 落地前再看一眼视图输入：A→B→A 快速连切时，任务 B 完成时当前视图已是 A，
/// 落地 B 会造成「旧调色板闪回」。
pub fn ctx_matches_view(ctx: &SynthCtx, view: &ViewState) -> bool {
    ctx.mode == shade_mode_of(view.mode)
        && ctx.soften == view.terrain_soften
        && ctx.theme_id == view.active_theme.id
}

// ══════ from src/terrain_view/isoline_overlay.rs :: sync_isoline_overlay ══════
/// 同步等值线叠加层。排在 [`super::terrain_chunks::sync_terrain_chunks`]
/// **之前**：借它还没消费的 `tex.chunks_dirty` 感知「地形内容变了」
/// （笔刷提交 / 整图重建都会置位），结构键本身不吸收这个瞬态位。
#[allow(clippy::too_many_arguments)]
pub fn sync_isoline_overlay(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    recipe: Res<crate::recipe::RenderRecipe>,
    mut overlay: ResMut<IsolineOverlay>,
) {
    // 模式轴：recipe 表（VectorFlat / Topographic 开档）；会话轴：`view.isolines`
    // 旗标（map controls 瓷砖，GridLines 同款，2026-10-23 默认关——ADR-0051 追记）。
    // 关图层（YAML ∨ 会话，ADR-0013）/ 换模式即退场（键随之清零，重进重算）。
    if !layers.terrain.visible || !recipe.enabled(crate::recipe::PassId::Isoline) || !view.isolines
    {
        overlay.despawn(&mut commands);
        return;
    }
    let Some(data) = texture_terrain(&tex, &projection.world, drill.current()) else {
        overlay.despawn(&mut commands);
        return;
    };
    let treatment = conv_treatment(&view.active_theme.treatment);
    let nation = view.scale >= crate::state::LOD_NATION_SCALE;
    let key = IsolineKey {
        map: tex.last_map.clone(),
        size: tex.size,
        height: tex.height,
        res: tex.res,
        soften: tex.last_soften,
        treatment: [
            treatment.coast_halo,
            treatment.river_centerline,
            treatment.ink_sublinear,
        ],
        tol_bucket: dp_tol_bucket(view.scale),
        nation,
    };
    let structural_same = overlay.key.as_ref() == Some(&key);
    let dirty_pending = tex.chunks_dirty.is_some();
    // 「几何不必重算」判定：结构同键，且（无脏区 ∨ 脏区来自纯换色——地形内容
    // 指纹未变）。palette 切换的整图重建会置全图 `chunks_dirty`，但 cells 逐格
    // 未动：指纹比对（~3ms）换掉整条派生管线（1024² 实测 ~75ms）。
    let geometry_unchanged = structural_same
        && (overlay.entity.is_some() || overlay.empty_settled())
        && (!dirty_pending || overlay.data_hash == Some(terrain_data_hash(data)));
    if geometry_unchanged {
        // 结构没变也可能只是图层绑定变了（作者改 opacity/z）：就地补丁，
        // 不重算派生几何。palette 变了同样只重上色（`ink_palette` 快路径）。
        if layers.is_changed() || overlay.ink_palette != Some(view.active_theme.id) {
            apply_material_colors(
                &mut materials,
                &overlay,
                &conv_palette(&view.active_theme.palette),
                layers.terrain.opacity,
            );
            overlay.ink_palette = Some(view.active_theme.id);
        }
        if layers.is_changed() {
            if let Some(entity) = overlay.entity {
                commands.entity(entity).insert(Transform::from_xyz(
                    0.0,
                    0.0,
                    ISOLINE_Z + layers.terrain.z_offset,
                ));
            }
        }
        return;
    }

    // 派生管线（plan §2.4：提取 → 装配 → Chaikin → DP）。
    let w = data.width;
    let h = data.height;
    let cell = |x: u32, y: u32| &data.cells[(y * w + x) as usize];
    // 容差取**档中心**（不用连续式）：否则键说「同档」而容差随 scale 漂移，
    // 几何变了却不重建（2026-10-23 D 项修复的配套约束）。
    let tol = dp_tol_for_bucket(key.tol_bucket);
    // 装饰旗标（ADR-0023 D4）：从当前主题包派生。内置两包都是 HAND_DRAWN（全
    // `true`）——所有 gate 分支在 `true` 时走与历史恒开**逐位相同**的路径。
    let treatment = conv_treatment(&view.active_theme.treatment);
    // 描边世界半宽（手绘地图 #10，2026-10-15）：**亚线性**缩放——
    // 世界半宽 = px × scale^0.78 而非 px × scale（ADR-0008 §8 乘口径的
    // 修正式）。civ-atlas GLYPH_SHRINK=0.22 同款思路：放大 4 倍时墨线的
    // *世界*尺寸只长 ~3 倍 → *屏幕*观感随放大反而变纤细（k^-0.22），地图
    // 细节浮现而墨线不霸道；缩小时屏幕上线变粗，远观仍可辨。
    // 世界下限保留：极远时防消隐。
    // 装饰旗标 gate：`ink_sublinear = false` 的主题回退 ADR-0008 §8 的严格
    // 乘口径（屏幕恒定宽）。
    let thin_half = if treatment.ink_sublinear {
        stroke_half_world(STROKE_PX, view.scale, grid.cell_size * 0.03)
    } else {
        (STROKE_PX * 0.5 * view.scale).max(grid.cell_size * 0.03)
    };
    // 类型边界加宽：格缘阶梯被 Chaikin 磨角后，平滑曲线偏离原阶梯至多约
    // 0.3 格——细线盖不住露出的「错色台阶」，按格径给足半宽才压得平。
    let kind_half = thin_half.max(grid.cell_size * KIND_STROKE_CELL);

    // ---- 提取 + 装配 + Chaikin 缓存（Task 3）----
    // 这些与缩放无关；只有 DP 容差 / Nation 档消费它们。缓存输入不含 tol_bucket：
    // 缩放只在缓存上重跑 DP（毫秒级），笔刷改地形 / 切图 / 改 treatment 才重建链。
    let data_hash = terrain_data_hash(data);
    let chain_inputs = ChainCacheInputs {
        map: key.map.clone(),
        data_hash,
        treatment: key.treatment,
        nation,
    };
    let rebuild_chains = overlay.chains.as_ref().map(|c| &c.inputs) != Some(&chain_inputs);
    if rebuild_chains {
        let coast_segs = height_segments(w, h, |x, y| cell(x, y).height as f32, 0.0);
        // Nation 档只画海岸线（与 POI / 建筑同口径，spec §2.2c）：
        // 类型边界在远观读不出差异，却让描边总长翻数倍。
        let kinds_segs = (!nation).then(|| {
            kind_segments(w, h, |x, y| {
                wv_terrain::biome::display_kind(&wv_terrain::TerrainCell {
                    height: cell(x, y).height,
                    surface: cell(x, y).surface,
                    biome: cell(x, y).biome,
                })
            })
        });
        let coast_smoothed: Vec<IsolineChain> = assemble_chains(&coast_segs)
            .into_iter()
            .map(|c| (chaikin(&c.pts, c.closed, 2), c.closed))
            .collect();
        let kinds_smoothed: Vec<IsolineChain> = kinds_segs
            .as_deref()
            .map(|segs| {
                assemble_chains(segs)
                    .into_iter()
                    // 类型场边界是格缘阶梯：先小容差预抽稀去共线中点，再 Chaikin，
                    // 否则平滑后的点数翻两倍全是楼梯碎点。
                    .map(|c| {
                        let pts = simplify_dp(&c.pts, c.closed, 0.30);
                        (chaikin(&pts, c.closed, 2), c.closed)
                    })
                    .collect()
            })
            .unwrap_or_default();
        overlay.chains = Some(CachedChains {
            inputs: chain_inputs,
            coast_smoothed,
            kinds_smoothed,
        });
    }
    // 缩放热路径：只跑 DP + 半宽。
    let (strokes, coast_halo_inner, coast_halo_outer, coast_river) = {
        let chains = overlay.chains.as_ref().expect("上面刚建立");
        assemble_strokes(
            &chains.coast_smoothed,
            &chains.kinds_smoothed,
            tol,
            thin_half,
            kind_half,
            treatment,
        )
    };

    let Some(mesh) = build_stroke_mesh(&strokes, grid.raster_origin, grid.cell_size) else {
        // 空几何（无等值线）也是稳态：0 顶点 mesh 会踩 bevy #24874
        // （MeshAllocator 误报 Use-after-free），退场不插；键记为「空已定」，
        // 下一帧走 `empty_settled` 快路，不重算。
        overlay.despawn(&mut commands);
        overlay.key = Some(key);
        overlay.data_hash = Some(data_hash);
        overlay.ink_palette = Some(view.active_theme.id);
        return;
    };
    let handle = meshes.add(mesh);
    let pal = &conv_palette(&view.active_theme.palette);
    let material = overlay
        .material
        .get_or_insert_with(|| {
            materials.add(ColorMaterial::from_color(
                conv_palette(&view.active_theme.palette).isoline_ink,
            ))
        })
        .clone();
    // 材质跨重建复用（get_or_insert）：重算后也要按当前绑定刷新 alpha。
    apply_material_colors(&mut materials, &overlay, pal, layers.terrain.opacity);

    overlay.despawn(&mut commands);
    let entity = commands
        .spawn((
            Mesh2d(handle.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, ISOLINE_Z + layers.terrain.z_offset),
        ))
        .id();
    overlay.entity = Some(entity);
    overlay.mesh = Some(handle);

    // 水彩外晕（手绘地图 #4）：仅当海岸链非空时生成。晕在主线下层（z 更小），
    // 避免遮住主线。alpha 渐变（`HALO_*_ALPHA`）实现水彩晕染感。
    if !coast_halo_inner.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_halo_inner, grid.raster_origin, grid.cell_size)
        {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.isoline_ink
                    .with_alpha(HALO_1_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z - 0.002 + layers.terrain.z_offset),
                ))
                .id();
            overlay.halo_outer = Some((e, h, mat));
        }
    }
    if !coast_halo_outer.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_halo_outer, grid.raster_origin, grid.cell_size)
        {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.isoline_ink
                    .with_alpha(HALO_2_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z - 0.004 + layers.terrain.z_offset),
                ))
                .id();
            overlay.halo_outer_outer = Some((e, h, mat));
        }
    }
    // 河水中线（手绘地图 #8）：在主线上方，z+0.001，宽度 0.5× main，
    // `label_water` 颜色 + 0.5 alpha——"水边反光"观感（civ-atlas 风格）。
    if !coast_river.is_empty() {
        if let Some(mesh) = build_stroke_mesh(&coast_river, grid.raster_origin, grid.cell_size) {
            let h = meshes.add(mesh);
            let mat = materials.add(ColorMaterial::from_color(
                pal.label_water
                    .with_alpha(RIVER_ALPHA * layers.terrain.opacity),
            ));
            let e = commands
                .spawn((
                    Mesh2d(h.clone()),
                    MeshMaterial2d(mat.clone()),
                    Transform::from_xyz(0.0, 0.0, ISOLINE_Z + 0.001 + layers.terrain.z_offset),
                ))
                .id();
            overlay.coast_river = Some((e, h, mat));
        }
    }

    overlay.key = Some(key);
    overlay.data_hash = Some(terrain_data_hash(data));
    overlay.ink_palette = Some(view.active_theme.id);
}

// ══════ from src/terrain_view/atmosphere.rs :: sync_atmosphere ══════
/// 同步氛围层。独立注册（主链已顶满 20 参元组上限），排在 `sync_map_frame` 之后。
///
/// 幅面 = **栅格幅面**（`raster_origin` + `size × cell_size`）——雾与噪点是
/// 「这张纸」的属性，与底图完全重合。
#[allow(clippy::too_many_arguments)]
pub fn sync_atmosphere(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    brush: Res<BrushState>,
    recipe: Res<crate::recipe::RenderRecipe>,
    mut atm: ResMut<AtmosphereLayer>,
) {
    // GoogleMap / Coastline 由 recipe 关 atmosphere pass；这里只关 texture
    // 维度。LayerFrame.gate 与 view.mode 排除都在 recipe 里集中。
    if !layers.terrain.visible
        || !recipe.enabled(crate::recipe::PassId::Atmosphere)
        || tex.size == 0
    {
        atm.despawn(&mut commands);
        return;
    }
    // 装饰旗标（ADR-0023 D4）：从当前主题包派生；内置两包都是 HAND_DRAWN。
    let treatment = conv_treatment(&view.active_theme.treatment);
    let key = AtmosphereKey {
        origin: (
            grid.raster_origin.x.to_bits(),
            grid.raster_origin.y.to_bits(),
        ),
        extent: (tex.size, tex.height, grid.cell_size.to_bits()),
        scale_bucket: scale_bucket(view.scale),
        treatment: (treatment.paper_fog, treatment.paper_grain),
    };
    if atm.key.as_ref() != Some(&key) {
        atm.despawn(&mut commands);
        let size = Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size;
        // 手绘地图 #6：grain uv 随 `view.scale` 微调——近看 grain 翻倍密、远看
        // 减半；连续变化用 `scale_bucket` 量化到 1/8 octaves 档避免逐帧重建。
        let grain_tile = (GRAIN_TILE_WORLD / view.scale.max(0.05)).max(1.0);
        let uv = Vec2::splat(size.x / grain_tile);
        // —— 雾霭（初色 NONE，末尾逐帧更新段马上会写入当前值）——
        // 装饰旗标 gate：`paper_fog = false` 的主题不铺雾（`atm.veil` 留 None，
        // 下方逐帧 alpha 更新段与 despawn 都对 None 天然短路）。
        if treatment.paper_fog {
            let veil_mesh = meshes.add(extent_quad_mesh(size, Vec2::ZERO));
            let veil_mat = materials.add(ColorMaterial::from_color(Color::NONE));
            let veil = commands
                .spawn((
                    Mesh2d(veil_mesh.clone()),
                    MeshMaterial2d(veil_mat.clone()),
                    Transform::from_xyz(grid.raster_origin.x, grid.raster_origin.y, VEIL_Z),
                ))
                .id();
            atm.veil = Some((veil, veil_mesh, veil_mat));
        }
        // —— 纸张噪点 ——
        // 装饰旗标 gate：`paper_grain = false` 的主题不铺纸纹。
        if treatment.paper_grain {
            let grain_tex = atm
                .grain_texture
                .get_or_insert_with(|| images.add(paper_grain_image(0x5EED_5EED)))
                .clone();
            let grain_mesh = meshes.add(extent_quad_mesh(size, uv));
            let grain_mat = materials.add(ColorMaterial {
                texture: Some(grain_tex),
                color: Color::WHITE.with_alpha(GRAIN_ALPHA),
                ..Default::default()
            });
            let grain = commands
                .spawn((
                    Mesh2d(grain_mesh.clone()),
                    MeshMaterial2d(grain_mat.clone()),
                    Transform::from_xyz(grid.raster_origin.x, grid.raster_origin.y, GRAIN_Z),
                ))
                .id();
            atm.grain = Some((grain, grain_mesh, grain_mat));
        }
        atm.key = Some(key);
    }
    // 雾霭 alpha 逐帧连续更新（缩放的空气感 + 编辑态免雾）；颜色随主题纸面
    //（ADR-0016）。免雾只动 alpha 不拆实体：笔刷一关，下一帧雾即回来。
    let pal = &conv_palette(&view.active_theme.palette);
    if let Some((_, _, mat)) = &atm.veil {
        if let Some(mut m) = materials.get_mut(mat) {
            m.color = pal
                .soften_toward
                .with_alpha(veil_alpha_for(view.scale, brush.enabled));
        }
    }
}

// ══════ from src/cell_selection.rs :: sync_cell_region ══════
/// 区域选区高亮（明显视觉 = 金色半透填充 + **alpha 脉动**）。
///
/// gate = `recipe.enabled(CellSelection) ∧ !edit_cell.region_cells.is_empty()`；
/// 与单格高亮（`sync_cell_selection`）并存——区域优先视觉（单格 outline
/// 画在 Z+0.001，区域填充在其下）。
#[allow(clippy::too_many_arguments)]
pub fn sync_cell_region(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    recipe: Res<crate::recipe::RenderRecipe>,
    edit_cell: Res<EditCellState>,
    grid: Res<GridConfig>,
    time: Res<Time>,
    mut entities: ResMut<CellRegionEntities>,
) {
    let cells = edit_cell.region_cells.as_slice();
    let active = recipe.enabled(PassId::CellSelection) && !cells.is_empty();
    if !active {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    }
    // 脉动：0.22 + 0.12·sin(3.5t) → alpha ∈ [0.10, 0.34]，肉眼可辨的呼吸感。
    let alpha = 0.22 + 0.12 * (time.elapsed_secs() * 3.5).sin();
    let accent = crate::theme::colors::ACCENT_SELECT;

    // ── 填充 mesh：只在格集合变化时重建 ──
    let fingerprint = cells_fingerprint(cells);
    if fingerprint != entities.last_fingerprint {
        entities.release_assets(&mut meshes);
        let Some((mesh, origin, bbox)) = build_region_fill_mesh(cells, &grid) else {
            entities.despawn_all(&mut commands);
            return;
        };
        entities.fill_mesh = Some(meshes.add(mesh));
        // 外包 4 边（世界厚度固定 2 单位）。
        let thickness = 2.0;
        let t2 = thickness * 0.5;
        let half = bbox * 0.5;
        let edge_defs: [(Vec2, Vec2); 4] = [
            (Vec2::new(0.0, -half.y + t2), Vec2::new(bbox.x, thickness)),
            (Vec2::new(0.0, half.y - t2), Vec2::new(bbox.x, thickness)),
            (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, bbox.y)),
            (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, bbox.y)),
        ];
        for (i, (_, edge_size)) in edge_defs.into_iter().enumerate() {
            entities.edge_meshes[i] = Some(meshes.add(Rectangle::new(edge_size.x, edge_size.y)));
        }
        entities.last_fingerprint = fingerprint;
        // 位置随 origin 变（格集合变了 → 包围盒原点可能变）。
        let center = origin + bbox * 0.5;
        if let Some(e) = entities.fill {
            commands
                .entity(e)
                .insert(Transform::from_translation(origin.extend(0.899)));
        }
        for (i, (offset, _)) in edge_defs.into_iter().enumerate() {
            if let Some(e) = entities.edges[i] {
                commands.entity(e).insert(Transform::from_translation(
                    (center + offset).extend(0.8995),
                ));
            }
        }
    }

    // ── 材质：只建一次，之后每帧只改 alpha ──
    if entities.fill_material.is_none() {
        entities.fill_material =
            Some(materials.add(ColorMaterial::from_color(accent.with_alpha(alpha))));
    } else if let Some(h) = entities.fill_material.clone() {
        if let Some(mut m) = materials.get_mut(&h) {
            m.color = accent.with_alpha(alpha);
        }
    }
    for slot in &mut entities.edge_materials {
        if slot.is_none() {
            *slot = Some(materials.add(ColorMaterial::from_color(accent.with_alpha(alpha))));
        } else if let Some(h) = slot.clone() {
            if let Some(mut m) = materials.get_mut(&h) {
                m.color = accent.with_alpha((alpha * 2.5).min(1.0));
            }
        }
    }

    // ── 实体：建一次，之后只挂句柄（句柄不变 → insert 无 GPU 重传）──
    let fill_mesh = entities.fill_mesh.clone().expect("集合非空,mesh 已建");
    let fill_mat = entities.fill_material.clone().expect("材质已建");
    match entities.fill {
        Some(e) => {
            commands
                .entity(e)
                .insert((Mesh2d(fill_mesh), MeshMaterial2d(fill_mat)));
        }
        None => {
            entities.fill = Some(
                commands
                    .spawn((
                        Transform::from_translation(Vec3::ZERO),
                        Mesh2d(fill_mesh),
                        MeshMaterial2d(fill_mat),
                    ))
                    .id(),
            );
        }
    }
    for i in 0..4 {
        let Some(mesh) = entities.edge_meshes[i].clone() else {
            continue;
        };
        let Some(mat) = entities.edge_materials[i].clone() else {
            continue;
        };
        match entities.edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((Mesh2d(mesh), MeshMaterial2d(mat)));
            }
            None => {
                entities.edges[i] = Some(
                    commands
                        .spawn((
                            Transform::from_translation(Vec3::ZERO),
                            Mesh2d(mesh),
                            MeshMaterial2d(mat),
                        ))
                        .id(),
                );
            }
        }
    }
}

// ══════ from src/terrain_chunks.rs :: sync_terrain_chunks ══════
/// 主 system：在 `flush_terrain_texture`（wv-app::edit） 与
/// [`crate::terrain_view::rebuild_texture_on_mode_change`] 之后跑。
///
/// 输入：整图 `TerrainTexture` + `view.scale` / `view.camera_center` / 窗口大小。
/// 输出：spawn / despawn chunk 实体；对已存在块按需 patch 像素或切 mip。
///
/// ## 每帧开销
///
/// 稳态（无编辑、无 spawn）：只做视口 AABB 与哈希查找，**零像素拷贝、零资产新建**。
/// 笔刷提交后一帧：克隆整图源像素一次，patch 相交块，重建其 mip1/mip2。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_chunks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut tex: ResMut<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    mut chunk_entities: ResMut<TerrainChunkEntities>,
    mut q_chunks: Query<(&mut TerrainChunk, &Mesh2d)>,
) {
    // ── ADR-0053 回退开关（2026-10-24 Phase 1 接线）────────────────────────
    // 场渲染开着 = 底图由 `sync_terrain_field` 的 GPU quad 负责。两条路径并存，
    // 但**同时只能有一条上屏**：整图烘焙纹理与场 quad 叠在一起就是双重底图。
    // 故这里整层退场（回退路径本身一行不动，见下方全部逻辑）。
    //
    // `chunks_dirty` 照旧在这里消费清零（语义不变）：`sync_terrain_field` 排在
    // **本系统之后**，它用的编辑信号是变更戳而不是这个旗标，故不需要抢。
    // 关回场时块按最新像素 spawn（与下方 `!visible` 分支同口径）。
    if view.field_render {
        let entities: Vec<Entity> = chunk_entities.by_coord.drain().map(|(_, e)| e).collect();
        for entity in entities {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
        if tex.chunks_dirty.is_some() {
            tex.chunks_dirty = None;
        }
        return;
    }

    // 关闭图层（YAML 声明 ∨ 会话开关，ADR-0013）→ 全部 despawn；
    // 丢掉待消费脏矩形（重开时 spawn 会拷最新像素）。
    if !layers.terrain.visible {
        let entities: Vec<Entity> = chunk_entities.by_coord.drain().map(|(_, e)| e).collect();
        for entity in entities {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
        if tex.chunks_dirty.is_some() {
            tex.chunks_dirty = None;
        }
        return;
    }
    // 绑定变化（作者改 YAML 的 opacity/z）：已存在块就地补丁——不重建像素，
    // 只写材质 alpha 与 Transform.z（重 spawn 的块在下方 spawn 路径取新值）。
    // 借**延迟命令**写：本系统的 `q_chunks` 已持有 `&mut TerrainChunk`，
    // 再开一个读它的查询会触发 Bevy B0001 访问冲突（启动即 panic）。
    if layers.is_changed() {
        let z = TERRAIN_Z + layers.terrain.z_offset;
        let alpha = layers.terrain.opacity;
        for &entity in chunk_entities.by_coord.values() {
            let Ok((chunk, _)) = q_chunks.get(entity) else {
                continue;
            };
            let material = chunk.material.clone();
            commands.queue(move |world: &mut World| {
                if let Some(mut tf) = world.get_mut::<Transform>(entity) {
                    tf.translation.z = z;
                }
                if let Some(mut mat) = world
                    .resource_mut::<Assets<ColorMaterial>>()
                    .get_mut(&material)
                {
                    mat.color = Color::WHITE.with_alpha(alpha);
                }
            });
        }
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let (size_w, size_h) = (tex.size, tex.height);
    let google_mode = view.mode == crate::state::ViewMode::GoogleMap;
    let cell_size = grid.cell_size;
    let [cw, ch] = chunk_grid(size_w, size_h);
    let res = tex.res;

    // 视口 AABB 与当前 mip 级。相机旋转（地图朝向）决定视口斜矩形的朝向。
    let cam_rot = camera
        .single()
        .map(|t| t.rotation)
        .unwrap_or(Quat::IDENTITY);
    let (vp_min, vp_max) = viewport_world_aabb(
        view.camera_center,
        view.scale,
        Vec2::new(window.resolution.width(), window.resolution.height()),
        cam_rot,
    );
    let mip_now = select_mip(view.scale, cell_size / res as f32);
    let chunk_world_size = CHUNK_SIZE as f32 * cell_size;

    // 1) 视口外 chunk despawn，**并强制清掉超出当前块网格的旧图残块**。
    //    换图后新图更小（如 1024² → 512×288）时，旧图右下角的块坐标已不在
    //    新网格内；它们的 `chunk_cell_rect` 退化成空矩形、原点又可能仍在
    //    视口内，只靠可见性判断会把这些旧像素留在画面上（评审用户报告的
    //    「新建地图后旧图还留在主画面」）。
    let mut to_despawn: Vec<[u32; 2]> = Vec::new();
    for &coord in chunk_entities.by_coord.keys() {
        if chunk_out_of_grid(coord, [cw, ch]) {
            to_despawn.push(coord);
            continue;
        }
        let [gx0, gy0, _, _] = chunk_cell_rect(coord, size_w, size_h);
        let cmin = grid.raster_origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
            to_despawn.push(coord);
        }
    }
    for coord in to_despawn {
        if let Some(entity) = chunk_entities.by_coord.remove(&coord) {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
    }

    // 2) 收集视口内块；判断本帧是否需要源像素——有待 spawn 块，或有与 dirty 相交的块。
    let pending_dirty = tex.chunks_dirty;
    let mut visible: Vec<([u32; 2], [u32; 4])> = Vec::new();
    let mut needs_src = false;
    for cy in 0..ch {
        for cx in 0..cw {
            let coord = [cx, cy];
            let cells = chunk_cell_rect(coord, size_w, size_h);
            let cmin = grid.raster_origin + Vec2::new(cells[0] as f32, cells[1] as f32) * cell_size;
            if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
                continue;
            }
            if !chunk_entities.by_coord.contains_key(&coord)
                || pending_dirty.is_some_and(|d| rects_intersect(d, cells))
            {
                needs_src = true;
            }
            visible.push((coord, cells));
        }
    }

    // 源像素只在"确实要写"时克隆——整图 4096²×4B ≈ 67MB，每帧克隆不可接受。
    let src_data: Option<Vec<u8>> = if needs_src {
        match images.get(&tex.handle) {
            Some(source) if source.data.as_ref().is_some_and(|d| !d.is_empty()) => {
                source.data.clone()
            }
            _other => {
                // TODO(诊断 2026-10-14): "只看到等值线、没有底图填充" 回归。
                // 触发条件是 needs_src=true 但 source.data 为空/缺。常见原因：
                // ① tex.handle 资产被 replace_terrain_texture 用 *img=image 整
                //    体替换——bevy 0.15 的 Assets<Image> 是 Arc-backed，clone 仍
                //    指向旧值（这是已知的迁移期坑）。
                // ② handle 与 images 里其他槽位撞键。
                // 关闭诊断后移除本分支。
                if std::env::var("WV_CHUNK_DIAG").is_ok() {
                    let has_handle = images.get(&tex.handle).is_some();
                    let data_len = images
                        .get(&tex.handle)
                        .and_then(|i| i.data.as_ref())
                        .map(|d| d.len())
                        .unwrap_or(0);
                    eprintln!(
                        "[chunk-diag] needs_src=true but source unusable: handle_valid={has_handle} data_len={data_len} tex.size={} tex.res={} chunk_entities={} visible_chunks={}",
                        tex.size,
                        tex.res,
                        chunk_entities.by_coord.len(),
                        visible.len(),
                    );
                }
                // `&Image` 是 Copy，match 块结束自动释放——不需要手动 drop。
                None
            }
        }
    } else {
        None
    };
    // 需要像素但源未就绪：保留 chunks_dirty，下帧重试。
    if needs_src && src_data.is_none() {
        if std::env::var("WV_CHUNK_DIAG").is_ok() {
            eprintln!("[chunk-diag] bailing: needs_src but src_data=None, keeping chunks_dirty");
        }
        return;
    }
    let src = src_data.as_deref();

    // 3) spawn / patch / 切 mip。
    for (coord, cells) in visible {
        let [gx0, gy0, gx1, gy1] = cells;
        let cmin = grid.raster_origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        // 块的**世界矩形**按实际格数算：16:9 地图的右下尾块只有 32 格高，
        // 不能拿 256×256 的方形去铺（会拉伸并画出地图边界）。
        let chunk_rect = chunk_world_rect(cells, cell_size);
        let center = cmin + chunk_rect * 0.5;

        if let Some(&entity) = chunk_entities.by_coord.get(&coord) {
            // 3a-0) 尺寸失配检测：tex.size/res 随世界切换变化（如 `-m` 加载
            // 1024 格工程，res 8→4）后，旧块的 mip 资产 extent 仍是旧 res 的。
            // `patch_chunk_mips` 用 `Vec::clone_from` 写 mip（长度可变），会把
            // 新 res 的数据塞进旧 extent 的 Image——GPU 上传即 panic
            // （wgpu device.rs「range end index out of range」）。整块作废，
            // 移出 by_coord 落到下方重生路径，按新尺寸重建。
            let mip0_matches = q_chunks
                .get(entity)
                .ok()
                .and_then(|(c, _)| c.mip_handles.first())
                .and_then(|h| images.get(h))
                .map(|mip0| {
                    mip0.texture_descriptor.size.width == (gx1 - gx0) * res
                        && mip0.texture_descriptor.size.height == (gy1 - gy0) * res
                        && q_chunks
                            .get(entity)
                            .map(|(c, _)| c.linear_filter == google_mode)
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if !mip0_matches {
                chunk_entities.by_coord.remove(&coord);
                despawn_chunk(&mut commands, entity, &q_chunks);
            } else {
                let Ok((mut chunk, _)) = q_chunks.get_mut(entity) else {
                    continue;
                };
                // 3a) dirty 相交 → patch mip0 像素，并逐级重建 mip1/mip2。
                if let (Some(dirty), Some(src)) = (pending_dirty, src) {
                    if rects_intersect(dirty, cells) {
                        patch_chunk_mips(&mut images, &chunk, src, size_w, res, cells, dirty);
                        // Bevy 0.15：就地改 `Image` 数据不使既有 `ColorMaterial` 的
                        // bindgroup 失效——GPU 上仍采样旧纹理。触碰材质资产强制
                        // 重建 bindgroup，让本帧 patch 的像素真正上屏（模式切换 /
                        // 笔刷提交共用此路径）。
                        if materials.get_mut(&chunk.material).is_none() {
                            continue;
                        }
                    }
                }
                // 3b) mip 级随缩放切换——只换材质纹理句柄，几何与资产不动。
                if chunk.current_mip != mip_now {
                    if let Some(mut mat) = materials.get_mut(&chunk.material) {
                        mat.texture = chunk.mip_handles.get(mip_now as usize).cloned();
                    }
                    chunk.current_mip = mip_now;
                }
                continue;
            }
        }

        // spawn：拷贝本块像素 + 建 mip 链 + 建几何与材质。
        let Some(src) = src else { continue };
        let chunk_pixels = copy_chunk_pixels(src, size_w, res, gx0, gy0, gx1, gy1);
        let pw = (gx1 - gx0) * res;
        let ph = (gy1 - gy0) * res;
        let mip_handles: Vec<Handle<Image>> = build_mip_chain(chunk_pixels, pw, ph, google_mode)
            .into_iter()
            .map(|img| images.add(img))
            .collect();
        let mesh_handle = meshes.add(Rectangle::new(chunk_rect.x, chunk_rect.y));
        let material = materials.add(ColorMaterial {
            // 图层透明度（ADR-0013）：作者 YAML 的 `opacity` 落在材质 alpha 上。
            color: Color::WHITE.with_alpha(layers.terrain.opacity),
            texture: mip_handles.get(mip_now as usize).cloned(),
            // V 翻转：Mesh2d + Rectangle 的默认 UV 把 image data 第 0 行映射到世界 +
            // Y 顶点，与 `apply_brush` / `isoline_overlay` 的「cell row 0 = 世界底部」
            // 约定相反。翻转后 data 行 0 落在世界底部，与笔刷和等值线描边同源——
            // 修复「VectorFlat 填充与描边呈 90° 错位 / 笔刷落点与网格点对不上」
            // （M2 提交 5f8cb73 后浮现的回归：色块模式没有描边时不可见）。
            uv_transform: Affine2::from_translation(Vec2::new(0.0, 1.0))
                * Affine2::from_scale(Vec2::new(1.0, -1.0)),
            ..default()
        });
        let entity = commands
            .spawn((
                Mesh2d(mesh_handle),
                MeshMaterial2d(material.clone()),
                Transform::from_translation(center.extend(TERRAIN_Z + layers.terrain.z_offset)),
                TerrainChunk {
                    _coord: coord,
                    current_mip: mip_now,
                    mip_handles,
                    material,
                    linear_filter: google_mode,
                },
            ))
            .id();
        chunk_entities.by_coord.insert(coord, entity);
    }

    // 4) 脏矩形已消费（只在确有写入时触碰资源，避免无谓的 changed 标记）。
    if tex.chunks_dirty.is_some() {
        tex.chunks_dirty = None;
    }
}

// ══════ ADR-0053 :: sync_terrain_field（GPU 逐像素底图，Phase 1 接线）══════
//
// 管线本体（数据打包 / 材质 / WGSL 逐像素求值）在
// `wv-world-cell-map::terrain_view::field`，且已过截图验收；本段只做**主 App
// 接线**——ADR-0053 §7 的 Phase 1「只接线、不加特性」。
//
// 唯一的接线示范是 `wv-world-cell-map/examples/terrain_field_demo.rs`；
// 本段与它的差别只有三处，都是「主 App 的数据约定与 demo 不同」造成的：
//
// | 项 | demo | 主 App（本段） |
// |---|---|---|
// | 地形数据 | 现造的合成 / fbm 512² | `active_terrain` 的真实栅格 |
// | 格 → 世界 | 1 世界单位/格，quad 居中在原点 | `grid.cell_size` × `grid.raster_origin` |
// | UV 方向 | 默认 quad（行 0 = 世界 **上**） | **V 翻转**（行 0 = 世界 **下**，见 [`field_quad_mesh`]） |
//
// ## 已知边界（本阶段刻意不做，别顺手补）
// - **小地图仍消费 CPU 烘焙纹理**：`rebuild_texture_on_mode_change` 的合成
//   照常跑（异步、不挡帧）。低分辨率 minimap 的场路径是后续项。
// - **T35 海面未接 SeaRecipe 全套**：P1 用 `TopographicPalette` 四色（照搬现状）。
// - **T25/T26 折线（河 / 路）未接**：走 `build_material`（空折线纹理，解码后
//   距离饱和 → shader 不画线）。接 `build_material_with_lines` 属下一轮。
// - **图层 opacity 未接**：`TerrainFieldMaterial` 没有 alpha 通道，Phase 1 只跟
//   `LayerFrame.terrain.visible` 与 `z_offset`。

/// 场渲染的缓存键——**绝不许含任何连续量**。
///
/// ## 为什么键里不能有 scale / `px_per_cell`
///
/// 本项目已经在同一口坑里栽过**两次**：网格的 `GridKey.scale_bits`、锚点的连续
/// `spacing`。smooth zoom 每帧 ×1.02，键每帧都变 = 每帧重建整层（实测 2050 条
/// 网格链 + 全量网格上传）。
///
/// 缩放对场只影响 uniform（`shape.x` = `px_per_cell`、`pad0.x` = 域扭曲幅度），
/// **一次 ping-pong 都不该发生**。故键里只有「离散的结构量 + 内容指纹」。
#[derive(Clone, PartialEq, Eq)]
pub struct FieldKey {
    /// 栅格来源图（`active_terrain` 给出的图，沿 `parent` 向上继承）。
    pub map: Option<wv_core::Id>,
    /// 地形内容指纹（[`terrain_data_hash`]，与等值线 / 装饰缓存同源）。
    /// 只有**编辑提交**（或换图）才会变。
    pub terrain_hash: u64,
    /// 取色分支：`Topographic → BiomeRaw`（ADR-0051 §5），其余 → `Themed`。
    pub branch: ViewBranch,
    /// 主题色板 id。换主题只换 uniform，**不必重打数据纹理**。
    pub palette_id: &'static str,
}

/// 场渲染的资产与缓存键（单 quad，最多一份材质）。
///
/// 资产句柄集中在此，是为了「关掉 → 全部 `Assets::remove`」能一处收口
/// ——开关连续 20 轮不许有资产累积（验收口径 2）。
#[derive(Resource, Default)]
pub struct TerrainFieldRender {
    /// 场 quad 实体（`None` = 未建 / 已回收）。
    pub quad: Option<Entity>,
    /// quad 的 mesh 句柄（单位方片，跨缩放恒定，建一次终身复用）。
    pub mesh: Option<Handle<Mesh>>,
    /// 当前材质句柄（重建时整份替换，旧的走 `remove`）。
    pub material: Option<Handle<TerrainFieldMaterial>>,
    /// 上次构建用的键（`None` = 尚未构建）。
    pub key: Option<FieldKey>,
    /// 上次写进 uniform 的 `px_per_cell`——用来判断「要不要重传 uniform」。
    /// **只做比较、不进 [`FieldKey`]**。
    last_ppc: Option<f32>,
    /// 上一份**打包结果**（三张数据纹理的字节缓冲）——增量补丁的唯一数据源。
    ///
    /// ## 为什么渲染层要留这 12 MB
    ///
    /// 留的是「上次上传给 GPU 的那份字节」，用途只有一个：给一次局部编辑
    /// （brush / 单格 / 群系分界线）**补几百个 texel**，而不是把整张图按
    /// `FieldPack::from_terrain` 重打一遍（1024² 实测 48.4 ms，其中 ~40 ms 是
    /// 海岸 EDT）。没有它，增量补丁无从比对「补哪些格」。
    ///
    /// 它**不是**世界状态：权威仍是磁盘 YAML，`wv-app` 的 `World` 是工作副本，
    /// 这里只是 GPU 上那张纹理的 CPU 影子副本（关掉场渲染即随
    /// [`teardown_field`] 一起释放）。
    ///
    /// 换图 / 幅面变化 / 编码变化时**必须整份替换**（见 `sync_terrain_field`
    /// 的「可增量」判据）——拿上一张图的字节去补这一张，是错位的静默画错。
    pack: Option<wv_world_cell_map::terrain_view::field::FieldPack>,
}

/// 场渲染的计数器（QA 断言口径：跨机器同值、只增不减）。
///
/// 三个字段对应验收 1–3：
/// - `field_repacks`：数据纹理重打次数。稳态 60 帧必须 **Δ == 0**；笔刷提交
///   一次必须**恰好 +1**（指纹 + 编辑信号共同判定，纯换色不重打）。
/// - `field_quads`：场 quad 实体数（开 1 / 关 0）。
/// - `field_render_on_chunk_entities`：**帧末**仍活着的 chunk 实体数
///   （场开着时必须是 0——两条路径不许同时上屏）。本系统排在
///   `sync_terrain_chunks` 之后，故读到的是本帧 spawn / despawn 之后的真值。
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct FieldRenderStats {
    /// 数据纹理**全量重打**次数（累计）。
    pub field_repacks: u64,
    /// 数据纹理**增量补丁**次数（累计）。
    ///
    /// 与 `field_repacks` 是**互斥**的两条路径：一次编辑要么重打要么补丁，
    /// 不会同时 +1（补丁失败会退回重打，那时只有 `field_repacks` 动）。
    /// 验收口径：一笔 brush 必须 **`field_patches` +1 且 `field_repacks` 不变**；
    /// 换图 / 载档 / `mark_all_dirty` 必须 `field_repacks` +1。
    pub field_patches: u64,
    /// 场 quad 实体数（0 或 1）。
    pub field_quads: usize,
    /// 场开着时残留的 chunk 实体数（应为 0）。
    pub field_render_on_chunk_entities: usize,
    /// 最近一次重打的**墙钟毫秒**（`Instant` 实测，含打包 + 纹理建资产）。
    pub last_repack_ms: f32,
    /// 最近一次补丁的**墙钟毫秒**（含窗口 EDT + 三张纹理的行段拷贝）。
    pub last_patch_ms: f32,
    /// 最近一次重打上传的字节数（三张数据纹理 + 折线通道）。
    pub last_repack_bytes: usize,
}

/// 场 quad 的 mesh：**单位方片 + 显式 V 翻转的 UV**。
///
/// ## 为什么不能照抄 demo 的默认 UV（本项目的高危点）
///
/// 坑不在「谁对谁错」，在**两套数据约定不同**：
///
/// | | 数据行 0 在哪 | 依据 |
/// |---|---|---|
/// | demo | 世界 **+y**（北 / 上） | demo 自己按 `g/2 − cy` 换算格→世界 |
/// | 主 App | 世界 **y=0**（南 / 下） | ADR-0034 D2「DEM 行 0 = 最南行」；`cell_pick` / `TerrainGrid::sample_world` 的 `gy = (y − origin)/cell_size` |
///
/// 而 shader 的约定是 `pos = in.uv × grid`，`pos.y` **就是数据行号**（见
/// `terrain_field.wgsl` 顶部「实测探针」表：`world.y ↑ ⇒ in.uv.y ↓`）。
/// 于是：
///
/// - demo 的做法（默认 quad）→ 行 0 落在世界**上**，对主 App 是**整幅上下镜像**；
/// - chunk 路径（回退路径）用 `ColorMaterial.uv_transform` 做 V 翻转，把行 0
///   落到世界**下**——`sync_terrain_chunks` 里那段「V 翻转」注释就是这件事。
///
/// 自定义 `Material2d` **没有** `uv_transform` 这个 uniform（那是 `ColorMaterial`
/// 的字段），故翻转只能落在**顶点属性**上。`in.uv.y` 随世界 y 增大而增大 ⇒ 行 0
/// 落在世界下，与 chunk 路径、笔刷落点、等值线描边同源。
///
/// 180° 旋转（而不是镜像）是这条路线历史上真实发生过的缺陷（SPEC 追记九：
/// `uv0 = (1−u, 1−v)` 造成整幅旋转 180°，因程序化地形近似对称，P1–P4 全部
/// 截图验收都漏过）——故这里把 UV 写成**显式顶点属性**，不靠「默认值 + 猜」。
fn field_quad_mesh() -> Mesh {
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    // 顶点顺序与 Bevy `Rectangle` 一致（左下 → 右下 → 右上 → 左上，双三角）：
    // 世界 y 向上为正，故 `y = −0.5` 是世界**下**边。
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    );
    // uv.y = 0 在世界**下**边 ⇒ 数据行 0 落在世界下（见上方长注释）。
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    );
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// 回收场 quad 的 mesh 资产（调用前实体必须已 despawn）。
fn reclaim_field_mesh(commands: &mut Commands, mesh: Handle<Mesh>) {
    commands.queue(move |world: &mut World| {
        if let Some(mut assets) = world.get_resource_mut::<Assets<Mesh>>() {
            assets.remove(&mesh);
        }
    });
}

/// 回收场材质**及其四张数据纹理**。
///
/// 材质句柄只是入口：真正的显存是 `height/coast/attr/line_dash` 四张图
/// （1024² 图 = 8 MB）。只 remove 材质会让纹理永远留在 `Assets<Image>` 里
/// ——开关 20 轮 = 160 MB。故先把句柄抄出来，再在同一个命令里一并 remove。
fn reclaim_field_material(
    commands: &mut Commands,
    material: Handle<TerrainFieldMaterial>,
    materials: &Assets<TerrainFieldMaterial>,
) {
    let mut textures: [Option<Handle<Image>>; 4] = [const { None }; 4];
    if let Some(mat) = materials.get(&material) {
        textures = [
            Some(mat.height.clone()),
            Some(mat.coast.clone()),
            Some(mat.attr.clone()),
            Some(mat.line_dash.clone()),
        ];
    }
    commands.queue(move |world: &mut World| {
        if let Some(mut assets) = world.get_resource_mut::<Assets<TerrainFieldMaterial>>() {
            assets.remove(&material);
        }
        if let Some(mut images) = world.get_resource_mut::<Assets<Image>>() {
            for texture in textures.into_iter().flatten() {
                images.remove(&texture);
            }
        }
    });
}

/// 场路径退场：quad despawn + mesh / 材质 / 数据纹理全部回收 + 清键。
///
/// 三条退场理由共用这一段（用户关开关 / 图层被隐藏 / 世界还没有任何栅格地形），
/// 保证「退场」在任何路径下都是**同一件事**（否则资产会从某一条路漏出去）。
fn teardown_field(
    commands: &mut Commands,
    field: &mut TerrainFieldRender,
    materials: &Assets<TerrainFieldMaterial>,
    stats: &mut FieldRenderStats,
) {
    if let Some(entity) = field.quad.take() {
        commands.entity(entity).despawn();
    }
    if let Some(mesh) = field.mesh.take() {
        reclaim_field_mesh(commands, mesh);
    }
    if let Some(material) = field.material.take() {
        reclaim_field_material(commands, material, materials);
    }
    field.key = None;
    field.last_ppc = None;
    // 12 MB 的 CPU 影子副本随场一起释放：关掉场还攥着上一张图的字节，
    // 既白占内存，也会让「下次开场的第一次补丁」用上过期数据。
    field.pack = None;
    stats.field_quads = 0;
}

/// 主 App 的场渲染同步（ADR-0053 D1，Phase 1 接线 2026-10-24）。
///
/// ## 职责
/// 把「当前活动地形 + 视图状态」翻译成一张场 quad：数据变了重打纹理，视图变了
/// 只改 uniform。**不自己做任何画法决策**——颜色公式全在 WGSL 与 `FieldParams`。
///
/// ## 关键流程
/// 1. gate：`view.field_render && layers.terrain.visible`，不满足 → [`teardown_field`]；
/// 2. 取 `active_terrain`（沿 `parent` 向上继承，与 chunk 路径同源）；
/// 3. 算 [`FieldKey`]：结构量直接读，内容指纹**只在可能有变化的帧**算
///    （`chunks_dirty` 待消费 / 结构变了 / 首次）——与 `sync_isoline_overlay`
///    同款纪律，稳态零成本；
/// 4. 键的内容部分变了 → 重打 [`FieldPack`] + 换材质（`Instant` 计量毫秒）；
///    只有分支 / 色板变了 → 只重写 uniform；
/// 5. quad 的变换每帧校正（`raster_origin` / `cell_size` 随图变）。
///
/// ## 对外接口
/// - 资源 [`TerrainFieldRender`]（资产与键）、[`FieldRenderStats`]（计数器）；
/// - 读取 `Res<TerrainChunkEntities>`：只在计数器里反映「场开着时还有多少块」。
///
/// ## 为什么排在 `sync_terrain_chunks` **之后**
///
/// 1. **计数器口径**：`field_render_on_chunk_entities` 必须等于**帧末**活着的
///    chunk 数（QA 把它与 `TerrainChunk` 组件数逐帧比对，两个口径不许互相否定）。
///    块的 despawn / spawn 都由 chunk 系统在**本帧**完成，故本系统读到的
///    `TerrainChunkEntities` 才是帧末真值。排在前面读到的是上一帧的残留。
/// 2. **不抢信号**：编辑信号取自**变更戳**（`WorldProjection` / `TerrainTexture`
///    的 `is_changed`），不依赖 `tex.chunks_dirty` 的消费顺序——那个旗标由
///    chunk 系统照旧消费清零，本系统**一行都不碰**。
/// 3. **独占性不靠顺序**：场开着时 chunk 系统自己早退（回退开关在它函数开头），
///    故「两条路径同帧只活一条」与谁先谁后无关。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_field(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainFieldMaterial>>,
    mut tex: ResMut<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    chunk_entities: Res<TerrainChunkEntities>,
    mut field: ResMut<TerrainFieldRender>,
    mut stats: ResMut<FieldRenderStats>,
    mut q_quads: Query<&mut Transform>,
) {
    // 帧末块数：本系统排在 `sync_terrain_chunks` 之后，块的 spawn / despawn
    // 都已在**本帧**完成（`by_coord` 是立即写，实体是延迟命令但同一帧落地），
    // 故这里读到的就是「这一帧结束时还活着几块」。
    stats.field_render_on_chunk_entities = chunk_entities.len();

    if !(view.field_render && layers.terrain.visible) {
        teardown_field(&mut commands, &mut field, &materials, &mut stats);
        return;
    }

    // 世界还没有任何带栅格的地形（启动早期 / 空世界）：无数据可求值，退场。
    let Some((source, data)) = active_terrain(&projection.world, drill.current()) else {
        teardown_field(&mut commands, &mut field, &materials, &mut stats);
        return;
    };

    // ── 分支：CellMap 看原始群系色（ADR-0051 §5），其余看主题色板 ──
    let branch = if view.mode == crate::state::ViewMode::Topographic {
        ViewBranch::BiomeRaw
    } else {
        ViewBranch::Themed
    };
    let map = Some(source.id.clone());
    let palette_id: &'static str = view.active_theme.id;

    let cached_hash = field.key.as_ref().map(|k| k.terrain_hash);
    let structure_changed = match &field.key {
        Some(k) => k.map != map || k.branch != branch || k.palette_id != palette_id,
        None => true,
    };
    // 「可能有编辑」的变更戳：世界被改（笔刷 / 撤销 / 换图 / 载入）或纹理被写过
    // （`flush_terrain_texture` 的 patch、整图重建）。
    // **不用 `tex.chunks_dirty`**：本系统排在 chunk 之后，那旗标已被消费。
    let edit_signal = projection.is_changed() || tex.is_changed();
    // 内容指纹 1M 格 ≈3 ms：只在「可能会有变化」的帧算（与 `sync_isoline_overlay`
    // 同款纪律），稳态零成本。
    let needs_hash = structure_changed || edit_signal || cached_hash.is_none();
    let terrain_hash = if needs_hash {
        // **必须用 `pack_content_hash`（裸 height/surface/biome），不能用
        // `terrain_data_hash`**：后者折叠的是 `display_kind` 显示槽位，而 attr
        // 纹理 G 通道存的是裸 biome 字节——两者不同源，会漏打（详见该函数文档）。
        wv_world_cell_map::terrain_view::field::pack_content_hash(data)
    } else {
        cached_hash.unwrap_or_default()
    };

    // ── 世界幅面：与 chunk 路径同一口径（`raster_origin` + 格数 × `cell_size`）──
    let cell_size = grid.cell_size.max(1e-6);
    let cols = data.width.max(1) as f32;
    let rows = data.height.max(1) as f32;
    let extent = Vec2::new(cols, rows) * cell_size;
    let center = grid.raster_origin + extent * 0.5;
    let z = TERRAIN_Z + layers.terrain.z_offset;
    // 屏幕像素 / 格：`ViewState.scale` 的语义是「世界单位 / 屏幕像素」，
    // 故 ppc = 格的世界尺寸 / scale（demo 里一格 = 1 世界单位，同一个式子）。
    let px_per_cell = (cell_size / view.scale.max(1e-6)).max(1e-4);

    let content_changed =
        cached_hash != Some(terrain_hash) || field.key.as_ref().is_some_and(|k| k.map != map);

    // 重打判据：**只看内容指纹**（没有 quad / 材质缺失 / 指纹不等）。
    //
    // ## 为什么 `edit_signal` 不能进这个判据（2026-10-24 修，N1）
    // 曾经的判据是 `... || edit_signal`，理由是「多打一次只花 ~10ms，漏打是
    // 画了不显示」——代价不对称的直觉。但它有两个实测后果：
    // 1. **一次编辑打两遍**：`edit_signal` 含 `tex.is_changed()`，而 chunk 系统
    //    排在本系统之后、会擦 `chunks_dirty`，那次写把 changed tick 推到下一帧
    //    ⇒ N+1 再命中一次（QA 实测 `left: 3, right: 2`）；
    // 2. **选区变更也重打**：`projection.is_changed()` 在「选中某个 region /
    //    place」时同样置真 ⇒ 点一下地图就付一次全量重打（48ms @1024²）。这正是
    //    `world_projection.rs` 里记过的「selection 写入置真 = 实机 1 FPS 根因」
    //    的同款陷阱。
    //
    // 正解是**比内容，不比通知**：指纹相同 ⇒ 打包结果逐字节相同 ⇒ 重打只是把
    // 同一份数据再上传一遍，没有任何视觉收益。`edit_signal` 现在唯一的职责是
    // 上面的 `needs_hash`——省掉「没变时也算一遍 hash」的开销，不许自己成为
    // 触发条件。（同类教训第三次：网格键 `scale_bits` → 锚点键连续 `spacing`
    // → 这里的 change tick。）
    if field.quad.is_none() || field.material.is_none() || content_changed {
        let started = std::time::Instant::now();
        // ── ① 先试增量补丁：局部编辑只碰脏矩形，不重打整张图 ──
        //
        // 可增量的三个条件（缺一就走下面的全量重打）：
        // 1. **同一张图**：`field.key.map` 与本次 `map` 相同——拿上一张图的
        //    字节补这一张，幅面碰巧相同就会静默画错；
        // 2. **有可信脏区**：`tex.field_dirty` 由 `mark_dirty` / `mark_all_dirty`
        //    写入。`None` 意味着「本帧没有局部信号」（载档 / 撤销全表 / 首次），
        //    不许猜一个矩形；
        // 3. **脏区不太大**：超过全场 1/4 时窗口 EDT 已接近全场规模，重建材质
        //    反而更省，也少一条「半新半旧」的路径。
        //
        // `bypass_change_detection`：消费脏区**不是**「纹理内容变了」，若照常
        // 标记 changed，下一帧 `edit_signal` 会假真一次、白算一遍内容指纹
        // （N1 那次「一次编辑打两遍」就是同款误信号）。
        let mut patched = false;
        if content_changed && field.material.is_some() {
            let same_map = field.key.as_ref().is_some_and(|k| k.map == map);
            let rect = tex
                .bypass_change_detection()
                .take_field_dirty()
                .map(CellRect::from_bounds);
            let full_area = u64::from(data.width.max(1)) * u64::from(data.height.max(1));
            if let (true, Some(rect)) = (same_map, rect) {
                if !rect.is_empty() && rect.area() * 4 <= full_area {
                    let handle = field.material.clone().expect("刚判过 is_some");
                    if let Some(pack) = field.pack.as_mut() {
                        if pack.patch_region(data, rect) {
                            if let Some(material) = materials.get(&handle) {
                                patched = patch_material_images(&mut images, material, pack, rect);
                            }
                        }
                    }
                    if patched {
                        let ms = started.elapsed().as_secs_f32() * 1000.0;
                        stats.field_patches += 1;
                        stats.last_patch_ms = ms;
                        // `last_repack_bytes` 不动：它记的是「最近一次**全量**重打的
                        // 字节数」，混进补丁字节会让这条诊断读数自相矛盾。
                        info!(
                            "地形场：增量补丁 {}×{} 格 @({}, {})（{:.2} ms）",
                            rect.width(),
                            rect.height(),
                            rect.x0,
                            rect.y0,
                            ms
                        );
                    }
                }
            }
        }

        if !patched {
            // ── ② 全量重打：三张数据纹理全量重建（1024² = 12 MB）──
            let pack = FieldPack::from_terrain(data);
            let mut params = default_params(
                [pack.width as f32, pack.height as f32],
                px_per_cell,
                branch,
                FieldPalette::default(),
            );
            // pack 与 shader 的解码开关必须同源（唯一耦合点，见 `set_coast_encoding`）。
            set_coast_encoding(&mut params, pack.encoding);
            let material = materials.add(build_material(&mut images, &pack, params));

            if let Some(entity) = field.quad {
                // 实体留着，只换材质——mesh 是单位方片，跨图跨缩放恒定。
                commands
                    .entity(entity)
                    .insert(MeshMaterial2d(material.clone()));
            } else {
                let mesh = match field.mesh.clone() {
                    Some(mesh) => mesh,
                    None => {
                        let mesh = meshes.add(field_quad_mesh());
                        field.mesh = Some(mesh.clone());
                        mesh
                    }
                };
                let entity = commands
                    .spawn((
                        Mesh2d(mesh),
                        MeshMaterial2d(material.clone()),
                        Transform::from_translation(center.extend(z))
                            .with_scale(extent.extend(1.0)),
                    ))
                    .id();
                field.quad = Some(entity);
            }
            if let Some(previous) = field.material.replace(material) {
                reclaim_field_material(&mut commands, previous, &materials);
            }
            // 留一份 CPU 影子副本给后续的增量补丁（见 `TerrainFieldRender::pack`）。
            field.pack = Some(pack);
            // uniform 已随新材质写入，记下当前 ppc 免得下面再传一次。
            field.last_ppc = Some(px_per_cell);

            stats.field_repacks += 1;
            stats.last_repack_ms = started.elapsed().as_secs_f32() * 1000.0;
            let pack = field.pack.as_ref().expect("刚写入");
            stats.last_repack_bytes =
                pack.height_tex.len() + pack.coast_tex.len() + pack.attr_tex.len();
            info!(
                "地形场：重打 {}×{} 格（{:.2} MB，{:.2} ms）ppc={:.2} 分支={:?}",
                pack.width,
                pack.height,
                stats.last_repack_bytes as f32 / (1024.0 * 1024.0),
                stats.last_repack_ms,
                px_per_cell,
                branch
            );
        }
    }

    // ── 变换每帧校正：换图 / 换格距都会改 `raster_origin` 与幅面 ──
    if let Some(entity) = field.quad {
        if let Ok(mut transform) = q_quads.get_mut(entity) {
            let translation = center.extend(z);
            if transform.translation != translation || transform.scale.truncate() != extent {
                transform.translation = translation;
                transform.scale = extent.extend(1.0);
            }
        }
    }

    // ── uniform：缩放是**连续量**，只在这里落地（不进键）──
    let ppc_changed = field
        .last_ppc
        .is_none_or(|last| (last - px_per_cell).abs() > 1e-6);
    if (ppc_changed || structure_changed) && field.material.is_some() {
        let handle = field.material.clone().expect("刚判过 is_some");
        if let Some(mut material) = materials.get_mut(&handle) {
            // shape: x = px_per_cell / y = 分支 / z = z_exag / w = grad_step（后两者
            // 由 `default_params` 填锁值 T15/T14，缩放不动它们）。
            material.params.shape[0] = px_per_cell;
            material.params.shape[1] = match branch {
                ViewBranch::BiomeRaw => 0.0,
                ViewBranch::Themed => 1.0,
            };
            // T19 域扭曲幅度（P1 预留槽 `pad0.x`）：随缩放重算。
            material.params.pad0[0] = warp_amp(px_per_cell);
            // 主题色板（T35）：P1 用 `TopographicPalette` 四色的默认转录——
            // 与 CPU 回退路径同色族，不同色就是回归（`FieldPalette::default` 的
            // 文档写着逐值抄录日期与出处）。
            let palette = FieldPalette::default();
            let rgba = |c: LinearRgba| Vec4::new(c.red, c.green, c.blue, c.alpha);
            material.params.theme = [
                rgba(palette.water),
                rgba(palette.land_low),
                rgba(palette.land_mid),
                rgba(palette.land_high),
            ];
            material.params.ink = rgba(palette.ink);
            material.params.river_ink = rgba(palette.river_ink);
            material.params.paper = rgba(palette.paper);
            field.last_ppc = Some(px_per_cell);
        }
    }

    field.key = Some(FieldKey {
        map,
        terrain_hash,
        branch,
        palette_id,
    });
    stats.field_quads = usize::from(field.quad.is_some());
}

// ══════ from src/region_overlay.rs :: sync_region_overlay_index ══════

/// 求值区域覆盖物索引（每帧检查变更，变了才重算）。
///
/// **重算条件**：世界变更 / 钻取换图 / 栅格换图 / 主题切换。
/// 纯相机运动**零工作**（同路网缓存纪律）。
pub fn sync_region_overlay_index(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut index: ResMut<RegionOverlayIndex>,
) {
    let map_id = drill.current().cloned();
    let overlay_rules = conv_region_set(&view.active_theme.region_overlays);
    let (Some(map_id), Some((w, h))) = (map_id, terrain_dims(&projection.world, drill.current()))
    else {
        index.entries.clear();
        index.cell_owner.clear();
        index.missing_cells.clear();
        index.missing_shapes.clear();
        return;
    };
    let stale = projection.is_changed()
        || drill.is_changed()
        || grid.is_changed()
        || view.active_theme.id != index.theme_palette
        || index.map_id.as_deref() != Some(map_id.as_str());
    if !stale {
        return;
    }
    // 两套原点的差：区域多边形是**图内局部 wu**，地形格属于**栅格所在的图**
    // （子图上不是同一张图）——换算必须带这个偏移。
    let raster_to_map = grid.map_origin - grid.raster_origin;
    *index = build_index(
        &projection.world.regions,
        &map_id,
        grid.cell_size,
        w,
        h,
        &overlay_rules,
        raster_to_map,
    );
    index.map_id = Some(map_id.as_str().to_string());
    index.theme_palette = view.active_theme.id;
}

// ══════ from src/region_overlay.rs :: sync_missing_overlay_markers ══════
/// 缺失覆盖物标记系统：白底红斜线**接管**该区域。
///
/// - **三档 LOD 全画**（不可被降档隐藏：装饰在 Nation 档本就不画，缺失标记
///   若跟着降档，作者切到全局视图反而在最需要发现时看不到）；
/// - **固定诊断色**（不随主题色板）；
/// - 绘制在装饰（0.075）之上、道路（0.09）之下；
/// - 底层装饰符号在该区域**跳过**（白底接管，见 `RegionOverlayIndex::is_missing_cell`）。
#[allow(clippy::too_many_arguments)]
pub fn sync_missing_overlay_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    index: Res<RegionOverlayIndex>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    recipe: Res<RenderRecipe>,
    layers: Res<LayerFrame>,
    mut assets: ResMut<MissingMarkerAssets>,
) {
    // 2026-10-22 内容矩阵：区域层从 TerrainDecor 拆到独立 pass（ADR-0051）。
    // CellMap（Topographic）：nature_overlay 放行 Nature 类（判定入口唯一）。
    let enabled = region_layer_visible(
        view.mode,
        view.nature_overlay,
        wv_core::model::region::RegionKind::Nature,
        recipe.enabled(crate::recipe::PassId::RegionLayers),
        layers.terrain.visible,
    );
    if !enabled || index.missing_shapes.is_empty() {
        for e in assets.entities.drain(..) {
            commands.entity(e).despawn();
        }
        assets.fingerprint = 0;
        return;
    }
    // 指纹：形状内容不变则不重建（相机平移零工作）。
    let fp = shapes_fingerprint(&index.missing_shapes);
    if fp == assets.fingerprint && !assets.entities.is_empty() {
        return;
    }
    for e in assets.entities.drain(..) {
        commands.entity(e).despawn();
    }
    assets.fingerprint = fp;

    // 斜纹贴图 + 材质（一次）。
    if assets.texture.is_none() {
        assets.texture = Some(hatch_texture(&mut images));
    }
    let texture = assets.texture.clone().expect("刚建");
    if assets.material.is_none() {
        // Bevy 0.19：ColorMaterial 无 `from_texture`，直接构造（贴图 + 全白染色）。
        let mat = ColorMaterial {
            color: Color::WHITE,
            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
            uv_transform: bevy::math::Affine2::default(),
            texture: Some(texture.clone()),
        };
        assets.material = Some(materials.add(mat));
    }
    let material = assets.material.clone().expect("刚建");

    let map_origin = Vec3::new(grid.map_origin.x, grid.map_origin.y, 0.0);
    for shape in &index.missing_shapes {
        if shape.outer.len() < 6 {
            continue;
        }
        let hole_refs: Vec<&[f32]> = shape.holes.iter().map(|h| h.as_slice()).collect();
        let mesh = build_hatch_mesh(&shape.outer, &hole_refs, HATCH_PERIOD_WU);
        let entity = commands.spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(map_origin + Vec3::Z * MISSING_MARKER_Z),
        ));
        assets.entities.push(entity.id());
    }
}

// ══════ from crates/wv-world-cell-map/src/terrain_view/grid_lines.rs :: sync_grid_lines ══════
/// 同步逐格网格叠加层（2026-10-23 重写：shader 双档网格 + 缩放零重建）。
///
/// ## 与原实现的三处差别（都是用户报的缺陷）
/// 1. **不再整层退场**：旧版 `grid_line_fade(cell_size / scale) <= 0` 即 despawn，
///    而世界级 fit（`scale 5.12`、`cell_size 4.0` → 格距 0.78px）恒为 0 →
///    「开了开关看不到」。现在格线由 fragment shader 按密度双档绘制，
///    CPU 侧只管「开关 + 图层 + 配方」三个 gate。
/// 2. **缩放不重建**：旧 `GridKey` 含 `scale_bits`，滚轮每档都整层重建
///    （2050 条链 + 全量网格上传）。新键只有 `size/height/cell_size/origin/
///    palette`，缩放**不碰 CPU**。重建计数见 `GridLinesOverlay::rebuilds`。
/// 3. **recipe 收口**：旧系统完全不读 `RenderRecipe`，表里 `Topographic` 行写
///    `false` 却照样画——表在骗人。现在 gate 读 `PassId::GridLines`，
///    且 Topographic 行已改为 `true`（CellMap 工作台要用网格定位，用户指令）。
///
/// 网格跟随 Terrain 图层的可见与透明度（ADR-0013）：作者把地形藏了，格线
/// 还浮着只会误导「这里可以画」。
pub fn sync_grid_lines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridLinesMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    mut overlay: ResMut<GridLinesOverlay>,
) {
    if !layers.terrain.visible
        || !grid_lines_visible(&view)
        || !recipe.enabled(PassId::GridLines)
        || tex.size == 0
        || tex.height == 0
    {
        overlay.despawn(&mut commands);
        return;
    }
    let key = GridKey {
        size: tex.size,
        height: tex.height,
        cell_bits: grid.cell_size.to_bits() as u64,
        origin: (
            grid.raster_origin.x.to_bits() as u64,
            grid.raster_origin.y.to_bits() as u64,
        ),
        palette: view.active_theme.id,
    };
    let width = tex.size as f32 * grid.cell_size;
    let height = tex.height as f32 * grid.cell_size;
    let ink = conv_palette(&view.active_theme.palette).grid_line;
    // 墨色进 uniform 必须是**线性**值（sRGB 值直塞会被再亮一次，
    // `field/material.rs` 的既有血书），算一次复用。
    let ink_linear = ink.to_linear();
    let base_alpha = GRID_LINE_BASE_ALPHA * layers.terrain.opacity.clamp(0.0, 1.0);
    let z = GRID_Z + layers.terrain.z_offset;
    // 幅面中心（矩形 mesh 以中心为锚点；shader 用世界坐标反推格坐标，
    // 所以变换与 uniform 的 `geo.zw` 必须同为真实栅格原点）。
    let center = Vec3::new(
        grid.raster_origin.x + width * 0.5,
        grid.raster_origin.y + height * 0.5,
        z,
    );

    if overlay.key.as_ref() == Some(&key) && overlay.entity.is_some() {
        // 快路径：结构性输入没变。**缩放在这里跑完就返回**——键里没有 scale，
        // 一次 PartialEq 之后什么都不做（uniform 也不必改：线宽/格距由
        // shader 的 fwidth 从世界坐标导数推出，与 scale 无关）。
        overlay.refresh_ink(&mut materials, ink_linear, base_alpha);
        if layers.is_changed() {
            if let Some(entity) = overlay.entity {
                commands
                    .entity(entity)
                    .insert(Transform::from_translation(center));
            }
        }
        return;
    }

    // 结构性重建：幅面 / 格径 / 原点 / 主题变了才到这里（1024² 实测
    // debug 0.74ms / release 0.07ms，而缩放**不再**触发它）。
    let mesh = meshes.add(Rectangle::new(width, height));
    let material = materials.add(GridLinesMaterial {
        params: grid_params(grid.cell_size, grid.raster_origin, ink_linear, base_alpha),
    });
    overlay.despawn(&mut commands);
    let entity = commands
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(center),
        ))
        .id();
    overlay.entity = Some(entity);
    overlay.mesh = Some(mesh);
    overlay.material = Some(material);
    overlay.ink_applied = Some([
        ink_linear.red,
        ink_linear.green,
        ink_linear.blue,
        base_alpha,
    ]);
    overlay.rebuilds += 1;
    overlay.key = Some(key);
}

// ══════ from crates/wv-world-cell-map/src/terrain_view/grid_lines.rs :: grid_lines_visible ══════
/// 网格可见性：单由 `view.grid_lines` 控制（2026-10-15 与笔刷解耦；
/// 2026-10-16 字段搬入 `ViewState`，开关 UI 上地图工具条）。
pub fn grid_lines_visible(view: &ViewState) -> bool {
    view.grid_lines
}

// ══════ from crates/wv-world-cell-map/src/cell_selection.rs :: sync_cell_selection ══════
/// 每帧同步选中高亮的 5 个实体。
///
/// gate = `recipe.enabled(CellSelection) ∧ edit_cell.selected.is_some()`；
/// 不满足时全 despawn（与笔刷光标的「切走即清」同口径，无残留）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn sync_cell_selection(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    recipe: Res<crate::recipe::RenderRecipe>,
    edit_cell: Res<EditCellState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut entities: ResMut<CellSelectionEntities>,
) {
    let Some((gx, gy)) = edit_cell
        .selected
        .filter(|_| recipe.enabled(PassId::CellSelection))
    else {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    };
    let (center, size) = cell_world_rect(&grid, gx, gy);
    let accent = crate::theme::colors::ACCENT_SELECT;
    // 世界厚度 = 屏幕恒定 2px（策略 A，与笔刷光标一致）。
    let thickness = THICKNESS_PX * view.scale;

    // ── mesh：只在尺寸变化时重建（其余帧复用句柄，避免每帧 GPU 重传）──
    if entities.last_size != size {
        entities.release_assets(&mut meshes);
        entities.fill_mesh = Some(meshes.add(Rectangle::new(size.x, size.y)));
        let half = size * 0.5;
        let t2 = thickness * 0.5;
        let edge_defs: [(Vec2, Vec2); 4] = [
            (Vec2::new(0.0, -half.y + t2), Vec2::new(size.x, thickness)),
            (Vec2::new(0.0, half.y - t2), Vec2::new(size.x, thickness)),
            (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, size.y)),
            (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, size.y)),
        ];
        for (i, (_, edge_size)) in edge_defs.into_iter().enumerate() {
            entities.edge_meshes[i] = Some(meshes.add(Rectangle::new(edge_size.x, edge_size.y)));
        }
        entities.last_size = size;
    }

    let half = size * 0.5;
    let t2 = thickness * 0.5;
    let edge_defs: [(Vec2, Vec2); 4] = [
        (Vec2::new(0.0, -half.y + t2), Vec2::new(size.x, thickness)),
        (Vec2::new(0.0, half.y - t2), Vec2::new(size.x, thickness)),
        (Vec2::new(-half.x + t2, 0.0), Vec2::new(thickness, size.y)),
        (Vec2::new(half.x - t2, 0.0), Vec2::new(thickness, size.y)),
    ];

    // ── 材质：只建一次 ──
    if entities.fill_material.is_none() {
        entities.fill_material =
            Some(materials.add(ColorMaterial::from_color(accent.with_alpha(FILL_ALPHA))));
    }
    for slot in &mut entities.edge_materials {
        if slot.is_none() {
            *slot = Some(materials.add(ColorMaterial::from_color(accent)));
        }
    }

    // ── 实体：建一次，之后只挂句柄 + 更新 Transform ──
    let fill_mesh = entities.fill_mesh.clone().expect("尺寸已定,mesh 已建");
    let fill_mat = entities.fill_material.clone().expect("材质已建");
    match entities.fill {
        Some(e) => {
            commands.entity(e).insert((
                Transform::from_translation(center.extend(SELECTION_Z)),
                Mesh2d(fill_mesh),
                MeshMaterial2d(fill_mat),
            ));
        }
        None => {
            entities.fill = Some(
                commands
                    .spawn((
                        Transform::from_translation(center.extend(SELECTION_Z)),
                        Mesh2d(fill_mesh),
                        MeshMaterial2d(fill_mat),
                    ))
                    .id(),
            );
        }
    }

    // ── 四条边：下 (0) / 上 (1) / 左 (2) / 右 (3) ──
    for (i, (offset, _)) in edge_defs.into_iter().enumerate() {
        let Some(mesh) = entities.edge_meshes[i].clone() else {
            continue;
        };
        let Some(mat) = entities.edge_materials[i].clone() else {
            continue;
        };
        let tf = Transform::from_translation((center + offset).extend(SELECTION_Z + 0.001));
        match entities.edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(mat)));
            }
            None => {
                entities.edges[i] =
                    Some(commands.spawn((tf, Mesh2d(mesh), MeshMaterial2d(mat))).id());
            }
        }
    }
}

// ══════ from crates/wv-world-cell-map/src/cell_selection.rs :: sync_cell_stroke ══════
/// 每帧同步 **待落笔触色块**（[ADR-0052] D3 / §9.5 四态视觉分工）。
///
/// gate = 笔触活跃 ∧ 笔触格非空；不满足时全 despawn。
///
/// 视觉：红橙 `STROKE_PENDING` 半透填充（alpha 0.26，**不脉动**）——
/// 表示「已采样但尚未写入世界」的格。
///
/// Z 低于选中（`STROKE_Z = 0.85 < SELECTION_Z = 0.9`）：笔触是**未提交**
/// 暂态，选中才是已提交态（§9.5 绘制序 region → stroke → selected → hover）。
///
/// **mesh 只在格集合变化时重建**——笔触可能几百格，每帧重建会吃帧
/// （与 `sync_cell_region` 同款策略）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn sync_cell_stroke(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    session: Res<wv_world_cell_map::cell_paint::CellPaintSession>,
    grid: Res<GridConfig>,
    mut entities: ResMut<CellStrokeEntities>,
) {
    let cells = &session.stroke.cells;
    if !session.stroke.active || cells.is_empty() {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    }
    let fingerprint = cells_fingerprint(cells);
    if fingerprint != entities.last_fingerprint || entities.fill_mesh.is_none() {
        entities.release_assets(&mut meshes);
        let Some((mesh, origin, bbox)) = build_stroke_fill_mesh(cells, &grid) else {
            entities.despawn_all(&mut commands);
            return;
        };
        entities.fill_mesh = Some(meshes.add(mesh));
        entities.last_fingerprint = fingerprint;
        // 重建 mesh → 实体也要重建（mesh 换了）。
        entities.despawn_all(&mut commands);
        if entities.fill_material.is_none() {
            entities.fill_material = Some(materials.add(ColorMaterial::from_color(
                crate::theme::colors::STROKE_PENDING.with_alpha(STROKE_FILL_ALPHA),
            )));
        }
        let mesh = entities.fill_mesh.clone().expect("刚建");
        let mat = entities.fill_material.clone().expect("刚建");
        entities.fill = Some(
            commands
                .spawn((
                    Transform::from_translation(origin.extend(STROKE_Z)),
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                    // bbox 不进 Transform（mesh 顶点已是绝对世界坐标的相对偏移，
                    // 与 `sync_cell_region` 同款：mesh 原点 + 顶点偏移 = 世界位置）。
                ))
                .id(),
        );
        let _ = bbox;
        return;
    }
    // 格集合未变 → 材质与实体保持不变（避免每帧重建）。
    let Some(e) = entities.fill else {
        return;
    };
    let mesh = entities.fill_mesh.clone().expect("已建");
    let mat = entities.fill_material.clone().expect("已建");
    commands.entity(e).insert((Mesh2d(mesh), MeshMaterial2d(mat)));
}

// ══════ from crates/wv-world-cell-map/src/cell_selection.rs :: sync_cell_hover ══════
/// 每帧同步 **hover 准星**（[ADR-0052] D2 / §9.5 四态视觉分工）。
///
/// gate = `edit_cell.tool_active ∧ edit_cell.hover.is_some()`；
/// 不满足时全 despawn（无残留）。
///
/// **双色描边**（D2）：每条边两层——外层深墨 `LINE_ON_DARK`（2px）、
/// 内层纯白（1px）。深墨在下、白线在上，构成「白内线 + 深墨外晕」的
/// 地图准星观感：在水面（浅蓝）与陆地（绿/褐）两种底色上都能读出来
/// （[ADR-0052] §5 M1 验收 ③）。
///
/// **不脉动**（D2）：hover 是瞬时态，脉动会与 selected 的金色脉动混淆。
/// 脉动是 selected 独占的信号。
///
/// **Z 高于 selected**（`HOVER_Z = 0.95 > SELECTION_Z = 0.9`）：
/// hover 是「此刻光标在哪」的瞬时反馈，必须画在最上层
/// （[ADR-0052] §9.5 绘制序 region → stroke → selected → hover）。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn sync_cell_hover(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    edit_cell: Res<EditCellState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut entities: ResMut<CellHoverEntities>,
) {
    let Some((gx, gy)) = edit_cell.hover.filter(|_| edit_cell.tool_active) else {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    };
    let (center, size) = cell_world_rect(&grid, gx, gy);
    // 世界厚度 = 屏幕恒定像素 × view.scale（与选中 outline 同策略 A）。
    let outer_t = HOVER_OUTER_PX * view.scale;
    let inner_t = HOVER_INNER_PX * view.scale;
    // 准星长度 = 格的 1/3（居中十字），不与边缘描边打架。
    let cross_len = size.x * 0.34;

    // ── mesh：只在尺寸变化时重建（其余帧复用句柄）──
    if entities.last_size != size || entities.outer_h_mesh.is_none() {
        entities.release_assets(&mut meshes);
        // 外晕比内线更宽——墨晕在格沿两侧铺开，白线正好落在格沿上。
        entities.outer_h_mesh = Some(meshes.add(Rectangle::new(size.x, outer_t)));
        entities.outer_v_mesh = Some(meshes.add(Rectangle::new(outer_t, size.y)));
        entities.inner_h_mesh = Some(meshes.add(Rectangle::new(size.x, inner_t)));
        entities.inner_v_mesh = Some(meshes.add(Rectangle::new(inner_t, size.y)));
        // 准星 2 条（横 / 竖）+ 白芯 2 条（比芯短一档，露出墨头）。
        entities.cross_h_mesh = Some(meshes.add(Rectangle::new(cross_len, outer_t)));
        entities.cross_v_mesh = Some(meshes.add(Rectangle::new(outer_t, cross_len)));
        let core_len = cross_len * 0.5;
        entities.core_h_mesh = Some(meshes.add(Rectangle::new(core_len, inner_t)));
        entities.core_v_mesh = Some(meshes.add(Rectangle::new(inner_t, core_len)));
    }
    entities.last_size = size;

    // ── 材质：只建一次 ──
    if entities.outer_material.is_none() {
        entities.outer_material = Some(materials.add(ColorMaterial::from_color(
            crate::theme::colors::LINE_ON_DARK,
        )));
    }
    if entities.inner_material.is_none() {
        entities.inner_material = Some(materials.add(ColorMaterial::from_color(
            bevy::color::Color::WHITE,
        )));
    }
    let outer_mat = entities.outer_material.clone().expect("材质已建");
    let inner_mat = entities.inner_material.clone().expect("材质已建");

    let half = size * 0.5;
    let ot2 = outer_t * 0.5;
    // `inner_t` 不需要偏移量：内线直接压在格沿上（外晕才向外让半个厚度）。

    // ── 外晕 4 条边：下 (0) / 上 (1) / 左 (2) / 右 (3) ──
    // 外晕中心比格边界再外移半个墨晕厚度（`ot2`），墨线铺在格沿两侧。
    let outer_offsets: [Vec2; 4] = [
        Vec2::new(0.0, -half.y - ot2),
        Vec2::new(0.0, half.y + ot2),
        Vec2::new(-half.x - ot2, 0.0),
        Vec2::new(half.x + ot2, 0.0),
    ];
    let outer_meshes = [
        entities.outer_h_mesh.clone(),
        entities.outer_h_mesh.clone(),
        entities.outer_v_mesh.clone(),
        entities.outer_v_mesh.clone(),
    ];
    for (i, offset) in outer_offsets.into_iter().enumerate() {
        let Some(mesh) = outer_meshes[i].clone() else {
            continue;
        };
        let tf = Transform::from_translation((center + offset).extend(HOVER_Z));
        match entities.outer_edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())));
            }
            None => {
                entities.outer_edges[i] =
                    Some(commands.spawn((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone()))).id());
            }
        }
    }

    // ── 内线 4 条边：白线压格沿（比外晕更靠内半个墨晕厚度）──
    let inner_offsets: [Vec2; 4] = [
        Vec2::new(0.0, -half.y),
        Vec2::new(0.0, half.y),
        Vec2::new(-half.x, 0.0),
        Vec2::new(half.x, 0.0),
    ];
    let inner_meshes = [
        entities.inner_h_mesh.clone(),
        entities.inner_h_mesh.clone(),
        entities.inner_v_mesh.clone(),
        entities.inner_v_mesh.clone(),
    ];
    for (i, offset) in inner_offsets.into_iter().enumerate() {
        let Some(mesh) = inner_meshes[i].clone() else {
            continue;
        };
        let tf = Transform::from_translation((center + offset).extend(HOVER_Z + 0.001));
        match entities.inner_edges[i] {
            Some(e) => {
                commands
                    .entity(e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())));
            }
            None => {
                entities.inner_edges[i] =
                    Some(commands.spawn((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone()))).id());
            }
        }
    }

    // ── 准星：格中心十字（外晕层 + 白芯层）──
    // 外晕横 / 竖：格中心。
    //
    // 先把 mesh 句柄克隆出来再可变借用 slot——数组元素与整体克隆写在
    // 同一条 `for` 的初始化式里会同时可变 + 不可变借用 `entities`
    // （编译期 E0499/E0502）。逐个处理（而非一次遍历两个 slot）是为
    // 避开「同时可变借用 `entities` 两次」。
    let cross_outer_meshes = [
        entities.cross_h_mesh.clone(),
        entities.cross_v_mesh.clone(),
    ];
    for (idx, mesh) in cross_outer_meshes.into_iter().enumerate() {
        let Some(mesh) = mesh else { continue };
        let tf = Transform::from_translation(center.extend(HOVER_Z));
        let slot = &mut entities.cross_outer[idx];
        match slot {
            Some(e) => {
                commands
                    .entity(*e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())));
            }
            None => {
                *slot = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh), MeshMaterial2d(outer_mat.clone())))
                        .id(),
                );
            }
        }
    }
    // 白芯（比墨晕短一档，露不出头）。
    let cross_inner_meshes = [
        entities.core_h_mesh.clone(),
        entities.core_v_mesh.clone(),
    ];
    for (idx, mesh) in cross_inner_meshes.into_iter().enumerate() {
        let Some(mesh) = mesh else { continue };
        let tf = Transform::from_translation(center.extend(HOVER_Z + 0.002));
        let slot = &mut entities.cross_inner[idx];
        match slot {
            Some(e) => {
                commands
                    .entity(*e)
                    .insert((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())));
            }
            None => {
                *slot = Some(
                    commands
                        .spawn((tf, Mesh2d(mesh), MeshMaterial2d(inner_mat.clone())))
                        .id(),
                );
            }
        }
    }
}

// ══════ from crates/wv-world-cell-map/src/cell_flash.rs :: sync_cell_flash ══════
/// 每帧同步 **撤销白闪 / 拒收红闪**（[ADR-0052] D8-A / D3-E）。
///
/// gate = `CellFlashState.is_active(now)`；到期即全 despawn（无残留）。
///
/// ## 颜色分工
///
/// - `Undo` → `UNDO_FLASH`（纯白，`0xFFFFFF`）；
/// - `Reject` → `UI_UNREACHABLE`（红，`0xB5453A`）——与
///   `UI_UNREACHABLE` 同源：语义都是「这一处不可用 / 没生效」。
///
/// ## Z 最顶层
///
/// `FLASH_Z = 1.0` **高于 hover（`HOVER_Z = 0.95`）**——闪是「刚刚发生」
/// 的强信号，必须压过所有常驻视觉（[ADR-0052] §9.9「白闪 z 最顶层
/// 250ms 短促」）。
///
/// ## mesh 只在格集合变化时重建
///
/// 闪期间格集合不变（撤销已定局），故 mesh 复用；**alpha 每帧改**
/// （材质句柄内的颜色随时间衰减）。
pub fn sync_cell_flash(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    flash: Res<CellFlashState>,
    grid: Res<GridConfig>,
    time: Res<Time>,
    mut entities: ResMut<CellFlashEntities>,
) {
    let now = time.elapsed_secs();
    if !flash.is_active(now) {
        entities.despawn_all(&mut commands);
        entities.release_assets(&mut meshes);
        return;
    }
    let fingerprint = cells_fingerprint(&flash.cells);
    if fingerprint != entities.last_fingerprint || entities.fill_mesh.is_none() {
        entities.release_assets(&mut meshes);
        let Some((mesh, origin)) = build_flash_mesh(&flash.cells, &grid) else {
            entities.despawn_all(&mut commands);
            return;
        };
        entities.fill_mesh = Some(meshes.add(mesh));
        entities.last_fingerprint = fingerprint;
        entities.despawn_all(&mut commands);
        // 材质：颜色固定（按 kind），alpha 每帧改。
        let base = match flash.kind {
            CellFlashKind::Undo => crate::theme::colors::UNDO_FLASH,
            CellFlashKind::Reject => crate::theme::colors::UI_UNREACHABLE,
        };
        if entities.fill_material.is_none() {
            entities.fill_material = Some(materials.add(ColorMaterial::from_color(
                base.with_alpha(flash.alpha(now)),
            )));
        }
        let mesh = entities.fill_mesh.clone().expect("刚建");
        let mat = entities.fill_material.clone().expect("刚建");
        entities.fill = Some(
            commands
                .spawn((
                    Transform::from_translation(origin.extend(FLASH_Z)),
                    Mesh2d(mesh),
                    MeshMaterial2d(mat),
                ))
                .id(),
        );
        return;
    }
    // 格集合未变 → 只改 alpha（材质句柄内的颜色）。
    let Some(e) = entities.fill else {
        return;
    };
    let Some(mat) = entities.fill_material.clone() else {
        return;
    };
    let base = match flash.kind {
        CellFlashKind::Undo => crate::theme::colors::UNDO_FLASH,
        CellFlashKind::Reject => crate::theme::colors::UI_UNREACHABLE,
    };
    if let Some(mut m) = materials.get_mut(&mat) {
        m.color = base.with_alpha(flash.alpha(now));
    }    commands.entity(e).insert(MeshMaterial2d(mat));
}

// ══════ from crates/wv-world-cell-map/src/region_overlay.rs :: region_layer_visible ══════
/// 区域层在本帧是否可见（2026-10-22 内容矩阵 §2.6 的唯一判定入口）。
///
/// - **CellMap（Topographic）**：只看 `nature_overlay` 开关，且**仅 Nature 类**
///   放行（Area 区属内容，CellMap 永远藏）；recipe 格（false）被旁路。
/// - **其他模式**：走 recipe（RegionLayers 行）∧ 图层。
///   Schematic 行=false ⇒ 编辑态区域消失（用户裁定：区域有时间性）。
pub(crate) fn region_layer_visible(
    mode: crate::state::ViewMode,
    nature_overlay: bool,
    kind: wv_core::model::region::RegionKind,
    recipe_enabled: bool,
    layer_visible: bool,
) -> bool {
    use crate::state::ViewMode;
    use wv_core::model::region::RegionKind;
    if !layer_visible {
        return false;
    }
    if matches!(mode, ViewMode::Topographic) {
        return nature_overlay && matches!(kind, RegionKind::Nature);
    }
    recipe_enabled
}
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
        // Schematic：recipe 格=false ⇒ 无论开关都藏（开关只在 CellMap 有意义）
        assert!(!region_layer_visible(
            ViewMode::Schematic,
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
        world.resource_mut::<ViewState>().mode = crate::state::ViewMode::Schematic;
        schedule.run(&mut world);
        assert_eq!(
            count_fades(&mut world),
            1,
            "换模式（换眼镜）不应生成第二个淡出精灵"
        );
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use bevy::render::mesh::MeshVertexAttribute;

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
                ViewMode::Schematic,
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

/// 网格同步（自 wv-render `terrain_view/grid_sync.rs` 迁入 bridge；ADR-0054 D5）：
/// 维护 `GridConfig` 的两套原点——`raster_origin` / `cell_size` 取**纹理实际来源图**，
/// `map_origin` / `map_size` 取**当前图**。早退条件里的 `drill.is_changed()` 不可省：
/// 钻入继承父图底图的城市时纹理不变、只有当前图变了（「钻进武昌看不到武昌」根因）。
pub fn sync_grid_config(
    tex: Res<wv_world_cell_map::terrain_view::terrain_texture::TerrainTexture>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut grid: ResMut<GridConfig>,
) {
    if !tex.is_added() && !tex.is_changed() && !drill.is_changed() {
        return;
    }
    let source = wv_world_cell_map::terrain_view::terrain_resolve::active_terrain(
        &projection.world,
        drill.current(),
    )
    .map(|(node, _)| node);
    let extent = source
        .and_then(|n| n.extent.as_ref())
        .map(|e| e.width)
        .or_else(|| {
            projection
                .world
                .meta
                .as_ref()
                .and_then(|m| {
                    projection
                        .world
                        .maps
                        .iter()
                        .find(|n| n.id == m.root_map)
                        .and_then(|n| n.extent.as_ref())
                })
                .map(|e| e.width)
        })
        .unwrap_or(4096.0);
    grid.cell_size = (extent / tex.size as f32).max(0.01);
    grid.raster_origin = source
        .and_then(|n| wv_core::resolve_frame_in(&projection.world.maps, &n.id))
        .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
        .unwrap_or(Vec2::ZERO);
    let current = drill.current_node(&projection.world);
    let map_origin = current
        .map(|n| crate::drill::map_origin(&projection.world, n, &grid))
        .unwrap_or(grid.raster_origin);
    let map_size = current
        .and_then(|n| n.extent.as_ref())
        .map(|e| Vec2::new(e.width, e.height))
        .unwrap_or(Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size);
    grid.map_origin = map_origin;
    grid.map_size = map_size;
}
