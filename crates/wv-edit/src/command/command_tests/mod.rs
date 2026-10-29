//! 编辑命令族单测（自 command.rs 拆分）。
//!
//! 各测试模块经 `use crate::command::*` 取契约与命令类型——拆分前为 `use super::*`
//!（= command 模块）；改用绝对路径以适配子模块层级。
//!
//! ## 拆分（P1-5，2026-10-29）
//! 原 5200 行单文件 → 目录：四个独立小模块原样保留在本文件；原 135 测试的
//! `tests` 大模块按命令族切 11 个子模块（公共夹具 `TEST_MAP` / `test_map`
//! 与 use 块提升到本文件，子模块经 `use super::*` 复用）。

//! 编辑命令族单测（自 command.rs 拆分）。
//!
//! 各测试模块经 `use crate::command::*` 取契约与命令类型——拆分前为 `use super::*`
//!（= command 模块）；改用绝对路径以适配子模块层级。

#[cfg(test)]
mod batch_edit_cells_tests {
    use crate::command::*;
    use wv_terrain::SURFACE_LAND;

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(n: u32) -> World {
        let cells = vec![
            wv_core::terrain::TerrainCellData {
                height: 0,
                surface: 0,
                biome: 0
            };
            (n * n) as usize
        ];
        let mut world = World::default();
        world.set_terrain(
            test_map(),
            wv_core::terrain::TerrainData {
                width: n,
                height: n,
                cells,
            },
        );
        world
    }

    /// 批量 apply 写 N 格 / revert 逐格还原；1 个撤销单位。
    #[test]
    fn batch_edit_cells_applies_and_reverts_all_changes() {
        let mut world = world_with_terrain(8);
        let cell_at =
            |t: &wv_core::terrain::TerrainData, gx: i32, gy: i32| t.cells[(gy * 8 + gx) as usize];
        let t0 = world.terrain_of(&test_map()).unwrap().clone();
        let changes = vec![
            CellChange {
                gx: 1,
                gy: 1,
                before: cell_at(&t0, 1, 1),
                after: wv_core::terrain::TerrainCellData {
                    height: 100,
                    surface: SURFACE_LAND,
                    biome: 3,
                },
            },
            CellChange {
                gx: 2,
                gy: 1,
                before: cell_at(&t0, 2, 1),
                after: wv_core::terrain::TerrainCellData {
                    height: 200,
                    surface: 0,
                    biome: 14,
                },
            },
        ];
        let cmd = BatchEditCellsCommand::new(test_map(), changes);
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec![format!("maps/{TEST_MAP}/terrain.bin")]);
        let t1 = world.terrain_of(&test_map()).unwrap().clone();
        assert_eq!(cell_at(&t1, 1, 1).height, 100);
        assert_eq!(cell_at(&t1, 2, 1).height, 200);
        assert_eq!(cell_at(&t1, 3, 1), cell_at(&t0, 3, 1), "未选格不动");

        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.terrain_of(&test_map()).unwrap().cells,
            t0.cells,
            "revert 后全图逐格还原"
        );
    }

    /// 任一格越界 → 整批拒收（不动 world）。
    #[test]
    fn batch_edit_cells_rejects_on_any_out_of_bounds() {
        let mut world = world_with_terrain(4);
        let cell = wv_core::terrain::TerrainCellData {
            height: 0,
            surface: 0,
            biome: 0,
        };
        let cmd = BatchEditCellsCommand::new(
            test_map(),
            vec![
                CellChange {
                    gx: 0,
                    gy: 0,
                    before: cell,
                    after: cell,
                },
                CellChange {
                    gx: 99,
                    gy: 0,
                    before: cell,
                    after: cell,
                },
            ],
        );
        let err = cmd.apply(&mut world).expect_err("越界必须整批拒收");
        assert!(matches!(err, EditError::Precondition(_)));
    }
}

#[cfg(test)]
mod edit_cell_tests {
    use crate::command::*;
    use wv_terrain::{SURFACE_LAND, SURFACE_WATER};

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(n: u32, surface: u8, height: i16, biome: u8) -> World {
        let cells = vec![
            TerrainCellData {
                height,
                surface,
                biome
            };
            (n as usize) * (n as usize)
        ];
        let mut world = World::default();
        world.set_terrain(
            test_map(),
            wv_core::terrain::TerrainData {
                width: n,
                height: n,
                cells,
            },
        );
        world
    }

    /// 基础 round-trip:apply 改一格,revert 还原。验收 [ADR-0040] §3 命令
    /// 沿"snapshot → 改 → revert"模式。
    #[test]
    fn edit_cell_command_round_trips_one_cell() {
        let mut world = world_with_terrain(8, SURFACE_LAND, 100, 0);
        let before = world.terrain_of(&test_map()).unwrap().cells[3 * 8 + 5];
        assert_eq!(before.surface, SURFACE_LAND);
        let after = TerrainCellData {
            height: 250,
            surface: SURFACE_WATER,
            biome: 5, // BIOME_DESERT
        };
        let cmd = EditCellCommand::new(test_map(), 5, 3, before, after);
        let dirty = cmd.apply(&mut world).expect("apply");
        assert_eq!(dirty, vec![format!("maps/{TEST_MAP}/terrain.bin")]);
        // 目标格被改了
        let cur = world.terrain_of(&test_map()).unwrap().cells[3 * 8 + 5];
        assert_eq!(cur, after, "apply 后目标格必须等于 after");
        // 其它格不变
        assert_eq!(world.terrain_of(&test_map()).unwrap().cells[0], before);
        assert_eq!(world.terrain_of(&test_map()).unwrap().cells[63], before);
        // 宽度/高度不动
        let t = world.terrain_of(&test_map()).unwrap();
        assert_eq!(t.width, 8);
        assert_eq!(t.height, 8);

        cmd.revert(&mut world).expect("revert");
        assert_eq!(
            world.terrain_of(&test_map()).unwrap().cells[3 * 8 + 5],
            before,
            "revert 后必须逐格还原"
        );
    }

    /// 起值不匹配时 apply 拒收,不动 world。
    #[test]
    fn edit_cell_command_rejects_stale_snapshot() {
        let mut world = world_with_terrain(8, SURFACE_LAND, 100, 0);
        let wrong_before = TerrainCellData {
            height: 999,
            surface: SURFACE_WATER,
            biome: 99,
        };
        let after = TerrainCellData {
            height: 0,
            surface: SURFACE_LAND,
            biome: 0,
        };
        let cmd = EditCellCommand::new(test_map(), 0, 0, wrong_before, after);
        let err = cmd.apply(&mut world).expect_err("stale before must fail");
        assert!(matches!(err, EditError::Precondition(_)), "got {err:?}");
        // world 不变
        assert_eq!(
            world.terrain_of(&test_map()).unwrap().cells[0].height,
            100,
            "拒收后格不被改动"
        );
    }

