//! 地形命令：增量落盘、重置到底图、重新生成、清除测量。

use super::*;

/// 把脏标记变为实际纹理更新（增量重传的出口）。
///
/// **优化项 3d**：
/// - 模式切换（`rebuild_texture_on_mode_change`）→ `tex.dirty = Some([0, 0, size, size])`
///   仍走 [`image_from_terrain`] 全图重建；
/// - 笔刷编辑 → `tex.dirty = Some(笔触包围盒)` 走 [`wv_render::terrain_view::patch_terrain_image`]
///   原地 patch 像素（**关键性能提升**：512² 整图重算 ≈ 80 ms → 几十格 patch ≈ <1 ms）。
///
/// **为何不能直接靠修改 `image.data` 触发 GPU upload**：bevy 0.15 的 `Image` 资源
/// 检测的是「资产添加/移除」，不是 `data` 字段写入——必须走 `Assets::get_mut` 拿到
/// `&mut Image` 后 `patch_terrain_image` 原地改 `data`（已是 `&mut`），bevy 在下一帧
/// 检测到 asset 变更时会上传。
pub fn flush_terrain_texture(
    mut tex: ResMut<TerrainTexture>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(rect) = tex.dirty else { return };
    // Phase 2：取**纹理正在显示的那张图**的地形（当前图或它向上继承到的祖先图）。
    let Some(data) =
        wv_render::terrain_view::texture_terrain(&tex, &projection.world, drill.current())
    else {
        tex.dirty = None;
        return;
    };
    let w = data.width.max(1);
    let h = data.height.max(1);
    // dirty 矩形 = 整图 → 走全图重建（模式切换 / 全图标记脏的场景）。
    let is_full = rect == [0, 0, w, h];
    if is_full {
        let image = image_from_terrain(
            data,
            wv_render::cellmap_bridge::shade_mode_of(view.mode),
            atlas.as_deref(),
            view.terrain_soften,
            &wv_render::cellmap_bridge::conv_pack_with(
                view.active_theme,
                view.hypsometric_tint,
                view.mode,
            ),
            0.0,
        );
        if let Some(mut img) = images.get_mut(&tex.handle) {
            *img = image;
        }
        tex.dirty = None;
        return;
    }
    // 增量 patch：只重算 dirty 矩形内的 cell，原地写进 `image.data`。
    let res = tex.res;
    if let Some(mut img) = images.get_mut(&tex.handle) {
        wv_render::terrain_view::patch_terrain_image(
            &mut img,
            data,
            wv_render::cellmap_bridge::shade_mode_of(view.mode),
            atlas.as_deref(),
            rect,
            res,
            view.terrain_soften,
            &wv_render::cellmap_bridge::conv_pack_with(
                view.active_theme,
                view.hypsometric_tint,
                view.mode,
            ),
        );
    }
    tex.dirty = None;
}

/// 逐格比较撤销/重做前后的地形 cells，返回变化格的包围盒（逻辑格，半开）。
///
/// 调用方保证两段 slices 等长（尺寸不一致由调用方退化为 `mark_all_dirty`）。
/// 无变化时返回 `None`。
pub(super) fn terrain_diff_bbox(
    before: &[wv_core::terrain::TerrainCellData],
    after: &[wv_core::terrain::TerrainCellData],
    w: u32,
) -> Option<[u32; 4]> {
    let mut x0 = u32::MAX;
    let mut y0 = u32::MAX;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    let mut any = false;
    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        if b == a {
            continue;
        }
        any = true;
        let cx = (i as u32) % w;
        let cy = (i as u32) / w;
        x0 = x0.min(cx);
        y0 = y0.min(cy);
        x1 = x1.max(cx + 1);
        y1 = y1.max(cy + 1);
    }
    any.then_some([x0, y0, x1, y1])
}

/// 逐格比较撤销/重做前后的地形 cells，返回**变化格集合**（[ADR-0052] D8-A 白闪用）。
///
/// 与 [`terrain_diff_bbox`] 同源遍历——同一份 diff 出两个结果（包围盒给纹理
/// 标脏，格集合给白闪定位），避免二次扫描 512² 地形。
///
/// **有上限**：超过 [`MAX_FLASH_CELLS`] 的格不再收集——整图撤销的白闪退化为
/// 「包围盒边框环」（渲染层按格数自动降级），应用层无需知道细节。
pub(super) fn terrain_diff_cells(
    before: &[wv_core::terrain::TerrainCellData],
    after: &[wv_core::terrain::TerrainCellData],
    w: u32,
) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        if b == a {
            continue;
        }
        if out.len() >= wv_world_cell_map::cell_flash::MAX_FLASH_CELLS {
            break;
        }
        let cx = (i as u32) % w;
        let cy = (i as u32) / w;
        out.push((cx as i32, cy as i32));
    }
    out
}

