//! Overture 唯一标识符。
//!
//! 本模块实现 ADR-0007「UUID 例外条款」：
//! - GERS ID 实测形态为 hex UUID（`2f099609-47e9-4c3e-bfeb-072ccd609a03`），
//!   与 `AGENTS.md` §4.4「ID 字符集限制为 `[A-Za-z0-9_]`」规则不兼容；
//! - 本类型是**唯一**豁免 §4.4 的 ID 类型，独立 newtype、不与 `Id(String)` 混用；
//! - 编译器层面拒绝 `Place.id: Id` 与 `Place.overture_id: OvertureId` 互换。
//!
//! 撤销条款：一旦 Overture 改为发行可读字符串 ID，本豁免作废，由新 ADR 撤销。
//!
//! 字段命名（ADR-0007 §待裁 (f) 已决议）：**`overture_id`**。

use std::fmt;

/// Overture Maps 的 GERS 全局唯一标识符。
///
/// 实测样本：Space Needle 的 `OvertureId` = `2f099609-47e9-4c3e-bfeb-072ccd609a03`。
///
/// 字符集豁免范围（ADR-0007 UUID 例外条款）：
/// - 仅 `OvertureId` 一种类型豁免 `AGENTS.md` §4.4 约束；
/// - `Id` / `PlaceId` / `EventId` 等全部领域 ID 仍受 §4.4 约束。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OvertureId(String);

impl OvertureId {
    /// 从字符串构造。返回的规则格式：标准 hex UUID（8-4-4-4-12）。
    pub fn new(s: impl Into<String>) -> Result<Self, OvertureIdError> {
        let s = s.into();
        if !is_valid_uuid_format(&s) {
            return Err(OvertureIdError::InvalidFormat(s));
        }
        Ok(Self(s))
    }

    /// 返回内部字符串切片（小写 hex UUID）。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OvertureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for OvertureId {
    type Err = OvertureIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Overture ID 格式错误。
#[derive(Debug, thiserror::Error)]
pub enum OvertureIdError {
    #[error("OvertureId 必须是 hex UUID 形式 (8-4-4-4-12)，得到: {0:?}")]
    InvalidFormat(String),
}

/// 标准 hex UUID 格式校验：长度 36、位置 8/13/18/23 为 '-'、其余为 hex。
/// 允许小写或大写 hex；不允许大括号、URN 前缀或省略连字符的形式。
fn is_valid_uuid_format(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

// Serde 实现：YAML 序列化为 `overture_id: "2f099609-..."`，
// 便于肉眼与 `PlaceId: city_seattle` 区分。
impl serde::Serialize for OvertureId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for OvertureId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::new(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_uuid_lowercase_is_accepted() {
        let id = OvertureId::new("2f099609-47e9-4c3e-bfeb-072ccd609a03").unwrap();
        assert_eq!(id.as_str(), "2f099609-47e9-4c3e-bfeb-072ccd609a03");
    }

    #[test]
    fn valid_uuid_uppercase_is_accepted() {
        let id = OvertureId::new("2F099609-47E9-4C3E-BFEB-072CCD609A03").unwrap();
        assert_eq!(id.as_str(), "2F099609-47E9-4C3E-BFEB-072CCD609A03");
    }

    #[test]
    fn invalid_uuid_length_is_rejected() {
        assert!(OvertureId::new("2f099609-47e9-4c3e-bfeb").is_err());
        assert!(OvertureId::new("").is_err());
        assert!(OvertureId::new("2f099609-47e9-4c3e-bfeb-072ccd609a033").is_err());
    }

    #[test]
    fn invalid_uuid_separator_is_rejected() {
        // 位置 8 必须是 '-'
        assert!(OvertureId::new("2f099609_47e9-4c3e-bfeb-072ccd609a03").is_err());
    }

    #[test]
    fn invalid_uuid_non_hex_is_rejected() {
        // 'g' 不是 hex
        assert!(OvertureId::new("gf099609-47e9-4c3e-bfeb-072ccd609a03").is_err());
    }

    #[test]
    fn uuid_braced_form_is_rejected() {
        // 不接受大括号或 URN 前缀——只接受裸 hex UUID
        assert!(OvertureId::new("{2f099609-47e9-4c3e-bfeb-072ccd609a03}").is_err());
    }

    #[test]
    fn display_and_from_str_round_trip() {
        let s = "2f099609-47e9-4c3e-bfeb-072ccd609a03";
        let id: OvertureId = s.parse().unwrap();
        assert_eq!(id.to_string(), s);
    }

    #[test]
    fn ordering_is_lexicographic() {
        let a = OvertureId::new("00000000-0000-0000-0000-000000000000").unwrap();
        let b = OvertureId::new("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
        assert!(a < b);
    }

    #[test]
    fn equality_and_hash_use_inner_string() {
        use std::collections::HashSet;
        let a = OvertureId::new("2f099609-47e9-4c3e-bfeb-072ccd609a03").unwrap();
        let b = OvertureId::new("2f099609-47e9-4c3e-bfeb-072ccd609a03").unwrap();
        assert_eq!(a, b);
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }
}