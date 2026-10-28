//! 编辑桥：笔刷笔触 → `EditCommand` → `wv-edit::History`（S5 / S9）。
//!
//! ## 职责
//! - 收集 [`wv_render::camera::ActiveStroke`] 的点列，在松手（左键释放）时把
//!   `wv_terrain::edit::Stroke` 包成 `wv_edit::PaintTerrainCommand` 提交给 `History`。
//! - 处理 Undo/Redo：`Ctrl+Z` / `Ctrl+Shift+Z` 由 `ui_shell::hotkeys` 收键，
//!   经 `CommandPalette` 派发到本模块的 `apply_history_command`。
//! - 播放中一律拒绝编辑（基线 §1.8 / V37）：时间游标「当前世界」是投影，不能原地改。
//!
//! ## 边界（不做什么）
//! - **不自己听键盘**——所有键一律经 `ui_shell::hotkeys` → `CommandPalette::pending`
//!   → 本模块执行，避免两个系统重复处理同一组键。
//! - **不直接修改 `WorldProjection.world`**——只能走 `wv-edit` 的 `History`（§3.3）。
//! - **不在播放中接受编辑**——门禁在 `apply_history_command` / `apply_brush`。
//!
//! ## 子模块（2026-10-24 由单文件 1797 行按命令族拆出）
//! - [`stroke`]：笔触提交（松手 → 命令）。
//! - [`terrain`]：地形落盘 / 重置 / 重生 / 清除测量。
//! - [`region`]：区域意图、大陆生成、批量选区栅格化。
//! - [`history`]：撤销 / 重做与派生诊断刷新（含 [`EditHistory`] 资源）。
//! - [`cell`]：单格点选与 hover 追踪。
//! - `tests`：原 `mod tests` 整段平移。
//!
//! ## 主要导出
//! - `apply_brush` / `apply_history_command` / `push_history` — 编辑命令入口。
//! - 资源 `History` / `EditMode` 由 `state.rs` 暴露；本模块只读 / 提交。

use bevy::input::mouse::MouseButton;
use bevy::input::ButtonInput;
use bevy::prelude::*;

use crate::state::{
    BrushState, BrushTool, DrillState, GridConfig, PlaybackState, ViewState, WorldProjection,
};
use crate::ui::cell_pick;
use wv_edit::command::{
    ClearMeasurementsCommand, EditCommand, GenerateContinentCommand, IntentTerrainCommand,
    PaintTerrainCommand, RegenWorldTerrainCommand, ResetTerrainToBaseCommand,
};
use wv_edit::history::Transaction;
use wv_edit::History;
use wv_render::camera::ActiveStroke;
use wv_render::state::EditCellState;
use wv_render::terrain_view::{TerrainTexture, TileAtlas};
use wv_terrain::continent::ContinentParams;
use wv_terrain::edit::{Brush, Stroke as TerrainStroke};
use wv_terrain::intent::Intent;

mod cell;
mod history;
mod landmark;
mod pin_drag;
mod region;
mod stroke;
mod terrain;
#[cfg(test)]
mod tests;

pub use cell::{
    clear_cell_selection_on_escape, commit_point_select_on_click, track_edit_cell_hover,
};
pub use history::{
    apply_history_command, apply_history_command_detailed, refresh_project_diagnostics,
    HistoryOutcome,
};
pub use landmark::{commit_landmark_click, landmark_anchor, landmark_candidates};
pub use pin_drag::drag_place_pin_on_hold;
pub use region::{
    commit_continent_generation, commit_region_intent, sync_region_cells, LassoLiveLink,
};
pub use stroke::commit_stroke_on_release;
pub use terrain::{
    apply_clear_measurements, apply_regen_world, apply_reset_terrain_to_base, commit_regen_world,
    flush_terrain_texture,
};

/// 编辑历史（`wv-edit::History` 的应用层持有者）。
///
/// `dirty_set` 累计自上次 `take_dirty` / `clear_dirty` 以来**所有**已提交命令
/// 返回的相对目录路径（如 `maps` / `distances/edges` / `geometry/faction`）。
/// 阶段七 v0.2：Save 命令用这个集合把 `save_world(..., &dirty)` 喂进去，
/// 避免每次保存都全目录重写。
///
/// 持久化模型：
/// - 编辑（commit/redo）→ `record_dirty` 累加
/// - 加载新工程（`apply_loaded_world`）→ `clear_dirty` 重置
/// - 保存成功 → `take_dirty` 排空
/// - 撤销（undo）→ 当前版本暂**不**回滚 dirty 集合（接受一定过度写盘换取
///   实现简单性；保存仍是幂等的 round-trip）
#[derive(Resource, Default)]
pub struct EditHistory {
    pub history: History,
    pub dirty_set: std::collections::BTreeSet<String>,
}

impl EditHistory {
    /// 把 `paths` 全部并入 dirty 集合。
    pub fn record_dirty<I, S>(&mut self, paths: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for p in paths {
            self.dirty_set.insert(p.into());
        }
    }

    /// 取出 dirty 集合并清空（原子）。
    pub fn take_dirty(&mut self) -> Vec<String> {
        let v: Vec<String> = self.dirty_set.iter().cloned().collect();
        self.dirty_set.clear();
        v
    }

    /// 不取，只清空（用于「加载新工程」之类的整体重置）。
    pub fn clear_dirty(&mut self) {
        self.dirty_set.clear();
    }

    /// 直应用通道（地图树垃圾桶 spec §10 追记 2，2026-10-27）：
    /// `apply` + 记脏路径，**不 commit 进 Undo 栈**。
    ///
    /// 「彻底删除」是用户确认过的永久动作——不该被 Ctrl+Z 复活；但脏路径
    /// 必须照记，否则下次保存不会物理删除目录。返回值即已记录的脏路径。
    pub fn apply_detached(
        &mut self,
        world: &mut wv_core::World,
        cmd: Box<dyn wv_edit::EditCommand>,
    ) -> wv_edit::EditResult<Vec<String>> {
        let dirty = cmd.apply(world)?;
        self.record_dirty(dirty.clone());
        Ok(dirty)
    }
}
