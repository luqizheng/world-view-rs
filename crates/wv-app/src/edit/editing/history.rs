//! 撤销 / 重做与派生诊断刷新。

use super::terrain::{terrain_diff_bbox, terrain_diff_cells};
use super::*;

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
    apply_history_command_detailed(redo, edit, projection, tex, playback).map(|o| o.message)
}

/// 撤销 / 重做的**详细结果**（[ADR-0052] D8-A 白闪需要受影响格集合）。
///
/// 与 [`apply_history_command`] 同一份逻辑，唯一区别是额外返回
/// **地形变化格集合**——撤销/重做后要对这几格做 250ms 白闪（D8-A：
/// 「撤销时画面差异肉眼不可辨，用户按 Ctrl+Z 后不知发生了什么」）。
///
/// 拆成两个函数而非改原函数签名：原函数有 8 处调用点（含 8 条测试），
/// 加参数会把「与白闪无关的调用方」也拖进来。
pub fn apply_history_command_detailed(
    redo: bool,
    edit: &mut EditHistory,
    projection: &mut WorldProjection,
    tex: &mut TerrainTexture,
    playback: &PlaybackState,
) -> Result<HistoryOutcome, String> {
    if playback.playing {
        return Err("播放中为只读：先暂停再撤销".into());
    }
    // 撤销 dirty 追踪（v0.2）：撤销/重做前快照地形 cells，事后逐格 diff 出
    // 变化包围盒，只把该矩形标脏——避免 `mark_all_dirty` 引发的整图重建。
    // 512² 地形 ≈ 786KB 克隆 + 一次 O(N) 比较，亚毫秒级；换来的收益是
    // 撤销一笔刷只重算笔刷包围盒而非整图（~80ms → <1ms）。
    //
    // Phase 2：快照对象必须是**纹理正在显示的那张图**的地形（当前图或它继承到的
    // 祖先图），否则 diff 出的矩形会落在另一张图上。cells 一并 clone 出来，
    // 免得 `after` 借着 `tex` 而后面又要 `tex.mark_dirty`。
    let before = wv_render::terrain_view::texture_terrain(tex, &projection.world, None)
        .map(|t| t.cells.clone());
    let result = if redo {
        edit.history.redo(&mut projection.world)
    } else {
        edit.history.undo(&mut projection.world)
    };
    match result {
        Ok((true, _dirty_paths)) => {
            projection.dirty = true;
            // ADR-0070 D5（T9）：撤销/重做会把命令整块回退（`cmd.revert`）或
            // 重放（事务重 commit），地点与手写边随之回退/重放——推导边是
            // 运行时投影，必须跟着重建，否则停在上一状态的陈旧投影上。
            // 不进脏路径：derived 永不落盘（见 road_sync 模块文档）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
            let after = wv_render::terrain_view::texture_terrain(tex, &projection.world, None)
                .map(|t| (t.width.max(1), t.cells.clone()));
            let mut changed_cells = Vec::new();
            match (before, after) {
                (Some(b), Some((w, a))) if b.len() == a.len() => {
                    // 有变化区域 → 增量重传该矩形；无地形变化（撤销的可能是
                    // 非地形命令）→ 不动纹理。
                    if let Some(rect) = terrain_diff_bbox(&b, &a, w) {
                        tex.mark_dirty(rect);
                    }
                    // 同一份 diff 出变化格集合 → D8-A 白闪定位。
                    changed_cells = terrain_diff_cells(&b, &a, w);
                }
                // 地形存在性或尺寸变了 → 全量重建。
                _ => tex.mark_all_dirty(),
            }
            Ok(HistoryOutcome {
                message: if redo {
                    "已重做".into()
                } else {
                    "已撤销".into()
                },
                changed_cells,
            })
        }
        Ok((false, _)) => Ok(HistoryOutcome {
            message: if redo {
                "没有可重做的操作".into()
            } else {
                "没有可撤销的操作".into()
            },
            changed_cells: Vec::new(),
        }),
        Err(e) => Err(format!("撤销/重做失败: {e}")),
    }
}

/// 撤销 / 重做的详细结果（[ADR-0052] D8-A）。
#[derive(Debug, Default)]
pub struct HistoryOutcome {
    /// 给用户看的结果说明（「已撤销」/「已重做」/「没有可撤销的操作」）。
    pub message: String,
    /// **地形变化格集合**——撤销/重做后白闪这几格。
    ///
    /// 空 = 没有地形变化（撤销的是非地形命令，或撤销前后地形一致）→ 不闪。
    pub changed_cells: Vec<(i32, i32)>,
}

/// 阶段六收尾：刷新 `ProjectDiagnostics`。
///
/// **触发条件**：`ProjectDiagnostics::needs_diagnose` 为 true。
/// 三个 set 点：`apply_loaded_world`（加载后立刻灌过，标志被清回 false，
/// 这里只在新工程/编辑/撤销时跑）、`commit_stroke_on_release`、
/// `cmd_dispatch`（撤销/重做成功时）。
///
/// **何时跑**：Update 阶段，**早于**校验面板所在的 UI 帧。
/// 这样面板当帧读到的就是新结果（避免半帧错位）。
///
/// **节流**：当前不做节流——`derive_hints` 在本工程规模（≤ 几百事件）单次
/// 亚毫秒，无须防抖。若规模上来再加重算预算。
pub fn refresh_project_diagnostics(
    projection: Res<WorldProjection>,
    view: Res<crate::state::ViewState>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
) {
    if !diagnostics.needs_diagnose {
        return;
    }
    // 错误 + 提示。复用 `wv-io` 的引用校验逻辑（与 `Project::diagnose` 共享底层），
    // 但走的是**内存里的** `World` 而不是磁盘——编辑器不应当每帧都敲盘。
    //
    // 主题（`view.map_palette`）一并传入：区域覆盖物键是否可解析**依赖当前
    // 主题**（ADR-0045 追记一——换主题会让某个键失配，提示随之增减）。
    let theme = wv_render::theme::theme_pack::builtin::for_palette(view.map_palette);
    let d = crate::edit::diagnostics_bridge::derive_diagnostics(&projection.world, theme);
    diagnostics.last_count = d.items.len();
    diagnostics.diagnostics = d;
    diagnostics.needs_diagnose = false;
}
