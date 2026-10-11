//! 交付稿全部数字/断言的**唯一守护测试**。
//!
//! 纪律（来自上一轮教训）：凡稿中出现的具体数字，必须在此钉死。

use wv_cal::calendar::{is_leap_year, Calendar, CalendarRule, CivilDate, ReignAlias};
use wv_cal::error::CoreError;
use wv_cal::span::{check_span, RECOMMENDED_MAX_SPAN_DAYS};
use wv_cal::story::{check_event_lower_bound, validate_timeline_item, Id, Story, TimelineItem};
use wv_cal::time::{TimePoint, TimeSpan};

fn greg() -> Calendar {
    Calendar::new("", 1840, CalendarRule::Gregorian)
}

// ---------- 闰年规则 ----------

#[test]
fn gregorian_leap_year_rule_is_exact() {
    assert!(is_leap_year(2024));
    assert!(is_leap_year(2020));
    assert!(!is_leap_year(1900));
    assert!(!is_leap_year(2100));
    assert!(is_leap_year(2000));
    assert!(is_leap_year(1600));
    assert!(!is_leap_year(2023));
    assert!(is_leap_year(0));
    assert!(is_leap_year(-4));
    assert!(!is_leap_year(-100));
    assert!(is_leap_year(-400));
}

#[test]
fn days_in_year_by_rule() {
    assert_eq!(CalendarRule::Gregorian.days_in_year(2024), 366);
    assert_eq!(CalendarRule::Gregorian.days_in_year(2023), 365);
    assert_eq!(CalendarRule::Simple365.days_in_year(2024), 365);
    assert_eq!(CalendarRule::Fixed360.days_in_year(2024), 360);
}

#[test]
fn days_in_month_gregorian_february() {
    assert_eq!(CalendarRule::Gregorian.days_in_month(2024, 2), 29);
    assert_eq!(CalendarRule::Gregorian.days_in_month(2023, 2), 28);
    assert_eq!(CalendarRule::Simple365.days_in_month(2024, 2), 28);
    assert_eq!(CalendarRule::Fixed360.days_in_month(2024, 2), 30);
}

// ---------- 公历换算：契约锚点 ----------

#[test]
fn user_anchor_1818_05_05_is_consistent() {
    let c = Calendar::new("", 1818, CalendarRule::Gregorian);
    assert_eq!(
        c.civil(1),
        CivilDate {
            year: 1818,
            month: 1,
            day: 1
        }
    );
    // 1818 平年：31+28+31+30 = 120 天到 5/1 之前，5/5 = 120 + 5 = 第 125 天。
    let day = c.day_counter(1818, 5, 5).unwrap();
    assert_eq!(day, 125, "1818/5/5 应为第 125 天");
    assert_eq!(
        c.civil(125),
        CivilDate {
            year: 1818,
            month: 5,
            day: 5
        }
    );
}

#[test]
fn spec_start_1840_first_day_is_jan_first() {
    let c = Calendar::new("公元", 1840, CalendarRule::Gregorian);
    assert_eq!(
        c.civil(1),
        CivilDate {
            year: 1840,
            month: 1,
            day: 1
        }
    );
    assert_eq!(c.day_counter(1840, 1, 1).unwrap(), 1);
}

#[test]
fn leap_year_february_29_round_trips() {
    let c = Calendar::new("", 2020, CalendarRule::Gregorian);
    assert_eq!(c.day_counter(2020, 2, 29).unwrap(), 60);
    assert_eq!(c.day_counter(2020, 3, 1).unwrap(), 61);
    assert!(c.day_counter(2021, 2, 29).is_err());
}

// ---------- 换算恒等式：全范围往返 ----------

#[test]
fn conversion_round_trips_over_full_range_gregorian() {
    let c = greg();
    let upper = 200 * 365 + 60;
    for counter in 1..=upper {
        let d = c.civil(counter);
        assert_eq!(
            c.day_counter(d.year, d.month, d.day).unwrap(),
            counter,
            "公历 counter {counter} → {d:?} 往返失败"
        );
    }
}

#[test]
fn conversion_round_trips_over_full_range_simple365() {
    let c = Calendar::new("", 1840, CalendarRule::Simple365);
    for counter in 1..=200 * 365 {
        let d = c.civil(counter);
        assert_eq!(c.day_counter(d.year, d.month, d.day).unwrap(), counter);
    }
}

#[test]
fn conversion_round_trips_over_full_range_fixed360() {
    let c = Calendar::new("无仙", 20, CalendarRule::Fixed360);
    for counter in 1..=200_000 {
        let d = c.civil(counter);
        assert_eq!(c.day_counter(d.year, d.month, d.day).unwrap(), counter);
    }
}

