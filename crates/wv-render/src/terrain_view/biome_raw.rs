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
//!   同族同色相、按编号做明度阶梯——家族关系一眼可辨。
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
/// 同族同色相、族内按编号排明度。**改这里的值 = 改 CellMap 的数据视图语义**
/// （ADR-0051 记档），不要为「好看」随手调——要调艺术色去 ThemePack。
pub const BIOME_RAW_COLORS: [[u8; 3]; 21] = [
    [0x8A, 0x8A, 0x82], // 0  NONE（占位；pick 按 surface 回退）
    [0x2E, 0x7D, 0x32], // 1  森林（深绿）
    [0xA8, 0xC2, 0x56], // 2  平原（黄绿）
    [0x7C, 0xB3, 0x42], // 3  草原（草绿）
    [0xB8, 0xA6, 0x6A], // 4  戈壁（灰黄褐）
    [0xE0, 0xC0, 0x68], // 5  沙漠（沙黄）
    [0x4E, 0x5B, 0x31], // 6  沼泽（暗褐绿）
    [0x5E, 0x8A, 0x6E], // 7  湿地（青灰绿）
    [0x9E, 0x9B, 0x8A], // 8  苔原（灰褐）
    [0xE8, 0xEC, 0xEF], // 9  雪原（雪白）
    [0x43, 0xA0, 0x47], // 10 丛林（鲜绿）
    [0x1B, 0x5E, 0x20], // 11 热带雨林（最深绿）
    [0xB5, 0xA6, 0x42], // 12 丘陵（黄褐）
    [0x8D, 0x7F, 0x6E], // 13 山地（灰褐）
    [0x1A, 0x52, 0x76], // 14 海洋（深蓝）
    [0x2E, 0x86, 0xC1], // 15 湖泊（中蓝）
    [0x34, 0x98, 0xDB], // 16 河流（亮蓝）
    [0x66, 0xBB, 0x6A], // 17 绿洲（青绿）
    [0xC9, 0xB4, 0x58], // 18 稀树草原（干草黄）
    [0xE3, 0xE8, 0xE6], // 19 盐碱地（白盐）
    [0x6B, 0x6B, 0x6B], // 20 道路（灰）
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
        assert_eq!(
            BIOME_RAW_COLORS.len(),
            21,
            "冻结群系表 21 编号必须全覆盖（wv-terrain::biome）"
        );
    }

    #[test]
    fn biome_raw_pick_falls_back_by_surface_when_biome_none() {
        let water = pick(BIOME_NONE, SURFACE_WATER);
        let land = pick(BIOME_NONE, SURFACE_LAND);
        assert_ne!(water, land, "NONE 的水/陆回退色必须不同");
        assert_eq!(water[0], FALLBACK_WATER[0] as f32 / 255.0);
        assert_eq!(land[0], FALLBACK_LAND[0] as f32 / 255.0);
    }

    #[test]
    fn biome_raw_pick_maps_each_frozen_biome_to_table() {
        // 冻结表全覆盖：任一合法 biome 直查色板（水面与否不影响陆上群系）。
        for b in 1u8..=20 {
            let got = pick(b, SURFACE_LAND);
            let want = rgb_to_rgba(BIOME_RAW_COLORS[b as usize]);
            assert_eq!(got, want, "biome {b} 必须直查色板");
        }
    }

    #[test]
    fn biome_raw_colors_are_distinct_within_each_family() {
        // 同族内明度阶梯的意义：族内可辨。抽三个族抽查。
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
}
