//! 街区框选工具与表单（2026-11-06 城市街区工具设计 §4.1）。
//!
//! ## 交互
//!
//! 1. 作者在**城市图**（当前图 `MapLevel::City`）激活「街区」工具 → 拖一个矩形
//!    （复用 `wv_render::selection` 的手势管线：`SelectionState.drag` 拖拽
//!    成形 → `closed` 闭合多边形，**本图局部坐标**）。
//! 2. 闭合瞬间打开 `DistrictForm`（密度 / 楼层带 / 肌理带 滑条 + 种子 +
//!    「生成」按钮）。
//! 3. 「生成」→ `UpsertDistrictCommand`（D3 单事务：upsert 街区 + 清环内
//!    全部建筑 + 重生成）→ 提交 History（一条 undo）。
//!
//! ## 为什么不用 Select 工具
//!
//! `MapToolId::Select` 的包络圈选**只在 Topographic 可用**（`available_in`
//! 白名单），而城市图走 Browse/Map 段——所以街区工具自带 `SelectionState`
//! 驱动，不复用 Select 的工具身份，但**复用它的手势与坐标链**（闭合顶点
//! 就是本图局部坐标，与 `District.ring` 同一坐标系）。

use bevy::prelude::*;

use crate::state::{DrillState, ViewState, WorldProjection};
use wv_core::base::geometry::Point;
use wv_core::source::district::{District, DistrictParams};
use wv_render::state::{GridConfig, SelectionShape, SelectionState};

/// 街区工具的会话状态。
#[derive(Resource, Default, Debug, Clone)]
pub struct DistrictToolState {
    /// 工具是否激活（激活时 selection 走矩形拖拽成形）。
    pub active: bool,
    /// 待确认的圈选轮廓（本图局部坐标，≥3 点）——闭合瞬间由
    /// [`sync_district_tool`] 从 `SelectionState.closed` 搬来。
    pub pending_ring: Option<Vec<(f32, f32)>>,
    /// 表单里正在编辑的街区（已存在的 → 重摇；新建的 → 首次生成）。
    pub form: Option<DistrictForm>,
    /// 已存在的街区里被点中的那个（点选重摇入口；v1 由 overlay 命中给出）。
    pub selected: Option<wv_core::Id>,
}

/// 街区参数表单（弹窗数据）。
#[derive(Debug, Clone)]
pub struct DistrictForm {
    /// 街区 ID（新建时预生成一个）。
    pub id: wv_core::Id,
    /// 街区名（可空）。
    pub name: Option<String>,
    /// 圈选轮廓（本图局部坐标）。
    pub ring: Vec<(f32, f32)>,
    pub params: DistrictParams,
    /// 生成种子（表单可改 → 换布局）。
    pub seed: u64,
    /// 是否为「已存在的街区」（重摇模式：按钮文案与语义不同）。
    pub existing: bool,
}

impl DistrictForm {
    /// 由圈选轮廓新建表单（随机种子）。
    pub fn from_ring(ring: Vec<(f32, f32)>) -> Self {
        let seed = random_seed();
        DistrictForm {
            id: wv_core::Id::new_unchecked(&format!("blk_{seed:x}")),
            name: None,
            ring,
            params: DistrictParams::default(),
            seed,
            existing: false,
        }
    }
}

/// 会话种子（纳秒时钟 ^ pid 混合；非密码学用途）。
pub fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    nanos ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// 街区工具驱动：把 `SelectionState` 的闭合轮廓搬进表单状态。
///
/// 守卫链：
/// - 仅在街区工具激活（表单已开或 pending 存在 = 工具进行中）；
/// - `SelectionState.closed` 成形（≥3 点）且尚未消费；
/// - 当前图是**城市图**（`MapLevel::City`）——世界图上圈街区无意义。
pub fn sync_district_tool_inner(
    selection: &mut SelectionState,
    state: &mut DistrictToolState,
    drill: &DrillState,
    projection: &WorldProjection,
) {
    // 只在「街区工具激活」时消费（激活 = 正在等作者圈一块地）。
    if !state.active {
        return;
    }
    if state.form.is_some() || state.pending_ring.is_some() {
        return; // 已消费（表单已开 / 轮廓已收），等用户点「生成」
    }
    let Some(ring) = selection.closed.clone() else {
        return;
    };
    if ring.len() < 3 {
        return;
    }
    // 城市图门：当前图 level == City。
    let is_city = drill
        .current_node(&projection.world)
        .map(|n| n.level == wv_core::map::MapLevel::City)
        .unwrap_or(false);
    if !is_city {
        return;
    }
    // 消费：搬进表单 + 清空选择态（避免重复触发）。
    state.form = Some(DistrictForm::from_ring(ring));
    selection.closed = None;
    selection.draft.clear();
}

/// 街区工具激活时把 `SelectionState` 拨到「矩形拖拽成形」模式；
/// 工具关闭时把选区清干净（避免与 Select 工具的状态互相污染）。
pub fn drive_and_sync_district_tool(
    mut selection: ResMut<SelectionState>,
    mut state: ResMut<DistrictToolState>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
) {
    drive_selection_for_district_inner(state.active, &mut selection, &state);
    sync_district_tool_inner(&mut selection, &mut state, &drill, &projection);
}