// ---------- 决策表：钉死稿中全部字面量 ----------

#[test]
fn draft_literal_values_are_all_verified() {
    let g = Calendar::new("公元", 1840, CalendarRule::Gregorian);
    let greg_table: &[(i64, i64, i64, i64)] = &[
        (1, 1840, 1, 1),
        (31, 1840, 1, 31),
        (32, 1840, 2, 1),
        (60, 1840, 2, 29),
        (61, 1840, 3, 1),
        (366, 1840, 12, 31),
        (367, 1841, 1, 1),
        (731, 1841, 12, 31),
        (732, 1842, 1, 1),
    ];
    for &(counter, y, m, d) in greg_table {
        assert_eq!(
            g.civil(counter),
            CivilDate {
                year: y,
                month: m,
                day: d
            },
            "公历 counter {counter}"
        );
        assert_eq!(g.day_counter(y, m, d).unwrap(), counter, "公历 {y}/{m}/{d}");
    }

    let f = Calendar::new("无仙", 20, CalendarRule::Fixed360);
    let fixed_table: &[(i64, i64, i64, i64)] = &[
        (1, 20, 1, 1),
        (2, 20, 1, 2),
        (30, 20, 1, 30),
        (31, 20, 2, 1),
        (360, 20, 12, 30),
        (361, 21, 1, 1),
        (93_615, 280, 1, 15),
        (100_801, 300, 1, 1),
        (100_810, 300, 1, 10),
        (102_601, 305, 1, 1),
        (102_602, 305, 1, 2),
        (180_360, 520, 12, 30),
    ];
    for &(counter, y, m, d) in fixed_table {
        assert_eq!(
            f.civil(counter),
            CivilDate {
                year: y,
                month: m,
                day: d
            },
            "360历 counter {counter}"
        );
        assert_eq!(
            f.day_counter(y, m, d).unwrap(),
            counter,
            "360历 {y}/{m}/{d}"
        );
    }
}

/// 10 万年天数：稿中唯一被点名的"散文数字"，必须钉死。
#[test]
fn span_of_100k_years_in_days_is_pinned() {
    let c = Calendar::new("", 2000, CalendarRule::Gregorian);
    let start = c.day_counter(2000, 1, 1).unwrap();
    let end = c.day_counter(102_000, 1, 1).unwrap();
    let days = end - start;
    assert_eq!(
        100_000 * 365 + 24_250,
        days,
        "10 万年（2000→102000）应为 36,524,250 天"
    );
    assert_eq!(days, 36_524_250);
}

/// i64 容量：远大于任何文学跨度。
#[test]
fn i64_capacity_far_exceeds_literary_span() {
    let max_days = i64::MAX as i128;
    let years = max_days * 10_000 / 3_652_425;
    assert!(years > 25_000_000_000_000_000i128, "i64 容量应 > 2.5e16 年");
    let ratio_ppm = 36_524_250i128 * 1_000_000_000_000 / max_days;
    assert_eq!(ratio_ppm, 3, "10 万年仅占 i64 容量的约 4e-12");
}

// ---------- 事件硬下界 ----------

#[test]
fn event_must_be_strictly_after_start_zeroth_hour() {
    let start_day = 1;
    assert!(check_event_lower_bound(1, start_day).is_err());
    assert!(check_event_lower_bound(0, start_day).is_err());
    assert!(check_event_lower_bound(2, start_day).is_ok());
}

#[test]
fn story_skips_lower_bound_check() {
    let story = Story {
        id: Id::new_unchecked("story_1"),
        name: "上古传说".into(),
        body: Some("创世".into()),
        tags: vec![],
        // placements 是展示层弱引用，不赋予时间语义。
        placements: vec![Id::new_unchecked("anchor_founding")],
    };
    let item = TimelineItem::Undated { story };
    assert!(validate_timeline_item(&item, 1).is_ok());

    let dated = TimelineItem::Dated {
        span: TimeSpan::new(1, 5).unwrap(),
    };
    assert!(validate_timeline_item(&dated, 1).is_err(), "from=1 应被拒");
    let dated2 = TimelineItem::Dated {
        span: TimeSpan::new(2, 5).unwrap(),
    };
    assert!(validate_timeline_item(&dated2, 1).is_ok());
}

// ---------- 三种历法的 YAML 形态 ----------

#[test]
fn calendar_yaml_round_trips_all_three_rules() {
    for rule in [
        CalendarRule::Gregorian,
        CalendarRule::Simple365,
        CalendarRule::Fixed360,
    ] {
        let c = Calendar::new("测试", 100, rule);
        let text = serde_yaml::to_string(&c).unwrap();
        let back: Calendar = serde_yaml::from_str(&text).unwrap();
        assert_eq!(c, back, "Calendar round-trip 失败: {rule:?}");
    }
}

