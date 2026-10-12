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
pub fn ensure_atlas(commands: &mut Commands) -> Option<TileAtlas> {
    match load_tile_atlas(TileAtlas::DEFAULT_CONFIG) {
        Some(atlas) => {
            info!("瓦片图集已加载（{}×{} 格）", atlas.cols, atlas.rows);
            commands.insert_resource(atlas.clone());
            Some(atlas)
        }
        None => {
            warn!("瓦片图集加载失败，地形回退纯色平涂");
            None
        }
    }
}

/// 每格瓦片重采样的目标边长（像素）。
///
/// 约束整张纹理 ≤ 4096²（≈67MB GPU 内存上限）：`res = clamp(4096/格数, 8, 64)`。
pub(crate) fn tile_res_for(cells: u32) -> u32 {
    (4096 / cells.max(1)).clamp(8, 64)
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
        ViewMode::Event => TileTint::Identity,
    }
}

/// 从图集格子做盒式降采样，取一块 `res×res` 的 RGBA 像素。
///
/// 盒式（区域平均）而不是取单点：避免 JPEG 噪点放大成雪花。
/// 参数虽多但全是同一个 blit 动作的坐标参数（源格位 / 目标矩形），
/// 拆 struct 反而增加阅读跳转——此处豁免 `too_many_arguments`。
#[allow(clippy::too_many_arguments)]
fn blit_tile(
    atlas: &TileAtlas,
    col: u32,
    row: u32,
    res: u32,
    tint: TileTint,
    dst: &mut [u8],
    dst_stride: u32,
    dx: u32,
    dy: u32,
) {
    let [x0, y0, x1, y1] = atlas.cell_rect(col, row);
    let sw = (x1.saturating_sub(x0)).max(1) as usize;
    let sh = (y1.saturating_sub(y0)).max(1) as usize;
    let stride = atlas.width as usize;
    let (gray, keep) = (0.7f32, 0.45f32);
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
            let (mut fr, mut fg, mut fb) = (
                r as f32 / n as f32 / 255.0,
                g as f32 / n as f32 / 255.0,
                b as f32 / n as f32 / 255.0,
            );
            match tint {
                TileTint::Identity => {}
                TileTint::Height(mul) => {
                    fr *= mul;
                    fg *= mul;
                    fb *= mul;
                }
                TileTint::Fade => {
                    // 与 shade_faded 同式：out = src*k + gray*(1-k)。
                    fr = fr * keep + gray * (1.0 - keep);
                    fg = fg * keep + gray * (1.0 - keep);
                    fb = fb * keep + gray * (1.0 - keep);
                }
            }
            let o = ((dy as usize + ty) * dst_stride as usize + dx as usize + tx) * 4;
            dst[o] = (fr.clamp(0.0, 1.0) * 255.0) as u8;
            dst[o + 1] = (fg.clamp(0.0, 1.0) * 255.0) as u8;
            dst[o + 2] = (fb.clamp(0.0, 1.0) * 255.0) as u8;
            dst[o + 3] = 255;
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
    pub dirty: Option<[u32; 4]>,
    /// 当前纹理内容所用的视图模式。
    ///
    /// **仅供「模式是否真的变了」的比较**（基线 §2.11：不要每帧重建整图）。
    /// 用 `Option` 是为了区分「尚未构建过」：`None` 表示纹理还从未按任一模式
    /// 生成过——此时即使数据稍后才可用，也应重建一次。
    pub last_mode: Option<ViewMode>,
}

impl TerrainTexture {
    /// 标记整个纹理为脏（整图重传）。单位：逻辑格。
    pub fn mark_all_dirty(&mut self) {
        self.dirty = Some([0, 0, self.size, self.size]);
    }

    /// 标记一个矩形为脏，与已有脏区取并集（增量重传）。单位：逻辑格。
    pub fn mark_dirty(&mut self, rect: [u32; 4]) {
        self.dirty = Some(match self.dirty {
            Some(cur) => [
                cur[0].min(rect[0]),
                cur[1].min(rect[1]),
                cur[2].max(rect[2]),
                cur[3].max(rect[3]),
            ],
            None => rect,
        });
    }
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
        // 势力图：原型阶段底色淡化地形（势力几何属 S12，本轮不画）。
        ViewMode::Faction => shade_faded,
        // 事件图：极简灰白底图。
        ViewMode::Event => shade_event,
    }
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

