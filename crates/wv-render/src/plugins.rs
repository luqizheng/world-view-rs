//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! Plugin 拆分（ADR-0023 D5）—— 把 wv-render 的系统按"游戏可复用 / 编辑器专属 / 装饰族"
//! 三类分组，让游戏 app 只装想要的（§1.2 差距 4）。
//!
//! ## **双轨装配的现状与分界（ADR-0086，必读）**
//!
//! 本仓库存在**两份系统装配清单**，服务两个不同宿主：
//!
//! | 路径 | 权威装配点 | 宿主 |
//! |---|---|---|
//! | **编辑器**（`wv gui`） | `wv-app::app_bootstrap::schedules.rs`（系统直装，带 `perf_timed!` 与精确排序） | 唯一日常入口 |
//! | **游戏外挂消费**（`game_consumer` example） | 本文件三 Plugin | 非 Bevy 原生宿主复用的演示/出口 |
//!
//! **编辑器从不装配本文件三 Plugin**（`resources.rs:90` 明载）。历史教训：
//! `EditorOverlayPlugin` 注册的系统以为"装了"实际没装，ADR-0040 单格金色
//! outline 不出现（2026-10-24 二次踩，见 schedules.rs 注释）。因此**任何
//! 新系统必须同时在两处装配**（或明确只属其一），且顺序约束（`.after` /
//! `.chain` / set）两处都要落——P0-4 已修的三处缺口
//! （`DrillLabelChain` / cellmap 四系统 / gizmo 六系统）即漏网实例。
//!
//! 改本文件清单时，`grep schedules.rs` 对应系统名核对编辑器侧是否同步。
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
//! ## 边界
//! - **不**做 Plugin 间的执行顺序约束（`chain()` 留在 bootstrap 内）；
//! - **不**做 Plugin 内子模块拆分（每 Plugin 内部保持平铺 `add_systems`）；
//! - **不**做热插拔 / 动态装载（M3 推迟）。

use bevy::prelude::*;

use crate::brush_cursor;
use crate::buildings;
use crate::cell_selection;
use crate::faction_overlays;
use crate::faction_partition;
use crate::layer_frame;
use crate::map_frame;
use crate::measurement;
use crate::paper_overlay;
use crate::streets;
use crate::stroke_preview;
use crate::vector_layers;
use crate::world_projection;

/// 地图本体渲染 Plugin（**游戏可用**）。
///
/// 装配：地形 chunk 同步 / 等值线叠加 / 图层帧 / 矢量层 / 街道 / 势力 / 钻取 /
/// 地图外框 / 建筑 / 世界投影（读路径）。**不**含笔刷 / 测量 / 编辑桥。
pub struct MapRenderPlugin;

