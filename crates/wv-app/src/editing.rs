//! 编辑桥：笔刷笔触 → `PaintTerrainCommand` → `wv-edit::History`（S5 / S9）。
//!
//! ## 铁律（`AGENTS.md` §3.3）
//!
//! **System 只能生成 `EditCommand`，不能自己改世界。** 本模块是唯一的桥：
//! 松手（左键释放）时，把 [`crate::camera::ActiveStroke`] 里的点列转成
//! `wv_terrain::edit::Stroke`，包成 `PaintTerrainCommand`，交给 `History` 提交。
//!
//! ## Undo/Redo（S9）
//!
//! `Ctrl+Z` / `Ctrl+Shift+Z` 触发 [`History::undo`] / [`History::redo`]。
//! 播放中禁止编辑与撤销（基线 §1.8 / V37）——播放中 `undo/redo` 与编辑命令全部禁用。

use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::camera::ActiveStroke;
use crate::state::{BrushState, GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::terrain_view::{image_from_terrain, TerrainTexture};
use wv_core::Id;
use wv_edit::command::PaintTerrainCommand;
use wv_edit::history::Transaction;
use wv_edit::History;
use wv_terrain::edit::{Brush, Stroke as TerrainStroke};

/// 编辑历史（`wv-edit::History` 的应用层持有者）。
#[derive(Resource, Default)]
pub struct EditHistory {
    pub history: History,
}

/// 松手时把笔触转成命令并提交。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道。
#[allow(clippy::too_many_arguments)]
pub fn commit_stroke_on_release(
    buttons: Res<ButtonInput<MouseButton>>,
    mut active: ResMut<ActiveStroke>,
    grid: Res<GridConfig>,
    brush: Res<BrushState>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    mut projection: ResMut<WorldProjection>,
    mut tex: ResMut<TerrainTexture>,
    mut edit: ResMut<EditHistory>,
) {
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    if active.points.is_empty() {
        return;
    }
    // 播放中只读（基线 §1.8 / V37）。
    if playback.playing {
        active.points.clear();
        return;
    }
    // 极低缩放禁画地形（基线 §1.16 L2：0.05×/0.15× 级）。
    if view.scale < 0.15 {
        active.points.clear();
        return;
    }

    // 世界坐标点列 → wv-terrain 的笔触（每点一个 Brush）。
    let mut stroke = TerrainStroke::new();
    for p in active.points.drain(..) {
        // 世界坐标 → 图内局部坐标（反转 GridConfig 的 origin 偏移）。
        let local = p - grid.origin;
        stroke.push(Brush {
            center: (local.x, local.y),
            radius: brush.radius,
            softness: brush.softness,
            kind: Some(brush.kind),
            height_delta: 0.0,
        });
    }

    let cmd = PaintTerrainCommand::new(
        Id::new_unchecked(view.current_map.clone()),
        stroke,
        grid.cell_size,
    );

    // 提交到 History（一次笔触 = 一个事务 = 一次可撤销单位）。
    let mut tx = Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(_dirty) => {
            projection.dirty = true;
            // 增量重传：标记受影响区域为脏（此处按笔触包围盒近似）。
            // 由于统一走「重建全图」在原型阶段更稳，这里改为整图脏标记，
            // 真正的分块 patch 留给 perf 迭代（S5 基线要求：不每帧重建）。
            tex.mark_all_dirty();
        }
        Err(e) => {
            bevy::log::warn!("编辑命令失败: {e}");
        }
    }
}

/// 把脏标记变为实际纹理更新（增量重传的出口）。
///
/// **只在有脏标记时重建**——不是每帧重建整图（基线 §2.11）。
pub fn flush_terrain_texture(
    mut tex: ResMut<TerrainTexture>,
    projection: Res<WorldProjection>,
    view: Res<ViewState>,
    mut images: ResMut<Assets<Image>>,
) {
    if tex.dirty.is_none() {
        return;
    }
    let Some(data) = projection.world.terrain.as_ref() else {
        tex.dirty = None;
        return;
    };
    let image = image_from_terrain(data, view.mode);
    if let Some(img) = images.get_mut(&tex.handle) {
        *img = image;
    }
    tex.dirty = None;
}

/// Ctrl+Z / Ctrl+Shift+Z（S9）。
pub fn handle_undo_redo_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut contexts: EguiContexts,
    mut edit: ResMut<EditHistory>,
    mut projection: ResMut<WorldProjection>,
    playback: Res<PlaybackState>,
    mut tex: ResMut<TerrainTexture>,
) {
    if contexts.ctx_mut().wants_keyboard_input() {
        return;
    }
    // 播放中只读：禁用撤销/重做（基线 V37）。
    if playback.playing {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl || !keys.just_pressed(KeyCode::KeyZ) {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let result = if shift {
        edit.history.redo(&mut projection.world)
    } else {
        edit.history.undo(&mut projection.world)
    };
    match result {
        Ok(did) => {
            if did {
                projection.dirty = true;
                tex.mark_all_dirty();
            }
        }
        Err(e) => bevy::log::warn!("撤销/重做失败: {e}"),
    }
}
