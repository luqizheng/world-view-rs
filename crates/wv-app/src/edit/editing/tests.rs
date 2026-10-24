//! `editing` 的系统级单测（原文件尾部 `mod tests` 整段平移）。

use super::region::polygon_fingerprint;
use super::stroke::{intent_bbox_in_cells, intent_from_stroke, stroke_bbox_in_cells};
use super::terrain::terrain_diff_bbox;
use super::*;

// 笔触包围盒 → dirty 矩形（优化项 3d 的入口算法）。
use crate::state::SelectionState;
use wv_terrain::edit::Brush;

/// 单点笔触：包围盒 = 中心格 ± radius/cell_size + 1。
#[test]
fn single_point_bbox_includes_radius_padding() {
    let grid = GridConfig {
        cell_size: 10.0,
        ..Default::default()
    };
    let mut stroke = TerrainStroke::new();
    stroke.push(Brush {
        center: (50.0, 50.0), // 格 (5, 5)
        radius: 10.0,         // 1 格半径
        softness: 0.0,
        target: None,
        height_delta: 0.0,
        exact: None,
    });
    let bbox = stroke_bbox_in_cells(&stroke, &grid);
    // center = 5, pad = ceil(10/10) + 1 = 2
    // → gx0 = 3, gx1 = 7；gy0 = 3, gy1 = 7
    assert_eq!(bbox, [3, 3, 7, 7]);
}

/// 多点笔触：取并集包围盒。
#[test]
fn multi_point_bbox_is_union() {
    let grid = GridConfig {
        cell_size: 10.0,
        ..Default::default()
    };
    let mut stroke = TerrainStroke::new();
    stroke.push(Brush {
        center: (10.0, 10.0), // 格 (1, 1)
        radius: 5.0,
        softness: 0.0,
        target: None,
        height_delta: 0.0,
        exact: None,
    });
    stroke.push(Brush {
        center: (50.0, 50.0), // 格 (5, 5)
        radius: 5.0,
        softness: 0.0,
        target: None,
        height_delta: 0.0,
        exact: None,
    });
    let bbox = stroke_bbox_in_cells(&stroke, &grid);
    // pad = ceil(5/10) + 1 = 1 + 1 = 2
    // → gx0 = max(1-2, 5-2) = 0；gx1 = max(1+2, 5+2) = 7
    assert_eq!(bbox, [0, 0, 7, 7]);
}

/// 钳到 [0, ∞)：负中心被 clamp。
#[test]
fn bbox_clamps_to_zero() {
    let grid = GridConfig {
        cell_size: 10.0,
        ..Default::default()
    };
    let mut stroke = TerrainStroke::new();
    stroke.push(Brush {
        center: (5.0, 5.0), // 格 (0, 0) + radius 边距会到负
        radius: 100.0,      // 远大于 cell_size
        softness: 0.0,
        target: None,
        height_delta: 0.0,
        exact: None,
    });
    let bbox = stroke_bbox_in_cells(&stroke, &grid);
    // gx0/gx1 在负值会被 clamp 到 0
    assert_eq!(bbox[0], 0);
    assert_eq!(bbox[1], 0);
    // max 仍 = 0 + 10 + 1 = 11
    assert_eq!(bbox[2], 11);
    assert_eq!(bbox[3], 11);
}

/// 空 stroke → `[0, 0, 0, 0]`。
#[test]
fn empty_stroke_returns_zero_rect() {
    let grid = GridConfig::default();
    let stroke = TerrainStroke::new();
    let bbox = stroke_bbox_in_cells(&stroke, &grid);
    assert_eq!(bbox, [0, 0, 0, 0]);
}

// -----------------------------------------------------------------------
// 意图笔刷：点列 → Intent 的映射 + 影响包围盒（意图笔刷 plan v0.1）
// -----------------------------------------------------------------------

