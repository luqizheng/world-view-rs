//! `DrillState` 同步：根图钉定 + 当前图镜像。
//!
//! 镜像方向只有一个（`DrillState` → `ViewState`），两份状态在同一帧内保持一致。

use bevy::prelude::*;

use crate::state::{DrillState, GridConfig, ViewState, WorldProjection};
use wv_render::zoom_level::fit_scale;
use wv_render::LOD_FALLBACK_FIT;

/// 把钻取路径钉到根图（`world.meta.root_map`），并镜像到 [`ViewState::current_map`]。
///
/// 放在 `PreUpdate`：本帧的渲染 / 拾取随后都读得到一致的两份状态。
/// 同时维护 [`ViewState::lod_fit`]（ADR-0103）：`fit_scale(当前图幅面, 窗口)`
/// ——LOD 比例阈值的分母。`GridConfig.map_size` 就是当前图 `extent` 的镜像
/// （`sync_grid_config` 维护），非有限/非正时 `fit_scale` 内部已兜
/// [`LOD_FALLBACK_FIT`] 同值（zoom_level 的 sanitize 即 4096 口径）。
pub(crate) fn seed_drill_state(
    projection: Res<WorldProjection>,
    grid: Res<GridConfig>,
    mut drill: ResMut<DrillState>,
    mut view: ResMut<ViewState>,
) {
    drill.ensure_root(&projection.world);
    drill.sync_view(&mut view);
    view.lod_fit = fit_scale(grid.map_size, view.window_size);
}

/// 每帧把 `DrillState` 的当前图写回 `ViewState::current_map` 镜像。
///
/// **方向只有一个**：`DrillState` → `ViewState`。反向赋值会让面包屑与
/// 世界树各说各话（`current_map` 没有「从哪来」的信息，表达不了路径）。
pub(crate) fn sync_drill_mirror(drill: Res<DrillState>, mut view: ResMut<ViewState>) {
    if drill.is_changed() {
        drill.sync_view(&mut view);
    }
}
