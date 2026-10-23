//! 地形数据解析：`texture_terrain` / `active_terrain` / `active_terrain_or_placeholder` / `replace_terrain_texture` / `force_rebuild`。
//!
//! ## 职责
//! - `texture_terrain` —— 优先按 `tex.last_map` 取数，拿不到回退 `active_terrain`。
//! - `active_terrain` —— 当前图沿 `parent` 向上找**第一张自带栅格**的图。
//! - `active_terrain_or_placeholder` —— 兜底版：根图塞占位地形并返回。
//! - `replace_terrain_texture` —— 用新 Image 替换既有纹理资源（优先复用 Handle）。
//! - `force_rebuild` —— 清掉 `last_mode/last_map`，让下一帧全图重建。
//!
//! ## 边界（不做什么）
//! - **不直接读盘**——`World.terrain_of(map)` 由 `wv-core` 提供。
//! - **不画到屏幕**。

use bevy::prelude::*;

use wv_core::terrain::TerrainData;

use crate::shade::ShadeMode;

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
#[allow(clippy::too_many_arguments)]
pub fn replace_terrain_texture(
    tex: &mut TerrainTexture,
    images: &mut Assets<Image>,
    image: Image,
    data: &TerrainData,
    map: Option<wv_core::Id>,
    mode: ShadeMode,
    soften: bool,
    theme_id: &'static str,
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
        height: data.height.max(1),
        res,
        dirty: None,
        chunks_dirty: None,
        last_mode: Some(mode),
        last_soften: soften,
        last_theme_id: theme_id,
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

/// 强制下一帧全图重建（2026-10-14 用户需求）。
///
/// 复位 `last_mode / last_soften / last_map`，并标全图脏。
/// [`super::rebuild::rebuild_texture_on_mode_change`] 看到三个「已知」
/// 字段都被清掉，必然走 `image_from_terrain` 整图重建路径。
///
/// **不**直接 `image_from_terrain` 写 `Image`——那是 `rebuild_texture_on_mode_change`
/// 在 `Update` 链里的活；本函数只**解锁**比较，渲染侧那一帧自己会重建。
/// 改 `Image` 资源会跨帧操作 GPU 上传链路（bevy 0.15 的 Image 资源变更
/// 在下一帧才上传），由 Update 链去管理更稳。
///
/// ## 与 `mark_all_dirty` 的区别
///
/// `mark_all_dirty` 仅设脏矩形，走 `flush_terrain_texture` 的 patch 路径——
/// 它复用既有 `Image` 资源，**不**调 `image_from_terrain`，所以如果上
/// 一帧的纹理本就和当前地形对不上（"数据改了、纹理没跟上"的场景），
/// patch 只是把已失真的源数据再 patch 一次。**必须**配合本函数把
/// `last_mode` 清掉，让 `rebuild_texture_on_mode_change` 跑全图重建。
pub fn force_rebuild(tex: &mut TerrainTexture) {
    tex.last_mode = None;
    tex.last_soften = false; // 与 `Default` 一致即可；不与 view.terrain_soften 比
    tex.last_theme_id = ""; // 空串 = 不可能命中的 id → 必走全图重建
    tex.last_map = None;
    tex.mark_all_dirty();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shade::ShadeMode;

    /// 复位三个「已知」字段 → `rebuild_texture_on_mode_change` 必走全图重建路径。
    #[test]
    fn force_rebuild_clears_all_three_known_fields() {
        let mut tex = TerrainTexture {
            handle: Handle::default(),
            size: 64,
            height: 64,
            res: 4,
            dirty: None,
            chunks_dirty: None,
            last_mode: Some(ShadeMode::VectorFlat),
            last_soften: true,
            last_theme_id: "fixture",
            last_map: Some(wv_core::Id::new_unchecked("map_x")),
        };
        force_rebuild(&mut tex);
        assert!(tex.last_mode.is_none(), "last_mode 应被清为 None");
        assert!(!tex.last_soften, "last_soften 应被重置为 false");
        assert!(tex.last_map.is_none(), "last_map 应被清为 None");
        assert!(tex.dirty.is_some(), "dirty 矩形应被设置");
        // 全图 = [0, 0, size, size]
        assert_eq!(tex.dirty, Some([0, 0, tex.size, tex.height]));
    }
}
