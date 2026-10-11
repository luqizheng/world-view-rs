//! # wv-core —— World View 数据层
//!
//! 纯 Rust 领域模型，**不依赖 Bevy**。
//!
//! 设计约束（见 `AGENTS.md`）：
//! - YAML 是 source of truth，Bevy ECS 只是投影。
//! - 本 crate 不得引入任何渲染/UI 依赖。
//! - 历史状态以「带时间区间的状态片段」存储，不存「当前值」。

pub mod distance;
pub mod error;
pub mod geometry;
pub mod id;
pub mod map;
pub mod time;

pub use error::{CoreError, CoreResult};
pub use id::{Id, IdError};
pub use time::{TimePoint, TimeSpan};

/// 工程级元数据。对应工程根目录的 `world.yaml`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct WorldMeta {
    /// 工程显示名称。
    pub name: String,
    /// 工程自身版本（作者可随意递增）。
    #[serde(default = "default_version")]
    pub version: String,
    /// 数据 schema 版本，用于迁移判定。见 `AGENTS.md` 的 Schema 迁移纪律。
    pub schema_version: u32,
    /// 地图树根节点 ID。
    pub root_map: Id,
    /// 时间轴覆盖范围。
    pub timeline_range: TimeSpan,
}

fn default_version() -> String {
    "0.1".to_string()
}

/// 顶层世界聚合：五个维度的统一入口。
///
/// 注意：这是一个**只读快照**的聚合视图，而非可变的全局单例。
/// 编辑操作通过 `wv-edit` 的命令产生新的聚合，而非原地修改。
#[derive(Debug, Clone, Default)]
pub struct World {
    /// 工程元数据。
    pub meta: Option<WorldMeta>,
    /// 地图层级树。
    pub maps: Vec<map::MapNode>,
    /// 时间轴定义。
    pub timeline: Option<time::Timeline>,
    /// 人物。
    pub characters: Vec<character::Character>,
    /// 事件。
    pub events: Vec<event::Event>,
    /// 关系网。
    pub relations: Vec<relation::Relation>,
    /// 距离图。
    pub distances: distance::DistanceGraph,
}

pub mod character;
pub mod event;
pub mod relation;
