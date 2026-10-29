//! attribute 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

// ── 事件属性命令（统一 Inspector · 2026-10-22）───────────────────────────────

use wv_core::model::event::EventLocation;

/// 登记事件（UX-002 / round-2 审计 P0：事件此前只能手编 YAML 创建）。
///
/// 整个 [`Event`] 由调用方构造好传入（对话框负责字段与 ID 生成），
/// 本命令只做**登记**这件事：查重 → 追加 → 报脏路径。
/// `revert` 按 ID 移除——只删本命令追加的那一条，不动其他事件。
///
/// ## 为什么整实体传入而非逐字段
///
/// 与 [`UpdateEventCommand`] 的「窄快照逐字段」相反：创建没有「改动面」
/// 可言——新实体的全部字段都是本命令引入的，整实体进、整实体出，
/// 撤销即完全消失（登记过 = 没登记过），不存在误退别人改动的可能。
#[derive(Debug, Clone)]
pub struct AddEventCommand {
    /// 待登记的事件（含 ID；与既有事件撞 ID 时 `apply` 报错）。
    pub event: wv_core::model::event::Event,
}

impl AddEventCommand {
    pub fn new(event: wv_core::model::event::Event) -> Self {
        Self { event }
    }
}

impl EditCommand for AddEventCommand {
    fn label(&self) -> String {
        format!("登记事件「{}」", self.event.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.events.iter().any(|e| e.id == self.event.id) {
            return Err(EditError::Precondition(format!(
                "事件 ID 已存在：{}",
                self.event.id
            )));
        }
        world.events.push(self.event.clone());
        Ok(vec!["timeline/events".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = world.events.len();
        world.events.retain(|e| e.id != self.event.id);
        if world.events.len() == before {
            return Err(EditError::TargetMissing(format!("Event {}", self.event.id)));
        }
        Ok(())
    }
}

/// 事件属性的**逐字段**快照。
///
/// 只含本命令会改的六个字段——**不含** `effects` / `narrations`。
/// 理由：这两个字段不在本命令的编辑面内，但可能有别的路径改它们
/// （将来的事件效果编辑器等）。若 revert 写回整实体快照，会把那些
/// 改动一并误退——「撤销一次改名，把作者加的效果也抹了」是实打实的
/// 数据丢失。故快照必须**窄**到本命令自己的改动面。
#[derive(Debug, Clone)]
struct EventFields {
    name: String,
    span: wv_core::time::TimeSpan,
    location: Option<EventLocation>,
    participants: Vec<wv_core::Id>,
    narrative: Option<String>,
    outcome: Option<String>,
}

/// 改事件属性（名称 / 时间跨度 / 地点 / 参与者 / 叙述 / 结果）。
///
/// 统一 Inspector 的「属性」页签用它写回事件字段
/// （[2026-10-22 统一 Inspector 设计](../../superpowers/specs/2026-10-22-unified-inspector-panel-design.md)）。
///
/// ## 不含的字段
///
/// `effects` / `narrations` 本轮**不提供编辑入口**（spec §11 Q1），
/// 故不进快照——`revert` 不会碰它们（见 [`EventFields`] 的理由）。
///
/// ## 时间语义
///
/// `span` 是事件自身的时间区间，改它是「改事件的发生时间」——
/// 事件本来就是**带区间的实体**，不是「当前值」字段，故不违反 P7。
/// 但**不允许**在事件里加 `current_day` 之类的当前值字段。
///
/// ## 回滚
///
/// `apply` 缓存 `EventFields`（窄快照），`revert` 只把这六个字段写回，
/// 逐字段对称——apply 改了哪些，revert 就退哪些，不多不少。
#[derive(Debug)]
pub struct UpdateEventCommand {
    /// 目标事件 ID。
    pub id: wv_core::Id,
    /// 新名称。
    pub new_name: String,
    /// 新时间跨度。
    pub new_span: wv_core::time::TimeSpan,
    /// 新发生地点。
    pub new_location: Option<EventLocation>,
    /// 新参与者名单。
    pub new_participants: Vec<wv_core::Id>,
    /// 新过程描述。
    pub new_narrative: Option<String>,
    /// 新结果描述。
    pub new_outcome: Option<String>,
    before: std::sync::Mutex<Option<EventFields>>,
}

impl UpdateEventCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: wv_core::Id,
        new_name: String,
        new_span: wv_core::time::TimeSpan,
        new_location: Option<EventLocation>,
        new_participants: Vec<wv_core::Id>,
        new_narrative: Option<String>,
        new_outcome: Option<String>,
    ) -> Self {
        Self {
            id,
            new_name,
            new_span,
            new_location,
            new_participants,
            new_narrative,
            new_outcome,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateEventCommand {
    fn label(&self) -> String {
        format!("改事件「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let e = world
            .events
            .iter_mut()
            .find(|e| e.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Event {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(EventFields {
            name: e.name.clone(),
            span: e.span,
            location: e.location.clone(),
            participants: e.participants.clone(),
            narrative: e.narrative.clone(),
            outcome: e.outcome.clone(),
        });
        e.name = self.new_name.clone();
        e.span = self.new_span;
        e.location = self.new_location.clone();
        e.participants = self.new_participants.clone();
        e.narrative = self.new_narrative.clone();
        e.outcome = self.new_outcome.clone();
        Ok(vec!["timeline/events".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateEvent 尚未 apply".into()))?;
        let e = world
            .events
            .iter_mut()
            .find(|e| e.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Event {}", self.id)))?;
        // 逐字段写回——**不**整实体替换：effects / narrations 不在快照里，
        // 整实体替换会把它们一并退掉（见 EventFields 文档）。
        e.name = prev.name;
        e.span = prev.span;
        e.location = prev.location;
        e.participants = prev.participants;
        e.narrative = prev.narrative;
        e.outcome = prev.outcome;
        Ok(())
    }
}

// ── 事实地点与地界命令（theme overlay） ─────────────────────────────────────

use wv_core::distance::Place;

/// 改地点属性（名称 / 呈现级别 / 锚点）。
///
/// 统一 Inspector 的「属性」页签用它写回地点字段
/// （[2026-10-22 统一 Inspector 设计](../../superpowers/specs/2026-10-22-unified-inspector-panel-design.md)）。
///
/// ## 锚点是**条件可写**的
///
/// `new_x` / `new_y` 为 `None` 表示「不改」——**不是**「写成 None」。
/// 之所以要这个区分：`Place.x/y/w/h` 对**有地界环**的地点是**派生量**
/// （`Place::fit_anchor_to_ring` 从环重算质心与包围盒，
/// `UpdatePlaceTerritoryCommand` 与 `UpdatePlaceLineCommand` 都会触发重算）。
/// 让 Inspector 直接改它们会造出「两个真相」——改了 x/y，下次重生成边界
/// 又被环拟合覆盖。故表单层负责按「有无地界环」决定传不传值；
/// 本命令只忠实执行「传了就写，不传就不动」。
///
/// ## 不含的字段
///
/// - `w` / `h`：范围是包围盒的推论，与锚点同源，由环拟合一并决定。
/// - `biome`：群系的真值是「标记包络」（`PlaceBiomeMarkerCommand` 会改地形格），
///   自由改会与地形格脱钩——它是那个命令唯一的写入口。
///
/// ## 回滚
///
/// `apply` 缓存旧 `Place` 全字段，`revert` 原样写回（与
/// `UpdateRegionCommand` 同款：快照整实体，保证 apply 改了哪些字段、
/// revert 就退哪些，不漏）。
#[derive(Debug)]
pub struct UpdatePlaceCommand {
    /// 目标地点 ID。
    pub id: wv_core::Id,
    /// 新名称。
    pub new_name: String,
    /// 新呈现级别（`None` = 不改）。
    pub new_rank: Option<u8>,
    /// 新锚点 X（`None` = 不改）。
    pub new_x: Option<f32>,
    /// 新锚点 Y（`None` = 不改）。
    pub new_y: Option<f32>,
    before: std::sync::Mutex<Option<Place>>,
}

impl UpdatePlaceCommand {
    pub fn new(
        id: wv_core::Id,
        new_name: String,
        new_rank: Option<u8>,
        new_x: Option<f32>,
        new_y: Option<f32>,
    ) -> Self {
        Self {
            id,
            new_name,
            new_rank,
            new_x,
            new_y,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdatePlaceCommand {
    fn label(&self) -> String {
        format!("改地点「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(p.clone());
        p.name = self.new_name.clone();
        if let Some(rank) = self.new_rank {
            p.rank = Some(rank);
        }
        if let Some(x) = self.new_x {
            p.x = Some(x);
        }
        if let Some(y) = self.new_y {
            p.y = Some(y);
        }
        Ok(vec!["distances/places".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdatePlace 尚未 apply".into()))?;
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        *p = prev;
        Ok(())
    }
}

/// 设置单个地点的 POI 类别（ADR-0064）。
///
/// **为什么单独一条命令而不并进 [`UpdatePlaceCommand`]**：后者是「整实体补丁」，
/// 构造点分布在面板 / HTTP skill / 测试里，加字段要全量改；而「选一个类别」
/// 是独立的作者意图，撤销粒度也应当是**一次选择**而不是一次表单提交。
///
/// `kind = None` 表示**清空**：渲染回退默认红点，YAML 不再写 `kind` 行
/// （`skip_serializing_if`）。
#[derive(Debug)]
pub struct SetPlaceKindCommand {
    /// 目标地点 ID。
    pub id: wv_core::Id,
    /// 新类别；`None` = 清空。
    pub kind: Option<wv_core::distance::PoiKind>,
    before: std::sync::Mutex<Option<Place>>,
}

impl SetPlaceKindCommand {
    pub fn new(id: wv_core::Id, kind: Option<wv_core::distance::PoiKind>) -> Self {
        Self {
            id,
            kind,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetPlaceKindCommand {
    fn label(&self) -> String {
        format!("改地点类别「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(p.clone());
        p.kind = self.kind;
        Ok(vec!["distances/places".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("SetPlaceKind 尚未 apply".into()))?;
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        *p = prev;
        Ok(())
    }
}

/// 改写某地点某个地界 span 的多边形环，并重拟合锚点。
///
/// ## 为什么定位键是 `(map, owner, span.from)`
///
/// 同一地点在同一张图上可以有多个时间区间的地界（边界演化 = 追加 span，
/// P6/P7），因此 `owner` 不足以唯一定位；`from` 是区间的起点，与
/// [`GeometrySpan`] 的 `TimeSpan` 一起构成稳定身份。低层 `GeometrySpan` 没有
/// 独立 ID，这是现有底座下最精确的定位方式。
///
/// ## 回滚
///
/// `apply` 缓存旧 `Place` 全字段与旧 `GeometrySpan`；`revert` 原样写回。
/// 缓存整段 span（而非只缓存旧环）是为了让回滚与 apply 的改动面严格对称——
/// 即使将来 apply 在此处多改一个字段，revert 也不会漏。
#[derive(Debug)]
pub struct UpdatePlaceTerritoryCommand {
    /// 目标地点 ID（同时是 span 的 `owner`）。
    pub place_id: wv_core::Id,
    /// 目标地图 ID（span 的 `map`）。
    pub map: wv_core::Id,
    /// 目标 span 的区间起点（`span.from`）。
    pub span_from: i64,
    /// 新的多边形环（扁平 `[x0, y0, x1, y1, ...]`，首尾不重复）。
    pub polygon: Vec<f32>,
    /// `apply` 时缓存的旧值：`(旧 Place 全字段, 旧 GeometrySpan)`。
    ///
    /// 用 `Mutex` 而非 `RefCell`，使命令满足 `Send + Sync`（Bevy 资源约束）。
    before: std::sync::Mutex<Option<(Place, GeometrySpan)>>,
}

impl UpdatePlaceTerritoryCommand {
    pub fn new(place_id: wv_core::Id, map: wv_core::Id, span_from: i64, polygon: Vec<f32>) -> Self {
        Self {
            place_id,
            map,
            span_from,
            polygon,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdatePlaceTerritoryCommand {
    fn label(&self) -> String {
        format!("改地界「{}」", self.place_id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let place_pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place_id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place_id)))?;
        let geo_pos = world
            .place_geometry
            .iter()
            .position(|g| {
                g.map == self.map && g.owner == self.place_id && g.span.from == self.span_from
            })
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "Place geometry (map {}, owner {}, from {})",
                    self.map, self.place_id, self.span_from
                ))
            })?;

        let old_place = world.distances.places[place_pos].clone();
        let old_span = world.place_geometry[geo_pos].clone();
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((old_place, old_span));

        world.place_geometry[geo_pos].geometry = GeometryData::Polygon(self.polygon.clone());
        world.distances.places[place_pos].fit_anchor_to_ring(&self.polygon);
        Ok(place_geometry_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (old_place, old_span) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdatePlaceTerritory 尚未 apply".into()))?;

        let place_pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place_id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place_id)))?;
        let geo_pos = world
            .place_geometry
            .iter()
            .position(|g| {
                g.map == old_span.map
                    && g.owner == old_span.owner
                    && g.span.from == old_span.span.from
            })
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "Place geometry (map {}, owner {}, from {})",
                    old_span.map, old_span.owner, old_span.span.from
                ))
            })?;

        // 两处定位都成功后再写回，避免半回退。
        world.distances.places[place_pos] = old_place;
        world.place_geometry[geo_pos] = old_span;
        Ok(())
    }
}

/// 重生成世界地形（ADR-0037 D4）：整张栅格换成模板生成的新版本。
///
/// 与 [`PaintTerrainCommand`] 的区别：这里换的是**整图**（模板重生成），
/// 不是局部笔触。`before` 缓存旧栅格 + 旧 `WorldTemplate`，撤销时原样还原。
///
/// 文件系统动作（`terrain.bin.bak.<ts>` 备份）在 app 层的命令分发器里做——
/// 本 crate 不碰磁盘（AGENTS §2.1）。
#[derive(Debug)]
pub struct RegenWorldTerrainCommand {
    map: wv_core::Id,
    new_terrain: TerrainData,
    new_template: wv_core::WorldTemplate,
    before: std::sync::Mutex<Option<(TerrainData, Option<wv_core::WorldTemplate>)>>,
}

impl RegenWorldTerrainCommand {
    pub fn new(
        map: wv_core::Id,
        new_terrain: TerrainData,
        new_template: wv_core::WorldTemplate,
    ) -> Self {
        Self {
            map,
            new_terrain,
            new_template,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RegenWorldTerrainCommand {
    fn label(&self) -> String {
        "重生成世界地形".into()
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let old = world
            .terrain_of(&self.map)
            .cloned()
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))?;
        let old_template = world.meta.as_ref().and_then(|m| m.template);
        *self
            .before
            .lock()
            .expect("RegenWorldTerrainCommand::before 锁") = Some((old, old_template));

        world.set_terrain(self.map.clone(), self.new_terrain.clone());
        if let Some(meta) = world.meta.as_mut() {
            meta.template = Some(self.new_template);
        }
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let guard = self
            .before
            .lock()
            .expect("RegenWorldTerrainCommand::before 锁");
        let Some((old, old_template)) = guard.as_ref() else {
            // apply 未成功（或未调用）→ 无可回退。
            return Ok(());
        };
        world.set_terrain(self.map.clone(), old.clone());
        if let Some(meta) = world.meta.as_mut() {
            meta.template = *old_template;
        }
        Ok(())
    }
}
