//! 编辑命令。
//!
//! 每个修改动作都实现 `EditCommand`。命令必须满足：
//! - 可序列化（用于事务日志与快照）；
//! - 可逆（或声明为不可逆，走「追加区间」路径）；
//! - 不直接触碰文件系统（那是 `wv-io` 的职责）。

use wv_core::map::GeometryData;
use wv_core::map::GeometrySpan;
use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_core::World;
#[cfg(test)]
use wv_core::{Faction, FactionColorMode, FactionLayer, Id, Measurement, Membership};
use wv_terrain::continent::{generate_continents, ContinentParams};
use wv_terrain::edit::{apply_stroke, Stroke};
use wv_terrain::intent::{apply_intent, Intent};
use wv_terrain::{TerrainCell, TerrainGrid};

mod attribute;
mod biome;
mod cell;
mod faction;
mod line_poi;
mod map;
mod measurement;
mod overlay;
mod partition;
mod region;
mod scene;
mod settlement;
mod terrain;
pub use attribute::*;
pub use biome::*;
pub use cell::*;
pub use faction::*;
pub use line_poi::*;
pub use map::*;
pub use measurement::*;
pub use overlay::*;
pub use partition::*;
pub use region::*;
pub use scene::*;
pub use settlement::*;
pub use terrain::*;

/// 标记命令 apply 前的快照（undo 用）；`PlaceBiomeMarker` / `RegenerateBiomeMarker`
/// 复用（原与 faction 同族文件，拆分后上移为跨族共享）。
#[derive(Debug, Default)]
struct BeforeMarker {
    /// 被改动的格（生成分支才非空）。
    cells: Vec<(i32, i32, TerrainCell)>,
}

/// 事实地点创建命令的脏路径：`Place` 本体与地界几何分属两个落盘目录。
///
/// `distances/places` 是 §4.5「每实体一文件」的地点表（全量重写 + 删除传播），
/// `geometry/place` 是地界 span 目录（`wv-io::save_world` 按目录写出）。
fn place_geometry_dirty_paths() -> Vec<String> {
    vec!["distances/places".into(), "geometry/place".into()]
}

/// 从 World 数据推导目标图的格尺寸（wu/格）——标记命令的唯一格口径。
///
/// 逐条镜像渲染层 `grid_sync` 的推导（图幅 extent / 栅格宽），但**纯从
/// World 数据算**：wv-edit 结构上摸不到会话态 `GridConfig`，「捕获时与提交时
/// 格口径漂移」的 C1 类 bug 在这里无法发生。回退链与 `grid_sync` 一致：
/// 本图 extent → 根图 extent → 4096；无地形栅格返回 `None`（调用方拒绝）。
fn marker_cell_size(world: &World, map: &wv_core::Id) -> Option<f32> {
    let data = world.terrain_of(map)?;
    let extent = world
        .maps
        .iter()
        .find(|m| m.id == *map)
        .and_then(|m| m.extent.as_ref())
        .or_else(|| {
            let root = world.meta.as_ref().map(|m| m.root_map.clone())?;
            world
                .maps
                .iter()
                .find(|n| n.id == root)
                .and_then(|n| n.extent.as_ref())
        })
        .map(|e| e.width)
        .unwrap_or(4096.0);
    Some((extent / data.width.max(1) as f32).max(0.01))
}

/// `wv-core::terrain::TerrainData` → `wv-terrain::TerrainGrid` 的桥。
///
/// `wv-core` 不认识 `TerrainGrid`（不依赖 wv-terrain），因此两层的接缝在这里。
/// `wv-io` 有对称的一对函数用于落盘，二者共享同一布局约定（行优先、三通道）。
pub(crate) fn to_grid(data: &TerrainData) -> EditResult<TerrainGrid> {
    let cells: Vec<TerrainCell> = data
        .cells
        .iter()
        .map(|c| TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
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
            surface: c.surface,
            biome: c.biome,
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

    #[error("地形网格已存在: {0}")]
    SceneGridExists(String),
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

#[cfg(test)]
mod command_tests;
