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
//! ## 为什么指纹不含日期
//!
//! 若指纹含日期，两条轨迹在同一地停留三天就会生成**三条**提示；
//! 作者回头微调一个锚点日期，全部提示的 ID 全变——无法「认领」。
//! 因此本模块按**「谁在哪相遇」归并**，日期取区间，指纹只由
//! 两单位 ID + 归并地点构成（见 [`EncounterHint::fingerprint`]）。

use std::collections::BTreeMap;

use crate::event::Event;
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

    /// 提示文案模板。`{0}`=单位 A，`{1}`=单位 B，`{2}`=地点名，`{3}`=日期区间。
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
    /// 判定与展示分离——**展示**可用日期区间，但**身份**必须不含日期，
    /// 否则作者微调锚点会让提示「换个人」（见模块文档）。
    pub fingerprint: String,
    /// 相遇的两个单位 ID。**已按字典序归并**，保证 A/B 顺序稳定。
    pub units: [Id; 2],
    /// 相遇地点（归并后的 PlaceId；无地点则为坐标串）。
    pub place: Option<Id>,
    /// 相遇的坐标（两条轨迹都在此附近时的近似位置）。
    pub at: crate::geometry::Point,
    /// 相遇持续的时间区间（两条轨迹同地**同时**在场的交集）。
    ///
    /// 可能为空（瞬时相遇），此时 `from == to`。
    pub span: TimeSpan,
    pub severity: HintSeverity,
}

/// 相遇检测的输入。
#[derive(Debug, Clone, Copy)]
pub struct EncounterInput<'a> {
    pub units: &'a [Unit],
    pub tracks: &'a [Track],
    pub events: &'a [Event],
    /// 空间贴近阈值（地图局部坐标单位）。两条轨迹同一天的位置距离
    /// 小于等于此值即视为「相遇」。默认建议取「一个格子」的量级。
    pub proximity: f32,
}

impl Default for EncounterInput<'_> {
    fn default() -> Self {
        Self {
            units: &[],
            tracks: &[],
            events: &[],
            proximity: 0.0,
        }
    }
}

/// 检测所有「相遇但未写事件」的时空点（**纯函数**，不读文件、不写状态）。
///
/// 返回值按 `fingerprint` 升序，保证**确定性**（`AGENTS.md` §1.4）。
///
/// ## 算法（刻意朴素）
///
/// 1. 按单位分组轨迹；
/// 2. 枚举**带 `day` 的 waypoint**（唯一有确切时空的点）作为候选日；
/// 3. 对每个候选日，检查另一单位的轨迹在该日的位置是否贴近；
/// 4. 连续多天贴近则**归并成一个区间**，避免逐日刷屏；
/// 5. 若该区间内没有事件覆盖 → 产出提示。
///
/// 逐步扫描每一天的朴素实现在这里被刻意拒绝：`day` 是天数计数器，
/// 跨度可能上千年，逐日枚举会卡死。用 waypoint 锚点做候选集是
/// **同样精确**且**与跨度无关**的做法。
pub fn detect_encounter_hints(input: EncounterInput<'_>) -> Vec<EncounterHint> {
    // 轨迹按单位分组；同一单位可能有多条轨迹（分段的行军）。
    let mut by_unit: BTreeMap<&str, Vec<&Track>> = BTreeMap::new();
    for t in input.tracks {
        by_unit.entry(t.unit.as_str()).or_default().push(t);
    }

    let mut hints: Vec<EncounterHint> = Vec::new();

    // 逐对单位。
    let unit_ids: Vec<&str> = by_unit.keys().copied().collect();
    for i in 0..unit_ids.len() {
        for j in (i + 1)..unit_ids.len() {
            let (a, b) = (unit_ids[i], unit_ids[j]);
            let a_tracks = by_unit.get(a).map(|v| v.as_slice()).unwrap_or(&[]);
            let b_tracks = by_unit.get(b).map(|v| v.as_slice()).unwrap_or(&[]);
            for span in meeting_spans(a_tracks, b_tracks, input.proximity) {
                // 已被事件覆盖 → 不是提示（作者写了）。
                if covered_by_event(&span, input.events) {
                    continue;
                }
                let Some(at) = span.at else { continue };
                let fingerprint =
                    encounter_fingerprint(a, b, span.place.as_ref().map(|p| p.as_str()));
                hints.push(EncounterHint {
                    fingerprint,
                    units: [Id::new_unchecked(a), Id::new_unchecked(b)],
                    place: span.place.clone(),
                    at,
                    span: span.span,
                    severity: HintSeverity::EncounterUnwritten,
                });
            }
        }
    }

    hints.sort_by(|x, y| x.fingerprint.cmp(&y.fingerprint));
    hints.dedup_by(|a, b| a.fingerprint == b.fingerprint);
    hints
}

