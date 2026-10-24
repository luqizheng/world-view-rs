//! 区域命令：意图圈选、大陆生成、批量选区栅格化。

use super::terrain::terrain_diff_bbox;
use super::*;

/// 一键应用区域地形意图（选择工具 spec §4 D10：无弹窗、一意图一事务）。
///
/// **0 改动预检**（纯海丘陵 / 湖心在海等语义性无操作）：草稿栅格以同 seed
/// 先跑一遍，`apply_intent` 返回 0 → 不提交、发通知、返回 `false`——空命令
/// 不进撤销栈。预检目标 = `world.terrain_of(&map)`（**自有栅格**，与
/// `IntentTerrainCommand::apply` 同一网格口径，终审 Important 1）。
/// 播放中拒绝（基线 V37）。成功返回 `true`；选区清空由调用方负责
/// （spec §4：撤销不回驻留）。
///
/// 纹理脏矩形与 [`apply_reset_terrain_to_base`] 同款：前后 cells diff 出包围盒。
#[allow(clippy::too_many_arguments)]
pub fn commit_region_intent(
    map: wv_core::Id,
    intent: Intent,
    cell_size: f32,
    playback: &PlaybackState,
    loc: crate::i18n::Locale,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    notice: &mut crate::ui_shell::ShellNotice,
    frame: u64,
) -> bool {
    if playback.playing {
        notice.error("播放中为只读：先暂停再应用地形意图", frame);
        return false;
    }
    // 预检：自有栅格上以同 seed 草稿跑一遍，无改动就不进历史。
    let seed = intent.derive_seed(map.as_str());
    let no_op = match projection.world.terrain_of(&map) {
        Some(t) => {
            let cells: Vec<wv_terrain::TerrainCell> = t
                .cells
                .iter()
                .map(|c| wv_terrain::TerrainCell {
                    height: c.height,
                    surface: c.surface,
                    biome: c.biome,
                })
                .collect();
            let mut scratch = match wv_terrain::TerrainGrid::from_cells(t.width, t.height, cells) {
                Some(g) => g,
                None => {
                    notice.error("地形网格尺寸不一致，意图未生效", frame);
                    return false;
                }
            };
            wv_terrain::intent::apply_intent(&mut scratch, &intent, seed, cell_size) == 0
        }
        None => {
            notice.error(
                format!("地图 `{map}` 没有自己的地形栅格，无法应用地形意图"),
                frame,
            );
            return false;
        }
    };
    if no_op {
        notice.error(
            crate::i18n::t(loc, crate::i18n::I18nKey::IntentNoticeNoOp),
            frame,
        );
        return false;
    }

    let before = projection.world.terrain_of(&map).map(|t| t.cells.clone());
    let mut tx = Transaction::new();
    tx.push(Box::new(IntentTerrainCommand::new(
        map.clone(),
        intent,
        cell_size,
    )));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            let after = projection
                .world
                .terrain_of(&map)
                .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                _ => tex.mark_all_dirty(),
            }
            notice.info(
                crate::i18n::t(loc, crate::i18n::I18nKey::IntentNoticeApplied),
                frame,
            );
            true
        }
        Err(e) => {
            notice.error(
                format!(
                    "{}：{e}",
                    crate::i18n::t(loc, crate::i18n::I18nKey::IntentNoticeFailed)
                ),
                frame,
            );
            false
        }
    }
}

