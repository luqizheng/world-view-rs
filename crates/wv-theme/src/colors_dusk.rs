//! 待批准：本文件在 `app` feature 下才编译。
//! 暮色旧纸色板 —— 「日间古舆图」（[`colors`]）的暗调同槽位镜像（ADR-0016）。
//!
//! ## 职责
//! - 承载 `MapPalette::Dusk` 的全部主题相关色值：深褐灰纸底 + 暗棕路 +
//!   深蓝灰水，低光复古不刺眼，适合黄昏 / 雨夜 / 夜景叙事场景。
//! - 由 [`super::palette`] 装配进 [`super::palette::PaletteColors::DUSK`]，
//!   渲染代码**不直引本模块**。
//!
//! ## 边界（不做什么）
//! - **不含主题不变项**（accent / ui_link / 派系 / 轮廓图三色）——
//!   那些只在 [`colors`] 里有一份。
//! - **不修改 `wv-core` 地形数据**——色板是渲染层语义的视觉解释。
//!
//! ## 色值来源标注
//! 「用户定稿」= 2026-10-14 用户提供；「推导·待复核」= 由日间色系同法压暗
//! 推导，实机校准后可能微调（定稿记录见 ART_STYLE_SPEC §1.4.4）。

use bevy::prelude::*;

/// 从 sRGB 十六进制构造 `Color`。
const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// 暮色地形基色。索引即 `TerrainKind` 编号（0..10），与 [`colors::CORE_TERRAIN`] 同序。
pub const DUSK_CORE_TERRAIN: [Color; 10] = [
    hex(0x3C4854), // 0 water_deep   深水（#44505C 同族压深）推导·待复核
    hex(0x44505C), // 1 water_shallow 浅水 / 海岸带（深蓝灰）用户定稿
    hex(0x5C534A), // 2 sand         沙 / 沙漠 推导·待复核
    hex(0x4A5640), // 3 grass        草原 / 平原（暗橄榄绿）用户定稿
    hex(0x414D38), // 4 forest       森林 推导·待复核
    hex(0x554B41), // 5 hill         丘陵 / 荒原 推导·待复核
    hex(0x6B5D4E), // 6 mountain     山地 推导·待复核
    hex(0x7A6F63), // 7 snow         雪线 / 冰原（暗纸灰，非亮白）推导·待复核
    hex(0x47513E), // 8 marsh        沼泽 推导·待复核
    hex(0x6E5A47), // 9 road         道路 / 区域分界 推导·待复核
];

/// 底图淡化目标色：= 暮色纸面深底（2026-10-14 用户定稿）。
pub const DUSK_SOFTEN_TOWARD: Color = hex(0x3A342E);

/// POI 图标底色：暗赭石（2026-10-14 用户定稿）。
pub const DUSK_PLACE_DOT: Color = hex(0x916F54);
/// POI 外环：= 暮色纸面深底。
pub const DUSK_POI_RING: Color = hex(0x3A342E);
/// 城市间连线描线：暗棕古道（2026-10-14 用户定稿）。
pub const DUSK_ROAD_STROKE: Color = hex(0x705844);

