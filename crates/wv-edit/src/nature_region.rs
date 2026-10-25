//! 自然区域命令族（ADR-0045）——覆盖物声明 / 嵌套 / 合并 / 边界重生成。
//!
//! ## 职责边界
//! - 命令只做**领域层**的事：改 `World.regions` 里的实体；
//! - 几何计算走 `wv-terrain::region`（biome 连通分区、掩膜取环、并集），
//!   本模块只做**坐标换算**（wu ↔ 格坐标）与**实体拼装**；
//! - 渲染层的事（覆盖物怎么画、缺失标记）不在此层（ADR-0045 D9 / 追记一）。
//!
//! ## 坐标口径
//! `Region.polygon` / `Region.holes` 是**图内局部 wu**（与 `Place.envelope`、
//! `Region.centroid` 同口径）；`wv-terrain::region` 的环是**格坐标**。
//! 换算一律 `wu ÷ cell_size` / `格 × cell_size`——与 `marker.rs` 的
//! 「包络（局部 wu）→ 格坐标」同款约定（根图正确；子图沿用同一约定，
//! 与生物群系标记一致）。

use std::sync::Mutex;

use wv_core::map::distance::Place;
use wv_core::model::region::Region;
use wv_core::{Id, World};

use crate::command::{region_dirty_paths, to_grid, EditCommand, EditError, EditResult};

/// wu 扁平点列 → 格坐标环。
///
/// **`offset` = `map_origin - raster_origin`**：区域多边形是**图内局部 wu**，
/// 而地形格属于**栅格所在的图**（子图上不是同一张图——两套原点）。换算必须
/// 先减掉偏移，否则子图上的区域会整体错位。
fn ring_to_cells(ring: &[f32], cell_size: f32, offset: (f32, f32)) -> Vec<(f32, f32)> {
    ring.chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| ((p[0] - offset.0) / cell_size, (p[1] - offset.1) / cell_size))
        .collect()
}

/// 格坐标环 → wu 扁平点列（**加回**偏移，回到图内局部 wu）。
fn cells_to_wu(ring: &[(f32, f32)], cell_size: f32, offset: (f32, f32)) -> Vec<f32> {
    ring.iter()
        .flat_map(|&(x, y)| [x * cell_size + offset.0, y * cell_size + offset.1])
        .collect()
}

// ── 覆盖物键（ADR-0045 D2） ────────────────────────────────────────────────

/// 设置 / 清除区域的**覆盖物键**（ADR-0045 D2）。
///
/// 覆盖物键是**表现层**声明：作者说「这片深林用针叶画」；键的词表由主题拥有
/// （`ThemePack::region_overlays`）。当前主题查无此键 → 渲染层画**缺失覆盖物
/// 标记**（白底红斜线，ADR-0045 追记一），**不回退**——回退等于拿另一个变体
/// 冒充作者声明。
///
/// **普通区域（`kind = area`）不承载覆盖物**：命令**不拦截**（V1 只提示不阻止），
/// 但生效判定在 `Region::declared_overlay()`——`area` 一律返回 `None`。
#[derive(Debug)]
pub struct SetRegionOverlayCommand {
    /// 目标区域。
    pub id: Id,
    /// `Some(key)` = 设置；`None` = 清除声明。
    pub overlay: Option<String>,
    before: Mutex<Option<Region>>,
}

