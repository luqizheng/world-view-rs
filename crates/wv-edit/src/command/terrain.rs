//! 地形生成命令族：刷地形 / 意图笔刷 / 大陆生成（自 command.rs 拆分）。

use super::*;

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
// 意图笔刷命令（2026-10-15，意图笔刷 plan v0.1）
// ---------------------------------------------------------------------------

/// 意图笔刷命令：作者给「意图 + 大致区域」，程序生成自然地形。
///
/// ## 与 [`PaintTerrainCommand`] 的分工
///
/// 两者都落 `maps/<map>/terrain.bin`、都用「快照 → 改格 → diff 缓存 → 写回 /
/// 按表回滚」的同一模式；差别只在第 ② 步调的是 `apply_stroke`（直接笔刷）
/// 还是 [`apply_intent`]（意图生成）。
///
/// ## seed 的确定性契约
///
/// `seed` 在 [`Self::new`] 里从 `(map, intent)` 派生并**存进命令**——
/// undo 后 redo 重放的是同一个 seed，wv-terrain 保证同 seed 同 intent 逐格
/// 一致，因此 redo 结果与首次 apply 完全相同。
#[derive(Debug)]
pub struct IntentTerrainCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 地形意图（山脉 / 高原 / 丘陵 / 火山 / 湖 / 河）。
    pub intent: Intent,
    /// 每格世界边长。
    pub cell_size: f32,
    /// 派生种子（构造时算好，随命令存活）。
    pub seed: u64,
    /// `apply` 时缓存的受影响格旧值：`(gx, gy, before_cell)`。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl IntentTerrainCommand {
    /// 构造时从图 ID 与意图内容派生 seed（见 [`Intent::derive_seed`]）。
    pub fn new(map: wv_core::Id, intent: Intent, cell_size: f32) -> Self {
        let seed = intent.derive_seed(map.as_str());
        Self {
            map,
            intent,
            cell_size,
            seed,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 与 [`PaintTerrainCommand::terrain`] 同款：按图查栅格，缺失报
    /// `TerrainMismatch`（不替作者凭空造尺寸）。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))
    }
}

impl EditCommand for IntentTerrainCommand {
    fn label(&self) -> String {
        let what = match &self.intent {
            Intent::MountainRange { .. } => "山脉",
            Intent::Plateau { .. } => "高原",
            Intent::Hills { .. } => "丘陵",
            Intent::Volcano { .. } => "火山",
            Intent::Lake { .. } => "湖",
            Intent::River { .. } => "河",
            Intent::MountainMassif { .. } => "高山群",
            Intent::HillsRegion { .. } => "丘陵",
            Intent::PlainsRegion { .. } => "平原",
            Intent::LakeRegion { .. } => "湖泊",
        };
        format!("生成{what}")
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        apply_intent(&mut grid, &self.intent, self.seed, self.cell_size);
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
// 大陆生成（大陆生成 UI plan v0.1 Task 1；README §12-40）
// ---------------------------------------------------------------------------

/// 大陆生成宏命令：一次 apply 完成「意图包络 → 大陆配方 → 双通道栅格」。
///
/// ## 与 [`IntentTerrainCommand`] 的两点差异
///
/// 1. **seed 显式传入**，不在构造时 derive——面板上有骰子按钮，同一个圈
///    「重骰」语义要求作者能换 seed 再生成；意图笔刷的确定性来自
///    `Intent::derive_seed`，大陆生成则把骰子交给作者。
/// 2. 一次命令可带**多条包络**（多块大陆同批生成），撤销是 Q16 的
///    原子语义：一撤全回。
///
/// before 缓存与 [`IntentTerrainCommand`] 同款：只记真正变过的格，
/// 撤销逐格回放；`revert` 后 `before` 保留，redo 重放同一 seed 结果逐位一致。
#[derive(Debug)]
pub struct GenerateContinentCommand {
    /// 目标地图 ID。
    pub map: wv_core::Id,
    /// 意图包络（每条 ≥ 3 点；世界单位）。图层面不落盘——只活在命令里。
    pub envelopes: Vec<Vec<(f32, f32)>>,
    /// 大陆配方八参数（全局单一面板）。
    pub params: ContinentParams,
    /// 骰子种子（面板「重骰」直接换这个值）。
    pub seed: u64,
    /// 每格世界边长。
    pub cell_size: f32,
    /// `apply` 时缓存的受影响格旧值：`(gx, gy, before_cell)`。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl GenerateContinentCommand {
    /// 按作者面板参数构造；`seed` 显式给出（见类型文档）。
    pub fn new(
        map: wv_core::Id,
        envelopes: Vec<Vec<(f32, f32)>>,
        params: ContinentParams,
        seed: u64,
        cell_size: f32,
    ) -> Self {
        Self {
            map,
            envelopes,
            params,
            seed,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 与 [`IntentTerrainCommand::terrain`] 同款：缺失栅格报 `TerrainMismatch`，
    /// 不替作者凭空造尺寸（V1：只提示，不阻止）。
    fn terrain<'a>(&self, world: &'a World) -> EditResult<&'a TerrainData> {
        world
            .terrain_of(&self.map)
            .ok_or_else(|| EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map)))
    }
}

impl EditCommand for GenerateContinentCommand {
    fn label(&self) -> String {
        "生成大陆".to_string()
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let data = self.terrain(world)?.clone();
        let mut grid = to_grid(&data)?;
        let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
        generate_continents(
            &mut grid,
            &self.envelopes,
            &self.params,
            self.seed,
            self.cell_size,
        );
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
