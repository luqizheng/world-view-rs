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

/// **泛洪选择工具**的会话态（[ADR-0052] D6 / D6-A）。
///
/// 与 [`EditCellState`] 同级——两者是**互斥工具**（同一时刻只有一个 active）。
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct FloodSelectState {
    /// 泛洪工具是否激活。
    pub tool_active: bool,
    /// **判据**：按地表 / 按群系 / 按海拔容差。
    ///
    /// 默认按**群系**——CellMap 的主要用途是「选出一片同群系的地」，
    /// 按地表（水/陆）粒度太粗，按海拔容差需要额外输入。
    pub criterion: FloodCriterion,
}

impl FloodSelectState {
    /// 关工具（切走时调用）——只翻 `tool_active`，不清选区
    /// （选区在 `EditCellState.region_cells`，跨工具保留）。
    pub fn close_tool(&mut self) {
        self.tool_active = false;
    }
}

/// **泛洪判据**（D6）：什么算「同值」。
///
/// 与 `wv_terrain::cellops::FloodCriterion` 同款语义——此处独立定义是为了
/// 让 `wv-world-cell-map` 不依赖 `wv-terrain`（[ADR-0054] D2/D4 的 crate
/// 分层纪律：地形域的 crate 只依赖 core）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FloodCriterion {
    /// 按**地表**判同（水 / 陆）。
    Surface,
    /// 按**群系**判同（默认）。
    #[default]
    Biome,
    /// 按**海拔容差**判同（`|h − h₀| ≤ tol`）。
    HeightTolerance(i16),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// **S0-1（[ADR-0052] §9.1）· `close_tool` 不清选区**。
    ///
    /// 「框选一片 → 切 EditCell」不丢选区：切走工具只翻 `tool_active`，
    /// `selected` / `region_cells` 必须保留。
    #[test]
    fn close_tool_keeps_selected_and_region_cells() {
        let mut s = EditCellState {
            tool_active: true,
            hover: Some((3, 4)),
            selected: Some((10, 20)),
            region_cells: vec![(1, 1), (2, 2), (3, 3)],
            region_fingerprint: 0xdead_beef,
            last_map_id: None,
        };
        s.close_tool();
        assert!(!s.tool_active, "工具应关闭");
        assert_eq!(s.selected, Some((10, 20)), "单格选必须保留");
        assert_eq!(s.region_cells.len(), 3, "区域选必须保留");
        assert_eq!(s.region_fingerprint, 0xdead_beef, "指纹保留（避免重算）");
        assert_eq!(s.hover, None, "hover 是瞬时态，关工具即清");
    }

    /// **S0-1 · `clear` 仍然全清**（切图 / Esc 走这条）。
    #[test]
    fn clear_wipes_everything_but_last_map_id() {
        let mut s = EditCellState {
            tool_active: true,
            hover: Some((1, 1)),
            selected: Some((2, 2)),
            region_cells: vec![(3, 3)],
            region_fingerprint: 42,
            last_map_id: Some(wv_core::Id::new_unchecked("map_a")),
        };
        s.clear();
        assert!(!s.tool_active);
        assert_eq!(s.hover, None);
        assert_eq!(s.selected, None);
        assert!(s.region_cells.is_empty());
        assert_eq!(s.region_fingerprint, 0);
        assert!(s.last_map_id.is_some(), "last_map_id 不由 clear 碰");
    }

    /// **切图清选区**（`on_map_change`）：换图 → selected / hover 清空；
    /// 同图 → 不动（避免每帧误清）。
    #[test]
    fn on_map_change_clears_only_when_map_actually_changes() {
        let mut s = EditCellState {
            tool_active: true,
            hover: Some((1, 1)),
            selected: Some((2, 2)),
            region_cells: vec![],
            region_fingerprint: 0,
            last_map_id: None,
        };
        let map_a = wv_core::Id::new_unchecked("map_a");
        s.on_map_change(&map_a);
        assert_eq!(s.selected, None, "首次记录即视为换图 → 清空");
        // 再设一次选中，同图再来 → 不清
        s.selected = Some((5, 5));
        s.on_map_change(&map_a);
        assert_eq!(s.selected, Some((5, 5)), "同图不应清选区");
        // 换图 → 清
        let map_b = wv_core::Id::new_unchecked("map_b");
        s.on_map_change(&map_b);
        assert_eq!(s.selected, None, "换图应清选区");
    }
}