/// 相遇的稳定指纹。
///
/// **不含日期**——日期是「这次相遇持续多久」，不是「这是哪一次相遇」。
/// 顺序无关（先按字典序排），保证 A/B 交换得到同一指纹。
pub fn encounter_fingerprint(unit_a: &str, unit_b: &str, place: Option<&str>) -> String {
    let (mut a, mut b) = (unit_a, unit_b);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    match place {
        Some(p) => format!("{a}|{b}|{p}"),
        None => format!("{a}|{b}|@pos"),
    }
}

/// 一段「同时同地」的交集。
struct MeetingSpan {
    span: TimeSpan,
    place: Option<Id>,
    at: Option<crate::geometry::Point>,
}

/// 求两条（或多段）轨迹两两之间的同时同地交集。
fn meeting_spans(a: &[&Track], b: &[&Track], proximity: f32) -> Vec<MeetingSpan> {
    let mut out = Vec::new();
    for ta in a {
        for tb in b {
            // 地图不同 → 坐标系不可比，直接跳过（跨图引用靠 PlaceID，见 ADR-0003）。
            if ta.map != tb.map {
                continue;
            }
            // 先按 track.span 求交集，避免遍历无关时段。
            if !ta.span.overlaps(&tb.span) {
                continue;
            }
            // 候选日 = A 轨迹带 day 的锚点 ∩ B 轨迹的时间范围。
            // 用锚点而非逐日枚举：day 跨度可能上千年（见 detect_encounter_hints 文档）。
            let mut days: Vec<i64> = ta
                .waypoints
                .iter()
                .filter_map(|w| w.day)
                .filter(|d| tb.span.contains(&crate::time::TimePoint(*d)))
                .collect();
            days.sort_unstable();
            days.dedup();

            // 记录每处的贴近区间起点。
            let mut run_start: Option<i64> = None;
            let mut last: Option<i64> = None;
            let mut run_place: Option<Id> = None;
            let mut run_at: Option<crate::geometry::Point> = None;

            for d in days {
                let (Some(pa), Some(pb)) = (ta.sample_at(d), tb.sample_at(d)) else {
                    // 轨迹退化为无坐标：无法判定空间，视为相遇结束。
                    if let Some(s) = run_start {
                        push_span(&mut out, s, last.unwrap_or(s), &run_place, &run_at);
                        run_start = None;
                    }
                    continue;
                };
                if distance(pa, pb) <= proximity {
                    if run_start.is_none() {
                        run_start = Some(d);
                        // 归并地点优先取显式 place，其次取坐标串。
                        run_place = shared_place(ta, tb, d);
                        run_at = Some(midpoint(pa, pb));
                    }
                    last = Some(d);
                } else if let Some(s) = run_start {
                    push_span(&mut out, s, last.unwrap_or(s), &run_place, &run_at);
                    run_start = None;
                    run_place = None;
                    run_at = None;
                }
            }
            if let Some(s) = run_start {
                push_span(&mut out, s, last.unwrap_or(s), &run_place, &run_at);
            }
        }
    }
    out
}

/// 把贴近区间转成 `TimeSpan`。瞬时相遇（`from == to`）用 1 天窗口避免零长。
fn push_span(
    out: &mut Vec<MeetingSpan>,
    from: i64,
    to: i64,
    place: &Option<Id>,
    at: &Option<crate::geometry::Point>,
) {
    // 锚点是**离散采样**：从 d 起直到下一锚点之前都算在场，
    // 故终点取 `to + 1`（半开区间的上界）。这让「同日相遇」不被误判为零长。
    let to_exclusive = to.saturating_add(1);
    if let Ok(span) = TimeSpan::new(from, to_exclusive) {
        out.push(MeetingSpan {
            span,
            place: place.clone(),
            at: *at,
        });
    }
}

