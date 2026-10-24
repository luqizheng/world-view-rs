//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! Plugin 拆分（ADR-0023 D5）—— 把 wv-render 的系统按"游戏可复用 / 编辑器专属 / 装饰族"
//! 三类分组，让游戏 app 只装想要的（§1.2 差距 4）。
//!
//! ## 三组分工
//!
//! - [`MapRenderPlugin`] —— 地图本体渲染（**游戏可用**）：
//!   地形 / 矢量层 / 标注 / 势力多边形 / 钻取 / 地图外框 / 街道渲染；
//!   不含任何编辑意图桥。
//! - [`EditorOverlayPlugin`] —— 编辑器专属（**游戏不必装**）：
//!   笔刷光标 / 测量 / 编辑意图桥 / 编辑态网格 / 编辑回执。
//! - [`MapShellPlugin`] —— 装饰族系统（**受 `TreatmentFlags` 控制**，游戏按需装）：
//!   纸面氛围（雾霭 + 纸张噪点）；M2 阶段该 Plugin **恒装载所有装饰系统**，
//!   开关逻辑 M3 推进（接入 `TreatmentFlags` 后）。
//!
//! ## 不重新实现逻辑
//! 三 Plugin 都只调 `add_systems` —— 当前 `app_bootstrap.rs` 的系统链**字面照搬**，
//! 只是搬到 Plugin 内。M1 行内调用的"350 行系统链"在 M2 验收时**未动**——
//! Plugin 是**新增装配入口**，**不**替代 bootstrap 的顺序链。
//!
//! ## 边界
//! - **不**做 Plugin 间的执行顺序约束（`chain()` 留在 bootstrap 内）；
//! - **不**做 Plugin 内子模块拆分（每 Plugin 内部保持平铺 `add_systems`）；
//! - **不**做热插拔 / 动态装载（M3 推迟）。

use bevy::prelude::*;

use crate::brush_cursor;
use crate::buildings;
use crate::cell_selection;
use crate::faction_overlays;
use crate::layer_frame;
use crate::map_frame;
use crate::measurement;
use crate::paper_overlay;
use crate::streets;
use crate::vector_layers;
use crate::world_projection;

/// 地图本体渲染 Plugin（**游戏可用**）。
///
/// 装配：地形 chunk 同步 / 等值线叠加 / 图层帧 / 矢量层 / 街道 / 势力 / 钻取 /
/// 地图外框 / 建筑 / 世界投影（读路径）。**不**含笔刷 / 测量 / 编辑桥。
pub struct MapRenderPlugin;

impl Plugin for MapRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // 投影（时间 → 渲染资源）
                world_projection::compute_projection,
                // 图层帧（ADR-0013）：先于一切消费它的渲染系统求值
                layer_frame::resolve_layer_frame,
                // 等值线矢量叠加（ADR-0012 / plan v0.1 M2）
                crate::cellmap_bridge::sync_isoline_overlay,
                // 地形分块 + mip + 视口拣选（优化项 3a/3b/3c）
                crate::cellmap_bridge::sync_terrain_chunks,
                // 纸纹叠加（ADR-0042）：斑驳 + 晕影，图幅变化时重建
                paper_overlay::sync_paper_overlay,
                // 街道图层缓存：当前图变化时重读（ADR-0010）
                streets::reload_street_cache,
                // 矢量叠加层（道路 + POI）
                vector_layers::sync_vector_layers,
                // 街道名标注
                streets::update_street_labels,
                // 势力多边形叠加层
                faction_overlays::sync_faction_overlays,
                // 地图外框 + 1/4 幅面参考网格
                map_frame::sync_map_frame,
            )
                .chain(),
        )
        // 2.5D 建筑挤出（ADR-0016 §6）
        .add_systems(
            Update,
            buildings::sync_building_overlays.after(vector_layers::sync_vector_layers),
        )
        // 地点缓存索引（A1）：compute_projection 的端点解析 / POI 收集查表用。
        .init_resource::<world_projection::PlaceIndex>()
        // CellMap 未命名候选高亮的缓存（2026-10-22 内容矩阵 T11）：
        // draw_map_gizmos ResMut 它——漏 init 即启动期 panic（同 EditCellState 教训）。
        .init_resource::<crate::region_overlay::CandidateOverlayCache>()
        // 纸纹叠层运行时状态（贴图句柄 + 当前台实体，ADR-0042）。
        .init_resource::<paper_overlay::PaperOverlay>();
    }
}

