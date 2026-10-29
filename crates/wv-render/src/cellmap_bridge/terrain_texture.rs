//! 地形底图纹理生命周期：整图合成任务排队 / 模式切换重建 / 后台落图
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

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
        &conv_pack_with(view.active_theme, view.hypsometric_tint, view.mode),
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
        view.hypsometric_tint,
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
        // ADR-0055：分层设色强度变了必须重烘（CPU 路径没有 uniform 可改）。
        && (tex.last_hypsometric - view.hypsometric_tint).abs() <= 1e-6
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
        pack: conv_pack_with(view.active_theme, view.hypsometric_tint, view.mode),
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
        result.ctx.pack.hypsometric,
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
