//! overlay 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

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

/// 改图层声明属性（可见 / 锁定 / 不透明度）——图层面板写回。
///
/// 《MAP_LAYER_RENDERING_SPEC》§8 缺口 #4 的收口：作者此前只能手编
/// `maps/<id>/index.yaml` 的 `layers:` 段，本命令让面板的眼睛 / 锁 /
/// 不透明度滑条落进编辑命令链（可撤销、驱动脏路径）。`z` 槽位偏移与
/// `source` 引用不在本命令范围。
///
/// 三个字段都是 `Option`：`None` = 不动。`apply` 把旧值三元组缓存进
/// `before`，`revert` 原样写回（运行期撤销走命令栈，跨会话历史由 YAML
/// 真源承载——图层显隐是**作者声明**而非「随时间演化的量」，不适用
/// §4.1 的区间化，P6 的非破坏语义由命令栈的 revert 保证）。
///
/// ## 脏路径
///
/// `maps/{map_id}/index.yaml`——layers 声明内联在图索引里，与
/// [`SetMapOrientationCommand`] 同一落点。
///
/// ## 失败语义
///
/// - 地图或图层不存在：`TargetMissing`（面板帧数据与命令构造同源，
///   正常不会触发；触发说明世界在帧间被换掉）。
#[derive(Debug)]
pub struct UpdateLayerCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 目标图层 ID（`MapNode.layers[].id`）。
    pub layer: wv_core::Id,
    /// 新可见性；`None` = 不动。
    pub visible: Option<bool>,
    /// 新锁定态；`None` = 不动。
    pub locked: Option<bool>,
    /// 新不透明度（apply 时 clamp 到 0..=1）；`None` = 不动。
    pub opacity: Option<f32>,
    /// `apply` 时缓存的旧值三元组 `(visible, locked, opacity)`。
    before: std::sync::Mutex<Option<(bool, bool, f32)>>,
}

impl UpdateLayerCommand {
    /// 只改可见性（图层面板的眼睛）。
    pub fn visibility(map: wv_core::Id, layer: wv_core::Id, visible: bool) -> Self {
        Self {
            map,
            layer,
            visible: Some(visible),
            locked: None,
            opacity: None,
            before: std::sync::Mutex::new(None),
        }
    }

    /// 只改锁定态（图层面板的锁）。
    pub fn locked(map: wv_core::Id, layer: wv_core::Id, locked: bool) -> Self {
        Self {
            map,
            layer,
            visible: None,
            locked: Some(locked),
            opacity: None,
            before: std::sync::Mutex::new(None),
        }
    }

    /// 只改不透明度（图层面板滑条，拖完一次性提交，不按帧碎提交）。
    pub fn opacity(map: wv_core::Id, layer: wv_core::Id, opacity: f32) -> Self {
        Self {
            map,
            layer,
            visible: None,
            locked: None,
            opacity: Some(opacity),
            before: std::sync::Mutex::new(None),
        }
    }

    /// 定位目标图层的可变引用（地图 → 图层两级查找不到都报 `TargetMissing`）。
    fn layer_mut<'w>(&self, world: &'w mut World) -> EditResult<&'w mut wv_core::map::Layer> {
        let node = world
            .maps
            .iter_mut()
            .find(|m| m.id == self.map)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map)))?;
        node.layers
            .iter_mut()
            .find(|l| l.id == self.layer)
            .ok_or_else(|| {
                EditError::TargetMissing(format!("图层 {}（地图 {}）", self.layer, self.map))
            })
    }
}

impl EditCommand for UpdateLayerCommand {
    fn label(&self) -> String {
        if let Some(v) = self.visible {
            format!("图层「{}」{}", self.layer, if v { "显示" } else { "隐藏" })
        } else if let Some(l) = self.locked {
            format!("图层「{}」{}", self.layer, if l { "锁定" } else { "解锁" })
        } else {
            format!(
                "图层「{}」不透明度 {:.0}%",
                self.layer,
                self.opacity.unwrap_or(1.0) * 100.0
            )
        }
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let layer = self.layer_mut(world)?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? =
            Some((layer.visible, layer.locked, layer.opacity));
        if let Some(v) = self.visible {
            layer.visible = v;
        }
        if let Some(l) = self.locked {
            layer.locked = l;
        }
        if let Some(o) = self.opacity {
            layer.opacity = o.clamp(0.0, 1.0);
        }
        Ok(vec![format!("maps/{}/index.yaml", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("UpdateLayer 尚未 apply".into()))?;
        let layer = self.layer_mut(world)?;
        layer.visible = prev.0;
        layer.locked = prev.1;
        layer.opacity = prev.2;
        Ok(())
    }
}

/// 重置当前地形到底图（2026-10-16 用户需求）。
///
/// 把整张图的 `TerrainData.cells` 替换为 [`wv_core::map::MapBase::cell()`]
/// 的复制；`base` 为 `None` 的旧地图按 [`MapBase::Ocean`] 兜底——
/// 向后兼容不锁住「重置」入口。
///
/// ## 与 P6（非破坏编辑）的关系
///
/// `apply` 把**整张** `TerrainData` 缓存进 `before`，`revert` 原样恢复——
/// 一次性写满 N² 格（512² ≈ 786 KB）就是这一命令的「diff」体积。
/// 无内存里别处再持有地形 cells 的可变引用，克隆一份独立即可。
///
/// ## 脏路径
///
/// `maps/{id}/terrain.bin`（与 `PaintTerrainCommand` / `IntentTerrainCommand`
/// 同源）。`index.yaml` **不**动（`base` 字段没变）。
///
/// ## 失败语义
///
/// - 地图不存在：`TargetMissing`
/// - 该图无地形数据（如尚未生成栅格的子图）：`Precondition`，
///   提示「地图「X」无地形数据可重置」——子图继承父图底图时常见。
#[derive(Debug)]
pub struct ResetTerrainToBaseCommand {
    pub map: wv_core::Id,
    before: std::sync::Mutex<Option<wv_core::terrain::TerrainData>>,
}

impl ResetTerrainToBaseCommand {
    pub fn new(map: wv_core::Id) -> Self {
        Self {
            map,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for ResetTerrainToBaseCommand {
    fn label(&self) -> String {
        format!("重置地形「{}」到底图", self.map)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // 1. 节点 → 决定底图（None 兜底 Ocean，向后兼容旧世界）
        let base = world
            .maps
            .iter()
            .find(|m| m.id == self.map)
            .ok_or_else(|| EditError::TargetMissing(format!("地图 {}", self.map)))?
            .base
            .unwrap_or(wv_core::map::MapBase::Ocean);
        // 2. 备份当前地形（含尺寸，决定替换后的 cells 数量）
        let prev = world.terrain_of(&self.map).cloned().ok_or_else(|| {
            EditError::Precondition(format!("地图「{}」无地形数据可重置", self.map))
        })?;
        let width = prev.width;
        let height = prev.height;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))? = Some(prev);
        // 3. 整片替换
        let new = wv_core::terrain::TerrainData {
            width,
            height,
            cells: vec![base.cell(); (width as usize) * (height as usize)],
        };
        world.set_terrain(self.map.clone(), new);
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .take()
            .ok_or_else(|| EditError::Precondition("ResetTerrainToBase 尚未 apply".into()))?;
        world.set_terrain(self.map.clone(), prev);
        Ok(())
    }
}
