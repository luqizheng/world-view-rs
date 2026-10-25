//! Google Default Map 色板 —— 地图浏览体验的第一入口。
//!
//! ## 为什么不是上一版 Retro JSON
//!
//! 2026-10-13 用户明确要求「大地图尽量模仿 Google Maps」。上一版把一份
//! 低饱和羊皮纸 style JSON 当成 Google 默认地图，实际结果是暖棕色纸图，
//! 与 Google Default Map 的「近白陆地 / 淡水 / 白路灰缘 / 红 POI」差距明显。
//! 本文件改为服务于这个可见目标；旧 Retro 值不再作为权威。
//!
//! ## 约束
//!
//! - 只改 `wv-app` 渲染令牌，不改 `wv-core` 地形数据；
//! - 不引入在线瓦片或网络依赖，遵守 P1 / P3；
//! - 地形只折叠为 Google 式底图语义：水、公园/绿地、湿地的低信息量色块；
//!   高度分带不在普通地图中展示，仍可切到「真实地形」分析。
//! - 色值唯一入口在此，不允许渲染系统散落裸 hex。
//!
//! ## 色值口径
//!
//! 色值取 Google Maps Default Map 的公共视觉特征，而非逐像素提取商标 UI：
//! 近白陆地 `#f8f9fa`、淡水 `#aadaff`、绿地 `#c9e7c7`、深水略压暗
//! `#9fc7ef`、道路 fill 白色、道路 casing 浅灰 `#dadce0`、文字灰
//! `#3c4043`、Google 红 POI `#ea4335`。

use bevy::prelude::*;

/// 从 sRGB 十六进制构造 `Color`。
const fn hex(rgb: u32) -> Color {
    Color::srgb(
        ((rgb >> 16) & 0xFF) as f32 / 255.0,
        ((rgb >> 8) & 0xFF) as f32 / 255.0,
        (rgb & 0xFF) as f32 / 255.0,
    )
}

/// Google Default Map 地形基色。索引即 `TerrainKind` 编号（0..10）。
///
/// Google 的普通地图不按海拔分带上色；`shade_google` 会把沙 / 草 / 丘 / 山 /
/// 雪等陆上类折叠到 `[2]`。这里仍为每档给出可辨识值，供未来专题图或调试使用。
pub const GOOGLE_CORE_TERRAIN: [Color; 10] = [
    hex(0x9FC7EF), // 0 water_deep     深水（水色压暗）
    hex(0xAADAFF), // 1 water_shallow  浅水
    hex(0xF8F9FA), // 2 sand           Google Default 的基础陆地
    hex(0xF3F2EE), // 3 grass          草原（普通地图折叠为基础陆地）
    hex(0xC9E7C7), // 4 forest         公园 / 绿地
    hex(0xEFEDE7), // 5 hill           丘陵（普通地图折叠为基础陆地）
    hex(0xE8E5DE), // 6 mountain       山地（普通地图折叠为基础陆地）
    hex(0xFBFBFA), // 7 snow           雪线（普通地图折叠为基础陆地）
    hex(0xBBDCC9), // 8 marsh          湿地 / 沼泽
    hex(0xFFFFFF), // 9 road           道路基色（矢量层另有 fill/casing）
];

/// Google 式海岸线：与浅水同色，靠陆地高明度差形成干净边界。
pub const GOOGLE_COAST_LINE: Color = hex(0xAADAFF);

/// 城市 / 地点 POI 内芯：Google 红。红色在近白底图上最像地图检索结果。
pub const GOOGLE_PLACE_DOT: Color = hex(0xEA4335);

/// POI 白环，用于在绿地和水边保持可辨。
pub const GOOGLE_POI_RING: Color = hex(0xFFFFFF);

/// 道路外缘（casing）：浅灰。它与白色 fill 的宽度差形成 Google 式路缘。
pub const GOOGLE_ROAD_CASING: Color = hex(0xDADCE0);

/// 道路主体（fill）：白色。
pub const GOOGLE_ROAD_FILL: Color = hex(0xFFFFFF);

