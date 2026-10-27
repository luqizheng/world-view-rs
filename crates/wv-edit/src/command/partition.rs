//! 势力图切蛋糕分区命令（ADR-0074）。
//!
//! [`BeginPartitionSpanCommand`] 是动刀的第 0 步：该层无任何历史段时，
//! 用 `land_outlines` 造**首段**（整块地 + 湖洞，初始归属可空=无主）；
//! 已有历史段时在 `from` 处 **fork**：命中段拆 `[a,from)` + 新段
//! `[from,b)`（快照复制，顶点/脸 id 保持）——此后 Cut / Move 就地改新段，
//! Assign 在 fork 上换归属。
//!
//! dirty 键 = `"partitions"`（wv-io 的 saver 路由）。

use crate::command::*;
use wv_core::model::faction_partition::{PartitionFace, PartitionSpan, PartitionVertex};
use wv_core::{Id, World};

/// 动刀第 0 步（ADR-0074 D2）：开一段新的划分历史。
///
/// 两分支：
/// - 该层（map × layer）**无任何历史段** → 用 [`wv_terrain::outline::land_outlines`]
///   造**首段**：最大外环为初始脸、湖为洞，`initial_owner` 可空 = 无主；
/// - **已有历史段** → 在 `from` 处 **fork**：命中段 `[a, to)` 拆成
///   `[a, from)` + 新段 `[from, to)`，新段为命中段的**快照复制**
///   （顶点/脸 id 保持——共享引用是零和的构造基础）。
///
/// 时间语义：`from` 必须落在既有首段之后（否则无历史可承，Err）；
/// 首段的 `from` 即所给的生效时间（spec D4：新增要填生效时间）。
///
/// dirty = `["partitions"]`。
#[derive(Debug)]
pub struct BeginPartitionSpanCommand {
    /// 目标地图。
    pub map: Id,
    /// 目标势力层。
    pub layer: Id,
    /// 生效日（半开区间下界）。
    pub from: i64,
    /// 首段初始归属；`None` = 无主（不造合成势力实体）。
    pub initial_owner: Option<Id>,
    /// `apply` 缓存的整个分区快照（revert 原样写回）。
    before: std::sync::Mutex<Option<wv_core::model::faction_partition::FactionPartition>>,
}

impl BeginPartitionSpanCommand {
    /// 构造（`before` 锁不外露）。
    pub fn new(map: Id, layer: Id, from: i64, initial_owner: Option<Id>) -> Self {
        Self {
            map,
            layer,
            from,
            initial_owner,
            before: std::sync::Mutex::new(None),
        }
    }

    /// 定位该 map × layer 的分区；无则建一份空分区挂进 `world`（调用方持可变借用）。
    fn partition_mut<'w>(
        &self,
        world: &'w mut World,
    ) -> &'w mut wv_core::model::faction_partition::FactionPartition {
        if let Some(i) = world
            .partitions
            .iter()
            .position(|p| p.map == self.map && p.layer == self.layer)
        {
            return &mut world.partitions[i];
        }
        world
            .partitions
            .push(wv_core::model::faction_partition::FactionPartition {
                map: self.map.clone(),
                layer: self.layer.clone(),
                spans: Vec::new(),
            });
        world.partitions.last_mut().expect("刚 push")
    }

    /// 造首段：整块地（最大外环）+ 湖洞。
    fn first_span(
        &self,
        world: &World,
    ) -> EditResult<wv_core::model::faction_partition::PartitionSpan> {
        let data = world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TargetMissing(format!("地图「{}」无地形栅格", self.map)))?;
        let grid = to_grid(data)?;
        // 取最大面积外环为蛋糕主体（碎屑小岛不进首段；作者要多块可后续再切）。
        let mut outlines = wv_terrain::outline::land_outlines(&grid, 1.0);
        outlines.sort_by_key(|o| {
            let n = o.outer.len();
            let mut s = 0.0f64;
            for i in 0..n {
                let (x1, y1) = o.outer[i];
                let (x2, y2) = o.outer[(i + 1) % n];
                s += x1 as f64 * y2 as f64 - x2 as f64 * y1 as f64;
            }
            std::cmp::Reverse(s.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal))
        });
        let Some(group) = outlines.first() else {
            return Err(EditError::TargetMissing(format!(
                "地图「{}」无陆地可分配（全海）",
                self.map
            )));
        };
        // 外环 → 顶点表 + 脸环。
        let n = group.outer.len();
        let mut vertices: Vec<PartitionVertex> = Vec::with_capacity(n);
        for (i, (x, y)) in group.outer.iter().enumerate() {
            vertices.push(PartitionVertex {
                id: i as u32,
                xy: [*x, *y],
            });
        }
        let ring: Vec<u32> = (0..n as u32).collect();
        // 洞：沿用外环坐标段编 id（续外环号）。
        let mut holes: Vec<Vec<u32>> = Vec::new();
        let mut next_id = n as u32;
        for hole in &group.holes {
            let mut h = Vec::with_capacity(hole.len());
            for (x, y) in hole {
                vertices.push(PartitionVertex {
                    id: next_id,
                    xy: [*x, *y],
                });
                h.push(next_id);
                next_id += 1;
            }
            holes.push(h);
        }
        Ok(PartitionSpan {
            from: self.from,
            to: None,
            vertices,
            faces: vec![PartitionFace {
                id: 0,
                owner: self.initial_owner.clone(),
                ring,
                holes,
            }],
        })
    }
}

