//! 网格同步系统：`sync_grid_config`。
//!
//! ## 职责
//! - `sync_grid_config` —— 维护 [`crate::state::GridConfig`] 的**两套原点**：
//!   `raster_origin` / `cell_size` 取自**纹理实际来源图**；`map_origin` / `map_size`
//!   取自**当前图**（与底图继承自谁无关）。
//!
//! ## 边界（不做什么）
//! - **不重建纹理** —— 那是 [`super::rebuild::rebuild_texture_on_mode_change`]。
//! - **不增量 patch** —— 那是 [`super::cells::patch_terrain_image`]。
//!
//! ## 主要导出
//! - [`sync_grid_config`]

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{DrillState, GridConfig, WorldProjection};

use super::terrain_resolve::active_terrain;
use super::terrain_texture::TerrainTexture;

/// 网格几何的默认配置（与纹理尺寸挂钩）。
///
/// 坐标口径统一到**地图 extent 坐标系**（`[0, extent]`）：Place、MapNode.icons、
/// 事件落点全部以 extent 为参照；网格若自作主张把原点挪到 `-extent/2`，
/// 地形四边形就会与所有标记错开半幅——主视口「看起来全黑」的根因之一。
///
/// 本系统维护 [`GridConfig`] 的**两套原点**（分工见该类型的文档表）：
///
/// - [`GridConfig::raster_origin`] / `cell_size` 取自**纹理的实际来源图**
///   （当前图，或它向上继承到的那张）。城市图自带 1024² 栅格时，格子边长与
///   栅格原点都要跟着城市走；继续按世界图 4096² 算，城市地形会被拉到 1/4
///   大小并与标记错位。
/// - [`GridConfig::map_origin`] / `map_size` 取自**当前图**（`drill.current()`），
///   与底图继承自谁无关。街道 / 地点 / 地图外框都靠它落位。
///
/// 早退条件里的 `drill.is_changed()` 不是可选优化：钻入一座**继承**父图底图的
/// 城市时纹理一个像素都没变（`tex` 不变），只有当前图变了；只看纹理就会漏掉
/// 这次变化，`map_origin` 停在世界原点，整座城的街道被画到视野外——
/// 「钻进武昌城看不到武昌」的根因。
pub fn sync_grid_config(
    tex: Res<TerrainTexture>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut grid: ResMut<GridConfig>,
) {
    if !tex.is_added() && !tex.is_changed() && !drill.is_changed() {
        return;
    }
    // 来源图（纹理正在显示的那张）决定尺寸与栅格原点；无来源时回退根图 / 4096²。
    let source = active_terrain(&projection.world, drill.current()).map(|(node, _)| node);
    let extent = source
        .and_then(|n| n.extent.as_ref())
        .map(|e| e.width.max(e.height))
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
                .map(|e| e.width.max(e.height))
        })
        .unwrap_or(4096.0);
    // 纹理 size 格 × cell_size = extent，一格一格对齐地图。
    grid.cell_size = (extent / tex.size as f32).max(0.01);
    // 栅格原点取来源图在世界坐标里的落点（`coord` 是唯一实现点，基线 §2.15）。
    grid.raster_origin = source
        .and_then(|n| wv_core::resolve_frame_in(&projection.world.maps, &n.id))
        .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
        .unwrap_or(Vec2::ZERO);
    // 图帧原点取**当前图**的落点（`drill::map_origin` 是唯一实现点），
    // 幅面取当前图的 extent；两者都不看底图继承自谁。
    let current = drill.current_node(&projection.world);
    let map_origin = current
        .map(|n| crate::drill::map_origin(&projection.world, n, &grid))
        .unwrap_or(grid.raster_origin);
    let map_size = current
        .and_then(|n| n.extent.as_ref())
        .map(|e| e.width.max(e.height))
        // 没有 extent（内存造的图）→ 退回「栅格能盖住的范围」，与旧行为一致。
        .unwrap_or(tex.size as f32 * grid.cell_size);
    grid.map_origin = map_origin;
    grid.map_size = map_size;
}