#[test]
fn intent_from_stroke_maps_each_tool_correctly() {
    let pts = vec![(10.0, 20.0), (30.0, 40.0)];
    // 线状意图：整条折线 + 半径 + 强度。
    match intent_from_stroke(BrushTool::MountainRange, &pts, 8.0, 1.5) {
        Some(Intent::MountainRange {
            pts: p,
            radius,
            strength,
        }) => {
            assert_eq!(p, pts);
            assert_eq!(radius, 8.0);
            assert_eq!(strength, 1.5);
        }
        other => panic!("应为 MountainRange，实得 {other:?}"),
    }
    // 点状意图：只取首点。
    match intent_from_stroke(BrushTool::Volcano, &pts, 8.0, 1.0) {
        Some(Intent::Volcano { center, .. }) => assert_eq!(center, (10.0, 20.0)),
        other => panic!("应为 Volcano，实得 {other:?}"),
    }
    match intent_from_stroke(BrushTool::Lake, &pts, 8.0, 1.0) {
        Some(Intent::Lake { center, .. }) => assert_eq!(center, (10.0, 20.0)),
        other => panic!("应为 Lake，实得 {other:?}"),
    }
    // 河：不带半径（河宽固定一格）。
    match intent_from_stroke(BrushTool::River, &pts, 8.0, 1.2) {
        Some(Intent::River { pts: p, strength }) => {
            assert_eq!(p, pts);
            assert_eq!(strength, 1.2);
        }
        other => panic!("应为 River，实得 {other:?}"),
    }
    // Paint 工具不产生意图（防御兜底）。
    assert_eq!(
        intent_from_stroke(BrushTool::PaintKind, &pts, 8.0, 1.0),
        None
    );
    // 空点列：任何工具都无意图。
    assert_eq!(intent_from_stroke(BrushTool::Volcano, &[], 8.0, 1.0), None);
}

/// 意图包围盒按 2.6×radius + 2 格外扩：单点 (50,50)、radius 10、cell 10
/// → pad = ceil(26/10)+2 = 5 → [0,0,10,10]。
#[test]
fn intent_bbox_expands_by_max_reach() {
    let grid = GridConfig {
        cell_size: 10.0,
        ..Default::default()
    };
    let bbox = intent_bbox_in_cells(&[(50.0, 50.0)], 10.0, &grid);
    // 中心格 (5,5)，pad = 2.6×10/10 → ceil(2.6)+2 = 5 → [0,0,10,10]
    assert_eq!(bbox, [0, 0, 10, 10]);
}

/// 意图包围盒取点列并集并钳到 0；空点列返回零矩形。
#[test]
fn intent_bbox_unions_and_clamps() {
    let grid = GridConfig {
        cell_size: 10.0,
        ..Default::default()
    };
    let bbox = intent_bbox_in_cells(&[(5.0, 5.0), (95.0, 95.0)], 10.0, &grid);
    // pad = 5；两点格 (0,0) 与 (9,9) → [0,0,14,14]
    assert_eq!(bbox, [0, 0, 14, 14]);
    assert_eq!(intent_bbox_in_cells(&[], 10.0, &grid), [0, 0, 0, 0]);
}

// -----------------------------------------------------------------------
// v0.2：terrain_diff_bbox 单元测试
// -----------------------------------------------------------------------

use wv_core::terrain::TerrainCellData;

fn cell(biome: u8) -> TerrainCellData {
    TerrainCellData {
        height: 0,
        surface: if biome == 0 { 0 } else { 1 },
        biome,
    }
}

/// 无变化 → None。
#[test]
fn diff_bbox_none_when_identical() {
    let a = vec![cell(3), cell(4), cell(3), cell(4)];
    let b = a.clone();
    assert_eq!(terrain_diff_bbox(&a, &b, 2), None);
}

/// 单格变化 → 恰好包住该格的 1×1 矩形。
#[test]
fn diff_bbox_single_changed_cell() {
    // 2×2：索引 3 = (1,1)。
    let before = vec![cell(1), cell(1), cell(1), cell(1)];
    let mut after = before.clone();
    after[3] = cell(7);
    assert_eq!(terrain_diff_bbox(&before, &after, 2), Some([1, 1, 2, 2]));
}

/// 多格分散变化 → 包围盒取并集。
#[test]
fn diff_bbox_scattered_changes_union() {
    // 4×4：改 (0,0) 与 (3,2) → 包围盒 [0,0]-[4,3]。
    let before = vec![cell(1); 16];
    let mut after = before.clone();
    after[0] = cell(2); // (0,0)
    after[2 * 4 + 3] = cell(2); // (3,2)
    assert_eq!(terrain_diff_bbox(&before, &after, 4), Some([0, 0, 4, 3]));
}

/// 高度变化也算变化（不只 kind）。
#[test]
fn diff_bbox_detects_height_change() {
    let before = vec![cell(1); 4];
    let mut after = before.clone();
    after[1].height = 500;
    assert_eq!(terrain_diff_bbox(&before, &after, 2), Some([1, 0, 2, 1]));
}

