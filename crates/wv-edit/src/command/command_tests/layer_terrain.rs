//! 图层显隐/地形涂刷/意图笔刷（世界与栅格夹具在本模块头部）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

// ── UpdateLayerCommand（图层面板写回）────────────────────────────

fn world_with_layers() -> World {
    use wv_core::map::{Layer, LayerKind, MapNode};
    let mut world = World::default();
    world.maps.push(MapNode {
        id: test_map(),
        layers: vec![Layer {
            id: Id::new_unchecked("layer_roads"),
            name: "官道".into(),
            kind: LayerKind::Road,
            visible: true,
            locked: false,
            opacity: 0.65,
            z: 4,
            source: None,
        }],
        ..MapNode::default()
    });
    world
}

fn roads_layer(world: &World) -> &wv_core::map::Layer {
    &world.maps[0].layers[0]
}

#[test]
fn update_layer_visibility_round_trips_and_reports_index_dirty() {
    let mut world = world_with_layers();
    let cmd = UpdateLayerCommand::visibility(test_map(), Id::new_unchecked("layer_roads"), false);

    let dirty = cmd.apply(&mut world).expect("apply");
    assert_eq!(
        dirty,
        vec![format!("maps/{TEST_MAP}/index.yaml")],
        "layers 声明内联在图索引，脏路径必须是 index.yaml"
    );
    assert!(!roads_layer(&world).visible, "眼睛关闭应写进声明层");
    assert!(
        (roads_layer(&world).opacity - 0.65).abs() < 1e-6,
        "未提交字段不动，got {}",
        roads_layer(&world).opacity
    );

    cmd.revert(&mut world).expect("revert");
    assert!(roads_layer(&world).visible, "revert 应还原可见性");
    assert!(
        (roads_layer(&world).opacity - 0.65).abs() < 1e-6,
        "revert 不应牵连其它字段，got {}",
        roads_layer(&world).opacity
    );
}

#[test]
fn update_layer_opacity_clamps_to_unit_range() {
    let mut world = world_with_layers();
    let cmd = UpdateLayerCommand::opacity(test_map(), Id::new_unchecked("layer_roads"), 1.7);
    cmd.apply(&mut world).expect("apply");
    assert!(
        (roads_layer(&world).opacity - 1.0).abs() < 1e-6,
        "越界不透明度应 clamp 到 1.0，got {}",
        roads_layer(&world).opacity
    );
}

#[test]
fn update_layer_missing_targets_report_errors() {
    let mut world = world_with_layers();

    let wrong_layer =
        UpdateLayerCommand::visibility(test_map(), Id::new_unchecked("layer_nope"), false);
    assert!(
        matches!(
            wrong_layer.apply(&mut world),
            Err(EditError::TargetMissing(_))
        ),
        "图层不存在应报 TargetMissing"
    );

    let wrong_map = UpdateLayerCommand::locked(
        Id::new_unchecked("map_nope"),
        Id::new_unchecked("layer_roads"),
        true,
    );
    assert!(
        matches!(
            wrong_map.apply(&mut world),
            Err(EditError::TargetMissing(_))
        ),
        "地图不存在应报 TargetMissing"
    );

    // 未 apply 先 revert → Precondition（before 还是空）。
    let fresh = UpdateLayerCommand::locked(test_map(), Id::new_unchecked("layer_roads"), true);
    assert!(
        matches!(fresh.revert(&mut world), Err(EditError::Precondition(_))),
        "未 apply 的命令不可 revert"
    );
}

// ── UpdateLayerCommand::ensure_terrain_visibility（ADR-0079 T4：
//    自然图开关声明化——图无 terrain 声明层时插入，有则改 visible）──

#[test]
fn ensure_terrain_visibility_inserts_declaration_when_absent() {
    let mut world = world_with_layers(); // 只有 roads 层，无 terrain 声明
    let cmd = EnsureTerrainLayerCommand::new(test_map(), false);
    let dirty = cmd.apply(&mut world).expect("apply");
    assert_eq!(
        dirty,
        vec![format!("maps/{TEST_MAP}/index.yaml")],
        "声明层内联图索引，脏路径必须是 index.yaml"
    );
    let node = &world.maps[0];
    let terrain = node
        .layers
        .iter()
        .find(|l| l.kind == wv_core::map::LayerKind::Terrain)
        .expect("应插入 terrain 声明层");
    assert!(!terrain.visible, "插入的 terrain 层应带目标可见性 false");
    cmd.revert(&mut world).expect("revert");
    let node = &world.maps[0];
    assert!(
        node.layers
            .iter()
            .all(|l| l.kind != wv_core::map::LayerKind::Terrain),
        "revert 应移除插入的 terrain 声明层"
    );
}

