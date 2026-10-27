//! 派生运算的**执行侧**（[ADR-0052] D6 · §5 M3）。
//!
//! ## 三个纯函数在 `wv-terrain::cellops`
//!
//! | 函数 | 用途 | 分期 |
//! |---|---|---|
//! | `flood_region` | 同值泛洪**选择**（不写世界） | M2 |
//! | `smooth_cells` | 邻域平滑，返回变更集 | M2 纯函数 / M3 接 UI |
//! | `quantize_heights` | 按 N m 阶梯取整 | M3 |
//!
//! 本模块只做**编排**：读面板排的请求 → 调纯函数 → 组
//! `BatchEditCellsCommand` → 交 `EditHistory`。
//!
//! ## 铁律
//!
//! - **不改世界**：变更集一律经 `BatchEditCellsCommand` 落盘
//!   （[ADR-0021] §3.3「System 不得直接改世界」）——1 次派生运算 = 1 个撤销单位。
//! - **作用范围 = 统一选区 `CellSelection`**（ADR-0057）：没有选区就不运算
//!   （对「全图」做平滑会把作者没碰过的地方也改掉，属越权）。
//! - **播放中只读**：与其它写入口同口径（§9.4 Gate-1）。面板按钮保持可见
//!   但按下无效，见 `cell_paint_panel::render_derived_section`。

use bevy::prelude::*;

use wv_render::state::CellSelection;

use crate::state::{DrillState, PlaybackState, WorldProjection};

/// 派生运算种类（[ADR-0052] D6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedOp {
    /// 邻域平滑（保边界、不漂移）。
    Smooth,
    /// 按步长量化海拔（做等高线地形）。
    Quantize,
}

/// **派生运算请求**（面板写、系统消费）。
///
/// 拆成「请求 + 参数」而不是直接调命令，理由与工具切换同款：面板只表达
/// 意图，落盘动作统一由系统做出（单一写入点，便于加门禁 / 记日志）。
#[derive(Resource, Debug)]
pub struct DerivedOpRequest {
    /// 待执行的运算（`None` = 无请求）。
    pub pending: Option<DerivedOp>,
    /// 平滑强度 `0..=1`（`smooth_cells` 的 `strength`）。
    pub smooth_strength: f32,
    /// 量化步长（米，`quantize_heights` 的 `step`）。
    pub quantize_step_m: i16,
}

impl Default for DerivedOpRequest {
    fn default() -> Self {
        Self {
            pending: None,
            // 0.5 是「一次平滑能看出变化、又不会抹平地形」的折中值。
            smooth_strength: 0.5,
            // 10 m 是常规地形的等高距档位之一，与默认容差同量级。
            quantize_step_m: 10,
        }
    }
}

impl DerivedOpRequest {
    /// 排一次运算（面板按钮调用）。
    pub fn request(&mut self, op: DerivedOp) {
        self.pending = Some(op);
    }
}

/// **派生运算门控**（[ADR-0052] §9.4 Gate-1 的派生运算分支）。
///
/// 抽成纯函数的理由：门控是「播放中只读」这条纪律的唯一判定点，散在
/// `if` 里时改判据极易漏改一支（面板按钮的 `can_run` 与系统内的拒绝
/// 是**两处**，必须同源）。
///
/// 返回 `Err(原因)` = 不可执行。
pub fn derived_op_gate(playback_playing: bool, region_count: usize) -> Result<(), &'static str> {
    if playback_playing {
        return Err("播放中为只读：先暂停再执行派生运算");
    }
    if region_count == 0 {
        return Err("先在图上圈选一片区域（或点泛洪选择）");
    }
    Ok(())
}

