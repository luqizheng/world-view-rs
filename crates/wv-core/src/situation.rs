//! 事态维度：事件之上的**组织层**（基线 §1.1–§1.7 / §2.7）。
//!
//! ## 事态是什么
//!
//! **事态 = 「一段时间内，一件有始有终的大事，拆成几个先后发生的关键事件。」**
//!
//! 它**不复制** `Event` 的内容，只引用 `EventId`（不变式 I1）。
//! 一个事件可以不属于任何事态（I3，孤立事件是一等公民），
//! 也可以属于多条事态（I4）。
//!
//! ## 链的顺序：只能由时间派生（硬约束 A1）
//!
//! `steps` 是**无序**的。播放顺序由每个 step 的事件 `span.from` **升序派生**，
//! **不存序号**。存序号 = 制造「两个真相」（时间 vs 序号），必然有一个是假的。
//!
//! 唯一的例外是**可选的 `narrative_order`**——它是「倒叙 / 插叙」的**呈现技法**，
//! 不影响时间轴的任何求值，只是改变播放的先后。
//!
//! 排序实现落在 [`Situation::playback_order`]（**纯函数，可单测**），
//! `wv-app` 只调用它，**不在渲染层排序**（`AGENTS.md` §3.2）。

use crate::{event::Event, id::Id, time::TimeSpan};

/// 事态。对应 `timeline/situations/<id>.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Situation {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// 事态包含的事件引用（**无序**，播放顺序由事件 `span.from` 升序派生）。
    #[serde(default)]
    pub steps: Vec<SituationStep>,
    /// 事态的整体时间范围。**必须覆盖其全部节点的时间**（不变式 I5）。
    ///
    /// 播放时锁定在本区间内循环（基线 §1.8）。
    #[serde(flatten)]
    pub span: TimeSpan,
    /// 可选的**叙事顺序**覆盖（D1 补充）：供倒叙 / 插叙编排。
    ///
    /// - `None`：按时间派生；
    /// - `Some(list)`：先按 `list` 顺序，未覆盖的 step 追加其后（按时间升序）。
    ///
    /// **它不影响时间轴的任何求值**——只是播放的先后。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative_order: Option<Vec<Id>>,
    /// 本事的涉及的地图（跨图播放时按需切换，基线 §2.14）。
    #[serde(default)]
    pub maps: Vec<Id>,
    /// 主角（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protagonist: Option<Id>,
}

/// 事态的一步。**不含序号**（D1：顺序由时间派生）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SituationStep {
    /// 引用的事件 ID。**不复制事件内容**（I1）。
    pub event: Id,
    /// 本节点是否为播放时的「焦点」。
    #[serde(default)]
    pub focus: bool,
    /// 播放到本节点时的停顿秒数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_secs: Option<f32>,
    /// 链视角的注释（不是事件本身的描述）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// 本节挂载的轨迹 ID 列表（轨迹是「事态节点」的属性，基线 §1.6）。
    #[serde(default)]
    pub tracks: Vec<Id>,
}

