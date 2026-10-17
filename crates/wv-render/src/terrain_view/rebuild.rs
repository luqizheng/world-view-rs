//! 换级重建与淡出：`rebuild_texture_on_mode_change` + `BasemapFadeOut` + `tick_basemap_fadeout`。
//!
//! ## 职责
//! - `rebuild_texture_on_mode_change` —— 模式 / 来源图 / 淡化开关任一变了就重建。
//! - `spawn_basemap_fadeout` —— 换图那一刻克隆旧底图像素铺淡出精灵。
//! - `tick_basemap_fadeout` —— 逐帧推进淡出，归零 despawn。
//! - `advance_fade` —— 淡出步进（私有）。
//!
//! ## 边界（不做什么）
//! - **不调图像合成** —— 那在 [`super::cells`]。
//!
//! ## 主要导出
//! - [`rebuild_texture_on_mode_change`] / [`tick_basemap_fadeout`] / [`BasemapFadeOut`] / [`LEVEL_FADE_SECS`]

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{DrillState, GridConfig, ViewMode, ViewState, WorldProjection};

use super::cells::terrain_pixels;
use super::isoline_overlay::terrain_data_hash;
use super::terrain_resolve::{active_terrain, replace_terrain_texture};
use super::terrain_texture::TerrainTexture;
use super::tile_atlas::TileAtlas;

/// 后台合成的上下文指纹：落地时校验「世界还是那个世界」。
#[derive(Debug, Clone, PartialEq)]
pub struct SynthCtx {
    pub mode: ViewMode,
    pub soften: bool,
    pub palette: crate::theme::palette::MapPalette,
    /// 被合成地形所属的图（`active_terrain` 给出的来源图）。
    pub map: Option<wv_core::Id>,
    /// 内容指纹（height+kind FNV-1a，与等值线缓存同源）。
    pub data_hash: u64,
}

/// 后台合成结果：像素 + 每格分辨率 + 指纹。
pub struct SynthResult {
    pub pixels: Vec<u8>,
    pub res: u32,
    pub ctx: SynthCtx,
}

/// 在途的整图合成任务（`Assets<Image>` 非线程安全，资产写入必须回主线程）。
#[derive(Resource, Default)]
pub struct PendingTerrainSynth {
    pub task: Option<bevy::tasks::Task<SynthResult>>,
    pub ctx: Option<SynthCtx>,
    /// 已经为哪次换图发过淡出精灵：异步落地期间 `tex.last_map` 仍是旧图，
    /// 防止每次重排（例如落地前又切了模式）重复克隆旧底图。
    pub fade_for_map: Option<wv_core::Id>,
}

