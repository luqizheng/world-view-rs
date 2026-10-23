//! 固定色包：wv-theme 对应类型的**平面镜像**与冻结取值（ADR-0054 D4）。
//!
//! ## 为什么是镜像
//! 本 crate 禁依赖任何 theme crate（crate 级固定色）。wv-theme 的
//! `PaletteColors` / `TreatmentFlags` / `SeaRecipe` 是**纯数据**结构，此处
//! 逐字段镜像（字段名 1:1），着色函数体零改动；`ThemePack → CellPack` 的
//! 换算在 wv-render 侧（只有它许依赖 wv-theme）。
//!
//! ## 冻结值的出处
//! [`CellPack::fixture`] = Daylight 内置色板（wv-theme `colors::` 常量投影）
//! 叠 `TreatmentFlags::HAND_DRAWN` 与 [`watercolor`] 配方，2026-10-23 自
//! wv-theme 逐值抄录（`hex(0x…)` 值与 f32 参数原样）。主题浏览态不走这里——
//! wv-render 会构造自己的 `CellPack` 喂同一条合成管线。
//!
//! ## 修改纪律
//! 镜像字段**只增不改名**（跟 wv-theme 源型对齐）；冻结值改动须在
//! ADR-0054 追记说明（它们是 CellMap 档的视觉契约）。

use bevy::prelude::*;

/// `0xRRGGBB` → Bevy `Color`（sRGB）。与 wv-theme `colors::hex` 同语义
/// （本 crate 零 theme 依赖，三行自持）。
pub const fn hex(rgb: u32) -> Color {
    let [r, g, b] = [(rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF];
    Color::srgb_u8(r as u8, g as u8, b as u8)
}

// ── 冻结常量（值源 = wv-theme colors.rs / colors_google.rs，2026-10-23 抄录） ──

/// 地形基色（日间 JRPG，ADR-0035）。索引即显示槽位 0..10。
pub const CORE_TERRAIN: [Color; 10] = [
    hex(0x2C6389), // 0 深水
    hex(0x57A0C0), // 1 浅水
    hex(0xE8D6A2), // 2 沙
    hex(0x8CB95F), // 3 草原
    hex(0x3F7A46), // 4 森林
    hex(0xB5A765), // 5 丘陵
    hex(0x9E9070), // 6 山地
    hex(0xF5F8F9), // 7 雪
    hex(0x6F8564), // 8 沼泽
    hex(0xC8A272), // 9 道路
];

/// Google 底图十色（`colors_google::GOOGLE_CORE_TERRAIN` 抄录）。
pub const GOOGLE_CORE_TERRAIN: [Color; 10] = [
    hex(0x9FC7EF), // 0 深水
    hex(0xAADAFF), // 1 浅水
    hex(0xF8F9FA), // 2 沙（基础陆地）
    hex(0xF3F2EE), // 3 草原
    hex(0xC9E7C7), // 4 森林
    hex(0xEFEDE7), // 5 丘陵
    hex(0xE8E5DE), // 6 山地
    hex(0xFBFBFA), // 7 雪
    hex(0xBBDCC9), // 8 沼泽
    hex(0xFFFFFF), // 9 道路
];

/// Google 底图地形基色（越界回退沼泽色，同源语义）。
pub fn google_core_terrain(kind: u8) -> Color {
    GOOGLE_CORE_TERRAIN
        .get(kind as usize)
        .copied()
        .unwrap_or(GOOGLE_CORE_TERRAIN[8])
}

/// 日间 JRPG 地形基色（越界回退沼泽色；= wv-theme `colors::core_terrain` 镜像）。
pub const fn core_terrain(kind: u8) -> Color {
    if kind < 10 {
        CORE_TERRAIN[kind as usize]
    } else {
        CORE_TERRAIN[8]
    }
}

/// 轮廓图：水面（近黑）。
pub const COAST_WATER: Color = hex(0x07090C);
/// 轮廓图：陆地（深灰）。
pub const COAST_LAND: Color = hex(0x1A1E24);
/// 轮廓图：海岸线（浅水色族）。
pub const COAST_LINE: Color = hex(0x4A90E2);
/// 雪冷阴影起点海拔（m）。
pub const SNOW_SHADOW_MIN_HEIGHT: i16 = 2500;
/// 雪冷阴影每公里叠加率。
pub const SNOW_SHADOW_PER_KM: f32 = 0.35;
/// 海冰纬度阈值 t（`≥` 视为极地）。
pub const ICE_LATITUDE_T: f32 = 0.20;
/// 海冰最大水深（m，绝对值）。
pub const ICE_MAX_DEPTH_M: i16 = 200;
/// Topographic viewmode 百分位阈值下限（wv-theme `topographic` 镜像）。
pub const TOPOGRAPHIC_P_LOW: f32 = 0.33;
/// Topographic viewmode 百分位阈值上限。
pub const TOPOGRAPHIC_P_HIGH: f32 = 0.66;

// ── 镜像：PaletteColors ──

/// 色板镜像（wv-theme `PaletteColors` 的平面拷贝；字段名 1:1）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellPalette {
    /// 地形基色（索引即 `TerrainKind` 0..10）。
    pub core: [Color; 10],
    /// 底图淡化保留比例。
    pub soften_keep: f32,
    /// 底图淡化目标色（= 纸面色）。
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
    /// 标注字重梯度：region（最深墨）。
    pub label_region: Color,
    /// 标注字重梯度：place。
    pub label_place: Color,
    /// 标注字重梯度：water。
    pub label_water: Color,
    /// POI 圆点色族。
    pub dot_place: Color,
    pub dot_region: Color,
    pub dot_faction: Color,
    pub dot_event: Color,
    /// POI hover 文字色族。
    pub text_place: Color,
    pub text_region: Color,
    pub text_faction: Color,
    pub text_event: Color,
    /// 雪高光色。
    pub snow_highlight: Color,
    /// 雪冷阴影色。
    pub snow_shadow: Color,
    /// 海冰纸面。
    pub ice_paper: Color,
    /// 冰缘墨线。
    pub ice_ink: Color,
    /// 海冰背光排线色。
    pub ice_shadow: Color,
    /// 文字晕边填充。
    pub halo_fill: Color,
    /// 等值线 / 海岸线墨色。
    pub isoline_ink: Color,
    /// 编辑网格线基色。
    pub grid_line: Color,
    /// 建筑顶面。
    pub building_top: Color,
    /// 建筑侧面 / 投影基色。
    pub building_side: Color,
    /// 事件图极简底图。
    pub event_backdrop: Color,
    /// 描线三档。
    pub line_on_light: Color,
    pub line_on_mid: Color,
    pub line_on_dark: Color,
    /// 「明确深蓝」：最深水位直接色（2026-10-16 用户裁定）。
    pub deep_ocean_blue: Color,
    /// 近岸浅滩色（2026-10-21 S2）。
    pub water_shore: Color,
}

