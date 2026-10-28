//! 分级缩放的「级」模型（ADR-0009 §2 决策 ⑤ / ⑦）。
//!
//! ## 职责
//! - 为每个**地图层级**（世界 → 区域 → 城市 → 城区 → 街道）提供独立缩放区间，
//!   锚在该图「全图适配」的缩放上，往里 32×、往外 `LEVEL_OVERVIEW`。
//! - 给 [`crate::camera`] 提供 [`LevelZoom::clamp`]：缩放到级边界时被夹紧，
//!   滚轮一格到底就停在内界 / 外界。
//!
//! ## 边界（不做什么）
//! - **不动相机**——只算区间；相机变换在 `camera` 模块。
//! - **不画下钻 UI**——下钻路径在 [`crate::drill`]；本模块按当前 `level` 选区间。
//! - **不触发进入子图**（ADR-0038）——撞内界继续往里滚只会被夹住停住；
//!   进入子图的唯一手势是双击钻入图标，飞入终点由 [`fly_in_scale`] 给出。
//!
//! ## 主要导出
//! - [`LevelZoom`] / `level_zoom` — 当前级的区间（按 `DrillState` 路径上的图算）。
//! - [`fly_in_scale`] — 双击钻入时相机过渡的终点缩放。
//! - [`ZoomCross`] / [`ZoomCrossRequest`] — 「撞外界继续往外滚 = 回父级」的意图载体。
//!
//! ## 模型（详见 ADR-0009）
//! 级 = 一张图，区间由这张图的尺寸定：
//!
//! 每个**地图层级**（世界 → 区域 → 城市 → 城区 → 街道，由 `drill` 的路径决定）
//! 自带一段缩放区间，锚在该图「全图适配」的缩放上：
//!
//! ```text
//! fit = max(图宽 / 窗口宽, 图高 / 窗口高)      // 世界单位 / 屏幕像素
//! 内界 min = fit / LEVEL_ZOOM_SPAN            // 往里 32×（ADR-0038 用户拍板；旧值 80×
//!                                             // 是「滚轮进城」的遗产，已废）
//! 外界 max = fit * LEVEL_OVERVIEW             // 往外留一点余量看到全图之外
//! ```
//!
//! 级越深，图的 `extent` 越小 → `fit` 越小 → **内界一路往里推**。总跨度因此是
//! 各级尺寸之比相乘，而不是每级常数的幂：
//!
//! | 级 | extent | fit（800px 高窗口） | [min, max] |
//! |---|---|---|---|
//! | 世界 | 4096 | 5.12 | [0.16, 8.0] |
//! | 城市 | 1024 | 1.28 | [0.04, 2.0] |
//! | 城区 | 256 | 0.32 | [0.01, 0.5] |
//! | 街道 | 64 | 0.08 | [0.0025, 0.125] |
//!
//! 世界级的外界 `8.0` **正好等于**历史 `SCALE_MAX`（标定依据：`LINE 5.12 × 1.5625 = 8.0`），
//! 内界 0.16 比历史 `SCALE_MIN = 0.1` **收紧** 1.6× —— ADR-0038 后往里不再进城，
//! 深放大没有内容支撑，32× 是刻意收浅（旧 80× 的 0.064 已废）。
//! 四级的绝对区间故意**互相重叠**：往外换级不改缩放（见下），区间只负责「本级允许
//! 停在哪」，重叠部分由当前级决定用哪一段。
//!
//! ## 往外换级为什么仍是连续的（ADR-0038 修正后仅存的方向）
//!
//! 子图的 `extent` 小于父图，故 `child.max < parent.max`：**滚到子图外界时，
//! 该缩放值早已落进父图的区间里**，换级不需要改 `scale` 一个像素——只是
//! 「现在显示哪张图的哪一块」换了。相机锚点仍是指针位置，手感是
//! 「继续往外拉，视野自然回到上一级」。
//!
//! **往里没有对称路径**（ADR-0038）：撞内界继续往里滚只会在内界停住——
//! 进入子图的唯一手势是双击钻入图标（子图是独立创作，不是父图的连续放大），
//! 飞入终点由 [`fly_in_scale`] 给出（改缩放到子图 `fit`），不再伪装连续。
//!
//! ## 为什么级由 `drill` 决定，而不是由 `scale` 反推
//!
//! 区间重叠意味着 `scale` 到 `current_map` 是多对多的，反推没有唯一解，
//! 而且会在边界抖动。级是**状态**（`DrillState` 的路径），`scale` 只是这一级里的
//! 取景。这与 P7 一致：不存「当前值」，当前值由状态求值。
//!
//! ## 地面尺度的诚实说明
//!
//! `coord` 的父子嵌套是**纯平移无缩放**，所以子图 `extent` 的世界单位数就是它
//! 在父图里占据的跨度。真实地理（Overture）要贴进来时，这个跨度必须与
//! `geographic_source.bbox` 自洽——那是数据侧的活（README §12-20），本模块只
//! 消费 `extent`。

