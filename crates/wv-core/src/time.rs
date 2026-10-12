//! 时间维度：时间点、时间区间、时间轴。
//!
//! ## 时间模型的骨架（基线 §2.16 后的定稿）
//!
//! 本模块只认**一个**时间度量：**从工程历法定点起算的天数计数器**（`i64`，1-based）。
//!
//! - **`TimePoint` 是纯计数器**，不带纪年 / 年 / 月的结构语义。纪年换算（`无仙 300 年
//!   1 月 5 日`）是**显示层派生**的事，由 [`Calendar`] 的 `prefix` + `start_year` +
//!   `days_per_year` + `days_per_month` 完成，**不进数据模型**。
//! - 理由：任何「年月日」的字段式表达都会引入「两个真相」——计数器与年月日字段可能互相
//!   矛盾。让数据只有计数器一个真相，显示换算纯函数化，是唯一不会分歧的形态。
//! - **半开区间**：`TimeSpan` 语义为 `[from, to)`。`to == None` 表示「至今有效」。
//!   半开区间使「上一年 12 月 31 日结束」与「下一年 1 月 1 日开始」不重叠也不留空，
//!   这与 P6/P7 的「区间化历史」直接相关。
//!
//! 核心约定（原则七）：历史状态以**时间区间**存储，
//! 任意时刻的视图是「对该时刻全部区间求值」的投影，而非直接读取的当前值。

use crate::{error::CoreError, id::Id};

/// 时间轴上的一个位置。
///
/// **元组式单字段结构体**（基线 §2.16 第 1 条）：唯一的真相是一个 `i64` 天数计数器。
/// 计数器约定 **1-based**：`counter >= 1`，`TimePoint(1)` 是时间轴的第一天。
///
/// 精度策略（对应项目文档 §13「待决问题」）：本项目的时间精度**锁定到天**。
/// 小时 / 分钟的细化不是待裁问题，而是**明确不做**——编辑器的用法是「排期与播放」，
/// 天是最小有意义单位；引入小时只会让每一位作者都要在无意义精度上做选择。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct TimePoint(pub i64);

impl TimePoint {
    /// 直接以天数计数器构造。
    ///
    /// 这是**唯一**的构造入口：不存在 `new(era, year, day)` 这类换算构造器，
    /// 因为纪年换算是显示层的事（见模块文档）。需要从年月日得到计数器时，
    /// 用 [`Calendar::to_day`]。
    pub const fn from_day(day: i64) -> Self {
        Self(day)
    }

    /// 天数计数器的原始值。
    pub const fn day(self) -> i64 {
        self.0
    }

    /// 是否是一个合法的时间点（计数器 `>= 1`）。
    ///
    /// 0 与负数不是合法时刻——时间轴从第 1 天开始，没有「第 0 天」。
    pub const fn is_valid(self) -> bool {
        self.0 >= 1
    }
}

impl std::fmt::Display for TimePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 基线 §2.16 第 3 条：Display 改为 `@day{n}`。
        // 不带年月日——年月日是日历派生的，`TimePoint` 自己不知道历法。
        write!(f, "@day{}", self.0)
    }
}

/// 时间区间。用于存储历史状态片段。
///
/// **半开语义 `[from, to)`**（基线 §2.16 第 5–8 条）：
/// - `contains(t)` 当且仅当 `from <= t < to`；
/// - `to == None` 表示「至今有效」（开放区间）；
/// - 零长度区间（`from == to`）**合法但不含任何时刻**——这是半开区间的自然推论，
///   不是特例。它使「两个相邻区间」可以精确拼接而不重叠。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimeSpan {
    /// 起始时间点（含）。
    pub from: i64,
    /// 结束时间点（**不含**）。`None` 表示至今有效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<i64>,
}

impl TimeSpan {
    /// 构造一个半开区间 `[from, to)`，并校验顺序。
    ///
    /// `to < from` 是错误（区间倒置）；`to == from` 是**合法的空区间**。
    pub fn new(from: i64, to: i64) -> Result<Self, CoreError> {
        if to < from {
            return Err(CoreError::InvalidSpan {
                from: TimePoint::from_day(from).to_string(),
                to: TimePoint::from_day(to).to_string(),
            });
        }
        Ok(Self { from, to: Some(to) })
    }

