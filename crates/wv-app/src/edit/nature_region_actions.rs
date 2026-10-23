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
    materialize_with_polygon(world, edit, view, name, cell, cell_size, offset, None)
}

/// 同 [`materialize_nature_region_now`]，但可带**作者编辑过的多边形**
/// （候选态拖锚点后物化用；`None` = 按格重新派生）。
#[allow(clippy::too_many_arguments)]
pub fn materialize_with_polygon(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    name: String,
    cell: (i32, i32),
    cell_size: f32,
    offset: (f32, f32),
    edited_polygon: Option<Vec<f32>>,
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
    // 作者拖过锚点 ⇒ 用编辑后的多边形（候选态调整形状）；否则按格派生。
    let polygon: Vec<f32> = edited_polygon.unwrap_or_else(|| {
        region
            .outer
            .iter()
            .flat_map(|&(x, y)| [x * cell_size + offset.0, y * cell_size + offset.1])
            .collect()
    });
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
    overlay_cache: Res<wv_render::region_overlay::CandidateOverlayCache>,
    mut out: ResMut<wv_render::region_outline::CandidateRegionInput>,
    mut cache: Local<CandidateRingCache>,
) {
    let map = wv_core::Id::new_unchecked(&view.current_map);
    let cursor_cell = view.cursor_world.and_then(|c| {
        let terrain = projection.world.terrain_of(&map)?;
        crate::ui::cell_pick::pick_at((c.x, c.y), &grid, terrain).map(|p| (p.gx, p.gy as i64))
    });
    // 派生身份：换图 / 换幅面 / 换网格换算 / 任何一次编辑提交都会变。
    let key = projection.world.terrain_of(&map).map(|t| CandidateKey {
        map: view.current_map.clone(),
        width: t.width,
        height: t.height,
        cells: t.cells.len(),
        grid_bits: [
            u64::from(grid.cell_size.to_bits()),
            u64::from(grid.raster_origin.x.to_bits()),
            u64::from(grid.raster_origin.y.to_bits()),
            u64::from(grid.map_origin.x.to_bits()),
            u64::from(grid.map_origin.y.to_bits()),
            u64::from(grid.map_size.x.to_bits()),
            u64::from(grid.map_size.y.to_bits()),
        ],
        epoch: overlay_cache.last_epoch,
    });
    if cache.key != key {
        *cache = CandidateRingCache {
            key,
            ..Default::default()
        };
        // 身份变了必须重算：`last_cell` 清空，否则「同格」会把它短路掉。
        out.ring = None;
    }
    if cursor_cell == cache.last_cell && cache.key.is_some() {
        return; // 光标格没变 → 环不变
    }
    cache.last_cell = cursor_cell;
    let Some((gx, gy)) = cursor_cell else {
        out.ring = None;
        return;
    };
    // 落进上一块连通区内部 ⇒ 环一模一样，**一次点包含判定**换掉整份重算。
    if cell_inside_rings(&cache.rings, gx, gy as i32) {
        out.ring = cache.view_ring.clone();
        return;
    }
    // 真换块了：地形网格只建一次（1024² 整份拷贝是这段的老大头）。
    if cache.grid.is_none() {
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
        cache.grid = wv_terrain::TerrainGrid::from_cells(data.width, data.height, cells);
    }
    let found = cache
        .grid
        .as_ref()
        .and_then(|gr| wv_terrain::region::biome_region_at(gr, gx, gy));
    let Some(region) = found else {
        cache.rings.clear();
        cache.view_ring = None;
        out.ring = None;
        return;
    };
    cache.rings = std::iter::once(region.outer.clone())
        .chain(region.holes.iter().cloned())
        .collect();
    // 格坐标 → 图内局部 wu → 视空间（加回两套原点的差）。
    let offset = grid.map_origin - grid.raster_origin;
    let cell = grid.cell_size;
    let mut ring: Vec<bevy::math::Vec2> = region
        .outer
        .iter()
        .map(|&(x, y)| bevy::math::Vec2::new(x * cell + offset.x, y * cell + offset.y))
        .collect();
    ring.dedup();
    cache.view_ring = (ring.len() >= 3).then_some(ring);
    out.ring = cache.view_ring.clone();
}

