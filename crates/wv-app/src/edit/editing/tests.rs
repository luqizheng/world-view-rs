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

/// [ADR-0090 D2] 浏览族的点选不再写格：点击命中格也不动既有选区
/// （格子选择移交圈选「单格」形状，仅 Topographic）。
#[test]
fn point_select_click_in_browse_no_longer_writes_cells() {
    use crate::state::{CellSelection, PointSelectState};

    let mut world = World::new();
    world.init_resource::<Assets<Image>>();

    // 设置投影: 1 张 root map + 8×8 地形(全 0);drill_to() 落地 trail。
    let map = wv_core::Id::new_unchecked("world_main");
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
    // 系统签名含 LayerFrame gate（ADR-0074 图钉命中要读 place 层可见性）。
    world.insert_resource(wv_render::layer_frame::LayerFrame::default());
    // 系统签名含 FloodSelectState（追记四：CellMap 默认点击=泛洪判据）。
    world.insert_resource(crate::state::FloodSelectState::default());
    world.init_resource::<super::region::LassoLiveLink>();

    world.insert_resource(PointSelectState { tool_active: true });
    // 预置一格旧选区：无修饰点击应整体替换（不是并入）。
    let mut sel = CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace([(0, 0)]);
    world.insert_resource(sel);
    world.insert_resource(ButtonInput::<KeyCode>::default());

    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<CellSelection>();
    assert!(
        sel.contains(0, 0) && sel.len() == 1,
        "浏览族点击不再写格：旧选区 (0,0) 原样保留，实际 {}",
        sel.len()
    );
    assert!(!sel.contains(3, 4), "命中格 (3,4) 不应被写入选区");
}

/// [ADR-0090 D2] 浏览族点击带修饰键同样不碰格子（Shift/Alt 是格子选择的
/// 「选区加减法」，随格子职责移交圈选「单格」形状）。
#[test]
fn point_select_click_in_browse_ignores_modifier_cells() {
    use crate::state::{CellSelection, PointSelectState};

    let mut world = World::new();
    world.init_resource::<Assets<Image>>();
    let map = wv_core::Id::new_unchecked("world_main");
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
    world.insert_resource(crate::state::ViewState {
        cursor_world: Some(Vec2::new(35.0, 45.0)),
        ..Default::default()
    });
    world.insert_resource(crate::state::PlaybackState::default());
    world.insert_resource(wv_render::camera::PointerOverUi::default());
    world.insert_resource(crate::app_screen::AppScreen::default());
    // 系统签名含 LayerFrame gate（ADR-0074 图钉命中要读 place 层可见性）。
    world.insert_resource(wv_render::layer_frame::LayerFrame::default());
    // 系统签名含 FloodSelectState（追记四：CellMap 默认点击=泛洪判据）。
    world.insert_resource(crate::state::FloodSelectState::default());
    world.init_resource::<super::region::LassoLiveLink>();
    world.insert_resource(PointSelectState { tool_active: true });
    // 既有选区含命中格 (3,4) 与另一格 (0,0)。
    let mut sel = CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace([(0, 0), (3, 4)]);
    world.insert_resource(sel);

    // Shift 点击 (3,4)：并入（已在 → 无新增，但 (0,0) 必须保留）。
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);
    {
        let sel = world.resource::<CellSelection>();
        assert_eq!(sel.len(), 2, "浏览族 Shift 点击不写格：集合原样");
        assert!(sel.contains(0, 0) && sel.contains(3, 4));
    }

    // Alt 点击 (3,4)：同样不动选区。
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);
    {
        let sel = world.resource::<CellSelection>();
        assert!(
            sel.contains(3, 4) && sel.len() == 2,
            "浏览族 Alt 点击不扣格：集合原样，实际 {}",
            sel.len()
        );
    }
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

// ── A4 · 套索 V / 框选 R 共用入口（`SelectionState.closed` → 统一选区）──

/// 构造「闭合环 → 统一选区」的测试世界（8×8 图幅、格宽 10、原点 0）。
///
/// 环是**本图局部坐标**（与 `commit_selection_click` 落盘口径一致）；
/// 本 fixture 图帧原点为 0，故局部 = 世界。
fn lasso_world(base: &[(i32, i32)], modifiers: &[KeyCode], ring: Vec<(f32, f32)>) -> World {
    use crate::state::SelectionState;

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
    world.insert_resource(SelectionState {
        enabled: true,
        closed: Some(ring),
        ..Default::default()
    });
    let mut keys = ButtonInput::<KeyCode>::default();
    for k in modifiers {
        keys.press(*k);
    }
    world.insert_resource(keys);
    let mut sel = crate::state::CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace(base.iter().copied());
    world.insert_resource(sel);
    world.init_resource::<super::region::LassoLiveLink>();
    world
}