#[test]
fn calendar_rule_serializes_snake_case() {
    assert_eq!(
        serde_yaml::to_string(&CalendarRule::Gregorian)
            .unwrap()
            .trim(),
        "gregorian"
    );
    assert_eq!(
        serde_yaml::to_string(&CalendarRule::Simple365)
            .unwrap()
            .trim(),
        "simple_365"
    );
    assert_eq!(
        serde_yaml::to_string(&CalendarRule::Fixed360)
            .unwrap()
            .trim(),
        "fixed_360"
    );
}

#[test]
fn reign_alias_shows_only_as_annotation() {
    let mut c = Calendar::new("公元", 1818, CalendarRule::Gregorian);
    let d = c.day_counter(1818, 5, 5).unwrap();
    c.reign_aliases.push(ReignAlias {
        era_name: "嘉庆".into(),
        dynasty: Some("清".into()),
        gregorian_start: 1796,
        gregorian_end: 1820,
        start: d,
        end: None,
        lunar_label: "嘉庆二十三年四月初一".into(),
    });
    let s = c.format(d, true);
    assert!(
        s.contains("嘉庆二十三年四月初一"),
        "年号应作为显示注释: {s}"
    );
    assert_eq!(
        c.civil(d),
        CivilDate {
            year: 1818,
            month: 5,
            day: 5
        }
    );
}

/// 年号年份级硬校验：嘉庆期 1796–1820；1818 落在期内、1821 不在。
#[test]
fn reign_alias_year_level_validation() {
    let a = ReignAlias {
        era_name: "嘉庆".into(),
        dynasty: Some("清".into()),
        gregorian_start: 1796,
        gregorian_end: 1820,
        start: 1,
        end: None,
        lunar_label: "嘉庆二十三年四月初一".into(),
    };
    assert!(a.is_year_consistent(1818), "1818 应在嘉庆期内");
    assert!(a.is_year_consistent(1796), "1796 是边界（含）");
    assert!(a.is_year_consistent(1820), "1820 是边界（含）");
    assert!(!a.is_year_consistent(1795), "1795 早于嘉庆元年");
    assert!(!a.is_year_consistent(1821), "1821 晚于嘉庆末年");
}

// ---------- TimePoint 单字段不变量 ----------

#[test]
fn timepoint_is_single_field_newtype() {
    assert_eq!(std::mem::size_of::<TimePoint>(), 8);
    let s = serde_yaml::to_string(&TimePoint::from_counter(42).unwrap()).unwrap();
    assert_eq!(s.trim(), "42", "TimePoint 应为裸整数 YAML");
}

// ---------- 上一轮固定 360 数字必须原样保留 ----------

#[test]
fn fixed360_reproduces_previous_round_numbers() {
    let c = Calendar::new("无仙", 20, CalendarRule::Fixed360);
    assert_eq!(c.day_counter(520, 12, 30).unwrap(), 180_360);
    assert_eq!(500 * 360 + 11 * 30 + 30, 180_360);
    assert_eq!(
        c.civil(180_361),
        CivilDate {
            year: 521,
            month: 1,
            day: 1
        }
    );
}

#[test]
fn error_variants_are_reachable() {
    let c = greg();
    assert!(matches!(
        c.day_counter(1839, 1, 1),
        Err(CoreError::BeforeTimelineStart { .. })
    ));
    assert!(matches!(
        c.day_counter(1840, 13, 1),
        Err(CoreError::InvalidMonth { .. })
    ));
    assert!(matches!(
        c.day_counter(1840, 2, 30),
        Err(CoreError::InvalidDay { .. })
    ));
}

// ---------- 跨度预警（降级，非门禁） ----------

#[test]
fn span_limit_is_one_million_days() {
    assert_eq!(RECOMMENDED_MAX_SPAN_DAYS, 1_000_000);
}

#[test]
fn span_warning_is_degradation_not_gate() {
    // 在上限内：无预警，正常渲染。
    assert!(check_span(1, 500_000).unwrap().is_none());
    assert!(check_span(1, 1_000_000).unwrap().is_none());
    // 超上限：返回预警但**不报错**（降级语义）。
    // 注意 days = last_day - first_day，故 last_day=1_000_002 ⇒ days=1_000_001 > 上限。
    let w = check_span(1, 1_000_002).unwrap();
    assert!(w.is_some(), "超出上限应给出预警而非报错");
    // 10 万年：预警，且年数约为 100000。
    let w2 = check_span(1, 36_524_250).unwrap().unwrap();
    assert!((w2.approx_years - 100_000.0).abs() < 1.0, "10万年≈100000年");
    // 上一轮 18 万格：**不触发**预警（仍在可承受区）。
    assert!(check_span(1, 180_000).unwrap().is_none(), "18万格不该报警");
    // 100 万格是 18 万格的 5.56 倍——正是取此上限的理由。
    assert!((1_000_000.0_f64 / 180_000.0 - 5.5556).abs() < 0.001);
}