    /// 坐标超界 → 拒收。
    #[test]
    fn edit_cell_command_rejects_out_of_bounds() {
        let mut world = world_with_terrain(4, SURFACE_LAND, 0, 0);
        let cell = TerrainCellData {
            height: 0,
            surface: 0,
            biome: 0,
        };
        let cmd = EditCellCommand::new(test_map(), 99, 99, cell, cell);
        let err = cmd.apply(&mut world).expect_err("oob must fail");
        assert!(matches!(err, EditError::Precondition(_)));
    }

    /// 缺图 → 返 TerrainMismatch,而非 panic。
    #[test]
    fn edit_cell_command_missing_map_returns_terrain_mismatch() {
        let mut world = World::default();
        let cell = TerrainCellData {
            height: 0,
            surface: 0,
            biome: 0,
        };
        let cmd = EditCellCommand::new(test_map(), 0, 0, cell, cell);
        let err = cmd.apply(&mut world).expect_err("missing must fail");
        assert!(matches!(err, EditError::TerrainMismatch(_)), "got {err:?}");
    }
}

// ── 原 tests 大模块的公共头（P1-5 提升）──
use crate::command::*;
use wv_core::map::GeometryData;
use wv_core::time::TimeSpan;
use wv_core::Id;

/// 测试用的图 ID——`PaintTerrainCommand` 的栅格是**按图**查的。
const TEST_MAP: &str = "world_main";

fn test_map() -> Id {
    Id::new_unchecked(TEST_MAP)
}

// ── 跨簇夹具（P1-5：被多个子模块引用，提升到顶层）──

fn world_with_terrain(size: u32) -> World {
    let grid = TerrainGrid::new(size, size);
    let data = from_grid(grid);
    let mut world = World::default();
    world.set_terrain(test_map(), data);
    world
}

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

fn land_world(size: u32) -> World {
    let mut w = world_with_terrain(size);
    let land: Vec<TerrainCellData> = vec![
        TerrainCellData {
            height: 100,
            surface: 1,
            biome: 3,
        };
        (size * size) as usize
    ];
    w.set_terrain(
        test_map(),
        TerrainData {
            width: size,
            height: size,
            cells: land,
        },
    );
    w
}

fn map_with_extent(id: &str, extent: Option<(f32, f32)>) -> wv_core::map::MapNode {
    let mut n = map_node(id, None, wv_core::map::MapLevel::World);
    n.extent = extent.map(|(w, h)| wv_core::map::Extent::sized(w, h));
    n
}

fn territory(
    map: &str,
    owner: &str,
    from: i64,
    to: Option<i64>,
    ring: Vec<f32>,
) -> wv_core::map::GeometrySpan {
    wv_core::map::GeometrySpan {
        map: Id::new_unchecked(map),
        owner: Id::new_unchecked(owner),
        span: TimeSpan { from, to },
        geometry: GeometryData::Polygon(ring),
        relation: None,
    }
}

/// 标记地点 fixture（ADR-0032：fixture 也只写 biome，不写旧 feature 词汇）。
fn span_fixture_place(id: &str) -> wv_core::distance::Place {
    wv_core::distance::Place {
        kind: None,
        line: None,
        id: Id::new_unchecked(id),
        name: format!("地点{id}"),
        feature: None,
        biome: Some(wv_terrain::biome::BIOME_HILLS),
        envelope: None,
        map: None,
        x: None,
        y: None,
        w: None,
        h: None,
        rank: None,
        line_kind: None,
        line_width_m: None,
    }
}

fn marker_place(id: &str, name: &str, biome: u8) -> wv_core::distance::Place {
    wv_core::distance::Place {
        kind: None,
        line: None,
        id: wv_core::Id::new_unchecked(id),
        name: name.into(),
        feature: None,
        biome: Some(biome),
        envelope: None,
        map: Some(test_map()),
        x: None,
        y: None,
        w: None,
        h: None,
        rank: None,
        line_kind: None,
        line_width_m: None,
    }
}

fn create_marker(
    w: &mut World,
    id: &str,
    cell_size: f32,
    envelope: Vec<(f32, f32)>,
) -> wv_core::Id {
    let place_id = Id::new_unchecked(id);
    let cmd =
        PlaceBiomeMarkerCommand::new(test_map(), cell_size, 5, marker_place(id, id, 1), envelope);
    cmd.apply(w).expect("创建标记应成功");
    place_id
}

// ── 按命令族的子模块 ──
mod city;
mod city_map;
mod continent_marker;
mod faction;
mod layer_terrain;
mod map_props;
mod measurement;
mod place_edges;
mod place_event;
mod regenerate;
mod settlement;

// ── 独立小模块（原文件位于 tests 大模块之后，P1-5 首版脚本漏切，此处补回）──

#[cfg(test)]
mod line_poi_tests {
    use crate::command::*;
    use wv_core::distance::{LineKind, Place};
    use wv_terrain::{SURFACE_LAND, SURFACE_WATER};

    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    /// 32×32 全陆地地形（100 m，陆地表面）。
    fn world_with_land() -> World {
        let land: Vec<TerrainCellData> = vec![
            TerrainCellData {
                height: 100,
                surface: SURFACE_LAND,
                biome: 3,
            };
            32 * 32
        ];
        let mut w = World::default();
        w.set_terrain(
            test_map(),
            TerrainData {
                width: 32,
                height: 32,
                cells: land,
            },
        );
        w
    }

    /// 两点线段地点（4 个浮点 = 2 点，恰过 `line.len() >= 4` 门槛）。
    fn line_place() -> Place {
        Place {
            id: Id::new_unchecked("river_line"),
            name: "小河".into(),
            feature: None,
            map: Some(test_map()),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(vec![2.0, 5.0, 18.0, 5.0]),
            kind: None,
            line_kind: Some(LineKind::River),
            line_width_m: Some(12.0),
        }
    }

    /// 沿折线中轴的下切规格（宽 1 格、写水面 = 既有涂格河语义）。
    fn carve_spec() -> CarveSpec {
        CarveSpec {
            pts: vec![(2.5, 5.5), (18.5, 5.5)],
            strength: 1.0,
            width_cells: 1,
            mark_water: true,
        }
    }

