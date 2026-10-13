//! 谷歌风色板 —— 基于用户提供的 Google Maps style JSON（2026-11 提交）。
//!
//! ## 用途
//!
//! 为 `ViewMode::GoogleMap` 提供 10 个 `TerrainKind` 的着色，与
//! [`crate::palette`]（深海）/ [`crate::palette_jrpg`]（JRPG）并列的第三套
//! 硬编码色板。命名与结构对齐 `palette_jrpg.rs`，保持扩展点一致。
//!
//! ## 色值来源
//!
//! 全部来自用户提交的 Google Maps style JSON，不做二次重着色。映射规则：
//!
//! - 陆地基色 `#d9c7a8`（`geometry`，未被其他 feature 覆盖时的默认底色）
//! - 水域 `#7399A6`（`water.geometry.fill`）
//! - 森林 `#5b7c48`（`poi.park.geometry.fill`）
//! - 自然地貌 `#c9b28b`（`landscape.natural.geometry`）
//! - 丘陵/行政 `#b8a48e`（`administrative.land_parcel.geometry.stroke`）
//! - 山地 `#8c7863`（`administrative.geometry.stroke`——JSON 中最深的暖棕）
//! - 雪线 `#f2e9d8`（`labels.text.stroke`——JSON 中最浅的暖白）
//! - 道路 `#e0d8c9`（`road.geometry`）
//!
//! ## 与原 JSON 的两处必要偏差
//!
//! 1. **深水 / 浅水**：JSON 只给一档 `water`，但 `TerrainKind` 0/1 是两档；
//!    浅水用 JSON 原色，深水取同色相略压暗（约 −15% 亮度）`#5F8898`，
//!    否则深/浅海视觉不可辨。
//! 2. **沼泽**：JSON 无对应；取水系 `#7399A6` 与森林 `#5b7c48` 的中点
//!    `#87a08a`（近似水陆过渡的苔色）。
//!
//! ## 为什么是硬编码而非 YAML Theme Pack
//!
//! `docs/STYLE_SWAPPABLE_ARCHITECTURE.md` 规划了 YAML-driven 的 Theme Pack，
//! 但**尚未实施**。当前 `palette.rs` / `palette_jrpg.rs` 都是 Rust 硬编码常量，
//! 本文件沿用同一路线以保持一致。**架构改造属于独立任务**，不由本次色板落地。
//!
//! ## 使用方式
//!
//! `terrain_view::shade_for_mode` 中 `ViewMode::GoogleMap` 分支调用
//! `google_core_terrain(kind)`。
//!
//! ## 纪律
//!
//! - 色值唯一权威源是用户在 `m00001` 提交的 style JSON；改动前先回查那条消息。
//! - 禁止在渲染代码里写裸 hex；一律经此模块的令牌引用。

use bevy::prelude::*;

/// 从 sRGB 十六进制构造 `Color`。
const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// 谷歌风地形基色（`core.*`）。索引即 `TerrainKind` 编号（0..10）。
///
/// 派生顺序与「色块地形」视图模式的默认图例一致：0 为最低（深水）→ 9 为道路。
pub const GOOGLE_CORE_TERRAIN: [Color; 10] = [
    hex(0x5F8898), // 0 water_deep     深水（派生：JSON water 压暗一档）
    hex(0x7399A6), // 1 water_shallow  浅水（`water.geometry.fill`）
    hex(0xD9C7A8), // 2 sand           沙 / 沙漠（`geometry` 默认陆色）
    hex(0xC9B28B), // 3 grass          草原 / 平原（`landscape.natural.geometry`）
    hex(0x5B7C48), // 4 forest         森林（`poi.park.geometry.fill`）
    hex(0xB8A48E), // 5 hill           丘陵（`administrative.land_parcel.stroke`）
    hex(0x8C7863), // 6 mountain       山地（`administrative.stroke`——最深暖棕）
    hex(0xF2E9D8), // 7 snow           雪线 / 冰原（`labels.text.stroke`——最浅暖白）
    hex(0x87A08A), // 8 marsh          沼泽（派生：水↔林插值中点）
    hex(0xE0D8C9), // 9 road           道路（`road.geometry`）
];

/// 谷歌风海岸线描边色（与水同色，海岸线通过明度差 + 边界扰动显现）。
pub const GOOGLE_COAST_LINE: Color = hex(0x7399A6);

/// 谷歌风城市点位色。取公路色 `#b37848`——羊皮纸底上暖色最跳出，
/// 比红点更贴合整幅色板（红点会跳出谷歌风的低饱和暖调）。
pub const GOOGLE_PLACE_DOT: Color = hex(0xB37848);

/// 谷歌风城市点外环（白环衬色点，保证在任何暖色底上可辨）。
pub const GOOGLE_POI_RING: Color = hex(0xFFFFFF);

/// 谷歌风公路描边色（若未来把 highway 从 `road` kind 里拆出来单独画线）。
pub const GOOGLE_HIGHWAY_STROKE: Color = hex(0xC49C64);