fn run_region_sync(world: &mut World) {
    let mut schedule = Schedule::default();
    schedule.add_systems(sync_region_cells);
    schedule.run(world);
}

/// 无修饰闭合 = **替换**（旧选区不保留）。
#[test]
fn lasso_close_without_modifier_replaces_unified_selection() {
    // 局部方环 (0,0)-(30,30)：格心 5/15/25 落入 → 3×3 = 9 格。
    let ring = vec![(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (0.0, 30.0)];
    let mut world = lasso_world(&[(7, 7)], &[], ring);
    run_region_sync(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(
        sel.len(),
        9,
        "方环应栅格化出 3×3 = 9 格，实际 {}",
        sel.len()
    );
    assert!(sel.contains(0, 0) && sel.contains(2, 2), "环内格入选");
    assert!(!sel.contains(7, 7), "无修饰 = 替换：旧选区必须清掉");
}

/// Shift 闭合 = **并集**（旧选区保留），且拖锚调形时从**冻结基座**重算。
#[test]
fn lasso_close_with_shift_unions_and_recomputes_from_frozen_base() {
    let ring = vec![(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (0.0, 30.0)];
    let mut world = lasso_world(&[(7, 7)], &[KeyCode::ShiftLeft], ring);
    run_region_sync(&mut world);
    {
        let sel = world.resource::<crate::state::CellSelection>();
        assert_eq!(sel.len(), 10, "Shift = 并集：9 格 + 既有 (7,7)");
        assert!(sel.contains(7, 7), "既有格必须保留");
    }

    // 拖锚调形：环缩小到 (0,0)-(10,10) → 仅 1 格。
    // 若基座没有冻结（在本帧选区上并），这里会残留上一帧的 9 格。
    world.resource_mut::<crate::state::SelectionState>().closed =
        Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
    run_region_sync(&mut world);
    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(
        sel.len(),
        2,
        "缩小后 = 基座 (7,7) + 新环内 1 格，实际 {}",
        sel.len()
    );
    assert!(sel.contains(0, 0) && sel.contains(7, 7));
    assert!(!sel.contains(2, 2), "上一帧的环内格必须被撤掉（基座冻结）");
}

/// Alt 闭合 = **差集**（从既有选区扣除环内格）。
#[test]
fn lasso_close_with_alt_subtracts_from_unified_selection() {
    let ring = vec![(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (0.0, 30.0)];
    let mut world = lasso_world(&[(0, 0), (7, 7)], &[KeyCode::AltLeft], ring);
    run_region_sync(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(sel.len(), 1, "Alt = 差集：只剩环外的 (7,7)");
    assert!(sel.contains(7, 7), "环外格不受影响");
    assert!(!sel.contains(0, 0), "环内格应被扣除");
}

/// 闭合消失（取消 / 切图 / 新手势）→ 结果**钉住不回滚**。
#[test]
fn lasso_link_disconnect_pins_result_without_rollback() {
    let ring = vec![(0.0, 0.0), (30.0, 0.0), (30.0, 30.0), (0.0, 30.0)];
    let mut world = lasso_world(&[], &[], ring);
    run_region_sync(&mut world);
    assert_eq!(world.resource::<crate::state::CellSelection>().len(), 9);

    world.resource_mut::<crate::state::SelectionState>().closed = None;
    run_region_sync(&mut world);
    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(sel.len(), 9, "闭合消失后结果钉住（不回滚到空）");
    assert!(sel.contains(1, 1));
}

/// 点选工具未激活时点击 = 不写统一选区（ADR-0057 补记 6：PointSelect 独立开关）。
#[test]
fn point_select_on_click_noop_when_tool_inactive() {
    use crate::state::{CellSelection, PointSelectState};

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
    world.insert_resource(PointSelectState::default()); // tool_active = false
    world.insert_resource(CellSelection::default());
    world.insert_resource(wv_render::camera::PointerOverUi::default());
    world.insert_resource(crate::app_screen::AppScreen::default());
    // 系统签名含 LayerFrame gate（ADR-0074 图钉命中要读 place 层可见性）。
    world.insert_resource(wv_render::layer_frame::LayerFrame::default());
    // 系统签名含 FloodSelectState（追记四：CellMap 默认点击=泛洪判据）。
    world.insert_resource(crate::state::FloodSelectState::default());
    world.init_resource::<super::region::LassoLiveLink>();
    world.insert_resource(ButtonInput::<KeyCode>::default());

    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<CellSelection>();
    assert!(sel.is_empty(), "tool 没激活 = 不动统一选区");
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

// ═══ ADR-0074：CellMap 点选三段裁定（图钉 > 群系区 > 格）═══

/// 点状图钉夹具：锚定 `map`、rank 3、无折线。
fn pin(id: &str, map: &wv_core::Id, x: f32, y: f32) -> wv_core::distance::Place {
    wv_core::distance::Place {
        id: wv_core::Id::new_unchecked(id),
        name: id.into(),
        map: Some(map.clone()),
        x: Some(x),
        y: Some(y),
        rank: Some(3),
        ..empty_place()
    }
}

/// 除 id/name 外全空的 Place（其余字段全 Option，`None` 即旧工程缺省语义）。
fn empty_place() -> wv_core::distance::Place {
    wv_core::distance::Place {
        id: wv_core::Id::new_unchecked("_"),
        name: String::new(),
        feature: None,
        map: None,
        x: None,
        y: None,
        w: None,
        h: None,
        biome: None,
        envelope: None,
        rank: None,
        line: None,
        kind: None,
        line_kind: None,
        line_width_m: None,
    }
}

/// Nature 区夹具：本图、已命名、方形轮廓 `[x0,y0]-[x1,y1]`。
fn nature_region(
    id: &str,
    map: &wv_core::Id,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
) -> wv_core::model::region::Region {
    wv_core::model::region::Region {
        id: wv_core::Id::new_unchecked(id),
        name: id.into(),
        map: map.clone(),
        polygon: vec![x0, y0, x1, y0, x1, y1, x0, y1],
        kind: wv_core::model::region::RegionKind::Nature,
        ..empty_region()
    }
}

fn empty_region() -> wv_core::model::region::Region {
    wv_core::model::region::Region {
        id: wv_core::Id::new_unchecked("_"),
        name: String::new(),
        map: wv_core::Id::new_unchecked("_"),
        polygon: Vec::new(),
        holes: Vec::new(),
        kind: wv_core::model::region::RegionKind::default(),
        parent: None,
        overlay: None,
    }
}

#[test]
fn point_in_flat_polygon_hits_inside_and_misses_outside() {
    let square = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
    assert!(
        super::cell::point_in_flat_polygon(5.0, 5.0, &square),
        "环内应命中"
    );
    assert!(
        !super::cell::point_in_flat_polygon(15.0, 5.0, &square),
        "环外不应命中"
    );
    assert!(
        !super::cell::point_in_flat_polygon(-1.0, -1.0, &square),
        "环外（负侧）不应命中"
    );
}

#[test]
fn point_in_flat_polygon_rejects_degenerate_rings() {
    assert!(!super::cell::point_in_flat_polygon(0.0, 0.0, &[]), "空环");
    assert!(
        !super::cell::point_in_flat_polygon(0.0, 0.0, &[1.0, 1.0]),
        "单点环"
    );
    assert!(
        !super::cell::point_in_flat_polygon(0.0, 0.0, &[0.0, 0.0, 4.0, 4.0]),
        "两点共线环"
    );
}

#[test]
fn pick_place_pin_prefers_nearest_pin_within_radius() {
    let map = wv_core::Id::new_unchecked("world_main");
    let mut world = wv_core::World::default();
    world
        .distances
        .places
        .push(pin("city_a", &map, 100.0, 100.0));
    world
        .distances
        .places
        .push(pin("city_b", &map, 130.0, 100.0));
    let grid = GridConfig::default();
    // 光标在 (108,100)：距 a 8、距 b 22，scale=1 → 半径 18 内只有 a。
    let got = wv_render::drill::pick_place_pin(
        Vec2::new(108.0, 100.0),
        &world,
        &map,
        &grid,
        true,
        1.0,
        wv_render::state::LodTier::Edit,
    );
    assert_eq!(
        got.as_ref().map(|i| i.as_str()),
        Some("city_a"),
        "最近的图钉先被点到"
    );
    // 两钉正中 (115,100)：距各 15 ≤ 18 → 应命中其一。
    let mid = wv_render::drill::pick_place_pin(
        Vec2::new(115.0, 100.0),
        &world,
        &map,
        &grid,
        true,
        1.0,
        wv_render::state::LodTier::Edit,
    );
    assert!(mid.is_some(), "等距 15 ≤ 18 应命中其一");
    // 远处点击：两钉都在半径外 → None。
    let far = wv_render::drill::pick_place_pin(
        Vec2::new(300.0, 300.0),
        &world,
        &map,
        &grid,
        true,
        1.0,
        wv_render::state::LodTier::Edit,
    );
    assert!(far.is_none(), "半径外不命中");
}

#[test]
fn pick_place_pin_follows_render_visibility_single_source() {
    let map = wv_core::Id::new_unchecked("world_main");
    let mut world = wv_core::World::default();
    // 线段 POI：rank 3 但带折线 → 无图钉。
    let mut river = pin("river_x", &map, 100.0, 100.0);
    river.line = Some(vec![0.0, 0.0, 200.0, 200.0]);
    world.distances.places.push(river);
    // 低级别地点：rank 1 → 渲染随 LOD 档显隐，拾取同门（UX-032 单真源）。
    let mut dot = pin("dot_y", &map, 100.0, 100.0);
    dot.rank = Some(1);
    world.distances.places.push(dot);
    // 其他图的图钉：不参与本图命中。
    let other_map = wv_core::Id::new_unchecked("other");
    world
        .distances
        .places
        .push(pin("other_city", &other_map, 100.0, 100.0));
    let grid = GridConfig::default();

    let cursor = Vec2::new(100.0, 100.0);
    // rank-1 在 Edit 档渲染 → 可拾取（旧实现自写 rank<2 硬门，点不中看得见的）。
    let got_edit = wv_render::drill::pick_place_pin(
        cursor,
        &world,
        &map,
        &grid,
        true,
        1.0,
        wv_render::state::LodTier::Edit,
    );
    assert_eq!(
        got_edit.as_ref().map(|i| i.as_str()),
        Some("dot_y"),
        "Edit 档可见的 rank-1 点应可拾取"
    );
    // Nation 档 rank-1 已隐藏 → 不可拾取（旧实现点得中看不见的）。
    let got_nation = wv_render::drill::pick_place_pin(
        cursor,
        &world,
        &map,
        &grid,
        true,
        1.0,
        wv_render::state::LodTier::Nation,
    );
    assert!(
        got_nation.is_none(),
        "Nation 档已隐藏的 rank-1 点不可拾取，线段 POI / 他图图钉也不命中"
    );
    // place 图层隐藏 → 图钉不画 → 不命中（即便真图钉在场）。
    world
        .distances
        .places
        .push(pin("city_a", &map, 100.0, 100.0));
    assert!(
        wv_render::drill::pick_place_pin(
            cursor,
            &world,
            &map,
            &grid,
            false,
            1.0,
            wv_render::state::LodTier::Edit,
        )
        .is_none(),
        "图层隐藏时不命中"
    );
}

#[test]
fn pick_nature_region_hits_only_named_nature_regions_of_this_map() {
    let map = wv_core::Id::new_unchecked("world_main");
    let mut world = wv_core::World::default();
    world
        .regions
        .push(nature_region("forest", &map, 0.0, 0.0, 40.0, 40.0));
    // 无名 Nature 区：不画 → 选不到。
    let mut unnamed = nature_region("unnamed", &map, 50.0, 50.0, 80.0, 80.0);
    unnamed.name = String::new();
    world.regions.push(unnamed);
    // 非 Nature 区（缺省 Area）：群系裁定不碰它。
    let mut area = nature_region("area_z", &map, 90.0, 90.0, 120.0, 120.0);
    area.kind = wv_core::model::region::RegionKind::Area;
    world.regions.push(area);
    // 他图 Nature 区。
    let other_map = wv_core::Id::new_unchecked("other");
    world.regions.push(nature_region(
        "other_forest",
        &other_map,
        0.0,
        0.0,
        40.0,
        40.0,
    ));

    let hit = super::cell::pick_nature_region((20.0, 20.0), &world, &map);
    assert_eq!(
        hit.as_ref().map(|i| i.as_str()),
        Some("forest"),
        "环内命中本图已命名 Nature 区"
    );
    // 无名区环内 → 不命中它（也不命中别的）。
    assert!(
        super::cell::pick_nature_region((60.0, 60.0), &world, &map).is_none(),
        "无名区不可选"
    );
    assert!(
        super::cell::pick_nature_region((100.0, 100.0), &world, &map).is_none(),
        "Area 区不参与群系裁定"
    );
    // map 过滤的正向证据：同一点 (20,20) 以他图身份查询 → 命中他图区
    // （而不是本图的 forest）。
    let cross = super::cell::pick_nature_region((20.0, 20.0), &world, &other_map);
    assert_eq!(
        cross.as_ref().map(|i| i.as_str()),
        Some("other_forest"),
        "按查询图命中他图区，本图 forest 不掺进来"
    );
}

/// 夹具世界：1 图 + 8×8 地形 + drill 落地 + CellMap（Topographic）视图。
fn cellmap_world(cursor: Vec2, nature_overlay: bool) -> bevy::prelude::World {
    let mut world = bevy::prelude::World::new();
    world.init_resource::<Assets<Image>>();
    let map = wv_core::Id::new_unchecked("world_main");
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
    world.insert_resource(crate::state::ViewState {
        mode: wv_render::state::ViewMode::Topographic,
        cursor_world: Some(cursor),
        scale: 1.0,
        nature_overlay,
        ..Default::default()
    });
    world.insert_resource(crate::state::PlaybackState::default());
    world.insert_resource(wv_render::camera::PointerOverUi::default());
    world.insert_resource(crate::app_screen::AppScreen::default());
    world.insert_resource(wv_render::layer_frame::LayerFrame::default());
    // 泛洪判据（ADR-0090 D3：圈选「单格」形状点击的泛洪档，默认 Off = 单格）。
    world.insert_resource(crate::state::FloodSelectState::default());
    world.init_resource::<super::region::LassoLiveLink>();
    world
}

/// CellMap 三段裁定第〇段（2026-10-31 补）：标签命中优先于图钉。
///
/// 背景：CellMap 的标签自 ADR-0051 §7 起可见且注册命中表，但消费者只有
/// 浏览族 `drill_pick`（Topographic 早退）——点标签文字毫无反应（用户实测
/// 「点东港不高亮」）。修复后 CellMap 与浏览族同序：标签 > 图钉 > 群系区。
#[test]
fn point_select_in_cellmap_picks_label_over_pin() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    let map = wv_core::Id::new_unchecked("world_main");
    {
        let mut p = world.resource_mut::<crate::state::WorldProjection>();
        p.world
            .distances
            .places
            .push(pin("city_qingyun", &map, 35.0, 45.0));
    }
    // 标签命中表：命中框以光标为中心（模拟上一帧标签系统的注册）。
    let mut hits = wv_render::label_pick::LabelHits::default();
    hits.push(wv_render::label_pick::LabelHit {
        center: Vec2::new(35.0, 45.0),
        half: Vec2::new(20.0, 8.0),
        target: wv_core::SelectedEntity::new(
            wv_core::EntityKind::Place,
            wv_core::Id::new_unchecked("city_qingyun"),
        ),
    });
    world.insert_resource(hits);
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let projection = world.resource::<crate::state::WorldProjection>();
    let got = projection
        .selection
        .as_ref()
        .map(|s| (s.kind, s.id.as_str()));
    assert_eq!(
        got,
        Some((wv_core::EntityKind::Place, "city_qingyun")),
        "点标签文字应选中该 Place，实际 {got:?}"
    );
}

/// 标签层 hidden → 标签段让路，图钉照常可点（hidden 图层不可选，ADR-0090 D2）。
#[test]
fn point_select_in_cellmap_skips_label_hits_when_label_layer_hidden() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    let map = wv_core::Id::new_unchecked("world_main");
    {
        let mut p = world.resource_mut::<crate::state::WorldProjection>();
        p.world
            .distances
            .places
            .push(pin("city_qingyun", &map, 35.0, 45.0));
    }
    let mut hits = wv_render::label_pick::LabelHits::default();
    hits.push(wv_render::label_pick::LabelHit {
        center: Vec2::new(35.0, 45.0),
        half: Vec2::new(20.0, 8.0),
        target: wv_core::SelectedEntity::new(
            wv_core::EntityKind::Place,
            wv_core::Id::new_unchecked("city_qingyun"),
        ),
    });
    world.insert_resource(hits);
    // 标签声明层关掉：LayerFrame::resolve(None, view) 的 label 槽沿会话开关
    // （缺省 true）——这里显式置 false 模拟「标签层关闭」的会话态。
    let mut layers = wv_render::layer_frame::LayerFrame::default();
    layers.label.visible = false;
    world.insert_resource(layers);
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let projection = world.resource::<crate::state::WorldProjection>();
    let got = projection
        .selection
        .as_ref()
        .map(|s| (s.kind, s.id.as_str()));
    assert_eq!(
        got,
        Some((wv_core::EntityKind::Place, "city_qingyun")),
        "标签层关 = 标签命中让路，光标同时压在图钉上 → 仍选图钉，实际 {got:?}"
    );
}

/// CellMap 三段裁定第一段：点在图钉上 → 选 Place 实体、清格选区（不写格）。
#[test]
fn point_select_in_cellmap_picks_place_pin_over_cell() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), true);
    let map = wv_core::Id::new_unchecked("world_main");
    {
        let mut p = world.resource_mut::<crate::state::WorldProjection>();
        p.world
            .distances
            .places
            .push(pin("city_qingyun", &map, 35.0, 45.0));
    }
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    let mut sel = crate::state::CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace([(0, 0)]);
    world.insert_resource(sel);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert!(sel.is_empty(), "选实体 = 清格选区（单焦点）");
    let projection = world.resource::<crate::state::WorldProjection>();
    let got = projection
        .selection
        .as_ref()
        .map(|s| (s.kind, s.id.as_str()));
    assert_eq!(
        got,
        Some((wv_core::EntityKind::Place, "city_qingyun")),
        "图钉命中优先于格（cursor 同格也应选钉），got={got:?}"
    );
}

/// CellMap 三段裁定第二段：无图钉 + 群系 overlay 开 + 点在 Nature 区内 → 选 Region。
#[test]
fn point_select_in_cellmap_picks_nature_region_when_overlay_on() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), true);
    let map = wv_core::Id::new_unchecked("world_main");
    {
        let mut p = world.resource_mut::<crate::state::WorldProjection>();
        p.world
            .regions
            .push(nature_region("forest_south", &map, 20.0, 20.0, 60.0, 60.0));
    }
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert!(sel.is_empty(), "选群系区 = 不写格");
    let projection = world.resource::<crate::state::WorldProjection>();
    let got = projection
        .selection
        .as_ref()
        .map(|s| (s.kind, s.id.as_str()));
    assert_eq!(
        got,
        Some((wv_core::EntityKind::Region, "forest_south")),
        "got={got:?}"
    );
}

