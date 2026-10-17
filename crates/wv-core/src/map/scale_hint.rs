//! 陆地尺度换算：世界单位 → km（README §12-39 只读提示的数据底座）。
//!
//! ## 职责
//! - [`land_extent_km`]：当前图 `height > 0` 格的 bbox → km（宽 / 高 / 对角线）。
//!
//! ## 边界（不做什么）
//! - **不**算行程时间：天数一律由 `MapNode::km_per_wu` 的下游与
//!   `DistanceGraph::segment_days` 的唯一入口给出（AGENTS §4.2）。
//! - **不**写世界：纯只读投影，供校验面板展示。
//! - 水下格（`height <= 0`）不计入陆地 bbox——负高度岛屿在尺度提示里是「海」。

use crate::{Id, World};

/// 陆地近似尺度（km）。bbox 是**近似**：真实海岸线不是矩形，面板文案用
/// 「约」字（V1：只提示不评估）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandExtentKm {
    /// 宽度（东西向）。
    pub width_km: f32,
    /// 高度（南北向）。
    pub height_km: f32,
    /// 对角线（bbox 对角，作「横穿」的粗估）。
    pub diagonal_km: f32,
}

/// 世界单位 → km：`km = wu / unit_per_km`（ADR-0022 D4 口径）；
/// `unit_per_km` 语义与 `MapNode::km_per_wu` 的 `1 / per_km` 一致。
///
/// 任一前提缺失（无图 / 无地形 / 无 `unit_per_km` / 无陆地 / 非正 cell_size）
/// 返回 `None`——面板静默不显示（缺数据是正常态，V1）。
pub fn land_extent_km(world: &World, map: &Id, cell_size_wu: f32) -> Option<LandExtentKm> {
    if !cell_size_wu.is_finite() || cell_size_wu <= 0.0 {
        return None;
    }
    let node = world.maps.iter().find(|m| &m.id == map)?;
    let per_km = node.unit_per_km?;
    if per_km <= 0.0 {
        return None;
    }
    let data = world.terrain_of(map)?;

    let mut min = (u32::MAX, u32::MAX);
    let mut max = (0u32, 0u32);
    let mut any = false;
    for (idx, cell) in data.cells.iter().enumerate() {
        if cell.height <= 0 || data.width == 0 {
            continue;
        }
        let x = (idx as u32) % data.width;
        let y = (idx as u32) / data.width;
        any = true;
        if x < min.0 {
            min.0 = x;
        }
        if y < min.1 {
            min.1 = y;
        }
        if x > max.0 {
            max.0 = x;
        }
        if y > max.1 {
            max.1 = y;
        }
    }
    if !any {
        return None;
    }

    // 20 个连续陆格 → 物理宽 20 × cell（左缘到右缘），不是中心距 19 × cell。
    let cells_w = (max.0 - min.0 + 1) as f32;
    let cells_h = (max.1 - min.1 + 1) as f32;
    let width_km = cells_w * cell_size_wu / per_km;
    let height_km = cells_h * cell_size_wu / per_km;
    Some(LandExtentKm {
        width_km,
        height_km,
        diagonal_km: (width_km * width_km + height_km * height_km).sqrt(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{MapLevel, MapNode};
    use crate::terrain::{TerrainCellData, TerrainData};

    fn node_with_scale(id: &str, unit_per_km: Option<f32>) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: id.into(),
            level: MapLevel::World,
            unit_per_km,
            ..Default::default()
        }
    }

    fn world_with_land(width: u32, height: u32, land_w: u32, land_h: u32) -> World {
        let mut world = World::default();
        world.maps.push(node_with_scale("map_world", Some(100.0)));
        let mut cells = vec![
            TerrainCellData {
                height: -3500,
                surface: 0,
                biome: 0,
            };
            (width * height) as usize
        ];
        for y in 0..land_h {
            for x in 0..land_w {
                cells[(y * width + x) as usize] = TerrainCellData {
                    height: 500,
                    surface: 1,
                    biome: 3,
                };
            }
        }
        world.set_terrain(
            Id::new_unchecked("map_world"),
            TerrainData {
                width,
                height,
                cells,
            },
        );
        world
    }

    #[test]
    fn land_extent_converts_cells_to_km_with_hand_computed_values() {
        // 20×10 陆格、cell 8 wu、unit_per_km 100：
        // 宽 = 20×8/100 = 1.6 km；高 = 10×8/100 = 0.8 km。
        let world = world_with_land(40, 40, 20, 10);
        let map = Id::new_unchecked("map_world");
        let e = land_extent_km(&world, &map, 8.0).expect("有陆地且有 unit_per_km");
        assert!(
            (e.width_km - 1.6).abs() < 1e-4,
            "宽应 1.6 km，得 {}",
            e.width_km
        );
        assert!(
            (e.height_km - 0.8).abs() < 1e-4,
            "高应 0.8 km，得 {}",
            e.height_km
        );
        let want = (1.6f32 * 1.6 + 0.8 * 0.8).sqrt();
        assert!(
            (e.diagonal_km - want).abs() < 1e-4,
            "对角线应 {want}，得 {}",
            e.diagonal_km
        );
    }

    #[test]
    fn all_ocean_map_returns_none() {
        let mut world = world_with_land(8, 8, 0, 0);
        // 负高度岛格仍算海：不改 height ≤ 0 的判定。
        let map = Id::new_unchecked("map_world");
        assert_eq!(land_extent_km(&world, &map, 8.0), None, "全海应返回 None");

        let terrain = world.terrain_of(&map).cloned().expect("地形存在");
        let mut cells = terrain.cells;
        cells[0] = TerrainCellData {
            height: -120,
            surface: 1,
            biome: 3,
        };
        world.set_terrain(
            map.clone(),
            TerrainData {
                width: 8,
                height: 8,
                cells,
            },
        );
        assert_eq!(
            land_extent_km(&world, &map, 8.0),
            None,
            "负高度岛屿不计入陆地 bbox"
        );
    }

    #[test]
    fn missing_unit_per_km_returns_none() {
        let mut world = world_with_land(40, 40, 20, 10);
        world.maps[0].unit_per_km = None;
        let map = Id::new_unchecked("map_world");
        assert_eq!(
            land_extent_km(&world, &map, 8.0),
            None,
            "无 unit_per_km 应静默 None"
        );
        assert_eq!(
            land_extent_km(&world, &map, 0.0),
            None,
            "非正 cell_size 应静默 None"
        );
    }
}
