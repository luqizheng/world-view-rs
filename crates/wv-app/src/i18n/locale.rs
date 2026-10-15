//! Locale 枚举与解析。
//!
//! ## 职责
//! - 定义 UI 文案语言（`Locale::Zh` / `Locale::En`）；`Default = Zh` 与现行
//!   唯一工作语言一致（ADR-0018 §2.1）。
//! - 暴露 `parse(s)` 接受 `"zh"` / `"zh-CN"` / `"zh-Hans"` / `"en"` / `"en-US"` 等，
//!   不区分大小写；未知返回 `None`（静默回退语言默认）。
//!
//! ## 边界（不做什么）
//! - **不持字符串所有权**——Locale 仅是个判别枚举；
//! - **不做 OS 探测**——ADR-0018 §3 决定不引入 sys-locale crate，
//!   启动期 fallback 由 `LayoutFile.language` 缺省为 `None` → 默认 zh 兜底。
//!
//! ## 主要导出
//! - [`Locale`] 枚举
//! - [`parse`] 自由函数
use serde::{Deserialize, Serialize};

/// UI 文案语言。
///
/// 默认 `Zh` 与工程当前唯一工作语言一致；`En` 由用户在顶栏切换或
/// `ui-layout.yaml#language` 覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Locale {
    /// 简体中文（语言标签：`zh`）
    #[default]
    Zh,
    /// English (BCP-47 tag `en`)
    En,
}

impl Locale {
    /// 所有支持的 locale，用于测试遍历。
    pub const ALL: [Locale; 2] = [Locale::Zh, Locale::En];

    /// 语言标签（BCP-47 前缀字符串），用于 `ui-layout.yaml#language` 写盘。
    pub const fn as_str(self) -> &'static str {
        match self {
            Locale::Zh => "zh",
            Locale::En => "en",
        }
    }
}

impl std::fmt::Display for Locale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 解析语言标签。接受 BCP-47 主语言子标签：
/// `zh` / `zh-CN` / `zh-Hans` / `zh-Hans-CN` → `Some(Zh)`；
/// `en` / `en-US` / `en-GB` → `Some(En)`；其余 `None`。
///
/// **不**区分大小写；允许前缀后跟 ASCII `-` 与下划线。
pub fn parse(s: &str) -> Option<Locale> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let bytes = s.as_bytes();
    let primary = [bytes[0].to_ascii_lowercase(), bytes[1].to_ascii_lowercase()];
    match &primary {
        b"zh" => Some(Locale::Zh),
        b"en" => Some(Locale::En),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zh_and_en_are_the_only_default_locales() {
        assert_eq!(Locale::default(), Locale::Zh);
        assert_eq!(Locale::ALL.len(), 2);
    }

    #[test]
    fn locale_as_str_round_trips_through_parse() {
        for l in Locale::ALL {
            assert_eq!(parse(l.as_str()), Some(l));
        }
    }

    #[test]
    fn locale_parse_accepts_bcp47_subtags() {
        assert_eq!(parse("zh"), Some(Locale::Zh));
        assert_eq!(parse("zh-CN"), Some(Locale::Zh));
        assert_eq!(parse("zh-Hans"), Some(Locale::Zh));
        assert_eq!(parse("zh-Hans-CN"), Some(Locale::Zh));
        assert_eq!(parse("en"), Some(Locale::En));
        assert_eq!(parse("en-US"), Some(Locale::En));
        assert_eq!(parse("en-GB"), Some(Locale::En));
    }

    #[test]
    fn locale_parse_is_case_insensitive() {
        assert_eq!(parse("ZH"), Some(Locale::Zh));
        assert_eq!(parse("Zh-Cn"), Some(Locale::Zh));
        assert_eq!(parse("EN"), Some(Locale::En));
    }

    #[test]
    fn locale_parse_rejects_unrelated_tags() {
        assert_eq!(parse("ja"), None);
        assert_eq!(parse("fr-FR"), None);
        assert_eq!(parse(""), None);
        assert_eq!(parse("z"), None);
        assert_eq!(parse("xx"), None);
    }

    #[test]
    fn locale_display_matches_as_str() {
        assert_eq!(Locale::Zh.to_string(), "zh");
        assert_eq!(Locale::En.to_string(), "en");
    }
}
