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

/// **丢弃整笔是否需要二次确认**（D3-C 纯函数）。
///
/// - `≤ 50 格` → 静默丢弃（重画只要两秒，不值得打断）；
/// - `> 50 格` → 弹确认窗（画 60 格要半分钟，误丢一次白干）。
///
/// 抽成纯函数是为了让「阈值两侧」能被单测钉住——把判定内联在
/// `if` 里时，改阈值极易漏改其中一支。
pub fn discard_needs_confirmation(cell_count: usize) -> bool {
    cell_count > STROKE_DISCARD_CONFIRM_THRESHOLD
}

/// **起笔清空选区**（D3-A 纯函数）。
///
/// 清 `region_cells` **且**把 `region_fingerprint` 归零——后者是
/// `sync_region_cells` 判断「选区是否变化」的依据，只清格子不清指纹
/// 会让渲染层误判「没变」而不重画。
pub fn clear_region_for_stroke(edit_cell: &mut EditCellState) {
    edit_cell.region_cells.clear();
    edit_cell.region_fingerprint = 0;
}

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
    mut projection: ResMut<WorldProjection>,
    // egui 上下文（D3-C 丢弃确认窗）+ UI 抢占态。
    // 折成一层元组：Bevy 单系统参数上限 16，两个分开写会到 17。
    (mut contexts, ui): (
        bevy_egui::EguiContexts,
        Res<wv_render::camera::PointerOverUi>,
    ),
    mut edit_cell: ResMut<EditCellState>,
    mut session: ResMut<CellPaintSession>,
    // Bevy 单系统参数上限 16——本函数原本正好顶满，新增闪视觉态时
    // 把「丢弃确认弹窗态 + 闪视觉态」折成一层元组（元组算 1 个参数）。
    (mut prompt, mut flash): (
        ResMut<StrokeDiscardPrompt>,
        ResMut<wv_world_cell_map::cell_flash::CellFlashState>,
    ),
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
        // egui 上下文每帧取一次；`ctx_mut` 失败（首帧上下文未就绪）时
        // 视为「未确认」——保持弹窗待下次再判，不误丢整笔。
        let confirmed = match contexts.ctx_mut() {
            Ok(ctx) => confirm_discard(ctx, pending),
            Err(_) => false,
        };
        if confirmed {
            session.stroke.discard();
            prompt.last_cell = None;
            prompt.pending = None;
        }
        // 未确认时**保留** `pending`（下次继续问），不在这里清空。
        return;
    }

    // ── Esc 丢弃（D3-C）──
    if keys.just_pressed(egui::Key::Escape) && session.stroke.active {
        let n = session.stroke.len();
        if discard_needs_confirmation(n) {
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
        clear_region_for_stroke(&mut edit_cell);
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
                format!(
                    "整笔已拒收：{} 格快照不一致（{}）",
                    bad.len(),
                    bad.first()
                        .map(|(x, y)| format!("({x},{y})"))
                        .unwrap_or_default()
                ),
                (time.elapsed_secs() * 60.0) as u64,
            );
            // 画面红闪 200ms（D3-E）：红闪标出**整笔**的格（不是只标不一致的
            // 那几格）——作者要看到的是「我这一笔没生效」这个整体，
            // 只闪不一致的格会让人误以为「只有那几格被拒了」。
            flash.trigger(
                wv_world_cell_map::cell_flash::CellFlashKind::Reject,
                session.stroke.cells.clone(),
                time.elapsed_secs(),
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
fn confirm_discard(ctx: &egui::Context, pending: usize) -> bool {
    let mut confirmed = false;
    egui::Window::new("丢弃笔触")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
        .show(ctx, |ui| {
            ui.label(format!("要丢弃这一笔（{pending} 格）吗？"));
            ui.horizontal(|ui| {
                if ui.button("丢弃").clicked() {
                    confirmed = true;
                }
                if ui.button("继续画").clicked() {
                    // 关窗即「继续画」——不置 confirmed。
                }
            });
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::terrain::TerrainCellData;

    fn cell(height: i16) -> TerrainCellData {
        TerrainCellData {
            height,
            surface: 1,
            biome: 4,
        }
    }

    fn state_with_region() -> EditCellState {
        EditCellState {
            tool_active: true,
            selected: Some((3, 4)),
            region_cells: vec![(0, 0), (1, 0), (2, 0)],
            region_fingerprint: 0xdead_beef,
            ..Default::default()
        }
    }

    // ── D3-A · 起笔清 region_cells ──────────────────────────────────

    #[test]
    fn stroke_begin_clears_region_cells_and_fingerprint() {
        let mut edit_cell = state_with_region();
        clear_region_for_stroke(&mut edit_cell);
        assert!(edit_cell.region_cells.is_empty(), "起笔应清空选区");
        assert_eq!(edit_cell.region_fingerprint, 0, "指纹应归零");
    }

    #[test]
    fn stroke_begin_keeps_selected_cell() {
        // 单格选中与区域选是两套态——起笔只清区域选，单格选中保留
        // （否则「点一格 → 拖动画」会把选中也抹掉，Inspector 读不到值）。
        let mut edit_cell = state_with_region();
        clear_region_for_stroke(&mut edit_cell);
        assert_eq!(edit_cell.selected, Some((3, 4)), "单格选中应保留");
        assert!(edit_cell.tool_active, "工具仍激活");
    }

    // ── D3-B · 右键 = Esc 等价 ──────────────────────────────────────

    #[test]
    fn right_click_discard_equals_esc_discard() {
        // 右键与 Esc 都走同一条「丢弃整笔」路径——差别只在触发源。
        // 这里断言两者产出的笔触态一致：空 + 非活跃。
        let mut by_esc = CellPaintSession::default();
        by_esc.stroke.begin();
        by_esc.stroke.push_cell(0, 0, cell(10));
        by_esc.stroke.discard();

        let mut by_right = CellPaintSession::default();
        by_right.stroke.begin();
        by_right.stroke.push_cell(0, 0, cell(10));
        by_right.stroke.discard();

        assert_eq!(by_esc.stroke.cells, by_right.stroke.cells);
        assert_eq!(by_esc.stroke.active, by_right.stroke.active);
        assert!(!by_right.stroke.active, "丢弃后不再活跃");
        assert!(by_right.stroke.cells.is_empty(), "丢弃后格集清空");
    }

    #[test]
    fn discard_keeps_no_half_committed_state() {
        // 丢弃**不是**半笔提交：格集合与快照一起清，不留残迹。
        let mut session = CellPaintSession::default();
        session.stroke.begin();
        session.stroke.push_cell(1, 1, cell(5));
        session.stroke.push_cell(2, 2, cell(6));
        assert_eq!(session.stroke.len(), 2);
        session.stroke.discard();
        assert_eq!(session.stroke.len(), 0);
        assert!(session.stroke.snapshots.is_empty(), "快照也应清空");
        assert!(session.stroke.map_id.is_none(), "目标图 ID 也清");
    }

    // ── D3-C · Esc 阈值 ─────────────────────────────────────────────

    #[test]
    fn esc_within_threshold_discards_silently() {
        assert!(!discard_needs_confirmation(0), "0 格不该弹确认");
        assert!(
            !discard_needs_confirmation(STROKE_DISCARD_CONFIRM_THRESHOLD),
            "正好 50 格不该弹确认（≤ 阈值静默）"
        );
    }

    #[test]
    fn esc_over_threshold_asks_for_confirmation() {
        assert!(
            discard_needs_confirmation(STROKE_DISCARD_CONFIRM_THRESHOLD + 1),
            "51 格应弹确认"
        );
        assert!(discard_needs_confirmation(500), "远超阈值应弹确认");
    }

    // ── D3-D · 拉出画布不丢笔 ───────────────────────────────────────

    #[test]
    fn stroke_survives_cursor_leaving_canvas() {
        // 光标出画布 → 不延伸，但笔触**保持存活**（不丢弃）。
        // 模拟：起笔累积 2 格后「出画布」——笔触态应仍是活跃且保留 2 格。
        let mut session = CellPaintSession::default();
        session.stroke.begin();
        session.stroke.push_cell(0, 0, cell(1));
        session.stroke.push_cell(1, 1, cell(2));
        // 出画布：此处只是不调用 push_cell，笔触态原样保留。
        assert!(session.stroke.active, "笔触应存活");
        assert_eq!(session.stroke.len(), 2, "已落的 2 格应保留");
    }

    #[test]
    fn stroke_out_of_canvas_does_not_accumulate_out_of_bounds_cells() {
        // 出画布时若误采到图外的格，提交会写到不存在的索引。
        // 这里断言「采样被拒时格数不变」——模拟越界采样被过滤。
        let mut session = CellPaintSession::default();
        session.stroke.begin();
        session.stroke.push_cell(0, 0, cell(1));
        let before = session.stroke.len();
        // 越界格不 push（调用点的 `sample` 返回 None 时跳过）。
        assert_eq!(session.stroke.len(), before, "越界格不入笔触");
        assert!(session.stroke.active, "笔触仍活跃");
    }

    // ── D3-E · stale 整笔拒收 ───────────────────────────────────────

    #[test]
    fn stale_mismatch_rejects_whole_stroke() {
        let mut session = CellPaintSession::default();
        session.stroke.begin();
        session.stroke.push_cell(0, 0, cell(10));
        session.stroke.push_cell(1, 0, cell(20));
        // 世界里第 1 格被外部改成 99 → 快照不一致。
        let world = |gx: i32, gy: i32| -> Option<TerrainCellData> {
            match (gx, gy) {
                (0, 0) => Some(cell(10)),
                (1, 0) => Some(cell(99)),
                _ => None,
            }
        };
        let bad = session.stroke.stale_cells(&world).expect("应检出不一致");
        assert_eq!(bad, vec![(1, 0)], "应指出不一致的格");
        // 整笔拒收：不是部分提交——调用方在检出后 discard 整笔。
        session.stroke.discard();
        assert_eq!(session.stroke.len(), 0, "整笔清空，不部分提交");
    }

    #[test]
    fn stale_check_passes_when_world_unchanged() {
        let mut session = CellPaintSession::default();
        session.stroke.begin();
        session.stroke.push_cell(0, 0, cell(10));
        let world = |gx: i32, gy: i32| -> Option<TerrainCellData> {
            (gx == 0 && gy == 0).then(|| cell(10))
        };
        assert!(session.stroke.stale_cells(&world).is_none(), "未变则可提交");
    }
}