    fn cell_at(w: &World, gx: usize, gy: usize) -> TerrainCellData {
        w.terrain_of(&test_map()).unwrap().cells[gy * 32 + gx]
    }

    /// 一次命令同时干两件事：Place 入表 + 沿线下切（ADR-0070：线段 POI 的
    /// 「创建即下切」是一个作者意图，撤销粒度也应当是它）。
    #[test]
    fn create_line_poi_adds_place_and_carves_in_one_command() {
        let mut w = world_with_land();
        let cmd = CreateLinePoiCommand::new(test_map(), line_place(), Some(carve_spec()), 1.0);
        let dirty = cmd.apply(&mut w).expect("apply");
        assert_eq!(
            dirty,
            vec![
                "distances/places".to_string(),
                format!("maps/{TEST_MAP}/terrain.bin")
            ],
            "脏路径 = 地点表 + 目标图地形（IntentTerrainCommand 同款拼法）"
        );
        assert!(
            w.distances
                .places
                .iter()
                .any(|p| p.id.as_str() == "river_line"),
            "线段地点应入表"
        );
        // 中轴格被下切：高度从 100 m 降、写水面（河语义）。
        let c = cell_at(&w, 10, 5);
        assert!(
            c.height < 100,
            "中轴格 (10,5) 应被下切，实得 {} m",
            c.height
        );
        assert_eq!(c.surface, SURFACE_WATER, "河走廊应写水面");
        // 走廊外不动。
        assert_eq!(cell_at(&w, 10, 1).height, 100, "走廊外格不应被改动");
    }

    /// 撤销 = 删地点 + 地形逐格还原（一次撤销含下切，ADR-0070）。
    #[test]
    fn undo_restores_both_place_and_carved_cells() {
        let mut w = world_with_land();
        let terrain_before = w.terrain_of(&test_map()).unwrap().clone();
        let cmd = CreateLinePoiCommand::new(test_map(), line_place(), Some(carve_spec()), 1.0);
        cmd.apply(&mut w).expect("apply");
        assert!(w
            .distances
            .places
            .iter()
            .any(|p| p.id.as_str() == "river_line"));

        cmd.revert(&mut w).expect("revert");
        assert!(
            !w.distances
                .places
                .iter()
                .any(|p| p.id.as_str() == "river_line"),
            "撤销后地点应从表里消失"
        );
        assert_eq!(
            w.terrain_of(&test_map()).unwrap().cells,
            terrain_before.cells,
            "撤销后地形逐格等于快照"
        );
    }

    /// 折线不足两点（`line.len() < 4`）→ `Precondition`，世界一个格都不动。
    #[test]
    fn create_rejects_degenerate_line() {
        let mut w = world_with_land();
        let terrain_before = w.terrain_of(&test_map()).unwrap().clone();
        let mut degenerate = line_place();
        degenerate.line = Some(vec![1.0, 1.0]);
        let cmd = CreateLinePoiCommand::new(test_map(), degenerate, Some(carve_spec()), 1.0);
        let err = cmd.apply(&mut w).expect_err("退化折线必须被拒收");
        assert!(matches!(err, EditError::Precondition(_)), "实得 {err:?}");
        assert!(
            !w.distances
                .places
                .iter()
                .any(|p| p.id.as_str() == "river_line"),
            "拒收后不得入表"
        );
        assert_eq!(
            w.terrain_of(&test_map()).unwrap().cells,
            terrain_before.cells,
            "拒收后地形不动"
        );
    }

    /// `None` = 不改：全 None 的属性命令 apply 后地点逐字段不变；
    /// 部分 Some 只动指定字段（改名不牵连基准宽 / 呈现级别）。
    #[test]
    fn update_line_poi_none_leaves_field_untouched() {
        let mut w = world_with_land();
        w.distances.places.push(line_place());
        let all_none = UpdateLinePoiCommand::new(Id::new_unchecked("river_line"), None, None, None);
        all_none.apply(&mut w).expect("apply");
        assert_eq!(w.distances.places[0].name, "小河", "None 不改名称");
        assert_eq!(
            w.distances.places[0].line_width_m,
            Some(12.0),
            "None 不改基准宽"
        );
        assert_eq!(w.distances.places[0].rank, None, "None 不改级别");

        // Some 只动指定字段：改名不牵连宽 / rank。
        let rename = UpdateLinePoiCommand::new(
            Id::new_unchecked("river_line"),
            Some("清河".into()),
            None,
            None,
        );
        rename.apply(&mut w).expect("apply");
        assert_eq!(w.distances.places[0].name, "清河", "显式改名应生效");
        assert_eq!(
            w.distances.places[0].line_width_m,
            Some(12.0),
            "改名不得牵连基准宽"
        );
        assert_eq!(w.distances.places[0].rank, None, "改名不得牵连级别");

        // revert 回到改名前（快照是「窄命令面」之外的整 Place，与
        // UpdatePlaceCommand 同款）。
        rename.revert(&mut w).expect("revert");
        assert_eq!(w.distances.places[0].name, "小河", "revert 回到改名前");
    }

    /// 不带下切的创建（`carve = None`）不依赖地形栅格——纯矢量线段 POI
    /// 可以先画线、回头再下切。
    #[test]
    fn create_without_carve_needs_no_terrain() {
        let mut w = World::default(); // 无任何地形栅格
        let cmd = CreateLinePoiCommand::new(test_map(), line_place(), None, 1.0);
        let dirty = cmd.apply(&mut w).expect("无下切不依赖地形");
        assert_eq!(dirty, vec!["distances/places".to_string()]);
        assert!(w
            .distances
            .places
            .iter()
            .any(|p| p.id.as_str() == "river_line"));
    }
}

#[cfg(test)]
mod trash_map_tests {
    use crate::command::*;
    use wv_core::{Id, World};

