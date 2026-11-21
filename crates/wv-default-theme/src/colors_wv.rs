//! World View **自有色板 WV** —— 群系真色主导下的冷中性矢量层唯一出处。
//!
//! 规格：`docs/reviews/2026-11-13-wv-palette-spec.md`（用户 2026-11-13 拍板
//! 「重新设计一套正确配色」/ 范围 A：只重建矢量层色板）。
//!
//! ## 为什么是第三套色板
//!
//! - HEAD 的 `colors_google::GOOGLE_CORE_TERRAIN`（`#AADAFF` 淡水 /
//!   `#C9E7C7` 公园绿 / `#F7F5F0` 暖白陆）正是用户要去掉的「Google 样式」；
//! - 工作树里 ① 阶段的古舆图 `colors::*` 褐墨 / 纸色 / 暖沙是用户已弃用的方向 A；
//! - 两头都不取 → 新建项目自有 `WV_*`，既不回退也不沿用。
//!
//! ## 三条可判边界
//!
//! 1. **群系真色主导**：画面上唯一的彩色来自冻结群系表 21 色（主画布 GPU
//!    `terrain_field.wgsl` 的 `biome_table` + `biome_tint = 0.82`）。本模块管的
//!    矢量层（注记 / 道路 / POI / 等值线 / 网格 / 氛围罩 / 区域轮廓）**零彩度**，
//!    只靠明度分级承载层级。
//! 2. **单一出处、同帧同源**：五枚中性墨是 `(c)` 三件套已拍板的 `GOOGLE_*`
//!    常量的**别名重导出**，不是第二份色表。同一帧出现两套灰就是「拼的」复发，
//!    故 `wv_neutral_inks_alias_the_furniture_tokens` 逐位钉死防漂移。
//!    命名带 `GOOGLE_` 只是历史（D7），值本身是中性灰、不含品牌色。
//! 3. **唯一强调色 = 石板蓝 `#2A5D8F`**：值取主画布
//!    `terrain_field.wgsl:148 WATER_SRGB`（ADR-0101），不引入新色相；只用于
//!    「事件族 POI / 海名 / 深水位」这类需跳出的极小面积。
//!
//! ## 依赖边界（实施时实测，规格未预先覆盖）
//!
//! 本 crate **只依赖 `bevy` + `wv-terrain`**（见 `Cargo.toml` / ADR-0054 单向
//! 依赖红线），**不能**直接引用 `wv_world_cell_map::terrain_view::biome_raw::
//! BIOME_RAW_COLORS`。故 [`WV_WATER_SHORE`] / [`WV_SNOW`] 只能按冻结表值**镜像**
//! 抄录。真正的「跨表逐位一致」断言落在 `wv-render::cellmap_bridge`（`app`
//! feature 下它同时依赖两个 crate），见该侧
//! `wv_water_and_snow_mirror_the_frozen_biome_table`。
//! 同理 `WATER_SRGB` 是 WGSL 字符串常量，Rust 侧无编译期引用，靠本文件
//! 单一出处 + 全仓 grep 保证，无自动断言。
//!
//! ## 禁用色（本模块零命中）
//!
//! `#F2EAD8` 纸色、`#4A3F35` / `#6B5B49` 褐墨、`#A67C52` 棕褐、
//! `#EA4335` 红、`#1A73E8` 蓝、`#E8710A` 橙、`#9334E6` 紫、
//! `#F9C25C` / `#FBD366` 橙黄干线、`#AADAFF` / `#C9E7C7` / `#F7F5F0`
//! Google 地形色。

use bevy::prelude::*;

use super::colors::hex;
use super::colors_google as g;

// ── 中性墨阶 5 档 ────────────────────────────────────────────────────────
// 别名重导出，零新字面量。与 (c) 三件套 `map_furniture.rs` 五枚令牌同值。

