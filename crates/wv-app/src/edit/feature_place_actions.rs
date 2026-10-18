//! 事实地点（地界）→ 提交 `wv_edit::command` 的动作层（theme overlay Task 7）。
//!
//! ## 职责
//! - [`add_feature_place_now`]：把选区意图环 + 作者输入的名称，
//!   包成一条 [`AddFeaturePlaceCommand`] 提交到 [`EditHistory`]。
//!
//! ## 设计依据
//!
//! 与 `region_panel_actions.rs` 同构：UI 只收集输入，提交集中在动作层；
//! 单条命令包成 `Transaction`（一次编辑 = 一次 Undo 单元）。入口是选择工具
//! M4 的「森林」意图（`FeaturePlaceDialog`），不是独立工具。
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
use wv_core::map::{GeometryData, GeometrySpan};
use wv_core::time::TimeSpan;
use wv_edit::command::AddFeaturePlaceCommand;
use wv_edit::history::Transaction;

/// 放置一个生物群系标记（ADR-0029 / 命名段 M1 Task 4）。
///
/// 与旧 `add_feature_place_now` 的区别：走 `PlaceBiomeMarkerCommand`——
/// 包络是意图、边界由内核识别/生成、生成分支写地形格子，三件事单事务原子。
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
    let map = world
        .world
        .distances
        .places
        .iter()
        .find(|p| p.id == place_id)
        .and_then(|p| p.map.clone())
        .ok_or_else(|| format!("地点 `{place_id}` 不存在或未锚定地图"))?;
    let before_cells = world.world.terrain_of(&map).map(|t| t.cells.clone());
    let cmd = wv_edit::command::RegenerateBiomeMarkerCommand::new(place_id, playback.current_day);
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

/// 把一圈地界落成事实地点 + 初始地界。
///
/// ## 为什么地点与地界一条命令
///
/// 与 [`AddFeaturePlaceCommand`] 的文档同源：地理事实不是先有点、再补范围，
/// 拆成两条命令会让 Undo 出现「地点还在、地界没了」的中间态（P6 要求一次
/// 编辑整体回退）。
///
/// ## 调用方
/// 选择工具 M4 的「森林」意图：`FeaturePlaceDialog` 确认时调用。
///
/// ## 参数
/// - `name`：作者在命名对话框输入的名称（调用方已 trim / 非空校验）；
/// - `feature`：事实类型编号（工具条写 `FEATURE_FOREST`）；
/// - `map`：**捕获图 id**（意图按钮点击那一刻 `drill.current()` 钉住、由
///   `FeaturePlaceDialog` 自持的图）。环坐标只在捕获图的局部帧里合法，这里
///   **不用也不看** `view.current_map`：命名对话框开着时作者仍可下钻换图，
///   现取当前图会把环写进错误的图、带着越帧坐标静默落盘（final review
///   数据完整性修复）；
/// - `ring`：**捕获图局部 wu** 的闭合环（首尾不重复，≥3 点）。
///
/// ## 返回
/// 成功返回新地点 ID；环退化 / 命令被拒（如 ID 冲突）返回 `None`。
/// 失败只记 warn 不抛——与区域动作层的 V1 守约同口径（不阻止作者继续编辑）。
pub fn add_feature_place_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    playback: &PlaybackState,
    name: String,
    feature: u8,
    map: &wv_core::Id,
    ring: &[(f32, f32)],
) -> Option<wv_core::Id> {
    // 环退化守门；地图 id 由 `try_close` 从 `drill.current()` 钉来，
    // 天然非空（点击系统只在有当前图时才落点），无需再查空串。
    if ring.len() < 3 {
        return None;
    }
    let map = map.clone();
    let id = wv_core::Id::new_unchecked(format!("place_feature_{}", current_unix_ms()));
    // `x/y/w/h` 留空：`AddFeaturePlaceCommand::apply` 会按环重拟合锚点，
    // 动作层不复制拟合公式（唯一实现点在地点模型里）。
    let place = Place {
        id: id.clone(),
        name,
        feature: Some(feature),
        biome: None,
        envelope: None,
        map: Some(map.clone()),
        x: None,
        y: None,
        w: None,
        h: None,
        rank: None,
    };
    let flat: Vec<f32> = ring.iter().flat_map(|&(x, y)| [x, y]).collect();
    let territory = GeometrySpan {
        map,
        owner: id.clone(),
        span: TimeSpan {
            from: playback.current_day,
            to: None,
        },
        geometry: GeometryData::Polygon(flat),
        relation: None,
    };

    let mut tx = Transaction::new();
    tx.push(Box::new(AddFeaturePlaceCommand::new(place, territory)));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            Some(id)
        }
        Err(e) => {
            bevy::log::warn!("地界创建失败: {e}");
            None
        }
    }
}