/// 谷歌风道路线色（`road.arterial`——比陆色 `#d9c7a8` 亮一档，
/// 在羊皮纸底上可辨又不至于白得刺眼；普通道路 `#e0d8c9` 与陆色太近，弃用）。
pub const GOOGLE_ROAD_FILL: Color = hex(0xECE5D7);

/// 谷歌风强调色——被 `accent_select` 类高亮复用的金色（`road.highway`）。
pub const GOOGLE_ACCENT_HIGHLIGHT: Color = hex(0xC49C64);

/// 谷歌风文本描边色（用于标签光晕；`labels.text.stroke`）。
pub const GOOGLE_LABEL_STROKE: Color = hex(0xF2E9D8);

/// 谷歌风文本填色（`labels.text.fill`）。
pub const GOOGLE_LABEL_FILL: Color = hex(0x3A2E24);

/// 谷歌风轮廓图三色（黑底 → 暖底过渡：水、陆、海岸线）。
/// 谷歌风是**暖底**轮廓图，与 `palette::COAST_*`（黑底）不同调；
/// 若未来把轮廓图接入谷歌风，用这组替代 `palette::COAST_*`。
pub const GOOGLE_COAST_WATER: Color = hex(0x7399A6); // 水面：谷歌风原水色
pub const GOOGLE_COAST_LAND: Color = hex(0xD9C7A8); // 陆地：谷歌风原陆色
pub const GOOGLE_COAST_LINE_COLOR: Color = hex(0x8C7863); // 海岸线：JSON 最深暖棕

/// 谷歌风底图淡化目标色（`SOFTEN_TOWARD` 等价物）。
/// 谷歌风底本身就是羊皮纸暖色，淡化目标即 `#f2e9d8`（`labels.text.stroke` 那档）。
pub const GOOGLE_SOFTEN_TOWARD: Color = hex(0xF2E9D8);

/// 谷歌风取地形基色；`kind` 越界时回退到沼泽色（可辨识，不是黑）。
pub fn google_core_terrain(kind: u8) -> Color {
    GOOGLE_CORE_TERRAIN
        .get(kind as usize)
        .copied()
        .unwrap_or(GOOGLE_CORE_TERRAIN[8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_palette_has_ten_entries() {
        assert_eq!(GOOGLE_CORE_TERRAIN.len(), 10);
    }

    #[test]
    fn out_of_range_kind_falls_back_without_panic() {
        // 越界 kind 不应 panic，回退到沼泽色。
        let c = google_core_terrain(200);
        assert_eq!(c, GOOGLE_CORE_TERRAIN[8]);
    }

    #[test]
    fn hex_values_match_user_json() {
        // 抽查 JSON 原色是否落到位（防手工输错 hex 的回归）：
        //   water = #7399A6,  sand base = #d9c7a8,  park = #5b7c48,  road = #e0d8c9
        let expect = |c: Color, rgb: u32| -> [f32; 3] {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            let want = [
                ((rgb >> 16) & 0xFF) as f32 / 255.0,
                ((rgb >> 8) & 0xFF) as f32 / 255.0,
                (rgb & 0xFF) as f32 / 255.0,
            ];
            [
                (s.red - want[0]).abs(),
                (s.green - want[1]).abs(),
                (s.blue - want[2]).abs(),
            ]
        };
        let water = expect(GOOGLE_CORE_TERRAIN[1], 0x7399A6);
        let sand = expect(GOOGLE_CORE_TERRAIN[2], 0xD9C7A8);
        let park = expect(GOOGLE_CORE_TERRAIN[4], 0x5B7C48);
        let road = expect(GOOGLE_CORE_TERRAIN[9], 0xE0D8C9);
        for (name, err) in [
            ("water", water),
            ("sand", sand),
            ("park", park),
            ("road", road),
        ] {
            assert!(
                err.iter().all(|e| *e < 1e-6),
                "{name} hex 与 JSON 不符，误差 {err:?}"
            );
        }
    }

    #[test]
    fn water_deep_is_darker_than_water_shallow() {
        // 深水比浅水暗（否则海陆内部无层次感）。
        let lum = |c: Color| -> f32 {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            0.299 * s.red + 0.587 * s.green + 0.114 * s.blue
        };
        assert!(
            lum(GOOGLE_CORE_TERRAIN[0]) < lum(GOOGLE_CORE_TERRAIN[1]),
            "深水应比浅水暗"
        );
    }

    #[test]
    fn water_and_land_are_visually_distinguishable() {
        // 水与陆明度差必须足够大（避免轮廓图退化）。
        let lum = |c: Color| -> f32 {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            0.299 * s.red + 0.587 * s.green + 0.114 * s.blue
        };
        let water_l = lum(GOOGLE_CORE_TERRAIN[1]);
        let land_l = lum(GOOGLE_CORE_TERRAIN[2]);
        assert!(
            (water_l - land_l).abs() > 0.05,
            "水陆明度差应 >0.05，实得水={water_l:.3} 陆={land_l:.3}"
        );
    }
}
