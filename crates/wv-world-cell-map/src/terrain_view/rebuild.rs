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
//! - [`SynthCtx`] / [`PendingTerrainSynth`] / [`tick_basemap_fadeout`] / [`BasemapFadeOut`] / [`LEVEL_FADE_SECS`]
//!
//! 桥接系统 `rebuild_texture_on_mode_change` / `land_pending_terrain_synth`
//! 在 wv-render `cellmap_bridge`（它们读 `ViewState` / `WorldProjection`）。

use bevy::prelude::*;

use crate::palette::CellPack;
use crate::shade::ShadeMode;
use crate::state::GridConfig;

use super::isoline_overlay::terrain_data_hash;
use super::terrain_resolve::active_terrain;
use super::terrain_texture::TerrainTexture;

/// 后台合成的上下文指纹：落地时校验「世界还是那个世界」。
#[derive(Debug, Clone)]
pub struct SynthCtx {
    pub mode: ShadeMode,
    pub soften: bool,
    /// 合成时生效的**主题包 id**（ADR-0023 M2：权威源 = `view.theme_pack_id`）。
    ///
    /// 为什么不用 `MapPalette` 作指纹：主题包可以是外部 crate 提供的（不止
    /// Daylight / Dusk 两档），`MapPalette` 表达不了。id 是稳定字符串标识。
    pub theme_id: &'static str,
    /// 合成时生效的**平面色包**（owned `CellPack`，`Copy` 语义等价——
    /// 异步闭包直接取用，无需 `'static` 捕获；ADR-0054 D4）。
    pub pack: CellPack,
    /// 被合成地形所属的图（`active_terrain` 给出的来源图）。
    pub map: Option<wv_core::Id>,
    /// 内容指纹（height+kind FNV-1a，与等值线缓存同源）。
    pub data_hash: u64,
    /// Topographic mode 专用：(p33, p66) 两个 height 阈值（米）。
    ///
    /// **仅 Topographic mode 有效**；其它 mode = `(0, 0)` 哨兵。
    /// 任务提交时由 [`rebuild_texture_on_mode_change`] 计算（先查
    /// `ViewState::topographic_thresholds` 缓存，未命中按本图 cell heights
    /// 临时求一次）；本字段保证后台着色纯函数无资源依赖。
    ///
    /// [ADR-0039]: ../../../../docs/adr/ADR-0039-topographic-viewmode.md
    pub topographic_thresholds: (i16, i16),
}

impl PartialEq for SynthCtx {
    /// **只比 `theme_id` 与内容指纹**——`pack` 由 `theme_id` 唯一决定，
    /// 且 `ThemePack` 不实现 `PartialEq`（含 f32 字段）。
    ///
    /// **例外：分层设色强度**（ADR-0055）不是主题字段（会话态骑在 pack 上），
    /// 必须单独比——否则拖动强度滑条时在途任务会被误判成「同一份任务」。
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.soften == other.soften
            && self.theme_id == other.theme_id
            && self.map == other.map
            && self.data_hash == other.data_hash
            && self.topographic_thresholds == other.topographic_thresholds
            && (self.pack.hypsometric - other.pack.hypsometric).abs() <= 1e-6
    }
}

/// 后台合成结果：像素 + 每格分辨率 + 指纹。
pub struct SynthResult {
    pub pixels: Vec<u8>,
    pub res: u32,
    pub ctx: SynthCtx,
}

/// 在途的整图合成任务（`Assets<Image>` 非线程安全，资产写入必须回主线程）。
#[derive(Resource, Default)]
pub struct PendingTerrainSynth {
    pub task: Option<bevy::tasks::Task<SynthResult>>,
    pub ctx: Option<SynthCtx>,
    /// 已经为哪次换图发过淡出精灵：异步落地期间 `tex.last_map` 仍是旧图，
    /// 防止每次重排（例如落地前又切了模式）重复克隆旧底图。
    pub fade_for_map: Option<wv_core::Id>,
}

pub fn ctx_matches_world(
    ctx: &SynthCtx,
    world: &wv_core::World,
    current: Option<&wv_core::Id>,
) -> bool {
    let Some((node, data)) = active_terrain(world, current) else {
        return false;
    };
    ctx.map.as_ref() == Some(&node.id) && ctx.data_hash == terrain_data_hash(data)
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
    let size = Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size;
    let center = grid.raster_origin + size * 0.5;
    commands.spawn((
        BasemapFadeOut { alpha: 1.0 },
        Sprite {
            image: handle,
            custom_size: Some(size),
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

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    fn test_world_with_terrain(id: &str, size: u32, height: i16) -> (wv_core::World, wv_core::Id) {
        let map = wv_core::Id::new_unchecked(id);
        let mut world = wv_core::World::default();
        world.maps.push(wv_core::map::MapNode {
            id: map.clone(),
            level: wv_core::map::MapLevel::World,
            ..Default::default()
        });
        let cells = vec![
            TerrainCellData {
                height,
                surface: 0,
                biome: 0,
            };
            (size * size) as usize
        ];
        world.set_terrain(
            map.clone(),
            TerrainData {
                width: size,
                height: size,
                cells,
            },
        );
        (world, map)
    }

    fn ctx_for(world: &wv_core::World, map: &wv_core::Id) -> SynthCtx {
        let data = world.terrain_of(map).expect("地形存在");
        SynthCtx {
            mode: ShadeMode::VectorFlat,
            soften: false,
            theme_id: "fixture",
            pack: CellPack::fixture(),
            map: Some(map.clone()),
            data_hash: terrain_data_hash(data),
            topographic_thresholds: (0, 0),
        }
    }

    /// 落地校验：完成时刻世界已换（data_hash / 来源图变）→ 拒绝落地。
    #[test]
    fn ctx_validation_rejects_stale_world() {
        let (world, id_a) = test_world_with_terrain("map_a", 8, -100);
        let ctx = ctx_for(&world, &id_a);
        assert!(ctx_matches_world(&ctx, &world, Some(&id_a)), "同世界应放行");

        let mut stale = ctx.clone();
        stale.data_hash = stale.data_hash.wrapping_add(1);
        assert!(
            !ctx_matches_world(&stale, &world, Some(&id_a)),
            "内容指纹变（笔刷已改地形）应拒绝"
        );

        let mut other_map = ctx.clone();
        other_map.map = Some(wv_core::Id::new_unchecked("map_b"));
        assert!(
            !ctx_matches_world(&other_map, &world, Some(&id_a)),
            "来源图变应拒绝"
        );
    }
}