    /// 构造一个开放区间（从 `from` 起至今）。
    pub const fn open(from: i64) -> Self {
        Self { from, to: None }
    }

    /// 判断时刻 `t` 是否落在本区间内（**半开**语义 `[from, to)`）。
    pub fn contains(&self, t: &TimePoint) -> bool {
        let t = t.0;
        if t < self.from {
            return false;
        }
        match self.to {
            Some(end) => t < end,
            None => true,
        }
    }

    /// 与另一区间是否有重叠。用于校验同一实体同一维度的状态片段不冲突。
    ///
    /// 半开区间重叠判定：`self.from < other.to && other.from < self.to`。
    /// 零长度区间与任何区间都不重叠（`[a, a)` 是空集）。
    pub fn overlaps(&self, other: &TimeSpan) -> bool {
        let self_before_other_end = match other.to {
            Some(o_end) => self.from < o_end,
            None => true,
        };
        let other_before_self_end = match self.to {
            Some(s_end) => other.from < s_end,
            None => true,
        };
        self_before_other_end && other_before_self_end
    }

    /// 区间是否为空（零长度）。空区间不含任何时刻。
    pub fn is_empty(&self) -> bool {
        matches!(self.to, Some(end) if end <= self.from)
    }
}

/// 历法种类（ADR-0006）。缺省 `simple`，旧 YAML 完全兼容。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarKind {
    /// 整除历法：`days_per_year` × `days_per_month`（设定文学常用）。
    #[default]
    Simple,
    /// 真公历（proleptic Gregorian）：真实月长 + 闰年，第 1 天 = `start_year` 年 1 月 1 日。
    Gregorian,
}

/// 历法定义。`Timeline` 的一部分，**只负责显示换算，不参与求值**。
///
/// 关键裁定（基线 §2.16）：**历法是显示层派生**。`TimePoint` 不知道历法，
/// 需要显示「无仙 300 年 1 月 5 日」时，用本结构把计数器换成年月日。
///
/// 双历制（ADR-0006）：`kind` 缺省 [`CalendarKind::Simple`]，既有工程零改动；
/// [`CalendarKind::Gregorian`] 用 proleptic 公历换算真实历史日期（月长表 + 闰年），
/// 天数计数器仍是唯一真相（P7 不动）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Calendar {
    /// 历法种类。缺省 `simple`。
    #[serde(default)]
    pub kind: CalendarKind,
    /// 纪年前缀，如 `无仙`。为空表示不显示年月日，只显示 `第 N 天`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// 第 1 天对应的年份。默认 1。
    #[serde(default = "default_start_year")]
    pub start_year: i32,
    /// 每年的天数（仅 `simple`）。默认 360（便于设定文学中常见的整除历法）。
    #[serde(default = "default_days_per_year")]
    pub days_per_year: i64,
    /// 每月的天数（仅 `simple`）。默认 30。
    #[serde(default = "default_days_per_month")]
    pub days_per_month: i64,
}

fn default_start_year() -> i32 {
    1
}

fn default_days_per_year() -> i64 {
    360
}

fn default_days_per_month() -> i64 {
    30
}

impl Default for Calendar {
    fn default() -> Self {
        Self {
            kind: CalendarKind::Simple,
            prefix: None,
            start_year: 1,
            days_per_year: 360,
            days_per_month: 30,
        }
    }
}

