//! CellMap 会话态（自 wv-render/src/state.rs 迁入，ADR-0054 D1）。
//!
//! `EditCellState` / `GridConfig` 是 CellMap 工作台**专属**的会话资源；
//! `ViewState` 等全渲染层共享的呈现态仍留 wv-render（经其 re-export，
//! `wv_render::state::EditCellState` 等旧路径零改动）。

use bevy::prelude::*;

/// EditCell 工具的会话态（ADR-0040 决策 5；ADR-0052 扩展）。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct EditCellState {
    /// EditCell 工具是否激活（与 wv-render `BrushState::enabled` 同款会话布尔）。
    /// 由命令分发器在 `SelectMapTool(EditCell)` 翻 true / 切走其它工具翻 false。
    pub tool_active: bool,
    /// 当前鼠标压住的格 (gx, gy)；`None` 表示鼠标不在地图内。
    pub hover: Option<(i32, i32)>,
    /// 点击落定的格 (gx, gy)。ESC 清空 / 切图也清空。
    pub selected: Option<(i32, i32)>,
    /// 多边形选区贴合的方格集合（2026-10-20 批量编辑）：V 工具闭合多边形后
    /// 由 `sync_region_cells` 栅格化写入（cell 中心点落在多边形内即选中）。
    /// 空 = 无区域选区。高亮渲染与批量应用（BatchEditCellsCommand）都消费它。
    pub region_cells: Vec<(i32, i32)>,
    /// 上次栅格化时**多边形**的指纹（2026-10-20 性能修）。
    ///
    /// `sync_region_cells` 每帧都会跑；多边形没变时**不必重算**格集合
    /// （栅格化是 O(bbox 格数 × 边数)，大选区每帧重算会明显吃帧）。
    /// 指纹变了（或为 0 = 从未算过）才重算。
    pub region_fingerprint: u64,
    /// 绑定的图 ID；变图即清空选中格。
    pub last_map_id: Option<wv_core::Id>,
}

impl EditCellState {
    /// 清空 tool_active + hover + selected + region_cells；
    /// **不会**清 `last_map_id`（那是别处碰）。
    pub fn clear(&mut self) {
        self.tool_active = false;
        self.hover = None;
        self.selected = None;
        self.region_cells.clear();
        self.region_fingerprint = 0;
    }

    /// **只关工具，不清选区**（[ADR-0052] §9.1 S0-1 修复口径）。
    ///
    /// 切走 EditCell 工具时**只**翻 `tool_active = false`——`selected`
    /// （单格选）与 `region_cells`（区域选）**保留**，这样：
    ///
    /// - 「框选一片 → 切 EditCell」后金选区仍在画面上（ADR-0052 §5 M1
    ///   验收 ④）；
    /// - 再切回 EditCell 时选区还在，不用重新框。
    ///
    /// 只有**切图**（`on_map_change`）与 **Esc 全量清理**才走 `clear()`。
    ///
    /// 口径与 §9.1 D3-D 一致：「切工具丢弃整笔 + 切图全量 clear」。
    pub fn close_tool(&mut self) {
        self.tool_active = false;
        // hover 是**瞬时**态（光标动就变），关工具后光标多半还在地图上，
        // 留着会画出「工具已关但还有 hover 高亮」的鬼影——清掉。
        // `selected` / `region_cells` 是**已提交**的选择态，保留。
        self.hover = None;
    }

    /// 切换图时调用——如果新图 ≠ 上次记录的图,清空所有格相关字段
    /// （与 wv-render `SelectionState::on_map_change` 同口径）。
    pub fn on_map_change(&mut self, new_map: &wv_core::Id) {
        if self.last_map_id.as_ref() != Some(new_map) {
            self.hover = None;
            self.selected = None;
        }
        self.last_map_id = Some(new_map.clone());
    }
}

/// 地形栅格的视图标定（自 wv-render 迁入；`grid_sync` 桥接系统在 wv-render
/// 侧每帧从呈现态投影写入）。
///
/// ⚠️ 本仓库有**两套原点**（详见原 wv-render state.rs 的对照表）：
/// `raster_origin` 服务**地形栅格**、`map_origin` 服务**本图局部内容**；
/// 两者只在「当前图自带底图」时相等。子图继承父图底图时栅格仍是父图那一张。
#[derive(Resource)]
pub struct GridConfig {
    /// 每格世界边长。
    pub cell_size: f32,
    /// 栅格空间原点的视图位置（**地形**专用）。
    pub raster_origin: Vec2,
    /// 当前图图帧原点的视图位置（**本图局部内容**专用）。
    pub map_origin: Vec2,
    /// 当前图幅面（`extent` 的宽 × 高，世界单位）：地图外框与 1/4 参考网格用。
    /// 16:9 缺省后不再是正方形——`x` / `y` 分开。
    pub map_size: Vec2,
}

impl Default for GridConfig {
    /// 缺省栅格：4096² 世界图口径（`cell_size = 8.0`，自 wv-render state.rs 迁入）。
    fn default() -> Self {
        Self {
            cell_size: 8.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        }
    }
}

impl GridConfig {
    /// 本图局部坐标 → 视图空间。
    ///
    /// **本图局部内容（地点 / 街道 / 街道名 / 外框）一律走这里**——绕过去手写
    /// `map_origin + local` 就等于把两套原点的对照表丢在一边。
    pub fn map_to_view(&self, local: Vec2) -> Vec2 {
        self.map_origin + local
    }
}
