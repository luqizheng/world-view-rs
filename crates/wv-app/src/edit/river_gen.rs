//! 生成器河实体（ADR-0070 D3 生成器统一 · 计划 Task 12）。
//!
//! ## 唯一定义点
//!
//! 种子河的「生成期定义」只有这一处：`wv_terrain::climate::generate_rivers`
//! 产折线 → 归一化 `[0,1]` × 图幅 extent → 图内局部 wu → 河线 POI 实体
//! （无下切、绕行模式语义、宽度不设）。消费方两个：
//!
//! - **RegenWorld**（`ui_shell/dispatch.rs` → `editing::commit_regen_world`）：
//!   新地形 + 新 `rivers_seed` 落实体，与 `RegenWorldTerrainCommand` 同一
//!   Transaction（一次撤销全回）；上一轮生成器河经
//!   [`ClearGeneratorRiversCommand`] 同事务下架——渲染期 seed 派生已删
//!   （T8），此处是生成期唯一定义点；
//! - **新建工程**（`ui/new_project_dialog.rs::initial_world`）：与城市初布点
//!   同款的「创建期即实体」，工程首保存即落盘。
//!
//! ## 识别口径
//!
//! 生成器河的 ID 以 [`GENERATOR_RIVER_PREFIX`]（`river_gen_`）开头，**带图 ID
//! 段**（`river_gen_<map>_<NN>`）——地点表是全工程一张扁平表，不带图的 ID 段
//! 会让两张图各自的第 0 条河撞名。作者手画的河（`river_<h6>`）与迁移实体
//! （`river_tpl_NN`）前缀不同，Regen 换血不碰它们。

#![cfg(feature = "app")]

use wv_core::distance::Place;
use wv_core::id::Id;
use wv_core::map::distance::LineKind;
use wv_core::terrain::TerrainData;
use wv_core::World;
use wv_edit::command::{EditCommand, EditError, EditResult};

/// 生成器河的 ID 前缀。RegenWorld 靠它识别「上一轮的生成器河」整批换血。
pub const GENERATOR_RIVER_PREFIX: &str = "river_gen_";

/// 生成器河条数上限（与渲染侧先例同参，ADR-0070 §4.2 生成器统一）。
pub const MAX_GENERATOR_RIVERS: usize = 12;

/// 种子 + 地形 → 河线 POI 实体列表（无下切；RegenWorld 与新建工程共用）。
///
/// 折线换算与迁移器（`wv-edit::command::line_poi` 模板河转实体段）同款：
/// `generate_rivers` 返回归一化 `[0,1]^2`（y 向下 = 行序方向），乘 extent 得
/// 图内局部 wu。实体字段口径：`line_kind = Some(River)`（迁移器同款显式）、
/// `line_width_m = None`（宽度不设，渲染退缺省宽）、锚点缺省（由折线中点
/// 派生）。ID 确定性：`river_gen_<map>_<NN>`，同输入必同 ID（幂等再落盘）。
pub fn generator_river_places(
    map: &Id,
    terrain: &TerrainData,
    seed: u64,
    extent: (f32, f32),
) -> Vec<Place> {
    let heights: Vec<i16> = terrain.cells.iter().map(|c| c.height).collect();
    let rivers = wv_terrain::climate::generate_rivers(
        terrain.width,
        terrain.height,
        &heights,
        seed,
        MAX_GENERATOR_RIVERS,
    );
    rivers
        .iter()
        .enumerate()
        .map(|(i, poly)| {
            let line: Vec<f32> = poly
                .iter()
                .flat_map(|p| [p[0] * extent.0, p[1] * extent.1])
                .collect();
            Place {
                // 图 ID 与序号都是受限字符集（AGENTS §4.4），受校验构造的
                // 哨兵分支不可达；带原串便于排障。
                id: Id::new(format!("{GENERATOR_RIVER_PREFIX}{}_{}", map.as_str(), i))
                    .unwrap_or_else(|e| panic!("生成器河 ID 构造失败：{e:?}")),
                name: format!("河流 {:02}", i + 1),
                feature: None,
                map: Some(map.clone()),
                x: None,
                y: None,
                w: None,
                h: None,
                biome: None,
                envelope: None,
                rank: None,
                line: Some(line),
                kind: None,
                line_kind: Some(LineKind::River),
                line_width_m: None,
            }
        })
        .collect()
}

