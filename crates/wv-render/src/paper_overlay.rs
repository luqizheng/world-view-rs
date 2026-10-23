//! 纸纹叠加（ADR-0042 手绘 Nation 表现层 · 第三块）。
//!
//! 参考图（手绘 JRPG）的第三层质感是**羊皮纸**：陆与海都蒙一层斑驳
//! 纸纹 + 四角晕影。本模块生成两张**程序化**贴图（零外部资产，D3 裁决），
//! 以两张全图幅面四边形叠在地形与装饰之上、河流/道路之下：
//!
//! | 层 | Z | 内容 |
//! |---|---|---|
//! | 地形 | 0.000 | chunk 底图 |
//! | 装饰 | 0.075+ | 山形/树团/波浪 |
//! | **纸纹斑驳** | **0.080** | 暖褐低频噪声，只压暗 ~13% |
//! | **四角晕影** | **0.0805** | 径向渐变，边缘 ~16% |
//! | 河流 | 0.082 | |
//!
//! 确定性：贴图像素由整数 hash 生成，同一构建内逐字节一致（P7 派生）。
//! 重建时机 = 当前图幅面变化（`GridConfig` 值指纹），钻取进城自动跟随。

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, Mesh, MeshVertexAttribute, PrimitiveTopology, VertexFormat};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// 纸纹斑驳层 Z（装饰 0.075 之上、河流 0.082 之下）。
pub const PAPER_MOTTLE_Z: f32 = 0.080;
/// 四角晕影层 Z（叠在斑驳上）。
pub const PAPER_VIGNETTE_Z: f32 = 0.0805;

/// 斑驳贴图边长（px）。低频噪声，256 足够；平铺周期 = 每世界 512wu 一次。
const MOTTLE_TEX: u32 = 256;
/// 晕影贴图边长（px）。
const VIGNETTE_TEX: u32 = 256;
/// 斑驳平铺周期（世界单位）：4096 幅面重复 8 次，块与块不重缝（贴图可平铺）。
const MOTTLE_TILE_WU: f32 = 512.0;

/// 斑驳最大 alpha：只「微微压暗」，叠多了变脏。
const MOTTLE_ALPHA_MAX: f32 = 0.13;
/// 晕影最大 alpha（四角）。
const VIGNETTE_ALPHA_MAX: f32 = 0.16;

/// 整数格点确定性 hash → `[0,1)`（与 `terrain_view::cells::hash01` 同族；
/// 独立副本避免跨模块私有依赖）。
fn hash01(x: i64, y: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h & 0xFFFF_FFFF) as f32 / u32::MAX as f32
}