use bevy::prelude::*;
use wv_core::World;

/// 级内往里可推进的倍数（内界 = `fit / LEVEL_ZOOM_SPAN`）。
///
/// **32 = 2026-10-19 ADR-0038 用户拍板**：旧的 80× 是「滚轮越界进城」设计的遗产——
/// 深放大的终点是「滑进子图」，级内糊一点无所谓。进城改双击图标后，深放大失去
/// 存在理由：矢量平涂底图（ADR-0012）与装饰（ADR-0031）在栅格分辨率之上没有
/// 内容支撑，80× 只会得到放大的色块。32× 取「糊线以下、刷格子仍可用」的折中：
/// 4096² 满配栅格（1 wu/格）在 720px 窗口约 5–6 px/格，1024² 栅格约 22 px/格。
/// 逐级相乘的总跨度仍由各级**尺寸比**提供（见模块文档表格），不依赖本常量。
pub const LEVEL_ZOOM_SPAN: f32 = 32.0;

/// 级内往外可退的宽松度（外界 = `fit * LEVEL_OVERVIEW`）。
///
/// `1.5625 = 8.0 / 5.12`：从历史 `SCALE_MAX = 8.0` 与 4096 图在 800px 窗口的
/// `fit = 5.12` 反推而来，使世界级的外界**恰好**等于历史常量——升级前后
/// 「能缩到多远」逐值相同，避免破坏作者既有手感（也可能被写进示例/测试）。
pub const LEVEL_OVERVIEW: f32 = 1.5625;

/// 图没有声明 `extent` 时的兜底边长（世界单位）。
///
/// 取 4096 是为了与历史行为一致（旧实现里 `sync_grid_config` 的 `unwrap_or(4096.0)`，
/// 以及渲染层唯一的地形栅格上界）。它**不是**一个推荐尺寸，只是「作者没写」时的
/// 保守回退。
pub const FALLBACK_MAP_SIZE: f32 = 4096.0;

/// 判定「是否已撞到边界」时的相对容差。
///
/// 滚轮是离散步进，`scale` 几乎不会精确等于 `min`/`max`，而 `clamp` 会把越界的
/// 值压到边界上；用一个千分之一量级的容差即可覆盖浮点误差与步进残差。
const LIMIT_EPSILON: f32 = 1e-3;

/// 当前级的缩放区间。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelZoom {
    /// 全图适配缩放（世界单位 / 屏幕像素）。
    pub fit: f32,
    /// 内界：继续往里滚就要换到更深的级。
    pub min: f32,
    /// 外界：继续往外滚就要换到更浅的级。
    pub max: f32,
}

/// 钻取过渡的目标缩放：整幅子图铺满窗口（`fit`），但不把更近的视野拉远。
///
/// 硬编码常量（曾经的 `4.0`）在分级后可能**越出子图区间**——例如 1024 的子图在
/// 800px 窗口里区间是 `[0.04, 2.0]`，`4.0` 在外面，于是双击进城后第一次滚轮
/// 会被 `clamp` 硬拽一下，LOD 与比例尺读数也自相矛盾。这里保证返回值一定落在
/// 子图区间内（ADR-0009 §2⑤）。
pub fn fly_in_scale(map_size: Vec2, window_size: Vec2, from_scale: f32) -> f32 {
    let zoom = LevelZoom::for_map(map_size, window_size);
    // `from_scale.min(fit)`：整幅子图可见；但用户已经比这更近时不为换级拉远视野。
    zoom.clamp(from_scale.min(zoom.fit))
}

impl LevelZoom {
    /// 按图尺寸与窗口尺寸算出一级的区间。
    ///
    /// 尺寸退化（0 或负数、NaN）时退回 [`FALLBACK_MAP_SIZE`]：宁可给一个保守
    /// 区间，也不要产出 `NaN` 把相机写坏。
    pub fn for_map(map_size: Vec2, window_size: Vec2) -> Self {
        let fit = fit_scale(map_size, window_size);
        Self {
            fit,
            min: fit / LEVEL_ZOOM_SPAN,
            max: fit * LEVEL_OVERVIEW,
        }
    }

    /// 把缩放夹进本级区间。
    pub fn clamp(&self, scale: f32) -> f32 {
        scale.clamp(self.min, self.max)
    }