/// 候选环派生缓存（`Local`）——见 [`sync_candidate_input`] 的头注释。
///
/// **为什么必须有**：2026-10-23 用户实测「静止 120FPS / 鼠标移动 40FPS」。
/// 旧实现对**每一个新光标格**都重做一遍 `Vec<TerrainCell>` 全量拷贝
/// （1024² = 1,048,576 个）+ `TerrainGrid::from_cells` + `biome_region_at`
/// （内部再分配 1M `bool` 掩膜 + 4 邻域 BFS + 环提取）。鼠标一动就换格，
/// 于是每帧都要付这份钱——而 99% 的移动都落在**同一块连通区内部**，环根本没变。
#[derive(Default)]
pub struct CandidateRingCache {
    /// 上一次的光标格（同格直接返回）。
    last_cell: Option<(i32, i64)>,
    /// 派生依据的身份；变了整份丢弃。
    key: Option<CandidateKey>,
    /// 缓存的 `TerrainGrid`（只在地形真变时重建）。
    grid: Option<wv_terrain::TerrainGrid>,
    /// 上一次连通区的环（**格角坐标**，`f32`）：外环 + 洞环，供点包含判定。
    rings: Vec<Vec<(f32, f32)>>,
    /// 视空间环（`rings` 按当前 grid 换算后的结果，直接喂渲染层）。
    view_ring: Option<Vec<bevy::math::Vec2>>,
}

/// 派生身份（`PartialEq` 全字段比较，无哈希——环的重算比比较贵得多）。
#[derive(PartialEq, Eq, Clone)]
struct CandidateKey {
    map: String,
    width: u32,
    height: u32,
    cells: usize,
    /// 格尺寸 / 两套原点 / 幅面的位模式（换图、缩放基准、换投影都要重算）。
    grid_bits: [u64; 7],
    /// app 侧编辑信号（`CandidateOverlayCache::last_epoch` = history 形状 + dirty）。
    epoch: u64,
}

