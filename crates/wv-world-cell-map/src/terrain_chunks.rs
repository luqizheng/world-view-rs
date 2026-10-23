//! 地形分块 + mip 链 + 视口拣选 + 脏矩形驱动（S3 优化项 3a + 3b + 3c + 3e）。
//!
//! ## 职责
//! - 把整张地形切成 N×N 的图块，每块持有 mip 链，屏幕恒定 1.5px 描边。
//! - 视口拣选：只 sync 当前视口内可见的 chunk；视口外的帧早退，省 fillrate。
//! - 脏矩形驱动：`dirty_chunks` 标记哪些 chunk 在编辑后需要重烘，其它早退。
//!
//! ## 边界（不做什么）
//! - **不画风格**（JRPG 图集 / Google / 古舆图色板）——单图块内部色值由
//!   [`crate::theme::palette`] + [`crate::terrain_view`] 决定；
//!   本模块只关心几何 / mip / 拣选。
//! - **不直接听编辑输入**——笔触收口在 wv-app 的编辑链；本模块订阅
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
//!   按 **mip0 单 texel 的屏幕尺寸**选级（[`select_mip`]）：
//!   - texel ≥ 1px（放大 / 1:1）：mip0（原分辨率，锐利）；
//!   - 缩小到 texel < 1px：降 mip1 / mip2（盒式降采样代替点采样，消摩尔纹——
//!     nearest 直缩 4× 时 3/4 的 texel 被随机丢弃，低对比填充会散成噪点）。
//!     切换只换材质的纹理句柄，几何不动。
//!
//!   ⚠️ 2026-10-15 修正：旧实现按 `view.scale` 常数阈值 `[0.5, 0.25]` 选级，
//!   是按「scale 大 = 近看」的旧语义写的，与现行「scale = 世界单位/像素」
//!   （ADR-0008 §8，相机 / 雾霭 / 线宽同一口径）**方向相反**——远观该降采样时
//!   恒用 mip0，近看放大时反而误用模糊的 mip1/mip2。现按 texel 实际屏幕尺寸
//!   选级，阈值随 `cell_size / res` 自然缩放，不再依赖语义记忆。
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

use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// 块边长（逻辑格）。基线 §2.11：分块 256×256 上传。
pub const CHUNK_SIZE: u32 = 256;
/// mip 链级数（mip0 + mip1 + mip2）。3 级 = 256 / 128 / 64。
const MIP_LEVELS: u32 = 3;
/// 视口外的"边距块数"——避免边缘瞬移/锯齿。
const VIEWPORT_MARGIN_CHUNKS: u32 = 1;
/// 地形在 Z 轴的偏移：在所有矢量层之下。
pub const TERRAIN_Z: f32 = 0.0;

/// 一个 chunk 的状态：Mesh2d 实体 + 各级 mip 的 `Image` 句柄 + 材质句柄。
///
/// 复用机制：稳态每个可见 chunk 有 1 个实体 + `MIP_LEVELS` 个 Image 句柄 + 1 个材质。
/// 几何（mesh）对固定坐标不变，因此只在 spawn 时建一次；mip 切换只改材质纹理。
#[derive(Component)]
pub struct TerrainChunk {
    /// 块在原图的逻辑索引（`[cx, cy]`，单位 = 块数）。仅诊断用。
    pub _coord: [u32; 2],
    /// 当前选中的 mip 级（0..MIP_LEVELS）。随 `view.scale` 变化。
    pub current_mip: u32,
    /// 各级 mip 的 `Image` 句柄（`mip_handles[i]` 是 mip_i 的 Image）。
    /// 块创建时一次性建好；笔刷更新时 patch mip0 并重降采样 mip1/mip2。
    pub mip_handles: Vec<Handle<Image>>,
    /// 材质句柄——切 mip 时原地改它的 `texture` 字段，不新建材质资产。
    pub material: Handle<ColorMaterial>,
    /// 是否使用线性采样。Google 式底图需要平滑边界；专题色块图保持锐利。
    pub linear_filter: bool,
}

