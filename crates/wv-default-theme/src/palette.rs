//! 待批准：本文件在 `app` feature 下才编译。
//! 地图配色轴（ADR-0016 / ADR-0066）——**单一默认风格**的色板结构与分发。
//!
//! ## 职责
//! - [`MapPalette`] —— 配色轴枚举（`ViewState.map_palette` 持有、YAML 持久化）。
//!   **单值**（2026-10-25，ADR-0066）：艺术主题整族退役，只留默认档。
//! - [`PaletteColors`] —— 配色相关令牌的**值集合**（结构体实例而非散常量），
//!   一个 `const` 实例；着色函数显式携带 `&PaletteColors` 参数（不用全局静态，
//!   测试可并发）。
//!
//! ## 为什么是「值集合 + 显式传参」
//! 地形颜色是**烘焙进纹理**的（`cells.rs` CPU 合成），palette 必须作为数据流过
//! 合成链路。全局静态会让测试互相污染、也让「同一帧渲染两个风格」永远不可能。
//!
//! ## 边界（不做什么）
//! - **不持有 Bevy 资源**——纯数据 + 纯函数；
//! - **不做多风格分发**——`for_palette` 现在是恒等映射（保留函数名与签名，
//!   让「按轴取色板」这条调用链在未来重新需要多风格时零改动）。
//!
//! [`colors_google`]: super::colors_google

use bevy::prelude::*;

use super::colors;

/// 地图配色轴：与视图模式（`ViewMode`，技法轴）**正交**（ADR-0016）。
///
/// **单值**（2026-10-25，ADR-0066）：暮色旧纸随艺术主题整族退役，只留默认档。
/// 枚举**不删**——它同时是「配色轴」这个概念的位置标记，且被 YAML 持久化键
/// （`ui-layout.yaml` 的 `map_theme`）引用；将来若要恢复多风格，加变体即可。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapPalette {
    /// 默认档（专业制图）：近白陆地 + 淡水 + 公园绿 + 橙黄分级道路。
    #[default]
    Daylight,
}

impl MapPalette {
    /// 全部配色档（顶栏分段 / 命令面板遍历用）。
    pub const ALL: [MapPalette; 1] = [MapPalette::Daylight];

    /// 顶栏 / 命令面板显示名。
    pub fn label(self) -> &'static str {
        match self {
            MapPalette::Daylight => "专业制图",
        }
    }

    /// 持久化键（`ui-layout.yaml` 的 `map_theme` 字段值）。
    ///
    /// **键名保持 `daylight`**（不改成 `default`）：存量工程的 `ui-layout.yaml`
    /// 里写的就是这个值，改键等于让旧工程「主题丢失」。ADR-0066 明确要求
    /// 持久化键**零迁移**。
    pub fn key(self) -> &'static str {
        match self {
            MapPalette::Daylight => "daylight",
        }
    }

    /// 从持久化键恢复；未知值回 `None`（调用方保持缺省，V1 不报错）。
    ///
    /// `dusk` **仍然被接受**（回落到默认档）：暮色已退役，但存量工程可能存着
    /// 这个值——静默回落到默认档，而不是让工程「打不开」。
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "daylight" | "dusk" => Some(MapPalette::Daylight),
            _ => None,
        }
    }

    /// 切换到另一配色档。
    ///
    /// **单值下是恒等映射**（ADR-0066）：艺术主题退役后没有第二档可切。
    /// 保留此方法是为了让「切换配色」这条调用链（顶栏控件 / 命令面板）不必
    /// 改签名——将来恢复多风格时改这里即可。
    pub fn toggled(self) -> Self {
        match self {
            MapPalette::Daylight => MapPalette::Daylight,
        }
    }
}