/// 公历月长表（平年）。索引 0 = 1 月。
const GREGORIAN_MONTH_LENGTHS: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// proleptic 公历闰年判定。
fn is_gregorian_leap_year(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Howard Hinnant `days_from_civil`：`(y, m, d)` → 自 1970-01-01 起的天数（可为负）。
/// 纯整数算法，无外部依赖（ADR-0006）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // 3 月=0 … 2 月=11
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant `civil_from_days`：自 1970-01-01 起的天数 → `(y, m, d)`。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Calendar {
    /// Gregorian 历第 1 天对应的 1970 纪元天数（即 `start_year` 年 1 月 1 日）。
    fn gregorian_epoch(&self) -> i64 {
        days_from_civil(self.start_year as i64, 1, 1)
    }

    /// 把天数计数器换算成 `(年, 月, 日)`。
    ///
    /// - `simple`（严格按 `days_per_year` / `days_per_month` 整除）：
    ///   - `n = day - 1`（转 0-based）
    ///   - `year = start_year + n / days_per_year`
    ///   - `month = (n % days_per_year) / days_per_month + 1`
    ///   - `day_of_month = (n % days_per_month) + 1`
    /// - `gregorian`：proleptic 公历，含闰年（ADR-0006）。
    pub fn to_ymd(&self, day: i64) -> (i32, i64, i64) {
        match self.kind {
            CalendarKind::Simple => {
                let n = (day - 1).max(0);
                let year = self.start_year as i64 + n / self.days_per_year;
                let month = (n % self.days_per_year) / self.days_per_month + 1;
                let day_of_month = (n % self.days_per_month) + 1;
                (year as i32, month, day_of_month)
            }
            CalendarKind::Gregorian => {
                let (y, m, d) = civil_from_days(self.gregorian_epoch() + day - 1);
                (y as i32, m, d)
            }
        }
    }

    /// 把 `(年, 月, 日)` 换算回天数计数器。`to_ymd` 的逆运算。
    ///
    /// 非法输入（月不在 1..=12、日超出当月长度——含闰年 2 月）返回 `None`。
    pub fn to_day(&self, year: i32, month: i64, day_of_month: i64) -> Option<i64> {
        match self.kind {
            CalendarKind::Simple => {
                let years = year as i64 - self.start_year as i64;
                Some(
                    years * self.days_per_year
                        + (month - 1) * self.days_per_month
                        + (day_of_month - 1)
                        + 1,
                )
            }
            CalendarKind::Gregorian => {
                if !(1..=12).contains(&month) || day_of_month < 1 {
                    return None;
                }
                let y = year as i64;
                let len = if month == 2 && is_gregorian_leap_year(y) {
                    29
                } else {
                    GREGORIAN_MONTH_LENGTHS[(month - 1) as usize]
                };
                if day_of_month > len {
                    return None;
                }
                Some(days_from_civil(y, month, day_of_month) - self.gregorian_epoch() + 1)
            }
        }
    }

    /// 格式化一个时刻供显示。
    ///
    /// - 有 `prefix`：`无仙 300 年 1 月 5 日` / `公元 1911 年 10 月 10 日`
    /// - 无 `prefix`：`第 100805 天`（**不显示年月日**，基线 §1.12）
    pub fn format(&self, day: i64) -> String {
        match &self.prefix {
            Some(p) if !p.is_empty() => {
                let (y, m, d) = self.to_ymd(day);
                format!("{p} {y} 年 {m} 月 {d} 日")
            }
            _ => format!("第 {day} 天"),
        }
    }
}

/// 时间轴定义。对应 `timeline/axis.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Timeline {
    /// 历法（显示换算用）。缺省时用 360 天/年、30 天/月、无前缀。
    #[serde(default)]
    pub calendar: Calendar,
    /// 时间轴覆盖范围。
    pub timeline_range: TimeSpan,
    /// 时间轴锚点：作者关心的关键时间点，用于 UI 快速跳转。
    #[serde(default)]
    pub anchors: Vec<Anchor>,
}

/// 时间轴锚点。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    pub id: Id,
    pub name: String,
    /// 锚点所在的天数计数器（基线 §2.16 第 12 条）。
    pub day: i64,
}

/// 时间轴求值入口。原则七的落地点。
///
/// 任何「某时刻的世界状态」都必须经过这里，禁止在渲染层自行比较时间点
/// （`AGENTS.md` §3.2 硬约束）。
pub struct TimelineQuery;

impl TimelineQuery {
    /// 对一组带区间的状态片段求 `t` 时刻的投影。
    ///
    /// 返回所有覆盖 `t` 的片段的引用。调用方负责按维度语义决定
    /// 「多片段同时命中」时如何处理（通常应视为数据冲突并报告）。
    ///
    /// 参数 `t` 直接是**天数计数器**（基线 §2.16 第 13 条）：
    /// 渲染层手里的 `current_day` 无需包装即可求值。
    pub fn at<T>(spans: &[(TimeSpan, T)], t: i64) -> Vec<&(TimeSpan, T)> {
        let t = TimePoint::from_day(t);
        spans.iter().filter(|(span, _)| span.contains(&t)).collect()
    }