/// 提交一次大陆生成（宏命令）。成功后**不清空选区**（ADR-0033）——
/// 作者可以继续加锚点、调轮廓再生成；切图 / 切工具 / 点空白 / Esc 才清。
///
/// 调用方只克隆环、不预消费选区——失败时选区原样保留，作者画了半天的圈
/// 不能因为一次提交失败就消失（V1 精神：只提示，不替你决定）。
///
/// 纹理脏矩形与 [`apply_reset_terrain_to_base`] 同款：前后 cells diff 出
/// 包围盒，避免整图重建。
#[allow(clippy::too_many_arguments)]
pub fn commit_continent_generation(
    map: wv_core::Id,
    envelopes: Vec<Vec<(f32, f32)>>,
    params: ContinentParams,
    seed: u64,
    cell_size: f32,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
) -> Result<(), String> {
    if envelopes.is_empty() {
        return Err("没有圈出大陆范围：先用「选择」工具圈一块闭合区域".into());
    }

    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(&map))
        .map(|t| t.cells.clone());
    let mut tx = Transaction::new();
    tx.push(Box::new(GenerateContinentCommand::new(
        map.clone(),
        envelopes.clone(),
        params,
        seed,
        cell_size,
    )));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            let after =
                wv_render::terrain_view::texture_terrain(tex, &projection.world, Some(&map))
                    .map(|t| (t.width.max(1), t.cells.clone()));
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                }
                _ => tex.mark_all_dirty(),
            }
            // 选区保持不动（ADR-0033）：意图生效 = 对当前环做一次操作，
            // 不是「交出去」。作者可继续编辑锚点再点其它意图。
            Ok(())
        }
        Err(e) => {
            // 提交失败：选区原样保留（本函数从不消费选区）。
            Err(format!("生成失败：{e}"))
        }
    }
}

/// 多边形顶点指纹（FNV-1a）——「多边形变没变」的判据。
///
/// 0 表示「无有效指纹」（空多边形）。
pub(super) fn polygon_fingerprint(poly: &[(f32, f32)]) -> u64 {
    if poly.len() < 3 {
        return 0;
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for (x, y) in poly {
        for v in [x.to_bits() as u64, y.to_bits() as u64] {
            h ^= v;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 多边形选区 → 方格集合同步（2026-10-20 批量编辑）。
///
/// `SelectionState.closed`（V 工具闭合的多边形，世界坐标）变化时重栅格化
/// （cell 中心落入即选中，[`wv_render::cell_selection::rasterize_polygon_cells`]），
/// 写入 `EditCellState.region_cells`——区域高亮渲染与批量应用（面板）都读它。
/// 闭合取消 / 切图 → 清空。
pub fn sync_region_cells(
    selection: Res<crate::state::SelectionState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    mut edit_cell: ResMut<EditCellState>,
) {
    use wv_render::cell_selection::rasterize_polygon_cells;
    let Some(poly) = selection.closed.as_ref() else {
        if !edit_cell.region_cells.is_empty() {
            edit_cell.region_cells.clear();
        }
        return;
    };
    // 来源图地形（无地形 = 无可贴合方格，清空）。
    let Some(target_map) = drill.current() else {
        edit_cell.region_cells.clear();
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        edit_cell.region_cells.clear();
        return;
    };
    // 变化检测：多边形点数 + 首点 + 栅格口径没变就跳过（避免每帧 O(N) 重算）。
    // 粗指纹够用——锚点拖动必然改首点或点数以外的坐标，此处选择每次
    // closed 存在就重算：8×8~512² 栅格 bbox 扫描 <1ms，拖锚帧率可承受。
    // 选区锚点存的是**本图局部坐标**（selection::commit_selection_click:
    // `local = cursor_world − map_origin`）——栅格化必须补上同一个
    // `map_origin` 才落在同一套坐标系（2026-10-20 实机：三角形只圈住
    // 一个小岛，却选出 21974 格——差的正是这个原点偏移）。
    // 多边形指纹：没变就跳过重栅格化（2026-10-20 性能修——大选区每帧
    // O(bbox×边数) 重算会明显吃帧）。
    let fingerprint = polygon_fingerprint(poly);
    if fingerprint != 0
        && fingerprint == edit_cell.region_fingerprint
        && !edit_cell.region_cells.is_empty()
    {
        return;
    }
    let origin = match drill.current_node(&projection.world) {
        Some(node) => wv_render::drill::map_origin(&projection.world, node, &grid),
        None => grid.raster_origin,
    };
    edit_cell.region_cells =
        rasterize_polygon_cells(poly, origin, &grid, terrain.width, terrain.height);
    edit_cell.region_fingerprint = fingerprint;
    edit_cell.last_map_id = Some(target_map.clone());
}