/// RegenWorld 事务的「上一轮生成器河下架」命令（应用层内部命令）。
///
/// ## 为什么下架必须与新河同一事务
///
/// 二次 Regen 时旧生成器河仍在表里，`CreateLinePoiCommand` 会因 ID 撞名
/// 整体失败；旧河折线还锚着**旧地形**（Regen 已换图），留在场上就是幽灵河。
/// 下架 + 上架同事务，撤销一次才回得到「旧地形 + 旧河」的完整原状。
///
/// ## 识别与边界
///
/// 只下架**本图**上 ID 以 [`GENERATOR_RIVER_PREFIX`] 开头的地点——作者
/// 手画的河、其他图的生成器河、迁移实体都不碰。apply 记 `Vec<(原下标,
/// Place)>`，revert 按**原下标升序**插回（undo 后地点表逐位复原，迁移命令
/// M1 同款口径）。
#[derive(Debug)]
pub struct ClearGeneratorRiversCommand {
    map: Id,
    before: std::sync::Mutex<Vec<(usize, Place)>>,
}

impl ClearGeneratorRiversCommand {
    /// 构造（目标图；改什么由 apply 时的世界数据决定）。
    pub fn new(map: Id) -> Self {
        Self {
            map,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for ClearGeneratorRiversCommand {
    fn label(&self) -> String {
        format!("下架上一轮生成器河（{}）", self.map)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
        before.clear();
        // 先收后删（降序删下标不漂移）。
        for (i, p) in world
            .distances
            .places
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.map.as_ref() == Some(&self.map)
                    && p.id.as_str().starts_with(GENERATOR_RIVER_PREFIX)
            })
            .collect::<Vec<_>>()
        {
            before.push((i, p.clone()));
        }
        for (i, _) in before.iter().rev() {
            world.distances.places.remove(*i);
        }
        if before.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec!["distances/places".into()])
        }
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let mut before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone();
        // 原下标升序插回（undo 后地点表与下架前逐位一致）。
        before.sort_by_key(|(i, _)| *i);
        for (i, p) in before {
            let at = i.min(world.distances.places.len());
            world.distances.places.insert(at, p);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::editing::{commit_regen_world, EditHistory};
    use crate::state::WorldProjection;
    use wv_core::{WorldMeta, WorldTemplate, WorldTemplateKind};

    const M: &str = "map_gen";

    fn vid(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 20×20 全 1000 m 高地图（源头门槛 700 必有候选；与 wv-edit 迁移测试
    /// 的夹具同款——同输入下 generate_rivers 必产 ≥1 条河）。
    fn high_terrain(height: i16) -> TerrainData {
        TerrainData {
            width: 20,
            height: 20,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height,
                    surface: 1,
                    biome: 1,
                };
                400
            ],
        }
    }

