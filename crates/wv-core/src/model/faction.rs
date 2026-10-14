//! 势力维度。
//!
//! ## 与 `Unit.owner` 的边界（ADR-0004 派生物）
//!
//! `Unit.owner: Option<Id>` 允许「单位归某势力所有」（"faction_red"），
//! 那是**单位-势力**的归属关系——本模块的 `Faction` 则是**势力本体**：
//! 有名字、有颜色、占据地图上的多边形几何。
//!
//! 两者通过 `Faction.id` 互相引用（`Unit.owner == Some(Faction.id)`）。
//! 关系是 `Option`——单位可以无主（`None`），势力可以无单位（不创建空约束）。
//!
//! ## 时间语义（P6/P7）
//!
//! **Faction 本体无时间区间**——势力是「长期存在的主体」（如「清政府」从
//! 1644 到 1912 一直存在），它的存废不在本模型里表达。
//!
//! 势力**何时占据何处**由 [`GeometrySpan`] 表达：
//! `GeometrySpan.owner = Faction.id`，`span` 限定有效时间。
//! 想要「某时刻某地图上的所有势力及其多边形」，走 `World::factions_at(map, day)`。
//!
//! ## 待决 §12-8：带洞多边形
//!
//! **v0.1 仅支持无洞单环多边形**（`GeometryData::Polygon(Vec<f32>)`），
//! 扁平坐标、首尾不重复。如要表达「环形的飞地」请拆成多个 polygon。
//! 这是 §12-8 待裁问题的第一版落地——文档已记录，等真用上再升级为
//! `PolygonWithHoles` 变体。
//!
//! [`GeometrySpan`]: crate::map::GeometrySpan

use serde::{Deserialize, Serialize};

use crate::base::color::Color;
use crate::base::id::Id;
use crate::time::{TimePoint, TimeSpan};

/// 势力本体。对应工程目录的 `factions/<id>.yaml`。
///
/// §4.5 原则「每实体一文件」：Faction 与 Place、Character 同级，
/// 几何、关系、轨迹各自另立文件，不在 Faction 内联大段数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Faction {
    pub id: Id,
    /// 显示名（中文 / 英文自由）。
    pub name: String,
    /// 势力色。YAML 里写 `"#RRGGBB"` 字符串。解析失败由渲染层回退到主题色，
    /// **不会**让工程打不开（`AGENTS.md` §7.5V1）。
    pub color: Color,
}

impl Faction {
    /// 构造一个势力。
    pub fn new(id: Id, name: impl Into<String>, color: Color) -> Self {
        Self {
            id,
            name: name.into(),
            color,
        }
    }
}

/// 给定 (地图, 天)，列出该地图该时刻所有生效的势力及其多边形顶点。
///
/// ## 行为
///
/// - 遍历 `world.faction_geometry`，过滤 `map == target_map` 且 `span.contains(day)`；
/// - 通过 `owner` 在 `world.factions` 中查 `Faction`（**找不到**→ 跳过，V1 守约）；
/// - 只取 `GeometryData::Polygon` 变体（其他变体 v0.1 不画，跳过）；
/// - 一次返回 (Faction 引用, 顶点切片) 对，**不**排序。
///
/// ## 为什么不在 `wv-app` 而在 `wv-core`
///
/// 这是**纯数据查询**——没有任何渲染/Bevy 依赖。`wv-app` 只读不写。
/// `wv-io` 的 `diagnostics` 也可消费（虽然目前用不到，但为未来势力一致性
/// 校验留口）。
pub fn factions_at<'a>(
    world: &'a crate::World,
    target_map: &Id,
    day: i64,
) -> Vec<(&'a Faction, &'a [f32])> {
    let t = TimePoint::from_day(day);
    let mut out = Vec::new();
    for g in &world.faction_geometry {
        if &g.map != target_map {
            continue;
        }
        if !g.span.contains(&t) {
            continue;
        }
        let Some(faction) = world.factions.iter().find(|f| f.id == g.owner) else {
            // 几何引用的 owner 不在 factions 列表里——按 V1 静默跳过。
            continue;
        };
        let GeometryDataView::Polygon(coords) = GeometryDataView::from(&g.geometry) else {
            continue;
        };
        out.push((faction, coords));
    }
    out
}