impl CellPalette {
    /// 地形基色取值（`kind` 越界回退沼泽色，同 wv-theme 源型语义）。
    pub fn core_terrain(&self, kind: u8) -> Color {
        self.core
            .get(kind as usize)
            .copied()
            .unwrap_or(self.core[8])
    }

    /// 底图淡化混合（公式同源不二写：`out = base*k + toward*(1-k)`）。
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
}

// ── 镜像：TreatmentFlags ──

/// 手绘旗标镜像（wv-theme `TreatmentFlags`，字段名 1:1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreatmentFlags {
    pub coast_halo: bool,
    pub water_gradient: bool,
    pub snow_shade: bool,
    pub sea_ice: bool,
    pub paper_grain: bool,
    pub river_centerline: bool,
    pub boundary_wobble: bool,
    pub ink_sublinear: bool,
    pub paper_fog: bool,
    pub land_grain: bool,
    pub watercolor_bleed: bool,
    pub shore_shallow: bool,
}

impl TreatmentFlags {
    /// 手绘全开（冻结 = `HAND_DRAWN`）。
    pub const HAND_DRAWN: Self = Self {
        coast_halo: true,
        water_gradient: true,
        snow_shade: true,
        sea_ice: true,
        paper_grain: true,
        river_centerline: true,
        boundary_wobble: true,
        ink_sublinear: true,
        paper_fog: true,
        land_grain: true,
        watercolor_bleed: true,
        shore_shallow: true,
    };

    /// 全 `false`（「干净矢量」对照组）。
    pub const EMPTY: Self = Self {
        coast_halo: false,
        water_gradient: false,
        snow_shade: false,
        sea_ice: false,
        paper_grain: false,
        river_centerline: false,
        boundary_wobble: false,
        ink_sublinear: false,
        paper_fog: false,
        land_grain: false,
        watercolor_bleed: false,
        shore_shallow: false,
    };
}

// ── 镜像：SeaRecipe ──

/// 波纹 / 碎浪圈规格镜像。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RippleSpec {
    /// 该圈到岸的距离（格）。
    pub dist_cells: f32,
    /// 高斯带宽（格）。
    pub sigma_cells: f32,
    /// 墨量（0..=1）。
    pub alpha: f32,
}

