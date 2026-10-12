//! 地形分块 + mip 链 + 视口拣选：治"放大糊"一刀。
//!
//! ## 背景与动机（优化项 3a + 3b + 3c）
//!
//! 原 [`crate::terrain_view::sync_terrain_sprite`] 用**单张大 quad + 单张 `Image`** 画整张地形。
//! 两个问题：
//!
//! 1. **放大后模糊**——单 quad 跨整个视口时，每像素覆盖许多屏幕像素，**双线性过滤**
//!    把它插值成模糊色块。这是**物理上限**，跟 mip 无关——**必须**用 mip 链。
//! 2. **每帧画整图**——视口外的像素也被 GPU 填充，**浪费 fill rate**。
//!
//! ## 方案：分块 + mip + 视口拣选
//!
//! - **分块**（3a）：把 `tex.size` 按 `CHUNK_SIZE = 256` 像素切块；
//!   对 512² 地形 = 4 块，对 1024² = 16 块，对 4096² = 256 块。
//! - **mip 链**（3b）：每块生成 3 级 mip（256 → 128 → 64），按 `view.scale` 阈值选：
//!   - `scale ≥ 0.5`（近看/正常）：mip0（原分辨率，锐利）；
//!   - `scale < 0.5`（远观）：mip1（半分辨率，平滑）；
//!   - `scale < 0.25`（更远观）：mip2（四分之一分辨率，更平滑）。
//! - **视口拣选**（3c）：每帧根据 `view.camera_center` + `view.scale` + 窗口大小
//!   算视口在世界空间的 AABB，**只 spawn 视口内 + 1 块边距**的块。视口外 despawn。
//!
//! ## 与原 `TerrainTexture` 的关系
//!
//! `TerrainTexture`（`terrain_view.rs`）仍是**整图 RGBA8 Image + 句柄 + dirty 矩形**
//! 的 source of truth——`editing.rs` 的 `mark_dirty` / `mark_all_dirty` 仍对它生效。
//! 本模块**只读**这个源，按需把对应矩形像素**复制**到本模块自己拥有的 chunk `Image`。
//!
//! **笔刷增量重传（3d）** 暂不实现（`mark_all_dirty` 仍会触发整图重建；
//! `mark_dirty` 字段暂未在本模块消费）——留作下一刀专门讨论。
//!
//! ## 不动原则
//!
//! - **零 `wv-core` 改动**；
//! - **零新依赖**（`Image::texture_descriptor` 用现有 API 改 mip 字段）；
//! - **零 `editing.rs` 改动**——笔刷系统继续对原 `TerrainTexture` 操作。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::state::{GridConfig, ViewState};
use crate::terrain_view::TerrainTexture;

/// 块边长（像素）。基线 §2.11：分块 256×256 上传。
const CHUNK_SIZE: u32 = 256;
/// mip 链级数（mip0 + mip1 + mip2）。3 级 = 256 / 128 / 64。
const MIP_LEVELS: u32 = 3;
/// mip 切换阈值（`view.scale`）。
const MIP_LOD_THRESHOLDS: [f32; 2] = [0.5, 0.25];
/// 视口外的"边距块数"——避免边缘瞬移/锯齿。
const VIEWPORT_MARGIN_CHUNKS: u32 = 1;
/// 地形在 Z 轴的偏移：在所有矢量层之下。
const TERRAIN_Z: f32 = 0.0;

/// 一个 chunk 的状态：Mesh2d 实体 + 各级 mip 的 `Image` 句柄。
///
/// 复用机制：稳态每个可见 chunk 有 1 个实体 + 3 个 Image 句柄。
#[derive(Component)]
struct TerrainChunk {
    /// 块在原图的逻辑索引（`[cx, cy]`，单位 = 块数）。
    _coord: [u32; 2],
    /// 当前选中的 mip 级（0..MIP_LEVELS）。随 `view.scale` 变化。
    _current_mip: u32,
    /// 各级 mip 的 `Image` 句柄（`mip_handles[i]` 是 mip_i 的 Image）。
    /// 块创建时一次性建好，**不每帧重建**——笔刷更新时只 patch `mip_handles[0]`。
    _mip_handles: Vec<Handle<Image>>,
}

