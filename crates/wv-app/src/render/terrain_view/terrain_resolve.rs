//! 地形数据解析：`texture_terrain` / `active_terrain` / `active_terrain_or_placeholder` / `replace_terrain_texture`。
//!
//! ## 职责
//! - `texture_terrain` —— 优先按 `tex.last_map` 取数，拿不到回退 `active_terrain`。
//! - `active_terrain` —— 当前图沿 `parent` 向上找**第一张自带栅格**的图。
//! - `active_terrain_or_placeholder` —— 兜底版：根图塞占位地形并返回。
//! - `replace_terrain_texture` —— 用新 Image 替换既有纹理资源（优先复用 Handle）。
//!
//! ## 边界（不做什么）
//! - **不直接读盘**——`World.terrain_of(map)` 由 `wv-core` 提供。
//! - **不画到屏幕**。
//!
//! ## 主要导出
//! - [`texture_terrain`] / [`active_terrain`] / [`active_terrain_or_placeholder`] / [`replace_terrain_texture`]

#![cfg(feature = "app")]

use bevy::prelude::*;

use wv_core::terrain::TerrainData;

use crate::state::ViewMode;

use super::terrain_texture::TerrainTexture;

/// 纹理当前承载的**哪张图的**地形数据。
///
/// 优先 `tex.last_map`（纹理内容与它严格一致），拿不到时退回
/// [`active_terrain`]（`current` 由调用方给，没有就给 `None`）。
///
/// ## 为什么增量 patch / 撤销 diff 必须走这里
///
/// Phase 2 起底图按图存且可向上继承：城市视图可能显示的是**世界图**的地形。
/// 若 patch / diff 固定读「当前图」的地形，而纹理里装的是父图的，就会拿另一张
/// 图的 cells 去算矩形（尺寸都可能不同），把纹理改花。
pub fn texture_terrain<'w>(
    tex: &TerrainTexture,
    world: &'w wv_core::World,
    current: Option<&wv_core::id::Id>,
) -> Option<&'w TerrainData> {
    if let Some(map) = tex.last_map.as_ref() {
        if let Some(data) = world.terrain_of(map) {
            return Some(data);
        }
    }
    active_terrain(world, current).map(|(_, data)| data)
}

/// 用新 Image 替换既有纹理资源。优先复用 Handle，项目切换 / 模式切换不再
/// 保留一串旧整图 source；资产不存在时回退为新增。
pub fn replace_terrain_texture(
    tex: &mut TerrainTexture,
    images: &mut Assets<Image>,
    image: Image,
    data: &TerrainData,
    map: Option<wv_core::Id>,
    mode: ViewMode,
    soften: bool,
) {
    let res = image.width() / data.width.max(1);
    let handle = if images.get_mut(&tex.handle).is_some() {
        if let Some(mut old) = images.get_mut(&tex.handle) {
            *old = image;
        }
        tex.handle.clone()
    } else {
        images.add(image)
    };
    *tex = TerrainTexture {
        handle,
        size: data.width.max(1),
        res,
        dirty: None,
        chunks_dirty: None,
        last_mode: Some(mode),
        last_soften: soften,
        last_map: map,
    };
}

/// 当前视图应当渲染哪张图的底图：从当前图沿 `parent` 向上找**第一张自带栅格**的图。
///
/// ## 为什么是「向上继承」而不是「没有就空白」
///
/// 「缩放到街道」的目标是每张图都有自己的底图（ADR-0009 决策 ③），但工程是
/// 渐进长出来的：作者建了一张城市图、还没给它刷地形时，城市视图下应当继续看到
/// 父图（世界图）的那一块，而不是一片空白。于是规则定为：
///
/// - 当前图自带栅格 → 用它（**城市一旦有了 `terrain.bin`，纹理自动切过去**）；
/// - 否则沿 `parent` 向上找最近的一张；
/// - 都没有 → `None`（调用方用占位地形兜底）。
///
/// 返回的**来源图节点**同时也是 `grid.cell_size` / `grid.raster_origin` 的唯一
/// 依据——**栅格空间**的尺寸与原必须同源，否则地形四边形会和标记错开半幅
/// （基线 §2.15 的老坑）。注意**本图局部内容**不看这张图，而看当前图的
/// `grid.map_origin`（见 [`GridConfig`] 的两套原点表）。
///
/// 有环保护：最多走 `maps.len() + 1` 步。
pub fn active_terrain<'w>(
    world: &'w wv_core::World,
    current: Option<&wv_core::id::Id>,
) -> Option<(&'w wv_core::map::MapNode, &'w TerrainData)> {
    let mut id = current?.clone();
    for _ in 0..=world.maps.len() {
        let node = world.maps.iter().find(|m| m.id == id)?;
        if let Some(data) = world.terrain_of(&id) {
            return Some((node, data));
        }
        id = node.parent.clone()?;
    }
    None
}

/// [`active_terrain`] 的「兜底版」：整个世界都没有地形时，给**根图**塞一张
/// 占位地形并返回它。
///
/// 与 Phase 2 之前「`world.terrain = Some(placeholder)`」是同一行为，只是现在
/// 有了明确的宿主（根图）。写入 `World` 而非每次现造，是为了让笔刷/撤销也有一个
/// 稳定的落脚图，而不是一个「只存在于纹理里」的幽灵地形。
///
/// `placeholder_cells` = 兜底地形的边长（格）。各调用点原有口径不同
/// （启动 64、工程加载后 512），故由调用方传入，保持既有行为不变。
///
/// **启动早期的坑**：`DrillState` 尚未 `ensure_root` 时 `current` 是 `None`，
/// 这时必须退回**根图**再判断有没有地形——否则会把「当前图未知」误当成
/// 「世界上没有地形」，用占位地形覆盖掉刚加载出来的真实地形。
pub fn active_terrain_or_placeholder(
    world: &mut wv_core::World,
    current: Option<&wv_core::id::Id>,
    placeholder_cells: u32,
) -> (Option<wv_core::Id>, TerrainData) {
    let root = world
        .meta
        .as_ref()
        .map(|m| m.root_map.clone())
        .or_else(|| world.maps.first().map(|m| m.id.clone()));
    let start = current.cloned().or_else(|| root.clone());
    if let Some((node, data)) = active_terrain(world, start.as_ref()) {
        return (Some(node.id.clone()), data.clone());
    }
    let data = super::setup::placeholder_terrain(placeholder_cells, placeholder_cells);
    match root {
        Some(root) => {
            world.set_terrain(root.clone(), data.clone());
            (Some(root), data)
        }
        None => (None, data),
    }
}
