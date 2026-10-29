//! UI 样式快照 shim（ADR-0026 D3）。
//!
//! `UiShell` 本体在 [`wv_ui::shell`]；此处 `pub use` 保持
//! `crate::ui::shell::UiShell` 全部旧路径零改动。地形两 helper **留守**——
//! 它们依赖 `crate::i18n` 与 `crate::theme::colors`（世界色板，wv_render
//! re-export），不属公共控件。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::i18n::{t, I18nKey, Locale};

pub use wv_ui::shell::UiShell;

/// 9 种地形的 i18n Key 表（与 `crate::i18n::table.rs` 的 zh/en 镜像同序）。
/// 「道路」（旧第 10 项）已退役（ADR-0070 D1）：路改由线段 POI 承载，
/// 不再入笔刷色板；i18n 键本体保留（存量格编号 9 的显示名仍在表内）。
const TERRAIN_KEYS: [I18nKey; 9] = [
    I18nKey::TerrainKindDeepWater,
    I18nKey::TerrainKindShallowWater,
    I18nKey::TerrainKindBeach,
    I18nKey::TerrainKindGrassland,
    I18nKey::TerrainKindForest,
    I18nKey::TerrainKindHill,
    I18nKey::TerrainKindMountain,
    I18nKey::TerrainKindSnow,
    I18nKey::TerrainKindSwamp,
];

/// 笔刷材料条的**唯一权威表**（地形档序号 = 笔刷色板序号）。
///
/// **为什么要有这张表**：材质条（`ui/top_option_bar.rs` 的 `material_strip`）
/// 与显示名表（`TERRAIN_KEYS`）曾各写各的循环上界——材质条画 10 块而名字表
/// 只有 9 项，多出的第 10 块 tooltip 是 `"?"`、点了还会涂出海（`brush_target`
/// 对越界档落 `_ => WATER`）。ADR-0070 D1 退役道路档时漏改了循环上界。
/// 两处都改成遍历本表，漂移无处可生。
pub const MATERIALS: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 7, 8];

/// 场景分区材料（biome 保留带 21..=32 的**人类城市词表**）。
///
/// **本轮只开放人类聚落**：未分区 + 贫民 / 富人 / 普通住宅 / 商业 / 工业。
/// 地下城（房间 / 走廊 / 大殿）、森林（密林 / 空地）、农田的码位与名字
/// 已在 `wv_terrain::biome::DISTRICT_NAMES_ZH` 登记（12 个全在），
/// 扩展只需往本表加码——词表是数据，不是代码。
pub const DISTRICT_MATERIALS: &[u8] = &[
    wv_terrain::biome::DISTRICT_FIRST,     // 未分区（基底）
    wv_terrain::biome::DISTRICT_FIRST + 1, // 贫民区
    wv_terrain::biome::DISTRICT_FIRST + 2, // 富人区
    wv_terrain::biome::DISTRICT_FIRST + 3, // 普通住宅
    wv_terrain::biome::DISTRICT_FIRST + 4, // 商业区
    wv_terrain::biome::DISTRICT_FIRST + 5, // 工业区
];

/// 按索引返回本地化后的地形名。索引越界时返回 `"?"`（防御性兜底，绝不在 UI 字符串里 panic）。
///
/// 场景分区码（21..=32）走 [`wv_terrain::biome::DISTRICT_NAMES_ZH`] 直查中文——
/// 分区名当前只有中文一种形态，与 CellMap 侧 `biome_name()` 的现状一致。
pub fn terrain_name_at(loc: Locale, idx: usize) -> &'static str {
    if idx >= wv_terrain::biome::DISTRICT_FIRST as usize {
        return match wv_terrain::biome::district_index(idx as u8) {
            Some(i) => wv_terrain::biome::DISTRICT_NAMES_ZH[i as usize],
            None => "?",
        };
    }
    match TERRAIN_KEYS.get(idx) {
        Some(k) => t(loc, *k),
        None => "?",
    }
}

