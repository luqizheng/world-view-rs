//! 逐格编辑网格（plan v0.1 D4）：笔刷开启或手动勾选时显示的格线叠加层。
//!
//! ## 职责
//! - 在**栅格幅面**（`tex.size × grid.cell_size`，`grid.raster_origin` 起）上
//!   画 `(size+1)` 竖 + `(size+1)` 横格线，单 mesh 批量（复用
//!   [`super::isoline_overlay::build_stroke_mesh`]）。
//! - 可见性 = `brush.grid_lines` 单控（用户新目标，2026-10-15：网格独立于笔刷）。
//!   旧版 `brush.enabled || brush.grid_lines` 让"关笔刷 → 网格自动隐"——
//!   作者想"不开笔刷但留网格定位"做不到。改后笔刷只管绘画，网格只管
//!   自己的 `grid_lines` 开关。
//! - **远观淡出**（2026-10-15 用户反馈）：格子屏幕尺寸 `< 4px` 起 alpha 线性
//!   衰减、`≤ 2px` 整层退场（[`grid_line_fade`]）——线宽是屏幕常量，缩得越远
//!   格子越密，~2px 格距时 1px 线条已盖住半幅画面，摩尔纹把低对比的笔迹整个
//!   淹没（作者看到的是「满屏布纹」，不是自己画的东西）。
//!
//! ## 边界（不做什么）
//! - **不画参考网格**——1/4 幅面参考网格是 `map_frame` 的（浏览态也在）；
//! - **不缓存跨缩放 mesh**——线宽是屏幕常量，`view.scale` 一变即重算
//!   （512² 图 4098 条线段毫秒级，且只在编辑态存在）。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::render::layer_frame::LayerFrame;
use crate::state::{BrushState, GridConfig, ViewState};
use crate::theme::palette::{self, MapPalette};

use super::isoline_overlay::build_stroke_mesh;
use super::terrain_texture::TerrainTexture;

/// 网格线本色的 alpha（作者未给 Terrain 图层设 opacity 时的值）。
/// 网格线基色走 `PaletteColors.grid_line`（ADR-0016：日间褐墨 / 暮色浅墨）。
const GRID_LINE_BASE_ALPHA: f32 = 0.35;

/// Z 序：底图块（0.0）之上、等值线描边（0.04）之下。
const GRID_Z: f32 = 0.03;

/// 网格线屏幕宽度（px）：世界宽度 = `px × view.scale`（ADR-0008 §8 的「乘」口径）。
const GRID_LINE_PX: f32 = 1.0;

/// 远观淡出的起点/终点（格子屏幕边长，px）：≥ 4px 全显，≤ 2px 消失。
///
/// 4px 是「还能数清格子」的下限经验值；2px 以下 1px 线条的覆盖率 ≥ 50%，
/// 画与不画只剩摩尔纹差别——与其铺一层噪声，不如整层退场让笔迹自己说话。
const GRID_FADE_START_PX: f32 = 4.0;
const GRID_FADE_END_PX: f32 = 2.0;

/// 网格可见性：单由 `brush.grid_lines` 控制（用户新目标，2026-10-15）。
/// 旧版与 `brush.enabled` 相或——关笔刷自动隐网格，作者无法"不开笔刷
/// 但留网格定位"。改后两态独立。
pub fn grid_lines_visible(brush: &BrushState) -> bool {
    brush.grid_lines
}

/// 远观淡出系数：格子屏幕边长 `cell_px` 在 `[END, START]` 间线性衰减，
/// 端点外钳制。纯函数（测试锚点：4px→1，3px→0.5，2px→0）。
pub fn grid_line_fade(cell_px: f32) -> f32 {
    ((cell_px - GRID_FADE_END_PX) / (GRID_FADE_START_PX - GRID_FADE_END_PX)).clamp(0.0, 1.0)
}

/// 资源：网格线实体 + 上次构建键。
#[derive(Resource, Default)]
pub struct GridLinesOverlay {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<ColorMaterial>>,
    key: Option<GridKey>,
}

/// 重算键：幅面 / 格径 / 原点 / 缩放任一变化即重算（f32 以位比较，精确）。
#[derive(Debug, Clone, PartialEq)]
struct GridKey {
    size: u32,
    cell_bits: u64,
    origin: (u64, u64),
    palette: MapPalette,
    scale_bits: u64,
}

