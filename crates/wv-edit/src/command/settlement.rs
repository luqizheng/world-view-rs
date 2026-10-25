//! 聚落命令族：街道 / 地点 / 建筑（自 command.rs 拆分）。

use super::*;

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
        if world.distances.places.iter().any(|p| p.id == self.place.id) {
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

/// 新增一条 `PlaceEdge`（距离图的边；`distances/edges/` 落盘）。
///
/// 与 [`AddPlaceCommand`] 同源：目标 ID 已存在时 `apply` 报错，不静默覆盖——
/// `PlaceEdge` 起点终点的 `Place` 引用是关键网络节点，悄无声覆盖会让
/// `DistanceGraph` 的拓扑在不可见处变化。
///
/// 悬空引用（`a` / `b` 指向不存在的 `Place`）按 AGENTS §7.5 V1 规则**不阻止
/// 保存**：作者可以先放一两个 Place 再回头补边，作者主人在工具之上。
#[derive(Debug)]
pub struct AddPlaceEdgeCommand {
    pub edge: wv_core::distance::PlaceEdge,
}

impl AddPlaceEdgeCommand {
    pub fn new(edge: wv_core::distance::PlaceEdge) -> Self {
        Self { edge }
    }
}

impl EditCommand for AddPlaceEdgeCommand {
    fn label(&self) -> String {
        format!("新增边「{}」", self.edge.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.distances.edges.iter().any(|e| e.id == self.edge.id) {
            return Err(EditError::Precondition(format!(
                "边 `{}` 已存在",
                self.edge.id
            )));
        }
        world.distances.edges.push(self.edge.clone());
        Ok(vec!["distances/edges".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let pos = world
            .distances
            .edges
            .iter()
            .position(|e| e.id == self.edge.id)
            .ok_or_else(|| EditError::TargetMissing(format!("PlaceEdge {}", self.edge.id)))?;
        world.distances.edges.remove(pos);
        Ok(())
    }
}

// 建筑编辑命令（ADR-0016 §6 落地项，2026-10-14）
// ---------------------------------------------------------------------------

/// 追加一栋建筑（2.5D 建筑挤出的数据入口）。
///
/// 语义与 [`AddStreetSegmentCommand`] 完全同构：目标图层未加载时**静默新建
/// 空文档**再追加（V1：作者画下的第一笔就是文档的诞生，不报错不追问）；
/// 落盘走虚拟脏路径 `buildings/<layer_id>`（`wv-io` 聚合写出）。
#[derive(Debug)]
pub struct AddBuildingCommand {
    /// 目标建筑图层 ID（`World.buildings` 的键）。
    pub layer: wv_core::Id,
    /// 要追加的建筑（轮廓 + 高度，本图局部坐标）。
    pub building: wv_core::Building,
    /// `apply` 时缓存的追加前栋数：revert 时把文档截回这个长度。
    before_len: std::sync::Mutex<Option<usize>>,
}

impl AddBuildingCommand {
    pub fn new(layer: wv_core::Id, building: wv_core::Building) -> Self {
        Self {
            layer,
            building,
            before_len: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for AddBuildingCommand {
    fn label(&self) -> String {
        format!("画建筑（{:.0} 米）", self.building.height_m)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        let before_len = doc.len();
        doc.push(self.building.clone());
        world.set_buildings(self.layer.clone(), doc);
        *(self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(before_len);
        Ok(vec![format!("buildings/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let before_len = self
            .before_len
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .ok_or_else(|| EditError::Precondition("AddBuilding 尚未 apply".into()))?;
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        doc.buildings.truncate(before_len);
        world.set_buildings(self.layer.clone(), doc);
        Ok(())
    }
}

/// 删除一栋建筑（按索引）。
///
/// 与 [`DeleteStreetSegmentCommand`] 对偶：apply 缓存被删栋，revert 插回原
/// 索引；越界报 `Precondition`（UI 侧点击前置灰，命令层兜底）。
#[derive(Debug)]
pub struct DeleteBuildingCommand {
    /// 目标建筑图层 ID（`World.buildings` 的键）。
    pub layer: wv_core::Id,
    /// 要删除的建筑索引（`buildings` 下标）。
    pub index: usize,
    /// `apply` 时缓存的被删栋：revert 时插回 `index`。
    before: std::sync::Mutex<Option<wv_core::Building>>,
}

impl DeleteBuildingCommand {
    pub fn new(layer: wv_core::Id, index: usize) -> Self {
        Self {
            layer,
            index,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for DeleteBuildingCommand {
    fn label(&self) -> String {
        format!("删除建筑 #{}", self.index)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        if self.index >= doc.len() {
            return Err(EditError::Precondition(format!(
                "建筑索引 {} 越界（图层 `{}` 共 {} 栋）",
                self.index,
                self.layer.as_str(),
                doc.len()
            )));
        }
        let removed = doc.buildings.remove(self.index);
        world.set_buildings(self.layer.clone(), doc);
        *(self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?) = Some(removed);
        Ok(vec![format!("buildings/{}", self.layer.as_str())])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let removed = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("DeleteBuilding 尚未 apply".into()))?;
        let mut doc = world.building_of(&self.layer).cloned().unwrap_or_default();
        let index = self.index.min(doc.buildings.len());
        doc.buildings.insert(index, removed);
        world.set_buildings(self.layer.clone(), doc);
        Ok(())
    }
}
