//! ThemePack → CellPack 换算（`conv_*` 唯一通道 + `shade_mode_of` 唯一映射点）
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;


/// `ViewMode → ShadeMode` 唯一映射点。
pub fn shade_mode_of(mode: crate::state::ViewMode) -> ShadeMode {
    match mode {
        crate::state::ViewMode::VectorFlat => ShadeMode::VectorFlat,
        crate::state::ViewMode::GoogleMap => ShadeMode::GoogleMap,
        crate::state::ViewMode::RealTerrain => ShadeMode::RealTerrain,
        crate::state::ViewMode::Faction => ShadeMode::Faction,
        crate::state::ViewMode::Event => ShadeMode::Event,
        crate::state::ViewMode::Coastline => ShadeMode::Coastline,
        crate::state::ViewMode::Topographic => ShadeMode::Topographic,
    }
}

/// 色板换算（字段 1:1）。
/// 地形底图色板：`GoogleMap` 模式下把 `core` 换成 `GOOGLE_CORE_TERRAIN`。
///
/// **为什么必须在这里换**：`colors_google::GOOGLE_CORE_TERRAIN` 自 2026-10-13
/// 定义以来**没有任何消费点**（`colors_google.rs` 注释提到的 `shade_google`
/// 从未存在过）——`ViewMode::GoogleMap` 的地形一直画的是**主题色板**（日间
/// 水彩褐阶），所以切过去也不像普通地图。本函数是那份色板的第一个、也是最
/// 唯一的消费入口。
///
/// 只换 `core` 与 `soften_toward`（近白纸面）：标注 / 道路 / POI 色在
/// `vector_layers` 已按模式分支取 `GOOGLE_LABEL_*` / `GOOGLE_ROAD_*`，不必
/// 在这里重复。
pub fn conv_palette_terrain(
    p: &wv_default_theme::palette::PaletteColors,
    mode: crate::state::ViewMode,
) -> CellPalette {
    let mut c = conv_palette(p);
    if mode == crate::state::ViewMode::GoogleMap {
        c.core = wv_default_theme::colors_google::GOOGLE_CORE_TERRAIN;
        c.soften_toward = wv_default_theme::colors_google::GOOGLE_SOFTEN_TOWARD;
    }
    c
}

pub fn conv_palette(p: &wv_default_theme::palette::PaletteColors) -> CellPalette {
    CellPalette {
        core: p.core,
        soften_keep: p.soften_keep,
        soften_toward: p.soften_toward,
        poi_dot: p.poi_dot,
        poi_ring: p.poi_ring,
        road_stroke: p.road_stroke,
        road_casing: p.road_casing,
        street_major: p.street_major,
        street_minor: p.street_minor,
        label_fill: p.label_fill,
        label_region: p.label_region,
        label_place: p.label_place,
        label_water: p.label_water,
        dot_place: p.dot_place,
        dot_region: p.dot_region,
        dot_faction: p.dot_faction,
        dot_event: p.dot_event,
        text_place: p.text_place,
        text_region: p.text_region,
        text_faction: p.text_faction,
        text_event: p.text_event,
        snow_highlight: p.snow_highlight,
        snow_shadow: p.snow_shadow,
        ice_paper: p.ice_paper,
        ice_ink: p.ice_ink,
        ice_shadow: p.ice_shadow,
        halo_fill: p.halo_fill,
        isoline_ink: p.isoline_ink,
        grid_line: p.grid_line,
        building_top: p.building_top,
        building_side: p.building_side,
        event_backdrop: p.event_backdrop,
        line_on_light: p.line_on_light,
        line_on_mid: p.line_on_mid,
        line_on_dark: p.line_on_dark,
        deep_ocean_blue: p.deep_ocean_blue,
        water_shore: p.water_shore,
        elevation_ramp: p.elevation_ramp,
        elevation_strength: p.elevation_strength,
    }
}

/// 旗标换算（字段 1:1）。
pub fn conv_treatment(t: &wv_default_theme::theme_pack::TreatmentFlags) -> TreatmentFlags {
    TreatmentFlags {
        coast_halo: t.coast_halo,
        water_gradient: t.water_gradient,
        snow_shade: t.snow_shade,
        sea_ice: t.sea_ice,
        paper_grain: t.paper_grain,
        river_centerline: t.river_centerline,
        boundary_wobble: t.boundary_wobble,
        ink_sublinear: t.ink_sublinear,
        paper_fog: t.paper_fog,
        land_grain: t.land_grain,
        watercolor_bleed: t.watercolor_bleed,
        shore_shallow: t.shore_shallow,
    }
}

