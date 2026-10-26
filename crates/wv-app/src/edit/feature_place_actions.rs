//! 生物群系标记（地界）→ 提交 `wv_edit::command` 的动作层。
//!
//! ## 职责
//! - [`place_biome_marker_now`]：把选区意图环 + 作者输入的名称/群系，
//!   包成一条 [`wv_edit::command::PlaceBiomeMarkerCommand`] 提交到
//!   [`EditHistory`]；
//! - [`regenerate_marker_now`]：以 Place 已存包络重跑识别/生成（M2）。
//!
//! ## 设计依据
//!
//! 与 `region_panel_actions.rs` 同构：UI 只收集输入，提交集中在动作层；
//! 单条命令包成 `Transaction`（一次编辑 = 一次 Undo 单元）。入口是选择工具
//! 意图菜单的「标记…」（`FeaturePlaceDialog`）与区域面板，不是独立工具。
//! 旧词汇动作 `add_feature_place_now` 已随 ADR-0032 D6 删除——旧 kind 词汇
//! 的最后一个写入者退场，词汇收口在写入侧闭合。
//!
//! ## 边界（不做什么）
//! - **不**直接改 `projection.world`——一律经 `History::commit`（AGENTS §3.3）；
//! - **不**做 UI——对话框在 `crate::ui::feature_place_dialog`；
//! - **不**做时间求值——`span.from` 直接取视图当前日（作者在「此刻」圈地，
//!   区间由 P6/P7 的既有模型表达，动作层不做任何比较）。

#![cfg(feature = "app")]

use crate::edit::editing::EditHistory;
use crate::state::{PlaybackState, WorldProjection};
use wv_core::distance::Place;
use wv_edit::history::Transaction;

/// 放置一个生物群系标记（ADR-0029 / 命名段 M1 Task 4）。
///
/// 走 `PlaceBiomeMarkerCommand`——包络是意图、边界由内核识别/生成、
/// 生成分支写地形格子，三件事单事务原子（旧 kind 词汇动作
/// `add_feature_place_now` 已随 ADR-0032 D6 删除）。
/// 地形若被改写，按 cells 前后差集标纹理脏区（与 `commit_region_intent` 同口径）。
#[allow(clippy::too_many_arguments)]
pub fn place_biome_marker_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    playback: &PlaybackState,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    map: wv_core::Id,
    name: String,
    biome: u8,
    ring: &[(f32, f32)],
    cell_size: f32,
    day: i64,
) -> Result<wv_core::Id, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再放置标记".into());
    }
    if ring.len() < 3 || cell_size <= 0.0 {
        return Err("标记包络至少需要 3 个点".into());
    }
    let id = wv_core::Id::new_unchecked(format!("place_marker_{}", current_unix_ms()));
    let place = Place {
        kind: None,
        line: None,
        id: id.clone(),
        name,
        feature: None,
        biome: Some(biome),
        envelope: None,
        map: Some(map.clone()),
        x: None,
        y: None,
        w: None,
        h: None,
        rank: None,
        line_kind: None,
        line_width_m: None,
    };
    let before_cells = world.world.terrain_of(&map).map(|t| t.cells.clone());
    let cmd = wv_edit::command::PlaceBiomeMarkerCommand::new(
        map.clone(),
        cell_size,
        day,
        place,
        ring.to_vec(),
    );
    let mut tx = Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            // 纹理脏区：生成分支改了格子才需要重传。
            if let (Some(before), Some(after)) = (
                before_cells,
                world.world.terrain_of(&map).map(|t| t.cells.clone()),
            ) {
                if let Some(rect) = cells_diff_rect(
                    &before,
                    &after,
                    world.world.terrain_of(&map).map(|t| t.width).unwrap_or(0),
                ) {
                    tex.mark_dirty(rect);
                }
            }
            Ok(id)
        }
        Err(e) => {
            bevy::log::warn!("生物群系标记失败: {e}");
            Err(format!("{e}"))
        }
    }
}

