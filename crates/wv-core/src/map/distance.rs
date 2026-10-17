//! 距离维度：地点图、边、交通工具、行程时间计算、路径搜索。
//!
//! 核心决策（§5.3 距离不存结论）：只存**参数**（距离、地形、速度、条件），
//! 行程时间永远由计算得出。交通工具或地形一改，所有结论自动更新。

use std::collections::HashMap;

use crate::{base::error::CoreError, base::id::Id};

/// 交通工具。对应 `distances/vehicles.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vehicle {
    pub id: Id,
    pub name: String,
    /// 瞬时交通工具（如传送阵）。`true` 时不参与速度公式，按零耗时边处理。
    #[serde(default)]
    pub instant: bool,
    /// 基础速度 km/h。`instant` 为 `true` 时可省略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_kmh: Option<f32>,
    /// 每日有效行驶时长（小时）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_hours: Option<f32>,
    /// 地形速度修正系数。
    #[serde(default)]
    pub terrain_modifiers: HashMap<String, f32>,
    /// 瞬时交通工具是否要求两端都有对应设施。
    #[serde(default)]
    pub requires_endpoints: bool,
    /// 前置条件（条件表达式引擎待定，见 §13）。此处先存裸字符串。
    #[serde(default)]
    pub requirements: Vec<String>,
}

impl Vehicle {
    /// 在给定地形上的有效日速度（km/天）。
    ///
    /// 公式（ADR-0024，单位修正）：
    /// ```text
    /// 有效日速度 = speed_kmh × terrain_mult × daily_hours   [km/日]
    /// ```
    /// 旧式 `× daily_hours / 24` 得到的是 km/h 量纲，`segment_days` 因之
    /// 放大 24 倍（2026-10-17 评审发现；ADR-0024 记录修正与影响面）。
    pub fn effective_daily_speed(&self, terrain: &str) -> Option<f32> {
        if self.instant {
            return None;
        }
        let speed = self.speed_kmh?;
        let hours = self.daily_hours?;
        let mult = self.terrain_modifiers.get(terrain).copied().unwrap_or(1.0);
        Some(speed * mult * hours)
    }

    /// 单段耗时（天）。返回 `None` 表示该工具无法使用（缺参数或瞬时）。
    pub fn segment_days(&self, distance_km: f32, terrain: &str) -> Option<f32> {
        if self.instant {
            return Some(0.0);
        }
        let daily = self.effective_daily_speed(terrain)?;
        if daily <= 0.0 {
            return None;
        }
        Some(distance_km / daily)
    }
}

/// 地点边。对应 `distances/edges.yaml`。
///
/// 同一对地点可有多条边（官道 vs 山道），路径搜索按时间最优选择。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaceEdge {
    pub id: Id,
    pub a: Id,
    pub b: Id,
    pub distance_km: f32,
    pub terrain: String,
    #[serde(default = "default_true")]
    pub bidirectional: bool,
}

fn default_true() -> bool {
    true
}

/// 地点节点。对应 `distances/places.yaml`。
///
/// ## 双重身份（2026-01 修订）
///
/// 1. **距离图节点**——`PlaceEdge.a/b` 与路径搜索用它做稳定 ID，不需要几何；
/// 2. **地图上的 icon + 范围**——用 `(map, x, y, w, h)` 定位。
///
/// `w/h` 可选：`None` = 地图上就是一个点 icon（如关隘），`Some` = 有矩形范围（如城市）。
/// 这正对应用户原话：「有些在世界地图上它可能就是一个 icon，但是他是有范围的」。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub id: Id,
    pub name: String,
    /// 地理事实类型（theme overlay 词汇，沿用 TerrainKind 的 u8 词汇表编号，
    /// 编号→名称→演绎规则映射由 ThemePack 承载）。`None` = 普通地点（现状）。
    ///
    /// 注意：wv-core 不依赖 wv-terrain（AGENTS §2.1），故此处是裸 `u8`——
    /// 语义上的 TerrainKind 编号，词汇对齐表见待裁 T4。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature: Option<u8>,
    /// 若该地点对应某张地图上的锚点，则引用其 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<Id>,
    /// 中心点 X（地图局部坐标）。`None` 表示不锚定到地图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// 中心点 Y（地图局部坐标）。`None` 表示不锚定到地图。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// 范围宽度（须与 `h` 成对出现）。`None` = 点 icon。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f32>,
    /// 范围高度（须与 `w` 成对出现）。`None` = 点 icon。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
}

