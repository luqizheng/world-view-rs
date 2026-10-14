//! 地形分块 + mip 链 + 视口拣选 + 脏矩形驱动（S3 优化项 3a + 3b + 3c + 3e）。
//!
//! ## 职责
//! - 把整张地形切成 N×N 的图块，每块持有 mip 链，屏幕恒定 1.5px 描边。
//! - 视口拣选：只 sync 当前视口内可见的 chunk；视口外的帧早退，省 fillrate。
//! - 脏矩形驱动：`dirty_chunks` 标记哪些 chunk 在编辑后需要重烘，其它早退。
//!
//! ## 边界（不做什么）
//! - **不画风格**（JRPG 图集 / Google / 古舆图色板）——单图块内部色值由
//!   [`crate::theme::palette`] + [`crate::render::terrain_view`] 决定；
//!   本模块只关心几何 / mip / 拣选。
//! - **不直接听编辑输入**——笔触收口在 [`crate::editing`]；本模块订阅
//!   `terrain.dirty_chunks`（`Changed<WorldProjection>` 之类）就够了。
//!
//! ## 主要导出
//! - `sync_visible_chunks` — 视口拣选 + mip 同步。
//! - `mark_dirty` — 把被编辑过的 chunk 写入 `dirty_chunks` 集合。
//! - 资源 `TerrainChunks`（mip 链 + 实体索引）、`ChunkCoords` / `MipLevel`。
//!
//! - **分块**（3a）：把 `tex.size` 按 `CHUNK_SIZE = 256` 逻辑格切块；
//!   对 512² 地形 = 4 块，对 1024² = 16 块，对 4096² = 256 块。
//! - **mip 链**（3b）：每块 spawn 时一次性生成 3 级 mip（逐级盒式 2×2 降采样），
//!   按 `view.scale` 阈值选：
//!   - `scale ≥ 0.5`（近看/正常）：mip0（原分辨率，锐利）；
//!   - `scale < 0.5`（远观）：mip1（半分辨率，平滑）；
//!   - `scale < 0.25`（更远观）：mip2（四分之一分辨率，更平滑）。
//!     切换只换材质的纹理句柄，几何不动。
//! - **视口拣选**（3c）：每帧根据 `view.camera_center` + `view.scale` + 窗口大小
//!   算视口在世界空间的 AABB，**只 spawn 视口内 + 1 块边距**的块。视口外 despawn。
//! - **脏矩形驱动**（3e）：稳态**不拷任何像素**——只有 `tex.chunks_dirty`
//!   非空（笔刷编辑 / 撤销 / 模式切换）或新块 spawn 时才克隆整图源像素，
//!   且只 patch 与脏矩形相交的块。整图 4096²×4B ≈ 67MB，每帧克隆不可接受。
//!
//! ## 与原 `TerrainTexture` 的关系
//!
//! `TerrainTexture`（`terrain_view.rs`）仍是**整图 RGBA8 Image + 句柄 + dirty 矩形**
//! 的 source of truth——`editing.rs` 的 `mark_dirty` / `mark_all_dirty` 仍对它生效。
//! 本模块**只读**这个源，按需把对应矩形像素**复制**到本模块自己拥有的 chunk `Image`。
//!
//! ## 与 `dirty` / `chunks_dirty` 的分工
//!
//! system 链顺序：`flush_terrain_texture`（patch 整图，清 `dirty`）
//! → `rebuild_texture_on_mode_change` → `sync_terrain_chunks`（本模块，清 `chunks_dirty`）。
//! 两个字段由 `mark_dirty` 同步置位、各自消费者分别清零，互不干扰。
//!
//! ## 不动原则
//!
//! - **零 `wv-core` 改动**；
//! - **零新依赖**（mip 用 CPU 盒式降采样生成独立 `Image`，不走 GPU mip 语义）；
//! - **零 `editing.rs` 改动**——笔刷系统继续对原 `TerrainTexture` 操作。

#![cfg(feature = "app")]

use bevy::image::ImageSampler;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::render::layer_frame::LayerFrame;
use crate::render::terrain_view::TerrainTexture;
use crate::state::{GridConfig, ViewState};

/// 块边长（逻辑格）。基线 §2.11：分块 256×256 上传。
const CHUNK_SIZE: u32 = 256;
/// mip 链级数（mip0 + mip1 + mip2）。3 级 = 256 / 128 / 64。
const MIP_LEVELS: u32 = 3;
/// mip 切换阈值（`view.scale`）。
const MIP_LOD_THRESHOLDS: [f32; 2] = [0.5, 0.25];
/// 视口外的"边距块数"——避免边缘瞬移/锯齿。
const VIEWPORT_MARGIN_CHUNKS: u32 = 1;
/// 地形在 Z 轴的偏移：在所有矢量层之下。
const TERRAIN_Z: f32 = 0.0;

