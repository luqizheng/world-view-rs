//! 编辑态逐格网格线（plan D4）：Terrain 图层可见性与透明度跟随
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

// ══════ from crates/wv-world-cell-map/src/terrain_view/grid_lines.rs :: sync_grid_lines ══════
/// 同步逐格网格叠加层（2026-10-23 重写：shader 双档网格 + 缩放零重建）。
///
/// ## 与原实现的三处差别（都是用户报的缺陷）
/// 1. **不再整层退场**：旧版 `grid_line_fade(cell_size / scale) <= 0` 即 despawn，
///    而世界级 fit（`scale 5.12`、`cell_size 4.0` → 格距 0.78px）恒为 0 →
///    「开了开关看不到」。现在格线由 fragment shader 按密度双档绘制，
///    CPU 侧只管「开关 + 图层 + 配方」三个 gate。
/// 2. **缩放不重建**：旧 `GridKey` 含 `scale_bits`，滚轮每档都整层重建
///    （2050 条链 + 全量网格上传）。新键只有 `size/height/cell_size/origin/
///    palette`，缩放**不碰 CPU**。重建计数见 `GridLinesOverlay::rebuilds`。
/// 3. **recipe 收口**：旧系统完全不读 `RenderRecipe`，表里 `Topographic` 行写
///    `false` 却照样画——表在骗人。现在 gate 读 `PassId::GridLines`，
///    且 Topographic 行已改为 `true`（CellMap 工作台要用网格定位，用户指令）。
///
/// 网格跟随 Terrain 图层的可见与透明度（ADR-0013）：作者把地形藏了，格线
/// 还浮着只会误导「这里可以画」。
pub fn sync_grid_lines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GridLinesMaterial>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    recipe: Res<RenderRecipe>,
    mut overlay: ResMut<GridLinesOverlay>,
) {
    if !layers.terrain.visible
        || !grid_lines_visible(&view)
        || !recipe.enabled(PassId::GridLines)
        || tex.size == 0
        || tex.height == 0
    {
        overlay.despawn(&mut commands);
        return;
    }
    let key = GridKey {
        size: tex.size,
        height: tex.height,
        cell_bits: grid.cell_size.to_bits() as u64,
        origin: (
            grid.raster_origin.x.to_bits() as u64,
            grid.raster_origin.y.to_bits() as u64,
        ),
        palette: view.active_theme.id,
    };
    let width = tex.size as f32 * grid.cell_size;
    let height = tex.height as f32 * grid.cell_size;
    let ink = conv_palette(&view.active_theme.palette).grid_line;
    // 墨色进 uniform 必须是**线性**值（sRGB 值直塞会被再亮一次，
    // `field/material.rs` 的既有血书），算一次复用。
    let ink_linear = ink.to_linear();
    let base_alpha = GRID_LINE_BASE_ALPHA * layers.terrain.opacity.clamp(0.0, 1.0);
    let z = GRID_Z + layers.terrain.z_offset;
    // 幅面中心（矩形 mesh 以中心为锚点；shader 用世界坐标反推格坐标，
    // 所以变换与 uniform 的 `geo.zw` 必须同为真实栅格原点）。
    let center = Vec3::new(
        grid.raster_origin.x + width * 0.5,
        grid.raster_origin.y + height * 0.5,
        z,
    );

    if overlay.key.as_ref() == Some(&key) && overlay.entity.is_some() {
        // 快路径：结构性输入没变。**缩放在这里跑完就返回**——键里没有 scale，
        // 一次 PartialEq 之后什么都不做（uniform 也不必改：线宽/格距由
        // shader 的 fwidth 从世界坐标导数推出，与 scale 无关）。
        overlay.refresh_ink(&mut materials, ink_linear, base_alpha);
        if layers.is_changed() {
            if let Some(entity) = overlay.entity {
                commands
                    .entity(entity)
                    .insert(Transform::from_translation(center));
            }
        }
        return;
    }

    // 结构性重建：幅面 / 格径 / 原点 / 主题变了才到这里（1024² 实测
    // debug 0.74ms / release 0.07ms，而缩放**不再**触发它）。
    let mesh = meshes.add(Rectangle::new(width, height));
    let material = materials.add(GridLinesMaterial {
        params: grid_params(grid.cell_size, grid.raster_origin, ink_linear, base_alpha),
    });
    overlay.despawn(&mut commands);
    let entity = commands
        .spawn((
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(center),
        ))
        .id();
    overlay.entity = Some(entity);
    overlay.mesh = Some(mesh);
    overlay.material = Some(material);
    overlay.ink_applied = Some([
        ink_linear.red,
        ink_linear.green,
        ink_linear.blue,
        base_alpha,
    ]);
    overlay.rebuilds += 1;
    overlay.key = Some(key);
}

// ══════ from crates/wv-world-cell-map/src/terrain_view/grid_lines.rs :: grid_lines_visible ══════
/// 网格可见性：单由 `view.grid_lines` 控制（2026-10-15 与笔刷解耦；
/// 2026-10-16 字段搬入 `ViewState`，开关 UI 上地图工具条）。
pub fn grid_lines_visible(view: &ViewState) -> bool {
    view.grid_lines
}
