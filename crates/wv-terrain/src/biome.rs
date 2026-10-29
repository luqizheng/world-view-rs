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
/// 海洋。显示名「咸水」（2026-10-27 用户裁定：水域群系表盐度不表岸体，
/// 原名「海洋」留档——编号与色值不变，改名仅显示层）。
pub const BIOME_OCEAN: Biome = 14;
/// 湖泊。显示名「淡水」（改名口径同 [`BIOME_OCEAN`]，原名「湖泊」留档）。
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

// ---------------------------------------------------------------------------
// 场景分区保留带（2026-10-29 · 场景编辑器 M1）
// ---------------------------------------------------------------------------

/// 场景分区编号带起点。
///
/// **为什么占用 biome 通道**：分区是「这张格子在城市里是什么功能区」，
/// 与群系一样是**格级自描述**的一维属性，且随 `TerrainCell` 一起走完整条
/// 链路（存盘 → CPU 烘焙 → GPU 场）。另起一条通道要改 `terrain.bin` 版本、
/// 加 254 处结构体字面量，收益远小于代价。
///
/// **为什么落在冻结表 21 之上**：0..=20 是冻结的生物群系编号表
/// （含唯一的工程扩展「道路」），分区不挤占它。
///
/// **为什么不归任何族**：分区是人为功能区，不是自然群系。`family_of` 对
/// 21..=32 返回 `None`——主题覆盖物规则（ADR-0032 D2）以族为键，分区不
/// 落族即不会被「森林规则」「道路规则」误触发。
pub const DISTRICT_FIRST: Biome = 21;
/// 场景分区数量（带内码位数）。
pub const DISTRICT_COUNT: u8 = 12;

/// 场景分区中文名（索引 = [`district_index`] 的返回值）。
///
/// 只放中文：分区名当前只有中文一种形态，UI 直查本表
/// （与 `biome_raw::BIOME_NAMES_ZH` 的现状一致），不进 i18n 体系。
pub const DISTRICT_NAMES_ZH: [&str; DISTRICT_COUNT as usize] = [
    "未分区",   // 21
    "贫民区",   // 22
    "富人区",   // 23
    "普通住宅", // 24
    "商业区",   // 25
    "工业区",   // 26
    "农田",     // 27
    "密林",     // 28
    "空地",     // 29
    "房间",     // 30
    "走廊",     // 31
    "大殿",     // 32
];

/// 该编号是否落在场景分区保留带内。
pub fn is_district(b: Biome) -> bool {
    (DISTRICT_FIRST..DISTRICT_FIRST + DISTRICT_COUNT).contains(&b)
}

/// 分区编号 → 带内索引（0..=11）；非分区编号返 `None`。
///
/// 索引即色表索引——主题色表与 CellMap 固定色表都按这个顺序排。
pub fn district_index(b: Biome) -> Option<u8> {
    if is_district(b) {
        Some(b - DISTRICT_FIRST)
    } else {
        None
    }
}

/// 浅水与深水的显示分界（米）：原 `continent.rs` 的 Q5 兜底口径，随分带
/// 迁入本模块（三通道后分带不再需要它，显示投影仍需要）。
const SHALLOW_WATER_MAX_DEPTH_M: i16 = -200;

/// 群系族（ADR-0032 D2）：主题覆盖物等**演绎规则**的键粒度。
///
/// 冻结群系表 21 编号按 `display_kind` 陆面槽位语义划分为八族（含水域族），
/// 划分**穷尽**全部编号。钉死的不变式是**槽位细分族**：同槽位的陆面群系必
/// 同族；族可跨多个槽位（高地族横跨丘陵/山地；水域族按深度分 0/1）——
/// 见测试 `slot_partition_refines_family_partition`。主题包以族为规则键——
/// 写一条 `Forest` 规则即覆盖森林/丛林/热带雨林三群系，杜绝旧数字键
/// 「写 1 漏 10/11」的碎裂。未来新增群系编号必须归入既有族或新开族
/// （后者是 ADR 事件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BiomeFamily {
    /// 森林族：森林(1)、丛林(10)、热带雨林(11)。
    Forest,
    /// 草原族：平原(2)、草原(3)、绿洲(17)、稀树草原(18)。
    Grassland,
    /// 干旱族：戈壁(4)、沙漠(5)、盐碱地(19)。
    Desert,
    /// 湿地族：沼泽(6)、湿地(7)。
    Wetland,
    /// 寒带族：苔原(8)、雪原(9)。
    Cold,
    /// 高地族：丘陵(12)、山地(13)。
    Highland,
    /// 水域族：海洋(14)、湖泊(15)、河流(16)。
    Water,
    /// 道路族：道路(20)。
    Road,
}

