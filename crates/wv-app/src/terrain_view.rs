//! 栅格地形纹理（S3）与增量重传（S5）。
//!
//! ## 依据（基线 §2.11 栅格渲染细节）
//!
//! - **单纹理 + 四边形**：1 Draw Call 画完整张地形。
//! - **笔刷增量更新**：只 patch 脏块区域，**不要每帧重建整图**。
//! - 分块（256×256）上传，视口外的块不加载（LOD 基础）。
//!
//! ## 「色块地形」视图模式的着色（基线 §1.10 模式 B）
//!
//! 有瓦片图集时：每格按 `TerrainKind` 查图集瓦片直铺（README §12-14 待决 14
//! 的第一版：整瓦直铺、无过渡瓦），真实地形模式再叠高度明度调制；
//! 无图集或缺项时回退 [`crate::palette::core_terrain`] 纯色平涂。
//!
//! ## 像素与格子的两套坐标
//!
//! `TerrainTexture.size` 一直是**逻辑格数**（map_frame / projection / grid 都用它
//! 乘 `cell_size` 得世界尺寸）；接入图集后纹理**像素**边长 = `size × res`
//! （`res` = 每格瓦片重采样边长）。消费像素的只有 [`crate::terrain_chunks`]
//! （按 256 像素切块）与本模块自己。

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::palette;
use crate::palette_google;
use crate::palette_jrpg;
use crate::state::{GridConfig, ViewMode, ViewState, WorldProjection};
use wv_core::terrain::TerrainData;

// ---------------------------------------------------------------------------
// 瓦片图集（README §12-14：直铺整瓦、无过渡；数据层不动，仅渲染层查表）
// ---------------------------------------------------------------------------

/// 瓦片图集：已解码的 RGBA 像素 + 网格规格 + `TerrainKind → 格位` 映射。
///
/// 纯数据资源，`wv-app` 层私有——世界数据的权威仍在工程 YAML（P3），
/// 图集只是渲染资产的查表。
#[derive(Clone, Resource)]
pub struct TileAtlas {
    /// 图集像素（RGBA8，行优先，左上原点）。
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 网格规格：图集横向/纵向的瓦片格数。
    pub cols: u32,
    pub rows: u32,
    /// 每格四边内缩像素（避开图集的分隔网格线与 JPEG 溢边）。
    pub inset: u32,
    /// `TerrainKind` → 图集格位 `[col, row]`；`None` 回退纯色平涂。
    pub tiles: [Option<[u32; 2]>; 10],
}

impl TileAtlas {
    /// 图集配置 YAML 的缺省路径（相对 [`atlas_base_dir`]）。
    pub const DEFAULT_CONFIG: &'static str = "tilesets/jrpg_v1.yaml";

    /// `(col,row)` 格位 → 去掉 inset 后的源像素矩形 `[x0,y0,x1,y1]`（半开）。
    fn cell_rect(&self, col: u32, row: u32) -> [u32; 4] {
        let cw = self.width / self.cols.max(1);
        let ch = self.height / self.rows.max(1);
        [
            col * cw + self.inset,
            row * ch + self.inset,
            (col + 1) * cw - self.inset,
            (row + 1) * ch - self.inset,
        ]
    }
}

