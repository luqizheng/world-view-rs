//! faction 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

// ---------------------------------------------------------------------------
// 势力编辑命令（S12 v0.2）
// ---------------------------------------------------------------------------

use wv_core::map::GeometrySpan;
use wv_core::{Color, Faction, FactionColorMode, FactionLayer, Membership};

/// 受影响的目录相对路径，供 `DirtySet` 落盘聚合。
///
/// Faction 与 GeometrySpan 是 YAML 实体（§4.5），分别落在
/// `factions/<id>.yaml` 与 `geometry/faction/<id>.yaml`；命令层无法知道
/// 具体文件名（owner 字符串），只声明目录，IO 落盘时再具体化。
fn faction_dirty_paths() -> Vec<String> {
    vec!["factions".into(), "geometry/faction".into()]
}

/// 势力层命令的脏路径：层与势力都写（删层会把成员的 `layer` 置空）。
fn faction_layer_dirty_paths() -> Vec<String> {
    vec!["faction_layers".into(), "factions".into()]
}

/// 新增一个 `Faction`。
///
/// `apply` 在 `world.factions` 末尾追加；`revert` 移除（按 ID 定位）。
#[derive(Debug)]
pub struct AddFactionCommand {
    pub faction: Faction,
}

impl AddFactionCommand {
    pub fn new(faction: Faction) -> Self {
        Self { faction }
    }
}

impl EditCommand for AddFactionCommand {
    fn label(&self) -> String {
        format!("新增势力「{}」", self.faction.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.factions.iter().any(|f| f.id == self.faction.id) {
            return Err(EditError::Precondition(format!(
                "势力 `{}` 已存在",
                self.faction.id
            )));
        }
        world.factions.push(self.faction.clone());
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .factions
            .iter()
            .position(|f| f.id == self.faction.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.faction.id)))?;
        world.factions.remove(pos);
        Ok(())
    }
}

/// 删除一个 `Faction`（及其名下的所有 `GeometrySpan`——避免悬空几何）。
///
/// **为什么连带删几何**：保留悬空几何会导致 `factions_at` 永远跳过该 owner，
/// 让作者误以为「数据没了」实则还在——更糟的是未来带洞多边形规则启用时，
/// 悬空会让校验规则错乱。一并删、撤销时一并恢复，是最干净的语义。
#[derive(Debug)]
pub struct RemoveFactionCommand {
    pub id: wv_core::Id,
    /// `apply` 时缓存的 Faction 本体（用于 revert 还原）。
    before_faction: std::sync::Mutex<Option<Faction>>,
    /// `apply` 时缓存的、本 Faction 名下所有 GeometrySpan（用于 revert 还原）。
    before_geos: std::sync::Mutex<Vec<GeometrySpan>>,
}

