//! 区域命令族：新增 / 删除 / 更新区域（ADR-0017 §2.2，自 command.rs 拆分）。

use super::*;

use wv_core::Region;

pub(crate) fn region_dirty_paths() -> Vec<String> {
    vec!["regions".into()]
}

/// 新增一个命名区域。
///
/// `apply` 在 `world.regions` 末尾追加；`revert` 移除（按 ID 定位）。
#[derive(Debug)]
pub struct AddRegionCommand {
    pub region: Region,
}

impl AddRegionCommand {
    pub fn new(region: Region) -> Self {
        Self { region }
    }
}

impl EditCommand for AddRegionCommand {
    fn label(&self) -> String {
        format!("新增区域「{}」", self.region.name)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
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
        let pos = world
            .regions
            .iter()
            .position(|r| r.id == self.region.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.region.id)))?;
        world.regions.remove(pos);
        Ok(())
    }
}

/// 删除一个命名区域。
///
/// `apply` 缓存被删的 Region（用于 revert 还原）；`revert` 按原位恢复。
#[derive(Debug)]
pub struct RemoveRegionCommand {
    pub id: wv_core::Id,
    before: std::sync::Mutex<Option<(usize, Region)>>,
}

impl RemoveRegionCommand {
    pub fn new(id: wv_core::Id) -> Self {
        Self {
            id,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for RemoveRegionCommand {
    fn label(&self) -> String {
        format!("删除区域「{}」", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let pos = world
            .regions
            .iter()
            .position(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        let prev = world.regions.remove(pos);
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some((pos, prev));
        Ok(region_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let (pos, prev) = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("RemoveRegion 尚未 apply".into()))?;
        let insert_at = pos.min(world.regions.len());
        world.regions.insert(insert_at, prev);
        Ok(())
    }
}

/// 更新区域名称 / 轮廓。
///
/// `apply` 缓存旧 Region 全字段；`revert` 用缓存恢复。
#[derive(Debug)]
pub struct UpdateRegionCommand {
    pub id: wv_core::Id,
    pub new_name: String,
    pub new_polygon: Vec<f32>,
    before: std::sync::Mutex<Option<Region>>,
}

impl UpdateRegionCommand {
    pub fn new(id: wv_core::Id, new_name: String, new_polygon: Vec<f32>) -> Self {
        Self {
            id,
            new_name,
            new_polygon,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateRegionCommand {
    fn label(&self) -> String {
        format!("改区域「{}」", self.id)
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
        r.name = self.new_name.clone();
        r.polygon = self.new_polygon.clone();
        Ok(region_dirty_paths())
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateRegion 尚未 apply".into()))?;
        let r = world
            .regions
            .iter_mut()
            .find(|r| r.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Region {}", self.id)))?;
        *r = prev;
        Ok(())
    }
}

// ── 事件属性命令（统一 Inspector · 2026-10-22）───────────────────────────────