/// 地形色块索引 → `palette::CORE_TERRAIN` 同序（§4.5：色块就是地形本色）。
pub fn terrain_swatch(kind: u8) -> egui::Color32 {
    // 场景分区走旁挂分区色表（主题侧），不经旧槽位投影。
    if let Some(idx) = wv_terrain::biome::district_index(kind) {
        let c = crate::theme::palette::PaletteColors::DEFAULT.district[idx as usize];
        let bevy::prelude::Color::Srgba(s) = c else {
            return egui::Color32::GRAY;
        };
        return egui::Color32::from_rgb(
            (s.red * 255.0) as u8,
            (s.green * 255.0) as u8,
            (s.blue * 255.0) as u8,
        );
    }
    let c = crate::theme::colors::core_terrain(kind);
    let bevy::prelude::Color::Srgba(s) = c else {
        return egui::Color32::GRAY;
    };
    egui::Color32::from_rgb(
        (s.red * 255.0) as u8,
        (s.green * 255.0) as u8,
        (s.blue * 255.0) as u8,
    )
}

/// 笔刷色板 → 三通道写入目标（**唯一权威表**，`BrushState.kind` 是色板序号、
/// 不是数据通道；设计稿 `2026-10-18-world-from-zero-map-design.md` D12/D19
/// 与计划 Task 6）。
///
/// 9 档（ADR-0070 D1 退役后 10 → 9）：水板写水面 + **咸水**群系（2026-10-27
/// 用户裁定 B 案：涂水自动带盐度群系——原名 BIOME_NONE 让涂出的水挂「无群
/// 系」，是水陆群系错配的来源之一；湖面淡水走湖意图 / 群系面板手涂）；丘陵/
/// 山地/雪原按群系落库（与「中央山」标记同语义，D9）——它们也可不写群系靠
/// 高度派生显示，两路并存即 D16。
///
/// 「道路」档（旧 swatch 9）已退役：路改由线段 POI 承载（ADR-0070 D1），
/// 分支删除后 swatch 9 落 `_ => WATER` 兜底——存量 `BIOME_ROAD` 格的读取
/// 防御在 `wv_terrain`（常量与 `family_of` 不退役），作者入口不再产出。
pub fn brush_target(swatch: u8) -> wv_terrain::edit::PaintTarget {
    use wv_terrain::edit::PaintTarget;
    use wv_terrain::{
        BIOME_DESERT, BIOME_FOREST, BIOME_GRASSLAND, BIOME_HILLS, BIOME_MOUNTAINS, BIOME_OCEAN,
        BIOME_SNOWFIELD, BIOME_SWAMP, SURFACE_LAND, SURFACE_WATER,
    };
    const WATER: PaintTarget = PaintTarget {
        surface: SURFACE_WATER,
        biome: BIOME_OCEAN,
    };
    const LAND: fn(u8) -> PaintTarget = |biome| PaintTarget {
        surface: SURFACE_LAND,
        biome,
    };
    // 场景分区：写陆面 + 分区码。高度由 `wv_terrain::apply_brush` 的
    // 「分区不改高度」规则保证（分区是同一张基底图上的涂色）。
    if let Some(idx) = wv_terrain::biome::district_index(swatch) {
        return PaintTarget {
            surface: SURFACE_LAND,
            biome: wv_terrain::biome::DISTRICT_FIRST + idx,
        };
    }
    match swatch {
        0 | 1 => WATER,             // 深水 / 浅水
        2 => LAND(BIOME_DESERT),    // 海滩
        3 => LAND(BIOME_GRASSLAND), // 草原
        4 => LAND(BIOME_FOREST),    // 森林
        5 => LAND(BIOME_HILLS),     // 丘陵
        6 => LAND(BIOME_MOUNTAINS), // 山地
        7 => LAND(BIOME_SNOWFIELD), // 雪原
        8 => LAND(BIOME_SWAMP),     // 沼泽
        _ => WATER,                 // 越界色板兜底为水面（防御，绝不在 UI 路径 panic）
    }
}