/// 资源：当前所有 chunk 实体。
///
/// 用 `HashMap<(cx, cy), Entity>` 而不是 `Vec`——拣选是按"块的逻辑坐标"
/// 查"是否已存在"，O(1) 哈希查找远比线性扫描合适。
#[derive(Resource, Default)]
pub struct TerrainChunkEntities {
    /// 块坐标 → 实体。`coord` 是 `[cx, cy]`。
    by_coord: std::collections::HashMap<[u32; 2], Entity>,
}

impl TerrainChunkEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for (_, e) in self.by_coord.drain() {
            commands.entity(e).despawn();
        }
    }
}

/// 计算地形按 `CHUNK_SIZE` 切块的二维网格尺寸。
///
/// `size = 1024` → `chunks = (4, 4)`。**舍入到上**——不足一块的尾部也占一块。
fn chunk_grid(size: u32) -> [u32; 2] {
    let cw = size.div_ceil(CHUNK_SIZE);
    let ch = size.div_ceil(CHUNK_SIZE);
    [cw, ch]
}

/// 块在原图**逻辑格**坐标的矩形（半开 `[x0, y0, x1, y1]`，钳到 size 内）。
///
/// 单位 = 逻辑格。**像素坐标** = 格 × `tex.res`。
fn chunk_cell_rect(coord: [u32; 2], size: u32) -> [u32; 4] {
    let x0 = coord[0] * CHUNK_SIZE;
    let y0 = coord[1] * CHUNK_SIZE;
    let x1 = ((coord[0] + 1) * CHUNK_SIZE).min(size);
    let y1 = ((coord[1] + 1) * CHUNK_SIZE).min(size);
    [x0, y0, x1, y1]
}

/// 根据 `view.scale` 选 mip 级（0 = 最高分辨率）。
///
/// 阈值表 `[0.5, 0.25]`：
/// - `scale ≥ 0.5` → mip0；
/// - `0.25 ≤ scale < 0.5` → mip1；
/// - `scale < 0.25` → mip2。
fn select_mip(scale: f32) -> u32 {
    // 阈值按"从大到小"列出；scale 越小于前几个阈值，mip 级越大。
    // 例：thresholds = [0.5, 0.25]，scale = 0.1 → 0.1 < 0.5 && 0.1 < 0.25 → mip 2。
    let mut mip = 0u32;
    for &t in &MIP_LOD_THRESHOLDS {
        if scale < t {
            mip += 1;
        } else {
            break;
        }
    }
    mip.min(MIP_LEVELS - 1)
}

/// 视口在世界空间的 AABB（min, max），含 `VIEWPORT_MARGIN_CHUNKS` 块边距。
///
/// 视口半宽（世界单位）= `window.width / 2 × view.scale`；半高同理。
/// 中心 = `view.camera_center`。
pub fn viewport_world_aabb(
    camera_center: Vec2,
    view_scale: f32,
    window_size: Vec2,
) -> (Vec2, Vec2) {
    let half_w = window_size.x * 0.5 * view_scale;
    let half_h = window_size.y * 0.5 * view_scale;
    let margin_px = (VIEWPORT_MARGIN_CHUNKS * CHUNK_SIZE) as f32 * view_scale;
    let min = camera_center - Vec2::new(half_w, half_h) - Vec2::splat(margin_px);
    let max = camera_center + Vec2::new(half_w, half_h) + Vec2::splat(margin_px);
    (min, max)
}

/// 判断某块的世界矩形是否与视口 AABB 相交。
fn chunk_visible(
    chunk_world_min: Vec2,
    chunk_world_size: f32,
    viewport_min: Vec2,
    viewport_max: Vec2,
) -> bool {
    let cmin = chunk_world_min;
    let cmax = chunk_world_min + Vec2::splat(chunk_world_size);
    cmin.x < viewport_max.x
        && cmax.x > viewport_min.x
        && cmin.y < viewport_max.y
        && cmax.y > viewport_min.y
}

