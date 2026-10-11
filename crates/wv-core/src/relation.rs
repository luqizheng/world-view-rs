//! 人物关系网。对应 `relations/graph.yaml`。

use crate::id::Id;

/// 角色之间的关系。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Relation {
    pub a: Id,
    pub b: Id,
    /// 关系类型，如 `kinship` / `mentor` / `rival`。
    pub kind: String,
    /// 关系强度或倾向，取值域由上层语义定义。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// 关系网整体。以邻接表形式便于查询。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RelationGraph {
    #[serde(default)]
    pub relations: Vec<Relation>,
}

impl RelationGraph {
    /// 查询与某角色相关的全部关系。
    pub fn of(&self, who: &Id) -> Vec<&Relation> {
        self.relations
            .iter()
            .filter(|r| &r.a == who || &r.b == who)
            .collect()
    }
}