/// 主干道：暗棕古道（2026-10-14 用户定稿）。
pub const DUSK_STREET_MAJOR: Color = hex(0x705844);
/// 次级街巷：深卡其（2026-10-14 用户定稿）。
pub const DUSK_STREET_MINOR: Color = hex(0x594F44);
/// 道路 casing = 暮色纸面深底。
pub const DUSK_ROAD_CASING: Color = hex(0x3A342E);
/// 地名 / 街名文字：浅米褐古墨字（2026-10-14 用户定稿）。
pub const DUSK_LABEL_FILL: Color = hex(0xC9BBA8);
/// 标注三级字重梯度（手绘地图 #2）—— 暮色版：
/// - region（国/大区）= 浅米偏白（最亮档，对应日间最深的褐墨）
/// - place（城/POI）= `DUSK_LABEL_FILL`（保持原观感，避免视觉回归）
/// - water（海名）= 暗青（暗纸上的青蓝，亮于海面同族）
pub const DUSK_LABEL_REGION: Color = hex(0xD6C9B6);
pub const DUSK_LABEL_WATER: Color = hex(0x5A7A88);
/// 雪格双调（手绘地图 #3）—— 暮色版：
/// - 雪 highlight = 暗纸上的近白（最亮档，比日间偏冷一点）
/// - 雪 shadow = 深青灰（暗纸冷阴影，比日间深一档）
pub const DUSK_SNOW_HIGHLIGHT: Color = hex(0xE5DDC8);
pub const DUSK_SNOW_SHADOW: Color = hex(0x6F7C88);
/// 海冰色族（手绘地图 #5）—— 暮色版：海冰纸面比日间偏深、阴影更冷。
pub const DUSK_ICE_PAPER: Color = hex(0xC8CCC6);
pub const DUSK_ICE_INK: Color = hex(0x2E3A42);
pub const DUSK_ICE_SHADOW: Color = hex(0x4A5A66);
/// POI 圆点色族（手绘地图 #9）—— 暮色版：暗纸上稍亮的同族色。
pub const DUSK_DOT_PLACE: Color = hex(0xD9B97A);
pub const DUSK_DOT_REGION: Color = hex(0x7A8EAE);
pub const DUSK_DOT_FACTION: Color = hex(0xE07A60);
pub const DUSK_DOT_EVENT: Color = hex(0xD8B4E8);
/// POI hover 文字色族（手绘地图 #12）—— 暮色版：暗纸上比 DOT 略亮的同族色。
pub const DUSK_TEXT_PLACE: Color = hex(0xC9A878);
pub const DUSK_TEXT_REGION: Color = hex(0x9CADC6);
pub const DUSK_TEXT_FACTION: Color = hex(0xE89580);
pub const DUSK_TEXT_EVENT: Color = hex(0xE0BAEC);
/// 文字晕边 = 暮色纸面深底。
pub const DUSK_HALO_FILL: Color = hex(0x3A342E);
/// 等值线 / 海岸线墨色：暗纸上的浅墨 推导·待复核。
pub const DUSK_ISOLINE_INK: Color = hex(0xA8977F);
/// 编辑网格线基色 推导·待复核。
pub const DUSK_GRID_LINE: Color = hex(0xC9BBA8);
/// 建筑顶面：暗陶土（2026-10-14 用户定稿）。
pub const DUSK_BUILDING_TOP: Color = hex(0x544B42);
/// 建筑侧面 / 投影基色（2026-10-14 用户定稿）。
pub const DUSK_BUILDING_SIDE: Color = hex(0x2E2924);

/// 事件图极简底图：= 暮色纸面深底。
pub const DUSK_EVENT_VIEW_BACKDROP: Color = hex(0x3A342E);

/// 「明确深蓝」色——暮色版（2026-10-16 与日间版同日同步）。
///
/// 比日间版再压一档亮度：暮色纸面深底上，深蓝若用日间值会显得过亮，
/// 抢了势力图 / 标注的颜色层级。这里 ≈ (6, 24, 130) = #061882。
/// 过 `soften + veil` 后仍能辨为「海」。
pub const DUSK_DEEP_OCEAN_BLUE: Color = hex(0x061882);

/// 近岸浅滩色——暮色版（2026-10-21 S2 与日间版同日同步）。
///
/// 暮色浅水 `DUSK_CORE_TERRAIN[1] = #44505C` 的亮一档镜像：暗纸上的
/// 「薄水覆沙」读作灰青高光，不能像日间那样亮到发白（会像月光反光）。
pub const DUSK_WATER_SHORE: Color = hex(0x6C8090);

/// 森林底色（林下深绿）——暮色版（2026-10-22 §九-1 与日间版同日同步）。
///
/// 暮色林色 `DUSK_CORE_TERRAIN[4] = #414D38` 的压深一档：暗纸上林下要
/// 更沉，否则「林间空地」的明暗分层会被纸色吃掉。
pub const DUSK_FOREST_FLOOR: Color = hex(0x2A3324); // ≈ (42, 51, 36) 暗纸林下

/// 描线三档：暗纸上全档浅墨 推导·待复核。
pub const DUSK_LINE_ON_LIGHT: Color = hex(0xC9BBA8);
pub const DUSK_LINE_ON_MID: Color = hex(0xB5A68F);
pub const DUSK_LINE_ON_DARK: Color = hex(0xD6C9B6);

#[cfg(test)]
mod tests {
    use super::*;

    /// 暮色地形基色与日间同槽位数一致（镜像完整性的一半，另一半在 palette）。
    #[test]
    fn dusk_core_terrain_has_ten_entries() {
        assert_eq!(DUSK_CORE_TERRAIN.len(), 10);
    }

    /// 暮色整体比日间暗：抽样地形档 + 文字色的亮度单调性（暗调变体的存在意义）。
    #[test]
    fn dusk_is_darker_than_daylight_on_sampled_slots() {
        let lum = |c: Color| -> f32 {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            (s.red + s.green + s.blue) / 3.0
        };
        assert!(
            lum(DUSK_CORE_TERRAIN[3]) < lum(crate::colors::CORE_TERRAIN[3]),
            "暮色草色应比日间暗"
        );
        assert!(
            lum(DUSK_LABEL_FILL) > lum(crate::colors::LABEL_FILL),
            "暗纸上的文字应比日间浅"
        );
    }
}
