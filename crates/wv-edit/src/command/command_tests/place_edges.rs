//! 地点关系边/级联删除（fixture：edge/place/world_with_places_and_edges）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

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

    // --- DeletePlaceCommand 测试（地标面板，ADR-0071） --------------------

    fn place(id: &str) -> wv_core::distance::Place {
        wv_core::distance::Place {
            kind: None,
            line: None,
            line_kind: None,
            line_width_m: None,
            id: wv_core::Id::new_unchecked(id),
            name: format!("地点{id}"),
            feature: None,
            biome: None,
            envelope: None,
            map: None,
            x: Some(1.0),
            y: Some(2.0),
            w: None,
            h: None,
            rank: None,
        }
    }

    fn world_with_places_and_edges() -> World {
        let mut w = World::default();
        w.distances.places = vec![place("place_a"), place("place_b"), place("place_c")];
        // edge_1/edge_3 端点含 place_b（级联测试用）；edge_2 与 place_b 无关。
        // place_a 参与 edge_1/edge_2；place_c 参与 edge_2/edge_3。
        w.distances.edges = vec![
            edge("edge_1", "place_b", "place_a"),
            edge("edge_2", "place_a", "place_c"),
            edge("edge_3", "place_c", "place_b"),
        ];
        w
    }

    #[test]
    fn delete_place_removes_the_place_and_marks_dirty() {
        let mut w = world_with_places_and_edges();
        let cmd = DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_a"));
        let dirty = cmd.apply(&mut w).expect("apply");
        // place_a 参与 edge_1/edge_2 → 级联命中，两类脏路径都应出现。
        assert_eq!(
            dirty,
            vec![
                "distances/places".to_string(),
                "distances/edges".to_string()
            ]
        );
        assert_eq!(w.distances.places.len(), 2);
        assert!(
            !w.distances
                .places
                .iter()
                .any(|p| p.id.as_str() == "place_a"),
            "place_a 应已被移除"
        );
        let ids: Vec<_> = w.distances.edges.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["edge_3"], "仅 edge_3（不含 place_a）应保留");
    }

    #[test]
    fn delete_place_cascades_incident_edges_and_keeps_others() {
        let mut w = world_with_places_and_edges();
        DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_b"))
            .apply(&mut w)
            .expect("apply");
        // edge_1（b-a）与 edge_3（c-b）端点含 place_b，应级联消失；
        // edge_2（a-c）与 place_b 无关，应保留。
        let ids: Vec<_> = w.distances.edges.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["edge_2"], "仅与 place_b 无关的 edge_2 应保留");
    }

    #[test]
    fn delete_place_revert_restores_place_and_edges_in_original_order() {
        let mut w = world_with_places_and_edges();
        let cmd = DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_b"));
        cmd.apply(&mut w).expect("apply");
        cmd.revert(&mut w).expect("revert");
        // place 顺序恢复：b 在原索引 1。
        let place_ids: Vec<_> = w.distances.places.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(place_ids, vec!["place_a", "place_b", "place_c"]);
        // 边顺序恢复：e1、e3 回到原槽位，e2 仍夹在中间。
        let edge_ids: Vec<_> = w.distances.edges.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(edge_ids, vec!["edge_1", "edge_2", "edge_3"]);
    }

    #[test]
    fn delete_place_reapply_after_undo_deletes_again() {
        let mut w = world_with_places_and_edges();
        let cmd = DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_b"));
        cmd.apply(&mut w).expect("first apply");
        cmd.revert(&mut w).expect("revert");
        cmd.apply(&mut w).expect("re-apply");
        assert_eq!(w.distances.places.len(), 2);
        assert_eq!(w.distances.edges.len(), 1, "级联边应再次被清");
    }

    #[test]
    fn delete_place_rejects_missing_place() {
        let mut w = world_with_places_and_edges();
        let err = DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_missing"))
            .apply(&mut w)
            .expect_err("不存在的地点必须报错");
        assert!(matches!(err, EditError::Precondition(_)));
        assert_eq!(w.distances.places.len(), 3, "失败不应改动世界");
    }

    #[test]
    fn delete_place_reports_edges_dirty_only_when_cascade_hit() {
        // 删一个没有任何边的点：脏路径只有 places，不该顺带重写 edges。
        let mut w = world_with_places_and_edges();
        let dirty = DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_c"))
            .apply(&mut w)
            .expect("apply");
        // place_c 在 edge_3（c-b）里——先删掉那条边再测纯无边情形。
        assert!(dirty.contains(&"distances/edges".to_string()));
        let mut w2 = World::default();
        w2.distances.places = vec![place("lonely")];
        let dirty2 = DeletePlaceCommand::new(wv_core::Id::new_unchecked("lonely"))
            .apply(&mut w2)
            .expect("apply");
        assert_eq!(dirty2, vec!["distances/places".to_string()]);
    }

    #[test]
    fn delete_place_revert_without_apply_fails() {
        let mut w = World::default();
        let cmd = DeletePlaceCommand::new(wv_core::Id::new_unchecked("place_a"));
        let err = cmd.revert(&mut w).expect_err("revert without apply");
        assert!(matches!(err, EditError::Precondition(_)));
    }

    // ---- 大陆生成宏命令（大陆生成 UI plan Task 1） ----