impl RemoveFactionCommand {
    pub fn new(id: wv_core::Id) -> Self {
        Self {
            id,
            before_faction: std::sync::Mutex::new(None),
            before_geos: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for RemoveFactionCommand {
    fn label(&self) -> String {
        format!("删除势力「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .factions
            .iter()
            .position(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        // 缓存本体与所属几何。
        let f = world.factions.remove(pos);
        let geos: Vec<GeometrySpan> = world
            .faction_geometry
            .iter()
            .filter(|g| g.owner == self.id)
            .cloned()
            .collect();
        world.faction_geometry.retain(|g| g.owner != self.id);
        *self
            .before_faction
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f);
        *self
            .before_geos
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = geos;
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let f = self
            .before_faction
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveFaction 尚未 apply".into()))?;
        let geos = self
            .before_geos
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        // 恢复几何（顺序不影响 `factions_at`，仅影响迭代顺序）。
        for g in geos {
            world.faction_geometry.push(g);
        }
        // 恢复 Faction 到原位（按 ID 排序语义不强——找首个能让 ID 升序的位置）。
        let insert_at = world
            .factions
            .iter()
            .position(|x| x.id > f.id)
            .unwrap_or(world.factions.len());
        world.factions.insert(insert_at, f);
        Ok(())
    }
}

/// 重命名 / 改色。
///
/// `apply` 缓存旧 `Faction` 全字段；`revert` 用缓存恢复。
#[derive(Debug)]
pub struct UpdateFactionCommand {
    pub id: wv_core::Id,
    pub new_name: String,
    pub new_color: Color,
    before: std::sync::Mutex<Option<Faction>>,
}

impl UpdateFactionCommand {
    pub fn new(id: wv_core::Id, new_name: String, new_color: Color) -> Self {
        Self {
            id,
            new_name,
            new_color,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateFactionCommand {
    fn label(&self) -> String {
        format!("改势力「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.name = self.new_name.clone();
        f.color = self.new_color.clone();
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateFaction 尚未 apply".into()))?;
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *f = prev;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 势力层 / 归属边命令（ADR-0027）
// ---------------------------------------------------------------------------

/// 新增一个势力层。
#[derive(Debug)]
pub struct AddFactionLayerCommand {
    pub layer: FactionLayer,
}

impl AddFactionLayerCommand {
    pub fn new(layer: FactionLayer) -> Self {
        Self { layer }
    }
}

impl EditCommand for AddFactionLayerCommand {
    fn label(&self) -> String {
        format!("新增势力层「{}」", self.layer.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.faction_layers.iter().any(|l| l.id == self.layer.id) {
            return Err(EditError::Precondition(format!(
                "势力层 `{}` 已存在",
                self.layer.id
            )));
        }
        world.faction_layers.push(self.layer.clone());
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .faction_layers
            .iter()
            .position(|l| l.id == self.layer.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.layer.id)))?;
        world.faction_layers.remove(pos);
        Ok(())
    }
}

/// 修改一个势力层的名称 / z / 配色策略 / 可见性（全字段快照，undo 原样恢复）。
#[derive(Debug)]
pub struct UpdateFactionLayerCommand {
    pub id: wv_core::Id,
    pub new_name: String,
    pub new_z: i32,
    pub new_color_mode: FactionColorMode,
    pub new_visible: bool,
    before: std::sync::Mutex<Option<FactionLayer>>,
}

impl UpdateFactionLayerCommand {
    pub fn new(
        id: wv_core::Id,
        new_name: String,
        new_z: i32,
        new_color_mode: FactionColorMode,
        new_visible: bool,
    ) -> Self {
        Self {
            id,
            new_name,
            new_z,
            new_color_mode,
            new_visible,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateFactionLayerCommand {
    fn label(&self) -> String {
        format!("改势力层「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let l = world
            .faction_layers
            .iter_mut()
            .find(|l| l.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(l.clone());
        l.name = self.new_name.clone();
        l.z = self.new_z;
        l.color_mode = self.new_color_mode;
        l.visible = self.new_visible;
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateFactionLayer 尚未 apply".into()))?;
        let l = world
            .faction_layers
            .iter_mut()
            .find(|l| l.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.id)))?;
        *l = prev;
        Ok(())
    }
}

/// 删除一个势力层：层下势力 `layer` 置 `None`（回落默认层），**不删势力本体**。
#[derive(Debug)]
pub struct RemoveFactionLayerCommand {
    pub id: wv_core::Id,
    before_layer: std::sync::Mutex<Option<(usize, FactionLayer)>>,
    before_factions: std::sync::Mutex<Vec<(wv_core::Id, Option<wv_core::Id>)>>,
}

impl RemoveFactionLayerCommand {
    pub fn new(id: wv_core::Id) -> Self {
        Self {
            id,
            before_layer: std::sync::Mutex::new(None),
            before_factions: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for RemoveFactionLayerCommand {
    fn label(&self) -> String {
        format!("删除势力层「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .faction_layers
            .iter()
            .position(|l| l.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("FactionLayer {}", self.id)))?;
        let layer = world.faction_layers.remove(pos);
        let mut affected = Vec::new();
        for f in &mut world.factions {
            if f.layer.as_ref() == Some(&self.id) {
                affected.push((f.id.clone(), f.layer.clone()));
                f.layer = None;
            }
        }
        *self
            .before_layer
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, layer));
        *self
            .before_factions
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = affected;
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, layer) = self
            .before_layer
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveFactionLayer 尚未 apply".into()))?;
        let affected = self
            .before_factions
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        for (fid, old_layer) in affected {
            if let Some(f) = world.factions.iter_mut().find(|f| f.id == fid) {
                f.layer = old_layer;
            }
        }
        let at = pos.min(world.faction_layers.len());
        world.faction_layers.insert(at, layer);
        Ok(())
    }
}

/// 把势力挂到某个层（`None` = 默认层）。
#[derive(Debug)]
pub struct SetFactionLayerCommand {
    pub id: wv_core::Id,
    pub layer: Option<wv_core::Id>,
    before: std::sync::Mutex<Option<Faction>>,
}

impl SetFactionLayerCommand {
    pub fn new(id: wv_core::Id, layer: Option<wv_core::Id>) -> Self {
        Self {
            id,
            layer,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetFactionLayerCommand {
    fn label(&self) -> String {
        format!("改势力「{}」的势力层", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.layer = self.layer.clone();
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.id, &self.before, "SetFactionLayer")
    }
}

/// 改势力的等级提示（自由字符串，不参与层级判定）。
#[derive(Debug)]
pub struct SetFactionRankCommand {
    pub id: wv_core::Id,
    pub rank: Option<String>,
    before: std::sync::Mutex<Option<Faction>>,
}

impl SetFactionRankCommand {
    pub fn new(id: wv_core::Id, rank: Option<String>) -> Self {
        Self {
            id,
            rank,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetFactionRankCommand {
    fn label(&self) -> String {
        format!("改势力「{}」的等级提示", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.rank = self.rank.clone();
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.id, &self.before, "SetFactionRank")
    }
}

/// 改势力的驻地 / 首府引用（`None` = 不锚定）。
#[derive(Debug)]
pub struct SetFactionSeatCommand {
    pub id: wv_core::Id,
    pub seat: Option<wv_core::Id>,
    before: std::sync::Mutex<Option<Faction>>,
}

impl SetFactionSeatCommand {
    pub fn new(id: wv_core::Id, seat: Option<wv_core::Id>) -> Self {
        Self {
            id,
            seat,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetFactionSeatCommand {
    fn label(&self) -> String {
        format!("改势力「{}」的驻地", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.seat = self.seat.clone();
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.id, &self.before, "SetFactionSeat")
    }
}

/// 追加一条归属边。同一 target 不同 from 的多条边**不合并**（多对多 + 时间）。
#[derive(Debug)]
pub struct AddMembershipCommand {
    pub faction: wv_core::Id,
    pub membership: Membership,
    before: std::sync::Mutex<Option<Faction>>,
}

impl AddMembershipCommand {
    pub fn new(faction: wv_core::Id, membership: Membership) -> Self {
        Self {
            faction,
            membership,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for AddMembershipCommand {
    fn label(&self) -> String {
        format!(
            "给势力「{}」添加归属 →「{}」",
            self.faction, self.membership.target
        )
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.faction)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.faction)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.memberships.push(self.membership.clone());
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.faction, &self.before, "AddMembership")
    }
}

/// 删除一条归属边（按 `(target, from)` 定位；同 target 可有多条不同 from）。
#[derive(Debug)]
pub struct RemoveMembershipCommand {
    pub faction: wv_core::Id,
    pub target: wv_core::Id,
    pub from: i64,
    before: std::sync::Mutex<Option<Faction>>,
}

impl RemoveMembershipCommand {
    pub fn new(faction: wv_core::Id, target: wv_core::Id, from: i64) -> Self {
        Self {
            faction,
            target,
            from,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveMembershipCommand {
    fn label(&self) -> String {
        format!("删除势力「{}」的归属 →「{}」", self.faction, self.target)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let f = world
            .factions
            .iter_mut()
            .find(|f| f.id == self.faction)
            .ok_or_else(|| EditError::TargetMissing(format!("Faction {}", self.faction)))?;
        let pos = f
            .memberships
            .iter()
            .position(|m| m.target == self.target && m.span.from == self.from)
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "Membership {} -> {} from {}",
                    self.faction, self.target, self.from
                ))
            })?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(f.clone());
        f.memberships.remove(pos);
        Ok(faction_layer_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        restore_faction_snapshot(world, &self.faction, &self.before, "RemoveMembership")
    }
}

/// 用已缓存的整份 `Faction` 快照恢复（恢复全部字段，不只被改的那一个）。
fn restore_faction_snapshot(
    world: &mut World,
    id: &wv_core::Id,
    before: &std::sync::Mutex<Option<Faction>>,
    what: &str,
) -> EditResult<()> {
    let prev = before
        .lock()
        .map_err(|_| EditError::Precondition("锁中毒".into()))?
        .clone()
        .ok_or_else(|| EditError::Precondition(format!("{what} 尚未 apply")))?;
    let f = world
        .factions
        .iter_mut()
        .find(|f| &f.id == id)
        .ok_or_else(|| EditError::TargetMissing(format!("Faction {id}")))?;
    *f = prev;
    Ok(())
}

/// 替换/新增一个势力的几何（按 `(map, owner, span.from, relation)` 视为同一实体的多版本）。
///
/// **同一性**：`GeometrySpan` 自身没有 `id` 字段（看 `map.rs`），但 路线图 §4.1
/// 原则六要求「同一实体同一维度的状态片段不覆盖」——这里把
/// `(map, owner, from, relation)` 四元组视为「实体的某次疆界变更」；四元组相同
/// → 是同一次变更的更新（**只允许一条**），不同 → 是不同次变更（多版本并存）。
/// 加入 `relation` 是 ADR-0027 的必要条件：同一势力同一天可以既有实控又有声称。
///
/// `apply` 若发现同 `(map, owner, from)` 已存在 → 替换；否则追加。`revert` 用
/// 缓存的"原位置 + 旧值"还原。
#[derive(Debug)]
pub struct UpsertFactionGeometryCommand {
    pub span: GeometrySpan,
    /// `apply` 时若发现已存在，原 span 的位置（用于 revert 还原顺序）。
    before_index: std::sync::Mutex<Option<usize>>,
    /// `apply` 时若发现已存在，原 span 的内容（用于 revert 还原值）。
    before_existing: std::sync::Mutex<Option<GeometrySpan>>,
}

impl UpsertFactionGeometryCommand {
    pub fn new(span: GeometrySpan) -> Self {
        Self {
            span,
            before_index: std::sync::Mutex::new(None),
            before_existing: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpsertFactionGeometryCommand {
    fn label(&self) -> String {
        format!("改势力「{}」在 {} 的疆界", self.span.owner, self.span.map)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 校验：owner 必须是已注册势力（V1 守约，悬空 owner 在 IO 加载时已经被
        // `factions_at` 静默跳过；编辑器里**创建新几何**则强制要求先有势力）。
        if !world.factions.iter().any(|f| f.id == self.span.owner) {
            return Err(EditError::Precondition(format!(
                "势力 `{}` 不存在；先创建势力再编辑几何",
                self.span.owner
            )));
        }
        let new_from = self.span.span.from;
        let pos = world.faction_geometry.iter().position(|g| {
            g.map == self.span.map
                && g.owner == self.span.owner
                && g.span.from == new_from
                && g.relation == self.span.relation
        });
        match pos {
            Some(i) => {
                let prev = world.faction_geometry[i].clone();
                world.faction_geometry[i] = self.span.clone();
                *self
                    .before_index
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(i);
                *self
                    .before_existing
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(prev);
            }
            None => {
                world.faction_geometry.push(self.span.clone());
                *self
                    .before_index
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))? = None;
            }
        }
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let idx = *self
            .before_index
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?;
        match idx {
            Some(i) => {
                let prev = self
                    .before_existing
                    .lock()
                    .map_err(|_| EditError::Precondition("锁中毒".into()))?
                    .clone()
                    .ok_or_else(|| EditError::Precondition("UpsertGeometry 尚未 apply".into()))?;
                if i < world.faction_geometry.len() {
                    world.faction_geometry[i] = prev;
                } else {
                    return Err(EditError::Precondition("revert 时几何位置越界".into()));
                }
            }
            None => {
                // 之前是追加；revert 时移除最后一条（apply 必是 push）。
                if world
                    .faction_geometry
                    .last()
                    .map(|g| g == &self.span)
                    .unwrap_or(false)
                {
                    world.faction_geometry.pop();
                } else {
                    return Err(EditError::Precondition(
                        "UpsertGeometry revert 找不到对应 span".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// 删除一个势力的某次几何（按 `(map, owner, from)` 定位）。
///
/// 与 `UpsertFactionGeometryCommand` 的同一性规则对称。
#[derive(Debug)]
pub struct RemoveFactionGeometryCommand {
    pub map: wv_core::Id,
    pub owner: wv_core::Id,
    pub from: i64,
    before: std::sync::Mutex<Option<(usize, GeometrySpan)>>,
}

impl RemoveFactionGeometryCommand {
    pub fn new(map: wv_core::Id, owner: wv_core::Id, from: i64) -> Self {
        Self {
            map,
            owner,
            from,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveFactionGeometryCommand {
    fn label(&self) -> String {
        format!("删除「{}」的某次疆界", self.owner)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .faction_geometry
            .iter()
            .position(|g| g.map == self.map && g.owner == self.owner && g.span.from == self.from)
            .ok_or_else(|| {
                EditError::TargetMissing(format!(
                    "GeometrySpan (map={}, owner={}, from={})",
                    self.map, self.owner, self.from
                ))
            })?;
        let prev = world.faction_geometry.remove(pos);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, prev));
        Ok(faction_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, prev) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveGeometry 尚未 apply".into()))?;
        let insert_at = pos.min(world.faction_geometry.len());
        world.faction_geometry.insert(insert_at, prev);
        Ok(())
    }
}

/// 划分模式（ADR-0028 D5）：与 `wv_terrain::divide::EdgeMode` 同义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DivisionMode {
    /// 边界吸附地物线（水陆界面 / 河链 / 山脊峰值）。
    #[default]
    Snap,
    /// 无视地貌，保留栅格提取的平滑边框。
    Straight,
}

impl DivisionMode {
    fn edge_mode(self) -> wv_terrain::divide::EdgeMode {
        match self {
            Self::Snap => wv_terrain::divide::EdgeMode::Snap,
            Self::Straight => wv_terrain::divide::EdgeMode::Straight,
        }
    }
}

/// 划分执行前的宿主快照（revert 用）。
#[derive(Debug, Clone, Default)]
struct BeforeDivision {
    host_index: Option<usize>,
    host_before: Option<GeometrySpan>,
    /// 宿主 span 被整体移除（意图吃光宿主）时 `revert` 需要插回。
    host_removed: bool,
    /// 历史切分时新插入的余部 span 下标（`revert` 删除）。
    remainder_index: Option<usize>,
}

/// 从宿主中间划出新势力（ADR-0028 D1/D2，势力交互 M1 Task 5）。
///
/// # 语义
///
/// - `host = Some(id)`：在该势力**生效中的疆界**里切下意图多边形——新势力
///   疆界 = 意图 ∩ 宿主，宿主疆界原位替换为余部（可为带洞环）。两环来自
///   同一次掩膜边界提取，**共边由构造保证**（D3）。
/// - `host = None`：空势力宿主——整张图的**陆格**都是宿主（水域归空势力，
///   D4/Q3）；只写新势力疆界，**不写任何宿主 span**。
///
/// # 原子性
///
/// 单命令单事务：失败（碎块化 / 意图不在宿主内 / 无地形）→ `Precondition`，
/// 世界零改动；`revert` 移除新势力及其全部几何并逐位还原宿主 span。
#[derive(Debug)]
pub struct DivideFactionCommand {
    pub map: wv_core::Id,
    /// 宿主势力；`None` = 空势力。
    pub host: Option<wv_core::Id>,
    /// 意图多边形（图内局部 wu）。
    pub intent: Vec<(f32, f32)>,
    pub mode: DivisionMode,
    /// 新势力（id / name / color 由调用方准备）。
    pub faction: Faction,
    /// 生效日（`TimeSpan::from`）。
    pub from: i64,
    /// 图内一格的世界单位（wu → 格坐标换算）。
    pub cell_size: f32,
    /// 贴地搜索窗口（格）。4 格 ≈ 4×cell_size wu。
    search_cells: f32,
    before: std::sync::Mutex<BeforeDivision>,
    /// `apply` 后置位：贴地是否真的吸附到地物（Q2 退化回执）。
    /// 由调用方经 [`Self::snapped_flag`] 共享读取（命令会被装进 `Box<dyn>`）。
    snapped: std::sync::Arc<std::sync::Mutex<bool>>,
}

impl DivideFactionCommand {
    pub fn new(
        map: wv_core::Id,
        host: Option<wv_core::Id>,
        intent: Vec<(f32, f32)>,
        mode: DivisionMode,
        faction: Faction,
        from: i64,
        cell_size: f32,
    ) -> Self {
        Self {
            map,
            host,
            intent,
            mode,
            faction,
            from,
            cell_size,
            search_cells: 4.0,
            before: std::sync::Mutex::new(BeforeDivision::default()),
            snapped: std::sync::Arc::new(std::sync::Mutex::new(false)),
        }
    }

    /// 共享的「贴地是否吸附到地物」标志（`apply` 之后读）。
    pub fn snapped_flag(&self) -> std::sync::Arc<std::sync::Mutex<bool>> {
        self.snapped.clone()
    }
}

/// `divide` 的领域错误 → 编辑错误（全部是 Precondition：可以直接改意图重试）。
fn divide_edit_error(e: wv_terrain::divide::DivideError) -> EditError {
    use wv_terrain::divide::DivideError;
    match e {
        DivideError::TooFewPoints => {
            EditError::Precondition("意图多边形至少需要 3 个点".into())
        }
        DivideError::NotInsideHost => {
            EditError::Precondition("意图多边形不在宿主（或陆地）范围内".into())
        }
        DivideError::Fragmented { inside, remainder } => EditError::Precondition(format!(
            "这次划分会把领地切成多块（v1 不支持）：新势力 {inside} 块、余部 {remainder} 块；请分步划分"
        )),
    }
}

fn flat_to_pts(flat: &[f32]) -> Vec<(f32, f32)> {
    flat.as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect()
}

/// 扁平点列的环（wu）。
type Ring = Vec<(f32, f32)>;

/// 疆界几何 →（外环 + 洞环），wu。非多边形 → `None`。
fn geometry_rings(geo: &GeometryData) -> Option<(Ring, Vec<Ring>)> {
    match geo {
        GeometryData::Polygon(v) => Some((flat_to_pts(v), Vec::new())),
        GeometryData::PolygonWithHoles { outer, holes } => Some((
            flat_to_pts(outer),
            holes.iter().map(|h| flat_to_pts(h)).collect(),
        )),
        GeometryData::Points(_) | GeometryData::Ref(_) => None,
    }
}

/// 划分产物（格坐标）→ 疆界几何（wu）。
fn rings_to_geometry(
    outers: &[Vec<(f32, f32)>],
    holes: &[Vec<Vec<(f32, f32)>>],
    cell_size: f32,
) -> GeometryData {
    let flat = |ring: &[(f32, f32)]| -> Vec<f32> {
        ring.iter()
            .flat_map(|&(x, y)| [x * cell_size, y * cell_size])
            .collect()
    };
    let outer = outers.first().map(|r| flat(r)).unwrap_or_default();
    let hole_rings: Vec<Vec<f32>> = holes
        .first()
        .map(|hs| hs.iter().map(|h| flat(h)).collect())
        .unwrap_or_default();
    if hole_rings.is_empty() {
        GeometryData::Polygon(outer)
    } else {
        GeometryData::PolygonWithHoles {
            outer,
            holes: hole_rings,
        }
    }
}

impl EditCommand for DivideFactionCommand {
    fn label(&self) -> String {
        match &self.host {
            Some(host) => format!("从「{host}」划分出「{}」", self.faction.name),
            None => format!("新建势力「{}」", self.faction.name),
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        use wv_core::time::TimePoint;

        if world.factions.iter().any(|f| f.id == self.faction.id) {
            return Err(EditError::Precondition(format!(
                "势力 `{}` 已存在",
                self.faction.id
            )));
        }
        if self.cell_size <= 0.0 || !self.cell_size.is_finite() {
            return Err(EditError::Precondition("格尺寸必须为正".into()));
        }
        let data = world.terrain_of(&self.map).ok_or_else(|| {
            EditError::Precondition(format!("地图 `{}` 没有地形栅格，无法划分势力", self.map))
        })?;
        let grid = to_grid(data)?;
        let intent_grid: Vec<(f32, f32)> = self
            .intent
            .iter()
            .map(|&(x, y)| (x / self.cell_size, y / self.cell_size))
            .collect();

        let (host_index, host_before, host_removed, remainder_index, new_geometry) = match &self
            .host
        {
            Some(host) => {
                if !world.factions.iter().any(|f| &f.id == host) {
                    return Err(EditError::Precondition(format!("宿主势力 `{host}` 不存在")));
                }
                let t = TimePoint::from_day(self.from);
                // 只认**实控** span（ADR-0027：同一 (map, owner, from) 可同时有
                // 声称与实控；划到声称上会留下实控未被切分、新势力重叠的烂摊子）。
                let idx = world
                    .faction_geometry
                    .iter()
                    .position(|g| {
                        g.map == self.map
                            && &g.owner == host
                            && g.relation.is_none()
                            && g.span.contains(&t)
                    })
                    .ok_or_else(|| {
                        EditError::Precondition(format!(
                            "宿主势力 `{host}` 在 {} 没有生效的实控疆界",
                            self.from
                        ))
                    })?;
                let before = world.faction_geometry[idx].clone();
                let (outer_wu, holes_wu) = geometry_rings(&before.geometry).ok_or_else(|| {
                    EditError::Precondition("宿主疆界不是多边形，无法划分".into())
                })?;
                // **单位换算（终审 C1）**：宿主几何是 wu，内核吃格坐标；
                // 意图在上方已换算，宿主这里同样必须除 cell_size，否则默认
                // 8 wu/格的工程里宿主会被当成「整张图」或直接落到图外。
                let to_cells = |ring: &[(f32, f32)]| -> Vec<(f32, f32)> {
                    ring.iter()
                        .map(|&(x, y)| (x / self.cell_size, y / self.cell_size))
                        .collect()
                };
                let outer = to_cells(&outer_wu);
                let holes: Vec<Vec<(f32, f32)>> = holes_wu.iter().map(|h| to_cells(h)).collect();
                let d = wv_terrain::divide::divide(
                    &grid,
                    &outer,
                    &holes,
                    &intent_grid,
                    self.mode.edge_mode(),
                    self.search_cells,
                )
                .map_err(divide_edit_error)?;
                if let Ok(mut flag) = self.snapped.lock() {
                    *flag = d.snapped;
                }
                let remainder_geometry =
                    rings_to_geometry(&d.remainder_outer, &d.remainder_holes, self.cell_size);
                let has_remainder = !d.remainder_outer.is_empty();
                // 历史切分（终审 I4 / ADR-0028 D6）：宿主 span 早于划分日开始时，
                // 把旧段截到 `from`、余部从 `from` 起新开一段——回拨时间轴仍能
                // 看到划分前的完整版图。
                let split_at_from = before.span.from < self.from
                    && before.span.to.map(|to| to > self.from).unwrap_or(true);
                let (host_index, host_before, host_removed, remainder_index) = if split_at_from {
                    let mut old = before.clone();
                    old.span.to = Some(self.from);
                    world.faction_geometry[idx] = old;
                    let mut rem_idx = None;
                    if has_remainder {
                        world.faction_geometry.push(GeometrySpan {
                            map: before.map.clone(),
                            owner: before.owner.clone(),
                            span: wv_core::time::TimeSpan {
                                from: self.from,
                                to: before.span.to,
                            },
                            geometry: remainder_geometry,
                            relation: before.relation.clone(),
                        });
                        rem_idx = Some(world.faction_geometry.len() - 1);
                    }
                    (Some(idx), Some(before), false, rem_idx)
                } else if has_remainder {
                    world.faction_geometry[idx] = GeometrySpan {
                        map: before.map.clone(),
                        owner: before.owner.clone(),
                        span: before.span,
                        geometry: remainder_geometry,
                        relation: before.relation.clone(),
                    };
                    (Some(idx), Some(before), false, None)
                } else {
                    // 意图吃光宿主：移除宿主 span（留空 Polygon 会让后续划分
                    // 报「意图不在宿主内」的误导错误）。
                    world.faction_geometry.remove(idx);
                    (Some(idx), Some(before), true, None)
                };
                let new_geometry = GeometrySpan {
                    map: self.map.clone(),
                    owner: self.faction.id.clone(),
                    span: wv_core::time::TimeSpan {
                        from: self.from,
                        to: None,
                    },
                    geometry: rings_to_geometry(&d.inside, &d.inside_holes, self.cell_size),
                    relation: None,
                };
                (
                    host_index,
                    host_before,
                    host_removed,
                    remainder_index,
                    new_geometry,
                )
            }
            None => {
                let d = wv_terrain::divide::divide_land_host(
                    &grid,
                    &intent_grid,
                    self.mode.edge_mode(),
                    self.search_cells,
                )
                .map_err(divide_edit_error)?;
                if let Ok(mut flag) = self.snapped.lock() {
                    *flag = d.snapped;
                }
                let new_geometry = GeometrySpan {
                    map: self.map.clone(),
                    owner: self.faction.id.clone(),
                    span: wv_core::time::TimeSpan {
                        from: self.from,
                        to: None,
                    },
                    geometry: rings_to_geometry(&d.inside, &d.inside_holes, self.cell_size),
                    relation: None,
                };
                (None, None, false, None, new_geometry)
            }
        };

        world.factions.push(self.faction.clone());
        world.faction_geometry.push(new_geometry);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = BeforeDivision {
            host_index,
            host_before,
            host_removed,
            remainder_index,
        };
        Ok(faction_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        world.factions.retain(|f| f.id != self.faction.id);
        world
            .faction_geometry
            .retain(|g| g.owner != self.faction.id);
        // 历史切分插入的余部段：按记录下标移除（它先于新势力几何 push，
        // 上面 retain 移除新几何不会移动它）。
        if let Some(i) = before.remainder_index {
            if i < world.faction_geometry.len() {
                world.faction_geometry.remove(i);
            }
        }
        if let (Some(idx), Some(prev)) = (before.host_index, before.host_before) {
            if before.host_removed {
                let at = idx.min(world.faction_geometry.len());
                world.faction_geometry.insert(at, prev);
            } else if idx < world.faction_geometry.len() {
                world.faction_geometry[idx] = prev;
            } else {
                return Err(EditError::Precondition("revert 时宿主疆界位置越界".into()));
            }
        }
        Ok(())
    }
}

/// 势力层可见性 **upsert**（终审 I4）：目标层不在 `world.faction_layers`
/// （= 默认层，[`wv_core::resolved_faction_layers`] 内存合成不写盘）时，
/// **物化**该层行再写 visible；已存在则与 [`UpdateFactionLayerCommand`]
/// 同款只改 visible。
///
/// 为什么不是改 `UpdateFactionLayerCommand`：它的 `apply` 对不存在的层
/// `TargetMissing`（ADR-0027 契约），revert 语义也随之不同——物化形态的
/// 撤销是「移除物化的行」。图层面板势力组眼睛走本命令。
#[derive(Debug)]
pub struct EnsureFactionLayerVisibleCommand {
    /// 目标层。
    pub id: wv_core::Id,
    /// 目标可见性。
    pub visible: bool,
    /// `apply` 缓存：`None` = 物化了新行（revert 移除）；`Some` = 更新既有行
    /// （revert 还原）。
    before: std::sync::Mutex<Option<EnsureLayerBefore>>,
}

#[derive(Debug)]
enum EnsureLayerBefore {
    Inserted,
    Updated(FactionLayer),
}

impl EnsureFactionLayerVisibleCommand {
    /// 构造。
    pub fn new(id: wv_core::Id, visible: bool) -> Self {
        Self {
            id,
            visible,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for EnsureFactionLayerVisibleCommand {
    fn label(&self) -> String {
        format!(
            "势力层「{}」{}",
            self.id,
            if self.visible { "显示" } else { "隐藏" }
        )
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if let Some(i) = world.faction_layers.iter().position(|l| l.id == self.id) {
            let prev = world.faction_layers[i].clone();
            world.faction_layers[i].visible = self.visible;
            *self
                .before
                .lock()
                .map_err(|_| EditError::Precondition("锁中毒".into()))? =
                Some(EnsureLayerBefore::Updated(prev));
        } else {
            // 物化：默认层合成口径（default_faction_layer）+ 目标 id/可见性。
            let mut layer = wv_core::default_faction_layer();
            layer.id = self.id.clone();
            layer.visible = self.visible;
            world.faction_layers.push(layer);
            *self
                .before
                .lock()
                .map_err(|_| EditError::Precondition("锁中毒".into()))? =
                Some(EnsureLayerBefore::Inserted);
        }
        Ok(faction_layer_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .take()
            .ok_or_else(|| {
                EditError::Precondition("EnsureFactionLayerVisible 尚未 apply".into())
            })?;
        match prev {
            EnsureLayerBefore::Inserted => {
                world.faction_layers.retain(|l| l.id != self.id);
            }
            EnsureLayerBefore::Updated(layer) => {
                if let Some(i) = world.faction_layers.iter().position(|l| l.id == self.id) {
                    world.faction_layers[i] = layer;
                }
            }
        }
        Ok(())
    }
}
