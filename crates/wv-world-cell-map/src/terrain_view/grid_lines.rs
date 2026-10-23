//! 逐格编辑网格（plan v0.1 D4）：笔刷开启或手动勾选时显示的格线叠加层。
//!
//! ## 职责
//! - 在**栅格幅面**（`tex.size × grid.cell_size`，`grid.raster_origin` 起）上
//!   画 `(size+1)` 竖 + `(size+1)` 横格线，单 mesh 批量（复用
//!   [`super::isoline_overlay::build_stroke_mesh`]）。
//! - 可见性 = `view.grid_lines` 单控（用户新目标，2026-10-15：网格独立于笔刷）。
//!   旧版 `brush.enabled || brush.grid_lines` 让"关笔刷 → 网格自动隐"——
//!   作者想"不开笔刷但留网格定位"做不到。2026-10-16 起字段也随开关 UI
//!   一起搬出笔刷（进 [`ViewState`]），笔刷与网格再无共享状态。
//! - **远观淡出**（2026-10-15 用户反馈）：格子屏幕尺寸 `< 4px` 起 alpha 线性
//!   衰减、`≤ 2px` 整层退场（[`grid_line_fade`]）——线宽是屏幕常量，缩得越远
//!   格子越密，~2px 格距时 1px 线条已盖住半幅画面，摩尔纹把低对比的笔迹整个
//!   淹没（作者看到的是「满屏布纹」，不是自己画的东西）。
//!
//! ## 边界（不做什么）
//! - **不画参考网格**——1/4 幅面参考网格是 `map_frame` 的（浏览态也在）；
//! - **不缓存跨缩放 mesh**——线宽是屏幕常量，`view.scale` 一变即重算
//!   （512² 图 4098 条线段毫秒级，且只在编辑态存在）。

use bevy::prelude::*;

/// 网格线本色的 alpha（作者未给 Terrain 图层设 opacity 时的值）。
/// 网格线基色走 `PaletteColors.grid_line`（ADR-0016：日间褐墨 / 暮色浅墨）。
pub const GRID_LINE_BASE_ALPHA: f32 = 0.35;

/// Z 序：底图块（0.0）之上、等值线描边（0.04）之下。
pub const GRID_Z: f32 = 0.03;

/// 网格线屏幕宽度（px）：世界宽度 = `px × view.scale`（ADR-0008 §8 的「乘」口径）。
pub const GRID_LINE_PX: f32 = 1.0;

/// 远观淡出的起点/终点（格子屏幕边长，px）：≥ 4px 全显，≤ 2px 消失。
///
/// 4px 是「还能数清格子」的下限经验值；2px 以下 1px 线条的覆盖率 ≥ 50%，
/// 画与不画只剩摩尔纹差别——与其铺一层噪声，不如整层退场让笔迹自己说话。
pub const GRID_FADE_START_PX: f32 = 4.0;
pub const GRID_FADE_END_PX: f32 = 2.0;

/// 远观淡出系数：格子屏幕边长 `cell_px` 在 `[END, START]` 间线性衰减，
/// 端点外钳制。纯函数（测试锚点：4px→1，3px→0.5，2px→0）。
pub fn grid_line_fade(cell_px: f32) -> f32 {
    ((cell_px - GRID_FADE_END_PX) / (GRID_FADE_START_PX - GRID_FADE_END_PX)).clamp(0.0, 1.0)
}

/// 资源：网格线实体 + 上次构建键。
#[derive(Resource, Default)]
pub struct GridLinesOverlay {
    pub entity: Option<Entity>,
    pub mesh: Option<Handle<Mesh>>,
    pub material: Option<Handle<ColorMaterial>>,
    pub key: Option<GridKey>,
}

/// 重算键：幅面 / 格径 / 原点 / 缩放任一变化即重算（f32 以位比较，精确）。
#[derive(Debug, Clone, PartialEq)]
pub struct GridKey {
    pub size: u32,
    pub height: u32,
    pub cell_bits: u64,
    pub origin: (u64, u64),
    pub palette: &'static str,
    pub scale_bits: u64,
}

impl GridLinesOverlay {
    pub fn despawn(&mut self, commands: &mut Commands) {
        if let (Some(entity), Some(mesh)) = (self.entity.take(), self.mesh.take()) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        self.key = None;
    }
}

/// 幅面 `size × size` 格的格线链：`(size+1)` 竖 + `(size+1)` 横，格坐标。
pub fn grid_chains(
    width: u32,
    height: u32,
    half_w: f32,
) -> Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> {
    use wv_terrain::isoline::Pt;
    let mut chains = Vec::with_capacity(2 * (width as usize + height as usize + 2));
    let pt = |x: f32, y: f32| Pt { x, y };
    let (w, h) = (width as f32, height as f32);
    for i in 0..=width {
        let f = i as f32;
        chains.push((vec![pt(f, 0.0), pt(f, h)], false, half_w));
    }
    for i in 0..=height {
        let f = i as f32;
        chains.push((vec![pt(0.0, f), pt(w, f)], false, half_w));
    }
    chains
}