impl GridLinesOverlay {
    fn despawn(&mut self, commands: &mut Commands) {
        if let (Some(entity), Some(mesh)) = (self.entity.take(), self.mesh.take()) {
            commands.entity(entity).despawn();
            commands.queue(move |world: &mut World| {
                world.resource_mut::<Assets<Mesh>>().remove(&mesh);
            });
        }
        self.key = None;
    }
}

/// 同步逐格网格叠加层。
///
/// 网格是**编辑参照**，跟随 Terrain 图层的可见与透明度（ADR-0013）：
/// 作者把地形藏了，格线还浮着只会误导「这里可以画」。
#[allow(clippy::too_many_arguments)]
pub fn sync_grid_lines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    brush: Res<BrushState>,
    layers: Res<LayerFrame>,
    mut overlay: ResMut<GridLinesOverlay>,
) {
    if !layers.terrain.visible || !grid_lines_visible(&brush) {
        overlay.despawn(&mut commands);
        return;
    }
    // 远观淡出：格距缩到 2px 以下整层退场。fade 是 cell_size/scale 的纯函数，
    // 两者都在重算键里——fade 一变必然伴随键变，不存在「键相同而 fade 变了」。
    let fade = grid_line_fade(grid.cell_size / view.scale);
    if fade <= 0.0 {
        overlay.despawn(&mut commands);
        return;
    }
    let key = GridKey {
        size: tex.size,
        cell_bits: grid.cell_size.to_bits() as u64,
        origin: (
            grid.raster_origin.x.to_bits() as u64,
            grid.raster_origin.y.to_bits() as u64,
        ),
        palette: view.map_palette,
        scale_bits: view.scale.to_bits() as u64,
    };
    let grid_ink = palette::colors(view.map_palette).grid_line;
    if overlay.key.as_ref() == Some(&key) && overlay.entity.is_some() {
        // 结构没变也可能只是图层绑定变了（作者改 opacity/z）：就地补丁。
        if layers.is_changed() {
            if let Some(handle) = &overlay.material {
                if let Some(mut mat) = materials.get_mut(handle) {
                    mat.color =
                        grid_ink.with_alpha(GRID_LINE_BASE_ALPHA * layers.terrain.opacity * fade);
                }
            }
            if let Some(entity) = overlay.entity {
                commands.entity(entity).insert(Transform::from_xyz(
                    0.0,
                    0.0,
                    GRID_Z + layers.terrain.z_offset,
                ));
            }
        }
        return;
    }
    if tex.size == 0 {
        overlay.despawn(&mut commands);
        return;
    }
    // 线宽屏幕常量（乘 scale），不设世界下限——网格是编辑参照，远观淡去无妨。
    let half_w = GRID_LINE_PX * 0.5 * view.scale;
    let chains = grid_chains(tex.size, half_w);
    let Some(mesh) = build_stroke_mesh(&chains, grid.raster_origin, grid.cell_size) else {
        // 仅 `cell_size` 退化为 0 时可达：不插空网格（bevy #24874 误报
        // Use-after-free），退场即可——链装配是 O(size²) 便宜操作，下帧重算无妨。
        overlay.despawn(&mut commands);
        return;
    };
    let handle = meshes.add(mesh);
    let material = overlay
        .material
        .get_or_insert_with(|| materials.add(ColorMaterial::from_color(grid_ink)))
        .clone();
    // 材质跨重建复用（get_or_insert）：重算后也要按当前绑定刷新 alpha。
    if let Some(mut mat) = materials.get_mut(&material) {
        mat.color = grid_ink.with_alpha(GRID_LINE_BASE_ALPHA * layers.terrain.opacity * fade);
    }

    overlay.despawn(&mut commands);
    let entity = commands
        .spawn((
            Mesh2d(handle.clone()),
            MeshMaterial2d(material),
            Transform::from_xyz(0.0, 0.0, GRID_Z + layers.terrain.z_offset),
        ))
        .id();
    overlay.entity = Some(entity);
    overlay.mesh = Some(handle);
    overlay.key = Some(key);
}

