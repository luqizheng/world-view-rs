//! 编辑命令。
//!
//! 每个修改动作都实现 `EditCommand`。命令必须满足：
//! - 可序列化（用于事务日志与快照）；
//! - 可逆（或声明为不可逆，走「追加区间」路径）；
//! - 不直接触碰文件系统（那是 `wv-io` 的职责）。

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::World;
use wv_terrain::edit::{apply_stroke, Stroke};
use wv_terrain::{TerrainCell, TerrainGrid};

/// `wv-core::terrain::TerrainData` → `wv-terrain::TerrainGrid` 的桥。
///
/// `wv-core` 不认识 `TerrainGrid`（不依赖 wv-terrain），因此两层的接缝在这里。
/// `wv-io` 有对称的一对函数用于落盘，二者共享同一布局约定（行优先、双通道）。
fn to_grid(data: &TerrainData) -> EditResult<TerrainGrid> {
    let cells: Vec<TerrainCell> = data
        .cells
        .iter()
        .map(|c| TerrainCell {
            height: c.height,
            kind: c.kind,
        })
        .collect();
    TerrainGrid::from_cells(data.width, data.height, cells)
        .ok_or_else(|| EditError::TerrainMismatch("cells 长度与尺寸不符".into()))
}

/// `TerrainGrid` → `TerrainData` 的桥（[`to_grid`] 的逆运算，保留尺寸与顺序）。
fn from_grid(grid: TerrainGrid) -> TerrainData {
    let (w, h) = (grid.width(), grid.height());
    let cells: Vec<TerrainCellData> = grid
        .into_cells()
        .into_iter()
        .map(|c| TerrainCellData {
            height: c.height,
            kind: c.kind,
        })
        .collect();
    TerrainData {
        width: w,
        height: h,
        cells,
    }
}

pub type EditResult<T> = Result<T, EditError>;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("命令作用对象不存在: {0}")]
    TargetMissing(String),

    #[error("命令不满足前置条件: {0}")]
    Precondition(String),

    #[error("时间区间冲突: {0}")]
    SpanConflict(String),

    #[error("地形网格不存在或尺寸不符: {0}")]
    TerrainMismatch(String),
}

/// 编辑命令 trait。
///
/// `apply` 就地修改世界；`revert` 必须能把世界恢复到 `apply` 之前的状态。
/// 实现者若无法提供 `revert`，应改为「追加一个区间」而非覆盖。
///
/// ## 为什么要求 `Send + Sync`
///
/// `History` 会被 `wv-app` 作为 Bevy 的 `Resource` 持有，而 Bevy 资源必须是
/// `Send + Sync + 'static`（系统可跨线程并行）。因此命令对象也必须满足同一约束。
/// 需要「就地缓存旧值」的实现应使用 `Mutex` 而非 `RefCell`（见 [`PaintTerrainCommand`]）。
pub trait EditCommand: std::fmt::Debug + Send + Sync {
    /// 命令的人类可读描述，用于 Undo/Redo UI。
    fn label(&self) -> String;

    /// 执行。返回受影响的文件相对路径，供 `DirtySet` 使用。
    ///
    /// 由于命令需要 `revert`，实现通常把「被改动的旧值」缓存进内部可变状态
    /// （见 [`PaintTerrainCommand`] 的 `before` 字段）。
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>>;

    /// 回退。
    fn revert(&self, world: &mut World) -> EditResult<()>;
}

/// 刷地形 / 改海拔的命令。
///
/// ## 为什么命令要缓存 `before`
///
/// 命令必须可 `revert`，但网格是「就地修改」的。因此 `apply` 时把**受影响格的旧值**
/// 记进 `before`（`Mutex` 包裹以在 `&self` 下可变），`revert` 时原样写回。
///
/// 这正是 `wv-terrain` 与 `wv-edit` 的职责分界（基线 §2.2）：
/// **`wv-terrain` 不感知 Undo/Redo，`wv-edit` 不感知像素**。
#[derive(Debug)]
pub struct PaintTerrainCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 一次笔触（一串笔刷采样）。
    pub stroke: Stroke,
    /// 每格世界边长。
    pub cell_size: f32,
    /// `apply` 时缓存的受影响格旧值：`(gx, gy, before_cell)`。
    ///
    /// 用 `Mutex` 而非 `RefCell`，使命令可跨线程（Bevy 系统是并行的）。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl PaintTerrainCommand {
    pub fn new(map: wv_core::Id, stroke: Stroke, cell_size: f32) -> Self {
        Self {
            map,
            stroke,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 取回目标网格的**中立数据**。
    ///
    /// 查的是**这张图自己的**栅格（`World.terrains` 按图 ID 索引，Phase 2 起
    /// 每个图各有各的底图）。该图没有栅格时报 `TerrainMismatch`——不替作者
    /// 凭空造一张，因为尺寸该由作者的地图范围决定。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))
    }
}

