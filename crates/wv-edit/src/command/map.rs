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
use wv_core::map::{GeometryData, IconShape, IconSpec, MapNode};
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