/// [ADR-0094 追记] 双击群系格 = 泛洪选中 + **打开笔刷面板**（CellMap 编辑
/// 段所在——格子表单 2026-10-24 迁入笔刷窗，双击把编辑框带上，不让作者
/// 猜「编辑框在哪里」）。实体双击仍拉 Inspector（别处已锚定）。
#[test]
fn double_click_on_biome_floods_and_opens_brush_panel() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(crate::state::CellSelection::default());
    let mut panels = crate::state::PanelState::default();
    panels.visible.insert(crate::state::Panel::Brush, false);
    world.insert_resource(panels);
    world.insert_resource(bevy::time::Time::<()>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);

    // 第一击：泛洪，笔刷面板不动。
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    schedule.run(&mut world);
    assert!(
        !world
            .resource::<crate::state::PanelState>()
            .visible
            .get(&crate::state::Panel::Brush)
            .copied()
            .unwrap_or(false),
        "单击不该开笔刷面板"
    );

    // 第二击（同位同时窗）= 双击：泛洪 + 开笔刷面板。
    let mut buttons = world
        .remove_resource::<ButtonInput<MouseButton>>()
        .expect("首击按钮应已插入");
    buttons.release(MouseButton::Left);
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(sel.len(), 64, "双击照常泛洪群系");
    let panels = world.resource::<crate::state::PanelState>();
    assert!(
        panels
            .visible
            .get(&crate::state::Panel::Brush)
            .copied()
            .unwrap_or(false),
        "双击群系格应打开笔刷面板（编辑框所在）"
    );
}