impl EditCommand for PaintTerrainCommand {
    fn label(&self) -> String {
        format!("刷地形（{} 笔）", self.stroke.brushes.len())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let cell_size = self.cell_size;
        let stroke = &self.stroke;

        // ① 中立数据 → TerrainGrid
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        // ② 施加笔触（wv-terrain 的职责）
        apply_stroke(&mut grid, stroke, cell_size);
        // ③ 收集差异格作为 `before`（用于 revert）
        let mut before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
        before.clear();
        let width = grid.width() as usize;
        for (idx, (old, new)) in snapshot.iter().zip(grid.cells().iter()).enumerate() {
            if old != new {
                let gx = (idx % width) as i32;
                let gy = (idx / width) as i32;
                before.push((gx, gy, *old));
            }
        }
        drop(before);
        // ④ 写回中立数据（写回**这张图**的格子，不动别的图）
        world.set_terrain(self.map.clone(), from_grid(grid));

        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?;
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        for (gx, gy, cell) in before.iter() {
            grid.set(*gx, *gy, *cell);
        }
        drop(before);
        world.set_terrain(self.map.clone(), from_grid(grid));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 地图新建（2026-10-14：「新建地图」对话框的落点）
// ---------------------------------------------------------------------------

/// 新增一张地图（节点 + 父引用 + 可选预填充栅格）。
///
/// ## 为什么「挂到父图 children」是命令的一部分
///
/// P4 的层级关系只存 ID 引用（`MapNode.parent` / `children`），新节点不挂进
/// 父图的 `children` 就是一张悬空图——钻取树、地图树都看不见它。三件事
/// （push 节点、回填父引用、可选写栅格）必须同生共死，`revert` 才能完整回退。
///
/// ## 幂等保护
///
/// 目标 ID 已存在时 `apply` 报错（调用方——「新建地图」对话框——负责先生成
/// 唯一 ID）；命令层不静默覆盖，那等于悄悄毁掉一张已有的图。
#[derive(Debug)]
pub struct AddMapCommand {
    /// 新地图节点（`parent` 必须已指向既有地图；根图 `parent` 为 `None`）。
    pub node: wv_core::map::MapNode,
    /// 预填充栅格（海洋 / 陆地）。`None` = 不带栅格（沿用父图底图继承）。
    pub terrain: Option<TerrainData>,
}

impl AddMapCommand {
    pub fn new(node: wv_core::map::MapNode, terrain: Option<TerrainData>) -> Self {
        Self { node, terrain }
    }

    /// 受影响的目录：节点 YAML + 预填充栅格的 `terrain.bin`。
    fn dirty_paths(&self) -> Vec<String> {
        let mut paths = vec!["maps".to_owned()];
        if self.terrain.is_some() {
            paths.push(format!("maps/{}/terrain.bin", self.node.id));
        }
        paths
    }
}

impl EditCommand for AddMapCommand {
    fn label(&self) -> String {
        format!("新建地图「{}」", self.node.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.maps.iter().any(|m| m.id == self.node.id) {
            return Err(EditError::Precondition(format!(
                "地图 `{}` 已存在",
                self.node.id
            )));
        }
        // 父引用必须在树上真实存在（悬空 parent 会让钻取与校验双输）。
        if let Some(parent) = &self.node.parent {
            let Some(pnode) = world.maps.iter_mut().find(|m| &m.id == parent) else {
                return Err(EditError::TargetMissing(format!("父地图 `{parent}`")));
            };
            pnode.children.push(self.node.id.clone());
        }
        if let Some(data) = &self.terrain {
            world.set_terrain(self.node.id.clone(), data.clone());
        }
        world.maps.push(self.node.clone());
        Ok(self.dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        // 对称拆三件套：移除节点、摘掉父引用、丢弃预填充栅格。
        let existed = world
            .maps
            .iter()
            .position(|m| m.id == self.node.id)
            .map(|i| world.maps.remove(i))
            .is_some();
        if !existed {
            return Err(EditError::TargetMissing(format!(
                "地图 `{}` 不存在，无法撤销新建",
                self.node.id
            )));
        }
        if let Some(parent) = &self.node.parent {
            if let Some(pnode) = world.maps.iter_mut().find(|m| &m.id == parent) {
                pnode.children.retain(|c| c != &self.node.id);
            }
        }
        world.terrains.remove(&self.node.id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 势力编辑命令（S12 v0.2）
// ---------------------------------------------------------------------------

use wv_core::map::GeometrySpan;
use wv_core::{Color, Faction};

/// 受影响的目录相对路径，供 `DirtySet` 落盘聚合。
///
/// Faction 与 GeometrySpan 是 YAML 实体（§4.5），分别落在
/// `factions/<id>.yaml` 与 `geometry/faction/<id>.yaml`；命令层无法知道
/// 具体文件名（owner 字符串），只声明目录，IO 落盘时再具体化。
fn faction_dirty_paths() -> Vec<String> {
    vec!["factions".into(), "geometry/faction".into()]
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

/// 替换/新增一个势力的几何（按 `(map, owner, span.from)` 视为同一实体的多版本）。
///
/// **同一性**：`GeometrySpan` 自身没有 `id` 字段（看 `map.rs`），但 路线图 §4.1
/// 原则六要求「同一实体同一维度的状态片段不覆盖」——这里把 `(map, owner, from)`
/// 三元组视为「实体的某次疆界变更」；`from` 相同 → 是同一次变更的更新（**只允许一
/// 条**），不同 → 是不同次变更（多版本并存）。
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
            g.map == self.span.map && g.owner == self.span.owner && g.span.from == new_from
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

// 把 `Color` / `GeometryData` 隐式导入给测试用——避免测试模块再加 use。
#[allow(unused_imports)]
use wv_core::{Color as _Color, Faction as _Faction};

// ---------------------------------------------------------------------------
// 街道编辑命令（ADR-0010 待做第 3 项，2026-10-13）
// ---------------------------------------------------------------------------

/// 追加一段手绘街道（虚构路网的供给入口，ADR-0010 决策 ②）。
///
/// 目标图层未加载时**静默新建空文档**再追加——不报错、不追问：街道文档在
/// `World.streets` 里的首次出现往往就是作者画下的第一笔（V1：校验只提示，
/// 编辑不设阻力）；磁盘上对应 `maps/<map>/<source>` 的 YAML，由 `wv-io` 在
/// 落盘时按虚拟脏路径 `streets/<layer_id>` 聚合写出。
#[derive(Debug)]
pub struct AddStreetSegmentCommand {
    /// 目标街道图层 ID（`World.streets` 的键）。
    pub layer: wv_core::Id,
    /// 要追加的段（折线 + 类别，本图局部坐标）。
    pub segment: wv_core::StreetSegment,
    /// `apply` 时缓存的追加前段数：revert 时把文档截回这个长度。
    /// 用 `Mutex` 而非 `RefCell`，使命令可跨线程（与 [`PaintTerrainCommand`] 同款）。
    before_len: std::sync::Mutex<Option<usize>>,
}

impl AddStreetSegmentCommand {
    pub fn new(layer: wv_core::Id, segment: wv_core::StreetSegment) -> Self {
        Self {
            layer,
            segment,
            before_len: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for AddStreetSegmentCommand {
    fn label(&self) -> String {
        format!("画街道段（{:?}）", self.segment.class)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        let before_len = doc.len();
        doc.push(self.segment.clone());
        world.set_streets(self.layer.clone(), doc);
        *(self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(before_len);
        Ok(vec![format!("streets/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before_len = self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .ok_or_else(|| EditError::Precondition("AddStreetSegment 尚未 apply".into()))?;
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        doc.segments.truncate(before_len);
        world.set_streets(self.layer.clone(), doc);
        Ok(())
    }
}

/// 删除一段手绘街道（按索引）。
///
/// 与 [`AddStreetSegmentCommand`] 对偶：apply 缓存被删段，revert 把它插回
/// 原索引。索引越界报 `Precondition`——UI 侧应在点击前用当前段数判断
/// 可删性（按钮置灰），命令层保留兜底。
#[derive(Debug)]
pub struct DeleteStreetSegmentCommand {
    /// 目标街道图层 ID（`World.streets` 的键）。
    pub layer: wv_core::Id,
    /// 要删除的段索引（`segments` 下标）。
    pub index: usize,
    /// `apply` 时缓存的被删段：revert 时插回 `index`。
    before: std::sync::Mutex<Option<wv_core::StreetSegment>>,
}

impl DeleteStreetSegmentCommand {
    pub fn new(layer: wv_core::Id, index: usize) -> Self {
        Self {
            layer,
            index,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for DeleteStreetSegmentCommand {
    fn label(&self) -> String {
        format!("删除街道段 #{}", self.index)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        if self.index >= doc.len() {
            return Err(EditError::Precondition(format!(
                "街道段索引 {} 越界（图层 `{}` 共 {} 段）",
                self.index,
                self.layer.as_str(),
                doc.len()
            )));
        }
        let removed = doc.segments.remove(self.index);
        world.set_streets(self.layer.clone(), doc);
        *(self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(removed);
        Ok(vec![format!("streets/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let removed = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("DeleteStreetSegment 尚未 apply".into()))?;
        let mut doc = world.street_of(&self.layer).cloned().unwrap_or_default();
        let index = self.index.min(doc.segments.len());
        doc.segments.insert(index, removed);
        world.set_streets(self.layer.clone(), doc);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 地点新增（ADR-0014 v0.1 骨架：HTTP skill `create_place` 的命令入口）
// ---------------------------------------------------------------------------

/// 新增一个 `Place`（距离图节点 + 地图上可选锚点）。
///
/// `Place` 的存储归属在 `World.distances.places`（`wv_core::distance::Place`），
/// 它是 §4.5「每实体一文件」下的一个实体——脏路径 `distances/places`，由
/// `wv-io::save_world` 触发该目录的全量重写 + 删除传播。
///
/// ## 幂等保护
///
/// 与 `AddMapCommand` 同源：目标 ID 已存在时 `apply` 报错，不静默覆盖——
/// Place 是引用网络中的关键节点，悄无声息覆盖会让 `PlaceEdge` / `Unit` 的
/// 起点终点成为悬空引用，校验面板得花更多轮才找得到原因。
#[derive(Debug)]
pub struct AddPlaceCommand {
    pub place: wv_core::distance::Place,
}

impl AddPlaceCommand {
    pub fn new(place: wv_core::distance::Place) -> Self {
        Self { place }
    }
}

impl EditCommand for AddPlaceCommand {
    fn label(&self) -> String {
        format!("新增地点「{}」", self.place.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world
            .distances
            .places
            .iter()
            .any(|p| p.id == self.place.id)
        {
            return Err(EditError::Precondition(format!(
                "地点 `{}` 已存在",
                self.place.id
            )));
        }
        world.distances.places.push(self.place.clone());
        Ok(vec!["distances/places".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .distances
            .places
            .iter()
            .position(|p| p.id == self.place.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.place.id)))?;
        world.distances.places.remove(pos);
        Ok(())
    }
}

/// 设置地图朝向（屏幕旋转角，单位：度）。
///
/// `apply` 缓存旧值；`revert` 用缓存恢复。`0.0` = 正北朝上；
/// 正值 = 顺时针偏转。命令层不校验范围——UI 侧应把值夹到 `[0, 360)`。
///
/// 脏路径：`maps/{id}/index.yaml`（地图节点落盘文件）。
#[derive(Debug)]
pub struct SetMapOrientationCommand {
    pub map_id: wv_core::Id,
    pub orientation: f32,
    before: std::sync::Mutex<Option<f32>>,
}

impl SetMapOrientationCommand {
    pub fn new(map_id: wv_core::Id, orientation: f32) -> Self {
        Self {
            map_id,
            orientation,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetMapOrientationCommand {
    fn label(&self) -> String {
        format!("改地图「{}」朝向为 {:.1}°", self.map_id, self.orientation)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.map_id)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map_id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = Some(node.orientation);
        node.orientation = self.orientation;
        Ok(vec![format!("maps/{}/index.yaml", self.map_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("SetMapOrientation 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.map_id)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map_id)))?;
        node.orientation = prev;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::GeometryData;
    use wv_core::time::TimeSpan;
    use wv_core::Id;

    /// 测试用的图 ID——`PaintTerrainCommand` 的栅格是**按图**查的。
    const TEST_MAP: &str = "world_main";

    fn test_map() -> Id {
        Id::new_unchecked(TEST_MAP)
    }

    fn world_with_terrain(size: u32) -> World {
        let grid = TerrainGrid::new(size, size);
        let data = from_grid(grid);
        let mut world = World::default();
        world.set_terrain(test_map(), data);
        world
    }

    #[test]
    fn paint_command_apply_then_revert_restores_grid() {
        let mut world = world_with_terrain(32);
        let before_snapshot = world.terrain_of(&test_map()).cloned().unwrap();

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint((10.5, 10.5), 5.0, 3));
        let cmd = PaintTerrainCommand::new(test_map(), stroke, 1.0);

        let dirty = cmd.apply(&mut world).unwrap();
        assert!(!dirty.is_empty(), "应报告脏文件");
        let after = world.terrain_of(&test_map()).cloned().unwrap();
        assert_ne!(before_snapshot, after, "apply 应改变地形");
        // 中心格类型被写为 3
        assert_eq!(after.cells[10 * 32 + 10].kind, 3);

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain_of(&test_map()).cloned().unwrap(),
            before_snapshot,
            "revert 应完全还原"
        );
    }

    #[test]
    fn painting_one_map_leaves_the_other_maps_grid_untouched() {
        // Phase 2 的关键不变量：栅格按图存放，刷 A 城不能碰到 B 城。
        let mut world = world_with_terrain(32);
        let other = Id::new_unchecked("map_city_other");
        world.set_terrain(other.clone(), from_grid(TerrainGrid::new(16, 16)));
        let other_before = world.terrain_of(&other).cloned().unwrap();

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint((10.5, 10.5), 5.0, 3));
        let cmd = PaintTerrainCommand::new(test_map(), stroke, 1.0);
        cmd.apply(&mut world).unwrap();

        assert_eq!(
            world.terrain_of(&other).cloned().unwrap(),
            other_before,
            "另一张图的栅格必须一个格都没动"
        );
        assert_eq!(world.terrains.len(), 2, "不应凭空多出或丢掉栅格");
    }

    #[test]
    fn paint_command_without_terrain_reports_mismatch() {
        let mut world = World::default();
        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint((0.0, 0.0), 1.0, 1));
        let cmd = PaintTerrainCommand::new(Id::new_unchecked("world_main"), stroke, 1.0);
        assert!(matches!(
            cmd.apply(&mut world),
            Err(EditError::TerrainMismatch(_))
        ));
    }

    // ---- 势力命令（S12 v0.2） ----

    fn f(id: &str, name: &str) -> Faction {
        Faction::new(
            Id::new_unchecked(id),
            name,
            wv_core::color::Color("#112233".into()),
        )
    }

    fn geo(map: &str, owner: &str, from: i64, to: Option<i64>) -> GeometrySpan {
        GeometrySpan {
            map: Id::new_unchecked(map),
            owner: Id::new_unchecked(owner),
            span: TimeSpan { from, to },
            geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
        }
    }

    #[test]
    fn add_faction_apply_then_revert_removes() {
        let mut w = World::default();
        let cmd = AddFactionCommand::new(f("f1", "F1"));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(w.factions.len(), 1);
        assert!(dirty.contains(&"factions".to_string()));
        cmd.revert(&mut w).unwrap();
        assert!(w.factions.is_empty());
    }

    #[test]
    fn add_faction_duplicate_id_is_precondition_error() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let err = AddFactionCommand::new(f("f1", "F1-again")).apply(&mut w);
        assert!(matches!(err, Err(EditError::Precondition(_))));
    }

    #[test]
    fn remove_faction_also_clears_its_geometry() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 2);

        let rm = RemoveFactionCommand::new(Id::new_unchecked("f1"));
        rm.apply(&mut w).unwrap();
        assert!(w.factions.is_empty(), "Faction 自身被删");
        assert!(w.faction_geometry.is_empty(), "名下几何连带清空");

        rm.revert(&mut w).unwrap();
        assert_eq!(w.factions.len(), 1, "Faction 还原");
        assert_eq!(w.faction_geometry.len(), 2, "几何还原");
    }

    #[test]
    fn remove_faction_missing_is_target_error() {
        let mut w = World::default();
        let cmd = RemoveFactionCommand::new(Id::new_unchecked("ghost"));
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    #[test]
    fn update_faction_round_trips_via_revert() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let upd = UpdateFactionCommand::new(
            Id::new_unchecked("f1"),
            "F1-Renamed".into(),
            wv_core::color::Color("#FF0000".into()),
        );
        upd.apply(&mut w).unwrap();
        assert_eq!(w.factions[0].name, "F1-Renamed");
        assert_eq!(w.factions[0].color.as_str(), "#FF0000");
        upd.revert(&mut w).unwrap();
        assert_eq!(w.factions[0].name, "F1");
        assert_eq!(w.factions[0].color.as_str(), "#112233");
    }

    #[test]
    fn upsert_geometry_inserts_when_new() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let upsert = UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)));
        let dirty = upsert.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 1);
        assert!(dirty.contains(&"geometry/faction".to_string()));
        // revert：应能移除刚 push 的那条
        upsert.revert(&mut w).unwrap();
        assert!(w.faction_geometry.is_empty());
    }

    #[test]
    fn upsert_geometry_replaces_when_same_key() {
        // (map, owner, from) 一致 → 替换。`from` 不同的算作"另一版本" → 共存。
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(20)))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 1, "from 相同 → 替换");
        assert_eq!(w.faction_geometry[0].span.to, Some(20));
        UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
            .apply(&mut w)
            .unwrap();
        assert_eq!(w.faction_geometry.len(), 2, "from 不同 → 共存");
    }

    #[test]
    fn upsert_geometry_revert_restores_replaced_value() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        let original = geo("m", "f1", 1, Some(10));
        UpsertFactionGeometryCommand::new(original.clone())
            .apply(&mut w)
            .unwrap();
        let updated = geo("m", "f1", 1, Some(99));
        let upd_cmd = UpsertFactionGeometryCommand::new(updated);
        upd_cmd.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry[0].span.to, Some(99));
        upd_cmd.revert(&mut w).unwrap();
        assert_eq!(w.faction_geometry[0].span.to, Some(10), "应恢复为 10");
    }

    #[test]
    fn upsert_geometry_rejects_orphan_owner() {
        let mut w = World::default();
        // 没有 add Faction。
        let cmd = UpsertFactionGeometryCommand::new(geo("m", "f_ghost", 1, Some(10)));
        assert!(matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))));
    }

    #[test]
    fn remove_geometry_apply_then_revert_restores_at_original_index() {
        let mut w = World::default();
        AddFactionCommand::new(f("f1", "F1")).apply(&mut w).unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 1, Some(10)))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 20, None))
            .apply(&mut w)
            .unwrap();
        UpsertFactionGeometryCommand::new(geo("m", "f1", 30, None))
            .apply(&mut w)
            .unwrap();
        // 删中间那条 (from=20)。
        let rm =
            RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 20);
        rm.apply(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 2);
        assert!(w.faction_geometry.iter().all(|g| g.span.from != 20));
        rm.revert(&mut w).unwrap();
        assert_eq!(w.faction_geometry.len(), 3);
        assert_eq!(w.faction_geometry[1].span.from, 20, "按原位置 1 还原");
    }

    #[test]
    fn remove_geometry_missing_is_target_error() {
        let mut w = World::default();
        let cmd =
            RemoveFactionGeometryCommand::new(Id::new_unchecked("m"), Id::new_unchecked("f1"), 999);
        assert!(matches!(
            cmd.apply(&mut w),
            Err(EditError::TargetMissing(_))
        ));
    }

    // ---------------------- 街道命令（ADR-0010 ③） ----------------------

    fn seg(class: wv_core::RoadClass) -> wv_core::StreetSegment {
        wv_core::StreetSegment::straight(class, 10.0, 20.0, 30.0, 40.0)
    }

    const TEST_LAYER: &str = "layer_city_streets";

    fn test_layer() -> Id {
        Id::new_unchecked(TEST_LAYER)
    }

    #[test]
    fn add_street_segment_then_revert_restores_the_document() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        w.set_streets(test_layer(), doc);

        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Service));
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("streets/{}", TEST_LAYER)],
            "脏路径应为虚拟命名空间 `streets/<layer>`"
        );
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);

        cmd.revert(&mut w).unwrap();
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            1,
            "revert 应截回追加前长度"
        );
        assert_eq!(
            w.street_of(&test_layer()).unwrap().segments[0].class,
            wv_core::RoadClass::Primary,
            "被截掉的是新画的那段，旧段原样"
        );
    }

