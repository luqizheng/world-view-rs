//! 地图属性改名/比例尺。（P1-5 拆分，2026-10-29；公共夹具经 `use super::*` 取自 mod.rs）
use super::*;

fn world_with_named_map() -> World {
    use wv_core::map::MapNode;
    let mut world = World::default();
    world.maps.push(MapNode {
        id: test_map(),
        name: "演示大陆".into(),
        unit_per_km: Some(4.0),
        ..MapNode::default()
    });
    world
}

#[test]
fn update_map_props_renames_and_sets_scale_with_index_dirty() {
    let mut world = world_with_named_map();
    let cmd = UpdateMapPropsCommand::new(test_map(), "新名字".into(), Some(100.0));

    let dirty = cmd.apply(&mut world).expect("apply");
    assert_eq!(
        dirty,
        vec![format!("maps/{TEST_MAP}/index.yaml")],
        "节点字段内联在图索引，脏路径必须是 index.yaml"
    );
    assert_eq!(world.maps[0].name, "新名字");
    assert_eq!(world.maps[0].unit_per_km, Some(100.0));

    cmd.revert(&mut world).expect("revert");
    assert_eq!(world.maps[0].name, "演示大陆", "revert 必须还原名字");
    assert_eq!(
        world.maps[0].unit_per_km,
        Some(4.0),
        "revert 必须还原尺度声明"
    );
}

#[test]
fn update_map_props_can_clear_scale_declaration_and_revert_restores_it() {
    let mut world = world_with_named_map();
    // 传 None = 未声明（比例尺静默不显示）。
    let cmd = UpdateMapPropsCommand::new(test_map(), "演示大陆".into(), None);
    cmd.apply(&mut world).expect("apply");
    assert_eq!(world.maps[0].unit_per_km, None, "None 应清掉声明");
    cmd.revert(&mut world).expect("revert");
    assert_eq!(world.maps[0].unit_per_km, Some(4.0), "revert 应还原声明");
}

#[test]
fn update_map_props_missing_map_errors_and_changes_nothing() {
    let mut world = world_with_named_map();
    let cmd = UpdateMapPropsCommand::new(Id::new_unchecked("map_nope"), "x".into(), None);
    let err = cmd.apply(&mut world).expect_err("missing must fail");
    assert!(matches!(err, EditError::TargetMissing(_)), "got {err:?}");
    assert_eq!(world.maps[0].name, "演示大陆", "失败不得改动任何字段");
}
