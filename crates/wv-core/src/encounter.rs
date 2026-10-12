//! 相遇检测：派生「本该发生却没写事件」的时空点（ADR-0005，2026-10-11 用户裁定）。
//!
//! ## 裁定原文（唯一权威规格）
//!
//! > 「他们独立地在地图上移动，再某一刻相遇发生的事情叫做事件，如果他们相遇，
//! > 但是没有事情发生，那么就是啥事都没有。」
//!
//! > 「男女主角在某地相遇，本应该发生什么，作者故意没写。后期安排再相遇，然后
//! > 安排促进感情的内容就可以提及此地。」
//!
//! ## 三条推论
//!
//! 1. **相遇不建实体**——它是两条轨迹时空交叉的**派生结果**，落盘必然冗余。
//! 2. **只提示，不报错**（`AGENTS.md` §7.5 V1）：理由不是「别挡作者」，
//!    而是**留白本身就是创作手段**——所以报错在语义上就是错的。
//! 3. **提示必须可回访**——「后期回来提及此地」要求同一处相遇能再次被找到，
//!    且身份不随作者微调锚点而漂移。
//!
//! ## 判定依据：Waypoint (place, day) 重叠，零物理量（2026-10-12 用户裁定）
//!
//! 2026-10-12 用户简化方案：既然「路线」已经声明「某时间从 A 到 B」，
//! 相遇 = **两 Track 的 Waypoint 在 `(place, day)` 上同时覆盖**。
//!
//! 三个判定条件**全部**：
//! 1. 两条 Track 的 `span` 有时间重叠；
//! 2. 两 Waypoint 的 `place` 字段**完全相等**（字符串相等）；
//! 3. 两 Waypoint 的 `day` 字段**完全相等**。
//!
//! 没有任何距离阈值、没有「视界」、没有「坐标换算桥梁」——
//! 这就是用户原话：「**他们是行军相遇的，必定都在路上相遇**」。
//!
//! ### 缺字段时一律不判
//!
//! - Waypoint **没有 `place`**：无法锚定到「地点」级相遇 → 跳过；
//! - Waypoint **没有 `day`**：无法对齐到「同一天」→ 跳过；
//!
//! 这是「宽进严出」的延续：宁可漏提示，不可误报。
//!
//! ## 为什么指纹不含日期
//!
//! 指纹 = `unit_a | unit_b | place`（按字典序归并）。
//! 不含日期：日期是「这次相遇持续多久」，不是「这是哪一次相遇」。
//! 作者微调锚点日期，提示身份不变——才能被「已读/已忽略」认领。

use std::collections::BTreeMap;

use crate::event::Event;
use crate::geometry::Point;
use crate::id::Id;
use crate::time::TimeSpan;
use crate::track::Track;
use crate::unit::Unit;

/// 相遇提示的严重度。
///
/// **刻意只有「提示」一级**：相遇未写事件**永远不是错误**（见模块文档推论 2）。
/// 保留 `enum` 而非直接用单元结构，是为了让 UI 层将来若要加过滤/分组有据可依，
/// 且新增等级时不必改签名。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HintSeverity {
    /// 两条轨迹在同一时空相遇，但该处没有事件记录。
    ///
    /// **这不是错误**，只是一个「你可能是有意留白」的提醒。
    EncounterUnwritten,
}

impl HintSeverity {
    /// 人类可读名（供 UI 直接展示，避免各处各写一份）。
    pub fn label(self) -> &'static str {
        match self {
            HintSeverity::EncounterUnwritten => "相遇未写事件",
        }
    }

    /// 提示文案模板。`{0}`=单位 A 名，`{1}`=单位 B 名，`{2}`=地点名，`{3}`=日期间。
    pub fn message(self, a: &str, b: &str, place: &str, days: &str) -> String {
        match self {
            HintSeverity::EncounterUnwritten => {
                format!(
                    "「{a}」与「{b}」在 {place} 相遇（{days}），此处未写事件——若为有意留白可忽略"
                )
            }
        }
    }
}