#[test]
fn ensure_terrain_visibility_updates_existing_row() {
    let mut world = world_with_layers();
    // 先造一条 terrain 声明（visible=true）。
    world.maps[0].layers.push(wv_core::map::Layer {
        id: Id::new_unchecked("layer_terrain"),
        name: "自然图".into(),
        kind: wv_core::map::LayerKind::Terrain,
        visible: true,
        locked: false,
        opacity: 1.0,
        z: 0,
        source: None,
    });
    let cmd = EnsureTerrainLayerCommand::new(test_map(), false);
    cmd.apply(&mut world).expect("apply");
    let terrain = world.maps[0]
        .layers
        .iter()
        .find(|l| l.kind == wv_core::map::LayerKind::Terrain)
        .expect("既有 terrain 层应被复用而非新增");
    assert_eq!(terrain.id, Id::new_unchecked("layer_terrain"), "应改既有行");
    assert!(!terrain.visible, "可见性应更新为 false");
    let node = &world.maps[0];
    assert_eq!(
        node.layers
            .iter()
            .filter(|l| l.kind == wv_core::map::LayerKind::Terrain)
            .count(),
        1,
        "不得插入第二条 terrain 声明"
    );
    cmd.revert(&mut world).expect("revert");
    let terrain = world.maps[0]
        .layers
        .iter()
        .find(|l| l.kind == wv_core::map::LayerKind::Terrain)
        .expect("既有行 revert 后仍在");
    assert!(terrain.visible, "revert 应还原既有行可见性");
}

#[test]
fn paint_command_writes_surface_and_biome_then_revert_restores() {
    let mut world = world_with_terrain(32);
    let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

    let mut stroke = Stroke::new();
    stroke.push(wv_terrain::edit::Brush::paint(
        (10.5, 10.5),
        5.0,
        wv_terrain::edit::PaintTarget {
            surface: wv_terrain::SURFACE_LAND,
            biome: wv_terrain::BIOME_GRASSLAND,
        },
    ));
    let cmd = PaintTerrainCommand::new(test_map(), stroke, 1.0);

    let dirty = cmd.apply(&mut world).unwrap();
    assert!(!dirty.is_empty(), "应报告脏文件");
    let after = world.terrain_of(&test_map()).cloned().unwrap();
    assert_ne!(before_snapshot, after, "apply 应改变地形");
    // 中心格地表/群系被写为目标值
    assert_eq!(after.cells[10 * 32 + 10].biome, wv_terrain::BIOME_GRASSLAND);
    assert_eq!(after.cells[10 * 32 + 10].surface, wv_terrain::SURFACE_LAND);

    cmd.revert(&mut world).unwrap();
    assert_eq!(
        world.terrain_of(&test_map()).cloned().unwrap(),
        before_snapshot,
        "revert 应完全还原"
    );
}

