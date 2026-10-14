//! 旁白：独立的一等叙事实体（2026-01 用户裁定「一并实现」）。
//!
//! ## 为什么旁白不挂在事件上而要独立成实体
//!
//! 1. **复用**：同一段旁白（如「铁路国有，举国哗然」）可能被多个节点引用；
//! 2. **资产化**：旁白可携带 `avatar`（旁白者头像等图片资产），资产路径跟着实体走；
//! 3. **因果链的载体**：辛亥样例的「A 导致 B」不用箭头字段表达（遵守 ADR-0002
//!    时间派生顺序、K2「因果依赖现在就砍」），而是用旁白文案讲述因果——
//!    旁白因此成为**演示层因果叙事的最小实体**。
//!
//! ## 与 Event 的关系
//!
//! `Event.narrations: Vec<Id>` **只存引用**（P4 同款纪律：只引用，不复制）。
//! 悬空引用由 `wv-io` 的 `diagnose` 检出（只提示，不阻止保存，§7.5 V1）。
//!
//! ## 播放节奏
//!
//! 旁白**不自带播放节奏**：暂停时长已由 `SituationStep.pause_secs` 承载（最小
//! 样例已有先例）。旁白只负责「说什么、谁说、配什么图」，何时停由事态层决定。

use crate::base::id::Id;

/// 一段旁白。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Narration {
    /// 全局唯一 ID，如 `nar_railway_nationalization`。
    pub id: Id,
    /// 旁白名（编辑器列表显示用），如「铁路国有诏下」。
    pub name: String,
    /// 旁白正文（因果叙事的载体）。
    pub text: String,
    /// 旁白者/配图资产路径（工程相对路径，图片资产）。可缺省。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}