/// **执行派生运算**（[ADR-0052] D6 · M3）。
///
/// gate：有请求 ∧ 非播放中 ∧ 有当前图 ∧ 统一选区非空。
#[allow(clippy::too_many_arguments)] // Bevy 系统参数即依赖声明，宽签名是框架惯例
pub fn apply_derived_op(
    mut request: ResMut<DerivedOpRequest>,
    playback: Res<PlaybackState>,
    drill: Res<DrillState>,
    mut projection: ResMut<WorldProjection>,
    sel: Res<CellSelection>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
) {
    let Some(op) = request.pending.take() else {
        return;
    };
    let frame = (time.elapsed_secs() * 60.0) as u64;
    // 门控与面板按钮的 `can_run` 同源（`derived_op_gate`）。
    if let Err(reason) = derived_op_gate(playback.playing, sel.len() as usize) {
        notice.error(reason, frame);
        return;
    }
    let Some(map) = drill.current().cloned() else {
        notice.error("没有当前地图", frame);
        return;
    };
    let Some(terrain) = projection.world.terrain_of(&map) else {
        notice.error("当前图没有地形数据", frame);
        return;
    };
    let cells: Vec<(i32, i32)> = sel.iter_cells().collect();
    let terrain_grid = to_terrain_grid(terrain);
    let changes = match op {
        DerivedOp::Smooth => {
            wv_terrain::cellops::smooth_cells(&terrain_grid, &cells, request.smooth_strength)
        }
        DerivedOp::Quantize => wv_terrain::cellops::quantize_heights(
            &terrain_grid,
            &cells,
            request.quantize_step_m as i32,
        ),
    };
    if changes.is_empty() {
        notice.info("没有需要变化的格（这一片已经是目标形态）", frame);
        return;
    }
    // 组 1 条批命令 = 1 个撤销单位（[ADR-0021] §3.3）。
    let cell_changes: Vec<wv_edit::command::CellChange> = changes
        .into_iter()
        .map(|((gx, gy), before, after)| wv_edit::command::CellChange {
            gx,
            gy,
            before: wv_core::terrain::TerrainCellData {
                height: before.height,
                surface: before.surface,
                biome: before.biome,
            },
            after: wv_core::terrain::TerrainCellData {
                height: after.height,
                surface: after.surface,
                biome: after.biome,
            },
        })
        .collect();
    let n = cell_changes.len();
    let cmd = wv_edit::command::BatchEditCellsCommand::new(map, cell_changes.clone());
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // ADR-0070 D5（T9）：提交后重建推导边（幂等亚毫秒；不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
            mark_dirty_cells(&mut tex, &cell_changes);
            let label = match op {
                DerivedOp::Smooth => "已平滑",
                DerivedOp::Quantize => "已量化",
            };
            notice.info(format!("{label} {n} 格"), frame);
        }
        Err(e) => notice.error(format!("派生运算提交失败：{e}"), frame),
    }
}

/// 世界地形（`wv_core::TerrainData`）→ `wv_terrain::TerrainGrid`。
///
/// 与 `edit::flood_select::to_terrain_grid` 同款结构转换（两处都用一次，
/// 各自持有一份而不抽公共：抽出去要开一个只装 12 行函数的新模块，
/// 收益低于可读性损耗）。
fn to_terrain_grid(terrain: &wv_core::terrain::TerrainData) -> wv_terrain::TerrainGrid {
    let cells = terrain
        .cells
        .iter()
        .map(|c| wv_terrain::TerrainCell {
            height: c.height,
            surface: c.surface,
            biome: c.biome,
        })
        .collect();
    wv_terrain::TerrainGrid::from_cells(terrain.width, terrain.height, cells)
        .expect("cells.len() == width * height（世界数据自洽）")
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

    // ── §9.4 Gate-1 · 派生运算门控 ──────────────────────────────────

    #[test]
    fn gate_blocks_while_playback_is_running() {
        // 播放中即使有选区也不许写——只读态优先于「有东西可改」。
        let err = derived_op_gate(true, 100).expect_err("播放中应拒绝");
        assert!(err.contains("播放中"), "原因应点明播放中，got {err}");
    }

    #[test]
    fn gate_blocks_without_region_selection() {
        let err = derived_op_gate(false, 0).expect_err("无选区应拒绝");
        assert!(err.contains("圈选"), "原因应指路「先圈选」，got {err}");
    }

    #[test]
    fn gate_allows_when_paused_and_region_present() {
        assert!(
            derived_op_gate(false, 1).is_ok(),
            "暂停 + 有选区应放行（哪怕只 1 格）"
        );
        assert!(derived_op_gate(false, 50_000).is_ok(), "大选区也放行");
    }

    // ── 请求默认值 ─────────────────────────────────────────────────

    #[test]
    fn request_defaults_are_usable() {
        let req = DerivedOpRequest::default();
        assert!(req.pending.is_none(), "默认无待执行运算");
        assert!(
            (0.0..=1.0).contains(&req.smooth_strength),
            "强度默认值应落在 0..=1，got {}",
            req.smooth_strength
        );
        assert!(req.quantize_step_m > 0, "步长默认值应为正");
    }

    #[test]
    fn request_sets_pending_op() {
        let mut req = DerivedOpRequest::default();
        req.request(DerivedOp::Quantize);
        assert_eq!(req.pending, Some(DerivedOp::Quantize));
        req.request(DerivedOp::Smooth);
        assert_eq!(req.pending, Some(DerivedOp::Smooth), "后来的请求覆盖前者");
    }
}