    fn map_node(
        id: &str,
        parent: Option<&str>,
        level: wv_core::map::MapLevel,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: format!("地图{id}"),
            level,
            parent: parent.map(Id::new_unchecked),
            ..wv_core::map::MapNode::default()
        }
    }
    // -----------------------------------------------------------------------
    // TrashMapCommand（地图树垃圾桶 · 2026-10-27 spec）
    // -----------------------------------------------------------------------

    fn trash_world() -> World {
        let mut w = World {
            maps: vec![
                map_node("world_main", None, wv_core::map::MapLevel::World),
                map_node(
                    "map_city_a",
                    Some("world_main"),
                    wv_core::map::MapLevel::City,
                ),
            ],
            ..World::default()
        };
        w.maps[0].children.push(Id::new_unchecked("map_city_a"));
        w.maps[0].icons.push(wv_core::map::IconSpec {
            id: Id::new_unchecked("icon_a"),
            anchor: Some([0.5, 0.5]),
            shape: wv_core::map::IconShape::Diamond,
            size: None,
            color: None,
            drill_into: Some(Id::new_unchecked("map_city_a")),
        });
        w
    }

    fn world_with_meta(root: &str, maps: Vec<wv_core::map::MapNode>) -> World {
        World {
            meta: Some(wv_core::WorldMeta {
                comment: None,
                name: "测试工程".into(),
                version: "0.1".into(),
                schema_version: 2,
                root_map: Id::new_unchecked(root),
                timeline_range: wv_core::time::TimeSpan { from: 0, to: None },
                template: None,
            }),
            maps,
            ..World::default()
        }
    }

    #[test]
    fn trash_marks_root_only_and_keeps_children_references() {
        let mut w = trash_world();
        let cmd = TrashMapCommand::new(Id::new_unchecked("map_city_a"), "2026-10-27 15:00");
        let dirty = cmd.apply(&mut w).expect("apply");
        let city = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "map_city_a")
            .expect("软删后节点仍在");
        assert!(city.trashed, "被删根应打标");
        assert_eq!(
            city.trashed_at.as_deref(),
            Some("2026-10-27 15:00"),
            "got {:?}",
            city.trashed_at
        );
        let parent = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "world_main")
            .expect("父图在");
        // spec §10 追记 1：children 引用与钻取图标原样保留，显隐由渲染派生。
        assert_eq!(parent.children.len(), 1, "children 引用应保留");
        assert_eq!(parent.icons.len(), 1, "钻取图标应保留");
        assert!(
            !w.map_alive(&Id::new_unchecked("map_city_a")),
            "子城应不可见"
        );
        assert_eq!(dirty, vec!["maps".to_string()]);
    }

    #[test]
    fn trash_twice_is_rejected() {
        let mut w = trash_world();
        let cmd = TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t0");
        cmd.apply(&mut w).expect("第一次应成功");
        let again = TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t1");
        let err = again.apply(&mut w).expect_err("重复删除应报错");
        assert!(
            err.to_string().contains("垃圾桶"),
            "错误信息应说明已在垃圾桶，got: {err}"
        );
    }

    #[test]
    fn trash_last_alive_root_is_rejected() {
        let mut w = World {
            maps: vec![map_node("world_solo", None, wv_core::map::MapLevel::World)],
            ..World::default()
        };
        let cmd = TrashMapCommand::new(Id::new_unchecked("world_solo"), "t0");
        let err = cmd.apply(&mut w).expect_err("最后一张地图不可删");
        assert!(err.to_string().contains("最后一张"), "got: {err}");
    }

    #[test]
    fn trash_root_map_rewrites_meta_and_revert_restores_it() {
        let mut maps = vec![
            map_node("world_a", None, wv_core::map::MapLevel::World),
            map_node("world_b", None, wv_core::map::MapLevel::World),
        ];
        maps[0].children.push(Id::new_unchecked("map_city_a"));
        maps.push(map_node(
            "map_city_a",
            Some("world_a"),
            wv_core::map::MapLevel::City,
        ));
        let mut w = world_with_meta("world_a", maps);
        let cmd = TrashMapCommand::new(Id::new_unchecked("world_a"), "t0");
        cmd.apply(&mut w).expect("apply");
        assert_eq!(
            w.meta.as_ref().expect("meta 在").root_map.as_str(),
            "world_b",
            "删 root_map 本尊应改写到幸存根"
        );
        cmd.revert(&mut w).expect("revert");
        assert_eq!(
            w.meta.as_ref().expect("meta 在").root_map.as_str(),
            "world_a",
            "revert 应还原 root_map"
        );
        assert!(
            !w.maps
                .iter()
                .find(|m| m.id.as_str() == "world_a")
                .expect("在")
                .trashed
        );
    }

    #[test]
    fn trash_revert_restores_alive_visibility() {
        let mut w = trash_world();
        let cmd = TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t0");
        cmd.apply(&mut w).expect("apply");
        assert!(!w.map_alive(&Id::new_unchecked("map_city_a")));
        cmd.revert(&mut w).expect("revert");
        assert!(
            w.map_alive(&Id::new_unchecked("map_city_a")),
            "撤销删除=恢复"
        );
        let city = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "map_city_a")
            .expect("在");
        assert_eq!(city.trashed_at, None, "revert 应清掉时间戳");
    }

    // -----------------------------------------------------------------------
    // RestoreMapCommand（垃圾桶恢复 · spec §5 D5）
    // -----------------------------------------------------------------------

    #[test]
    fn restore_unmarks_trashed_root_and_descendants_alive() {
        let mut w = trash_world();
        TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t0")
            .apply(&mut w)
            .expect("trash");
        let cmd = RestoreMapCommand::new(Id::new_unchecked("map_city_a"));
        let dirty = cmd.apply(&mut w).expect("restore");
        assert!(
            w.map_alive(&Id::new_unchecked("map_city_a")),
            "恢复后应存活"
        );
        let city = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "map_city_a")
            .expect("在");
        assert!(!city.trashed);
        assert_eq!(city.trashed_at, None);
        assert_eq!(dirty, vec!["maps".to_string()]);
        // 恢复也可撤销：revert 回到垃圾桶态。
        cmd.revert(&mut w).expect("revert");
        assert!(
            !w.map_alive(&Id::new_unchecked("map_city_a")),
            "撤销恢复=回到垃圾桶"
        );
    }

    #[test]
    fn restore_rejects_map_not_in_trash() {
        let mut w = trash_world();
        let err = RestoreMapCommand::new(Id::new_unchecked("map_city_a"))
            .apply(&mut w)
            .expect_err("未删除的图不可恢复");
        assert!(err.to_string().contains("垃圾桶"), "got: {err}");
    }

    #[test]
    fn restore_blocked_while_parent_still_trashed() {
        // 世界→城：先删城、再删世界，然后直接恢复城 → 必须先恢复父图。
        let mut maps = vec![
            map_node("world_main", None, wv_core::map::MapLevel::World),
            map_node(
                "map_city_a",
                Some("world_main"),
                wv_core::map::MapLevel::City,
            ),
        ];
        maps[0].children.push(Id::new_unchecked("map_city_a"));
        let mut w = World {
            maps: vec![
                map_node("world_b", None, wv_core::map::MapLevel::World), // 陪跑根，让删世界能过守卫
            ],
            ..World::default()
        };
        w.maps.extend(maps);
        TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t1")
            .apply(&mut w)
            .expect("trash city");
        TrashMapCommand::new(Id::new_unchecked("world_main"), "t2")
            .apply(&mut w)
            .expect("trash world");
        let err = RestoreMapCommand::new(Id::new_unchecked("map_city_a"))
            .apply(&mut w)
            .expect_err("父图在垃圾桶时应拒绝");
        assert!(err.to_string().contains("先恢复父图"), "got: {err}");
        // 恢复父图后，子图恢复放行。
        RestoreMapCommand::new(Id::new_unchecked("world_main"))
            .apply(&mut w)
            .expect("restore world");
        RestoreMapCommand::new(Id::new_unchecked("map_city_a"))
            .apply(&mut w)
            .expect("父图存活后子图可恢复");
    }

    #[test]
    fn restore_blocked_when_parent_was_purged() {
        let mut w = trash_world();
        TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t0")
            .apply(&mut w)
            .expect("trash");
        // 模拟父图被彻底删除：节点从 maps 移除。
        w.maps.retain(|m| m.id.as_str() != "world_main");
        let err = RestoreMapCommand::new(Id::new_unchecked("map_city_a"))
            .apply(&mut w)
            .expect_err("父图不存在时应拒绝");
        assert!(err.to_string().contains("彻底删除"), "got: {err}");
    }
}