/// 一个 chunk 的状态：Mesh2d 实体 + 各级 mip 的 `Image` 句柄 + 材质句柄。
///
/// 复用机制：稳态每个可见 chunk 有 1 个实体 + `MIP_LEVELS` 个 Image 句柄 + 1 个材质。
/// 几何（mesh）对固定坐标不变，因此只在 spawn 时建一次；mip 切换只改材质纹理。
#[derive(Component)]
pub struct TerrainChunk {
    /// 块在原图的逻辑索引（`[cx, cy]`，单位 = 块数）。仅诊断用。
    _coord: [u32; 2],
    /// 当前选中的 mip 级（0..MIP_LEVELS）。随 `view.scale` 变化。
    current_mip: u32,
    /// 各级 mip 的 `Image` 句柄（`mip_handles[i]` 是 mip_i 的 Image）。
    /// 块创建时一次性建好；笔刷更新时 patch mip0 并重降采样 mip1/mip2。
    mip_handles: Vec<Handle<Image>>,
    /// 材质句柄——切 mip 时原地改它的 `texture` 字段，不新建材质资产。
    material: Handle<ColorMaterial>,
    /// 是否使用线性采样。Google 式底图需要平滑边界；专题色块图保持锐利。
    linear_filter: bool,
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
    /// 当前活跃的 chunk 数（仅诊断/测试用，不参与渲染逻辑）。
    pub fn len(&self) -> usize {
        self.by_coord.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_coord.is_empty()
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

/// 两个半开矩形（`[x0, y0, x1, y1]`）是否相交。边贴边不算相交。
fn rects_intersect(a: [u32; 4], b: [u32; 4]) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}

/// 视口在世界空间的 AABB（min, max），含 `VIEWPORT_MARGIN_CHUNKS` 块边距。
///
/// 视口半宽（世界单位）= `window.width / 2 × view.scale`；半高同理。
/// 中心 = `view.camera_center`。相机带旋转（地图朝向）时视口是**斜矩形**，
/// 其世界 AABB 半轴吃进另一轴的 |sin| 分量——不扩宽的话斜着看时边缘块
/// 会被误判视野外，画面出现空洞。
pub fn viewport_world_aabb(
    camera_center: Vec2,
    view_scale: f32,
    window_size: Vec2,
    camera_rotation: Quat,
) -> (Vec2, Vec2) {
    let half_w = window_size.x * 0.5 * view_scale;
    let half_h = window_size.y * 0.5 * view_scale;
    // 单位轴经旋转后的分量绝对值 = 该轴在各世界方向上的投影长度。
    let rx = camera_rotation.mul_vec3(Vec3::X).truncate();
    let ry = camera_rotation.mul_vec3(Vec3::Y).truncate();
    let ext = Vec2::new(
        half_w * rx.x.abs() + half_h * ry.x.abs(),
        half_w * rx.y.abs() + half_h * ry.y.abs(),
    );
    let margin_px = (VIEWPORT_MARGIN_CHUNKS * CHUNK_SIZE) as f32 * view_scale;
    let min = camera_center - ext - Vec2::splat(margin_px);
    let max = camera_center + ext + Vec2::splat(margin_px);
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

/// 分块日志开关位（诊断遗留，恒 false；保留函数签名避免无谓 churn）。
#[allow(dead_code)]
fn pending_dirty_or_diag(_scale: f32) -> bool {
    false
}

/// Despawn 一个 chunk，并在实体消失后释放其独占 Image / Material 资产。
///
/// Bevy 的 `Assets` 删除不是引用计数；直接在 despawn 前同步移除可能让同一帧
/// 的渲染仍拿着悬空句柄。这里把清理排队到 despawn 之后执行。
fn despawn_chunk(
    commands: &mut Commands,
    entity: Entity,
    q_chunks: &Query<(&mut TerrainChunk, &Mesh2d)>,
) {
    let Ok((chunk, mesh)) = q_chunks.get(entity) else {
        return;
    };
    let image_handles = chunk.mip_handles.clone();
    let material = chunk.material.clone();
    let mesh_handle = mesh.0.clone();
    commands.entity(entity).despawn();
    commands.queue(move |world: &mut World| {
        world.resource_mut::<Assets<Mesh>>().remove(&mesh_handle);
    });
    for handle in image_handles {
        commands.queue(move |world: &mut World| {
            world.resource_mut::<Assets<Image>>().remove(&handle);
        });
    }
    commands.queue(move |world: &mut World| {
        world
            .resource_mut::<Assets<ColorMaterial>>()
            .remove(&material);
    });
}

/// 主 system：在 [`crate::edit::editing::flush_terrain_texture`] 与
/// [`crate::render::terrain_view::rebuild_texture_on_mode_change`] 之后跑。
///
/// 输入：整图 `TerrainTexture` + `view.scale` / `view.camera_center` / 窗口大小。
/// 输出：spawn / despawn chunk 实体；对已存在块按需 patch 像素或切 mip。
///
/// ## 每帧开销
///
/// 稳态（无编辑、无 spawn）：只做视口 AABB 与哈希查找，**零像素拷贝、零资产新建**。
/// 笔刷提交后一帧：克隆整图源像素一次，patch 相交块，重建其 mip1/mip2。
#[allow(clippy::too_many_arguments)]
pub fn sync_terrain_chunks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut tex: ResMut<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    mut chunk_entities: ResMut<TerrainChunkEntities>,
    mut q_chunks: Query<(&mut TerrainChunk, &Mesh2d)>,
) {
    // 关闭图层（YAML 声明 ∨ 会话开关，ADR-0013）→ 全部 despawn；
    // 丢掉待消费脏矩形（重开时 spawn 会拷最新像素）。
    if !layers.terrain.visible {
        let entities: Vec<Entity> = chunk_entities.by_coord.drain().map(|(_, e)| e).collect();
        for entity in entities {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
        if tex.chunks_dirty.is_some() {
            tex.chunks_dirty = None;
        }
        return;
    }
    // 绑定变化（作者改 YAML 的 opacity/z）：已存在块就地补丁——不重建像素，
    // 只写材质 alpha 与 Transform.z（重 spawn 的块在下方 spawn 路径取新值）。
    // 借**延迟命令**写：本系统的 `q_chunks` 已持有 `&mut TerrainChunk`，
    // 再开一个读它的查询会触发 Bevy B0001 访问冲突（启动即 panic）。
    if layers.is_changed() {
        let z = TERRAIN_Z + layers.terrain.z_offset;
        let alpha = layers.terrain.opacity;
        for &entity in chunk_entities.by_coord.values() {
            let Ok((chunk, _)) = q_chunks.get(entity) else {
                continue;
            };
            let material = chunk.material.clone();
            commands.queue(move |world: &mut World| {
                if let Some(mut tf) = world.get_mut::<Transform>(entity) {
                    tf.translation.z = z;
                }
                if let Some(mut mat) = world
                    .resource_mut::<Assets<ColorMaterial>>()
                    .get_mut(&material)
                {
                    mat.color = Color::WHITE.with_alpha(alpha);
                }
            });
        }
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let size = tex.size;
    let google_mode = view.mode == crate::state::ViewMode::GoogleMap;
    let cell_size = grid.cell_size;
    let [cw, ch] = chunk_grid(size);
    let res = tex.res;

    // 视口 AABB 与当前 mip 级。相机旋转（地图朝向）决定视口斜矩形的朝向。
    let cam_rot = camera
        .single()
        .map(|t| t.rotation)
        .unwrap_or(Quat::IDENTITY);
    let (vp_min, vp_max) = viewport_world_aabb(
        view.camera_center,
        view.scale,
        Vec2::new(window.resolution.width(), window.resolution.height()),
        cam_rot,
    );
    let mip_now = select_mip(view.scale);
    let chunk_world_size = CHUNK_SIZE as f32 * cell_size;

    // 1) 视口外 chunk despawn。
    let mut to_despawn: Vec<[u32; 2]> = Vec::new();
    for &coord in chunk_entities.by_coord.keys() {
        let [gx0, gy0, _, _] = chunk_cell_rect(coord, size);
        let cmin = grid.raster_origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
            to_despawn.push(coord);
        }
    }
    for coord in to_despawn {
        if let Some(entity) = chunk_entities.by_coord.remove(&coord) {
            despawn_chunk(&mut commands, entity, &q_chunks);
        }
    }

    // 2) 收集视口内块；判断本帧是否需要源像素——有待 spawn 块，或有与 dirty 相交的块。
    let pending_dirty = tex.chunks_dirty;
    let mut visible: Vec<([u32; 2], [u32; 4])> = Vec::new();
    let mut needs_src = false;
    for cy in 0..ch {
        for cx in 0..cw {
            let coord = [cx, cy];
            let cells = chunk_cell_rect(coord, size);
            let cmin = grid.raster_origin + Vec2::new(cells[0] as f32, cells[1] as f32) * cell_size;
            if !chunk_visible(cmin, chunk_world_size, vp_min, vp_max) {
                continue;
            }
            if !chunk_entities.by_coord.contains_key(&coord)
                || pending_dirty.is_some_and(|d| rects_intersect(d, cells))
            {
                needs_src = true;
            }
            visible.push((coord, cells));
        }
    }

    // 源像素只在"确实要写"时克隆——整图 4096²×4B ≈ 67MB，每帧克隆不可接受。
    let src_data: Option<Vec<u8>> = if needs_src {
        match images.get(&tex.handle) {
            Some(source) if source.data.as_ref().is_some_and(|d| !d.is_empty()) => {
                source.data.clone()
            }
            _other => {
                // TODO(诊断 2026-10-14): "只看到等值线、没有底图填充" 回归。
                // 触发条件是 needs_src=true 但 source.data 为空/缺。常见原因：
                // ① tex.handle 资产被 replace_terrain_texture 用 *img=image 整
                //    体替换——bevy 0.15 的 Assets<Image> 是 Arc-backed，clone 仍
                //    指向旧值（这是已知的迁移期坑）。
                // ② handle 与 images 里其他槽位撞键。
                // 关闭诊断后移除本分支。
                if std::env::var("WV_CHUNK_DIAG").is_ok() {
                    let has_handle = images.get(&tex.handle).is_some();
                    let data_len = images
                        .get(&tex.handle)
                        .and_then(|i| i.data.as_ref())
                        .map(|d| d.len())
                        .unwrap_or(0);
                    eprintln!(
                        "[chunk-diag] needs_src=true but source unusable: handle_valid={has_handle} data_len={data_len} tex.size={} tex.res={} chunk_entities={} visible_chunks={}",
                        tex.size,
                        tex.res,
                        chunk_entities.by_coord.len(),
                        visible.len(),
                    );
                }
                // `&Image` 是 Copy，match 块结束自动释放——不需要手动 drop。
                None
            }
        }
    } else {
        None
    };
    // 需要像素但源未就绪：保留 chunks_dirty，下帧重试。
    if needs_src && src_data.is_none() {
        if std::env::var("WV_CHUNK_DIAG").is_ok() {
            eprintln!("[chunk-diag] bailing: needs_src but src_data=None, keeping chunks_dirty");
        }
        return;
    }
    let src = src_data.as_deref();

    // 3) spawn / patch / 切 mip。
    for (coord, cells) in visible {
        let [gx0, gy0, gx1, gy1] = cells;
        let cmin = grid.raster_origin + Vec2::new(gx0 as f32, gy0 as f32) * cell_size;
        let center = cmin + Vec2::splat(chunk_world_size * 0.5);

        if let Some(&entity) = chunk_entities.by_coord.get(&coord) {
            // 3a-0) 尺寸失配检测：tex.size/res 随世界切换变化（如 `-m` 加载
            // 1024 格工程，res 8→4）后，旧块的 mip 资产 extent 仍是旧 res 的。
            // `patch_chunk_mips` 用 `Vec::clone_from` 写 mip（长度可变），会把
            // 新 res 的数据塞进旧 extent 的 Image——GPU 上传即 panic
            // （wgpu device.rs「range end index out of range」）。整块作废，
            // 移出 by_coord 落到下方重生路径，按新尺寸重建。
            let mip0_matches = q_chunks
                .get(entity)
                .ok()
                .and_then(|(c, _)| c.mip_handles.first())
                .and_then(|h| images.get(h))
                .map(|mip0| {
                    mip0.texture_descriptor.size.width == (gx1 - gx0) * res
                        && mip0.texture_descriptor.size.height == (gy1 - gy0) * res
                        && q_chunks
                            .get(entity)
                            .map(|(c, _)| c.linear_filter == google_mode)
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if !mip0_matches {
                chunk_entities.by_coord.remove(&coord);
                despawn_chunk(&mut commands, entity, &q_chunks);
            } else {
                let Ok((mut chunk, _)) = q_chunks.get_mut(entity) else {
                    continue;
                };
                // 3a) dirty 相交 → patch mip0 像素，并逐级重建 mip1/mip2。
                if let (Some(dirty), Some(src)) = (pending_dirty, src) {
                    if rects_intersect(dirty, cells) {
                        patch_chunk_mips(&mut images, &chunk, src, size, res, cells, dirty);
                        // Bevy 0.15：就地改 `Image` 数据不使既有 `ColorMaterial` 的
                        // bindgroup 失效——GPU 上仍采样旧纹理。触碰材质资产强制
                        // 重建 bindgroup，让本帧 patch 的像素真正上屏（模式切换 /
                        // 笔刷提交共用此路径）。
                        if materials.get_mut(&chunk.material).is_none() {
                            continue;
                        }
                    }
                }
                // 3b) mip 级随缩放切换——只换材质纹理句柄，几何与资产不动。
                if chunk.current_mip != mip_now {
                    if let Some(mut mat) = materials.get_mut(&chunk.material) {
                        mat.texture = chunk.mip_handles.get(mip_now as usize).cloned();
                    }
                    chunk.current_mip = mip_now;
                }
                continue;
            }
        }

        // spawn：拷贝本块像素 + 建 mip 链 + 建几何与材质。
        let Some(src) = src else { continue };
        let chunk_pixels = copy_chunk_pixels(src, size, res, gx0, gy0, gx1, gy1);
        let pw = (gx1 - gx0) * res;
        let ph = (gy1 - gy0) * res;
        let mip_handles: Vec<Handle<Image>> = build_mip_chain(chunk_pixels, pw, ph, google_mode)
            .into_iter()
            .map(|img| images.add(img))
            .collect();
        let mesh_handle = meshes.add(Rectangle::new(chunk_world_size, chunk_world_size));
        let material = materials.add(ColorMaterial {
            // 图层透明度（ADR-0013）：作者 YAML 的 `opacity` 落在材质 alpha 上。
            color: Color::WHITE.with_alpha(layers.terrain.opacity),
            texture: mip_handles.get(mip_now as usize).cloned(),
            // V 翻转：Mesh2d + Rectangle 的默认 UV 把 image data 第 0 行映射到世界 +
            // Y 顶点，与 `apply_brush` / `isoline_overlay` 的「cell row 0 = 世界底部」
            // 约定相反。翻转后 data 行 0 落在世界底部，与笔刷和等值线描边同源——
            // 修复「VectorFlat 填充与描边呈 90° 错位 / 笔刷落点与网格点对不上」
            // （M2 提交 5f8cb73 后浮现的回归：色块模式没有描边时不可见）。
            uv_transform: Affine2::from_translation(Vec2::new(0.0, 1.0))
                * Affine2::from_scale(Vec2::new(1.0, -1.0)),
            ..default()
        });
        let entity = commands
            .spawn((
                Mesh2d(mesh_handle),
                MeshMaterial2d(material.clone()),
                Transform::from_translation(center.extend(TERRAIN_Z + layers.terrain.z_offset)),
                TerrainChunk {
                    _coord: coord,
                    current_mip: mip_now,
                    mip_handles,
                    material,
                    linear_filter: google_mode,
                },
            ))
            .id();
        chunk_entities.by_coord.insert(coord, entity);
    }

    // 4) 脏矩形已消费（只在确有写入时触碰资源，避免无谓的 changed 标记）。
    if tex.chunks_dirty.is_some() {
        tex.chunks_dirty = None;
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

/// 盒式 2×2 降采样：`(w, h)` → `(max(w/2,1), max(h/2,1))`，每通道取 ≤4 个源像素平均。
///
/// 奇数尺寸向下取整（尾部像素被钳进邻近采样点）——mip 本就是近似，不追求可逆。
fn downsample_box(src: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let ow = (w / 2).max(1);
    let oh = (h / 2).max(1);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh {
        for x in 0..ow {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let sx = (x * 2 + dx).min(w.saturating_sub(1));
                    let sy = (y * 2 + dy).min(h.saturating_sub(1));
                    let o = (sy * w + sx) as usize * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += src[o + c] as u32;
                    }
                    n += 1;
                }
            }
            let o = (y * ow + x) as usize * 4;
            for (c, a) in acc.iter().enumerate() {
                out[o + c] = (a / n) as u8;
            }
        }
    }
    (out, ow, oh)
}

/// 由 mip0 像素生成完整 mip 链：mip0 原样，其后逐级 [`downsample_box`]。
fn build_mip_chain(pixels: Vec<u8>, w: u32, h: u32, linear_filter: bool) -> Vec<Image> {
    let mut out = Vec::with_capacity(MIP_LEVELS as usize);
    let (mut cur, mut cw, mut ch) = (pixels, w.max(1), h.max(1));
    for _ in 0..MIP_LEVELS {
        // 先算下一级再移交当前级所有权（末级多算的一次降采样仅 64²，可忽略）。
        let next = downsample_box(&cur, cw, ch);
        out.push(build_chunk_image(cur, cw, ch, linear_filter));
        cur = next.0;
        cw = next.1;
        ch = next.2;
    }
    out
}

/// 把整图 dirty 矩形与本块的交集 patch 进本块 mip0，再逐级重建 mip1/mip2。
///
/// 只拷交集区域的像素行（开销 ∝ 笔刷面积）；mip1/mip2 从 patch 后的 mip0
/// 全量重降采样——块 ≤ 256² 像素，这笔开销可忽略。
#[allow(clippy::too_many_arguments)]
fn patch_chunk_mips(
    images: &mut Assets<Image>,
    chunk: &TerrainChunk,
    src: &[u8],
    size: u32,
    res: u32,
    cells: [u32; 4],
    dirty: [u32; 4],
) {
    let [gx0, gy0, gx1, gy1] = cells;
    // 交集（逻辑格，半开）。
    let ix0 = dirty[0].max(gx0);
    let iy0 = dirty[1].max(gy0);
    let ix1 = dirty[2].min(gx1);
    let iy1 = dirty[3].min(gy1);
    if ix0 >= ix1 || iy0 >= iy1 {
        return;
    }
    let chunk_w_px = (gx1 - gx0) * res;
    let chunk_h_px = (gy1 - gy0) * res;
    let src_stride = (size * res) as usize * 4;
    let (pw, ph) = ((ix1 - ix0) * res, (iy1 - iy0) * res);
    // 块内像素起点。
    let (px0, py0) = ((ix0 - gx0) * res, (iy0 - gy0) * res);

    let Some(mip0_handle) = chunk.mip_handles.first() else {
        return;
    };
    let Some(mut mip0) = images.get_mut(mip0_handle) else {
        return;
    };
    // TODO(诊断): patch 取证，验证后移除。
    if false {
        eprintln!(
            "[diag] patch chunk {:?} cells={:?} dirty={:?} src_first={:02X}{:02X}{:02X} dst_first={:02X}{:02X}{:02X}",
            chunk._coord,
            cells,
            dirty,
            src[0],
            src[1],
            src[2],
            mip0.data.as_ref().expect("mip 像素数据")[0],
            mip0.data.as_ref().expect("mip 像素数据")[1],
            mip0.data.as_ref().expect("mip 像素数据")[2]
        );
    }
    for row in 0..ph as usize {
        let src_y = (iy0 * res) as usize + row;
        let s = src_y * src_stride + (ix0 * res) as usize * 4;
        let d = ((py0 as usize + row) * chunk_w_px as usize + px0 as usize) * 4;
        let len = pw as usize * 4;
        mip0.data.as_mut().expect("mip 像素数据")[d..d + len].copy_from_slice(&src[s..s + len]);
    }
    // mip 链逐级重建（mip1 ← mip0，mip2 ← mip1）。
    // `mip0` 的借用在此最后一次使用后结束，下面才能再次 `images.get_mut`。
    let (mut px, mut w, mut h) = (
        mip0.data.clone().unwrap_or_default(),
        chunk_w_px,
        chunk_h_px,
    );
    // `AssetMut` 的借用活到析构（不像 `&mut` 那样随最后一次使用结束），
    // 必须显式 drop，才能在同一帧里对本资产表再次 `get_mut`。
    drop(mip0);
    for level in 1..MIP_LEVELS {
        let (dp, dw, dh) = downsample_box(&px, w, h);
        let Some(handle) = chunk.mip_handles.get(level as usize) else {
            break;
        };
        let Some(mut img) = images.get_mut(handle) else {
            break;
        };
        img.data = Some(dp.clone());
        px = dp;
        w = dw;
        h = dh;
    }
}

/// 由 RGBA8 像素构造一个 `Image`（单级；mip 链由 [`build_mip_chain`] 组装）。
fn build_chunk_image(pixels: Vec<u8>, w: u32, h: u32, linear_filter: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD | bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = if linear_filter {
        ImageSampler::linear()
    } else {
        ImageSampler::nearest()
    };
    image
}

#[cfg(test)]
mod tests {
    //! 测试覆盖纯计算：chunk 划分 / mip 选择 / 视口 AABB / 矩形复制 /
    //! 矩形相交 / 降采样 / mip 链 / 脏矩形 patch。
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
    fn rects_intersect_uses_half_open_semantics() {
        // 部分重叠 → 相交。
        assert!(rects_intersect([0, 0, 4, 4], [2, 2, 6, 6]));
        // 包含 → 相交。
        assert!(rects_intersect([0, 0, 8, 8], [2, 2, 4, 4]));
        // 边贴边（半开区间）→ 不相交。
        assert!(!rects_intersect([0, 0, 2, 2], [2, 0, 4, 2]));
        assert!(!rects_intersect([0, 0, 2, 2], [0, 2, 2, 4]));
        // 完全分离 → 不相交。
        assert!(!rects_intersect([0, 0, 1, 1], [5, 5, 6, 6]));
    }

    #[test]
    fn viewport_world_aabb_scales_with_view_scale() {
        // 正常缩放 1.0：视口 1000×800 像素 → AABB 半宽 500 / 半高 400。
        // + 1 块边距 = 256 世界单位。恒等旋转下与历史口径逐值相同。
        let (min, max) =
            viewport_world_aabb(Vec2::ZERO, 1.0, Vec2::new(1000.0, 800.0), Quat::IDENTITY);
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
        let (min, max) =
            viewport_world_aabb(Vec2::ZERO, 0.5, Vec2::new(1000.0, 800.0), Quat::IDENTITY);
        assert!(
            (min.x - (-250.0 - CHUNK_SIZE as f32 * 0.5)).abs() < 1e-3,
            "got {}",
            min.x
        );
        let _ = max;
    }

    /// 相机转 45° 时视口是斜矩形：AABB 半轴 = (500+400)·√2/2 ≈ 636.396，
    /// 比±500 宽——不扩宽的话斜视时边缘块会被误判视野外。
    #[test]
    fn viewport_world_aabb_widens_for_camera_rotation() {
        let rot = Quat::from_rotation_z(std::f32::consts::FRAC_PI_4);
        let (min, _) = viewport_world_aabb(Vec2::ZERO, 1.0, Vec2::new(1000.0, 800.0), rot);
        let expected = 900.0 * std::f32::consts::FRAC_1_SQRT_2 + CHUNK_SIZE as f32;
        assert!(
            (min.x - -expected).abs() < 1e-2,
            "got {} want {}",
            min.x,
            -expected
        );
        // y 半轴同式：|sin| 与 |cos| 在 45° 相等。
        let (_, max) = viewport_world_aabb(Vec2::ZERO, 1.0, Vec2::new(1000.0, 800.0), rot);
        assert!((max.y - expected).abs() < 1e-2, "got {}", max.y);
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
    fn downsample_box_averages_2x2_pixels() {
        // 2×2 源 → 1×1：R = (100+200)/4 = 75，G = (100+200)/4 = 75。
        let src: Vec<u8> = vec![
            100, 0, 0, 255, //
            200, 0, 0, 255, //
            0, 100, 0, 255, //
            0, 200, 0, 255,
        ];
        let (out, w, h) = downsample_box(&src, 2, 2);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![75, 75, 0, 255]);
    }

    #[test]
    fn downsample_box_halves_dimensions_with_floor() {
        // 偶数尺寸对半。
        let src = vec![7u8; (4 * 2 * 4) as usize];
        let (_, w, h) = downsample_box(&src, 4, 2);
        assert_eq!((w, h), (2, 1));
        // 奇数尺寸向下取整，但至少为 1。
        let src3 = vec![7u8; (3 * 3 * 4) as usize];
        let (_, w3, h3) = downsample_box(&src3, 3, 3);
        assert_eq!((w3, h3), (1, 1));
    }

    #[test]
    fn chunk_sampler_follows_requested_filter_mode() {
        use bevy::image::ImageFilterMode;
        let pixels = vec![255u8; 4];
        let linear = build_chunk_image(pixels.clone(), 1, 1, true).sampler;
        let nearest = build_chunk_image(pixels, 1, 1, false).sampler;
        let is_linear = matches!(
            linear,
            bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                mag_filter: ImageFilterMode::Linear,
                ..
            })
        );
        let is_nearest = matches!(
            nearest,
            bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
                mag_filter: ImageFilterMode::Nearest,
                ..
            })
        );
        assert!(is_linear, "Google chunk 应使用线性采样");
        assert!(is_nearest, "分析视图 chunk 应使用最近邻采样");
    }

    #[test]
    fn build_mip_chain_produces_halving_levels() {
        let pixels = vec![128u8; (8 * 8 * 4) as usize];
        let chain = build_mip_chain(pixels, 8, 8, false);
        assert_eq!(chain.len(), MIP_LEVELS as usize);
        assert_eq!((chain[0].width(), chain[0].height()), (8, 8));
        assert_eq!((chain[1].width(), chain[1].height()), (4, 4));
        assert_eq!((chain[2].width(), chain[2].height()), (2, 2));
    }

    #[test]
    fn patch_chunk_mips_updates_only_intersection() {
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<ColorMaterial>::default();
        // 整图 4×4（res=1）：R = x*10，G = y*10。
        let size = 4u32;
        let mut src = vec![0u8; (4 * 4 * 4) as usize];
        for y in 0..4usize {
            for x in 0..4usize {
                let i = (y * 4 + x) * 4;
                src[i] = (x * 10) as u8;
                src[i + 1] = (y * 10) as u8;
                src[i + 3] = 255;
            }
        }
        // chunk 覆盖整图 [0,0,4,4]，mip0 初始全 0。
        let mip_handles = build_mip_chain(vec![0u8; 4 * 4 * 4], 4, 4, false)
            .into_iter()
            .map(|img| images.add(img))
            .collect::<Vec<_>>();
        let chunk = TerrainChunk {
            _coord: [0, 0],
            current_mip: 0,
            mip_handles,
            material: materials.add(ColorMaterial::default()),
            linear_filter: false,
        };
        // dirty = (1,1)-(3,3)：只 patch 像素 x∈{1,2}, y∈{1,2}。
        patch_chunk_mips(
            &mut images,
            &chunk,
            &src,
            size,
            1,
            [0, 0, 4, 4],
            [1, 1, 3, 3],
        );
        let mip0 = images.get(&chunk.mip_handles[0]).unwrap();
        // 交集内 (2,2) → (20, 20)。
        let o = (2 * 4 + 2) * 4;
        assert_eq!(
            &mip0.data.as_ref().expect("mip 像素数据")[o..o + 2],
            &[20, 20]
        );
        // 交集外 (0,0) 与 (3,3) 仍是 0。
        assert_eq!(&mip0.data.as_ref().expect("mip 像素数据")[0..2], &[0, 0]);
        let o33 = (3 * 4 + 3) * 4;
        assert_eq!(
            &mip0.data.as_ref().expect("mip 像素数据")[o33..o33 + 2],
            &[0, 0]
        );
    }

    #[test]
    fn patch_chunk_mips_rebuilds_lower_mips_from_mip0() {
        let mut images = Assets::<Image>::default();
        let mut materials = Assets::<ColorMaterial>::default();
        // 整图 4×4（res=1）：R = x*10，G = y*10。
        let size = 4u32;
        let mut src = vec![0u8; (4 * 4 * 4) as usize];
        for y in 0..4usize {
            for x in 0..4usize {
                let i = (y * 4 + x) * 4;
                src[i] = (x * 10) as u8;
                src[i + 1] = (y * 10) as u8;
                src[i + 3] = 255;
            }
        }
        let mip_handles = build_mip_chain(vec![0u8; 4 * 4 * 4], 4, 4, false)
            .into_iter()
            .map(|img| images.add(img))
            .collect::<Vec<_>>();
        let chunk = TerrainChunk {
            _coord: [0, 0],
            current_mip: 0,
            mip_handles,
            material: materials.add(ColorMaterial::default()),
            linear_filter: false,
        };
        // 整块 dirty → mip0 变成 src 的完整拷贝。
        patch_chunk_mips(
            &mut images,
            &chunk,
            &src,
            size,
            1,
            [0, 0, 4, 4],
            [0, 0, 4, 4],
        );
        let mip0 = images.get(&chunk.mip_handles[0]).unwrap();
        assert_eq!(mip0.data.as_ref().expect("mip 像素数据"), &src);
        // mip1（2×2）左上 = mip0 (0..2, 0..2) 平均：R = (0+10+0+10)/4 = 5，G = (0+0+10+10)/4 = 5。
        let mip1 = images.get(&chunk.mip_handles[1]).unwrap();
        assert_eq!((mip1.width(), mip1.height()), (2, 2));
        assert_eq!(&mip1.data.as_ref().expect("mip 像素数据")[0..2], &[5, 5]);
        // mip1 右下 = mip0 (2..4, 2..4) 平均：R = (20+30+20+30)/4 = 25，G = (20+20+30+30)/4 = 25。
        // (row*2 + col) 的网格语义：第 1 行第 1 列的左上像素。
        #[allow(clippy::identity_op)]
        let o = (1 * 2 + 1) * 4;
        assert_eq!(
            &mip1.data.as_ref().expect("mip 像素数据")[o..o + 2],
            &[25, 25]
        );
        // mip2（1×1）= mip1 全图平均：R = (5+15+15+25)/4 = 15，G 同理 = 15。
        let mip2 = images.get(&chunk.mip_handles[2]).unwrap();
        assert_eq!((mip2.width(), mip2.height()), (1, 1));
        assert_eq!(&mip2.data.as_ref().expect("mip 像素数据")[0..2], &[15, 15]);
    }

    #[test]
    fn terrain_chunk_entities_default_is_empty() {
        let e = TerrainChunkEntities::default();
        assert!(e.by_coord.is_empty());
    }
}