/// 一套风格的全部**风格相关**色值（单值实例 [`PaletteColors::DEFAULT`]）。
///
/// 风格**不变**项（accent / ui_link / 派系 / 描线三档 / 编辑器反馈色）不在
/// 本结构里——它们仍直引 [`super::colors`] 常量。
///
/// **两份地形色**（ADR-0066）：
///
/// - [`Self::core`] —— **风格色**：本风格给每种地形分的颜色。专业制图把多数
///   陆上类折叠为近白陆地（`shade_google`），故它只在水 / 森林 / 沼泽三档
///   上真正有区分度。
/// - [`Self::kind_core`] —— **逐格技术色**：色块地形 / 编辑视图用的「一格一色」
///   对照色（每种地形都不同）。它**不是风格**，是**视图技术**——色块视图的
///   存在意义就是让作者一眼看出每格是什么地形，用风格色会全变成近白。
///
/// 两份色表分离后，「换风格」不再影响色块视图的可读性——这正是把
/// 「风格」与「视图技术」拆开的收益。
#[derive(Debug, Clone, Copy)]
pub struct PaletteColors {
    /// 风格色：地形基色（索引即 `TerrainKind` 0..10）。
    pub core: [Color; 10],
    /// 逐格技术色：色块 / 编辑视图用的「一格一色」对照色（同索引语义）。
    pub kind_core: [Color; 10],
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
    /// POI 圆点色族（手绘地图 #9）—— 按 `EntityKind` 选色。
    pub dot_place: Color,
    pub dot_region: Color,
    pub dot_faction: Color,
    pub dot_event: Color,
    /// POI hover 文字色族（手绘地图 #12）—— 与 `dot_*` 同色族略深。
    pub text_place: Color,
    pub text_region: Color,
    pub text_faction: Color,
    pub text_event: Color,
    /// 雪高光色（手绘地图 #3）—— 比 `core_terrain[7]` 略亮，近白。
    pub snow_highlight: Color,
    /// 雪冷阴影色（手绘地图 #3）—— 冷蓝灰，叠在 high-altitude 雪格上。
    pub snow_shadow: Color,
    /// 海冰纸面（手绘地图 #5）—— 极地海域叠的近白略冷。
    pub ice_paper: Color,
    /// 冰缘墨线（手绘地图 #5）—— 偏冷的冰边描线色（预留，未在 isoline 消费）。
    pub ice_ink: Color,
    /// 海冰背光排线色（手绘地图 #5）—— 淡蓝灰。
    pub ice_shadow: Color,
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
    /// 「明确深蓝」（2026-10-16 用户裁定）：最深水位直接走这个颜色，
    /// 不再 lerp `core_terrain` 的「深海」+ soften 让过薄 + veil 让过淡——
    /// 任何染色叠加后仍能辨为「海」。见 `colors::DEEP_OCEAN_BLUE` 注释。
    pub deep_ocean_blue: Color,
    /// 近岸浅滩色（2026-10-21 S2）：离岸距离场驱动的「极浅滩」带——
    /// 与 `deep_ocean_blue` 对偶：那个管最深的一格，这个管贴岸的一圈。
    /// 见 `colors::WATER_SHORE` 注释。
    pub water_shore: Color,
    /// **海拔色带**（ADR-0067 D3）：8 阶，低 → 高。
    ///
    /// 专业制图用它表达山体的高度层次（色带读「高」，晕渲读「坡」）。
    /// 艺术主题不设（`None`）——它们的边界靠手绘晕 / 等值线表达，不需要色带。
    pub elevation_ramp: Option<[[u8; 3]; 8]>,
    /// 海拔色带的混入强度（0..=1）。
    pub elevation_strength: f32,
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

