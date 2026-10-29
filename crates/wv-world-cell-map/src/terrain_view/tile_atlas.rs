//! 瓦片图集（README §12-14：直铺整瓦、无过渡；数据层不动，仅渲染层查表）。
//!
//! ## 职责
//! - `TileAtlas` —— 图集资源（已解码 RGBA + 网格规格 + `TerrainKind → 格位` 映射）。
//! - `load_tile_atlas` —— 从 YAML 配置加载图集（任何一步失败返回 `None`）。
//! - `ensure_atlas` —— 启动期向 `Commands` 插入图集资源（**当前恒返回 `None`**）。
//! - `tile_res_for` —— 单元纹理的瓦片分辨率推算（私有 helper）。
//! - `tile_tint_for_mode` —— 视图模式 → 瓦片着色方式（私有）。
//! - `sample_tile_block` / `blit_block_tinted` —— 瓦片块降采样 + tint 写入（私有）。
//!
//! ## 边界（不做什么）
//! - **JRPG 图集已弃用（2026-11 用户裁定）** —— `ensure_atlas` 恒 `None`，渲染侧
//!   回退纯色平涂（[`crate::palette::core_terrain`]）；`load_tile_atlas` 保留供未来
//!   接入其他图集包。
//!
//! ## 主要导出
//! - [`TileAtlas`] / [`load_tile_atlas`] / [`ensure_atlas`]

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler};
use bevy::prelude::*;

use crate::shade::ShadeMode;

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
    pub fn cell_rect(&self, col: u32, row: u32) -> [u32; 4] {
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
        rgba: image.data.unwrap_or_default(),
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
pub fn tile_res_for(cells: u32) -> u32 {
    (4096 / cells.max(1)).clamp(1, 64)
}

/// 瓦片像素的着色方式（与视图模式一一对应，见 [`tile_tint_for_mode`]）。
#[derive(Clone, Copy)]
pub enum TileTint {
    /// 原样直铺（色块模式）。
    Identity,
    /// 高度明度调制系数（真实地形模式）：RGB 乘该系数。
    Height(f32),
    /// 淡化：与中性灰 0.7 混合，保留比例 0.45（势力图；同 [`shade_faded`]）。
    Fade,
}

pub fn tile_tint_for_mode(mode: ShadeMode) -> TileTint {
    match mode {
        ShadeMode::Schematic => TileTint::Identity,
        ShadeMode::RealTerrain => TileTint::Height(1.0), // 系数逐格覆写
        ShadeMode::Faction => TileTint::Fade,
        // 事件图是极简灰白底图（基线 §1.10 模式 D），不铺瓦片。
        // 轮廓图（模式 E）同样不铺瓦片——只有水陆三色与海岸线。
        ShadeMode::Event => TileTint::Identity,
        ShadeMode::Coastline => TileTint::Identity,
        // Google Default Map（ShadeMode::GoogleMap）与色块地形同为平铺色块，Identity 保留原色。
        ShadeMode::GoogleMap => TileTint::Identity,
        // TODO(plan v0.1 D3-A): 等值线矢量平涂管线未落地前，瓦片按原色直铺
        // （平涂家族，与 shade_for_mode 的 shade_flat 回退同一口径）。
        ShadeMode::VectorFlat => TileTint::Identity,
        // Topographic 分析视图（[ADR-0039]）—— 不经瓦片管线（`CellRenderer::new`
        // 已经 `atlas = None`）；`TileTint::Identity` 是兜底，本路径不会被调用。
        ShadeMode::Topographic => TileTint::Identity,
    }
}

/// 把图集一格盒式降采样成 `res×res` 的 RGBA 块（**无 tint**）。
///
/// **启动性能（2026-10 修复）**：全图合成时同一 tile 的降采样结果对每个 cell
/// 都相同，而瓦片种类只有几十个——过去 [`write_terrain_pixels`] 对 512² = 26.2 万格
/// 每格都重采样一遍（≈10 亿次采样累加，debug 构建实测 29s，启动白屏「未响应」
/// 的直接原因）。现在按 `(col, row)` 只采样一次、整图复用，重采样总量与格数解耦。
pub fn sample_tile_block(atlas: &TileAtlas, col: u32, row: u32, res: u32) -> Vec<u8> {
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
/// `soften_toward`（当前主题的纸面色，ADR-0016）混 `(1 - soften_keep)`。
/// 实现上把两步合并为一条定点管线（先 tint 后 soften，每通道两次 Q8 乘加），
/// 避免中间缓冲。两个淡化参数由调用方从 `PaletteColors` 透传（勿直引常量）。
#[allow(clippy::too_many_arguments)]
pub fn blit_block_tinted(
    block: &[u8],
    res: u32,
    tint: TileTint,
    soften: bool,
    soften_toward: Color,
    soften_keep: f32,
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
        let Color::Srgba(t) = soften_toward else {
            unreachable!("soften_toward 应为 Srgba")
        };
        let k = soften_keep;
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
        for (d, s) in dst_row
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(src_row.as_chunks::<4>().0)
        {
            for ch in 0..3 {
                let v = ((s[ch] as u32 * mul_q + add_q) >> 8).min(255);
                d[ch] = ((v * keep_q2 + add2[ch]) >> 8).min(255) as u8;
            }
            d[3] = 255;
        }
    }
}
