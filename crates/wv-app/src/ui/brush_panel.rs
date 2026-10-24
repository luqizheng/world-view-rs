//! CellMap 编辑面板（沿用 Brush 浮动窗；P-3 / ADR-0010 编辑器 UI）。
//!
//! ## 职责
//! - `panel_brush` —— 重置到底图 / CellMap 编辑段。
//! - 工具区（2026-10-24 用户裁定）：原「涂·塑·放」三段控件与组内图标行**整体
//!   移除**——工具切换只走命令面板 / 快捷键（`CommandId::SelectBrushTool`）。
//!   原结论（2026-10-16「三组常驻、1 击直达」）由此条修正。
//! - 笔刷段（2026-10-25 用户裁定）：Brush 开关段、强度 / 半径 / 软度滑条、
//!   地形材料网格、「工具名 · 提示」行**整体移除**——本面板只服务 CellMap
//!   编辑，不再承载地形笔刷调参；上一条中「面板只显示当前工具的名称与交互
//!   提示」随之失效。
//! - 街道 / 小河段（2026-10-25 用户裁定追记）：「Road drawing」「River
//!   drawing」两个折叠段**移除**——街道 / 小河绘制入口不在本窗。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界 —— 重置底图 / CellMap 编辑均生成 `EditCommand` 交给
//!   [`crate::edit::editing::EditHistory`]。
//! - **不**管参照网格 —— 显示开关已上地图工具条（`map_controls`），状态在
//!   `ViewState.grid_lines`，与本面板无关。
//!
//! ## 主要导出
//! - `panel_brush` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{BrushState, DrillState, Panel, PanelState, PlaybackState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::space;
use crate::ui_layout::LayoutFile;
use crate::ui_shell::ShellNotice;
use wv_render::terrain_view::TerrainTexture;

use super::shell::UiShell;
use super::widgets::float_window;

/// CellMap 编辑面板（Brush 浮动窗；P-3 / ADR-0010 编辑器 UI）。
///
/// **参数按元组折叠**（Bevy 0.19 单系统 16 参上限）：本系统带上了 CellMap
/// 编辑段（读数条 / 落笔值 / 泛洪判据 / 派生运算 / 单格与批量表单，
/// 2026-10-24 自 Inspector 迁入），资源超限，折成嵌套元组——每层算 1 参。
#[allow(clippy::too_many_arguments)]
pub fn panel_brush(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    brush: Res<BrushState>,
    (mut projection, drill, mut edit): (
        ResMut<WorldProjection>,
        Res<DrillState>,
        ResMut<editing::EditHistory>,
    ),
    (mut tex, playback, view): (
        ResMut<TerrainTexture>,
        Res<PlaybackState>,
        Res<crate::state::ViewState>,
    ),
    (mut notice, time, current_loc): (ResMut<ShellNotice>, Res<Time>, Res<CurrentLocale>),
    // ── CellMap 编辑段（2026-10-24 自 Inspector 迁入）──
    (mut edit_cell, mut selection, mut cell_mode, mut batch_land): (
        ResMut<wv_render::state::EditCellState>,
        ResMut<wv_render::state::SelectionState>,
        ResMut<crate::ui::inspector::cell::CellPanelMode>,
        ResMut<crate::ui::inspector::cell::BatchModeAutoLand>,
    ),
    mut paint_session: ResMut<wv_world_cell_map::cell_paint::CellPaintSession>,
    mut flood_select: ResMut<crate::state::FloodSelectState>,
    mut derived: ResMut<crate::edit::derived_op::DerivedOpRequest>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = current_loc.0;
    // v0.3：标题徽标 `ACTIVE` 当笔刷开着时亮起（金底），Figma 风格。
    let badge = if brush.enabled {
        Some((crate::ui::BadgeKind::Warn, "ACTIVE".into()))
    } else {
        None
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Brush,
        badge,
        loc,
        |ui| {
            // ── 「重置到底图」（2026-10-16）──
            // 破坏性操作：撤销栈兜底（EditCommand 走 History），所以不弹确认窗。
            // 「无工程闸门 / 无当前图」时按钮置灰——避免误触空指针。
            let has_map = drill.current().is_some();
            if ui
                .add_enabled(
                    has_map,
                    egui::Button::new(t_key(&current_loc, I18nKey::CmdResetTerrainToBase)),
                )
                .on_hover_text(t_key(&current_loc, I18nKey::CmdResetTerrainToBase))
                .clicked()
            {
                if let Some(map_id) = drill.current().cloned() {
                    let frame = (time.elapsed_secs() * 60.0) as u64;
                    match editing::apply_reset_terrain_to_base(
                        &map_id,
                        &mut edit,
                        &mut projection,
                        &mut tex,
                        &playback,
                    ) {
                        Ok(msg) => notice.info(msg, frame),
                        Err(msg) => notice.error(msg, frame),
                    }
                }
            }

            // ══ CellMap 编辑段（2026-10-24 自 Inspector「属性」页签迁入）══
            //
            // 用户裁定：「笔刷放到单独的浮动窗体中，采用既有笔刷窗」——
            // CellMap 的读数条 / 落笔值 / 泛洪判据 / 派生运算 / 单格与批量
            // 表单全部挪到这里；Inspector 回归通用属性检查。
            //
            // **显示判据**（与旧 Inspector 的 `cell_form_active` 同口径）：
            // EditCell / CellSelect 工具激活，或 Topographic 档下圈了选区
            // （圈选必切 V 工具，批量应用入口必须仍然可见）。
            // 非编辑态（普通笔刷模式）整段不渲染——笔刷窗保持原貌。
            let cell_seg_active = edit_cell.tool_active
                || (view.mode == wv_render::state::ViewMode::Topographic
                    && !edit_cell.region_cells.is_empty());
            if cell_seg_active {
                ui.add_space(space::SM);
                ui.separator();
                ui.add_space(space::XS);

                // 圈完自动落「批量」（边沿触发，随迁自 Inspector）。
                let has_region = !edit_cell.region_cells.is_empty();
                if has_region && !batch_land.had_region {
                    *cell_mode = crate::ui::inspector::cell::CellPanelMode::Batch;
                }
                batch_land.had_region = has_region;

                crate::ui::inspector::cell::cell_form(
                    ui,
                    &shell,
                    &mut projection,
                    &mut edit_cell,
                    &mut edit,
                    &mut selection,
                    &mut tex,
                    current_loc.0,
                    &mut cell_mode,
                    &mut paint_session,
                    &mut flood_select,
                    &mut derived,
                    playback.playing,
                );
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use crate::state::{BrushState, BrushTool};

    /// `BrushState::default()` 字段：笔刷默认关（Google 式手势——左键拖动
    /// 平移），默认工具 = 涂抹、默认材质 = 草原。
    #[test]
    fn brush_state_default_is_off_with_paint_tool() {
        let b = BrushState::default();
        assert!(!b.enabled, "笔刷默认关");
        assert_eq!(b.kind, 3, "默认 kind=草原");
        assert_eq!(b.tool, BrushTool::PaintKind, "默认工具=Paint");
        assert!((b.strength - 1.0).abs() < 1e-6, "默认强度 1.0");
    }
}