impl Place {
    /// 点是否落在地点的范围内（含边界）。
    ///
    /// - `w` / `h` 都是 `Some` → 矩形判定（含边界）；
    /// - `w` / `h` 都是 `None` → 点判定（`EPS = 1e-3`）；
    /// - `x` / `y` 缺失 → 恒 `false`（未锚定到地图，无从判定）。
    ///
    /// 用途：几何化校验（「单位进入了事件范围却没列为参与者」）。
    pub fn contains(&self, px: f32, py: f32) -> bool {
        let (Some(cx), Some(cy)) = (self.x, self.y) else {
            return false;
        };
        match (self.w, self.h) {
            (Some(w), Some(h)) => (px - cx).abs() <= w / 2.0 && (py - cy).abs() <= h / 2.0,
            _ => {
                const EPS: f32 = 1e-3;
                (px - cx).abs() <= EPS && (py - cy).abs() <= EPS
            }
        }
    }

    /// 按地界环拟合锚点与范围：质心写入 `x/y`，包围盒宽高写入 `w/h`。
    ///
    /// 为什么需要同步：POI、标签远档判定与 `EventLocation::resolve` 今天都只读
    /// `x/y/w/h` 这四个投影字段；地界多边形才是几何真值。创建/编辑地界时在
    /// `wv-edit` 侧调用本方法，把真值投影回既有字段（与 ADR-0022 D2「双字段
    /// 同步」同口径），渲染与校验无需改读取方式。
    ///
    /// - 点数不足 3（无法构成多边形面）→ 原样返回，不改写任何字段；
    /// - 质心 = 顶点简单平均（复用 `Region::centroid` 同口径，够用且稳定）；
    /// - `w/h` = 包围盒宽高（不存旋转，与 `contains` 的轴对齐矩形判定一致）。
    pub fn fit_anchor_to_ring(&mut self, ring: &[f32]) {
        if ring.len() < 6 {
            return;
        }
        let n = (ring.len() / 2) as f32;
        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for pair in ring.chunks_exact(2) {
            let (px, py) = (pair[0], pair[1]);
            sx += px;
            sy += py;
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }
        self.x = Some(sx / n);
        self.y = Some(sy / n);
        self.w = Some(max_x - min_x);
        self.h = Some(max_y - min_y);
    }
}

/// 地点构成的图。原则以「图而非表」表达距离维度（§5.5）。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DistanceGraph {
    #[serde(default)]
    pub places: Vec<Place>,
    #[serde(default)]
    pub edges: Vec<PlaceEdge>,
    #[serde(default)]
    pub vehicles: Vec<Vehicle>,
}

/// 路径搜索中的一个状态：位置 + 当前交通工具。
///
/// 状态扩展为 `(PlaceId, VehicleId)` 是为了支持**中途换乘**（§10.2）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchState {
    pub place: Id,
    pub vehicle: Id,
}

/// 一段行程。
#[derive(Debug, Clone, PartialEq)]
pub struct JourneyLeg {
    pub from: Id,
    pub to: Id,
    pub vehicle: Id,
    pub edge: Id,
    pub days: f32,
}

/// 行程时间计算结果，含命中的路径。
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledJourney {
    pub legs: Vec<JourneyLeg>,
    pub total_days: f32,
}

/// 时间可行性校验结论（§10.3）。
#[derive(Debug, Clone, PartialEq)]
pub enum Feasibility {
    /// 可达，且在截止时间前到达。`arrival` 为天数计数器。
    Reachable {
        arrival: i64,
        journey: ScheduledJourney,
    },
    /// 可达但会迟到。`earliest_arrival` 为天数计数器。
    ArrivesLate {
        earliest_arrival: i64,
        delay_days: f32,
        journey: ScheduledJourney,
    },
    /// 不可达，附原因。
    Unreachable { reason: UnreachableReason },
}

/// 不可达的具体原因，直接反馈到 UI。
#[derive(Debug, Clone, PartialEq)]
pub enum UnreachableReason {
    /// 图中不存在从起点到终点的路径。
    NoPath,
    /// 存在路径，但没有任何交通工具可用。
    NoUsableVehicle,
    /// 交通工具的前置条件未被满足。
    RequirementsUnmet { vehicle: Id },
}

impl DistanceGraph {
    /// 按 ID 查地点。
    pub fn place(&self, id: &Id) -> Option<&Place> {
        self.places.iter().find(|p| &p.id == id)
    }

    /// 按 ID 查交通工具。
    pub fn vehicle(&self, id: &Id) -> Option<&Vehicle> {
        self.vehicles.iter().find(|v| &v.id == id)
    }

