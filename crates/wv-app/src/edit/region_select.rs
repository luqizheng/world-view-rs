//! 自然区的鼠标选择手势（2026-10-24 用户裁定）。
//!
//! ## 两条手势
//!
//! | 手势 | 行为 | 为什么这么定 |
//! |---|---|---|
//! | **左键点在自然区内** | 选中该区域（`WorldProjection.selection`） | 数据编辑器口径：点谁选谁。此前只有「点区域**标签**」能选中（ADR-0017 §2.7），大区域里标签可能不在光标附近 |
//! | **右键单击（不拖动）** | 取消选中 | 右键本来就是平移地图的键；松手时零位移 = 单击，不是拖拽——用位移量区分「想平移」与「想取消」 |
//!
//! ## 边界（不做什么）
//!
//! - **不写世界**：只动 `WorldProjection.selection`（会话态，不落盘、不经
//!   `EditCommand`，与区域面板 / 标签拾取同口径）。
//! - **不抢锚点拖动**：`region_adjust_input` 命中顶点锚点时本系统让位
//!   （否则「拽锚点」会顺带换掉选中，Inspector 跟着跳）。
//! - **只在数据视图生效**（`Topographic ∧ nature_overlay`）：这是「显示自然区
//!   与候选」打开时的编辑语境；其他模式右键盘仍是纯平移，左键仍是钻取 / 刷子。

use bevy::prelude::*;

use crate::state::{ViewState, WorldProjection};
use wv_render::state::RegionAdjustState;

use wv_core::model::region::Region;

/// 右键位移超过这个像素数就算「拖动平移」，不算单击。
///
/// 4px：手抖与触控板轻触的抖动在 1–2px；真拖一定会超过。比双击容差
/// （`DOUBLE_CLICK_SLOP`）小一档，因为这里判的是「有没有意图」，不是「准不准」。
const RIGHT_DRAG_SLOP_PX: f32 = 4.0;

/// 右键追迹（起点 + 是否已拖过阈值）。
///
/// `pub` 是必须的：它出现在 pub 系统的 `Local<>` 参数里（同 `CandidateRingCache`
/// 的教训——`SystemParam` 类型比系统更私有会直接编译失败）。
#[derive(Default)]
pub struct RightDragWatch {
    start: Option<Vec2>,
    moved: bool,
}

/// 本系统生效的语境：数据视图（`Topographic` + 群系视图开关）。
fn data_view_active(view: &ViewState) -> bool {
    matches!(view.mode, wv_render::state::ViewMode::Topographic) && view.nature_overlay
}

