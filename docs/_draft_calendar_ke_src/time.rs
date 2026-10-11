//! 时间主表示：天数计数器 newtype。历法换算下沉到 `Calendar`。

use crate::calendar::{Calendar, CivilDate};
use crate::error::CoreError;

/// 时间轴上的一个位置。主表示 = 天数计数器（`counter >= 1`）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct TimePoint(i64);

impl TimePoint {
    pub const FIRST: TimePoint = TimePoint(1);

    pub fn from_counter(counter: i64) -> Result<Self, CoreError> {
        if counter < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter });
        }
        Ok(TimePoint(counter))
    }

    pub const fn from_counter_unchecked(counter: i64) -> Self {
        TimePoint(counter)
    }

    pub const fn counter(self) -> i64 {
        self.0
    }

    pub fn advance(self, days: i64) -> Result<Self, CoreError> {
        Self::from_counter(self.0 + days)
    }

    pub fn date(self, calendar: &Calendar) -> CivilDate {
        calendar.civil(self.0)
    }
}

impl std::fmt::Display for TimePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@day{}", self.0)
    }
}

/// 半开区间 `[from, to)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimeSpan {
    pub from: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<i64>,
}

impl TimeSpan {
    pub fn new(from: i64, to: i64) -> Result<Self, CoreError> {
        if from < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter: from });
        }
        if to < from {
            return Err(CoreError::InvalidSpan {
                from: from.to_string(),
                to: to.to_string(),
            });
        }
        Ok(Self { from, to: Some(to) })
    }

    pub fn open(from: i64) -> Result<Self, CoreError> {
        if from < 1 {
            return Err(CoreError::DayCounterOutOfRange { counter: from });
        }
        Ok(Self { from, to: None })
    }

    pub fn contains(&self, t: i64) -> bool {
        if t < self.from {
            return false;
        }
        match self.to {
            Some(end) => t < end,
            None => true,
        }
    }

    pub fn overlaps(&self, other: &TimeSpan) -> bool {
        let self_end_before_other = self.to.is_some_and(|b| b <= other.from);
        let other_end_before_self = other.to.is_some_and(|d| d <= self.from);
        !(self_end_before_other || other_end_before_self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_is_day_counter_ordering() {
        let a = TimePoint::from_counter(1).unwrap();
        let b = TimePoint::from_counter(2).unwrap();
        assert!(a < b);
    }

    #[test]
    fn counter_below_one_is_rejected() {
        assert!(matches!(
            TimePoint::from_counter(0),
            Err(CoreError::DayCounterOutOfRange { .. })
        ));
        assert!(TimePoint::from_counter(1).unwrap().advance(-1).is_err());
    }

    #[test]
    fn half_open_span_excludes_its_end() {
        let s = TimeSpan::new(100_801, 100_810).unwrap();
        assert!(s.contains(100_801));
        assert!(s.contains(100_809));
        assert!(!s.contains(100_810));
    }

    #[test]
    fn adjacent_spans_do_not_overlap() {
        let a = TimeSpan::new(1, 10).unwrap();
        let b = TimeSpan::new(10, 20).unwrap();
        assert!(!a.overlaps(&b));
        assert!(!b.overlaps(&a));
    }
}
