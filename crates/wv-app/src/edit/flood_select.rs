//! 泛洪选择（[ADR-0052] D6 / D6-A · §9.1）。
//!
//! ## 交互形态（D6-A）
//!
//! **点一下立即泛洪**，结果直接进 `region_cells`——无「预览 → 确认」中间态。
//!
//! 理由：
//!
//! 1. 合理图幅下 `flood_region` 是毫秒级，预览增加一次「看 → 松手」的往返
//!    而不提供决策价值；
//! 2. 不满意可立刻点别处重新泛洪，**替换**零成本；
//! 3. 大图若慢，用底栏「泛洪中…」做感知补偿即可。
//!
//! **替换而非累加**：每次泛洪的结果**覆盖** `region_cells`。累加会产生
//! 多区域并集，后续批操作（smooth / quantize）的作用范围不可推演。

use bevy::prelude::*;

use wv_render::state::{EditCellState, GridConfig};
use wv_world_cell_map::state::{FloodCriterion, FloodSelectState};

use crate::state::{DrillState, PlaybackState, ViewState, WorldProjection};
use crate::ui::cell_pick;

/// **泛洪选择**：点一下 → 泛洪 → 选区替换。
///
/// gate：
/// - `flood_select.tool_active` 为 false → 不响应；
/// - 指针压在 egui 面板上 → 不响应（面板交互不是选格）；
/// - 时间轴播放中 → 不响应（投影态）；
/// - 无图 / 无地形 → 不响应。
#[allow(clippy::too_many_arguments)]
pub fn flood_select_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut edit_cell: ResMut<EditCellState>,
    flood_select: Res<FloodSelectState>,
    screen: Res<crate::app_screen::AppScreen>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if !flood_select.tool_active {
        return;
    }
    if screen.shows_loading_overlay() || ui.wants_pointer || playback.playing {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(target_map) = drill.current() else {
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        return;
    };
    let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) else {
        // 点在图外 → 不动选区（不误清）。
        return;
    };

    // 把世界地形转成 `wv_terrain::TerrainGrid` 供 `flood_region` 用。
    let grid_terrain = to_terrain_grid(terrain);
    let criterion = match flood_select.criterion {
        FloodCriterion::Surface => wv_terrain::cellops::FloodCriterion::Surface,
        FloodCriterion::Biome => wv_terrain::cellops::FloodCriterion::Biome,
        FloodCriterion::HeightTolerance(tol) => {
            wv_terrain::cellops::FloodCriterion::HeightTolerance(tol)
        }
    };
    let cells = wv_terrain::cellops::flood_region(&grid_terrain, pick.gx, pick.gy, criterion);
    if cells.is_empty() {
        return;
    }
    // **替换**（D6-A：不累加）。
    edit_cell.region_cells = cells;
    edit_cell.region_fingerprint = 0;
    edit_cell.last_map_id = Some(target_map.clone());
}

/// 世界地形（`wv_core::TerrainData`）→ `wv_terrain::TerrainGrid`。
///
/// **为什么要转**：泛洪的连通性判定在 `wv_terrain::cellops` 里（地形域
/// 的纯函数），而世界数据是 `wv_core::TerrainData`（序列化格式）。
/// 这里做一次**结构转换**（不复制语义），代价 O(格数)。
fn to_terrain_grid(terrain: &wv_core::terrain::TerrainData) -> wv_terrain::TerrainGrid {
    let cells = terrain
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    wv_terrain::TerrainGrid::from_cells(terrain.width, terrain.height, cells)
        .expect("cells.len() == width * height（世界数据自洽）")
}