/// 一级墨：地名 / 街名 / POI 点 / 区域轮廓。`#3C4043`。
pub const WV_INK_PRIMARY: Color = g::GOOGLE_LABEL_FILL;
/// 三级墨：势力名 / 中等线 / 势力 POI。`#5F6368`。
pub const WV_INK_SECONDARY: Color = g::GOOGLE_LINE_ON_DARK;
/// 二级墨：等值线 / 冰缘 / 次级线 / Region 族 hover 圆点。`#9AA0A6`。
/// 第三档中性明度，用于让 POI 四族颜色互异（见 `palette.rs` POI 4 族说明）。
pub const WV_INK_MUTED: Color = g::GOOGLE_ISOLINE_INK;
/// 极淡线：网格 / 路缘 / 建筑侧 / 雪阴影 / 图例描边。`#DADCE0`。
pub const WV_LINE_FAINT: Color = g::GOOGLE_CONTROL_BORDER;
/// 白晕（**双通道硬前提**）/ 道路白填充 / POI 白环 / 雪高光。`#FFFFFF`。
///
/// 数值判据：一级墨压最暗群系底（热带雨林 `#3A6B45`）只有 1.68:1，
/// 纯白对它是 6.23:1；反过来一级墨对雪原 `#F2F5F7` 有 9.56:1，纯白只有
/// 1.09:1。故所有画布文字必须「深墨字身 + 纯白 8 向晕圈」双通道，
/// 单靠一种墨色不可判（工程 `sync_halo` / ADR-0016 M2 已落地）。
pub const WV_PAPER: Color = g::GOOGLE_CONTROL_BG;

// ── 新增语义色（本轮真正的新设计）────────────────────────────────────────
/// 冷近白底：`soften_toward` / `event_backdrop` / `ice_paper` / `building_top`。
/// 取 ② 阶段冷灰绿海拔带第 0 阶，与山势底同族；替代暖纸 `#F2EAD8`
/// 与 Google 暖白 `#F7F5F0`。
pub const WV_SURFACE: Color = hex(0xEE_F1_EF);
/// 淡化次级面：`core[5]` 丘陵底。
pub const WV_SURFACE_DEEP: Color = hex(0xE8_EB_E8);
/// **唯一强调色**：石板蓝，= `WATER_SRGB`（ADR-0101）。豁免 B1 零彩度判据——
/// 它属水体语义，不属矢量墨阶。
pub const WV_ACCENT: Color = hex(0x2A_5D_8F);
/// 近岸咸水 = 冻结群系表 `BIOME_RAW_COLORS[14]`，与图例 / 数据视图同值。
pub const WV_WATER_SHORE: Color = hex(0x9D_C3_D8);
/// 深水位 = [`WV_ACCENT`]（别名），与 GPU 收敛水色同值，消掉两套水色口径。
pub const WV_WATER_DEEP: Color = WV_ACCENT;
/// 雪原 = 冻结群系表 `BIOME_RAW_COLORS[9]`。
pub const WV_SNOW: Color = hex(0xF2_F5_F7);
/// 冰背光冷灰。
pub const WV_ICE_SHADOW: Color = hex(0xCF_D6_D3);
/// 道路白填充 = [`WV_PAPER`]（别名）。
pub const WV_ROAD: Color = WV_PAPER;
/// 底图淡化保留比例：0.96 = 几乎不洗，保留要素可读性（与 (c) 三件套口径一致）。
pub const WV_SOFTEN_KEEP: f32 = 0.96;