impl SetRegionOverlayCommand {
    pub fn new(id: Id, overlay: Option<String>) -> Self {
        Self {
            id,
            overlay,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for SetRegionOverlayCommand {
    fn label(&self) -> String {
        match &self.overlay {
            Some(k) => format!("设区域「{}」覆盖物「{}」", self.id, k),
            None => format!("清区域「{}」覆盖物", self.id),
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(r.clone());
        r.overlay = self
            .overlay
            .as_ref()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        Ok(region_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("SetRegionOverlay 尚未 apply".into()))?;
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *r = prev;
        Ok(())
    }
}

// ── 类别互转（ADR-0045 D3） ───────────────────────────────────────────────

/// 设置区域的**类别**（ADR-0045 D3）：普通区域 ↔ 自然区互转。
///
/// 为什么需要：作者圈了一个「关中平原」后想给它加覆盖物声明，必须先把它
/// 转成自然区——`Region::declared_overlay()` 对 `area` 一律返回 `None`
/// （泛用面挂覆盖物 / 嵌套会引入无意义的组合爆炸）。
#[derive(Debug)]
pub struct SetRegionKindCommand {
    pub id: Id,
    pub kind: wv_core::model::region::RegionKind,
    before: Mutex<Option<Region>>,
}

impl SetRegionKindCommand {
    pub fn new(id: Id, kind: wv_core::model::region::RegionKind) -> Self {
        Self {
            id,
            kind,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for SetRegionKindCommand {
    fn label(&self) -> String {
        format!("把区域「{}」设为{}", self.id, self.kind.label())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(r.clone());
        r.kind = self.kind;
        Ok(region_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("SetRegionKind 尚未 apply".into()))?;
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *r = prev;
        Ok(())
    }
}

// ── 嵌套（ADR-0045 D3 / D5） ───────────────────────────────────────────────

/// 设置 / 清除区域的**父区**（ADR-0045 D3 嵌套 / D5 合并的层级面）。
///
/// **环检测**：父区不能是自己，也不能是自己的后代（否则深度计算不收敛）。
#[derive(Debug)]
pub struct SetRegionParentCommand {
    pub id: Id,
    /// `Some(parent)` = 设为子块；`None` = 提到顶层。
    pub parent: Option<Id>,
    before: Mutex<Option<Region>>,
}

impl SetRegionParentCommand {
    pub fn new(id: Id, parent: Option<Id>) -> Self {
        Self {
            id,
            parent,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for SetRegionParentCommand {
    fn label(&self) -> String {
        match &self.parent {
            Some(p) => format!("区域「{}」挂到「{}」下", self.id, p),
            None => format!("区域「{}」提到顶层", self.id),
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if let Some(p) = &self.parent {
            if p == &self.id {
                return Err(EditError::Precondition("区域不能以自身为父区".into()));
            }
            if !world.regions.iter().any(|r| &r.id == p) {
                return Err(EditError::TargetMissing(format!("Region {}", p)));
            }
            // 父区 p 不能是 a 的后代：从 p 沿 parent 链上溯若能走到 a → 成环。
            if is_descendant(world, p, &self.id) {
                return Err(EditError::Precondition(
                    "父区不能是自身的后代（会成环）".into(),
                ));
            }
        }
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(r.clone());
        r.parent = self.parent.clone();
        Ok(region_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("SetRegionParent 尚未 apply".into()))?;
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *r = prev;
        Ok(())
    }
}

/// `candidate` 是否是 `ancestor` 的后代（沿 `parent` 链上溯）。
fn is_descendant(world: &World, candidate: &Id, ancestor: &Id) -> bool {
    let mut cur = Some(candidate.clone());
    let mut guard = 0usize;
    while let Some(id) = cur {
        guard += 1;
        if guard > world.regions.len() + 1 {
            return false;
        }
        if &id == ancestor {
            return true;
        }
        cur = world
            .regions
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.parent.clone());
    }
    false
}

// ── 合并（ADR-0045 D5） ────────────────────────────────────────────────────

/// 合并两个自然区（ADR-0045 D5）：**几何并集 + 旧成员变子块**。
///
/// - 几何 = A ∪ B（掩膜并 → 重新取环；共边在并集后自然消失）；
/// - 层级 = `A.parent = B.parent = 新区`；A、B **保留**（不删）——
///   因此「合并」是「嵌套」的特例，一套语义打天下；
/// - 两区**不相邻**时并集得到多块外环 → 拒绝（`Disjoint`），同 ADR-0028 D2
///   的 `Fragmented` 处置：给作者提示，不静默产出碎块。
#[derive(Debug)]
pub struct MergeNatureRegionCommand {
    /// 参与合并的区域 A。
    pub a: Id,
    /// 参与合并的区域 B。
    pub b: Id,
    /// 合并后新区域的 ID（由调用方生成，保证撤销后不冲突）。
    pub new_id: Id,
    /// 新区域名。
    pub new_name: String,
    /// 每格世界边长（wu ÷ cell_size = 格坐标）。
    pub cell_size: f32,
    /// `map_origin - raster_origin`（两套原点的差，子图上非零）。
    pub offset: (f32, f32),
    before: Mutex<Option<(Region, Region)>>,
}

impl MergeNatureRegionCommand {
    pub fn new(
        a: Id,
        b: Id,
        new_id: Id,
        new_name: String,
        cell_size: f32,
        offset: (f32, f32),
    ) -> Self {
        Self {
            a,
            b,
            new_id,
            new_name,
            cell_size,
            offset,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for MergeNatureRegionCommand {
    fn label(&self) -> String {
        format!(
            "合并自然区「{}」「{}」→「{}」",
            self.a, self.b, self.new_name
        )
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if self.cell_size <= 0.0 || !self.cell_size.is_finite() {
            return Err(EditError::Precondition("格尺寸必须为正".into()));
        }
        if world.regions.iter().any(|r| r.id == self.new_id) {
            return Err(EditError::Precondition(format!(
                "区域 `{}` 已存在",
                self.new_id
            )));
        }
        let ra = world
            .regions
            .iter()
            .find(|r| r.id == self.a)
            .cloned()
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.a)))?;
        let rb = world
            .regions
            .iter()
            .find(|r| r.id == self.b)
            .cloned()
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.b)))?;
        if ra.map != rb.map {
            return Err(EditError::Precondition("两个区域不在同一张图上".into()));
        }
        if !ra.is_nature() || !rb.is_nature() {
            return Err(EditError::Precondition("只有自然区可以合并".into()));
        }
        let data = world.terrain_of(&ra.map).ok_or_else(|| {
            EditError::Precondition(format!("地图 `{}` 没有地形栅格，无法合并区域", ra.map))
        })?;
        let (w, h) = (data.width, data.height);

        let set_a = wv_terrain::region::RingSet {
            outer: ring_to_cells(&ra.polygon, self.cell_size, self.offset),
            holes: ra
                .holes
                .iter()
                .map(|hole| ring_to_cells(hole, self.cell_size, self.offset))
                .collect(),
        };
        let set_b = wv_terrain::region::RingSet {
            outer: ring_to_cells(&rb.polygon, self.cell_size, self.offset),
            holes: rb
                .holes
                .iter()
                .map(|hole| ring_to_cells(hole, self.cell_size, self.offset))
                .collect(),
        };
        let union = wv_terrain::region::union_regions(&set_a, &set_b, w, h).map_err(|e| {
            EditError::Precondition(match e {
                wv_terrain::region::RegionError::Disjoint => "两个区域不相邻，无法合并".to_string(),
                wv_terrain::region::RegionError::Degenerate => "区域轮廓退化，无法合并".to_string(),
                wv_terrain::region::RegionError::EmptyGrid => "地图没有地形栅格".to_string(),
            })
        })?;

        // 父区：两者同父则继承，���则取顶层。
        let parent = match (&ra.parent, &rb.parent) {
            (Some(x), Some(y)) if x == y => Some(x.clone()),
            _ => None,
        };
        let merged = Region {
            id: self.new_id.clone(),
            name: self.new_name.clone(),
            map: ra.map.clone(),
            polygon: cells_to_wu(&union.outer, self.cell_size, self.offset),
            holes: union
                .holes
                .iter()
                .map(|hole| cells_to_wu(hole, self.cell_size, self.offset))
                .collect(),
            kind: wv_core::model::region::RegionKind::Nature,
            parent: parent.clone(),
            // 覆盖物**不继承**（ADR-0045 D5 + §待裁 4）：子块各自保留自己的表现。
            overlay: None,
        };

        // 旧成员变子块。
        for r in world.regions.iter_mut() {
            if r.id == self.a || r.id == self.b {
                r.parent = Some(self.new_id.clone());
            }
        }
        world.regions.push(merged);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((ra, rb));
        Ok(region_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (ra, rb) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("MergeNatureRegion 尚未 apply".into()))?;
        world.regions.retain(|r| r.id != self.new_id);
        for old in [ra, rb] {
            if let Some(r) = world.regions.iter_mut().find(|r| r.id == old.id) {
                *r = old;
            }
        }
        Ok(())
    }
}

// ── 边界重生成（ADR-0045 D6） ──────────────────────────────────────────────

/// 重生成自然区域的边界（ADR-0045 D6）。
///
/// 区域边界是**物化时刻的快照**（同 ADR-0029 D2 的先例）：笔刷改了地形后
/// **不自动跟随**，由作者显式重算。重算口径 = 「识别或生成」的**识别**分支
/// （ADR-0029 D3）：区域内**占多数**的群系 → 取该群系的连通块 → 新边界。
#[derive(Debug)]
pub struct RegenerateNatureRegionCommand {
    pub id: Id,
    /// 每格世界边长。
    pub cell_size: f32,
    /// `map_origin - raster_origin`（两套原点的差，子图上非零）。
    pub offset: (f32, f32),
    before: Mutex<Option<Region>>,
}

impl RegenerateNatureRegionCommand {
    pub fn new(id: Id, cell_size: f32, offset: (f32, f32)) -> Self {
        Self {
            id,
            cell_size,
            offset,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for RegenerateNatureRegionCommand {
    fn label(&self) -> String {
        format!("重生成区域「{}」边界", self.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if self.cell_size <= 0.0 || !self.cell_size.is_finite() {
            return Err(EditError::Precondition("格尺寸必须为正".into()));
        }
        let r = world
            .regions
            .iter()
            .find(|r| r.id == self.id)
            .cloned()
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        let data = world.terrain_of(&r.map).ok_or_else(|| {
            EditError::Precondition(format!("地图 `{}` 没有地形栅格，无法重生成边界", r.map))
        })?;
        let grid = to_grid(data)?;
        let (w, h) = (grid.width(), grid.height());

        let outer = ring_to_cells(&r.polygon, self.cell_size, self.offset);
        let mask = wv_terrain::region::mask_of_ring_set(
            &wv_terrain::region::RingSet {
                outer: outer.clone(),
                holes: r
                    .holes
                    .iter()
                    .map(|hole| ring_to_cells(hole, self.cell_size, self.offset))
                    .collect(),
            },
            w,
            h,
        );
        let new_rings = wv_terrain::region::identify_region(&grid, &mask).ok_or_else(|| {
            EditError::Precondition("区域内没有可识别的群系连通块（请重圈或改用物化）".into())
        })?;

        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(r.clone());
        r.polygon = cells_to_wu(&new_rings.outer, self.cell_size, self.offset);
        r.holes = new_rings
            .holes
            .iter()
            .map(|hole| cells_to_wu(hole, self.cell_size, self.offset))
            .collect();
        Ok(region_dirty_paths())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RegenerateNatureRegion 尚未 apply".into()))?;
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *r = prev;
        Ok(())
    }
}

/// 物化一个自动分区候选区（ADR-0045 D4「命名即物化」）。
///
/// 候选区本身**不落盘**；作者给它起名时才生成实体。本命令 = 建一个
/// `kind = Nature` 的区域实体（几何由调用方从分区结果换算而来）。
#[derive(Debug)]
pub struct MaterializeNatureRegionCommand {
    /// 待创建的实体（调用方填好 id / 名字 / 几何 / `kind = Nature`）。
    pub region: Region,
}

impl MaterializeNatureRegionCommand {
    pub fn new(region: Region) -> Self {
        Self { region }
    }
}

impl EditCommand for MaterializeNatureRegionCommand {
    fn label(&self) -> String {
        format!("物化自然区「{}」", self.region.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if !self.region.is_nature() {
            return Err(EditError::Precondition("物化的区域必须是自然区".into()));
        }
        if world.regions.iter().any(|r| r.id == self.region.id) {
            return Err(EditError::Precondition(format!(
                "区域 `{}` 已存在",
                self.region.id
            )));
        }
        world.regions.push(self.region.clone());
        Ok(region_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        world.regions.retain(|r| r.id != self.region.id);
        Ok(())
    }
}

// ── 地点折线（ADR-0046 D1 / D6） ───────────────────────────────────────────

/// 地点的**折线**脏路径（`§4.5` 每实体一文件 → `distances/places`）。
fn place_dirty_paths() -> Vec<String> {
    vec!["distances/places".into()]
}

/// 给地点**加**一条折线（ADR-0046 D6）。
///
/// 折线是**亚格**的作者线状数据（小溪 / 小河）：不占格、不写 biome
/// （ADR-0046 D3），与一格宽的笔刷河（`Intent::River`）并存。
#[derive(Debug)]
pub struct AddPlaceLineCommand {
    pub id: Id,
    /// 折线点列（图内局部 wu，扁平 `[x0,y0,x1,y1,...]`）。
    pub line: Vec<f32>,
    before: Mutex<Option<Place>>,
}

impl AddPlaceLineCommand {
    pub fn new(id: Id, line: Vec<f32>) -> Self {
        Self {
            id,
            line,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for AddPlaceLineCommand {
    fn label(&self) -> String {
        format!("给地点「{}」加折线", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if self.line.len() < 4 {
            return Err(EditError::Precondition("折线至少需要 2 个点".into()));
        }
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        if p.line.is_some() {
            return Err(EditError::Precondition(
                "该地点已有折线（请用改折线）".into(),
            ));
        }
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(p.clone());
        p.line = Some(self.line.clone());
        Ok(place_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("AddPlaceLine 尚未 apply".into()))?;
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

/// 改地点的折线（ADR-0046 D6）。
#[derive(Debug)]
pub struct UpdatePlaceLineCommand {
    pub id: Id,
    /// 新折线（`None` = 不改）。
    pub line: Option<Vec<f32>>,
    before: Mutex<Option<Place>>,
}

impl UpdatePlaceLineCommand {
    pub fn new(id: Id, line: Option<Vec<f32>>) -> Self {
        Self {
            id,
            line,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for UpdatePlaceLineCommand {
    fn label(&self) -> String {
        format!("改地点「{}」折线", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if let Some(line) = &self.line {
            if line.len() < 4 {
                return Err(EditError::Precondition("折线至少需要 2 个点".into()));
            }
        }
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
        if let Some(line) = &self.line {
            p.line = Some(line.clone());
        }
        Ok(place_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdatePlaceLine 尚未 apply".into()))?;
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

/// 删掉地点的折线（ADR-0046 D6）。
#[derive(Debug)]
pub struct DeletePlaceLineCommand {
    pub id: Id,
    before: Mutex<Option<Place>>,
}

impl DeletePlaceLineCommand {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            before: Mutex::new(None),
        }
    }
}

impl EditCommand for DeletePlaceLineCommand {
    fn label(&self) -> String {
        format!("删地点「{}」折线", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let p = world
            .distances
            .places
            .iter_mut()
            .find(|p| p.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Place {}", self.id)))?;
        if p.line.is_none() {
            return Err(EditError::Precondition("该地点没有折线".into()));
        }
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(p.clone());
        p.line = None;
        Ok(place_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("DeletePlaceLine 尚未 apply".into()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::map::distance::Place;
    use wv_core::model::region::RegionKind;
    use wv_core::terrain::{TerrainCellData, TerrainData};

    const TEST_MAP: &str = "map_world";

    fn nature_region(id: &str, name: &str, polygon: Vec<f32>) -> Region {
        Region {
            id: Id::new_unchecked(id),
            name: name.into(),
            map: Id::new_unchecked(TEST_MAP),
            polygon,
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: None,
            overlay: None,
        }
    }

    /// 4×4 栅格：全陆地、全群系 1。
    fn world_with_terrain() -> World {
        let mut w = World::default();
        let cells = vec![
            TerrainCellData {
                height: 100,
                surface: 1,
                biome: 1,
            };
            16
        ];
        w.set_terrain(
            Id::new_unchecked(TEST_MAP),
            TerrainData {
                width: 4,
                height: 4,
                cells,
            },
        );
        w
    }

    fn place(id: &str) -> Place {
        Place {
            kind: None,
            id: Id::new_unchecked(id),
            name: "小溪".into(),
            feature: None,
            map: Some(Id::new_unchecked(TEST_MAP)),
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: None,
        }
    }

    #[test]
    fn set_region_overlay_sets_and_clears_the_key() {
        let mut w = World::default();
        w.regions.push(nature_region(
            "nr_a",
            "深林",
            vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0],
        ));
        let cmd = SetRegionOverlayCommand::new(Id::new_unchecked("nr_a"), Some("conifer".into()));
        cmd.apply(&mut w).expect("应用");
        assert_eq!(w.regions[0].overlay.as_deref(), Some("conifer"));
        cmd.revert(&mut w).expect("回退");
        assert_eq!(w.regions[0].overlay, None);
    }

    #[test]
    fn set_region_parent_rejects_cycles() {
        let mut w = World::default();
        w.regions.push(nature_region("nr_a", "父", vec![]));
        w.regions.push(nature_region("nr_b", "子", vec![]));
        // b 的父 = a，再把 a 的父设成 b → 成环 → 拒绝。
        let cmd =
            SetRegionParentCommand::new(Id::new_unchecked("nr_b"), Some(Id::new_unchecked("nr_a")));
        cmd.apply(&mut w).expect("首次挂接应成功");
        let cycle =
            SetRegionParentCommand::new(Id::new_unchecked("nr_a"), Some(Id::new_unchecked("nr_b")));
        assert!(
            cycle.apply(&mut w).is_err(),
            "成环应被拒绝（父区不能是自身后代）"
        );
    }

    #[test]
    fn set_region_kind_switches_between_area_and_nature() {
        let mut w = World::default();
        w.regions.push(nature_region(
            "nr_a",
            "甲",
            vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0],
        ));
        let cmd = SetRegionKindCommand::new(
            Id::new_unchecked("nr_a"),
            wv_core::model::region::RegionKind::Area,
        );
        cmd.apply(&mut w).expect("应用");
        assert_eq!(w.regions[0].kind, wv_core::model::region::RegionKind::Area);
        cmd.revert(&mut w).expect("回退");
        assert_eq!(
            w.regions[0].kind,
            wv_core::model::region::RegionKind::Nature
        );
    }

    #[test]
    fn merge_two_adjacent_nature_regions_creates_parent_and_keeps_members() {
        let mut w = world_with_terrain();
        w.regions.push(nature_region(
            "nr_a",
            "东林",
            vec![0.0, 0.0, 2.0, 0.0, 2.0, 2.0],
        ));
        w.regions.push(nature_region(
            "nr_b",
            "西林",
            vec![2.0, 0.0, 4.0, 0.0, 4.0, 2.0],
        ));
        let cmd = MergeNatureRegionCommand::new(
            Id::new_unchecked("nr_a"),
            Id::new_unchecked("nr_b"),
            Id::new_unchecked("nr_m"),
            "合并林".into(),
            1.0,
            (0.0, 0.0),
        );
        cmd.apply(&mut w).expect("相邻应可合并");
        assert_eq!(w.regions.len(), 3, "新增一个合并区");
        let merged = w
            .regions
            .iter()
            .find(|r| r.id.as_str() == "nr_m")
            .expect("合并区");
        assert_eq!(merged.kind, RegionKind::Nature);
        assert!(merged.polygon.len() >= 6, "合并区应有外环");
        // 旧成员变子块（保留不删）。
        for id in ["nr_a", "nr_b"] {
            let r = w
                .regions
                .iter()
                .find(|r| r.id.as_str() == id)
                .expect("旧成员保留");
            assert_eq!(r.parent.as_ref().map(|p| p.as_str()), Some("nr_m"));
        }
        cmd.revert(&mut w).expect("回退");
        assert_eq!(w.regions.len(), 2, "回退后合并区消失、旧成员恢复");
        for r in &w.regions {
            assert!(r.parent.is_none(), "回退后旧成员无父区");
        }
    }

    /// 合并时**两套原点的偏移**参与换算（子图场景）：区域多边形是图内局部
    /// wu，格子属于栅格所在的图——偏移错了就会并到别的格上。
    #[test]
    fn merge_uses_the_raster_offset_when_mapping_polygon_to_cells() {
        let mut w = world_with_terrain();
        // 偏移 (10,10)：图内局部 wu (10..12) 对应栅格格 (0..2)。
        w.regions.push(nature_region(
            "nr_a",
            "东林",
            vec![10.0, 10.0, 12.0, 10.0, 12.0, 12.0],
        ));
        w.regions.push(nature_region(
            "nr_b",
            "西林",
            vec![12.0, 10.0, 14.0, 10.0, 14.0, 12.0],
        ));
        let cmd = MergeNatureRegionCommand::new(
            Id::new_unchecked("nr_a"),
            Id::new_unchecked("nr_b"),
            Id::new_unchecked("nr_m"),
            "合并林".into(),
            1.0,
            (10.0, 10.0),
        );
        cmd.apply(&mut w).expect("相邻应可合并");
        let merged = w
            .regions
            .iter()
            .find(|r| r.id.as_str() == "nr_m")
            .expect("合并区");
        // 合并区应横跨两区（图内局部 wu 10..14）。
        let min_x = merged
            .polygon
            .iter()
            .step_by(2)
            .copied()
            .fold(f32::MAX, f32::min);
        let max_x = merged
            .polygon
            .iter()
            .step_by(2)
            .copied()
            .fold(f32::MIN, f32::max);
        assert!(min_x <= 10.6, "左界应贴近 10，实得 {min_x}");
        assert!((max_x - 14.0).abs() <= 0.6, "右界应贴近 14，实得 {max_x}");
    }

    #[test]
    fn merge_of_disjoint_regions_is_rejected() {
        let mut w = world_with_terrain();
        w.regions.push(nature_region(
            "nr_a",
            "甲",
            vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
        ));
        w.regions.push(nature_region(
            "nr_b",
            "乙",
            vec![3.0, 3.0, 4.0, 3.0, 4.0, 4.0],
        ));
        let cmd = MergeNatureRegionCommand::new(
            Id::new_unchecked("nr_a"),
            Id::new_unchecked("nr_b"),
            Id::new_unchecked("nr_m"),
            "合并".into(),
            1.0,
            (0.0, 0.0),
        );
        assert!(cmd.apply(&mut w).is_err(), "不相邻应拒绝");
    }

    #[test]
    fn regenerate_nature_region_recomputes_boundary_from_cells() {
        let mut w = world_with_terrain();
        // 先给区域一个偏小的轮廓（只覆盖 1 格），重生成应扩到整个群系连通块。
        w.regions.push(nature_region(
            "nr_a",
            "林",
            vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
        ));
        let cmd = RegenerateNatureRegionCommand::new(Id::new_unchecked("nr_a"), 1.0, (0.0, 0.0));
        cmd.apply(&mut w).expect("重生成应成功");
        let r = &w.regions[0];
        let max_x = r.polygon.iter().step_by(2).copied().fold(0.0f32, f32::max);
        assert!(
            max_x > 1.5,
            "重生成后边界应覆盖整块连通区，实得 max_x={max_x}"
        );
        cmd.revert(&mut w).expect("回退");
        assert_eq!(w.regions[0].polygon, vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn add_place_line_sets_polyline_and_reverts() {
        let mut w = World::default();
        w.distances.places.push(place("place_river"));
        let line = vec![0.0, 0.0, 10.0, 5.0, 20.0, 0.0];
        let cmd = AddPlaceLineCommand::new(Id::new_unchecked("place_river"), line.clone());
        cmd.apply(&mut w).expect("应用");
        assert_eq!(w.distances.places[0].line.as_deref(), Some(line.as_slice()));
        // 锚点：x/y 缺省 → 由**弧长中点**派生（两段等长 → 中点落在中间顶点）。
        let anchor = w.distances.places[0].effective_anchor().expect("锚点");
        assert!(
            (anchor.0 - 10.0).abs() < 1e-3,
            "中点 x 应为 10，实得 {}",
            anchor.0
        );
        assert!(
            (anchor.1 - 5.0).abs() < 1e-3,
            "中点 y 应为 5，实得 {}",
            anchor.1
        );
        cmd.revert(&mut w).expect("回退");
        assert!(w.distances.places[0].line.is_none());
    }

    #[test]
    fn delete_place_line_clears_polyline() {
        let mut w = World::default();
        let mut p = place("place_river");
        p.line = Some(vec![0.0, 0.0, 1.0, 1.0]);
        w.distances.places.push(p);
        let cmd = DeletePlaceLineCommand::new(Id::new_unchecked("place_river"));
        cmd.apply(&mut w).expect("应用");
        assert!(w.distances.places[0].line.is_none());
        cmd.revert(&mut w).expect("回退");
        assert!(w.distances.places[0].line.is_some());
    }

    #[test]
    fn add_place_line_rejects_degenerate_polyline() {
        let mut w = World::default();
        w.distances.places.push(place("place_river"));
        let cmd = AddPlaceLineCommand::new(Id::new_unchecked("place_river"), vec![1.0, 1.0]);
        assert!(cmd.apply(&mut w).is_err(), "单点折线应拒绝");
    }

    #[test]
    fn materialize_nature_region_requires_nature_kind() {
        let mut w = World::default();
        let mut r = nature_region("nr_a", "甲", vec![]);
        r.kind = RegionKind::Area;
        let cmd = MaterializeNatureRegionCommand::new(r);
        assert!(cmd.apply(&mut w).is_err(), "普通区域不该被物化为自然区");
    }
}
