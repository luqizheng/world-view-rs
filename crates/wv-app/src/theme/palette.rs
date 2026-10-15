//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! 地图主题运行时分发（ADR-0016）——「日间古舆图 / 暮色旧纸」二选一的唯一入口。
//!
//! ## 职责
//! - [`MapPalette`] —— 主题枚举（`ViewState.map_palette` 持有、UI 切换、YAML 持久化）。
//! - [`PaletteColors`] —— 主题相关令牌的**值集合**（结构体实例而非散常量），
//!   日间 / 暮色各一个 `const` 实例；着色函数显式携带 `&PaletteColors`
//!   参数（不用全局静态，测试可并发）。
//!
//! ## 为什么是「值集合 + 显式传参」
//! 地形颜色是**烘焙进纹理**的（`cells.rs` CPU 合成），palette 必须作为数据流过
//! 合成链路，切主题 = 按新 palette 重建纹理（`TerrainTexture.last_palette` 比较）。
//! 全局静态会让测试互相污染、也让「同一帧渲染两个主题」永远不可能。
//!
//! ## 边界（不做什么）
//! - **不持有 Bevy 资源**——纯数据 + 纯函数；
//! - **不是 palette.yaml Theme Pack**——本模块是常量层前身，YAML 化时
//!   接口（`PaletteColors` 字段 + `colors()` 分发）不动、只换数据源
//!   （README §12-30 待裁）。

use bevy::prelude::*;

use super::colors;
use super::colors_dusk;

/// 地图主题（配色轴）：与视图模式（`ViewMode`，技法轴）**正交**（ADR-0016）。
///
/// GoogleMap 模式继续绑 `colors_google`、轮廓图三色为模式语义不随主题切换——
/// 本枚举只管「古舆图」家族内部的日间 / 暮色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapPalette {
    /// 日间古舆图（默认）：泛黄宣纸底 + 褐墨线条。
    #[default]
    Daylight,
    /// 暮色旧纸：深褐灰纸底，黄昏 / 雨夜 / 夜景叙事场景。
    Dusk,
}

impl MapPalette {
    /// 全部主题（顶栏分段 / 命令面板遍历用）。
    pub const ALL: [MapPalette; 2] = [MapPalette::Daylight, MapPalette::Dusk];

    /// 顶栏 / 命令面板显示名。
    pub fn label(self) -> &'static str {
        match self {
            MapPalette::Daylight => "日间古舆图",
            MapPalette::Dusk => "暮色旧纸",
        }
    }

    /// 持久化键（`ui-layout.yaml` 的 `map_theme` 字段值）。
    pub fn key(self) -> &'static str {
        match self {
            MapPalette::Daylight => "daylight",
            MapPalette::Dusk => "dusk",
        }
    }

    /// 从持久化键恢复；未知值回 `None`（调用方保持缺省，V1 不报错）。
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "daylight" => Some(MapPalette::Daylight),
            "dusk" => Some(MapPalette::Dusk),
            _ => None,
        }
    }

    /// 切换到另一主题（顶栏 / 命令面板共用）。
    pub fn toggled(self) -> Self {
        match self {
            MapPalette::Daylight => MapPalette::Dusk,
            MapPalette::Dusk => MapPalette::Daylight,
        }
    }
}

/// 一套主题的全部**主题相关**色值（日间 / 暮色各一常量实例）。
///
/// 主题**不变**项（accent / ui_link / 派系 / 轮廓图三色 / Google 色板）不在
/// 本结构里——它们仍直引 [`super::colors`] 常量。
#[derive(Debug, Clone, Copy)]
pub struct PaletteColors {
    /// 地形基色（索引即 `TerrainKind` 0..10）。
    pub core: [Color; 10],
    /// 底图淡化保留比例（混合公式见 [`Self::soften_color`]）。
    pub soften_keep: f32,
    /// 底图淡化目标色（= 该主题的纸面色）。
    pub soften_toward: Color,
    /// POI 图标底色。
    pub poi_dot: Color,
    /// POI 外环。
    pub poi_ring: Color,
    /// 城市间连线描线。
    pub road_stroke: Color,
    /// 道路 casing（外缘）。
    pub road_casing: Color,
    /// 主干道填充。
    pub street_major: Color,
    /// 次级街巷填充。
    pub street_minor: Color,
    /// 地名 / 街名文字填充。
    pub label_fill: Color,
    /// 标注字重梯度（手绘地图 #2，2026-10-15）：region（国/大区）= 最深墨。
    pub label_region: Color,
    /// 标注字重梯度：place（城/POI）= `label_fill` 浅一档。
    pub label_place: Color,
    /// 标注字重梯度：water（海名）= 青蓝（civ-atlas 海名色族）。
    pub label_water: Color,
    /// 文字晕边填充（= 纸面色族）。
    pub halo_fill: Color,
    /// 等值线 / 海岸线墨色。
    pub isoline_ink: Color,
    /// 编辑网格线基色。
    pub grid_line: Color,
    /// 建筑顶面（M3 建筑挤出消费）。
    pub building_top: Color,
    /// 建筑侧面 / 投影基色。
    pub building_side: Color,
    /// 事件图极简底图。
    pub event_backdrop: Color,
    /// 描线三档（浅 / 中 / 深）。
    pub line_on_light: Color,
    pub line_on_mid: Color,
    pub line_on_dark: Color,
}