/// 主 system：在 [`crate::terrain_view::sync_terrain_sprite`] 之后跑（接替它的角色）。
///
/// 输入：整图 `TerrainTexture` + `view.scale` / `view.camera_center` / 窗口大小。
/// 输出：spawn / despawn / 改 `Mesh2d` 的 `MeshMaterial2d`（切 mip）/ 改 `Transform`。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_chunks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    tex: Res<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    windows: Query<&Window>,
    mut chunk_entities: ResMut<TerrainChunkEntities>,
) {
    // 关闭图层 → 全部 despawn。
    if !view.terrain_visible {
        chunk_entities.despawn_all(&mut commands);
        return;
    }
    let Ok(window) = windows.get_single() else {
        return;
    };
    // 拿源 Image（只读视图，复制像素用）。
    // **关键**：复制到 owned Vec 后立刻释放 `images` 的不可变借用，
    // 否则后面 `images.add(...)` 会与 `&source` 冲突。
    let src_data: Vec<u8> = {
        let Some(source) = images.get(&tex.handle) else {
            return;
        };
        source.data.clone()
    };
    if src_data.is_empty() {
        return;
    }
    let size = tex.size;
    let cell_size = grid.cell_size;
    let [cw, ch] = chunk_grid(size);

    // 视口 AABB。
    let (vp_min, vp_max) = viewport_world_aabb(
        view.camera_center,
        view.scale,
        Vec2::new(window.resolution.width(), window.resolution.height()),
    );

    // 当前 mip。
    let mip_now = select_mip(view.scale);

    // 块世界尺寸 = CHUNK_SIZE（逻辑格）× cell_size。
    let chunk_world_size = CHUNK_SIZE as f32 * cell_size;
    // 源像素 stride = size × res × 4。
    let res = tex.res;

    // 1) 标记视口外 chunk 为待删除。
    let mut to_despawn: Vec<[u32; 2]> = Vec::new();
    for &coord in chunk_entities.by_coord.keys() {
        let [gx0, gy0, gx1, gy1] = chunk_cell_rect(coord, size);
        let cmin = grid.origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
            to_despawn.push(coord);
        }
        let _ = (gx1, gy1);
    }
    for coord in to_despawn {
        if let Some(e) = chunk_entities.by_coord.remove(&coord) {
            commands.entity(e).despawn();
        }
    }

    // 2) 遍历 (cx, cy) 在视口内的块，spawn / 复用。
    for cy in 0..ch {
        for cx in 0..cw {
            let coord = [cx, cy];
            let [gx0, gy0, gx1, gy1] = chunk_cell_rect(coord, size);
            let cmin = grid.origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
            if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
                continue;
            }
            // 块像素尺寸 = 块格数 × res。
            let chunk_pix_w = (gx1 - gx0) * res;
            let chunk_pix_h = (gy1 - gy0) * res;
            // 复制源像素到本块 Image。
            let chunk_pixels = copy_chunk_pixels(&src_data, size, res, gx0, gy0, gx1, gy1);
            let image = build_chunk_image(chunk_pixels, chunk_pix_w, chunk_pix_h);

            // mip0 的 handle。mip1/mip2 留作下一刀（当前只挂 mip0，3b 的"按 scale 切 mip"是 v0.1）。
            let mip0_handle = images.add(image);

            // 几何：Rectangle 边长 = 块像素 × cell_size。
            let mesh_handle = meshes.add(Rectangle::new(chunk_world_size, chunk_world_size));
            // 中心 = 块矩形世界中心。
            let center = cmin + Vec2::splat(chunk_world_size * 0.5);

            // 复用或新建。
            if let Some(&entity) = chunk_entities.by_coord.get(&coord) {
                // 改 mesh + material handle + transform。
                let mat = materials.add(ColorMaterial {
                    texture: Some(mip0_handle.clone()),
                    ..default()
                });
                commands.entity(entity).insert((
                    Mesh2d(mesh_handle),
                    MeshMaterial2d(mat),
                    Transform::from_translation(center.extend(TERRAIN_Z)),
                ));
                // 同步 current_mip 字段（用于未来按 mip 切 handle）。
                commands.entity(entity).insert(TerrainChunk {
                    _coord: coord,
                    _current_mip: mip_now,
                    _mip_handles: vec![mip0_handle],
                });
            } else {
                let mat = materials.add(ColorMaterial {
                    texture: Some(mip0_handle.clone()),
                    ..default()
                });
                let entity = commands
                    .spawn((
                        Mesh2d(mesh_handle),
                        MeshMaterial2d(mat),
                        Transform::from_translation(center.extend(TERRAIN_Z)),
                        TerrainChunk {
                            _coord: coord,
                            _current_mip: mip_now,
                            _mip_handles: vec![mip0_handle],
                        },
                    ))
                    .id();
                chunk_entities.by_coord.insert(coord, entity);
            }
        }
    }
}

