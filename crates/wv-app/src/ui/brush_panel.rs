//! 地形笔刷 + 街道编辑器面板（P-3 / ADR-0010 编辑器 UI）。
//!
//! ## 职责
//! - `panel_brush` —— 笔刷开关 / 三行行内工具分组（涂·塑·放）/ 地形材料 /
//!   半径 / 软度 / 强度 / 街道模式 / 撤销段。
//! - 工具区（2026-10-16 改版）：三个操作语义组**全部常驻**（不折叠、不 tab）——
//!   跨组切换是高频动作（涂几片 → 丢一座山 → 补条河），1 击直达比省高度值钱；
//!   活动工具高亮永远在视野里，左键会做什么一目了然。
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
use crate::i18n::{t, t_key, CurrentLocale, I18nKey, Locale};
use crate::state::{
    BrushState, BrushTool, BrushToolGroup, DrillState, Panel, PanelState, PlaybackState,
    WorldProjection,
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
use super::widgets::{
    float_window, segmented_control::segmented, slider_row_brush, terrain_grid, ToolButton,
    ToolIcon,
};
use super::{
    ICON_BRUSH, ICON_ERASER, ICON_HILLS, ICON_LAKE, ICON_MOUNTAIN_RANGE, ICON_PLATEAU, ICON_RIVER,
    ICON_VOLCANO,
};

/// 地形笔刷 + 街道编辑器面板（P-3 / ADR-0010 编辑器 UI）。
#[allow(clippy::too_many_arguments)]
pub fn panel_brush(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut brush: ResMut<BrushState>,
    mut streets: ResMut<streets::StreetEditorState>,
    mut rivers: ResMut<PlaceLineEditorState>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    mut edit: ResMut<editing::EditHistory>,
    grid: Res<crate::state::GridConfig>,
    mut tex: ResMut<TerrainTexture>,
    playback: Res<PlaybackState>,
    mut notice: ResMut<ShellNotice>,
    time: Res<Time>,
    current_loc: Res<CurrentLocale>,
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

            // ── 工具区：3 段控件（涂 / 塑 / 放）+ 段下瓷砖行 ──
            // 段控件在「段/工具/组」三态中担当「组」维度的 1 选 1，与顶栏
            // Browse/Edit / 主题段同源；下方瓷砖行只显示当前组内的工具
            // （1–4 块），状态机由 `sync_tool_group` 同步。
            tool_segmented(ui, &shell, &mut brush, current_loc.0);
            tool_row_for_group(ui, &shell, &mut brush, current_loc.0);
            ui.add_space(space::SM);
            ui.separator();
            ui.add_space(space::XS);

            if brush.tool.is_intent() {
                // ── 意图工具：强度 + 半径（河不用半径）+ 交互提示 ──
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
                ui.label(
                    egui::RichText::new(t(loc, tool_hint_key(brush.tool)))
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
        },
    );
}

/// 3 段控件：涂 / 塑 / 放。
///
/// 选段 = 写 `tool_group` + `sync_tool_group()`（让 tool 落到该组第一个
/// 工具，避免段/瓷砖分裂）+ 激活笔刷——「选工具 = 准备画」语义沿用提交 2。
fn tool_segmented(ui: &mut egui::Ui, shell: &UiShell, brush: &mut BrushState, loc: Locale) {
    let labels = [
        t(loc, I18nKey::BrushGroupPaint),
        t(loc, I18nKey::BrushGroupShape),
        t(loc, I18nKey::BrushGroupPlace),
    ];
    let groups = [
        BrushToolGroup::Paint,
        BrushToolGroup::Shape,
        BrushToolGroup::Place,
    ];
    let active = groups
        .iter()
        .position(|g| *g == brush.tool_group)
        .unwrap_or(0);
    segmented(
        ui,
        shell,
        "wv_brush_tool_group",
        60.0,
        &labels,
        active,
        font_size::SMALL,
        |i| {
            brush.tool_group = groups[i];
            brush.on_group_changed();
            brush.enabled = true;
        },
    );
}

/// 段下瓷砖行：只渲染 `brush.tool_group` 该组的 1–4 个工具。
///
/// 点瓷砖 = 写 `tool` + `sync_tool_group()` 反向同步 + 激活笔刷。
/// 1 击选工具，跨组 = 先段 1 击再瓷砖 1 击。
fn tool_row_for_group(ui: &mut egui::Ui, shell: &UiShell, brush: &mut BrushState, loc: Locale) {
    let tools: &[(egui::ImageSource<'static>, BrushTool)] = match brush.tool_group {
        BrushToolGroup::Paint => &[
            (ICON_BRUSH, BrushTool::PaintKind),
            (ICON_ERASER, BrushTool::Eraser),
        ],
        BrushToolGroup::Shape => &[
            (ICON_MOUNTAIN_RANGE, BrushTool::MountainRange),
            (ICON_PLATEAU, BrushTool::Plateau),
            (ICON_HILLS, BrushTool::Hills),
            (ICON_RIVER, BrushTool::River),
        ],
        BrushToolGroup::Place => &[
            (ICON_VOLCANO, BrushTool::Volcano),
            (ICON_LAKE, BrushTool::Lake),
        ],
    };
    ui.horizontal(|ui| {
        for (icon, tool) in tools.iter() {
            let tip = format!(
                "{}\n{}",
                t(loc, tool_name_key(*tool)),
                t(loc, tool_hint_key(*tool)),
            );
            ToolButton::icon(ToolIcon::Svg(icon.clone()), &tip)
                .active(brush.tool == *tool)
                .show(ui, shell, || {
                    brush.tool = *tool;
                    brush.on_tool_changed();
                    brush.enabled = true;
                });
        }
    });
}

/// 工具 → 名称 Key（i18n 表是唯一文案源；`BrushTool::label()` 已退役）。
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

/// 工具 → 交互提示 Key（瓷砖 tooltip 第二行 / 面板底部提示行共用）。
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
    use crate::state::{BrushState, BrushTool, BrushToolGroup};

    /// `BrushState::default()` 字段：笔刷默认关（Google 式手势——左键拖动
    /// 平移），默认工具 = 涂抹、默认材质 = 草原。
    #[test]
    fn brush_state_default_is_off_with_paint_tool() {
        let b = BrushState::default();
        assert!(!b.enabled, "笔刷默认关");
        assert_eq!(b.kind, 3, "默认 kind=草原");
        assert_eq!(b.tool, BrushTool::PaintKind, "默认工具=Paint");
        assert_eq!(
            b.tool_group,
            BrushToolGroup::Paint,
            "默认组=Paint（与 tool.group() 一致）"
        );
        assert!((b.strength - 1.0).abs() < 1e-6, "默认强度 1.0");
    }

    /// 7 个工具的名称 / 提示 Key 全覆盖：中英双语都非空——瓷砖纯图标，
    /// tooltip 是这些名字的唯一入口，漏译 = 悬停出一个空泡。
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

    /// 分组口径与分派闸一致（三组划分全体的完整证明在 `state.rs` 测试，
    /// 这里钉的是面板行与组的对应关系不被手滑改乱）。
    #[test]
    fn panel_group_rows_match_brush_tool_groups() {
        assert_eq!(BrushTool::PaintKind.group(), BrushToolGroup::Paint);
        assert_eq!(BrushTool::SHAPE.len(), 4);
        assert_eq!(BrushTool::PLACE.len(), 2);
    }
}