/// 近岸排线规格镜像。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatchSpec {
    /// 排线径向长度（格）。
    pub dist_cells: f32,
    /// 行距（纹理像素）。
    pub row_spacing_px: u32,
    /// 墨量（0..=1）。
    pub alpha: f32,
}

/// 海面配方镜像（wv-theme `SeaRecipe`，字段名 1:1）。
#[derive(Debug, Clone, PartialEq)]
pub struct SeaRecipe {
    /// 深度量程（米）。
    pub depth_scale_m: f32,
    /// 深度曲线指数。
    pub depth_exponent: f32,
    /// 近岸浅滩带宽度（格）。
    pub shore_band_cells: f32,
    /// 浅滩带着色峰值（0..=1）。
    pub shore_peak: f32,
    /// 水面纸纹颗粒振幅（0..=1）。
    pub water_grain: f32,
    /// 波纹离岸抖动幅度（格）。
    pub ripple_warp_cells: f32,
    /// 离岸墨线波纹。空 = 不画。
    ///
    /// 持 `Vec` 而非 `&'static`：wv-render 侧的 `ThemePack→CellPack` 换算
    /// 要能把**任意**主题包（含外部 crate 定义的非 const 配方）搬进来，
    /// 运行时无法凭空产出 `&'static` 切片（ADR-0054 D4）。
    pub ripples: Vec<RippleSpec>,
    /// 波纹墨色。
    pub ink: [u8; 3],
    /// 近岸碎浪。空 = 不画。
    pub foam: Vec<RippleSpec>,
    /// 碎浪墨色。
    pub foam_ink: [u8; 3],
    /// 碎浪破碎度。
    pub foam_break: f32,
    /// 碎浪抖动幅度（格）。
    pub foam_warp_cells: f32,
    /// 近岸排线。
    pub shore_hatch: Option<HatchSpec>,
    /// 浅滩外缘积墨强度。
    pub shore_edge_strength: f32,
    /// 浅滩外缘积墨宽度（格）。
    pub shore_edge_width_cells: f32,
    /// 浅滩外缘积墨色。
    pub shore_edge_ink: Option<[u8; 3]>,
    /// 碎浪破碎频率。
    pub foam_break_freq: f32,
}

impl SeaRecipe {
    /// 海拔 → 深度归一 t [0,1]（`depth^exponent / scale`，同 wv-theme 源型）。
    pub fn depth_t(&self, height: i16) -> f32 {
        let scale = self.depth_scale_m.max(1.0);
        let depth = (-height as f32).clamp(0.0, scale);
        (depth / scale).powf(self.depth_exponent)
    }

    /// 浅滩带着色强度（smoothstep 曲线，同 wv-theme 源型）。
    pub fn shore_band_strength(&self, dist_cells: u16) -> f32 {
        if self.shore_band_cells <= 0.0 || self.shore_peak <= 0.0 {
            return 0.0;
        }
        let t = (1.0 - dist_cells as f32 / self.shore_band_cells).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t) * self.shore_peak
    }

    /// 距离场需要的最大支撑半径（格；下限 12）。
    pub fn field_radius(&self) -> u16 {
        let mut r = self.shore_band_cells.max(0.0);
        for rp in &self.ripples {
            r = r.max(rp.dist_cells + rp.sigma_cells * 3.0);
        }
        (r.ceil() as u16 + self.ripple_warp_cells.ceil() as u16).max(12)
    }
}

/// 水彩海配方（日间，= wv-theme `WATERCOLOR` 逐值冻结）。
pub fn watercolor() -> SeaRecipe {
    SeaRecipe {
        depth_scale_m: 3500.0,
        depth_exponent: 0.5,
        shore_band_cells: 4.0,
        shore_peak: 0.55,
        water_grain: 0.035,
        ripple_warp_cells: 1.2,
        ripples: vec![
            RippleSpec {
                dist_cells: 4.0,
                sigma_cells: 0.55,
                alpha: 0.42,
            },
            RippleSpec {
                dist_cells: 9.0,
                sigma_cells: 0.55,
                alpha: 0.30,
            },
            RippleSpec {
                dist_cells: 15.0,
                sigma_cells: 0.60,
                alpha: 0.18,
            },
        ],
        ink: [0x5A, 0x4E, 0x42],
        foam: vec![
            RippleSpec {
                dist_cells: 1.0,
                sigma_cells: 0.45,
                alpha: 0.60,
            },
            RippleSpec {
                dist_cells: 2.6,
                sigma_cells: 0.50,
                alpha: 0.26,
            },
        ],
        foam_ink: [0xF2, 0xF6, 0xF2],
        foam_break: 0.78,
        foam_warp_cells: 1.4,
        foam_break_freq: 1.15,
        shore_hatch: Some(HatchSpec {
            dist_cells: 4.5,
            row_spacing_px: 16,
            alpha: 0.16,
        }),
        shore_edge_strength: 0.30,
        shore_edge_width_cells: 1.2,
        shore_edge_ink: None,
    }
}

