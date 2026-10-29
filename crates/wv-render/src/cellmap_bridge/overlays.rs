//! 区域覆盖索引 + 缺失覆盖物标记（白底接管 / 群系缺规则提示）
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;


/// 求值区域覆盖物索引（每帧检查变更，变了才重算）。
///
/// **重算条件**：世界变更 / 钻取换图 / 栅格换图 / 主题切换。
/// 纯相机运动**零工作**（同路网缓存纪律）。
pub fn sync_region_overlay_index(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut index: ResMut<RegionOverlayIndex>,
) {
    let map_id = drill.current().cloned();
    let overlay_rules = conv_region_set(&view.active_theme.region_overlays);
    let (Some(map_id), Some((w, h))) = (map_id, terrain_dims(&projection.world, drill.current()))
    else {
        index.entries.clear();
        index.cell_owner.clear();
        index.missing_cells.clear();
        index.missing_shapes.clear();
        return;
    };
    let stale = projection.is_changed()
        || drill.is_changed()
        || grid.is_changed()
        || view.active_theme.id != index.theme_palette
        || index.map_id.as_deref() != Some(map_id.as_str());
    if !stale {
        return;
    }
    // 两套原点的差：区域多边形是**图内局部 wu**，地形格属于**栅格所在的图**
    // （子图上不是同一张图）——换算必须带这个偏移。
    let raster_to_map = grid.map_origin - grid.raster_origin;
    *index = build_index(
        &projection.world.regions,
        &map_id,
        grid.cell_size,
        w,
        h,
        &overlay_rules,
        raster_to_map,
    );
    index.map_id = Some(map_id.as_str().to_string());
    index.theme_palette = view.active_theme.id;
}

// ══════ from src/region_overlay.rs :: sync_missing_overlay_markers ══════
/// 缺失覆盖物标记系统：白底红斜线**接管**该区域。
///
/// - **三档 LOD 全画**（不可被降档隐藏：装饰在 Nation 档本就不画，缺失标记
///   若跟着降档，作者切到全局视图反而在最需要发现时看不到）；
/// - **固定诊断色**（不随主题色板）；
/// - 绘制在装饰（0.075）之上、道路（0.09）之下；
/// - 底层装饰符号在该区域**跳过**（白底接管，见 `RegionOverlayIndex::is_missing_cell`）。
#[allow(clippy::too_many_arguments)]
pub fn sync_missing_overlay_markers(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    index: Res<RegionOverlayIndex>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    recipe: Res<RenderRecipe>,
    layers: Res<LayerFrame>,
    mut assets: ResMut<MissingMarkerAssets>,
) {
    // 2026-10-22 内容矩阵：区域层从 TerrainDecor 拆到独立 pass（ADR-0051）。
    // CellMap（Topographic）：nature_overlay 放行 Nature 类（判定入口唯一）。
    let enabled = region_layer_visible(
        view.mode,
        view.nature_overlay,
        wv_core::model::region::RegionKind::Nature,
        recipe.enabled(crate::recipe::PassId::RegionLayers),
        layers.terrain.visible,
    );
    if !enabled || index.missing_shapes.is_empty() {
        for e in assets.entities.drain(..) {
            commands.entity(e).despawn();
        }
        assets.fingerprint = 0;
        return;
    }
    // 指纹：形状内容不变则不重建（相机平移零工作）。
    let fp = shapes_fingerprint(&index.missing_shapes);
    if fp == assets.fingerprint && !assets.entities.is_empty() {
        return;
    }
    for e in assets.entities.drain(..) {
        commands.entity(e).despawn();
    }
    assets.fingerprint = fp;

    // 斜纹贴图 + 材质（一次）。
    if assets.texture.is_none() {
        assets.texture = Some(hatch_texture(&mut images));
    }
    let texture = assets.texture.clone().expect("刚建");
    if assets.material.is_none() {
        // Bevy 0.19：ColorMaterial 无 `from_texture`，直接构造（贴图 + 全白染色）。
        let mat = ColorMaterial {
            color: Color::WHITE,
            alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
            uv_transform: bevy::math::Affine2::default(),
            texture: Some(texture.clone()),
        };
        assets.material = Some(materials.add(mat));
    }
    let material = assets.material.clone().expect("刚建");

    let map_origin = Vec3::new(grid.map_origin.x, grid.map_origin.y, 0.0);
    for shape in &index.missing_shapes {
        if shape.outer.len() < 6 {
            continue;
        }
        let hole_refs: Vec<&[f32]> = shape.holes.iter().map(|h| h.as_slice()).collect();
        let mesh = build_hatch_mesh(&shape.outer, &hole_refs, HATCH_PERIOD_WU);
        let entity = commands.spawn((
            Mesh2d(meshes.add(mesh)),
            MeshMaterial2d(material.clone()),
            Transform::from_translation(map_origin + Vec3::Z * MISSING_MARKER_Z),
        ));
        assets.entities.push(entity.id());
    }
}