// -----------------------------------------------------------------------
// 大陆生成接线（大陆生成 UI plan Task 4）
// -----------------------------------------------------------------------

fn continent_test_world(size: u32) -> (WorldProjection, EditHistory, TerrainTexture, wv_core::Id) {
    let map = wv_core::Id::new_unchecked("world_main");
    let mut projection = WorldProjection::default();
    let grid = wv_terrain::TerrainGrid::new(size, size);
    let cells: Vec<wv_core::terrain::TerrainCellData> = grid
        .into_cells()
        .into_iter()
        .map(|c| wv_core::terrain::TerrainCellData {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    projection.world.set_terrain(
        map.clone(),
        wv_core::terrain::TerrainData {
            width: size,
            height: size,
            cells,
        },
    );
    let tex = TerrainTexture {
        handle: Handle::default(),
        size,
        height: size,
        res: 1,
        dirty: None,
        chunks_dirty: None,
        field_dirty: None,
        last_mode: None,
        last_soften: false,
        last_theme_id: wv_render::theme::theme_pack::builtin::DEFAULT.id,
        last_hypsometric: 0.0,
        last_map: Some(map.clone()),
    };
    (projection, EditHistory::default(), tex, map)
}

fn square_envelope() -> Vec<(f32, f32)> {
    vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)]
}

fn generate_once(
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    tex: &mut TerrainTexture,
    selection: &mut SelectionState,
    map: &wv_core::Id,
) {
    selection.close_with(square_envelope());
    let ring = selection.closed.clone().expect("选区已闭合");
    commit_continent_generation(
        map.clone(),
        vec![ring],
        ContinentParams::default(),
        7,
        1.0,
        edit,
        projection,
        tex,
    )
    .expect("生成应成功");
}

/// ADR-0033：生成成功不消费选区——圈还在，作者可继续加锚点再生成。
/// 撤销只回滚地形，选区自始至终不动。
#[test]
fn generation_keeps_selection_and_undo_restores_grid() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
    let before = projection
        .world
        .terrain_of(&map)
        .cloned()
        .expect("地形存在");
    let mut selection = SelectionState::default();

    generate_once(&mut projection, &mut edit, &mut tex, &mut selection, &map);
    assert!(selection.is_closed(), "生成成功后选区应保留");
    assert_eq!(
        selection.closed,
        Some(square_envelope()),
        "保留的应是原选区"
    );
    let after = projection.world.terrain_of(&map).cloned().unwrap();
    assert_ne!(before, after, "生成应改变地形");
    assert!(after.cells.iter().any(|c| c.height > 0), "应长出陆地");

    apply_history_command(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("撤销应成功");
    assert_eq!(
        projection.world.terrain_of(&map).cloned().unwrap(),
        before,
        "撤销后网格应逐格复原"
    );
    assert!(selection.is_closed(), "撤销生成不得清空选区");
    assert_eq!(
        selection.closed,
        Some(square_envelope()),
        "撤销后选区仍是原选区"
    );
}

#[test]
fn undo_of_other_command_keeps_current_selection() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
    let mut selection = SelectionState::default();
    generate_once(&mut projection, &mut edit, &mut tex, &mut selection, &map);

    // 再提交一条别的命令（意图山）——历史变长。
    let mut tx = Transaction::new();
    tx.push(Box::new(IntentTerrainCommand::new(
        map.clone(),
        Intent::Hills {
            pts: vec![(24.0, 24.0)],
            radius: 4.0,
            strength: 1.0,
        },
        1.0,
    )));
    edit.history
        .commit(&mut projection.world, tx)
        .expect("第二条命令应成功");

    apply_history_command(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("撤销应成功");
    assert!(selection.is_closed(), "撤销别的命令不得动选区");
    assert_eq!(selection.closed, Some(square_envelope()), "选区应原样保留");
}