// ── CellPack ──

/// 合成入参包：色板 + 旗标 + 海配方（= wv-render 侧 `ThemePack` 三字段的
/// 镜像捆绑；字段名与 `ThemePack` 对齐：`palette` / `treatment` / `sea`）。
#[derive(Debug, Clone, PartialEq)]
pub struct CellPack {
    /// 色板。
    pub palette: CellPalette,
    /// 手绘旗标。
    pub treatment: TreatmentFlags,
    /// 海面配方。
    pub sea: SeaRecipe,
}

impl CellPack {
    /// 冻结缺省包：Daylight 色板 + 手绘全开 + 水彩海。
    ///
    /// CellMap 档的**默认数据源**（ADR-0054 D4「固定体现在默认数据源，
    /// 不烧 shader 常量」）；也是本 crate 测试的对照基准。
    pub fn fixture() -> CellPack {
        CellPack {
            palette: CellPalette {
                core: CORE_TERRAIN,
                soften_keep: 0.92,
                soften_toward: hex(0xF2EAD8),
                poi_dot: hex(0xB88E6C),
                poi_ring: hex(0xF2EAD8),
                road_stroke: hex(0xB08968),
                road_casing: hex(0xE8DCC0),
                street_major: hex(0xB08968),
                street_minor: hex(0xC8A272),
                label_fill: hex(0x4A3F35),
                label_region: hex(0x3A3028),
                label_place: hex(0x5A4E42),
                label_water: hex(0x3F5B6A),
                dot_place: hex(0xB88E6C),
                dot_region: hex(0x8A7A66),
                dot_faction: hex(0x9E2B25),
                dot_event: hex(0x4A90E2),
                text_place: hex(0x4A3F35),
                text_region: hex(0x3A3028),
                text_faction: hex(0x7A1F1A),
                text_event: hex(0x2C6389),
                snow_highlight: hex(0xFCFEFF),
                snow_shadow: hex(0xAEBBC3),
                ice_paper: hex(0xFDFEFF),
                ice_ink: hex(0x9FB8C8),
                ice_shadow: hex(0xC5D6E0),
                halo_fill: hex(0xF2EAD8),
                isoline_ink: hex(0x6B5B49),
                grid_line: hex(0x4A3F35),
                building_top: hex(0xD9C7A0),
                building_side: hex(0xB08968),
                event_backdrop: hex(0xEFE9DA),
                line_on_light: hex(0x6B5B49),
                line_on_mid: hex(0x4A3F35),
                line_on_dark: hex(0xE8DCC0),
                deep_ocean_blue: hex(0x0C32C8),
                water_shore: hex(0xA9D3D5),
            },
            treatment: TreatmentFlags::HAND_DRAWN,
            sea: watercolor(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_freezes_expected_anchor_colors() {
        // 固定色的视觉契约锚点：改值必须过 ADR-0054 追记。
        assert_eq!(CellPack::fixture().palette.deep_ocean_blue, hex(0x0C32C8));
        assert_eq!(CellPack::fixture().palette.water_shore, hex(0xA9D3D5));
        assert_eq!(CellPack::fixture().palette.core[3], hex(0x8CB95F));
        assert_eq!(CellPack::fixture().sea.shore_band_cells, 4.0);
        assert!(CellPack::fixture().treatment.boundary_wobble);
    }

    #[test]
    fn core_terrain_falls_back_to_marsh_on_out_of_range_kind() {
        let pal = CellPack::fixture().palette;
        assert_eq!(pal.core_terrain(255), pal.core[8]);
        assert_eq!(pal.core_terrain(3), pal.core[3]);
    }

    #[test]
    fn field_radius_covers_ripple_support_with_floor_of_twelve() {
        // 最远波纹 15 + 0.6×3 = 16.8 → ceil 17，加抖动 ceil(1.2)=2 → 19。
        assert_eq!(watercolor().field_radius(), 19);
        let empty = SeaRecipe {
            ripples: vec![],
            shore_band_cells: 0.0,
            ..watercolor()
        };
        assert_eq!(empty.field_radius(), 12);
    }

    #[test]
    fn shore_band_strength_peaks_at_shoreline_and_fades_to_zero() {
        assert_eq!(watercolor().shore_band_strength(0), watercolor().shore_peak);
        assert_eq!(watercolor().shore_band_strength(4), 0.0);
        assert!(watercolor().shore_band_strength(2) > 0.0);
    }
}