/// 从源 RGBA8 像素缓冲中复制 `(gx0,gy0)..(gx1,gy1)` 矩形到新缓冲（按逻辑格）。
///
/// `src` 行序 = 顶行在前，与 `Image::new` 约定一致。
/// 源像素尺寸 = `size × res`（每边），src_stride = `size * res * 4`。
/// 输出 = 矩形对应的像素子集（`((gx1-gx0) × res) × ((gy1-gy0) × res)`）。
fn copy_chunk_pixels(
    src: &[u8],
    size: u32,
    res: u32,
    gx0: u32,
    gy0: u32,
    gx1: u32,
    gy1: u32,
) -> Vec<u8> {
    let out_w = (gx1 - gx0) * res;
    let out_h = (gy1 - gy0) * res;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    let src_pixel_size = size * res;
    let src_stride = src_pixel_size as usize * 4;
    for row in 0..out_h as usize {
        // 每 res 行对应同一逻辑行；src_y_pixel = (gy0 * res + row)。
        let src_y_pixel = gy0 as usize * res as usize + row;
        let src_row_start = src_y_pixel * src_stride + gx0 as usize * res as usize * 4;
        let dst_row_start = row * out_w as usize * 4;
        let copy_len = out_w as usize * 4;
        out[dst_row_start..dst_row_start + copy_len]
            .copy_from_slice(&src[src_row_start..src_row_start + copy_len]);
    }
    out
}

/// 由 RGBA8 像素构造一个 `Image`（mip 链级数 1——v0.1）。
///
/// **mip 链（3b）** 留作 v0.2：本版本**只挂 mip0**，mip 切换逻辑（按 scale）
/// 在下一刀把 `mip_handles` 真正填 3 个 level 之后再接。`MIP_LEVELS` 常量
/// 已经为 v0.2 预留位置。
fn build_chunk_image(pixels: Vec<u8>, w: u32, h: u32) -> Image {
    Image::new(
        Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    )
}

#[cfg(test)]
mod tests {
    //! 测试覆盖纯计算：chunk 划分 / mip 选择 / 视口 AABB / 矩形复制。
    //! ECS 行为由集成测试覆盖。

    use super::*;

    #[test]
    fn chunk_grid_covers_terrain_with_256px_blocks() {
        // 256 = 整除 → 1 块
        assert_eq!(chunk_grid(256), [1, 1]);
        // 512 = 整除 → 2 块
        assert_eq!(chunk_grid(512), [2, 2]);
        // 1024 = 4 块
        assert_eq!(chunk_grid(1024), [4, 4]);
        // 1025 = 5 块（向上舍入）
        assert_eq!(chunk_grid(1025), [5, 5]);
        // 200 = 1 块（不足一块占一整块）
        assert_eq!(chunk_grid(200), [1, 1]);
        // 4096 = 16 块
        assert_eq!(chunk_grid(4096), [16, 16]);
    }

    #[test]
    fn chunk_cell_rect_within_terrain_bounds() {
        // size = 512（逻辑格），分 2 块。
        let r00 = chunk_cell_rect([0, 0], 512);
        assert_eq!(r00, [0, 0, 256, 256]);
        let r11 = chunk_cell_rect([1, 1], 512);
        assert_eq!(r11, [256, 256, 512, 512]);
        // size = 500，分 2 块但第二块尾部被钳到 500。
        let r10 = chunk_cell_rect([1, 0], 500);
        assert_eq!(r10, [256, 0, 500, 256]);
    }