/// 重置当前图的地形到底图（2026-10-16 用户需求）。
///
/// 走和「撤销/重做」同一套纹理脏追踪（前后 cells diff → 矩形标脏；
/// 尺寸变了走 `mark_all_dirty`），让笔刷面板上的「重置到底图」按钮
/// 不需要绕过 dispatcher。
///
/// 播放中只读（基线 V37）：拒绝重置并说明原因，而不是静默无反应。
/// 无工程闸门由 [`crate::ui_shell`] 提前把 `CommandId::ResetTerrainToBase`
/// 挡掉，本函数不再重复。
pub fn apply_reset_terrain_to_base(
    map_id: &wv_core::Id,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    playback: &PlaybackState,
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再重置".into());
    }

    // 与 apply_history_command 同款：前后 cells diff → 矩形标脏
    // （避免 mark_all_dirty 的整图重建）。
    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(map_id))
        .map(|t| t.cells.clone());

    let mut tx = Transaction::new();
    tx.push(Box::new(ResetTerrainToBaseCommand::new(map_id.clone())));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);

            let after =
                wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(map_id))
                    .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                _ => tex.mark_all_dirty(),
            }
            Ok("已重置到底图".into())
        }
        Err(e) => Err(format!("重置失败：{e}")),
    }
}

/// 重生成世界地形（ADR-0037 D4）：整图替换 + 模板记录更新，单事务可撤销。
///
/// 与 [`apply_reset_terrain_to_base`] 同款：前后 cells diff 出脏矩形，
/// 避免 `mark_all_dirty` 的整图重建。磁盘备份（`terrain.bin.bak.<ts>`）
/// 由调用方在提交前做——本函数只负责世界数据与纹理。
#[allow(clippy::too_many_arguments)]
pub fn apply_regen_world(
    map_id: &wv_core::Id,
    new_terrain: wv_core::terrain::TerrainData,
    new_template: wv_core::WorldTemplate,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    playback: &PlaybackState,
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再重生成".into());
    }

    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(map_id))
        .map(|t| t.cells.clone());

    let mut tx = Transaction::new();
    tx.push(Box::new(RegenWorldTerrainCommand::new(
        map_id.clone(),
        new_terrain,
        new_template,
    )));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);

            let after =
                wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(map_id))
                    .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                _ => tex.mark_all_dirty(),
            }
            Ok("已重生成世界地形".into())
        }
        Err(e) => Err(format!("重生成失败：{e}")),
    }
}

/// 一键清除当前图的全部测量（2026-10-23 用户需求）。
///
/// 走 `ClearMeasurementsCommand`：整份 overlay 换空 + 单槽缓存旧 overlay，
/// **一次撤销整体还原**（不用 N 条 Remove 拼事务，撤销栈也不会被刷屏）。
///
/// 空图早退：`world.measurements[map]` 本来就空时**不提交**命令——
/// 空命令进撤销栈等于让作者按一次 Ctrl+Z 什么都没变（与
/// [`ClearMeasurementsCommand::apply`] 的「空→空不记脏」同口径）。
///
/// 播放中只读（基线 V37）：拒绝并说明原因，不静默无反应。
/// 无工程闸门由 [`crate::ui_shell`] 提前挡（本函数只处理「有图但没测量」）。
pub fn apply_clear_measurements(
    map_id: &wv_core::Id,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    playback: &PlaybackState,
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再清除测量".into());
    }
    let count = projection
        .world
        .measurement_of(map_id)
        .map(|o| o.measurements.len())
        .unwrap_or(0);
    if count == 0 {
        return Ok("当前图没有测量".into());
    }

    let mut tx = Transaction::new();
    tx.push(Box::new(ClearMeasurementsCommand::new(map_id.clone())));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
            Ok(format!("已清除 {count} 条测量"))
        }
        Err(e) => Err(format!("清除测量失败：{e}")),
    }
}
