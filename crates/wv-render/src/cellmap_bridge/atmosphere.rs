//! 纸面大气氛围（雾 + 噪点）：与底图同幅面的「这张纸」属性
//!
//! 由 `cellmap_bridge.rs` 拆分（P1-1，2026-10-29）：原单文件 3600+ 行承载
//! 11 个职责；本模块是其中之一，经 `super::` 复用 mod.rs 的文件级 use，
//! 公开符号经 mod.rs `pub use` 重导出——**外部路径零改动**。
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]

use super::*;

// ══════ from src/terrain_view/atmosphere.rs :: sync_atmosphere ══════
/// 同步氛围层。独立注册（主链已顶满 20 参元组上限），排在 `sync_map_frame` 之后。
///
/// 幅面 = **栅格幅面**（`raster_origin` + `size × cell_size`）——雾与噪点是
/// 「这张纸」的属性，与底图完全重合。
#[allow(clippy::too_many_arguments)]
pub fn sync_atmosphere(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    tex: Res<TerrainTexture>,
    layers: Res<LayerFrame>,
    brush: Res<BrushState>,
    recipe: Res<crate::recipe::RenderRecipe>,
    mut atm: ResMut<AtmosphereLayer>,
) {
    // GoogleMap / Coastline 由 recipe 关 atmosphere pass；这里只关 texture
    // 维度。LayerFrame.gate 与 view.mode 排除都在 recipe 里集中。
    if !layers.terrain.visible
        || !recipe.enabled(crate::recipe::PassId::Atmosphere)
        || tex.size == 0
    {
        atm.despawn(&mut commands);
        return;
    }
    // 装饰旗标（ADR-0023 D4）：从当前主题包派生；内置两包都是 HAND_DRAWN。
    let treatment = conv_treatment(&view.active_theme.treatment);
    let key = AtmosphereKey {
        origin: (
            grid.raster_origin.x.to_bits(),
            grid.raster_origin.y.to_bits(),
        ),
        extent: (tex.size, tex.height, grid.cell_size.to_bits()),
        scale_bucket: scale_bucket(view.scale),
        treatment: (treatment.paper_fog, treatment.paper_grain),
    };
    if atm.key.as_ref() != Some(&key) {
        atm.despawn(&mut commands);
        let size = Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size;
        // 手绘地图 #6：grain uv 随 `view.scale` 微调——近看 grain 翻倍密、远看
        // 减半；连续变化用 `scale_bucket` 量化到 1/8 octaves 档避免逐帧重建。
        let grain_tile = (GRAIN_TILE_WORLD / view.scale.max(0.05)).max(1.0);
        let uv = Vec2::splat(size.x / grain_tile);
        // —— 雾霭（初色 NONE，末尾逐帧更新段马上会写入当前值）——
        // 装饰旗标 gate：`paper_fog = false` 的主题不铺雾（`atm.veil` 留 None，
        // 下方逐帧 alpha 更新段与 despawn 都对 None 天然短路）。
        if treatment.paper_fog {
            let veil_mesh = meshes.add(extent_quad_mesh(size, Vec2::ZERO));
            let veil_mat = materials.add(ColorMaterial::from_color(Color::NONE));
            let veil = commands
                .spawn((
                    Mesh2d(veil_mesh.clone()),
                    MeshMaterial2d(veil_mat.clone()),
                    Transform::from_xyz(grid.raster_origin.x, grid.raster_origin.y, VEIL_Z),
                ))
                .id();
            atm.veil = Some((veil, veil_mesh, veil_mat));
        }
        // —— 纸张噪点 ——
        // 装饰旗标 gate：`paper_grain = false` 的主题不铺纸纹。
        if treatment.paper_grain {
            let grain_tex = atm
                .grain_texture
                .get_or_insert_with(|| images.add(paper_grain_image(0x5EED_5EED)))
                .clone();
            let grain_mesh = meshes.add(extent_quad_mesh(size, uv));
            let grain_mat = materials.add(ColorMaterial {
                texture: Some(grain_tex),
                color: Color::WHITE.with_alpha(GRAIN_ALPHA),
                ..Default::default()
            });
            let grain = commands
                .spawn((
                    Mesh2d(grain_mesh.clone()),
                    MeshMaterial2d(grain_mat.clone()),
                    Transform::from_xyz(grid.raster_origin.x, grid.raster_origin.y, GRAIN_Z),
                ))
                .id();
            atm.grain = Some((grain, grain_mesh, grain_mat));
        }
        atm.key = Some(key);
    }
    // 雾霭 alpha 逐帧连续更新（缩放的空气感 + 编辑态免雾）；颜色随主题纸面
    //（ADR-0016）。免雾只动 alpha 不拆实体：笔刷一关，下一帧雾即回来。
    //
    // WV_STRIP bit3（ADR-0088 Task 4）：掩码 bit3 置 1 = 剥掉 veil（alpha 恒 0）
    // ——veil **不在场 shader 里**，它是本桥的独立雾 quad，故探针的 bit3 在
    // 这里消费。探针只影响场管线 **Themed 分支**在场时的画面：分支判据与场
    // 桥接同一条 `field_branch_of`（不另写布尔式），其余分支/模式 veil 照旧。
    let strip_veil_alive = strip_veil_on(view.strip_mask)
        || field_branch_of(view.mode, layers.terrain.visible) != ViewBranch::Themed;
    let pal = &conv_palette(&view.active_theme.palette);
    if let Some((_, _, mat)) = &atm.veil {
        let veil_alpha = if strip_veil_alive {
            veil_alpha_for(view.scale, brush.enabled)
        } else {
            0.0
        };
        if let Some(mut m) = materials.get_mut(mat) {
            m.color = pal.soften_toward.with_alpha(veil_alpha);
        }
    }
}