    #[test]
    fn drawing_on_an_unloaded_street_layer_creates_it_silently() {
        let mut w = World::default();
        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Residential));
        cmd.apply(&mut w).unwrap();
        let doc = w.street_of(&test_layer()).expect("首次落笔应新建文档");
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.segments[0].class, wv_core::RoadClass::Residential);
    }

    #[test]
    fn reverting_before_apply_is_a_precondition_error() {
        let mut w = World::default();
        let cmd = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Path));
        assert!(matches!(
            cmd.revert(&mut w),
            Err(EditError::Precondition(_))
        ));
    }

    #[test]
    fn two_street_commands_undo_independently() {
        let mut w = World::default();
        let a = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Primary));
        let b = AddStreetSegmentCommand::new(test_layer(), seg(wv_core::RoadClass::Path));
        a.apply(&mut w).unwrap();
        b.apply(&mut w).unwrap();
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);
        b.revert(&mut w).unwrap();
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 1);
        a.revert(&mut w).unwrap();
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            0,
            "逐条撤销后文档应为空"
        );
    }

    #[test]
    fn deleting_a_segment_then_reverting_restores_it_at_the_same_index() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        doc.push(seg(wv_core::RoadClass::Path));
        doc.push(seg(wv_core::RoadClass::Service));
        w.set_streets(test_layer(), doc);

        let cmd = DeleteStreetSegmentCommand::new(test_layer(), 1);
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec![format!("streets/{}", TEST_LAYER)],
            "脏路径应为虚拟命名空间 `streets/<layer>`"
        );
        assert_eq!(w.street_of(&test_layer()).unwrap().len(), 2);
        assert_eq!(
            w.street_of(&test_layer()).unwrap().segments[1].class,
            wv_core::RoadClass::Service,
            "中间那段应被删掉"
        );

        cmd.revert(&mut w).unwrap();
        let doc = w.street_of(&test_layer()).unwrap();
        assert_eq!(doc.len(), 3, "revert 应把段插回原索引");
        assert_eq!(
            doc.segments[1].class,
            wv_core::RoadClass::Path,
            "插回的必须是原来那一段"
        );
    }

    #[test]
    fn deleting_an_out_of_range_segment_is_a_precondition_error() {
        let mut w = World::default();
        let mut doc = wv_core::StreetLayer::new();
        doc.push(seg(wv_core::RoadClass::Primary));
        w.set_streets(test_layer(), doc);

        let cmd = DeleteStreetSegmentCommand::new(test_layer(), 1);
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "越界索引必须被拒绝而不是 panic"
        );
        assert_eq!(
            w.street_of(&test_layer()).unwrap().len(),
            1,
            "失败的命令不得改动文档"
        );
    }

    #[test]
    fn set_map_orientation_applies_and_reverts() {
        use wv_core::map::{MapLevel, MapNode};
        use wv_core::Id;

        let mut w = World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: None,
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        });

        let cmd = SetMapOrientationCommand::new(Id::new_unchecked("world_main"), 90.0);
        let dirty = cmd.apply(&mut w).unwrap();
        assert_eq!(
            dirty,
            vec!["maps/world_main/index.yaml".to_string()],
            "脏路径应为 maps/<id>/index.yaml"
        );
        assert_eq!(w.maps[0].orientation, 90.0, "apply 后朝向应为 90.0");

        cmd.revert(&mut w).unwrap();
        assert_eq!(w.maps[0].orientation, 0.0, "revert 后朝向应恢复为 0.0");
    }

    #[test]
    fn set_map_orientation_missing_map_is_a_target_missing_error() {
        let mut w = World::default();
        let cmd = SetMapOrientationCommand::new(Id::new_unchecked("ghost_map"), 45.0);
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "不存在的地图必须报 TargetMissing"
        );
    }

    // -----------------------------------------------------------------------
    // AddMapCommand（新建地图对话框的落点）
    // -----------------------------------------------------------------------

    fn map_node(
        id: &str,
        parent: Option<&str>,
        level: wv_core::map::MapLevel,
    ) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: format!("地图{id}"),
            level,
            parent: parent.map(Id::new_unchecked),
            children: Vec::new(),
            layers: Vec::new(),
            icons: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        }
    }

    fn flat_terrain(kind: u8, height: i16) -> TerrainData {
        TerrainData {
            width: 4,
            height: 4,
            cells: vec![TerrainCellData { height, kind }; 16],
        }
    }

    #[test]
    fn add_map_command_links_parent_and_prefills_terrain() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));

        let cmd = AddMapCommand::new(
            map_node("map_a", Some("world_main"), wv_core::map::MapLevel::City),
            Some(flat_terrain(0, -40)),
        );
        let dirty = cmd.apply(&mut w).unwrap();
        assert!(
            dirty.contains(&"maps".to_owned())
                && dirty.contains(&"maps/map_a/terrain.bin".to_owned()),
            "脏路径应含节点目录与栅格文件，实得 {dirty:?}"
        );
        // 节点入树 + 父引用回填 + 栅格落位，三件都在。
        assert!(w.maps.iter().any(|m| m.id == Id::new_unchecked("map_a")));
        assert!(w
            .maps
            .iter()
            .find(|m| m.id == Id::new_unchecked("world_main"))
            .unwrap()
            .children
            .contains(&Id::new_unchecked("map_a")));
        assert_eq!(
            w.terrain_of(&Id::new_unchecked("map_a")).unwrap().cells[0].kind,
            0
        );

        // revert 对称拆三件套。
        cmd.revert(&mut w).unwrap();
        assert!(!w.maps.iter().any(|m| m.id == Id::new_unchecked("map_a")));
        assert!(!w
            .maps
            .iter()
            .find(|m| m.id == Id::new_unchecked("world_main"))
            .unwrap()
            .children
            .contains(&Id::new_unchecked("map_a")));
        assert!(w.terrain_of(&Id::new_unchecked("map_a")).is_none());
    }

    #[test]
    fn add_map_command_rejects_duplicate_id_instead_of_overwriting() {
        let mut w = World::default();
        w.maps
            .push(map_node("world_main", None, wv_core::map::MapLevel::World));
        w.maps.push(map_node(
            "map_a",
            Some("world_main"),
            wv_core::map::MapLevel::City,
        ));
        let before = w.maps.len();

        let cmd = AddMapCommand::new(
            map_node("map_a", Some("world_main"), wv_core::map::MapLevel::Region),
            None,
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::Precondition(_))),
            "重复 ID 必须报错，不能静默覆盖已有地图"
        );
        assert_eq!(w.maps.len(), before, "失败的 apply 不得改动地图表");
    }

    #[test]
    fn add_map_command_rejects_dangling_parent() {
        let mut w = World::default();
        let cmd = AddMapCommand::new(
            map_node("map_a", Some("ghost_parent"), wv_core::map::MapLevel::City),
            None,
        );
        assert!(
            matches!(cmd.apply(&mut w), Err(EditError::TargetMissing(_))),
            "悬空父引用必须报 TargetMissing"
        );
        assert!(w.maps.is_empty(), "失败的 apply 不得留下半张图");
    }

    #[test]
    fn root_map_adds_without_parent_and_revert_removes_it() {
        // 空编辑器的「第一张图」：parent = None，revert 后世界回到空。
        let mut w = World::default();
        let cmd = AddMapCommand::new(
            map_node("world_main", None, wv_core::map::MapLevel::World),
            Some(flat_terrain(3, 80)),
        );
        cmd.apply(&mut w).unwrap();
        assert_eq!(w.maps.len(), 1);
        cmd.revert(&mut w).unwrap();
        assert!(w.maps.is_empty());
        assert!(w.terrains.is_empty());
    }
}
