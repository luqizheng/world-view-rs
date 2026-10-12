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
//! `Ctrl+Z` / `Ctrl+Shift+Z` 由 `crate::ui_shell::hotkeys` 收键、经 `CommandPalette`
//! 派发回本模块的 [`apply_history_command`] 执行——**这里不再自己听键盘**。
//! 撤销/重做与编辑命令都服从同一条门禁：播放中一律拒绝（基线 §1.8 / V37），
//! 因为播放中「当前世界」是时间游标的投影，改它等于改了一个并不存在的现在。
//!
//! 注意这个布局的另一个好处：快捷键只在「输入框未聚焦」时生效这件事，
//! 只在一个地方处理一次（`ui_shell::hotkeys`），不会出现「两个系统都听同一组键」
//! 导致一次按键应用两次的经典 bug。

use bevy::prelude::*;

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

/// 执行一次撤销 / 重做，返回给用户看的结果说明。
///
/// **唯一入口**：快捷键、命令面板、菜单都经此函数，避免「两处各写一遍」导致
/// 同一次 `Ctrl+Z` 被应用两次（曾经 `handle_undo_redo_keys` 与界面快捷键各自
/// 直接改历史，就是这个隐患）。
///
/// 播放中只读（基线 V37）：拒绝撤销并说明原因，而不是静默无反应。
pub fn apply_history_command(
    redo: bool,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    playback: &PlaybackState,
) -> Result<String, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再撤销".into());
    }
    let result = if redo {
        edit.history.redo(&mut projection.world)
    } else {
        edit.history.undo(&mut projection.world)
    };
    match result {
        Ok(true) => {
            projection.dirty = true;
            tex.mark_all_dirty();
            Ok(if redo {
                "已重做".into()
            } else {
                "已撤销".into()
            })
        }
        Ok(false) => Ok(if redo {
            "没有可重做的操作".into()
        } else {
            "没有可撤销的操作".into()
        }),
        Err(e) => Err(format!("撤销/重做失败: {e}")),
    }
}