/// 格中心是否落在环集合内（even-odd 射线法；外环与洞环一起判即为「外环 − 洞」）。
///
/// 环来自 `ring_set_from_mask`，顶点是**格角**（`f32`，整数格角）；格 `(gx, gy)`
/// 取中心 `(gx+0.5, gy+0.5)` 判定，避免落在格线上时正负不定。
fn cell_inside_rings(rings: &[Vec<(f32, f32)>], gx: i32, gy: i32) -> bool {
    if rings.is_empty() {
        return false;
    }
    let (px, py) = (gx as f32 + 0.5, gy as f32 + 0.5);
    let mut inside = false;
    for ring in rings {
        if ring.len() < 3 {
            continue;
        }
        let mut j = ring.len() - 1;
        for i in 0..ring.len() {
            let (xi, yi) = ring[i];
            let (xj, yj) = ring[j];
            if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
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

/// 候选上下文同步（2026-10-22 验收）：把 app 侧 `NatureRegionUiState` 的候选格
/// 与编辑中多边形镜像到渲染层（渲染不依赖 wv-app 类型）。
pub fn sync_candidate_context(
    nature: Res<crate::state::NatureRegionUiState>,
    mut ctx: ResMut<wv_render::state::RegionCandidateContext>,
) {
    if !nature.is_changed() {
        return;
    }
    *ctx = wv_render::state::RegionCandidateContext {
        cell: nature.pending_candidate,
        polygon: nature.candidate_polygon.clone(),
    };
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

    // ───── 2026-10-23 · 光标热路径探针（手动跑）─────────────────────────────
    //
    // ```bash
    // cargo test -p wv-app --features app --lib -- --ignored --nocapture \
    //   nature_region_actions::tests::perf_probe_cursor_cell_change
    // ```
    //
    // 口径：1024² 地形（= lod-demo raster），`cell_size = 4.0`，光标**每帧换一格**
    // （= 鼠标在地图上移动）。先量「旧路径」单价的三个组成部分，再用
    // `perf_timed!` 量修好后的 `sync_candidate_input` 整系统耗时。
    #[test]
    #[ignore = "探针：1024² 地形 + 需 --ignored --nocapture 手动跑"]
    fn perf_probe_cursor_cell_change() {
        use bevy::ecs::schedule::IntoScheduleConfigs; // `perf_timed!` 里 `.chain()` 的来源
        use std::time::Instant;

        const SIDE: u32 = 1024;
        let cells: Vec<TerrainCellData> = (0..SIDE * SIDE)
            .map(|i| {
                let x = i % SIDE;
                let y = i / SIDE;
                TerrainCellData {
                    height: 100,
                    surface: 1,
                    // 64 格见方的棋盘 → 连通区边界密集，flood fill 真的在干活。
                    biome: if (x / 64 + y / 64) % 2 == 0 { 3 } else { 5 },
                }
            })
            .collect();
        let mut world = WorldProjection::default();
        world.world.maps.push(MapNode {
            id: Id::new_unchecked("map_world"),
            name: "世界".into(),
            level: MapLevel::World,
            parent: None,
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });
        let terrain = TerrainData {
            width: SIDE,
            height: SIDE,
            cells,
        };
        // ── 旧路径单价（每换一格都做一遍）：三段分开量 ──
        let t0 = Instant::now();
        let grid_cells: Vec<wv_terrain::TerrainCell> = terrain
            .cells
            .iter()
            .map(|c| wv_terrain::TerrainCell {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
            .collect();
        let clone_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = Instant::now();
        let gr = wv_terrain::TerrainGrid::from_cells(SIDE, SIDE, grid_cells).expect("栅格");
        let build_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let t2 = Instant::now();
        let region = wv_terrain::region::biome_region_at(&gr, 10, 10);
        let region_ms = t2.elapsed().as_secs_f64() * 1000.0;
        println!(
            "[PROBE] 旧路径单价：clone={clone_ms:.2}ms + from_cells={build_ms:.2}ms + \
             biome_region_at={region_ms:.2}ms = {:.2}ms/次（每换一格付一遍）",
            clone_ms + build_ms + region_ms
        );
        println!(
            "[PROBE] biome_region_at 结果：cells={} outer={} 点",
            region.as_ref().map(|r| r.cells).unwrap_or(0),
            region.as_ref().map(|r| r.outer.len()).unwrap_or(0)
        );

        // ── 同一个函数的「大片连通群系」单价（真实地形更接近这一档：一个连通区
        // 几十万格 ⇒ BFS 要走的格子数完全不同）──
        let big_cells: Vec<TerrainCellData> = (0..SIDE * SIDE)
            .map(|_| TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            })
            .collect();
        let big = TerrainData {
            width: SIDE,
            height: SIDE,
            cells: big_cells,
        };
        let b0 = Instant::now();
        let big_grid_cells: Vec<wv_terrain::TerrainCell> = big
            .cells
            .iter()
            .map(|c| wv_terrain::TerrainCell {
                height: c.height,
                surface: c.surface,
                biome: c.biome,
            })
            .collect();
        let big_clone = b0.elapsed().as_secs_f64() * 1000.0;
        let b1 = Instant::now();
        let big_gr = wv_terrain::TerrainGrid::from_cells(SIDE, SIDE, big_grid_cells).expect("栅格");
        let big_build = b1.elapsed().as_secs_f64() * 1000.0;
        let b2 = Instant::now();
        let big_region = wv_terrain::region::biome_region_at(&big_gr, 10, 10);
        let big_region_ms = b2.elapsed().as_secs_f64() * 1000.0;
        println!(
            "[PROBE] 旧路径单价（整张连成一片）：clone={big_clone:.2}ms + from_cells={big_build:.2}ms \
             + biome_region_at={big_region_ms:.2}ms = {:.2}ms/次（cells={}）",
            big_clone + big_build + big_region_ms,
            big_region.as_ref().map(|r| r.cells).unwrap_or(0)
        );

        world
            .world
            .set_terrain(Id::new_unchecked("map_world"), terrain);

        let mut app = bevy::prelude::App::new();
        app.insert_resource(world)
            .insert_resource(crate::state::GridConfig {
                cell_size: 4.0,
                raster_origin: bevy::math::Vec2::ZERO,
                map_origin: bevy::math::Vec2::ZERO,
                map_size: bevy::math::Vec2::splat(4096.0),
            })
            .insert_resource(crate::state::ViewState {
                cursor_world: Some(bevy::math::Vec2::new(100.0, 100.0)),
                current_map: "map_world".into(),
                ..Default::default()
            })
            .init_resource::<wv_render::region_overlay::CandidateOverlayCache>()
            .init_resource::<wv_render::region_outline::CandidateRegionInput>()
            .insert_resource(wv_render::perf::PerfRecorder::new(true))
            .add_systems(
                bevy::prelude::Update,
                wv_render::perf_timed!("sync_candidate_input", sync_candidate_input),
            );
        app.update(); // 暖机（首帧要建缓存）
        for frame in 0..30 {
            // 每帧右移一格 = 鼠标在地图上匀速移动。
            let x = 100.0 + frame as f32 * 4.0;
            app.world_mut()
                .resource_mut::<crate::state::ViewState>()
                .cursor_world = Some(bevy::math::Vec2::new(x, 100.0));
            app.update();
        }
        for (name, n, max, avg) in app
            .world()
            .resource::<wv_render::perf::PerfRecorder>()
            .snapshot()
        {
            println!("[PROBE] {name}: n={n} max={max:.2}ms avg={avg:.2}ms");
        }
    }
}
