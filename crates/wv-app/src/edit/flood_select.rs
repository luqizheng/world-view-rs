//! 泛洪的**结构转换**辅助（[ADR-0052] D6 · ADR-0086 收窄）。
//!
//! ## 泛洪工具的退役与遗留（ADR-0086，2026-10-29 用户裁定）
//!
//! 独立的「泛洪选择工具」（`MapToolId::FloodSelect` + 本模块原
//! `flood_select_on_click` 系统）已退役：ADR-0074 追记四裁定 CellMap 里
//! **点选工具的默认点击 = 泛洪**（点谁淹谁，同判据连通区进统一选区），
//! 与该工具行为完全重复——工具只是同一手势的第二个入口。
//!
//! 遗留两件仍活着：
//!
//! 1. **判据**（[`wv_world_cell_map::state::FloodSelectState`]）：点选
//!    默认泛洪按什么算「同值」，浮窗「泛洪判据」段编辑（跟随点选工具显示）；
//! 2. **判据的执行**：在 `edit::editing::cell::commit_point_select_on_click`
//!    的默认分支（走 [`wv_terrain::cellops::flood_region`]）。
//!
//! 本模块只剩世界地形 → `wv_terrain::TerrainGrid` 的结构转换（泛洪与
//! 派生运算共用一次的那步）。

/// 世界地形（`wv_core::TerrainData`）→ `wv_terrain::TerrainGrid`。
///
/// **为什么要转**：泛洪的连通性判定在 `wv_terrain::cellops` 里（地形域
/// 的纯函数），而世界数据是 `wv_core::TerrainData`（序列化格式）。
/// 这里做一次**结构转换**（不复制语义），代价 O(格数)。
pub(crate) fn to_terrain_grid(terrain: &wv_core::terrain::TerrainData) -> wv_terrain::TerrainGrid {
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
