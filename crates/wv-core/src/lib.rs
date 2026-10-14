//! # wv-core —— World View 数据层
//!
//! 纯 Rust 领域模型，**不依赖 Bevy**。
//!
//! 设计约束（见 `AGENTS.md`）：
//! - YAML 是 source of truth，Bevy ECS 只是投影。
//! - 本 crate 不得引入任何渲染/UI 依赖。
//! - 历史状态以「带时间区间的状态片段」存储，不存「当前值」。
//!
//! ## 模块导航表（按"被谁找"分组；目录化分组，主路径尽量保顶层可达）
//!
//! | 目录 / 模块 | 用途 |
//! |---|---|
//! | [`base`] | 通用基础（`Id` / `CoreError` / `Point` / `Color`）。 |
//! | [`time`] / [`track`] | 时间模型 + 轨迹（`TimeSpan` / `Timeline` / `Track` / `TimePoint`）。 |
//! | [`map`] / [`coord`] / [`distance`] / [`unit`] / [`geometry`] | 地图层级、坐标系、行程时间计算、单位。 |
//! | [`model`] | 领域模型（人物 / 事件 / 关系 / 势力）。 |
//! | [`derived`] | 派生层（相遇提示 / 事态 / 旁白）。 |
//! | [`source`] | 外部数据来源（街道 / Overture ID / 外部地理数据源）。 |
//!
//! 顶层：构建入口 [`World`] + 元数据 [`WorldMeta`]，时间区间化由 [`time`] / [`track`] 提供。

use std::collections::BTreeMap;

pub mod base;
pub mod derived;
pub mod map;
pub mod model;
pub mod source;
pub mod time;

// **保路径重导出**（B1 设计，详见 `docs/adr/README.md` 的「模块路径对照」）。
// AGENTS.md §3 与大量文档直接引用 `wv_core::time::TimelineQuery` /
// `wv_core::map::MapNode` / `wv_core::track` 等顶层路径；
// 重新导出模块本身让 `wv_core::xxx` 仍可达（**与下面 `pub use ::Type` 互不冲突**，
// 前者让 `mod`，后者让 `Type`）。
pub use base::color;
pub use base::error;
pub use base::geometry;
pub use base::id;
pub use derived::encounter;
pub use derived::narration;
pub use derived::situation;
pub use map::coord;
pub use map::distance;
pub use map::unit;
pub use model::character;
pub use model::event;
pub use model::faction;
pub use model::relation;
pub use source::geographic_source;
pub use source::label;
pub use source::measurement;
pub use source::overture_id;
pub use source::street;
pub use time::track;

// 顶层类型重导出（与上面的「保模块路径」是**两个名字空间**，不冲突）。
// 这些让 `wv_core::Color` / `wv_core::Id` / `wv_core::TimeSpan` 等直接可达，
// 不必写 `wv_core::base::color::Color`。
pub use base::color::Color;
pub use base::error::{CoreError, CoreResult};
pub use base::geometry::Point;
pub use base::id::{Id, IdError};
pub use derived::encounter::{detect_encounter_hints, EncounterHint, EncounterInput, HintSeverity};
pub use derived::narration::Narration;
pub use derived::situation::{Situation, SituationStep};
pub use map::coord::{resolve_frame, resolve_frame_in, FrameNode, MapFrame};
pub use map::unit::Unit;
pub use model::character::Character;
pub use model::event::{Event, EventLocation};
pub use model::faction::{factions_at, Faction};
pub use source::geographic_source::{GeographicSourceV1, LatLonBbox};
pub use source::label::{label_visible_at, EntityKind, LabelLayerConfig, ZoomTier};
pub use source::measurement::{Measurement, MeasurementKind, MeasurementOverlay};
pub use source::overture_id::{OvertureId, OvertureIdError};
pub use source::street::{RoadClass, StreetLayer, StreetSegment};
pub use time::track::{Track, Waypoint};
pub use time::{Calendar, TimePoint, TimeSpan};

/// 当前选中的实体（ADR-0016 §2.7）。
///
/// 标签层点击派发的目标——`Place` / `Region` / `Faction` / `StreetSegment` /
/// `Event` 之一。该结构只携带定位信息（kind + id），**不**携带任何渲染态或
/// 投影态；详情面板用 `kind` 决定读哪张表（`World.places` / `World.factions` /
/// ...），用 `id` 定位。
///
/// ## 与 World 持久化的关系
///
/// `SelectedEntity` 是**会话状态**——存于 `wv_app::state::WorldProjection.selection`，
/// **不**进 `World`、不落盘、不经 `EditCommand` 流水线（与 undo 隔离：
/// 撤销实体编辑不会一并撤销"选谁"——选谁不是世界数据，是 UI 焦点）。
///
/// ## 何时清除
///
/// - 用户点击空白处 → `Selection::None`
/// - 切换工程 / 关闭工程 → `Selection::None`（id 跨工程无意义）
/// - 当前不被自动清除（"上次选中的实体"不持久化，但当前会话内会保留）
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SelectedEntity {
    /// 实体类型（决定查哪张表）。
    pub kind: EntityKind,
    /// 实体 ID（在所属表内唯一）。
    pub id: Id,
}