/// 高速公路 / 主干道强调色。
///
/// **2026-10-25 起已接线**：本令牌此前是「当前 Edge 尚无道路等级，先作为专题图
/// 预留令牌」的空转常量；`PlaceEdge::effective_rank` 提供 1/2/3 三档之后，
/// 改由 [`GOOGLE_ROAD_GRADES`] 承担分级配色，本常量保留为最高档的参照值。
pub const GOOGLE_HIGHWAY_STROKE: Color = hex(0xF9C25C);

/// 一档道路的分级样式（填充色 / 路缘色 / 两个宽度，单位屏幕像素）。
///
/// **为什么需要分级**：Google / 百度普通地图的层次感几乎全部来自「粗细 + 色阶」。
/// 单一白路 + 灰缘会让整个路网糊成一片同质的网——2026-10-25 的样张对比里，
/// 关掉分级后地图立刻失去可读的主次。
#[derive(Debug, Clone, Copy)]
pub struct RoadGradeStyle {
    /// 道路主体色。
    pub fill: Color,
    /// 路缘色（比 fill 宽出的那一圈）。
    pub casing: Color,
    /// 主体宽度（屏幕像素）。
    pub fill_px: f32,
    /// 路缘宽度（屏幕像素，必须 > `fill_px`）。
    pub casing_px: f32,
}

/// 三档道路样式，索引 0 = 最高等级。
///
/// **与 LOD 三档同构**（ADR-0047 D4 的 `road_rank_cuts`）：`rank ≥ 3` 城际干线
/// （Nation 档起可见）/ `rank = 2` 区域干道（Region 档起）/ `rank = 1` 地方道路
/// （仅 Edit 档）。排序后同档连续，故渲染侧只需三段前缀切片，零额外存储。
pub const GOOGLE_ROAD_GRADES: [RoadGradeStyle; 3] = [
    // 城际干线：橙黄填充 + 深一档橙缘（「沈海高速」的观感，最粗）
    RoadGradeStyle {
        fill: hex(0xF9C25C),
        casing: hex(0xE3A54B),
        fill_px: 5.0,
        casing_px: 7.2,
    },
    // 区域干道：饱和黄填充 + 浅灰缘（百度城市区最常见的「主干黄」，2026-10-25
    // 依第二张参考图（珠海 / 澳门）校准：初版 #FBDB92 过淡，密集路网里读不出主次）
    RoadGradeStyle {
        fill: hex(0xFBD366),
        casing: hex(0xDADCE0),
        fill_px: 3.6,
        casing_px: 5.4,
    },
    // 地方道路：白填充 + 灰缘（分级前所有道路的样子）
    RoadGradeStyle {
        fill: hex(0xFFFFFF),
        casing: hex(0xDADCE0),
        fill_px: 2.4,
        casing_px: 4.0,
    },
];

/// 选中 / 链接等交互强调色：Google Blue。
pub const GOOGLE_ACCENT_HIGHLIGHT: Color = hex(0x1A73E8);

/// 文字光晕：白底描边色。Text2d 暂未做描边时仍保留为唯一视觉目标。
pub const GOOGLE_LABEL_STROKE: Color = hex(0xFFFFFF);

/// 文字填色：Google UI 的深灰，而不是纯黑，避免标签压过地图。
pub const GOOGLE_LABEL_FILL: Color = hex(0x3C4043);
/// 地图控件底色：白卡片。它是地图体验的一部分，不跟随暗色软件壳。
pub const GOOGLE_CONTROL_BG: Color = hex(0xFFFFFF);
/// 地图控件边框。
pub const GOOGLE_CONTROL_BORDER: Color = hex(0xDADCE0);
/// 地图控件文字 / 图标色。
pub const GOOGLE_CONTROL_TEXT: Color = hex(0x3C4043);
/// 地图控件当前工具底色。
pub const GOOGLE_CONTROL_ACTIVE: Color = hex(0xE8F0FE);

/// Google 式浅色轮廓图水面。
pub const GOOGLE_COAST_WATER: Color = hex(0xAADAFF);
/// Google 式浅色轮廓图陆地。
pub const GOOGLE_COAST_LAND: Color = hex(0xF8F9FA);
/// Google 式浅色轮廓图海岸线。
pub const GOOGLE_COAST_LINE_COLOR: Color = hex(0x9AA0A6);

