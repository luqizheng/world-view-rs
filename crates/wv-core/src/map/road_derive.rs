//! 线即路网：从道路线段 POI 推导距离图的边（ADR-0070 D5）。
//!
//! ## 为什么推导而不是让作者手填
//!
//! 作者画一条路，关心的是「从哪到哪、多宽」；「这条路 12.5 km」是**几何 ×
//! 比例尺**的计算结论（AGENTS §4.2：距离只存参数，不存结论）。本模块把
//! 「道路线两端吸附到地点」翻译成运行时的投影边（[`PlaceEdge`] 且
//! `derived = true`），`edges.yaml` 里**永不落盘**——作者手写的边才是真值。
//!
//! ## `km_per_wu` 的消费口径
//!
//! [`crate::map::MapNode::km_per_wu`] 的文档自称「纯展示用」；ADR-0070 D5
//! 把它的消费面**收窄为「展示 + 本模块的推导折算」**——推导边是运行时
//! 投影（`derived` 永不落盘），比例尺一改重算即可，不违反「距离只存参数」。

use crate::base::id::Id;
use crate::map::distance::{LineKind, Place, PlaceEdge};
use crate::map::MapNode;
use crate::World;

/// 道路线端点吸附地点的默认容差（wu，ADR-0070 D5）。
///
/// 路端不必精确钉在 POI 锚点上——作者画线时肉眼对齐即可；32 wu 约为一屏
/// 缩放下肉眼对齐的合理误差。改动本值 = 改「哪些路算连上了」的判定，须走 ADR。
pub const ROAD_SNAP_TOLERANCE_WU: f32 = 32.0;

/// 线段 POI 的**判别唯一入口**（ADR-0070 D2 判别口径的收口）。
///
/// 判别口径：`place.line` 缺席 → `None`（点状地点，`line_kind` 无意义）；
/// 有 `line` 时，`line_kind` 缺省（`None`）或显式 `Some(River)` →
/// `Some(River)`（ADR-0046 存量手绘折线全是小溪小河，零迁移）；
/// 显式 `Some(Road)` → `Some(Road)`。
///
/// **为什么收口成函数**：渲染收集、迁移判据、推导边、面板回填都要问
/// 「这是不是线段 POI、是河还是路」——口径散落各处，一处改成
/// 「`line_kind` 直接读」就会把 ADR-0046 存量河误判成路。后续所有任务
/// 一律走本函数，**禁止**各自重写判别条件。
pub fn effective_line_kind(place: &Place) -> Option<LineKind> {
    place.line.as_ref()?;
    Some(match place.line_kind {
        Some(LineKind::Road) => LineKind::Road,
        // 缺省（None）与显式 River 同归河：ADR-0070 D2「缺省 = River」。
        _ => LineKind::River,
    })
}

/// 推导报告：产出的边 + 两类跳过计数（ADR-0070 D5）。
///
/// 跳过**不报错**（AGENTS §7.5 V1：只提示不阻止）——无比例尺就只显示 wu，
/// 端点没吸附上就当这条路还没修到镇口，作者回过头补即可。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RoadDeriveReport {
    /// 本轮成功推导出的边（`derived = true`，`rank = None`）。
    pub edges: Vec<PlaceEdge>,
    /// **产边溯源**（ADR-0070 D5 评审裁定）：与 [`Self::edges`] 逐位平行，
    /// 记录每条推导边由哪条道路线 POI 推出：`(边 a, 边 b, 路 Place ID)`。
    ///
    /// **为什么放 Report 而不是 PlaceEdge 字段**：产边之路是推导过程的
    /// 中间事实，落进 PlaceEdge 就会顺着 clone / 序列化路径到处流（污染
    /// 数据模型）；Report 是一次性诊断产物，用完即弃。消费方：同端点对
    /// 去重的并列裁定（见 [`derive_road_edges`]）与应用侧诊断展示。
    pub sources: Vec<(Id, Id, Id)>,
    /// 因地图无比例尺（`km_per_wu` 为 `None`）被跳过的道路线条数。
    pub skipped_no_scale: usize,
    /// 因端点容差内无合法锚定 POI（或两端吸到同一地点）被跳过的道路线条数。
    pub skipped_no_anchor: usize,
}

