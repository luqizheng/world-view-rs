//! 生物群系编号表 + 三通道 → 旧调色板槽位投影（设计稿 `2026-10-18-world-from-zero-map-design.md`
//! D10/D11/D12）。
//!
//! ## 编号表是唯一的
//!
//! `Biome = u8` 开放集的**冻结编号表**落在本模块（设计稿 §6.1 的 19 清单 +
//! 1 项工程扩展）。编号 → 名称 → 色值的映射仍由上层（Theme Pack / wv-app）
//! 承载；本层只锚定编号与「群系 → 旧 0–9 调色板槽位」的投影
//! （[`display_kind`]），让渲染层在 Theme Pack 化之前零改动续用既有色板。
//!
//! ## 湿判定与 authoring 真值分层（D11）
//!
//! surface 通道是 authoring 真值；[`crate::edit::is_water_cell`] 的湿判定
//! 额外包含「海拔 < 0」。[`display_kind`] 按湿判定给水槽位（0/1），保证
//! 被塑笔刷压到海下的陆格照旧显示为海。

use crate::continent::{HILL_MIN_M, MOUNTAIN_MIN_M, SNOW_LINE_M};
use crate::{Surface, TerrainCell, SURFACE_LAND, SURFACE_WATER};

// ---------------------------------------------------------------------------
// 冻结编号表（2026-10-18 计划 Task 2；与 wv-io 的 v1 迁移表必须一致）
// ---------------------------------------------------------------------------

/// 无群系（显示走高度回退）。
pub const BIOME_NONE: Biome = 0;
/// 森林。
pub const BIOME_FOREST: Biome = 1;
/// 平原。
pub const BIOME_PLAINS: Biome = 2;
/// 草原。
pub const BIOME_GRASSLAND: Biome = 3;
/// 戈壁。
pub const BIOME_GOBI: Biome = 4;
/// 沙漠。
pub const BIOME_DESERT: Biome = 5;
/// 沼泽。
pub const BIOME_SWAMP: Biome = 6;
/// 湿地。
pub const BIOME_WETLAND: Biome = 7;
/// 苔原。
pub const BIOME_TUNDRA: Biome = 8;
/// 雪原。
pub const BIOME_SNOWFIELD: Biome = 9;
/// 丛林。
pub const BIOME_JUNGLE: Biome = 10;
/// 热带雨林。
pub const BIOME_TROPICAL_RAINFOREST: Biome = 11;
/// 丘陵。
pub const BIOME_HILLS: Biome = 12;
/// 山地。
pub const BIOME_MOUNTAINS: Biome = 13;
/// 海洋。
pub const BIOME_OCEAN: Biome = 14;
/// 湖泊。
pub const BIOME_LAKE: Biome = 15;
/// 河流。
pub const BIOME_RIVER: Biome = 16;
/// 绿洲。
pub const BIOME_OASIS: Biome = 17;
/// 稀树草原。
pub const BIOME_SAVANNA: Biome = 18;
/// 盐碱地。
pub const BIOME_SALT_FLAT: Biome = 19;
/// 道路。**19 清单之外的唯一工程扩展**：道路非自然生物群系，但既有笔刷
/// 色板有道路槽（旧 kind 9），保住该绘制能力；Theme Pack 化时可重审。
pub const BIOME_ROAD: Biome = 20;

/// 生物群系编号。
pub type Biome = u8;

/// 浅水与深水的显示分界（米）：原 `continent.rs` 的 Q5 兜底口径，随分带
/// 迁入本模块（三通道后分带不再需要它，显示投影仍需要）。
const SHALLOW_WATER_MAX_DEPTH_M: i16 = -200;

/// 森林族（森林 / 丛林 / 热带雨林）：主题覆盖物与近景符号共用的判定。
pub fn is_forest_family(b: Biome) -> bool {
    matches!(b, BIOME_FOREST | BIOME_JUNGLE | BIOME_TROPICAL_RAINFOREST)
}

/// 群系的默认地表：海洋/湖泊/河流是水，其余（含 `NONE`）按陆处理。
pub fn surface_for_biome(b: Biome) -> Surface {
    match b {
        BIOME_OCEAN | BIOME_LAKE | BIOME_RIVER => SURFACE_WATER,
        _ => SURFACE_LAND,
    }
}