/// 内部 enum view：避免在 `wv-core` 公共 API 暴露 `GeometryData` 的所有权语义。
/// 等价于 `GeometryData::Polygon` 的「看一眼」版本——只读切片。
enum GeometryDataView<'a> {
    Polygon(&'a [f32]),
    Other,
}

impl<'a> From<&'a crate::map::GeometryData> for GeometryDataView<'a> {
    fn from(g: &'a crate::map::GeometryData) -> Self {
        use crate::map::GeometryData;
        match g {
            GeometryData::Polygon(v) => GeometryDataView::Polygon(v.as_slice()),
            GeometryData::Points(_) | GeometryData::Ref(_) => GeometryDataView::Other,
        }
    }
}

/// 校验同一 (map, owner) 在时间上是否有重叠的多边形——v0.1 决策：
///
/// **允许重叠**。理由：作者可能想表达「同一势力在不同时间扩张疆界」，
/// 把新边界作为新 `GeometrySpan` 追加，旧的不删（满足 P6/P7）。重叠
/// 区间在渲染层按"后画覆盖前画"处理即可——这是时间区间的自然推论。
///
/// 本函数留作显式表达"我们不检查重叠"的契约位，以便未来要做
/// 重叠校验时挂载在此。
pub fn allow_overlapping_spans(_a: &TimeSpan, _b: &TimeSpan) -> bool {
    true
}

#[cfg(test)]
mod tests {
    //! Faction 模型 + factions_at 单元测试。
    use super::*;
    use crate::map::{GeometryData, GeometrySpan};
    use crate::World;

    fn world_with_faction(faction: Faction, geos: Vec<GeometrySpan>) -> World {
        let mut w = World::default();
        w.factions.push(faction);
        w.faction_geometry = geos;
        w
    }

    fn span(from: i64, to: i64) -> TimeSpan {
        TimeSpan { from, to: Some(to) }
    }