/// [ADR-0094] 群系兜底：群系区实体**存在但 overlay 关**（看不见的实体
/// 不可选）→ 点击退到**同群系连通域泛洪**——「哪怕它没有创建」的一半：
/// 没创建泛洪格；建了但看不见也泛洪格。夹具 8×8 全同群系 → 淹全图 64 格。
#[test]
fn point_select_floods_biome_when_region_exists_but_overlay_off() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    let map = wv_core::Id::new_unchecked("world_main");
    {
        let mut p = world.resource_mut::<crate::state::WorldProjection>();
        p.world
            .regions
            .push(nature_region("forest_south", &map, 20.0, 20.0, 60.0, 60.0));
    }
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(
        sel.len(),
        64,
        "ADR-0094 群系兜底：非 POI 点击 = 同群系泛洪，实际 {}",
        sel.len()
    );
    let projection = world.resource::<crate::state::WorldProjection>();
    assert!(
        projection.selection.is_none(),
        "泛洪写格 = 实体选中让位（单焦点模型）"
    );
}

/// [ADR-0094] 群系兜底的另一半：工程里**根本没有**群系区（jrpg-world
/// 实况）→ 点击照样泛洪选中脚下群系。
#[test]
fn point_select_floods_biome_without_any_nature_region() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(
        sel.len(),
        64,
        "群系区未创建也应有得选（同群系泛洪），实际 {}",
        sel.len()
    );
}

