//! 单格连续绘制（[ADR-0052] D3 · §9.1）。
//!
//! ## 生命周期
//!
//! ```
//! press ──▶ 起笔（记快照）──▶ 拖动（累积格 + 待落色块）──▶ release ──▶ 提交
//!                │                                                    │
//!                └─ Esc / 右键 / 切工具 ──▶ 丢弃整笔 ────────────────┘
//! ```
//!
//! ## 与既有笔刷（`commit_stroke_on_release`）的分工
//!
//! 笔刷走 `wv_terrain::Brush`（圆形软边、连续抬落），本模块走**硬边单格**
//! （D5）——两者刻意分开：地形编辑要「这格是森林」的确切控制，软边会引入
//! 「边界那格算不算」的歧义。两者共用 `BatchEditCellsCommand` 的提交接口
//! （N 格 = 1 个撤销单位）。
//!
//! ## 铁律
//!
//! - **落笔值**（[`CellPaintValue`]）是**会话态**——拖绘期间只累积格，
//!   **不写世界**；松手才一次性提交（[ADR-0021] §3.3「System 不得直接
//!   改世界」：一切写入经 `EditCommand → History`）。
//! - **起笔清 region_cells**（D3-A）：从「选择」意图切到「涂画」意图的
//!   时序信号；两者不可同框（§9.5 四态视觉分工）。
//! - **Esc 丢弃整笔**，>50 格需确认（D3-C）。
//! - **右键 = Esc 等价**（D3-B）：丢弃整笔，不做半笔提交。

use bevy::prelude::*;
use bevy_egui::egui;

use wv_render::state::{EditCellState, GridConfig};
use wv_world_cell_map::cell_paint::{bresenham_cells, CellPaintSession};

use crate::state::{DrillState, PlaybackState, ViewState, WorldProjection};
use crate::ui::cell_pick;

/// **笔触进行中的格数阈值**（D3-C）：超过此格数，Esc 丢弃需二次确认。
///
/// 50 格是「小笔触重画快」与「误丢成本高」的折中：画 60 格要 30 秒，
/// 误丢一次白干半分钟；画 10 格重画两秒，不值得打断。
pub const STROKE_DISCARD_CONFIRM_THRESHOLD: usize = 50;

/// **丢弃确认弹窗态**（D3-C）。
#[derive(Resource, Debug, Default)]
pub struct StrokeDiscardPrompt {
    /// 待确认的笔触格数（`None` = 无弹窗）。
    pub pending: Option<usize>,
    /// 上一次采样到的格（补间的起点）——`wv-app` 侧私有。
    pub last_cell: Option<(i32, i32)>,
}