/// 幅面 `size × size` 格的格线链：`(size+1)` 竖 + `(size+1)` 横，格坐标。
fn grid_chains(size: u32, half_w: f32) -> Vec<(Vec<wv_terrain::isoline::Pt>, bool, f32)> {
    use wv_terrain::isoline::Pt;
    let n = size;
    let mut chains = Vec::with_capacity(2 * (n as usize + 1));
    let pt = |x: f32, y: f32| Pt { x, y };
    for i in 0..=n {
        let f = i as f32;
        chains.push((vec![pt(f, 0.0), pt(f, n as f32)], false, half_w));
        chains.push((vec![pt(0.0, f), pt(n as f32, f)], false, half_w));
    }
    chains
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::mesh::MeshVertexAttribute;

    fn brush(enabled: bool, grid_lines: bool) -> BrushState {
        BrushState {
            enabled,
            grid_lines,
            ..BrushState::default()
        }
    }

    /// 独立开关语义（2026-10-15 用户新目标）：只由 `grid_lines` 控制。
    /// 笔刷开/关不影响网格可见性——作者可"不开笔刷但留网格定位"。
    #[test]
    fn grid_lines_visible_depends_only_on_grid_lines() {
        assert!(!grid_lines_visible(&brush(false, false)));
        assert!(
            !grid_lines_visible(&brush(true, false)),
            "笔刷开不再强制显网格"
        );
        assert!(
            grid_lines_visible(&brush(false, true)),
            "笔刷关 + grid 开 → 显"
        );
        assert!(grid_lines_visible(&brush(true, true)));
    }

    /// 用户场景：关笔刷 + 开网格（之前的"做不到"组合）现在可行。
    #[test]
    fn grid_alone_without_brush_is_visible() {
        // 关笔刷 + 开网格 = 唯一的"网格单独"路径。
        let only_grid = brush(false, true);
        assert!(grid_lines_visible(&only_grid));
        // 反向：开笔刷 + 关网格 = 笔刷绘制时无网格（作者选择"裸画"）。
        let only_brush = brush(true, false);
        assert!(!grid_lines_visible(&only_brush));
    }

    /// 远观淡出锚点：≥4px 全显、≤2px 消失、中点线性一半、端点外钳制。
    #[test]
    fn grid_line_fade_is_linear_between_two_and_four_pixel_cells() {
        assert_eq!(grid_line_fade(8.0), 1.0, "宽格全显");
        assert_eq!(grid_line_fade(GRID_FADE_START_PX), 1.0, "起点全显");
        let mid = grid_line_fade(3.0);
        assert!((mid - 0.5).abs() < 1e-6, "中点应线性一半：got {mid}");
        assert_eq!(grid_line_fade(GRID_FADE_END_PX), 0.0, "终点消失");
        assert_eq!(grid_line_fade(1.0), 0.0, "更密仍钳在 0");
    }

    /// 2×2 幅面 → 3 竖 + 3 横 = 6 条链，每条 2 点 1 段：24 顶点 36 索引。
    #[test]
    fn grid_chains_cover_full_raster_extent() {
        let chains = grid_chains(2, 0.5);
        assert_eq!(chains.len(), 6, "3 竖 + 3 横");
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0).expect("规则格链应有 mesh");
        let idx = mesh.indices().expect("索引应存在").len();
        assert_eq!(idx, 6 * 6, "每段一个四边形 = 6 索引");
    }

    /// 网格线落在格缘整数位：2×2 幅面、格径 10 → 最外缘 x = 0 与 x = 20。
    #[test]
    fn grid_chains_span_raster_bounds_exactly() {
        let chains = grid_chains(2, 0.0);
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0).expect("规则格链应有 mesh");
        let pos = mesh
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                bevy::render::render_resource::VertexFormat::Float32x3,
            ))
            .expect("位置属性应存在")
            .as_float3()
            .expect("应为 Float32x3")
            .to_vec();
        // half_w = 0：顶点即格缘本身；端点外扩 0 不越界。
        let xs: Vec<f32> = pos.iter().map(|v| v[0]).collect();
        let ys: Vec<f32> = pos.iter().map(|v| v[1]).collect();
        assert!(
            xs.iter().all(|x| (0.0..=20.0).contains(x)),
            "xs 越界 {xs:?}"
        );
        assert!(
            ys.iter().all(|y| (0.0..=20.0).contains(y)),
            "ys 越界 {ys:?}"
        );
    }

    /// 资源默认值：未构建过。
    #[test]
    fn grid_lines_overlay_default_is_empty() {
        let o = GridLinesOverlay::default();
        assert!(o.entity.is_none() && o.key.is_none());
    }
}
