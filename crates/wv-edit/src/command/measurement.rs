//! measurement 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

// ---------------------------------------------------------------------------
// 测量编辑命令（工具产物，每图一份，2026-10-14）
// ---------------------------------------------------------------------------

use wv_core::Measurement;
use wv_core::MeasurementOverlay;

/// 受影响的脏路径：测量走虚拟命名空间 `measurements/<map_id>`——避免
/// `maps/<map_id>/` 前缀把同图其它文件（`index.yaml` / `terrain.bin`）误判脏。
/// 与街道 `streets/<layer_id>` 同款越界防护（见 ADR-0010）。
fn measurement_dirty_path(map: &wv_core::Id) -> Vec<String> {
    vec![format!("measurements/{}", map.as_str())]
}

/// 新增一条测量。
///
/// `apply` 在 `world.measurements[map]` 末尾追加；`revert` 按 ID 移除。
///
/// **为什么按 ID 而非索引定位**：测量是用户手画的对象，**ID 是显式声明**
/// 的稳定键，撤销按 ID 还原比按索引更安全（索引会随增删漂移）。
///
/// **顶替语义**：ID 已存在时 `apply` 报 `Precondition`，**不**静默覆盖
/// （命令层「不擅自做作者的决定」）。
#[derive(Debug)]
pub struct AddMeasurementCommand {
    pub map: wv_core::Id,
    pub measurement: Measurement,
}

impl AddMeasurementCommand {
    pub fn new(map: wv_core::Id, measurement: Measurement) -> Self {
        Self { map, measurement }
    }
}

impl EditCommand for AddMeasurementCommand {
    fn label(&self) -> String {
        format!("新增测量「{}」", self.measurement.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let overlay = world.measurement_of_mut(&self.map);
        if overlay.add(self.measurement.clone()).is_err() {
            return Err(EditError::Precondition(format!(
                "测量 `{}` 已存在",
                self.measurement.id
            )));
        }
        Ok(measurement_dirty_path(&self.map))
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let overlay = world.measurement_of_mut(&self.map);
        if overlay.remove(&self.measurement.id).is_none() {
            return Err(EditError::Precondition(
                "AddMeasurement revert 找不到对应记录".into(),
            ));
        }
        Ok(())
    }
}

/// 删除一条测量（按 ID）。
///
/// `apply` 缓存被删的 measurement（位置 + 内容）；`revert` 用缓存恢复。
#[derive(Debug)]
pub struct RemoveMeasurementCommand {
    pub map: wv_core::Id,
    pub id: wv_core::Id,
    before: std::sync::Mutex<Option<(usize, Measurement)>>,
}

impl RemoveMeasurementCommand {
    pub fn new(map: wv_core::Id, id: wv_core::Id) -> Self {
        Self {
            map,
            id,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveMeasurementCommand {
    fn label(&self) -> String {
        format!("删除测量「{}」", self.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let overlay = world.measurement_of_mut(&self.map);
        let pos = overlay
            .measurements
            .iter()
            .position(|m| m.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Measurement {}", self.id)))?;
        let prev = overlay.measurements.remove(pos);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, prev));
        Ok(measurement_dirty_path(&self.map))
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, prev) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveMeasurement 尚未 apply".into()))?;
        let overlay = world.measurement_of_mut(&self.map);
        let insert_at = pos.min(overlay.measurements.len());
        overlay.measurements.insert(insert_at, prev);
        Ok(())
    }
}

/// 清空某张图的全部测量。
///
/// `apply` 缓存整份旧 overlay（可能为空）；`revert` 整份还原。
/// **仅在该图原本非空时**才记脏路径——空→空不写盘（V1：作者没动就别替他造）。
#[derive(Debug)]
pub struct ClearMeasurementsCommand {
    pub map: wv_core::Id,
    before: std::sync::Mutex<Option<MeasurementOverlay>>,
}

impl ClearMeasurementsCommand {
    pub fn new(map: wv_core::Id) -> Self {
        Self {
            map,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for ClearMeasurementsCommand {
    fn label(&self) -> String {
        format!("清空「{}」的全部测量", self.map)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let existing = world.measurement_of(&self.map).cloned();
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = existing.clone();
        if let Some(doc) = existing {
            if doc.is_empty() {
                return Ok(Vec::new());
            }
            world.set_measurement(self.map.clone(), MeasurementOverlay::new());
            return Ok(measurement_dirty_path(&self.map));
        }
        Ok(Vec::new())
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone();
        // 缓存 `None` = `apply` 时该图无 overlay（连键都没有），
        // revert 时也不该留下键。
        // 缓存 `Some(empty_overlay)` = `apply` 时键存在但清单空，
        // revert 还原成空文档。
        // 缓存 `Some(doc)` 且非空 = `apply` 实际清空了一份非空 overlay，
        // revert 整份还原。
        match prev {
            Some(doc) if !doc.is_empty() => {
                world.set_measurement(self.map.clone(), doc);
            }
            Some(_) => {
                world.set_measurement(self.map.clone(), MeasurementOverlay::new());
            }
            None => {
                world.measurements.remove(&self.map);
            }
        }
        Ok(())
    }
}
