//! 「故事」实体：**无日期**的叙事单元。

use crate::error::CoreError;
use crate::time::TimeSpan;

/// 故事。**无 day 字段**是规范要求，不是省事。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Story {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 可选：它想靠近的锚点（**仅用于显示定位，不赋予任何时间语义**）。
    ///
    /// 这只是"指向已有锚点的弱引用"——**不进 `TimePoint`/`TimeSpan`/`TimelineQuery`**，
    /// 不参与排序、不参与校验。加此字段**不破坏"故事无 day"的硬约束**。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<Id>,
}

/// 时间的**校验分流**枚举：有日期走 `Dated`，无日期走 `Undated`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TimelineItem {
    /// 可排期（事件）：必须 `from > day_counter(start_year,1,1)`。
    Dated {
        #[serde(flatten)]
        span: TimeSpan,
    },
    /// 不可排期（故事）：无日期，恒通过时间校验。
    Undated { story: Story },
}

/// 事件日期硬下界校验：`from` 必须 **严格大于** 起点 0 时。
pub fn check_event_lower_bound(from: i64, start_day: i64) -> Result<(), CoreError> {
    if from <= start_day {
        return Err(CoreError::EventAtOrBeforeStart {
            day: from,
            start_day,
        });
    }
    Ok(())
}

/// 统一校验入口：按 `TimelineItem` 分流。
pub fn validate_timeline_item(item: &TimelineItem, start_day: i64) -> Result<(), CoreError> {
    match item {
        TimelineItem::Dated { span } => check_event_lower_bound(span.from, start_day),
        TimelineItem::Undated { .. } => Ok(()),
    }
}

/// ID 最小化实现（独立验证 crate 用）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    pub fn new_unchecked(s: impl Into<String>) -> Self {
        Id(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
