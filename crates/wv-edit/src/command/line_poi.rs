//! 线段 POI 命令族（ADR-0070）：创建（含地形下切）与属性修改。
//!
//! ## 为什么「创建 + 下切」是**一条**命令
//!
//! 作者画一条河 / 一条路时，「线段 POI 入表」与「沿线下切地形」是**同一个
//! 意图**的两半——拆成两条命令，撤销一次只退一半，世界就停在「有河名、
//! 没河床」的半成品态。命令的原子性该跟作者的心智单位走，不跟实现步骤走。
//!
//! 职责分界与 [`IntentTerrainCommand`] 同款（基线 §2.2）：
//! **`wv-terrain` 不感知 Undo/Redo，`wv-edit` 不感知像素**——下切本身由
//! `wv_terrain::line_path::carve_corridor` 完成，本族只负责快照、回放与
//! 脏路径报告。

use super::*;
use wv_core::distance::Place;
use wv_terrain::line_path::carve_corridor;

/// 一次走廊下切的**参数包**（ADR-0070 D3）。
///
/// `width_cells` 是**总宽格数**（评审裁定 I4）：走廊 = 中轴 ±
/// `(width_cells − 1) / 2` 格；`width_cells = 1` 即单格链（= 既有涂格河）。
/// 传「宽度米 ÷ 格宽」的商进来时按总宽换算，按半径解释会宽出约 2 倍。
#[derive(Debug, Clone)]
pub struct CarveSpec {
    /// 下切走廊的中轴折线（图内局部 wu 坐标）。
    pub pts: Vec<(f32, f32)>,
    /// 下切强度（0.0..=1.0，透传 `carve_corridor` 的 `strength`）。
    pub strength: f32,
    /// 走廊**总宽**格数（见类型文档——不是半径）。
    pub width_cells: u32,
    /// `true` = 河（写水面、清群系、复刻 legacy 下切深度）；`false` = 路
    /// （削到岸高附近、不写水面、跳过海面格）。
    pub mark_water: bool,
}

/// 创建线段 POI：一条命令同时「地点入表 + 沿线地形下切」（ADR-0070）。
///
/// ## 前置校验（不动世界）
///
/// - `place.line` 不足两点（`len < 4`）→ [`EditError::Precondition`]——
///   点状地点请走 `AddPlaceCommand`，线段 POI 的最小形状就是两点折线；
/// - 地点 ID 已存在 → [`EditError::Precondition`]（与 `AddPlaceCommand`
///   同款：不静默覆盖）；
/// - 带 `carve` 但目标图无地形栅格 → [`EditError::TerrainMismatch`]
///   （不替作者凭空造尺寸）。
///
/// ## 回滚
///
/// `apply` 缓存「被下切格的旧值」与「地点表」两份状态；`revert` 先逐格
/// 回放地形，再删地点——两半都退，与 apply 的改动面严格对称。
#[derive(Debug)]
pub struct CreateLinePoiCommand {
    /// 目标地图 ID（下切作用在**这张图**的栅格上）。
    pub map: wv_core::Id,
    /// 要创建的线段地点（含 `line` / `line_kind` / `line_width_m`）。
    pub place: Place,
    /// `Some` = 沿线下切；`None` = 纯矢量创建（不碰地形，也不要求有栅格）。
    pub carve: Option<CarveSpec>,
    /// 每格世界边长（wu/格）。
    pub cell_size: f32,
    /// `apply` 时缓存的受下切格旧值：`(gx, gy, before_cell)`。
    ///
    /// 空 Vec = 未 apply 过或未下切；`revert` 里地点删除是「apply 过」的
    /// 权威判据（`AddPlaceCommand` 同款语义）。
    before: std::sync::Mutex<Vec<(i32, i32, TerrainCell)>>,
}