/// [ADR-0090 D2 → ADR-0094] 点选的 Alt 直选单格已退役（Alt 随格子职责
/// 移交圈选「单格」形状）：Alt+左键不再抓单格，但按群系兜底裁定照样
/// **泛洪群系**（修饰键不改变「非 POI = 选群系」的语义）。
#[test]
fn alt_click_with_point_select_no_longer_grabs_cell() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    world.insert_resource(crate::state::CellSelection::default());
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(
        sel.len(),
        64,
        "Alt 不再抓单格（抓格归圈选），但群系兜底照样泛洪，实际 {}",
        sel.len()
    );
    let projection = world.resource::<crate::state::WorldProjection>();
    assert!(projection.selection.is_none());
}

/// 单焦点另一面：泛洪写格清实体选中——否则 Cell 表单恒优先
/// （`resolve_form`），实体会变成看不见的悬挂选中。
#[test]
fn cell_shape_click_floods_and_clears_entity_selection() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), true);
    let map = wv_core::Id::new_unchecked("world_main");
    {
        let mut p = world.resource_mut::<crate::state::WorldProjection>();
        p.selection = Some(wv_core::SelectedEntity::new(
            wv_core::EntityKind::Place,
            wv_core::Id::new_unchecked("city_old"),
        ));
    }
    // 工具态 = 圈选激活 + 单格形状（ADR-0090 D3）。
    world.insert_resource(SelectionState {
        enabled: true,
        shape: wv_render::state::SelectionShape::Cell,
        ..Default::default()
    });
    // 判据 = 按群系（非 Off 档 → 点击 = 泛洪，ADR-0088 手感由判据档位承载）。
    world
        .resource_mut::<crate::state::FloodSelectState>()
        .criterion = wv_world_cell_map::state::FloodCriterion::Biome;
    let mut sel = crate::state::CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace([(0, 0)]);
    world.insert_resource(sel);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_cell_shape_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(
        sel.len(),
        64,
        "判据=按群系 → 泛洪全图同群系连通区，实际 {}",
        sel.len()
    );
    assert!(sel.contains(3, 4), "落点格 (3,4) 在泛洪区内");
    let projection = world.resource::<crate::state::WorldProjection>();
    assert!(
        projection.selection.is_none(),
        "泛洪写格 = 实体选中让位（单焦点模型）"
    );
}

