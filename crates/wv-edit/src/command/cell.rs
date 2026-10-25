//! cell 命令族（自 command.rs 拆分，保持行为不变）。

use super::*;

// ---------------------------------------------------------------------------
// 单格编辑命令（ADR-0040 §决策 2）
// ---------------------------------------------------------------------------

/// 单格编辑命令：[ADR-0040] 最 detail 的精细编辑器,每格 1 command,P6 友好。
///
/// 与 `PaintTerrainCommand`（批量沿 stroke 改格）的差异：
/// - `EditCellCommand` 改**精确一格**的 surface / height / biome 三通道；
/// - 撤销栈每格 1 条记录（沿 snapshot-diff-revert 四步模式）；
/// - 不读 `wv_terrain`（layer）；本命令只动 `wv_core::terrain::TerrainData` 的
///   一格 —— `wv-io` 落盘时整栅格统一序列化（terrain.bin 不变字节布局）。
///
/// **biome 通道允许独立改**（Q3 默认）：改 BIOME_LAKE / RIVER / OCEAN 时,
/// 湿判定 [`wv_terrain::edit::is_water_cell`]（surface==水 ∨ height<0）
/// 仍然只读 surface 与 height —— biome 改了但视觉判定不立刻变,作者需同时
/// 改 surface 或 height 才能看到湿渲染。Inspector UI 上挂一行提示（PR-2 UI
/// 落地后由 [ADR-0040] §决策 6 实施）。
///
/// [ADR-0040]: ../docs/adr/ADR-0040-cell-inspector-tool.md
#[derive(Debug, Clone)]
pub struct EditCellCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 格点整数坐标（`0 ≤ gx < width`，`0 ≤ gy < height`）。
    pub gx: i32,
    pub gy: i32,
    /// 起始值的快照（构造时由调用方读取后传入,作为 `apply` 前置校验与
    /// `revert` 的回滚值）。
    pub before_cell: TerrainCellData,
    /// 编辑目标值。`apply` 写整格的 `cells[idx]` 等于此。
    pub after: TerrainCellData,
}

impl EditCellCommand {
    /// 构造时由调用方给出 (map, gx, gy) 当前 cell,以及期望的 `after`。
    ///
    /// 调用方应**预先从 `world.terrain_of(&map)` 读到 before_cell** 再构造,
    /// 而不是构造后再读——command 一旦出手就要把"作者当下看到的是 X"与
    /// "实际想改成 Y"打包成不可变快照。
    pub fn new(
        map: wv_core::Id,
        gx: i32,
        gy: i32,
        before_cell: TerrainCellData,
        after: TerrainCellData,
    ) -> Self {
        Self {
            map,
            gx,
            gy,
            before_cell,
            after,
        }
    }
}

impl EditCommand for EditCellCommand {
    fn label(&self) -> String {
        format!("改 cell ({}, {})", self.gx, self.gy)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut data = world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))?
            .clone();
        let idx = self.cell_index(&data)?;
        if data.cells[idx] != self.before_cell {
            // 起始值已被其他动作改了——中止,避免基于过期快照的"幽灵编辑"。
            return Err(EditError::Precondition(format!(
                "起始值不匹配 (期望 {:?}, 实际 {:?})",
                self.before_cell, data.cells[idx]
            )));
        }
        data.cells[idx] = self.after;
        world.set_terrain(self.map.clone(), data);
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let mut data = world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))?
            .clone();
        let idx = self.cell_index(&data)?;
        data.cells[idx] = self.before_cell;
        world.set_terrain(self.map.clone(), data);
        Ok(())
    }
}

impl EditCellCommand {
    /// `(gx, gy) → cells` 行优先索引 + 边界检查（防御性,作者给的坐标
    /// 应已合法;真出错就返错）。
    fn cell_index(&self, data: &wv_core::terrain::TerrainData) -> EditResult<usize> {
        let w = data.width as i32;
        let h = data.height as i32;
        if self.gx < 0 || self.gy < 0 || self.gx >= w || self.gy >= h {
            return Err(EditError::Precondition(format!(
                "cell 坐标 ({}, {}) 超出地形范围 {}x{}",
                self.gx, self.gy, w, h
            )));
        }
        Ok((self.gy as usize) * (w as usize) + (self.gx as usize))
    }
}

// ---------------------------------------------------------------------------
// 批量单格编辑命令（2026-10-20 多边形选区批量修改）
// ---------------------------------------------------------------------------

/// 批量单格变更载荷：一格的 before / after。
#[derive(Debug, Clone)]
pub struct CellChange {
    pub gx: i32,
    pub gy: i32,
    pub before: wv_core::terrain::TerrainCellData,
    pub after: wv_core::terrain::TerrainCellData,
}

/// 批量单格编辑命令：多边形选区贴合方格后，**整体**修改 N 格。
///
/// 与 [`EditCellCommand`]（单格）的关系：同一提交语义，N 格合 **1 个撤销
/// 单位**——用户「圈一片 → 一次改 → 一次撤」的心智。changes 在 UI 侧构建时
/// 即从当前 world 读取 before（无并发编辑场景），apply 直接逐格写 after；
/// revert 逐格写回 before。
#[derive(Debug, Clone)]
pub struct BatchEditCellsCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 逐格变更（调用方保证各格不重复）。
    pub changes: Vec<CellChange>,
}

impl BatchEditCellsCommand {
    pub fn new(map: wv_core::Id, changes: Vec<CellChange>) -> Self {
        Self { map, changes }
    }
}

impl EditCommand for BatchEditCellsCommand {
    fn label(&self) -> String {
        format!("批量改 {} 格", self.changes.len())
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let mut data = world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))?
            .clone();
        let w = data.width as i32;
        let h = data.height as i32;
        for c in &self.changes {
            if c.gx < 0 || c.gy < 0 || c.gx >= w || c.gy >= h {
                return Err(EditError::Precondition(format!(
                    "cell 坐标 ({}, {}) 超出地形范围 {}x{}",
                    c.gx, c.gy, w, h
                )));
            }
            let idx = (c.gy as usize) * (w as usize) + (c.gx as usize);
            data.cells[idx] = c.after;
        }
        world.set_terrain(self.map.clone(), data);
        Ok(vec![format!("maps/{}/terrain.bin", self.map)])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let mut data = world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))?
            .clone();
        let w = data.width as i32;
        for c in &self.changes {
            let idx = (c.gy as usize) * (w as usize) + (c.gx as usize);
            if idx < data.cells.len() {
                data.cells[idx] = c.before;
            }
        }
        world.set_terrain(self.map.clone(), data);
        Ok(())
    }
}
