//! map 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

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
// 城市子图编辑命令（ADR-0022 · 2026-10-16）
// ---------------------------------------------------------------------------

use wv_core::base::geometry::Point;
use wv_core::map::{GeometryData, IconShape, IconSpec, MapLevel, MapNode};
use wv_core::Id;

/// 新建城市子图（ADR-0022 D5 · `AddCityCommand`）。
///
/// 在指定父图下挂一张 `MapLevel::City` 子图，同时在父图上落一个 `IconSpec`
///（默认菱形，`drill_into = city.id`）作为钻入入口——这是用户在父图上
/// 「看见这座城」的唯一视觉锚点。
///
/// ## 与 `AddMapCommand` 的边界
///
/// `AddMapCommand` 是「建任意图」的通用入口（任意 `MapLevel`、可选 terrain）；
/// `AddCityCommand` 是「建城市图」的特化版本：
/// - 强制 `level = MapLevel::City`（建错了 call site 应自行校验）；
/// - **不**带 terrain 预填（城市图继承父图底图即可，per ADR-0009 §7）；
/// - 自动落 `IconSpec`（普通子图不需要这个）。
///
/// ## 河流复制
///
/// M2 阶段**不**复制父图河流入子图——这是 M3 (`D3`) 的语义；
/// 本命令 apply 仅挂图标 + 注册子图节点。M3 接 D3 时在本命令的 `apply`
/// 末尾追加 clipping 步骤即可，向后兼容。
///
/// ## 脏路径
///
/// - `maps`（新增子图节点的 `index.yaml`）
/// - `maps/{parent_id}/index.yaml`（父图加了 icons / children）
#[derive(Debug)]
pub struct AddCityCommand {
    pub city: MapNode,
    /// 钻入图标的 ID（`Id` 字符集 `[A-Za-z0-9_]`）。
    pub icon_id: Id,
    /// 钻入图标的形状（默认菱形；可用 `IconShape::City` 之类预留枚举）。
    pub icon_shape: IconShape,
    /// 钻入图标的中心锚点（[0,1]²，父图 extent 归一化）。默认 `(0.5, 0.5)`。
    pub icon_anchor: [f32; 2],
}

impl AddCityCommand {
    pub fn new(city: MapNode, icon_id: Id, icon_shape: IconShape) -> Self {
        Self {
            city,
            icon_id,
            icon_shape,
            icon_anchor: [0.5, 0.5],
        }
    }
}

