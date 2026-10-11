//! 可插拔历法：`CalendarRule`（工程级唯一）+ 换算。
//!
//! 核心规格（用户确认，唯一权威）：
//! - 内部主表示 = **整数天数计数器**，第 1 天 = 游戏第一日；每天自动 +1。
//! - 历法是**可插拔的显示/解析规则**，不是运算主键；运算主键永远是 counter。
//! - 只有**一个起点**，往前不存在（无负数计数器）。
//! - 前缀（如 `无仙` / `帝国历`）只是**起点的命名标签**，不是可切换纪元。
//! - 农历/年号**只做显示（作者手填注释）**，不实现农历算法、不内置农历表。

use crate::error::CoreError;

/// 内置历法种类（封闭三值）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum CalendarRule {
    /// ① 公历（真实格里高利历，含闰年）。
    Gregorian,
    /// ② 简单 365 天历（无闰年，月长与公历非闰年一致）。
    #[serde(rename = "simple_365")]
    Simple365,
    /// ③ 360 天/年、30 天/月（奇幻默认，等价上一轮实现）。
    #[default]
    #[serde(rename = "fixed_360")]
    Fixed360,
}

impl CalendarRule {
    /// 平年天数（非闰年基准）。
    pub const fn days_per_common_year(self) -> i64 {
        match self {
            CalendarRule::Gregorian | CalendarRule::Simple365 => 365,
            CalendarRule::Fixed360 => 360,
        }
    }

    pub const fn has_leap_year(self) -> bool {
        matches!(self, CalendarRule::Gregorian)
    }

    pub fn days_in_year(self, year: i64) -> i64 {
        match self {
            CalendarRule::Gregorian if is_leap_year(year) => 366,
            _ => self.days_per_common_year(),
        }
    }

    pub fn days_in_month(self, year: i64, month: i64) -> i64 {
        debug_assert!((1..=12).contains(&month));
        match self {
            CalendarRule::Fixed360 => 30,
            CalendarRule::Simple365 | CalendarRule::Gregorian => match month {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                2 => {
                    if self == CalendarRule::Gregorian && is_leap_year(year) {
                        29
                    } else {
                        28
                    }
                }
                _ => 30,
            },
        }
    }

    pub const fn months_per_year(self) -> i64 {
        12
    }
}

/// 格里高利闰年规则：**4 年一闰、百年不闰、四百年再闰**。
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 时间轴坐标系：历法规则 + 起点命名 + 起点年。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Calendar {
    #[serde(default)]
    pub prefix: String,
    pub start_year: i32,
    #[serde(default)]
    pub rule: CalendarRule,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reign_aliases: Vec<ReignAlias>,
}

/// 年号对照。**仅显示用**——不参与排序、不参与区间求值。
///
/// 七字段：前四为**内置静态史实**（支撑年份级硬校验），中二为**工具换算**的
/// 天数计数器区间，末一为**作者手填**、工具绝不解析。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReignAlias {
    /// 【内置静态史实】年号名，如 `嘉庆`。
    pub era_name: String,
    /// 【内置静态史实】朝代，如 `清`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynasty: Option<String>,
    /// 【内置静态史实】该年号的公历起年（含），如 1796。用于年份级硬校验。
    pub gregorian_start: i32,
    /// 【内置静态史实】该年号的公历止年（含），如 1820。
    pub gregorian_end: i32,
    /// 【工具换算】本对照在时间轴上的天数计数器起点（含）。
    pub start: i64,
    /// 【工具换算】终点（不含）；省略 = 至今。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    /// 【作者手填】农历写法，如 `嘉庆二十三年四月初一`。**工具绝不解析此字段**。
    pub lunar_label: String,
}

impl ReignAlias {
    /// 年份级硬校验：作者填的公历年份是否落在年号期内。
    ///
    /// **不需要农历算法**——只是比较两个公历年份（`gregorian_start/end` 是静态史实）。
    /// 月/日级换算不在此函数（工具不推算农历，只警告）。
    pub fn is_year_consistent(&self, gregorian_year: i32) -> bool {
        (self.gregorian_start..=self.gregorian_end).contains(&gregorian_year)
    }
}

impl Calendar {
    pub fn new(prefix: impl Into<String>, start_year: i32, rule: CalendarRule) -> Self {
        Self {
            prefix: prefix.into(),
            start_year,
            rule,
            reign_aliases: Vec::new(),
        }
    }

    /// 天数计数器 → 年月日。**不使用 `offset / 365` 近似**。
    pub fn civil(&self, counter: i64) -> CivilDate {
        let mut offset = counter - 1;
        let mut year = self.start_year as i64;

        loop {
            let diy = self.rule.days_in_year(year);
            if offset < diy {
                break;
            }
            offset -= diy;
            year += 1;
        }

        let mut month = 1i64;
        loop {
            let dim = self.rule.days_in_month(year, month);
            if offset < dim {
                break;
            }
            offset -= dim;
            month += 1;
        }

        CivilDate {
            year,
            month,
            day: offset + 1,
        }
    }

    /// 年月日 → 天数计数器。**整年天数累加**，非近似。
    pub fn day_counter(&self, year: i64, month: i64, day: i64) -> Result<i64, CoreError> {
        if year < self.start_year as i64 {
            return Err(CoreError::BeforeTimelineStart {
                year,
                start_year: self.start_year,
            });
        }
        let months = self.rule.months_per_year();
        if !(1..=months).contains(&month) {
            return Err(CoreError::InvalidMonth { month, max: months });
        }
        let dim = self.rule.days_in_month(year, month);
        if !(1..=dim).contains(&day) {
            return Err(CoreError::InvalidDay { day, max: dim });
        }

        let mut counter: i64 = 1;
        let mut y = self.start_year as i64;
        while y < year {
            counter += self.rule.days_in_year(y);
            y += 1;
        }
        let mut m = 1i64;
        while m < month {
            counter += self.rule.days_in_month(year, m);
            m += 1;
        }
        counter += day - 1;
        Ok(counter)
    }

    pub fn first_day(&self) -> i64 {
        1
    }

    /// 展示文本。`with_year = false` 时省略年份（对应口语省略句式）。
    pub fn format(&self, counter: i64, with_year: bool) -> String {
        let d = self.civil(counter);
        let core = if with_year {
            format!("{} {} 年 {} 月 {} 日", self.prefix, d.year, d.month, d.day)
        } else {
            format!("{} {} 月 {} 日", self.prefix, d.month, d.day)
        };
        if let Some(alias) = self
            .reign_aliases
            .iter()
            .find(|a| counter >= a.start && a.end.map_or(true, |e| counter < e))
        {
            return format!("{core}（{}）", alias.lunar_label);
        }
        core
    }
}

/// 换算后的年月日。**只用于展示/解析**，不得作为存盘字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CivilDate {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl CivilDate {
    pub fn to_counter(self, calendar: &Calendar) -> Result<i64, CoreError> {
        calendar.day_counter(self.year, self.month, self.day)
    }
}