/// 撤销生成后再重做：选区全程保留（不再有「再消费」语义）。
#[test]
fn redo_after_undo_keeps_selection() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
    let mut selection = SelectionState::default();
    generate_once(&mut projection, &mut edit, &mut tex, &mut selection, &map);
    apply_history_command(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("撤销");
    assert!(selection.is_closed(), "撤销后选区保留");
    assert_eq!(
        selection.closed,
        Some(square_envelope()),
        "撤销后选区仍是原选区"
    );

    apply_history_command(
        true,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("重做");
    assert!(selection.is_closed(), "重做生成不得消费选区");
    assert_eq!(
        selection.closed,
        Some(square_envelope()),
        "重做后选区原样保留"
    );
}

/// 作者在生成后改画新选区 B：撤销/重做都不得把 B 换回 A。
#[test]
fn undo_and_redo_keep_newly_drawn_selection() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
    let mut selection = SelectionState::default();
    generate_once(&mut projection, &mut edit, &mut tex, &mut selection, &map);
    let outline_b = vec![(44.0, 44.0), (47.0, 44.0), (47.0, 47.0)];
    selection.close_with(outline_b.clone());

    apply_history_command(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("撤销");
    assert_eq!(
        selection.closed,
        Some(outline_b.clone()),
        "作者新画的 B 不得被旧圈 A 覆盖"
    );

    apply_history_command(
        true,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("重做");
    assert_eq!(
        selection.closed,
        Some(outline_b),
        "重做不得覆盖作者新画的 B"
    );
}

/// 无关命令的撤销/重做都不得动选区。
#[test]
fn undo_and_redo_of_other_commands_keep_selection() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
    let mut selection = SelectionState::default();
    generate_once(&mut projection, &mut edit, &mut tex, &mut selection, &map);

    // 提交另一条命令、撤销它、再重做它——选区必须原样保留。
    let mut tx = Transaction::new();
    tx.push(Box::new(IntentTerrainCommand::new(
        map.clone(),
        Intent::Hills {
            pts: vec![(24.0, 24.0)],
            radius: 4.0,
            strength: 1.0,
        },
        1.0,
    )));
    edit.history
        .commit(&mut projection.world, tx)
        .expect("第二条命令");
    apply_history_command(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("撤销别的命令");
    apply_history_command(
        true,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("重做别的命令");
    assert_eq!(
        selection.closed,
        Some(square_envelope()),
        "无关命令的撤销/重做不得动选区"
    );
}
/// 播放中只读（基线 V37，终审 Important 3）：地形意图在播放期拒绝。
#[test]
fn commit_region_intent_rejects_during_playback() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(48);
    let before = projection.world.terrain_of(&map).cloned().unwrap();
    let mut notice = crate::ui_shell::ShellNotice::default();
    let playing = PlaybackState {
        playing: true,
        ..PlaybackState::default()
    };
    assert!(!commit_region_intent(
        map.clone(),
        Intent::MountainMassif {
            polygon: vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)],
        },
        1.0,
        &playing,
        crate::i18n::Locale::Zh,
        &mut edit,
        &mut projection,
        &mut tex,
        &mut notice,
        0,
    ));
    assert_eq!(
        projection.world.terrain_of(&map).cloned().unwrap(),
        before,
        "播放中不得改地形"
    );
}

/// 鼠标按下 + tool_active=true → cell_pick 命中 → 写入 selected。
/// [ADR-0040] §决策 3 + §决策 5。
#[test]
fn cell_edit_on_click_records_the_hit_under_the_cursor() {
    use wv_render::state::EditCellState;

    let mut world = World::new();
    world.init_resource::<Assets<Image>>();

    // 设置投影: 1 张 root map + 8×8 地形(全 0);drill_to() 落地 trail。
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
    // 命中 (gx=3, gy=4):世界坐标 (35,45) 落在 [30,40) × [40,50)。
    let view = crate::state::ViewState {
        cursor_world: Some(Vec2::new(35.0, 45.0)),
        ..Default::default()
    };
    world.insert_resource(view);
    world.insert_resource(crate::state::PlaybackState::default());
    // 系统签名含 PointerOverUi gate（2026-10-20 面板焦点 bug 修复）。
    world.insert_resource(wv_render::camera::PointerOverUi::default());
    // 系统签名含 AppScreen gate（2026-10-22 异步加载：加载期不选格）。
    world.insert_resource(crate::app_screen::AppScreen::default());

    let edit_cell = EditCellState {
        tool_active: true,
        ..Default::default()
    };
    world.insert_resource(edit_cell);

    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_edit_cell_on_click);
    schedule.run(&mut world);

    let ec = world.resource::<EditCellState>();
    assert_eq!(
        ec.selected,
        Some((3, 4)),
        "cursor_world=(35,45) + cell_size=10 + origin=0 → 命中 (gx=3, gy=4)"
    );
}

