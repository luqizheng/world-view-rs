//! Mini-DSL filter 解析（ADR-0014 §2.5.1）。
//!
//! ## 作用域
//!
//! 三种构造：
//!
//! - **等值** `field = 'value'`：字符串完全相等。
//! - **子串** `field ~ 'value'`：`literal` 是 `haystack` 的子串。
//! - **合取** `expr AND expr`：仅 AND 合取；不开放 OR / NOT / 正则 / 数值范围。
//!
//! ## 失败语义
//!
//! 解析失败 → 返回 `FilterError`，handler 把它翻译为 JSON-RPC `INVALID_PARAMS`
//! 并附错误消息列出当前支持的语法。**不**静默接受「解析成全 true」——
//! 那等于欺骗 LLM「过滤生效了」。
//!
//! ## 与结构化字段（`{"name_contains": "X"}`）的取舍
//!
//! ADR §2.5.1 注脚：v0.1 用 Mini-DSL；若实现期发现 LLM 表达超出 DSL
//! 频繁失败，回退到结构化字段（改 handler 入参与 `tools/list` schema，
//! 契约不动）。本模块是**唯一**该决策的落地位置——别处别再发明过滤语法。
//!
//! 语法参考：
//!
//! ```text
//! filter      := predicate ( " AND " predicate )*
//! predicate   := field op "'" literal "'"
//! field       := [A-Za-z_][A-Za-z0-9_]*
//! op          := "=" | "~"
//! literal     := any character except "'"
//! ```

use serde_json::Value;

/// 解析后的过滤条件。LLM 调用 list_* 时传 `filter` 字符串，先
/// [`parse_filter`] 再用 [`matches`] 逐项判定。
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    predicates: Vec<Predicate>,
}

/// 单个谓词：`field op 'literal'`。`field` 是 JSON 对象的字段名（**未做
/// 嵌套展开**——v0.1 只支持顶层字段）。
#[derive(Debug, Clone, PartialEq)]
pub struct Predicate {
    pub field: String,
    pub op: Op,
    pub literal: String,
}

/// 比较运算符。**v0.1 只两种**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `=` 字符串完全相等。
    Eq,
    /// `~` 字符串子串匹配（`literal` 是 `haystack` 的子串）。
    Contains,
}

/// 解析或匹配时的错误。
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum FilterError {
    #[error("filter 不能为空字符串")]
    Empty,
    #[error("filter 语法错：{0}；当前支持 `field = 'value'` / `field ~ 'value'` / `expr AND expr`")]
    Syntax(String),
    #[error("未闭合的引号")]
    UnclosedQuote,
    #[error("未知的运算符；当前仅 `=` 与 `~`")]
    UnknownOp(String),
}

impl Filter {
    /// 把 Mini-DSL 字符串解析成 [`Filter`]。
    ///
    /// ## 语法（v0.1）
    /// ```text
    /// filter      := predicate ( "AND" predicate )*
    /// predicate   := field op "'" literal "'"
    /// field       := [A-Za-z_][A-Za-z0-9_]*
    /// op          := "=" | "~"
    /// literal     := any character except "'"
    /// ```
    pub fn parse(s: &str) -> Result<Self, FilterError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(FilterError::Empty);
        }
        // 顶层 `AND` 分割（不支持字面值里有 AND——本 DSL 的字面值是带引号的，
        // 不需要复杂的 token 化）。
        let mut predicates = Vec::new();
        for part in trimmed.split(" AND ") {
            let p = parse_predicate(part.trim())?;
            predicates.push(p);
        }
        Ok(Self { predicates })
    }

    /// `value`（一个 JSON 对象）是否满足全部谓词。**空谓词 = 全 true**。
    pub fn matches(&self, value: &Value) -> bool {
        self.predicates.iter().all(|p| p.matches(value))
    }

    /// 仅供测试 / 错误消息使用。
    pub fn predicates(&self) -> &[Predicate] {
        &self.predicates
    }
}

impl Predicate {
    pub fn matches(&self, value: &Value) -> bool {
        // 仅顶层字段；不存在 → 视为不等。LLM 拼错字段时直接落「不命中」——
        // 与「找不到就当相等」相比，更容易让 LLM 自查 schema。
        let Some(field) = value.get(&self.field) else {
            return false;
        };
        // 仅 string 字段参与。数值 / bool / 数组不参与（v0.1 不支持范围）。
        let Some(s) = field.as_str() else {
            return false;
        };
        match self.op {
            Op::Eq => s == self.literal,
            Op::Contains => s.contains(&self.literal),
        }
    }
}