#[cfg(test)]
mod paste_map_tests {
    use crate::command::*;
    use wv_core::map::{Extent, GeometryData, Layer, LayerKind, MapLevel, MapNode};
    use wv_core::{Id, World};

    fn node(id: &str, parent: Option<&str>, level: MapLevel, name: &str) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: name.into(),
            level,
            parent: parent.map(Id::new_unchecked),
            extent: Some(Extent {
                width: 200.0,
                height: 200.0,
                origin: Some(wv_core::Point::new(100.0, 100.0)),
            }),
            ..MapNode::default()
        }
    }

    /// 世界 W（extent 1000×1000）+ 城 C（路网图层 + 街道文档 + 地形 + 测量
    /// + 内部图 I）。粘贴测试的标准底座。
    fn fixture() -> World {
        let mut city = node("map_city_a", Some("world_main"), MapLevel::City, "青州城");
        city.layers.push(Layer {
            id: Id::new_unchecked("layer_roads_a"),
            name: "路网".into(),
            kind: LayerKind::Road,
            visible: true,
            locked: false,
            opacity: 1.0,
            z: 0,
            source: Some("maps/map_city_a/roads.yaml".into()),
        });
        city.outline_polygon = Some(GeometryData::Polygon(vec![
            100.0, 100.0, 140.0, 100.0, 140.0, 140.0, 100.0, 140.0,
        ]));
        city.children.push(Id::new_unchecked("map_interior_a"));
        let mut world = World {
            maps: vec![
                {
                    let mut w = node("world_main", None, MapLevel::World, "主世界");
                    w.extent = Some(Extent {
                        width: 1000.0,
                        height: 1000.0,
                        origin: None,
                    });
                    w.children.push(Id::new_unchecked("map_city_a"));
                    w
                },
                city,
                node(
                    "map_interior_a",
                    Some("map_city_a"),
                    MapLevel::Interior,
                    "市政厅",
                ),
            ],
            ..World::default()
        };
        world.set_terrain(
            Id::new_unchecked("map_city_a"),
            wv_core::terrain::TerrainData {
                width: 4,
                height: 4,
                cells: vec![wv_core::terrain::TerrainCellData::default(); 16],
            },
        );
        world.streets.insert(
            Id::new_unchecked("layer_roads_a"),
            wv_core::source::street::StreetLayer::default(),
        );
        world
            .measurements
            .insert(Id::new_unchecked("map_city_a"), Default::default());
        world
    }

    #[test]
    fn paste_city_clones_nodes_layers_and_terrain_with_new_ids() {
        let mut w = fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(500.0, 500.0),
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        let new_id = cmd.created_root().expect("created_root 应可读");
        assert_ne!(new_id.as_str(), "map_city_a", "ID 必须换新");
        assert_eq!(
            w.maps.len(),
            5,
            "3 张原图 + 城 + 内部图，got {}",
            w.maps.len()
        );
        let parent = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "world_main")
            .expect("父图在");
        assert_eq!(parent.children.len(), 2, "世界 children 应含新旧两城");
        let new_city = w.maps.iter().find(|m| m.id == new_id).expect("新城在");
        assert_eq!(new_city.name, "青州城 2", "got {}", new_city.name);
        assert_eq!(new_city.level, MapLevel::City);
        assert_eq!(new_city.children.len(), 1, "内部图应跟随");
        let new_interior = w
            .maps
            .iter()
            .find(|m| Some(m.id.clone()) == new_city.children.first().cloned())
            .expect("新内部图在");
        assert_eq!(new_interior.level, MapLevel::Interior);
        assert_eq!(new_interior.parent.as_ref(), Some(&new_id));
        assert_eq!(new_interior.name, "市政厅", "后代同名保留");
        // 图层：新 ID + source 路径改写到新图目录；街道文档挂新键。
        let new_layer = &new_city.layers[0];
        assert_ne!(new_layer.id.as_str(), "layer_roads_a", "图层 ID 必须换新");
        let src = new_layer.source.as_deref().unwrap_or("");
        assert!(
            src.starts_with(&format!("maps/{}/", new_id.as_str())),
            "source 应改写进新图目录，got {src}"
        );
        assert!(
            w.streets.contains_key(&new_layer.id),
            "街道文档应挂新图层键"
        );
        assert!(
            w.streets.contains_key(&Id::new_unchecked("layer_roads_a")),
            "原图层文档应保留"
        );
        assert!(w.terrains.contains_key(&new_id), "地形应按新图 ID 克隆");
        assert!(
            w.measurements.contains_key(&new_id),
            "测量叠加应按新图 ID 克隆"
        );
        assert!(dirty.contains(&"maps".to_owned()));
    }

    #[test]
    fn paste_numbers_name_on_subsequent_collisions() {
        let mut w = fixture();
        for expected in ["青州城 2", "青州城 3"] {
            let cmd = PasteMapCommand::new(
                Id::new_unchecked("map_city_a"),
                Some(Id::new_unchecked("world_main")),
                wv_core::Point::new(10.0, 10.0),
            );
            cmd.apply(&mut w).expect("apply");
            let new_id = cmd.created_root().expect("root");
            let name = w
                .maps
                .iter()
                .find(|m| m.id == new_id)
                .expect("在")
                .name
                .clone();
            assert_eq!(name, expected, "连续粘贴应依次补号，got {name}");
        }
    }

    #[test]
    fn paste_world_lands_as_independent_root() {
        let mut w = fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("world_main"),
            None,
            wv_core::Point::new(0.0, 0.0),
        );
        cmd.apply(&mut w).expect("apply");
        let new_id = cmd.created_root().expect("root");
        let new_root = w.maps.iter().find(|m| m.id == new_id).expect("在");
        assert!(new_root.parent.is_none(), "世界源粘贴 = 新独立根");
        assert_eq!(new_root.level, MapLevel::World);
        assert_eq!(new_root.name, "主世界 2", "根同级重名也补号");
        assert!(w.alive_roots().contains(&new_id), "新根应出现在存活根列表");
        // 新根下带走整套子树（青州城 + 市政厅）。
        assert_eq!(new_root.children.len(), 1);
        assert_eq!(w.maps.len(), 6, "3 原图 + 3 副本，got {}", w.maps.len());
    }

    #[test]
    fn paste_level_gating_rejects_wrong_target() {
        let mut w = fixture();
        // City 贴 City。
        let err = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("map_city_a")),
            wv_core::Point::new(0.0, 0.0),
        )
        .apply(&mut w)
        .expect_err("城市图只能贴世界图");
        assert!(err.to_string().contains("层级"), "got: {err}");
        // Interior 贴 World。
        let err = PasteMapCommand::new(
            Id::new_unchecked("map_interior_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(0.0, 0.0),
        )
        .apply(&mut w)
        .expect_err("内部图只能贴城市图");
        assert!(err.to_string().contains("层级"), "got: {err}");
        // World 贴在别的图下（必须落为独立根）。
        let err = PasteMapCommand::new(
            Id::new_unchecked("world_main"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(0.0, 0.0),
        )
        .apply(&mut w)
        .expect_err("世界图只能贴为新根");
        assert!(err.to_string().contains("层级"), "got: {err}");
    }

    #[test]
    fn paste_emits_per_id_dirty_paths_for_copied_data() {
        // C1（终审）：wv-io 的逐项落盘过滤器要精确路径——`maps/<id>/terrain.bin`
        // / `streets/<layer>` / `buildings/<layer>` / `measurements/<map>`。
        // 粗粒度 `maps` 只触发 index.yaml 重写，副本数据静默丢盘。
        let mut w = fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(500.0, 500.0),
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        let rid = cmd.created_root().expect("root");
        let new_layer = w.maps.iter().find(|m| m.id == rid).expect("新城在").layers[0]
            .id
            .clone();
        assert!(
            dirty.contains(&format!("maps/{}/terrain.bin", rid.as_str())),
            "副本地形必须给精确脏路径，got {dirty:?}"
        );
        assert!(
            dirty.contains(&format!("measurements/{}", rid.as_str())),
            "副本测量必须给精确脏路径，got {dirty:?}"
        );
        assert!(
            dirty.contains(&format!("streets/{}", new_layer.as_str())),
            "副本街道文档必须给精确脏路径，got {dirty:?}"
        );
    }

    #[test]
    fn paste_anchor_clamped_into_target_extent_and_shifts_outline() {
        let mut w = fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(9999.0, -50.0),
        );
        cmd.apply(&mut w).expect("apply");
        let rid = cmd.created_root().expect("root");
        let new_city = w.maps.iter().find(|m| m.id == rid).expect("在");
        let origin = new_city
            .extent
            .as_ref()
            .and_then(|e| e.origin.as_ref())
            .expect("锚定后应有 origin");
        // 目标 extent 1000×1000、origin 缺省按 (0,0)：x 夹到 1000，y 夹到 0。
        assert!(
            (origin.x - 1000.0).abs() < 1e-3,
            "x 应夹进目标图，got {}",
            origin.x
        );
        assert!(
            (origin.y - 0.0).abs() < 1e-3,
            "y 应夹进目标图，got {}",
            origin.y
        );
        // outline 随 delta=(900,-100) 平移：首顶点 (100,100) → (1000,0)。
        let GeometryData::Polygon(pts) = new_city.outline_polygon.as_ref().expect("outline 在")
        else {
            panic!("应仍是 Polygon");
        };
        assert!(
            (pts[0] - 1000.0).abs() < 1e-3 && (pts[1] - 0.0).abs() < 1e-3,
            "outline 应随锚点平移，got ({}, {})",
            pts[0],
            pts[1]
        );
    }
    /// 实体版底座：城市图上挂 Place×2 + 内部边 + 跨界边 + Region + Sea +
    /// place_geometry + faction_geometry；世界图上挂外部 Place（跨界边的另一端）。
    fn entity_fixture() -> World {
        let mut w = fixture();
        let place = |id: &str, map: Option<&str>| wv_core::distance::Place {
            kind: None,
            line: None,
            line_kind: None,
            line_width_m: None,
            id: Id::new_unchecked(id),
            name: format!("地点{id}"),
            feature: None,
            biome: None,
            envelope: None,
            map: map.map(Id::new_unchecked),
            x: Some(1.0),
            y: Some(2.0),
            w: None,
            h: None,
            rank: None,
        };
        let edge = |id: &str, a: &str, b: &str| wv_core::distance::PlaceEdge {
            rank: None,
            id: Id::new_unchecked(id),
            a: Id::new_unchecked(a),
            b: Id::new_unchecked(b),
            distance_km: 12.5,
            terrain: "road".into(),
            bidirectional: true,
            derived: false,
        };
        w.distances.places = vec![
            place("p_in_1", Some("map_city_a")),
            place("p_in_2", Some("map_city_a")),
            place("p_ext", Some("world_main")),
        ];
        // edge_in 两端都在城内（跟随）；edge_out 一端在外（丢弃）。
        w.distances.edges = vec![
            edge("edge_in", "p_in_1", "p_in_2"),
            edge("edge_out", "p_in_1", "p_ext"),
        ];
        w.regions.push(wv_core::model::region::Region {
            id: Id::new_unchecked("region_r1"),
            name: "东境".into(),
            map: Id::new_unchecked("map_city_a"),
            polygon: vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0],
            holes: Vec::new(),
            kind: wv_core::model::region::RegionKind::default(),
            parent: None,
            overlay: None,
        });
        w.seas.push(wv_core::map::sea::Sea {
            id: Id::new_unchecked("sea_s1"),
            name: "雾海".into(),
            map: Id::new_unchecked("map_city_a"),
            x: 5.0,
            y: 5.0,
        });
        let span = |map: &str, owner: &str| wv_core::map::GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: wv_core::time::TimeSpan { from: 0, to: None },
            geometry: wv_core::map::GeometryData::Polygon(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0]),
            relation: None,
        };
        w.place_geometry.push(span("map_city_a", "p_in_1"));
        w.faction_geometry.push(span("map_city_a", "faction_f"));
        w
    }

    #[test]
    fn paste_copies_places_regions_seas_and_geometry_with_remapped_ids() {
        let mut w = entity_fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(300.0, 300.0),
        );
        let dirty = cmd.apply(&mut w).expect("apply");
        let rid = cmd.created_root().expect("root");
        // Place：城内 2 个跟随（新 id、map 重映射、坐标保留），外部 1 个不动。
        let on_new: Vec<_> = w
            .distances
            .places
            .iter()
            .filter(|p| p.map.as_ref() == Some(&rid))
            .collect();
        assert_eq!(on_new.len(), 2, "got {}", on_new.len());
        for p in &on_new {
            assert!(
                !["p_in_1", "p_in_2"].contains(&p.id.as_str()),
                "Place ID 必须换新"
            );
            assert_eq!(p.x, Some(1.0), "本图局部坐标不动");
        }
        assert!(
            w.distances.places.iter().any(|p| p.id.as_str() == "p_ext"),
            "外部 Place 不动"
        );
        // Region / Sea：克隆 + map 重映射 + id 换新。
        let region = w
            .regions
            .iter()
            .find(|r| r.map == rid)
            .expect("Region 应克隆");
        assert_ne!(region.id.as_str(), "region_r1", "Region ID 必须换新");
        let sea = w.seas.iter().find(|s| s.map == rid).expect("Sea 应克隆");
        assert_ne!(sea.id.as_str(), "sea_s1", "Sea ID 必须换新");
        // place_geometry：map + owner 双重映射；faction_geometry：map 映射、owner（全局势力）保留。
        assert_eq!(
            w.place_geometry.iter().filter(|g| g.map == rid).count(),
            1,
            "地界 span 应跟随"
        );
        let fg: Vec<_> = w.faction_geometry.iter().filter(|g| g.map == rid).collect();
        assert_eq!(fg.len(), 1, "势力几何应跟随");
        assert_eq!(
            fg[0].owner.as_str(),
            "faction_f",
            "势力是全局实体，owner 保留"
        );
        // 原侧全部保留。
        assert!(w
            .place_geometry
            .iter()
            .any(|g| g.map.as_str() == "map_city_a"));
        assert!(w.regions.iter().any(|r| r.id.as_str() == "region_r1"));
        // 脏路径覆盖实体分支。
        for p in [
            "distances/places",
            "distances/edges",
            "geometry/place",
            "geometry/faction",
            "regions",
            "seas",
        ] {
            assert!(dirty.contains(&p.to_owned()), "脏路径缺 {p}");
        }
    }

    #[test]
    fn paste_drops_edges_touching_places_outside_copy_set() {
        let mut w = entity_fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(300.0, 300.0),
        );
        cmd.apply(&mut w).expect("apply");
        let rid = cmd.created_root().expect("root");
        // 新侧端点集合 = 新 Place 集合 → 只有内部边（1 条），跨界边被丢弃。
        let new_places: std::collections::HashSet<Id> = w
            .distances
            .places
            .iter()
            .filter(|p| p.map.as_ref() == Some(&rid))
            .map(|p| p.id.clone())
            .collect();
        let new_edges: Vec<_> = w
            .distances
            .edges
            .iter()
            .filter(|e| new_places.contains(&e.a) || new_places.contains(&e.b))
            .collect();
        assert_eq!(new_edges.len(), 1, "只应有内部边，got {}", new_edges.len());
        assert!(
            new_places.contains(&new_edges[0].a) && new_places.contains(&new_edges[0].b),
            "跟随边的端点必须都在复制集内"
        );
        // 原侧两条边不动。
        assert_eq!(w.distances.edges.len(), 3, "2 原 + 1 跟随");
    }

    #[test]
    fn revert_paste_removes_every_created_entity() {
        let mut w = entity_fixture();
        let cmd = PasteMapCommand::new(
            Id::new_unchecked("map_city_a"),
            Some(Id::new_unchecked("world_main")),
            wv_core::Point::new(300.0, 300.0),
        );
        cmd.apply(&mut w).expect("apply");
        cmd.revert(&mut w).expect("revert");
        assert_eq!(w.maps.len(), 3, "got {}", w.maps.len());
        assert_eq!(
            w.distances.places.len(),
            3,
            "got {}",
            w.distances.places.len()
        );
        assert_eq!(
            w.distances.edges.len(),
            2,
            "got {}",
            w.distances.edges.len()
        );
        assert_eq!(w.regions.len(), 1, "got {}", w.regions.len());
        assert_eq!(w.seas.len(), 1, "got {}", w.seas.len());
        assert_eq!(w.place_geometry.len(), 1, "got {}", w.place_geometry.len());
        assert_eq!(
            w.faction_geometry.len(),
            1,
            "got {}",
            w.faction_geometry.len()
        );
        assert_eq!(w.terrains.len(), 1, "got {}", w.terrains.len());
        assert_eq!(w.streets.len(), 1, "got {}", w.streets.len());
        assert_eq!(w.measurements.len(), 1, "got {}", w.measurements.len());
        assert_eq!(cmd.created_root(), None, "revert 后 root 回 None");
        let parent = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "world_main")
            .expect("在");
        assert_eq!(parent.children.len(), 1, "父 children 应回原状");
    }
}