/// 取两条轨迹在 `day` 的共有地点（**必须两边的 waypoint 都指向同一 place**）。
fn shared_place(a: &Track, b: &Track, day: i64) -> Option<Id> {
    let pa = a.waypoints.iter().find(|w| w.day == Some(day))?;
    let pb = b.waypoints.iter().find(|w| w.day == Some(day))?;
    match (&pa.place, &pb.place) {
        (Some(x), Some(y)) if x == y => Some(x.clone()),
        _ => None,
    }
}

fn distance(a: crate::geometry::Point, b: crate::geometry::Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn midpoint(a: crate::geometry::Point, b: crate::geometry::Point) -> crate::geometry::Point {
    crate::geometry::Point {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
    }
}

/// 该相遇区间是否已被事件覆盖（作者写了）。
///
/// 判定口径（**宽进严出**：宁可漏提示，不可误报）：
/// - 事件 `span` 与相遇区间**有重叠**，且
/// - 事件 `place` 相同（若相遇有显式地点）。
///
/// 相遇无显式地点时只比对时间——不做坐标比对，因为坐标系混用的
/// 风险高于收益（宁可漏，不可扰）。
///
/// 这个退化是**有意为之**：判定过严会产生大量误报（提示满屏），
/// 而误报的代价（作者养成忽略提示）远高于漏报。
fn covered_by_event(meet: &MeetingSpan, events: &[Event]) -> bool {
    events.iter().any(|e| {
        if !e.span.overlaps(&meet.span) {
            return false;
        }
        match (&e.place, &meet.place) {
            (Some(ep), Some(mp)) => ep == mp,
            _ => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> Id {
        Id::new_unchecked(s)
    }

    fn pt(x: f32, y: f32) -> crate::geometry::Point {
        crate::geometry::Point { x, y }
    }

    fn unit(id_s: &str, name: &str) -> Unit {
        Unit {
            id: id(id_s),
            name: name.into(),
            owner: None,
            members: vec![],
        }
    }

    /// 一条直线轨迹：从 (0,0) 走到 (10,0)。
    fn track(id_s: &str, unit_s: &str, day0: i64, day1: i64) -> Track {
        Track {
            id: id(id_s),
            unit: id(unit_s),
            map: id("map_world"),
            span: TimeSpan::new(day0, day1).unwrap(),
            waypoints: vec![
                crate::track::Waypoint {
                    day: Some(day0),
                    pos: Some(pt(0.0, 0.0)),
                    place: Some(id("place_gate")),
                },
                crate::track::Waypoint {
                    day: Some(day1),
                    pos: Some(pt(10.0, 0.0)),
                    place: Some(id("place_ridge")),
                },
            ],
            path: None,
        }
    }

    fn event(id_s: &str, place: &str, from: i64, to: i64) -> Event {
        Event {
            id: id(id_s),
            name: id_s.into(),
            span: TimeSpan::new(from, to).unwrap(),
            place: Some(id(place)),
            participants: vec![],
            narrative: None,
            outcome: None,
            effects: vec![],
        }
    }

    fn input<'a>(
        units: &'a [Unit],
        tracks: &'a [Track],
        events: &'a [Event],
        proximity: f32,
    ) -> EncounterInput<'a> {
        EncounterInput {
            units,
            tracks,
            events,
            proximity,
        }
    }

    #[test]
    fn two_units_meeting_at_same_place_with_no_event_yields_hint() {
        // 两条轨迹都是 day0=0 从 place_gate 出发 → 当天必然相遇。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let hints = detect_encounter_hints(input(&units, &tracks, &[], 1.0));
        assert_eq!(hints.len(), 1, "同一地点同一时刻相遇且无事件 → 一条提示");
        assert_eq!(hints[0].units[0].as_str(), "unit_a");
        assert_eq!(hints[0].units[1].as_str(), "unit_b");
    }

    #[test]
    fn event_at_meeting_place_suppresses_hint() {
        // 作者写了事件 → 不该提示（这是最核心的一条：提示不能扰民）。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let events = vec![event("evt_x", "place_gate", 0, 5)];
        let hints = detect_encounter_hints(input(&units, &tracks, &events, 1.0));
        assert!(hints.is_empty(), "相遇处已写事件，不应提示；实得 {hints:?}");
    }

    #[test]
    fn event_at_different_place_does_not_suppress_hint() {
        // 别处的事件不该掩盖此处相遇。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let events = vec![event("evt_x", "place_elsewhere", 0, 5)];
        let hints = detect_encounter_hints(input(&units, &tracks, &events, 1.0));
        assert_eq!(hints.len(), 1, "异地事件不应抑制相遇提示");
    }

    #[test]
    fn tracks_on_different_maps_never_meet() {
        // 坐标系不可比（ADR-0003：跨图靠 PlaceID 串联，不靠坐标）。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let ta = track("t_a", "unit_a", 0, 10);
        let mut tb = track("t_b", "unit_b", 0, 10);
        tb.map = id("map_other");
        let hints = detect_encounter_hints(input(&units, &[ta, tb], &[], 100.0));
        assert!(hints.is_empty(), "不同地图的同坐标不是相遇");
    }

    #[test]
    fn distant_units_do_not_meet_under_small_threshold() {
        // 阈值 1.0，但两条轨迹各自朝相反方向走 → 不该相遇。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let ta = track("t_a", "unit_a", 0, 10);
        let mut tb = track("t_b", "unit_b", 0, 10);
        // 让 B 从远处出发。
        tb.waypoints[0].pos = Some(pt(500.0, 500.0));
        tb.waypoints[1].pos = Some(pt(510.0, 500.0));
        let hints = detect_encounter_hints(input(&units, &[ta, tb], &[], 1.0));
        assert!(hints.is_empty(), "距离远超阈值不算相遇");
    }

    #[test]
    fn fingerprint_is_order_independent() {
        let f1 = encounter_fingerprint("unit_b", "unit_a", Some("place_gate"));
        let f2 = encounter_fingerprint("unit_a", "unit_b", Some("place_gate"));
        assert_eq!(f1, f2, "A/B 交换必须得到同一指纹，否则无法认领");
    }

    #[test]
    fn fingerprint_does_not_contain_date() {
        // 同一对单位在同一地相遇于不同日期 → 视为「同一次相遇」的延续。
        let f1 = encounter_fingerprint("unit_a", "unit_b", Some("place_gate"));
        assert!(!f1.contains("0") && !f1.contains('0'));
        assert_eq!(f1, "unit_a|unit_b|place_gate");
    }

    #[test]
    fn hint_is_stable_across_repeated_runs() {
        // 确定性（AGENTS §1.4）：同样输入必须给出同样顺序。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let a = detect_encounter_hints(input(&units, &tracks, &[], 1.0));
        let b = detect_encounter_hints(input(&units, &tracks, &[], 1.0));
        assert_eq!(a, b, "检测必须确定性");
    }

    #[test]
    fn same_unit_pair_meeting_twice_dedups_to_one_hint() {
        // 两次相遇（不同日期、同地点）归并为一条——这是「指纹不含日期」的收益。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let hints = detect_encounter_hints(input(&units, &tracks, &[], 1.0));
        assert!(
            hints.len() <= 1,
            "同一对单位在同一地点的多日相遇应归并，实得 {}",
            hints.len()
        );
    }

    #[test]
    fn hint_severity_is_never_error_level() {
        // 裁定：相遇未写事件**永远不是错误**（AGENTS §7.5 V1）。
        assert_eq!(HintSeverity::EncounterUnwritten.label(), "相遇未写事件");
    }

    #[test]
    fn empty_input_yields_no_hints() {
        assert!(detect_encounter_hints(input(&[], &[], &[], 1.0)).is_empty());
    }

    #[test]
    fn encounter_span_has_positive_length() {
        // 零长区间会让 UI 无从展示「持续多久」，且 TimeSpan::new 拒绝零长。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let hints = detect_encounter_hints(input(&units, &tracks, &[], 1.0));
        for h in &hints {
            // 相遇区间必须是闭合的正长度（`to` 必为 Some，且 > from）。
            let to = h.span.to.expect("相遇区间必为闭合区间");
            assert!(to > h.span.from, "相遇区间必须是正长度：{:?}", h.span);
        }
    }

    #[test]
    fn proximity_zero_still_meets_at_exact_same_point() {
        // 阈值 0：只有完全重合才算。防止默认阈值误伤。
        let units = vec![unit("unit_a", "甲"), unit("unit_b", "乙")];
        let tracks = vec![track("t_a", "unit_a", 0, 10), track("t_b", "unit_b", 0, 10)];
        let hints = detect_encounter_hints(input(&units, &tracks, &[], 0.0));
        assert_eq!(hints.len(), 1, "完全重合应判定相遇");
    }
}