impl SelectedEntity {
    /// 构造一个选中引用。
    pub fn new(kind: EntityKind, id: Id) -> Self {
        Self { kind, id }
    }
}

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
    pub characters: Vec<model::character::Character>,
    /// 事件。
    pub events: Vec<model::event::Event>,
    /// 关系网。
    pub relations: Vec<model::relation::Relation>,
    /// 距离图。
    pub distances: map::distance::DistanceGraph,
    /// 事态（**归纳容器**：收拢事件与单位，基线 §1.1 / §2.7）。
    pub situations: Vec<derived::situation::Situation>,
    /// 单位（在地图上独立运动的主体，ADR-0004）。
    pub units: Vec<map::unit::Unit>,
    /// 轨迹（运动的主体是 `unit`；事态节点通过 ID 引用，见 ADR-0004）。
    pub tracks: Vec<time::track::Track>,
    /// 旁白（独立叙事实体，事件只存 ID 引用；2026-01 用户裁定）。
    pub narrations: Vec<derived::narration::Narration>,
    /// 势力本体（路线图 S12，2026-10-12）。
    ///
    /// §4.5 原则「每实体一文件」：每个势力一个 `factions/<id>.yaml`。
    /// 势力占据地图的几何**不**内联在这里——见下方的 `faction_geometry`。
    pub factions: Vec<model::faction::Faction>,
    /// 势力的几何（含时间区间，§4.1 P6/P7）。
    ///
    /// `GeometrySpan.owner` 引用 `factions` 中的 `Faction.id`。
    /// 想要「某时刻某地图上所有势力 + 多边形」走 [`crate::factions_at`]。
    pub faction_geometry: Vec<map::GeometrySpan>,
    /// 各图各自的**地形栅格**，键 = [`map::MapNode::id`]。
    ///
    /// 注意：`wv-core` **不认识** `wv_terrain::TerrainGrid`（不依赖 wv-terrain，基线 §2.1）。
    /// 因此这里存的是**该网格的二进制投影** `terrain.bin` 的原始单元格数据，
    /// 而不是 `TerrainGrid` 类型本身。`wv-io` 负责双向转换，`wv-edit` 在写命令时
    /// 把网格投影回本字段。
    ///
    /// 之所以放在 `World` 上而非 ECS：地形是 P3 唯一进二进制的一等数据，
    /// 必须与其它领域数据同处一个聚合快照中，才能被 Undo/快照一致地处理。
    ///
    /// **追加（2026-10-13，Phase 2）：单张 → 按图索引。**
    /// 原先这里是 `terrain: Option<TerrainData>`——整个世界只能有一张栅格，
    /// 于是「城市图有自己的底图」在数据模型里**无处可放**（`wv-io` 的加载器
    /// 也只能取第一张声明了 `raster` 的图）。改为按图 ID 索引后：
    ///
    /// - 一张图 = 0 或 1 份栅格，与 `MapNode.raster` 一一对应（`raster` 只存
    ///   **元数据**：路径/尺寸/编码/校验和；单元格数据在这里，仍是单一真源）；
    /// - 它仍是 `World` 的字段，所以 Undo / 快照 / 脏路径的语义**原样不变**（P6）；
    /// - 不新增「当前图」这类会随时间/视图变化的字段（P7 不受影响）。
    ///
    /// **驻留策略待裁**（README §12-19：全部常驻内存？还是按当前图懒加载 + 淘汰？）。
    /// 当前实现选**最容易回退**的做法：`wv-io` 加载时把每张声明了 `raster` 的图
    /// 全部读入，不做淘汰。将来引入懒加载时，本字段的**形状不必再改**——
    /// 撤掉的是「何时填、何时清」，不是键值结构。
    pub terrains: BTreeMap<Id, terrain::TerrainData>,

    /// 街道矢量图层文档，键 = **图层 ID**（ADR-0010 待做第 3 项，2026-10-13）。
    ///
    /// 与 [`World::terrains`] 同构：磁盘真源仍是各 `Layer.source` 指向的
    /// YAML 文件（几何不进 `index.yaml`，ADR-0010 决策 ⑤）；`World` 持有
    /// 运行副本只为让 `wv-edit` 的命令有处可改、Undo 有处可回（P6 语义
    /// 原样不变）。不存「当前图层」这类视图状态（P7）。
    ///
    /// **驻留策略同 §12-19**：加载时全读入、不淘汰——最易回退的形态。
    pub streets: BTreeMap<Id, source::street::StreetLayer>,

    /// 各图的测量叠加，键 = **地图 ID**（不是图层 ID）。
    ///
    /// 与 [`World::streets`] / [`World::terrains`] 同构：磁盘真源是各
    /// `maps/<map_id>/measurements.yaml`（**固定路径**，无需 `Layer.source`
    /// 派生），加载期一次性读入。
    ///
    /// 测量是**工具产物**（不是领域实体），不带 `TimeSpan`——撤销走
    /// `wv-edit` 命令链，不走「区间不覆盖」。作者要清就清、要回退就 Undo
    /// （`AGENTS.md §7.5V2`）。不存「当前图」这类视图状态（P7）。
    pub measurements: BTreeMap<Id, source::measurement::MeasurementOverlay>,
}