/// 指纹：同一多边形 → 同指纹；改一个顶点 → 变。
#[test]
fn polygon_fingerprint_is_stable_and_change_sensitive() {
    let a = [(0.0f32, 0.0f32), (10.0, 0.0), (10.0, 10.0)];
    let b = [(0.0f32, 0.0f32), (10.0, 0.0), (10.0, 10.5)];
    assert_eq!(
        polygon_fingerprint(&a),
        polygon_fingerprint(&a),
        "同一多边形指纹稳定"
    );
    assert_ne!(
        polygon_fingerprint(&a),
        polygon_fingerprint(&b),
        "动一个顶点指纹即变"
    );
    assert_eq!(polygon_fingerprint(&[(0.0, 0.0)]), 0, "少于 3 顶点 → 0");
}

/// 工具未激活时点击 = 不写 selected。与 `Brush.enabled` flag 同口径。
#[test]
fn cell_edit_on_click_noop_when_tool_inactive() {
    use wv_render::state::EditCellState;

    let mut world = World::new();
    world.init_resource::<Assets<Image>>();
    world.insert_resource(crate::state::WorldProjection::default());
    world.insert_resource(crate::state::DrillState::default());
    world.insert_resource(crate::state::GridConfig::default());
    let view = crate::state::ViewState {
        cursor_world: Some(Vec2::new(35.0, 45.0)),
        ..Default::default()
    };
    world.insert_resource(view);
    world.insert_resource(crate::state::PlaybackState::default());
    world.insert_resource(EditCellState::default());
    world.insert_resource(wv_render::camera::PointerOverUi::default());
    world.insert_resource(crate::app_screen::AppScreen::default());

    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_edit_cell_on_click);
    schedule.run(&mut world);

    let ec = world.resource::<EditCellState>();
    assert_eq!(ec.selected, None, "tool 没激活 = 不动 selected");
}

// -----------------------------------------------------------------------
// D8-A · 撤销白闪（[ADR-0052] §9.3 / §9.8）
// -----------------------------------------------------------------------

/// 撤销一笔地形编辑 → `changed_cells` 指出变化格（D8-A 白闪的定位依据）。
#[test]
fn undo_reports_changed_cells_for_white_flash() {
    let (mut projection, mut edit, mut tex, map) = continent_test_world(8);
    let mut before = projection.world.terrain_of(&map).cloned().unwrap();
    // 改一格高度 → 提交为一条可撤销的命令。
    before.cells[0].height += 100;
    projection.world.set_terrain(map.clone(), before.clone());
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::command::BatchEditCellsCommand::new(
        map.clone(),
        vec![wv_edit::command::CellChange {
            gx: 0,
            gy: 0,
            before: before.cells[0],
            after: {
                let mut c = before.cells[0];
                c.height += 100;
                c
            },
        }],
    )));
    edit.history
        .commit(&mut projection.world, tx)
        .expect("应提交");

    let outcome = apply_history_command_detailed(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("撤销应成功");
    assert_eq!(outcome.message, "已撤销");
    assert!(
        !outcome.changed_cells.is_empty(),
        "撤销地形编辑应报出变化格（D8-A 白闪据此定位）"
    );
    assert!(
        outcome.changed_cells.contains(&(0, 0)),
        "变化格应包含被改的那格，实际 {:?}",
        outcome.changed_cells
    );
}

/// 没有可撤销的操作 → 不报变化格（不闪）。
#[test]
fn undo_without_history_reports_no_changed_cells() {
    let (mut projection, mut edit, mut tex, _map) = continent_test_world(8);
    let outcome = apply_history_command_detailed(
        false,
        &mut edit,
        &mut projection,
        &mut tex,
        &PlaybackState::default(),
    )
    .expect("无可撤销也应返回 Ok（不是错误）");
    assert_eq!(outcome.message, "没有可撤销的操作");
    assert!(outcome.changed_cells.is_empty(), "没撤销任何东西就不该闪");
}

/// 闪视觉态：撤销后触发 → 250ms 内活跃、之后自动熄灭（D8-A）。
#[test]
fn undo_flash_is_active_for_250ms_after_undo() {
    use wv_world_cell_map::cell_flash::{CellFlashKind, CellFlashState};
    let mut flash = CellFlashState::default();
    flash.trigger(CellFlashKind::Undo, vec![(2, 3)], 10.0);
    assert!(flash.is_active(10.0), "触发即活跃");
    assert!(flash.is_active(10.249), "249ms 仍活跃");
    assert!(!flash.is_active(10.25), "250ms 到期");
}