impl EditCommand for BeginPartitionSpanCommand {
    fn label(&self) -> String {
        format!("势力层划分：自第 {} 天开段", self.from)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 快照在动任何东西之前抓（partition_mut 可能新建分区）。
        let snapshot = world
            .partitions
            .iter()
            .find(|p| p.map == self.map && p.layer == self.layer)
            .cloned();
        let is_first = snapshot
            .as_ref()
            .map(|p| p.spans.is_empty())
            .unwrap_or(true);
        let new_span = if is_first {
            self.first_span(world)?
        } else {
            // fork：找命中 `from` 的段（from 必须落在首段之后）。
            let existing = snapshot.as_ref().expect("非首段必有分区");
            let hit = existing
                .spans
                .iter()
                .position(|s| self.from >= s.from && s.to.is_none_or(|to| self.from < to))
                .ok_or_else(|| {
                    EditError::Precondition(format!(
                        "生效日 {} 不在任何既有历史段内（首段自 {} 起）",
                        self.from, existing.spans[0].from
                    ))
                })?;
            if existing.spans[hit].from == self.from {
                // 恰在段起点 = 幂等：直接复用该段（不开零长度段）。
                return Ok(vec!["partitions".to_string()]);
            }
            let mut old = existing.spans[hit].clone();
            let mut fork = old.clone();
            old.to = Some(self.from);
            fork.from = self.from;
            fork.to = existing.spans[hit].to;
            let partition = self.partition_mut(world);
            partition.spans[hit] = old;
            partition.spans.insert(hit + 1, fork);
            *self
                .before
                .lock()
                .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
            return Ok(vec!["partitions".to_string()]);
        };
        // 首段：清空（空表不该有段，防御）后挂入。
        let partition = self.partition_mut(world);
        partition.spans.clear();
        partition.spans.push(new_span);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = snapshot;
        Ok(vec!["partitions".to_string()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snapshot = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("BeginPartitionSpan 尚未 apply".into()))?;
        let partition = self.partition_mut(world);
        *partition = snapshot;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    /// 3×3 陆块、中心一格水（湖），cell=10。整图 8×8。
    fn world_with_island_lake() -> World {
        let mut grid = wv_terrain::TerrainGrid::new(8, 8);
        for y in 3..6 {
            for x in 3..6 {
                grid.set(
                    x,
                    y,
                    wv_terrain::TerrainCell {
                        height: 100,
                        surface: wv_terrain::SURFACE_LAND,
                        biome: 0,
                    },
                );
            }
        }
        grid.set(
            4,
            4,
            wv_terrain::TerrainCell {
                height: -10,
                surface: wv_terrain::SURFACE_WATER,
                biome: 0,
            },
        );
        let mut world = World::default();
        world.set_terrain(test_map(), from_grid(grid));
        world
    }

    /// 已有一段 `[0,None)` 的分区（单脸单方环）。
    fn world_with_existing_span() -> World {
        let mut world = World::default();
        world
            .partitions
            .push(wv_core::model::faction_partition::FactionPartition {
                map: test_map(),
                layer: Id::new_unchecked("layer_borders"),
                spans: vec![PartitionSpan {
                    from: 0,
                    to: None,
                    vertices: vec![
                        PartitionVertex {
                            id: 0,
                            xy: [0.0, 0.0],
                        },
                        PartitionVertex {
                            id: 1,
                            xy: [10.0, 0.0],
                        },
                        PartitionVertex {
                            id: 2,
                            xy: [10.0, 10.0],
                        },
                    ],
                    faces: vec![PartitionFace {
                        id: 0,
                        owner: Some(Id::new_unchecked("faction_a")),
                        ring: vec![0, 1, 2],
                        holes: Vec::new(),
                    }],
                }],
            });
        world
    }

    #[test]
    fn first_span_is_whole_land_with_lake_hole() {
        let mut world = world_with_island_lake();
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            0,
            Some(Id::new_unchecked("faction_a")),
        );
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec!["partitions".to_string()], "脏键应为 partitions");

        let p = &world.partitions[0];
        assert_eq!(p.map, test_map());
        assert_eq!(p.layer, Id::new_unchecked("layer_borders"));
        assert_eq!(p.spans.len(), 1, "首段");
        let span = &p.spans[0];
        assert_eq!(span.from, 0);
        assert_eq!(span.to, None, "首段至今");
        // 3×3 陆块 → 外环抽稀后 4 角；中心湖 → 1 洞（4 角）。
        assert_eq!(span.faces.len(), 1, "单脸");
        let face = &span.faces[0];
        assert_eq!(face.ring.len(), 4, "3×3 外环应抽稀到 4 角: {:?}", face.ring);
        assert_eq!(face.holes.len(), 1, "湖应成洞: {:?}", face.holes);
        assert_eq!(
            face.owner,
            Some(Id::new_unchecked("faction_a")),
            "初始归属写入"
        );
        span.validate_topology().expect("生成的首段应过拓扑自检");
    }

