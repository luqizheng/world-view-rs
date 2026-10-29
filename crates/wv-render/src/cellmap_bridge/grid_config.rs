//! 栅格配置求值（`sync_grid_config`）：原文件孤悬在测试模块之后的收尾系统
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;


/// 网格同步（自 wv-render `terrain_view/grid_sync.rs` 迁入 bridge；ADR-0054 D5）：
/// 维护 `GridConfig` 的两套原点——`raster_origin` / `cell_size` 取**纹理实际来源图**，
/// `map_origin` / `map_size` 取**当前图**。早退条件里的 `drill.is_changed()` 不可省：
/// 钻入继承父图底图的城市时纹理不变、只有当前图变了（「钻进武昌看不到武昌」根因）。
pub fn sync_grid_config(
    tex: Res<wv_world_cell_map::terrain_view::terrain_texture::TerrainTexture>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut grid: ResMut<GridConfig>,
) {
    if !tex.is_added() && !tex.is_changed() && !drill.is_changed() {
        return;
    }
    let source = wv_world_cell_map::terrain_view::terrain_resolve::active_terrain(
        &projection.world,
        drill.current(),
    )
    .map(|(node, _)| node);
    let extent = source
        .and_then(|n| n.extent.as_ref())
        .map(|e| e.width)
        .or_else(|| {
            projection
                .world
                .meta
                .as_ref()
                .and_then(|m| {
                    projection
                        .world
                        .maps
                        .iter()
                        .find(|n| n.id == m.root_map)
                        .and_then(|n| n.extent.as_ref())
                })
                .map(|e| e.width)
        })
        .unwrap_or(4096.0);
    grid.cell_size = (extent / tex.size as f32).max(0.01);
    grid.raster_origin = source
        .and_then(|n| wv_core::resolve_frame_in(&projection.world.maps, &n.id))
        .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
        .unwrap_or(Vec2::ZERO);
    let current = drill.current_node(&projection.world);
    let map_origin = current
        .map(|n| crate::drill::map_origin(&projection.world, n, &grid))
        .unwrap_or(grid.raster_origin);
    let map_size = current
        .and_then(|n| n.extent.as_ref())
        .map(|e| Vec2::new(e.width, e.height))
        .unwrap_or(Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size);
    grid.map_origin = map_origin;
    grid.map_size = map_size;
}