/// 从世界里的道路线段 POI 推导距离图的边（纯函数，不改 `world`）。
///
/// 遍历 `effective_line_kind == Road` 且地图锚定的 Place：
///
/// 1. 线两端各找**容差内最近的锚定非线 POI**（`effective_anchor` 距离
///    ≤ `tolerance_wu`；线段地点与未锚定地点不作端点）；
/// 2. 两端都命中且所在图有比例尺 → 一条候选边；
/// 3. 同端点对（无向）已有**手写 road 边**（`derived == false`）→ 让位
///    不产边——推导边与手写边不得为同一对端点各存一份（单一真值）；
/// 4. 同端点对的**多条候选边**（平行路线吸到同一对镇子）确定性去重：
///    `distance_km` 最长者赢，并列取「产边之路 Place ID」字典序小者
///    （ADR-0070 D5 评审裁定；产边之路见 [`RoadDeriveReport::sources`]）；
/// 5. 两端吸到同一地点 = 自环，不产边（计入 `skipped_no_anchor`）。
///
/// `distance_km = 线长(wu) × km_per_wu`（[`MapNode::km_per_wu`] 的 x 分量，
/// 虚构图各向同性；真图经 bbox 折算，长度取 x/y 均值的 x 侧是可接受近似）。
/// 边 ID 确定性：`road_<字典序小端点>_<字典序大端点>`。
pub fn derive_road_edges(world: &World, tolerance_wu: f32) -> RoadDeriveReport {
    let mut report = RoadDeriveReport::default();
    let places = &world.distances.places;

    // 手写 road 边的端点对（无向）缓存：同对即让位。只看 derived == false
    // ——推导边自己不压制推导（rebuild 每轮先清后算）。
    let hand_road_pairs: Vec<(&Id, &Id)> = world
        .distances
        .edges
        .iter()
        .filter(|e| !e.derived && e.terrain == "road")
        .map(|e| (&e.a, &e.b))
        .collect();

    // 候选边 + 产边之路：同端点对去重的并列裁定要问「这条边是谁修的」，
    // 溯源挂在 Report 侧（评审裁定：不污染 PlaceEdge），先随候选一起收集。
    let mut candidates: Vec<(PlaceEdge, Id)> = Vec::new();

    for line_place in places {
        if effective_line_kind(line_place) != Some(LineKind::Road) {
            continue;
        }
        // 道路线必须地图锚定：端点吸附要在同一张图的两点之间比距离。
        let Some(map_id) = &line_place.map else {
            continue;
        };
        let Some(line) = &line_place.line else {
            continue;
        };
        let Some((start, end)) = line_endpoints(line) else {
            continue;
        };
        let Some(km_per_wu) = scale_of(world, map_id) else {
            report.skipped_no_scale += 1;
            continue;
        };
        let Some(a) = nearest_anchor(places, map_id, line_place, start, tolerance_wu) else {
            report.skipped_no_anchor += 1;
            continue;
        };
        let Some(b) = nearest_anchor(places, map_id, line_place, end, tolerance_wu) else {
            report.skipped_no_anchor += 1;
            continue;
        };
        // 两端吸到同一地点 = 自环：边表语义里 a→a 没有意义，按未成对计。
        if a == b {
            report.skipped_no_anchor += 1;
            continue;
        }
        // 边 ID 确定性：端点按字典序摆进 ID 与 a/b 字段。
        let (lo, hi) = ordered_pair(&a, &b);
        let suppressed = hand_road_pairs.iter().any(|(x, y)| {
            (x.as_str() == lo && y.as_str() == hi) || (x.as_str() == hi && y.as_str() == lo)
        });
        if suppressed {
            continue;
        }
        candidates.push((
            PlaceEdge {
                id: Id::new_unchecked(format!("road_{lo}_{hi}")),
                a: Id::new_unchecked(lo.clone()),
                b: Id::new_unchecked(hi),
                distance_km: line_length_wu(line) * km_per_wu,
                terrain: "road".into(),
                bidirectional: true,
                rank: None,
                derived: true,
            },
            line_place.id.clone(),
        ));
    }

    // ── 同端点对（无向）去重（ADR-0070 D5 评审裁定）──
    //
    // 平行路线吸到同一对镇子时只留一条：`distance_km` 最长者赢；并列取
    // 「产边之路 Place ID」字典序小者。确定性：候选序 = places 表序，
    // 替换只在「严格更长」或「同长且路 ID 更小」时发生——结果与遍历起点
    // 无关。O(n²) 扫描：推导边规模 = 路线数（编辑器规模 ≤ 数百），无须
    // 哈希索引；端点对已按字典序摆正，`a、b` 同时相等即同对。
    let mut picked: Vec<(PlaceEdge, Id)> = Vec::new();
    for (edge, src) in candidates {
        match picked
            .iter_mut()
            .find(|(e, _)| e.a == edge.a && e.b == edge.b)
        {
            Some((win, win_src)) => {
                if edge.distance_km > win.distance_km
                    || (edge.distance_km == win.distance_km && src < *win_src)
                {
                    *win = edge;
                    *win_src = src;
                }
            }
            None => picked.push((edge, src)),
        }
    }
    report.edges = picked.iter().map(|(e, _)| e.clone()).collect();
    report.sources = picked
        .into_iter()
        .map(|(e, s)| (e.a.clone(), e.b.clone(), s))
        .collect();
    report
}