/// 群系编号 → 所属族；`BIOME_NONE`(0) 与未登记编号（开放集）返回 `None`。
///
/// 族界与 [`display_kind`] 的陆面槽位分带逐槽对齐（绿洲入草原族即从该
/// 先例）；`is_forest_family` 经此单源实现。
pub fn family_of(b: Biome) -> Option<BiomeFamily> {
    match b {
        BIOME_FOREST | BIOME_JUNGLE | BIOME_TROPICAL_RAINFOREST => Some(BiomeFamily::Forest),
        BIOME_PLAINS | BIOME_GRASSLAND | BIOME_SAVANNA | BIOME_OASIS => {
            Some(BiomeFamily::Grassland)
        }
        BIOME_GOBI | BIOME_DESERT | BIOME_SALT_FLAT => Some(BiomeFamily::Desert),
        BIOME_SWAMP | BIOME_WETLAND => Some(BiomeFamily::Wetland),
        BIOME_TUNDRA | BIOME_SNOWFIELD => Some(BiomeFamily::Cold),
        BIOME_HILLS | BIOME_MOUNTAINS => Some(BiomeFamily::Highland),
        BIOME_OCEAN | BIOME_LAKE | BIOME_RIVER => Some(BiomeFamily::Water),
        BIOME_ROAD => Some(BiomeFamily::Road),
        _ => None,
    }
}

/// 森林族（森林 / 丛林 / 热带雨林）：主题覆盖物与近景符号共用的判定。
///
/// ADR-0032 后经 [`family_of`] 单源实现（`Some(BiomeFamily::Forest)`）。
pub fn is_forest_family(b: Biome) -> bool {
    matches!(family_of(b), Some(BiomeFamily::Forest))
}

/// 族序表（ADR-0081 自然蛋糕）：`PartitionFace.kind` 跨 crate 存**本表下标**
/// （wv-core 不依赖 wv-terrain，u8 是两边唯一的交汇类型——序即契约，
/// **只增不改**；新族必须追加到表尾）。
pub const BIOME_FAMILY_ORDER: [BiomeFamily; 8] = [
    BiomeFamily::Forest,
    BiomeFamily::Grassland,
    BiomeFamily::Desert,
    BiomeFamily::Wetland,
    BiomeFamily::Cold,
    BiomeFamily::Highland,
    BiomeFamily::Water,
    BiomeFamily::Road,
];

/// 族显示名（与 [`BIOME_FAMILY_ORDER`] 同序）：自然蛋糕的「草原N / 沙漠N」
/// 命名前缀取自本表。
pub const BIOME_FAMILY_NAMES_ZH: [&str; 8] = [
    "森林", "草原", "干旱", "湿地", "寒带", "高地", "水域", "道路",
];

/// 下标 → 族；越界（坏 YAML）返回 `None`。
pub fn family_at(index: u8) -> Option<BiomeFamily> {
    BIOME_FAMILY_ORDER.get(index as usize).copied()
}

