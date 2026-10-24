//! 地形笔刷 + 街道编辑器面板（P-3 / ADR-0010 编辑器 UI）。
//!
//! ## 职责
//! - `panel_brush` —— 笔刷开关 / 地形材料 / 半径 / 软度 / 强度 / 街道模式 / 撤销段。
//! - 工具区（2026-10-24 用户裁定）：原「涂·塑·放」三段控件与组内图标行**整体
//!   移除**——工具切换只走命令面板 / 快捷键（`CommandId::SelectBrushTool`），
//!   面板只显示当前工具的名称与交互提示。原结论（2026-10-16「三组常驻、1 击
//!   直达」）由此条修正，理由：分组控件与图标行把面板上半截占满，而工具切换
//!   的实际频次低于材质 / 半径调参。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界 —— 撤销段生成 `DeleteStreetSegmentCommand` 交给 [`crate::edit::editing::EditHistory`]。
//! - **不**持有街道矢量数据 —— 那是 [`crate::streets`]。
//! - **不**管参照网格 —— 显示开关已上地图工具条（`map_controls`），状态在
//!   `ViewState.grid_lines`，与本面板无关。
//!
//! ## 主要导出
//! - `panel_brush` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::i18n::{t, t_key, CurrentLocale, I18nKey};
use crate::state::{
    BrushState, BrushTool, DrillState, Panel, PanelState, PlaybackState, WorldProjection,
};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui_layout::LayoutFile;
use crate::ui_shell::ShellNotice;
use wv_render::place_line::PlaceLineEditorState;
use wv_render::streets;
use wv_render::terrain_view::TerrainTexture;

use super::shell::{terrain_name_at, UiShell};
use super::timeline::undo_last_street_segment;
use super::widgets::{float_window, slider_row_brush, terrain_grid};

