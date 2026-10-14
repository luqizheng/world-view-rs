//! 换级重建与淡出：`rebuild_texture_on_mode_change` + `BasemapFadeOut` + `tick_basemap_fadeout`。
//!
//! ## 职责
//! - `rebuild_texture_on_mode_change` —— 模式 / 来源图 / 淡化开关任一变了就重建。
//! - `spawn_basemap_fadeout` —— 换图那一刻克隆旧底图像素铺淡出精灵。
//! - `tick_basemap_fadeout` —— 逐帧推进淡出，归零 despawn。
//! - `advance_fade` —— 淡出步进（私有）。
//!
//! ## 边界（不做什么）
//! - **不调图像合成** —— 那在 [`super::cells`]。
//!
//! ## 主要导出
//! - [`rebuild_texture_on_mode_change`] / [`tick_basemap_fadeout`] / [`BasemapFadeOut`] / [`LEVEL_FADE_SECS`]

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{DrillState, GridConfig, ViewState, WorldProjection};

use super::cells::image_from_terrain;
use super::terrain_resolve::{active_terrain, replace_terrain_texture};
use super::terrain_texture::TerrainTexture;
use super::tile_atlas::TileAtlas;

/// 当**视图模式真的变化**时重建整张纹理（换的是眼镜，不是世界，基线 §1.10）。
///
/// ## 为什么不能监听 `Res<ViewState>::is_changed()`（缺陷修复）
///
/// `ViewState` 是「大杂烩资源」：既有低频的 `mode`，也有**每帧无条件写入**的
/// `cursor_world`（见 [`crate::render::camera::track_cursor_world`]）。对 `ResMut<T>` 的
/// `deref_mut` 写入会标记资源 changed，于是**只要鼠标在地图上移动，
/// `view.is_changed()` 每帧为真**，整张 512×512 纹理就被逐格重着色并重传 GPU。
///
/// 这违反基线 §2.11「不要每帧重建整图」，也违反 `AGENTS.md` §3.4 的拉动式刷新
/// 原则（刷新应由「模式」这一语义输入驱动，而不是「整个 ViewState 资源」变化）。
///
/// 因此改为比较 `tex.last_mode` 与当前 `view.mode`，**只有模式字段真的变了才重建**。
///
/// Phase 2 补一条同类比较：`tex.last_map` 与「当前应有的来源图」。换图（钻入一座
/// 自带地形的城市）与换模式一样是**语义输入变化**，必须重建；否则会继续画着
/// 世界图的底图、而 `grid` 已按城市帧对齐，地形与标记错开整幅。
///
/// ## 边界：首次地形可用
///
/// 若地形数据尚不可用（`terrain` 为 `None`），直接返回，**不写 `last_mode`**——
/// 否则地形稍后加载完时，`last_mode` 已等于当前 mode，会误判「没变」而永不重建。
/// `last_mode` 仅在**真正生成了纹理之后**才更新。
/// 系统参数即函数参数（Bevy 注入），7 个都是必要资源——与 sync_street_layers 同理由允许超参。
#[allow(clippy::too_many_arguments)]
pub fn rebuild_texture_on_mode_change(
    mut commands: Commands,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    atlas: Option<Res<TileAtlas>>,
    mut tex: ResMut<TerrainTexture>,
    mut images: ResMut<Assets<Image>>,
) {
    // 模式、淡化开关、来源图三者任一真的变了才需要重建（不看整个 ViewState 资源）。
    let map_changed = match active_terrain(&projection.world, drill.current()) {
        Some((node, _)) => tex.last_map.as_ref() != Some(&node.id),
        None => false,
    };
    if !map_changed && tex.last_mode == Some(view.mode) && tex.last_soften == view.terrain_soften {
        return;
    }
    let Some((node, data)) = active_terrain(&projection.world, drill.current()) else {
        // 地形尚未可用：不重建，也不置位 last_mode，等地形就绪后再走这里。
        return;
    };
    // 换级淡化（ADR-0009 §2⑦ / README §12-25）：只有**换图**才淡出旧底图；
    // 换模式/换淡化开关是「换眼镜」，保持既有的瞬时重建（淡化是 12-25 的收敛项）。
    // 注意此刻 `grid` 仍是旧图的栅格口径（sync_grid_config 在本系统之后运行），
    // 淡出精灵恰好盖住旧底图原本的位置与幅面。
    if map_changed && tex.last_map.is_some() {
        spawn_basemap_fadeout(&mut commands, &mut images, &tex, &grid);
    }
    let image = image_from_terrain(data, view.mode, atlas.as_deref(), view.terrain_soften);
    replace_terrain_texture(
        &mut tex,
        &mut images,
        image,
        data,
        Some(node.id.clone()),
        view.mode,
        view.terrain_soften,
    );
    // 整图内容已整体替换：chunk 层需要全量重贴（不经 `dirty`，避免
    // `flush_terrain_texture` 下帧再做一次重复的全图重建）。
    tex.chunks_dirty = Some([0, 0, tex.size, tex.size]);
    // 仅在实际重建后记录（保证纹理内容与记录一致）。
    tex.last_mode = Some(view.mode);
    tex.last_soften = view.terrain_soften;
    tex.last_map = Some(node.id.clone());
}

