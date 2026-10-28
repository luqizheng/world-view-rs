//! 图钉拖动 + 双击属性（[ADR-0074] 追记三：点选器交互闭环的第二、三段）。
//!
//! 与点选单击（[`super::cell::commit_point_select_on_click`]）拼成完整手势：
//!
//! - **单击** → 选中（点击系统已做）；
//! - **按住拖动** → 钉跟光标走（本系统：候选 → 激活 → 松手提交）；
//! - **双击** → 弹出属性窗（本系统：选中 + 确保 Inspector 可见——
//!   地点表单的名字 / 呈现级别 / 锚点编辑全部现成，不另造窗体）。
//!
//! ## 提交路径（§3.3）
//!
//! 拖动期间世界数据**不动**：预览位写在 [`wv_render::state::PinDragState`]
//! （wv-render 侧资源，帧构建 / 标签读它做预览覆盖）；松手才组
//! `UpdatePlaceCommand` 经 [`EditHistory`] 一次性落账（可撤销）。
//! 与 `BiomeBoundaryState`（拖线扫格松手提交）同一手法。
//!
//! ## 双击的档位界线
//!
//! 双击弹属性**仅 Topographic（CellMap）**——浏览族的双击已被「下钻进城」
//! 占用（ADR-0074 追记一：单击选中 / 双击下钻），不抢。

use bevy::prelude::*;

use wv_render::drill::pick_place_pin;
use wv_render::state::{GridConfig, PinDragState};

use crate::state::{DrillState, Panel, PanelState, PlaybackState, ViewState, WorldProjection};
use crate::ui::inspector::place::{update_place_now, PlaceFieldPatch};

use super::EditHistory;

/// 位移门槛（屏幕像素）：按下后光标移出此半径才算拖动，否则是单击。
const DRAG_SLOP_PX: f32 = 4.0;

/// 双击窗口（秒）/ 同点容差（px）——与 `drill.rs` 的下钻双击同数值口径。
const DOUBLE_CLICK_SECS: f64 = 0.40;
const DOUBLE_CLICK_SLOP_PX: f32 = 6.0;

