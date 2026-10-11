//! 单位（Unit）维度：**在地图上独立运动的主体**（ADR-0004）。
//!
//! ## 为什么需要 Unit
//!
//! 旧模型（基线 §1.6 原文）把「谁在动」含糊地写成 `Track.owner: Id`（注释为「角色或势力」），
//! 并把运动**归给事态节点**。2026-10-11 用户裁定推翻了这一层：
//!
//! - **运动先于事态存在**——一支队伍在地图上走，不因为「被写进某个事态」才开始动；
//! - 事态只是**归纳容器**（把若干 `Event` / `Unit` 收拢到一起），**引用**运动，不**拥有**运动。
//!
//! 因此本项目的归属裁定为：
//!
//! | 关系 | 形态 |
//! |---|---|
//! | `Track` 的动者 | **`Track.unit: Id`**（指向 `Unit`），不是含糊的「角色或势力」 |
//! | 事态与轨迹 | **多对多引用**——`SituationStep.tracks: Vec<Id>`，一条轨迹可被多个节点引用 |
//! | 事态与单位 | `Situation.units: Vec<Id>`（归纳容器，见 ADR-0004） |
//!
//! ## Unit 与 Character 的边界（用户裁定 P2：两者分离）
//!
//! `Character` 是「**人**」，`Unit` 是「**在地图上移动的主体**」。不是「角色变成单位」，
//! 而是 Unit **引用**角色——避免同一个角色在两处各存一份（两个真相）。
//!
//! - 单人移动也建一个 Unit，`members` 长度为 1；
//! - 「红一军」这类复合单位，`members` 列出其领导人物。

use crate::id::Id;

/// 单位。对应 `units/<id>.yaml`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Unit {
    pub id: Id,
    pub name: String,
    /// 归属主体（角色或势力）。**可无主**——如「中央红军」不隶属任何个人。
    ///
    /// 与 `members` 的分工：
    /// - `owner` = 这个单位**属于谁**（人 / 势力）；
    /// - `members` = 这个单位**由谁组成**（角色列表）。
    ///
    /// 两者都只是引用，不复制被引用对象的内容（原则四）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<Id>,
    /// 成员角色 ID 列表。复合单位的领导 / 成员；单人单位填 1 个。
    #[serde(default)]
    pub members: Vec<Id>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(owner: Option<&str>, members: &[&str]) -> Unit {
        Unit {
            id: Id::new_unchecked("unit_red1"),
            name: "红一军".into(),
            owner: owner.map(Id::new_unchecked),
            members: members.iter().map(|s| Id::new_unchecked(*s)).collect(),
        }
    }

    #[test]
    fn unit_with_members_records_them_in_order() {
        let u = unit(Some("faction_red"), &["char_a", "char_b"]);
        let names: Vec<&str> = u.members.iter().map(|m| m.as_str()).collect();
        assert_eq!(names, vec!["char_a", "char_b"]);
    }

    #[test]
    fn unit_may_be_owner_less() {
        // 「中央红军」不隶属任何个人 —— owner 必须是可选的。
        let u = unit(None, &["char_a"]);
        assert!(u.owner.is_none());
        assert_eq!(u.members.len(), 1, "单人单位 members 长度为 1");
    }

    #[test]
    fn unit_may_have_no_members() {
        // 「后卫阻击部队」这种只作为整体运动、未指定具体人物的单位必须合法。
        let u = unit(Some("faction_red"), &[]);
        assert!(u.members.is_empty());
        assert!(u.owner.is_some());
    }
}
