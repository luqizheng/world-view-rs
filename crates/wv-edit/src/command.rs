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

    /// 取回目标网格的**中立数据**。当前原型：世界持单张 `terrain`。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain
            .as_ref()
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
        // ④ 写回中立数据
        world.terrain = Some(from_grid(grid));

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
        world.terrain = Some(from_grid(grid));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::Id;

    fn world_with_terrain(size: u32) -> World {
        let grid = TerrainGrid::new(size, size);
        let data = from_grid(grid);
        World {
            terrain: Some(data),
            ..Default::default()
        }
    }

    #[test]
    fn paint_command_apply_then_revert_restores_grid() {
        let mut world = world_with_terrain(32);
        let before_snapshot = world.terrain.clone().unwrap();

        let mut stroke = Stroke::new();
        stroke.push(wv_terrain::edit::Brush::paint((10.5, 10.5), 5.0, 3));
        let cmd = PaintTerrainCommand::new(Id::new_unchecked("world_main"), stroke, 1.0);

        let dirty = cmd.apply(&mut world).unwrap();
        assert!(!dirty.is_empty(), "应报告脏文件");
        let after = world.terrain.clone().unwrap();
        assert_ne!(before_snapshot, after, "apply 应改变地形");
        // 中心格类型被写为 3
        assert_eq!(after.cells[10 * 32 + 10].kind, 3);

        cmd.revert(&mut world).unwrap();
        assert_eq!(
            world.terrain.clone().unwrap(),
            before_snapshot,
            "revert 应完全还原"
        );
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
}
