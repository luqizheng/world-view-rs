//! 启动系统：`setup_terrain_texture` + 占位地形生成 `placeholder_terrain`。
//!
//! ## 职责
//! - `setup_terrain_texture` —— Startup 期由工程地形数据建纹理；无地形则占位。
//! - `placeholder_terrain` —— 用 Perlin 生成确定性占位场（无工程数据时兜底）。
//!
//! ## 边界（不做什么）
//! - **不监听模式变化** —— 那是 [`super::rebuild::rebuild_texture_on_mode_change`]。
//! - **不调淡出** —— 那是 [`super::rebuild`]。
//!
//! ## 主要导出
//! - [`setup_terrain_texture`] / [`placeholder_terrain`]

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{DrillState, ViewState, WorldProjection};

use super::cells::image_from_terrain;
use super::terrain_resolve::active_terrain_or_placeholder;
use super::terrain_texture::TerrainTexture;

use wv_core::terrain::TerrainData;

/// 启动时由工程的地形数据建纹理。若无地形，建一张占位（避免空句柄）。
///
/// Phase 2 起底图**属于某张图**：这里取「当前图向上的第一张自带栅格」，
/// 并把来源图 id 记进 [`TerrainTexture::last_map`]。
pub fn setup_terrain_texture(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
) {
    let atlas = super::tile_atlas::ensure_atlas(&mut commands);
    let (map, data) = active_terrain_or_placeholder(&mut projection.world, drill.current(), 64);
    let image = image_from_terrain(
        &data,
        view.mode,
        atlas.as_ref(),
        view.terrain_soften,
        crate::theme::palette::colors(view.map_palette),
        crate::theme::theme_pack::builtin::for_palette(view.map_palette).treatment,
    );
    commands.insert_resource(new_terrain_texture(
        &mut images,
        image,
        &data,
        map,
        view.mode,
        view.terrain_soften,
        view.map_palette,
    ));
}

/// 创建新的地形纹理资源。分辨率元数据从实际 Image 反推，避免第二套推断逻辑。
///
/// `map` = 这份地形属于哪张图（占位纹理传 `None`），记进 [`TerrainTexture::last_map`]。
#[allow(clippy::too_many_arguments)]
pub fn new_terrain_texture(
    images: &mut Assets<Image>,
    image: Image,
    data: &TerrainData,
    map: Option<wv_core::Id>,
    mode: crate::state::ViewMode,
    soften: bool,
    palette: crate::theme::palette::MapPalette,
) -> TerrainTexture {
    let res = image.width() / data.width.max(1);
    TerrainTexture {
        handle: images.add(image),
        size: data.width.max(1),
        height: data.height.max(1),
        res,
        dirty: None,
        chunks_dirty: None,
        last_mode: Some(mode),
        last_soften: soften,
        last_palette: palette,
        last_map: map,
    }
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
            // 分档 →（地表，群系）：显示槽位经 `display_kind` 复现旧档位观感。
            // 深水/浅水共用 (水, 无群系)——水的层次由显示层按海拔渐变给出。
            let (surface, biome) = if t < 0.33 {
                (0u8, 0u8) // 水（原深水/浅水两档）
            } else if t < 0.44 {
                (1, wv_terrain::biome::BIOME_DESERT) // 沙
            } else if t < 0.68 {
                (1, wv_terrain::biome::BIOME_GRASSLAND) // 草
            } else if t < 0.79 {
                (1, wv_terrain::biome::BIOME_FOREST) // 林
            } else if t < 0.87 {
                (1, wv_terrain::biome::BIOME_HILLS) // 丘
            } else if t < 0.94 {
                (1, wv_terrain::biome::BIOME_MOUNTAINS) // 山
            } else {
                (1, wv_terrain::biome::BIOME_SNOWFIELD) // 雪
            };
            cells.push(wv_core::terrain::TerrainCellData {
                height: (t * 2000.0) as i16,
                surface,
                biome,
            });
        }
    }
    TerrainData {
        width: w,
        height: h,
        cells,
    }
}