/// [ADR-0090 D3] 判据 Off（默认档）→ 圈选「单格」形状点击 = 直选单格。
#[test]
fn cell_shape_click_selects_single_cell_when_criterion_off() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    world.insert_resource(SelectionState {
        enabled: true,
        shape: wv_render::state::SelectionShape::Cell,
        ..Default::default()
    });
    // 判据默认 = Off（新默认档），无需显式设置。
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_cell_shape_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert!(
        sel.contains(3, 4) && sel.len() == 1,
        "判据 Off = 直选单格 (3,4)，实际 {}",
        sel.len()
    );
    let projection = world.resource::<crate::state::WorldProjection>();
    assert!(projection.selection.is_none(), "选格 = 实体选中让位");
}

/// [ADR-0090 D3] 工具态未激活（圈选关）→ 单格点击 no-op。
#[test]
fn cell_shape_click_noop_when_select_tool_inactive() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    // SelectionState 默认 enabled=false，形状即便默认多边形也无所谓。
    world.init_resource::<SelectionState>();
    world.insert_resource(crate::state::CellSelection::default());
    world.insert_resource(ButtonInput::<KeyCode>::default());
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);

    let mut schedule = Schedule::default();
    schedule.add_systems(commit_cell_shape_click);
    schedule.run(&mut world);

    let sel = world.resource::<crate::state::CellSelection>();
    assert_eq!(sel.len(), 0, "工具未激活不得写选区");
}

