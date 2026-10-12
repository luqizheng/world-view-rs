//! # wv-core —— World View 数据层
//!
//! 纯 Rust 领域模型，**不依赖 Bevy**。
//!
//! 设计约束（见 `AGENTS.md`）：
//! - YAML 是 source of truth，Bevy ECS 只是投影。
//! - 本 crate 不得引入任何渲染/UI 依赖。
//! - 历史状态以「带时间区间的状态片段」存储，不存「当前值」。

pub mod color;
pub mod distance;
pub mod encounter;
pub mod error;
pub mod faction;
pub mod geometry;
pub mod id;
pub mod map;
pub mod narration;
pub mod situation;
pub mod time;
pub mod track;
pub mod unit;

pub use character::Character;
pub use encounter::{detect_encounter_hints, EncounterHint, EncounterInput, HintSeverity};
pub use error::{CoreError, CoreResult};
pub use event::{Event, EventLocation};
pub use faction::{factions_at, Faction};
pub use id::{Id, IdError};
pub use narration::Narration;
pub use situation::{Situation, SituationStep};
pub use time::{Calendar, TimePoint, TimeSpan};
pub use track::{Track, Waypoint};
pub use unit::Unit;

/// 工程级元数据。对应工程根目录的 `world.yaml`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct WorldMeta {
    /// 工程显示名称。
    pub name: String,
    /// 工程自身版本（作者可随意递增）。
    #[serde(default = "default_version")]
    pub version: String,
    /// 数据 schema 版本，用于迁移判定。见 `AGENTS.md` 的 Schema 迁移纪律。
    ///
    /// 当前版本为 **2**（基线 §2.16：时间模型重做 + 事态/轨迹落地）。
    /// v1 的时间模型是「纪元 + 年 + 日」字段式，v2 改为天数计数器；
    /// **不做自动迁移**（信息不足时无法可靠换算），读到 v1 时给出可操作的错误指引。
    pub schema_version: u32,
    /// 地图树根节点 ID。
    pub root_map: Id,
    /// 时间轴覆盖范围。
    pub timeline_range: TimeSpan,
}

fn default_version() -> String {
    "0.1".to_string()
}

/// 顶层世界聚合：全部维度的统一入口。
///
/// **关于可变性**（2026-10-11 用户裁定，见 `AGENTS.md` §2.1）：
/// 本类型是**数据聚合**，不是「不可变」的承诺。`wv-app` 持有一个**可变工作副本**
/// 并在其上驱动编辑流程，最终经 `wv-edit` 的 `EditCommand` 落到 YAML；
/// **渲染层**对它的访问仍是只读的（`WorldProjection`）。
/// 一句话：**「谁有权改」由持有者与编辑链决定，不由本类型决定。**
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
    /// 事态（**归纳容器**：收拢事件与单位，基线 §1.1 / §2.7）。
    pub situations: Vec<situation::Situation>,
    /// 单位（在地图上独立运动的主体，ADR-0004）。
    pub units: Vec<unit::Unit>,
    /// 轨迹（运动的主体是 `unit`；事态节点通过 ID 引用，见 ADR-0004）。
    pub tracks: Vec<track::Track>,
    /// 旁白（独立叙事实体，事件只存 ID 引用；2026-01 用户裁定）。
    pub narrations: Vec<narration::Narration>,
    /// 势力本体（路线图 S12，2026-10-12）。
    ///
    /// §4.5 原则「每实体一文件」：每个势力一个 `factions/<id>.yaml`。
    /// 势力占据地图的几何**不**内联在这里——见下方的 `faction_geometry`。
    pub factions: Vec<faction::Faction>,
    /// 势力的几何（含时间区间，§4.1 P6/P7）。
    ///
    /// `GeometrySpan.owner` 引用 `factions` 中的 `Faction.id`。
    /// 想要「某时刻某地图上所有势力 + 多边形」走 [`crate::factions_at`]。
    pub faction_geometry: Vec<map::GeometrySpan>,
    /// 主地形栅格网格。
    ///
    /// 注意：`wv-core` **不认识** `wv_terrain::TerrainGrid`（不依赖 wv-terrain，基线 §2.1）。
    /// 因此这里存的是**该网格的二进制投影** `terrain.bin` 的原始单元格数据，
    /// 而不是 `TerrainGrid` 类型本身。`wv-io` 负责双向转换，`wv-edit` 在写命令时
    /// 把网格投影回本字段。
    ///
    /// 之所以放在 `World` 上而非 ECS：地形是 P3 唯一进二进制的一等数据，
    /// 必须与其它领域数据同处一个聚合快照中，才能被 Undo/快照一致地处理。
    pub terrain: Option<terrain::TerrainData>,
}

/// 栅格地形的**中立数据表示**（与 `wv-terrain` 解耦的桥）。
///
/// `wv-io` 在 `TerrainData <-> TerrainGrid` 之间转换：
/// - 读写 `terrain.bin` 时用 `TerrainData`；
/// - 编辑时 `wv-edit` 把它转成 `TerrainGrid`，改完再转回。
pub mod terrain {
    /// 一格地形（与 `wv_terrain::TerrainCell` 布局一致）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
    pub struct TerrainCellData {
        pub height: i16,
        pub kind: u8,
    }

    /// 栅格地形数据。
    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    pub struct TerrainData {
        pub width: u32,
        pub height: u32,
        /// 行优先的单元格。
        pub cells: Vec<TerrainCellData>,
    }

    impl TerrainData {
        pub fn cell_count(&self) -> usize {
            self.cells.len()
        }
    }
}

pub mod character;
pub mod event;
pub mod relation;
