//! Place 地界（territory）的求值。
//!
//! ## 与 `faction_geometry` 的关系
//!
//! 地界复用 [`GeometrySpan`] 这个「带时间区间的几何片段」底座：`owner` 是
//! `Place.id`，几何是多边形环，集合落在 [`crate::World::place_geometry`]，
//! 与 `World.faction_geometry` **同构并列**——不泛化为共用集合（统一 territory
//! 底座留待将来 ADR，见主题覆盖物设计稿 §6.1 裁定）。
//!
//! ## 时间语义（P6/P7）
//!
//! [`Place`] 本体无时间区间——地界**何时覆盖何处**由 `GeometrySpan.span` 表达。
//! 同一 Place 的边界演化 = 追加新 span、不覆盖旧 span（P6/P7）。
//! 想要「某时刻某地图上的所有地界」，走 [`place_territory_at`]。
//!
//! ## 为什么求值在 `wv-core`
//!
//! 这是**纯数据查询**，没有任何渲染/Bevy 依赖。反过来，AGENTS §3.2 禁止
//! 渲染层自己比较时间——「本帧哪些地界生效」只在本模块求值，`wv-render`
//! 只拿结果画。
//!
//! [`GeometrySpan`]: crate::map::GeometrySpan
//! [`Place`]: crate::map::distance::Place

use crate::base::id::Id;
use crate::map::distance::Place;
use crate::map::GeometryData;
use crate::time::TimePoint;

/// 给定 (地图, 天)，列出该地图该时刻所有生效的 Place 地界及其多边形顶点。
///
/// ## 行为
///
/// - 遍历 [`crate::World::place_geometry`]，过滤 `map == target_map` 且
///   `span.contains(day)`（半开区间 `[from, to)`；`to == None` = 至今有效）；
/// - 通过 `owner` 在 `world.distances.places` 中查 `Place`（**找不到** → 跳过，
///   V1 守约：悬空引用由校验面板列出，不阻止求值）；
/// - 只取 [`GeometryData::Polygon`] 变体（`Points` / `Ref` v0.1 不画，跳过）；
/// - 一次返回 `(Place 引用, 顶点切片)` 对，**不**排序（返回顺序即
///   `World.place_geometry` 的声明顺序，渲染层按「后画覆盖前画」处理重叠）。
///
/// ## 为什么是渲染层的唯一入口
///
/// AGENTS §3.2 禁止渲染层自己比较时间：某时刻哪些地界生效只在本函数求值，
/// `wv-render` 拿到的永远是「本帧该画的东西」。这与 [`crate::factions_at`]
/// 的契约逐条同构——两条查询各自独立、互不调用，避免为了复用把 `Place` /
/// `Faction` 强行泛化（统一 territory 底座留待将来 ADR）。
///
/// 返回切片指向 `World` 内部，**零拷贝**——渲染层每帧调用也不产生分配
/// 以外的开销；所有权始终留在 `wv-app`（AGENTS §2.1）。
pub fn place_territory_at<'a>(
    world: &'a crate::World,
    target_map: &Id,
    day: i64,
) -> Vec<(&'a Place, &'a [f32])> {
    let t = TimePoint::from_day(day);
    let mut out = Vec::new();
    for g in &world.place_geometry {
        if &g.map != target_map {
            continue;
        }
        if !g.span.contains(&t) {
            continue;
        }
        let Some(place) = world.distances.place(&g.owner) else {
            // 几何引用的 owner 不在 distances.places 里——按 V1 静默跳过。
            continue;
        };
        let GeometryData::Polygon(coords) = &g.geometry else {
            continue;
        };
        out.push((place, coords.as_slice()));
    }
    out
}

#[cfg(test)]
mod tests {
    //! `place_territory_at` 单元测试——逐条镜像 `model/faction.rs` 的
    //! `factions_at` 用例，保证两个同构查询的语义不漂移。
    use super::*;
    use crate::map::distance::Place;
    use crate::map::{GeometryData, GeometrySpan};
    use crate::time::TimeSpan;
    use crate::World;

    fn vid(s: &str) -> crate::Id {
        crate::Id::new_unchecked(s)
    }

    fn place(id: &str) -> Place {
        Place {
            id: vid(id),
            name: id.to_string(),
            feature: None,
            biome: None,
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
        }
    }

    fn world_with_place(place: Place, geos: Vec<GeometrySpan>) -> World {
        let mut w = World::default();
        w.distances.places.push(place);
        w.place_geometry = geos;
        w
    }

    fn span(from: i64, to: i64) -> TimeSpan {
        TimeSpan { from, to: Some(to) }
    }