/// 重生成一个生物群系标记的边界（ADR-0029 M2 / spec D13 调整路径）。
///
/// 以 Place 已存的 `envelope` + `biome` 重跑识别/生成：识别分支格子逐位不动、
/// 只换边界 span 与锚点；生成分支重写格子并按前后差集标纹理脏区（与
/// [`place_biome_marker_now`] 同口径）。格尺寸由命令在 apply 内从目标图
/// raster 推导（C1 教训：动作层不经手，会话态 `GridConfig` 无从漂移）。
pub fn regenerate_marker_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    playback: &PlaybackState,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    place_id: wv_core::Id,
) -> Result<(), String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再重生成标记".into());
    }
    let place = world
        .world
        .distances
        .places
        .iter()
        .find(|p| p.id == place_id)
        .ok_or_else(|| format!("地点 `{place_id}` 不存在"))?;
    let (map, name) = (
        place
            .map
            .clone()
            .ok_or_else(|| format!("地点 `{place_id}` 未锚定地图"))?,
        place.name.clone(),
    );
    let before_cells = world.world.terrain_of(&map).map(|t| t.cells.clone());
    let cmd =
        wv_edit::command::RegenerateBiomeMarkerCommand::named(place_id, playback.current_day, name);
    let mut tx = Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            // 纹理脏区：生成分支改了格子才需要重传（识别分支 diff 为空）。
            if let (Some(before), Some(after)) = (
                before_cells,
                world.world.terrain_of(&map).map(|t| t.cells.clone()),
            ) {
                if let Some(rect) = cells_diff_rect(
                    &before,
                    &after,
                    world.world.terrain_of(&map).map(|t| t.width).unwrap_or(0),
                ) {
                    tex.mark_dirty(rect);
                }
            }
            Ok(())
        }
        Err(e) => {
            bevy::log::warn!("标记重生成失败: {e}");
            Err(format!("{e}"))
        }
    }
}

/// 两版 cells 的差异包围盒（**半开** `[x0,y0,x1,y1)`；与
/// `TerrainTexture::mark_dirty` / `patch_terrain_image` 的迭代口径一致）。
pub(crate) fn cells_diff_rect(
    before: &[wv_core::terrain::TerrainCellData],
    after: &[wv_core::terrain::TerrainCellData],
    width: u32,
) -> Option<[u32; 4]> {
    if width == 0 || before.len() != after.len() {
        return None;
    }
    let mut rect: Option<[u32; 4]> = None;
    for (i, (a, b)) in before.iter().zip(after.iter()).enumerate() {
        if a == b {
            continue;
        }
        let x = (i % width as usize) as u32;
        let y = (i / width as usize) as u32;
        rect = Some(match rect {
            // 半开：单格变化也要 ≥1 宽高，末端 +1。
            None => [x, y, x + 1, y + 1],
            Some(r) => [r[0].min(x), r[1].min(y), r[2].max(x + 1), r[3].max(y + 1)],
        });
    }
    rect
}

