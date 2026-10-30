//! [`AddRidgeDetailCommand`]（ADR-0092 §9 B1）：apply 改陆地高度、revert
//! 还原、同图 seed 确定性。

use super::*;
use crate::command::AddRidgeDetailCommand;
use wv_core::terrain::{TerrainCellData, TerrainData};

fn world_with_flat_land() -> World {
    let mut world = World::default();
    world.maps.push(wv_core::map::MapNode {
        id: test_map(),
        ..wv_core::map::MapNode::default()
    });
    let land = vec![
        TerrainCellData {
            height: 2000,
            surface: 1,
            biome: 0,
        };
        16 * 16
    ];
    world.set_terrain(
        test_map(),
        TerrainData {
            width: 16,
            height: 16,
            cells: land,
        },
    );
    world
}

fn heights(world: &World) -> Vec<i16> {
    world
        .terrain_of(&test_map())
        .unwrap()
        .cells
        .iter()
        .map(|c| c.height)
        .collect()
}

#[test]
fn ridge_command_changes_land_then_revert_restores() {
    let mut world = world_with_flat_land();
    let original = heights(&world);

    let cmd = AddRidgeDetailCommand::new(test_map());
    cmd.apply(&mut world).unwrap();

    let after = heights(&world);
    assert_ne!(after, original, "apply 应改变陆地高度");
    // 幅度上界（t = 2000/2500 = 0.8）：[2000−352, 2000+528]。
    for h in &after {
        assert!((1648..=2529).contains(h), "越界: {h}");
    }

    cmd.revert(&mut world).unwrap();
    assert_eq!(heights(&world), original, "revert 应逐格还原");

    // 同 seed 重放：与第一次 apply 结果一致（确定性）。
    cmd.apply(&mut world).unwrap();
    assert_eq!(heights(&world), after, "同 seed 重放必须逐格相同");
}

#[test]
fn ridge_command_seed_derives_from_map_id() {
    let a = AddRidgeDetailCommand::new(test_map()).seed;
    let b = AddRidgeDetailCommand::new(test_map()).seed;
    assert_eq!(a, b, "同图 seed 稳定");
}