#[test]
fn span_never_used_as_gate() {
    // 门禁类（last_day）是 wv-io 的事；本函数只降级。
    let w = check_span(1, 36_524_250).unwrap();
    assert!(w.is_some());
    let msg = w.unwrap().message();
    assert!(msg.contains("骨架"), "预警文案应说明降级为骨架模式: {msg}");
}

// ---------- 示例 YAML 数字（A/B/C 三份）逐一钉死 ----------

#[test]
fn example_yaml_numbers_are_all_verified() {
    // 示例 A：公历 start_year=1840，last_day=73050 = 2040/1/1（200 年）
    let a = Calendar::new("公元", 1840, CalendarRule::Gregorian);
    assert_eq!(
        a.civil(1),
        CivilDate {
            year: 1840,
            month: 1,
            day: 1
        }
    );
    assert_eq!(a.day_counter(2040, 1, 1).unwrap(), 73_050);
    assert_eq!(
        a.civil(73_050),
        CivilDate {
            year: 2040,
            month: 1,
            day: 1
        }
    );
    // 交叉验证：200 年 = 200*365 + 49 个闰日 + 1
    assert_eq!(200 * 365 + 49 + 1, 73_050);

    // 示例 B：simple365 start_year=1，last_day=365000 = 1000/12/31（1000 年最后一天）
    let b = Calendar::new("新历", 1, CalendarRule::Simple365);
    assert_eq!(b.day_counter(1000, 12, 31).unwrap(), 365_000);
    assert_eq!(
        b.civil(365_000),
        CivilDate {
            year: 1000,
            month: 12,
            day: 31
        }
    );
    // 1001/1/1 = 第 365001 天（1000 年整 + 1）
    assert_eq!(b.day_counter(1001, 1, 1).unwrap(), 365_001);
    assert_eq!(365 * 1000, 365_000);

    // 示例 C：fixed360 start_year=20，last_day=180360 / anchor=100801
    let c = Calendar::new("无仙", 20, CalendarRule::Fixed360);
    assert_eq!(c.day_counter(520, 12, 30).unwrap(), 180_360);
    assert_eq!(c.day_counter(300, 1, 1).unwrap(), 100_801);
}

// ---------- 七字段 ReignAlias 与 placements 的 YAML 形态 ----------

#[test]
fn reign_alias_seven_fields_round_trip() {
    let a = ReignAlias {
        era_name: "嘉庆".into(),
        dynasty: Some("清".into()),
        gregorian_start: 1796,
        gregorian_end: 1820,
        start: 125,
        end: Some(490),
        lunar_label: "嘉庆二十三年四月初一".into(),
    };
    let text = serde_yaml::to_string(&a).unwrap();
    // 七字段键名必须都在
    for key in [
        "era_name",
        "dynasty",
        "gregorian_start",
        "gregorian_end",
        "start",
        "end",
        "lunar_label",
    ] {
        assert!(text.contains(key), "YAML 缺字段 {key}: {text}");
    }
    let back: ReignAlias = serde_yaml::from_str(&text).unwrap();
    assert_eq!(a, back, "ReignAlias round-trip 必须稳定");
}

#[test]
fn story_placements_round_trip_and_stays_dateless() {
    let s = Story {
        id: Id::new_unchecked("story_creation"),
        name: "创世神话".into(),
        body: Some("天地初开".into()),
        tags: vec!["神话".into()],
        placements: vec![Id::new_unchecked("anchor_founding")],
    };
    let text = serde_yaml::to_string(&s).unwrap();
    // 故事**没有** day 字段（物理上写不出来）。
    assert!(!text.contains("day"), "故事 YAML 不应含 day 字段: {text}");
    let back: Story = serde_yaml::from_str(&text).unwrap();
    assert_eq!(s, back);
    assert_eq!(back.placements.len(), 1);
    // placements 为空时省略该键。
    let s2 = Story {
        placements: vec![],
        ..s
    };
    let t2 = serde_yaml::to_string(&s2).unwrap();
    assert!(!t2.contains("placements"), "空 placements 不应输出: {t2}");
}