/// **可平铺** value noise（格点环绕取模）：输出 [0,1]。
///
/// 平铺是硬要求——斑驳四边形用 repeat UV，噪声格点跨边不绕接会出现
/// 直线接缝（纸面上肉眼可见的「瓦片线」）。
fn tile_noise(px: i64, py: i64, cells: i64, period: i64, seed: u64) -> f32 {
    let wrap = |v: i64| v.rem_euclid(period);
    let (gx, gy) = (px * cells / period, py * cells / period);
    let (fx, fy) = (
        px as f32 * cells as f32 / period as f32 - gx as f32,
        py as f32 * cells as f32 / period as f32 - gy as f32,
    );
    let at = |dx: i64, dy: i64| hash01(wrap(gx + dx), wrap(gy + dy), seed);
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = at(0, 0);
    let b = at(1, 0);
    let c = at(0, 1);
    let d = at(1, 1);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// 生成斑驳贴图：暖褐低频噪声 + 高频细斑，alpha 只在「要压暗」处出现。
fn make_mottle_image() -> Image {
    let n = MOTTLE_TEX;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n as i64 {
        for x in 0..n as i64 {
            // 三阶可平铺噪声：大斑（32px 格）+ 中纹（8px）+ 细粒（2px）。
            let big = tile_noise(x, y, 8, n as i64, 0xA1);
            let mid = tile_noise(x, y, 32, n as i64, 0xB2);
            let fine = tile_noise(x, y, 128, n as i64, 0xC3);
            let v = big * 0.55 + mid * 0.30 + fine * 0.15;
            // 只取「暗斑」半边：v 低于中线的部分按深度压暗，亮处全透。
            let dark = ((0.52 - v) / 0.52).clamp(0.0, 1.0);
            let a = (dark * dark * MOTTLE_ALPHA_MAX).clamp(0.0, 1.0);
            // 暖褐墨色（羊皮纸的陈旧感是偏红褐，不是灰黑）。
            data.extend_from_slice(&[92, 74, 55, (a * 255.0) as u8]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: n,
            height: n,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// 生成晕影贴图：中心全透，四角逐级压暗（径向 smoothstep）。
fn make_vignette_image() -> Image {
    let n = VIGNETTE_TEX as f32;
    let mut data = Vec::with_capacity((VIGNETTE_TEX * VIGNETTE_TEX * 4) as usize);
    for y in 0..VIGNETTE_TEX as i64 {
        for x in 0..VIGNETTE_TEX as i64 {
            let dx = (x as f32 + 0.5) / n * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / n * 2.0 - 1.0;
            // 椭圆径向距离：角最深（t≈1）、边中点温和（t≈0.71）、中心全透。
            let t = (dx * dx + dy * dy).sqrt() / std::f32::consts::SQRT_2;
            // 0.55 之前全透，0.55..1.05 smoothstep 起晕。
            let k = ((t - 0.55) / 0.50).clamp(0.0, 1.0);
            let a = (k * k * (3.0 - 2.0 * k)) * VIGNETTE_ALPHA_MAX;
            data.extend_from_slice(&[70, 55, 40, (a * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: VIGNETTE_TEX,
            height: VIGNETTE_TEX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// 全幅面四边形 mesh：绝对世界坐标 + 重复 UV（`tiles` = 平铺次数）。
fn quad_mesh(origin: Vec2, size: Vec2, tiles: Vec2) -> Mesh {
    let (x0, y0) = (origin.x, origin.y);
    let (x1, y1) = (origin.x + size.x, origin.y + size.y);
    let (u0, v0) = (0.0f32, 0.0f32);
    let (u1, v1) = (tiles.x, tiles.y);
    // 四角逆时针（Bevy 2D 正面朝 +Z 观察者）。
    let verts: Vec<[f32; 3]> = vec![[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]];
    let uvs: Vec<[f32; 2]> = vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let indices: Vec<u32> = vec![0, 1, 2, 0, 2, 3];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    mesh.insert_attribute(
        MeshVertexAttribute::new("Vertex_Position", 0, VertexFormat::Float32x3),
        verts,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// 纸纹叠层运行时状态：贴图句柄（懒生成一次）+ 当前台几何实体。
#[derive(Resource, Default)]
pub struct PaperOverlay {
    textures: Option<(Handle<Image>, Handle<Image>)>,
    mottle: Option<Entity>,
    vignette: Option<Entity>,
    /// 当前台指纹：幅面宽高（原点/尺寸变了 = 换图了）。
    key: Option<[u32; 2]>,
}

/// 系统参数束（参数数量超 Bevy 16 上限的拆法）。
#[derive(SystemParam)]
pub struct PaperOverlayParams<'w, 's> {
    commands: Commands<'w, 's>,
    grid: Res<'w, crate::state::GridConfig>,
    view: Res<'w, crate::state::ViewState>,
    state: ResMut<'w, PaperOverlay>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<ColorMaterial>>,
}

/// 按当前图幅面同步两张全幅纸纹四边形。
///
/// `GridConfig` 值指纹（幅面宽高取整）没变 → 零工作；变了（换图 / 钻取）
/// → 旧实体下线、按新幅面重建。贴图只生成一次，跨图复用。
pub fn sync_paper_overlay(mut p: PaperOverlayParams) {
    // 2026-10-22 验收（用户裁定：CellMap 零 theme 影响）：纸纹是主题的
    // 艺术修饰（斑驳 + 晕影），Topographic 下整体下线。
    if matches!(p.view.mode, crate::state::ViewMode::Topographic) {
        if let Some(e) = p.state.mottle.take() {
            p.commands.entity(e).despawn();
        }
        if let Some(e) = p.state.vignette.take() {
            p.commands.entity(e).despawn();
        }
        p.state.key = None;
        return;
    }
    if !p.grid.is_changed() && p.state.key.is_some() {
        return;
    }
    let key = [
        (p.grid.map_size.x * 100.0) as u32,
        (p.grid.map_size.y * 100.0) as u32,
    ];
    if p.state.key == Some(key) && p.state.mottle.is_some() {
        return;
    }
    // 幅面退化（空编辑器首帧）先不画。
    if p.grid.map_size.x <= 0.0 || p.grid.map_size.y <= 0.0 {
        return;
    }
    let (mottle_tex, vignette_tex) = match &p.state.textures {
        Some(t) => t.clone(),
        None => {
            let m = p.images.add(make_mottle_image());
            let v = p.images.add(make_vignette_image());
            p.state.textures = Some((m.clone(), v.clone()));
            (m, v)
        }
    };
    for e in [p.state.mottle.take(), p.state.vignette.take()]
        .into_iter()
        .flatten()
    {
        p.commands.entity(e).despawn();
    }
    let size = p.grid.map_size;
    let origin = p.grid.map_origin;
    let tiles = Vec2::new(size.x / MOTTLE_TILE_WU, size.y / MOTTLE_TILE_WU);

    let mottle_mesh = p.meshes.add(quad_mesh(origin, size, tiles));
    let mottle_mat = p.materials.add(ColorMaterial {
        color: Color::WHITE,
        texture: Some(mottle_tex),
        alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
        ..default()
    });
    p.state.mottle = Some(
        p.commands
            .spawn((
                Mesh2d(mottle_mesh),
                MeshMaterial2d(mottle_mat),
                Transform::from_xyz(0.0, 0.0, PAPER_MOTTLE_Z),
                Name::new("paper_mottle"),
            ))
            .id(),
    );

    let vig_mesh = p.meshes.add(quad_mesh(origin, size, Vec2::ONE));
    let vig_mat = p.materials.add(ColorMaterial {
        color: Color::WHITE,
        texture: Some(vignette_tex),
        alpha_mode: bevy::sprite_render::AlphaMode2d::Blend,
        ..default()
    });
    p.state.vignette = Some(
        p.commands
            .spawn((
                Mesh2d(vig_mesh),
                MeshMaterial2d(vig_mat),
                Transform::from_xyz(0.0, 0.0, PAPER_VIGNETTE_Z),
                Name::new("paper_vignette"),
            ))
            .id(),
    );
    p.state.key = Some(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mottle_alpha_stays_subtle_and_tinted_warm() {
        let img = make_mottle_image();
        let d = img.data.as_ref().unwrap();
        let mut max_a = 0u8;
        let mut any_opaque = false;
        for px in d.chunks_exact(4) {
            max_a = max_a.max(px[3]);
            if px[3] > 200 {
                any_opaque = true;
            }
            // 暖褐：红 > 蓝（陈旧纸的偏红方向）。
            if px[3] > 0 {
                assert!(px[0] > px[2], "斑驳色应偏暖（红 > 蓝）");
            }
        }
        assert!(
            max_a as f32 / 255.0 <= MOTTLE_ALPHA_MAX + 0.005,
            "max alpha {max_a}"
        );
        assert!(!any_opaque, "纸纹不应有任何不透明像素");
    }

    #[test]
    fn mottle_texture_tiles_without_seams() {
        // 平铺硬契约：左列 == 右列、顶行 == 底行（同一位形逐字节一致）。
        let img = make_mottle_image();
        let d = img.data.as_ref().unwrap();
        let n = MOTTLE_TEX as usize;
        for i in 0..n {
            let left = &d[(i * n) * 4..(i * n) * 4 + 4];
            let right = &d[(i * n + n - 1) * 4..(i * n + n - 1) * 4 + 4];
            assert_eq!(&left[..3], &right[..3], "row {i} 左右缘应一致");
            let top = &d[i * 4..i * 4 + 4];
            let bottom = &d[((n - 1) * n + i) * 4..((n - 1) * n + i) * 4 + 4];
            assert_eq!(&top[..3], &bottom[..3], "col {i} 上下缘应一致");
        }
    }

    #[test]
    fn vignette_darkens_corners_only() {
        let img = make_vignette_image();
        let d = img.data.as_ref().unwrap();
        let n = VIGNETTE_TEX as usize;
        let center = &d[(n / 2 * n + n / 2) * 4..][..4];
        let corner = &d[0..4];
        assert_eq!(center[3], 0, "中心应全透");
        assert!(corner[3] > 0, "角部应有晕影");
        let (mid_edge_x, mid_edge_y) = (n / 2, 0);
        let edge = &d[(mid_edge_y * n + mid_edge_x) * 4..][..4];
        assert!(edge[3] > 0 && edge[3] < corner[3], "边中点晕影弱于角部");
    }

    #[test]
    fn quad_mesh_spans_origin_to_origin_plus_size() {
        let m = quad_mesh(
            Vec2::new(10.0, 20.0),
            Vec2::new(300.0, 200.0),
            Vec2::new(3.0, 2.0),
        );
        let bevy::mesh::VertexAttributeValues::Float32x3(pos) = m
            .attribute(MeshVertexAttribute::new(
                "Vertex_Position",
                0,
                VertexFormat::Float32x3,
            ))
            .unwrap()
        else {
            panic!("pos 应为 Float32x3");
        };
        assert_eq!(pos[0], [10.0, 20.0, 0.0]);
        assert_eq!(pos[2], [310.0, 220.0, 0.0]);
        let bevy::mesh::VertexAttributeValues::Float32x2(uv) =
            m.attribute(Mesh::ATTRIBUTE_UV_0).unwrap()
        else {
            panic!("uv 应为 Float32x2");
        };
        assert_eq!(uv[2], [3.0, 2.0], "UV 应按平铺次数伸展");
    }
}
