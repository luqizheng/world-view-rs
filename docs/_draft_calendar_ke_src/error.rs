//! 数据层错误类型（本轮增量）。

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("天数计数器 {counter} 非法：时间轴只有起点、往前不存在，counter 必须 >= 1")]
    DayCounterOutOfRange { counter: i64 },

    #[error("年份 {year} 早于时间轴起点 {start_year}：起点之前不存在")]
    BeforeTimelineStart { year: i64, start_year: i32 },

    #[error("月份 {month} 非法：取值 1..={max}")]
    InvalidMonth { month: i64, max: i64 },

    #[error("日期 {day} 非法：取值 1..={max}")]
    InvalidDay { day: i64, max: i64 },

    #[error("事件日期 {day} 未晚于时间轴起点 0 时（必须 > {start_day}）")]
    EventAtOrBeforeStart { day: i64, start_day: i64 },

    #[error("时间轴跨度 {days} 天（约 {years} 年）超过建议上限 {limit} 天：渲染/导航将不可用")]
    TimelineSpanTooLarge { days: i64, years: i64, limit: i64 },

    #[error("时间区间非法: from={from} to={to}")]
    InvalidSpan { from: String, to: String },
}