    /// day 空间插值（`ANIMATION_SPEC` §5.1）：过渡进度是 `current_day` 的**纯函数**。
    ///
    /// 返回 `clamp((current_day - boundary_day) / delta_day, 0.0, 1.0)`：
    /// - `current_day < boundary_day` → `0.0`（尚未跨过边界）；
    /// - `current_day >= boundary_day + delta_day` → `1.0`（过渡已完成）；
    /// - `delta_day <= 0.0` → `0.0`（零长度段视为「未开始」，同时避免除零 panic）。
    ///
    /// 偏移量用 [`i64::saturating_sub`] 计算：极端输入（如 `current_day` 接近
    /// `i64::MAX` 而 `boundary_day` 为负）下裸减法会 panic，饱和减法则把结果
    /// 钳到 `i64::MAX` / `i64::MIN`，经下面的 `clamp(0,1)` 收敛为 `1.0` / `0.0`，
    /// 语义正确。正常输入下 `saturating_sub` 与裸减法结果完全相同。
    ///
    /// 本函数**只做线性归一化**，不含 easing 曲线——`ease-out-cubic` 等是调用方
    /// 在 day 参数上的可选加工（`ANIMATION_SPEC` §5.1 / §5.3），不进数据层。
    pub fn day_progress(current_day: i64, boundary_day: i64, delta_day: f32) -> f32 {
        if delta_day <= 0.0 {
            return 0.0;
        }
        let offset = current_day.saturating_sub(boundary_day);
        ((offset as f32) / delta_day).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timepoint_orders_by_day_counter() {
        let a = TimePoint::from_day(1);
        let b = TimePoint::from_day(2);
        let c = TimePoint::from_day(1000);
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn timepoint_displays_as_day_counter() {
        assert_eq!(TimePoint::from_day(100805).to_string(), "@day100805");
    }

    #[test]
    fn reversed_span_bounds_are_rejected() {
        assert!(TimeSpan::new(300, 280).is_err());
    }

    #[test]
    fn zero_length_span_is_legal_but_contains_nothing() {
        // 半开区间 `[n, n)` 是合法空区间——这是半开语义的自然推论。
        let span = TimeSpan::new(300, 300).unwrap();
        assert!(span.is_empty());
        assert!(!span.contains(&TimePoint::from_day(300)));
        assert!(!span.contains(&TimePoint::from_day(299)));
    }

    #[test]
    fn half_open_span_excludes_end_point() {
        let span = TimeSpan::new(300, 400).unwrap();
        assert!(span.contains(&TimePoint::from_day(300)), "起点应含");
        assert!(span.contains(&TimePoint::from_day(399)));
        assert!(!span.contains(&TimePoint::from_day(400)), "终点应不含");
        assert!(!span.contains(&TimePoint::from_day(299)));
    }

    #[test]
    fn adjacent_spans_do_not_overlap_in_half_open_semantics() {
        // `[100, 200)` 与 `[200, 300)` 相邻但不重叠——半开区间的核心收益。
        let a = TimeSpan::new(100, 200).unwrap();
        let b = TimeSpan::new(200, 300).unwrap();
        assert!(!a.overlaps(&b));
        assert!(!b.overlaps(&a));
    }

    #[test]
    fn open_span_contains_all_later_points() {
        let span = TimeSpan::open(300);
        assert!(span.contains(&TimePoint::from_day(300)));
        assert!(span.contains(&TimePoint::from_day(9000)));
        assert!(!span.contains(&TimePoint::from_day(299)));
    }

    #[test]
    fn overlaps_detects_intersection() {
        let a = TimeSpan::new(300, 450).unwrap();
        let b = TimeSpan::open(400);
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));

        let c = TimeSpan::new(100, 200).unwrap();
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn query_projects_only_covering_spans() {
        let spans = vec![
            (TimeSpan::new(300, 450).unwrap(), "kingdom_a"),
            (TimeSpan::new(500, 600).unwrap(), "kingdom_b"),
        ];
        let hit = TimelineQuery::at(&spans, 400);
        assert_eq!(hit.len(), 1, "got {}", hit.len());
        assert_eq!(hit[0].1, "kingdom_a");

        // 边界：450 是 a 的终点，半开语义下不含。
        let boundary = TimelineQuery::at(&spans, 450);
        assert!(boundary.is_empty(), "半开区间终点不应命中");

        let miss = TimelineQuery::at(&spans, 480);
        assert!(miss.is_empty());
    }

    #[test]
    fn day_progress_interpolates_linearly_inside_band() {
        // 段内中点：boundary=100, delta=10, current=105 → 0.5
        let p = TimelineQuery::day_progress(105, 100, 10.0);
        assert!((p - 0.5).abs() < 1e-6, "got {}", p);
    }

    #[test]
    fn day_progress_clamps_before_boundary_to_zero() {
        let p = TimelineQuery::day_progress(50, 100, 10.0);
        assert_eq!(p, 0.0, "未跨过边界应返回 0.0");
    }

    #[test]
    fn day_progress_clamps_after_band_to_one() {
        let p = TimelineQuery::day_progress(9999, 100, 10.0);
        assert_eq!(p, 1.0, "越过终点应返回 1.0");
    }

    #[test]
    fn day_progress_zero_length_band_returns_zero_without_panic() {
        // delta_day == 0.0：零长度段视为未开始，且不得除零 panic。
        let p = TimelineQuery::day_progress(100, 100, 0.0);
        assert_eq!(p, 0.0, "零长度段应返回 0.0");
        // 负 delta 同样安全。
        let q = TimelineQuery::day_progress(100, 100, -1.0);
        assert_eq!(q, 0.0, "负 delta 应返回 0.0");
    }

    #[test]
    fn day_progress_at_exact_boundaries() {
        // 恰好等于起始边界 → 0.0
        let start = TimelineQuery::day_progress(100, 100, 10.0);
        assert_eq!(start, 0.0, "起点应返回 0.0");
        // 恰好等于终点边界 → 1.0
        let end = TimelineQuery::day_progress(110, 100, 10.0);
        assert_eq!(end, 1.0, "终点应返回 1.0");
    }

    #[test]
    fn day_progress_does_not_panic_on_extreme_day_values() {
        // 用 black_box 从常量派生运行期值，避免编译器常量折叠掉溢出。
        // 正向饱和：current 极大、boundary 极负 → 偏移饱和到 i64::MAX → 1.0。
        let hi = std::hint::black_box(i64::MAX - 1);
        let lo = std::hint::black_box(i64::MIN + 1);
        let up = TimelineQuery::day_progress(hi, lo, 10.0);
        assert_eq!(up, 1.0, "正向溢出应饱和为 1.0");

        // 负向饱和：current 极负、boundary 极大 → 偏移饱和到 i64::MIN → 0.0。
        let down = TimelineQuery::day_progress(lo, hi, 10.0);
        assert_eq!(down, 0.0, "负向溢出应饱和为 0.0");
    }

    #[test]
    fn calendar_maps_day_counter_to_year_month_day() {
        let cal = Calendar {
            kind: CalendarKind::Simple,
            prefix: Some("无仙".into()),
            start_year: 1,
            days_per_year: 360,
            days_per_month: 30,
        };
        // 第 1 天 = 1 年 1 月 1 日
        assert_eq!(cal.to_ymd(1), (1, 1, 1));
        // 第 360 天 = 1 年 12 月 30 日
        assert_eq!(cal.to_ymd(360), (1, 12, 30));
        // 第 361 天 = 2 年 1 月 1 日
        assert_eq!(cal.to_ymd(361), (2, 1, 1));
        // 第 100805 天 → (100805-1)/360 = 279 余 364 → 年 280、月 13? 校验整除关系
        // n = 100804; 100804 / 360 = 280 余 4 → 年 281, 月 1, 日 5
        assert_eq!(cal.to_ymd(100805), (281, 1, 5));
    }

    #[test]
    fn calendar_round_trips_year_month_day() {
        let cal = Calendar::default();
        for day in [1i64, 30, 360, 361, 100805] {
            let (y, m, d) = cal.to_ymd(day);
            assert_eq!(cal.to_day(y, m, d), Some(day), "day {day} 往返不一致");
        }
    }

    #[test]
    fn calendar_formats_with_and_without_prefix() {
        let mut cal = Calendar::default();
        assert_eq!(cal.format(100805), "第 100805 天");

        cal.prefix = Some("无仙".into());
        assert_eq!(cal.format(1), "无仙 1 年 1 月 1 日");
    }

    // ---- Gregorian（ADR-0006，2026-01 用户裁定「公历，这不是幻想世界」）----

    /// 公历历法：第 1 天 = 1911-01-01（辛亥样例同款）。
    fn gregorian_1911() -> Calendar {
        Calendar {
            kind: CalendarKind::Gregorian,
            prefix: Some("公元".into()),
            start_year: 1911,
            days_per_year: 365,
            days_per_month: 30,
        }
    }

    #[test]
    fn gregorian_epoch_maps_day_one_to_january_first() {
        assert_eq!(
            gregorian_1911().to_ymd(1),
            (1911, 1, 1),
            "第 1 天必须是 start_year 元旦"
        );
    }

    #[test]
    fn gregorian_resolves_real_historical_dates() {
        let cal = gregorian_1911();
        // 手算锚点：1911-05-09 = 31+28+31+30 + 9 = 129（1911 平年）。
        assert_eq!(cal.to_day(1911, 5, 9), Some(129), "铁路国有令");
        // 1911-10-10 = 31+28+31+30+31+30+31+31+30 + 10 = 283。
        assert_eq!(cal.to_day(1911, 10, 10), Some(283), "武昌起义");
        assert_eq!(cal.to_ymd(283), (1911, 10, 10));
    }

    #[test]
    fn gregorian_handles_leap_day_of_1912() {
        let cal = gregorian_1911();
        // 1912 是闰年（%4==0 且 %100!=0）：2 月有 29 天。
        assert_eq!(
            cal.to_day(1912, 2, 29),
            Some(425),
            "手算：1911 平年 365 天，1912 年内 31+29=60 → 365+60=425"
        );
        assert_eq!(cal.to_ymd(425), (1912, 2, 29));
        // 1911 平年没有 2 月 29 日。
        assert_eq!(cal.to_day(1911, 2, 29), None, "平年 2/29 必须被拒");
    }

    #[test]
    fn gregorian_century_leap_rule_rejects_1900_but_accepts_2000() {
        assert!(
            !is_gregorian_leap_year(1900),
            "世纪年 %100==0 但 %400!=0 → 平年"
        );
        assert!(is_gregorian_leap_year(2000), "%400==0 → 闰年");
        assert!(is_gregorian_leap_year(1912), "普通 %4==0 → 闰年");
        assert!(!is_gregorian_leap_year(1911), "普通 %4!=0 → 平年");
    }

    #[test]
    fn gregorian_year_boundary_wraps_december_thirty_first() {
        let cal = gregorian_1911();
        assert_eq!(cal.to_ymd(365), (1911, 12, 31), "平年最后一天");
        assert_eq!(cal.to_ymd(366), (1912, 1, 1), "次日跨年");
    }

    #[test]
    fn gregorian_rejects_month_out_of_range_and_day_overflow() {
        let cal = gregorian_1911();
        assert_eq!(cal.to_day(1911, 0, 1), None, "月 0 非法");
        assert_eq!(cal.to_day(1911, 13, 1), None, "月 13 非法");
        assert_eq!(cal.to_day(1911, 4, 31), None, "4 月只有 30 天");
        assert_eq!(cal.to_day(1911, 1, 0), None, "日 0 非法");
    }

    #[test]
    fn gregorian_round_trips_every_day_of_the_sample_window() {
        let cal = gregorian_1911();
        for day in 1..=410 {
            let (y, m, d) = cal.to_ymd(day);
            assert_eq!(cal.to_day(y, m, d), Some(day), "公历 day {day} 往返不一致");
        }
    }

    #[test]
    fn gregorian_formats_with_chinese_era_prefix() {
        assert_eq!(
            gregorian_1911().format(283),
            "公元 1911 年 10 月 10 日",
            "样例主锚点的显示串"
        );
    }

    #[test]
    fn gregorian_day_difference_matches_real_calendar_distance() {
        // 5/9 → 10/10 相隔 283-129 = 154 天（真实历法距离），360/30 近似会差成 151。
        assert_eq!(
            gregorian_1911().to_day(1911, 10, 10).unwrap()
                - gregorian_1911().to_day(1911, 5, 9).unwrap(),
            154
        );
    }
}
