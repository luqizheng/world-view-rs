//! 距离维度：地点图、边、交通工具、行程时间计算、路径搜索。
//!
//! 核心决策（§5.3 距离不存结论）：只存**参数**（距离、地形、速度、条件），
//! 行程时间永远由计算得出。交通工具或地形一改，所有结论自动更新。

use std::collections::HashMap;

use crate::{error::CoreError, id::Id, time::TimePoint};

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
    /// 公式（§10.2）：
    /// ```text
    /// 有效日速度 = speed_kmh × terrain_mult × (daily_hours / 24)
    /// ```
    pub fn effective_daily_speed(&self, terrain: &str) -> Option<f32> {
        if self.instant {
            return None;
        }
        let speed = self.speed_kmh?;
        let hours = self.daily_hours?;
        let mult = self.terrain_modifiers.get(terrain).copied().unwrap_or(1.0);
        Some(speed * mult * (hours / 24.0))
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    pub id: Id,
    pub name: String,
    /// 若该地点对应某张地图上的锚点，则引用其 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<Id>,
    /// 该地点具备的设施标签，用于 `requires_endpoints` 判定。
    #[serde(default)]
    pub facilities: Vec<String>,
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
    /// 可达，且在截止时间前到达。
    Reachable {
        arrival: TimePoint,
        journey: ScheduledJourney,
    },
    /// 可达但会迟到。
    ArrivesLate {
        earliest_arrival: TimePoint,
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
                    facilities: vec![],
                },
                Place {
                    id: vid("village_heishui"),
                    name: "黑水村".into(),
                    map: None,
                    facilities: vec![],
                },
                Place {
                    id: vid("pass_luoxia"),
                    name: "落霞关".into(),
                    map: None,
                    facilities: vec![],
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
        // 5 km/h × 1.2 (road) × (10/24) = 2.5 km/天
        let s = walk.effective_daily_speed("road").unwrap();
        assert!((s - 2.5).abs() < 1e-6);
        // 未声明地形回退到 1.0
        let s2 = walk.effective_daily_speed("swamp").unwrap();
        assert!((s2 - (5.0 * 1.0 * 10.0 / 24.0)).abs() < 1e-6);
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
        // 300km 走官道：300 / (5*1.2*10/24) = 300 / 2.5   = 120 天
        // 120km 走山道：120 / (5*0.4*10/24) = 120 / 0.8333 = 144 天
        // 合计 264 天
        let j = g
            .shortest_journey(&vid("city_qingyun"), &vid("pass_luoxia"), &[vid("walk")])
            .unwrap();
        assert_eq!(j.legs.len(), 2);
        assert!((j.total_days - 264.0).abs() < 1e-3, "got {}", j.total_days);
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
}
