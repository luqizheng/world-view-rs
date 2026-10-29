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
    (mut tex, playback, mut view): (
        ResMut<TerrainTexture>,
        Res<PlaybackState>,
        ResMut<crate::state::ViewState>,
    ),
    (mut notice, time, current_loc): (ResMut<ShellNotice>, Res<Time>, Res<CurrentLocale>),
    // ── CellMap 编辑段（2026-10-24 自 Inspector 迁入）──
    // 第三个是统一选区（ADR-0057：Cell 表单的唯一编辑对象，可写——批量区
    // 「取消」按钮 = 清空选区）；第四个是点选工具激活态（ADR-0086：泛洪
    // 判据段跟随点选工具显示）。
    (mut edit_cell, mut cell_sel, point_select): (
        ResMut<wv_render::state::EditCellState>,
        ResMut<wv_render::state::CellSelection>,
        Res<crate::state::PointSelectState>,
    ),
    mut paint_session: ResMut<wv_world_cell_map::cell_paint::CellPaintSession>,
    mut flood_select: ResMut<crate::state::FloodSelectState>,
    mut derived: ResMut<crate::edit::derived_op::DerivedOpRequest>,
    // 批量区「生成大陆」按钮的目标面板（2026-10-26）——只置 open，确认
    // 提交在 continent_dialog_system（§3.3：UI 不碰 History 的例外同款）。
    mut continent: ResMut<crate::ui::ContinentDialog>,
    // 自然区划分段（ADR-0082）：CellMap 里的自然蛋糕编辑开关。
    mut partition_edit: ResMut<crate::ui::partition_pen::PartitionEditState>,
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

            // ══ 自然区划分段（ADR-0082，用户裁定 2026-10-28）══
            // 自然层编辑在 **CellMap**（Map 只留眼睛）；层全工程唯一。
            // 「划分自然区」= 打开分区编辑态（HUD + 切/拖/改群系三工具，
            // 与政治层同一套手势），顺带开会话势力可见性让分区叠加画出。
            if view.mode == wv_render::state::ViewMode::Topographic {
                let nature_layer = projection
                    .world
                    .faction_layers
                    .iter()
                    .find(|l| l.usage == wv_core::model::faction::LayerUsage::Nature);
                if let Some(nl) = nature_layer {
                    ui.add_space(space::SM);
                    ui.separator();
                    ui.add_space(space::XS);
                    let editing = partition_edit.active.as_ref().map(|(id, _)| id) == Some(&nl.id);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(t_key(&current_loc, I18nKey::NaturePartitionTitle))
                                .small()
                                .color(shell.t2()),
                        );
                        let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
                        let span_from = wv_core::partition_span_at(
                            &projection.world,
                            &map_id,
                            &nl.id,
                            playback.current_day,
                        )
                        .map(|s| s.from)
                        .unwrap_or(playback.current_day);
                        let label = t_key(
                            &current_loc,
                            if editing {
                                I18nKey::NaturePartitionExit
                            } else {
                                I18nKey::NaturePartitionEnter
                            },
                        );
                        if ui
                            .add_enabled(!playback.playing, egui::Button::new(label))
                            .clicked()
                        {
                            if editing {
                                partition_edit.active = None;
                            } else {
                                partition_edit.active = Some((nl.id.clone(), span_from));
                                partition_edit.tool = crate::ui::partition_pen::PartitionTool::Cut;
                                view.factions_visible = true;
                            }
                        }
                    });
                }
            }

            // ══ CellMap 编辑段（2026-10-24 自 Inspector「属性」页签迁入；
            // ADR-0058 统一属性表单）══
            //
            // **显示判据**（ADR-0058 D2，ADR-0086 修正）：EditCell 激活（落笔
            // 表单唯一合法作用域），或笔刷（Brush）激活（读数条 / 单格批量
            // 表单仍有效），或 Topographic 档下**选区非空**，或点选工具激活
            // （泛洪判据段跟随它显示）。
            //
            // **ADR-0086 修正**：落笔表单（落笔地表/群系/增减/半径）的值源
            // `CellPaintSession` **只被 EditCell 硬边拖绘消费**——笔刷（软边，
            // `commit_stroke_on_release`）读的是 `BrushState`（材料/半径/软度
            // 在 topOptionBar 调）。旧判据把 Brush(PaintKind) 也算落笔作用域，
            // 等于给作者看一组**调了没反应**的死控件（2026-10-28 实机反馈
            // 「笔刷大小调不动」的根因）。
            let paint_tool_active =
                brush.enabled && brush.tool == crate::state::BrushTool::PaintKind;
            let paint_scope = edit_cell.tool_active;
            let flood_criterion_scope =
                point_select.tool_active && view.mode == wv_render::state::ViewMode::Topographic;
            let cell_seg_active = paint_tool_active
                || paint_scope
                || flood_criterion_scope
                || (view.mode == wv_render::state::ViewMode::Topographic && !cell_sel.is_empty());
            if cell_seg_active {
                ui.add_space(space::SM);
                ui.separator();
                ui.add_space(space::XS);

                crate::ui::inspector::cell::cell_form(
                    ui,
                    &shell,
                    &mut projection,
                    &mut edit_cell,
                    &mut edit,
                    &mut tex,
                    current_loc.0,
                    &mut paint_session,
                    &mut flood_select,
                    &mut derived,
                    &mut continent,
                    playback.playing,
                    &mut cell_sel,
                    drill.current(),
                    paint_scope,
                    flood_criterion_scope,
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
