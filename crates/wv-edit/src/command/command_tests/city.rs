//! 城市增删/轮廓（fixture：city_node/rect_polygon）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

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