/// 一次「相遇但未写事件」的提示（**派生数据，不落盘**）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EncounterHint {
    /// 稳定指纹：`{unit_a}|{unit_b}|{place}`。
    ///
    /// **不含日期**——日期是「这次相遇持续多久」，不是「这是哪一次相遇」。
    /// 顺序无关（先按字典序排），保证 A/B 交换得到同一指纹。
    pub fingerprint: String,
    /// 相遇的两个单位 ID。**已按字典序归并**，保证 A/B 顺序稳定。
    pub units: [Id; 2],
    /// 相遇地点（两边 Waypoint 共享的 place 引用）。
    pub place: Id,
    /// 相遇持续的时间区间（两条轨迹所有 `(place, day)` 命中的并集）。
    ///
    /// 可能为单点（瞬时相遇），此时 `from == to`。
    pub span: TimeSpan,
    /// 所属地图 ID（两条 Track 都必须挂在同一张图）。
    pub map: Id,
    pub severity: HintSeverity,
}

/// 相遇检测的输入。
///
/// **零物理量**：没有 maps 参数，没有 width_km，没有 sight_km。
/// 判定完全由 `Track.waypoints` 上的 `place` 与 `day` 字段决定。
#[derive(Debug, Clone, Copy, Default)]
pub struct EncounterInput<'a> {
    pub units: &'a [Unit],
    pub tracks: &'a [Track],
    pub events: &'a [Event],
}

/// 检测所有「相遇但未写事件」的时空点（**纯函数**，不读文件、不写状态）。
///
/// 返回值按 `fingerprint` 升序，保证**确定性**（`AGENTS.md` §1.4）。
///
/// ## 算法（刻意朴素，与跨度无关）
///
/// 1. 按单位分组轨迹；
/// 2. 逐对单位：枚举两条 Track 的 Waypoint 对；
/// 3. 当且仅当「place 相等 ∧ day 相等」时，命中一次相遇；
/// 4. 若该 `(place, day)` 没有事件覆盖 → 收集进对应指纹的 hint；
/// 5. 同一指纹的所有命中日归并为一个 `TimeSpan`。
pub fn detect_encounter_hints(input: EncounterInput<'_>) -> Vec<EncounterHint> {
    // 轨迹按单位分组；同一单位可能有多条轨迹（分段的行军）。
    let mut by_unit: BTreeMap<&str, Vec<&Track>> = BTreeMap::new();
    for t in input.tracks {
        by_unit.entry(t.unit.as_str()).or_default().push(t);
    }

    // fingerprint -> 暂存的 hint（先收集所有 day，最后归并 span）。
    let mut buckets: BTreeMap<String, EncounterHint> = BTreeMap::new();

    // 逐对单位。
    let unit_ids: Vec<&str> = by_unit.keys().copied().collect();
    for i in 0..unit_ids.len() {
        for j in (i + 1)..unit_ids.len() {
            let (a, b) = (unit_ids[i], unit_ids[j]);
            let a_tracks = by_unit.get(a).map(|v| v.as_slice()).unwrap_or(&[]);
            let b_tracks = by_unit.get(b).map(|v| v.as_slice()).unwrap_or(&[]);

            for ta in a_tracks {
                for tb in b_tracks {
                    // 地图不同 → 坐标系不可比，直接跳过。
                    if ta.map != tb.map {
                        continue;
                    }
                    // 两条 Track 的时间区间必须重叠。
                    if !ta.span.overlaps(&tb.span) {
                        continue;
                    }
                    let map_id = ta.map.clone();
                    for wa in &ta.waypoints {
                        for wb in &tb.waypoints {
                            let (Some(place_a), Some(place_b)) =
                                (wa.place.as_ref(), wb.place.as_ref())
                            else {
                                continue;
                            };
                            if place_a != place_b {
                                continue;
                            }
                            let (Some(day_a), Some(day_b)) = (wa.day, wb.day) else {
                                continue;
                            };
                            if day_a != day_b {
                                continue;
                            }
                            // 命中：两单位同一天同一地点。
                            if covered_by_event(place_a, wa.pos, day_a, input.events) {
                                continue;
                            }
                            let fingerprint = encounter_fingerprint(a, b, place_a.as_str());
                            let day = day_a;
                            buckets
                                .entry(fingerprint.clone())
                                .and_modify(|h| {
                                    // 扩张 span（保持确定性：取 [min(from), max(to+1))）。
                                    if h.span.from > day {
                                        h.span.from = day;
                                    }
                                    match h.span.to {
                                        Some(t) if t <= day => {
                                            h.span.to = Some(day.saturating_add(1));
                                        }
                                        None => {}
                                        _ => {}
                                    }
                                })
                                .or_insert_with(|| EncounterHint {
                                    fingerprint: fingerprint.clone(),
                                    units: [Id::new_unchecked(a), Id::new_unchecked(b)],
                                    place: place_a.clone(),
                                    span: TimeSpan::new(day, day.saturating_add(1)).unwrap(),
                                    map: map_id.clone(),
                                    severity: HintSeverity::EncounterUnwritten,
                                });
                        }
                    }
                }
            }
        }
    }

    let mut hints: Vec<EncounterHint> = buckets.into_values().collect();
    hints.sort_by(|x, y| x.fingerprint.cmp(&y.fingerprint));
    hints
}

