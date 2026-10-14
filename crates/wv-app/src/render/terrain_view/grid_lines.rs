//! 逐格编辑网格（plan v0.1 D4）：笔刷开启或手动勾选时显示的格线叠加层。
//!
//! ## 职责
//! - 在**栅格幅面**（`tex.size × grid.cell_size`，`grid.raster_origin` 起）上
//!   画 `(size+1)` 竖 + `(size+1)` 横格线，单 mesh 批量（复用
//!   [`super::isoline_overlay::build_stroke_mesh`]）。
//! - 可见性 = `brush.enabled || brush.grid_lines`（相或，见 [`grid_lines_visible`]）。
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

/// 网格可见性（D4）：笔刷开启**恒显**（编辑需要格子参照）；
/// 手动勾选是「不开笔刷也想看格子」的覆盖项。
pub fn grid_lines_visible(brush: &BrushState) -> bool {
    brush.enabled || brush.grid_lines
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
                    mat.color = grid_ink.with_alpha(GRID_LINE_BASE_ALPHA * layers.terrain.opacity);
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
    let mesh = build_stroke_mesh(&chains, grid.raster_origin, grid.cell_size);
    let handle = meshes.add(mesh);
    let material = overlay
        .material
        .get_or_insert_with(|| materials.add(ColorMaterial::from_color(grid_ink)))
        .clone();
    // 材质跨重建复用（get_or_insert）：重算后也要按当前绑定刷新 alpha。
    if let Some(mut mat) = materials.get_mut(&material) {
        mat.color = grid_ink.with_alpha(GRID_LINE_BASE_ALPHA * layers.terrain.opacity);
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

    /// D4 语义：笔刷开着恒显；关闭时可被手动勾选顶起；两态皆关则不显示。
    #[test]
    fn grid_lines_visible_follows_brush_or_override() {
        assert!(!grid_lines_visible(&brush(false, false)));
        assert!(grid_lines_visible(&brush(true, false)));
        assert!(grid_lines_visible(&brush(false, true)));
        assert!(grid_lines_visible(&brush(true, true)));
    }

    /// 2×2 幅面 → 3 竖 + 3 横 = 6 条链，每条 2 点 1 段：24 顶点 36 索引。
    #[test]
    fn grid_chains_cover_full_raster_extent() {
        let chains = grid_chains(2, 0.5);
        assert_eq!(chains.len(), 6, "3 竖 + 3 横");
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0);
        let idx = mesh.indices().expect("索引应存在").len();
        assert_eq!(idx, 6 * 6, "每段一个四边形 = 6 索引");
    }

    /// 网格线落在格缘整数位：2×2 幅面、格径 10 → 最外缘 x = 0 与 x = 20。
    #[test]
    fn grid_chains_span_raster_bounds_exactly() {
        let chains = grid_chains(2, 0.0);
        let mesh = build_stroke_mesh(&chains, Vec2::ZERO, 10.0);
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
