//! 街区命令（2026-11-06 城市街区工具设计 D2–D4）：框选生成 / 调参重摇 /
//! 清空重摇共用一个事务。
//!
//! ## 语义（设计 D3，用户拍板）
//!
//! [`UpsertDistrictCommand`] 一个事务同时做三件事：
//! 1. upsert 街区本体（参数 + seed）到 `World.city_blocks`；
//! 2. **清空该区 ring 内全部建筑（含作者手摆的）**；
//! 3. 按 (参数, seed) 重新生成建筑落 `World.buildings`。
//!
//! 无出身标记、不追问——误重摇靠 undo 兜底（revert 恢复整份建筑文档与
//! 街区旧值，逐位还原）。
//!
//! ## 图层自动补建（评审 Minor 定夺）
//!
//! 目标图没有 Building / District 图层时，apply 前由调用方用
//! [`ensure_district_layers`] 补建并挂 `MapNode.layers`——命令本身不碰
//! `MapNode`（编辑命令只改文档，图层是结构声明）。

use wv_core::source::district::{generate_buildings, District, DistrictParams};
use wv_core::{Id, World};

/// 生成命令的落盘脏路径：街区文档 + 建筑文档两条虚拟命名空间。
pub fn district_dirty_paths(district_layer: &Id, building_layer: &Id) -> Vec<String> {
    vec![
        format!("city_blocks/{}", district_layer.as_str()),
        format!("buildings/{}", building_layer.as_str()),
    ]
}

/// 确保目标图有 District + Building 两枚图层（缺则补建），返回两者的 Id。
///
/// - 已有同名 kind 图层 → 沿用（首个命中）；
/// - 缺 → 新建（id = `<map>_districts` / `<map>_buildings`，source 指
///   `maps/<map>/districts.yaml` / `buildings.yaml`——与 buildings 文件
///   落点惯例同款）。
///
/// 返回 `(district_layer, building_layer)`。
pub fn ensure_district_layers(world: &mut World, map: &Id) -> (Id, Id) {
    let dl_id = Id::new_unchecked(&format!("{}_districts", map.as_str()));
    let bl_id = Id::new_unchecked(&format!("{}_buildings", map.as_str()));
    let Some(node) = world.maps.iter_mut().find(|m| &m.id == map) else {
        return (dl_id, bl_id); // 图不存在：返回约定 id，命令 apply 时会因
                               // 图层缺失找不到 owner 而自然不落盘（V1）。
    };
    let has_district = node
        .layers
        .iter()
        .any(|l| l.kind == wv_core::map::LayerKind::District);
    let has_building = node
        .layers
        .iter()
        .any(|l| l.kind == wv_core::map::LayerKind::Building);
    if !has_building {
        node.layers.push(wv_core::map::Layer {
            id: bl_id.clone(),
            name: "建筑".into(),
            kind: wv_core::map::LayerKind::Building,
            visible: true,
            locked: false,
            opacity: 1.0,
            z: 30,
            source: Some(format!("maps/{}/buildings.yaml", map.as_str())),
        });
    }
    if !has_district {
        node.layers.push(wv_core::map::Layer {
            id: dl_id.clone(),
            name: "街区".into(),
            kind: wv_core::map::LayerKind::District,
            visible: true,
            locked: false,
            opacity: 1.0,
            z: 25,
            source: Some(format!("maps/{}/districts.yaml", map.as_str())),
        });
    }
    (dl_id, bl_id)
}

/// 框选生成 / 调参重摇 / 清空重摇的**单一事务命令**（设计 D3）。
///
/// `params = None` 表示沿用区里已有的参数（「清空重摇」= density=0 的
/// 显式参数；「重摇换布局」= 换 seed 的显式 seed）。`district.seed` 是
/// **权威 seed**——调用方改 seed 就传新 district。
#[derive(Debug)]
pub struct UpsertDistrictCommand {
    /// 街区图层 ID（`World.city_blocks` 的键）。
    pub district_layer: Id,
    /// 派生建筑落进的建筑图层 ID（`World.buildings` 的键）。
    pub building_layer: Id,
    /// 本图的街道折线快照（生成器沿街判定用；本图局部坐标）。
    pub streets: Vec<Vec<wv_core::base::geometry::Point>>,
    /// 要落位的街区（新框选 = 全新 id；重摇 = 沿用旧 id + 新参数/seed）。
    pub district: District,
    /// `apply` 时缓存的建筑文档快照：revert 整份还原。
    before_buildings: std::sync::Mutex<Option<wv_core::BuildingLayer>>,
    /// `apply` 时缓存的街区旧值（首次创建为 `None`）。
    before_district: std::sync::Mutex<Option<Option<District>>>,
    /// `apply` 时缓存的**本命令实际落位的街区**（revert 后丢弃生成结果用）。
    applied: std::sync::Mutex<Option<District>>,
}

