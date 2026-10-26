//! 编辑命令族单测（自 command.rs 拆分）。
//!
//! 各测试模块经 `use crate::command::*` 取契约与命令类型——拆分前为 `use super::*`
//!（= command 模块）；改用绝对路径以适配子模块层级。

#[cfg(test)]
mod batch_edit_cells_tests {
    use crate::command::*;
    use wv_terrain::SURFACE_LAND;

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(n: u32) -> World {
        let cells = vec![
            wv_core::terrain::TerrainCellData {
                height: 0,
                surface: 0,
                biome: 0
            };
            (n * n) as usize
        ];
        let mut world = World::default();
        world.set_terrain(
            test_map(),
            wv_core::terrain::TerrainData {
                width: n,
                height: n,
                cells,
            },
        );
        world
    }

    /// 批量 apply 写 N 格 / revert 逐格还原；1 个撤销单位。
    #[test]
    fn batch_edit_cells_applies_and_reverts_all_changes() {
        let mut world = world_with_terrain(8);
        let cell_at =
            |t: &wv_core::terrain::TerrainData, gx: i32, gy: i32| t.cells[(gy * 8 + gx) as usize];
        let t0 = world.terrain_of(&test_map()).unwrap().clone();
        let changes = vec![
            CellChange {
                gx: 1,
                gy: 1,
                before: cell_at(&t0, 1, 1),
                after: wv_core::terrain::TerrainCellData {
                    height: 100,
                    surface: SURFACE_LAND,
                    biome: 3,
                },
            },
            CellChange {
                gx: 2,
                gy: 1,
                before: cell_at(&t0, 2, 1),
                after: wv_core::terrain::TerrainCellData {
                    height: 200,
                    surface: 0,
                    biome: 14,
                },
            },
        ];
        let cmd = BatchEditCellsCommand::new(test_map(), changes);
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec![format!("maps/{TEST_MAP}/terrain.bin")]);
        let t1 = world.terrain_of(&test_map()).unwrap().clone();
        assert_eq!(cell_at(&t1, 1, 1).height, 100);
        assert_eq!(cell_at(&t1, 2, 1).height, 200);
        assert_eq!(cell_at(&t1, 3, 1), cell_at(&t0, 3, 1), "未选格不动");

        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.terrain_of(&test_map()).unwrap().cells,
            t0.cells,
            "revert 后全图逐格还原"
        );
    }

    /// 任一格越界 → 整批拒收（不动 world）。
    #[test]
    fn batch_edit_cells_rejects_on_any_out_of_bounds() {
        let mut world = world_with_terrain(4);
        let cell = wv_core::terrain::TerrainCellData {
            height: 0,
            surface: 0,
            biome: 0,
        };
        let cmd = BatchEditCellsCommand::new(
            test_map(),
            vec![
                CellChange {
                    gx: 0,
                    gy: 0,
                    before: cell,
                    after: cell,
                },
                CellChange {
                    gx: 99,
                    gy: 0,
                    before: cell,
                    after: cell,
                },
            ],
        );
        let err = cmd.apply(&mut world).expect_err("越界必须整批拒收");
        assert!(matches!(err, EditError::Precondition(_)));
    }
}

#[cfg(test)]
mod edit_cell_tests {
    use crate::command::*;
    use wv_terrain::{SURFACE_LAND, SURFACE_WATER};

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(n: u32, surface: u8, height: i16, biome: u8) -> World {
        let cells = vec![
            TerrainCellData {
                height,
                surface,
                biome
            };
            (n as usize) * (n as usize)
        ];
        let mut world = World::default();
        world.set_terrain(
            test_map(),
            wv_core::terrain::TerrainData {
                width: n,
                height: n,
                cells,
            },
        );
        world
    }

    /// 基础 round-trip:apply 改一格,revert 还原。验收 [ADR-0040] §3 命令
    /// 沿"snapshot → 改 → revert"模式。
    #[test]
    fn edit_cell_command_round_trips_one_cell() {
        let mut world = world_with_terrain(8, SURFACE_LAND, 100, 0);
        let before = world.terrain_of(&test_map()).unwrap().cells[3 * 8 + 5];
        assert_eq!(before.surface, SURFACE_LAND);
        let after = TerrainCellData {
            height: 250,
            surface: SURFACE_WATER,
            biome: 5, // BIOME_DESERT
        };
        let cmd = EditCellCommand::new(test_map(), 5, 3, before, after);
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec![format!("maps/{TEST_MAP}/terrain.bin")]);
        // 目标格被改了
        let cur = world.terrain_of(&test_map()).unwrap().cells[3 * 8 + 5];
        assert_eq!(cur, after, "apply 后目标格必须等于 after");
        // 其它格不变
        assert_eq!(world.terrain_of(&test_map()).unwrap().cells[0], before);
        assert_eq!(world.terrain_of(&test_map()).unwrap().cells[63], before);
        // 宽度/高度不动
        let t = world.terrain_of(&test_map()).unwrap();
        assert_eq!(t.width, 8);
        assert_eq!(t.height, 8);

        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.terrain_of(&test_map()).unwrap().cells[3 * 8 + 5],
            before,
            "revert 后必须逐格还原"
        );
    }

    /// 起值不匹配时 apply 拒收,不动 world。
    #[test]
    fn edit_cell_command_rejects_stale_snapshot() {
        let mut world = world_with_terrain(8, SURFACE_LAND, 100, 0);
        let wrong_before = TerrainCellData {
            height: 999,
            surface: SURFACE_WATER,
            biome: 99,
        };
        let after = TerrainCellData {
            height: 0,
            surface: SURFACE_LAND,
            biome: 0,
        };
        let cmd = EditCellCommand::new(test_map(), 0, 0, wrong_before, after);
        let err = cmd.apply(&mut world).expect_err("stale before must fail");
        assert!(matches!(err, EditError::Precondition(_)), "got {err:?}");
        // world 不变
        assert_eq!(
            world.terrain_of(&test_map()).unwrap().cells[0].height,
            100,
            "拒收后格不被改动"
        );
    }

    /// 坐标超界 → 拒收。
    #[test]
    fn edit_cell_command_rejects_out_of_bounds() {
        let mut world = world_with_terrain(4, SURFACE_LAND, 0, 0);
        let cell = TerrainCellData {
            height: 0,
            surface: 0,
            biome: 0,
        };
        let cmd = EditCellCommand::new(test_map(), 99, 99, cell, cell);
        let err = cmd.apply(&mut world).expect_err("oob must fail");
        assert!(matches!(err, EditError::Precondition(_)));
    }

    /// 缺图 → 返 TerrainMismatch,而非 panic。
    #[test]
    fn edit_cell_command_missing_map_returns_terrain_mismatch() {
        let mut world = World::default();
        let cell = TerrainCellData {
            height: 0,
            surface: 0,
            biome: 0,
        };
        let cmd = EditCellCommand::new(test_map(), 0, 0, cell, cell);
        let err = cmd.apply(&mut world).expect_err("missing must fail");
        assert!(matches!(err, EditError::TerrainMismatch(_)), "got {err:?}");
    }
}

#[cfg(test)]
mod tests {
    use crate::command::*;
    use wv_core::map::GeometryData;
    use wv_core::time::TimeSpan;
    use wv_core::Id;

    /// 测试用的图 ID——`PaintTerrainCommand` 的栅格是**按图**查的。
    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(size: u32) -> World {
        let grid = TerrainGrid::new(size, size);
        let data = from_grid(grid);
        let mut world = World::default();
        world.set_terrain(test_map(), data);
        world
    }

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
        let cmd =
            UpdateLayerCommand::visibility(test_map(), Id::new_unchecked("layer_roads"), false);

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

    fn f(id: &str, name: &str) -> Faction {
        Faction::new(
            Id::new_unchecked(id),
            name,
            wv_core::color::Color("#112233".into()),
        )
    }