/// 地形笔刷 + 街道编辑器面板（P-3 / ADR-0010 编辑器 UI）。
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
    mut brush: ResMut<BrushState>,
    (mut streets, mut rivers): (
        ResMut<streets::StreetEditorState>,
        ResMut<PlaceLineEditorState>,
    ),
    (mut projection, drill, mut edit): (
        ResMut<WorldProjection>,
        Res<DrillState>,
        ResMut<editing::EditHistory>,
    ),
    (grid, mut tex, playback, view): (
        Res<crate::state::GridConfig>,
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
            // ── Brush 段切换（真切换：点亮时再点 = 关笔刷）──
            // 历史上这里只有「开」没有「关」（点击无条件置 true），作者开了
            // 笔刷就只能重启找回左键平移——现在对称了。
            ui.horizontal(|ui| {
                let seg_w = 60.0;
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(seg_w, shell.hit), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(rect, radius::CONTROL, shell.raised_bg());
                if brush.enabled {
                    p.rect_filled(rect, radius::CONTROL, shell.accent_soft());
                }
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t_key(&current_loc, I18nKey::BrushModeShort),
                    proportional(font_size::SMALL),
                    if brush.enabled {
                        shell.t1()
                    } else {
                        shell.t2()
                    },
                );
                let resp = ui.interact(rect, egui::Id::new("wv_brush_seg"), egui::Sense::click());
                if resp.clicked() {
                    brush.enabled = !brush.enabled;
                }
            });
            ui.add_space(space::SM);

            // 工具选择（原「涂 / 塑 / 放」三段控件 + 组内图标行）已于
            // 2026-10-24 用户裁定移除：改工具只在命令面板 / 快捷键里
            // （`CommandId::SelectBrushTool`），面板只负责显示当前工具。
            if brush.tool.is_intent() {
                // ── 意图工具：强度 + 半径（河不用半径）+ 当前工具与提示 ──
                slider_row_brush(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::BrushStrengthLabel),
                    &mut brush.strength,
                    0.2..=2.0,
                    |n| format!("×{:.2}", n),
                    false,
                );
                if brush.tool != BrushTool::River {
                    slider_row_brush(
                        ui,
                        &shell,
                        t_key(&current_loc, I18nKey::BrushRadiusLabel),
                        &mut brush.radius,
                        grid.cell_size * 2.0..=600.0,
                        |n| format!("{} m", (n / grid.cell_size * 10.0).round() as i32),
                        true,
                    );
                }
                ui.add_space(space::XS);
                // 图标行没了，工具名是「当前手里是哪把工具」的唯一可见来源。
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        t(loc, tool_name_key(brush.tool)),
                        t(loc, tool_hint_key(brush.tool)),
                    ))
                    .size(shell.scale.small)
                    .color(shell.t3()),
                );
            } else if brush.tool == BrushTool::Eraser {
                // ── 地形橡皮刷（ADR-0043）：只留半径 + 软度 ──
                //
                // **刻意不渲染 10 格地形材料选择器**：橡皮写入的是本图底图态
                // （`MapBase`），`brush.kind` 在这条路径上完全不参与。若照常
                // 画出来，面板会一边显示「当前材质 · 草原」一边把材质忽略掉——
                // 那是一句谎话，而且比不画更糟：作者会以为自己擦出来的草原
                // 是他选的那块草原。
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::BrushBaseLabel))
                        .size(shell.scale.micro)
                        .color(shell.t3()),
                );
                ui.add_space(space::XS);
                slider_row_brush(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::BrushRadiusLabel),
                    &mut brush.radius,
                    grid.cell_size * 2.0..=600.0,
                    |n| format!("{} m", (n / grid.cell_size * 10.0).round() as i32),
                    true,
                );
                slider_row_brush(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::BrushSoftnessLabel),
                    &mut brush.softness,
                    0.0..=1.0,
                    |n| format!("{}%", (n * 100.0).round() as i32),
                    false,
                );
                ui.add_space(space::XS);
                // 明确说出「擦成什么」——底图是海洋还是陆地，是作者下笔前
                // 必须知道的一件事，且它随图而变（切子图就可能变）。
                let base_zh = drill
                    .current()
                    .and_then(|id| {
                        projection
                            .world
                            .maps
                            .iter()
                            .find(|m| Some(&m.id) == Some(id))
                    })
                    .and_then(|m| m.base)
                    .unwrap_or(wv_core::map::MapBase::Ocean)
                    .zh_label();
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        t_key(&current_loc, I18nKey::BrushHintErase),
                        t_key(&current_loc, I18nKey::BrushBaseNow).replace("{base}", base_zh)
                    ))
                    .size(shell.scale.small)
                    .color(shell.t3()),
                );
            } else {
                // ── 涂抹：地形材料（5×2 色块瓷砖，选中名常显在段标题旁）──
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(t_key(&current_loc, I18nKey::BrushMaterialHeader))
                            .size(shell.scale.micro)
                            .color(shell.t3()),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "· {}",
                            terrain_name_at(loc, brush.kind as usize)
                        ))
                        .size(shell.scale.micro)
                        .color(shell.t1()),
                    );
                });
                terrain_grid(ui, &shell, &mut brush.kind, loc);
                ui.add_space(space::SM);

                // ── 半径 / 软度滑条（Figma #3:20791：标签左、值右、轨在中）──
                slider_row_brush(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::BrushRadiusLabel),
                    &mut brush.radius,
                    grid.cell_size * 2.0..=600.0,
                    |n| format!("{} m", (n / grid.cell_size * 10.0).round() as i32),
                    true,
                );
                slider_row_brush(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::BrushSoftnessLabel),
                    &mut brush.softness,
                    0.0..=1.0,
                    |n| format!("{}%", (n * 100.0).round() as i32),
                    false,
                );
                ui.add_space(space::XS);
                // 快捷提示 + 交互提示合一行（t3 小字，不抢材料区的视觉）。
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        t_key(&current_loc, I18nKey::BrushHintPaint),
                        t_key(&current_loc, I18nKey::BrushHintRadiusShortcut),
                    ))
                    .size(shell.scale.small)
                    .color(shell.t3()),
                );
            }

            ui.add_space(space::SM);
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

            ui.add_space(space::SM);
            // 街道区（Figma `Road drawing · 3 segments` 折叠段；头文案 i18n）
            egui::CollapsingHeader::new(format!(
                "{} · {}{}",
                t_key(&current_loc, I18nKey::BrushRoadDrawingHeader),
                streets.drawn,
                t_key(&current_loc, I18nKey::BrushStreetsDrawnUnit),
            ))
            .default_open(streets.enabled || streets.drawn > 0)
            .show(ui, |ui| {
                ui.checkbox(
                    &mut streets.enabled,
                    t_key(&current_loc, I18nKey::BrushStreetsToggle),
                );
                if streets.enabled {
                    egui::ComboBox::new("wv_road_class", "")
                        .selected_text(streets.class.label())
                        .show_ui(ui, |ui| {
                            for c in wv_core::RoadClass::ALL {
                                ui.selectable_value(&mut streets.class, c, c.label());
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.label(t_key(&current_loc, I18nKey::BrushStreetNameLabel));
                        ui.text_edit_singleline(&mut streets.name);
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(t_key(
                                        &current_loc,
                                        I18nKey::BrushFinishLineBtn,
                                    ))
                                    .size(shell.scale.base)
                                    .color(shell.t1()),
                                )
                                .fill(shell.accent())
                                .min_size(egui::vec2(120.0, shell.hit))
                                .corner_radius(radius::CONTROL),
                            )
                            .clicked()
                        {
                            streets.last_click = None;
                        }
                        let deletable = drill
                            .current_node(&projection.world)
                            .and_then(streets::target_road_layer)
                            .and_then(|l| projection.world.street_of(&l.id))
                            .map_or(0, |d| d.len());
                        if ui
                            .add_enabled(deletable > 0, egui::Button::new("↶"))
                            .clicked()
                        {
                            undo_last_street_segment(&mut projection, &drill, &mut edit);
                            streets.drawn = streets.drawn.saturating_sub(1);
                        }
                    });
                }
            });

            // 小河区（ADR-0046）：左键逐点连线 / 右键收线；收线 = 建一个带
            // 折线的地点（亚格矢量，不占格）。
            ui.add_space(space::SM);
            egui::CollapsingHeader::new(format!(
                "{} · {}{}",
                t_key(&current_loc, I18nKey::BrushRiverDrawingHeader),
                rivers.drawn,
                t_key(&current_loc, I18nKey::BrushRiversDrawnUnit),
            ))
            .default_open(rivers.enabled || rivers.drawn > 0)
            .show(ui, |ui| {
                ui.checkbox(
                    &mut rivers.enabled,
                    t_key(&current_loc, I18nKey::BrushRiverToggle),
                );
                // 互斥：画小河与画街道都吃左键，同开没法用。
                if rivers.enabled && streets.enabled {
                    streets.enabled = false;
                }
                if rivers.enabled {
                    ui.horizontal(|ui| {
                        ui.label(t_key(&current_loc, I18nKey::BrushRiverNameLabel));
                        ui.text_edit_singleline(&mut rivers.name);
                    });
                    ui.horizontal(|ui| {
                        let finish = ui.add_enabled(
                            rivers.can_finish(),
                            egui::Button::new(
                                egui::RichText::new(t_key(
                                    &current_loc,
                                    I18nKey::BrushRiverFinishBtn,
                                ))
                                .size(shell.scale.base)
                                .color(shell.t1()),
                            ),
                        );
                        if finish.clicked() {
                            rivers.clear_stroke();
                        }
                        if ui.button("✕").clicked() {
                            rivers.enabled = false;
                            rivers.clear_stroke();
                        }
                    });
                    ui.label(
                        egui::RichText::new("收线 = 沿折线建一条小河（亚格矢量，不占格）")
                            .small()
                            .color(shell.t2()),
                    );
                }
            });

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