impl UpsertDistrictCommand {
    pub fn new(
        district_layer: Id,
        building_layer: Id,
        streets: Vec<Vec<wv_core::base::geometry::Point>>,
        district: District,
    ) -> Self {
        Self {
            district_layer,
            building_layer,
            streets,
            district,
            before_buildings: std::sync::Mutex::new(None),
            before_district: std::sync::Mutex::new(None),
            applied: std::sync::Mutex::new(None),
        }
    }

    /// ring 内建筑清空 + 按 (params, seed) 重生成（D3 核心语义）。
    ///
    /// 抽出来供 apply / 测试共用；`world` 只读入参、结果写回调用方。
    fn regenerate(world: &mut World, cmd: &Self, district: &District) {
        let params: DistrictParams = district.params.clone();
        let generated = generate_buildings(&district.ring, &params, district.seed, &cmd.streets);
        let mut doc = world
            .building_of(&cmd.building_layer)
            .cloned()
            .unwrap_or_default();
        // D3：清空 ring 内**全部**建筑（质心在环内即算——与生成器的放置
        // 判据同一口径，凹环同样成立），再追加生成结果。
        doc.buildings
            .retain(|b| !b.centroid().is_some_and(|c| in_ring(&c, &district.ring)));
        doc.buildings.extend(generated);
        world.set_buildings(cmd.building_layer.clone(), doc);
    }
}

fn in_ring(p: &wv_core::base::geometry::Point, ring: &[wv_core::base::geometry::Point]) -> bool {
    // 与 wv_core::source::district::point_in_ring 同式（射线法）。
    wv_core::source::district::point_in_ring(p.x, p.y, ring)
}

impl crate::EditCommand for UpsertDistrictCommand {
    fn label(&self) -> String {
        match &self.district.name {
            Some(n) => format!("街区「{n}」生成"),
            None => "街区生成".into(),
        }
    }

    fn apply(&self, world: &mut World) -> crate::EditResult<Vec<String>> {
        // ① 缓存建筑文档快照（revert 整份还原——生成替换是整区级动作）。
        *(self
            .before_buildings
            .lock()
            .map_err(|_| crate::EditError::Precondition("命令锁中毒".into()))?) = Some(
            world
                .building_of(&self.building_layer)
                .cloned()
                .unwrap_or_default(),
        );
        // ② 缓存街区旧值。
        *(self
            .before_district
            .lock()
            .map_err(|_| crate::EditError::Precondition("命令锁中毒".into()))?) = Some(
            world
                .city_block_of(&self.district_layer)
                .and_then(|doc| doc.find(&self.district.id))
                .cloned(),
        );
        // ③ upsert 街区。
        let mut doc = world
            .city_block_of(&self.district_layer)
            .cloned()
            .unwrap_or_default();
        doc.upsert(self.district.clone());
        world.set_city_blocks(self.district_layer.clone(), doc);
        // ④ 重生成建筑。
        Self::regenerate(world, self, &self.district);
        *(self
            .applied
            .lock()
            .map_err(|_| crate::EditError::Precondition("命令锁中毒".into()))?) =
            Some(self.district.clone());
        Ok(district_dirty_paths(
            &self.district_layer,
            &self.building_layer,
        ))
    }