    fn geo(map: &str, owner: &str, from: i64, to: Option<i64>) -> GeometrySpan {
        GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: TimeSpan { from, to },
            geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
            relation: None,
        }
    }

    fn layer(id: &str) -> FactionLayer {
        FactionLayer {
            id: Id::new_unchecked(id),
            name: id.into(),
            z: 0,
            color_mode: FactionColorMode::Own,
            visible: true,
        }
    }

    fn mem(target: &str, from: i64) -> Membership {
        Membership {
            target: Id::new_unchecked(target),
            kind: Some("实控".into()),
            span: TimeSpan::open(from),
        }
    }

    // ---- 势力划分（ADR-0028 / 势力交互 M1 Task 5） ----

    fn square_ring(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<f32> {
        vec![x0, y0, x1, y0, x1, y1, x0, y1]
    }

    /// 一张带方形宿主势力 `faction_host` 的世界（地图 = TEST_MAP，格 1 wu）。
    fn world_with_host_faction() -> World {
        let mut w = world_with_terrain(64);
        w.factions
            .push(f(Id::new_unchecked("faction_host").as_str(), "宿主国"));
        w.factions.last_mut().expect("刚推入").id = Id::new_unchecked("faction_host");
        w.faction_geometry.push(GeometrySpan {
            map: test_map(),
            owner: Id::new_unchecked("faction_host"),
            span: TimeSpan { from: 0, to: None },
            geometry: GeometryData::Polygon(square_ring(4.0, 4.0, 60.0, 60.0)),
            relation: None,
        });
        w
    }

    /// 宿主疆界的外环（武: 带洞时取 outer），wu。
    fn faction_ring(w: &World, owner: &str) -> Vec<(f32, f32)> {
        let g = w
            .faction_geometry
            .iter()
            .find(|g| g.owner.as_str() == owner && g.relation.is_none())
            .expect("宿主疆界应存在");
        let flat = match &g.geometry {
            GeometryData::Polygon(v) => v.clone(),
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            other => panic!("意外的几何形态：{other:?}"),
        };
        flat.chunks_exact(2).map(|c| (c[0], c[1])).collect()
    }

    /// 连续两次划分：第二次没碰到的宿主外沿必须**逐点**不变（M2 切弧拼接），
    /// 且两次 undo 逐位回到划分前。
    #[test]
    fn two_divisions_keep_untouched_host_edges_pointwise_and_undo() {
        let mut w = world_with_host_faction();
        let host = Id::new_unchecked("faction_host");
        let first = DivideFactionCommand::new(
            test_map(),
            Some(host.clone()),
            vec![(4.0, 4.0), (24.0, 4.0), (24.0, 24.0), (4.0, 24.0)],
            DivisionMode::Straight,
            f("faction_a", "A 国"),
            0,
            1.0,
        );
        first.apply(&mut w).expect("第一次划分");
        let rem1 = faction_ring(&w, "faction_host");
        assert!(rem1.len() >= 4, "余部应是环，实得 {rem1:?}");

        let second = DivideFactionCommand::new(
            test_map(),
            Some(host.clone()),
            // 完全在宿主内部的切块（不碰任何外沿）——这样外沿的「未触碰」语义
            // 才没有歧义。
            vec![(30.0, 30.0), (50.0, 30.0), (50.0, 50.0), (30.0, 50.0)],
            DivisionMode::Straight,
            f("faction_b", "B 国"),
            0,
            1.0,
        );
        second.apply(&mut w).expect("第二次划分");
        let rem2 = faction_ring(&w, "faction_host");

        // 「未触碰的边」= rem1 中两端都落在宿主原外沿线上的边段。
        let on_outer = |p: (f32, f32)| {
            (p.0 - 4.0).abs() < 1e-4
                || (p.1 - 4.0).abs() < 1e-4
                || (p.0 - 60.0).abs() < 1e-4
                || (p.1 - 60.0).abs() < 1e-4
        };
        let n = rem1.len();
        let mut checked = 0usize;
        for i in 0..n {
            let pair = [rem1[i], rem1[(i + 1) % n]];
            if !(on_outer(pair[0]) && on_outer(pair[1])) {
                continue;
            }
            checked += 1;
            assert!(
                rem2.windows(2)
                    .any(|w| w == pair || (w[0] == pair[1] && w[1] == pair[0])),
                "未触碰的宿主外沿边段 {pair:?} 必须在第二次划分后逐点不变；rem2={rem2:?}"
            );
        }
        assert!(checked >= 4, "应有足够多的未触碰边段，实得 {checked}");

        // undo 第二次：宿主疆界必须逐点回到 rem1。
        second.revert(&mut w).expect("撤销第二次划分");
        assert_eq!(
            faction_ring(&w, "faction_host"),
            rem1,
            "撤销第二次划分后宿主疆界必须与第一次的余部逐点一致"
        );
        // undo 第一次：回到原始方形。
        first.revert(&mut w).expect("撤销第一次划分");
        let back = faction_ring(&w, "faction_host");
        assert_eq!(
            back,
            vec![(4.0, 4.0), (60.0, 4.0), (60.0, 60.0), (4.0, 60.0)],
            "撤销第一次划分后宿主应回到原方形，实得 {back:?}"
        );
        assert!(w.factions.iter().all(|f| f.id.as_str() == "faction_host"));
    }

    #[test]
    fn divide_creates_faction_and_shrinks_host_atomically() {
        let mut w = world_with_host_faction();
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        let dirty = cmd.apply(&mut w).expect("划分应成功");
        assert!(dirty.contains(&"factions".to_string()));
        assert_eq!(w.factions.len(), 2, "新势力应入场");
        assert_eq!(w.faction_geometry.len(), 2, "新势力疆界应入场");
        // 宿主余部变成带洞环：1 个洞。
        let host = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_host"))
            .expect("宿主疆界仍在");
        let GeometryData::PolygonWithHoles { holes, .. } = &host.geometry else {
            panic!("宿主余部应为带洞环，实得 {:?}", host.geometry);
        };
        assert_eq!(holes.len(), 1, "宿主余部应有 1 个洞");
        // 共边（D3）：新势力外环 == 宿主洞沿反序（逐点相等）。
        let new_geo = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_new"))
            .expect("新势力疆界");
        let GeometryData::Polygon(new_outer) = &new_geo.geometry else {
            panic!("新势力应为单环，实得 {:?}", new_geo.geometry);
        };
        let mut reversed = new_outer.clone();
        let n = reversed.len() / 2;
        let mut pts: Vec<[f32; 2]> = reversed.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
        pts.reverse();
        reversed = pts.into_iter().flatten().collect();
        assert_eq!(&holes[0], &reversed, "共边必须逐点相等（D3）");
        assert_eq!(n * 2, new_outer.len(), "顶点数自洽");
    }

    #[test]
    fn divide_at_real_cell_size_keeps_remainder_inside_host() {
        // C1 回归：宿主几何是 wu，内核吃格坐标——cell_size=8 时宿主必须
        // 先除再进内核。宿主 80..480 wu（格 10..60），意图 160..320 wu（格 20..40）。
        let mut w = world_with_land_terrain();
        let cell = 8.0;
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            };
            64 * 64
        ];
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells: land,
            },
        );
        w.factions
            .push(f(Id::new_unchecked("faction_host").as_str(), "宿主国"));
        w.factions.last_mut().expect("刚推入").id = Id::new_unchecked("faction_host");
        w.faction_geometry.push(GeometrySpan {
            map: test_map(),
            owner: Id::new_unchecked("faction_host"),
            span: TimeSpan { from: 0, to: None },
            geometry: GeometryData::Polygon(square_ring(80.0, 80.0, 480.0, 480.0)),
            relation: None,
        });
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![
                (160.0, 160.0),
                (320.0, 160.0),
                (320.0, 320.0),
                (160.0, 320.0),
            ],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            cell,
        );
        cmd.apply(&mut w).expect("cell_size=8 的划分应成功");

        let host = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_host"))
            .expect("宿主疆界仍在");
        let bbox = |flat: &[f32]| -> (f32, f32, f32, f32) {
            let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for p in flat.chunks_exact(2) {
                b.0 = b.0.min(p[0]);
                b.1 = b.1.min(p[1]);
                b.2 = b.2.max(p[0]);
                b.3 = b.3.max(p[1]);
            }
            b
        };
        let host_outer = match &host.geometry {
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            GeometryData::Polygon(v) => v.clone(),
            other => panic!("宿主余部应为多边形，实得 {other:?}"),
        };
        let hb = bbox(&host_outer);
        assert!(
            hb.0 >= 72.0 && hb.1 >= 72.0 && hb.2 <= 488.0 && hb.3 <= 488.0,
            "宿主余部必须留在原宿主 80..480 wu 内，实得 bbox {hb:?}"
        );
        let new_geo = w
            .faction_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("faction_new"))
            .expect("新势力疆界");
        let new_outer = match &new_geo.geometry {
            GeometryData::Polygon(v) => v.clone(),
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            other => panic!("新势力应为多边形，实得 {other:?}"),
        };
        let nb = bbox(&new_outer);
        assert!(
            nb.0 >= 152.0 && nb.1 >= 152.0 && nb.2 <= 328.0 && nb.3 <= 328.0,
            "新势力必须落在意图 160..320 wu 附近，实得 bbox {nb:?}"
        );
        // 余部面积应约为宿主减新势力（不是整张图）。
        let area = |flat: &[f32]| -> f32 {
            let n = flat.len() / 2;
            let mut a = 0.0;
            for i in 0..n {
                let (x0, y0) = (flat[2 * i], flat[2 * i + 1]);
                let (x1, y1) = (flat[2 * ((i + 1) % n)], flat[2 * ((i + 1) % n) + 1]);
                a += x0 * y1 - x1 * y0;
            }
            (a / 2.0).abs()
        };
        let host_area = area(&host_outer);
        assert!(
            (150_000.0..=170_000.0).contains(&host_area),
            "宿主余部外环应 ≈400×400=160000 wu²（不是整图），实得 {host_area}"
        );
        let GeometryData::PolygonWithHoles { holes, .. } = &host.geometry else {
            panic!("居中切应产生带洞余部，实得 {:?}", host.geometry);
        };
        let hole_area = area(&holes[0]);
        assert!(
            (24_000.0..=28_000.0).contains(&hole_area),
            "洞应 ≈160×160=25600 wu²，实得 {hole_area}"
        );
    }

    #[test]
    fn divide_undo_restores_host_and_removes_new_faction_bitwise() {
        let mut w = world_with_host_faction();
        let factions_before = w.factions.clone();
        let geometry_before = w.faction_geometry.clone();
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        cmd.apply(&mut w).unwrap();
        cmd.revert(&mut w).unwrap();
        assert_eq!(w.factions, factions_before, "revert 应移除新势力");
        assert_eq!(
            w.faction_geometry, geometry_before,
            "revert 应逐位还原宿主疆界"
        );
    }

    /// 全陆地地形（格 1 wu）：空势力宿主的划分对象。
    fn world_with_land_terrain() -> World {
        let mut w = world_with_terrain(64);
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            };
            64 * 64
        ];
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells: land,
            },
        );
        w
    }

    #[test]
    fn divide_from_null_host_writes_no_host_span() {
        let mut w = world_with_land_terrain();
        let before_count = w.faction_geometry.len();
        let cmd = DivideFactionCommand::new(
            test_map(),
            None,
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        cmd.apply(&mut w).expect("空势力划分应成功");
        assert_eq!(w.factions.len(), 1);
        assert_eq!(w.faction_geometry.len(), before_count + 1, "只加新势力疆界");
    }

    #[test]
    fn null_host_excludes_water_from_the_new_faction() {
        // 意图正中有一片水（湖/海）：新势力应带洞（水域归空势力，D4/Q3）。
        let mut w = world_with_land_terrain();
        let mut cells = w.terrain_of(&test_map()).cloned().unwrap().cells;
        for y in 26..38 {
            for x in 26..38 {
                let c = &mut cells[y * 64 + x];
                c.height = -20;
                c.surface = 0;
                c.biome = 0;
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let cmd = DivideFactionCommand::new(
            test_map(),
            None,
            vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "环湖国"),
            0,
            1.0,
        );
        cmd.apply(&mut w).expect("环湖划分应成功");
        let geo = &w.faction_geometry[0];
        let GeometryData::PolygonWithHoles { holes, .. } = &geo.geometry else {
            panic!("环湖国应带洞，实得 {:?}", geo.geometry);
        };
        assert_eq!(holes.len(), 1, "湖应成为一个洞");
    }

    #[test]
    fn divide_fragmenting_intent_is_precondition_error() {
        let mut w = world_with_host_faction();
        // 宿主改成哑铃形：两间房 + 1 格走廊（横切走廊 → 余部碎块化）。
        w.faction_geometry[0].geometry = GeometryData::Polygon(vec![
            4.0, 4.0, 24.0, 4.0, 24.0, 24.0, 26.0, 24.0, 26.0, 4.0, 46.0, 4.0, 46.0, 44.0, 26.0,
            44.0, 26.0, 24.0, 24.0, 24.0, 24.0, 44.0, 4.0, 44.0,
        ]);
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(22.0, 0.0), (28.0, 0.0), (28.0, 50.0), (22.0, 50.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_new").as_str(), "新国"),
            0,
            1.0,
        );
        let err = cmd.apply(&mut w).expect_err("碎块化必须被拒绝");
        assert!(
            matches!(err, EditError::Precondition(_)),
            "应是 Precondition，实得 {err:?}"
        );
        assert_eq!(w.factions.len(), 1, "失败不得改动世界");
        assert_eq!(w.faction_geometry.len(), 1, "失败不得改动疆界");
    }

    #[test]
    fn divide_duplicate_faction_id_is_precondition_error() {
        let mut w = world_with_host_faction();
        let cmd = DivideFactionCommand::new(
            test_map(),
            Some(Id::new_unchecked("faction_host")),
            vec![(24.0, 24.0), (40.0, 24.0), (40.0, 40.0), (24.0, 40.0)],
            DivisionMode::Straight,
            f(Id::new_unchecked("faction_host").as_str(), "重名"),
            0,
            1.0,
        );
        assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
    }

    #[test]
    fn add_faction_layer_apply_then_revert_removes() {
        let mut w = World::default();
        let cmd = AddFactionLayerCommand::new(layer("layer_admin"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_layers.len(), 1);
        assert!(dirty.contains(&"faction_layers".to_string()));
        cmd.revert(&mut w).unwrap();
        assert!(w.faction_layers.is_empty());
    }

    #[test]
    fn update_faction_layer_round_trip() {
        let mut w = World::default();
        AddFactionLayerCommand::new(layer("layer_admin"))
            .apply(&mut w)
            .unwrap();
        let cmd = UpdateFactionLayerCommand::new(
            Id::new_unchecked("layer_admin"),
            "行政区".into(),
            5,
            FactionColorMode::Inherit,
            false,
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_layers[0].name, "行政区");
        assert_eq!(w.faction_layers[0].z, 5);
        assert_eq!(w.faction_layers[0].color_mode, FactionColorMode::Inherit);
        assert!(!w.faction_layers[0].visible);
        cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_layers[0].name, "layer_admin");
        assert_eq!(w.faction_layers[0].z, 0);
    }

    #[test]
    fn remove_faction_layer_reassigns_and_undo_restores() {
        let mut w = World::default();
        AddFactionLayerCommand::new(layer("layer_admin"))
            .apply(&mut w)
            .unwrap();
        let mut faction = f("f1", "F1");
        faction.layer = Some(Id::new_unchecked("layer_admin"));
        w.factions.push(faction);

        let cmd = RemoveFactionLayerCommand::new(Id::new_unchecked("layer_admin"));
        cmd.apply(&mut w).unwrap();
        assert!(w.faction_layers.is_empty());
        assert_eq!(w.factions[0].layer, None, "删层不得删势力，成员回落默认层");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_layers.len(), 1);
        assert_eq!(
            w.factions[0].layer.as_ref().map(|i| i.as_str()),
            Some("layer_admin")
        );
    }

    #[test]
    fn set_faction_layer_rank_and_seat_round_trip() {
        let mut w = World::default();
        w.factions.push(f("f1", "F1"));

        let set_layer = SetFactionLayerCommand::new(
            Id::new_unchecked("f1"),
            Some(Id::new_unchecked("layer_admin")),
        );
        set_layer.apply(&mut w).unwrap();
        assert_eq!(
            w.factions[0].layer.as_ref().map(|i| i.as_str()),
            Some("layer_admin")
        );
        set_layer.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].layer, None);

        let set_rank = SetFactionRankCommand::new(Id::new_unchecked("f1"), Some("province".into()));
        set_rank.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].rank.as_deref(), Some("province"));
        set_rank.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].rank, None);

        let set_seat =
            SetFactionSeatCommand::new(Id::new_unchecked("f1"), Some(Id::new_unchecked("p1")));
        set_seat.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].seat.as_ref().map(|i| i.as_str()), Some("p1"));
        set_seat.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].seat, None);
    }

    #[test]
    fn add_membership_keeps_same_target_with_different_from() {
        let mut w = World::default();
        w.factions.push(f("f1", "F1"));
        AddMembershipCommand::new(Id::new_unchecked("f1"), mem("f2", 1))
            .apply(&mut w)
            .unwrap();
        AddMembershipCommand::new(Id::new_unchecked("f1"), mem("f2", 2))
            .apply(&mut w)
            .unwrap();
        assert_eq!(
            w.factions[0].memberships.len(),
            2,
            "同一 target 不同 from 是两条独立边，不得合并"
        );
    }

    #[test]
    fn remove_membership_by_target_and_from_round_trip() {
        let mut w = World::default();
        let mut faction = f("f1", "F1");
        faction.memberships.push(mem("f2", 1));
        faction.memberships.push(mem("f2", 2));
        w.factions.push(faction);

        let cmd = RemoveMembershipCommand::new(Id::new_unchecked("f1"), Id::new_unchecked("f2"), 1);
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].memberships.len(), 1);
        assert_eq!(w.factions[0].memberships[0].span.from, 2);
        cmd.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].memberships.len(), 2);
    }

    #[test]
    fn upsert_geometry_keeps_claim_and_control_with_same_from() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let mut control = geo("m", "f1", 1, None);
        control.relation = None;
        let mut claim = geo("m", "f1", 1, None);
        claim.relation = Some("claim".into());
        UpsertFactionGeometryCommand::new(control)
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(claim)
            .apply(&mut w)
            .unwrap();
        assert_eq!(
            w.faction_geometry.len(),
            2,
            "同 map+owner+from 的 claim/control 必须并存（ADR-0027 D5）"
        );
    }

    #[test]
    fn add_faction_apply_then_revert_removes() {
        let mut w = World::default();
        let cmd = AddFactionCommand::new(f("f1", "F1"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.factions.len(), 1);
        assert!(dirty.contains(&"factions".to_string()));
        cmd.revert(&mut w).unwrap();
        assert!(w.factions.is_empty());
    }

    #[test]
    fn add_faction_duplicate_id_is_precondition_error() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let err = AddFactionCommand::new(f("f1", "F1-again")).apply(&mut w);
        assert!(matches!(err, Err(EditError::Precondition(_))));
    }

    #[test]
    fn remove_faction_also_clears_its_geometry() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 2);

        let rm = RemoveFactionCommand::new(Id::new_unchecked("f1"));
        rm.apply(&mut w).unwrap();
        assert!(w.factions.is_empty(), "Faction 自身被删");
        assert!(w.faction_geometry.is_empty(), "名下几何连带清空");

        rm.revert(&mut w).unwrap();
        assert_eq!(w.factions.len(), 1, "Faction 还原");
        assert_eq!(w.faction_geometry.len(), 2, "几何还原");
    }

    #[test]
    fn remove_faction_missing_is_target_error() {
        let mut w = World::default();
        let cmd = RemoveFactionCommand::new(Id::new_unchecked("ghost"));
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    #[test]
    fn update_faction_round_trips_via_revert() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let upd = UpdateFactionCommand::new(
            Id::new_unchecked("f1"),
            "F1-Renamed".into(),
            wv_core::color::Color("#FF0000".into()),
        );
        upd.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].name, "F1-Renamed");
        assert_eq!(w.factions[0].color.as_str(), "#FF0000");
        upd.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].name, "F1");
        assert_eq!(w.factions[0].color.as_str(), "#112233");
    }

    #[test]
    fn upsert_geometry_inserts_when_new() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let upsert = UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)));
        let dirty = upsert.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 1);
        assert!(dirty.contains(&"geometry/faction".to_string()));
        // revert：应能移除刚 push 的那条
        upsert.revert(&mut w).unwrap();
        assert!(w.faction_geometry.is_empty());
    }

    #[test]
    fn upsert_geometry_replaces_when_same_key() {
        // (map, owner, from) 一致 → 替换。`from` 不同的算作"另一版本" → 共存。
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(20)))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 1, "from 相同 → 替换");
        assert_eq!(w.faction_geometry[0].span.to, Some(20));
        UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 2, "from 不同 → 共存");
    }

    #[test]
    fn upsert_geometry_revert_restores_replaced_value() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let original = geo("m", "f1", 1, Some(10));
        UpsertFactionGeometryCommand::new(original.clone())
            .apply(&mut w)
            .unwrap();
        let updated = geo("m", "f1", 1, Some(99));
        let upd_cmd = UpsertFactionGeometryCommand::new(updated);
        upd_cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry[0].span.to, Some(99));
        upd_cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_geometry[0].span.to, Some(10), "应恢复为 10");
    }

    #[test]
    fn upsert_geometry_rejects_orphan_owner() {
        let mut w = World::default();
        // 没有 add Faction。
        let cmd = UpsertFactionGeometryCommand::new(geo("m", "f_ghost", 1, Some(10)));
        assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
    }

    #[test]
    fn remove_geometry_apply_then_revert_restores_at_original_index() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
            .apply(&mut w)
            .unwrap();
        // 删中间那条 (from=20)。
        let rm =
            RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 20);
        rm.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 2);
        assert!(w.faction_geometry.iter().all(|g| g.span.from != 20));
        rm.revert(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 3);
        assert_eq!(w.faction_geometry[1].span.from, 20, "按原位置 1 还原");
    }

    #[test]
    fn remove_geometry_missing_is_target_error() {
        let mut w = World::default();
        let cmd =
            RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 999);
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    // ---------------------- 街道命令（ADR-0010 ③） ----------------------

    fn seg(class: wv_core::RoadClass) -> wv_core::StreetSegment {
        wv_core::StreetSegment::straight(class, 10.0, 20.0, 30.0, 40.0)
    }

    const TEST_LAYER: &str = "layer_city_streets";

    fn test_layer() -> Id {
        Id::new_unchecked(TEST_LAYER)
    }

    // ---------------------- 建筑命令（ADR-0016 §6） ----------------------

    const TEST_BUILDING_LAYER: &str = "layer_city_buildings";

    fn test_building_layer() -> Id {
        Id::new_unchecked(TEST_BUILDING_LAYER)
    }

    fn sample_building(h: f32) -> wv_core::Building {
        wv_core::Building::rect(0.0, 0.0, 8.0, 8.0, h)
    }

    #[test]
    fn add_building_then_revert_restores_the_document() {
        let mut w = World::default();
        let mut doc = wv_core::BuildingLayer::new();
        doc.push(sample_building(6.0));
        w.set_buildings(test_building_layer(), doc);

        let cmd = AddBuildingCommand::new(test_building_layer(), sample_building(12.0));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("buildings/{}", TEST_BUILDING_LAYER)],
            "脏路径应为虚拟命名空间 `buildings/<layer>`"
        );
        assert_eq!(w.building_of(&test_building_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        let doc = w.building_of(&test_building_layer()).unwrap();
        assert_eq!(doc.len(), 1, "revert 应截回追加前长度");
        assert!(
            (doc.buildings[0].height_m - 6.0).abs() < 1e-6,
            "被截掉的是新画的那栋，旧栋原样（高度实得 {}）",
            doc.buildings[0].height_m
        );
    }

    #[test]
    fn delete_building_reverts_by_reinserting_at_the_same_index() {
        let mut w = World::default();
        let mut doc = wv_core::BuildingLayer::new();
        doc.push(sample_building(6.0));
        doc.push(sample_building(12.0));
        doc.push(sample_building(18.0));
        w.set_buildings(test_building_layer(), doc);

        let cmd = DeleteBuildingCommand::new(test_building_layer(), 1);
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.building_of(&test_building_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        let doc = w.building_of(&test_building_layer()).unwrap();
        assert_eq!(doc.len(), 3, "revert 应插回原索引");
        assert!(
            (doc.buildings[1].height_m - 12.0).abs() < 1e-6,
            "插回的应是原 #1（高度实得 {}）",
            doc.buildings[1].height_m
        );
    }

    #[test]
    fn delete_building_out_of_range_is_a_precondition_error() {
        let mut w = World::default();
        let err = DeleteBuildingCommand::new(test_building_layer(), 5)
            .apply(&mut w)
            .unwrap_err();
        assert!(
            matches!(err, crate::EditError::Precondition(_)),
            "越界删除应报 Precondition，实得 {err:?}"
        );
    }

    #[test]
    fn add_street_segment_then_revert_restores_the_document() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        w.set_streets(test_layer(), doc);

        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Service));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("streets/{}", TEST_LAYER)],
            "脏路径应为虚拟命名空间 `streets/<layer>`"
        );
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            1,
            "revert 应截回追加前长度"
        );
        assert_eq!(
            w.street_of(&test_layer()).unwrap().segments[0].class,
            wv_core::RoadClass::Primary,
            "被截掉的是新画的那段，旧段原样"
        );
    }

    #[test]
    fn drawing_on_an_unloaded_street_layer_creates_it_silently() {
        let mut w = World::default();
        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Residential));
        cmd.apply(&mut w).unwrap();
        let doc = w.street_of(&test_layer()).expect("首次落笔应新建文档");
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.segments[0].class, wv_core::RoadClass::Residential);
    }

    #[test]
    fn reverting_before_apply_is_a_precondition_error() {
        let mut w = World::default();
        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Path));
        assert!(matches!(
            cmd.revert(&mut w),
            Err(EditError::Precondition(_))
        ));
    }

    #[test]
    fn two_street_commands_undo_independently() {
        let mut w = World::default();
        let a = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Primary));
        let b = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Path));
        a.apply(&mut w).unwrap();
        b.apply(&mut w).unwrap();
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);
        b.revert(&mut w).unwrap();
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 1);
        a.revert(&mut w).unwrap();
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            0,
            "逐条撤销后文档应为空"
        );
    }

    #[test]
    fn deleting_a_segment_then_reverting_restores_it_at_the_same_index() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        doc.push(seg(wv_core::RoadClass::Path));
        doc.push(seg(wv_core::RoadClass::Service));
        w.set_streets(test_layer(), doc);

        let cmd = DeleteStreetSegmentCommand::new(test_layer(), 1);
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("streets/{}", TEST_LAYER)],
            "脏路径应为虚拟命名空间 `streets/<layer>`"
        );
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);
        assert_eq!(
            w.street_of(&test_layer()).unwrap().segments[1].class,
            wv_core::RoadClass::Service,
            "中间那段应被删掉"
        );

        cmd.revert(&mut w).unwrap();
        let doc = w.street_of(&test_layer()).unwrap();
        assert_eq!(doc.len(), 3, "revert 应把段插回原索引");
        assert_eq!(
            doc.segments[1].class,
            wv_core::RoadClass::Path,
            "插回的必须是原来那一段"
        );
    }

    #[test]
    fn deleting_an_out_of_range_segment_is_a_precondition_error() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        w.set_streets(test_layer(), doc);

        let cmd = DeleteStreetSegmentCommand::new(test_layer(), 1);
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "越界索引必须被拒绝而不是 panic"
        );
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            1,
            "失败的命令不得改动文档"
        );
    }

    #[test]
    fn move_city_translates_holed_outline_including_holes() {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let mut w = World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("map_city"),
            name: "城".into(),
            level: MapLevel::City,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: Some(GeometryData::PolygonWithHoles {
                outer: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
                holes: vec![vec![4.0, 4.0, 6.0, 4.0, 6.0, 6.0, 4.0, 6.0]],
            }),
        });
        let cmd = MoveCityCommand::new(
            Id::new_unchecked("map_city"),
            wv_core::Point::new(5.0, -3.0),
        );
        cmd.apply(&mut w).expect("带洞轮廓应可平移");
        let GeometryData::PolygonWithHoles { outer, holes } =
            w.maps[0].outline_polygon.clone().expect("轮廓仍在")
        else {
            panic!("变体不应变化");
        };
        assert_eq!(outer[0], 5.0, "外环 x 应 +5，实得 {}", outer[0]);
        assert_eq!(outer[1], -3.0, "外环 y 应 -3，实得 {}", outer[1]);
        assert_eq!(holes[0][0], 9.0, "洞环 x 也应 +5，实得 {}", holes[0][0]);
        assert_eq!(holes[0][1], 1.0, "洞环 y 也应 -3，实得 {}", holes[0][1]);
        cmd.revert(&mut w).expect("revert");
        let GeometryData::PolygonWithHoles { outer, .. } =
            w.maps[0].outline_polygon.clone().expect("轮廓仍在")
        else {
            panic!("变体不应变化");
        };
        assert_eq!(outer[0], 0.0, "revert 应还原");
    }

    #[test]
    fn set_map_orientation_applies_and_reverts() {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let mut w = World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        });

        let cmd = SetMapOrientationCommand::new(Id::new_unchecked("world_main"), 90.0);
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec!["maps/world_main/index.yaml".to_string()],
            "脏路径应为 maps/<id>/index.yaml"
        );
        assert_eq!(w.maps[0].orientation, 90.0, "apply 后朝向应为 90.0");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.maps[0].orientation, 0.0, "revert 后朝向应恢复为 0.0");
    }

    #[test]
    fn set_map_orientation_missing_map_is_a_target_missing_error() {
        let mut w = World::default();
        let cmd = SetMapOrientationCommand::new(Id::new_unchecked("ghost_map"), 45.0);
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "不存在的地图必须报 TargetMissing"
        );
    }

    // -----------------------------------------------------------------------
    // AddMapCommand（新建地图对话框的落点）
    // -----------------------------------------------------------------------

    fn map_node(
        id: &str,
        parent: Option<&str>,
        level: wv_core::map::MapLevel,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: format!("地图{id}"),
            level,
            parent: parent.map(Id::new_unchecked),
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
        }
    }

    fn flat_terrain(biome: u8, height: i16) -> TerrainData {
        TerrainData {
            width: 4,
            height: 4,
            cells: vec![
                TerrainCellData {
                    height,
                    surface: if biome == 0 { 0 } else { 1 },
                    biome,
                };
                16
            ],
        }
    }

    #[test]
    fn add_map_command_links_parent_and_prefills_terrain() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));

        let cmd = AddMapCommand::new(
            map_node("map_a", Some("world_main"), wv_core::map::MapLevel::City),
            Some(flat_terrain(0, -40)),
        );
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(
            dirty.contains(&"maps".to_owned())
                && dirty.contains(&"maps/map_a/terrain.bin".to_owned()),
            "脏路径应含节点目录与栅格文件，实得 {dirty:?}"
        );
        // 节点入树 + 父引用回填 + 栅格落位，三件都在。
        assert!(w.maps.iter().any(|m| m.id == Id::new_unchecked("map_a")));
        assert!(w
            .maps
            .iter()
            .find(|m| m.id == Id::new_unchecked("world_main"))
            .unwrap()
            .children
            .contains(&Id::new_unchecked("map_a")));
        assert_eq!(
            w.terrain_of(&Id::new_unchecked("map_a")).unwrap().cells[0].surface,
            0
        );

        // revert 对称拆三件套。
        cmd.revert(&mut w).unwrap();
        assert!(!w.maps.iter().any(|m| m.id == Id::new_unchecked("map_a")));
        assert!(!w
            .maps
            .iter()
            .find(|m| m.id == Id::new_unchecked("world_main"))
            .unwrap()
            .children
            .contains(&Id::new_unchecked("map_a")));
        assert!(w.terrain_of(&Id::new_unchecked("map_a")).is_none());
    }

    #[test]
    fn add_map_command_rejects_duplicate_id_instead_of_overwriting() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        w.maps.push(map_node(
            "map_a",
            Some("world_main"),
            wv_core::map::MapLevel::City,
        ));
        let before = w.maps.len();

        let cmd = AddMapCommand::new(
            map_node("map_a", Some("world_main"), wv_core::map::MapLevel::Region),
            None,
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "重复 ID 必须报错，不能静默覆盖已有地图"
        );
        assert_eq!(w.maps.len(), before, "失败的 apply 不得改动地图表");
    }

    #[test]
    fn add_map_command_rejects_dangling_parent() {
        let mut w = World::default();
        let cmd = AddMapCommand::new(
            map_node("map_a", Some("ghost_parent"), wv_core::map::MapLevel::City),
            None,
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "悬空父引用必须报 TargetMissing"
        );
        assert!(w.maps.is_empty(), "失败的 apply 不得留下半张图");
    }

    #[test]
    fn root_map_adds_without_parent_and_revert_removes_it() {
        // 空编辑器的「第一张图」：parent = None，revert 后世界回到空。
        let mut w = World::default();
        let cmd = AddMapCommand::new(
            map_node("world_main", None, wv_core::map::MapLevel::World),
            Some(flat_terrain(3, 80)),
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.maps.len(), 1);
        cmd.revert(&mut w).unwrap();
        assert!(w.maps.is_empty());
        assert!(w.terrains.is_empty());
    }

    // -----------------------------------------------------------------------
    // 测量命令（2026-10-14）
    // -----------------------------------------------------------------------

    use wv_core::MeasurementKind;

    fn m_line(id: &str) -> Measurement {
        Measurement::polyline(vec![
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(10.0, 0.0),
            wv_core::Point::new(10.0, 5.0),
        ])
        .with_id(Id::new_unchecked(id))
    }

    fn m_rect(id: &str) -> Measurement {
        Measurement::rectangle(
            wv_core::Point::new(0.0, 0.0),
            wv_core::Point::new(20.0, 10.0),
        )
        .with_id(Id::new_unchecked(id))
    }

    fn m_kind(m: &Measurement) -> MeasurementKind {
        m.kind
    }

    #[test]
    fn add_measurement_pushes_and_reports_dirty_path() {
        let mut w = World::default();
        let cmd = AddMeasurementCommand::new(Id::new_unchecked("world_main"), m_line("m1"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec!["measurements/world_main".to_string()],
            "脏路径应为虚拟命名空间 measurements/<map>"
        );
        let overlay = w.measurement_of(&Id::new_unchecked("world_main")).unwrap();
        assert_eq!(overlay.len(), 1);
        assert_eq!(overlay.measurements[0].id.as_str(), "m1");
        assert_eq!(m_kind(&overlay.measurements[0]), MeasurementKind::Polyline);

        cmd.revert(&mut w).unwrap();
        let overlay = w.measurement_of(&Id::new_unchecked("world_main")).unwrap();
        assert!(overlay.is_empty(), "revert 后该图 overlay 应清空");
    }

    #[test]
    fn add_measurement_rejects_duplicate_id() {
        let mut w = World::default();
        AddMeasurementCommand::new(Id::new_unchecked("m1"), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        // 同 ID 二次添加 = Precondition 错。
        let err = AddMeasurementCommand::new(Id::new_unchecked("m1"), m_rect("m1")).apply(&mut w);
        assert!(matches!(err, Err(EditError::Precondition(_))));
        // overlay 长度仍是 1（失败的 apply 不得增改）
        assert_eq!(w.measurement_of(&Id::new_unchecked("m1")).unwrap().len(), 1);
    }

    #[test]
    fn add_measurement_revert_before_apply_is_precondition() {
        let mut w = World::default();
        let cmd = AddMeasurementCommand::new(Id::new_unchecked("m1"), m_line("m1"));
        assert!(matches!(
            cmd.revert(&mut w),
            Err(EditError::Precondition(_))
        ));
    }

    #[test]
    fn remove_measurement_then_revert_restores_at_same_index() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        AddMeasurementCommand::new(map.clone(), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        AddMeasurementCommand::new(map.clone(), m_rect("m2"))
            .apply(&mut w)
            .unwrap();
        AddMeasurementCommand::new(map.clone(), m_line("m3"))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.measurement_of(&map).unwrap().len(), 3);

        // 删中间那条 m2。
        let rm = RemoveMeasurementCommand::new(map.clone(), Id::new_unchecked("m2"));
        let dirty = rm.apply(&mut w).unwrap();
        assert_eq!(dirty, vec!["measurements/world_main".to_string()]);
        let overlay = w.measurement_of(&map).unwrap();
        assert_eq!(overlay.len(), 2);
        assert!(overlay.find(&Id::new_unchecked("m2")).is_none());

        rm.revert(&mut w).unwrap();
        let overlay = w.measurement_of(&map).unwrap();
        assert_eq!(overlay.len(), 3);
        assert_eq!(
            overlay.measurements[1].id.as_str(),
            "m2",
            "应按原位置 1 还原"
        );
        assert_eq!(
            m_kind(&overlay.measurements[1]),
            MeasurementKind::Rectangle,
            "还原的应是矩形那条"
        );
    }

    #[test]
    fn remove_measurement_missing_is_target_error() {
        let mut w = World::default();
        let cmd = RemoveMeasurementCommand::new(
            Id::new_unchecked("world_main"),
            Id::new_unchecked("ghost"),
        );
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    #[test]
    fn clear_measurements_drops_and_revert_restores_full_overlay() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        AddMeasurementCommand::new(map.clone(), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        AddMeasurementCommand::new(map.clone(), m_rect("m2"))
            .apply(&mut w)
            .unwrap();
        let before = w.measurement_of(&map).unwrap().measurements.clone();

        let cmd = ClearMeasurementsCommand::new(map.clone());
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(!dirty.is_empty(), "非空→空 应记脏路径");
        assert!(w.measurement_of(&map).unwrap().is_empty());

        cmd.revert(&mut w).unwrap();
        let overlay = w.measurement_of(&map).unwrap();
        assert_eq!(overlay.measurements, before, "整份还原");
    }

    #[test]
    fn clear_measurements_on_empty_or_missing_overlay_reports_no_dirty() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        // 第一次：根本没 overlay（连 BTreeMap 键都没）→ apply 应记空脏路径。
        let cmd = ClearMeasurementsCommand::new(map.clone());
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(dirty.is_empty(), "原本就空→应不写盘");
        // revert 也应合法（缓存的「None」可还原）。
        cmd.revert(&mut w).unwrap();

        // 第二次：键存在但内部空 → apply 空。revert 不报错。
        w.measurement_of_mut(&map);
        let cmd = ClearMeasurementsCommand::new(map.clone());
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(dirty.is_empty());
        cmd.revert(&mut w).unwrap();
    }

    #[test]
    fn three_measurement_commands_undo_independently() {
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        let a = AddMeasurementCommand::new(map.clone(), m_line("m1"));
        let b = AddMeasurementCommand::new(map.clone(), m_rect("m2"));
        a.apply(&mut w).unwrap();
        b.apply(&mut w).unwrap();
        assert_eq!(w.measurement_of(&map).unwrap().len(), 2);

        b.revert(&mut w).unwrap();
        assert_eq!(w.measurement_of(&map).unwrap().len(), 1);
        a.revert(&mut w).unwrap();
        // 两个 revert 之后：BTreeMap 键里仍留有该键，但 measurements 为空。
        let overlay = w.measurement_of(&map).unwrap();
        assert!(overlay.is_empty(), "overlay 应留空文档兜底");
    }

    #[test]
    fn measurement_dirty_path_does_not_collide_with_streets_or_maps() {
        // 关键不变量：脏路径不与 streets/、maps/ 共享前缀 → 不应误触发
        // terrain.bin / index.yaml / roads.yaml 重写。
        let mut w = World::default();
        let map = Id::new_unchecked("world_main");
        AddMeasurementCommand::new(map.clone(), m_line("m1"))
            .apply(&mut w)
            .unwrap();
        let dirty_paths = RemoveMeasurementCommand::new(map.clone(), Id::new_unchecked("m1"))
            .apply(&mut w)
            .unwrap();
        assert!(dirty_paths.iter().all(|p| p.starts_with("measurements/")));
        assert!(!dirty_paths.iter().any(|p| p.starts_with("maps/")));
        assert!(!dirty_paths.iter().any(|p| p.starts_with("streets/")));
    }

    // ── Region 命令测试 ────────────────────────────────────────────────────

    use wv_core::Region;

    fn sample_region(id: &str, name: &str) -> Region {
        Region {
            id: Id::new_unchecked(id),
            name: name.into(),
            map: Id::new_unchecked(TEST_MAP),
            polygon: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            // ADR-0045 D3：洞环 / 类别 / 父区 / 覆盖物键。
            holes: Vec::new(),
            kind: wv_core::model::region::RegionKind::Area,
            parent: None,
            overlay: None,
        }
    }

    #[test]
    fn add_region_pushes_to_world_and_reverts() {
        let mut w = World::default();
        let cmd = AddRegionCommand::new(sample_region("r1", "关中平原"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.regions.len(), 1);
        assert_eq!(w.regions[0].id.as_str(), "r1");
        assert!(dirty.iter().any(|p| p == "regions"));

        cmd.revert(&mut w).unwrap();
        assert!(w.regions.is_empty(), "revert 应移除区域");
    }

    #[test]
    fn add_region_rejects_duplicate_id() {
        let mut w = World::default();
        w.regions.push(sample_region("r1", "A"));
        let cmd = AddRegionCommand::new(sample_region("r1", "B"));
        assert!(
            cmd.apply(&mut w).is_err(),
            "重复 ID 应返回 Precondition 错误"
        );
    }

    #[test]
    fn remove_region_removes_and_reverts_to_original_position() {
        let mut w = World::default();
        w.regions.push(sample_region("r0", "先"));
        w.regions.push(sample_region("r1", "目标"));
        w.regions.push(sample_region("r2", "后"));

        let cmd = RemoveRegionCommand::new(Id::new_unchecked("r1"));
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.regions.len(), 2);
        assert_eq!(w.regions[0].id.as_str(), "r0");
        assert_eq!(w.regions[1].id.as_str(), "r2");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.regions.len(), 3);
        assert_eq!(w.regions[1].id.as_str(), "r1", "应恢复到原位置");
        assert_eq!(w.regions[1].name, "目标");
    }

    #[test]
    fn remove_region_on_missing_id_fails() {
        let mut w = World::default();
        let cmd = RemoveRegionCommand::new(Id::new_unchecked("ghost"));
        assert!(cmd.apply(&mut w).is_err(), "不存在的 ID 应报 TargetMissing");
    }

    #[test]
    fn update_region_changes_name_and_polygon_and_reverts() {
        let mut w = World::default();
        w.regions.push(sample_region("r1", "旧名"));

        let cmd = UpdateRegionCommand::new(
            Id::new_unchecked("r1"),
            "新名".into(),
            vec![0.0, 0.0, 5.0, 0.0, 5.0, 5.0],
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.regions[0].name, "新名");
        assert_eq!(w.regions[0].polygon, vec![0.0, 0.0, 5.0, 0.0, 5.0, 5.0]);

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.regions[0].name, "旧名");
        assert_eq!(
            w.regions[0].polygon,
            vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
            "revert 应恢复原始 polygon"
        );
    }

    #[test]
    fn update_region_on_missing_id_fails() {
        let mut w = World::default();
        let cmd = UpdateRegionCommand::new(Id::new_unchecked("ghost"), "X".into(), vec![]);
        assert!(cmd.apply(&mut w).is_err());
    }

    #[test]
    fn region_dirty_path_is_regions_prefix() {
        let mut w = World::default();
        let dirty = AddRegionCommand::new(sample_region("r1", "R"))
            .apply(&mut w)
            .unwrap();
        assert!(dirty.iter().all(|p| p.starts_with("regions")));
        assert!(!dirty.iter().any(|p| p.starts_with("maps/")));
        assert!(!dirty.iter().any(|p| p.starts_with("geometry/")));
    }

    // ====== 地点属性命令（统一 Inspector · 2026-10-22）======================

    /// 地点夹具：锚点已就位，便于验证「锚点条件可写」。
    fn sample_place(id: &str, name: &str) -> wv_core::distance::Place {
        wv_core::distance::Place {
            kind: None,
            line: None,
            id: Id::new_unchecked(id),
            name: name.into(),
            feature: None,
            biome: Some(wv_terrain::biome::BIOME_HILLS),
            envelope: None,
            map: Some(Id::new_unchecked(TEST_MAP)),
            x: Some(10.0),
            y: Some(20.0),
            w: Some(4.0),
            h: Some(6.0),
            rank: Some(1),
            line_kind: None,
            line_width_m: None,
        }
    }

    #[test]
    fn update_place_changes_name_rank_anchor_then_reverts() {
        let mut w = World::default();
        w.distances.places.push(sample_place("p1", "旧名"));

        let cmd = UpdatePlaceCommand::new(
            Id::new_unchecked("p1"),
            "新名".into(),
            Some(3),
            Some(99.0),
            Some(88.0),
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.distances.places[0].name, "新名");
        assert_eq!(w.distances.places[0].rank, Some(3));
        assert_eq!(w.distances.places[0].x, Some(99.0));
        assert_eq!(w.distances.places[0].y, Some(88.0));

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.distances.places[0].name, "旧名");
        assert_eq!(w.distances.places[0].rank, Some(1));
        assert_eq!(
            w.distances.places[0].x,
            Some(10.0),
            "revert 应恢复原始锚点 X"
        );
        assert_eq!(w.distances.places[0].y, Some(20.0));
    }

    #[test]
    fn update_place_keeps_anchor_when_xy_none() {
        let mut w = World::default();
        w.distances.places.push(sample_place("p1", "旧名"));

        // new_x / new_y 为 None = 「不改」——**不是**「写成 None」。
        let cmd =
            UpdatePlaceCommand::new(Id::new_unchecked("p1"), "新名".into(), Some(2), None, None);
        cmd.apply(&mut w).unwrap();
        assert_eq!(
            w.distances.places[0].x,
            Some(10.0),
            "new_x = None 时锚点 X 应保持原值，不得被清空"
        );
        assert_eq!(w.distances.places[0].y, Some(20.0));
        assert_eq!(w.distances.places[0].name, "新名");
        assert_eq!(w.distances.places[0].rank, Some(2));
    }

    #[test]
    fn update_place_on_missing_id_fails() {
        let mut w = World::default();
        let cmd = UpdatePlaceCommand::new(Id::new_unchecked("ghost"), "X".into(), None, None, None);
        assert!(cmd.apply(&mut w).is_err());
    }

    #[test]
    fn update_place_dirty_path_is_places_prefix() {
        let mut w = World::default();
        w.distances.places.push(sample_place("p1", "旧名"));
        let dirty =
            UpdatePlaceCommand::new(Id::new_unchecked("p1"), "新名".into(), None, None, None)
                .apply(&mut w)
                .unwrap();
        assert_eq!(dirty, vec!["distances/places".to_string()]);
    }

    #[test]
    fn set_place_kind_writes_and_reverts_the_category() {
        use wv_core::distance::PoiKind;
        let mut w = World::default();
        w.distances.places.push(sample_place("p1", "医院"));
        let cmd = SetPlaceKindCommand::new(Id::new_unchecked("p1"), Some(PoiKind::Medical));

        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            w.distances.places[0].kind,
            Some(PoiKind::Medical),
            "apply 应写入类别"
        );
        assert_eq!(dirty, vec!["distances/places".to_string()]);

        cmd.revert(&mut w).unwrap();
        assert_eq!(
            w.distances.places[0].kind, None,
            "revert 应恢复原类别（此处为缺省 None）"
        );
    }

    /// `kind = None` 是「清空」而不是「不改」——与 `UpdatePlaceCommand` 的
    /// `Option` 语义不同，这条测试钉住区别。
    #[test]
    fn set_place_kind_none_clears_the_category() {
        use wv_core::distance::PoiKind;
        let mut w = World::default();
        let mut p = sample_place("p1", "医院");
        p.kind = Some(PoiKind::Medical);
        w.distances.places.push(p);

        SetPlaceKindCommand::new(Id::new_unchecked("p1"), None)
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.distances.places[0].kind, None, "None 应清空类别");
    }

    #[test]
    fn set_place_kind_on_missing_id_fails() {
        use wv_core::distance::PoiKind;
        let mut w = World::default();
        let cmd = SetPlaceKindCommand::new(Id::new_unchecked("ghost"), Some(PoiKind::Food));
        assert!(cmd.apply(&mut w).is_err());
    }

    // ====== 事件属性命令（统一 Inspector · 2026-10-22）======================

    /// 事件夹具：带时间跨度 + 一个参与者。
    fn sample_event(id: &str, name: &str) -> wv_core::model::event::Event {
        wv_core::model::event::Event {
            id: Id::new_unchecked(id),
            name: name.into(),
            span: TimeSpan {
                from: 100,
                to: Some(200),
            },
            location: None,
            participants: vec![Id::new_unchecked("char_a")],
            narrative: Some("旧叙述".into()),
            outcome: None,
            effects: Vec::new(),
            narrations: Vec::new(),
        }
    }

    #[test]
    fn update_event_changes_fields_then_reverts() {
        let mut w = World::default();
        w.events.push(sample_event("e1", "旧名"));

        let cmd = UpdateEventCommand::new(
            Id::new_unchecked("e1"),
            "新名".into(),
            TimeSpan {
                from: 300,
                to: None,
            },
            None,
            vec![Id::new_unchecked("char_b")],
            Some("新叙述".into()),
            Some("新结果".into()),
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.events[0].name, "新名");
        assert_eq!(w.events[0].span.from, 300);
        assert_eq!(w.events[0].span.to, None);
        assert_eq!(w.events[0].participants, vec![Id::new_unchecked("char_b")]);
        assert_eq!(w.events[0].narrative.as_deref(), Some("新叙述"));
        assert_eq!(w.events[0].outcome.as_deref(), Some("新结果"));

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.events[0].name, "旧名");
        assert_eq!(w.events[0].span.from, 100);
        assert_eq!(w.events[0].span.to, Some(200));
        assert_eq!(w.events[0].participants, vec![Id::new_unchecked("char_a")]);
        assert_eq!(w.events[0].narrative.as_deref(), Some("旧叙述"));
        assert_eq!(w.events[0].outcome, None);
    }

    #[test]
    fn update_event_revert_preserves_untouched_effects() {
        let mut w = World::default();
        w.events.push(sample_event("e1", "旧名"));
        // effects 不在本命令的编辑面内——revert **不得**把它一并退掉。
        w.events[0]
            .effects
            .push(wv_core::model::event::Effect::Custom {
                note: "作者手写的效果".into(),
            });

        let cmd = UpdateEventCommand::new(
            Id::new_unchecked("e1"),
            "新名".into(),
            TimeSpan {
                from: 100,
                to: Some(200),
            },
            None,
            Vec::new(),
            None,
            None,
        );
        cmd.apply(&mut w).unwrap();
        cmd.revert(&mut w).unwrap();

        assert_eq!(w.events[0].name, "旧名", "本命令改的字段应回退");
        assert_eq!(
            w.events[0].effects.len(),
            1,
            "revert 只写回本命令触及的字段——effects 不在其列，应保持不变"
        );
    }

    #[test]
    fn update_event_on_missing_id_fails() {
        let mut w = World::default();
        let cmd = UpdateEventCommand::new(
            Id::new_unchecked("ghost"),
            "X".into(),
            TimeSpan { from: 0, to: None },
            None,
            Vec::new(),
            None,
            None,
        );
        assert!(cmd.apply(&mut w).is_err());
    }

    #[test]
    fn update_event_dirty_path_is_events_prefix() {
        let mut w = World::default();
        w.events.push(sample_event("e1", "旧名"));
        let dirty = UpdateEventCommand::new(
            Id::new_unchecked("e1"),
            "新名".into(),
            TimeSpan { from: 0, to: None },
            None,
            Vec::new(),
            None,
            None,
        )
        .apply(&mut w)
        .unwrap();
        assert_eq!(dirty, vec!["timeline/events".to_string()]);
    }

    // ====== 城市子图命令（ADR-0022 D2 / D5）========================

    fn city_node(
        id: &str,
        parent: Option<&str>,
        outline: Option<GeometryData>,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked(id),
            name: format!("城{id}"),
            level: wv_core::map::MapLevel::City,
            parent: parent.map(wv_core::Id::new_unchecked),
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            outline_polygon: outline,
            ..wv_core::map::MapNode::default()
        }
    }

    fn rect_polygon(w: f32, h: f32) -> GeometryData {
        GeometryData::Polygon(vec![0.0, 0.0, w, 0.0, w, h, 0.0, h])
    }

    #[test]
    fn add_city_pushes_node_and_icon_and_marks_dirty() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 100.0)),
        );
        let cmd = AddCityCommand::new(
            city.clone(),
            wv_core::Id::new_unchecked("icon_city_a"),
            wv_core::map::IconShape::Diamond,
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        // 城市节点 + 父图 icons 都该落地。
        assert_eq!(w.maps.len(), 2, "世界图 + 城市图");
        assert!(
            w.maps
                .iter()
                .any(|m| m.id == wv_core::Id::new_unchecked("map_city_a")),
            "城市子图应在"
        );
        let parent = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("world_main"))
            .expect("父图应在");
        assert_eq!(parent.children.len(), 1, "父图 children 应含城市");
        assert_eq!(parent.icons.len(), 1, "父图 icons 应含钻入图标");
        assert_eq!(
            parent.icons[0].drill_into.as_ref().map(|i| i.as_str()),
            Some("map_city_a"),
            "图标 drill_into 应指向城市"
        );
        // 脏路径：maps + 父图 index + 子图 index
        assert!(dirty.contains(&"maps".to_owned()));
        assert!(dirty.contains(&"maps/world_main/index.yaml".to_owned()));
        assert!(dirty.contains(&"maps/map_city_a/index.yaml".to_owned()));
    }

    #[test]
    fn add_city_revert_removes_node_and_parent_icon() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        let cmd = AddCityCommand::new(
            city_node("map_city_a", Some("world_main"), None),
            wv_core::Id::new_unchecked("icon_city_a"),
            wv_core::map::IconShape::Diamond,
        );
        cmd.apply(&mut w).expect("apply");
        assert_eq!(w.maps.len(), 2);
        cmd.revert(&mut w).expect("revert");
        assert_eq!(w.maps.len(), 1, "revert 应移除城市");
        let parent = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("world_main"))
            .expect("父图应在");
        assert!(parent.children.is_empty(), "父图 children 应清空");
        assert!(parent.icons.is_empty(), "父图 icons 应清空");
    }

    #[test]
    fn add_city_rejects_duplicate_id() {
        let mut w = World::default();
        w.maps
            .push(map_node("map_city_a", None, wv_core::map::MapLevel::World));
        let cmd = AddCityCommand::new(
            city_node("map_city_a", Some("map_city_a"), None),
            wv_core::Id::new_unchecked("icon_dup"),
            wv_core::map::IconShape::Diamond,
        );
        assert!(cmd.apply(&mut w).is_err(), "重复 ID 应 Precondition");
    }

    #[test]
    fn add_city_requires_parent() {
        let mut w = World::default();
        let mut city = city_node("map_orphan", None, None);
        city.parent = None;
        let cmd = AddCityCommand::new(
            city,
            wv_core::Id::new_unchecked("icon_orphan"),
            wv_core::map::IconShape::Diamond,
        );
        assert!(cmd.apply(&mut w).is_err(), "无 parent 应 Precondition");
    }

    #[test]
    fn move_city_translates_origin_and_outline_polygon() {
        let mut w = World::default();
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 200.0)),
        );
        w.maps.push(city.clone());
        let delta = wv_core::geometry::Point::new(10.0, 20.0);
        let cmd = MoveCityCommand::new(wv_core::Id::new_unchecked("map_city_a"), delta);
        cmd.apply(&mut w).expect("apply");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        // extent.origin: None → (10, 20)
        let origin = after.extent.as_ref().unwrap().origin.unwrap();
        assert!(
            (origin.x - 10.0).abs() < 1e-6,
            "x 应 +10，实得 {}",
            origin.x
        );
        assert!(
            (origin.y - 20.0).abs() < 1e-6,
            "y 应 +20，实得 {}",
            origin.y
        );
        // outline_polygon 各顶点平移
        let poly = match after.outline_polygon.as_ref().unwrap() {
            GeometryData::Polygon(v) => v.clone(),
            _ => panic!("outline 应是 Polygon"),
        };
        assert_eq!(
            poly,
            vec![10.0, 20.0, 110.0, 20.0, 110.0, 220.0, 10.0, 220.0]
        );
    }

    #[test]
    fn move_city_revert_restores_origin_and_polygon() {
        let mut w = World::default();
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 200.0)),
        );
        w.maps.push(city.clone());
        let delta = wv_core::geometry::Point::new(10.0, 20.0);
        let cmd = MoveCityCommand::new(wv_core::Id::new_unchecked("map_city_a"), delta);
        cmd.apply(&mut w).expect("apply");
        cmd.revert(&mut w).expect("revert");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        // 初始 extent 由 Extent::sized() 构造（origin = None）；revert 应回 None。
        assert!(
            after.extent.as_ref().unwrap().origin.is_none(),
            "revert 应把 origin 还原为 None（city_node 默认用 Extent::sized）"
        );
        let poly = match after.outline_polygon.as_ref().unwrap() {
            GeometryData::Polygon(v) => v.clone(),
            _ => panic!("outline 应是 Polygon"),
        };
        assert_eq!(poly, vec![0.0, 0.0, 100.0, 0.0, 100.0, 200.0, 0.0, 200.0]);
    }

    #[test]
    fn move_city_no_origin_no_outline_is_noop() {
        let mut w = World::default();
        let mut city = city_node("map_bare", None, None);
        city.extent = None;
        city.outline_polygon = None;
        w.maps.push(city);
        let cmd = MoveCityCommand::new(
            wv_core::Id::new_unchecked("map_bare"),
            wv_core::geometry::Point::new(10.0, 20.0),
        );
        // 既无 extent 又无 outline：apply 不应 panic，应走「无字段可动」通路。
        cmd.apply(&mut w).expect("apply 静默成功");
        cmd.revert(&mut w).expect("revert");
    }

    #[test]
    fn move_city_missing_id_errors() {
        let mut w = World::default();
        let cmd = MoveCityCommand::new(
            wv_core::Id::new_unchecked("ghost"),
            wv_core::geometry::Point::new(1.0, 1.0),
        );
        assert!(cmd.apply(&mut w).is_err(), "不存在应 TargetMissing");
    }

    #[test]
    fn set_city_outline_replaces_polygon_and_reverts_to_old() {
        let mut w = World::default();
        let old_poly = rect_polygon(100.0, 100.0);
        let city = city_node("map_city_a", Some("world_main"), Some(old_poly.clone()));
        w.maps.push(city);
        let new_poly = GeometryData::Polygon(vec![5.0, 5.0, 50.0, 5.0, 50.0, 50.0]);
        let cmd = SetCityOutlineCommand::new(
            wv_core::Id::new_unchecked("map_city_a"),
            Some(new_poly.clone()),
        );
        cmd.apply(&mut w).expect("apply");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        assert_eq!(after.outline_polygon.as_ref().unwrap(), &new_poly);
        cmd.revert(&mut w).expect("revert");
        let reverted = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        assert_eq!(reverted.outline_polygon.as_ref().unwrap(), &old_poly);
    }

    #[test]
    fn set_city_outline_to_none_erases_polygon() {
        let mut w = World::default();
        let city = city_node(
            "map_city_a",
            Some("world_main"),
            Some(rect_polygon(100.0, 100.0)),
        );
        w.maps.push(city);
        let cmd = SetCityOutlineCommand::new(wv_core::Id::new_unchecked("map_city_a"), None);
        cmd.apply(&mut w).expect("apply");
        let after = w
            .maps
            .iter()
            .find(|m| m.id == wv_core::Id::new_unchecked("map_city_a"))
            .expect("城市应在");
        assert!(after.outline_polygon.is_none(), "擦除后应为 None");
    }

    #[test]
    fn set_city_outline_rejects_too_few_vertices() {
        let mut w = World::default();
        let city = city_node("map_city_a", Some("world_main"), None);
        w.maps.push(city);
        let bad = GeometryData::Polygon(vec![0.0, 0.0, 1.0, 1.0]); // 仅 2 顶点
        let cmd = SetCityOutlineCommand::new(wv_core::Id::new_unchecked("map_city_a"), Some(bad));
        assert!(
            cmd.apply(&mut w).is_err(),
            "<3 顶点的 polygon 应 Precondition 拒绝"
        );
    }

    // --- AddPlaceEdgeCommand 测试 ----------------------------------------

    fn edge(id: &str, a: &str, b: &str) -> wv_core::distance::PlaceEdge {
        wv_core::distance::PlaceEdge {
            rank: None,
            id: wv_core::Id::new_unchecked(id),
            a: wv_core::Id::new_unchecked(a),
            b: wv_core::Id::new_unchecked(b),
            distance_km: 12.5,
            terrain: "road".into(),
            bidirectional: true,
            derived: false,
        }
    }

    #[test]
    fn add_place_edge_appends_and_marks_dirty() {
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"));
        let dirty = cmd.apply(&mut w).expect("apply");
        assert_eq!(dirty, vec!["distances/edges".to_string()]);
        assert_eq!(w.distances.edges.len(), 1);
        assert_eq!(w.distances.edges[0].id.as_str(), "edge_a_b");
    }

    #[test]
    fn add_place_edge_rejects_duplicate_id() {
        let mut w = World::default();
        AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"))
            .apply(&mut w)
            .expect("first apply");
        let err = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_c", "place_d"))
            .apply(&mut w)
            .expect_err("second apply with same id must fail");
        assert!(matches!(err, EditError::Precondition(_)));
    }

    #[test]
    fn add_place_edge_revert_removes_the_edge() {
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"));
        cmd.apply(&mut w).expect("apply");
        assert_eq!(w.distances.edges.len(), 1);
        cmd.revert(&mut w).expect("revert");
        assert!(w.distances.edges.is_empty());
    }

    #[test]
    fn add_place_edge_revert_target_missing_when_never_applied() {
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_a_b", "place_a", "place_b"));
        let err = cmd.revert(&mut w).expect_err("revert without apply");
        assert!(matches!(err, EditError::TargetMissing(_)));
    }

    #[test]
    fn add_place_edge_accepts_dangling_endpoints_per_v1() {
        // AGENTS §7.5 V1：悬空引用只提示不阻止——本测试锁住这条契约。
        let mut w = World::default();
        let cmd = AddPlaceEdgeCommand::new(edge("edge_orphan", "place_does_not_exist", "place_x"));
        cmd.apply(&mut w).expect("V1 允许悬空 endpoint");
        assert_eq!(w.distances.edges.len(), 1);
    }

    // ---- 大陆生成宏命令（大陆生成 UI plan Task 1） ----

    fn continent_envelope() -> Vec<Vec<(f32, f32)>> {
        vec![vec![(8.0, 8.0), (40.0, 8.0), (40.0, 40.0), (8.0, 40.0)]]
    }

    #[test]
    fn generate_continent_apply_then_undo_restores_grid_cell_for_cell() {
        let mut world = world_with_terrain(48);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();
        let cmd = GenerateContinentCommand::new(
            test_map(),
            continent_envelope(),
            ContinentParams::default(),
            7,
            1.0,
        );
        let dirty = cmd.apply(&mut world).unwrap();
        assert_eq!(dirty, vec!["maps/world_main/terrain.bin".to_string()]);
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        assert_ne!(before_snapshot, after, "apply 应改变地形");
        assert!(
            after.cells.iter().any(|c| c.height > 0),
            "方形包络内应长出陆地"
        );

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "撤销应逐格复原"
        );
    }

    #[test]
    fn generate_continent_missing_terrain_reports_mismatch_not_panic() {
        let mut world = World::default();
        let cmd = GenerateContinentCommand::new(
            Id::new_unchecked("world_main"),
            continent_envelope(),
            ContinentParams::default(),
            7,
            1.0,
        );
        assert!(matches!(
            cmd.apply(&mut world),
            Err(EditError::TerrainMismatch(_))
        ));
    }

    #[test]
    fn generate_continent_empty_envelopes_is_a_noop_command() {
        let mut world = world_with_terrain(32);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();
        let cmd =
            GenerateContinentCommand::new(test_map(), vec![], ContinentParams::default(), 7, 1.0);
        cmd.apply(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "空包络不得改动网格"
        );
        cmd.revert(&mut world).expect("空命令的 undo 不得炸");
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot
        );
    }

    // ── 事实地点 / 地界命令测试（theme overlay）────────────────────────────
    // ── 事实地点 / 地界命令测试（theme overlay）────────────────────────────

    /// 标记地点 fixture（ADR-0032：fixture 也只写 biome，不写旧 feature 词汇）。
    fn span_fixture_place(id: &str) -> wv_core::distance::Place {
        wv_core::distance::Place {
            kind: None,
            line: None,
            id: Id::new_unchecked(id),
            name: format!("地点{id}"),
            feature: None,
            biome: Some(wv_terrain::biome::BIOME_HILLS),
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    fn territory(
        map: &str,
        owner: &str,
        from: i64,
        to: Option<i64>,
        ring: Vec<f32>,
    ) -> wv_core::map::GeometrySpan {
        wv_core::map::GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: TimeSpan { from, to },
            geometry: GeometryData::Polygon(ring),
            relation: None,
        }
    }

    // ---- 生物群系标记（ADR-0029 / 命名段 M1 Task 3） ----

    fn marker_place(id: &str, name: &str, biome: u8) -> wv_core::distance::Place {
        wv_core::distance::Place {
            kind: None,
            line: None,
            id: wv_core::Id::new_unchecked(id),
            name: name.into(),
            feature: None,
            biome: Some(biome),
            envelope: None,
            map: Some(test_map()),
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    fn land_world(size: u32) -> World {
        let mut w = world_with_terrain(size);
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 3,
            };
            (size * size) as usize
        ];
        w.set_terrain(
            test_map(),
            TerrainData {
                width: size,
                height: size,
                cells: land,
            },
        );
        w
    }

    #[test]
    fn marker_command_generates_cells_place_and_span_atomically() {
        let mut w = land_world(64);
        // 草原格（biome 3）换成森林（1）：低于识别阈值 → 生成。
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        let dirty = cmd.apply(&mut w).expect("标记应成功");
        assert!(dirty.contains(&"distances/places".to_string()));
        assert!(
            dirty.iter().any(|d| d.starts_with("maps/")),
            "写格子必须报地形脏路径：{dirty:?}"
        );
        // 格子：包络内变森林。
        let t = w.terrain_of(&test_map()).expect("地形");
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "包络内应生成森林");
        // Place：biome + envelope 落库，锚点已拟合。
        let p = w
            .distances
            .places
            .iter()
            .find(|p| p.id == wv_core::Id::new_unchecked("place_dark_forest"))
            .expect("Place 应入场");
        assert_eq!(p.biome, Some(1));
        assert_eq!(p.envelope.as_ref().map(|e| e.len()), Some(8));
        assert!(p.x.is_some() && p.w.is_some(), "锚点应从生成边界拟合");
        // 边界 span：外环在包络附近。
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == wv_core::Id::new_unchecked("place_dark_forest"))
            .expect("边界应入场");
        let GeometryData::Polygon(ring) = &span.geometry else {
            panic!("v1 边界应为单环，实得 {:?}", span.geometry);
        };
        let xs: Vec<f32> = ring.chunks_exact(2).map(|c| c[0]).collect();
        let min_x = xs.iter().cloned().fold(f32::MAX, f32::min);
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            (9.0..=11.0).contains(&min_x) && (29.0..=31.0).contains(&max_x),
            "边界应贴包络 10..30 wu，实得 {min_x}..{max_x}"
        );
    }

    #[test]
    fn marker_command_identify_leaves_cells_bitwise() {
        let mut w = land_world(64);
        // 先铺一片森林（>阈值），再按森林标记 → 识别，不写格子。
        let mut cells = w.terrain_of(&test_map()).unwrap().cells.clone();
        for y in 10..30 {
            for x in 10..30 {
                cells[y * 64 + x].biome = 1;
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let before = w.terrain_of(&test_map()).cloned().unwrap();
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(8.0, 8.0), (34.0, 8.0), (34.0, 34.0), (8.0, 34.0)],
        );
        cmd.apply(&mut w).expect("识别应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            before,
            "识别分支不得改动地形"
        );
    }

    #[test]
    fn marker_command_undo_restores_cells_place_and_span_bitwise() {
        let mut w = land_world(64);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        cmd.apply(&mut w).unwrap();
        cmd.revert(&mut w).unwrap();
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_before,
            "revert 应逐位还原地形"
        );
        assert!(w.distances.places.is_empty(), "revert 应移除 Place");
        assert!(w.place_geometry.is_empty(), "revert 应移除边界");
    }

    #[test]
    fn marker_command_at_real_cell_size_uses_grid_units() {
        // cell_size=8：包络 80..320 wu = 格 10..40。若单位换算缺失，
        // 包络会被当成格 80..320（越界）→ 空区域错误。
        let mut w = land_world(64);
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            8.0,
            5,
            marker_place("place_dark_forest", "黑暗森林", 1),
            vec![(80.0, 80.0), (320.0, 80.0), (320.0, 320.0), (80.0, 320.0)],
        );
        cmd.apply(&mut w).expect("cell_size=8 的标记应成功");
        let t = w.terrain_of(&test_map()).unwrap();
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "格 (20,20) 应被标记");
        assert_eq!(t.cells[2 * 64 + 2].biome, 3, "包络外格不应被动");
    }

    // ---- 标记重生成 M2（feat/marker-regen Task 1/4） ----

    fn map_with_extent(id: &str, extent: Option<(f32, f32)>) -> wv_core::map::MapNode {
        let mut n = map_node(id, None, wv_core::map::MapLevel::World);
        n.extent = extent.map(|(w, h)| wv_core::map::Extent::sized(w, h));
        n
    }

    #[test]
    fn marker_cell_size_derives_from_map_extent_over_raster_width() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((512.0, 512.0))));
        let cs = marker_cell_size(&w, &test_map()).expect("有地形就应推出格尺寸");
        assert!(
            (cs - 8.0).abs() < 1e-6,
            "extent 512 / 栅格 64 格应得 8.0 wu/格，实得 {cs}"
        );
    }

    #[test]
    fn marker_cell_size_falls_back_to_root_extent_then_default() {
        // 目标图未声明 extent → 根图 extent 兜底；再无 → 4096（grid_sync 同链）。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, None));
        w.maps
            .push(map_with_extent("root_map", Some((2048.0, 2048.0))));
        w.meta = Some(wv_core::WorldMeta {
            name: "t".into(),
            version: "1".into(),
            schema_version: 2,
            root_map: Id::new_unchecked("root_map"),
            timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
            template: None,
        });
        let cs = marker_cell_size(&w, &test_map()).expect("应回退根图 extent");
        assert!(
            (cs - 32.0).abs() < 1e-6,
            "根图 2048 / 64 格应得 32.0，实得 {cs}"
        );

        // 连根图也没声明 → 4096 / 64 = 64。
        let mut bare = land_world(64);
        bare.maps.push(map_with_extent(TEST_MAP, None));
        let cs = marker_cell_size(&bare, &test_map()).expect("无 extent 应回退 4096");
        assert!(
            (cs - 64.0).abs() < 1e-6,
            "缺省 4096 / 64 格应得 64.0，实得 {cs}"
        );
    }

    #[test]
    fn marker_cell_size_is_none_without_terrain() {
        let w = World::default();
        assert!(
            marker_cell_size(&w, &test_map()).is_none(),
            "无地形栅格的图不该有格尺寸（调用方应拒绝重生成）"
        );
    }

    #[test]
    fn marker_command_stores_holes_when_generated_region_encloses_water() {
        // 陆群系包络套住一片水：生成分支掩膜带洞（水域格不写）→ 几何存洞。
        let mut w = land_world(64);
        let mut cells = w.terrain_of(&test_map()).unwrap().cells.clone();
        for y in 24..40 {
            for x in 24..40 {
                cells[y * 64 + x] = TerrainCellData {
                    height: -20,
                    surface: 0,
                    biome: 0,
                };
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_ring_woods", "环湖林地", 1),
            vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
        );
        cmd.apply(&mut w).expect("生成应成功");
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("place_ring_woods"))
            .expect("边界应入场");
        let GeometryData::PolygonWithHoles { outer, holes } = &span.geometry else {
            panic!("包络套水应存带洞几何，实得 {:?}", span.geometry);
        };
        assert!(outer.len() >= 6, "外环应成环，实得 {outer:?}");
        assert!(!holes.is_empty(), "水域芯应至少成一个洞环，实得 {holes:?}");
        // 洞应贴着水域块 24..40 格（cell_size=1 → 24..40 wu，±1 格容差）。
        let hx: Vec<f32> = holes[0].chunks_exact(2).map(|c| c[0]).collect();
        let (min_x, max_x) = (
            hx.iter().cloned().fold(f32::MAX, f32::min),
            hx.iter().cloned().fold(f32::MIN, f32::max),
        );
        assert!(
            (23.0..=25.0).contains(&min_x) && (39.0..=41.0).contains(&max_x),
            "洞应贴水域 24..40 wu，实得 {min_x}..{max_x}"
        );
    }

    #[test]
    fn marker_command_identify_stores_holes_of_matched_mask() {
        // 森林环 + 草地芯：识别分支的命中掩膜同样是环 → 边界带洞，地形逐位不动。
        let mut w = land_world(64);
        let mut cells = w.terrain_of(&test_map()).unwrap().cells.clone();
        for y in 10..30 {
            for x in 10..30 {
                if (16..24).contains(&x) && (16..24).contains(&y) {
                    continue; // 草地芯
                }
                cells[y * 64 + x].biome = 1;
            }
        }
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 64,
                height: 64,
                cells,
            },
        );
        let before = w.terrain_of(&test_map()).cloned().unwrap();
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            1.0,
            5,
            marker_place("place_hollow_forest", "空心森林", 1),
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        cmd.apply(&mut w).expect("识别应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            before,
            "识别分支不得改动地形"
        );
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == Id::new_unchecked("place_hollow_forest"))
            .expect("边界应入场");
        let GeometryData::PolygonWithHoles { holes, .. } = &span.geometry else {
            panic!("草地芯应使识别边界带洞，实得 {:?}", span.geometry);
        };
        assert!(!holes.is_empty(), "草地芯应至少成一个洞环，实得 {holes:?}");
    }

    // ---- 重生成命令（M2 Task 4） ----

    fn create_marker(
        w: &mut World,
        id: &str,
        cell_size: f32,
        envelope: Vec<(f32, f32)>,
    ) -> wv_core::Id {
        let place_id = Id::new_unchecked(id);
        let cmd = PlaceBiomeMarkerCommand::new(
            test_map(),
            cell_size,
            5,
            marker_place(id, id, 1),
            envelope,
        );
        cmd.apply(w).expect("创建标记应成功");
        place_id
    }

    fn set_biome_block(w: &mut World, x0: i32, y0: i32, x1: i32, y1: i32, biome: u8) {
        let mut data = w.terrain_of(&test_map()).cloned().unwrap();
        for y in y0..y1 {
            for x in x0..x1 {
                data.cells[(y * data.width as i32 + x) as usize].biome = biome;
            }
        }
        w.set_terrain(test_map(), data);
    }

    fn span_ring_of(w: &World, owner: &Id) -> Vec<f32> {
        let span = w
            .place_geometry
            .iter()
            .find(|g| &g.owner == owner)
            .unwrap_or_else(|| panic!("标记 {owner} 应有边界 span"));
        match &span.geometry {
            GeometryData::Polygon(ring) => ring.clone(),
            GeometryData::PolygonWithHoles { outer, .. } => outer.clone(),
            other => panic!("意外的几何形态：{other:?}"),
        }
    }

    #[test]
    fn regenerate_rewrites_stale_boundary_after_brush_without_touching_terrain() {
        // ②主场景：笔刷吃掉半片森林 → 重生成走识别分支，边界收缩、地形逐位不动。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        // 笔刷把 x ≥ 20 的森林改回草原（模拟作者手改 / 后续涂刷）。
        set_biome_block(&mut w, 20, 0, 64, 64, 3);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();

        let dirty = RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("重生成应成功");
        assert!(
            !dirty.iter().any(|p| p.contains("terrain.bin")),
            "识别分支不写格子，dirty 不得含地形路径（省一次无谓的 terrain.bin 重写），实得 {dirty:?}"
        );
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_before,
            "识别分支的重生成不得改动地形格子"
        );
        // span 替换且只剩一条，from 取重生成日。
        let spans: Vec<&GeometrySpan> = w.place_geometry.iter().filter(|g| g.owner == id).collect();
        assert_eq!(spans.len(), 1, "重生成后 owner 名下应恰好 1 条 span");
        assert_eq!(spans[0].span.from, 9, "新 span.from 应取重生成日");
        // 边界收缩到剩余森林（≈8..20 wu）。
        let ring = span_ring_of(&w, &id);
        let xs: Vec<f32> = ring.chunks_exact(2).map(|c| c[0]).collect();
        let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(
            (18.0..=23.0).contains(&max_x),
            "边界应收缩到剩余森林（max_x≈20），实得 {max_x}"
        );
        // 锚点随新边界移动（原 8..32 中点 20 → 剩余 8..20 中点 ≈14）。
        let place = w.distances.places.iter().find(|p| p.id == id).unwrap();
        let x = place.x.expect("锚点应已重拟合");
        assert!(
            (x - 14.0).abs() < 3.0,
            "锚点应随收缩后的边界重拟合（≈14），实得 {x}"
        );
    }

    #[test]
    fn regenerate_generated_branch_rewrites_cells_and_undo_restores_bitwise() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        // 清空包络内森林 → 重生成必须走生成分支重新写格。
        set_biome_block(&mut w, 10, 10, 30, 30, 3);
        let terrain_cleared = w.terrain_of(&test_map()).cloned().unwrap();
        let (anchor_before, span_before) = {
            let p = w.distances.places.iter().find(|p| p.id == id).unwrap();
            (p.x, w.place_geometry.clone())
        };

        let cmd = RegenerateBiomeMarkerCommand::new(id.clone(), 9);
        let dirty = cmd.apply(&mut w).expect("生成分支重生成应成功");
        assert!(
            dirty.contains(&format!("maps/{}/terrain.bin", test_map())),
            "写格子必须报地形脏路径：{dirty:?}"
        );
        let t = w.terrain_of(&test_map()).unwrap();
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "包络内应重新生成森林");
        assert_eq!(t.cells[2 * 64 + 2].biome, 3, "包络外格不应被动");

        cmd.revert(&mut w).expect("revert 应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_cleared,
            "revert 应逐位还原到 apply 前的（已清空）地形"
        );
        let p = w.distances.places.iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.x, anchor_before, "revert 应还原锚点");
        assert_eq!(w.place_geometry, span_before, "revert 应逐条还原 span 列表");
    }

    #[test]
    fn regenerate_replaces_span_instead_of_appending() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        // 裸实例（未 apply）revert 必须无操作：不抹锚点、不动 span。
        RegenerateBiomeMarkerCommand::new(id.clone(), 99)
            .revert(&mut w)
            .expect("未 apply 的 revert 应无操作成功");
        assert!(
            w.place_geometry.iter().any(|g| g.owner == id),
            "无操作 revert 不得动 span"
        );
        assert!(
            w.distances
                .places
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .x
                .is_some(),
            "无操作 revert 不得抹锚点"
        );

        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("第一次重生成");
        RegenerateBiomeMarkerCommand::new(id.clone(), 12)
            .apply(&mut w)
            .expect("第二次重生成");
        let spans: Vec<&GeometrySpan> = w.place_geometry.iter().filter(|g| g.owner == id).collect();
        assert_eq!(
            spans.len(),
            1,
            "两次重生成后 owner 名下仍应恰好 1 条 span（R2 替换），实得 {}",
            spans.len()
        );
        assert_eq!(spans[0].span.from, 12, "最新 span.from 应取第二次重生成日");
    }

    #[test]
    fn regenerate_uses_raster_derived_cell_size() {
        // extent 512 / 栅格 64 格 → 推导 cell_size=8；包络 80..320 wu = 格 10..40。
        // 若格尺寸漂移（如按 1.0 算），包络会越界成 80..320 格 → 空区域错误。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((512.0, 512.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            8.0,
            vec![(80.0, 80.0), (320.0, 80.0), (320.0, 320.0), (80.0, 320.0)],
        );
        set_biome_block(&mut w, 10, 10, 40, 40, 3);
        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("重生成应按 raster 推导的格尺寸成功（C1 回归）");
        let t = w.terrain_of(&test_map()).unwrap();
        assert_eq!(t.cells[20 * 64 + 20].biome, 1, "格 (20,20) 应重新生成森林");
        assert_eq!(t.cells[5 * 64 + 5].biome, 3, "包络外格不应被动");
    }

    #[test]
    fn regenerate_errors_without_biome_or_envelope() {
        let mut w = land_world(64);
        let mut p = marker_place("place_plain", "普通地点", 1);
        p.biome = None;
        p.map = Some(test_map());
        w.distances.places.push(p);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_plain"), 9)
            .apply(&mut w)
            .expect_err("缺 biome 的地点不是标记");
        assert!(
            matches!(err, EditError::Precondition(_)),
            "应为前置条件错误，实得 {err:?}"
        );
        assert_eq!(w.terrain_of(&test_map()).cloned().unwrap(), terrain_before);

        w.distances.places[0].biome = Some(1);
        w.distances.places[0].envelope = None;
        let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_plain"), 9)
            .apply(&mut w)
            .expect_err("缺 envelope 无法重生成");
        assert!(
            matches!(err, EditError::Precondition(_)),
            "应为前置条件错误，实得 {err:?}"
        );
        assert!(w.place_geometry.is_empty(), "失败不得写入任何 span");

        let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_ghost"), 9)
            .apply(&mut w)
            .expect_err("不存在的地点应报 TargetMissing");
        assert!(matches!(err, EditError::TargetMissing(_)), "实得 {err:?}");
    }

    #[test]
    fn regenerate_errors_when_envelope_now_all_water_leaving_world_untouched() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        // 沧海桑田：包络整体沉成海（陆群系无处可写 → EmptyRegion）。
        let mut data = w.terrain_of(&test_map()).cloned().unwrap();
        for y in 10..30 {
            for x in 10..30 {
                data.cells[y * 64 + x] = TerrainCellData {
                    height: -30,
                    surface: 0,
                    biome: 0,
                };
            }
        }
        w.set_terrain(test_map(), data);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        let spans_before = w.place_geometry.clone();
        let places_before = w.distances.places.clone();

        let err = RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect_err("全水包络 + 陆群系应空区域失败");
        assert!(matches!(err, EditError::Precondition(_)), "实得 {err:?}");
        assert_eq!(w.terrain_of(&test_map()).cloned().unwrap(), terrain_before);
        assert_eq!(w.place_geometry, spans_before, "失败不得改动 span");
        assert_eq!(w.distances.places, places_before, "失败不得改动地点");
    }

    /// undo 最险路径：redo（同实例再次 apply）→ 再 revert，仍逐位还原。
    /// `History::redo` 对命令重调 `apply`——快照被重写、span 处于已还原态，
    /// 这条回环必须有测试锁定。
    #[test]
    fn regenerate_revert_redo_cycle_restores_bitwise_each_time() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)],
        );
        set_biome_block(&mut w, 10, 10, 30, 30, 3);
        let snapshot = |w: &World| {
            (
                w.terrain_of(&test_map()).cloned().unwrap(),
                w.distances.places.clone(),
                w.place_geometry.clone(),
            )
        };
        let original = snapshot(&w);

        let cmd = RegenerateBiomeMarkerCommand::new(id, 9);
        cmd.apply(&mut w).expect("首次 apply");
        cmd.revert(&mut w).expect("首次 revert");
        assert_eq!(snapshot(&w), original, "第一次回环后应逐位还原");
        cmd.apply(&mut w).expect("redo（同实例再次 apply）");
        cmd.revert(&mut w).expect("redo 后再 revert");
        assert_eq!(snapshot(&w), original, "redo 回环后仍应逐位还原");
    }

    /// owner 名下多条旧 span 与外部 span 交错：revert 按原索引升序回插，
    /// 全表顺序必须与 apply 前一致。
    #[test]
    fn regenerate_revert_restores_interleaved_spans_in_original_order() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(8.0, 8.0), (32.0, 8.0), (32.0, 32.0), (8.0, 32.0)],
        );
        let mk_span = |owner: &str, from: i64| GeometrySpan {
            map: test_map(),
            owner: Id::new_unchecked(owner),
            span: wv_core::time::TimeSpan { from, to: None },
            geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
            relation: None,
        };
        // 重排成 [外部A, marker(5), 外部B, marker 旧史 span(2)] —— owner 与
        // 外部交错，回插顺序是本命令最精巧的不变式。
        let marker_span = w.place_geometry.remove(0);
        w.place_geometry = vec![
            mk_span("place_other_a", 1),
            marker_span,
            mk_span("place_other_b", 3),
            mk_span(id.as_str(), 2),
        ];
        let before = w.place_geometry.clone();

        let cmd = RegenerateBiomeMarkerCommand::new(id.clone(), 9);
        cmd.apply(&mut w).expect("重生成应替换两条 owner span");
        let owner_spans: Vec<&GeometrySpan> = w
            .place_geometry
            .iter()
            .filter(|g| g.owner.as_ref() == id.as_str())
            .collect();
        assert_eq!(
            owner_spans.len(),
            1,
            "重生成后 owner 名下应只剩 1 条新 span，实得 {}",
            owner_spans.len()
        );
        assert_eq!(owner_spans[0].span.from, 9);
        cmd.revert(&mut w).expect("revert");
        assert_eq!(
            w.place_geometry, before,
            "revert 应把交错的多条旧 span 按原顺序全表还原"
        );
    }

    /// 手改 YAML 可能留下畸形包络（奇数长度 / 少于 3 点）→ 人话错误、零改动。
    #[test]
    fn regenerate_rejects_malformed_envelope_flat_array() {
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let mut p = marker_place("place_bad", "坏包络", 1);
        p.map = Some(test_map());
        w.distances.places.push(p);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();

        for bad in [vec![1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0, 5.0]] {
            w.distances.places[0].envelope = Some(bad.clone());
            let err = RegenerateBiomeMarkerCommand::new(Id::new_unchecked("place_bad"), 9)
                .apply(&mut w)
                .unwrap_err();
            assert!(matches!(err, EditError::Precondition(_)), "实得 {err:?}");
        }
        assert_eq!(w.terrain_of(&test_map()).cloned().unwrap(), terrain_before);
        assert!(w.place_geometry.is_empty(), "畸形包络不得写入任何 span");
    }

    #[test]
    fn regenerate_stores_holes_as_polygon_with_holes() {
        // 生成时是实心林块；后来中间挖出湖 → 重生成识别分支命中掩膜成环 → 带洞。
        let mut w = land_world(64);
        w.maps.push(map_with_extent(TEST_MAP, Some((64.0, 64.0))));
        let id = create_marker(
            &mut w,
            "place_dark_forest",
            1.0,
            vec![(16.0, 16.0), (48.0, 16.0), (48.0, 48.0), (16.0, 48.0)],
        );
        let mut data = w.terrain_of(&test_map()).cloned().unwrap();
        for y in 24..40 {
            for x in 24..40 {
                data.cells[y * 64 + x] = TerrainCellData {
                    height: -20,
                    surface: 0,
                    biome: 0,
                };
            }
        }
        w.set_terrain(test_map(), data);
        let terrain_before = w.terrain_of(&test_map()).cloned().unwrap();
        RegenerateBiomeMarkerCommand::new(id.clone(), 9)
            .apply(&mut w)
            .expect("重生成应成功");
        assert_eq!(
            w.terrain_of(&test_map()).cloned().unwrap(),
            terrain_before,
            "识别分支不得改动地形"
        );
        let span = w
            .place_geometry
            .iter()
            .find(|g| g.owner == id)
            .expect("span 应存在");
        let GeometryData::PolygonWithHoles { holes, .. } = &span.geometry else {
            panic!("湖芯应使重生成边界带洞，实得 {:?}", span.geometry);
        };
        assert!(!holes.is_empty(), "应至少一个洞环，实得 {holes:?}");
    }

    #[test]
    fn update_place_territory_replaces_ring_and_resyncs_anchors() {
        let mut w = World::default();
        let old_ring = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let mut place = span_fixture_place("place_forest");
        place.fit_anchor_to_ring(&old_ring);
        w.distances.places.push(place);
        w.place_geometry.push(territory(
            TEST_MAP,
            "place_forest",
            0,
            None,
            old_ring.clone(),
        ));
        let old_place = w.distances.places[0].clone();

        let new_ring = vec![0.0, 0.0, 20.0, 0.0, 20.0, 10.0, 0.0, 10.0];
        let cmd = UpdatePlaceTerritoryCommand::new(
            Id::new_unchecked("place_forest"),
            test_map(),
            0,
            new_ring.clone(),
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        assert_eq!(
            dirty,
            vec!["distances/places".to_string(), "geometry/place".to_string()],
            "脏路径应同时覆盖地点表与地界目录"
        );
        assert_eq!(w.place_geometry.len(), 1, "改地界不得追加 span");
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(new_ring),
            "地界应替换为新环"
        );
        let p = &w.distances.places[0];
        assert_eq!(p.x, Some(10.0), "新质心 X 应为 10，实际 {:?}", p.x);
        assert_eq!(p.y, Some(5.0), "新质心 Y 应为 5，实际 {:?}", p.y);
        assert_eq!(p.w, Some(20.0), "新宽应为 20，实际 {:?}", p.w);
        assert_eq!(p.h, Some(10.0), "新高应为 10，实际 {:?}", p.h);

        cmd.revert(&mut w).expect("revert");
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(old_ring),
            "revert 应恢复旧环"
        );
        assert_eq!(
            w.distances.places[0], old_place,
            "revert 应恢复 Place 全字段（含旧锚点 x/y/w/h）"
        );
    }

    #[test]
    fn update_place_territory_missing_span_is_an_error() {
        let mut w = World::default();
        let mut place = span_fixture_place("place_forest");
        place.fit_anchor_to_ring(&[0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]);
        w.distances.places.push(place);
        w.place_geometry.push(territory(
            TEST_MAP,
            "place_forest",
            0,
            None,
            vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0],
        ));

        let cmd = UpdatePlaceTerritoryCommand::new(
            Id::new_unchecked("place_forest"),
            test_map(),
            99,
            vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "不存在的 span 应返回 TargetMissing"
        );
        assert_eq!(
            w.place_geometry[0].geometry,
            GeometryData::Polygon(vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]),
            "失败的 apply 不应改动地界"
        );
        assert_eq!(
            w.distances.places[0].x,
            Some(5.0),
            "失败的 apply 不应改动锚点"
        );
    }

    /// ADR-0037 D4：重生成命令必须可撤销——地形与模板记录一起还原。
    #[test]
    fn regen_world_command_restores_terrain_and_template_on_revert() {
        use wv_core::WorldTemplateKind;

        let map = wv_core::Id::new_unchecked("world");
        let old_cells = vec![
            TerrainCellData {
                height: -100,
                surface: 0,
                biome: 0,
            };
            4
        ];
        let new_cells = vec![
            TerrainCellData {
                height: 320,
                surface: 1,
                biome: 3,
            };
            4
        ];
        let mut world = World {
            meta: Some(wv_core::WorldMeta {
                name: "t".into(),
                version: "0.1".into(),
                schema_version: 2,
                root_map: map.clone(),
                timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
                template: Some(wv_core::WorldTemplate {
                    kind: WorldTemplateKind::Ocean,
                    seed: 1,
                    rivers_seed: 2,
                }),
            }),
            ..World::default()
        };
        world.set_terrain(
            map.clone(),
            TerrainData {
                width: 2,
                height: 2,
                cells: old_cells.clone(),
            },
        );

        let cmd = RegenWorldTerrainCommand::new(
            map.clone(),
            TerrainData {
                width: 2,
                height: 2,
                cells: new_cells.clone(),
            },
            wv_core::WorldTemplate {
                kind: WorldTemplateKind::ColdNorth,
                seed: 7,
                rivers_seed: 8,
            },
        );
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec!["maps/world/terrain.bin".to_owned()]);
        assert_eq!(
            world.terrain_of(&map).map(|t| t.cells.clone()),
            Some(new_cells),
            "apply 后必须是新栅格"
        );
        let t = world
            .meta
            .as_ref()
            .and_then(|m| m.template)
            .expect("模板记录");
        assert_eq!(t.seed, 7);

        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.terrain_of(&map).map(|t| t.cells.clone()),
            Some(old_cells),
            "revert 后必须逐格还原"
        );
        let t = world
            .meta
            .as_ref()
            .and_then(|m| m.template)
            .expect("模板记录");
        assert_eq!(t.seed, 1, "模板记录也必须还原");
    }
}