impl Plugin for MapRenderPlugin {
    fn build(&self, app: &mut App) {
        // 渲染配方资源必须存在：`sync_map_frame` 等系统读它做门（ADR-0021 D1）。
        // 编辑器路径由 `app_bootstrap::register_recipe_system` 单独求值；这里是
        // **兜底初始化**，让只装本 Plugin 的游戏 app 不会因缺资源 panic
        // （Default = 全关，fail-safe：未配置即不画）。
        app.init_resource::<crate::recipe::RenderRecipe>();
        app.add_systems(
            Update,
            (
                // 投影（时间 → 渲染资源）
                world_projection::compute_projection,
                // 图层帧（ADR-0013）：先于一切消费它的渲染系统求值
                layer_frame::resolve_layer_frame,
                // 等值线矢量叠加（ADR-0012 / plan v0.1 M2）
                crate::cellmap_bridge::sync_isoline_overlay,
                // 群系矢量面（ADR-0062，专业制图专属）：压在纹理块之上、
                // 海岸线之下——把绿地从「格子块」换成平滑多边形。
                crate::biome_fill::sync_biome_fills,
                // 地形分块 + mip + 视口拣选（优化项 3a/3b/3c）
                crate::cellmap_bridge::sync_terrain_chunks,
                // 纸纹叠加（ADR-0042）：斑驳 + 晕影，图幅变化时重建
                paper_overlay::sync_paper_overlay,
                // 街道图层缓存：当前图变化时重读（ADR-0010）
                streets::reload_street_cache,
                // 矢量叠加层（道路 + POI）
                vector_layers::sync_vector_layers,
                // 街道名标注。ADR-0073：街名在统一占位池中**最后落位**——
                // 显式排在钻取标签链（drill::register 的 chain）之后，落位
                // 次序「地名 → … → 海名 → 街名」由此成立；游戏路径不装钻取
                // 链时该集合为空，约束自然退化。
                streets::update_street_labels.after(crate::label_collision::DrillLabelChain),
                // 势力多边形叠加层
                faction_overlays::sync_faction_overlays,
                // 切蛋糕分区渲染（ADR-0079 T10）：压势力叠加之后
                faction_partition::sync_partition_overlays
                    .after(faction_overlays::sync_faction_overlays),
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
        .init_resource::<faction_partition::PartitionOverlayEntities>()
        // CellMap 未命名候选高亮的缓存（2026-10-22 内容矩阵 T11）：
        // draw_map_gizmos ResMut 它——漏 init 即启动期 panic（同 EditCellState 教训）。
        .init_resource::<crate::region_overlay::CandidateOverlayCache>()
        // 纸纹叠层运行时状态（贴图句柄 + 当前台实体，ADR-0042）。
        .init_resource::<paper_overlay::PaperOverlay>()
        // 群系矢量面状态（ADR-0062）：漏 init 即启动期 panic（同 EditCellState 教训）。
        .init_resource::<crate::biome_fill::BiomeFills>();
    }
}

/// 编辑器专属 Plugin（**游戏不必装**）。
///
/// 装配：笔刷光标 / 测量 / 编辑桥 / 测量回执。游戏接入时**不**装本 Plugin 即
/// 拖入对应代码。
pub struct EditorOverlayPlugin;

impl Plugin for EditorOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<cell_selection::CellAntsEntities>()
            .init_resource::<cell_selection::CellRegionEntities>()
            // hover 准星（[ADR-0052] D2）：双色描边 + 中心十字。
            .init_resource::<cell_selection::CellHoverEntities>()
            // 待落笔触色块（[ADR-0052] D3）：红橙半透填充。
            .init_resource::<cell_selection::CellStrokeEntities>()
            // 落笔会话态（[ADR-0052] D4）：`wv-app` 写、`wv-render` 读
            // （待落色块要读笔触格）。
            .init_resource::<wv_world_cell_map::cell_paint::CellPaintSession>()
            // 撤销白闪 / 拒收红闪（[ADR-0052] D8-A / D3-E）。
            .init_resource::<wv_world_cell_map::cell_flash::CellFlashState>()
            .init_resource::<wv_world_cell_map::cell_flash::CellFlashEntities>()
            .add_systems(Update, brush_cursor::sync_brush_cursor)
            // UX-004：世界级笔刷拖拽预览（点列半透明足印，与光标同轨注册）。
            .add_systems(Update, stroke_preview::sync_stroke_preview)
            .add_systems(Update, crate::cellmap_bridge::sync_cell_region)
            .add_systems(Update, crate::cellmap_bridge::sync_cell_stroke)
            // 闪视觉（撤销白闪 / 拒收红闪）画在最顶层。
            .add_systems(Update, crate::cellmap_bridge::sync_cell_flash)
            // hover 准星画在选区**之上**（Z 序：hover 是最上层的瞬时反馈，
            // ADR-0052 D2）；ADR-0057 后 selected 已由 sync_cell_region 统一承担。
            .add_systems(
                Update,
                crate::cellmap_bridge::sync_cell_hover
                    .after(crate::cellmap_bridge::sync_cell_region),
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
                    // 选中图钉高亮环（ADR-0074 追记三 followup）：选谁亮谁。
                    world_projection::draw_pin_highlight,
                    // gizmo 线宽随缩放（1 屏幕像素）——默认世界单位线宽在
                    // CellMap 深缩放下亚像素隐形。
                    world_projection::sync_gizmo_line_width,
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
