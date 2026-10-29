//! 大陆生成 + 群系标记（fixture：continent_envelope/span_fixture_place/territory）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

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
            comment: None,
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