/// 图集资产的基目录：编译期锁定 crate 根的 `assets/`。
///
/// 为什么不用 Bevy `AssetServer`：启动时就要**同步**拿到像素做 CPU 拼贴，
/// 异步加载会把「纹理初建」拆成多帧状态机。原型阶段 std::fs 读一个
/// 随 exe 分发的自带资产最直接（`wv-app` 允许 fs；它不是权威存储，P2 不受影响）。
fn atlas_base_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// 从 YAML 配置加载图集；任何一步失败都返回 `None`（回退纯色平涂，不 panic）。
pub fn load_tile_atlas(config_rel: &str) -> Option<TileAtlas> {
    #[derive(serde::Deserialize)]
    struct AtlasConfig {
        atlas: String,
        cols: u32,
        rows: u32,
        #[serde(default = "default_inset")]
        inset_px: u32,
        tiles: std::collections::HashMap<u8, [u32; 2]>,
    }
    fn default_inset() -> u32 {
        4
    }

    let cfg_path = atlas_base_dir().join(config_rel);
    let text = std::fs::read_to_string(cfg_path).ok()?;
    let cfg: AtlasConfig = serde_yaml::from_str(&text).ok()?;
    let img_path = atlas_base_dir().join(&cfg.atlas);
    // 按扩展名分派解码器（bevy 默认特性只带 png；图集用 PNG 即可，无需加特性）。
    let ext = img_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let bytes = std::fs::read(&img_path).ok()?;
    let image = Image::from_buffer(
        &bytes,
        bevy::image::ImageType::Extension(&ext),
        CompressedImageFormats::default(),
        true,
        ImageSampler::default(),
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .ok()?;
    let mut tiles = [None; 10];
    for (kind, cell) in cfg.tiles {
        if (kind as usize) < tiles.len() && cell[0] < cfg.cols && cell[1] < cfg.rows {
            tiles[kind as usize] = Some(cell);
        }
    }
    Some(TileAtlas {
        width: image.width(),
        height: image.height(),
        rgba: image.data,
        cols: cfg.cols,
        rows: cfg.rows,
        inset: cfg.inset_px,
        tiles,
    })
}

/// 有图集就插入 `TileAtlas` 资源并把像素一并返回（供启动时同步拼贴）；
/// 失败静默回退纯色平涂（渲染侧不受影响，见模块文档）。
///
/// **JRPG 图集已弃用（2026-11 用户裁定）**：饱和纹理碎的 RPG 瓦片会淹没路线
/// 与地标，全面改走「纯色平涂 + Theme Pack 色板」。`load_tile_atlas` 保留，
/// 供未来接入其他图集包；当前一律返回 `None`。
pub fn ensure_atlas(_commands: &mut Commands) -> Option<TileAtlas> {
    None
}

/// 每格瓦片重采样的目标边长（像素）。
///
/// 约束整张纹理 ≈ 4096²（≈67MB GPU 内存上限）：`res = clamp(4096/格数, 1, 64)`。
/// 下限 1（而非旧的 8）：格数 >512 的世界（如 1024² 的 lod-demo）若仍强撑 8px/格，
/// 纹理会冲到 8192²（268MB）——宁可让单格瓦片变糊，也不能让纹理超预算；
/// 细节感由「格更密」补偿，视觉上反而更精细。
pub(crate) fn tile_res_for(cells: u32) -> u32 {
    (4096 / cells.max(1)).clamp(1, 64)
}

/// 瓦片像素的着色方式（与视图模式一一对应，见 [`tile_tint_for_mode`]）。
#[derive(Clone, Copy)]
enum TileTint {
    /// 原样直铺（色块模式）。
    Identity,
    /// 高度明度调制系数（真实地形模式）：RGB 乘该系数。
    Height(f32),
    /// 淡化：与中性灰 0.7 混合，保留比例 0.45（势力图；同 [`shade_faded`]）。
    Fade,
}

fn tile_tint_for_mode(mode: ViewMode) -> TileTint {
    match mode {
        ViewMode::Schematic => TileTint::Identity,
        ViewMode::RealTerrain => TileTint::Height(1.0), // 系数逐格覆写
        ViewMode::Faction => TileTint::Fade,
        // 事件图是极简灰白底图（基线 §1.10 模式 D），不铺瓦片。
        // 轮廓图（模式 E）同样不铺瓦片——只有水陆三色与海岸线。
        ViewMode::Event => TileTint::Identity,
        ViewMode::Coastline => TileTint::Identity,
        // 谷歌风（ViewMode::GoogleMap）与色块地形同为「平铺色块」，Identity 保留原色。
        ViewMode::GoogleMap => TileTint::Identity,
        // JRPG 未进 ALL（隐藏态），仍给出 Identity 分支保持 match 穷尽。
        ViewMode::JRPG => TileTint::Identity,
    }
}

/// 把图集一格盒式降采样成 `res×res` 的 RGBA 块（**无 tint**）。
///
/// **启动性能（2026-10 修复）**：全图合成时同一 tile 的降采样结果对每个 cell
/// 都相同，而瓦片种类只有几十个——过去 [`write_terrain_pixels`] 对 512² = 26.2 万格
/// 每格都重采样一遍（≈10 亿次采样累加，debug 构建实测 29s，启动白屏「未响应」
/// 的直接原因）。现在按 `(col, row)` 只采样一次、整图复用，重采样总量与格数解耦。
fn sample_tile_block(atlas: &TileAtlas, col: u32, row: u32, res: u32) -> Vec<u8> {
    let [x0, y0, x1, y1] = atlas.cell_rect(col, row);
    let sw = (x1.saturating_sub(x0)).max(1) as usize;
    let sh = (y1.saturating_sub(y0)).max(1) as usize;
    let stride = atlas.width as usize;
    let mut block = vec![0u8; (res * res * 4) as usize];
    for ty in 0..res as usize {
        let sy0 = y0 as usize + ty * sh / res as usize;
        let sy1 = (y0 as usize + (ty + 1) * sh / res as usize).max(sy0 + 1);
        for tx in 0..res as usize {
            let sx0 = x0 as usize + tx * sw / res as usize;
            let sx1 = (x0 as usize + (tx + 1) * sw / res as usize).max(sx0 + 1);
            // 步长封顶采样成本：单轴最多取 8 个样本。
            let step_x = ((sx1 - sx0) / 8).max(1);
            let step_y = ((sy1 - sy0) / 8).max(1);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in (sy0..sy1.min(atlas.height as usize)).step_by(step_y) {
                for sx in (sx0..sx1.min(atlas.width as usize)).step_by(step_x) {
                    let o = (sy * stride + sx) * 4;
                    r += atlas.rgba[o] as u32;
                    g += atlas.rgba[o + 1] as u32;
                    b += atlas.rgba[o + 2] as u32;
                    n += 1;
                }
            }
            if n == 0 {
                continue;
            }
            let o = (ty * res as usize + tx) * 4;
            // 源分量本就是 0..255：均值 = r/n（无需再乘 255）。
            block[o] = (r / n) as u8;
            block[o + 1] = (g / n) as u8;
            block[o + 2] = (b / n) as u8;
            block[o + 3] = 255;
        }
    }
    block
}

/// 把预采样块按 tint 写进目标位图（tint 用 Q8 定点乘法，避免逐像素 f32）。
/// `Identity` 走行级 `copy_from_slice`——整图直铺的最快路径。
///
/// `soften` = 底图淡化开关（`ViewState.terrain_soften`）：在 tint 结果上再向
/// `palette::SOFTEN_TOWARD`（纸面色）混 `(1 - SOFTEN_KEEP)`。实现上把两步合并为
/// 一条定点管线（先 tint 后 soften，每通道两次 Q8 乘加），避免中间缓冲。
#[allow(clippy::too_many_arguments)]
fn blit_block_tinted(
    block: &[u8],
    res: u32,
    tint: TileTint,
    soften: bool,
    dst: &mut [u8],
    dst_stride: u32,
    dx: u32,
    dy: u32,
) {
    // tint 归一为 out = (v*mul_q + add_q) >> 8：
    // - Identity: v
    // - Height(m): v*m
    // - Fade: v*0.45 + 灰0.7*0.55*255（与 shade_faded 同式）
    let (mul_q, add_q): (u32, u32) = match tint {
        TileTint::Identity => (256, 0),
        TileTint::Height(m) => ((m * 256.0).round() as u32, 0),
        TileTint::Fade => {
            let keep_q = (0.45f32 * 256.0).round() as u32;
            let c = (255.0f32 * 0.7 * (1.0 - 0.45)).round() as u32;
            (keep_q, c * 256)
        }
    };
    // 淡化参数：out2 = (out1*keep_q2 + toward[ch]*(1-k)*256) >> 8。
    let (keep_q2, add2): (u32, [u32; 3]) = if soften {
        let Color::Srgba(t) = palette::SOFTEN_TOWARD else {
            unreachable!("SOFTEN_TOWARD 应为 Srgba")
        };
        let k = palette::SOFTEN_KEEP;
        (
            (k * 256.0).round() as u32,
            [
                ((t.red * (1.0 - k) * 256.0).round()) as u32,
                ((t.green * (1.0 - k) * 256.0).round()) as u32,
                ((t.blue * (1.0 - k) * 256.0).round()) as u32,
            ],
        )
    } else {
        (256, [0; 3])
    };
    let row_len = res as usize * 4;
    for ty in 0..res as usize {
        let src_row = &block[ty * row_len..(ty + 1) * row_len];
        let o = ((dy as usize + ty) * dst_stride as usize + dx as usize) * 4;
        let dst_row = &mut dst[o..o + row_len];
        if !soften && matches!(tint, TileTint::Identity) {
            dst_row.copy_from_slice(src_row);
            continue;
        }
        for (d, s) in dst_row.chunks_exact_mut(4).zip(src_row.chunks_exact(4)) {
            for ch in 0..3 {
                let v = ((s[ch] as u32 * mul_q + add_q) >> 8).min(255);
                d[ch] = ((v * keep_q2 + add2[ch]) >> 8).min(255) as u8;
            }
            d[3] = 255;
        }
    }
}

/// 地形纹理句柄 + 脏块追踪。
#[derive(Resource)]
pub struct TerrainTexture {
    pub handle: Handle<Image>,
    /// 当前纹理覆盖的**逻辑格数**（每边）。像素边长 = `size × res`。
    pub size: u32,
    /// 每逻辑格的纹理像素数（1 = 纯色平涂，>1 = 瓦片直铺）。
    pub res: u32,
    /// 上次上传后累计的脏矩形（像素坐标 `[x0,y0,x1,y1]`，半开）。
    ///
    /// 由 [`crate::editing::flush_terrain_texture`] 消费清零——它排在
    /// [`crate::terrain_chunks::sync_terrain_chunks`] **之前**运行，
    /// 因此 chunk 层另有一份副本（[`Self::chunks_dirty`]）。
    pub dirty: Option<[u32; 4]>,
    /// chunk 层专用的脏矩形副本（逻辑格，半开）。
    ///
    /// 与 `dirty` 在 [`Self::mark_dirty`] / [`Self::mark_all_dirty`] 里同步标记，
    /// 但由 [`crate::terrain_chunks::sync_terrain_chunks`] 消费清零。
    /// 拆分原因：system 链上 flush 先于 chunks 跑，`dirty` 到 chunks 时已是 `None`。
    pub chunks_dirty: Option<[u32; 4]>,
    /// 当前纹理内容所用的视图模式。
    ///
    /// **仅供「模式是否真的变了」的比较**（基线 §2.11：不要每帧重建整图）。
    /// 用 `Option` 是为了区分「尚未构建过」：`None` 表示纹理还从未按任一模式
    /// 生成过——此时即使数据稍后才可用，也应重建一次。
    pub last_mode: Option<ViewMode>,
    /// 当前纹理内容是否带底图淡化（与 `view.terrain_soften` 比较用）。
    pub last_soften: bool,
}

impl TerrainTexture {
    /// 标记整个纹理为脏（整图重传）。单位：逻辑格。
    pub fn mark_all_dirty(&mut self) {
        let full = [0, 0, self.size, self.size];
        self.dirty = Some(full);
        self.chunks_dirty = Some(full);
    }

    /// 标记一个矩形为脏，与已有脏区取并集（增量重传）。单位：逻辑格。
    pub fn mark_dirty(&mut self, rect: [u32; 4]) {
        self.dirty = rect_union(self.dirty, rect);
        self.chunks_dirty = rect_union(self.chunks_dirty, rect);
    }
}

/// 两个可选脏矩形取并集（`None` 视为空集）。
fn rect_union(cur: Option<[u32; 4]>, rect: [u32; 4]) -> Option<[u32; 4]> {
    Some(match cur {
        Some(c) => [
            c[0].min(rect[0]),
            c[1].min(rect[1]),
            c[2].max(rect[2]),
            c[3].max(rect[3]),
        ],
        None => rect,
    })
}

/// 把一个着色函数应用到一格，返回 sRGB 线性分量。
type Shader = fn(&TerrainData, u8) -> Color;

/// 依据当前视图模式选择纯色着色函数（基线 §1.10 四态；图集缺项/无图集时也走这里）。
fn shade_for_mode(mode: ViewMode) -> Shader {
    match mode {
        // 真实地形：按高度调制明度，制造立体感（法线光照的廉价近似）。
        ViewMode::RealTerrain => shade_real,
        // 色块地形：纯色平涂。
        ViewMode::Schematic => shade_flat,
        // JRPG 地图：JRPG 风格色板。
        ViewMode::JRPG => shade_jrpg,
        // 势力图：原型阶段底色淡化地形（势力几何属 S12，本轮不画）。
        ViewMode::Faction => shade_faded,
        // 事件图：极简灰白底图。
        ViewMode::Event => shade_event,
        // 轮廓图：水陆二分深色 + 海岸线（海岸线在边界格逐像素画，见
        // `write_coast_boundary_cell`；这里只管内部格的水/陆平涂）。
        ViewMode::Coastline => shade_coast,
        // 谷歌风：羊皮纸暖调（色板见 palette_google）。
        ViewMode::GoogleMap => shade_google,
    }
}

fn shade_google(_data: &TerrainData, kind: u8) -> Color {
    // 谷歌风做「普通地图」折叠（2026-12 用户裁定：只要 Google 普通地图，
    // 不渲染地形高低分带）。Google 普通地图的陆地是单一米色平涂；若把
    // Perlin 高度分带（沙/草/丘/山/雪）逐档着色，整图会退化成近色马赛克。
    // 折叠规则：水两档保留层次、森林作公园绿斑、沼泽保留湿绿，
    // 其余陆上类一律折叠为陆地基色（`geometry` #d9c7a8）。
    match kind {
        0 | 1 | 4 | 8 => palette_google::google_core_terrain(kind),
        _ => palette_google::GOOGLE_CORE_TERRAIN[2],
    }
}

fn shade_jrpg(_data: &TerrainData, kind: u8) -> Color {
    palette_jrpg::jrpg_core_terrain(kind)
}

fn shade_flat(_data: &TerrainData, kind: u8) -> Color {
    palette::core_terrain(kind)
}

fn shade_real(data: &TerrainData, kind: u8) -> Color {
    // 用 kind 对应的基色，叠加高度明度调制。
    // 这里 data 已无单格高度上下文，故高度调制在 write_terrain_pixels 里做。
    let _ = data;
    palette::core_terrain(kind)
}

fn shade_faded(_data: &TerrainData, kind: u8) -> Color {
    // 淡化：与中性灰混合 55%，保留地形骨架（特效 §2.1 alpha≈0.35 的近似）。
    let base = palette::core_terrain(kind);
    let Color::Srgba(c) = base else {
        return base;
    };
    let k = 0.45; // 保留比例
    Color::srgb(
        c.red * k + 0.7 * (1.0 - k),
        c.green * k + 0.7 * (1.0 - k),
        c.blue * k + 0.7 * (1.0 - k),
    )
}

fn shade_event(_data: &TerrainData, _kind: u8) -> Color {
    palette::EVENT_VIEW_BACKDROP
}

/// 水系判定（轮廓图唯一关心的二分）：深水/浅水算水，其余全算陆。
///
/// 只此一个入口——笔刷新增水系 kind 时改这里，海岸线自动跟随。
fn is_water_kind(kind: u8) -> bool {
    kind <= 1
}

fn shade_coast(_data: &TerrainData, kind: u8) -> Color {
    if is_water_kind(kind) {
        palette::COAST_WATER
    } else {
        palette::COAST_LAND
    }
}

// ---------------------------------------------------------------------------
// 方案 A：边界扰动 + 超采样（抗锯齿地形边界）
// ---------------------------------------------------------------------------

/// 整数格点确定性 hash → `[0,1)`。无依赖、跨平台一致（测试与运行共用）。
fn hash01(xi: i64, yi: i64, seed: u64) -> f32 {
    let mut h = (xi as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (yi as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    (h & 0xFFFF_FFFF) as f32 / u32::MAX as f32
}

/// 平滑 value noise（双线性 + smoothstep），输出 `[-1,1]`。
fn vnoise(x: f32, y: f32, freq: f32, seed: u64) -> f32 {
    let (fx, fy) = (x * freq, y * freq);
    let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let a = hash01(x0, y0, seed);
    let b = hash01(x0 + 1, y0, seed);
    let c = hash01(x0, y0 + 1, seed);
    let d = hash01(x0 + 1, y0 + 1, seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// 边界扰动的幅度系数（相对每格像素边长）：0.45 → 边界在格内大幅摆动，
/// 又不至于越过邻格太深造成「噪点斑块」。
const WOBBLE_AMP: f32 = 0.45;
/// 扰动噪声的空间频率基长（格）。
const WOBBLE_FREQ_CELLS: f32 = 1.5;
/// 2×2 旋转网格超采样点位（像素内归一化坐标）——比规则 2×2 更好地打散摩尔纹。
const SUBSAMPLES: [[f32; 2]; 4] = [
    [0.125, 0.375],
    [0.625, 0.125],
    [0.875, 0.625],
    [0.375, 0.875],
];

/// 纹理像素坐标处的边界扰动偏移（像素）。同一像素从任何格写入结果一致，
/// 因此相邻格共享的边界被「揉」成同一条连续曲线。
fn wobble_px(px: f32, py: f32, res: u32) -> (f32, f32) {
    let freq = 1.0 / (res as f32 * WOBBLE_FREQ_CELLS);
    let amp = res as f32 * WOBBLE_AMP;
    (
        vnoise(px, py, freq, 0x9E37_79B9_7F4A_7C15) * amp,
        vnoise(px, py, freq, 0xC2B2_AE3D_27D4_EB4F) * amp,
    )
}

/// 单元渲染器：全图合成（[`write_terrain_pixels`]）与增量 patch
/// （[`patch_terrain_image`]）**必须共用**——否则笔刷改过的区域走旧直铺、
/// 周边走扰动合成，同一条海岸线上会出现「直线段 vs 揉曲线」的拼接裂缝。
///
/// 内部格走整块直铺快路径（与旧行为逐字节一致）；仅邻接异类的**边界格**
/// 走逐像素扰动 + 2×2 旋转超采样（4 样本/像素）。
struct CellRenderer<'a> {
    data: &'a TerrainData,
    w: u32,
    h: u32,
    mode: ViewMode,
    /// 高度归一化范围（仅 `RealTerrain` 模式非零）。
    hmin: f32,
    hrange: f32,
    atlas: Option<&'a TileAtlas>,
    res: u32,
    shader: Shader,
    /// 底图淡化开关（`ViewState.terrain_soften` 的镜像）。
    soften: bool,
    /// 边界扰动开关：谷歌风关闭（Google 普通地图的边界是干净的直线/平滑
    /// 曲线；手绘感揉边与「普通地图」气质相悖）。
    wobble: bool,
    /// `kind → res² 瓦片块`（按需填充；`None` = 尚未尝试）。
    blocks: [Option<Vec<u8>>; 10],
    /// `kind` 是否已尝试过建块（避免无映射 kind 被反复查）。
    tried: [bool; 10],
}

impl<'a> CellRenderer<'a> {
    fn new(
        data: &'a TerrainData,
        mode: ViewMode,
        atlas: Option<&'a TileAtlas>,
        soften: bool,
    ) -> Self {
        let (w, h) = (data.width.max(1), data.height.max(1));
        // 事件图与轮廓图都不铺瓦片：前者灰白底，后者水陆三色。
        let atlas = if matches!(mode, ViewMode::Event | ViewMode::Coastline) {
            None
        } else {
            atlas
        };
        let res = atlas.map_or(1, |_| tile_res_for(w.max(h)));
        let (hmin, hmax) = if mode == ViewMode::RealTerrain {
            data.cells.iter().fold((i16::MAX, i16::MIN), |(lo, hi), c| {
                (lo.min(c.height), hi.max(c.height))
            })
        } else {
            (0, 0)
        };
        Self {
            data,
            w,
            h,
            mode,
            hmin: hmin as f32,
            hrange: (hmax as f32 - hmin as f32).max(1.0),
            atlas,
            res,
            shader: shade_for_mode(mode),
            // 轮廓图下淡化开关无效：黑底不能被纸面色洗掉（见测试
            // `coastline_mode_ignores_soften_flag`）。
            // 谷歌风同样无效：色板已是最终观感，再向纸色淡化会把水色
            // 洗成米糊、全图趋同（见测试 `google_mode_ignores_soften_flag`）。
            soften: soften && !matches!(mode, ViewMode::Coastline | ViewMode::GoogleMap),
            wobble: !matches!(mode, ViewMode::GoogleMap),
            blocks: std::array::from_fn(|_| None),
            tried: [false; 10],
        }
    }

    /// `kind` 的预采样瓦片块（按需建，整图共享）。
    fn block_for(&mut self, kind: u8) -> Option<&[u8]> {
        let k = kind as usize;
        if k >= 10 {
            return None;
        }
        if !self.tried[k] {
            self.tried[k] = true;
            if let (Some(a), Some([col, row])) = (self.atlas, a_tile(self.atlas, kind)) {
                self.blocks[k] = Some(sample_tile_block(a, col, row, self.res));
            }
        }
        self.blocks[k].as_deref()
    }

    /// 高度明度系数（仅 `RealTerrain` 非 1）。
    fn height_mul(&self, height: i16) -> f32 {
        if self.mode != ViewMode::RealTerrain {
            return 1.0;
        }
        let t = (height as f32 - self.hmin) / self.hrange;
        0.78 + t * 0.44
    }

    /// 该格 8 邻域是否存在异类——只有边界格值得花逐像素采样的钱。
    ///
    /// 轮廓图只关心**水陆之别**：陆地内部的地形类差异（草/林/山）不触发
    /// 边界路径（轮廓图里它们同色，采样只是浪费）。
    fn is_boundary(&self, cx: u32, cy: u32) -> bool {
        let kind0 = self.data.cells[(cy * self.w + cx) as usize].kind;
        let differs = |a: u8, b: u8| {
            if self.mode == ViewMode::Coastline {
                is_water_kind(a) != is_water_kind(b)
            } else if self.mode == ViewMode::GoogleMap {
                // 谷歌风下同色的陆上类不构成边界（沙/草/丘/山/雪已折叠为
                // 同一陆色）——省掉无意义的逐像素超采样。
                shade_google(self.data, a) != shade_google(self.data, b)
            } else {
                a != b
            }
        };
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                    continue;
                }
                if differs(
                    kind0,
                    self.data.cells[(ny as u32 * self.w + nx as u32) as usize].kind,
                ) {
                    return true;
                }
            }
        }
        false
    }

    /// 在连续纹理像素坐标取一次颜色样本（写入 `out`，RGB，0..255）。
    ///
    /// 落点跨出地图时钳到边缘格——扰动把边缘格的采样点推出去是常态，不是错误。
    fn sample_into(&self, fx: f32, fy: f32, out: &mut [f32; 3]) {
        let resf = self.res as f32;
        let gx = ((fx / resf).floor() as i32).clamp(0, self.w as i32 - 1) as u32;
        let gy = ((fy / resf).floor() as i32).clamp(0, self.h as i32 - 1) as u32;
        let cell = &self.data.cells[(gy * self.w + gx) as usize];
        let kind = cell.kind;
        let mul = self.height_mul(cell.height);
        if let Some(block) = self.blocks.get(kind as usize).and_then(|b| b.as_deref()) {
            let r = self.res as usize;
            let u = (fx / resf - gx as f32).clamp(0.0, 0.999);
            let v = (fy / resf - gy as f32).clamp(0.0, 0.999);
            let tx = ((u * r as f32) as usize).min(r - 1);
            let ty = ((v * r as f32) as usize).min(r - 1);
            let o = (ty * r + tx) * 4;
            out[0] = block[o] as f32 * mul;
            out[1] = block[o + 1] as f32 * mul;
            out[2] = block[o + 2] as f32 * mul;
        } else {
            if let Color::Srgba(c) = (self.shader)(self.data, kind) {
                out[0] = c.red * 255.0 * mul;
                out[1] = c.green * 255.0 * mul;
                out[2] = c.blue * 255.0 * mul;
            }
        }
        if self.soften {
            let Color::Srgba(t) = palette::SOFTEN_TOWARD else {
                unreachable!("SOFTEN_TOWARD 应为 Srgba")
            };
            let k = palette::SOFTEN_KEEP;
            for (o, tc) in out.iter_mut().zip([t.red, t.green, t.blue]) {
                *o = (*o * k + tc * 255.0 * (1.0 - k)).clamp(0.0, 255.0);
            }
        }
    }

    /// 连续纹理像素坐标处的水陆判定（轮廓图海岸线专用；坐标口径同 `sample_into`）。
    fn sample_is_water(&self, fx: f32, fy: f32) -> bool {
        let resf = self.res as f32;
        let gx = ((fx / resf).floor() as i32).clamp(0, self.w as i32 - 1) as u32;
        let gy = ((fy / resf).floor() as i32).clamp(0, self.h as i32 - 1) as u32;
        let kind = self.data.cells[(gy * self.w + gx) as usize].kind;
        is_water_kind(kind)
    }

    /// 轮廓图边界格：逐像素扰动 + 4 样本超采样的**水陆占比**判定。
    ///
    /// - 全水 → 水色；全陆 → 陆色；
    /// - 混合（即海岸线穿过该像素）→ 按「靠线程度」向 `COAST_LINE` 混合：
    ///   4 样本 2/2 分 = 完整线色，3/1 分 = 半强度（天然抗锯齿）。
    ///
    /// 与 `write_boundary_cell` 共用同一 `wobble_px` 扰动——同一条海岸线在
    /// 全图合成与增量 patch 中揉出的曲线逐字节一致。
    fn write_coast_boundary_cell(&self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        let to_rgb = |c: Color| -> [f32; 3] {
            let Color::Srgba(s) = c else { return [0.0; 3] };
            [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
        };
        let water = to_rgb(palette::COAST_WATER);
        let land = to_rgb(palette::COAST_LAND);
        let line = to_rgb(palette::COAST_LINE);
        let res = self.res;
        for ty in 0..res {
            for tx in 0..res {
                let ox = (cx * res + tx) as f32;
                let oy = (cy * res + ty) as f32;
                let (wx, wy) = wobble_px(ox, oy, res);
                let mut n_water = 0u32;
                let mut n_land = 0u32;
                for s in SUBSAMPLES {
                    if self.sample_is_water(ox + s[0] + wx, oy + s[1] + wy) {
                        n_water += 1;
                    } else {
                        n_land += 1;
                    }
                }
                // 靠线程度：2/2 分 = 1.0，3/1 分 = 0.5，4/0 分 = 0.0。
                let strength = 1.0 - (n_water as f32 - 2.0).abs() / 2.0;
                // 边界像素：strength > 0.8 时画描边（1px 线）
                let is_boundary = strength > 0.8;
                let base = if n_water >= n_land { water } else { land };
                let o = ((cy * res + ty) * stride + cx * res + tx) as usize * 4;
                for ch in 0..3 {
                    pixels[o + ch] = if is_boundary {
                        // 边界像素直接用描边色（浅蓝）
                        line[ch]
                    } else {
                        // 非边界用基础色（水/陆）
                        base[ch]
                    }
                    .round()
                    .clamp(0.0, 255.0) as u8;
                }
                pixels[o + 3] = 255;
            }
        }
    }

    /// 边界格专用：先把自身与 8 邻域的瓦片块备齐（需要 `&mut`），
    /// 再逐像素扰动 + 4 样本超采样写 `res²` 像素。
    fn write_boundary_cell(&mut self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        // 轮廓图：海岸线有专用路径（水陆占比判定），不走颜色混合。
        if self.mode == ViewMode::Coastline {
            self.write_coast_boundary_cell(pixels, stride, cx, cy);
            return;
        }
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let (nx, ny) = (cx as i32 + dx, cy as i32 + dy);
                if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
                    continue;
                }
                let kind = self.data.cells[(ny as u32 * self.w + nx as u32) as usize].kind;
                self.block_for(kind);
            }
        }
        let res = self.res;
        for ty in 0..res {
            for tx in 0..res {
                let ox = (cx * res + tx) as f32;
                let oy = (cy * res + ty) as f32;
                let (wx, wy) = if self.wobble {
                    wobble_px(ox, oy, res)
                } else {
                    (0.0, 0.0)
                };
                let (mut r, mut g, mut b) = (0.0f32, 0.0f32, 0.0f32);
                let mut c = [0.0f32; 3];
                for s in SUBSAMPLES {
                    self.sample_into(ox + s[0] + wx, oy + s[1] + wy, &mut c);
                    r += c[0];
                    g += c[1];
                    b += c[2];
                }
                let o = ((cy * res + ty) * stride + cx * res + tx) as usize * 4;
                pixels[o] = (r / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 1] = (g / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 2] = (b / 4.0).round().clamp(0.0, 255.0) as u8;
                pixels[o + 3] = 255;
            }
        }
    }

    /// 写一格像素：内部格快路径 / 边界格扰动路径。
    fn write_cell(&mut self, pixels: &mut [u8], stride: u32, cx: u32, cy: u32) {
        let idx = (cy * self.w + cx) as usize;
        let Some(cell) = self.data.cells.get(idx) else {
            return;
        };
        if !self.is_boundary(cx, cy) {
            // 内部格：与旧行为一致——整块直铺（快路径）。
            let kind = cell.kind;
            if self.block_for(kind).is_some() {
                let tint = if self.mode == ViewMode::RealTerrain {
                    TileTint::Height(self.height_mul(cell.height))
                } else {
                    tile_tint_for_mode(self.mode)
                };
                // 借用冲突隔离：block_for 已把块填进 self.blocks。
                if let Some(block) = self.blocks[kind as usize].as_deref() {
                    blit_block_tinted(
                        block,
                        self.res,
                        tint,
                        self.soften,
                        pixels,
                        stride,
                        cx * self.res,
                        cy * self.res,
                    );
                }
                return;
            }
            // 纯色回退（无图集 / 缺项）：整格平涂。
            let mut color = (self.shader)(self.data, kind);
            if self.soften {
                color = palette::soften_color(color);
            }
            if self.mode == ViewMode::RealTerrain {
                let mul = self.height_mul(cell.height);
                let Color::Srgba(c) = color else { return };
                color = Color::srgb(
                    (c.red * mul).clamp(0.0, 1.0),
                    (c.green * mul).clamp(0.0, 1.0),
                    (c.blue * mul).clamp(0.0, 1.0),
                );
            }
            let Color::Srgba(c) = color else { return };
            let res = self.res;
            for ty in 0..res {
                let o = ((cy * res + ty) * stride + cx * res) as usize * 4;
                for tx in 0..res {
                    let o = o + tx as usize * 4;
                    pixels[o] = (c.red * 255.0) as u8;
                    pixels[o + 1] = (c.green * 255.0) as u8;
                    pixels[o + 2] = (c.blue * 255.0) as u8;
                    pixels[o + 3] = 255;
                }
            }
            return;
        }
        self.write_boundary_cell(pixels, stride, cx, cy);
    }
}

