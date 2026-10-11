//! 轨迹维度：某主体在某张地图上的一段运动记录（基线 §1.6 / §2.7）。
//!
//! ## 轨迹挂在哪
//!
//! 轨迹是**独立实体**，由**事态节点**（[`crate::situation::SituationStep`]）通过 ID 引用。
//! 理由（基线 §1.6）：轨迹需要同时知道 ① 属于哪个事态节点（语境）② 谁在动（主体）
//! ③ 在哪张图（坐标系）——只有事态节点同时持有这三个上下文。
//!
//! 独立成实体而非内联，是为了避免 P4 复制：同一条轨迹被两条事态引用时**只存一份**。
//!
//! ## 时间戳三条规则（T1–T3）
//!
//! | # | 规则 |
//! |---|---|
//! | **T1** | 有 `day` 的点 → 该主体在该天确定在此位置（参与四维校验） |
//! | **T2** | 无 `day` 的点 → 视为「顺序点」，由插值决定，**不参与校验** |
//! | **T3** | 只填两端点的轨迹，其时间由两端点事件的时间范围给出 |

use crate::{geometry::Point, id::Id, map::GeometryData, time::TimeSpan};

/// 轨迹。对应 `tracks/<id>.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub id: Id,
    /// 轨迹所属主体（角色或势力）。
    pub owner: Id,
    /// 轨迹发生的地图（决定坐标系与 LOD）。
    pub map: Id,
    /// 轨迹的时间区间（半开 `[from, to)`）。
    #[serde(flatten)]
    pub span: TimeSpan,
    /// 途经点序列。
    pub waypoints: Vec<Waypoint>,
    /// 轨迹的几何表达（用于渲染连线）。可选。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<GeometryData>,
}

impl Track {
    /// 在 `day` 时刻采样轨迹的位置（**纯函数**）。
    ///
    /// 规范（基线 §1.6 T1–T3 + 动画规格 §1.2 逐段生长）：
    ///
    /// 1. 只考虑**带 `day` 且按时序排列**的锚点；`day == None` 的顺序点在此版本
    ///    暂不参与插值（其推算归属 `ANIMATION_SPEC` §1.3 的「均分到 span」，
    ///    留给 S8 的播放实现；本函数先给出精轨迹的确切结果）。
    /// 2. `t <= p0.day` → 返回 `p0`；`t >= pn.day` → 返回 `pn`。
    /// 3. 落在两锚点之间 → 对 `pos` 做**线性插值**。
    ///
    /// 位置只由 `day` 与 `waypoints` 决定，**不存在独立的「速度」参数**——
    /// 「速度」是段长 / 段时长的产物（动画规格 §1.5）。
    pub fn sample_at(&self, day: i64) -> Option<Point> {
        // 收集带时间戳、且有坐标的锚点，按 day 升序。
        let mut anchors: Vec<(i64, Point)> = self
            .waypoints
            .iter()
            .filter_map(|w| w.day.zip(w.pos))
            .collect();
        if anchors.is_empty() {
            // 退化：没有带时间戳的点时，退回第一个有坐标的点。
            return self.waypoints.iter().find_map(|w| w.pos);
        }
        anchors.sort_by_key(|(d, _)| *d);

        if day <= anchors[0].0 {
            return Some(anchors[0].1);
        }
        let last = anchors.len() - 1;
        if day >= anchors[last].0 {
            return Some(anchors[last].1);
        }

        // 找到 day 所在区间 [i, i+1)。
        let mut i = 0;
        while i + 1 < anchors.len() && anchors[i + 1].0 <= day {
            i += 1;
        }
        let (d0, p0) = anchors[i];
        let (d1, p1) = anchors[i + 1];
        let span = (d1 - d0) as f32;
        if span <= 0.0 {
            return Some(p0);
        }
        let u = (day - d0) as f32 / span;
        Some(Point {
            x: p0.x + (p1.x - p0.x) * u,
            y: p0.y + (p1.y - p0.y) * u,
        })
    }
}

/// 轨迹上的一个途经点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Waypoint {
    /// 到达该点的时间（天数计数器）。**可选**（无 = 顺序点，不参与校验，T2）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<i64>,
    /// 该点在 `map` 内的局部坐标。与 `place` 二选一（`place` 优先）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<Point>,
    /// 该点对应的地点（可选）。若有，则与 `Place` 图对齐，可用于距离校验。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Id>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wp(day: Option<i64>, x: f32, y: f32) -> Waypoint {
        Waypoint {
            day,
            pos: Some(Point { x, y }),
            place: None,
        }
    }

    fn track(waypoints: Vec<Waypoint>) -> Track {
        Track {
            id: Id::new_unchecked("track_t"),
            owner: Id::new_unchecked("char_a"),
            map: Id::new_unchecked("map_world"),
            span: TimeSpan::new(100, 400).unwrap(),
            waypoints,
            path: None,
        }
    }

    #[test]
    fn before_first_waypoint_returns_start() {
        let t = track(vec![wp(Some(100), 0.0, 0.0), wp(Some(200), 100.0, 0.0)]);
        let p = t.sample_at(50).unwrap();
        assert_eq!((p.x, p.y), (0.0, 0.0));
    }

    #[test]
    fn between_waypoints_lerps_linearly() {
        let t = track(vec![wp(Some(100), 0.0, 0.0), wp(Some(200), 100.0, 200.0)]);
        let p = t.sample_at(150).unwrap();
        assert!((p.x - 50.0).abs() < 1e-4, "got {}", p.x);
        assert!((p.y - 100.0).abs() < 1e-4, "got {}", p.y);
    }

    #[test]
    fn after_last_waypoint_returns_end() {
        let t = track(vec![wp(Some(100), 0.0, 0.0), wp(Some(200), 100.0, 0.0)]);
        let p = t.sample_at(999).unwrap();
        assert_eq!((p.x, p.y), (100.0, 0.0));
    }

    #[test]
    fn mixed_timestamped_and_sequential_points_use_timestamped_anchors() {
        // 中间的 None 点是顺序点，本函数只用带时间戳的锚点插值。
        let t = track(vec![
            wp(Some(100), 0.0, 0.0),
            wp(None, 50.0, 50.0),
            wp(Some(200), 100.0, 0.0),
        ]);
        let p = t.sample_at(150).unwrap();
        assert!((p.x - 50.0).abs() < 1e-4, "got {}", p.x);
        assert!((p.y - 0.0).abs() < 1e-4, "got {}", p.y);
    }

    #[test]
    fn unsorted_waypoints_are_sorted_before_sampling() {
        let t = track(vec![wp(Some(200), 100.0, 0.0), wp(Some(100), 0.0, 0.0)]);
        let p = t.sample_at(150).unwrap();
        assert!((p.x - 50.0).abs() < 1e-4, "乱序航点应先排序, got {}", p.x);
    }
}