/// 换级淡化时长（秒）：旧底图整幅淡出、露出新底图。
///
/// 第一版取 0.4s——比「闪一下」（<0.2s）有过渡感，比「拖沓」（>0.8s）短，
/// 与常见地图应用的级间过渡同一量级。是否可调/可关注册于 README §12-25。
pub const LEVEL_FADE_SECS: f32 = 0.4;

/// 旧底图淡出精灵的 Z：盖住新 chunk 层（`TERRAIN_Z = 0.0`），
/// 但在道路（`ROAD_Z = 0.1`）与全部矢量层之下——过渡只属于底图，
/// 标记/街道/文字从第一帧起就画在新图的正确位置上。
pub const FADEOUT_Z: f32 = 0.05;

/// 换级时旧底图的淡出精灵标记（防重复 + 淡出系统的过滤口径）。
#[derive(Component)]
pub struct BasemapFadeOut {
    /// 剩余不透明度 `1.0 → 0.0`；归零即 despawn。
    pub alpha: f32,
}

/// 淡出步进（纯函数，便于测试）：返回下一帧 alpha，`None` = 已到 0，应 despawn。
pub fn advance_fade(alpha: f32, dt: f32) -> Option<f32> {
    let next = alpha - dt / LEVEL_FADE_SECS;
    (next > 0.0).then_some(next)
}

/// 换级那一刻：把**旧底图像素**克隆成一份淡出资产并铺在新底图之上。
///
/// 为什么必须克隆：[`replace_terrain_texture`] 优先**复用旧句柄**原地覆写像素，
/// 不克隆的话淡出精灵与主底图共用同一张 `Image`，旧画面会瞬间变成新画面。
/// 克隆是一帧一次性开销（整图 memcpy），换级本来就伴随整图重建，可接受。
///
/// 摆位用调用时刻的 `grid`（此刻仍是**旧图**的栅格原点/格长——本系统排在
/// [`sync_grid_config`] 之前），所以淡出精灵精确盖住旧底图原本的位置与幅面。
pub fn spawn_basemap_fadeout(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    tex: &TerrainTexture,
    grid: &GridConfig,
) {
    let Some(old) = images.get(&tex.handle) else {
        return;
    };
    let handle = images.add(old.clone());
    let size = tex.size as f32 * grid.cell_size;
    let center = grid.raster_origin + Vec2::splat(size * 0.5);
    commands.spawn((
        BasemapFadeOut { alpha: 1.0 },
        Sprite {
            image: handle,
            custom_size: Some(Vec2::splat(size)),
            color: Color::WHITE,
            ..Default::default()
        },
        Transform::from_xyz(center.x, center.y, FADEOUT_Z),
    ));
}

/// 淡出推进：逐帧降低旧底图精灵的不透明度，归零即回收实体。
pub fn tick_basemap_fadeout(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Sprite, &mut BasemapFadeOut)>,
) {
    let dt = time.delta_secs();
    for (entity, mut sprite, mut fade) in &mut q {
        match advance_fade(fade.alpha, dt) {
            Some(next) => {
                fade.alpha = next;
                sprite.color = Color::srgba(1.0, 1.0, 1.0, next);
            }
            None => {
                commands.entity(entity).despawn();
            }
        }
    }
}