/// 推导边的**全量重建**（ADR-0070 D5 的调用入口）。
///
/// 先清掉场上全部推导边（`derived == true`），再按 [`derive_road_edges`]
/// 重算追加——推导边是投影，比例尺 / 线几何 / 锚点任一变了，投影必须整体
/// 重算而不是增量缝补。**不进脏路径**：调用方不得因 derived 变化标脏
/// `distances/edges`（推导边永不落盘，落盘的是作者手写的边）。
pub fn rebuild_derived_edges(world: &mut World, tolerance_wu: f32) -> RoadDeriveReport {
    world.distances.edges.retain(|e| !e.derived);
    let report = derive_road_edges(world, tolerance_wu);
    world.distances.edges.extend(report.edges.iter().cloned());
    report
}

/// 道路线 POI 自身的**有效呈现级别**（与 [`PlaceEdge::effective_rank`]
/// 同一张冻结表，ADR-0047 D1）。
///
/// 显式 `place.rank` 原样赢；`None` 时按公里折算：≥ 100 km → 3、
/// ≥ 30 km → 2、否则 → 1；`distance_km = None`（图无比例尺）→ 1。
/// 与边的折算同表同注释——改动它等于改旧工程的可视行为，必须走 ADR。
pub fn road_line_effective_rank(place: &Place, distance_km: Option<f32>) -> u8 {
    if let Some(r) = place.rank {
        return r;
    }
    match distance_km {
        Some(km) if km >= 100.0 => 3,
        Some(km) if km >= 30.0 => 2,
        _ => 1,
    }
}

/// 地图的公里/世界单位系数（长度折算取 x 分量；虚构图各向同性）。
fn scale_of(world: &World, map_id: &Id) -> Option<f32> {
    world
        .maps
        .iter()
        .find(|m| &m.id == map_id)
        .and_then(MapNode::km_per_wu)
        .map(|(kx, _)| kx as f32)
}

/// 折线的首末端点（扁平 `[x0, y0, ...]` wu 点列）。不足两点 → `None`。
fn line_endpoints(line: &[f32]) -> Option<((f32, f32), (f32, f32))> {
    if line.len() < 4 {
        return None;
    }
    let n = line.len();
    Some(((line[0], line[1]), (line[n - 2], line[n - 1])))
}

/// 线长（wu）：相邻顶点距离之和（多段折线 = 各段相加，不是首尾直线距离）。
fn line_length_wu(line: &[f32]) -> f32 {
    let pts: Vec<(f32, f32)> = line
        .chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| (p[0], p[1]))
        .collect();
    pts.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// 端点在容差内的**最近**锚定非线 POI（ADR-0070 D5）。