impl CreateLinePoiCommand {
    /// 从 `(map, place, carve, cell_size)` 一步构造。
    pub fn new(map: wv_core::Id, place: Place, carve: Option<CarveSpec>, cell_size: f32) -> Self {
        Self {
            map,
            place,
            carve,
            cell_size,
            before: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl EditCommand for CreateLinePoiCommand {
    fn label(&self) -> String {
        format!("新增线段地点「{}」", self.place.name)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        // ① 前置校验（先于一切写操作，保证失败 = 世界原样）。
        let line_len = self.place.line.as_ref().map_or(0, |l| l.len());
        if line_len < 4 {
            return Err(EditError::Precondition(
                "线段 POI 的折线至少需要 2 个点".into(),
            ));
        }
        if world.distances.places.iter().any(|p| p.id == self.place.id) {
            return Err(EditError::Precondition(format!(
                "地点 `{}` 已存在",
                self.place.id
            )));
        }

        // ② 下切（可选）：快照 → carve → diff 缓存 → 写回（IntentTerrainCommand 同款）。
        let mut carved = false;
        if let Some(carve) = &self.carve {
            let data = world.terrain_of(&self.map).cloned().ok_or_else(|| {
                EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map))
            })?;
            let mut grid = to_grid(&data)?;
            let snapshot: Vec<TerrainCell> = grid.cells().to_vec();
            carve_corridor(
                &mut grid,
                &carve.pts,
                carve.strength,
                self.cell_size,
                carve.width_cells,
                carve.mark_water,
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
            carved = true;
        }

        // ③ 地点入表（AddPlace 语义）。
        world.distances.places.push(self.place.clone());

        let mut dirty = vec!["distances/places".into()];
        if carved {
            dirty.push(format!("maps/{}/terrain.bin", self.map));
        }
        Ok(dirty)
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        // ① 地形回放（快照 diff 逐格写回）。
        let before = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("命令锁中毒".into()))?
            .clone();
        if !before.is_empty() {
            let data = world.terrain_of(&self.map).cloned().ok_or_else(|| {
                EditError::TerrainMismatch(format!("地图 `{}` 无地形网格", self.map))
            })?;
            let mut grid = to_grid(&data)?;
            for (gx, gy, cell) in &before {
                grid.set(*gx, *gy, *cell);
            }
            world.set_terrain(self.map.clone(), from_grid(grid));
        }
        // ② 删地点（AddPlaceCommand::revert 同款：不在表里 = TargetMissing，
        //    顺带守住了「未 apply 先 revert」）。
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

/// 改线段 POI 的属性（名称 / 基准宽 / 呈现级别）。
///
/// `None` = **不改**（`UpdatePlaceCommand` 同语义）——不是「写成 None」。
/// `line` 本身的改动（重画折线）继续走既有 `UpdatePlaceLineCommand`；
/// `line_kind` 的改动走 `SetPlaceKindCommand` 式的独立命令待后续任务，
/// 本命令只管「这条线多宽、多显眼、叫什么」。
///
/// ## 回滚
///
/// `apply` 缓存整 `Place`（与 `UpdatePlaceCommand` 同款：快照整实体，
/// apply 改了哪些字段 revert 就退哪些，不漏）。
#[derive(Debug)]
pub struct UpdateLinePoiCommand {
    /// 目标地点 ID。
    pub id: wv_core::Id,
    /// 新名称（`None` = 不改）。
    pub new_name: Option<String>,
    /// 新基准宽米（`None` = 不改；仅河流语义消费，道路不落盘此字段）。
    pub new_width_m: Option<f32>,
    /// 新呈现级别（`None` = 不改）。
    pub new_rank: Option<u8>,
    before: std::sync::Mutex<Option<Place>>,
}

impl UpdateLinePoiCommand {
    pub fn new(
        id: wv_core::Id,
        new_name: Option<String>,
        new_width_m: Option<f32>,
        new_rank: Option<u8>,
    ) -> Self {
        Self {
            id,
            new_name,
            new_width_m,
            new_rank,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for UpdateLinePoiCommand {
    fn label(&self) -> String {
        format!("改线段地点「{}」", self.id)
    }

    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
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
        if let Some(name) = &self.new_name {
            p.name = name.clone();
        }
        if let Some(w) = self.new_width_m {
            p.line_width_m = Some(w);
        }
        if let Some(r) = self.new_rank {
            p.rank = Some(r);
        }
        Ok(vec!["distances/places".into()])
    }

    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("UpdateLinePoi 尚未 apply".into()))?;
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
