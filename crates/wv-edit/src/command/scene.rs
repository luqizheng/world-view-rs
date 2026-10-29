//! scene 命令族（2026-10-29 · 场景编辑器 M1）。
//!
//! 场景图（City / Interior 级）的**基底网格**铺设命令。
//!
//! ## 为什么需要它
//!
//! 场景分区走每图一张的地形栅格（biome 通道，见 `wv_terrain::biome` 的
//! 分区保留带 21..=32）。但一张新建的城市图**没有自己的栅格**——而笔刷
//! 要求「这张图有栅格」才生效（`PaintTerrainCommand` 查不到就报
//! `TerrainMismatch`）。
//!
//! 作者的心智是「进图就画」，不是「先做一���隐藏的技术准备」。故本命令
//! 在**首次刷色**时自动补一张基底网格（全「未分区」分区码），让第一笔
//! 直接落在可画的东西上。
//!
//! ## 尺寸口径
//!
//! 格数 = 图幅 extent ÷ 格边长。extent 缺省或非正 → 256×256 兜底；
//! 格边长非正 → 8.0 兜底。兜底不是「随便定个值」，是「别让作者卡住」——
//! 尺寸随时可以用后续编辑调整。

use super::*;
use wv_terrain::biome::DISTRICT_FIRST;

/// 铺一张场景基底网格的命令。
///
/// ## 幂等与前置
///
/// - 目标图不存在 → `TargetMissing`；
/// - 目标是世界图 → `Precondition`（世界图的地形由大陆生成器负责，
///   不该被「场景基底」这条路径铺满）；
/// - 目标图已有栅格 → `SceneGridExists`（不静默覆盖——那等于悄悄毁掉
///   作者已经画好的地形）。
#[derive(Debug)]
pub struct CreateSceneBaseGridCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 每格世界边长（wu/格）。
    pub cell_size: f32,
    /// 本次是否真的写入了栅格（供 `revert` 判断该不该删）。
    created: std::sync::Mutex<bool>,
}

impl CreateSceneBaseGridCommand {
    pub fn new(map: wv_core::Id, cell_size: f32) -> Self {
        Self {
            map,
            cell_size,
            created: std::sync::Mutex::new(false),
        }
    }

    /// 目标图的格数（宽, 高）：extent ÷ 格边长，带兜底。
    fn grid_dims(&self, world: &World) -> (u32, u32) {
        const FALLBACK_CELLS: u32 = 256;
        let cell_size = if self.cell_size > 0.0 { self.cell_size } else { 8.0 };
        let extent = world
            .maps
            .iter()
            .find(|m| m.id == self.map)
            .and_then(|m| m.extent.as_ref());
        let (w, h) = match extent {
            Some(e) if e.width > 0.0 && e.height > 0.0 => (e.width, e.height),
            _ => return (FALLBACK_CELLS, FALLBACK_CELLS),
        };
        (
            (w / cell_size).round().max(1.0) as u32,
            (h / cell_size).round().max(1.0) as u32,
        )
    }
}