/// 当前 Unix 毫秒——地点 ID 的时间后缀（`place_feature_<unix_ms>`）。
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
mod tests {
    use super::*;
    use wv_core::map::GeometryData;
    use wv_render::theme::overlays::FEATURE_FOREST;

    /// 提交后：地点 1 条（feature=Some(4)）、地界 1 条（开放区间从当前日）、
    /// 锚点由命令按环拟合，返回新 ID。
    #[test]
    fn add_feature_place_now_writes_open_span_and_anchors() {
        let mut world = WorldProjection::default();
        let mut edit = EditHistory::default();
        let playback = PlaybackState {
            current_day: 42,
            ..PlaybackState::default()
        };
        let ring = [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)];
        let map = wv_core::Id::new_unchecked("world_main");

        let id = add_feature_place_now(
            &mut world,
            &mut edit,
            &playback,
            "黑森林".into(),
            FEATURE_FOREST,
            &map,
            &ring,
        )
        .expect("四点环应提交成功");

        assert_eq!(
            world.world.distances.places.len(),
            1,
            "应恰好新增 1 条事实地点"
        );
        let place = &world.world.distances.places[0];
        assert_eq!(place.id, id, "返回的 ID 必须是入库的地点 ID");
        assert_eq!(place.feature, Some(FEATURE_FOREST), "事实类型应为森林");
        assert_eq!(place.name, "黑森林");
        assert_eq!(
            place.map.as_ref().map(wv_core::Id::as_str),
            Some("world_main"),
            "地点应锚定到捕获图"
        );
        assert_eq!(place.x, Some(20.0), "x 应为环质心");
        assert_eq!(place.y, Some(20.0), "y 应为环质心");
        assert_eq!(place.w, Some(20.0), "w 应为包围盒宽");
        assert_eq!(place.h, Some(20.0), "h 应为包围盒高");

        assert_eq!(world.world.place_geometry.len(), 1, "应恰好新增 1 条地界");
        let geo = &world.world.place_geometry[0];
        assert_eq!(geo.owner, id, "地界 owner 应是新地点");
        assert_eq!(geo.map.as_str(), "world_main", "地界 map 应是捕获图");
        assert_eq!(geo.span.from, 42, "span.from 应取播放当前日");
        assert_eq!(geo.span.to, None, "新建地界必须是开放区间（至今）");
        assert_eq!(
            geo.geometry,
            GeometryData::Polygon(vec![10.0, 10.0, 30.0, 10.0, 30.0, 30.0, 10.0, 30.0]),
            "环应扁平化为 [x0,y0,x1,y1,...]"
        );

        assert!(world.dirty, "提交成功后工作副本应标记为脏");
        // 一次提交 = 一次 Undo 单元：撤销后地点与地界一起消失（P6）。
        assert!(edit.history.undo(&mut world.world).is_ok(), "应可撤销");
        assert!(world.world.distances.places.is_empty(), "撤销后地点应移除");
        assert!(
            world.world.place_geometry.is_empty(),
            "撤销后地界应一并移除"
        );
    }

    /// final review 数据完整性修复：提交**只认闭合时钉住的捕获图 id**。
    /// 命名对话框开着时作者钻进另一张图（`view.current_map` 已变）也不影响
    /// ——环坐标只在捕获图局部帧里合法，落错图等于带着越帧坐标静默写盘。
    /// 场景：环在 `city_zunyi` 里闭合，此刻视图已切回 `world_main`
    /// （对话框仍开着、地图背后仍可交互），提交必须落 `city_zunyi`。
    #[test]
    fn add_feature_place_now_lands_on_the_pinned_capture_map() {
        let mut world = WorldProjection::default();
        let mut edit = EditHistory::default();
        let playback = PlaybackState::default();
        let ring = [(0.0, 0.0), (64.0, 0.0), (64.0, 64.0), (0.0, 64.0)];
        // 捕获图 = 子图；对话框提交时视图可以停在别的图（默认 world_main）。
        let capture_map = wv_core::Id::new_unchecked("city_zunyi");

        let id = add_feature_place_now(
            &mut world,
            &mut edit,
            &playback,
            "城郊林地".into(),
            FEATURE_FOREST,
            &capture_map,
            &ring,
        )
        .expect("钉住的捕获图应提交成功");

        let place = &world.world.distances.places[0];
        assert_eq!(
            place.map.as_ref().map(wv_core::Id::as_str),
            Some("city_zunyi"),
            "地点必须落在闭合时钉住的捕获图，而不是提交时的当前图"
        );
        let geo = &world.world.place_geometry[0];
        assert_eq!(
            geo.map.as_str(),
            "city_zunyi",
            "地界必须与地点同图（同一捕获图 id）"
        );
        assert_eq!(geo.owner, id);
        assert!(
            edit.history.undo(&mut world.world).is_ok(),
            "钉图路径同样整体可撤销"
        );
    }
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
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
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