///
/// 四个过滤条件都不可省：不吸附线段地点（河不该成为路的端点）、
/// 不吸附未锚定地点（没落图就无从比距离）、不吸附线自身、
/// **不吸附别的图上的地点**——Place 坐标是图内局部 wu，跨图同坐标是两套
/// 坐标系，数值撞进容差只是巧合（评审修复）。
fn nearest_anchor(
    places: &[Place],
    map_id: &Id,
    line_place: &Place,
    endpoint: (f32, f32),
    tolerance_wu: f32,
) -> Option<String> {
    let mut best: Option<(f32, &Place)> = None;
    for p in places {
        if p.id == line_place.id {
            continue;
        }
        if effective_line_kind(p).is_some() {
            continue;
        }
        // 同图性（含未锚定 = map 为 None）：端点吸附要在同一张图的两点之间比距离。
        if p.map.as_ref() != Some(map_id) {
            continue;
        }
        let Some((ax, ay)) = p.effective_anchor() else {
            continue;
        };
        let d = ((ax - endpoint.0).powi(2) + (ay - endpoint.1).powi(2)).sqrt();
        if d <= tolerance_wu {
            // 严格更近才替换：并列时保留先声明者（遍历序 = places 表序，稳定）。
            let take = match &best {
                None => true,
                Some((bd, _)) => d < *bd,
            };
            if take {
                best = Some((d, p));
            }
        }
    }
    best.map(|(_, p)| p.id.as_str().to_string())
}

