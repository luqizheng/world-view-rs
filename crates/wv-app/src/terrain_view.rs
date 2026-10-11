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
//! 每格按 `TerrainKind` 取 [`crate::palette::core_terrain`] 的纯色。
//! 真实地形模式（A）在本原型中同样使用色块 + 按高度做明度调制——
//! 完整的贴图管线属后续（K5：真实地形贴图可砍，但海拔编辑不砍）。

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::palette;
use crate::state::{GridConfig, ViewMode, ViewState, WorldProjection};
use wv_core::terrain::TerrainData;

/// 地形纹理句柄 + 脏块追踪。
#[derive(Resource)]
pub struct TerrainTexture {
    pub handle: Handle<Image>,
    /// 当前纹理的尺寸（每边的像素数 = 网格格数）。
    pub size: u32,
    /// 上次上传后累计的脏矩形（像素坐标 `[x0,y0,x1,y1]`，半开）。
    pub dirty: Option<[u32; 4]>,
}

impl TerrainTexture {
    /// 标记整个纹理为脏（整图重传）。
    pub fn mark_all_dirty(&mut self) {
        self.dirty = Some([0, 0, self.size, self.size]);
    }

    /// 标记一个矩形为脏，与已有脏区取并集（增量重传）。
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

/// 依据当前视图模式选择着色函数（基线 §1.10 四态）。
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

/// 把整张 `TerrainData` 写成 RGBA8 像素。
///
/// 返回 `(pixels, size)`。`origin` 用于高度调制的归一化（真实地形模式）。
fn write_terrain_pixels(data: &TerrainData, mode: ViewMode) -> Vec<u8> {
    let shader = shade_for_mode(mode);
    let (w, h) = (data.width, data.height);
    let mut pixels = vec![0u8; (w * h * 4) as usize];

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
        let mut color = shader(data, cell.kind);
        if mode == ViewMode::RealTerrain {
            // 明度调制：高 = 亮，低 = 暗，幅度 ±22%（光影基调左侧 45° 的廉价近似）。
            let t = (cell.height as f32 - hmin as f32) / hrange; // 0..1
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
        let o = idx * 4;
        pixels[o] = (c.red * 255.0) as u8;
        pixels[o + 1] = (c.green * 255.0) as u8;
        pixels[o + 2] = (c.blue * 255.0) as u8;
        pixels[o + 3] = 255;
    }
    pixels
}

/// 由 `TerrainData` 构造 Bevy `Image`（RGBA8）。
pub fn image_from_terrain(data: &TerrainData, mode: ViewMode) -> Image {
    let pixels = write_terrain_pixels(data, mode);
    Image::new(
        Extent3d {
            width: data.width.max(1),
            height: data.height.max(1),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        // 注意：Bevy 行序为「左上原点」，与我们的行优先一致。
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

/// 启动时由工程的地形数据建纹理。若无地形，建一张占位（避免空句柄）。
pub fn setup_terrain_texture(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    projection: Res<WorldProjection>,
    view: Res<ViewState>,
) {
    let data = projection
        .world
        .terrain
        .clone()
        .unwrap_or_else(|| placeholder_terrain(64, 64));
    let image = image_from_terrain(&data, view.mode);
    let handle = images.add(image);
    commands.insert_resource(TerrainTexture {
        handle,
        size: data.width.max(1),
        dirty: None,
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

/// 当视图模式变化时重建整张纹理（换的是眼镜，不是世界，基线 §1.10）。
pub fn rebuild_texture_on_mode_change(
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    tex: ResMut<TerrainTexture>,
    mut images: ResMut<Assets<Image>>,
) {
    if !view.is_changed() {
        return;
    }
    let Some(data) = projection.world.terrain.as_ref() else {
        return;
    };
    let image = image_from_terrain(data, view.mode);
    if let Some(img) = images.get_mut(&tex.handle) {
        *img = image;
    }
}

/// 网格几何的默认配置（与纹理尺寸挂钩）。
pub fn sync_grid_config(tex: Res<TerrainTexture>, mut grid: ResMut<GridConfig>) {
    if !tex.is_added() && !tex.is_changed() {
        return;
    }
    // 让初始视图「整图入窗」：cell_size 使网格边长约为 1200 世界单位。
    let target_world = 1200.0;
    grid.cell_size = (target_world / tex.size as f32).max(0.5);
    grid.origin = Vec2::new(
        -(tex.size as f32 * grid.cell_size) / 2.0,
        -(tex.size as f32 * grid.cell_size) / 2.0,
    );
}