    fn revert(&self, world: &mut World) -> crate::EditResult<()> {
        let before_b = self
            .before_buildings
            .lock()
            .map_err(|_| crate::EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| crate::EditError::Precondition("UpsertDistrict 尚未 apply".into()))?;
        world.set_buildings(self.building_layer.clone(), before_b);
        let before_d = self
            .before_district
            .lock()
            .map_err(|_| crate::EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| crate::EditError::Precondition("UpsertDistrict 尚未 apply".into()))?;
        let mut doc = world
            .city_block_of(&self.district_layer)
            .cloned()
            .unwrap_or_default();
        match before_d {
            Some(old) => {
                doc.upsert(old);
            }
            None => {
                doc.remove(&self.district.id);
            }
        }
        world.set_city_blocks(self.district_layer.clone(), doc);
        *self
            .applied
            .lock()
            .map_err(|_| crate::EditError::Precondition("命令锁中毒".into()))? = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditCommand;
    use wv_core::base::geometry::Point;
    use wv_core::map::{Extent, MapLevel, MapNode};

    fn ring(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Point> {
        vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]
    }

    fn world_with_map() -> World {
        let mut world = World::default();
        world.maps.push(MapNode {
            id: Id::new_unchecked("map_1"),
            name: "城".into(),
            level: MapLevel::City,
            parent: None,
            children: Vec::new(),
            icons: Vec::new(),
            layers: Vec::new(),
            extent: Some(Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        });
        world
    }

    fn district(id: &str, seed: u64, density: f32) -> District {
        District {
            id: Id::new_unchecked(id),
            name: None,
            ring: ring(0.0, 0.0, 200.0, 200.0),
            params: DistrictParams {
                density,
                ..DistrictParams::default()
            },
            seed,
        }
    }

    /// apply：街区落位 + ring 内建筑 = 生成结果；脏路径两条。
    #[test]
    fn apply_upserts_district_and_regenerates_buildings() {
        let mut world = world_with_map();
        let (dl, bl) = ensure_district_layers(&mut world, &Id::new_unchecked("map_1"));
        let cmd = UpsertDistrictCommand::new(
            dl.clone(),
            bl.clone(),
            Vec::new(),
            district("blk_1", 42, 0.8),
        );
        let dirty = cmd.apply(&mut world).expect("apply 应成功");
        assert_eq!(
            dirty,
            vec![
                format!("city_blocks/{}", dl.as_str()),
                format!("buildings/{}", bl.as_str())
            ]
        );
        let doc = world.city_block_of(&dl).expect("街区文档应存在");
        assert_eq!(doc.find(&Id::new_unchecked("blk_1")).unwrap().seed, 42);
        let buildings = &world.building_of(&bl).unwrap().buildings;
        assert!(!buildings.is_empty(), "密度 0.8 应有产出");
        for b in buildings {
            let c = b.centroid().unwrap();
            assert!(
                wv_core::source::district::point_in_ring(c.x, c.y, &ring(0.0, 0.0, 200.0, 200.0)),
                "全部建筑应在环内"
            );
        }
    }

    /// D3 核心：重摇**清掉手摆建筑**——ring 内既有建筑被替换为生成结果。
    #[test]
    fn regenerate_clears_hand_placed_buildings_in_ring() {
        let mut world = world_with_map();
        let (dl, bl) = ensure_district_layers(&mut world, &Id::new_unchecked("map_1"));
        // 作者手摆一栋（环内）+ 一栋（环外）。
        let mut doc = wv_core::BuildingLayer::new();
        doc.push(wv_core::Building::rect(50.0, 50.0, 10.0, 10.0, 9.0));
        doc.push(wv_core::Building::rect(500.0, 500.0, 10.0, 10.0, 9.0));
        world.set_buildings(bl.clone(), doc);
        // density=0：环内长不出新建筑 → 环内应被清空、环外保留。
        let cmd = UpsertDistrictCommand::new(dl, bl.clone(), Vec::new(), district("blk_1", 7, 0.0));
        cmd.apply(&mut world).expect("apply 应成功");
        let buildings = &world.building_of(&bl).unwrap().buildings;
        assert_eq!(buildings.len(), 1, "环外手摆建筑应保留");
        let c = buildings[0].centroid().unwrap();
        assert!((c.x - 505.0).abs() < 1e-4, "保留的应是环外那栋");
    }

    /// revert 逐位还原：街区消失（首建）+ 建筑文档回到快照。
    #[test]
    fn revert_restores_both_docs_bit_exact() {
        let mut world = world_with_map();
        let (dl, bl) = ensure_district_layers(&mut world, &Id::new_unchecked("map_1"));
        let mut doc = wv_core::BuildingLayer::new();
        doc.push(wv_core::Building::rect(500.0, 500.0, 10.0, 10.0, 9.0));
        world.set_buildings(bl.clone(), doc);
        let cmd = UpsertDistrictCommand::new(
            dl.clone(),
            bl.clone(),
            Vec::new(),
            district("blk_1", 42, 0.8),
        );
        cmd.apply(&mut world).expect("apply");
        cmd.revert(&mut world).expect("revert");
        assert!(
            world.city_block_of(&dl).unwrap().districts.is_empty(),
            "首建 revert 后街区消失"
        );
        let buildings = &world.building_of(&bl).unwrap().buildings;
        assert_eq!(buildings.len(), 1, "建筑文档应回到快照");
        let c = buildings[0].centroid().unwrap();
        assert!((c.x - 505.0).abs() < 1e-4);
        // apply 后未 revert 就再 apply → Precondition 不炸（V1 允许重复 apply
        // 覆盖快照？——不允许：编辑链保证 apply/revert 成对，这里只测单对）。
    }

    /// 未 apply 就 revert → Precondition（命令层兜底）。
    #[test]
    fn revert_without_apply_is_precondition_error() {
        let mut world = world_with_map();
        let (dl, bl) = ensure_district_layers(&mut world, &Id::new_unchecked("map_1"));
        let cmd = UpsertDistrictCommand::new(dl, bl, Vec::new(), district("blk_1", 1, 0.5));
        assert!(cmd.revert(&mut world).is_err());
    }
}