    fn gen_world() -> World {
        let mut w = World::default();
        w.maps.push(wv_core::map::MapNode {
            id: vid(M),
            name: "生成图".into(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: vec![],
            layers: vec![],
            icons: vec![],
            extent: Some(wv_core::map::Extent::sized(256.0, 256.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            ..Default::default()
        });
        w.set_terrain(vid(M), high_terrain(800));
        w.meta = Some(WorldMeta {
            name: "t".into(),
            version: "1".into(),
            schema_version: 2,
            root_map: vid(M),
            timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
            template: Some(WorldTemplate {
                kind: WorldTemplateKind::ColdNorth,
                seed: 1,
                rivers_seed: 42,
            }),
        });
        w
    }

    /// 作者手画的河（前缀不同，Regen 换血不得碰）。
    fn author_river() -> Place {
        Place {
            id: vid("river_keep"),
            name: "作者的河".into(),
            feature: None,
            map: Some(vid(M)),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(vec![10.0, 10.0, 200.0, 10.0]),
            kind: None,
            line_kind: Some(LineKind::River),
            line_width_m: None,
        }
    }

    fn gen_river_count(w: &World) -> usize {
        w.distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with(GENERATOR_RIVER_PREFIX))
            .count()
    }

    /// RegenWorld 落生成器河实体（计划 Task 12 点名）：河线 POI 在场、
    /// 数量与种子口径一致、地形不被下切；二次 Regen 旧河整批换血、数量
    /// 与新种子一致（确定性）；作者手画的河两次都幸存；一次撤销全回。
    #[test]
    fn regen_world_creates_river_entities() {
        let mut edit = EditHistory::default();
        let mut projection = WorldProjection {
            world: gen_world(),
            ..Default::default()
        };
        projection.world.distances.places.push(author_river());
        let map = vid(M);

        // 合成器确定性：同输入两跑，ID 与折线逐位一致。
        let t1 = high_terrain(1000);
        let a = generator_river_places(&map, &t1, 42, (256.0, 256.0));
        let b = generator_river_places(&map, &t1, 42, (256.0, 256.0));
        assert!(!a.is_empty(), "高地夹具应至少产 1 条河，实际 {}", a.len());
        assert_eq!(
            a.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            b.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            "同输入必同 ID"
        );
        assert_eq!(
            a.iter().map(|p| p.line.clone()).collect::<Vec<_>>(),
            b.iter().map(|p| p.line.clone()).collect::<Vec<_>>(),
            "同输入必同折线"
        );
        // 折线已换算到图幅内（归一化 × extent）。
        for p in &a {
            assert_eq!(p.line_kind, Some(LineKind::River), "河语义显式");
            assert_eq!(p.line_width_m, None, "宽度不设");
            let l = p.line.as_ref().expect("有折线");
            assert!(l
                .chunks(2)
                .all(|c| c[0] >= 0.0 && c[0] <= 256.0 && c[1] >= 0.0 && c[1] <= 256.0));
        }

        // 第一次 Regen（seed 42）：实体在场、数量 = 生成器折线数、地形不动。
        let expected1 = {
            let heights: Vec<i16> = t1.cells.iter().map(|c| c.height).collect();
            wv_terrain::climate::generate_rivers(20, 20, &heights, 42, MAX_GENERATOR_RIVERS).len()
        };
        let tpl1 = WorldTemplate {
            kind: WorldTemplateKind::ColdNorth,
            seed: 1,
            rivers_seed: 42,
        };
        commit_regen_world(
            &map,
            t1.clone(),
            tpl1,
            a.clone(),
            &mut edit,
            &mut projection,
        )
        .expect("首次 Regen 应成功");
        assert_eq!(
            gen_river_count(&projection.world),
            expected1,
            "实体数应与种子口径一致"
        );
        assert!(
            projection
                .world
                .terrain_of(&map)
                .expect("有地形")
                .cells
                .iter()
                .all(|c| c.height == 1000),
            "生成器河无下切（height 全不动）"
        );
        assert!(
            projection
                .world
                .distances
                .places
                .iter()
                .any(|p| p.id == vid("river_keep")),
            "作者的河必须幸存"
        );

        // 第二次 Regen（新地形 + 新种子）：旧河整批换血，数量与新种子一致。
        let t2 = high_terrain(1200);
        let rivers2 = generator_river_places(&map, &t2, 77, (256.0, 256.0));
        let expected2 = {
            let heights: Vec<i16> = t2.cells.iter().map(|c| c.height).collect();
            wv_terrain::climate::generate_rivers(20, 20, &heights, 77, MAX_GENERATOR_RIVERS).len()
        };
        let tpl2 = WorldTemplate {
            kind: WorldTemplateKind::ColdNorth,
            seed: 2,
            rivers_seed: 77,
        };
        commit_regen_world(&map, t2.clone(), tpl2, rivers2, &mut edit, &mut projection)
            .expect("二次 Regen 应成功");
        assert_eq!(
            gen_river_count(&projection.world),
            expected2,
            "二次 Regen 后实体数应与新种子口径一致（旧河已换血，无残留无增殖）"
        );
        assert!(
            projection
                .world
                .distances
                .places
                .iter()
                .any(|p| p.id == vid("river_keep")),
            "作者的河二次幸存"
        );

        // 一次撤销：回到第一次 Regen 的完整原状（地形 + 模板 + 河）。
        edit.history.undo(&mut projection.world).expect("应可撤销");
        assert_eq!(
            gen_river_count(&projection.world),
            expected1,
            "撤销应回到首轮河数"
        );
        assert!(
            projection
                .world
                .terrain_of(&map)
                .expect("有地形")
                .cells
                .iter()
                .all(|c| c.height == 1000),
            "撤销应回滚首轮地形"
        );
        assert_eq!(
            projection
                .world
                .meta
                .as_ref()
                .and_then(|m| m.template)
                .expect("模板在场")
                .rivers_seed,
            42,
            "撤销应回滚模板记录"
        );
    }

    /// 下架命令的边界：只清本图的 `river_gen_*`，作者河 / 他图生成器河 /
    /// 迁移实体不碰；revert 按原下标升序插回（地点表逐位复原）。
    #[test]
    fn clear_generator_rivers_keeps_author_and_foreign_places() {
        let mut w = gen_world();
        let other = vid("map_other");
        w.maps.push(wv_core::map::MapNode {
            id: other.clone(),
            ..w.maps[0].clone()
        });
        w.distances.places = vec![
            author_river(),
            Place {
                id: vid("river_tpl_00"),
                name: "迁移河".into(),
                feature: None,
                map: Some(vid(M)),
                x: None,
                y: None,
                w: None,
                h: None,
                biome: None,
                envelope: None,
                rank: None,
                line: Some(vec![0.0, 0.0, 1.0, 1.0]),
                kind: None,
                line_kind: Some(LineKind::River),
                line_width_m: None,
            },
        ];
        // 他图生成器河 + 本图生成器河（占位 0 与 3）。
        let foreign = Place {
            map: Some(other.clone()),
            id: vid("river_gen_map_other_00"),
            name: "他图河".into(),
            ..author_river()
        };
        let mine = Place {
            id: vid(format!("{GENERATOR_RIVER_PREFIX}{M}_00").as_str()),
            name: "本轮河".into(),
            ..author_river()
        };
        w.distances.places.push(foreign);
        w.distances.places.push(mine);
        let order_before: Vec<String> = w
            .distances
            .places
            .iter()
            .map(|p| p.id.as_str().to_owned())
            .collect();

        let cmd = ClearGeneratorRiversCommand::new(vid(M));
        let dirty = cmd.apply(&mut w).expect("下架应成功");
        assert_eq!(dirty, vec!["distances/places".to_owned()], "动了表必须标脏");
        assert!(
            !w.distances
                .places
                .iter()
                .any(|p| p.id.as_str() == format!("{GENERATOR_RIVER_PREFIX}{M}_00")),
            "本图生成器河应被下架"
        );
        for keep in ["river_keep", "river_tpl_00", "river_gen_map_other_00"] {
            assert!(
                w.distances.places.iter().any(|p| p.id.as_str() == keep),
                "{keep} 不得被下架"
            );
        }

        cmd.revert(&mut w).expect("回退应成功");
        let order_after: Vec<String> = w
            .distances
            .places
            .iter()
            .map(|p| p.id.as_str().to_owned())
            .collect();
        assert_eq!(order_after, order_before, "revert 后地点表应逐位复原");
    }
}