/// 资源：当前所有 chunk 实体。
///
/// 用 `HashMap<(cx, cy), Entity>` 而不是 `Vec`——拣选是按"块的逻辑坐标"
/// 查"是否已存在"，O(1) 哈希查找远比线性扫描合适。
///
/// ## 计数器（2026-10-23 · QA 断言口径，跨机器同值）
///
/// 本轮把 `sync_terrain_chunks` 的「每帧克隆整图源像素（lod-demo 4096²×4B ≈ 67MB）」
/// 拆成「相 A 只读拷贝 + 相 B 写资产」，并给 spawn 加了每帧预算。下列计数器就是
/// 那两条的判据：
#[derive(Resource, Default)]
pub struct TerrainChunkEntities {
    /// 块坐标 → 实体。`coord` 是 `[cx, cy]`。
    pub by_coord: std::collections::HashMap<[u32; 2], Entity>,
    /// **整图克隆次数**——本轮目标：恒为 0（改前每帧一次 ≈ 67MB）。
    pub src_clones: u64,
    /// 从源图里**拷出的字节数**（只算 spawn 的整块 + patch 的脏矩形交集，
    /// 不含 mip 重降采样）。用来抓「偷偷又拷了整图」。
    pub bytes_copied: u64,
    /// 新 spawn 的块数（累计）。
    pub chunk_spawns: u64,
    /// 因每帧预算被推迟到下一帧的块数（累计）。
    pub spawn_deferred: u64,
    /// dirty 补丁次数（累计，按块计）。
    pub dirty_patches: u64,
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
pub fn chunk_grid(width: u32, height: u32) -> [u32; 2] {
    [width.div_ceil(CHUNK_SIZE), height.div_ceil(CHUNK_SIZE)]
}

/// 块的世界矩形（按块内实际格数 × cell_size）；非方形地图的尾块不是正方形。
pub fn chunk_world_rect(cells: [u32; 4], cell_size: f32) -> Vec2 {
    Vec2::new(
        (cells[2] - cells[0]) as f32 * cell_size,
        (cells[3] - cells[1]) as f32 * cell_size,
    )
}

/// 块在原图**逻辑格**坐标的矩形（半开 `[x0, y0, x1, y1]`，钳到 width/height 内）。
///
/// 单位 = 逻辑格。**像素坐标** = 格 × `tex.res`。
pub fn chunk_cell_rect(coord: [u32; 2], width: u32, height: u32) -> [u32; 4] {
    let x0 = coord[0] * CHUNK_SIZE;
    let y0 = coord[1] * CHUNK_SIZE;
    let x1 = ((coord[0] + 1) * CHUNK_SIZE).min(width);
    let y1 = ((coord[1] + 1) * CHUNK_SIZE).min(height);
    [x0, y0, x1, y1]
}

/// 选 mip 级（0 = 最高分辨率），依据是 **mip0 单 texel 的屏幕尺寸**。
///
/// `texel_world` = mip0 单 texel 的世界边长（`cell_size / res`）；其屏幕尺寸 =
/// `texel_world / scale`（scale = 世界单位/像素，ADR-0008 §8）。texel 还 ≥ 1px
/// （放大或 1:1）就用 mip0；一旦缩小到 < 1px，逐级降 mip 让 texel 翻倍回
/// ~1px——盒式预降采样代替采样时的点丢弃，这正是 mip 存在的目的。
///
/// ⚠️ 2026-10-15 修正：旧版用常数阈值 `[0.5, 0.25]` 比较 `scale`，按的是
/// 「scale 大 = 近看」的旧语义，与现行 world-per-px 口径方向相反（远观摩尔纹、
/// 近看误糊）。改按 texel 尺寸后阈值随纹理分辨率自然缩放，语义不再可反装。
pub fn select_mip(scale: f32, texel_world: f32) -> u32 {
    let mut mip = 0u32;
    let mut texel = texel_world.max(1e-6);
    while texel < scale && mip < MIP_LEVELS - 1 {
        texel *= 2.0;
        mip += 1;
    }
    mip
}

/// 块坐标是否已超出当前块网格（换到更小的图后，旧图的尾块必须强制清掉）。
pub fn chunk_out_of_grid(coord: [u32; 2], grid: [u32; 2]) -> bool {
    coord[0] >= grid[0] || coord[1] >= grid[1]
}

/// 两个半开矩形（`[x0, y0, x1, y1]`）是否相交。边贴边不算相交。
pub fn rects_intersect(a: [u32; 4], b: [u32; 4]) -> bool {
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
pub fn chunk_visible(
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
pub fn despawn_chunk(
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

/// 从源 RGBA8 像素缓冲中复制 `(gx0,gy0)..(gx1,gy1)` 矩形到新缓冲（按逻辑格）。
///
/// `src` 行序 = 顶行在前，与 `Image::new` 约定一致。
/// 源像素尺寸 = `size × res`（每边），src_stride = `size * res * 4`。
/// 输出 = 矩形对应的像素子集（`((gx1-gx0) × res) × ((gy1-gy0) × res)`）。
pub fn copy_chunk_pixels(
    src: &[u8],
    src_width: u32,
    res: u32,
    gx0: u32,
    gy0: u32,
    gx1: u32,
    gy1: u32,
) -> Vec<u8> {
    let out_w = (gx1 - gx0) * res;
    let out_h = (gy1 - gy0) * res;
    let mut out = vec![0u8; (out_w * out_h * 4) as usize];
    let src_pixel_size = src_width * res;
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
pub fn build_mip_chain(pixels: Vec<u8>, w: u32, h: u32, linear_filter: bool) -> Vec<Image> {
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
///
/// **2026-10-23**：本函数保留为「一步到位」的参考实现（等价性测试的旧路径），
/// 生产路径已改成两相 [`copy_patch_pixels`]（只读）+ [`apply_chunk_patch`]（写），
/// 以便在不克隆整图的前提下写 `Assets<Image>`。
#[allow(clippy::too_many_arguments)]
pub fn patch_chunk_mips(
    images: &mut Assets<Image>,
    chunk: &TerrainChunk,
    src: &[u8],
    src_width: u32,
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
    let src_stride = (src_width * res) as usize * 4;
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

/// 一个块的**补丁像素包**（相 A 的产物）：数据 + 它在块 mip0 内的像素矩形。
///
/// 为什么单独成型：生产路径要在「不可变借用整图源像素」的同一段里把所有块的
/// 补丁都拷出来，再放开借用去写 `Assets<Image>`（否则只能克隆整图绕开借用冲突，
/// 那正是每帧 67MB 的来源）。
#[derive(Debug, Clone)]
pub struct ChunkPatch {
    pub data: Vec<u8>,
    /// 补丁像素宽高。
    pub w: u32,
    pub h: u32,
    /// 补丁左上角在**块 mip0 像素坐标**里的位置。
    pub x0: u32,
    pub y0: u32,
}

/// 相 A：从源图拷出「本块 ∩ dirty」那一小块像素（**只读**，不动资产）。
///
/// 与 [`patch_chunk_mips`] 的区别：不碰 `Assets<Image>`，因此可以在借用
/// 整图源像素的时候一次性为所有块备料；返回 `None` 表示交集为空。
pub fn copy_patch_pixels(
    src: &[u8],
    src_width: u32,
    res: u32,
    cells: [u32; 4],
    dirty: [u32; 4],
) -> Option<ChunkPatch> {
    let [gx0, gy0, gx1, gy1] = cells;
    let ix0 = dirty[0].max(gx0);
    let iy0 = dirty[1].max(gy0);
    let ix1 = dirty[2].min(gx1);
    let iy1 = dirty[3].min(gy1);
    if ix0 >= ix1 || iy0 >= iy1 {
        return None;
    }
    let (pw, ph) = ((ix1 - ix0) * res, (iy1 - iy0) * res);
    let src_stride = (src_width * res) as usize * 4;
    let mut data = vec![0u8; (pw * ph * 4) as usize];
    let row_len = pw as usize * 4;
    for row in 0..ph as usize {
        let src_y = (iy0 * res) as usize + row;
        let s = src_y * src_stride + (ix0 * res) as usize * 4;
        let d = row * row_len;
        data[d..d + row_len].copy_from_slice(&src[s..s + row_len]);
    }
    Some(ChunkPatch {
        data,
        w: pw,
        h: ph,
        x0: (ix0 - gx0) * res,
        y0: (iy0 - gy0) * res,
    })
}

/// 相 B：把补丁写进本块 mip0，并从 patch 后的 mip0 逐级重建 mip1/mip2。
///
/// 与 [`patch_chunk_mips`] 的结果**逐字节等价**（见 `patch_paths_agree_byte_for_byte`）：
/// 只是把「拷像素」与「写资产」拆成两段，好让调用方在中间放开整图借用来 `add` 新块。
pub fn apply_chunk_patch(images: &mut Assets<Image>, chunk: &TerrainChunk, patch: &ChunkPatch) {
    let Some(mip0_handle) = chunk.mip_handles.first() else {
        return;
    };
    let Some((chunk_w_px, chunk_h_px)) = images.get(mip0_handle).map(|m| {
        (
            m.texture_descriptor.size.width,
            m.texture_descriptor.size.height,
        )
    }) else {
        return;
    };
    let (mut px, mut w, mut h) = {
        let Some(mut mip0) = images.get_mut(mip0_handle) else {
            return;
        };
        let Some(buf) = mip0.data.as_mut() else {
            return;
        };
        let row_len = patch.w as usize * 4;
        let dst_stride = chunk_w_px as usize * 4;
        for row in 0..patch.h as usize {
            let d = (patch.y0 as usize + row) * dst_stride + patch.x0 as usize * 4;
            let s = row * row_len;
            if d + row_len > buf.len() || s + row_len > patch.data.len() {
                break;
            }
            buf[d..d + row_len].copy_from_slice(&patch.data[s..s + row_len]);
        }
        (
            mip0.data.clone().unwrap_or_default(),
            chunk_w_px,
            chunk_h_px,
        )
    };
    // 与 `patch_chunk_mips` 同款：`AssetMut` 的借用活到析构，必须显式结束，
    // 才能在同一帧里对本资产表再次 `get_mut`（上面的块表达式已结束借用）。
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
        assert_eq!(chunk_grid(256, 256), [1, 1]);
        // 512 = 整除 → 2 块
        assert_eq!(chunk_grid(512, 512), [2, 2]);
        // 1024 = 4 块
        assert_eq!(chunk_grid(1024, 1024), [4, 4]);
        // 1025 = 5 块（向上舍入）
        assert_eq!(chunk_grid(1025, 1025), [5, 5]);
        // 200 = 1 块（不足一块占一整块）
        assert_eq!(chunk_grid(200, 200), [1, 1]);
        // 4096 = 16 块
        assert_eq!(chunk_grid(4096, 4096), [16, 16]);
    }

    #[test]
    fn chunk_cell_rect_within_terrain_bounds() {
        // size = 512（逻辑格），分 2 块。
        let r00 = chunk_cell_rect([0, 0], 512, 512);
        assert_eq!(r00, [0, 0, 256, 256]);
        let r11 = chunk_cell_rect([1, 1], 512, 512);
        assert_eq!(r11, [256, 256, 512, 512]);
        // size = 500，分 2 块但第二块尾部被钳到 500。
        let r10 = chunk_cell_rect([1, 0], 500, 500);
        assert_eq!(r10, [256, 0, 500, 256]);
    }

    /// texel 被放大或 1:1 时恒用 mip0（最细分辨率）——近看的锐利不许被 mip 吃掉。
    /// 512 格图（res 8 → texel 1 世界单位）：内界 0.16（ADR-0038 的 32× 上限）到
    /// 1:1000 全段 mip0。
    #[test]
    fn select_mip_stays_finest_while_texel_is_magnified() {
        assert_eq!(select_mip(0.16, 1.0), 0, "内界深度放大应 mip0");
        assert_eq!(select_mip(0.5, 1.0), 0, "2px/texel 放大应 mip0");
        assert_eq!(select_mip(1.0, 1.0), 0, "1:1 恰好 1px/texel 应 mip0");
    }

    /// 缩小到 texel < 1px 才逐级降级：1.5 → mip1，2.9（用户截图的 1:345 档）
    /// → mip2；越远钳在末级。修正前这段恒返回 mip0（阈值方向反装），
    /// nearest 直缩把低对比填充散成噪点。
    #[test]
    fn select_mip_steps_down_only_when_the_view_minifies() {
        assert_eq!(select_mip(1.5, 1.0), 1, "texel 缩到 0.67px 应降 mip1");
        assert_eq!(select_mip(2.9, 1.0), 2, "1:345 远观应降 mip2");
        assert_eq!(select_mip(99.0, 1.0), 2, "越远应钳在末级");
    }

    /// texel 尺寸随纹理分辨率缩放：1024 格图 res=4 → texel 2，降级点整体右移；
    /// 4096 格图 res=1 → texel 8，外界 8.0 恰好 1px/texel，全程不降级。
    #[test]
    fn select_mip_adapts_to_texture_resolution() {
        assert_eq!(select_mip(1.0, 2.0), 0, "1024 格图 1:1000 应 mip0");
        assert_eq!(select_mip(2.9, 2.0), 1, "1024 格图 1:345 应 mip1");
        assert_eq!(
            select_mip(8.0, 8.0),
            0,
            "4096 格图最远端恰 1px/texel 应 mip0"
        );
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

    /// 16:9 地图（512×288 格）：块网格与块矩形按真实宽高切，不再按单边方形。
    #[test]
    fn non_square_maps_get_two_dimensional_chunk_grid() {
        assert_eq!(chunk_grid(512, 288), [2, 2], "512×288 应为 2×2 块");
        assert_eq!(
            chunk_cell_rect([1, 1], 512, 288),
            [256, 256, 512, 288],
            "右下块的高必须钳到 288（旧实现会取到 512 → 越界）"
        );
        assert_eq!(chunk_grid(128, 72), [1, 1]);
        assert_eq!(chunk_cell_rect([0, 0], 128, 72), [0, 0, 128, 72]);
    }

    /// 尾块的世界矩形按实际格数（右下 32 格高的块不得铺成正方形）。
    #[test]
    fn edge_chunks_use_their_true_world_rect() {
        let full = chunk_world_rect([0, 0, 256, 256], 8.0);
        assert_eq!(full, Vec2::splat(2048.0), "整块 256×256 格");
        let edge = chunk_world_rect([256, 256, 512, 288], 8.0);
        assert_eq!(
            edge,
            Vec2::new(2048.0, 256.0),
            "16:9 右下尾块 = 256×32 格 = 2048×256 wu，得 {edge:?}"
        );
    }

    /// 换到更小的图：旧图超出新块网格的坐标必须被判定为待清理
    /// （否则它们会以空矩形留在视口里，把旧像素留在画面上）。
    #[test]
    fn chunks_outside_the_new_grid_are_evicted() {
        let new_grid = chunk_grid(512, 288); // (2,2)
        assert!(chunk_out_of_grid([2, 0], new_grid), "右侧旧块应清");
        assert!(chunk_out_of_grid([0, 2], new_grid), "下侧旧块应清");
        assert!(chunk_out_of_grid([3, 3], new_grid), "旧图角块应清");
        assert!(!chunk_out_of_grid([1, 1], new_grid), "新网格内应保留");
    }

    /// 16:9 地图的**每一块**都能从源缓冲拷出（旧实现在右下块越界 panic）。
    #[test]
    fn every_chunk_of_a_16_9_map_copies_without_panic() {
        let (w, h) = (512u32, 288u32);
        let src = vec![7u8; (w * h * 4) as usize];
        let [cw, ch] = chunk_grid(w, h);
        assert_eq!([cw, ch], [2, 2]);
        for cy in 0..ch {
            for cx in 0..cw {
                let cells = chunk_cell_rect([cx, cy], w, h);
                let px = copy_chunk_pixels(&src, w, 1, cells[0], cells[1], cells[2], cells[3]);
                let want = ((cells[2] - cells[0]) * (cells[3] - cells[1]) * 4) as usize;
                assert_eq!(px.len(), want, "块 {cx},{cy} 像素数");
            }
        }
    }

    /// 非方形源缓冲：copy 的 stride 必须用**宽**而不是高，否则读越界/取错行。
    #[test]
    fn copy_chunk_pixels_uses_width_stride_for_non_square_sources() {
        // 4×3 源，每像素 R = 线性索引，便于手算行内容。
        let (w, h) = (4u32, 3u32);
        let src: Vec<u8> = (0..(w * h)).flat_map(|i| [i as u8, 0, 0, 255]).collect();
        let out = copy_chunk_pixels(&src, w, 1, 2, 0, 4, 3);
        assert_eq!(out.len(), 2 * 3 * 4, "2×3 格 × 4 字节");
        assert_eq!(
            &out[0..8],
            &[2, 0, 0, 255, 3, 0, 0, 255],
            "第一行 = 源 (2,0),(3,0)"
        );
        assert_eq!(
            &out[8..16],
            &[6, 0, 0, 255, 7, 0, 0, 255],
            "第二行 = 源 (2,1),(3,1)"
        );
        assert_eq!(
            &out[16..24],
            &[10, 0, 0, 255, 11, 0, 0, 255],
            "第三行 = 源 (2,2),(3,2)"
        );
    }
}