/// [ADR-0090 D3] 圈选「单格」形状的修饰键 = 选区加减法：
/// Shift 点击 = 并入、Alt 点击 = 扣除（判据 Off 档）。
#[test]
fn cell_shape_click_shift_unions_and_alt_subtracts() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), false);
    let map = wv_core::Id::new_unchecked("world_main");
    world.insert_resource(SelectionState {
        enabled: true,
        shape: wv_render::state::SelectionShape::Cell,
        ..Default::default()
    });
    // 既有选区含命中格 (3,4) 与另一格 (0,0)。
    let mut sel = crate::state::CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace([(0, 0), (3, 4)]);
    world.insert_resource(sel);

    // Shift 点击 (3,4)：并入（已在 → 集合不变，(0,0) 保留）。
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ShiftLeft);
    world.insert_resource(keys);
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_cell_shape_click);
    schedule.run(&mut world);
    {
        let sel = world.resource::<crate::state::CellSelection>();
        assert_eq!(sel.len(), 2, "Shift 并入已选格 = 集合不变");
        assert!(sel.contains(0, 0) && sel.contains(3, 4));
    }

    // Alt 点击 (3,4)：从选区扣除。
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_cell_shape_click);
    schedule.run(&mut world);
    {
        let sel = world.resource::<crate::state::CellSelection>();
        assert!(!sel.contains(3, 4), "Alt 点击应把命中格扣出选区");
        assert!(sel.contains(0, 0), "Alt 点击不动其余选区");
        assert_eq!(sel.len(), 1);
    }
}

