//! 自然区域的动作层（ADR-0045 D2 / D4 / D5 / D6）。
//!
//! ## 职责
//! 面板 / 命令面板只负责**收集用户输入**；所有「提交 `EditCommand` 到
//! `EditHistory`」的动作都在这里集中（与 `region_panel_actions.rs` 同构）。
//!
//! ## 关键流程：命名即物化（ADR-0045 D4）
//!
//! ```text
//! 作者点中地图某格
//!   → 找到该格所在的 biome 连通区（wv_terrain::region::biome_region_at）
//!   → 起名
//!   → 物化成一个 kind = nature 的区域实体（落 regions/<id>.yaml）
//! ```
//!
//! **自动分区候选本身不落盘**（D4）：几百个连通区若全落盘就是几百个 YAML 文件
//! + 大量会过期的多边形；只有**被作者命名**的才进工程。

#![cfg(feature = "app")]

use bevy::ecs::change_detection::DetectChanges;
use bevy::prelude::{Local, Res, ResMut};

use crate::edit::editing::EditHistory;
use crate::state::{ViewState, WorldProjection};

// 候选区预览的输入 Resource（`CandidateRegionInput`）定义在渲染层
// （`wv_render::region_outline`）——渲染系统消费它，app 层只写。
use wv_core::model::region::{Region, RegionKind};
use wv_core::Id;
use wv_edit::command::EditCommand;
use wv_edit::history::Transaction;
use wv_edit::nature_region::{
    MaterializeNatureRegionCommand, MergeNatureRegionCommand, RegenerateNatureRegionCommand,
    SetRegionKindCommand, SetRegionOverlayCommand,
};

/// 光标所在格的**候选区信息**（群系编号 + 格数）——面板用来告诉作者
/// 「你正在物化的是什么」（ADR-0045 D4）。
///
/// 与 [`materialize_nature_region_now`] 用**同一套**连通块判定，所以面板显示的
/// 与实际物化出来的必然一致。
pub fn candidate_at(world: &WorldProjection, map: &Id, cell: (i32, i32)) -> Option<(u8, usize)> {
    let data = world.world.terrain_of(map)?;
    let cells: Vec<wv_terrain::TerrainCell> = data
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let grid = wv_terrain::TerrainGrid::from_cells(data.width, data.height, cells)?;
    let region = wv_terrain::region::biome_region_at(&grid, cell.0, cell.1 as i64)?;
    Some((region.biome, region.cells))
}

/// 物化一个自动分区候选区（ADR-0045 D4「命名即物化」）。
///
/// `cell` = 栅格坐标（由调用方按 `cell_pick` 口径从光标反查）。
///
/// 返回新区域的 ID；失败给作者一句人话（V1：不阻止、不 panic）。
#[allow(clippy::too_many_arguments)]
pub fn materialize_nature_region_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    name: String,
    cell: (i32, i32),
    cell_size: f32,
    offset: (f32, f32),
) -> Result<Id, String> {
    if cell_size <= 0.0 || !cell_size.is_finite() {
        return Err("格尺寸必须为正".into());
    }
    let map = Id::new_unchecked(&view.current_map);
    let data = world
        .world
        .terrain_of(&map)
        .ok_or_else(|| format!("地图 `{}` 没有地形栅格", view.current_map))?;
    let cells: Vec<wv_terrain::TerrainCell> = data
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let grid = wv_terrain::TerrainGrid::from_cells(data.width, data.height, cells)
        .ok_or_else(|| "地形栅格与尺寸不符".to_string())?;
    let region = wv_terrain::region::biome_region_at(&grid, cell.0, cell.1 as i64)
        .ok_or_else(|| "该位置没有可物化的自然区（点上陆地或水域）".to_string())?;

    // 格坐标 → 图内局部 wu（**加回**两套原点的差：区域多边形是图内局部 wu，
    // 而格子属于栅格所在的图——子图上两者不是同一坐标系）。
    let polygon = region
        .outer
        .iter()
        .flat_map(|&(x, y)| [x * cell_size + offset.0, y * cell_size + offset.1])
        .collect();
    let holes: Vec<Vec<f32>> = region
        .holes
        .iter()
        .map(|h| {
            h.iter()
                .flat_map(|&(x, y)| [x * cell_size + offset.0, y * cell_size + offset.1])
                .collect()
        })
        .collect();

    let id = Id::new_unchecked(format!("region_nature_{}", current_unix_ms()));
    let r = Region {
        id: id.clone(),
        name,
        map,
        polygon,
        holes,
        kind: RegionKind::Nature,
        parent: None,
        overlay: None,
    };
    push_cmd(
        world,
        edit,
        view,
        Box::new(MaterializeNatureRegionCommand::new(r)),
    );
    Ok(id)
}