/// 专业制图的行政 / 势力边界线（样张图例「边界 · 虚线 #C4C4C4」）。
///
/// 与 `BORDER_INK`（势力图模式的深褐墨线）分开：势力图要「墨线夹两色」的
/// 手绘语汇，普通地图只要一条低对比灰线，不抢道路与注记。
pub const GOOGLE_BORDER: Color = hex(0xC4C4C4);

// ── POI 分类色（ADR-0064）──
//
// 色相取检索类地图的通用语汇：餐饮橙、住宿紫、医疗红、景点绿、交通蓝、
// 行政青、教育靛、商业品红。未分类 / 聚落沿用 `GOOGLE_PLACE_DOT`（Google 红）
// ——红点是「搜索结果」的默认外观，也是最不容易与绿地/水面混淆的一档。
//
// 为什么不放在 `wv-world-cell-map`：那是数据视图的固定色（ADR-0054 禁令），
// 而 POI 分类色属于地图呈现的主题色，归 here。

/// 餐饮。
pub const GOOGLE_POI_FOOD: Color = hex(0xF57C00);
/// 住宿。
pub const GOOGLE_POI_LODGING: Color = hex(0x7B1FA2);
/// 医疗。
pub const GOOGLE_POI_MEDICAL: Color = hex(0xD93025);
/// 景点 / 公园 / 古迹。
pub const GOOGLE_POI_SCENIC: Color = hex(0x1E8E3E);
/// 交通站点（车站 / 码头 / 渡口）。
pub const GOOGLE_POI_TRANSIT: Color = hex(0x1A73E8);
/// 行政 / 政府。
pub const GOOGLE_POI_GOVERNMENT: Color = hex(0x00796B);
/// 教育 / 科研。
pub const GOOGLE_POI_EDUCATION: Color = hex(0x3949AB);
/// 商业 / 购物。
pub const GOOGLE_POI_SHOP: Color = hex(0xC2185B);

/// 兼容旧调用的淡化目标色；Google 模式实际会忽略淡化开关，避免洗掉水色。
pub const GOOGLE_SOFTEN_TOWARD: Color = hex(0xF8F9FA);

/// 取地形基色；越界回退湿地色，可辨识且不 panic。
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
        // 越界 kind 不应 panic，回退到湿地色。
        let c = google_core_terrain(200);
        assert_eq!(c, GOOGLE_CORE_TERRAIN[8]);
    }

    #[test]
    fn hex_values_match_google_default_map() {
        // 抽查 Default Map 关键色是否落到位（防手工输错 hex 的回归）：
        //   water = #aadaff,  land = #f8f9fa,  park = #c9e7c7,  road fill = #ffffff
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
        let water = expect(GOOGLE_CORE_TERRAIN[1], 0xAADAFF);
        let land = expect(GOOGLE_CORE_TERRAIN[2], 0xF8F9FA);
        let park = expect(GOOGLE_CORE_TERRAIN[4], 0xC9E7C7);
        let road = expect(GOOGLE_ROAD_FILL, 0xFFFFFF);
        for (name, err) in [
            ("water", water),
            ("land", land),
            ("park", park),
            ("road", road),
        ] {
            assert!(
                err.iter().all(|e| *e < 1e-6),
                "{name} hex 与 Google Default Map 不符，误差 {err:?}"
            );
        }
    }

    #[test]
    fn google_default_uses_red_pois_and_gray_road_casing() {
        let expect_rgb = |c: Color, want: u32| {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba");
            };
            let got = [
                (s.red * 255.0).round() as u32,
                (s.green * 255.0).round() as u32,
                (s.blue * 255.0).round() as u32,
            ];
            assert_eq!(got[0], (want >> 16) & 0xFF, "R mismatch: got {:?}", got);
            assert_eq!(got[1], (want >> 8) & 0xFF, "G mismatch: got {:?}", got);
            assert_eq!(got[2], want & 0xFF, "B mismatch: got {:?}", got);
        };
        expect_rgb(GOOGLE_PLACE_DOT, 0xEA4335);
        expect_rgb(GOOGLE_ROAD_CASING, 0xDADCE0);
        expect_rgb(GOOGLE_LABEL_FILL, 0x3C4043);
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
        // 水陆不只靠色相区分，也保持明度差以支持低视力 /灰阶阅读。
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