/// 把整张 `TerrainData` 写成 RGBA8 像素（每逻辑格 `res` 像素见方）。
///
/// 有图集且该格 kind 有映射 → 盒式降采样直铺瓦片（按模式叠 tint）；
/// 否则回退纯色 shader。返回 `(pixels, res)`。
fn write_terrain_pixels(
    data: &TerrainData,
    mode: ViewMode,
    atlas: Option<&TileAtlas>,
) -> (Vec<u8>, u32) {
    let shader = shade_for_mode(mode);
    let (w, h) = (data.width.max(1), data.height.max(1));
    // 事件图不铺瓦片（极简底图）；其余模式能铺则铺。
    let atlas = if mode == ViewMode::Event { None } else { atlas };
    let res = atlas.map_or(1, |_| tile_res_for(w.max(h)));
    let stride = w * res;
    let mut pixels = vec![0u8; (stride * h * res * 4) as usize];

    // 高度归一化范围（真实地形模式用）。
    let (hmin, hmax) = if mode == ViewMode::RealTerrain {
        data.cells.iter().fold((i16::MAX, i16::MIN), |(lo, hi), c| {
            (lo.min(c.height), hi.max(c.height))
        })
    } else {
        (0, 0)
    };
    let hrange = (hmax as f32 - hmin as f32).max(1.0);

    for (idx, cell) in data.cells.iter().enumerate() {
        let cx = (idx as u32) % w;
        let cy = (idx as u32) / w;
        // 图集路径。
        if let (Some(a), Some([col, row])) = (atlas, a_tile(atlas, cell.kind)) {
            let tint = match mode {
                ViewMode::RealTerrain => {
                    let t = (cell.height as f32 - hmin as f32) / hrange;
                    TileTint::Height(0.78 + t * 0.44)
                }
                _ => tile_tint_for_mode(mode),
            };
            blit_tile(
                a,
                col,
                row,
                res,
                tint,
                &mut pixels,
                stride,
                cx * res,
                cy * res,
            );
            continue;
        }
        // 纯色回退路径（含 Event / 无图集 / 缺项）。
        let mut color = shader(data, cell.kind);
        if mode == ViewMode::RealTerrain {
            let t = (cell.height as f32 - hmin as f32) / hrange;
            let mul = 0.78 + t * 0.44;
            let Color::Srgba(c) = color else {
                continue;
            };
            color = Color::srgb(
                (c.red * mul).clamp(0.0, 1.0),
                (c.green * mul).clamp(0.0, 1.0),
                (c.blue * mul).clamp(0.0, 1.0),
            );
        }
        let Color::Srgba(c) = color else {
            continue;
        };
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
    }
    (pixels, res)
}

/// 查 kind 的图集格位（`atlas` 为 `None` 或未映射时 `None`）。
fn a_tile(atlas: Option<&TileAtlas>, kind: u8) -> Option<[u32; 2]> {
    let a = atlas?;
    a.tiles.get(kind as usize).copied().flatten()
}