/// 合并两个自然区（ADR-0045 D5：几何并集 + 旧成员变子块）。
///
/// 两区**不相邻**时拒绝（并集得到多块外环，v1 不支持）。
#[allow(clippy::too_many_arguments)]
pub fn merge_nature_regions_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    a: Id,
    b: Id,
    new_name: String,
    cell_size: f32,
    offset: (f32, f32),
) -> Result<Id, String> {
    let new_id = Id::new_unchecked(format!("region_nature_{}", current_unix_ms()));
    let cmd = MergeNatureRegionCommand::new(a, b, new_id.clone(), new_name, cell_size, offset);
    commit_cmd(world, edit, view, Box::new(cmd), "合并自然区")?;
    Ok(new_id)
}

/// 重生成自然区域边界（ADR-0045 D6：按当前格子重算）。
pub fn regenerate_nature_region_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    cell_size: f32,
    offset: (f32, f32),
) -> Result<(), String> {
    let cmd = RegenerateNatureRegionCommand::new(id, cell_size, offset);
    commit_cmd(world, edit, view, Box::new(cmd), "重生成区域边界")
}

/// 设置 / 清除区域的**覆盖物键**（ADR-0045 D2）。
///
/// 键的词表由主题拥有；当前主题查无此键 → 地图上画**缺失覆盖物标记**
/// （白底红斜线），校验面板给一条提示（V1：不阻止保存）。
pub fn set_region_overlay_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    overlay: Option<String>,
) -> Result<(), String> {
    let cmd = SetRegionOverlayCommand::new(id, overlay);
    commit_cmd(world, edit, view, Box::new(cmd), "设置区域覆盖物")
}

/// 设置区域的**类别**（普通区域 ↔ 自然区，ADR-0045 D3）。
pub fn set_region_kind_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    kind: wv_core::model::region::RegionKind,
) -> Result<(), String> {
    let cmd = SetRegionKindCommand::new(id, kind);
    commit_cmd(world, edit, view, Box::new(cmd), "设置区域类别")
}

/// 每帧刷新候选区预览的输入（ADR-0045 D4 的可视化面）。
///
/// 读光标格（`cell_pick`）→ 查该格的 biome 连通区 → 换算成视空间环。
/// **节流**：光标格没变就不重查（连通区计算是 O(格数)，147k 格毫秒级，
/// 但没必要每帧算）。
pub fn sync_candidate_input(
    view: Res<crate::state::ViewState>,
    grid: Res<crate::state::GridConfig>,
    projection: Res<WorldProjection>,
    mut out: ResMut<wv_render::region_outline::CandidateRegionInput>,
    mut last_cell: Local<Option<(i32, i64)>>,
) {
    let cursor_cell = view.cursor_world.and_then(|c| {
        let map = wv_core::Id::new_unchecked(&view.current_map);
        let terrain = projection.world.terrain_of(&map)?;
        crate::ui::cell_pick::pick_at((c.x, c.y), &grid, terrain).map(|p| (p.gx, p.gy as i64))
    });
    if cursor_cell == *last_cell {
        return; // 光标格没变 → 环不变
    }
    *last_cell = cursor_cell;
    let Some((gx, gy)) = cursor_cell else {
        out.ring = None;
        return;
    };
    let map = wv_core::Id::new_unchecked(&view.current_map);
    let Some(data) = projection.world.terrain_of(&map) else {
        out.ring = None;
        return;
    };
    let cells: Vec<wv_terrain::TerrainCell> = data
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    let Some(gr) = wv_terrain::TerrainGrid::from_cells(data.width, data.height, cells) else {
        out.ring = None;
        return;
    };
    let Some(region) = wv_terrain::region::biome_region_at(&gr, gx, gy) else {
        out.ring = None;
        return;
    };
    // 格坐标 → 图内局部 wu → 视空间（加回两套原点的差）。
    let offset = grid.map_origin - grid.raster_origin;
    let cell = grid.cell_size;
    let mut ring: Vec<bevy::math::Vec2> = region
        .outer
        .iter()
        .map(|&(x, y)| bevy::math::Vec2::new(x * cell + offset.x, y * cell + offset.y))
        .collect();
    ring.dedup();
    out.ring = (ring.len() >= 3).then_some(ring);
}

/// 提交一条命令（失败给作者一句人话）。
fn commit_cmd(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    cmd: Box<dyn EditCommand>,
    what: &str,
) -> Result<(), String> {
    let mut tx = Transaction::new();
    tx.push(cmd);
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            let _ = view;
            Ok(())
        }
        Err(e) => {
            bevy::log::warn!("{what}失败: {e}");
            Err(format!("{what}失败：{e}"))
        }
    }
}

/// 提交命令（不返回结果——用于「成功与否都由作者看见」的场景）。
fn push_cmd(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    cmd: Box<dyn EditCommand>,
) {
    let mut tx = Transaction::new();
    tx.push(cmd);
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            let _ = view;
        }
        Err(e) => {
            bevy::log::warn!("自然区域编辑失败: {e}");
        }
    }
}