/// 右键已完全不绑定（追记四废除 Ctrl+右键）；Alt+左键点在图外
/// 不动选区（显式指令不误清）。
#[test]
fn right_click_and_alt_click_off_map_do_nothing() {
    let mut world = cellmap_world(Vec2::new(35.0, 45.0), true);
    let map = wv_core::Id::new_unchecked("world_main");
    world.insert_resource(crate::state::PointSelectState { tool_active: true });
    let mut sel = crate::state::CellSelection::default();
    sel.bind_map(&map, 8, 8);
    sel.replace([(0, 0)]);
    world.insert_resource(sel);
    world.insert_resource(ButtonInput::<KeyCode>::default());
    // 裸右键：不归本系统。
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Right);
    world.insert_resource(buttons);
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);
    let sel = world.resource::<crate::state::CellSelection>();
    assert!(sel.contains(0, 0), "右键不动选区");

    // Alt+左键但光标在图外（-50,-50）：不动选区（不误清）。
    world.resource_mut::<crate::state::ViewState>().cursor_world = Some(Vec2::new(-50.0, -50.0));
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::AltLeft);
    world.insert_resource(keys);
    let mut buttons = ButtonInput::<MouseButton>::default();
    buttons.press(MouseButton::Left);
    world.insert_resource(buttons);
    let mut schedule = Schedule::default();
    schedule.add_systems(commit_point_select_on_click);
    schedule.run(&mut world);
    let sel = world.resource::<crate::state::CellSelection>();
    assert!(sel.contains(0, 0), "Alt+左键点图外不动选区（不误清）");
}
#[test]
fn apply_detached_records_dirty_without_pushing_history() {
    use wv_core::map::{MapLevel, MapNode};

    let node = |id: &str| MapNode {
        id: wv_core::Id::new_unchecked(id),
        name: id.into(),
        level: MapLevel::World,
        ..MapNode::default()
    };
    let mut edit = crate::edit::editing::EditHistory::default();
    let mut world = wv_core::World::default();
    // 普通提交一条（入栈）。
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(wv_edit::AddMapCommand::new(node("m1"), None)));
    edit.history.commit(&mut world, tx).expect("commit");
    let len_before = edit.history.len();
    // 直应用一条（不入栈，只记脏路径）。
    edit.apply_detached(
        &mut world,
        Box::new(wv_edit::AddMapCommand::new(node("m2"), None)),
    )
    .expect("detached");
    assert_eq!(edit.history.len(), len_before, "直应用不得进 Undo 栈");
    assert!(
        world.maps.iter().any(|m| m.id.as_str() == "m2"),
        "世界应已生效"
    );
    assert!(
        edit.dirty_set.contains("maps"),
        "脏路径应被记录，got {:?}",
        edit.dirty_set
    );
}