    fn square_polygon(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<f32> {
        vec![x0, y0, x1, y0, x1, y1, x0, y1]
    }

    /// Faction 序列化由 `Color` 同源（`#[serde(transparent)] String`）——
    /// round-trip 行为已在 `color.rs` 测试覆盖；本模块测的是**领域语义**。
    ///
    /// `factions_at`：地图不匹配 → 不返回。
    #[test]
    fn factions_at_filters_by_map() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#000000".into()));
        let geos = vec![GeometrySpan {
            map: Id::new_unchecked("map_world"),
            owner: f.id.clone(),
            span: span(1, 100),
            geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 10.0, 10.0)),
        }];
        let w = world_with_faction(f, geos);
        let other_map = Id::new_unchecked("map_other");
        let result = factions_at(&w, &other_map, 50);
        assert!(result.is_empty(), "其他地图不应被返回");
    }

    /// `factions_at`：时间在 span 外 → 不返回。
    #[test]
    fn factions_at_filters_by_time() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#000000".into()));
        let map = Id::new_unchecked("map_w");
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: f.id.clone(),
            span: span(10, 20),
            geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
        }];
        let w = world_with_faction(f, geos);
        // 区间 [10,20) 半开。
        assert_eq!(factions_at(&w, &map, 1).len(), 0, "区间之前");
        assert_eq!(factions_at(&w, &map, 10).len(), 1, "区间首含");
        assert_eq!(factions_at(&w, &map, 19).len(), 1, "区间内");
        assert_eq!(factions_at(&w, &map, 20).len(), 0, "区间终不含");
    }

    /// `factions_at`：开放区间 `to == None` 视为「至今有效」。
    #[test]
    fn factions_at_open_end_means_until_now() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#000000".into()));
        let map = Id::new_unchecked("map_w");
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: f.id.clone(),
            span: TimeSpan { from: 1, to: None },
            geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
        }];
        let w = world_with_faction(f, geos);
        assert_eq!(factions_at(&w, &map, 1).len(), 1);
        assert_eq!(factions_at(&w, &map, 100000).len(), 1, "至今有效");
    }

    /// `factions_at`：owner 找不到对应 Faction → 跳过（V1）。
    #[test]
    fn factions_at_skips_orphan_geometry() {
        let map = Id::new_unchecked("map_w");
        let orphan_owner = Id::new_unchecked("f_ghost");
        // factions 列表**不**含 f_ghost。
        let w = world_with_faction(
            Faction::new(Id::new_unchecked("f_real"), "Real", Color("#000000".into())),
            vec![GeometrySpan {
                map: map.clone(),
                owner: orphan_owner,
                span: span(1, 100),
                geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
            }],
        );
        assert!(factions_at(&w, &map, 50).is_empty());
    }

    /// `factions_at`：非 Polygon 变体（Points/Ref）→ 跳过。
    #[test]
    fn factions_at_skips_non_polygon_geometry() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#000000".into()));
        let map = Id::new_unchecked("map_w");
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: f.id.clone(),
            span: span(1, 100),
            geometry: GeometryData::Points(vec![0.0, 0.0]),
        }];
        let w = world_with_faction(f, geos);
        assert!(factions_at(&w, &map, 50).is_empty());
    }

    /// `factions_at`：返回的多边形坐标是**原始切片**——非拷贝。
    #[test]
    fn factions_at_returns_slice_ref_into_world() {
        let f = Faction::new(Id::new_unchecked("f1"), "F1", Color("#000000".into()));
        let map = Id::new_unchecked("map_w");
        let coords = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let len = coords.len();
        let geos = vec![GeometrySpan {
            map: map.clone(),
            owner: f.id.clone(),
            span: span(1, 100),
            geometry: GeometryData::Polygon(coords.clone()),
        }];
        let w = world_with_faction(f, geos);
        let result = factions_at(&w, &map, 50);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.id.as_str(), "f1");
        assert_eq!(result[0].1.len(), len);
        assert_eq!(result[0].1, &coords[..]);
    }

    /// `factions_at`：两个势力在同一 (map, day) 生效 → 都返回。
    #[test]
    fn factions_at_returns_all_active() {
        let f1 = Faction::new(Id::new_unchecked("f1"), "F1", Color("#FF0000".into()));
        let f2 = Faction::new(Id::new_unchecked("f2"), "F2", Color("#00FF00".into()));
        let map = Id::new_unchecked("map_w");
        let geos = vec![
            GeometrySpan {
                map: map.clone(),
                owner: f1.id.clone(),
                span: span(1, 100),
                geometry: GeometryData::Polygon(square_polygon(0.0, 0.0, 1.0, 1.0)),
            },
            GeometrySpan {
                map: map.clone(),
                owner: f2.id.clone(),
                span: span(50, 200),
                geometry: GeometryData::Polygon(square_polygon(2.0, 2.0, 3.0, 3.0)),
            },
        ];
        let mut w = World::default();
        w.factions.push(f1);
        w.factions.push(f2);
        w.faction_geometry = geos;
        // day=10：f1 生效，f2 未开始。
        assert_eq!(factions_at(&w, &map, 10).len(), 1);
        // day=99：f1 [1,100) 半开仍含 99，f2 [50,200) 含 99 → 双方都生效。
        let r = factions_at(&w, &map, 99);
        assert_eq!(r.len(), 2);
        // day=100：f1 半开不含 100（[1,100)），f2 含 → 仅 f2。
        assert_eq!(factions_at(&w, &map, 100).len(), 1);
        // day=199：f1 早已结束（[1,100)），f2 [50,200) 含 199 → 仅 f2。
        assert_eq!(factions_at(&w, &map, 199).len(), 1);
        // day=200：双方都结束。
        assert_eq!(factions_at(&w, &map, 200).len(), 0);
    }
}
