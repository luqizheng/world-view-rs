//! 群系色块底图（CellMap · 2026-10-22 三模式内容矩阵 spec §2.5）。
//!
//! ## 这是什么
//!
//! Topographic（CellMap 工作区）的地形画法：每格颜色 = **冻结群系表的固定色**
//! ——「最底层的地形编辑器」看的是原始数据，不是艺术渲染。
//!
//! ## 为什么固定、不随主题
//!
//! [`crate::theme`] 的色板是「看世界」用的（Browse 的主题艺术色、覆盖物规则色）。
//! 数据视图要的是诚实：切任何 ThemePack、开任何 TreatmentFlags（水彩晕开等），
//! 本模块的输出**逐位不变**。先例：region_overlay 的「固定诊断色，不随主题色板」
//! （ADR-0045 追记一）。结构上由**签名保证**——[`pick`] 不接收任何 palette 参数。
//!
//! ## 语义
//!
//! - `biome != 0` → 直查 [`BIOME_RAW_COLORS`]（索引 = `wv_terrain::biome` 冻结编号）。
//! - `biome == 0`（NONE）→ 按 surface 回退：水 → [`FALLBACK_WATER`]，陆 → [`FALLBACK_LAND`]
//!   （旧工程的格子常是 NONE，显示不能开天窗）。
//! - 高度**不参与**颜色（改海拔不变色；海拔参照交给等高线叠加与 cell 高亮）。
//! - **族间色相拉开 ≥40°、族内按明度阶梯**（2026-10-22 验收：旧色板族内只差
//!   ~15% 明度、族间撞色相，森林/丛林/雨林与沙漠/戈壁/稀树草原在图上看不出区别）。
//!
//! [spec]: ../../../../docs/superpowers/specs/2026-10-22-mode-content-matrix-design.md

use wv_terrain::biome;

/// 一格的最终 RGBA（0.0–1.0，与 `theme::topographic::pick` 同口径）。
pub fn pick(biome: u8, surface: u8) -> [f32; 4] {
    if biome == biome::BIOME_NONE {
        let rgb = if surface == wv_terrain::SURFACE_WATER {
            FALLBACK_WATER
        } else {
            FALLBACK_LAND
        };
        return rgb_to_rgba(rgb);
    }
    rgb_to_rgba(BIOME_RAW_COLORS[biome as usize])
}

/// 冻结群系表的固定色（索引 = `wv_terrain::biome` 编号 0–20）。
///
/// **改这里的值 = 改 CellMap 的数据视图语义**（ADR-0051 记档），不要为
/// 「好看」随手调——要调艺术色去 ThemePack。
pub const BIOME_RAW_COLORS: [[u8; 3]; 21] = [
    [0x9E, 0x9E, 0x9E], // 0  无群系（占位；pick 按 surface 回退）
    [0x2E, 0x7D, 0x32], // 1  森林（中绿 · 森林族）
    [0xAE, 0xEA, 0x00], // 2  平原（亮黄绿 · 草原族，族内最亮）
    [0x7C, 0xB3, 0x42], // 3  草原（草绿 · 草原族）
    [0xC1, 0x9A, 0x6B], // 4  戈壁（土黄褐 · 干旱族）
    [0xF2, 0xC1, 0x4E], // 5  沙漠（沙黄 · 干旱族，族内最亮）
    [0x3E, 0x5F, 0x3E], // 6  沼泽（暗青绿 · 湿地族）
    [0x2E, 0x7D, 0x6B], // 7  湿地（中青绿 · 湿地族）
    [0x8D, 0x9E, 0x8A], // 8  苔原（灰绿 · 寒带族）
    [0xF5, 0xF7, 0xFA], // 9  雪原（雪白 · 寒带族）
    [0x43, 0xA0, 0x47], // 10 丛林（亮绿 · 森林族）
    [0x1B, 0x5E, 0x20], // 11 热带雨林（最深绿 · 森林族）
    [0x8D, 0x6E, 0x63], // 12 丘陵（褐 · 高地族）
    [0x6D, 0x4C, 0x41], // 13 山地（深褐 · 高地族）
    [0x0D, 0x47, 0xA1], // 14 海洋（深蓝 · 水域族）
    [0x19, 0x76, 0xD2], // 15 湖泊（中蓝 · 水域族）
    [0x42, 0xA5, 0xF5], // 16 河流（亮蓝 · 水域族）
    [0x66, 0xBB, 0x6A], // 17 绿洲（亮绿 · 草原族）
    [0xC0, 0xCA, 0x33], // 18 稀树草原（黄绿 · 草原族）
    [0xE0, 0xE0, 0xE0], // 19 盐碱地（浅灰 · 干旱族）
    [0x61, 0x61, 0x61], // 20 道路（深灰 · 道路族）
];