    #[test]
    fn first_span_owner_none_is_unclaimed() {
        let mut world = world_with_island_lake();
        let cmd =
            BeginPartitionSpanCommand::new(test_map(), Id::new_unchecked("layer_borders"), 5, None);
        cmd.apply(&mut world).expect("apply");
        assert_eq!(
            world.partitions[0].spans[0].faces[0].owner, None,
            "无主 = owner None，不造合成势力实体"
        );
    }

    #[test]
    fn fork_closes_old_span_and_copies_snapshot() {
        let mut world = world_with_existing_span();
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            10,
            None,
        );
        cmd.apply(&mut world).expect("apply");
        let p = &world.partitions[0];
        assert_eq!(p.spans.len(), 2, "fork 出两段");
        assert_eq!(
            (p.spans[0].from, p.spans[0].to),
            (0, Some(10)),
            "旧段收口于 fork 日"
        );
        assert_eq!(
            (p.spans[1].from, p.spans[1].to),
            (10, None),
            "新段自 fork 日至今"
        );
        // 快照复制：顶点/脸逐位相等（id 保持——共享引用的零和基础）。
        assert_eq!(p.spans[0].vertices, p.spans[1].vertices, "顶点快照复制");
        assert_eq!(p.spans[0].faces, p.spans[1].faces, "脸快照复制");
    }

    #[test]
    fn fork_day_before_first_span_errors() {
        let mut world = world_with_existing_span(); // 首段 [0,None)
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            -5,
            None,
        );
        assert!(
            cmd.apply(&mut world).is_err(),
            "fork 日在首段之前无历史可承，应 Err 不落盘"
        );
        assert!(world.partitions[0].spans.len() == 1, "Err 时不得改动世界");
    }

    #[test]
    fn begin_without_terrain_errors() {
        let mut world = World::default(); // 无栅格
        let cmd =
            BeginPartitionSpanCommand::new(test_map(), Id::new_unchecked("layer_borders"), 0, None);
        assert!(
            cmd.apply(&mut world).is_err(),
            "无自有地形栅格的图造不出蛋糕，应 Err"
        );
    }

    #[test]
    fn revert_restores_previous_partition_state() {
        let mut world = world_with_existing_span();
        let before = world.partitions.clone();
        let cmd = BeginPartitionSpanCommand::new(
            test_map(),
            Id::new_unchecked("layer_borders"),
            10,
            None,
        );
        cmd.apply(&mut world).expect("apply");
        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.partitions, before,
            "revert 应恢复到 apply 前的分区状态"
        );
    }
}