impl World {
    /// 取某张图的地形栅格（未声明则 `None`）。
    pub fn terrain_of(&self, map: &Id) -> Option<&terrain::TerrainData> {
        self.terrains.get(map)
    }

    /// 写入某张图的地形栅格，返回被替换掉的旧值（首次写入时为 `None`）。
    pub fn set_terrain(
        &mut self,
        map: Id,
        data: terrain::TerrainData,
    ) -> Option<terrain::TerrainData> {
        self.terrains.insert(map, data)
    }

    /// 取某个街道图层的文档（未加载/未声明则 `None`）。
    pub fn street_of(&self, layer: &Id) -> Option<&source::street::StreetLayer> {
        self.streets.get(layer)
    }

    /// 写入某个街道图层的文档，返回被替换掉的旧值（首次写入时为 `None`）。
    pub fn set_streets(
        &mut self,
        layer: Id,
        doc: source::street::StreetLayer,
    ) -> Option<source::street::StreetLayer> {
        self.streets.insert(layer, doc)
    }

    /// 取某张图的测量叠加（无测量时 `None`）。
    pub fn measurement_of(&self, map: &Id) -> Option<&source::measurement::MeasurementOverlay> {
        self.measurements.get(map)
    }

    /// 取某张图的测量叠加（可变）；不存在时**空文档兜底**——写入便利
    /// （调用方在第一次写之前不必先 `insert`）。
    pub fn measurement_of_mut(&mut self, map: &Id) -> &mut source::measurement::MeasurementOverlay {
        self.measurements.entry(map.clone()).or_default()
    }

    /// 写入某张图的测量叠加，返回被替换掉的旧值（首次写入时为 `None`）。
    pub fn set_measurement(
        &mut self,
        map: Id,
        doc: source::measurement::MeasurementOverlay,
    ) -> Option<source::measurement::MeasurementOverlay> {
        self.measurements.insert(map, doc)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── ADR-0016 §2.7：SelectedEntity ────────────────────────────────────

    #[test]
    fn selected_entity_new_stores_kind_and_id() {
        let id = Id::new("city_qingyun").unwrap();
        let sel = SelectedEntity::new(EntityKind::Place, id.clone());
        assert_eq!(sel.kind, EntityKind::Place);
        assert_eq!(sel.id, id);
    }

    #[test]
    fn selected_entity_equality_uses_both_fields() {
        let id = Id::new("x").unwrap();
        let a = SelectedEntity::new(EntityKind::Place, id.clone());
        let b = SelectedEntity::new(EntityKind::Place, id.clone());
        let c = SelectedEntity::new(EntityKind::Faction, id.clone());
        let d = SelectedEntity::new(EntityKind::Place, Id::new("y").unwrap());
        assert_eq!(a, b, "同 kind + 同 id 应相等");
        assert_ne!(a, c, "不同 kind 不等");
        assert_ne!(a, d, "不同 id 不等");
    }

    #[test]
    fn selected_entity_is_hashable() {
        use std::collections::HashSet;
        let id = Id::new("x").unwrap();
        let mut set = HashSet::new();
        set.insert(SelectedEntity::new(EntityKind::Place, id.clone()));
        set.insert(SelectedEntity::new(EntityKind::Place, id));
        assert_eq!(set.len(), 1, "同 kind+id 应去重为 1");
    }
}
