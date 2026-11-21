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
use super::colors_wv;

/// 地图配色轴：与视图模式（`ViewMode`，技法轴）**正交**（ADR-0016）。
///
/// **单值**（2026-10-25，ADR-0066）：暮色旧纸随艺术主题整族退役，只留默认档。
/// 枚举**不删**——它同时是「配色轴」这个概念的位置标记，且被 YAML 持久化键
/// （`ui-layout.yaml` 的 `map_theme`）引用；将来若要恢复多风格，加变体即可。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MapPalette {
    /// 默认档（专业制图 · WV 自有色板）：群系真色主导 + 冷中性矢量层。
    /// 持久化键仍为 `daylight`（零迁移，见 [`Self::key`]）。
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
    /// 场景分区色：索引即 `wv_terrain::biome::district_index` 的返回值（21..=32 → 0..=11）。
    ///
    /// **为什么旁挂而不扩 `core`**：`core` 的十槽是「群系 → 旧槽位」投影的
    /// 目标空间，槽位语义被 `shade_google` 折叠规则 / `is_boundary` 边界判定
    /// / tile atlas 三处共同依赖，扩槽位会连带炸这三处。分区色按 biome
    /// 编号直查、不经投影，故独立成表。
    pub district: [Color; 12],
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

    /// **默认风格色板 = 专业制图 · WV 自有色板**（ADR-0066；2026-11-13 重绑）。
    ///
    /// 36 槽按 `docs/reviews/2026-11-13-wv-palette-spec.md §2.4` 全量接到
    /// [`colors_wv`]：
    /// - **群系真色主导**：主画布 GPU Themed 走 `biome_table` + `biome_tint=0.82`
    ///   （`terrain_field.wgsl`），`core` 只在 CPU 留守档 / CellMap 折叠语义消费，
    ///   故按低信息量折叠设计，不与 21 色群系底抢戏；
    /// - **矢量层零彩度**：注记 / 道路 / POI / 等值线 / 网格一律五枚中性墨 +
    ///   冷近白面，层级靠明度分级而非色相；
    /// - **双通道硬前提**：所有画布文字走「`WV_INK_PRIMARY` 字身 +
    ///   `WV_PAPER` 纯白晕圈」。一级墨压最暗群系底仅 1.68:1、纯白对它是
    ///   6.23:1，反之亮群系上深墨 9.56:1、纯白 1.09:1——单墨通道不可判；
    /// - **唯一强调色**：事件族 POI / 海名 / 深水位取石板蓝 `#2A5D8F`
    ///   （= GPU `WATER_SRGB`），全幅占比极小，不引入新色相。
    ///
    /// **逐格技术色**（[`Self::kind_core`]）仍沿用高饱和十色：它是**色块 / 编辑
    /// 视图**的对照色，不参与底图渲染。分区色 [`Self::district`] 12 槽暖褐为
    /// 范围外残留（规格 D5），本轮不动。
    ///
    /// POI 4 族从 Google 品牌四色改为「3 档中性明度 + 事件族唯一石板蓝」：
    /// Place=`WV_INK_PRIMARY` / Faction=`WV_INK_SECONDARY` /
    /// Region=`WV_INK_MUTED` / Event=`WV_ACCENT`，**四值互异**——
    /// `wv-app::hover_tooltip::kind_color_per_kind_distinct` 是既有产品契约
    /// （四色是浮窗里区分实体种类的唯一线索，同色即无法分辨）。
    /// 分类信息由既有 21 枚 POI 图形（ADR-0105）+ 文字标签承载，
    /// **颜色不再承载分类色相**（沿用 `(c)` L04 / B01 纪律）。
    ///
    /// 实施期裁决：规格 §2.4 接线表曾把 `dot_region` / `text_region` 也写作
    /// `WV_INK_PRIMARY`，与「3 档中性明度」的文字描述自相矛盾（只用了两档）。
    /// 这里按文字描述用足第三档，见规格文档末尾追记。
    pub const DEFAULT: PaletteColors = PaletteColors {
        core: colors_wv::WV_CORE_TERRAIN,
        kind_core: colors::CORE_TERRAIN,
        district: colors::DISTRICT_COLORS,
        soften_keep: colors_wv::WV_SOFTEN_KEEP,
        soften_toward: colors_wv::WV_SURFACE,
        poi_dot: colors_wv::WV_INK_PRIMARY,
        poi_ring: colors_wv::WV_PAPER,
        road_stroke: colors_wv::WV_ROAD,
        road_casing: colors_wv::WV_LINE_FAINT,
        street_major: colors_wv::WV_ROAD,
        street_minor: colors_wv::WV_ROAD,
        label_fill: colors_wv::WV_INK_PRIMARY,
        label_region: colors_wv::WV_INK_PRIMARY,
        label_place: colors_wv::WV_INK_PRIMARY,
        label_water: colors_wv::WV_ACCENT,
        dot_place: colors_wv::WV_INK_PRIMARY,
        // 第三档中性明度：hover 浮窗四族必须互异（见下方 POI 4 族说明）。
        dot_region: colors_wv::WV_INK_MUTED,
        dot_faction: colors_wv::WV_INK_SECONDARY,
        dot_event: colors_wv::WV_ACCENT,
        text_place: colors_wv::WV_INK_PRIMARY,
        text_region: colors_wv::WV_INK_MUTED,
        text_faction: colors_wv::WV_INK_SECONDARY,
        text_event: colors_wv::WV_ACCENT,
        snow_highlight: colors_wv::WV_PAPER,
        snow_shadow: colors_wv::WV_LINE_FAINT,
        ice_paper: colors_wv::WV_SURFACE,
        ice_ink: colors_wv::WV_INK_MUTED,
        ice_shadow: colors_wv::WV_ICE_SHADOW,
        halo_fill: colors_wv::WV_PAPER,
        isoline_ink: colors_wv::WV_INK_MUTED,
        grid_line: colors_wv::WV_LINE_FAINT,
        building_top: colors_wv::WV_SURFACE,
        building_side: colors_wv::WV_LINE_FAINT,
        event_backdrop: colors_wv::WV_SURFACE,
        line_on_light: colors_wv::WV_LINE_FAINT,
        line_on_mid: colors_wv::WV_INK_MUTED,
        line_on_dark: colors_wv::WV_INK_SECONDARY,
        deep_ocean_blue: colors_wv::WV_WATER_DEEP,
        water_shore: colors_wv::WV_WATER_SHORE,
        // 海拔色带：② 阶段冷灰绿 8 阶（ADR-0067 D3），本轮不动。
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

    /// WV 默认色板的**用户拍板色值锚点**：防手滑改值。
    ///
    /// 期望值手抄自 `docs/reviews/2026-11-13-wv-palette-spec.md §2`（不引用
    /// 实现常量，避免「改一处两处一起错」的假绿）。2026-11-13 用户拍板
    /// 「重新设计一套正确配色 / A 只重建矢量层色板」后，默认档既不回退到
    /// Google 旧色，也不沿用 ① 阶段古舆图褐墨纸色。
    #[test]
    fn wv_palette_user_pinned_hexes_are_stable() {
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
        // §2.3 core 十槽抽查：水体语义槽取 GPU / 冻结表真值，陆面槽冷中性。
        assert_eq!(f(p.core[1]), [0x9D, 0xC3, 0xD8], "浅水 = 冻结群系表 [14]");
        assert_eq!(f(p.core[2]), [0xEE, 0xF1, 0xEF], "基础陆 = 冷灰绿海拔带第 0 阶");
        assert_eq!(f(p.core[4]), [0xE6, 0xEB, 0xE6], "林地 = 低信息量冷中性面");
        assert_eq!(f(p.core[8]), [0xE2, 0xEA, 0xE3], "沼泽 = 越界回退色");
        assert_eq!(f(p.soften_toward), [0xEE, 0xF1, 0xEF], "淡化目标 = WV_SURFACE");
        // §2.4 矢量层令牌：零彩度墨阶 + 纯白双通道。
        assert_eq!(f(p.street_major), [0xFF, 0xFF, 0xFF], "主干道白填充");
        assert_eq!(f(p.road_casing), [0xDA, 0xDC, 0xE0], "路缘极淡线");
        assert_eq!(f(p.label_fill), [0x3C, 0x40, 0x43], "注记一级墨");
        assert_eq!(f(p.halo_fill), [0xFF, 0xFF, 0xFF], "注记纯白晕圈（双通道）");
        assert_eq!(f(p.label_water), [0x2A, 0x5D, 0x8F], "海名 = 唯一强调色");
        // T3/T6：接线必须指向 WV 令牌本身，而不是「恰好同值」的第二份字面量。
        assert_eq!(p.halo_fill, colors_wv::WV_PAPER, "halo_fill 必须直连 WV_PAPER");
        assert_eq!(
            p.deep_ocean_blue, colors_wv::WV_ACCENT,
            "deep_ocean_blue 必须直连 WV_ACCENT（消两套水色口径）"
        );
        assert_eq!(
            p.water_shore, colors_wv::WV_WATER_SHORE,
            "water_shore 必须直连冻结表镜像值"
        );
        // T4：两头都不取（Google 旧色 / 古舆图暖沙）。
        assert_ne!(
            p.core,
            crate::colors_google::GOOGLE_CORE_TERRAIN,
            "不得回退到 Google 地形色"
        );
        // T11（实施期新增）：hover 浮窗四族必须互异。三档中性明度 + 唯一
        // 石板蓝凑齐四值；若把 dot_region 并回一级墨就会撞色。
        let dots = [p.dot_place, p.dot_region, p.dot_faction, p.dot_event];
        for i in 0..dots.len() {
            for j in (i + 1)..dots.len() {
                assert_ne!(dots[i], dots[j], "POI kind {i} 与 kind {j} 同色");
            }
        }
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

    /// 场景分区色板：12 槽对应分区保留带 21..=32。
    ///
    /// 为什么旁挂一张表而不扩 `core`：`core` / `kind_core` 的十槽空间已被
    /// 「群系 → 旧调色板槽位」投影占满，槽位语义被 `shade_google` 折叠规则、
    /// `is_boundary` 边界判定、tile atlas 三处共同依赖——扩槽位会连带炸这三处。
    #[test]
    fn district_palette_has_one_slot_per_district() {
        let p = PaletteColors::DEFAULT;
        assert_eq!(p.district.len(), 12, "分区色板须与分区带等长");
        // 不得落进 `core[8]`（沼泽）——那是越界回退色，落进去等于分区没色。
        let fallback = p.core[8];
        for (i, c) in p.district.iter().enumerate() {
            assert_ne!(*c, fallback, "分区色槽 {i} 落进回退色");
        }
        // 分区之间必须彼此可辨——作者靠色差读分区。
        for i in 0..p.district.len() {
            for j in (i + 1)..p.district.len() {
                assert_ne!(p.district[i], p.district[j], "分区色槽 {i} 与 {j} 必须不同");
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