impl EditCommand for CreateSceneBaseGridCommand {
    fn label(&self) -> String {
        "铺设场景基底网格".to_string()
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let Some(node) = world.maps.iter().find(|m| m.id == self.map) else {
            return Err(EditError::TargetMissing(format!("地图 `{}`", self.map)));
        };
        if node.level == wv_core::map::MapLevel::World {
            return Err(EditError::Precondition(format!(
                "地图 `{}` 是世界图，地形不该由场景基底铺设",
                self.map
            )));
        }
        if world.terrain_of(&self.map).is_some() {
            return Err(EditError::SceneGridExists(format!(
                "地图 `{}` 已有地形网格",
                self.map
            )));
        }
        let (w, h) = self.grid_dims(world);
        let mut grid = TerrainGrid::new(w, h);
        // 全格铺「未分区」分区码 + 陆面 + 零海拔：场景图是平底，分区是
        // 在这张平底上涂的功能块。
        let base = TerrainCell {
            height: 0,
            surface: wv_terrain::SURFACE_LAND,
            biome: DISTRICT_FIRST,
        };
        for i in 0..grid.cells().len() {
            grid.set((i as u32 % w) as i32, (i as u32 / w) as i32, base);
        }
        world.set_terrain(self.map.clone(), from_grid(grid));
        if let Ok(mut c) = self.created.lock() {
            *c = true;
        }
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let created = self.created.lock().map(|c| *c).unwrap_or(false);
        if created {
            world.remove_terrain(&self.map);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::{Extent, MapLevel, MapNode};

    fn city_node(id: &str) -> MapNode {
        MapNode {
            id: wv_core::Id::new_unchecked(id),
            name: "城".into(),
            level: MapLevel::City,
            extent: Some(Extent::sized(1024.0, 1024.0)),
            ..MapNode::default()
        }
    }

    #[test]
    fn scene_base_grid_fills_flat_unzoned_cells_from_extent() {
        let mut world = World::default();
        world.maps.push(city_node("city_a"));
        let cmd = CreateSceneBaseGridCommand::new(wv_core::Id::new_unchecked("city_a"), 8.0);
        let dirty = cmd.apply(&mut world).unwrap();
        let g = world
            .terrain_of(&wv_core::Id::new_unchecked("city_a"))
            .unwrap();
        assert_eq!((g.width, g.height), (128, 128), "1024/8=128");
        assert!(
            g.cells.iter().all(|c| c.height == 0
                && c.biome == DISTRICT_FIRST
                && c.surface == wv_terrain::SURFACE_LAND),
            "全格应是 height=0 / biome=未分区 / surface=陆"
        );
        assert_eq!(dirty, vec!["maps/city_a/terrain.bin".to_string()]);
        cmd.revert(&mut world).unwrap();
        assert!(
            world.terrain_of(&wv_core::Id::new_unchecked("city_a")).is_none(),
            "revert 必须移除本次网格"
        );
    }

    #[test]
    fn scene_base_grid_refuses_world_level_and_existing_grid() {
        let mut world = World::default();
        world.maps.push(MapNode {
            level: MapLevel::World,
            ..city_node("world_main")
        });
        let cmd = CreateSceneBaseGridCommand::new(wv_core::Id::new_unchecked("world_main"), 8.0);
        assert!(cmd.apply(&mut world).is_err(), "世界图不该铺场景基底");
        assert!(world.terrain_of(&wv_core::Id::new_unchecked("world_main")).is_none());

        // 已有栅格 → 拒绝，不覆盖作者画好的地形。
        let mut world = World::default();
        world.maps.push(city_node("city_b"));
        world.set_terrain(
            wv_core::Id::new_unchecked("city_b"),
            TerrainData {
                width: 2,
                height: 2,
                cells: vec![TerrainCellData::default(); 4],
            },
        );
        let cmd = CreateSceneBaseGridCommand::new(wv_core::Id::new_unchecked("city_b"), 8.0);
        assert!(matches!(cmd.apply(&mut world), Err(EditError::SceneGridExists(_))));
        assert_eq!(world.terrain_of(&wv_core::Id::new_unchecked("city_b")).unwrap().width, 2);
    }

    #[test]
    fn scene_base_grid_falls_back_to_256_cells_without_extent() {
        let mut world = World::default();
        world.maps.push(MapNode {
            extent: None,
            ..city_node("city_c")
        });
        let cmd = CreateSceneBaseGridCommand::new(wv_core::Id::new_unchecked("city_c"), 8.0);
        cmd.apply(&mut world).unwrap();
        let g = world.terrain_of(&wv_core::Id::new_unchecked("city_c")).unwrap();
        assert_eq!((g.width, g.height), (256, 256), "无 extent 时兜底 256×256");
    }
}