impl EditCommand for AddCityCommand {
    fn label(&self) -> String {
        format!("新建城市「{}」", self.city.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if world.maps.iter().any(|m| m.id == self.city.id) {
            return Err(EditError::Precondition(format!(
                "城市 `{}` 已存在",
                self.city.id
            )));
        }
        let parent_id = self
            .city
            .parent
            .clone()
            .ok_or_else(|| EditError::Precondition("城市必须挂在某父图下".into()))?;
        let parent = world
            .maps
            .iter_mut()
            .find(|m| m.id == parent_id)
            .ok_or_else(|| EditError::TargetMissing(format!("父图 `{parent_id}`")))?;
        parent.icons.push(IconSpec {
            id: self.icon_id.clone(),
            anchor: Some(self.icon_anchor),
            shape: self.icon_shape,
            size: None,
            color: None,
            drill_into: Some(self.city.id.clone()),
        });
        parent.children.push(self.city.id.clone());

        let city_id = self.city.id.clone();
        world.maps.push(self.city.clone());
        Ok(vec![
            "maps".to_owned(),
            format!("maps/{parent_id}/index.yaml"),
            format!("maps/{city_id}/index.yaml"),
        ])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let existed = world
            .maps
            .iter()
            .position(|m| m.id == self.city.id)
            .map(|i| world.maps.remove(i))
            .is_some();
        if !existed {
            return Err(EditError::TargetMissing(format!(
                "城市 `{}` 不存在，无法撤销新建",
                self.city.id
            )));
        }
        let parent_id_ref = self
            .city
            .parent
            .as_ref()
            .ok_or_else(|| EditError::Precondition("城市必须挂在某父图下".into()))?;
        if let Some(parent) = world.maps.iter_mut().find(|m| &m.id == parent_id_ref) {
            parent.children.retain(|c| c != &self.city.id);
            parent
                .icons
                .retain(|ic| ic.drill_into.as_ref() != Some(&self.city.id));
        }
        Ok(())
    }
}

/// 平移城市（ADR-0022 D2 · `MoveCityCommand`）。
///
/// 把城市在父图坐标系里平移 `delta`（wu）：
/// - `extent.origin` += delta（若 `Some`）
/// - `outline_polygon`（`Polygon` / `PolygonWithHoles`，含洞环）所有顶点 += delta
/// - 子图 LOCAL 几何（街道、地点、building）**不动**——per ADR-0009 §9 「本图 local」
/// - 引用该城市的 `Place` / `Track` / `Faction` **不动**——它们是独立实体
///
/// ## revert
///
/// 缓存整张 `MapNode` 的快照，`revert` 时把 `extent` 与 `outline_polygon` 还原；
/// 其余字段（icons / layers / children / 名字等）不参与同步，保留当前值。
/// 这避免了「撤回期间又有人改了同一节点」导致的字段级不一致。
///
/// ## 失败语义
///
/// - 城市不存在：`TargetMissing`
/// - `outline_polygon = Some(Points)` 或 `Ref`：`Precondition`（不支持变体）
#[derive(Debug)]
pub struct MoveCityCommand {
    pub city_id: Id,
    pub delta: Point,
    before_snapshot: std::sync::Mutex<Option<MapNode>>,
}

impl MoveCityCommand {
    pub fn new(city_id: Id, delta: Point) -> Self {
        Self {
            city_id,
            delta,
            before_snapshot: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for MoveCityCommand {
    fn label(&self) -> String {
        format!(
            "平移城市「{}」by ({:.1}, {:.1})",
            self.city_id, self.delta.x, self.delta.y
        )
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        // 缓存快照。
        *self
            .before_snapshot
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = Some(node.clone());
        // 平移 extent.origin。
        if let Some(ext) = node.extent.as_mut() {
            let origin = ext.origin.unwrap_or(Point::ORIGIN);
            ext.origin = Some(Point {
                x: origin.x + self.delta.x,
                y: origin.y + self.delta.y,
            });
        }
        // 平移 outline_polygon（仅 Polygon 变体）。
        if let Some(poly) = node.outline_polygon.as_mut() {
            match poly {
                GeometryData::Polygon(v) => {
                    for chunk in v.chunks_exact_mut(2) {
                        chunk[0] += self.delta.x;
                        chunk[1] += self.delta.y;
                    }
                }
                GeometryData::PolygonWithHoles { outer, holes } => {
                    for chunk in outer.chunks_exact_mut(2) {
                        chunk[0] += self.delta.x;
                        chunk[1] += self.delta.y;
                    }
                    for hole in holes {
                        for chunk in hole.chunks_exact_mut(2) {
                            chunk[0] += self.delta.x;
                            chunk[1] += self.delta.y;
                        }
                    }
                }
                GeometryData::Points(_) | GeometryData::Ref(_) => {
                    return Err(EditError::Precondition(format!(
                        "城市「{}」的 outline_polygon 不是多边形变体，不支持平移",
                        self.city_id
                    )));
                }
            }
        }
        Ok(vec![format!("maps/{}/index.yaml", self.city_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let snap = self
            .before_snapshot
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("MoveCity 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        node.extent = snap.extent;
        node.outline_polygon = snap.outline_polygon;
        Ok(())
    }
}

/// 重画城市轮廓（ADR-0022 D5 · `SetCityOutlineCommand`）。
///
/// 用新 polygon 覆盖 `outline_polygon`（允许 `None` 把轮廓擦掉）；
/// `revert` 缓存旧值原子还原。
///
/// ## 用途
///
/// 用户在父图上重画轮廓（拖 4 个角点编辑）；与 `AddCityCommand` 的
/// `outline_polygon` 字段共享同一 `GeometryData::Polygon` 变体约定。
#[derive(Debug)]
pub struct SetCityOutlineCommand {
    pub city_id: Id,
    pub new_polygon: Option<GeometryData>,
    before: std::sync::Mutex<Option<Option<GeometryData>>>,
}

impl SetCityOutlineCommand {
    pub fn new(city_id: Id, new_polygon: Option<GeometryData>) -> Self {
        Self {
            city_id,
            new_polygon,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetCityOutlineCommand {
    fn label(&self) -> String {
        if self.new_polygon.is_some() {
            format!("重画城市「{}」轮廓", self.city_id)
        } else {
            format!("擦除城市「{}」轮廓", self.city_id)
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        if let Some(GeometryData::Polygon(v)) = &self.new_polygon {
            if v.len() % 2 != 0 || v.len() < 6 {
                return Err(EditError::Precondition(
                    "outline_polygon 必须是 ≥3 顶点的扁平坐标对".into(),
                ));
            }
        }
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? =
            Some(node.outline_polygon.clone());
        node.outline_polygon = self.new_polygon.clone();
        Ok(vec![format!("maps/{}/index.yaml", self.city_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("SetCityOutline 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.city_id)
            .ok_or_else(|| EditError::TargetMissing(format!("城市 {}", self.city_id)))?;
        node.outline_polygon = prev;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 地图属性编辑（2026-10-27：地图属性对话框的落点）
// ---------------------------------------------------------------------------

/// 修改地图基本属性（名称 / `unit_per_km` 尺度声明）。
///
/// ## 为什么只有这两个字段
/// 作者日常要改的「地图属性」就是名字与尺度声明（视口比例尺、陆地尺度
/// 提示都依 `unit_per_km` 换算）；extent / raster / geographic_source 属于
/// 结构性数据，改它们等于换一张图（各有专属命令或重导入路径），不混进
/// 本命令。后续要扩字段时 revert 快照改成整节点即可，调用方零改动。
///
/// ## revert
/// apply 前快照 `(name, unit_per_km)` 原子还原；其余字段不参与同步
/// （同 [`MoveCityCommand`] 的字段隔离策略：撤回期间他人的改动不被覆盖）。
#[derive(Debug)]
pub struct UpdateMapPropsCommand {
    /// 目标地图。
    pub map_id: Id,
    /// 新显示名（可中文；**不改** `id`——ID 是 YAML/Git 引用锚，改名不改号）。
    pub new_name: String,
    /// 新尺度声明；`None` = 未声明（比例尺静默不显示，V1）。
    pub new_unit_per_km: Option<f32>,
    before: std::sync::Mutex<Option<(String, Option<f32>)>>,
}

impl UpdateMapPropsCommand {
    pub fn new(map_id: Id, new_name: String, new_unit_per_km: Option<f32>) -> Self {
        Self {
            map_id,
            new_name,
            new_unit_per_km,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateMapPropsCommand {
    fn label(&self) -> String {
        format!("修改地图「{}」属性", self.map_id)
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
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? =
            Some((node.name.clone(), node.unit_per_km));
        node.name = self.new_name.clone();
        node.unit_per_km = self.new_unit_per_km;
        Ok(vec![format!("maps/{}/index.yaml", self.map_id)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (name, unit_per_km) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("UpdateMapProps 尚未 apply".into()))?;
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.map_id)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map_id)))?;
        node.name = name;
        node.unit_per_km = unit_per_km;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 地图树垃圾桶（2026-10-27 spec §5 D5 / §10 追记 1）
// ---------------------------------------------------------------------------

/// 软删整棵子树进垃圾桶。
///
/// 只在**被删子树的根**上打 `trashed` + `trashed_at`；children 引用与父图
/// 钻取图标**原样保留**，显隐由渲染层按 [`World::map_alive`] 派生——
/// 保存/重启后（Undo 栈不持久）「恢复」依然完整，彻底删除时才物理清理。
///
/// ## root_map 改写
///
/// 被删的恰是 `meta.root_map` 时，顺带把 root_map 改写为幸存的第一张根图
/// （`World::alive_roots` 序）——这是**首条改动 `meta` 的 EditCommand**：
/// 不改写的话，下次启动 `ensure_root` 会把钻取轨迹钉在一张隐形图上。
///
/// ## 守卫
///
/// 最后一张存活根图不可删（工程至少保留一张地图）。
///
/// ## 与 Undo 的关系
///
/// `revert` 完整可逆——Ctrl+Z 撤销删除与垃圾桶面板「恢复」是等效的两条
/// 回家路（spec §6）。
#[derive(Debug)]
pub struct TrashMapCommand {
    pub map_id: Id,
    pub trashed_at: String,
    /// root_map 改写前的旧值（仅当 apply 发生了改写才非空）。
    before_root_map: std::sync::Mutex<Option<Id>>,
}

impl TrashMapCommand {
    pub fn new(map_id: Id, trashed_at: impl Into<String>) -> Self {
        Self {
            map_id,
            trashed_at: trashed_at.into(),
            before_root_map: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for TrashMapCommand {
    fn label(&self) -> String {
        format!("删除地图「{}」", self.map_id.as_str())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 借用分趟：alive_roots 计算需要 &world，先取只读事实再改。
        let (is_root, already_trashed) = {
            let Some(node) = world.maps.iter().find(|m| m.id == self.map_id) else {
                return Err(EditError::TargetMissing(format!("地图 `{}`", self.map_id)));
            };
            (node.parent.is_none(), node.trashed)
        };
        if already_trashed {
            return Err(EditError::Precondition(format!(
                "地图 `{}` 已在垃圾桶中",
                self.map_id
            )));
        }
        if is_root && world.alive_roots().len() <= 1 {
            return Err(EditError::Precondition("最后一张地图不可删除".into()));
        }
        if let Some(node) = world.maps.iter_mut().find(|m| m.id == self.map_id) {
            node.trashed = true;
            node.trashed_at = Some(self.trashed_at.clone());
        }
        // root_map 改写：此时目标已打标，alive_roots 自动排除它。
        if world
            .meta
            .as_ref()
            .is_some_and(|m| m.root_map == self.map_id)
        {
            let next = world
                .alive_roots()
                .into_iter()
                .next()
                .ok_or_else(|| EditError::Precondition("没有幸存根图可改写 root_map".into()))?;
            if let Some(meta) = world.meta.as_mut() {
                let old = std::mem::replace(&mut meta.root_map, next);
                *self.before_root_map.lock().expect("before_root_map 锁") = Some(old);
            }
        }
        Ok(vec!["maps".to_owned()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        {
            let Some(node) = world.maps.iter_mut().find(|m| m.id == self.map_id) else {
                return Err(EditError::TargetMissing(format!("地图 `{}`", self.map_id)));
            };
            node.trashed = false;
            node.trashed_at = None;
        }
        if let Some(old) = self
            .before_root_map
            .lock()
            .expect("before_root_map 锁")
            .take()
        {
            if let Some(meta) = world.meta.as_mut() {
                meta.root_map = old;
            }
        }
        Ok(())
    }
}

/// 从垃圾桶整条还原（spec §5 D5 + §6「嵌套恢复守卫」）。
///
/// 软删只动了根上的标志（children/图标从未被动过），恢复 = 翻回标志；
/// 守卫：原父图必须**存活**——父图仍在垃圾桶 → 提示先恢复父图，
/// 父图已被彻底删除 → 明确拒绝。不产生挂在已删父图下的僵尸节点。
///
/// 恢复本身可撤销（revert 翻回垃圾桶态）。
#[derive(Debug)]
pub struct RestoreMapCommand {
    pub map_id: Id,
}

impl RestoreMapCommand {
    pub fn new(map_id: Id) -> Self {
        Self { map_id }
    }
}

impl EditCommand for RestoreMapCommand {
    fn label(&self) -> String {
        format!("恢复地图「{}」", self.map_id.as_str())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let Some(node) = world.maps.iter().find(|m| m.id == self.map_id) else {
            return Err(EditError::TargetMissing(format!("地图 `{}`", self.map_id)));
        };
        if !node.trashed {
            return Err(EditError::Precondition(format!(
                "地图 `{}` 不在垃圾桶中",
                self.map_id
            )));
        }
        if let Some(parent_id) = &node.parent {
            if !world.maps.iter().any(|m| &m.id == parent_id) {
                return Err(EditError::Precondition(format!(
                    "原父图 `{parent_id}` 已被彻底删除，无法恢复"
                )));
            }
            if !world.map_alive(parent_id) {
                return Err(EditError::Precondition("先恢复父图".into()));
            }
        }
        if let Some(node) = world.maps.iter_mut().find(|m| m.id == self.map_id) {
            node.trashed = false;
            node.trashed_at = None;
        }
        Ok(vec!["maps".to_owned()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let Some(node) = world.maps.iter_mut().find(|m| m.id == self.map_id) else {
            return Err(EditError::TargetMissing(format!("地图 `{}`", self.map_id)));
        };
        // 回到垃圾桶态；时间戳丢失可接受（垃圾桶按次存放，撤销恢复本就罕见）。
        node.trashed = true;
        node.trashed_at = None;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PasteMapCommand（深拷贝粘贴 · 2026-10-27 spec §5 D2–D4 / §7 深拷贝要点）
// ---------------------------------------------------------------------------

/// 粘贴产物账本（revert 用）：记录本次新建的全部键。
/// ID 在创建时即唯一，revert 按 ID 移除即可完整回退。
#[derive(Debug, Default)]
struct PasteLedger {
    root: Id,
    map_ids: Vec<Id>,
    layer_ids: Vec<Id>,
    place_ids: Vec<Id>,
    edge_ids: Vec<Id>,
    region_ids: Vec<Id>,
    sea_ids: Vec<Id>,
    /// 追加的几何 span 身份键 `(map, owner, span.from)`——span 无 ID，按三元组对账。
    span_keys: Vec<(Id, Id, i64)>,
}

/// 在 `reserved` 之外找第一个空闲的 `<base>_<n>`（n 从 2 起），并把它登记进 reserved。
///
/// 批量换新 ID 时，后续候选不能撞上**同批**已取的名字——world 里查不到
/// 同批新 ID，只有 reserved 集合兜得住。
fn fresh_suffixed_id(base: &str, reserved: &mut std::collections::HashSet<String>) -> Id {
    for n in 2u32.. {
        let cand = format!("{base}_{n}");
        if reserved.insert(cand.clone()) {
            return Id::new_unchecked(cand);
        }
    }
    unreachable!("u32 序列不可能穷尽")
}

/// 兄弟内名字唯一化：无冲突保原名，有冲突依次 `名 2`、`名 3`（spec D4）。
fn unique_sibling_name(siblings: &[String], name: &str) -> String {
    if !siblings.iter().any(|s| s == name) {
        return name.to_owned();
    }
    for n in 2u32.. {
        let cand = format!("{name} {n}");
        if !siblings.iter().any(|s| s == &cand) {
            return cand;
        }
    }
    unreachable!()
}

/// 平移扁平点列（`[x0, y0, x1, y1, ...]`）。
fn shift_points(pts: &[f32], delta: &Point) -> Vec<f32> {
    let mut out = Vec::with_capacity(pts.len());
    for pair in pts.chunks(2) {
        out.push(pair[0] + delta.x);
        if let Some(y) = pair.get(1) {
            out.push(y + delta.y);
        }
    }
    out
}

/// 把 `GeometryData` 的全部顶点平移 `delta`（Polygon 变体；Points/Ref 原样）。
fn shift_geometry(geo: &GeometryData, delta: &Point) -> GeometryData {
    match geo {
        GeometryData::Polygon(pts) => GeometryData::Polygon(shift_points(pts, delta)),
        GeometryData::PolygonWithHoles { outer, holes } => GeometryData::PolygonWithHoles {
            outer: shift_points(outer, delta),
            holes: holes.iter().map(|h| shift_points(h, delta)).collect(),
        },
        other => other.clone(),
    }
}

/// 轮廓质心（outline 无 extent.origin 时的锚定参照）。仅 Polygon/Points。
fn outline_centroid(geo: Option<&GeometryData>) -> Option<Point> {
    let pts = match geo? {
        GeometryData::Polygon(p) => p,
        GeometryData::Points(p) => p,
        _ => return None,
    };
    if pts.is_empty() {
        return None;
    }
    let (mut sx, mut sy, mut n) = (0.0f32, 0.0f32, 0usize);
    for pair in pts.chunks(2) {
        sx += pair[0];
        sy += pair.get(1).copied().unwrap_or(0.0);
        n += 1;
    }
    Some(Point::new(sx / n as f32, sy / n as f32))
}

/// 深拷贝粘贴整棵子树（spec §5 D2–D4）。
///
/// - ID 全量重生成（地图 / 图层），名字仅根节点按兄弟补号；
/// - 根节点按 `anchor` 锚定（先夹进目标图 extent），outline 随 delta 平移，
///   后代 LOCAL 数据不动（`MoveCityCommand` 同款平移语义）；
/// - 逐图数据（terrain / streets / buildings / measurements）按新键整份克隆；
/// - 副本与源**零共享**：任何 ID 不复用（撞车即覆盖 BTreeMap 键）。
///
/// ## 层级门控
///
/// `World → None`（独立新根）、`Region|City → World`、`Interior → City`；
/// 不匹配报 `Precondition`。
///
/// ## revert
///
/// 一次性命令：`apply` 后再 `apply` 报错；`revert` 按账本移除全部新建键。
#[derive(Debug, Clone)]
pub struct PasteMapCommand {
    pub source_id: Id,
    /// 粘贴落点父图；`None` = 世界图源落为独立新根。
    pub target_parent: Option<Id>,
    /// 落点锚（目标图局部坐标；目标图有 extent 时先夹进范围）。
    pub anchor: Point,
    /// `Arc` 共享：提交方克隆一份，apply 后仍能读 `created_root` 导航
    /// （命令本体已被 `Box` 进 History，拿不回来）。
    ledger: std::sync::Arc<std::sync::Mutex<Option<PasteLedger>>>,
}

impl PasteMapCommand {
    pub fn new(source_id: Id, target_parent: Option<Id>, anchor: Point) -> Self {
        Self {
            source_id,
            target_parent,
            anchor,
            ledger: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// `apply` 后新建子树的根 ID（`revert` 后回 `None`）——粘贴后导航用。
    pub fn created_root(&self) -> Option<Id> {
        self.ledger
            .lock()
            .expect("paste ledger 锁")
            .as_ref()
            .map(|l| l.root.clone())
    }
}

impl EditCommand for PasteMapCommand {
    fn label(&self) -> String {
        format!("粘贴地图「{}」", self.source_id.as_str())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if self.ledger.lock().expect("paste ledger 锁").is_some() {
            return Err(EditError::Precondition(
                "粘贴命令已应用（一次性命令）".into(),
            ));
        }
        let Some(source) = world.maps.iter().find(|m| m.id == self.source_id).cloned() else {
            return Err(EditError::TargetMissing(format!(
                "地图 `{}`",
                self.source_id
            )));
        };
        if !world.map_alive(&self.source_id) {
            return Err(EditError::Precondition(
                "源图在垃圾桶中，先恢复再复制".into(),
            ));
        }

        // 目标图（Some 时必须存在）+ 层级门控。
        let target = match &self.target_parent {
            Some(t) => {
                let node = world
                    .maps
                    .iter()
                    .find(|m| &m.id == t)
                    .ok_or_else(|| EditError::TargetMissing(format!("目标图 `{t}`")))?;
                Some(node.clone())
            }
            None => None,
        };
        let level_ok = match source.level {
            MapLevel::World => target.is_none(),
            MapLevel::Region | MapLevel::City => {
                target.as_ref().is_some_and(|t| t.level == MapLevel::World)
            }
            MapLevel::Interior => target.as_ref().is_some_and(|t| t.level == MapLevel::City),
        };
        if !level_ok {
            let (src, allowed) = match source.level {
                MapLevel::World => ("世界图", "独立根位置"),
                MapLevel::Region => ("区域图", "世界图下"),
                MapLevel::City => ("城市图", "世界图下"),
                MapLevel::Interior => ("内部图", "城市图下"),
            };
            return Err(EditError::Precondition(format!(
                "层级不匹配：{src} 只能粘贴到{allowed}"
            )));
        }

        // 锚点夹进目标图 extent（origin 缺省按 (0,0)；独立新根不夹）。
        let mut anchor = self.anchor;
        if let Some(ext) = target.as_ref().and_then(|t| t.extent.as_ref()) {
            let (ox, oy) = match ext.origin {
                Some(p) => (p.x, p.y),
                None => (0.0, 0.0),
            };
            anchor.x = anchor.x.clamp(ox, ox + ext.width);
            anchor.y = anchor.y.clamp(oy, oy + ext.height);
        }

        // 平移量 = 锚点 − 源根当前位置（extent.origin → outline 质心 → 零位移）。
        let src_pos = source
            .extent
            .as_ref()
            .and_then(|e| e.origin)
            .or_else(|| outline_centroid(source.outline_polygon.as_ref()))
            .unwrap_or(anchor);
        let delta = Point::new(anchor.x - src_pos.x, anchor.y - src_pos.y);

        // ID 换新：地图批 + 图层批（图层名空间 = 全部图的 layers ∪ streets/buildings 键）。
        let subtree = world.subtree_ids(&self.source_id);
        let mut reserved: std::collections::HashSet<String> = world
            .maps
            .iter()
            .map(|m| m.id.as_str().to_owned())
            .collect();
        let mut layer_reserved: std::collections::HashSet<String> = world
            .maps
            .iter()
            .flat_map(|m| m.layers.iter().map(|l| l.id.as_str().to_owned()))
            .collect();
        for k in world.streets.keys().chain(world.buildings.keys()) {
            layer_reserved.insert(k.as_str().to_owned());
        }
        let mut id_map: std::collections::HashMap<Id, Id> = std::collections::HashMap::new();
        for old in &subtree {
            let fresh = fresh_suffixed_id(old.as_str(), &mut reserved);
            id_map.insert(old.clone(), fresh);
        }
        let new_root = id_map
            .get(&self.source_id)
            .expect("根必在子树映射中")
            .clone();

        // 根名字补号（兄弟 = 目标父的 children；独立根 = 全部根）。
        let siblings: Vec<String> = match &target {
            Some(t) => t
                .children
                .iter()
                .filter_map(|c| world.maps.iter().find(|m| &m.id == c))
                .map(|m| m.name.clone())
                .collect(),
            None => world
                .maps
                .iter()
                .filter(|m| m.parent.is_none())
                .map(|m| m.name.clone())
                .collect(),
        };
        let root_name = unique_sibling_name(&siblings, &source.name);

        // 逐节点克隆（先备好后提交，避免在遍历中改 world.maps）。
        let mut layer_map: std::collections::HashMap<(Id, Id), Id> =
            std::collections::HashMap::new();
        let mut new_nodes: Vec<MapNode> = Vec::with_capacity(subtree.len());
        for old in &subtree {
            let node = world
                .maps
                .iter()
                .find(|m| &m.id == old)
                .expect("子树节点必在");
            let new_id = id_map.get(old).expect("映射必在").clone();
            let mut nn = node.clone();
            nn.id = new_id.clone();
            nn.parent = if old == &self.source_id {
                self.target_parent.clone()
            } else {
                node.parent
                    .as_ref()
                    .map(|p| id_map.get(p).cloned().unwrap_or_else(|| p.clone()))
            };
            nn.children = node
                .children
                .iter()
                .map(|c| id_map.get(c).cloned().unwrap_or_else(|| c.clone()))
                .collect();
            if old == &self.source_id {
                nn.name = root_name.clone();
            }
            // 图层：ID 换新 + source 路径前缀改写到新图目录。
            let mut new_layers = Vec::with_capacity(node.layers.len());
            for layer in &node.layers {
                let mut nl = layer.clone();
                let key = (node.id.clone(), layer.id.clone());
                let new_lid = match layer_map.get(&key) {
                    Some(prev) => prev.clone(),
                    None => {
                        let fresh = fresh_suffixed_id(layer.id.as_str(), &mut layer_reserved);
                        layer_map.insert(key, fresh.clone());
                        fresh
                    }
                };
                nl.id = new_lid;
                if let Some(src) = &nl.source {
                    let old_prefix = format!("maps/{}/", node.id.as_str());
                    if let Some(rest) = src.strip_prefix(&old_prefix) {
                        nl.source = Some(format!("maps/{}/{rest}", new_id.as_str()));
                    }
                }
                new_layers.push(nl);
            }
            nn.layers = new_layers;
            // 钻取图标重映射到新后代。
            for icon in &mut nn.icons {
                if let Some(d) = &icon.drill_into {
                    if let Some(m) = id_map.get(d) {
                        icon.drill_into = Some(m.clone());
                    }
                }
            }
            // 根锚定：extent.origin 落锚点，outline 随 delta 平移。
            if old == &self.source_id {
                if let Some(ext) = &mut nn.extent {
                    ext.origin = Some(anchor);
                }
                if let Some(geo) = &nn.outline_polygon {
                    nn.outline_polygon = Some(shift_geometry(geo, &delta));
                }
            }
            new_nodes.push(nn);
        }

        // 逐图数据搬家（terrain / measurements 按图 ID；streets / buildings 按图层 ID）。
        for old in &subtree {
            let new_id = id_map.get(old).expect("映射必在");
            if let Some(t) = world.terrains.get(old).cloned() {
                world.set_terrain(new_id.clone(), t);
            }
            if let Some(m) = world.measurements.get(old).cloned() {
                world.measurements.insert(new_id.clone(), m);
            }
            let old_layers: Vec<Id> = world
                .maps
                .iter()
                .find(|m| &m.id == old)
                .expect("在")
                .layers
                .iter()
                .map(|l| l.id.clone())
                .collect();
            for old_layer in old_layers {
                let new_layer = layer_map
                    .get(&(old.clone(), old_layer.clone()))
                    .expect("图层映射必在");
                if let Some(doc) = world.streets.get(&old_layer).cloned() {
                    world.streets.insert(new_layer.clone(), doc);
                }
                if let Some(doc) = world.buildings.get(&old_layer).cloned() {
                    world.buildings.insert(new_layer.clone(), doc);
                }
            }
        }

        // 提交节点 + 父图 children 回填。
        for nn in new_nodes {
            world.maps.push(nn);
        }
        if let Some(t) = self.target_parent.clone() {
            if let Some(p) = world.maps.iter_mut().find(|m| m.id == t) {
                p.children.push(new_root.clone());
            }
        }

        // —— 实体深拷贝（spec §7：按 map 归属收集，Id 全量重生成）——
        let subtree_set: std::collections::HashSet<Id> = subtree.iter().cloned().collect();
        let remap_map = |m: &Id| id_map.get(m).cloned().unwrap_or_else(|| m.clone());

        // Place：map ∈ 子树 → 克隆 + 新 id + map 重映射（本图局部坐标不动）。
        let mut place_map: std::collections::HashMap<Id, Id> = std::collections::HashMap::new();
        let mut place_ids: Vec<Id> = Vec::new();
        let mut place_reserved: std::collections::HashSet<String> = world
            .distances
            .places
            .iter()
            .map(|p| p.id.as_str().to_owned())
            .collect();
        let mut new_places = Vec::new();
        for p in world
            .distances
            .places
            .iter()
            .filter(|p| p.map.as_ref().is_some_and(|m| subtree_set.contains(m)))
        {
            let mut np = p.clone();
            np.id = fresh_suffixed_id(p.id.as_str(), &mut place_reserved);
            np.map = p.map.as_ref().map(&remap_map);
            place_map.insert(p.id.clone(), np.id.clone());
            place_ids.push(np.id.clone());
            new_places.push(np);
        }
        world.distances.places.append(&mut new_places);

        // Edge：两端都在复制集内才跟随（端点重映射）；跨界边属于源世界距离图，不搬。
        let mut edge_ids: Vec<Id> = Vec::new();
        let mut edge_reserved: std::collections::HashSet<String> = world
            .distances
            .edges
            .iter()
            .map(|e| e.id.as_str().to_owned())
            .collect();
        let mut new_edges = Vec::new();
        for e in world.distances.edges.iter() {
            if let (Some(a), Some(b)) = (place_map.get(&e.a), place_map.get(&e.b)) {
                let mut ne = e.clone();
                ne.id = fresh_suffixed_id(e.id.as_str(), &mut edge_reserved);
                ne.a = a.clone();
                ne.b = b.clone();
                edge_ids.push(ne.id.clone());
                new_edges.push(ne);
            }
        }
        world.distances.edges.append(&mut new_edges);

        // Region / Sea：克隆 + id 换新 + map 重映射。
        let mut region_ids: Vec<Id> = Vec::new();
        let mut region_reserved: std::collections::HashSet<String> = world
            .regions
            .iter()
            .map(|r| r.id.as_str().to_owned())
            .collect();
        let mut new_regions = Vec::new();
        for r in world
            .regions
            .iter()
            .filter(|r| subtree_set.contains(&r.map))
        {
            let mut nr = r.clone();
            nr.id = fresh_suffixed_id(r.id.as_str(), &mut region_reserved);
            nr.map = remap_map(&r.map);
            region_ids.push(nr.id.clone());
            new_regions.push(nr);
        }
        world.regions.append(&mut new_regions);

        let mut sea_ids: Vec<Id> = Vec::new();
        let mut sea_reserved: std::collections::HashSet<String> = world
            .seas
            .iter()
            .map(|s| s.id.as_str().to_owned())
            .collect();
        let mut new_seas = Vec::new();
        for sea in world.seas.iter().filter(|s| subtree_set.contains(&s.map)) {
            let mut ns = sea.clone();
            ns.id = fresh_suffixed_id(sea.id.as_str(), &mut sea_reserved);
            ns.map = remap_map(&sea.map);
            sea_ids.push(ns.id.clone());
            new_seas.push(ns);
        }
        world.seas.append(&mut new_seas);

        // 几何 span：map ∈ 子树 → 克隆 + map 重映射；place_geometry 的 owner
        // 随 Place 换新，faction_geometry 的 owner（全局势力实体）保留。
        let mut span_keys: Vec<(Id, Id, i64)> = Vec::new();
        let mut new_pg = Vec::new();
        for g in world
            .place_geometry
            .iter()
            .filter(|g| subtree_set.contains(&g.map))
        {
            let mut ng = g.clone();
            ng.map = remap_map(&g.map);
            ng.owner = place_map
                .get(&g.owner)
                .cloned()
                .unwrap_or_else(|| g.owner.clone());
            span_keys.push((ng.map.clone(), ng.owner.clone(), ng.span.from));
            new_pg.push(ng);
        }
        world.place_geometry.append(&mut new_pg);
        let mut new_fg = Vec::new();
        for g in world
            .faction_geometry
            .iter()
            .filter(|g| subtree_set.contains(&g.map))
        {
            let mut ng = g.clone();
            ng.map = remap_map(&g.map);
            span_keys.push((ng.map.clone(), ng.owner.clone(), ng.span.from));
            new_fg.push(ng);
        }
        world.faction_geometry.append(&mut new_fg);

        let layer_ids: Vec<Id> = layer_map.values().cloned().collect();
        *self.ledger.lock().expect("paste ledger 锁") = Some(PasteLedger {
            root: new_root,
            map_ids: id_map.values().cloned().collect(),
            layer_ids,
            place_ids,
            edge_ids,
            region_ids,
            sea_ids,
            span_keys,
        });
        Ok(vec![
            "maps".to_owned(),
            "distances/places".to_owned(),
            "distances/edges".to_owned(),
            "geometry/place".to_owned(),
            "geometry/faction".to_owned(),
            "regions".to_owned(),
            "seas".to_owned(),
        ])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let Some(l) = self.ledger.lock().expect("paste ledger 锁").take() else {
            return Err(EditError::Precondition("粘贴尚未应用，无法撤销".into()));
        };
        world.maps.retain(|m| !l.map_ids.contains(&m.id));
        if let Some(t) = &self.target_parent {
            if let Some(p) = world.maps.iter_mut().find(|m| &m.id == t) {
                p.children.retain(|c| c != &l.root);
            }
        }
        for id in &l.map_ids {
            world.terrains.remove(id);
            world.measurements.remove(id);
        }
        for id in &l.layer_ids {
            world.streets.remove(id);
            world.buildings.remove(id);
        }
        world
            .distances
            .places
            .retain(|p| !l.place_ids.contains(&p.id));
        world
            .distances
            .edges
            .retain(|e| !l.edge_ids.contains(&e.id));
        world.regions.retain(|r| !l.region_ids.contains(&r.id));
        world.seas.retain(|s| !l.sea_ids.contains(&s.id));
        world.place_geometry.retain(|g| {
            !l.span_keys
                .contains(&(g.map.clone(), g.owner.clone(), g.span.from))
        });
        world.faction_geometry.retain(|g| {
            !l.span_keys
                .contains(&(g.map.clone(), g.owner.clone(), g.span.from))
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PurgeMapCommand（彻底删除 · 2026-10-27 spec §5 D7 / §10 追记 2）
// ---------------------------------------------------------------------------

/// 彻底删除的完整快照（revert 兜底用）。
///
/// 正常路径下本命令经 wv-app 的「直应用」通道提交（不进 Undo 栈，
/// spec §10 追记 2），快照永远不会被读到；但实现完整 revert，万一误入
/// 栈也不崩——这是防御性兜底，不是功能承诺。
#[derive(Debug, Default)]
struct PurgeSnapshot {
    /// 被移除的全部节点（原声明顺序，含 trashed 标志原样）。
    maps: Vec<MapNode>,
    /// 父图信息：`(父 id, map_id 原在 children 中的下标, 被摘除的钻取图标)`。
    parent: Option<(Id, usize, Vec<IconSpec>)>,
    terrains: Vec<(Id, wv_core::terrain::TerrainData)>,
    streets: Vec<(Id, wv_core::source::street::StreetLayer)>,
    buildings: Vec<(Id, wv_core::source::building::BuildingLayer)>,
    measurements: Vec<(Id, wv_core::source::measurement::MeasurementOverlay)>,
    places: Vec<wv_core::distance::Place>,
    edges: Vec<wv_core::distance::PlaceEdge>,
    regions: Vec<wv_core::model::region::Region>,
    seas: Vec<wv_core::map::sea::Sea>,
    place_geometry: Vec<GeometrySpan>,
    faction_geometry: Vec<GeometrySpan>,
}

/// 彻底删除整棵子树（真删：内存移除 + 脏路径全量标记，保存时物理删目录）。
///
/// ## 前置
///
/// 只接受**已在垃圾桶**的图（软删是硬删的必经之路，防止误触直接蒸发）。
///
/// ## 清理范围
///
/// 子树全部节点 + 挂在世父图上的 children 引用与钻取图标（此时才物理清理，
/// 软删期间它们靠派生显隐活着）+ 逐图数据（terrain/streets/buildings/
/// measurements）+ 实体（places/regions/seas/双几何 span）+ 端点落在被删
/// places 上的边。
#[derive(Debug)]
pub struct PurgeMapCommand {
    pub map_id: Id,
    snapshot: std::sync::Mutex<Option<PurgeSnapshot>>,
}

impl PurgeMapCommand {
    pub fn new(map_id: Id) -> Self {
        Self {
            map_id,
            snapshot: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for PurgeMapCommand {
    fn label(&self) -> String {
        format!("彻底删除地图「{}」", self.map_id.as_str())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        if self.snapshot.lock().expect("purge snapshot 锁").is_some() {
            return Err(EditError::Precondition("命令已应用（一次性命令）".into()));
        }
        let Some(node) = world.maps.iter().find(|m| m.id == self.map_id) else {
            return Err(EditError::TargetMissing(format!("地图 `{}`", self.map_id)));
        };
        if !node.trashed {
            return Err(EditError::Precondition(format!(
                "地图 `{}` 不在垃圾桶中——只有垃圾桶内的图才能彻底删除",
                self.map_id
            )));
        }
        let parent_id = node.parent.clone();
        let subtree = world.subtree_ids(&self.map_id);
        let set: std::collections::HashSet<Id> = subtree.iter().cloned().collect();

        let mut snap = PurgeSnapshot::default();

        // 父图：摘 children + 清悬空钻取图标（物理清理只在此时发生）。
        if let Some(pid) = &parent_id {
            if let Some(pos) = world.maps.iter().position(|m| &m.id == pid) {
                let p = &mut world.maps[pos];
                let idx = p.children.iter().position(|c| c == &self.map_id);
                p.children.retain(|c| !set.contains(c));
                let mut removed_icons = Vec::new();
                p.icons.retain(|ic| match &ic.drill_into {
                    Some(d) if set.contains(d) => {
                        removed_icons.push(ic.clone());
                        false
                    }
                    _ => true,
                });
                snap.parent = Some((pid.clone(), idx.unwrap_or(p.children.len()), removed_icons));
            }
        }

        // 收集被删节点的图层 ID（streets/buildings 的键）。
        let layer_ids: Vec<Id> = world
            .maps
            .iter()
            .filter(|m| set.contains(&m.id))
            .flat_map(|m| m.layers.iter().map(|l| l.id.clone()))
            .collect();

        // 节点移除。
        snap.maps = world
            .maps
            .iter()
            .filter(|m| set.contains(&m.id))
            .cloned()
            .collect();
        world.maps.retain(|m| !set.contains(&m.id));

        // 逐图数据。
        for id in &set {
            if let Some(t) = world.terrains.remove(id) {
                snap.terrains.push((id.clone(), t));
            }
            if let Some(m) = world.measurements.remove(id) {
                snap.measurements.push((id.clone(), m));
            }
        }
        for lid in layer_ids {
            if let Some(doc) = world.streets.remove(&lid) {
                snap.streets.push((lid.clone(), doc));
            }
            if let Some(doc) = world.buildings.remove(&lid) {
                snap.buildings.push((lid.clone(), doc));
            }
        }

        // 实体：按 map 归属收集后移除；边按「端点 ∈ 被删 places」级联。
        let removed_place_ids: std::collections::HashSet<Id> = world
            .distances
            .places
            .iter()
            .filter(|p| p.map.as_ref().is_some_and(|m| set.contains(m)))
            .map(|p| p.id.clone())
            .collect();
        let mut kept_places = Vec::new();
        for p in std::mem::take(&mut world.distances.places) {
            if removed_place_ids.contains(&p.id) {
                snap.places.push(p);
            } else {
                kept_places.push(p);
            }
        }
        world.distances.places = kept_places;
        let mut kept_edges = Vec::new();
        for e in std::mem::take(&mut world.distances.edges) {
            if removed_place_ids.contains(&e.a) || removed_place_ids.contains(&e.b) {
                snap.edges.push(e);
            } else {
                kept_edges.push(e);
            }
        }
        world.distances.edges = kept_edges;
        let mut kept_regions = Vec::new();
        for r in std::mem::take(&mut world.regions) {
            if set.contains(&r.map) {
                snap.regions.push(r);
            } else {
                kept_regions.push(r);
            }
        }
        world.regions = kept_regions;
        let mut kept_seas = Vec::new();
        for s in std::mem::take(&mut world.seas) {
            if set.contains(&s.map) {
                snap.seas.push(s);
            } else {
                kept_seas.push(s);
            }
        }
        world.seas = kept_seas;
        let mut kept_pg = Vec::new();
        for g in std::mem::take(&mut world.place_geometry) {
            if set.contains(&g.map) {
                snap.place_geometry.push(g);
            } else {
                kept_pg.push(g);
            }
        }
        world.place_geometry = kept_pg;
        let mut kept_fg = Vec::new();
        for g in std::mem::take(&mut world.faction_geometry) {
            if set.contains(&g.map) {
                snap.faction_geometry.push(g);
            } else {
                kept_fg.push(g);
            }
        }
        world.faction_geometry = kept_fg;

        *self.snapshot.lock().expect("purge snapshot 锁") = Some(snap);
        Ok(vec![
            "maps".to_owned(),
            "distances/places".to_owned(),
            "distances/edges".to_owned(),
            "geometry/place".to_owned(),
            "geometry/faction".to_owned(),
            "regions".to_owned(),
            "seas".to_owned(),
        ])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let Some(snap) = self.snapshot.lock().expect("purge snapshot 锁").take() else {
            return Err(EditError::Precondition("命令尚未应用，无法回退".into()));
        };
        for node in snap.maps {
            world.maps.push(node);
        }
        if let Some((pid, idx, icons)) = snap.parent {
            if let Some(p) = world.maps.iter_mut().find(|m| m.id == pid) {
                let idx = idx.min(p.children.len());
                p.children.insert(idx, self.map_id.clone());
                p.icons.extend(icons);
            }
        }
        for (id, t) in snap.terrains {
            world.set_terrain(id, t);
        }
        for (id, doc) in snap.streets {
            world.streets.insert(id, doc);
        }
        for (id, doc) in snap.buildings {
            world.buildings.insert(id, doc);
        }
        for (id, doc) in snap.measurements {
            world.measurements.insert(id, doc);
        }
        world.distances.places.extend(snap.places);
        world.distances.edges.extend(snap.edges);
        world.regions.extend(snap.regions);
        world.seas.extend(snap.seas);
        world.place_geometry.extend(snap.place_geometry);
        world.faction_geometry.extend(snap.faction_geometry);
        Ok(())
    }
}
