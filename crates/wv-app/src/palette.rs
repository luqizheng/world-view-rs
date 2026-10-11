//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! 色板令牌 —— `ART_STYLE_SPEC_v0.1.md` §1.4 的 **28 色**骨架值的唯一代码载体。
//!
//! ## 纪律
//!
//! - **色彩唯一权威源是 `docs/ART_STYLE_SPEC_v0.1.md`**（基线 §3.1）。
//!   本文件是它的**投影**，不是第二权威——若两者冲突，改本文件。
//! - **禁止**在渲染代码里写裸十六进制色值（`Color::srgb(0.2, 0.3, 0.4)`）：
//!   一律经此模块的令牌引用。这样换 Theme Pack 时只改一处。
//! - `ui_link.*` 三色是**语义色**（可达绿 / 迟到黄 / 不可达红），
//!   Theme Pack 只允许微调明度，色相不得替换（美术 §1.4.3 ①）。
//!
//! ## 结构（28 色）
//!
//! ```text
//! core     10 色  地形基色（水/沙/草/林/丘/山/雪/沼/路 + 深水/浅水）
//! faction   2 色  已定骨架色（neutral / player）；a..f 由 pack 提供
//! ui_link   3 色  语义色
//! line      3 色  描线色（浅底/中底/深底）
//! accent    4 色  强调色（选中/游标/事件/便签）
//! ```

use bevy::prelude::*;

/// 从 sRGB 十六进制构造 `Color`。
const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// 地形基色（`core.*`）。索引即 `TerrainKind` 编号（0..10）。
///
/// 顺序刻意与「色块地形」视图模式的默认图例一致：0 为最低（深水）→ 8 为最高（雪），
/// 9 为道路。作者改 `kind` 编号即可换色，无需改代码。
pub const CORE_TERRAIN: [Color; 10] = [
    hex(0x1E5A8A), // 0 core.water_deep   深水
    hex(0x3E8FBF), // 1 core.water_shallow 浅水
    hex(0xD9C58F), // 2 core.sand         沙滩 / 荒原
    hex(0x7FA85C), // 3 core.grass        草原 / 平原
    hex(0x3E6B3A), // 4 core.forest       林地
    hex(0xA89060), // 5 core.hill         丘陵
    hex(0x7A6A5E), // 6 core.mountain     山地岩体
    hex(0xE6ECEF), // 7 core.snow         雪线 / 冰原
    hex(0x6B7A55), // 8 core.marsh        沼泽
    hex(0xC9A87C), // 9 core.road         道路
];

/// 派系色（`faction.*`，已定的 2 个骨架值）。
pub const FACTION_NEUTRAL: Color = hex(0x8C8C94); // 无主 / 未归属
pub const FACTION_PLAYER: Color = hex(0xC4553F); // 玩家方（暖红）

/// 语义色（`ui_link.*`）—— 色相不得替换。
pub const UI_REACHABLE: Color = hex(0x3F9B4E); // 可达绿
pub const UI_LATE: Color = hex(0xC9971F); // 迟到黄
pub const UI_UNREACHABLE: Color = hex(0xB5453A); // 不可达红

/// 描线色（`line.*`）。
pub const LINE_ON_LIGHT: Color = hex(0x2B2B33); // 浅底描线
pub const LINE_ON_MID: Color = hex(0x1A1A22); // 中底描线
pub const LINE_ON_DARK: Color = hex(0xEFE9DC); // 深底描线（反白）

/// 强调色（`accent.*`）。
pub const ACCENT_SELECT: Color = hex(0xF2C14E); // 选中高亮（金）
/// **当前时刻指示色**：时间轴游标 + 生长笔尖**共用**（动画规格 §7.1：禁止拆成两令牌）。
pub const ACCENT_CURSOR: Color = hex(0xE8623C);
pub const ACCENT_EVENT: Color = hex(0xC08BD8); // 事件标记（紫）
pub const ACCENT_NOTE: Color = hex(0x9AA5B1); // 「故事」便签（灰蓝）

/// 视图模式 D（事件图）的极简底图色。取石灰色，中性、不抢事件高亮。
pub const EVENT_VIEW_BACKDROP: Color = hex(0xE8E6E1);

/// 取地形基色；`kind` 越界时回退到沼泽色（可辨识，不是黑）。
pub fn core_terrain(kind: u8) -> Color {
    CORE_TERRAIN
        .get(kind as usize)
        .copied()
        .unwrap_or(CORE_TERRAIN[8])
}

/// 规格锚点：基准格边长（`TILE_BASE = 32px`，美术 §3.1）。
pub const TILE_BASE: f32 = 32.0;
/// 允许的格边长档位（美术 §3.1：必须五档之一）。
pub const TILE_SIZE_STEPS: [f32; 5] = [16.0, 24.0, 32.0, 48.0, 64.0];

/// 描线线宽 = `TILE_BASE / 16`（美术 §3.2：32px → 2px）。
pub const OUTLINE_WIDTH: f32 = TILE_BASE / 16.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_palette_has_ten_entries() {
        assert_eq!(CORE_TERRAIN.len(), 10);
    }

    #[test]
    fn out_of_range_kind_falls_back_without_panic() {
        // 越界 kind 不应 panic，回退到沼泽色。
        let c = core_terrain(200);
        assert_eq!(c, CORE_TERRAIN[8]);
    }

    #[test]
    fn tile_base_matches_style_anchor() {
        assert_eq!(TILE_BASE, 32.0);
        assert_eq!(OUTLINE_WIDTH, 2.0);
        assert!(TILE_SIZE_STEPS.contains(&TILE_BASE));
    }

    #[test]
    fn hex_decodes_to_expected_channels() {
        // 0xE8623C → r=232/255, g=98/255, b=60/255
        let Color::Srgba(c) = ACCENT_CURSOR else {
            panic!("应为 Srgba");
        };
        assert!((c.red - 232.0 / 255.0).abs() < 1e-6);
        assert!((c.green - 98.0 / 255.0).abs() < 1e-6);
        assert!((c.blue - 60.0 / 255.0).abs() < 1e-6);
    }
}
