//! 时间维度：时间点、时间区间、时间轴。
//!
//! 核心约定（原则七）：历史状态以**时间区间**存储，
//! 任意时刻的视图是「对该时刻全部区间求值」的投影，而非直接读取的当前值。

use crate::{error::CoreError, id::Id};

/// 时间轴上的一个位置。
///
/// 精度策略（对应项目文档 §13「待决问题」）：本结构**至少**精确到「纪元+年+日」，
/// `hour` 与 `minute` 为可选细化字段。是否提升精度到小时/分钟是待裁问题，
/// 因此这里不强制——**存得下比要求高**，后续提升精度不需要破坏 schema。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct TimePoint {
    /// 纪元序号，从 1 开始。
    pub era: i32,
    /// 纪元内的年份。允许负数表示纪元前。
    pub year: i32,
    /// 年内的日。约定 1..=N，具体每年天数由 `Timeline` 定义。
    #[serde(default)]
    pub day: u32,
    /// 可选：小时。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    /// 可选：分钟。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u8>,
}

impl TimePoint {
    pub fn new(era: i32, year: i32, day: u32) -> Self {
        Self {
            era,
            year,
            day,
            hour: None,
            minute: None,
        }
    }
}

impl std::fmt::Display for TimePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "E{}.{}.{}", self.era, self.year, self.day)?;
        if let Some(h) = self.hour {
            write!(f, " {h:02}")?;
            if let Some(m) = self.minute {
                write!(f, ":{m:02}")?;
            }
        }
        Ok(())
    }
}

/// 时间区间。用于存储历史状态片段。
///
/// `to` 为 `None` 表示「至今有效」——这是开放区间，
/// 使作者无需为仍在延续的状态预设结束时间。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimeSpan {
    pub from: TimePoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<TimePoint>,
}

impl TimeSpan {
    /// 构造一个闭合区间，并校验顺序。
    pub fn new(from: TimePoint, to: TimePoint) -> Result<Self, CoreError> {
        if to < from {
            return Err(CoreError::InvalidSpan {
                from: from.to_string(),
                to: to.to_string(),
            });
        }
        Ok(Self { from, to: Some(to) })
    }

    /// 构造一个开放区间（从 `from` 起至今）。
    pub fn open(from: TimePoint) -> Self {
        Self { from, to: None }
    }

    /// 判断时刻 `t` 是否落在本区间内（闭区间语义）。
    pub fn contains(&self, t: &TimePoint) -> bool {
        if t < &self.from {
            return false;
        }
        match &self.to {
            Some(end) => t <= end,
            None => true,
        }
    }

    /// 与另一区间是否有重叠。用于校验同一实体同一维度的状态片段不冲突。
    pub fn overlaps(&self, other: &TimeSpan) -> bool {
        let self_start_ok = match &other.to {
            Some(o_end) => &self.from <= o_end,
            None => true,
        };
        let other_start_ok = match &self.to {
            Some(s_end) => &other.from <= s_end,
            None => true,
        };
        self_start_ok && other_start_ok
    }
}

/// 时间轴定义。对应 `timeline/axis.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Timeline {
    /// 纪元列表，按 `order` 排序。
    pub eras: Vec<Era>,
    /// 每年的天数。默认 360（便于设定文学中常见的整除历法）。
    #[serde(default = "default_days_per_year")]
    pub days_per_year: u32,
    /// 每年是否按 12 月 × 30 日划分。细化历法规则属待裁问题。
    #[serde(default)]
    pub months: Vec<MonthDef>,
    /// 时间轴锚点：作者关心的关键时间点，用于 UI 快速跳转。
    #[serde(default)]
    pub anchors: Vec<Anchor>,
}

fn default_days_per_year() -> u32 {
    360
}

/// 纪元定义。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Era {
    pub id: Id,
    pub name: String,
    /// 纪元排序权重，数值越大越晚。
    pub order: i32,
    /// 该纪元的起始时间点（可为空表示无明确起点）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<TimePoint>,
}

/// 月份/分段定义，可选。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MonthDef {
    pub name: String,
    pub days: u32,
}

/// 时间轴锚点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    pub id: Id,
    pub name: String,
    pub at: TimePoint,
}

/// 时间轴求值入口。原则七的落地点。
///
/// 任何「某时刻的世界状态」都必须经过这里，禁止在渲染层自行比较时间点。
pub struct TimelineQuery;

impl TimelineQuery {
    /// 对一组带区间的状态片段求 `t` 时刻的投影。
    ///
    /// 返回所有覆盖 `t` 的片段的引用。调用方负责按维度语义决定
    /// 「多片段同时命中」时如何处理（通常应视为数据冲突并报告）。
    pub fn at<'a, T>(spans: &'a [(TimeSpan, T)], t: &TimePoint) -> Vec<&'a (TimeSpan, T)> {
        spans.iter().filter(|(span, _)| span.contains(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timepoint_ordering_follows_era_year_day() {
        let a = TimePoint::new(1, 300, 1);
        let b = TimePoint::new(1, 301, 1);
        let c = TimePoint::new(2, 1, 1);
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn closed_span_rejects_reversed_bounds() {
        let from = TimePoint::new(1, 300, 1);
        let to = TimePoint::new(1, 280, 1);
        assert!(TimeSpan::new(from, to).is_err());
    }

    #[test]
    fn open_span_contains_all_later_points() {
        let span = TimeSpan::open(TimePoint::new(1, 300, 1));
        assert!(span.contains(&TimePoint::new(1, 300, 1)));
        assert!(span.contains(&TimePoint::new(9, 1, 1)));
        assert!(!span.contains(&TimePoint::new(1, 299, 1)));
    }

    #[test]
    fn overlaps_detects_intersection() {
        let a = TimeSpan::new(TimePoint::new(1, 300, 1), TimePoint::new(1, 450, 1)).unwrap();
        let b = TimeSpan::open(TimePoint::new(1, 400, 1));
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));

        let c = TimeSpan::new(TimePoint::new(1, 100, 1), TimePoint::new(1, 200, 1)).unwrap();
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn query_projects_only_covering_spans() {
        let spans = vec![
            (
                TimeSpan::new(TimePoint::new(1, 300, 1), TimePoint::new(1, 450, 1)).unwrap(),
                "kingdom_a",
            ),
            (
                TimeSpan::new(TimePoint::new(1, 500, 1), TimePoint::new(1, 600, 1)).unwrap(),
                "kingdom_b",
            ),
        ];
        let hit = TimelineQuery::at(&spans, &TimePoint::new(1, 400, 1));
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].1, &"kingdom_a".to_string());

        let miss = TimelineQuery::at(&spans, &TimePoint::new(1, 480, 1));
        assert!(miss.is_empty());
    }
}