/// 落地校验：当前 `active_terrain` 的来源图与内容指纹都必须与任务上下文一致。
///
/// 快速连切 palette / 合成完成前换图 → 旧像素不得贴进新世界；丢弃后由
/// `rebuild_texture_on_mode_change` 在后续帧按新条件重新触发。
pub fn ctx_matches_world(
    ctx: &SynthCtx,
    world: &wv_core::World,
    current: Option<&wv_core::Id>,
) -> bool {
    let Some((node, data)) = active_terrain(world, current) else {
        return false;
    };
    ctx.map.as_ref() == Some(&node.id) && ctx.data_hash == terrain_data_hash(data)
}

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
        && tex.last_mode == Some(view.mode)
        && tex.last_soften == view.terrain_soften
        && tex.last_palette == view.map_palette
    {
        return;
    }
    let Some((node, data)) = active_terrain(&projection.world, drill.current()) else {
        // 地形尚未可用：不重建，也不置位 last_mode，等地形就绪后再走这里。
        return;
    };
    let ctx = SynthCtx {
        mode: view.mode,
        soften: view.terrain_soften,
        palette: view.map_palette,
        map: Some(node.id.clone()),
        data_hash: terrain_data_hash(data),
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
            let colors = crate::theme::palette::colors(task_ctx.palette);
            let treatment =
                crate::theme::theme_pack::builtin::for_palette(task_ctx.palette).treatment;
            let (pixels, res) = terrain_pixels(
                &data_clone,
                task_ctx.mode,
                atlas_clone.as_ref(),
                task_ctx.soften,
                colors,
                treatment,
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

/// 主线程落地：任务完成且世界未变 → 建 `Image` 资产并替换纹理。
///
/// 校验失败（快速连切 palette / 完成前换图）直接丢弃；`last_*` 未置位，
/// 下一帧 `rebuild_texture_on_mode_change` 会按新条件重新触发。
pub fn land_pending_terrain_synth(
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
    if !ctx_matches_world(&result.ctx, &projection.world, drill.current()) {
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
        result.ctx.palette,
    );
    // 整图内容已整体替换：chunk 层需要全量重贴。
    tex.chunks_dirty = Some([0, 0, tex.size, tex.size]);
    tex.last_mode = Some(result.ctx.mode);
    tex.last_soften = result.ctx.soften;
    tex.last_palette = result.ctx.palette;
    tex.last_map = Some(node.id.clone());
    // 这次换图的淡出精灵已经就位；下一次换图再重新判定。
    pending.fade_for_map = None;
}

/// 换级淡化时长（秒）：旧底图整幅淡出、露出新底图。
///
/// 第一版取 0.4s——比「闪一下」（<0.2s）有过渡感，比「拖沓」（>0.8s）短，
/// 与常见地图应用的级间过渡同一量级。是否可调/可关注册于 README §12-25。
pub const LEVEL_FADE_SECS: f32 = 0.4;

/// 旧底图淡出精灵的 Z：盖住新 chunk 层（`TERRAIN_Z = 0.0`），
/// 但在道路（`ROAD_Z = 0.1`）与全部矢量层之下——过渡只属于底图，
/// 标记/街道/文字从第一帧起就画在新图的正确位置上。
pub const FADEOUT_Z: f32 = 0.05;

/// 换级时旧底图的淡出精灵标记（防重复 + 淡出系统的过滤口径）。
#[derive(Component)]
pub struct BasemapFadeOut {
    /// 剩余不透明度 `1.0 → 0.0`；归零即 despawn。
    pub alpha: f32,
}

/// 淡出步进（纯函数，便于测试）：返回下一帧 alpha，`None` = 已到 0，应 despawn。
pub fn advance_fade(alpha: f32, dt: f32) -> Option<f32> {
    let next = alpha - dt / LEVEL_FADE_SECS;
    (next > 0.0).then_some(next)
}

/// 换级那一刻：把**旧底图像素**克隆成一份淡出资产并铺在新底图之上。
///
/// 为什么必须克隆：[`replace_terrain_texture`] 优先**复用旧句柄**原地覆写像素，
/// 不克隆的话淡出精灵与主底图共用同一张 `Image`，旧画面会瞬间变成新画面。
/// 克隆是一帧一次性开销（整图 memcpy），换级本来就伴随整图重建，可接受。
///
/// 摆位用调用时刻的 `grid`（此刻仍是**旧图**的栅格原点/格长——本系统排在
/// [`sync_grid_config`] 之前），所以淡出精灵精确盖住旧底图原本的位置与幅面。
pub fn spawn_basemap_fadeout(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    tex: &TerrainTexture,
    grid: &GridConfig,
) {
    let Some(old) = images.get(&tex.handle) else {
        return;
    };
    let handle = images.add(old.clone());
    let size = tex.size as f32 * grid.cell_size;
    let center = grid.raster_origin + Vec2::splat(size * 0.5);
    commands.spawn((
        BasemapFadeOut { alpha: 1.0 },
        Sprite {
            image: handle,
            custom_size: Some(Vec2::splat(size)),
            color: Color::WHITE,
            ..Default::default()
        },
        Transform::from_xyz(center.x, center.y, FADEOUT_Z),
    ));
}

/// 淡出推进：逐帧降低旧底图精灵的不透明度，归零即回收实体。
pub fn tick_basemap_fadeout(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Sprite, &mut BasemapFadeOut)>,
) {
    let dt = time.delta_secs();
    for (entity, mut sprite, mut fade) in &mut q {
        match advance_fade(fade.alpha, dt) {
            Some(next) => {
                fade.alpha = next;
                sprite.color = Color::srgba(1.0, 1.0, 1.0, next);
            }
            None => {
                commands.entity(entity).despawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    fn test_world_with_terrain(id: &str, size: u32, height: i16) -> (wv_core::World, wv_core::Id) {
        let map = wv_core::Id::new_unchecked(id);
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            level: wv_core::map::MapLevel::World,
            ..Default::default()
        });
        let cells = vec![TerrainCellData { height, kind: 0 }; (size * size) as usize];
        world.set_terrain(
            map.clone(),
            TerrainData {
                width: size,
                height: size,
                cells,
            },
        );
        (world, map)
    }

    fn ctx_for(world: &wv_core::World, map: &wv_core::Id) -> SynthCtx {
        let data = world.terrain_of(map).expect("地形存在");
        SynthCtx {
            mode: ViewMode::VectorFlat,
            soften: false,
            palette: crate::theme::palette::MapPalette::default(),
            map: Some(map.clone()),
            data_hash: terrain_data_hash(data),
        }
    }

    /// 落地校验：完成时刻世界已换（data_hash / 来源图变）→ 拒绝落地。
    #[test]
    fn ctx_validation_rejects_stale_world() {
        let (world, id_a) = test_world_with_terrain("map_a", 8, -100);
        let ctx = ctx_for(&world, &id_a);
        assert!(ctx_matches_world(&ctx, &world, Some(&id_a)), "同世界应放行");

        let mut stale = ctx.clone();
        stale.data_hash = stale.data_hash.wrapping_add(1);
        assert!(
            !ctx_matches_world(&stale, &world, Some(&id_a)),
            "内容指纹变（笔刷已改地形）应拒绝"
        );

        let mut other_map = ctx.clone();
        other_map.map = Some(wv_core::Id::new_unchecked("map_b"));
        assert!(
            !ctx_matches_world(&other_map, &world, Some(&id_a)),
            "来源图变应拒绝"
        );
    }
}