/// 相遇的稳定指纹。
///
/// **不含日期**——日期是「这次相遇持续多久」，不是「这是哪一次相遇」。
/// 顺序无关（先按字典序排），保证 A/B 交换得到同一指纹。
pub fn encounter_fingerprint(unit_a: &str, unit_b: &str, place: &str) -> String {
    let (mut a, mut b) = (unit_a, unit_b);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    format!("{a}|{b}|{place}")
}

/// 该 `(place, pos, day)` 是否已被事件覆盖（作者写了）。
///
/// 判定口径（**宽进严出**：宁可漏提示，不可误报）：
/// - 事件 `span` 覆盖该 `day`（半开区间 `[from, to)`），且
/// - 事件的 `location.place` 与该 `place` 相等；并且
/// - 若事件**自带完整矩形**（`x/y/w/h` 齐全）且相遇点有坐标，
///   则进一步要求坐标落在矩形内；否则退回「同一 Place 即视为覆盖」。
///
/// 无 `location` 的事件不参与压制（无法可靠对齐地点）。
fn covered_by_event(
    meet_place: &Id,
    meet_pos: Option<Point>,
    meet_day: i64,
    events: &[Event],
) -> bool {
    let tp = crate::time::TimePoint(meet_day);
    events.iter().any(|e| {
        if !e.span.contains(&tp) {
            return false;
        }
        let Some(loc) = &e.location else {
            return false;
        };
        if &loc.place != meet_place {
            return false;
        }
        // 事件自带矩形 + 相遇点有坐标 → 精确几何判定。
        match (loc.x, loc.y, loc.w, loc.h, meet_pos) {
            (Some(cx), Some(cy), Some(w), Some(h), Some(p)) => {
                (p.x - cx).abs() <= w / 2.0 && (p.y - cy).abs() <= h / 2.0
            }
            // 缺几何信息 → 无法证伪，按「已覆盖」处理（宁可漏提示）。
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventLocation;
    use crate::track::Waypoint;

    fn id(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    /// 单位（不携带视野字段）。
    fn unit(id_s: &str, name: &str) -> Unit {
        Unit {
            id: id(id_s),
            name: name.into(),
            owner: None,
            members: vec![],
        }
    }

    /// 一条轨迹：在 `day0` 时位于 place_gate、在 `day1` 时位于 place_ridge。
    /// span = [day0, day1)。
    fn track(id_s: &str, unit_s: &str, day0: i64, day1: i64) -> Track {
        Track {
            id: id(id_s),
            unit: id(unit_s),
            map: id("map_world"),
            span: TimeSpan::new(day0, day1).unwrap(),
            waypoints: vec![
                Waypoint {
                    day: Some(day0),
                    pos: None,
                    place: Some(id("place_gate")),
                },
                Waypoint {
                    day: Some(day1),
                    pos: None,
                    place: Some(id("place_ridge")),
                },
            ],
            path: None,
        }
    }

    /// 一条「单点轨迹」：只有一个 Waypoint，落在 (day, place)。
    /// span = [day, day+1)（半开 1 天）。
    fn track_at(id_s: &str, unit_s: &str, day: i64, place: &str) -> Track {
        Track {
            id: id(id_s),
            unit: id(unit_s),
            map: id("map_world"),
            span: TimeSpan::new(day, day + 1).unwrap(),
            waypoints: vec![Waypoint {
                day: Some(day),
                pos: None,
                place: Some(id(place)),
            }],
            path: None,
        }
    }

    fn event(id_s: &str, place: &str, from: i64, to: i64) -> Event {
        Event {
            id: id(id_s),
            name: id_s.into(),
            span: TimeSpan::new(from, to).unwrap(),
            location: Some(EventLocation {
                place: id(place),
                x: None,
                y: None,
                w: None,
                h: None,
            }),
            participants: vec![],
            narrative: None,
            outcome: None,
            effects: vec![],
            narrations: vec![],
        }
    }

    fn input<'a>(
        units: &'a [Unit],
        tracks: &'a [Track],
        events: &'a [Event],
    ) -> EncounterInput<'a> {
        EncounterInput {
            units,
            tracks,
            events,
        }
    }

    #[test]
    fn two_units_with_shared_place_and_day_yields_hint() {
        // 两条轨迹都在 day 0 @ place_gate → 必相遇。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![
            track_at("t_a", "unit_a", 0, "place_gate"),
            track_at("t_b", "unit_b", 0, "place_gate"),
        ];
        let hints = detect_encounter_hints(input(&units, &tracks, &[]));
        assert_eq!(hints.len(), 1, "同地同天相遇且未写事件 → 一条提示");
        assert_eq!(hints[0].units[0].as_str(), "unit_a");
        assert_eq!(hints[0].units[1].as_str(), "unit_b");
        assert_eq!(hints[0].place.as_str(), "place_gate");
        assert_eq!(hints[0].span.from, 0);
    }

    #[test]
    fn waypoint_without_place_never_participates() {
        // 核心：Waypoint 缺 place → 无锚定地点 → 不参与判定。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let mut ta = track_at("t_a", "unit_a", 0, "place_gate");
        ta.waypoints[0].place = None; // 移除 place
        let tb = track_at("t_b", "unit_b", 0, "place_gate");
        assert!(
            detect_encounter_hints(input(&units, &[ta, tb], &[])).is_empty(),
            "Waypoint 无 place 不参与相遇判定"
        );
    }

    #[test]
    fn waypoint_without_day_never_participates() {
        // Waypoint 缺 day（顺序点，T2）→ 不参与校验。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let mut ta = track_at("t_a", "unit_a", 0, "place_gate");
        ta.waypoints[0].day = None;
        let tb = track_at("t_b", "unit_b", 0, "place_gate");
        assert!(
            detect_encounter_hints(input(&units, &[ta, tb], &[])).is_empty(),
            "Waypoint 无 day 不参与相遇判定"
        );
    }

    #[test]
    fn tracks_on_different_maps_never_meet() {
        // 坐标系不可比（ADR-0003：跨图靠 PlaceID 串联）。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let ta = track_at("t_a", "unit_a", 0, "place_gate");
        let mut tb = track_at("t_b", "unit_b", 0, "place_gate");
        tb.map = id("map_other");
        assert!(
            detect_encounter_hints(input(&units, &[ta, tb], &[])).is_empty(),
            "不同地图的同地不是相遇"
        );
    }

    #[test]
    fn tracks_with_disjoint_spans_never_meet() {
        // 两条 Track 时间完全不重叠 → 即使在「同一 place」也不会相遇。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let ta = track_at("t_a", "unit_a", 0, "place_gate");
        let tb = track_at("t_b", "unit_b", 100, "place_gate");
        assert!(
            detect_encounter_hints(input(&units, &[ta, tb], &[])).is_empty(),
            "时段无交集即使同地也不算相遇"
        );
    }

    #[test]
    fn different_place_does_not_count_as_meeting() {
        // 同天但在不同地点 → 不相遇。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let ta = track_at("t_a", "unit_a", 0, "place_gate");
        let tb = track_at("t_b", "unit_b", 0, "place_ridge");
        assert!(
            detect_encounter_hints(input(&units, &[ta, tb], &[])).is_empty(),
            "异地不同算相遇"
        );
    }

    #[test]
    fn event_at_meeting_place_and_day_suppresses_hint() {
        // 作者写了事件 → 不该提示（这是最核心的一条：提示不能扰民）。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![
            track_at("t_a", "unit_a", 0, "place_gate"),
            track_at("t_b", "unit_b", 0, "place_gate"),
        ];
        let events = vec![event("evt_x", "place_gate", 0, 1)];
        assert!(
            detect_encounter_hints(input(&units, &tracks, &events)).is_empty(),
            "相遇处已写事件，不应提示"
        );
    }

    #[test]
    fn event_at_different_place_does_not_suppress_hint() {
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![
            track_at("t_a", "unit_a", 0, "place_gate"),
            track_at("t_b", "unit_b", 0, "place_gate"),
        ];
        let events = vec![event("evt_x", "place_elsewhere", 0, 1)];
        assert_eq!(
            detect_encounter_hints(input(&units, &tracks, &events)).len(),
            1,
            "异地事件不应抑制相遇提示"
        );
    }

    #[test]
    fn event_without_place_does_not_suppress_hint() {
        // 缺 place 的事件无法对齐地点 → 不压制。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![
            track_at("t_a", "unit_a", 0, "place_gate"),
            track_at("t_b", "unit_b", 0, "place_gate"),
        ];
        let mut e = event("evt_x", "place_gate", 0, 1);
        e.location = None;
        assert_eq!(
            detect_encounter_hints(input(&units, &tracks, &[e])).len(),
            1,
            "事件无 place 不应压制相遇提示"
        );
    }

    #[test]
    fn fingerprint_is_order_independent() {
        let f1 = encounter_fingerprint("unit_b", "unit_a", "place_gate");
        let f2 = encounter_fingerprint("unit_a", "unit_b", "place_gate");
        assert_eq!(f1, f2, "A/B 交换必须得到同一指纹，否则无法认领");
    }

    #[test]
    fn fingerprint_does_not_contain_date() {
        let f1 = encounter_fingerprint("unit_a", "unit_b", "place_gate");
        assert_eq!(f1, "unit_a|unit_b|place_gate");
    }

    #[test]
    fn hint_is_stable_across_repeated_runs() {
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let a = detect_encounter_hints(input(&units, &tracks, &[]));
        let b = detect_encounter_hints(input(&units, &tracks, &[]));
        assert_eq!(a, b, "检测必须确定性");
    }

    #[test]
    fn hint_severity_is_never_error_level() {
        assert_eq!(HintSeverity::EncounterUnwritten.label(), "相遇未写事件");
    }

    #[test]
    fn empty_input_yields_no_hints() {
        assert!(detect_encounter_hints(EncounterInput::default()).is_empty());
    }

    #[test]
    fn encounter_only_on_exact_day_match_not_proximity() {
        // 关键：判定完全依赖数据严格相等（place + day），
        // 没有任何「距离相近也算」的弹性。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let ta = track_at("t_a", "unit_a", 0, "place_gate");
        let tb = track_at("t_b", "unit_b", 1, "place_gate"); // 错一天
        assert!(
            detect_encounter_hints(input(&units, &[ta, tb], &[])).is_empty(),
            "同地但 day 错开一天 → 不算相遇（无任何距离弹性）"
        );
    }

    #[test]
    fn multiple_days_at_same_place_merge_into_one_hint() {
        // 同一对 (units, place) 多天相遇 → 归并为一条 hint，span 覆盖多天。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let mut ta = track_at("t_a", "unit_a", 0, "place_gate");
        let mut tb = track_at("t_b", "unit_b", 0, "place_gate");
        // 加一个中间点，让两条轨迹在 day 5 也都到 place_gate。
        ta.waypoints.push(Waypoint {
            day: Some(5),
            pos: None,
            place: Some(id("place_gate")),
        });
        tb.waypoints.push(Waypoint {
            day: Some(5),
            pos: None,
            place: Some(id("place_gate")),
        });
        let hints = detect_encounter_hints(input(&units, &[ta, tb], &[]));
        assert_eq!(hints.len(), 1, "同指纹多天相遇归并为一条");
        assert_eq!(hints[0].span.from, 0);
    }
}
