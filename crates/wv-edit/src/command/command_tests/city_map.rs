//! 城市平移 + 地图新建/根图（fixture：map_node/flat_terrain）。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

    fn move_city_translates_holed_outline_including_holes() {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let mut w = World::default();
        w.maps.push(MapNode {
            comment: None,
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
            trashed: false,
            trashed_at: None,
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
            comment: None,
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
            trashed: false,
            trashed_at: None,
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
            comment: None,
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
            trashed: false,
            trashed_at: None,
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