/// 当前 Unix 毫秒——地点 ID 的时间后缀（`place_marker_<unix_ms>`）。
///
/// 与 `region_panel_actions::current_unix_ms` 同款；ID 只需可读 + 低冲突，
/// 不需要全局计数器（工程是单机单作者，§4.4）。
fn current_unix_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod marker_action_tests {
    use super::*;

    fn flat(size: u32, biome: u8) -> wv_core::terrain::TerrainData {
        wv_core::terrain::TerrainData {
            width: size,
            height: size,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 100,
                    surface: 1,
                    biome,
                };
                (size * size) as usize
            ],
        }
    }

    fn test_terrain_texture(size: u32) -> wv_render::terrain_view::TerrainTexture {
        wv_render::terrain_view::TerrainTexture {
            handle: bevy::prelude::Handle::default(),
            size,
            height: size,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            field_dirty: None,
            last_mode: None,
            last_soften: false,
            last_theme_id: wv_render::theme::theme_pack::builtin::DEFAULT.id,
            last_hypsometric: 0.0,
            last_map: None,
        }
    }

    fn marker_world_with_extent() -> (WorldProjection, EditHistory, wv_core::Id) {
        let mut world = WorldProjection::default();
        // extent 512 / 栅格 64 格 → 重生成推导 cell_size=8，与创建口径一致。
        world.world.maps.push(wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked("world_main"),
            name: "世界".into(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(512.0, 512.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });
        world.world.set_terrain(
            wv_core::Id::new_unchecked("world_main"),
            wv_core::terrain::TerrainData {
                width: 64,
                height: 64,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height: 100,
                        surface: 1,
                        biome: 3,
                    };
                    64 * 64
                ],
            },
        );
        (
            world,
            EditHistory::default(),
            wv_core::Id::new_unchecked("world_main"),
        )
    }

    /// 提交成功：生成分支重写格子 + 纹理脏区已标 + 一次 Undo 整体回退。
    #[test]
    fn regenerate_marker_now_commits_and_marks_texture_dirty() {
        let (mut world, mut edit, map) = marker_world_with_extent();
        let playback = PlaybackState {
            current_day: 42,
            ..PlaybackState::default()
        };
        let mut tex = test_terrain_texture(64);
        let ring = [(80.0, 80.0), (320.0, 80.0), (320.0, 320.0), (80.0, 320.0)];
        let id = place_biome_marker_now(
            &mut world,
            &mut edit,
            &playback,
            &mut tex,
            map.clone(),
            "黑森林".into(),
            wv_terrain::biome::BIOME_FOREST,
            &ring,
            8.0,
            42,
        )
        .expect("创建标记应成功");

        // 笔刷清空包络内森林 → 重生成必须走生成分支重写格子。
        let mut data = world.world.terrain_of(&map).cloned().unwrap();
        for y in 10..40 {
            for x in 10..40 {
                data.cells[y * 64 + x].biome = 3;
            }
        }
        world.world.set_terrain(map.clone(), data);

        regenerate_marker_now(&mut world, &mut edit, &playback, &mut tex, id.clone())
            .expect("重生成应成功");
        let after = world.world.terrain_of(&map).unwrap();
        assert_eq!(
            after.cells[20 * 64 + 20].biome,
            wv_terrain::biome::BIOME_FOREST,
            "包络内应重新生成森林"
        );
        assert!(
            tex.dirty.is_some(),
            "生成分支改了格子，纹理脏区必须已标（识别分支才可为 None）"
        );
        assert!(world.dirty, "提交成功后工作副本应标记为脏");
        // 一次提交 = 一次 Undo 单元：撤销后格子回到（已清空的）apply 前状态。
        assert!(edit.history.undo(&mut world.world).is_ok(), "应可撤销");
        let undone = world.world.terrain_of(&map).unwrap();
        assert_eq!(undone.cells[20 * 64 + 20].biome, 3, "撤销后应回到清空状态");
    }

    /// 播放中只读拒绝；不存在/未锚定的地点给人话错误。
    #[test]
    fn regenerate_marker_now_rejects_while_playing() {
        let (mut world, mut edit, _map) = marker_world_with_extent();
        let playback = PlaybackState {
            playing: true,
            ..PlaybackState::default()
        };
        let mut tex = test_terrain_texture(64);
        let err = regenerate_marker_now(
            &mut world,
            &mut edit,
            &playback,
            &mut tex,
            wv_core::Id::new_unchecked("place_ghost"),
        )
        .expect_err("播放中必须拒绝");
        assert!(err.contains("播放中"), "实得 {err}");

        let idle = PlaybackState::default();
        let err = regenerate_marker_now(
            &mut world,
            &mut edit,
            &idle,
            &mut tex,
            wv_core::Id::new_unchecked("place_ghost"),
        )
        .expect_err("幽灵地点应给人话错误");
        assert!(err.contains("place_ghost"), "实得 {err}");
    }

    #[test]
    fn cells_diff_rect_is_half_open_and_covers_the_last_cell() {
        let before = flat(4, 3);
        let mut after = before.clone();
        after.cells[3 * 4 + 3].biome = 1; // 右下角单格
        let rect = cells_diff_rect(&before.cells, &after.cells, 4).expect("应有差异");
        assert_eq!(rect, [3, 3, 4, 4], "半开区间且覆盖末格");
    }

    #[test]
    fn cells_diff_rect_none_when_identical() {
        let before = flat(4, 3);
        assert_eq!(cells_diff_rect(&before.cells, &before.cells, 4), None);
    }
}
