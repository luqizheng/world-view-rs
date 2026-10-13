//! JRPG 风格色板 —— 基于用户提供的 Google Maps JSON 色彩方案。
//!
//! ## 设计思路
//!
//! 将 Google Maps 的自然色调转换为 JRPG 风格，保留原色彩层次但增强对比度和艺术感：
//! - 水体：保持蓝色系但增加饱和度，深水更暗，浅水更亮
//! - 陆地：沙漠/草原/森林/山地采用更饱和的暖色调
//! - 道路：使用更明显的道路色
//! - 强调色：使用 JRPG 经典的金色、紫色等高对比色彩
//!
//! ## 使用方式
//!
//! 在 `terrain_view.rs` 中添加新的视图模式，使用此色板替换默认的 `CORE_TERRAIN`。

use bevy::prelude::*;

/// 从 sRGB 十六进制构造 `Color`。
const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// JRPG 风格地形基色（`core.*`）。索引即 `TerrainKind` 编号（0..10）。
pub const JRPG_CORE_TERRAIN: [Color; 10] = [
    hex(0x1E3A8F), // 0 core.water_deep   深水（远海，30,58,143）
    hex(0x4A90E2), // 1 core.water_shallow 浅水（近海，74,144,226）
    hex(0xE8B86D), // 2 core.sand         沙 / 沙漠（232,184,109）
    hex(0x7CB342), // 3 core.grass        草原 / 平原（124,179,66）
    hex(0x2E7D32), // 4 core.forest       森林（46,125,50）
    hex(0xD4B483), // 5 core.hill         丘陵 / 荒原（212,180,131）
    hex(0xA67C52), // 6 core.mountain     山地（166,124,82）
    hex(0xE6ECEF), // 7 core.snow         雪线 / 冰原
    hex(0x6B7A55), // 8 core.marsh        沼泽
    hex(0x222222), // 9 core.road         道路 / 区域分界（34,34,34）
];

/// JRPG 风格浅水色（与近海一致）。
///
/// 命名修正：此常量曾被误命名为 `JRPG_COAST_LINE`，与第 75 行真正的海岸线
/// 常量同名，触发 E0428 编译错误。该常量语义上是「浅水」，与 `JRPG_CORE_TERRAIN[1]`
/// 同色，改名以反映真实用途。参见 `palette_google.rs` 的深水/浅水分档同理。
pub const JRPG_SHALLOW_WATER: Color = hex(0x4A90E2);

/// JRPG 风格城市点位色。
pub const JRPG_PLACE_DOT: Color = hex(0xC0392B); // 纎红色

/// JRPG 风格道路描线。
pub const JRPG_ROAD_STROKE: Color = hex(0xFFFFFF); // 白色道路

/// JRPG 风格派系色。
pub const JRPG_FACTION_NEUTRAL: Color = hex(0x8C8C94); // 无主 / 未归属
pub const JRPG_FACTION_PLAYER: Color = hex(0xC4553F); // 玩家方（暖红）

/// JRPG 风格语义色。
pub const JRPG_UI_REACHABLE: Color = hex(0x3F9B4E); // 可达绿
pub const JRPG_UI_LATE: Color = hex(0xC9971F); // 迟到黄
pub const JRPG_UI_UNREACHABLE: Color = hex(0xB5453A); // 不可达红

/// JRPG 风格描线色。
pub const JRPG_LINE_ON_LIGHT: Color = hex(0x2B2B33); // 浅底描线
pub const JRPG_LINE_ON_MID: Color = hex(0x1A1A22); // 中底描线
pub const JRPG_LINE_ON_DARK: Color = hex(0xEFE9DC); // 深底描线（反白）

/// JRPG 风格强调色。
pub const JRPG_ACCENT_SELECT: Color = hex(0xF2C14E); // 选中高亮（金）
pub const JRPG_ACCENT_CURSOR: Color = hex(0xE8623C); // 时间轴游标（橙红）
pub const JRPG_ACCENT_EVENT: Color = hex(0xC08BD8); // 事件标记（紫）
pub const JRPG_ACCENT_NOTE: Color = hex(0x9AA5B1); // 「故事」便签（灰蓝）

/// JRPG 风格视图模式 D（事件图）的底图色。
pub const JRPG_EVENT_VIEW_BACKDROP: Color = hex(0xE8E6E1);

/// JRPG 风格轮廓图三色。
pub const JRPG_COAST_WATER: Color = hex(0x07090C); // 水面：近黑
pub const JRPG_COAST_LAND: Color = hex(0x1A1E24); // 陆地：深灰
pub const JRPG_COAST_LINE: Color = hex(0x9AA5B1); // 海岸线：中亮灰

/// JRPG 风格底图淡化保留比例。
pub const JRPG_SOFTEN_KEEP: f32 = 0.35;

/// JRPG 风格底图淡化目标色。
pub const JRPG_SOFTEN_TOWARD: Color = hex(0xEDE8DC);

/// JRPG 风格取地形基色；`kind` 越界时回退到沼泽色。
pub fn jrpg_core_terrain(kind: u8) -> Color {
    JRPG_CORE_TERRAIN
        .get(kind as usize)
        .copied()
        .unwrap_or(JRPG_CORE_TERRAIN[8])
}
