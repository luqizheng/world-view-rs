//! 实体标识。
//!
//! 约定：ID 是**人类可读的稳定字符串**（如 `city_qingyun`），
//! 而非自增整数或 UUID。理由见 `AGENTS.md`：
//! ID 会出现在 YAML、Git diff 与作者手写内容中，必须可读、可手工引用。

use std::fmt;

/// 实体标识。内部为轻量字符串包装，避免误把任意 `String` 当 ID 传递。
#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Id(String);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("ID 不能为空")]
    Empty,
    #[error("ID `{0}` 含非法字符：只允许 ASCII 字母、数字与下划线")]
    InvalidChar(String),
}

impl Id {
    /// 构造一个 ID，并校验字符集。
    ///
    /// 字符集限制为 `[A-Za-z0-9_]`：这是 Git 友好的、跨文件系统的、
    /// 且在 YAML 中无需引号的最小安全集。
    pub fn new(raw: impl Into<String>) -> Result<Self, IdError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(IdError::Empty);
        }
        if !raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(IdError::InvalidChar(raw));
        }
        Ok(Id(raw))
    }

    /// 不经校验地构造。仅供测试与从可信来源（如已校验的 YAML）加载时使用。
    pub fn new_unchecked(raw: impl Into<String>) -> Self {
        Id(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