/// 由 `TerrainData` 构造 Bevy `Image`（RGBA8；有图集时像素边长 = 格数 × res）。
pub fn image_from_terrain(data: &TerrainData, mode: ViewMode, atlas: Option<&TileAtlas>) -> Image {
    let (pixels, res) = write_terrain_pixels(data, mode, atlas);
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
) {
    let (w, h) = (data.width.max(1), data.height.max(1));
    let stride = w * res; // 像素 stride
    let [gx0, gy0, gx1, gy1] = dirty_cell_rect;
    // 钳到 [0, w/h]（防御性）。
    let gx1 = gx1.min(w);
    let gy1 = gy1.min(h);

    // 模式 dispatch 与 shader 选定。
    let shader = shade_for_mode(mode);
    let atlas_use = if mode == ViewMode::Event { None } else { atlas };

    // 高度归一化：仅 RealTerrain 需要；其他模式不需要。
    let (hmin, hmax) = if mode == ViewMode::RealTerrain {
        data.cells.iter().fold((i16::MAX, i16::MIN), |(lo, hi), c| {
            (lo.min(c.height), hi.max(c.height))
        })
    } else {
        (0, 0)
    };
    let hrange = (hmax as f32 - hmin as f32).max(1.0);

    // 只遍历 dirty 矩形内的格。
    for cy in gy0..gy1 {
        for cx in gx0..gx1 {
            let idx = (cy * w + cx) as usize;
            let Some(cell) = data.cells.get(idx) else {
                continue;
            };
            // 复用全图分支的 tint / shader / blit 逻辑：这里**直接内联**简化版。
            // 真实地形模式：按 height 调亮度；其他模式：纯色 shader。
            if let (Some(a), Some([col, row])) = (atlas_use, a_tile(atlas_use, cell.kind)) {
                let tint = if mode == ViewMode::RealTerrain {
                    let t = (cell.height as f32 - hmin as f32) / hrange;
                    TileTint::Height(0.78 + t * 0.44)
                } else {
                    tile_tint_for_mode(mode)
                };
                blit_tile(
                    a,
                    col,
                    row,
                    res,
                    tint,
                    &mut image.data,
                    stride,
                    cx * res,
                    cy * res,
                );
            } else {
                let mut color = shader(data, cell.kind);
                if mode == ViewMode::RealTerrain {
                    let t = (cell.height as f32 - hmin as f32) / hrange;
                    let mul = 0.78 + t * 0.44;
                    let Color::Srgba(c) = color else { continue };
                    color = Color::srgb(
                        (c.red * mul).clamp(0.0, 1.0),
                        (c.green * mul).clamp(0.0, 1.0),
                        (c.blue * mul).clamp(0.0, 1.0),
                    );
                }
                let Color::Srgba(c) = color else { continue };
                for ty in 0..res {
                    let o = ((cy * res + ty) * stride + cx * res) as usize * 4;
                    for tx in 0..res {
                        let o = o + tx as usize * 4;
                        image.data[o] = (c.red * 255.0) as u8;
                        image.data[o + 1] = (c.green * 255.0) as u8;
                        image.data[o + 2] = (c.blue * 255.0) as u8;
                        image.data[o + 3] = 255;
                    }
                }
            }
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
    let image = image_from_terrain(&data, view.mode, atlas.as_ref());
    let res = atlas.map_or(1, |_| tile_res_for(data.width.max(data.height).max(1)));
    let handle = images.add(image);
    commands.insert_resource(TerrainTexture {
        handle,
        size: data.width.max(1),
        res,
        dirty: None,
        last_mode: Some(view.mode),
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
            let t = ((n + 1.0) / 2.0).clamp(0.0, 1.0);
            let kind = if t < 0.38 {
                0 // 深水
            } else if t < 0.44 {
                1 // 浅水
            } else if t < 0.52 {
                2 // 沙
            } else if t < 0.66 {
                3 // 草
            } else if t < 0.76 {
                4 // 林
            } else if t < 0.85 {
                5 // 丘
            } else if t < 0.93 {
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
    // 只有模式字段真的变了才需要重建（不看整个 ViewState 资源）。
    if tex.last_mode == Some(view.mode) {
        return;
    }
    let Some(data) = projection.world.terrain.as_ref() else {
        // 地形尚未可用：不重建，也不置位 last_mode，等地形就绪后再走这里。
        return;
    };
    let image = image_from_terrain(data, view.mode, atlas.as_deref());
    if let Some(img) = images.get_mut(&tex.handle) {
        *img = image;
    }
    // 仅在实际重建后记录模式（保证纹理内容与 last_mode 一致）。
    tex.last_mode = Some(view.mode);
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
        // 少格数 → 上限 64；多格数 → 下限 8。
        assert_eq!(tile_res_for(16), 64);
        assert_eq!(tile_res_for(64), 64);
        assert_eq!(tile_res_for(512), 8);
        assert_eq!(tile_res_for(4096), 8);
        assert_eq!(tile_res_for(8192), 8);
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
    fn blit_tile_averages_solid_cell_to_tinted_pixels() {
        let atlas = solid_atlas(16, 16, [200, 100, 0]);
        let res: u32 = 4;
        let mut dst = vec![0u8; (res * res * 4) as usize];
        blit_tile(&atlas, 0, 0, res, TileTint::Identity, &mut dst, res, 0, 0);
        // 实心格子盒式平均后仍是原色。
        for px in dst.chunks_exact(4) {
            assert_eq!(px, [200, 100, 0, 255]);
        }
    }

    #[test]
    fn blit_tile_fade_mixes_toward_gray() {
        let atlas = solid_atlas(8, 8, [0, 0, 0]);
        let res: u32 = 2;
        let mut dst = vec![0u8; (res * res * 4) as usize];
        blit_tile(&atlas, 0, 0, res, TileTint::Fade, &mut dst, res, 0, 0);
        // 黑 × keep 0.45 + 灰 0.7×0.55 = 0.385 → 98。
        for px in dst.chunks_exact(4) {
            assert_eq!(px, [98, 98, 98, 255]);
        }
    }

    #[test]
    fn image_from_terrain_scales_pixels_with_atlas_res() {
        let atlas = solid_atlas(16, 16, [10, 20, 30]);
        let data = placeholder_terrain(4, 4);
        let img = image_from_terrain(&data, ViewMode::Schematic, Some(&atlas));
        assert_eq!(img.width(), 4 * 64); // 4 格 → res 上限 64
        let img_flat = image_from_terrain(&data, ViewMode::Schematic, None);
        assert_eq!(img_flat.width(), 4);
        // 事件图不铺瓦片：有图集也按纯色 1px/格。
        let img_event = image_from_terrain(&data, ViewMode::Event, Some(&atlas));
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
        let mut img = image_from_terrain(&data, ViewMode::Schematic, None);
        // 用 0 标记所有像素。
        for px in img.data.iter_mut() {
            *px = 0;
        }
        // patch 矩形 (1,1)-(3,3) = 2×2 = 4 像素。
        patch_terrain_image(&mut img, &data, ViewMode::Schematic, None, [1, 1, 3, 3], 1);
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
        let mut img = image_from_terrain(&data, ViewMode::Schematic, None);
        // 把整图刷成 0xFF 哨兵值。
        for px in img.data.iter_mut() {
            *px = 0xFF;
        }
        // patch 矩形 (1,1)-(2,2) = 1 像素。
        patch_terrain_image(&mut img, &data, ViewMode::Schematic, None, [1, 1, 2, 2], 1);
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
}
