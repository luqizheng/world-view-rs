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

use crate::i18n::{I18nKey, Locale};
use crate::state::{PlaybackState, WorldProjection};

/// 把天数计数器格式化为作者可读的日期。
///
/// - 有历法：`无仙 3 年 5 月 12 日`（经 `Calendar::format`）。
/// - 无历法：`第 N 天`（zh）/ `Day N`（en）。
///
/// **不接受** `@dayN` 这种内部记号——那是调试写法，不该出现在界面上。
///
/// `locale` 参数：v1 仅驱动「无历法」分支（zh 用「第 N 天」，en 用 `Day N`）。
/// `wv_core::Calendar::format` 是项目内嵌的域数据格式（作者可改前缀），按
/// ADR-0018 §3.5 不视为本地化目标，保持原样返回。
pub fn format_date_with(calendar: Option<&wv_core::Calendar>, day: i64, locale: Locale) -> String {
    match calendar {
        Some(c) => c.format(day),
        None => format!(
            "{}{}{}",
            match locale {
                Locale::Zh => crate::i18n::t(locale, I18nKey::TimeNoCalZhPrefix),
                Locale::En => crate::i18n::t(locale, I18nKey::TimeNoCalEnPrefix),
            },
            day,
            match locale {
                Locale::Zh => crate::i18n::t(locale, I18nKey::TimeNoCalZhSuffix),
                Locale::En => crate::i18n::t(locale, I18nKey::TimeNoCalEnSuffix),
            }
        ),
    }
}

/// 从世界投影取历法，格式化当前时刻。
pub fn format_date_for(projection: &WorldProjection, day: i64, locale: Locale) -> String {
    let cal = projection.world.timeline.as_ref().map(|t| &t.calendar);
    format_date_with(cal, day, locale)
}

/// 便捷入口：从 [`PlaybackState`] 取当前时刻（供无 `World` 引用的组件用）。
///
/// 注意这**不是**权威换算——它只在拿不到 `World` 时兜底成「第 N 天」。
/// 真正要显示作者日期的地方应走 [`format_date_for`]。
pub fn format_date(day: i64, locale: Locale) -> String {
    format_date_with(None, day, locale)
}

/// 读出当前时刻并格式化（顶栏/时间轴共用，保证一致）。
pub fn current_date(
    projection: &WorldProjection,
    playback: &PlaybackState,
    locale: Locale,
) -> String {
    format_date_for(projection, playback.current_day, locale)
}

/// 世界的历法（若无则 None）。
pub fn calendar_of(projection: &WorldProjection) -> Option<&wv_core::Calendar> {
    projection.world.timeline.as_ref().map(|t| &t.calendar)
}

/// 面包屑段：把当前世界层级结构展成 `[World, Region, City, Interior]` 字符串数组。
///
/// v0.3（Figma `5E8BCBCmtGAHqk3ORibBlU`）：顶栏与状态栏都从这条函数取
/// `World › Zunyi › Government Office` 段序列。**所有显示层级路径的地方必须
/// 走这里**（§5.3「唯一真源」原则的扩展）。
///
/// - `current_map_id` 为空：返回 `["World"]`。
/// - 找到当前地图但无显示名：返回 `["World", "<id>"]`。
///
/// 不在显示串里硬塞 emoji / 装饰符（让 caller 用图标资源自行装饰）。
///
/// `current_map_id` 来源：[`crate::state::ViewState::current_map`]（不是
/// `WorldProjection` 字段——后者没有该字段）。
pub fn format_breadcrumb(projection: &WorldProjection, current_map_id: &str) -> Vec<String> {
    if current_map_id.is_empty() {
        return vec!["World".to_string()];
    }
    let name = projection
        .world
        .maps
        .iter()
        .find(|m| m.id.as_str() == current_map_id)
        .map(|m| m.name.clone())
        .unwrap_or_else(|| current_map_id.to_string());
    vec!["World".to_string(), name]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_without_calendar_reads_as_day_counter() {
        let s = format_date_with(None, 100805, Locale::Zh);
        assert_eq!(s, "第 100805 天", "got {s}");
    }

    #[test]
    fn day_without_calendar_in_english_reads_as_day_counter() {
        let s = format_date_with(None, 100805, Locale::En);
        assert_eq!(s, "Day 100805", "got {s}");
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
        let s = format_date_with(Some(&c), 721, Locale::Zh);
        assert_eq!(s, "无仙 3 年 1 月 1 日", "got {s}");
        assert_eq!(
            format_date_with(Some(&c), 1081, Locale::Zh),
            "无仙 4 年 1 月 1 日"
        );
        // 跨月边界：第 1 年第 2 月第 1 天 = 30 + 1。
        assert_eq!(
            format_date_with(Some(&c), 31, Locale::Zh),
            "无仙 1 年 2 月 1 日"
        );
        // 与 `to_day` 互为逆运算（手册 §7.2：用算出来的期望值断言）。
        assert_eq!(c.to_day(3, 1, 1), Some(721));
    }

    #[test]
    fn never_emits_internal_at_day_notation() {
        let s = format_date(1, Locale::Zh);
        assert!(!s.contains('@'), "got {s}");
    }

    #[test]
    fn breadcrumb_with_no_map_returns_world_only() {
        let p = WorldProjection::default();
        assert_eq!(format_breadcrumb(&p, ""), vec!["World".to_string()]);
    }

    #[test]
    fn breadcrumb_returns_world_then_current_map_name() {
        let mut p = WorldProjection::default();
        let node = wv_core::map::MapNode {
            id: wv_core::id::Id::new_unchecked("city_zunyi"),
            name: "Zunyi".into(),
            level: wv_core::map::MapLevel::City,
            ..map_node_stub()
        };
        p.world.maps.push(node);
        assert_eq!(
            format_breadcrumb(&p, "city_zunyi"),
            vec!["World".to_string(), "Zunyi".to_string()]
        );
    }

    fn map_node_stub() -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: wv_core::id::Id::new_unchecked(""),
            name: String::new(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: Vec::new(),
            icons: Vec::new(),
            layers: Vec::new(),
            extent: None,
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
        }
    }
}