    /// 计算单段耗时（天）。
    ///
    /// 这是 §10.2 公式的唯一入口；禁止在别处重复实现速度换算。
    pub fn segment_days(&self, edge: &PlaceEdge, vehicle_id: &Id) -> Option<f32> {
        let vehicle = self.vehicle(vehicle_id)?;
        vehicle.segment_days(edge.distance_km, &edge.terrain)
    }

    /// Dijkstra 路径搜索，边权为该段在可用交通工具下的最短耗时。
    ///
    /// `allowed` 为本次搜索允许使用的交通工具集合。若为空，视为全部可用。
    ///
    /// 注意：节点状态扩展为 `(PlaceId, VehicleId)`，因此「换乘」被建模为
    /// 位于同一地点、但交通工具不同的两个状态之间的零耗时转移。
    pub fn shortest_journey(
        &self,
        from: &Id,
        to: &Id,
        allowed: &[Id],
    ) -> Result<ScheduledJourney, CoreError> {
        if self.place(from).is_none() {
            return Err(CoreError::UnknownPlace(from.to_string()));
        }
        if self.place(to).is_none() {
            return Err(CoreError::UnknownPlace(to.to_string()));
        }

        let vehicles: Vec<&Vehicle> = if allowed.is_empty() {
            self.vehicles.iter().collect()
        } else {
            self.vehicles
                .iter()
                .filter(|v| allowed.contains(&v.id))
                .collect()
        };
        if vehicles.is_empty() {
            return Err(CoreError::PathSearch("没有可用交通工具".into()));
        }

        // 距离表：state -> 最短天数
        let mut best: HashMap<SearchState, f32> = HashMap::new();
        // 前驱：state -> (prev_state, leg)
        let mut prev: HashMap<SearchState, (SearchState, JourneyLeg)> = HashMap::new();

        // 起点状态：起点 × 每种交通工具
        let mut frontier: Vec<(f32, SearchState)> = Vec::new();
        for v in &vehicles {
            let s = SearchState {
                place: from.clone(),
                vehicle: v.id.clone(),
            };
            best.insert(s.clone(), 0.0);
            frontier.push((0.0, s));
        }

        let mut goal: Option<SearchState> = None;

        // 朴素 Dijkstra（节点数级别为「地点 × 工具」，规模可控）
        while !frontier.is_empty() {
            // 取当前最小
            let (idx, _) = frontier
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap();
            let (cur_cost, cur) = frontier.swap_remove(idx);

            if cur.place == *to {
                goal = Some(cur.clone());
                break;
            }
            if cur_cost > *best.get(&cur).unwrap_or(&f32::INFINITY) {
                continue;
            }

            // 1) 换乘：同一地点，换另一种交通工具，零耗时
            for v in &vehicles {
                if v.id == cur.vehicle {
                    continue;
                }
                let next = SearchState {
                    place: cur.place.clone(),
                    vehicle: v.id.clone(),
                };
                if cur_cost < *best.get(&next).unwrap_or(&f32::INFINITY) {
                    best.insert(next.clone(), cur_cost);
                    prev.insert(
                        next.clone(),
                        (
                            cur.clone(),
                            JourneyLeg {
                                from: cur.place.clone(),
                                to: cur.place.clone(),
                                vehicle: v.id.clone(),
                                edge: Id::new_unchecked("__transfer__"),
                                days: 0.0,
                            },
                        ),
                    );
                    frontier.push((cur_cost, next));
                }
            }

            // 2) 沿边前进
            for edge in self
                .edges
                .iter()
                .filter(|e| e.a == cur.place || e.b == cur.place)
            {
                let other = if edge.a == cur.place {
                    &edge.b
                } else {
                    &edge.a
                };
                // 单向边只能从 a 到 b
                if !edge.bidirectional && edge.b != *other {
                    continue;
                }
                if !edge.bidirectional && edge.a != cur.place {
                    continue;
                }
                let Some(days) = self.segment_days(edge, &cur.vehicle) else {
                    continue;
                };
                let next = SearchState {
                    place: other.clone(),
                    vehicle: cur.vehicle.clone(),
                };
                let cost = cur_cost + days;
                if cost < *best.get(&next).unwrap_or(&f32::INFINITY) {
                    best.insert(next.clone(), cost);
                    prev.insert(
                        next.clone(),
                        (
                            cur.clone(),
                            JourneyLeg {
                                from: cur.place.clone(),
                                to: other.clone(),
                                vehicle: cur.vehicle.clone(),
                                edge: edge.id.clone(),
                                days,
                            },
                        ),
                    );
                    frontier.push((cost, next));
                }
            }
        }

        let Some(goal) = goal else {
            return Err(CoreError::PathSearch("无可行路径".into()));
        };

        // 回溯路径
        let mut legs = Vec::new();
        let mut cursor = goal.clone();
        while let Some((p, leg)) = prev.get(&cursor).cloned() {
            if leg.edge.as_str() != "__transfer__" {
                legs.push(leg);
            }
            cursor = p;
        }
        legs.reverse();

        let total_days = *best.get(&goal).unwrap_or(&0.0);
        Ok(ScheduledJourney { legs, total_days })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vid(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 构造测试图：青云城 --官道300km-- 黑水村 --山道120km-- 落霞关
    fn fixture() -> DistanceGraph {
        DistanceGraph {
            places: vec![
                Place {
                    id: vid("city_qingyun"),
                    name: "青云城".into(),
                    map: None,
                    x: None,
                    y: None,
                    w: None,
                    h: None,
                    feature: None,
                },
                Place {
                    id: vid("village_heishui"),
                    name: "黑水村".into(),
                    map: None,
                    x: None,
                    y: None,
                    w: None,
                    h: None,
                    feature: None,
                },
                Place {
                    id: vid("pass_luoxia"),
                    name: "落霞关".into(),
                    map: None,
                    x: None,
                    y: None,
                    w: None,
                    h: None,
                    feature: None,
                },
            ],
            edges: vec![
                PlaceEdge {
                    id: vid("e_qingyun_heishui"),
                    a: vid("city_qingyun"),
                    b: vid("village_heishui"),
                    distance_km: 300.0,
                    terrain: "road".into(),
                    bidirectional: true,
                },
                PlaceEdge {
                    id: vid("e_heishui_luoxia"),
                    a: vid("village_heishui"),
                    b: vid("pass_luoxia"),
                    distance_km: 120.0,
                    terrain: "mountain".into(),
                    bidirectional: true,
                },
            ],
            vehicles: vec![
                Vehicle {
                    id: vid("walk"),
                    name: "步行".into(),
                    instant: false,
                    speed_kmh: Some(5.0),
                    daily_hours: Some(10.0),
                    terrain_modifiers: [
                        ("road".to_string(), 1.2),
                        ("plain".to_string(), 1.0),
                        ("forest".to_string(), 0.7),
                        ("mountain".to_string(), 0.4),
                    ]
                    .into_iter()
                    .collect(),
                    requires_endpoints: false,
                    requirements: vec![],
                },
                Vehicle {
                    id: vid("teleport_array"),
                    name: "传送阵".into(),
                    instant: true,
                    speed_kmh: None,
                    daily_hours: None,
                    terrain_modifiers: HashMap::new(),
                    requires_endpoints: true,
                    requirements: vec!["地点具有传送阵".into()],
                },
            ],
        }
    }

    #[test]
    fn effective_speed_applies_terrain_multiplier() {
        let g = fixture();
        let walk = g.vehicle(&vid("walk")).unwrap();
        // 5 km/h × 1.2 (road) × 10 h = 60 km/日（ADR-0024 单位修正）
        let s = walk.effective_daily_speed("road").unwrap();
        assert!((s - 60.0).abs() < 1e-6, "got {s}");
        // 未声明地形回退到 1.0
        let s2 = walk.effective_daily_speed("swamp").unwrap();
        assert!((s2 - 50.0).abs() < 1e-6, "got {s2}");
    }

    #[test]
    fn instant_vehicle_costs_zero_days() {
        let g = fixture();
        let tp = g.vehicle(&vid("teleport_array")).unwrap();
        assert_eq!(tp.segment_days(9999.0, "mountain"), Some(0.0));
        assert_eq!(tp.effective_daily_speed("road"), None);
    }

    #[test]
    fn shortest_journey_uses_fastest_terrain() {
        let g = fixture();
        // 300km 走官道：300 / (5*1.2*10) = 300 / 60 = 5 天
        // 120km 走山道：120 / (5*0.4*10) = 120 / 20 = 6 天
        // 合计 11 天
        let j = g
            .shortest_journey(&vid("city_qingyun"), &vid("pass_luoxia"), &[vid("walk")])
            .unwrap();
        assert_eq!(j.legs.len(), 2);
        assert!((j.total_days - 11.0).abs() < 1e-3, "got {}", j.total_days);
    }

    #[test]
    fn teleport_makes_route_instant() {
        let g = fixture();
        let j = g
            .shortest_journey(
                &vid("city_qingyun"),
                &vid("pass_luoxia"),
                &[vid("walk"), vid("teleport_array")],
            )
            .unwrap();
        // 传送阵使用同一张边表，但每段耗时为 0
        assert_eq!(j.total_days, 0.0);
    }

    #[test]
    fn unknown_place_is_reported() {
        let g = fixture();
        let err = g
            .shortest_journey(&vid("nowhere"), &vid("pass_luoxia"), &[])
            .unwrap_err();
        assert!(matches!(err, CoreError::UnknownPlace(_)));
    }

    // ---- 几何：Place 的范围判定（2026-01 修订）----

    /// 矩形地点：中心 (100, 200)，宽 40、高 20 → x ∈ [80, 120]，y ∈ [190, 210]。
    fn rect_place() -> Place {
        Place {
            id: vid("city_rect"),
            name: "rect city".into(),
            map: Some(vid("map_world")),
            x: Some(100.0),
            y: Some(200.0),
            w: Some(40.0),
            h: Some(20.0),
            feature: None,
        }
    }

    fn point_place() -> Place {
        Place {
            id: vid("pass_point"),
            name: "point pass".into(),
            map: Some(vid("map_world")),
            x: Some(10.0),
            y: Some(20.0),
            w: None,
            h: None,
            feature: None,
        }
    }

    fn unanchored_place() -> Place {
        Place {
            id: vid("place_nomap"),
            name: "unanchored".into(),
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            feature: None,
        }
    }

    #[test]
    fn rect_place_contains_interior_point() {
        assert!(rect_place().contains(100.0, 200.0), "centre must be inside");
        assert!(
            rect_place().contains(81.0, 191.0),
            "inner point must be inside"
        );
    }

    #[test]
    fn rect_place_contains_boundary_point() {
        // 含边界：四个角（x=80/120，y=190/210）都必须算在内。
        for (px, py) in [(80.0, 190.0), (120.0, 190.0), (80.0, 210.0), (120.0, 210.0)] {
            assert!(
                rect_place().contains(px, py),
                "boundary point ({px}, {py}) must be accepted (inclusive bounds)"
            );
        }
    }

    #[test]
    fn rect_place_rejects_point_just_outside_boundary() {
        assert!(
            !rect_place().contains(79.99, 200.0),
            "just outside the left edge must be rejected"
        );
        assert!(
            !rect_place().contains(100.0, 210.01),
            "just outside the bottom edge must be rejected"
        );
    }

    #[test]
    fn point_place_matches_exact_coordinate_only() {
        assert!(
            point_place().contains(10.0, 20.0),
            "point place hits its own coordinate"
        );
        assert!(
            !point_place().contains(10.5, 20.0),
            "point place must not hit a coordinate 0.5 away (EPS = 1e-3)"
        );
    }

    #[test]
    fn point_place_tolerates_sub_epsilon_drift() {
        assert!(
            point_place().contains(10.0005, 20.0),
            "5e-4 drift is within EPS = 1e-3 and must still hit"
        );
    }

    #[test]
    fn unanchored_place_contains_nothing() {
        assert!(
            !unanchored_place().contains(0.0, 0.0),
            "a place without x/y cannot be judged geometrically, so it is always false"
        );
    }

    #[test]
    fn half_given_extent_falls_back_to_point_judgement() {
        // w 有、h 无 → 不构成矩形，退化为点判定（校验层另行报「必须成对」）。
        let p = Place {
            w: Some(40.0),
            h: None,
            ..rect_place()
        };
        assert!(p.contains(100.0, 200.0), "centre still hits");
        assert!(
            !p.contains(110.0, 200.0),
            "w without h degrades to a point, so 110 must not hit"
        );
    }

    // ---- 地理事实类型 + 地界锚点拟合（theme overlay）----

    fn feature_place(feature: Option<u8>) -> Place {
        Place {
            id: Id::new_unchecked("place_dark_forest"),
            name: "黑森林".into(),
            map: Some(Id::new_unchecked("map_world")),
            x: None,
            y: None,
            w: None,
            h: None,
            feature,
        }
    }

    #[test]
    fn fit_anchor_to_ring_sets_centroid_and_bbox() {
        let mut p = feature_place(Some(4));
        p.fit_anchor_to_ring(&[0.0, 0.0, 40.0, 0.0, 40.0, 20.0, 0.0, 20.0]);
        assert_eq!((p.x, p.y), (Some(20.0), Some(10.0)), "质心为 (20,10)");
        assert_eq!((p.w, p.h), (Some(40.0), Some(20.0)), "包围盒 40×20");
    }

    #[test]
    fn fit_anchor_to_ring_with_degenerate_ring_leaves_anchors_untouched() {
        let mut p = feature_place(Some(4));
        p.fit_anchor_to_ring(&[1.0, 2.0]);
        assert_eq!(
            (p.x, p.y, p.w, p.h),
            (None, None, None, None),
            "不足 3 点不写锚点"
        );
    }
}