/// 选区蒙版（ADR-0086，2026-10-28 实机反馈「多选后笔刷不该到处生效」）：
/// 带蒙版的涂抹只落蒙版内的格——软边笔刷扫过的选区外格**一格不动**，
/// `before` 只记蒙版内的真实改动，revert 完整还原。
///
/// 手算口径：无蒙版时半径 2 笔刷（世界单位，cell_size=1）在 (4.5,4.5)
/// 覆盖 (4,4) 周围约 5×5 ——先断言无蒙版改动 > 1 格（证明蒙版外确有
/// 笔触落点，下面的「只有一格变」才有牙齿），再断言带蒙版后恰 (4,4) 一格变。
#[test]
fn masked_paint_only_changes_cells_inside_the_mask() {
    let paint_stroke = || {
        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint(
            (4.5, 4.5),
            2.0,
            wv_terrain::edit::PaintTarget {
                surface: wv_terrain::SURFACE_LAND,
                biome: wv_terrain::BIOME_GRASSLAND,
            },
        ));
        stroke
    };
    let changed_cells = |world: &World| -> Vec<(i32, i32)> {
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        (0usize..64)
            .filter(|&idx| after.cells[idx] != wv_core::terrain::TerrainCellData::default())
            .map(|idx| ((idx % 8) as i32, (idx / 8) as i32))
            .collect()
    };

    // 对照组：无蒙版 → 笔刷 footprint 内全部落色（必须 > 1 格）。
    let mut unmasked = world_with_terrain(8);
    PaintTerrainCommand::new(test_map(), paint_stroke(), 1.0)
        .apply(&mut unmasked)
        .unwrap();
    let footprint = changed_cells(&unmasked);
    assert!(
        footprint.len() > 1,
        "半径 2 笔刷应覆盖多格，实际只动了 {:?}——蒙版断言失去前提",
        footprint
    );

    // 蒙版组：只放 (4,4) 一格 → apply 后恰这一格变。
    let mut world = world_with_terrain(8);
    let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();
    let cmd = PaintTerrainCommand::new_masked(test_map(), paint_stroke(), 1.0, vec![(4, 4)]);
    let dirty = cmd.apply(&mut world).unwrap();
    assert_eq!(dirty, vec![format!("maps/{TEST_MAP}/terrain.bin")]);
    assert_eq!(
        changed_cells(&world),
        vec![(4, 4)],
        "蒙版外不得有任何格被改动"
    );
    assert_eq!(
        world.terrain_of(&test_map()).unwrap().cells[4 * 8 + 4].biome,
        wv_terrain::BIOME_GRASSLAND,
        "蒙版内格应写入目标群系"
    );

    // 蒙版不影响可撤销性：revert 只回滚蒙版内的真实改动。
    cmd.revert(&mut world).unwrap();
    assert_eq!(
        world.terrain_of(&test_map()).cloned().unwrap(),
        before_snapshot,
        "revert 应完全还原"
    );
}

/// 域对齐不回退（设计稿 D19/Review Focus 3）：画水把高度推过海平面下方。
#[test]
fn water_target_pushes_height_below_sea_level() {
    let mut world = world_with_terrain(32);
    // 先铺 +200 m 陆地。
    let land: Vec<TerrainCellData> = vec![
        TerrainCellData {
            height: 200,
            surface: 1,
            biome: 0,
        };
        32 * 32
    ];
    world.set_terrain(
        test_map(),
        TerrainData {
            width: 32,
            height: 32,
            cells: land,
        },
    );

    let mut stroke = Stroke::new();
    stroke.push(wv_terrain::edit::Brush::paint(
        (10.5, 10.5),
        5.0,
        wv_terrain::edit::PaintTarget {
            surface: wv_terrain::SURFACE_WATER,
            biome: wv_terrain::BIOME_NONE,
        },
    ));
    PaintTerrainCommand::new(test_map(), stroke, 1.0)
        .apply(&mut world)
        .unwrap();
    let after = world.terrain_of(&test_map()).cloned().unwrap();
    let c = &after.cells[10 * 32 + 10];
    assert_eq!(c.surface, wv_terrain::SURFACE_WATER);
    assert!(c.height < 0, "画水应推到海平面下，得 {}", c.height);
}

/// 域对齐对称侧：画陆把海底格抬到海平面上方。
#[test]
fn land_target_pushes_height_above_sea_level() {
    let mut world = world_with_terrain(32);
    let sea: Vec<TerrainCellData> = vec![
        TerrainCellData {
            height: -200,
            surface: 0,
            biome: 0,
        };
        32 * 32
    ];
    world.set_terrain(
        test_map(),
        TerrainData {
            width: 32,
            height: 32,
            cells: sea,
        },
    );

    let mut stroke = Stroke::new();
    stroke.push(wv_terrain::edit::Brush::paint(
        (10.5, 10.5),
        5.0,
        wv_terrain::edit::PaintTarget {
            surface: wv_terrain::SURFACE_LAND,
            biome: wv_terrain::BIOME_GRASSLAND,
        },
    ));
    PaintTerrainCommand::new(test_map(), stroke, 1.0)
        .apply(&mut world)
        .unwrap();
    let after = world.terrain_of(&test_map()).cloned().unwrap();
    let c = &after.cells[10 * 32 + 10];
    assert_eq!(c.surface, wv_terrain::SURFACE_LAND);
    assert!(c.height >= 0, "画陆应抬到海平面上，得 {}", c.height);
}