/// 风格色 `core[10]`（`TerrainKind` 0..10，CPU 留守档 / CellMap 折叠语义）。
///
/// 主画布 GPU Themed **不读 `core`**（走 `biome_table`），此表只服务 CPU 留守档
/// 与等值线 / 网格取色，故按「低信息量折叠」设计，不与 21 色群系真色竞争。
/// 槽 0 / 1 / 7 取自冻结群系表或 GPU 真值（水体语义，豁免 B1）；其余全为
/// `S ≤ 0.12`、`|R−B| ≤ 12` 的冷中性面。
pub const WV_CORE_TERRAIN: [Color; 10] = [
    WV_ACCENT,       // 0 深水（GPU 收敛水色）
    WV_WATER_SHORE,  // 1 浅水（冻结表 [14]）
    WV_SURFACE,      // 2 沙 / 基础陆
    hex(0xF1_F3_F0), // 3 草原
    hex(0xE6_EB_E6), // 4 林地
    WV_SURFACE_DEEP, // 5 丘陵
    hex(0xDD_E3_DD), // 6 山地
    WV_SNOW,         // 7 雪原（冻结表 [9]）
    hex(0xE2_EA_E3), // 8 沼泽
    WV_ROAD,         // 9 道路
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 从 `Color` 取 8-bit sRGB 三通道（与 `palette.rs` 锚点测试同口径）。
    fn rgb(c: Color) -> [u8; 3] {
        let Color::Srgba(s) = c else {
            panic!("WV 令牌应为 Srgba")
        };
        [
            (s.red * 255.0).round() as u8,
            (s.green * 255.0).round() as u8,
            (s.blue * 255.0).round() as u8,
        ]
    }

    /// T1 · 单一出处：五枚中性墨逐位等于 (c) 三件套已拍板的令牌。
    /// 同帧出现两套灰 = D2「拼的」复发，这里逐位钉死防漂移。
    #[test]
    fn wv_neutral_inks_alias_the_furniture_tokens() {
        assert_eq!(WV_INK_PRIMARY, g::GOOGLE_LABEL_FILL);
        assert_eq!(WV_INK_SECONDARY, g::GOOGLE_LINE_ON_DARK);
        assert_eq!(WV_INK_MUTED, g::GOOGLE_ISOLINE_INK);
        assert_eq!(WV_LINE_FAINT, g::GOOGLE_CONTROL_BORDER);
        assert_eq!(WV_PAPER, g::GOOGLE_CONTROL_BG);
    }

    /// T2 · B1 中性判据：所有**墨阶 / 中性面**满足 `S ≤ 0.12`、`|R−B| ≤ 12`、
    /// 零 `R>G>B` 暖序。豁免项仅为水体语义色（`WV_ACCENT` /
    /// `WV_WATER_SHORE` / `WV_WATER_DEEP` / `WV_SNOW`），它们取自 GPU 真值或
    /// 冻结群系表，语义是水/雪而不是墨。
    #[test]
    fn wv_neutral_inks_satisfy_b1() {
        let neutrals: [(&str, Color); 15] = [
            ("WV_INK_PRIMARY", WV_INK_PRIMARY),
            ("WV_INK_SECONDARY", WV_INK_SECONDARY),
            ("WV_INK_MUTED", WV_INK_MUTED),
            ("WV_LINE_FAINT", WV_LINE_FAINT),
            ("WV_PAPER", WV_PAPER),
            ("WV_SURFACE", WV_SURFACE),
            ("WV_SURFACE_DEEP", WV_SURFACE_DEEP),
            ("WV_ICE_SHADOW", WV_ICE_SHADOW),
            ("core[2]", WV_CORE_TERRAIN[2]),
            ("core[3]", WV_CORE_TERRAIN[3]),
            ("core[4]", WV_CORE_TERRAIN[4]),
            ("core[5]", WV_CORE_TERRAIN[5]),
            ("core[6]", WV_CORE_TERRAIN[6]),
            ("core[8]", WV_CORE_TERRAIN[8]),
            ("core[9]", WV_CORE_TERRAIN[9]),
        ];
        for (name, c) in neutrals {
            let [r, g_, b] = rgb(c);
            let mx = r.max(g_).max(b) as f32;
            let mn = r.min(g_).min(b) as f32;
            let sat = if mx == 0.0 { 0.0 } else { (mx - mn) / mx };
            assert!(sat <= 0.12, "{name} 饱和度 S={sat:.4} > 0.12");
            assert!(
                (i32::from(r) - i32::from(b)).abs() <= 12,
                "{name} |R−B|={} > 12",
                (i32::from(r) - i32::from(b)).abs()
            );
            assert!(!(r > g_ && g_ > b), "{name} 出现 R>G>B 暖序：{r},{g_},{b}");
        }
    }

    /// T3 · 双通道硬前提：白晕必须是**纯白**。深墨压暗群系底只有 1.68:1，
    /// 靠纯白晕圈补到 6.23:1；纸色 / 米白都会让这条判据失效。
    #[test]
    fn wv_paper_is_pure_white_for_halo() {
        assert_eq!(rgb(WV_PAPER), [0xFF, 0xFF, 0xFF]);
        assert_eq!(rgb(WV_ROAD), [0xFF, 0xFF, 0xFF]);
    }

    /// T5（模块侧镜像自证）· 水体 / 雪色按冻结表与 GPU 真值抄录。
    ///
    /// 本 crate 不依赖 `wv-world-cell-map`，无法引用 `BIOME_RAW_COLORS`；
    /// 真正的跨表逐位断言在 `wv-render::cellmap_bridge`。这里只锁住「本文件
    /// 手抄值」，防改色板时无意把水色漂回 `#AADAFF` 那套。
    #[test]
    fn wv_water_and_snow_mirror_frozen_sources() {
        assert_eq!(rgb(WV_WATER_DEEP), [0x2A, 0x5D, 0x8F], "= WATER_SRGB");
        assert_eq!(rgb(WV_ACCENT), [0x2A, 0x5D, 0x8F], "= WATER_SRGB");
        assert_eq!(rgb(WV_WATER_SHORE), [0x9D, 0xC3, 0xD8], "= BIOME_RAW_COLORS[14]");
        assert_eq!(rgb(WV_SNOW), [0xF2, 0xF5, 0xF7], "= BIOME_RAW_COLORS[9]");
        assert_eq!(WV_WATER_DEEP, WV_ACCENT, "深水位与强调色必须同源");
    }

    /// T4 · `core` 十槽齐全、风格色不与逐格技术色撞值，且**两头都不取**：
    /// 既不是古舆图 `ANTIQUE_TERRAIN`（灰蓝海 / 暖沙 / 褐墨路），
    /// 也不是 `GOOGLE_CORE_TERRAIN`（淡水 / 公园绿 / 暖白陆）。
    #[test]
    fn wv_core_terrain_takes_neither_antique_nor_google() {
        assert_eq!(WV_CORE_TERRAIN.len(), 10);
        // 逐格技术色是高饱和十色：草原 `#8CB95F` / 林地 `#3F7A46`。
        assert_ne!(WV_CORE_TERRAIN[3], super::super::colors::CORE_TERRAIN[3]);
        assert_ne!(WV_CORE_TERRAIN[4], super::super::colors::CORE_TERRAIN[4]);
        // 两头都不取：抽查最易手滑的三个槽。
        assert_eq!(rgb(WV_CORE_TERRAIN[0]), [0x2A, 0x5D, 0x8F], "非灰蓝海 #8FA2B3");
        assert_eq!(rgb(WV_CORE_TERRAIN[1]), [0x9D, 0xC3, 0xD8], "非 Google 淡水 #AADAFF");
        assert_eq!(rgb(WV_CORE_TERRAIN[9]), [0xFF, 0xFF, 0xFF], "非城墙墨棕 #735D48");
    }

    /// T7 · 禁用色在本模块零命中（去品牌 / 去古舆图 / 去 Google 地形色）。
    #[test]
    fn wv_forbidden_hexes_are_absent() {
        const FORBIDDEN: [[u8; 3]; 15] = [
            [0xF2, 0xEA, 0xD8], // 纸色
            [0x4A, 0x3F, 0x35], // 褐墨
            [0x6B, 0x5B, 0x49], // 褐墨
            [0xA6, 0x7C, 0x52], // 棕褐
            [0xEA, 0x43, 0x35], // Google 红
            [0x1A, 0x73, 0xE8], // Google 蓝
            [0xE8, 0x71, 0x0A], // Google 橙
            [0x93, 0x34, 0xE6], // Google 紫
            [0xF9, 0xC2, 0x5C], // 橙黄干线
            [0xFB, 0xD3, 0x66], // 橙黄干线
            [0xAA, 0xDA, 0xFF], // Google 淡水
            [0xC9, 0xE7, 0xC7], // Google 公园绿
            [0xF7, 0xF5, 0xF0], // Google 暖白陆
            [0x8F, 0xA2, 0xB3], // 古舆图灰蓝海
            [0x73, 0x5D, 0x48], // 古舆图城墙墨棕
        ];
        let all: Vec<[u8; 3]> = [
            WV_INK_PRIMARY,
            WV_INK_SECONDARY,
            WV_INK_MUTED,
            WV_LINE_FAINT,
            WV_PAPER,
            WV_SURFACE,
            WV_SURFACE_DEEP,
            WV_ACCENT,
            WV_WATER_SHORE,
            WV_SNOW,
            WV_ICE_SHADOW,
        ]
        .into_iter()
        .chain(WV_CORE_TERRAIN)
        .map(rgb)
        .collect();
        for bad in FORBIDDEN {
            assert!(
                !all.contains(&bad),
                "禁用色 #{:02X}{:02X}{:02X} 出现在 WV 色板",
                bad[0],
                bad[1],
                bad[2]
            );
        }
    }
}