/// 族 → 下标（[`BIOME_FAMILY_ORDER`] 的反查）。
pub fn family_index(f: BiomeFamily) -> u8 {
    BIOME_FAMILY_ORDER
        .iter()
        .position(|&x| x == f)
        .expect("族序表穷尽 BiomeFamily（编译期保证）") as u8
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

    /// 族序表契约（ADR-0081 自然蛋糕）：PartitionFace.kind 存本表下标——
    /// 序即契约只增不改；roundtrip 与名称表同长必须钉死。
    #[test]
    fn family_order_index_roundtrip_and_names_align() {
        assert_eq!(BIOME_FAMILY_ORDER.len(), 8);
        assert_eq!(BIOME_FAMILY_NAMES_ZH.len(), BIOME_FAMILY_ORDER.len());
        for (i, &f) in BIOME_FAMILY_ORDER.iter().enumerate() {
            assert_eq!(family_index(f), i as u8, "族 {f:?} 的下标应为 {i}");
            assert_eq!(family_at(i as u8), Some(f));
        }
        assert_eq!(family_at(8), None, "越界 = 坏 YAML → None");
        // 命名前缀锚点：「草原N / 沙漠N」的草原=1、干旱族显示名含「干旱」。
        assert_eq!(BIOME_FAMILY_NAMES_ZH[1], "草原");
    }

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

    // -----------------------------------------------------------------------
    // 群系族（ADR-0032 D2）
    // -----------------------------------------------------------------------

    /// 冻结表 21 编号的期望族归属，逐条断言（含 NONE → None）。
    #[test]
    fn family_of_covers_every_frozen_biome_id() {
        assert_eq!(family_of(BIOME_NONE), None, "NONE 无族");
        assert_eq!(family_of(BIOME_FOREST), Some(BiomeFamily::Forest));
        assert_eq!(family_of(BIOME_JUNGLE), Some(BiomeFamily::Forest));
        assert_eq!(
            family_of(BIOME_TROPICAL_RAINFOREST),
            Some(BiomeFamily::Forest)
        );
        assert_eq!(family_of(BIOME_PLAINS), Some(BiomeFamily::Grassland));
        assert_eq!(family_of(BIOME_GRASSLAND), Some(BiomeFamily::Grassland));
        assert_eq!(family_of(BIOME_OASIS), Some(BiomeFamily::Grassland));
        assert_eq!(family_of(BIOME_SAVANNA), Some(BiomeFamily::Grassland));
        assert_eq!(family_of(BIOME_GOBI), Some(BiomeFamily::Desert));
        assert_eq!(family_of(BIOME_DESERT), Some(BiomeFamily::Desert));
        assert_eq!(family_of(BIOME_SALT_FLAT), Some(BiomeFamily::Desert));
        assert_eq!(family_of(BIOME_SWAMP), Some(BiomeFamily::Wetland));
        assert_eq!(family_of(BIOME_WETLAND), Some(BiomeFamily::Wetland));
        assert_eq!(family_of(BIOME_TUNDRA), Some(BiomeFamily::Cold));
        assert_eq!(family_of(BIOME_SNOWFIELD), Some(BiomeFamily::Cold));
        assert_eq!(family_of(BIOME_HILLS), Some(BiomeFamily::Highland));
        assert_eq!(family_of(BIOME_MOUNTAINS), Some(BiomeFamily::Highland));
        assert_eq!(family_of(BIOME_OCEAN), Some(BiomeFamily::Water));
        assert_eq!(family_of(BIOME_LAKE), Some(BiomeFamily::Water));
        assert_eq!(family_of(BIOME_RIVER), Some(BiomeFamily::Water));
        assert_eq!(family_of(BIOME_ROAD), Some(BiomeFamily::Road));
    }

    /// 开放集的未登记编号（≥ 21）无族——主题包自定群系不落入内置规则。
    #[test]
    fn family_of_returns_none_for_unregistered_ids() {
        for b in [21u8, 42, 100, 255] {
            assert_eq!(family_of(b), None, "编号 {b} 应无族");
        }
        // 21..=32 是**场景分区保留带**（见 [`DISTRICT_FIRST`]）：分区同样无族——
        // 分区是人为功能区，不是生物群系，主题覆盖物规则不应误触发。
        for b in DISTRICT_FIRST..DISTRICT_FIRST + DISTRICT_COUNT {
            assert_eq!(family_of(b), None, "分区 {b} 不应落任何群系族");
        }
    }

    /// 场景分区保留带：从冻结群系表 21 之上起，12 个码位，带外不入。
    #[test]
    fn district_band_is_reserved_above_frozen_biomes() {
        assert_eq!(DISTRICT_FIRST, 21, "分区带必须从冻结表 20 之上起");
        assert_eq!(DISTRICT_COUNT, 12);
        assert_eq!(DISTRICT_NAMES_ZH.len(), 12);
        for i in 0..DISTRICT_COUNT {
            let b = DISTRICT_FIRST + i;
            assert!(is_district(b), "{b} 应是分区");
            assert_eq!(district_index(b), Some(i));
            assert!(!DISTRICT_NAMES_ZH[i as usize].is_empty(), "分区 {b} 缺名");
        }
        assert!(!is_district(DISTRICT_FIRST - 1), "20 是道路群系，不是分区");
        assert!(
            !is_district(DISTRICT_FIRST + DISTRICT_COUNT),
            "带外不是分区"
        );
        assert_eq!(district_index(255), None);
        assert_eq!(district_index(0), None);
    }

    /// 森林族判定与 `family_of` 单源一致：全编号域上
    /// `is_forest_family(b) == (family_of(b) == Some(Forest))`。
    #[test]
    fn is_forest_family_agrees_with_family_of_forest() {
        for b in 0..=255u8 {
            assert_eq!(
                is_forest_family(b),
                family_of(b) == Some(BiomeFamily::Forest),
                "编号 {b} 两口径不一致"
            );
        }
    }

    /// 不变式（ADR-0032 D2）：槽位划分**细分**族划分——同槽位的两个陆面
    /// 群系必同族（高地族横跨丘陵/山地两槽位是唯一允许的合并方向）。
    #[test]
    fn slot_partition_refines_family_partition() {
        let land: [Biome; 16] = [
            BIOME_FOREST,
            BIOME_JUNGLE,
            BIOME_TROPICAL_RAINFOREST,
            BIOME_PLAINS,
            BIOME_GRASSLAND,
            BIOME_OASIS,
            BIOME_SAVANNA,
            BIOME_GOBI,
            BIOME_DESERT,
            BIOME_SALT_FLAT,
            BIOME_SWAMP,
            BIOME_WETLAND,
            BIOME_TUNDRA,
            BIOME_SNOWFIELD,
            BIOME_HILLS,
            BIOME_MOUNTAINS,
        ];
        let cell = |b: Biome| TerrainCell {
            height: 100,
            surface: SURFACE_LAND,
            biome: b,
        };
        for a in land {
            for b in land {
                let slot_a = display_kind(&cell(a));
                let slot_b = display_kind(&cell(b));
                if slot_a == slot_b {
                    assert_eq!(
                        family_of(a),
                        family_of(b),
                        "槽位相同（{slot_a}）的 {a} 与 {b} 必须同族"
                    );
                }
            }
        }
    }
}