/// 端点对按字典序摆正：`(小, 大)`。边 ID 与 a/b 字段的确定性都靠它。
fn ordered_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapLevel;

    fn vid(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 点状 POI：锚定在 world_main 上（x/y 显式）。
    fn point_place(id: &str, x: f32, y: f32) -> Place {
        Place {
            id: vid(id),
            name: id.into(),
            feature: None,
            map: Some(vid("world_main")),
            x: Some(x),
            y: Some(y),
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: None,
            kind: None,
            line_kind: None,
            line_width_m: None,
        }
    }

    /// 道路线段 POI：`line_kind = Road`，锚定在 world_main 上。
    fn road_line_place(id: &str, line: Vec<f32>) -> Place {
        Place {
            id: vid(id),
            name: id.into(),
            feature: None,
            map: Some(vid("world_main")),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(line),
            kind: None,
            line_kind: Some(LineKind::Road),
            line_width_m: None,
        }
    }

    /// 河流线段 POI：缺省（`line_kind = None`）语义即河流。
    fn river_line_place(id: &str, line: Vec<f32>) -> Place {
        Place {
            line_kind: None,
            ..road_line_place(id, line)
        }
    }

    fn map_node(unit_per_km: Option<f32>) -> MapNode {
        MapNode {
            id: vid("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            unit_per_km,
            ..MapNode::default()
        }
    }

    fn world_with(map: MapNode, places: Vec<Place>, edges: Vec<PlaceEdge>) -> World {
        let mut w = World::default();
        w.maps.push(map);
        w.distances.places = places;
        w.distances.edges = edges;
        w
    }

    /// 作者手写的 road 边（`derived = false`）。
    fn hand_edge(id: &str, a: &str, b: &str, km: f32) -> PlaceEdge {
        PlaceEdge {
            id: vid(id),
            a: vid(a),
            b: vid(b),
            distance_km: km,
            terrain: "road".into(),
            bidirectional: true,
            rank: None,
            derived: false,
        }
    }

    /// 百 wu 直路夹具：`unit_per_km = 8`（1 km = 8 wu）。
    fn straight_road_world(unit_per_km: Option<f32>) -> World {
        world_with(
            map_node(unit_per_km),
            vec![
                road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
                point_place("hamlet_alpha", 0.0, 0.0),
                point_place("village_beta", 100.0, 0.0),
            ],
            vec![],
        )
    }

    /// 冻结契约：推导边 `rank = None`，公里折算与 `PlaceEdge::effective_rank`
    /// 同一张表（≥100 → 3、≥30 → 2、否则 → 1；无比例尺 → 1）。
    #[test]
    fn derived_rank_folds_by_kilometres_like_place_edge() {
        let plain = point_place("probe", 0.0, 0.0);
        assert_eq!(
            road_line_effective_rank(&plain, Some(100.0)),
            3,
            "100 km 恰在干线门槛上"
        );
        assert_eq!(
            road_line_effective_rank(&plain, Some(520.0)),
            3,
            "长线是干线"
        );
        assert_eq!(
            road_line_effective_rank(&plain, Some(99.9)),
            2,
            "门槛下沿归区域级"
        );
        assert_eq!(
            road_line_effective_rank(&plain, Some(30.0)),
            2,
            "30 km 恰在区域门槛上"
        );
        assert_eq!(
            road_line_effective_rank(&plain, Some(29.9)),
            1,
            "门槛下沿归局部"
        );
        assert_eq!(
            road_line_effective_rank(&plain, Some(5.0)),
            1,
            "短线是局部道路"
        );
        assert_eq!(
            road_line_effective_rank(&plain, None),
            1,
            "无比例尺（distance_km = None）→ 局部"
        );
        let mut ranked = point_place("probe", 0.0, 0.0);
        ranked.rank = Some(0);
        assert_eq!(
            road_line_effective_rank(&ranked, Some(500.0)),
            0,
            "显式 rank 压过公里折算（作者定级永远赢）"
        );

        // 推导边本体：rank 恒 None，经 PlaceEdge::effective_rank 走同一张表。
        let w = straight_road_world(Some(1.0)); // 100 wu × 1.0 km/wu = 100 km
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(report.edges.len(), 1, "实得 {:?}", report.edges);
        assert_eq!(report.edges[0].rank, None, "推导边不落显式 rank");
        assert_eq!(
            report.edges[0].effective_rank(),
            3,
            "100 km 推导边按同一张表折算为干线"
        );
    }

    /// 手算期望值：线长 100 wu、`unit_per_km = 8.0`（1 km = 8 wu）
    /// → distance_km = 100 ÷ 8 = 12.5。边 ID 取两端点 ID 的字典序。
    #[test]
    fn derived_distance_km_uses_line_length_over_unit_per_km() {
        let w = straight_road_world(Some(8.0));
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(
            report.edges.len(),
            1,
            "两端命中 + 有比例尺 → 恰一条推导边，实得 {:?}",
            report.edges
        );
        let e = &report.edges[0];
        assert_eq!(
            e.id.as_str(),
            "road_hamlet_alpha_village_beta",
            "边 ID = road_<字典序小>_<字典序大>"
        );
        assert_eq!(e.a.as_str(), "hamlet_alpha", "a 取字典序小的端点");
        assert_eq!(e.b.as_str(), "village_beta", "b 取字典序大的端点");
        assert!(
            (e.distance_km - 12.5).abs() < 1e-3,
            "100 wu ÷ 8 (wu/km) = 12.5 km，实得 {}",
            e.distance_km
        );
        assert_eq!(e.terrain, "road", "推导边 terrain = road");
        assert!(e.bidirectional, "推导边双向");
        assert!(e.derived, "推导边必须带 derived 标记（ADR-0070 D5）");
        assert_eq!(report.skipped_no_scale, 0);
        assert_eq!(report.skipped_no_anchor, 0);
    }

    /// 多段折线的线长 = 各段距离之和（不是首尾直线距离）。
    #[test]
    fn polyline_distance_sums_all_segments() {
        // (0,0) → (60,0) → (60,80)：60 + 80 = 140 wu；8 wu/km → 17.5 km。
        // 首尾直线距离是 100 wu（12.5 km）——若实现只算首尾，此断言必炸。
        let w = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_bent", vec![0.0, 0.0, 60.0, 0.0, 60.0, 80.0]),
                point_place("hamlet_alpha", 0.0, 0.0),
                point_place("village_beta", 60.0, 80.0),
            ],
            vec![],
        );
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(report.edges.len(), 1, "实得 {:?}", report.edges);
        assert!(
            (report.edges[0].distance_km - 17.5).abs() < 1e-3,
            "折线长 140 wu ÷ 8 = 17.5 km（首尾直线只有 12.5 km），实得 {}",
            report.edges[0].distance_km
        );
    }

    /// 地图没有比例尺（`unit_per_km = None` 且无真实底图）→ 跳过推导并计数，
    /// 不产边、不猜单位。
    #[test]
    fn maps_without_unit_per_km_skip_derivation() {
        let w = straight_road_world(None);
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert!(
            report.edges.is_empty(),
            "无比例尺不得产边，实得 {:?}",
            report.edges
        );
        assert_eq!(report.skipped_no_scale, 1, "恰一条道路线因无比例尺被跳过");
        assert_eq!(report.skipped_no_anchor, 0);
    }

    /// 同端点对（无向）已有作者手写的 road 边 → 不产推导边（单一真值：
    /// 推导边与手写边不能为同一对端点各存一份）。
    #[test]
    fn existing_hand_edge_suppresses_derivation() {
        let places = vec![
            road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
            point_place("hamlet_alpha", 0.0, 0.0),
            point_place("village_beta", 100.0, 0.0),
            point_place("town_gamma", 200.0, 0.0),
        ];
        // 手写边恰是道路线连接的那一对 → 推导让位。
        let w = world_with(
            map_node(Some(8.0)),
            places.clone(),
            vec![hand_edge("e_hand", "village_beta", "hamlet_alpha", 12.0)],
        );
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert!(
            report.edges.is_empty(),
            "同端点对手写边在场 → 不产推导边，实得 {:?}",
            report.edges
        );
        assert_eq!(report.skipped_no_scale, 0);
        assert_eq!(report.skipped_no_anchor, 0, "让位不是失败，不计锚定跳过");
        // 反例：手写边接的是**别的**端点对 → 推导边照常产出。
        let w2 = world_with(
            map_node(Some(8.0)),
            places,
            vec![hand_edge("e_hand2", "hamlet_alpha", "town_gamma", 12.0)],
        );
        let report2 = derive_road_edges(&w2, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(
            report2.edges.len(),
            1,
            "不同端点对的手写边不压制推导，实得 {:?}",
            report2.edges
        );
    }

    /// 端点吸附只认容差内（≤ tolerance）的最近锚定 POI：容差内吸得上，
    /// 差 1 wu 都吸不上。
    #[test]
    fn line_endpoint_snaps_within_tolerance_only() {
        // 30 wu < 32 wu 容差 → 吸附成功，产边。
        let w_in = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
                point_place("hamlet_alpha", 30.0, 0.0),
                point_place("village_beta", 100.0, 0.0),
            ],
            vec![],
        );
        let r_in = derive_road_edges(&w_in, 32.0);
        assert_eq!(r_in.edges.len(), 1, "容差内应吸附成功，实得 {:?}", r_in);
        // 33 wu > 32 wu 容差 → 吸附失败，跳过并计数。
        let w_out = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
                point_place("hamlet_alpha", 33.0, 0.0),
                point_place("village_beta", 100.0, 0.0),
            ],
            vec![],
        );
        let r_out = derive_road_edges(&w_out, 32.0);
        assert!(
            r_out.edges.is_empty(),
            "容差外不得产边，实得 {:?}",
            r_out.edges
        );
        assert_eq!(r_out.skipped_no_anchor, 1, "端点未命中恰计一次");
        // 常量契约：默认容差 32 wu（ADR-0070 D5）。
        assert_eq!(ROAD_SNAP_TOLERANCE_WU, 32.0);
    }

    /// 吸附目标只认**锚定的非线 POI**：线段地点（哪怕贴着端点）与未锚定
    /// 地点都不是合法端点。
    #[test]
    fn line_places_and_unanchored_places_are_not_snapped() {
        let w = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
                // 河线中点 (5,5) 距道路起点约 7 wu——若实现误吸线 POI，必命中。
                river_line_place("river_a", vec![2.0, 5.0, 8.0, 5.0]),
                // 未锚定地点恰在起点上——若实现漏查锚定，必命中。
                Place {
                    map: None,
                    x: Some(0.0),
                    y: Some(0.0),
                    ..point_place("nowhere", 0.0, 0.0)
                },
                point_place("village_beta", 100.0, 0.0),
            ],
            vec![],
        );
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert!(
            report.edges.is_empty(),
            "线 POI / 未锚定地点都不得作端点，实得 {:?}",
            report.edges
        );
        assert_eq!(report.skipped_no_anchor, 1, "起点端无合法锚点，恰计一次");
    }

    /// 线段判别唯一入口（ADR-0070 D2 判别口径的收口）：`line` 缺席 → 不是
    /// 线段 POI；有 `line` 时缺省或显式 River 都是河，显式 Road 才是路。
    #[test]
    fn effective_line_kind_classifies_line_places_only() {
        let mut p = road_line_place("probe", vec![0.0, 0.0, 10.0, 0.0]);
        assert_eq!(
            effective_line_kind(&p),
            Some(LineKind::Road),
            "显式 Road → 路"
        );
        p.line_kind = None;
        assert_eq!(
            effective_line_kind(&p),
            Some(LineKind::River),
            "缺省（None）= 河（ADR-0046 存量零迁移）"
        );
        p.line_kind = Some(LineKind::River);
        assert_eq!(
            effective_line_kind(&p),
            Some(LineKind::River),
            "显式 River → 河"
        );
        p.line = None;
        assert_eq!(
            effective_line_kind(&p),
            None,
            "无 line → 不是线段 POI，line_kind 此时无意义"
        );
        assert_eq!(effective_line_kind(&point_place("q", 0.0, 0.0)), None);
    }

    /// rebuild：先清全部推导边再重算——手写边不动，重复 rebuild 不叠加。
    #[test]
    fn rebuild_derived_edges_replaces_projection_and_keeps_hand_edges() {
        let mut w = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
                point_place("hamlet_alpha", 0.0, 0.0),
                point_place("village_beta", 100.0, 0.0),
            ],
            vec![hand_edge("e_hand", "hamlet_alpha", "village_beta", 999.0)],
        );
        // 手写边占住端点对 → 推导让位；rebuild 只清 derived，手写边原样。
        let r1 = rebuild_derived_edges(&mut w, ROAD_SNAP_TOLERANCE_WU);
        assert!(
            r1.edges.is_empty(),
            "手写边在场，推导让位，实得 {:?}",
            r1.edges
        );
        assert_eq!(w.distances.edges.len(), 1, "手写边保留");

        // 删掉手写边后 rebuild → 产推导边；再 rebuild → 不叠加。
        w.distances.edges.clear();
        let r2 = rebuild_derived_edges(&mut w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(r2.edges.len(), 1, "首次 rebuild 产恰一条推导边");
        assert_eq!(w.distances.edges.len(), 1);
        let r3 = rebuild_derived_edges(&mut w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(r3.edges.len(), 1, "重复 rebuild 不得叠加推导边");
        assert_eq!(w.distances.edges.len(), 1, "边表长度稳定");
        assert_eq!(
            w.distances.edges.iter().filter(|e| e.derived).count(),
            1,
            "场上恰一条 derived 边"
        );
    }

    /// 吸附目标只认**与道路线同一张图**的锚定 POI：Place 坐标是图内局部 wu，
    /// 子图上的同坐标地点与世界图端点在数值上可能撞进容差，但那是两套坐标系
    /// ——跨图吸附会把毫不相干的子图地点焊进世界图路网（评审 Important 修复：
    /// 此前只查了「已锚定」，漏查「同一张图」）。
    #[test]
    fn anchors_from_other_maps_are_not_snapped() {
        let mut w = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_mid", vec![0.0, 0.0, 100.0, 0.0]),
                // 子图地点恰在道路起点上（数值距离 0 < 32 wu 容差）——
                // 若实现漏查同图性，必被误吸为端点。
                Place {
                    map: Some(vid("island_child")),
                    ..point_place("isle_harbour", 0.0, 0.0)
                },
                point_place("village_beta", 100.0, 0.0),
            ],
            vec![],
        );
        let mut island = map_node(Some(8.0));
        island.id = vid("island_child");
        w.maps.push(island);

        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert!(
            report.edges.is_empty(),
            "跨图地点不得作端点，实得 {:?}",
            report.edges
        );
        assert_eq!(report.skipped_no_anchor, 1, "起点端无同图锚点，恰计一次");
    }

    /// 平行路线同端点对（ADR-0070 D5 评审裁定）：两条路吸到同一对镇子 →
    /// 只剩一条推导边，且 `distance_km` 最长者赢。
    ///
    /// 手算夹具：短路直连 200 wu；长路绕行 100 + 30 + 100 = 230 wu
    /// （末端 (200, 30) 距 village_beta 30 wu，仍在 32 wu 容差内）。
    /// 8 wu/km 下长路 28.75 km、短路 25 km——若实现不去重或短边赢了，必炸。
    #[test]
    fn parallel_road_lines_dedupe_longest_wins() {
        let w = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_short", vec![0.0, 0.0, 200.0, 0.0]),
                road_line_place(
                    "road_long",
                    vec![0.0, 0.0, 100.0, 0.0, 100.0, 30.0, 200.0, 30.0],
                ),
                point_place("hamlet_alpha", 0.0, 0.0),
                point_place("village_beta", 200.0, 0.0),
            ],
            vec![],
        );
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(
            report.edges.len(),
            1,
            "同端点对只留一条推导边，实得 {:?}",
            report.edges
        );
        assert!(
            (report.edges[0].distance_km - 28.75).abs() < 1e-3,
            "长路赢：230 wu ÷ 8 = 28.75 km（短路只有 25 km），实得 {}",
            report.edges[0].distance_km
        );
        // sources 与 edges 逐位平行：胜者边由哪条路推出一目了然。
        assert_eq!(
            report.sources,
            vec![(vid("hamlet_alpha"), vid("village_beta"), vid("road_long"))],
            "产边溯源应指向长路，实得 {:?}",
            report.sources
        );
    }

    /// 去重的并列裁定：同端点对的候选边一样长时，取「产边之路 Place ID」
    /// 字典序小者——两条等长平行路，`road_alpha` 压过 `road_zeta`。
    /// 边本体（ID / 端点 / 公里）两案完全相同，唯一可观测差异在 sources。
    #[test]
    fn dedupe_tie_breaks_by_smaller_source_place_id() {
        let w = world_with(
            map_node(Some(8.0)),
            vec![
                // 表序故意让 road_zeta 在前：胜者由裁定规则选出，不由遍历序。
                road_line_place("road_zeta", vec![0.0, 0.0, 200.0, 0.0]),
                road_line_place("road_alpha", vec![0.0, 2.0, 200.0, 2.0]),
                point_place("hamlet_alpha", 0.0, 0.0),
                point_place("village_beta", 200.0, 0.0),
            ],
            vec![],
        );
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert_eq!(
            report.edges.len(),
            1,
            "同端点对只留一条推导边，实得 {:?}",
            report.edges
        );
        assert_eq!(
            report.sources.len(),
            1,
            "sources 与 edges 平行，实得 {:?}",
            report.sources
        );
        assert_eq!(
            report.sources[0].2.as_str(),
            "road_alpha",
            "并列时产边之路取字典序小者，实得 {:?}",
            report.sources
        );
    }

    /// 环形小路两端吸到**同一**地点 → 自环无意义，不产边。
    #[test]
    fn both_ends_snapping_to_one_place_yields_no_edge() {
        let w = world_with(
            map_node(Some(8.0)),
            vec![
                road_line_place("road_loop", vec![0.0, 0.0, 20.0, 0.0]),
                point_place("village_beta", 12.0, 0.0),
            ],
            vec![],
        );
        let report = derive_road_edges(&w, ROAD_SNAP_TOLERANCE_WU);
        assert!(
            report.edges.is_empty(),
            "两端同点 = 自环，不产边，实得 {:?}",
            report.edges
        );
        assert_eq!(report.skipped_no_anchor, 1, "未成对的锚定计入跳过");
    }
}