    /// 取本风格的实例（渲染系统的唯一入口）。
    ///
    /// **单值**（ADR-0066）：多风格机制退役，只留一个实例。保留 `for_palette`
    /// 这层间接是为了让「按轴取色板」的调用链在将来恢复多风格时零改动。
    pub fn for_palette(_p: MapPalette) -> &'static PaletteColors {
        &Self::DEFAULT
    }

    /// **默认风格色板 = 专业制图**（ADR-0066）。
    ///
    /// 风格色取百度 / Google 普通地图那套：近白陆地 + 淡水 + 公园绿 + 橙黄
    /// 分级道路 + 分类 POI + 中性灰描线。底图极简，要素（道路 / 注记 / POI）
    /// 才是可读性的来源——这是「专业地图」与「艺术地图」的根本分野。
    ///
    /// **逐格技术色**（[`Self::kind_core`]）沿用旧的十色高饱和表：它是**色块 /
    /// 编辑视图**的对照色，不参与专业制图的底图渲染。
    pub const DEFAULT: PaletteColors = PaletteColors {
        core: super::colors_google::GOOGLE_CORE_TERRAIN,
        kind_core: colors::CORE_TERRAIN,
        soften_keep: super::colors_google::GOOGLE_SOFTEN_KEEP,
        soften_toward: super::colors_google::GOOGLE_PAPER,
        poi_dot: super::colors_google::GOOGLE_PLACE_DOT,
        poi_ring: super::colors_google::GOOGLE_POI_RING,
        road_stroke: super::colors_google::GOOGLE_ROAD_FILL,
        road_casing: super::colors_google::GOOGLE_ROAD_CASING,
        street_major: super::colors_google::GOOGLE_ROAD_FILL,
        street_minor: super::colors_google::GOOGLE_ROAD_FILL,
        label_fill: super::colors_google::GOOGLE_LABEL_FILL,
        label_region: super::colors_google::GOOGLE_LABEL_REGION,
        label_place: super::colors_google::GOOGLE_LABEL_FILL,
        label_water: super::colors_google::GOOGLE_LABEL_WATER,
        dot_place: super::colors_google::GOOGLE_DOT_PLACE,
        dot_region: super::colors_google::GOOGLE_DOT_REGION,
        dot_faction: super::colors_google::GOOGLE_DOT_FACTION,
        dot_event: super::colors_google::GOOGLE_DOT_EVENT,
        text_place: super::colors_google::GOOGLE_TEXT_PLACE,
        text_region: super::colors_google::GOOGLE_TEXT_REGION,
        text_faction: super::colors_google::GOOGLE_TEXT_FACTION,
        text_event: super::colors_google::GOOGLE_TEXT_EVENT,
        snow_highlight: super::colors_google::GOOGLE_SNOW_HIGHLIGHT,
        snow_shadow: super::colors_google::GOOGLE_SNOW_SHADOW,
        ice_paper: super::colors_google::GOOGLE_ICE_PAPER,
        ice_ink: super::colors_google::GOOGLE_ICE_INK,
        ice_shadow: super::colors_google::GOOGLE_ICE_SHADOW,
        halo_fill: super::colors_google::GOOGLE_LABEL_STROKE,
        isoline_ink: super::colors_google::GOOGLE_ISOLINE_INK,
        grid_line: super::colors_google::GOOGLE_GRID_LINE,
        building_top: super::colors_google::GOOGLE_BUILDING_TOP,
        building_side: super::colors_google::GOOGLE_BUILDING_SIDE,
        event_backdrop: super::colors_google::GOOGLE_EVENT_BACKDROP,
        line_on_light: super::colors_google::GOOGLE_LINE_ON_LIGHT,
        line_on_mid: super::colors_google::GOOGLE_LINE_ON_MID,
        line_on_dark: super::colors_google::GOOGLE_LINE_ON_DARK,
        deep_ocean_blue: super::colors_google::GOOGLE_DEEP_OCEAN_BLUE,
        water_shore: super::colors_google::GOOGLE_WATER_SHORE,
        // 海拔色带：专业制图专有（ADR-0067 D3）。
        elevation_ramp: Some(super::colors_google::GOOGLE_ELEVATION_RAMP),
        elevation_strength: super::colors_google::GOOGLE_ELEVATION_STRENGTH,
    };
}

/// 按主题取实例的自由函数版（惯用短名）。
pub fn colors(p: MapPalette) -> &'static PaletteColors {
    PaletteColors::for_palette(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 专业制图色板的用户定稿色值锚点：防手滑改值。
    ///
    /// 这些值取自百度 / Google 普通地图，是「专业制图」的视觉契约——改了它们
    /// 就是改了本软件的默认观感（ADR-0066）。
    #[test]
    fn professional_palette_user_pinned_hexes_are_stable() {
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
        let p = PaletteColors::DEFAULT;
        assert_eq!(f(p.core[1]), [0xAA, 0xDA, 0xFF], "浅水");
        assert_eq!(f(p.core[2]), [0xF8, 0xF9, 0xFA], "近白陆地");
        assert_eq!(f(p.core[4]), [0xC9, 0xE7, 0xC7], "公园绿");
        assert_eq!(f(p.core[8]), [0xBB, 0xDC, 0xC9], "湿绿");
        assert_eq!(f(p.soften_toward), [0xF8, 0xF9, 0xFA], "近白纸面");
        assert_eq!(f(p.street_major), [0xFF, 0xFF, 0xFF], "道路白填充");
        assert_eq!(f(p.road_casing), [0xDA, 0xDC, 0xE0], "路缘灰");
        assert_eq!(f(p.label_fill), [0x3C, 0x40, 0x43], "注记深灰");
        assert_eq!(f(p.halo_fill), [0xFF, 0xFF, 0xFF], "注记白描边");
    }

    /// **风格色**与**逐格技术色**必须不同：前者是专业制图（多数陆上类折叠为
    /// 近白），后者是色块 / 编辑视图的「一格一色」对照色。若两者被写成同一份，
    /// 色块视图会退化成一片近白——作者就看不出每格是什么地形了。
    #[test]
    fn style_core_and_kind_core_are_distinct() {
        let p = PaletteColors::DEFAULT;
        assert_ne!(
            p.core[3], p.kind_core[3],
            "草色：风格色（近白系）与逐格技术色（高饱和）必须不同"
        );
        assert_ne!(p.core[4], p.kind_core[4], "林色同理");
        // 逐格技术色必须「十档互不相同」——色块视图靠差异读地形。
        for i in 0..10 {
            for j in (i + 1)..10 {
                assert_ne!(
                    p.kind_core[i], p.kind_core[j],
                    "逐格技术色槽 {i} 与 {j} 必须不同（色块视图靠差异读地形）"
                );
            }
        }
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
    fn soften_color_of_toward_is_idempotent() {
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
