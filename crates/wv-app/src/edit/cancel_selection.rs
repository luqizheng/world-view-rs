//! 右键单击 = 取消选择（ADR-0089 D1 通用层）。
//!
//! 交互语法的通用语义之一：**右键单击 = 取消选择**（点空白 / Esc 等价），
//! 右键**拖拽** = 平移（`camera_pan` 并行读增量，互不干扰）。原先该语义只有
//! nature overlay 一处实现（`region_select::cancel_selection_on_right_click`，
//! 本系统收编退役之）；现在跨全部工作区生效。
//!
//! ## 手势让位（ADR-0089 D1 两级仲裁）
//!
//! 进行中手势吞掉三键——下列工具的右键各有既定语义，本系统一律让位：
//! 圈选 = 撤锚（`SelectionState.enabled`）、测量 = 退模式（`MeasureMode`）、
//! 线段 POI = 收线（`LinePoiState`）、街道 = 清落点（`StreetEditorState`）、
//! 分界线在场 = 拖线（`BiomeBoundaryState`）、EditCell 笔触 = 丢笔
//! （`CellPaintSession.stroke`）、分区切刀 = 丢笔（`CutStrokeSession`）。
//! 手势空闲时，右键单击清**双选中**（实体选中 + 统一格选区，单焦点模型
//! 两面一并归零——与 `clear_cell_selection_on_escape` 同口径）。

use bevy::prelude::*;

use crate::state::{LinePoiState, MeasureState, WorldProjection};
use crate::ui::partition_edit::CutStrokeSession;
use wv_render::camera::PointerOverUi;
use wv_render::input::CLICK_MAX_PX;
use wv_render::state::{BiomeBoundaryState, CellSelection, SelectionState};
use wv_render::streets::StreetEditorState;

use super::region_select::RightDragWatch;

/// 右键**单击**（松手时位移 ≤ [`CLICK_MAX_PX`]）⇒ 取消双选中。
#[allow(clippy::too_many_arguments)]
pub fn cancel_selection_on_right_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    ui: Res<PointerOverUi>,
    selection: Res<SelectionState>,
    measure: Res<MeasureState>,
    line_poi: Res<LinePoiState>,
    paint_session: Res<wv_world_cell_map::cell_paint::CellPaintSession>,
    boundary: Res<BiomeBoundaryState>,
    streets: Res<StreetEditorState>,
    cut: Res<CutStrokeSession>,
    mut projection: ResMut<WorldProjection>,
    mut sel: ResMut<CellSelection>,
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
            if now.distance(start) > CLICK_MAX_PX {
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
    if !clicked || ui.wants_pointer {
        return;
    }
    // 手势让位：判定到松手这一刻仍有手势在场 → 右键归手势，不取消。
    // （按下后手势才激活的极端时序也在此兜住。）
    if selection.enabled
        || measure.mode != crate::state::MeasureMode::Off
        || line_poi.tool_active
        || boundary.line.is_some()
        || streets.enabled
        || cut.active
        || paint_session.stroke.active
    {
        return;
    }
    // 只在真有东西可清时写（避免每帧把 `is_changed` 打起来）。
    if projection.selection.is_some() {
        projection.selection = None;
    }
    if !sel.is_empty() {
        sel.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{MeasureMode, ViewState};

    fn set_cursor(w: &mut bevy::prelude::World, pos: Vec2) {
        let mut q = w.query::<&mut Window>();
        let mut win = q.iter_mut(w).next().unwrap();
        win.set_cursor_position(Some(pos));
    }

    fn clear_input(w: &mut bevy::prelude::World) {
        // 裸 World 没有 bevy_input 的每帧 clear——just_pressed / just_released
        // 不清就永远为真，三帧手势序列会被误判成三次按下。
        let mut input = w.remove_resource::<ButtonInput<MouseButton>>().unwrap();
        input.clear();
        w.insert_resource(input);
    }

    /// 右键按下 → （可选拖动位移）→ 松手，每步各跑一次系统
    /// （just_pressed / pressed 的移动判定 / just_released 各占一帧）。
    fn press_and_release(w: &mut bevy::prelude::World, drag_px: f32) {
        let mut schedule = bevy::ecs::schedule::Schedule::default();
        schedule.add_systems(cancel_selection_on_right_click);
        let mut input = w.remove_resource::<ButtonInput<MouseButton>>().unwrap();
        input.press(MouseButton::Right);
        w.insert_resource(input);
        schedule.run(w); // just_pressed：记起点
        clear_input(w);
        if drag_px > 0.0 {
            set_cursor(w, Vec2::new(640.0 + drag_px, 360.0));
        }
        schedule.run(w); // pressed：移动是否超阈值
        clear_input(w);
        let mut input = w.remove_resource::<ButtonInput<MouseButton>>().unwrap();
        input.release(MouseButton::Right);
        w.insert_resource(input);
        schedule.run(w); // just_released：点击判定
    }

    fn fixture(measure_gesture_on: bool) -> bevy::prelude::World {
        let mut w = bevy::prelude::World::default();
        w.insert_resource(ViewState::default());
        w.insert_resource(SelectionState::default());
        let mut measure = MeasureState::default();
        if measure_gesture_on {
            measure.mode = MeasureMode::Line;
        }
        w.insert_resource(measure);
        w.insert_resource(LinePoiState::default());
        w.insert_resource(StreetEditorState::default());
        w.insert_resource(CutStrokeSession::default());
        w.insert_resource(wv_world_cell_map::cell_paint::CellPaintSession::default());
        w.insert_resource(BiomeBoundaryState::default());
        let mut sel = CellSelection::default();
        sel.bind_map(&wv_core::Id::new_unchecked("m"), 8, 8);
        sel.replace([(1, 1)]);
        w.insert_resource(sel);
        let projection = WorldProjection {
            selection: Some(wv_core::SelectedEntity::new(
                wv_core::EntityKind::Place,
                wv_core::Id::new_unchecked("p1"),
            )),
            ..Default::default()
        };
        w.insert_resource(projection);
        w.insert_resource(PointerOverUi::default());
        w.init_resource::<ButtonInput<MouseButton>>();
        let mut window = Window::default();
        window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));
        w.spawn(window);
        w
    }

    fn selections(w: &mut bevy::prelude::World) -> (bool, bool) {
        let ent_cleared = w.resource::<WorldProjection>().selection.is_none();
        let sel_cleared = w.resource::<CellSelection>().is_empty();
        (ent_cleared, sel_cleared)
    }

    #[test]
    fn right_click_without_drag_clears_both_selections() {
        let mut w = fixture(false);
        press_and_release(&mut w, 0.0);
        let (ent_cleared, sel_cleared) = selections(&mut w);
        assert!(ent_cleared && sel_cleared, "右键单击必须清双选中");
    }

    #[test]
    fn right_click_drag_keeps_selections() {
        let mut w = fixture(false);
        press_and_release(&mut w, CLICK_MAX_PX + 10.0);
        let (ent_cleared, sel_cleared) = selections(&mut w);
        assert!(
            !ent_cleared && !sel_cleared,
            "右键拖拽 = 平移，不得取消选择"
        );
    }

    #[test]
    fn gesture_owning_tool_right_click_keeps_selections() {
        let mut w = fixture(true); // 测量模式在场 = 右键归手势（退模式）
        press_and_release(&mut w, 0.0);
        let (ent_cleared, sel_cleared) = selections(&mut w);
        assert!(
            !ent_cleared && !sel_cleared,
            "手势让位：测量在场时右键单击不得取消选择"
        );
    }
}