impl PaletteColors {
    /// 取地形基色；`kind` 越界时回退到沼泽色（可辨识，不是黑）。
    pub fn core_terrain(&self, kind: u8) -> Color {
        self.core
            .get(kind as usize)
            .copied()
            .unwrap_or(self.core[8])
    }

    /// 底图淡化混合（`colors::soften_color` 的按主题版；公式同源不二写）。
    pub fn soften_color(&self, base: Color) -> Color {
        let (Color::Srgba(b), Color::Srgba(t)) = (base, self.soften_toward) else {
            return base;
        };
        let k = self.soften_keep;
        Color::srgb(
            b.red * k + t.red * (1.0 - k),
            b.green * k + t.green * (1.0 - k),
            b.blue * k + t.blue * (1.0 - k),
        )
    }

    /// 按主题取实例（渲染系统的唯一入口：`palette::colors(view.map_palette)`）。
    pub fn for_palette(p: MapPalette) -> &'static PaletteColors {
        match p {
            MapPalette::Daylight => &Self::DAYLIGHT,
            MapPalette::Dusk => &Self::DUSK,
        }
    }

    /// 日间古舆图：值源 = [`colors`]（`colors.rs` 是 ART_STYLE_SPEC §1.4.4 的投影）。
    pub const DAYLIGHT: PaletteColors = PaletteColors {
        core: colors::CORE_TERRAIN,
        soften_keep: colors::SOFTEN_KEEP,
        soften_toward: colors::SOFTEN_TOWARD,
        poi_dot: colors::PLACE_DOT,
        poi_ring: colors::POI_RING,
        road_stroke: colors::ROAD_STROKE,
        road_casing: colors::ROAD_CASING,
        street_major: colors::STREET_MAJOR,
        street_minor: colors::STREET_MINOR,
        label_fill: colors::LABEL_FILL,
        label_region: colors::LABEL_REGION,
        label_place: colors::LABEL_PLACE,
        label_water: colors::LABEL_WATER,
        halo_fill: colors::HALO_FILL,
        isoline_ink: colors::ISOLINE_INK,
        grid_line: colors::GRID_LINE,
        building_top: colors::BUILDING_TOP,
        building_side: colors::BUILDING_SIDE,
        event_backdrop: colors::EVENT_VIEW_BACKDROP,
        line_on_light: colors::LINE_ON_LIGHT,
        line_on_mid: colors::LINE_ON_MID,
        line_on_dark: colors::LINE_ON_DARK,
    };

    /// 暮色旧纸：值源 = [`colors_dusk`]（同槽位镜像）。
    pub const DUSK: PaletteColors = PaletteColors {
        core: colors_dusk::DUSK_CORE_TERRAIN,
        soften_keep: colors::SOFTEN_KEEP,
        soften_toward: colors_dusk::DUSK_SOFTEN_TOWARD,
        poi_dot: colors_dusk::DUSK_PLACE_DOT,
        poi_ring: colors_dusk::DUSK_POI_RING,
        road_stroke: colors_dusk::DUSK_ROAD_STROKE,
        road_casing: colors_dusk::DUSK_ROAD_CASING,
        street_major: colors_dusk::DUSK_STREET_MAJOR,
        street_minor: colors_dusk::DUSK_STREET_MINOR,
        label_fill: colors_dusk::DUSK_LABEL_FILL,
        label_region: colors_dusk::DUSK_LABEL_REGION,
        label_place: colors_dusk::DUSK_LABEL_FILL, // place 复用 LABEL_FILL（视觉一致）
        label_water: colors_dusk::DUSK_LABEL_WATER,
        halo_fill: colors_dusk::DUSK_HALO_FILL,
        isoline_ink: colors_dusk::DUSK_ISOLINE_INK,
        grid_line: colors_dusk::DUSK_GRID_LINE,
        building_top: colors_dusk::DUSK_BUILDING_TOP,
        building_side: colors_dusk::DUSK_BUILDING_SIDE,
        event_backdrop: colors_dusk::DUSK_EVENT_VIEW_BACKDROP,
        line_on_light: colors_dusk::DUSK_LINE_ON_LIGHT,
        line_on_mid: colors_dusk::DUSK_LINE_ON_MID,
        line_on_dark: colors_dusk::DUSK_LINE_ON_DARK,
    };
}

