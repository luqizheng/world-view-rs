//! 街道段与建筑增删（fixture：seg/sample_building）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

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