/// 毫秒时间戳（ID 后缀；与 `faction_panel_actions.rs` 同款）。
fn current_unix_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// CellMap 候选点选（2026-10-22 内容矩阵 §2.6-4）：EditCell 点选的格落在
/// **未命名候选**内且 Nature 开关开启 → 记为候选命名上下文（Inspector 出
/// 命名表单）；落在候选外 → 清上下文。只读判定 + 写会话态，不经命令。
pub fn pick_candidate_on_click(
    edit_cell: Res<crate::state::EditCellState>,
    view: Res<crate::state::ViewState>,
    playback: Res<crate::state::PlaybackState>,
    projection: Res<crate::state::WorldProjection>,
    drill: Res<wv_render::state::DrillState>,
    mut nature: ResMut<crate::state::NatureRegionUiState>,
) {
    use crate::state::ViewMode;
    if !view.nature_overlay || !matches!(view.mode, ViewMode::Topographic) {
        return;
    }
    if !edit_cell.is_changed() || playback.playing {
        return;
    }
    let Some((gx, gy)) = edit_cell.selected else {
        return;
    };
    let Some(map) = drill.current() else {
        return;
    };
    nature.pending_candidate = candidate_at(&projection, map, (gx, gy)).map(|_| (gx, gy));
}



/// 候选过期的编辑信号（2026-10-22 内容矩阵）：把 `EditHistory` 的形状
/// （undo/redo 栈长 + 顶 seq + 脏标记）折成一个 epoch 写进候选缓存——
/// **渲染层不得依赖 wv-edit/wv-app 类型**（分层红线），故信号由本系统
/// app 侧派生后写入 wv-render 的缓存资源。任一提交/撤销/重做都会翻转。
pub fn sync_candidate_invalidation(
    edit: Res<crate::edit::editing::EditHistory>,
    projection: Res<crate::state::WorldProjection>,
    mut cache: ResMut<wv_render::region_overlay::CandidateOverlayCache>,
) {
    let h = &edit.history;
    let epoch = {
        // FNV-1a 简易混合（形状元组 → u64）；避免引额外依赖。
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for v in [
            projection.dirty as u64,
            h.len() as u64,
            h.can_redo() as u64,
            h.undo_top_seq().unwrap_or(u64::MAX),
            h.redo_top_seq().unwrap_or(u64::MAX),
        ] {
            hash ^= v;
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        }
        hash
    };
    cache.last_epoch = epoch;
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::MapLevel;
    use wv_core::map::MapNode;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    /// 4×4 栅格：全陆地、全群系 3（草原）。
    fn world_with_terrain() -> (WorldProjection, EditHistory) {
        let mut world = WorldProjection::default();
        world.world.maps.push(MapNode {
            id: Id::new_unchecked("map_world"),
            name: "世界".into(),
            level: MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(4.0, 4.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });
        world.world.set_terrain(
            Id::new_unchecked("map_world"),
            TerrainData {
                width: 4,
                height: 4,
                cells: vec![
                    TerrainCellData {
                        height: 100,
                        surface: 1,
                        biome: 3,
                    };
                    16
                ],
            },
        );
        (world, EditHistory::default())
    }

    fn view() -> ViewState {
        ViewState {
            current_map: "map_world".into(),
            ..Default::default()
        }
    }

    #[test]
    fn materialize_nature_region_creates_a_nature_region_at_the_clicked_cell() {
        let (mut world, mut edit) = world_with_terrain();
        let id = materialize_nature_region_now(
            &mut world,
            &mut edit,
            &view(),
            "老君山".into(),
            (1, 1),
            1.0,
            (0.0, 0.0),
        )
        .expect("物化应成功");
        let r = world
            .world
            .regions
            .iter()
            .find(|r| r.id == id)
            .expect("区域应已创建");
        assert!(r.is_nature(), "物化产出自然区");
        assert_eq!(r.name, "老君山");
        assert!(!r.polygon.is_empty(), "应有外环几何");
        assert!(r.overlay.is_none(), "新物化的区域无覆盖物声明");
        assert!(world.dirty, "提交后工作副本应标脏");
    }

    #[test]
    fn materialize_rejects_a_cell_without_a_region() {
        let (mut world, mut edit) = world_with_terrain();
        let err = materialize_nature_region_now(
            &mut world,
            &mut edit,
            &view(),
            "虚空".into(),
            (99, 99),
            1.0,
            (0.0, 0.0),
        )
        .expect_err("越界格应被拒");
        assert!(err.contains("没有可物化"), "错误文案应说明原因：{err}");
    }

    #[test]
    fn set_region_overlay_records_the_key() {
        let (mut world, mut edit) = world_with_terrain();
        let id = materialize_nature_region_now(
            &mut world,
            &mut edit,
            &view(),
            "迷幻深林".into(),
            (0, 0),
            1.0,
            (0.0, 0.0),
        )
        .expect("物化应成功");
        set_region_overlay_now(
            &mut world,
            &mut edit,
            &view(),
            id.clone(),
            Some("conifer".into()),
        )
        .expect("设置覆盖物应成功");
        let r = world
            .world
            .regions
            .iter()
            .find(|r| r.id == id)
            .expect("区域");
        assert_eq!(r.declared_overlay(), Some("conifer"));
    }
}
