//! 区域命令：意图圈选、大陆生成、批量选区栅格化。

use super::terrain::terrain_diff_bbox;
use super::*;
use wv_render::state::CellSelection;

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

/// 套索活链接（ADR-0057 补记 5 B 方案）：套索**闭合那刻**快照基座选区与
/// 修饰键 op，拖锚调形期间每帧 `CellSelection` = 基座 ∪/− 当前多边形格集。
///
/// 断开时机：任何新选择动作（点选 / 泛洪，各自调用 [`Self::deactivate`]）、
/// 闭合消失（取消 / 切图 / 新手势把 `SelectionState.closed` 置回 `None`）。
/// 断开**不回滚**——最后一帧的结果钉在 `CellSelection` 里。
#[derive(Resource, Default)]
pub struct LassoLiveLink {
    /// 闭合快照基座（无修饰 = 空集 = 纯替换语义）。**整个拖锚期间冻结**——
    /// 每帧重写都从这个固定快照出发，否则差集口径下「拖过再拖走」无法恢复。
    base: Vec<(i32, i32)>,
    /// 闭合时的修饰键：Alt = 差集，其余（含无修饰、Shift）= 并集
    /// （无修饰时 base 为空，并集即替换）。
    subtract: bool,
    /// 活链接是否挂着（闭合后 true；断开后 false）。
    active: bool,
    /// 闭合时绑定的图（`SelectionState.on_map_change` 会清 closed，此处仅兜底）。
    map: Option<wv_core::Id>,
    /// 上次已栅格化的多边形指纹（拖锚没变就跳过 O(bbox×边数) 重算）。
    last_fingerprint: u64,
}

impl LassoLiveLink {
    /// 断开活链接（新选择动作 / 取消 / 切工具）——结果已钉在选区里。
    pub fn deactivate(&mut self) {
        self.active = false;
        self.last_fingerprint = 0;
    }
}

/// 多边形选区 → 统一选区同步（ADR-0057 活链接 B：基座快照 + 拖锚实时调形）。
///
/// `SelectionState.closed`（V 工具闭合的多边形，图内局部坐标）闭合沿快照
/// 基座（Shift/Alt = 当前选区，无修饰 = 空）与 op；之后每帧把
/// `CellSelection` 重写为「基座 ∪/− 当前多边形格集」（格心落入即选中，
/// [`wv_render::cell_selection::rasterize_polygon_cells`]）。闭合消失 =
/// 断开活链接，结果钉住不回滚。
pub fn sync_region_cells(
    selection: Res<crate::state::SelectionState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    keys: Res<ButtonInput<KeyCode>>,
    mut sel: ResMut<CellSelection>,
    mut link: ResMut<LassoLiveLink>,
) {
    use wv_render::cell_selection::rasterize_polygon_cells;
    let Some(poly) = selection.closed.as_ref() else {
        // 闭合消失 = 断开活链接；结果已逐帧钉在 CellSelection 里，不回滚。
        link.deactivate();
        return;
    };
    // 来源图地形（无地形 = 无可贴合方格，不动选区）。
    let Some(target_map) = drill.current() else {
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        return;
    };
    // 惰性定容（同图同尺寸 = 不动；切图由 bind_map 清空）。
    sel.bind_map(target_map, terrain.width, terrain.height);
    // 切图兜底：活链接绑的是另一张图 → 断开（`SelectionState.on_map_change`
    // 正常会先把 closed 清掉而走上面分支，这里是双保险）。
    if link.active && link.map.as_ref() != Some(target_map) {
        link.deactivate();
    }

    // 闭合沿（非激活 → 激活）：快照基座 + 修饰键 op。
    if !link.active {
        let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        link.base = if shift || alt {
            sel.iter_cells().collect()
        } else {
            Vec::new() // 无修饰 = 替换（基座为空）
        };
        link.subtract = alt;
        link.map = Some(target_map.clone());
        link.active = true;
        link.last_fingerprint = 0;
    }

    // 多边形指纹：没变就跳过重栅格化（大选区每帧 O(bbox×边数) 重算会明显吃帧）。
    let fingerprint = polygon_fingerprint(poly);
    if fingerprint != 0 && fingerprint == link.last_fingerprint {
        return;
    }
    // 选区锚点存的是**本图局部坐标**（`selection::commit_selection_click`：
    // `local = cursor_world − map_origin`）——栅格化必须补上同一个
    // `map_origin` 才落在同一套坐标系（2026-10-20 实机：三角形只圈住
    // 一个小岛，却选出 21974 格——差的正是这个原点偏移）。
    let origin = match drill.current_node(&projection.world) {
        Some(node) => wv_render::drill::map_origin(&projection.world, node, &grid),
        None => grid.raster_origin,
    };
    let cells = rasterize_polygon_cells(poly, origin, &grid, terrain.width, terrain.height);
    // 每帧口径：选区 = 基座 ∪/− 当前多边形格集（基座冻结，见结构体文档）。
    if link.subtract {
        let minus: std::collections::BTreeSet<(i32, i32)> = cells.iter().copied().collect();
        sel.replace(link.base.iter().copied().filter(|c| !minus.contains(c)));
    } else {
        sel.replace(link.base.iter().copied());
        sel.union(cells);
    }
    link.last_fingerprint = fingerprint;
}