/// 地形橡皮刷的写入目标：本图底图（`MapBase`）对应的整格（ADR-0043）。
///
/// **为什么必须走 `exact` 而不是 `brush_target`**：[`brush_target`] 的水板写
/// `{SURFACE_WATER, BIOME_NONE}`，高度随后被 `wv_terrain::apply_brush` 的跨域规则
/// 压到 `PAINT_SEA_DEPTH`（-40 m）——那是「涂抹」要的软化高度。但底图海洋是
/// **-3500 m 整片深水**（`MapBase::Ocean.cell()`），底图陆地是 **+80 m 整片草原**
/// （`MapBase::Land.cell()`）。`wv_core` 之所以选这两个整图同值，正是为了让
/// VectorFlat 的双线性高度采样保持常数、不在格边界拉出陡坎——所以橡皮必须
/// 原样回填那个整值，走 `Brush::exact_fill` 的整格直写通道。
///
/// `base` 缺省（老工程没写 `base` 字段）时兜底 **Ocean**，与
/// `wv_edit::ResetTerrainToBaseCommand::apply` 同口径——「擦回底图」与
/// 「重置到底图」必须落到同一张图上，否则作者会看到两个语义相同的按钮
/// 擦出不同的结果。
pub fn eraser_base_cell(world: &wv_core::World, map: &wv_core::Id) -> wv_terrain::TerrainCell {
    use wv_terrain::TerrainCell;
    let base = world
        .maps
        .iter()
        .find(|m| &m.id == map)
        .and_then(|m| m.base)
        .unwrap_or(wv_core::map::MapBase::Ocean);
    let c = base.cell();
    TerrainCell {
        height: c.height,
        surface: c.surface,
        biome: c.biome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 材质条不得再画「僵尸砖」。
    ///
    /// 现状 bug（ADR-0070 D1 退役道路档时漏改循环上界）：材质条画 10 块，
    /// 而名字表只有 9 项 → 第 10 块 tooltip 是 `"?"`；且 `brush_target` 对
    /// 越界档落 `_ => WATER` → 点它会**涂出海**。
    #[test]
    fn materials_table_has_no_zombie_swatch() {
        assert_eq!(MATERIALS.len(), 9, "退役道路档后材料表应是 9 项");
        assert!(
            !MATERIALS.contains(&9),
            "kind 9（道路）已退役，不得出现在材料表"
        );
        for &k in MATERIALS {
            assert_ne!(
                terrain_name_at(Locale::Zh, k as usize),
                "?",
                "材料 {k} 无显示名"
            );
        }
    }

    /// 本轮只做人类城市词表：未分区 + 贫民 / 富人 / 住宅 / 商业 / 工业。
    ///
    /// 12 个分区码位与名字已在 `wv_terrain::biome` 登记（扩展只需加本表）。
    #[test]
    fn district_materials_cover_city_vocabulary() {
        assert_eq!(DISTRICT_MATERIALS.len(), 6);
        assert!(
            DISTRICT_MATERIALS
                .iter()
                .all(|k| wv_terrain::biome::is_district(*k)),
            "分区材料必须都在保留带内"
        );
        assert_eq!(
            terrain_name_at(Locale::Zh, 30),
            "房间",
            "分区名走 DISTRICT_NAMES_ZH 直查（12 个全可查）"
        );
        // 分区笔刷写陆面 + 分区码——高度由 apply_brush 的分区规则保持不变。
        let t = brush_target(wv_terrain::biome::DISTRICT_FIRST + 1);
        assert_eq!(t.surface, wv_terrain::SURFACE_LAND);
        assert_eq!(t.biome, wv_terrain::biome::DISTRICT_FIRST + 1);
    }

    /// 橡皮擦的三态：`base` 海洋 / 陆地 / 缺省（老工程），各擦出正确整格。
    ///
    /// 缺省必须兜底 Ocean 而不是报错——否则老工程打开时橡皮一按就静默失灵。
    #[test]
    fn eraser_base_cell_follows_the_map_base_and_defaults_to_ocean() {
        use wv_core::map::MapBase;

        let mk = |id: &str, base: Option<MapBase>| wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked(id),
            name: id.to_string(),
            base,
            ..Default::default()
        };

        let ocean = wv_core::World {
            maps: vec![mk("m_ocean", Some(MapBase::Ocean))],
            ..Default::default()
        };
        let got = eraser_base_cell(&ocean, &wv_core::Id::new_unchecked("m_ocean"));
        assert_eq!(got.height, -3500, "海洋底图应擦成 -3500 m 深水");
        assert_eq!(got.surface, wv_terrain::SURFACE_WATER);

        let land = wv_core::World {
            maps: vec![mk("m_land", Some(MapBase::Land))],
            ..Default::default()
        };
        let got = eraser_base_cell(&land, &wv_core::Id::new_unchecked("m_land"));
        assert_eq!(got.height, 80, "陆地底图应擦成 +80 m");
        assert_eq!(got.surface, wv_terrain::SURFACE_LAND);
        assert_eq!(got.biome, wv_terrain::BIOME_GRASSLAND);

        // 老工程没有 `base` 字段 → 与 ResetTerrainToBaseCommand 同口径兜底 Ocean。
        let legacy = wv_core::World {
            maps: vec![mk("m_legacy", None)],
            ..Default::default()
        };
        let got = eraser_base_cell(&legacy, &wv_core::Id::new_unchecked("m_legacy"));
        assert_eq!(
            got.height, -3500,
            "缺省 base 应兜底海洋，实得 {}",
            got.height
        );

        // 图不存在时同样兜底海洋，绝不 panic（橡皮路径不该因为取不到图就崩）。
        let got = eraser_base_cell(&ocean, &wv_core::Id::new_unchecked("m_missing"));
        assert_eq!(got.height, -3500, "取不到图时应兜底海洋");
    }

    /// 9 个色板各能取到目标：水板给水面、其余给陆面（计划 Task 6 Review 焦点；
    /// ADR-0070 D1 退役后 10 → 9，「道路」档退场）。
    #[test]
    fn brush_target_table_covers_all_nine_swatches() {
        for swatch in 0..9u8 {
            let target = brush_target(swatch);
            if swatch <= 1 {
                assert_eq!(
                    target.surface,
                    wv_terrain::SURFACE_WATER,
                    "色板 {swatch} 应为水面"
                );
                assert_eq!(
                    target.biome,
                    wv_terrain::BIOME_OCEAN,
                    "水面色板 {swatch} 应带咸水（2026-10-27 B 案）"
                );
            } else {
                assert_eq!(
                    target.surface,
                    wv_terrain::SURFACE_LAND,
                    "色板 {swatch} 应为陆面"
                );
                assert_ne!(
                    target.biome,
                    wv_terrain::BIOME_NONE,
                    "陆面色板 {swatch} 应带群系真值"
                );
            }
        }
        // 越界兜底不 panic。
        let _ = brush_target(200);
    }

    /// 河路退役（ADR-0070 D1）：笔刷色板不再含「道路」档——旧道路位
    /// （swatch 9）落 `_ => WATER` 兜底，且现存 9 档没有任何一档能涂出
    /// `BIOME_ROAD`（读取防御在 wv-terrain，作者入口退役）。
    #[test]
    fn brush_target_has_no_road_entry() {
        let fallback = brush_target(9);
        assert_eq!(
            fallback.surface,
            wv_terrain::SURFACE_WATER,
            "swatch 9 应落 WATER 兜底，实得 surface={}",
            fallback.surface
        );
        assert_eq!(
            fallback.biome,
            wv_terrain::BIOME_OCEAN,
            "swatch 9 兜底是 WATER 档，按 2026-10-27 B 案应带咸水，实得 biome={}",
            fallback.biome
        );
        for swatch in 0..9u8 {
            assert_ne!(
                brush_target(swatch).biome,
                wv_terrain::BIOME_ROAD,
                "色板 {swatch} 不应写出道路群系"
            );
        }
    }

    /// 色板名与目标表一一对应（同序 9 项，防两表漂移）。
    #[test]
    fn terrain_name_lookup_covers_every_swatch() {
        for swatch in 0..9u8 {
            assert_ne!(
                terrain_name_at(crate::i18n::Locale::Zh, swatch as usize),
                "?",
                "色板 {swatch} 应有本地化名"
            );
        }
    }
}