#[test]
fn painting_one_map_leaves_the_other_maps_grid_untouched() {
    // Phase 2 的关键不变量：栅格按图存放，刷 A 城不能碰到 B 城。
    let mut world = world_with_terrain(32);
    let other = Id::new_unchecked("map_city_other");
    world.set_terrain(other.clone(), from_grid(TerrainGrid::new(16, 16)));
    let other_before = world.terrain_of(&other).cloned().unwrap();

    let mut stroke = Stroke::new();
    stroke.push(wv_terrain::edit::Brush::paint(
        (10.5, 10.5),
        5.0,
        wv_terrain::edit::PaintTarget {
            surface: wv_terrain::SURFACE_LAND,
            biome: wv_terrain::BIOME_GRASSLAND,
        },
    ));
    let cmd = PaintTerrainCommand::new(test_map(), stroke, 1.0);
    cmd.apply(&mut world).unwrap();

    assert_eq!(
        world.terrain_of(&other).cloned().unwrap(),
        other_before,
        "另一张图的栅格必须一个格都没动"
    );
    assert_eq!(world.terrains.len(), 2, "不应凭空多出或丢掉栅格");
}

#[test]
fn paint_command_without_terrain_reports_mismatch() {
    let mut world = World::default();
    let mut stroke = Stroke::new();
    stroke.push(wv_terrain::edit::Brush::paint(
        (0.0, 0.0),
        1.0,
        wv_terrain::edit::PaintTarget {
            surface: wv_terrain::SURFACE_WATER,
            biome: wv_terrain::BIOME_NONE,
        },
    ));
    let cmd = PaintTerrainCommand::new(Id::new_unchecked("world_main"), stroke, 1.0);
    assert!(matches!(
        cmd.apply(&mut world),
        Err(EditError::TerrainMismatch(_))
    ));
}

// ---- 区域意图命令（2026-10-17 选择工具 M3：复用 IntentTerrainCommand） ----

/// 区域意图经同一条命令路径：apply 改地形、revert 逐位复原（零新命令面）。
#[test]
fn region_intent_command_undo_restores_bitwise() {
    let mut world = world_with_terrain(48);
    let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

    // 用 MountainMassif（fixture 是海面，平原只在陆上生效会 0 改动——
    // 压平带断言由 wv-terrain 的 plains 测试背书）。
    let cmd = IntentTerrainCommand::new(
        test_map(),
        Intent::MountainMassif {
            polygon: vec![(12.0, 12.0), (36.0, 12.0), (36.0, 36.0), (12.0, 36.0)],
        },
        1.0,
    );
    let dirty = cmd.apply(&mut world).unwrap();
    assert_eq!(dirty, vec!["maps/world_main/terrain.bin".to_string()]);
    let after = world.terrain_of(&test_map()).cloned().unwrap();
    assert_ne!(before_snapshot, after, "apply 应在掩膜内隆起山体");

    cmd.revert(&mut world).unwrap();
    assert_eq!(
        world.terrain_of(&test_map()).cloned().unwrap(),
        before_snapshot,
        "revert 后网格逐位复原"
    );
}

/// 同一多边形两次构造 → 同 seed（spec：确定性，重放不漂移）。
#[test]
fn region_intent_seed_is_deterministic_for_same_polygon() {
    let polygon = vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)];
    let a = IntentTerrainCommand::new(
        test_map(),
        Intent::MountainMassif {
            polygon: polygon.clone(),
        },
        1.0,
    );
    let b = IntentTerrainCommand::new(test_map(), Intent::MountainMassif { polygon }, 1.0);
    assert_eq!(a.seed, b.seed, "同环同图 seed 必须一致");
    let c = IntentTerrainCommand::new(
        test_map(),
        Intent::LakeRegion {
            polygon: vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)],
        },
        1.0,
    );
    assert_ne!(a.seed, c.seed, "不同意图 seed 必须不同");
}

// ---- 意图笔刷命令（意图笔刷 plan v0.1） ----

#[test]
fn intent_command_apply_then_revert_restores_grid() {
    let mut world = world_with_terrain(48);
    let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

    let cmd = IntentTerrainCommand::new(
        test_map(),
        Intent::MountainRange {
            pts: vec![(10.0, 24.0), (38.0, 24.0)],
            radius: 5.0,
            strength: 1.0,
        },
        1.0,
    );
    let dirty = cmd.apply(&mut world).unwrap();
    assert_eq!(dirty, vec!["maps/world_main/terrain.bin".to_string()]);
    let after = world.terrain_of(&test_map()).cloned().unwrap();
    assert_ne!(before_snapshot, after, "apply 应改变地形");
    // 中段脊线确实隆起（高度远超 0）。
    assert!(
        after.cells[24 * 48 + 24].height > 500,
        "脊线中段应显著隆起，实得 {}",
        after.cells[24 * 48 + 24].height
    );

    cmd.revert(&mut world).unwrap();
    assert_eq!(
        world.terrain_of(&test_map()).cloned().unwrap(),
        before_snapshot,
        "revert 应完全还原"
    );
}

