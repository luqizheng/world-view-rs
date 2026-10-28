//! 泛洪选择（[ADR-0052] D6 / D6-A · §9.1；ADR-0057 统一选区）。
//!
//! ## 交互形态（D6-A）
//!
//! **点一下立即泛洪**，结果直接进统一选区 `CellSelection`——无「预览 → 确认」中间态。
//!
//! 理由：
//!
//! 1. 合理图幅下 `flood_region` 是毫秒级，预览增加一次「看 → 松手」的往返
//!    而不提供决策价值；
//! 2. 不满意可立刻点别处重新泛洪，**替换**零成本；
//! 3. 大图若慢，用底栏「泛洪中…」做感知补偿即可。
//!
//! **修饰键**（ADR-0057 D2）：无修饰 = 替换 / Shift = 并入 / Alt = 扣除。
//! 落笔断开套索活链接（ADR-0057 补记 5）。

use bevy::prelude::*;

use wv_render::state::{CellSelection, GridConfig};
use wv_world_cell_map::state::{FloodCriterion, FloodSelectState};

use crate::edit::editing::LassoLiveLink;
use crate::state::{DrillState, PlaybackState, ViewState, WorldProjection};
use crate::ui::cell_pick;

/// **泛洪选择**：点一下 → 泛洪 → 写统一选区（无修饰 = 替换 / Shift = 并 / Alt = 差）。
///
/// gate：
/// - `flood_select.tool_active` 为 false → 不响应；
/// - 指针压在 egui 面板上 → 不响应（面板交互不是选格）；
/// - 时间轴播放中 → 不响应（投影态）；
/// - 无图 / 无地形 → 不响应。
#[allow(clippy::too_many_arguments)]
pub fn flood_select_on_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut sel: ResMut<CellSelection>,
    flood_select: Res<FloodSelectState>,
    screen: Res<crate::app_screen::AppScreen>,
    mut link: ResMut<LassoLiveLink>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if !flood_select.tool_active {
        return;
    }
    if screen.shows_loading_overlay() || ui.wants_pointer || playback.playing {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let Some(target_map) = drill.current() else {
        return;
    };
    let Some(terrain) = projection.world.terrain_of(target_map) else {
        return;
    };
    let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) else {
        // 点在图外 → 不动选区（不误清）。
        return;
    };

    // 把世界地形转成 `wv_terrain::TerrainGrid` 供 `flood_region` 用。
    let grid_terrain = to_terrain_grid(terrain);
    let criterion = match flood_select.criterion {
        FloodCriterion::Surface => wv_terrain::cellops::FloodCriterion::Surface,
        FloodCriterion::Biome => wv_terrain::cellops::FloodCriterion::Biome,
        FloodCriterion::HeightTolerance(tol) => {
            wv_terrain::cellops::FloodCriterion::HeightTolerance(tol)
        }
    };
    let cells = wv_terrain::cellops::flood_region(&grid_terrain, pick.gx, pick.gy, criterion);
    if cells.is_empty() {
        return;
    }
    // 落笔前惰性定容（同图同尺寸 = 不动；切图 = 清空后重写）。
    sel.bind_map(target_map, terrain.width, terrain.height);
    // 新选择动作 → 断开套索活链接（ADR-0057 补记 5）。
    link.deactivate();
    // 修饰键（ADR-0057 D2）：无修饰 = 替换 / Shift = 并 / Alt = 差。
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if alt {
        sel.subtract(cells);
    } else if shift {
        sel.union(cells);
    } else {
        sel.replace(cells);
    }
}

