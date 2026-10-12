//! 时间显示的唯一真源（设计规格 §5.3）。
#![cfg(feature = "app")]

//! ## 纪律
//!
//! **任何组件不得自行格式化日期**（§5.3）：顶栏日期、时间轴日期、地图上的
//! 事件高亮，全部从 [`PlaybackState::current_day`] 派生，经本模块的
//! [`format_date`] 一处换算。三处日期曾不同步是原设计的隐患（§P4）。
//!
//! ## 为什么需要这个薄层
//!
//! `wv_core::time::Calendar::format` 是权威换算，但它挂在 `World.timeline` 上，
//! 而 UI 组件拿不到 `World` 时就会各自 fallback 成 `@dayN`——那就出现了
//! 「顶栏显示 `@day1`、时间轴显示 `无仙 1 年 1 月 1 日`」的错位。本模块把
//! 「历法在哪、没有历法怎么办」收敛成一个入口。

use crate::state::{PlaybackState, WorldProjection};

/// 把天数计数器格式化为作者可读的日期。
///
/// - 有历法：`无仙 3 年 5 月 12 日`（经 `Calendar::format`）。
/// - 无历法：`第 N 天`（基线 §1.12：不显示年月日）。
///
/// **不接受** `@dayN` 这种内部记号——那是调试写法，不该出现在界面上。
pub fn format_date_with(calendar: Option<&wv_core::Calendar>, day: i64) -> String {
    match calendar {
        Some(c) => c.format(day),
        None => format!("第 {day} 天"),
    }
}

/// 从世界投影取历法，格式化当前时刻。
pub fn format_date_for(projection: &WorldProjection, day: i64) -> String {
    let cal = projection.world.timeline.as_ref().map(|t| &t.calendar);
    format_date_with(cal, day)
}

/// 便捷入口：从 [`PlaybackState`] 取当前时刻（供无 `World` 引用的组件用）。
///
/// 注意这**不是**权威换算——它只在拿不到 `World` 时兜底成「第 N 天」。
/// 真正要显示作者日期的地方应走 [`format_date_for`]。
pub fn format_date(day: i64) -> String {
    format_date_with(None, day)
}

/// 读出当前时刻并格式化（顶栏/时间轴共用，保证一致）。
pub fn current_date(projection: &WorldProjection, playback: &PlaybackState) -> String {
    format_date_for(projection, playback.current_day)
}

/// 世界的历法（若无则 None）。
pub fn calendar_of(projection: &WorldProjection) -> Option<&wv_core::Calendar> {
    projection.world.timeline.as_ref().map(|t| &t.calendar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_without_calendar_reads_as_day_counter() {
        let s = format_date_with(None, 100805);
        assert_eq!(s, "第 100805 天", "got {s}");
    }

    #[test]
    fn day_with_calendar_uses_calendar_format() {
        let c = wv_core::Calendar {
            kind: wv_core::time::CalendarKind::Simple,
            prefix: Some("无仙".into()),
            start_year: 1,
            days_per_year: 360,
            days_per_month: 30,
        };
        // 天数计数器是 **1-based**：day 1 = 第 1 年第 1 月第 1 日。
        // 所以第 3 年第 1 天 = (3−1)×360 + 1 = 721，不是 1081
        // （1081 是第 4 年第 1 天——旧断言在此差了一年，代码没错）。
        let s = format_date_with(Some(&c), 721);
        assert_eq!(s, "无仙 3 年 1 月 1 日", "got {s}");
        assert_eq!(format_date_with(Some(&c), 1081), "无仙 4 年 1 月 1 日");
        // 跨月边界：第 1 年第 2 月第 1 天 = 30 + 1。
        assert_eq!(format_date_with(Some(&c), 31), "无仙 1 年 2 月 1 日");
        // 与 `to_day` 互为逆运算（手册 §7.2：用算出来的期望值断言）。
        assert_eq!(c.to_day(3, 1, 1), Some(721));
    }

    #[test]
    fn never_emits_internal_at_day_notation() {
        let s = format_date(1);
        assert!(!s.contains('@'), "got {s}");
    }
}