/// **连续绘制**（D3 主系统）。
///
/// 每帧跑，做三件事：
///
/// 1. **起笔**：`just_pressed` 且 `tool_active` → 清 `region_cells`（D3-A）
///    + 记起笔快照；
/// 2. **拖动**：`pressed` → `pick_at` 采样 + Bresenham 补间 → 累积到
///    `stroke.cells`（**不写世界**）；
/// 3. **松手**：`just_released` → stale 复核（D3-E）→ 通过则提交
///    `BatchEditCellsCommand`（1 个撤销单位）。
///
/// **Esc / 右键** → 丢弃整笔（D3-B / D3-C）。
#[allow(clippy::too_many_arguments)]
pub fn paint_edit_cell_on_drag(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<egui::Key>>,
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    ui: Res<wv_render::camera::PointerOverUi>,
    mut edit_cell: ResMut<EditCellState>,
    mut session: ResMut<CellPaintSession>,
    mut prompt: ResMut<StrokeDiscardPrompt>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
    screen: Res<crate::app_screen::AppScreen>,
) {
    // 加载中 / 播放中 / 无工具 → 不参与。
    if screen.shows_loading_overlay() || playback.playing || !edit_cell.tool_active {
        if session.stroke.active {
            session.stroke.discard();
            prompt.last_cell = None;
        }
        return;
    }

    // ── 丢弃确认弹窗（D3-C：>50 格需确认）──
    if let Some(pending) = prompt.pending {
        let confirmed = pending <= STROKE_DISCARD_CONFIRM_THRESHOLD
            || confirm_discard(ui, pending);
        if confirmed {
            session.stroke.discard();
            prompt.last_cell = None;
        }
        prompt.pending = None;
        return;
    }

    // ── Esc 丢弃（D3-C）──
    if keys.any_pressed(egui::Key::Escape) && session.stroke.active {
        let n = session.stroke.len();
        if n > STROKE_DISCARD_CONFIRM_THRESHOLD {
            prompt.pending = Some(n);
        } else {
            session.stroke.discard();
            prompt.last_cell = None;
        }
        return;
    }

    // ── 右键 = Esc 等价（D3-B：丢弃整笔，不半笔提交）──
    if buttons.just_pressed(MouseButton::Right) && session.stroke.active {
        session.stroke.discard();
        prompt.last_cell = None;
        return;
    }

    // ── 起笔（D3-A：清 region_cells）──
    if buttons.just_pressed(MouseButton::Left) {
        if ui.wants_pointer {
            return;
        }
        session.stroke.begin();
        prompt.last_cell = None;
        // **D3-A：起笔清空 region_cells**——从「选择」意图切到「涂画」意图的
        // 时序信号；选区与笔触不可同框（§9.5 四态视觉分工）。
        edit_cell.region_cells.clear();
        edit_cell.region_fingerprint = 0;
        if let Some(map) = drill.current() {
            session.stroke.map_id = Some(map.clone());
        }
    }

    // ── 拖动采样 + 补间 ──
    if buttons.pressed(MouseButton::Left) && session.stroke.active {
        if ui.wants_pointer {
            return;
        }
        let Some(cursor) = view.cursor_world else {
            // 光标出画布 → **不延伸**（D3-D：拉出画布不丢笔），笔触保持存活。
            return;
        };
        let Some(map) = drill.current() else {
            return;
        };
        let Some(terrain) = projection.world.terrain_of(map) else {
            return;
        };
        let Some(pick) = cell_pick::pick_at((cursor.x, cursor.y), &grid, terrain) else {
            // 落在图外 → 不延伸，笔触存活（D3-D）。
            return;
        };
        let cur = (pick.gx, pick.gy);
        let w = terrain.width as i32;
        let h = terrain.height as i32;
        let sample = |gx: i32, gy: i32| -> Option<wv_core::terrain::TerrainCellData> {
            if gx < 0 || gy < 0 || gx >= w || gy >= h {
                return None;
            }
            terrain
                .cells
                .get((gy as usize) * (w as usize) + (gx as usize))
                .copied()
        };
        // 补间：上一采样点 → 本次采样点之间补齐中间的格（D3：不漏格）。
        let from = prompt.last_cell.unwrap_or(cur);
        for (gx, gy) in bresenham_cells(from, cur) {
            if let Some(before) = sample(gx, gy) {
                session.stroke.push_cell(gx, gy, before);
            }
        }
        prompt.last_cell = Some(cur);
        return;
    }

    // ── 松手提交（D3 / D3-E）──
    if buttons.just_released(MouseButton::Left) && session.stroke.active {
        let Some(map) = session.stroke.map_id.clone() else {
            session.stroke.discard();
            prompt.last_cell = None;
            return;
        };
        let Some(terrain) = projection.world.terrain_of(&map) else {
            session.stroke.discard();
            prompt.last_cell = None;
            return;
        };
        let w = terrain.width as i32;
        let h = terrain.height as i32;
        let current = |gx: i32, gy: i32| -> Option<wv_core::terrain::TerrainCellData> {
            if gx < 0 || gy < 0 || gx >= w || gy >= h {
                return None;
            }
            terrain
                .cells
                .get((gy as usize) * (w as usize) + (gx as usize))
                .copied()
        };
        // **D3-E：stale 复核**——任一格与快照不符 → 整笔拒收。
        if let Some(bad) = session.stroke.stale_cells(&current) {
            notice.error(
                &format!(
                    "整笔已拒收：{} 格快照不一致（{}）",
                    bad.len(),
                    bad.first().map(|(x, y)| format!("({x},{y})")).unwrap_or_default()
                ),
                (time.elapsed_secs() * 60.0) as u64,
            );
            session.stroke.discard();
            prompt.last_cell = None;
            return;
        }
        // 构建变更集：只收「有变化」的格（无变化不进，避免空命令）。
        let mut changes = Vec::new();
        for (i, (gx, gy)) in session.stroke.cells.iter().enumerate() {
            let Some(before) = session.stroke.snapshots.get(i).copied() else {
                continue;
            };
            let after = session.paint.apply(before);
            if after != before {
                changes.push(wv_edit::command::CellChange {
                    gx: *gx,
                    gy: *gy,
                    before,
                    after,
                });
            }
        }
        if changes.is_empty() {
            session.stroke.discard();
            prompt.last_cell = None;
            return;
        }
        // 1 条 `BatchEditCellsCommand` = 1 个撤销单位（D3）。
        let cmd = wv_edit::command::BatchEditCellsCommand::new(map, changes.clone());
        let mut tx = wv_edit::history::Transaction::new();
        tx.push(Box::new(cmd));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                mark_dirty_cells(&mut tex, &changes);
            }
            Err(e) => bevy::log::warn!("连续绘制提交失败: {e}"),
        }
        session.stroke.discard();
        prompt.last_cell = None;
    }
}

/// **丢弃确认弹窗**（D3-C）。
///
/// 不用模态框（打断感太强）——用 egui 的窗口式确认，放在画布中央偏上。
fn confirm_discard(ui: Res<PointerOverUi>, pending: usize) -> bool {
    let mut confirmed = false;
    let mut cancelled = false;
    egui::Window::new("丢弃笔触")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
        .show(ui.ctx(), |ui| {
            ui.label(format!("要丢弃这一笔（{pending} 格）吗？"));
            ui.horizontal(|ui| {
                if ui.button("丢弃").clicked() {
                    confirmed = true;
                }
                if ui.button("继续画").clicked() {
                    cancelled = true;
                }
            });
        });
    let _ = cancelled;
    confirmed
}

/// 把改动过的格子标脏（地形纹理按脏矩形增量 patch）。
fn mark_dirty_cells(
    tex: &mut wv_render::terrain_view::TerrainTexture,
    changes: &[wv_edit::command::CellChange],
) {
    if changes.is_empty() {
        return;
    }
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for c in changes {
        min_x = min_x.min(c.gx.max(0) as u32);
        min_y = min_y.min(c.gy.max(0) as u32);
        max_x = max_x.max(c.gx.max(0) as u32 + 1);
        max_y = max_y.max(c.gy.max(0) as u32 + 1);
    }
    tex.mark_dirty([min_x, min_y, max_x, max_y]);
}