/// 世界地形（`wv_core::TerrainData`）→ `wv_terrain::TerrainGrid`。
///
/// **为什么要转**：泛洪的连通性判定在 `wv_terrain::cellops` 里（地形域
/// 的纯函数），而世界数据是 `wv_core::TerrainData`（序列化格式）。
/// 这里做一次**结构转换**（不复制语义），代价 O(格数)。
pub(crate) fn to_terrain_grid(terrain: &wv_core::terrain::TerrainData) -> wv_terrain::TerrainGrid {
    let cells = terrain
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    wv_terrain::TerrainGrid::from_cells(terrain.width, terrain.height, cells)
        .expect("cells.len() == width * height（世界数据自洽）")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8×8 图幅：第 0 行前 3 格群系 = 1（其余 0）——泛洪的「同值连通区」。
    fn flood_world(base: &[(i32, i32)], modifiers: &[KeyCode]) -> World {
        let map = wv_core::Id::new_unchecked("world_main");
        let mut world = World::new();
        let mut projection = crate::state::WorldProjection::default();
        projection.world.meta = Some(wv_core::WorldMeta {
            comment: None,
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
        let mut cells = vec![wv_core::terrain::TerrainCellData::default(); 64];
        for c in cells.iter_mut().take(3) {
            c.biome = 1;
        }
        projection.world.set_terrain(
            map.clone(),
            wv_core::terrain::TerrainData {
                width: 8,
                height: 8,
                cells,
            },
        );
        world.insert_resource(projection);
        let mut drill = crate::state::DrillState::default();
        drill.drill_to(
            &world.resource::<crate::state::WorldProjection>().world,
            &map,
        );
        world.insert_resource(drill);
        world.insert_resource(crate::state::GridConfig {
            cell_size: 10.0,
            raster_origin: Vec2::ZERO,
            map_origin: Vec2::ZERO,
            map_size: Vec2::new(80.0, 80.0),
        });
        // 光标世界坐标 (5,5) → 格心 → 命中 (gx=0, gy=0)（群系 1）。
        world.insert_resource(crate::state::ViewState {
            cursor_world: Some(Vec2::new(5.0, 5.0)),
            ..Default::default()
        });
        world.insert_resource(crate::state::PlaybackState::default());
        world.insert_resource(wv_render::camera::PointerOverUi::default());
        world.insert_resource(crate::app_screen::AppScreen::default());
        world.insert_resource(FloodSelectState {
            tool_active: true,
            criterion: FloodCriterion::Biome,
        });
        let mut sel = CellSelection::default();
        sel.bind_map(&map, 8, 8);
        sel.replace(base.iter().copied());
        world.insert_resource(sel);
        world.init_resource::<LassoLiveLink>();
        let mut keys = ButtonInput::<KeyCode>::default();
        for k in modifiers {
            keys.press(*k);
        }
        world.insert_resource(keys);
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Left);
        world.insert_resource(buttons);
        world
    }

    fn run_flood(world: &mut World) {
        let mut schedule = Schedule::default();
        schedule.add_systems(flood_select_on_click);
        schedule.run(world);
    }

    /// 无修饰 = **替换**（ADR-0057 D2）。
    #[test]
    fn flood_without_modifier_replaces_unified_selection() {
        let mut world = flood_world(&[(7, 7)], &[]);
        run_flood(&mut world);
        let sel = world.resource::<CellSelection>();
        assert_eq!(sel.len(), 3, "群系 1 的连通区 = 3 格，实际 {}", sel.len());
        for gx in 0..3 {
            assert!(sel.contains(gx, 0), "同群系格 ({gx},0) 应入选");
        }
        assert!(!sel.contains(7, 7), "无修饰 = 替换：旧选区必须清掉");
    }

    /// Shift = **并集**。
    #[test]
    fn flood_with_shift_unions_into_unified_selection() {
        let mut world = flood_world(&[(7, 7)], &[KeyCode::ShiftLeft]);
        run_flood(&mut world);
        let sel = world.resource::<CellSelection>();
        assert_eq!(sel.len(), 4, "3 格泛洪 ∪ 既有 (7,7) = 4");
        assert!(sel.contains(7, 7));
    }

    /// Alt = **差集**。
    #[test]
    fn flood_with_alt_subtracts_from_unified_selection() {
        let mut world = flood_world(&[(0, 0), (7, 7)], &[KeyCode::AltLeft]);
        run_flood(&mut world);
        let sel = world.resource::<CellSelection>();
        assert_eq!(sel.len(), 1, "3 格泛洪被扣除，只剩 (7,7)");
        assert!(sel.contains(7, 7));
        assert!(!sel.contains(0, 0) && !sel.contains(1, 0));
    }
}