    /// 是否已顶到内界（可以换到更深的级）。
    pub fn at_inner_limit(&self, scale: f32) -> bool {
        scale <= self.min * (1.0 + LIMIT_EPSILON)
    }

    /// 是否已顶到外界（可以换到更浅的级）。
    pub fn at_outer_limit(&self, scale: f32) -> bool {
        scale >= self.max * (1.0 - LIMIT_EPSILON)
    }
}

/// 换级请求：撞到当前级**外界**后继续往外滚动所表达的意图（ADR-0038）。
///
/// 往里方向已删除——撞内界继续往里滚只会被 [`LevelZoom::clamp`] 夹住停住，
/// 进入子图的唯一手势是双击钻入图标（见 [`crate::drill`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomCross {
    /// 继续往外：换到更浅的一级（父图）。
    Out,
}

/// 换级请求的载体（`camera_zoom` 写，`drill::apply_zoom_cross` 消费）。
///
/// 做成资源而不是直接在 `camera_zoom` 里改 `DrillState`：地图路径的写入口只有
/// 「双击图标 / 面包屑跳转 / 本请求」三处，而「写入后还要决定本帧是否播过渡动画」
/// 属于钻取模块的职责（见 `drill::apply_zoom_cross` 的文档）。
#[derive(Resource, Default)]
pub struct ZoomCrossRequest {
    pub pending: Option<ZoomCross>,
}

/// 全图适配缩放：取两轴中**较紧**的一轴，保证整幅图都在窗口里。
///
/// 这是「世界单位 ↔ 屏幕像素」换算里的 `fit` 一项，也是基线 §2.15「一个实现点」
/// 约束下的唯一实现（`ui.rs` 的适应按钮与相机滚轮都用它）。
pub fn fit_scale(map_size: Vec2, window_size: Vec2) -> f32 {
    let map = sanitize(map_size);
    let window = sanitize(window_size);
    (map.x / window.x).max(map.y / window.y)
}

/// 取当前图（`drill` 所在的那张）的尺寸，供分级用。
///
/// **不是**「当前显示的栅格属于哪张图」——那是 [`crate::terrain_view`] 的
/// `active_terrain`（底图可以向上继承），而缩放要的是作者此刻**在哪一级**。
/// 图没声明 `extent` 时退回 `fallback`（调用方通常给「当前显示的栅格 × 格子尺寸」，
/// 那至少与实际画面同尺，比一个常数更准）。
pub fn map_size_of(world: &World, current: Option<&wv_core::id::Id>, fallback: Vec2) -> Vec2 {
    let fallback = sanitize(fallback);
    let Some(id) = current else {
        return fallback;
    };
    let Some(node) = world.maps.iter().find(|m| &m.id == id) else {
        return fallback;
    };
    match node.extent.as_ref() {
        Some(e)
            if e.width.is_finite() && e.height.is_finite() && e.width > 0.0 && e.height > 0.0 =>
        {
            Vec2::new(e.width, e.height)
        }
        _ => fallback,
    }
}

/// 当前级的区间（`map_size_of` + [`LevelZoom::for_map`]）。
pub fn level_zoom(
    world: &World,
    current: Option<&wv_core::id::Id>,
    window_size: Vec2,
    fallback: Vec2,
) -> LevelZoom {
    LevelZoom::for_map(map_size_of(world, current, fallback), window_size)
}