/// 冻结群系表的**中文显示名**（索引 = 编号；图例 / 表单用）。
pub const BIOME_NAMES_ZH: [&str; 21] = [
    "无群系",
    "森林",
    "平原",
    "草原",
    "戈壁",
    "沙漠",
    "沼泽",
    "湿地",
    "苔原",
    "雪原",
    "丛林",
    "热带雨林",
    "丘陵",
    "山地",
    "海洋",
    "湖泊",
    "河流",
    "绿洲",
    "稀树草原",
    "盐碱地",
    "道路",
];

/// 图例分组：(族名, 该族群系编号)。255 = 空位。
pub const BIOME_LEGEND_GROUPS: [(&str, [u8; 4]); 8] = [
    ("森林", [1, 10, 11, 255]),
    ("草原", [2, 3, 17, 18]),
    ("干旱", [4, 5, 19, 255]),
    ("湿地", [6, 7, 255, 255]),
    ("寒带", [8, 9, 255, 255]),
    ("高地", [12, 13, 255, 255]),
    ("水域", [14, 15, 16, 255]),
    ("道路 / 无", [20, 0, 255, 255]),
];

/// NONE + 水面的回退色（固定水蓝）。
pub const FALLBACK_WATER: [u8; 3] = [0x24, 0x71, 0x8F];
/// NONE + 陆面的回退色（固定灰米）。
pub const FALLBACK_LAND: [u8; 3] = [0xB9, 0xB2, 0xA2];

fn rgb_to_rgba(rgb: [u8; 3]) -> [f32; 4] {
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_terrain::biome::{
        BIOME_JUNGLE as JUNGLE, BIOME_TROPICAL_RAINFOREST as TROPICAL_RAINFOREST,
    };
    use wv_terrain::{BIOME_NONE, SURFACE_LAND, SURFACE_WATER};

    #[test]
    fn biome_raw_colors_cover_all_21_frozen_biomes() {
        assert_eq!(BIOME_RAW_COLORS.len(), 21, "冻结群系表 21 编号必须全覆盖");
        assert_eq!(BIOME_NAMES_ZH.len(), 21, "名称表必须与色板同长");
    }

    #[test]
    fn biome_raw_pick_falls_back_by_surface_when_biome_none() {
        let water = pick(BIOME_NONE, SURFACE_WATER);
        let land = pick(BIOME_NONE, SURFACE_LAND);
        assert_ne!(water, land, "NONE 的水/陆回退色必须不同");
    }

    #[test]
    fn biome_raw_pick_maps_each_frozen_biome_to_table() {
        for b in 1u8..=20 {
            let got = pick(b, SURFACE_LAND);
            let want = rgb_to_rgba(BIOME_RAW_COLORS[b as usize]);
            assert_eq!(got, want, "biome {b} 必须直查色板");
        }
    }

    #[test]
    fn biome_raw_colors_are_distinct_within_each_family() {
        let forest = [
            BIOME_RAW_COLORS[1],
            BIOME_RAW_COLORS[JUNGLE as usize],
            BIOME_RAW_COLORS[TROPICAL_RAINFOREST as usize],
        ];
        assert!(
            forest[0] != forest[1] && forest[1] != forest[2],
            "森林族内三色互异"
        );
        let water = [
            BIOME_RAW_COLORS[14],
            BIOME_RAW_COLORS[15],
            BIOME_RAW_COLORS[16],
        ];
        assert!(
            water[0] != water[1] && water[1] != water[2],
            "水域族内三色互异"
        );
    }

    /// 族间必须可辨（2026-10-22 验收的核心诉求）：抽查森林 vs 草原 vs 干旱
    /// 三个代表色的 RGB 距离 ≥ 60（欧氏，0–441 量程）。
    #[test]
    fn biome_raw_families_are_visually_separated() {
        let dist = |a: [u8; 3], b: [u8; 3]| -> f32 {
            ((a[0] as f32 - b[0] as f32).powi(2)
                + (a[1] as f32 - b[1] as f32).powi(2)
                + (a[2] as f32 - b[2] as f32).powi(2))
            .sqrt()
        };
        let forest = BIOME_RAW_COLORS[1];
        let grassland = BIOME_RAW_COLORS[3];
        let desert = BIOME_RAW_COLORS[5];
        let water = BIOME_RAW_COLORS[14];
        let highland = BIOME_RAW_COLORS[12];
        for (a, b, name) in [
            (forest, grassland, "森林↔草原"),
            (grassland, desert, "草原↔干旱"),
            (forest, desert, "森林↔干旱"),
            (desert, water, "干旱↔水域"),
            (grassland, highland, "草原↔高地"),
        ] {
            assert!(
                dist(a, b) >= 60.0,
                "{name} 色距应 ≥60（实得 {:.0}）",
                dist(a, b)
            );
        }
    }

    #[test]
    fn biome_legend_groups_cover_every_biome_exactly_once() {
        let mut seen = [0u8; 21];
        for (_, ids) in BIOME_LEGEND_GROUPS {
            for id in ids {
                if id != 255 {
                    seen[id as usize] += 1;
                }
            }
        }
        for (b, n) in seen.iter().enumerate() {
            assert_eq!(*n, 1, "群系 {b} 在图例里应恰好出现一次");
        }
    }
}
