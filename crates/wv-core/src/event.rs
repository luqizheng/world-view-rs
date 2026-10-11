//! 事件维度。每实体一文件（§5.4），对应 `timeline/events/<id>.yaml`。
//!
//! 事件是**状态变化的驱动源**：事件产生状态变化，
//! 状态变化反过来约束事件的合理性（§3.4）。

use crate::{id::Id, time::TimeSpan};

/// 历史事件。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    pub id: Id,
    pub name: String,
    /// 发生时间。用 `TimeSpan` 而非 `TimePoint`：
    /// 需要支持「持续一个月」「持续三年」这类事件，而非只有瞬时点。
    ///
    /// 半开语义 `[from, to)`：事件在其 `span` 内「正在发生」。
    #[serde(flatten)]
    pub span: TimeSpan,
    /// 发生地点。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<Id>,
    /// 参与人物。
    #[serde(default)]
    pub participants: Vec<Id>,
    /// 事件结果描述。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// 本事件结束后触发的状态变化。
    #[serde(default)]
    pub effects: Vec<Effect>,
}

/// 事件引发的状态变化。
///
/// 设计取舍：这里刻意保持**声明式**而非脚本式。
/// 脚本式（内嵌表达式求值）更灵活，但会让 YAML 不可静态校验、
/// 也会让「事件合理性反向约束」无从实现。声明式先覆盖高频场景。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    /// 移动角色到某地点。
    MoveCharacter { character: Id, to: Id },
    /// 改变角色身份。
    SetTitle { character: Id, title: String },
    /// 势力在某时间点起控制某区域。`at` 是天数计数器。
    ClaimRegion { region: Id, at: i64 },
    /// 自定义效果，保留扩展口，但需在 GLOSSARY 登记语义。
    Custom { note: String },
}