#[test]
fn intent_command_redo_replays_identical_grid() {
    // undo 后 redo：revert 已把 before 写回，re-apply 用同一 seed，
    // 结果必须与首次 apply 逐格一致（确定性契约）。
    let mut world = world_with_terrain(48);
    let cmd = IntentTerrainCommand::new(
        test_map(),
        Intent::Volcano {
            center: (24.0, 24.0),
            radius: 8.0,
            strength: 1.0,
        },
        1.0,
    );
    cmd.apply(&mut world).unwrap();
    let first = world.terrain_of(&test_map()).cloned().unwrap();
    cmd.revert(&mut world).unwrap();
    cmd.apply(&mut world).unwrap();
    let second = world.terrain_of(&test_map()).cloned().unwrap();
    assert_eq!(first.cells, second.cells, "redo 应逐格重现首次结果");
}

#[test]
fn intent_command_without_terrain_reports_mismatch() {
    let mut world = World::default();
    let cmd = IntentTerrainCommand::new(
        Id::new_unchecked("world_main"),
        Intent::Hills {
            pts: vec![(5.0, 5.0)],
            radius: 4.0,
            strength: 1.0,
        },
        1.0,
    );
    assert!(matches!(
        cmd.apply(&mut world),
        Err(EditError::TerrainMismatch(_))
    ));
}

// ---- 势力命令（S12 v0.2） ----

// ── SetLayerZCommand / MoveLayerCommand（UX-007 图层重排）────────────────

/// 三层 fixture（layer_a/b/c，声明序即 Vec 序）。
fn world_three_layers() -> World {
    use wv_core::map::{Layer, LayerKind, MapNode};
    let mut world = World::default();
    let mk = |name: &str| Layer {
        id: Id::new_unchecked(name),
        name: name.into(),
        kind: LayerKind::Road,
        visible: true,
        locked: false,
        opacity: 1.0,
        z: 0,
        source: None,
    };
    world.maps.push(MapNode {
        id: test_map(),
        layers: vec![mk("layer_a"), mk("layer_b"), mk("layer_c")],
        ..MapNode::default()
    });
    world
}

/// z 写回与撤销：apply 改 z、revert 还原旧值。
#[test]
fn set_layer_z_round_trips() {
    let mut world = world_with_layers();
    assert_eq!(roads_layer(&world).z, 4);
    let cmd = SetLayerZCommand::new(test_map(), Id::new_unchecked("layer_roads"), 2);
    cmd.apply(&mut world).expect("apply 应成功");
    assert_eq!(roads_layer(&world).z, 2);
    cmd.revert(&mut world).expect("revert 应成功");
    assert_eq!(roads_layer(&world).z, 4, "撤销应还原旧 z");
}

/// 声明序重排：下移/上移对合、revert（反向再换）还原、首尾越界报错不改序。
#[test]
fn move_layer_reorders_and_round_trips() {
    let mut world = world_three_layers();
    let order = |world: &World| -> Vec<String> {
        world.maps[0]
            .layers
            .iter()
            .map(|l| l.id.as_str().to_owned())
            .collect()
    };
    assert_eq!(order(&world), vec!["layer_a", "layer_b", "layer_c"]);

    // 下移首层一格。
    MoveLayerCommand::new(test_map(), Id::new_unchecked("layer_a"), false)
        .apply(&mut world)
        .expect("下移应成功");
    assert_eq!(
        order(&world),
        vec!["layer_b", "layer_a", "layer_c"],
        "首层下移后应与 b 交换"
    );

    // revert（反向再换）还原。
    MoveLayerCommand::new(test_map(), Id::new_unchecked("layer_a"), false)
        .revert(&mut world)
        .expect("revert 应成功");
    assert_eq!(order(&world), vec!["layer_a", "layer_b", "layer_c"]);

    // 首层上移 / 末层下移报 Precondition，且不改序。
    assert!(
        MoveLayerCommand::new(test_map(), Id::new_unchecked("layer_a"), true)
            .apply(&mut world)
            .is_err()
    );
    assert!(
        MoveLayerCommand::new(test_map(), Id::new_unchecked("layer_c"), false)
            .apply(&mut world)
            .is_err()
    );
    assert_eq!(order(&world), vec!["layer_a", "layer_b", "layer_c"]);
}