/// 工具 → 名称 Key（i18n 表是唯一文案源；`BrushTool::label()` 已退役）。
///
/// 图标行移除后仍需要它：面板顶部用「工具名 · 交互提示」告诉作者手里是哪把
/// 工具（`CommandId::SelectBrushTool` 换工具时面板也会跟着变名）。
fn tool_name_key(tool: BrushTool) -> I18nKey {
    match tool {
        BrushTool::PaintKind => I18nKey::BrushToolNamePaint,
        BrushTool::MountainRange => I18nKey::BrushToolNameRange,
        BrushTool::Plateau => I18nKey::BrushToolNamePlateau,
        BrushTool::Hills => I18nKey::BrushToolNameHills,
        BrushTool::Volcano => I18nKey::BrushToolNameVolcano,
        BrushTool::Lake => I18nKey::BrushToolNameLake,
        BrushTool::River => I18nKey::BrushToolNameRiver,
        BrushTool::Eraser => I18nKey::BrushToolNameErase,
    }
}

/// 工具 → 交互提示 Key（面板「工具名 · 提示」行用；原瓷砖 tooltip 第二行已随
/// 图标行移除）。
fn tool_hint_key(tool: BrushTool) -> I18nKey {
    match tool {
        BrushTool::PaintKind => I18nKey::BrushHintPaint,
        BrushTool::MountainRange => I18nKey::BrushHintRange,
        BrushTool::Plateau => I18nKey::BrushHintPlateau,
        BrushTool::Hills => I18nKey::BrushHintHills,
        BrushTool::Volcano => I18nKey::BrushHintVolcano,
        BrushTool::Lake => I18nKey::BrushHintLake,
        BrushTool::River => I18nKey::BrushHintRiver,
        BrushTool::Eraser => I18nKey::BrushHintErase,
    }
}

#[cfg(test)]
mod tests {
    use super::{tool_hint_key, tool_name_key};
    use crate::i18n::{t, Locale};
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

    /// 8 个工具的名称 / 提示 Key 全覆盖：中英双语都非空。图标行移除后，
    /// 面板只靠「工具名 · 交互提示」这一行说明手里是哪把工具，漏译 = 空泡。
    #[test]
    fn every_tool_resolves_name_and_hint_in_both_locales() {
        for tool in BrushTool::ALL {
            let name_key = tool_name_key(tool);
            let hint_key = tool_hint_key(tool);
            for loc in [Locale::Zh, Locale::En] {
                assert!(!t(loc, name_key).is_empty(), "{tool:?} {loc:?} 名称缺失");
                assert!(!t(loc, hint_key).is_empty(), "{tool:?} {loc:?} 提示缺失");
            }
        }
    }
}