/// 点在环内（even-odd 射线法，格点无关的纯几何判定）。
///
/// 与 `nature_region_actions` / `cell_selection` 里的同名判定同源同算法：
/// 三处各自私有，是因为它们分属「区域物化」「方格栅格化」「鼠标拾取」三个
/// 不同层，合并会造出一个谁都不认的公共模块。
fn point_in_ring(px: f32, py: f32, ring: &[f32]) -> bool {
    let n = ring.len() / 2;
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (ring[i * 2], ring[i * 2 + 1]);
        let (xj, yj) = (ring[j * 2], ring[j * 2 + 1]);
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 区域是否包含该点（外环内 ∧ 不在任何洞里）。
fn region_contains(region: &Region, p: (f32, f32)) -> bool {
    if !point_in_ring(p.0, p.1, &region.polygon) {
        return false;
    }
    !region.holes.iter().any(|h| point_in_ring(p.0, p.1, h))
}

/// 外接框面积（只用来在「嵌套区域都命中」时挑最里面的那个）。
fn bbox_area(ring: &[f32]) -> f32 {
    let mut min = (f32::INFINITY, f32::INFINITY);
    let mut max = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for c in ring.as_chunks::<2>().0 {
        min = (min.0.min(c[0]), min.1.min(c[1]));
        max = (max.0.max(c[0]), max.1.max(c[1]));
    }
    if min.0.is_finite() && max.0.is_finite() {
        ((max.0 - min.0) * (max.1 - min.1)).max(0.0)
    } else {
        f32::INFINITY
    }
}

/// 光标下的自然区（**最里面的那个**：嵌套时取外接框最小的）。
///
/// 只考虑当前图 ∧ `Nature` 类 ∧ 有轮廓的区域——`Area` 类在 CellMap 里本来就
/// 不显示（门控见 `region_layer_visible`），点它等于点空白。
pub fn nature_region_at(
    projection: &WorldProjection,
    map: &wv_core::Id,
    local: (f32, f32),
) -> Option<wv_core::Id> {
    projection
        .world
        .regions
        .iter()
        .filter(|r| r.map == *map)
        .filter(|r| r.is_nature() && r.polygon.len() >= 6)
        .filter(|r| region_contains(r, local))
        .min_by(|a, b| bbox_area(&a.polygon).total_cmp(&bbox_area(&b.polygon)))
        .map(|r| r.id.clone())
}

/// 左键点在自然区内 ⇒ 选中它（点空白/点区域外**不清除**——那是 `drill_pick` 的事）。
///
/// 排序要求（调用方负责）：`.after(drill_pick)`（它会在空白单击时清选中）、
/// `.after(region_adjust::region_adjust_input)`（锚点拖动优先）。
#[allow(clippy::too_many_arguments)]
pub fn select_nature_region_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    playback: Res<crate::state::PlaybackState>,
    view: Res<ViewState>,
    grid: Res<wv_render::state::GridConfig>,
    drill: Res<wv_render::state::DrillState>,
    region_adjust: Res<RegionAdjustState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut projection: ResMut<WorldProjection>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if ui.wants_pointer || playback.playing || !data_view_active(&view) {
        return;
    }
    // 锚点拖动优先：本帧 `region_adjust_input` 已命中顶点 ⇒ 这次按下是「拽锚点」，
    // 不是「选区域」。判 `preview_polygon` 而不是 `just_pressed`，是因为预览
    // 只在命中锚点后才被写。
    if region_adjust.drag_vertex.is_some() || region_adjust.preview_polygon.is_some() {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(map) = drill.current().cloned() else {
        return;
    };
    // 区域多边形存的是「图内局部 wu」——与 `region_adjust_input` 同一换算
    // （`cursor - grid.map_origin`），偏移量按当前层级的图求，钻取后仍成立。
    let origin = drill
        .current_node(&projection.world)
        .map(|node| wv_render::drill::map_origin(&projection.world, node, &grid))
        .unwrap_or(grid.map_origin);
    let local = (cursor - origin).into();
    let Some(id) = nature_region_at(&projection, &map, local) else {
        return;
    };
    let target = wv_core::SelectedEntity::new(wv_core::EntityKind::Region, id);
    if projection.selection.as_ref() != Some(&target) {
        projection.selection = Some(target);
    }
}

/// 右键**单击**（松手时位移 < 4px）⇒ 取消选中。
///
/// 右键按住拖动 = 平移，不做任何选择操作（`camera_pan` 与此并行，互不干扰：
/// 它读鼠标增量，本系统只读按下/松开两点的距离）。
pub fn cancel_selection_on_right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    view: Res<ViewState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut projection: ResMut<WorldProjection>,
    mut watch: Local<RightDragWatch>,
) {
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());
    if buttons.just_pressed(MouseButton::Right) {
        *watch = RightDragWatch {
            start: cursor,
            moved: false,
        };
        return;
    }
    if buttons.pressed(MouseButton::Right) {
        if let (Some(start), Some(now)) = (watch.start, cursor) {
            if now.distance(start) > RIGHT_DRAG_SLOP_PX {
                watch.moved = true;
            }
        }
        return;
    }
    if !buttons.just_released(MouseButton::Right) {
        return;
    }
    let clicked = watch.start.is_some() && !watch.moved;
    *watch = RightDragWatch::default();
    if !clicked || ui.wants_pointer || !data_view_active(&view) {
        return;
    }
    // 只在真有东西可选时写（避免每帧把 `is_changed` 打起来）。
    if projection.selection.is_some() {
        projection.selection = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::model::region::RegionKind;
    use wv_core::{EntityKind, Id, SelectedEntity, World};

    const MAP: &str = "map_world";

    fn region(id: &str, poly: Vec<f32>, kind: RegionKind) -> Region {
        Region {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            map: Id::new_unchecked(MAP),
            polygon: poly,
            holes: Vec::new(),
            kind,
            parent: None,
            overlay: None,
        }
    }

    /// 正方形环（手算：`(x, y)` 起，边长 `s`）。
    fn square(x: f32, y: f32, s: f32) -> Vec<f32> {
        vec![x, y, x + s, y, x + s, y + s, x, y + s, x, y]
    }

    fn projection(regions: Vec<Region>) -> WorldProjection {
        let mut world = World {
            maps: vec![MapNode {
                id: Id::new_unchecked(MAP),
                name: MAP.into(),
                level: MapLevel::World,
                parent: None,
                children: Vec::new(),
                layers: Vec::new(),
                icons: Vec::new(),
                extent: Some(Extent::sized(4096.0, 4096.0)),
                raster: None,
                geographic_source: None,
                time_span: None,
                unit_per_km: None,
                orientation: 0.0,
                base: None,
                outline_polygon: None,
                ..Default::default()
            }],
            regions,
            ..Default::default()
        };
        world.terrains.clear();
        WorldProjection {
            world,
            ..Default::default()
        }
    }

    #[test]
    fn point_in_ring_accepts_inside_and_rejects_outside() {
        let ring = square(0.0, 0.0, 10.0);
        assert!(point_in_ring(5.0, 5.0, &ring), "环内应命中");
        assert!(!point_in_ring(15.0, 5.0, &ring), "环右外不命中");
        assert!(!point_in_ring(5.0, -1.0, &ring), "环下方不命中");
    }

    #[test]
    fn nature_region_at_returns_the_region_under_the_cursor() {
        let p = projection(vec![region(
            "nr_a",
            square(0.0, 0.0, 100.0),
            RegionKind::Nature,
        )]);
        let map = Id::new_unchecked(MAP);
        assert_eq!(
            nature_region_at(&p, &map, (50.0, 50.0)).map(|i| i.as_str().to_owned()),
            Some("nr_a".to_owned())
        );
        assert_eq!(
            nature_region_at(&p, &map, (150.0, 50.0)),
            None,
            "环外不该命中"
        );
    }

    #[test]
    fn nature_region_at_ignores_area_kind_regions() {
        // `Area` 类在 CellMap 里不显示，点它必须等于点空白。
        let p = projection(vec![region(
            "area_a",
            square(0.0, 0.0, 100.0),
            RegionKind::Area,
        )]);
        assert_eq!(
            nature_region_at(&p, &Id::new_unchecked(MAP), (50.0, 50.0)),
            None
        );
    }

    #[test]
    fn nested_regions_resolve_to_the_smallest_one() {
        let p = projection(vec![
            region("out", square(0.0, 0.0, 100.0), RegionKind::Nature),
            region("in", square(20.0, 20.0, 10.0), RegionKind::Nature),
        ]);
        let map = Id::new_unchecked(MAP);
        assert_eq!(
            nature_region_at(&p, &map, (25.0, 25.0)).map(|i| i.as_str().to_owned()),
            Some("in".to_owned()),
            "嵌套命中应取最里面（外接框最小）的那个"
        );
        assert_eq!(
            nature_region_at(&p, &map, (60.0, 60.0)).map(|i| i.as_str().to_owned()),
            Some("out".to_owned())
        );
    }

    #[test]
    fn hole_does_not_belong_to_the_region() {
        let mut r = region("nr_hole", square(0.0, 0.0, 100.0), RegionKind::Nature);
        r.holes = vec![square(40.0, 40.0, 20.0)];
        let p = projection(vec![r]);
        let map = Id::new_unchecked(MAP);
        assert_eq!(
            nature_region_at(&p, &map, (50.0, 50.0)),
            None,
            "洞里不算该区域"
        );
        assert!(nature_region_at(&p, &map, (10.0, 10.0)).is_some());
    }

    #[test]
    fn selection_target_is_a_region_selected_entity() {
        let want = SelectedEntity::new(EntityKind::Region, Id::new_unchecked("nr_a"));
        assert_eq!(
            want.kind,
            EntityKind::Region,
            "选中体的 kind 决定 Inspector 表单"
        );
    }
}