/// 把非有限/非正的尺寸换成兜底值，避免 `fit` 产出 `NaN`。
fn sanitize(size: Vec2) -> Vec2 {
    let ok = |v: f32| v.is_finite() && v > 0.0;
    Vec2::new(
        if ok(size.x) {
            size.x
        } else {
            FALLBACK_MAP_SIZE
        },
        if ok(size.y) {
            size.y
        } else {
            FALLBACK_MAP_SIZE
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::id::Id;
    use wv_core::map::{Extent, MapLevel, MapNode};

    fn map(id: &str, level: MapLevel, extent: Option<Extent>) -> MapNode {
        MapNode {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            level,
            parent: None,
            children: Vec::new(),
            icons: Vec::new(),
            layers: Vec::new(),
            extent,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        }
    }

    fn world_with(maps: Vec<MapNode>) -> World {
        World {
            maps,
            ..Default::default()
        }
    }

    /// 世界图 4096 在 1280×800 窗口里：fit = 5.12（取较紧的高轴），
    /// 外界 = 5.12 × 1.5625 = **8.0**，正好是历史 `SCALE_MAX`；
    /// 内界 = 5.12 / 32 = 0.16（ADR-0038 收浅后的上限）。
    #[test]
    fn world_level_reproduces_the_historical_scale_max() {
        let lz = LevelZoom::for_map(Vec2::splat(4096.0), Vec2::new(1280.0, 800.0));
        assert!((lz.fit - 5.12).abs() < 1e-4, "got {}", lz.fit);
        assert!((lz.max - 8.0).abs() < 1e-4, "got {}", lz.max);
        assert!((lz.min - 0.16).abs() < 1e-6, "got {}", lz.min);
    }

    /// 城市图 1024 的区间整体落在世界图区间**内侧**：内界更往里，
    /// 于是「滚到世界的内界」那一点已经是合法城市缩放，换级无需改 scale。
    #[test]
    fn a_smaller_map_extends_the_inner_limit_and_overlaps_the_parent() {
        let window = Vec2::new(1280.0, 800.0);
        let world = LevelZoom::for_map(Vec2::splat(4096.0), window);
        let city = LevelZoom::for_map(Vec2::splat(1024.0), window);
        assert!((city.fit - 1.28).abs() < 1e-4, "got {}", city.fit);
        assert!(city.min < world.min, "{} !< {}", city.min, world.min);
        assert!(
            city.min < world.min && world.min < city.max,
            "世界内界 {} 必须落在城市区间 [{}, {}] 内",
            world.min,
            city.min,
            city.max
        );
    }

    /// 逐级内界一路往里推，总跨度 ≈ 4×10³（世界外界 → 街道内界），且
    /// **跨度来自各级图的尺寸比**，不是「每级常数 32×」的幂——
    /// 「级间相乘凑 10⁴–10⁵」是旧滚轮进城时代的论证（ADR-0009 §1 第 4 条），
    /// ADR-0038 后进城靠双击，深缩放只服务本级编辑。
    #[test]
    fn the_ladder_span_comes_from_map_size_ratios_not_from_level_span() {
        let window = Vec2::new(1280.0, 800.0);
        let world = LevelZoom::for_map(Vec2::splat(4096.0), window);
        let city = LevelZoom::for_map(Vec2::splat(1024.0), window);
        let district = LevelZoom::for_map(Vec2::splat(256.0), window);
        let street = LevelZoom::for_map(Vec2::splat(64.0), window);
        // 每一级的内界都应比上一级更深（往外换级时 scale 才不必改）。
        assert!(city.min < world.min, "{} !< {}", city.min, world.min);
        assert!(district.min < city.min, "{} !< {}", district.min, city.min);
        assert!(
            street.min < district.min,
            "{} !< {}",
            street.min,
            district.min
        );
        // 世界外界 → 街道内界 = 8.0 / (64/32) = 4000。
        let span = world.max / street.min;
        assert!(
            (3000.0..=6000.0).contains(&span),
            "总跨度应≈4×10³，实际 {span}"
        );
    }

    /// 换级之所以能「不改缩放」，靠的是这条区间包含关系：
    /// 往外换级的触发点（子级外界）必须落在父级区间内。
    /// 第二条断言（父级内界 ∈ 子级区间）在 ADR-0038 后不再服务「往里换级」
    /// ——那个入口已废——但区间重叠本身仍是双击飞入落点合法性的前提
    /// （`fly_in_scale` 的终点必须落在新级区间内），性质继续钉住。
    #[test]
    fn adjacent_levels_each_contain_the_other_ones_crossing_point() {
        let window = Vec2::new(1280.0, 800.0);
        let parent = LevelZoom::for_map(Vec2::splat(4096.0), window);
        let child = LevelZoom::for_map(Vec2::splat(1024.0), window);
        assert!(
            parent.min <= child.max && child.max <= parent.max,
            "父级 [{}, {}] 必须容纳子级外界 {}",
            parent.min,
            parent.max,
            child.max
        );
        assert!(
            child.min <= parent.min && parent.min <= child.max,
            "子级 [{}, {}] 必须容纳父级内界 {}",
            child.min,
            child.max,
            parent.min
        );
    }

    #[test]
    fn clamp_holds_scale_inside_the_level_band() {
        let lz = LevelZoom::for_map(Vec2::splat(1024.0), Vec2::new(1280.0, 800.0));
        assert!((lz.clamp(100.0) - lz.max).abs() < 1e-6, "上越界应夹到 max");
        assert!((lz.clamp(0.0001) - lz.min).abs() < 1e-9, "下越界应夹到 min");
        let mid = (lz.min + lz.max) * 0.5;
        assert!((lz.clamp(mid) - mid).abs() < 1e-9, "区间内不应改动");
    }

    #[test]
    fn limits_are_detected_at_the_clamped_ends() {
        let lz = LevelZoom::for_map(Vec2::splat(4096.0), Vec2::new(1280.0, 800.0));
        assert!(lz.at_inner_limit(lz.min));
        assert!(lz.at_outer_limit(lz.max));
        assert!(!lz.at_inner_limit(lz.min * 1.5));
        assert!(!lz.at_outer_limit(lz.max * 0.5));
    }

    #[test]
    fn fit_scale_takes_the_tighter_axis() {
        let s = fit_scale(Vec2::new(2048.0, 1024.0), Vec2::new(1280.0, 800.0));
        assert!((s - 1.6).abs() < 1e-4, "got {s}, expected 1.6");
    }

    #[test]
    fn degenerate_sizes_fall_back_instead_of_producing_nan() {
        let lz = LevelZoom::for_map(Vec2::new(0.0, f32::NAN), Vec2::ZERO);
        assert!(lz.fit.is_finite() && lz.fit > 0.0, "got {}", lz.fit);
        assert!(lz.min.is_finite() && lz.max.is_finite());
    }

    #[test]
    fn map_size_of_reads_the_current_map_extent() {
        let w = world_with(vec![
            map(
                "map_world",
                MapLevel::World,
                Some(Extent::sized(4096.0, 4096.0)),
            ),
            map(
                "map_city",
                MapLevel::City,
                Some(Extent::sized(1024.0, 512.0)),
            ),
        ]);
        let city = Id::new_unchecked("map_city");
        assert_eq!(
            map_size_of(&w, Some(&city), Vec2::splat(FALLBACK_MAP_SIZE)),
            Vec2::new(1024.0, 512.0)
        );
        let world_id = Id::new_unchecked("map_world");
        assert_eq!(
            map_size_of(&w, Some(&world_id), Vec2::splat(FALLBACK_MAP_SIZE)),
            Vec2::splat(4096.0)
        );
    }

    #[test]
    fn a_map_without_extent_falls_back_to_the_given_size() {
        let w = world_with(vec![map("map_city", MapLevel::City, None)]);
        let city = Id::new_unchecked("map_city");
        assert_eq!(
            map_size_of(&w, Some(&city), Vec2::splat(1234.0)),
            Vec2::splat(1234.0)
        );
    }

    /// 未知/缺失的当前图（启动早期、或工程刚加载）不得产出 `NaN` 区间，
    /// 否则相机会被写坏且没有任何提示。
    #[test]
    fn unknown_current_map_uses_the_fallback_without_nan() {
        let w = world_with(vec![]);
        let lz = level_zoom(&w, None, Vec2::new(1280.0, 800.0), Vec2::splat(4096.0));
        assert!(lz.min.is_finite() && lz.max.is_finite() && lz.min > 0.0);
        assert!((lz.fit - 5.12).abs() < 1e-4, "got {}", lz.fit);
    }

    /// 从很远（世界级 fit 5.12）双击进一座 1024 的城市：目标应把整座城放进窗口
    /// （fit = 1.28），而不是历史硬编码的 4.0——4.0 在城市的区间 `[0.016, 2.0]`
    /// 之外，进城后第一次滚轮会被 clamp 硬拽一下。
    #[test]
    fn fly_in_scale_shows_the_whole_child_map_instead_of_a_hardcoded_constant() {
        let window = Vec2::new(1280.0, 800.0);
        let got = fly_in_scale(Vec2::splat(1024.0), window, 5.12);
        assert!((got - 1.28).abs() < 1e-4, "got {got}");
        let child = LevelZoom::for_map(Vec2::splat(1024.0), window);
        assert!(child.min <= got && got <= child.max, "必须在子级区间内");
        assert!(4.0 > child.max, "旧硬编码常量确实越界（回归前提）");
    }

    /// 反过来：作者已经在比 `fit` 更近处看子图时，换级**不得**把视野拉远。
    #[test]
    fn fly_in_scale_never_pulls_a_closer_view_back_out() {
        let window = Vec2::new(1280.0, 800.0);
        let got = fly_in_scale(Vec2::splat(1024.0), window, 0.5);
        assert!((got - 0.5).abs() < 1e-6, "got {got}");
    }

    /// 极端近处（比子级内界还近）也必须被夹进区间，过渡终点才是合法的。
    #[test]
    fn fly_in_scale_clamps_extreme_input_into_the_child_band() {
        let window = Vec2::new(1280.0, 800.0);
        let child = LevelZoom::for_map(Vec2::splat(1024.0), window);
        let got = fly_in_scale(Vec2::splat(1024.0), window, 1e-6);
        assert!((got - child.min).abs() < 1e-9, "got {got}");
    }
}