/// 按主题取实例的自由函数版（惯用短名）。
pub fn colors(p: MapPalette) -> &'static PaletteColors {
    PaletteColors::for_palette(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 镜像完整性：两套主题槽位一一对应，且关键槽位色值**真的不同**
    ///（防「改了日间忘配暮色」或装配时接错槽位——切暮色没反应的缺陷在这拦）。
    #[test]
    fn dusk_mirrors_daylight_slots_and_differs_on_them() {
        let day = PaletteColors::DAYLIGHT;
        let dusk = PaletteColors::DUSK;
        // 每个槽位都必须「两主题不同」——同值即装配错误的信号。
        assert_ne!(day.core[3], dusk.core[3], "草色两主题应不同");
        assert_ne!(day.soften_toward, dusk.soften_toward, "纸面色应不同");
        assert_ne!(day.poi_dot, dusk.poi_dot, "POI 底色应不同");
        assert_ne!(day.street_major, dusk.street_major, "主干道应不同");
        assert_ne!(day.street_minor, dusk.street_minor, "次级街巷应不同");
        assert_ne!(day.label_fill, dusk.label_fill, "文字色应不同");
        assert_ne!(day.isoline_ink, dusk.isoline_ink, "等值线墨应不同");
        assert_ne!(day.building_top, dusk.building_top, "建筑顶面应不同");
        assert_ne!(day.road_stroke, dusk.road_stroke, "城市连线应不同");
    }

    /// 用户定稿色值锚点：防手滑改值（改动应连带更新 ART_STYLE_SPEC §1.4.4）。
    #[test]
    fn daylight_user_pinned_hexes_are_stable() {
        let f = |c: Color| -> [u8; 3] {
            let Color::Srgba(s) = c else {
                panic!("应为 Srgba")
            };
            [
                (s.red * 255.0).round() as u8,
                (s.green * 255.0).round() as u8,
                (s.blue * 255.0).round() as u8,
            ]
        };
        assert_eq!(
            f(PaletteColors::DAYLIGHT.core[1]),
            [0x98, 0xA9, 0xB9],
            "浅水"
        );
        assert_eq!(f(PaletteColors::DAYLIGHT.core[3]), [0xA8, 0xB8, 0x98], "草");
        assert_eq!(
            f(PaletteColors::DAYLIGHT.soften_toward),
            [0xF2, 0xEA, 0xD8],
            "纸面"
        );
        assert_eq!(
            f(PaletteColors::DAYLIGHT.street_major),
            [0xA6, 0x7C, 0x52],
            "主干道"
        );
        assert_eq!(
            f(PaletteColors::DAYLIGHT.street_minor),
            [0xD9, 0xC8, 0xB4],
            "次级"
        );
        assert_eq!(
            f(PaletteColors::DAYLIGHT.label_fill),
            [0x4A, 0x3F, 0x35],
            "文字"
        );
        assert_eq!(
            f(PaletteColors::DUSK.core[1]),
            [0x44, 0x50, 0x5C],
            "暮色浅水"
        );
        assert_eq!(
            f(PaletteColors::DUSK.label_fill),
            [0xC9, 0xBB, 0xA8],
            "暮色文字"
        );
    }

    /// 越界 kind 回退沼泽色（与 `colors::core_terrain` 同口径）。
    #[test]
    fn out_of_range_kind_falls_back_without_panic() {
        for p in MapPalette::ALL {
            let c = colors(p).core_terrain(200);
            assert_eq!(c, colors(p).core[8], "{p:?} 越界回退应为沼泽色");
        }
    }

    /// 持久化键 round-trip + 未知键拒绝。
    #[test]
    fn palette_key_round_trips_and_rejects_unknown() {
        for p in MapPalette::ALL {
            assert_eq!(MapPalette::from_key(p.key()), Some(p), "{} 应还原", p.key());
        }
        assert_eq!(MapPalette::from_key("sepia"), None);
    }

    /// 淡化混合的不动点：目标色淡化后仍是目标色（两主题各自成立）。
    #[test]
    fn soften_color_of_toward_is_idempotent_for_both_palettes() {
        for p in MapPalette::ALL {
            let c = colors(p);
            let out = c.soften_color(c.soften_toward);
            let (Color::Srgba(a), Color::Srgba(b)) = (out, c.soften_toward) else {
                panic!("应为 Srgba")
            };
            assert!((a.red - b.red).abs() < 1e-5, "{p:?} R 漂移");
            assert!((a.green - b.green).abs() < 1e-5, "{p:?} G 漂移");
            assert!((a.blue - b.blue).abs() < 1e-5, "{p:?} B 漂移");
        }
    }
}