/// 模式拨动的内部实现（可单测直接调）。
pub fn drive_selection_for_district_inner(
    active: bool,
    selection: &mut SelectionState,
    state: &DistrictToolState,
) {
    if active {
        if !selection.enabled {
            selection.enabled = true;
            selection.shape = SelectionShape::Rect;
            selection.draft.clear();
            selection.closed = None;
            selection.drag = None;
        }
    } else if state.form.is_none() && state.pending_ring.is_none() {
        // 工具关闭：只清 selection 的形状，不动别的工具的状态。
        if selection.enabled && selection.shape == SelectionShape::Rect {
            selection.enabled = false;
            selection.draft.clear();
            selection.closed = None;
            selection.drag = None;
        }
    }
}

/// 提交：把表单转成 `District` + 派生建筑，走 `UpsertDistrictCommand`。
///
/// 街道折线快照取自当前图的 Road 图层（生成器沿街判定用）。
pub fn district_from_form(
    form: &DistrictForm,
    projection: &WorldProjection,
    drill: &DrillState,
) -> (District, Vec<Vec<Point>>) {
    let ring: Vec<Point> = form.ring.iter().map(|(x, y)| Point::new(*x, *y)).collect();
    let streets = streets_of_current_map(projection, drill);
    let district = District {
        id: form.id.clone(),
        name: form.name.clone(),
        ring,
        params: form.params.clone(),
        seed: form.seed,
    };
    (district, streets)
}

/// 当前图的街道折线快照（局部坐标；Road 图层的 `StreetLayer`）。
pub fn streets_of_current_map(projection: &WorldProjection, drill: &DrillState) -> Vec<Vec<Point>> {
    let Some(node) = drill.current_node(&projection.world) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for l in &node.layers {
        if l.kind != wv_core::map::LayerKind::Road {
            continue;
        }
        let Some(doc) = projection.world.street_of(&l.id) else {
            continue;
        };
        for seg in &doc.segments {
            out.push(seg.points.clone());
        }
    }
    out
}

/// 街区工具是否可用（当前图是城市图）。
pub fn district_tool_available(drill: Res<DrillState>, projection: Res<WorldProjection>) -> bool {
    drill
        .current_node(&projection.world)
        .map(|n| n.level == wv_core::map::MapLevel::City)
        .unwrap_or(false)
}

/// 当前视图尺度（表单预览用）。
pub fn view_scale(view: Res<ViewState>) -> f32 {
    view.scale
}

/// 格尺寸（表单预览用）。
pub fn cell_size(grid: Res<GridConfig>) -> f32 {
    grid.cell_size
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::{Extent, MapLevel, MapNode};

    fn city_world(level: MapLevel) -> wv_core::World {
        let mut w = wv_core::World::default();
        // 根图（World 级）：`drill_to` 需要它才能定位起点。
        w.maps.push(MapNode {
            id: wv_core::Id::new_unchecked("map_root"),
            name: "世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![wv_core::Id::new_unchecked("map_city")],
            icons: Vec::new(),
            layers: Vec::new(),
            extent: Some(Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        });
        w.maps.push(MapNode {
            id: wv_core::Id::new_unchecked("map_city"),
            name: "城".into(),
            level,
            parent: Some(wv_core::Id::new_unchecked("map_root")),
            children: Vec::new(),
            icons: Vec::new(),
            layers: Vec::new(),
            extent: Some(Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        });
        w
    }

    /// 工具激活 → selection 进入矩形模式。
    #[test]
    fn activating_district_tool_switches_selection_to_rect() {
        let mut sel = SelectionState::default();
        let state = DistrictToolState {
            active: true,
            ..Default::default()
        };
        drive_selection_for_district_inner(true, &mut sel, &state);
        assert!(sel.enabled, "激活后 selection 应启用");
        assert_eq!(sel.shape, SelectionShape::Rect);
    }

    /// 工具关闭 → selection 复位。
    #[test]
    fn deactivating_district_tool_clears_selection() {
        let mut sel = SelectionState::default();
        sel.enabled = true;
        sel.shape = SelectionShape::Rect;
        sel.closed = Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        let state = DistrictToolState::default();
        drive_selection_for_district_inner(false, &mut sel, &state);
        assert!(!sel.enabled);
        assert!(sel.closed.is_none());
    }

    /// 闭合轮廓 → 表单（城市图门通过）。
    #[test]
    fn closed_ring_opens_the_form_on_a_city_map() {
        let world = city_world(MapLevel::City);
        let mut drill = DrillState::default();
        drill.drill_to(&world, &wv_core::Id::new_unchecked("map_city"));
        let mut sel = SelectionState::default();
        sel.enabled = true;
        sel.shape = SelectionShape::Rect;
        sel.closed = Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        let mut state = DistrictToolState {
            active: true,
            ..Default::default()
        };
        let mut projection = WorldProjection::default();
        projection.world = world;
        sync_district_tool_inner(&mut sel, &mut state, &drill, &projection);
        assert!(state.form.is_some(), "闭合轮廓应打开表单");
        assert_eq!(state.form.unwrap().ring.len(), 3);
        assert!(sel.closed.is_none(), "消费后选区应清空");
    }

    /// 世界图上圈选 → 不开表单（城市图门）。
    #[test]
    fn closed_ring_on_a_world_map_does_not_open_the_form() {
        let world = city_world(MapLevel::World);
        let mut drill = DrillState::default();
        drill.drill_to(&world, &wv_core::Id::new_unchecked("map_city"));
        let mut sel = SelectionState::default();
        sel.enabled = true;
        sel.shape = SelectionShape::Rect;
        sel.closed = Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]);
        let mut state = DistrictToolState {
            active: true,
            ..Default::default()
        };
        let mut projection = WorldProjection::default();
        projection.world = world;
        sync_district_tool_inner(&mut sel, &mut state, &drill, &projection);
        assert!(state.form.is_none(), "世界图不应开街区表单");
    }
}
