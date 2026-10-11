//! 人物维度。每实体一文件（§5.4），对应 `characters/<id>.yaml`。
//!
//! ## 生卒：都是可选
//!
//! `born_at` / `died_at` **都是天数计数器**（`TimePoint`），
//! 「康熙帝 1654-1722」里的年份由可插拔 `Calendar` 派生，不是存储值。
//!
//! 两者都**可留白**——留白是创作手段，不是数据缺失。

use std::collections::HashMap;

use crate::{id::Id, time::TimeSpan};

/// 角色。基础信息 + 性格 + 自定义属性。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Character {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// 性格标签。结构化程度待定，先用字符串。
    #[serde(default)]
    pub traits: Vec<String>,
    /// 自由扩展属性。刻意用 map 而非固定字段：
    /// 不同作者的设定维度差异极大，硬编码字段会逼出「用不上但必须填」的噪声。
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    /// 出生时间（用于推算年龄）。天数计数器。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub born_at: Option<crate::time::TimePoint>,
    /// 卒年（**可选**）。天数计数器，语义同 `born_at`。
    ///
    /// 2026-10-11 用户裁定：人物视图要显示「康熙帝(1654-1722)」，
    /// 需要卒年。**刻意用 `Option` 而非必填**——绝大多数配角没有卒年，
    /// 必填会逼出「填 0 或填个假年份」的噪声（同 `attributes` 用 map 的理由）。
    ///
    /// 显示层注意：`born_at` / `died_at` 是**天数计数器**，
    /// 「1654 / 1722」这种年份由可插拔 `Calendar` 派生，不是存储值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub died_at: Option<crate::time::TimePoint>,
    /// 状态片段：位置、身份、能力等随时间变化的量。
    ///
    /// 注意「状态随时间变化」在本模型里的表达方式：
    /// **不存 `current_position` 字段**，只存区间化的 `state_spans`。
    /// 原则七。
    #[serde(default)]
    pub state_spans: Vec<CharacterStateSpan>,
}

/// 角色在某段时间区间内的一段状态。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CharacterStateSpan {
    #[serde(flatten)]
    pub span: TimeSpan,
    /// 该段时间内的所在地点。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Id>,
    /// 该段时间内的身份/头衔。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 该段时间内的能力/等级。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power: Option<f32>,
}