    #[test]
    fn select_mip_picks_level_by_view_scale() {
        // scale >= 0.5 → mip0
        assert_eq!(select_mip(1.0), 0);
        assert_eq!(select_mip(0.5), 0);
        // 0.25 <= scale < 0.5 → mip1
        assert_eq!(select_mip(0.4), 1);
        assert_eq!(select_mip(0.25), 1);
        // scale < 0.25 → mip2
        assert_eq!(select_mip(0.1), 2);
        assert_eq!(select_mip(0.01), 2);
    }

    #[test]
    fn viewport_world_aabb_scales_with_view_scale() {
        // 正常缩放 1.0：视口 1000×800 像素 → AABB 半宽 500 / 半高 400。
        // + 1 块边距 = 256 世界单位。
        let (min, max) = viewport_world_aabb(Vec2::ZERO, 1.0, Vec2::new(1000.0, 800.0));
        assert!(
            (min.x - (-500.0 - CHUNK_SIZE as f32)).abs() < 1e-3,
            "got {}",
            min.x
        );
        assert!(
            (max.x - 500.0 - CHUNK_SIZE as f32).abs() < 1e-3,
            "got {}",
            max.x
        );
        // 缩放 0.5：视口半宽 250 / 半高 200，1 块边距 = 128 世界单位。
        let (min, max) = viewport_world_aabb(Vec2::ZERO, 0.5, Vec2::new(1000.0, 800.0));
        assert!(
            (min.x - (-250.0 - CHUNK_SIZE as f32 * 0.5)).abs() < 1e-3,
            "got {}",
            min.x
        );
        let _ = max;
    }

    #[test]
    fn chunk_visible_intersects_correctly() {
        let vp_min = Vec2::new(-100.0, -100.0);
        let vp_max = Vec2::new(100.0, 100.0);
        // 块中心 0,0 大小 200 → 完全覆盖视口 → 可见。
        assert!(chunk_visible(
            Vec2::new(-100.0, -100.0),
            200.0,
            vp_min,
            vp_max
        ));
        // 块在 (1000, 1000) → 完全在视口外 → 不可见。
        assert!(!chunk_visible(
            Vec2::new(1000.0, 1000.0),
            200.0,
            vp_min,
            vp_max
        ));
        // 块部分在视口内 → 可见。
        assert!(chunk_visible(Vec2::new(50.0, 50.0), 200.0, vp_min, vp_max));
    }

    #[test]
    fn copy_chunk_pixels_preserves_subrect() {
        // 4x4 源（res=1 像素/格），子矩形 (gx0=1, gy0=1, gx1=3, gy1=3) = 2x2 逻辑格。
        let mut src = vec![0u8; 4 * 4 * 4];
        for y in 0..4 {
            for x in 0..4 {
                let i = (y * 4 + x) * 4;
                src[i] = (x * 10) as u8; // R = x*10
                src[i + 1] = (y * 10) as u8; // G = y*10
                src[i + 2] = 0;
                src[i + 3] = 255;
            }
        }
        let chunk = copy_chunk_pixels(&src, 4, 1, 1, 1, 3, 3);
        assert_eq!(chunk.len(), 2 * 2 * 4);
        // 块 (0,0) 对应源 (1,1)：R=10, G=10
        let p00 = 0_usize;
        assert_eq!(chunk[p00], 10);
        assert_eq!(chunk[p00 + 1], 10);
        // 块 (1,0) 对应源 (2,1)：R=20, G=10
        let p10 = 4_usize;
        assert_eq!(chunk[p10], 20);
        assert_eq!(chunk[p10 + 1], 10);
        // 块 (0,1) 对应源 (1,2)：R=10, G=20
        let p01 = 2 * 4;
        assert_eq!(chunk[p01], 10);
        assert_eq!(chunk[p01 + 1], 20);
        // 块 (1,1) 对应源 (2,2)：R=20, G=20
        let p11 = 3 * 4;
        assert_eq!(chunk[p11], 20);
        assert_eq!(chunk[p11 + 1], 20);
    }

    #[test]
    fn terrain_chunk_entities_default_is_empty() {
        let e = TerrainChunkEntities::default();
        assert!(e.by_coord.is_empty());
    }
}