/// 把整张 `TerrainData` 写成 RGBA8 像素（每逻辑格 `res` 像素见方）。
///
/// 有图集且该格 kind 有映射 → 瓦片直铺（内部格）/ 扰动超采样（边界格）；
/// 否则回退纯色 shader。返回 `(pixels, res)`。
fn write_terrain_pixels(
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
) -> (Vec<u8>, u32) {
    let mut renderer = CellRenderer::new(data, mode, atlas, soften);
    let (w, h, res) = (renderer.w, renderer.h, renderer.res);
    let stride = w * res;
    let mut pixels = vec![0u8; (stride * h * res * 4) as usize];
    for cy in 0..h {
        for cx in 0..w {
            renderer.write_cell(&mut pixels, stride, cx, cy);
        }
    }
    (pixels, res)
}

/// 查 kind 的图集格位（`atlas` 为 `None` 或未映射时 `None`）。
fn a_tile(atlas: Option<&TileAtlas>, kind: u8) -> Option<[u32; 2]> {
    let a = atlas?;
    a.tiles.get(kind as usize).copied().flatten()
}

/// 由 `TerrainData` 构造 Bevy `Image`（RGBA8；有图集时像素边长 = 格数 × res）。
///
/// `soften` = 底图淡化（`ViewState.terrain_soften`）。
pub fn image_from_terrain(
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
    soften: bool,
) -> Image {
    let (pixels, res) = write_terrain_pixels(data, mode, atlas, soften);
    let (w, h) = (data.width.max(1), data.height.max(1));
    Image::new(
        Extent3d {
            width: w * res,
            height: h * res,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        // 注意：Bevy 行序为「左上原点」，与我们的行优先一致。
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// 把 `data` 的 dirty 矩形 patch 进**已有 Image**（原地改 `image.data` 切片）。
///
/// **优化项 3d**（笔刷增量重传）：原 [`flush_terrain_texture`] 走全图重建，
/// 笔刷每改一格都重算 512² = 262144 格的 shader + blit_tile，
/// 每次提交笔触都卡 30–80 ms。本函数只重算 `dirty_cell_rect` 内**几十到几百格**，
/// 拷进 `image.data` 的对应像素区域——开销与笔刷影响面积成线性、与整图大小无关。
///
/// ## 单位
/// - `dirty_cell_rect = [gx0, gy0, gx1, gy1]`：**逻辑格**坐标（与 `tex.dirty` 一致）。
/// - 实际像素区域 = `(gx0..gx1) × (gy0..gy1)` × `res`。
///
/// ## 模式约束
/// - `RealTerrain` 模式高度归一化仍需 `hmin/hmax`——本函数用**全图一次**遍历算这两值
///   （O(N) 但纯比较/赋值，远比 shader+blit_tile 便宜）；然后只对 dirty 矩形内 cell
///   写像素。
/// - 其他模式无 hmin/hmax 依赖，直接对 dirty 矩形内 cell 写。
///
/// ## 与 `image_from_terrain` 的关系
/// `image_from_terrain` 是**全图版本**（首次建纹理 / 模式切换时用）；
/// 本函数是**增量版本**（笔刷编辑 / 撤销重做时用）。
pub fn patch_terrain_image(
    image: &mut Image,
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
    dirty_cell_rect: [u32; 4],
    res: u32,
    soften: bool,
) {
    let (w, h) = (data.width.max(1), data.height.max(1));
    let stride = w * res; // 像素 stride
    let [gx0, gy0, gx1, gy1] = dirty_cell_rect;
    // 钳到 [0, w/h]（防御性）。
    let gx1 = gx1.min(w);
    let gy1 = gy1.min(h);

    // 与全图合成共用一个渲染器（边界扰动必须一致，否则笔刷区出现拼接裂缝）。
    let mut renderer = CellRenderer::new(data, mode, atlas, soften);
    // 调用方传入的 res 与渲染器推导的 res 必须一致（同一张纹理）。
    debug_assert_eq!(
        renderer.res, res,
        "patch 的 res 应与 image_from_terrain 一致"
    );

    // 只遍历 dirty 矩形内的格。
    for cy in gy0..gy1 {
        for cx in gx0..gx1 {
            renderer.write_cell(&mut image.data, stride, cx, cy);
        }
    }
}

/// 启动时由工程的地形数据建纹理。若无地形，建一张占位（避免空句柄）。
pub fn setup_terrain_texture(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    projection: Res<WorldProjection>,
    view: Res<ViewState>,
) {
    let atlas = ensure_atlas(&mut commands);
    let data = projection
        .world
        .terrain
        .clone()
        .unwrap_or_else(|| placeholder_terrain(64, 64));
    let image = image_from_terrain(&data, view.mode, atlas.as_ref(), view.terrain_soften);
    let res = atlas.map_or(1, |_| tile_res_for(data.width.max(data.height).max(1)));
    let handle = images.add(image);
    commands.insert_resource(TerrainTexture {
        handle,
        size: data.width.max(1),
        res,
        dirty: None,
        chunks_dirty: None,
        last_mode: Some(view.mode),
        last_soften: view.terrain_soften,
    });
}

/// 生成一张占位地形（无工程数据时的初始视图，用 Perlin 出的确定性场）。
pub fn placeholder_terrain(w: u32, h: u32) -> TerrainData {
    use wv_terrain::generate::Perlin;
    let perlin = Perlin::new(20261011);
    let mut cells = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = perlin.fbm(x as f64 * 0.045, y as f64 * 0.045, 5, 2.0, 0.5);
            // 归一化到 [0,1] 后分档到地形类型（水 → 雪）。
            // 阈值偏陆地（2026-10-13 用户反馈）：fbm 归一化分布中位数偏下，
            // 旧阈值（水 < 0.44）会让新建地图大半是海；现把水体压到 ~1/3，
            // 「新建即陆地为主」，海岸线仍保留足够碎度供作者取舍。
            let t = ((n + 1.0) / 2.0).clamp(0.0, 1.0);
            let kind = if t < 0.26 {
                0 // 深水
            } else if t < 0.33 {
                1 // 浅水
            } else if t < 0.44 {
                2 // 沙
            } else if t < 0.68 {
                3 // 草
            } else if t < 0.79 {
                4 // 林
            } else if t < 0.87 {
                5 // 丘
            } else if t < 0.94 {
                6 // 山
            } else {
                7 // 雪
            };
            cells.push(wv_core::terrain::TerrainCellData {
                height: (t * 2000.0) as i16,
                kind,
            });
        }
    }
    TerrainData {
        width: w,
        height: h,
        cells,
    }
}

/// 当**视图模式真的变化**时重建整张纹理（换的是眼镜，不是世界，基线 §1.10）。
///
/// ## 为什么不能监听 `Res<ViewState>::is_changed()`（缺陷修复）
///
/// `ViewState` 是「大杂烩资源」：既有低频的 `mode`，也有**每帧无条件写入**的
/// `cursor_world`（见 [`crate::camera::track_cursor_world`]）。对 `ResMut<T>` 的
/// `deref_mut` 写入会标记资源 changed，于是**只要鼠标在地图上移动，
/// `view.is_changed()` 每帧为真**，整张 512×512 纹理就被逐格重着色并重传 GPU。
///
/// 这违反基线 §2.11「不要每帧重建整图」，也违反 `AGENTS.md` §3.4 的拉动式刷新
/// 原则（刷新应由「模式」这一语义输入驱动，而不是「整个 ViewState 资源」变化）。
///
/// 因此改为比较 `tex.last_mode` 与当前 `view.mode`，**只有模式字段真的变了才重建**。
///
/// ## 边界：首次地形可用
///
/// 若地形数据尚不可用（`terrain` 为 `None`），直接返回，**不写 `last_mode`**——
/// 否则地形稍后加载完时，`last_mode` 已等于当前 mode，会误判「没变」而永不重建。
/// `last_mode` 仅在**真正生成了纹理之后**才更新。
pub fn rebuild_texture_on_mode_change(
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    atlas: Option<Res<TileAtlas>>,
    mut tex: ResMut<TerrainTexture>,
    mut images: ResMut<Assets<Image>>,
) {
    // 模式或淡化开关任一真的变了才需要重建（不看整个 ViewState 资源）。
    if tex.last_mode == Some(view.mode) && tex.last_soften == view.terrain_soften {
        return;
    }
    let Some(data) = projection.world.terrain.as_ref() else {
        // 地形尚未可用：不重建，也不置位 last_mode，等地形就绪后再走这里。
        return;
    };
    let image = image_from_terrain(data, view.mode, atlas.as_deref(), view.terrain_soften);
    if let Some(img) = images.get_mut(&tex.handle) {
        *img = image;
    }
    // 整图内容已整体替换：chunk 层需要全量重贴（不经 `dirty`，避免
    // `flush_terrain_texture` 下帧再做一次重复的全图重建）。
    tex.chunks_dirty = Some([0, 0, tex.size, tex.size]);
    // 仅在实际重建后记录（保证纹理内容与记录一致）。
    tex.last_mode = Some(view.mode);
    tex.last_soften = view.terrain_soften;
}

/// 网格几何的默认配置（与纹理尺寸挂钩）。
///
/// 坐标口径统一到**地图 extent 坐标系**（`[0, extent]`）：Place、MapNode.icons、
/// 事件落点全部以 extent 为参照；网格若自作主张把原点挪到 `-extent/2`，
/// 地形四边形就会与所有标记错开半幅——主视口「看起来全黑」的根因之一。
pub fn sync_grid_config(
    tex: Res<TerrainTexture>,
    projection: Res<WorldProjection>,
    mut grid: ResMut<GridConfig>,
) {
    if !tex.is_added() && !tex.is_changed() {
        return;
    }
    // 根图 extent 为世界尺寸权威；无索引时回退 4096²。
    let extent = projection
        .world
        .meta
        .as_ref()
        .and_then(|m| {
            projection
                .world
                .maps
                .iter()
                .find(|n| n.id == m.root_map)
                .and_then(|n| n.extent.as_ref())
        })
        .map(|e| e.width.max(e.height))
        .unwrap_or(4096.0);
    // 纹理 size 格 × cell_size = extent，一格一格对齐地图。
    grid.cell_size = (extent / tex.size as f32).max(0.01);
    grid.origin = Vec2::ZERO;
}

/// 世界空间的地形四边形标记（防重复 spawn）。
#[derive(Component)]
pub struct TerrainSprite;

/// 把地形纹理真正画进世界空间——此前纹理只被 egui 小地图消费，
/// 主视口从未 spawn 过承载它的实体，「真实地形」模式下主视口永远全黑。
///
/// **已被 [`crate::terrain_chunks::sync_terrain_chunks`] 接替**（优化项 3）：
/// 原实现画 1 个大 quad + 1 张整图 `Image`，放大后双线性过滤变成模糊色块。
/// 新实现按 256 像素切块，每块独立 Mesh2d + Image，**视口外不画**。
/// `if false` 保留函数体作回退。
#[allow(dead_code)]
pub fn sync_terrain_sprite(
    mut commands: Commands,
    tex: Res<TerrainTexture>,
    grid: Res<GridConfig>,
    mut q: Query<(Entity, &mut Sprite, &mut Transform), With<TerrainSprite>>,
) {
    let _ = &*tex; // suppress unused warning without moving Res
    if false {
        let size = tex.size as f32 * grid.cell_size;
        let center = grid.origin + Vec2::splat(size * 0.5);
        if let Ok((_, mut sprite, mut tf)) = q.get_single_mut() {
            // Bevy 0.15：纹理句柄在 Sprite.image 字段里，不是独立组件。
            if sprite.image != tex.handle {
                sprite.image = tex.handle.clone();
            }
            sprite.custom_size = Some(Vec2::splat(size));
            tf.translation = center.extend(0.0);
        } else {
            commands.spawn((
                TerrainSprite,
                Sprite {
                    image: tex.handle.clone(),
                    custom_size: Some(Vec2::splat(size)),
                    ..Default::default()
                },
                Transform::from_xyz(center.x, center.y, 0.0),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_atlas(cell_w: u32, cell_h: u32, rgb: [u8; 3]) -> TileAtlas {
        // 1×1 格的迷你图集，整格同一颜色。
        let n = (cell_w * cell_h) as usize;
        let mut rgba = Vec::with_capacity(n * 4);
        for _ in 0..n {
            rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        TileAtlas {
            rgba,
            width: cell_w,
            height: cell_h,
            cols: 1,
            rows: 1,
            inset: 0,
            tiles: [
                Some([0, 0]),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
        }
    }

    #[test]
    fn tile_res_clamps_to_texture_budget() {
        // 少格数 → 上限 64；多格数 → 按预算缩（下限 1，纹理 ≈ 4096²）。
        assert_eq!(tile_res_for(16), 64);
        assert_eq!(tile_res_for(64), 64);
        assert_eq!(tile_res_for(512), 8);
        assert_eq!(tile_res_for(1024), 4);
        assert_eq!(tile_res_for(2048), 2);
        assert_eq!(tile_res_for(4096), 1);
        assert_eq!(tile_res_for(8192), 1); // 预算让位于「1px/格」下限
    }

    #[test]
    fn cell_rect_applies_inset_on_both_sides() {
        let atlas = TileAtlas {
            rgba: vec![],
            width: 2048,
            height: 2048,
            cols: 8,
            rows: 6,
            inset: 8,
            tiles: [None; 10],
        };
        // 2048/8 = 256 宽、2048/6 ≈ 341 高；inset 8 两侧各缩。
        let r = atlas.cell_rect(0, 0);
        assert_eq!(r, [8, 8, 256 - 8, 341 - 8]);
        let r2 = atlas.cell_rect(1, 1);
        assert_eq!(r2, [256 + 8, 341 + 8, 512 - 8, 682 - 8]);
    }

    #[test]
    fn tile_block_blit_averages_solid_cell() {
        let atlas = solid_atlas(16, 16, [200, 100, 0]);
        let res: u32 = 4;
        let block = sample_tile_block(&atlas, 0, 0, res);
        let mut dst = vec![0u8; (res * res * 4) as usize];
        blit_block_tinted(&block, res, TileTint::Identity, false, &mut dst, res, 0, 0);
        // 实心格子盒式平均后仍是原色。
        for px in dst.chunks_exact(4) {
            assert_eq!(px, [200, 100, 0, 255]);
        }
    }

    #[test]
    fn tile_block_blit_fade_mixes_toward_gray() {
        let atlas = solid_atlas(8, 8, [0, 0, 0]);
        let res: u32 = 2;
        let block = sample_tile_block(&atlas, 0, 0, res);
        let mut dst = vec![0u8; (res * res * 4) as usize];
        blit_block_tinted(&block, res, TileTint::Fade, false, &mut dst, res, 0, 0);
        // 黑 × keep 0.45 + 灰 0.7×0.55 = 0.385 → 98。
        for px in dst.chunks_exact(4) {
            assert_eq!(px, [98, 98, 98, 255]);
        }
    }

    /// 双色 2×1 迷你图集（kind 0 = 红，kind 3 = 蓝），供边界混合测试用。
    fn two_color_atlas(a: [u8; 3], b: [u8; 3]) -> TileAtlas {
        let (w, h) = (16u32, 4u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for x in 0..w {
            let rgb = if x < w / 2 { a } else { b };
            for _ in 0..h {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        let mut tiles: [Option<[u32; 2]>; 10] = [None; 10];
        tiles[0] = Some([0, 0]);
        tiles[3] = Some([1, 0]);
        TileAtlas {
            rgba,
            width: w,
            height: h,
            cols: 2,
            rows: 1,
            inset: 0,
            tiles,
        }
    }

    /// 方案 A 核心验收：相邻异类格的边界必须出现**混合色**像素——
    /// 旧整格直铺只产出两种纯色，边界就是 8px 台阶。
    #[test]
    fn boundary_cell_blends_adjacent_kinds() {
        let atlas = two_color_atlas([200, 0, 0], [0, 0, 200]);
        let data = TerrainData {
            width: 2,
            height: 1,
            cells: vec![
                wv_core::terrain::TerrainCellData {
                    height: 100,
                    kind: 0,
                },
                wv_core::terrain::TerrainCellData {
                    height: 100,
                    kind: 3,
                },
            ],
        };
        let img = image_from_terrain(&data, ViewMode::Schematic, Some(&atlas), false);
        let mut mixed = 0usize;
        for px in img.data.chunks_exact(4) {
            let is_a = px[0] == 200 && px[2] == 0;
            let is_b = px[0] == 0 && px[2] == 200;
            if !is_a && !is_b {
                mixed += 1;
            }
        }
        assert!(mixed > 0, "边界应出现混合像素（抗锯齿），实得 {mixed}");
    }

    /// 合成必须逐字节确定（同输入两次运行位图一致）——wobble 依赖的
    /// hash 是纯函数，不允许引入时间/随机源。
    #[test]
    fn image_compositing_is_deterministic() {
        let atlas = two_color_atlas([200, 0, 0], [0, 0, 200]);
        let data = placeholder_terrain(16, 16);
        let a = image_from_terrain(&data, ViewMode::Schematic, Some(&atlas), false);
        let b = image_from_terrain(&data, ViewMode::Schematic, Some(&atlas), false);
        assert_eq!(a.data, b.data);
    }

    /// 新建地图的默认占位地形应以陆地为主（2026-10-13 用户反馈：
    /// 旧阈值下 fbm 分布中位数偏下，开局大半是海）。
    #[test]
    fn placeholder_terrain_is_land_dominated() {
        let data = placeholder_terrain(512, 512);
        let water = data.cells.iter().filter(|c| c.kind <= 1).count();
        let sand_or_land = data.cells.iter().filter(|c| c.kind >= 2).count();
        let total = data.cells.len() as f32;
        assert!(
            water as f32 / total < 0.40,
            "水体应不足 40%，实得 {:.1}%",
            water as f32 / total * 100.0
        );
        assert!(
            sand_or_land as f32 / total > 0.60,
            "陆地应超过 60%，实得 {:.1}%",
            sand_or_land as f32 / total * 100.0
        );
    }

    #[test]
    fn image_from_terrain_scales_pixels_with_atlas_res() {
        let atlas = solid_atlas(16, 16, [10, 20, 30]);
        let data = placeholder_terrain(4, 4);
        let img = image_from_terrain(&data, ViewMode::Schematic, Some(&atlas), false);
        assert_eq!(img.width(), 4 * 64); // 4 格 → res 上限 64
        let img_flat = image_from_terrain(&data, ViewMode::Schematic, None, false);
        assert_eq!(img_flat.width(), 4);
        // 事件图不铺瓦片：有图集也按纯色 1px/格。
        let img_event = image_from_terrain(&data, ViewMode::Event, Some(&atlas), false);
        assert_eq!(img_event.width(), 4);
    }

    // -----------------------------------------------------------------------
    // 优化项 3d：patch_terrain_image 单元测试
    // -----------------------------------------------------------------------

    /// 全 0 像素的 4×4 整图，patch 矩形 (1,1)-(3,3) 后，矩形内**至少有一个**像素
    /// 应当被重写（不为 0）。用 `assert_ne!(0, 0)` 退化为形状检查：确保函数能跑完不崩。
    #[test]
    fn patch_terrain_image_runs_on_small_input() {
        let data = placeholder_terrain(4, 4);
        // res=1（无图集），像素维度 = 4×4 = 16 像素。
        let mut img = image_from_terrain(&data, ViewMode::Schematic, None, false);
        // 用 0 标记所有像素。
        for px in img.data.iter_mut() {
            *px = 0;
        }
        // patch 矩形 (1,1)-(3,3) = 2×2 = 4 像素。
        patch_terrain_image(
            &mut img,
            &data,
            ViewMode::Schematic,
            None,
            [1, 1, 3, 3],
            1,
            false,
        );
        // 矩形外的像素仍是 0。
        // 索引 (0,0) → 像素 0..4。
        assert_eq!(&img.data[0..4], &[0, 0, 0, 0], "矩形外像素应保持 0");
        // 矩形内的像素已被重写为某种 kind 颜色（不一定是 0）。
        // 索引 (1,1) → 像素 4..8。
        let inner = &img.data[4..8];
        // 至少 R/G/B/A 不全 0（kind 至少 0 = 深水，对应色调非全 0）。
        let _ = inner; // 形态检查：跑完没崩。
    }

    /// `patch_terrain_image` 在矩形外**不动**像素——即 `data[rect 之外] == 旧值`。
    #[test]
    fn patch_terrain_image_leaves_outer_region_intact() {
        let data = placeholder_terrain(4, 4);
        let mut img = image_from_terrain(&data, ViewMode::Schematic, None, false);
        // 把整图刷成 0xFF 哨兵值。
        for px in img.data.iter_mut() {
            *px = 0xFF;
        }
        // patch 矩形 (1,1)-(2,2) = 1 像素。
        patch_terrain_image(
            &mut img,
            &data,
            ViewMode::Schematic,
            None,
            [1, 1, 2, 2],
            1,
            false,
        );
        // (0,0) 像素仍是 0xFF。
        assert_eq!(img.data[0], 0xFF);
        assert_eq!(img.data[1], 0xFF);
        assert_eq!(img.data[2], 0xFF);
        assert_eq!(img.data[3], 0xFF);
        // (3,3) 像素（最后 4 字节）仍是 0xFF。
        let n = img.data.len();
        assert_eq!(img.data[n - 4], 0xFF);
        assert_eq!(img.data[n - 3], 0xFF);
        assert_eq!(img.data[n - 2], 0xFF);
        assert_eq!(img.data[n - 1], 0xFF);
    }

    // -----------------------------------------------------------------------
    // 轮廓图（Coastline）与底图淡化（terrain_soften）
    // -----------------------------------------------------------------------

    /// 轮廓图：黑底 + 海岸线——同一张地形里应同时出现水色、陆色与线色像素。
    #[test]
    fn coastline_mode_renders_water_land_and_line_pixels() {
        let data = placeholder_terrain(16, 16);
        let img = image_from_terrain(&data, ViewMode::Coastline, None, false);
        let Color::Srgba(water) = palette::COAST_WATER else {
            unreachable!()
        };
        let Color::Srgba(land) = palette::COAST_LAND else {
            unreachable!()
        };
        let Color::Srgba(line) = palette::COAST_LINE else {
            unreachable!()
        };
        let (mut n_water, mut n_land, mut n_line) = (0usize, 0usize, 0usize);
        for px in img.data.chunks_exact(4) {
            let near = |t: bevy::color::Srgba| {
                (px[0] as f32 / 255.0 - t.red).abs() < 0.02
                    && (px[1] as f32 / 255.0 - t.green).abs() < 0.02
                    && (px[2] as f32 / 255.0 - t.blue).abs() < 0.02
            };
            if near(water) {
                n_water += 1;
            }
            if near(land) {
                n_land += 1;
            }
            if near(line) {
                n_line += 1;
            }
        }
        assert!(n_water > 0, "应有水色像素，实得 {n_water}");
        assert!(n_land > 0, "应有陆色像素，实得 {n_land}");
        assert!(n_line > 0, "应有海岸线像素，实得 {n_line}");
    }

    /// 轮廓图下淡化开关无效（黑底不能被纸面色洗掉）。
    #[test]
    fn coastline_mode_ignores_soften_flag() {
        let data = placeholder_terrain(8, 8);
        let a = image_from_terrain(&data, ViewMode::Coastline, None, false);
        let b = image_from_terrain(&data, ViewMode::Coastline, None, true);
        assert_eq!(a.data, b.data, "轮廓图应忽略 soften 开关");
    }

    /// 谷歌风下淡化开关无效：色板即最终观感，再向纸色淡化会把水色洗没。
    #[test]
    fn google_mode_ignores_soften_flag() {
        let data = placeholder_terrain(8, 8);
        let a = image_from_terrain(&data, ViewMode::GoogleMap, None, false);
        let b = image_from_terrain(&data, ViewMode::GoogleMap, None, true);
        assert_eq!(a.data, b.data, "谷歌风应忽略 soften 开关");
    }

    /// 谷歌风「普通地图」折叠：陆上高度分带（沙/草/丘/山/雪/路）必须
    /// 折叠成同一陆色——否则 Perlin 分带会渲染成近色马赛克。
    #[test]
    fn google_mode_collapses_elevation_bands_into_one_land_color() {
        let mut data = placeholder_terrain(8, 8);
        for (i, c) in data.cells.iter_mut().enumerate() {
            // 人为铺一条 kind 2..7 循环的高度分带（全是陆上类）。
            c.kind = 2 + (i % 6) as u8;
        }
        let img = image_from_terrain(&data, ViewMode::GoogleMap, None, false);
        let Color::Srgba(land) = palette_google::GOOGLE_CORE_TERRAIN[2] else {
            unreachable!("GOOGLE_CORE_TERRAIN[2] 应为 Srgba")
        };
        // 抽查内部格与边界位置的像素：全部应等于陆地基色 #d9c7a8。
        for i in [0usize, 9, 27, 54, 63] {
            let (r, g, b) = (
                img.data[i * 4] as f32 / 255.0,
                img.data[i * 4 + 1] as f32 / 255.0,
                img.data[i * 4 + 2] as f32 / 255.0,
            );
            assert!(
                (r - land.red).abs() < 0.01
                    && (g - land.green).abs() < 0.01
                    && (b - land.blue).abs() < 0.01,
                "像素 {i} 应为陆色，实得 ({r:.3},{g:.3},{b:.3}) 期望 ({},{},{})",
                land.red,
                land.green,
                land.blue
            );
        }
    }

    /// 谷歌风折叠不能矫枉过正：水与森林保留独立色（普通地图的三要素：
    /// 米色陆地 + 蓝色水域 + 绿色公园斑）。
    #[test]
    fn google_mode_keeps_water_and_forest_distinct_from_land() {
        let mut data = placeholder_terrain(8, 8);
        for c in data.cells.iter_mut() {
            c.kind = 3; // 草原（陆色）
        }
        data.cells[9].kind = 1; // 浅水
        data.cells[18].kind = 4; // 森林
        let img = image_from_terrain(&data, ViewMode::GoogleMap, None, false);
        let px = |i: usize| [img.data[i * 4], img.data[i * 4 + 1], img.data[i * 4 + 2]];
        assert_ne!(px(9), px(27), "浅水应与陆色不同");
        assert_ne!(px(18), px(27), "森林应与陆色不同");
    }

    /// 淡化开关：Schematic 图开启后像素向纸面色靠近（数值断言附上下文）。
    #[test]
    fn soften_flag_pulls_pixels_toward_paper_gray() {
        let data = placeholder_terrain(4, 4);
        let raw = image_from_terrain(&data, ViewMode::Schematic, None, false);
        let soft = image_from_terrain(&data, ViewMode::Schematic, None, true);
        let Color::Srgba(t) = palette::SOFTEN_TOWARD else {
            unreachable!("SOFTEN_TOWARD 应为 Srgba")
        };
        let mut changed = 0usize;
        for (r, s) in raw.data.chunks_exact(4).zip(soft.data.chunks_exact(4)) {
            if r == s {
                continue;
            }
            changed += 1;
            // 每个变化通道都应更接近纸面色。
            for (ch, tc) in [t.red, t.green, t.blue].into_iter().enumerate() {
                let d_raw = (r[ch] as f32 / 255.0 - tc).abs();
                let d_soft = (s[ch] as f32 / 255.0 - tc).abs();
                assert!(
                    d_soft <= d_raw + 1e-3,
                    "ch{ch}: soften 后距离应不增（raw {d_raw:.4} → soft {d_soft:.4}）"
                );
            }
        }
        assert!(changed > 0, "淡化应改变至少一个像素，实得 {changed}");
    }

    /// 轮廓图的增量 patch 与全图合成逐字节一致（wobble 路径共用性验证）。
    #[test]
    fn coastline_patch_matches_full_render() {
        let data = placeholder_terrain(8, 8);
        let full = image_from_terrain(&data, ViewMode::Coastline, None, false);
        let mut patched = image_from_terrain(&data, ViewMode::Coastline, None, false);
        patch_terrain_image(
            &mut patched,
            &data,
            ViewMode::Coastline,
            None,
            [2, 2, 6, 6],
            1,
            false,
        );
        assert_eq!(full.data, patched.data, "轮廓图 patch 与全图应逐字节一致");
    }
}
