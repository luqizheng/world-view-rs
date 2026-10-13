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
/// 「深海」Theme Pack（2026-11 用户指定色值）：远海深蓝 + 高饱和陆域色，
/// 对比强烈、轮廓一眼可辨。顺序刻意与「色块地形」视图模式的默认图例一致：
/// 0 为最低（深水）→ 8 为最高（雪），9 为道路。作者改 `kind` 编号即可换色，无需改代码。
pub const CORE_TERRAIN: [Color; 10] = [
    hex(0x1E3A5F), // 0 core.water_deep   深水（远海，30,58,95）
    hex(0x4A90E2), // 1 core.water_shallow 浅水（近海，74,144,226）
    hex(0xE8B86D), // 2 core.sand         沙 / 沙漠（232,184,109）
    hex(0xCDE77F), // 3 core.grass        草原 / 平原（205,231,127）
    hex(0x3E8E41), // 4 core.forest       森林（62,142,65）
    hex(0xD4B483), // 5 core.hill         丘陵 / 荒原（212,180,131）
    hex(0xA67C52), // 6 core.mountain     山地（166,124,82）
    hex(0xE6ECEF), // 7 core.snow         雪线 / 冰原
    hex(0x6B7A55), // 8 core.marsh        沼泽
    hex(0x222222), // 9 core.road         道路 / 区域分界（34,34,34）
];

/// 海岸线描边色（水陆边界浅蓝，2026-11 用户需求）。
pub const COAST_LINE: Color = hex(0x4A90E2); // 浅水色，与近海一致
/// 城市点位色（POI 内芯实心红点，2026-11 用户指定 192,57,43）。
pub const PLACE_DOT: Color = hex(0xC0392B);
/// 城市点外环（白环衬红点，保证在任何陆域色上可辨）。
pub const POI_RING: Color = hex(0xFFFFFF);
/// 道路描线（亮色地图上白色道路，对标 Google Maps）。
pub const ROAD_STROKE: Color = hex(0xFFFFFF);

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

/// 视图模式 E（轮廓图）三色（2026-11 用户需求：黑底 + 海岸线）。
/// 水面近黑、陆地深灰、海岸线中亮灰——黑底上白圈 POI 与强调色全高对比。
pub const COAST_WATER: Color = hex(0x07090C); // 水面：近黑
pub const COAST_LAND: Color = hex(0x1A1E24); // 陆地：深灰（与水可辨但不抢要素）

/// 底图淡化的保留比例：`out = base*KEEP + SOFTEN_TOWARD*(1-KEEP)`。
///
/// 0.35 = 与势力图淡化同源（`shade_faded` 的 k=0.45 略再淡一档），
/// 目标是「仍可辨山川骨架，但饱和度让位给路线与地标」。
pub const SOFTEN_KEEP: f32 = 0.35;
/// 底图淡化目标色：亮暖灰（纸面色），去饱和 + 提亮一并完成。
pub const SOFTEN_TOWARD: Color = hex(0xEDE8DC);

/// 底图淡化混合（所有视图模式共用的唯一实现；禁止散落重写）。
pub fn soften_color(base: Color) -> Color {
    let (Color::Srgba(b), Color::Srgba(t)) = (base, SOFTEN_TOWARD) else {
        return base;
    };
    let k = SOFTEN_KEEP;
    Color::srgb(
        b.red * k + t.red * (1.0 - k),
        b.green * k + t.green * (1.0 - k),
        b.blue * k + t.blue * (1.0 - k),
    )
}

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

/// 世界色令牌 → egui 颜色（UI 层画世界语义色时经此桥接）。
///
/// 为什么需要：UI（egui）要画事件紫 / 游标橙红，但本文件的令牌是
/// `bevy::Color`。直接在 UI 代码里写裸 hex 会破坏「世界色唯一真源」，
/// 所以在这里做一次桥接（§9.2：palette.rs 只增不改）。
pub fn palette_to_color32(c: Color) -> bevy_egui::egui::Color32 {
    let Color::Srgba(s) = c else {
        return bevy_egui::egui::Color32::GRAY;
    };
    bevy_egui::egui::Color32::from_rgb(
        (s.red * 255.0).round() as u8,
        (s.green * 255.0).round() as u8,
        (s.blue * 255.0).round() as u8,
    )
}

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

    #[test]
    fn soften_color_keeps_keep_ratio_toward_paper_gray() {
        // 纯红 (255,0,0) × keep 0.35 + 纸面色 (237,232,220) × 0.65。
        let Color::Srgba(t) = SOFTEN_TOWARD else {
            panic!("应为 Srgba")
        };
        let out = soften_color(Color::srgb(1.0, 0.0, 0.0));
        let Color::Srgba(o) = out else {
            panic!("应为 Srgba")
        };
        let k = SOFTEN_KEEP;
        assert!(
            (o.red - (1.0 * k + t.red * (1.0 - k))).abs() < 1e-6,
            "got {o:?}"
        );
        assert!((o.green - t.green * (1.0 - k)).abs() < 1e-6, "got {o:?}");
        assert!((o.blue - t.blue * (1.0 - k)).abs() < 1e-6, "got {o:?}");
    }

    #[test]
    fn soften_color_of_paper_gray_is_idempotent() {
        // 目标色自身淡化后仍是目标色（混合的不动点）——保证连续两次淡化不漂白。
        // 用近似比较：f32 舍入会引入 ~1e-7 误差，不应用 ==。
        let out = soften_color(SOFTEN_TOWARD);
        let Color::Srgba(a) = out else {
            unreachable!("soften_color 应返回 Srgba")
        };
        let Color::Srgba(b) = SOFTEN_TOWARD else {
            unreachable!("SOFTEN_TOWARD 应为 Srgba")
        };
        assert!(
            (a.red - b.red).abs() < 1e-6,
            "R: got {:.8} want {:.8}",
            a.red,
            b.red
        );
        assert!(
            (a.green - b.green).abs() < 1e-6,
            "G: got {:.8} want {:.8}",
            a.green,
            b.green
        );
        assert!(
            (a.blue - b.blue).abs() < 1e-6,
            "B: got {:.8} want {:.8}",
            a.blue,
            b.blue
        );
    }

    #[test]
    fn coastline_triple_is_dark_background_with_visible_line() {
        // 轮廓图三色的约束：水最暗，陆地略亮，线最亮（黑底可辨）。
        let f = |c: Color| -> [f32; 3] {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            [s.red, s.green, s.blue]
        };
        let lum = |c: Color| -> f32 { f(c).iter().sum::<f32>() / 3.0 };
        assert!(lum(COAST_WATER) < lum(COAST_LAND));
        assert!(lum(COAST_LAND) < lum(COAST_LINE));
    }
}