/// 编辑器专属 Plugin（**游戏不必装**）。
///
/// 装配：笔刷光标 / 测量 / 编辑桥 / 测量回执。游戏接入时**不**装本 Plugin 即
/// 拖入对应代码。
pub struct EditorOverlayPlugin;

impl Plugin for EditorOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<cell_selection::CellSelectionEntities>()
            .init_resource::<cell_selection::CellRegionEntities>()
            // hover 准星（[ADR-0052] D2）：双色描边 + 中心十字。
            .init_resource::<cell_selection::CellHoverEntities>()
            // 待落笔触色块（[ADR-0052] D3）：红橙半透填充。
            .init_resource::<cell_selection::CellStrokeEntities>()
            // 落笔会话态（[ADR-0052] D4）：`wv-app` 写、`wv-render` 读
            // （待落色块要读笔触格）。
            .init_resource::<wv_world_cell_map::cell_paint::CellPaintSession>()
            .add_systems(Update, brush_cursor::sync_brush_cursor)
            .add_systems(Update, crate::cellmap_bridge::sync_cell_selection)
            .add_systems(Update, crate::cellmap_bridge::sync_cell_region)
            .add_systems(Update, crate::cellmap_bridge::sync_cell_stroke)
            // hover 准星画在 selected **之上**（Z 序：D2 要求 hover 是最上层
            // 的瞬时反馈）。
            .add_systems(
                Update,
                crate::cellmap_bridge::sync_cell_hover.after(crate::cellmap_bridge::sync_cell_selection),
            )
            .add_systems(
                Update,
                (
                    measurement::reload_measurement_cache,
                    measurement::commit_measurement_click
                        .after(measurement::reload_measurement_cache),
                    // `Del` 删光标下的测量（2026-10-23）：与清空按钮互补——
                    // 后者清一图（应用层命令），本系统删一条（渲染层意图）。
                    measurement::delete_measurement_under_cursor
                        .after(measurement::reload_measurement_cache),
                ),
            )
            .init_resource::<crate::state::SelectionState>()
            .add_systems(
                Update,
                (
                    crate::selection::sync_selection_map_change,
                    crate::selection::commit_selection_click
                        .after(crate::selection::sync_selection_map_change),
                    crate::selection::drag_selected_anchor
                        .after(crate::selection::commit_selection_click),
                    crate::selection::draw_selection,
                )
                    .after(crate::camera::track_cursor_world),
            );
    }
}

/// 装饰族 Plugin（**受 `TreatmentFlags` 控制**；M2 阶段恒装全部装饰系统）。
///
/// 装配：纸面氛围（雾霭 + 纸张噪点，ADR-0016 M2）。
/// M2 阶段该 Plugin **不读 `TreatmentFlags`**——M3 推进时把 `isoline_overlay`
/// 的水彩晕 / `cells` 的水色渐变 / 雪调 / 海冰等旗标接到 `Recipe::treatment()`
/// 后，本 Plugin 会按旗标 gate 装饰系统。
pub struct MapShellPlugin;

impl Plugin for MapShellPlugin {
    fn build(&self, app: &mut App) {
        // 纸面氛围层（ADR-0016 M2：雾霭 + 纸张噪点）
        app.add_systems(
            Update,
            crate::cellmap_bridge::sync_atmosphere.after(map_frame::sync_map_frame),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三个 Plugin 都能构造（不 panic）。
    /// 验证 trait 实现 + 单元字段就位；不验证系统运行（那是 smoke-gui 范畴）。
    #[test]
    fn plugins_are_constructible() {
        let _map = MapRenderPlugin;
        let _editor = EditorOverlayPlugin;
        let _shell = MapShellPlugin;
    }

    /// Plugin 装入 App 不 panic（最小冒烟——只装 plugin 不跑循环）。
    /// `App::new()` + `add_plugins` 即覆盖 `build(&mut self, app: &mut App)` 路径。
    #[test]
    fn plugins_install_into_app_without_panic() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.add_plugins(MapRenderPlugin);
        app.add_plugins(MapShellPlugin);
        // EditorOverlayPlugin 引入 brush_cursor 等需 UI 资源——本最小 App 不验证它
    }
}