/// **图钉拖动 + 双击属性**：点选器激活时，按住图钉拖动改坐标、双击弹属性。
///
/// gate：点选器未激活 / 播放中 / 加载中 → 任何进行中的拖动作废并跳过；
/// 中途换图 → 预览坐标属于旧图，作废。
#[allow(clippy::too_many_arguments)]
pub fn drag_place_pin_on_hold(
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    point_select: Res<crate::state::PointSelectState>,
    screen: Res<crate::app_screen::AppScreen>,
    layers: Res<wv_render::layer_frame::LayerFrame>,
    mut projection: ResMut<WorldProjection>,
    mut drag: ResMut<PinDragState>,
    mut panels: ResMut<PanelState>,
    mut edit: ResMut<EditHistory>,
) {
    // 会话失效族：工具切走 / 播放开始 / 加载中 → 拖动作废。
    if !point_select.tool_active || playback.playing || screen.shows_loading_overlay() {
        if drag.place.is_some() {
            drag.cancel();
        }
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(map_id) = drill.current() else {
        return;
    };
    if drag.place.is_some() && drag.map.as_ref() != Some(map_id) {
        // 预览位（图内局部）属于旧图，带到新图会提交出错误坐标。
        drag.cancel();
        return;
    }

    // ── 按住：超门槛激活 → 预览位跟光标（钳在图幅内） ─────────────
    // **先于按下判定**：Bevy `ButtonInput::press` 对已按下键会再置
    // just_pressed，拖到一半的「重复按下事件」不得重置候选——凡
    // 「键仍按着 ∧ 候选在场」一律先走按住逻辑（else-if 链保证互斥）。
    if buttons.pressed(MouseButton::Left) && drag.place.is_some() {
        if let Some(start) = drag.start_world {
            if !drag.active && cursor.distance(start) > DRAG_SLOP_PX * view.scale {
                drag.active = true;
            }
            if drag.active {
                let local = cursor - grid.map_origin;
                drag.preview_local = Some((
                    local.x.clamp(0.0, grid.map_size.x),
                    local.y.clamp(0.0, grid.map_size.y),
                ));
            }
        }
    } else if buttons.just_pressed(MouseButton::Left) {
        if ui.wants_pointer {
            // 面板交互不是地图点击；若正拖着，面板弹起会松键自然收尾。
            return;
        }
        let now = time.elapsed_secs_f64();
        let is_double = drag
            .last_click
            .map(|(t, p)| {
                now - t <= DOUBLE_CLICK_SECS && p.distance(cursor) <= DOUBLE_CLICK_SLOP_PX
            })
            .unwrap_or(false);
        // 双击后清空点击史——「三连击」不得判成两段双击（drill 同口径）。
        drag.last_click = if is_double { None } else { Some((now, cursor)) };

        let pin = pick_place_pin(
            cursor,
            &projection.world,
            map_id,
            &grid,
            layers.place.visible,
            view.scale,
        );
        if is_double {
            // 双击 = 属性窗（仅 CellMap；浏览族双击归下钻）。
            if view.mode == wv_render::state::ViewMode::Topographic {
                if let Some(id) = pin {
                    projection.selection =
                        Some(wv_core::SelectedEntity::new(wv_core::EntityKind::Place, id));
                    // 地点表单在 Inspector「属性」页签——面板不可见则拉起。
                    panels.visible.insert(Panel::Inspector, true);
                }
            }
            drag.cancel();
            return;
        }
        // 单按：命中即候选（选中由点击系统同帧完成）。
        drag.place = pin;
        drag.map = Some(map_id.clone());
        drag.start_world = Some(cursor);
        drag.active = false;
        drag.preview_local = None;
    } else if buttons.just_released(MouseButton::Left) {
        if let (Some(id), true, Some((x, y))) =
            (drag.place.clone(), drag.active, drag.preview_local)
        {
            if let Some(place) = projection
                .world
                .distances
                .places
                .iter()
                .find(|p| p.id == id)
            {
                let patch = PlaceFieldPatch::anchor_only(id.clone(), place.name.clone(), x, y);
                update_place_now(&mut projection, &mut edit, &view, patch);
            }
        }
        drag.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_render::state::ViewMode;

    /// 夹具世界：1 图 + 8×8 地形 + 一枚 rank3 图钉在 (35,45) + 点选器激活 +
    /// CellMap（Topographic）视图 + Time/PanelState/PinDragState 就绪。
    fn drag_world(cursor: Vec2) -> World {
        let mut world = World::new();
        let map = wv_core::Id::new_unchecked("world_main");
        let mut projection = crate::state::WorldProjection::default();
        projection.world.meta = Some(wv_core::WorldMeta {
            name: "test".into(),
            version: "0.1".into(),
            schema_version: 2,
            root_map: map.clone(),
            timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
            template: None,
        });
        projection.world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            level: wv_core::map::MapLevel::World,
            ..Default::default()
        });
        projection.world.set_terrain(
            map.clone(),
            wv_core::terrain::TerrainData {
                width: 8,
                height: 8,
                cells: vec![wv_core::terrain::TerrainCellData::default(); 64],
            },
        );
        projection
            .world
            .distances
            .places
            .push(wv_core::distance::Place {
                kind: None,
                line: None,
                id: wv_core::Id::new_unchecked("city_pin"),
                name: "钉上城".into(),
                feature: None,
                biome: None,
                envelope: None,
                map: Some(map),
                x: Some(35.0),
                y: Some(45.0),
                w: Some(12.0),
                h: Some(8.0),
                rank: None,
                line_kind: None,
                line_width_m: None,
            });
        world.insert_resource(projection);
        let mut drill = crate::state::DrillState::default();
        drill.drill_to(
            &world.resource::<crate::state::WorldProjection>().world,
            &wv_core::Id::new_unchecked("world_main"),
        );
        world.insert_resource(drill);
        world.insert_resource(crate::state::GridConfig {
            cell_size: 10.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::new(80.0, 80.0),
        });
        world.insert_resource(crate::state::ViewState {
            mode: ViewMode::Topographic,
            cursor_world: Some(cursor),
            scale: 1.0,
            ..Default::default()
        });
        world.insert_resource(crate::state::PlaybackState::default());
        world.insert_resource(wv_render::camera::PointerOverUi::default());
        world.insert_resource(crate::app_screen::AppScreen::default());
        world.insert_resource(wv_render::layer_frame::LayerFrame::default());
        world.insert_resource(crate::state::PointSelectState { tool_active: true });
        world.insert_resource(crate::state::PanelState::default());
        world.init_resource::<crate::state::PinDragState>();
        world.init_resource::<EditHistory>();
        world.insert_resource::<bevy::time::Time>(Time::default());
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<ButtonInput<MouseButton>>();
        world
    }

    fn press_left(world: &mut World) {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
    }

    fn release_left(world: &mut World) {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
    }

    /// 进帧：清 just_pressed / just_released（保留 pressed）——
    /// `ButtonInput::clear` 即官方「模拟时间流逝」入口。
    fn advance_frame(world: &mut World) {
        world.resource_mut::<ButtonInput<MouseButton>>().clear();
    }

    fn run_drag(world: &mut World) {
        let mut schedule = Schedule::default();
        schedule.add_systems(drag_place_pin_on_hold);
        schedule.run(world);
    }

    fn set_cursor(world: &mut World, p: Vec2) {
        world.resource_mut::<crate::state::ViewState>().cursor_world = Some(p);
    }

    fn pin_pos(world: &World) -> (Option<f32>, Option<f32>) {
        let projection = world.resource::<crate::state::WorldProjection>();
        let p = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id.as_str() == "city_pin")
            .unwrap();
        (p.x, p.y)
    }

    /// 按住图钉拖动：候选 → 超门槛激活（预览跟光标）→ 松手一次性提交新坐标。
    #[test]
    fn pin_drag_moves_the_place_on_release() {
        let mut world = drag_world(Vec2::new(35.0, 45.0));
        press_left(&mut world);
        run_drag(&mut world);
        {
            let drag = world.resource::<crate::state::PinDragState>();
            assert_eq!(
                drag.place.as_ref().map(|i| i.as_str()),
                Some("city_pin"),
                "按下命中图钉 → 候选"
            );
            assert!(!drag.active, "未超门槛不激活");
        }

        // 拖到 (60,60)：距起点 ~25 > 4px 门槛 → 激活 + 预览。
        set_cursor(&mut world, Vec2::new(60.0, 60.0));
        run_drag(&mut world);
        advance_frame(&mut world);
        {
            let drag = world.resource::<crate::state::PinDragState>();
            assert!(
                drag.active,
                "超门槛应激活 place={:?} start={:?} preview={:?}",
                drag.place.as_ref().map(|i| i.as_str()),
                drag.start_world,
                drag.preview_local
            );
            assert_eq!(drag.preview_local, Some((60.0, 60.0)), "预览位跟光标");
        }
        // 松手前世界数据不动（§3.3：预览只活在渲染侧）。
        assert_eq!(pin_pos(&world), (Some(35.0), Some(45.0)), "拖动中世界不动");

        release_left(&mut world);
        run_drag(&mut world);
        assert_eq!(
            pin_pos(&world),
            (Some(60.0), Some(60.0)),
            "松手一次性提交新坐标"
        );
        {
            let projection = world.resource::<crate::state::WorldProjection>();
            assert!(projection.dirty, "提交置脏（保存提示）");
            let drag = world.resource::<crate::state::PinDragState>();
            assert!(drag.place.is_none() && !drag.active, "松手清拖动态");
            let edit = world.resource::<EditHistory>();
            assert!(edit.history.can_undo(), "提交进历史（可撤销）");
        }
    }

    /// 按下即松开（未超门槛）= 单击：不提交坐标、拖动态清空。
    #[test]
    fn pin_click_without_move_commits_nothing() {
        let mut world = drag_world(Vec2::new(35.0, 45.0));
        press_left(&mut world);
        run_drag(&mut world);
        release_left(&mut world);
        run_drag(&mut world);
        assert_eq!(pin_pos(&world), (Some(35.0), Some(45.0)), "单击不挪钉");
        let edit = world.resource::<EditHistory>();
        assert!(!edit.history.can_undo(), "单击不进历史");
        let drag = world.resource::<crate::state::PinDragState>();
        assert!(drag.place.is_none(), "松手清候选");
    }

    /// 双击图钉（CellMap）= 选中 + Inspector 可见（属性窗），且不产生拖动。
    #[test]
    fn pin_double_click_opens_inspector_in_cellmap() {
        let mut world = drag_world(Vec2::new(35.0, 45.0));
        // 预置 Inspector 不可见，验证「拉起」。
        world
            .resource_mut::<crate::state::PanelState>()
            .visible
            .insert(crate::state::Panel::Inspector, false);

        // 第一击：按下帧 → 松开帧（中间进帧清 just_pressed 残留）。
        press_left(&mut world);
        run_drag(&mut world);
        advance_frame(&mut world);
        release_left(&mut world);
        run_drag(&mut world);
        advance_frame(&mut world);
        // 第二击（Time 未推进，elapsed 仍 0 ≤ 0.40s → 双击）。
        press_left(&mut world);
        run_drag(&mut world);

        let projection = world.resource::<crate::state::WorldProjection>();
        let got = projection.selection.as_ref().map(|s| s.id.as_str());
        assert_eq!(got, Some("city_pin"), "双击选中该地点");
        let panels = world.resource::<crate::state::PanelState>();
        assert!(
            panels.is_visible(crate::state::Panel::Inspector),
            "双击拉起属性窗（Inspector）"
        );
        let drag = world.resource::<crate::state::PinDragState>();
        assert!(
            drag.place.is_none(),
            "双击后清候选，got {:?}",
            drag.place.as_ref().map(|i| i.as_str())
        );
        assert!(
            drag.last_click.is_none(),
            "双击后清点击史，got {:?}",
            drag.last_click
        );
    }

    /// 预览助手（wv-render 纯函数）：激活且匹配才给预览位。
    #[test]
    fn pin_preview_view_only_for_the_dragged_place() {
        let grid = crate::state::GridConfig {
            cell_size: 10.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::new(100.0, 100.0),
            map_size: Vec2::new(80.0, 80.0),
        };
        let id = wv_core::Id::new_unchecked("p1");
        let other = wv_core::Id::new_unchecked("p2");
        let mut drag = crate::state::PinDragState {
            place: Some(id.clone()),
            ..Default::default()
        };
        // 未激活 → None。
        assert!(wv_render::world_projection::pin_preview_view(&drag, &id, &grid).is_none());
        drag.active = true;
        drag.preview_local = Some((10.0, 20.0));
        // 激活 + 匹配 → map_to_view(预览)。
        assert_eq!(
            wv_render::world_projection::pin_preview_view(&drag, &id, &grid),
            Some(Vec2::new(110.0, 120.0)),
            "预览位 = map_origin + local"
        );
        // 别的钉 → None。
        assert!(wv_render::world_projection::pin_preview_view(&drag, &other, &grid).is_none());
    }
}
