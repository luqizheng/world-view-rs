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

/// 10 种地形的 i18n Key 表（与 `crate::i18n::table.rs` 的 zh/en 镜像同序）。
const TERRAIN_KEYS: [I18nKey; 10] = [
    I18nKey::TerrainKindDeepWater,
    I18nKey::TerrainKindShallowWater,
    I18nKey::TerrainKindBeach,
    I18nKey::TerrainKindGrassland,
    I18nKey::TerrainKindForest,
    I18nKey::TerrainKindHill,
    I18nKey::TerrainKindMountain,
    I18nKey::TerrainKindSnow,
    I18nKey::TerrainKindSwamp,
    I18nKey::TerrainKindRoad,
];

/// 按索引返回本地化后的地形名。索引越界时返回 `"?"`（防御性兜底，绝不在 UI 字符串里 panic）。
pub fn terrain_name_at(loc: Locale, idx: usize) -> &'static str {
    match TERRAIN_KEYS.get(idx) {
        Some(k) => t(loc, *k),
        None => "?",
    }
}

/// 地形色块索引 → `palette::CORE_TERRAIN` 同序（§4.5：色块就是地形本色）。
pub fn terrain_swatch(kind: u8) -> egui::Color32 {
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
/// 水板写水面（无群系，层次由显示层按海拔渐变）；丘陵/山地/雪原按群系
/// 落库（与「中央山」标记同语义，D9）——它们也可不写群系靠高度派生显示，
/// 两路并存即 D16。
pub fn brush_target(swatch: u8) -> wv_terrain::edit::PaintTarget {
    use wv_terrain::edit::PaintTarget;
    use wv_terrain::{
        BIOME_DESERT, BIOME_FOREST, BIOME_GRASSLAND, BIOME_HILLS, BIOME_MOUNTAINS, BIOME_NONE,
        BIOME_ROAD, BIOME_SNOWFIELD, BIOME_SWAMP, SURFACE_LAND, SURFACE_WATER,
    };
    const WATER: PaintTarget = PaintTarget {
        surface: SURFACE_WATER,
        biome: BIOME_NONE,
    };
    const LAND: fn(u8) -> PaintTarget = |biome| PaintTarget {
        surface: SURFACE_LAND,
        biome,
    };
    match swatch {
        0 | 1 => WATER,             // 深水 / 浅水
        2 => LAND(BIOME_DESERT),    // 海滩
        3 => LAND(BIOME_GRASSLAND), // 草原
        4 => LAND(BIOME_FOREST),    // 森林
        5 => LAND(BIOME_HILLS),     // 丘陵
        6 => LAND(BIOME_MOUNTAINS), // 山地
        7 => LAND(BIOME_SNOWFIELD), // 雪原
        8 => LAND(BIOME_SWAMP),     // 沼泽
        9 => LAND(BIOME_ROAD),      // 道路
        _ => WATER,                 // 越界色板兜底为水面（防御，绝不在 UI 路径 panic）
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10 个色板各能取到目标：水板给水面、其余给陆面（计划 Task 6 Review 焦点）。
    #[test]
    fn brush_target_table_covers_all_ten_swatches() {
        for swatch in 0..10u8 {
            let target = brush_target(swatch);
            if swatch <= 1 {
                assert_eq!(
                    target.surface,
                    wv_terrain::SURFACE_WATER,
                    "色板 {swatch} 应为水面"
                );
                assert_eq!(target.biome, wv_terrain::BIOME_NONE);
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

    /// 色板名与目标表一一对应（同序 10 项，防两表漂移）。
    #[test]
    fn terrain_name_lookup_covers_every_swatch() {
        for swatch in 0..10u8 {
            assert_ne!(
                terrain_name_at(crate::i18n::Locale::Zh, swatch as usize),
                "?",
                "色板 {swatch} 应有本地化名"
            );
        }
    }
}