fn parse_predicate(s: &str) -> Result<Predicate, FilterError> {
    // 形如 `field = 'value'` 或 `field ~ 'value'`。先按第一个 ASCII 空白
    // 拆字段名与剩余，再在剩余里找运算符与字面值。
    let mut chars = s.chars();
    let field: String = chars
        .by_ref()
        .take_while(|c| !c.is_whitespace())
        .collect();
    if field.is_empty() {
        return Err(FilterError::Syntax(format!("缺少字段名：`{s}`")));
    }
    let rest = chars.as_str().trim_start();
    let (op, rest) = if let Some(r) = rest.strip_prefix('~') {
        (Op::Contains, r.trim_start())
    } else if let Some(r) = rest.strip_prefix('=') {
        (Op::Eq, r.trim_start())
    } else {
        return Err(FilterError::Syntax(format!(
            "运算符错：`{s}`；仅 `=` 与 `~` 可用"
        )));
    };
    // 字面值：单引号包裹；引号内允许任意非 `'` 字符。
    let rest = rest
        .strip_prefix('\'')
        .ok_or_else(|| FilterError::Syntax(format!("字面值需用单引号包裹：`{s}`")))?;
    let end = rest
        .find('\'')
        .ok_or(FilterError::UnclosedQuote)?;
    let literal = rest[..end].to_owned();
    Ok(Predicate {
        field,
        op,
        literal,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_eq_and_contains_with_and_conjunction() {
        let f = Filter::parse("name = 'qingyun' AND kind ~ 'city'").unwrap();
        assert_eq!(f.predicates().len(), 2);
        assert_eq!(f.predicates()[0].field, "name");
        assert_eq!(f.predicates()[0].op, Op::Eq);
        assert_eq!(f.predicates()[0].literal, "qingyun");
        assert_eq!(f.predicates()[1].op, Op::Contains);
    }

    #[test]
    fn empty_filter_is_an_error_not_a_pass_through() {
        assert_eq!(Filter::parse(""), Err(FilterError::Empty));
        assert_eq!(Filter::parse("   "), Err(FilterError::Empty));
    }

    #[test]
    fn syntax_error_for_unknown_operator() {
        let err = Filter::parse("name > 'x'").unwrap_err();
        assert!(matches!(err, FilterError::Syntax(_)));
    }

    #[test]
    fn syntax_error_for_unquoted_literal() {
        let err = Filter::parse("name = qingyun").unwrap_err();
        assert!(matches!(err, FilterError::Syntax(_)));
    }

    #[test]
    fn unclosed_quote_is_its_own_error() {
        let err = Filter::parse("name = 'qingyun").unwrap_err();
        assert_eq!(err, FilterError::UnclosedQuote);
    }

    #[test]
    fn matches_predicates_against_a_json_object() {
        let f = Filter::parse("name = 'qingyun'").unwrap();
        assert!(f.matches(&json!({"name": "qingyun"})));
        assert!(!f.matches(&json!({"name": "other"})));
        assert!(!f.matches(&json!({"other": "qingyun"})), "缺失字段不命中");
        assert!(!f.matches(&json!({"name": 42})), "非 string 字段不命中");
    }

    #[test]
    fn contains_is_substring_match() {
        let f = Filter::parse("name ~ 'qing'").unwrap();
        assert!(f.matches(&json!({"name": "qingyun"})));
        assert!(!f.matches(&json!({"name": "wuhan"})));
    }

    #[test]
    fn conjunction_requires_all_predicates() {
        let f = Filter::parse("name = 'qingyun' AND kind = 'city'").unwrap();
        assert!(!f.matches(&json!({"name": "qingyun", "kind": "town"})));
        assert!(f.matches(&json!({"name": "qingyun", "kind": "city"})));
    }

    #[test]
    fn filter_with_no_predicates_is_impossible_to_construct_publicly() {
        // 构造路径只走 parse("...")，空字符串必报错——保证 Filter 至少有一个谓词。
        let f = Filter::parse("name = 'x'").unwrap();
        assert!(!f.predicates().is_empty());
    }
}