    fn square_polygon(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<f32> {
        vec![x0, y0, x1, y0, x1, y1, x0, y1]
    }

    /// 地图不匹配 → 不返回。
    #[test]
    fn place_territory_at_filters_by_map() {
        let p = place("place_forest");
        let geos = vec![GeometrySpan {
            map: vid("map_world"),
            owner: p.id.clone(),
            span: span(1, 100),
            geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 10.0, 10.0)),
            relation: None,
        }];
        let w = world_with_place(p, geos);
        let other_map = vid("map_other");
        let result = place_territory_at(&w, &other_map, 50);
        assert!(result.is_empty(), "其他地图的地界不应被返回");
    }

    /// 时间区间 `[10, 20)` 半开：首含、终不含。
    #[test]
    fn place_territory_at_filters_by_time_half_open() {
        let p = place("place_forest");
        let map = vid("map_w");
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: p.id.clone(),
            span: span(10, 20),
            geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
            relation: None,
        }];
        let w = world_with_place(p, geos);
        assert_eq!(place_territory_at(&w, &map, 9).len(), 0, "区间之前");
        assert_eq!(place_territory_at(&w, &map, 10).len(), 1, "区间首含");
        assert_eq!(place_territory_at(&w, &map, 19).len(), 1, "区间内");
        assert_eq!(place_territory_at(&w, &map, 20).len(), 0, "区间终不含");
    }

    /// 开放区间 `to == None` 视为「至今有效」。
    #[test]
    fn place_territory_at_open_end_means_until_now() {
        let p = place("place_forest");
        let map = vid("map_w");
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: p.id.clone(),
            span: TimeSpan { from: 1, to: None },
            geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
            relation: None,
        }];
        let w = world_with_place(p, geos);
        assert_eq!(place_territory_at(&w, &map, 1).len(), 1);
        assert_eq!(place_territory_at(&w, &map, 100000).len(), 1, "至今有效");
    }

    /// owner 找不到对应 Place → 跳过（V1：悬空引用不阻止求值）。
    #[test]
    fn place_territory_at_skips_orphan_geometry() {
        let map = vid("map_w");
        // distances.places **不含** place_ghost。
        let w = world_with_place(
            place("place_real"),
            vec![GeometrySpan {
                map: map.clone(),
                owner: vid("place_ghost"),
                span: span(1, 100),
                geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
                relation: None,
            }],
        );
        assert!(
            place_territory_at(&w, &map, 50).is_empty(),
            "孤儿地界（owner 不在 distances.places）应被跳过"
        );
    }

    /// 非 Polygon 变体（Points / Ref）→ 跳过。
    #[test]
    fn place_territory_at_skips_non_polygon_geometry() {
        let p = place("place_forest");
        let map = vid("map_w");
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: p.id.clone(),
            span: span(1, 100),
            geometry: GeometryData::Points(vec![0.0, 0.0]),
            relation: None,
        }];
        let w = world_with_place(p, geos);
        assert!(
            place_territory_at(&w, &map, 50).is_empty(),
            "Points 变体不构成地界面，应被跳过"
        );
    }

    /// 返回的多边形坐标是 **World 内部切片的引用**——非拷贝。
    #[test]
    fn place_territory_at_returns_slice_ref_into_world() {
        let p = place("place_forest");
        let map = vid("map_w");
        let coords = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: p.id.clone(),
            span: span(1, 100),
            geometry: GeometryData::Polygon(coords.clone()),
            relation: None,
        }];
        let w = world_with_place(p, geos);
        let result = place_territory_at(&w, &map, 50);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.id.as_str(), "place_forest");
        assert_eq!(result[0].1.len(), coords.len(), "顶点数量应原样返回");
        assert_eq!(result[0].1, &coords[..], "坐标值应与写入时一致");
        let GeometryData::Polygon(stored) = &w.place_geometry[0].geometry else {
            panic!("测试前提：写入的是 Polygon");
        };
        assert!(
            std::ptr::eq(result[0].1.as_ptr(), stored.as_ptr()),
            "应返回 World 内部切片的引用，而不是拷贝"
        );
    }

    /// 同一 (map, day) 两个 Place 的地界都生效 → 都返回。
    #[test]
    fn place_territory_at_returns_all_active() {
        let p1 = place("place_forest");
        let p2 = place("place_swamp");
        let map = vid("map_w");
        let mut w = World::default();
        w.distances.places.push(p1.clone());
        w.distances.places.push(p2.clone());
        w.place_geometry = vec![
            GeometrySpan {
                map: map.clone(),
                owner: p1.id,
                span: span(1, 100),
                geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
                relation: None,
            },
            GeometrySpan {
                map: map.clone(),
                owner: p2.id,
                span: span(50, 200),
                geometry: GeometryData::Polygon(square_polygon(2.0, 2.0, 3.0, 3.0)),
                relation: None,
            },
        ];
        // day=10：p1 生效，p2 未开始。
        assert_eq!(place_territory_at(&w, &map, 10).len(), 1);
        // day=99：p1 [1,100) 含 99，p2 [50,200) 含 99 → 双方都生效。
        let r = place_territory_at(&w, &map, 99);
        assert_eq!(r.len(), 2, "两个地界同时生效时应都返回");
        assert_eq!(r[0].0.id.as_str(), "place_forest", "返回顺序即声明顺序");
        assert_eq!(r[1].0.id.as_str(), "place_swamp", "不排序，按集合顺序");
        // day=100：p1 半开不含 100（[1,100)），p2 含 → 仅 p2。
        assert_eq!(place_territory_at(&w, &map, 100).len(), 1);
        // day=199：p1 早已结束，p2 [50,200) 含 199 → 仅 p2。
        assert_eq!(place_territory_at(&w, &map, 199).len(), 1);
        // day=200：双方都结束。
        assert_eq!(place_territory_at(&w, &map, 200).len(), 0);
    }
}