/// 海配方换算（波纹 / 碎浪转 owned `Vec`；镜像字段 1:1）。
pub fn conv_sea(s: &wv_default_theme::sea::SeaRecipe) -> SeaRecipe {
    let conv = |specs: &'static [wv_default_theme::sea::RippleSpec]| {
        specs
            .iter()
            .map(|r| RippleSpec {
                dist_cells: r.dist_cells,
                sigma_cells: r.sigma_cells,
                alpha: r.alpha,
            })
            .collect()
    };
    SeaRecipe {
        depth_scale_m: s.depth_scale_m,
        depth_exponent: s.depth_exponent,
        shore_band_cells: s.shore_band_cells,
        shore_peak: s.shore_peak,
        water_grain: s.water_grain,
        ripple_warp_cells: s.ripple_warp_cells,
        ripples: conv(s.ripples),
        ink: s.ink,
        foam: conv(s.foam),
        foam_ink: s.foam_ink,
        foam_break: s.foam_break,
        foam_warp_cells: s.foam_warp_cells,
        shore_hatch: s.shore_hatch.map(|h| HatchSpec {
            dist_cells: h.dist_cells,
            row_spacing_px: h.row_spacing_px,
            alpha: h.alpha,
        }),
        shore_edge_strength: s.shore_edge_strength,
        shore_edge_width_cells: s.shore_edge_width_cells,
        shore_edge_ink: s.shore_edge_ink,
        foam_break_freq: s.foam_break_freq,
    }
}

/// 区域覆盖物规则集换算（键 `&'static str` 保主题字面量；规则 1:1）。
pub fn conv_region_set(
    set: &wv_default_theme::overlays::RegionOverlaySet,
) -> wv_world_cell_map::region_overlay::RegionOverlaySet {
    use wv_world_cell_map::region_overlay::{RegionOverlayRule, RegionOverlaySet};
    RegionOverlaySet {
        rules: set
            .keys()
            .filter_map(|k| {
                let r = set.rule_for(k)?;
                Some((
                    k,
                    RegionOverlayRule {
                        variant: r.variant,
                        color: r.color,
                        alpha: r.alpha,
                        size_scale: r.size_scale,
                        spacing_scale: r.spacing_scale,
                    },
                ))
            })
            .collect(),
    }
}

/// 整包换算（`ThemePack` 三字段 → `CellPack`）。
///
/// **分层设色强度出 0**（中性基准）。需要按会话开关着色时用
/// [`conv_pack_with`]——两函数分开是为了让测试 / 示例的逐位对照基准恒定。
pub fn conv_pack(pack: &wv_default_theme::theme_pack::ThemePack) -> CellPack {
    // 基准换算与模式无关：用 `VectorFlat`（= 主题色板原样），保持测试 / 示例的
    // 逐位对照恒定。Google 色只在真正渲染时经 `conv_pack_with(.., mode)` 生效。
    conv_pack_with(pack, 0.0, crate::state::ViewMode::VectorFlat)
}

/// 同 [`conv_pack`]，并带上**分层设色强度**（ADR-0055；`ViewState.hypsometric_tint`
/// 的落地入口）。强度是唯一的非主题字段，理由见 `CellPack::hypsometric` 的文档。
pub fn conv_pack_with(
    pack: &wv_default_theme::theme_pack::ThemePack,
    hypsometric: f32,
    mode: crate::state::ViewMode,
) -> CellPack {
    let cartographic = mode == crate::state::ViewMode::GoogleMap;
    CellPack {
        // GoogleMap 的地形色由 `conv_palette_terrain` 换成 GOOGLE_CORE_TERRAIN。
        palette: conv_palette_terrain(&pack.palette, mode),
        // 专业制图 = 干净平涂：海岸晕 / 水彩渐变 / 纸纹都是「手绘」语汇，
        // 与「普通地图」的一眼看懂相冲突。EMPTY 而非主题旗标。
        treatment: if cartographic {
            conv_treatment(&wv_default_theme::theme_pack::TreatmentFlags::EMPTY)
        } else {
            conv_treatment(&pack.treatment)
        },
        // 海配方与 treatment 是**两套独立开关**：旗标管「晕 / 纸纹」，海配方管
        // 「水面上的笔触」（水纹颗粒 / 离岸波纹 / 碎浪 / 近岸排线 / 浅滩积墨）。
        // 只清 treatment 而透传海配方，专业制图的水面仍会带回水彩笔触——故两处
        // 必须一起中性化。深度渐变（`depth_scale_m` / `depth_exponent`）保留：
        // 真实海图同样按深浅分色，它是数据而非画法。
        sea: if cartographic {
            conv_sea_flat(&pack.sea)
        } else {
            conv_sea(&pack.sea)
        },
        // 分层设色是「地形分析」的表达（ADR-0055）：普通地图不按海拔分带
        // （Google 底图陆地单色），作者调过滑条也不该把专业制图染成梯队。
        hypsometric: if cartographic {
            0.0
        } else {
            hypsometric.clamp(0.0, 1.0)
        },
    }
}

/// 海配方的「专业制图」中性化：清空一切手绘笔触，只留深度渐变。
///
/// 与 [`conv_sea`] 分开而不是加 mode 参数：换算（主题 → 渲染）与风格截断
/// （渲不渲染笔触）是两件事，前者对所有模式同构，后者只在专业制图发生。
fn conv_sea_flat(s: &wv_default_theme::sea::SeaRecipe) -> SeaRecipe {
    let mut out = conv_sea(s);
    out.shore_band_cells = 0.0;
    out.shore_peak = 0.0;
    out.water_grain = 0.0;
    out.ripple_warp_cells = 0.0;
    out.ripples.clear();
    out.foam.clear();
    out.shore_hatch = None;
    out.shore_edge_strength = 0.0;
    out.shore_edge_width_cells = 0.0;
    out
}