impl Situation {
    /// 派生播放顺序：返回 step 引用的 `EventId` 序列。
    ///
    /// 规则（基线 §2.7）：
    /// | 情况 | 播放顺序 |
    /// |---|---|
    /// | `narrative_order == None` | 按 `step.event` 对应 `Event.span.from` **升序** |
    /// | `narrative_order == Some(list)` | 按 `list` 顺序；未覆盖的 step 追加其后（按时间升序） |
    ///
    /// 时间相同时按 `EventId` 字典序兜底，保证排序**确定性**（基线 §1.4）。
    ///
    /// 本函数**信任已校验的数据**：`event` 引用必须在 `events` 中能找到，
    /// 否则在 `wv-io` 加载期就应报 `DanglingReference`（基线 §2.21）。
    /// 找不到引用的 step 被跳过（防御性，不 panic）。
    pub fn playback_order(&self, events: &[Event]) -> Vec<Id> {
        // 时间派生顺序：按 (span.from, event_id) 升序。
        let mut by_time: Vec<(i64, Id)> = self
            .steps
            .iter()
            .filter_map(|s| {
                events
                    .iter()
                    .find(|e| e.id == s.event)
                    .map(|e| (e.span.from, s.event.clone()))
            })
            .collect();
        by_time.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let by_time: Vec<Id> = by_time.into_iter().map(|(_, id)| id).collect();

        let Some(order) = &self.narrative_order else {
            return by_time;
        };

        // 叙事顺序优先；未覆盖的 step 按时间顺序追加。
        let mut result: Vec<Id> = Vec::with_capacity(self.steps.len());
        for id in order {
            // 只接受真正在本事态里的 step（拒绝悬空 ID）。
            if self.steps.iter().any(|s| &s.event == id) && !result.contains(id) {
                result.push(id.clone());
            }
        }
        for id in by_time {
            if !result.contains(&id) {
                result.push(id);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::TimeSpan;

    fn ev(id: &str, from: i64, to: i64) -> Event {
        Event {
            id: Id::new_unchecked(id),
            name: id.into(),
            span: TimeSpan::new(from, to).unwrap(),
            place: None,
            participants: vec![],
            outcome: None,
            effects: vec![],
        }
    }

    fn step(id: &str) -> SituationStep {
        SituationStep {
            event: Id::new_unchecked(id),
            focus: false,
            pause_secs: None,
            caption: None,
            tracks: vec![],
        }
    }

    #[test]
    fn playback_order_follows_event_time_ascending() {
        let events = vec![
            ev("evt_c", 300, 310),
            ev("evt_a", 100, 110),
            ev("evt_b", 200, 210),
        ];
        let sit = Situation {
            id: Id::new_unchecked("sit_x"),
            name: "x".into(),
            summary: None,
            // steps 刻意乱序——顺序必须由时间派生，而非列表位置。
            steps: vec![step("evt_c"), step("evt_a"), step("evt_b")],
            span: TimeSpan::new(100, 310).unwrap(),
            narrative_order: None,
            maps: vec![],
            protagonist: None,
        };
        let order: Vec<String> = sit
            .playback_order(&events)
            .iter()
            .map(|i| i.as_str().to_string())
            .collect();
        assert_eq!(order, vec!["evt_a", "evt_b", "evt_c"]);
    }

    #[test]
    fn same_time_falls_back_to_event_id_lexicographic_order() {
        let events = vec![ev("evt_b", 100, 110), ev("evt_a", 100, 110)];
        let sit = Situation {
            id: Id::new_unchecked("sit_x"),
            name: "x".into(),
            summary: None,
            steps: vec![step("evt_b"), step("evt_a")],
            span: TimeSpan::new(100, 110).unwrap(),
            narrative_order: None,
            maps: vec![],
            protagonist: None,
        };
        let order: Vec<String> = sit
            .playback_order(&events)
            .iter()
            .map(|i| i.as_str().to_string())
            .collect();
        assert_eq!(order, vec!["evt_a", "evt_b"], "同时间须有确定性兜底");
    }

    #[test]
    fn narrative_order_overrides_time_derived_sequence() {
        let events = vec![
            ev("evt_a", 100, 110),
            ev("evt_b", 200, 210),
            ev("evt_c", 300, 310),
        ];
        let sit = Situation {
            id: Id::new_unchecked("sit_x"),
            name: "x".into(),
            summary: None,
            steps: vec![step("evt_a"), step("evt_b"), step("evt_c")],
            span: TimeSpan::new(100, 310).unwrap(),
            // 倒叙：先讲 c，再讲 a；b 未被覆盖 → 追加其后。
            narrative_order: Some(vec![Id::new_unchecked("evt_c"), Id::new_unchecked("evt_a")]),
            maps: vec![],
            protagonist: None,
        };
        let order: Vec<String> = sit
            .playback_order(&events)
            .iter()
            .map(|i| i.as_str().to_string())
            .collect();
        assert_eq!(order, vec!["evt_c", "evt_a", "evt_b"]);
    }

    #[test]
    fn dangling_step_reference_is_skipped_without_panic() {
        let events = vec![ev("evt_a", 100, 110)];
        let sit = Situation {
            id: Id::new_unchecked("sit_x"),
            name: "x".into(),
            summary: None,
            steps: vec![step("evt_a"), step("evt_missing")],
            span: TimeSpan::new(100, 110).unwrap(),
            narrative_order: None,
            maps: vec![],
            protagonist: None,
        };
        let order = sit.playback_order(&events);
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].as_str(), "evt_a");
    }
}