#[cfg(test)]
mod purge_map_tests {
    use crate::command::*;
    use wv_core::map::{GeometryData, IconShape, IconSpec, MapLevel, MapNode};
    use wv_core::{Id, World};

    fn node(id: &str, parent: Option<&str>, level: MapLevel) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: format!("图{id}"),
            level,
            parent: parent.map(Id::new_unchecked),
            ..MapNode::default()
        }
    }

    /// 世界 W（icon→C）+ 城 C（place + region + sea + 双几何 + 内部图 I + 地形）
    /// + 外部 place p_ext + 跨界 edge。
    fn fixture() -> World {
        let mut city = node("map_city_a", Some("world_main"), MapLevel::City);
        city.children.push(Id::new_unchecked("map_interior_a"));
        let mut world_main = node("world_main", None, MapLevel::World);
        world_main.children.push(Id::new_unchecked("map_city_a"));
        world_main.icons.push(IconSpec {
            id: Id::new_unchecked("icon_a"),
            anchor: Some([0.5, 0.5]),
            shape: IconShape::Diamond,
            size: None,
            color: None,
            drill_into: Some(Id::new_unchecked("map_city_a")),
        });
        let mut w = World {
            maps: vec![
                world_main,
                city,
                node("map_interior_a", Some("map_city_a"), MapLevel::Interior),
            ],
            ..World::default()
        };
        w.distances.places = vec![
            wv_core::distance::Place {
                id: Id::new_unchecked("p_in"),
                name: "城内".into(),
                map: Some(Id::new_unchecked("map_city_a")),
                x: Some(1.0),
                y: Some(2.0),
                ..place_rest()
            },
            wv_core::distance::Place {
                id: Id::new_unchecked("p_ext"),
                name: "外部".into(),
                map: Some(Id::new_unchecked("world_main")),
                x: Some(3.0),
                y: Some(4.0),
                ..place_rest()
            },
        ];
        w.distances.edges = vec![wv_core::distance::PlaceEdge {
            id: Id::new_unchecked("edge_x"),
            a: Id::new_unchecked("p_in"),
            b: Id::new_unchecked("p_ext"),
            distance_km: 5.0,
            terrain: "road".into(),
            bidirectional: true,
            ..edge_rest()
        }];
        w.regions.push(wv_core::model::region::Region {
            id: Id::new_unchecked("region_r"),
            name: "东境".into(),
            map: Id::new_unchecked("map_city_a"),
            polygon: vec![0.0, 0.0, 4.0, 0.0],
            holes: Vec::new(),
            kind: wv_core::model::region::RegionKind::default(),
            parent: None,
            overlay: None,
        });
        w.seas.push(wv_core::map::sea::Sea {
            id: Id::new_unchecked("sea_s"),
            name: "雾海".into(),
            map: Id::new_unchecked("map_city_a"),
            x: 1.0,
            y: 1.0,
        });
        let span = |map: &str, owner: &str| wv_core::map::GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: wv_core::time::TimeSpan { from: 0, to: None },
            geometry: GeometryData::Polygon(vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0]),
            relation: None,
        };
        w.place_geometry.push(span("map_city_a", "p_in"));
        w.faction_geometry.push(span("map_city_a", "faction_f"));
        w.set_terrain(
            Id::new_unchecked("map_city_a"),
            wv_core::terrain::TerrainData {
                width: 2,
                height: 2,
                cells: vec![wv_core::terrain::TerrainCellData::default(); 4],
            },
        );
        w.measurements
            .insert(Id::new_unchecked("map_city_a"), Default::default());
        w
    }

    fn place_rest() -> wv_core::distance::Place {
        wv_core::distance::Place {
            kind: None,
            line: None,
            line_kind: None,
            line_width_m: None,
            id: Id::new_unchecked("_"),
            name: String::new(),
            feature: None,
            biome: None,
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
        }
    }

    fn edge_rest() -> wv_core::distance::PlaceEdge {
        wv_core::distance::PlaceEdge {
            rank: None,
            id: Id::new_unchecked("_"),
            a: Id::new_unchecked("_"),
            b: Id::new_unchecked("_"),
            distance_km: 0.0,
            terrain: String::new(),
            bidirectional: true,
            derived: false,
        }
    }

    #[test]
    fn purge_removes_subtree_entities_and_dangling_drill_icons() {
        let mut w = fixture();
        TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t0")
            .apply(&mut w)
            .expect("先入垃圾桶");
        let dirty = PurgeMapCommand::new(Id::new_unchecked("map_city_a"))
            .apply(&mut w)
            .expect("purge");
        assert_eq!(w.maps.len(), 1, "只剩世界图，got {}", w.maps.len());
        let world_main = &w.maps[0];
        assert!(world_main.children.is_empty(), "children 应摘除");
        assert!(world_main.icons.is_empty(), "悬空钻取图标应清理");
        assert_eq!(w.distances.places.len(), 1, "只剩外部 place");
        assert!(w.distances.edges.is_empty(), "跨界边随之消失");
        assert!(w.regions.is_empty());
        assert!(w.seas.is_empty());
        assert!(w.place_geometry.is_empty());
        assert!(w.faction_geometry.is_empty());
        assert!(w.terrains.is_empty());
        assert!(w.measurements.is_empty());
        for p in [
            "maps",
            "distances/places",
            "distances/edges",
            "geometry/place",
            "geometry/faction",
            "regions",
            "seas",
        ] {
            assert!(dirty.contains(&p.to_owned()), "脏路径缺 {p}");
        }
    }

    #[test]
    fn purge_rejects_alive_map() {
        let mut w = fixture();
        let err = PurgeMapCommand::new(Id::new_unchecked("map_city_a"))
            .apply(&mut w)
            .expect_err("未入垃圾桶的图不可彻底删除");
        assert!(err.to_string().contains("垃圾桶"), "got: {err}");
    }

    #[test]
    fn revert_purge_restores_everything() {
        let mut w = fixture();
        TrashMapCommand::new(Id::new_unchecked("map_city_a"), "t0")
            .apply(&mut w)
            .expect("trash");
        let cmd = PurgeMapCommand::new(Id::new_unchecked("map_city_a"));
        cmd.apply(&mut w).expect("purge");
        cmd.revert(&mut w).expect("revert 快照兜底");
        assert_eq!(w.maps.len(), 3, "got {}", w.maps.len());
        let world_main = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "world_main")
            .expect("在");
        assert_eq!(world_main.children.len(), 1, "children 应还原");
        assert_eq!(world_main.icons.len(), 1, "图标应还原");
        assert_eq!(
            w.distances.places.len(),
            2,
            "got {}",
            w.distances.places.len()
        );
        assert_eq!(w.distances.edges.len(), 1);
        assert_eq!(w.regions.len(), 1);
        assert_eq!(w.seas.len(), 1);
        assert_eq!(w.place_geometry.len(), 1);
        assert_eq!(w.faction_geometry.len(), 1);
        assert_eq!(w.terrains.len(), 1);
        assert_eq!(w.measurements.len(), 1);
        // 还原后的城仍在垃圾桶（purge 快照按原样保存，含 trashed 标志）。
        let city = w
            .maps
            .iter()
            .find(|m| m.id.as_str() == "map_city_a")
            .expect("在");
        assert!(city.trashed, "还原后应保持垃圾桶态");
    }
}
