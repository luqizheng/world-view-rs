//! 标记重生成 + 更新领地 + RegenWorld（fixture：create_marker/set_biome_block）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

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
                comment: None,
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

    // ── UpdateMapPropsCommand（地图属性对话框写回）────────────────────
