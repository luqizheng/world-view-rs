//! 人物维度。每实体一文件（§5.4），对应 `characters/<id>.yaml`。

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
    /// 出生时间（用于推算年龄）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub born_at: Option<crate::time::TimePoint>,
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