/// 三通道 → 旧 0–9 调色板槽位（`wv-render/theme/colors.rs::CORE_TERRAIN` 索引）。
///
/// 这是渲染取色的**唯一投影入口**：色板数组不动，槽位映射只活在这一处。
/// 映射表：湿格按深度给 0/1；陆格按群系给 2–9；`NONE` 落高度回退
/// （2600/1200/300 米三阈值，与大陆分带同源）。
pub fn display_kind(c: &TerrainCell) -> u8 {
    let wet = c.surface == SURFACE_WATER || c.height < 0;
    if wet {
        return if c.height < SHALLOW_WATER_MAX_DEPTH_M {
            0
        } else {
            1
        };
    }
    match c.biome {
        BIOME_DESERT | BIOME_GOBI | BIOME_SALT_FLAT => 2,
        BIOME_PLAINS | BIOME_GRASSLAND | BIOME_SAVANNA | BIOME_OASIS => 3,
        BIOME_FOREST | BIOME_JUNGLE | BIOME_TROPICAL_RAINFOREST => 4,
        BIOME_HILLS => 5,
        BIOME_MOUNTAINS => 6,
        BIOME_SNOWFIELD | BIOME_TUNDRA => 7,
        BIOME_SWAMP | BIOME_WETLAND => 8,
        BIOME_ROAD => 9,
        // 水系群系落在陆面（数据不一致）时按浅水兜底，不 panic（V1 精神）。
        BIOME_OCEAN | BIOME_LAKE | BIOME_RIVER => 1,
        BIOME_NONE => {
            if c.height >= SNOW_LINE_M {
                7
            } else if c.height >= MOUNTAIN_MIN_M {
                6
            } else if c.height >= HILL_MIN_M {
                5
            } else {
                3
            }
        }
        // 未登记编号（开放集，Theme Pack 自定群系）按草原兜底。
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BIOME_DESERT, BIOME_FOREST, BIOME_GOBI, BIOME_GRASSLAND, BIOME_HILLS, BIOME_JUNGLE,
        BIOME_LAKE, BIOME_MOUNTAINS, BIOME_OASIS, BIOME_OCEAN, BIOME_PLAINS, BIOME_RIVER,
        BIOME_ROAD, BIOME_SALT_FLAT, BIOME_SAVANNA, BIOME_SNOWFIELD, BIOME_SWAMP, BIOME_TUNDRA,
        BIOME_WETLAND, SURFACE_LAND, SURFACE_WATER,
    };

    #[test]
    fn display_kind_maps_each_land_biome_to_legacy_palette_slot() {
        let cell = |b: Biome| TerrainCell {
            height: 100,
            surface: SURFACE_LAND,
            biome: b,
        };
        assert_eq!(display_kind(&cell(BIOME_DESERT)), 2);
        assert_eq!(display_kind(&cell(BIOME_GOBI)), 2);
        assert_eq!(display_kind(&cell(BIOME_SALT_FLAT)), 2);
        assert_eq!(display_kind(&cell(BIOME_GRASSLAND)), 3);
        assert_eq!(display_kind(&cell(BIOME_PLAINS)), 3);
        assert_eq!(display_kind(&cell(BIOME_SAVANNA)), 3);
        assert_eq!(display_kind(&cell(BIOME_OASIS)), 3);
        assert_eq!(display_kind(&cell(BIOME_FOREST)), 4);
        assert_eq!(display_kind(&cell(BIOME_JUNGLE)), 4);
        assert_eq!(display_kind(&cell(BIOME_HILLS)), 5);
        assert_eq!(display_kind(&cell(BIOME_MOUNTAINS)), 6);
        assert_eq!(display_kind(&cell(BIOME_SNOWFIELD)), 7);
        assert_eq!(display_kind(&cell(BIOME_TUNDRA)), 7);
        assert_eq!(display_kind(&cell(BIOME_SWAMP)), 8);
        assert_eq!(display_kind(&cell(BIOME_WETLAND)), 8);
        assert_eq!(display_kind(&cell(BIOME_ROAD)), 9);
    }

    #[test]
    fn display_kind_falls_back_to_height_bands_when_biome_none() {
        let cell = |h: i16| TerrainCell {
            height: h,
            surface: SURFACE_LAND,
            biome: BIOME_NONE,
        };
        assert_eq!(display_kind(&cell(299)), 3, "got below hill line");
        assert_eq!(display_kind(&cell(300)), 5, "got at hill line");
        assert_eq!(display_kind(&cell(1199)), 5);
        assert_eq!(display_kind(&cell(1200)), 6, "got at mountain line");
        assert_eq!(display_kind(&cell(2599)), 6);
        assert_eq!(display_kind(&cell(2600)), 7, "got at snow line");
    }

    #[test]
    fn wet_cells_take_water_slots_by_depth_and_biome_beats_height() {
        let deep = TerrainCell {
            height: -3000,
            surface: SURFACE_WATER,
            biome: BIOME_NONE,
        };
        let lake_on_mountain = TerrainCell {
            height: 10,
            surface: SURFACE_WATER,
            biome: BIOME_LAKE,
        };
        assert_eq!(display_kind(&deep), 0);
        assert_eq!(display_kind(&lake_on_mountain), 1);
        // 沉入海底的陆格（塑笔刷产物）按湿渲染。
        let sunken = TerrainCell {
            height: -100,
            surface: SURFACE_LAND,
            biome: BIOME_NONE,
        };
        assert_eq!(display_kind(&sunken), 1);
    }

    #[test]
    fn surface_for_biome_classifies_water_biomes() {
        assert_eq!(surface_for_biome(BIOME_OCEAN), SURFACE_WATER);
        assert_eq!(surface_for_biome(BIOME_LAKE), SURFACE_WATER);
        assert_eq!(surface_for_biome(BIOME_RIVER), SURFACE_WATER);
        assert_eq!(surface_for_biome(BIOME_FOREST), SURFACE_LAND);
        assert_eq!(surface_for_biome(BIOME_NONE), SURFACE_LAND);
    }
}
