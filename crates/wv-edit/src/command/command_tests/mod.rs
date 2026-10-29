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


// ── 按命令族的子模块 ──
mod layer_terrain;
mod faction;
mod settlement;
mod city_map;
mod measurement;
mod place_event;
mod city;
mod place_edges;
mod continent_marker;
mod regenerate;
mod map_props;
