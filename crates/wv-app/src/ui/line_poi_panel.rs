//! 「河流和道路」面板（ADR-0070 D4 · 计划 Task 11）。
//!
//! ## 一浮窗两态
//!
//! | 态 | 判据 | 内容 |
//! |---|---|---|
//! | **选中态** | `LinePoiState.selected` 有值 | 命中线 POI 的属性编辑：名称 +（河）宽度 /（路）级别，写回走 `UpdateLinePoiCommand` |
//! | **新建态** | 无选中（工具开着） | 当前工具设置：种类高亮段 + 名称 +（河）宽度 + 模式（绕行/下切）+ 钉点确认/取消 |
//!
//! 工具条分裂按钮换类 / 开关工具走分发器（T10）；面板里的种类段同样发
//! `SelectMapTool(LinePoi(kind))` 命令——同一条互斥链，不旁路。
//!
//! ## 边界（不做什么）
//!
//! - **不**直接改世界——写回与创建都经 `wv-edit` 命令
//!   （AGENTS §3.3；创建在 [`crate::edit::line_poi_tool::commit_pending_line`]）。
//! - **不**做地图手势——钉点 / 命中在 [`crate::edit::line_poi_tool`] 的
//!   交互系统里，本面板只出表单。
//!
//! ## 纯函数纪律
//!
//! 选中字段的填充（[`fill_fields_from_place`]）与写回裁决
//! （[`line_poi_writeback`]）是纯函数——「只在真变化时提交」可以单测，
//! 不把断言埋在 egui 渲染里。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use wv_core::distance::{LineKind, Place};
use wv_terrain::line_path::LinePathMode;

use crate::edit::editing;
use crate::edit::line_poi_tool::{
    cancel_pending, commit_pending_line, map_km_per_wu, width_m_to_wu, width_wu_to_m,
};
use crate::i18n::{t, CurrentLocale, I18nKey, Locale};
use crate::state::{DrillState, LinePoiState, Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_layout::LayoutFile;
use crate::ui_shell::ShellNotice;

use super::shell::UiShell;
use super::widgets::float_window;

/// 宽度数值输入的步进（显示单位）。
const WIDTH_STEP: f32 = 10.0;
/// 宽度上界（显示单位；走上界的河比多数图幅还宽，够用）。
const WIDTH_MAX: f32 = 100_000.0;
/// 宽度显示值在「真变化」判定里的相对容差（scrub 连续写值的浮点噪音）。
const WIDTH_EPS: f32 = 1e-2;

/// 面板属性写回（选中态）。「只在真变化时提交」的裁决（纯函数）：
///
/// - 三个草稿全部等于现值 → `None`（不发命令，不进撤销栈）；
/// - 任一真变化 → `Some(UpdateLinePoiCommand)`，未变的字段带 `None`
///   （**不改**语义，不是「写成 None」——命令族文档同款）；
/// - 河：名称 + 宽度（显示单位 ↔ 米经 `km_per_wu` 换算，方向与 T8 一致）；
/// - 路：名称 + 级别（宽度对道路无意义，不落盘）。
///
/// 空白名称不提交（改名改成空 = 误触，不是意图）。
pub fn line_poi_writeback(
    place: &Place,
    kind: LineKind,
    km_per_wu: Option<f32>,
    draft_name: &str,
    draft_width_display: f32,
    draft_rank: u8,
) -> Option<wv_edit::command::UpdateLinePoiCommand> {
    let name = draft_name.trim();
    let name_changed = !name.is_empty() && name != place.name.as_str();
    match kind {
        LineKind::River => {
            let cur = width_m_to_wu(place.line_width_m.unwrap_or(0.0), km_per_wu);
            let width_changed = (draft_width_display - cur).abs() > WIDTH_EPS * cur.abs().max(1.0);
            (name_changed || width_changed).then(|| {
                wv_edit::command::UpdateLinePoiCommand::new(
                    place.id.clone(),
                    name_changed.then(|| name.to_owned()),
                    width_changed.then(|| width_wu_to_m(draft_width_display, km_per_wu)),
                    None,
                )
            })
        }
        LineKind::Road => {
            let cur_rank = place.rank.unwrap_or_else(|| place.effective_rank());
            let rank_changed = draft_rank != cur_rank;
            (name_changed || rank_changed).then(|| {
                wv_edit::command::UpdateLinePoiCommand::new(
                    place.id.clone(),
                    name_changed.then(|| name.to_owned()),
                    None,
                    rank_changed.then_some(draft_rank),
                )
            })
        }
    }
}

/// 选中字段的填充（纯函数）：名称 + **显示单位**宽度。
///
/// 显示单位与 [`LinePoiState::width_m`] 同口径：有比例尺 = 米、无 = wu 原值。
pub fn fill_fields_from_place(place: &Place, km_per_wu: Option<f32>) -> (String, f32) {
    (
        place.name.clone(),
        width_m_to_wu(place.line_width_m.unwrap_or(0.0), km_per_wu),
    )
}

/// 浮窗一帧的产出（面板只表态，提交在窗外做——闭包借用干净）。
enum PanelOutcome {
    /// 无事发生。
    None,
    /// 「确认」按下且预览可成线（≥ 2 点）→ 提交 Create 事务。
    ConfirmPending,
    /// 「取消」按下 → 弃钉点会话。
    CancelPending,
}

/// 「河流和道路」面板（egui 浮窗；随工具开关，分发器裁决）。
#[allow(clippy::too_many_arguments)]
pub fn panel_line_poi(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut line_poi: ResMut<LinePoiState>,
    mut rank_draft: Local<(Option<wv_core::Id>, f32)>,
    drill: Res<DrillState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<editing::EditHistory>,
    mut notice: ResMut<ShellNotice>,
    time: Res<Time>,
    current_loc: Res<CurrentLocale>,
    mut palette: ResMut<crate::ui_command_palette::CommandPalette>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = current_loc.0;
    let frame = (time.elapsed_secs() * 60.0) as u64;
    let map = drill.current().cloned();
    let km = map
        .as_ref()
        .and_then(|mid| map_km_per_wu(&projection.world, mid));

    let mut outcome = PanelOutcome::None;
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::LinePoi,
        None,
        loc,
        |ui| {
            // 选中态优先：命中线 POI 的属性编辑。
            if let Some(id) = line_poi.selected.clone() {
                let Some(place) = projection
                    .world
                    .distances
                    .places
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                else {
                    // 选中目标已不在表里（撤销删了它等）：静默清选中回新建态。
                    line_poi.selected = None;
                    return;
                };
                let kind = selected_kind(&place);
                // 级别草稿跨帧驻留：换选中目标时从现值重灌（egui 滑条需要稳定的
                // &mut 目标，寄在 Local 里——级别不在 LinePoiState 的字段面里）。
                if rank_draft.0.as_ref() != Some(&place.id) {
                    rank_draft.0 = Some(place.id.clone());
                    rank_draft.1 = place.rank.unwrap_or_else(|| place.effective_rank()) as f32;
                }
                render_selected(
                    ui,
                    &shell,
                    loc,
                    &place,
                    kind,
                    km,
                    &mut line_poi,
                    &mut rank_draft.1,
                    &mut projection,
                    &mut edit,
                    &mut notice,
                    frame,
                );
                return;
            }
            outcome = render_new(ui, &shell, loc, &mut line_poi, km, &mut palette);
        },
    );

    // 窗外提交（闭包内不可再借 projection / edit）。
    match outcome {
        PanelOutcome::None => {}
        PanelOutcome::CancelPending => {
            cancel_pending(&mut line_poi.pending);
        }
        PanelOutcome::ConfirmPending => {
            let Some(map) = map else { return };
            let Some(pending) = line_poi.pending.clone() else {
                return;
            };
            let ok = commit_pending_line(
                &pending,
                line_poi.kind,
                line_poi.mode,
                line_poi.width_m,
                &line_poi.name,
                &map,
                &mut edit,
                &mut projection,
                &mut notice,
                frame,
            );
            if ok {
                line_poi.pending = None; // 连画：工具保持激活。
            }
        }
    }
}

/// 选中地点的**有效种类**（ADR-0070 D2 判别：缺省 = 河）。
fn selected_kind(place: &Place) -> LineKind {
    place.line_kind.unwrap_or(LineKind::River)
}

/// 选中态：属性编辑（名称 + 河:宽度 / 路:级别），写回走 `UpdateLinePoiCommand`。
#[allow(clippy::too_many_arguments)]
fn render_selected(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: Locale,
    place: &Place,
    kind: LineKind,
    km_per_wu: Option<f32>,
    line_poi: &mut LinePoiState,
    rank_draft: &mut f32,
    projection: &mut ResMut<WorldProjection>,
    edit: &mut editing::EditHistory,
    notice: &mut ShellNotice,
    frame: u64,
) {
    use crate::theme::ui_tokens::font_size;

    // 段头：编辑中的线（名称 + 种类）。
    ui.label(
        egui::RichText::new(format!(
            "{} · {}",
            place.name,
            t(
                loc,
                match kind {
                    LineKind::River => I18nKey::LinePoiKindRiver,
                    LineKind::Road => I18nKey::LinePoiKindRoad,
                }
            )
        ))
        .size(font_size::SMALL)
        .color(shell.accent()),
    );
    ui.add_space(4.0);

    // 名称（草稿寄 LinePoiState.name；失焦 / 回车 = 提交点）。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::LinePoiName))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        let resp = wv_ui::widgets::controls::FieldInput::new(&mut line_poi.name, "")
            .fill_width(true)
            .show(ui, shell);
        if resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            try_writeback(
                place,
                kind,
                km_per_wu,
                line_poi,
                *rank_draft,
                projection,
                edit,
                notice,
                frame,
            );
        }
    });

    match kind {
        LineKind::River => {
            // 宽度（显示单位；数值框手势完成帧才裁决提交）。
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(t(loc, I18nKey::LinePoiWidth))
                        .size(font_size::MICRO)
                        .color(shell.t3()),
                );
                let unit = width_unit_label(loc, km_per_wu);
                let mut w = line_poi.width_m;
                let report = ui
                    .allocate_ui(egui::vec2(120.0, 26.0), |ui| {
                        wv_ui::widgets::controls::numeric_input_salted_report(
                            ui,
                            shell,
                            "line_poi_width_sel",
                            &mut w,
                            unit,
                            WIDTH_STEP,
                            0.0,
                            WIDTH_MAX,
                        )
                        .1
                    })
                    .inner;
                line_poi.width_m = w.clamp(0.0, WIDTH_MAX);
                // 手势完成帧（scrub 松手 / 文本提交 / stepper 点击）才落盘。
                if report.changed && !report.gesture_active {
                    try_writeback(
                        place,
                        kind,
                        km_per_wu,
                        line_poi,
                        *rank_draft,
                        projection,
                        edit,
                        notice,
                        frame,
                    );
                }
            });
        }
        LineKind::Road => {
            // 级别（0–3；拖动中只改草稿，松手帧提交）。
            let mut rank_f = *rank_draft;
            let resp = wv_ui::widgets::slider_row(
                ui,
                shell,
                t(loc, I18nKey::LinePoiRank),
                &mut rank_f,
                0.0..=3.0,
                |v| format!("{}", v.round() as u8),
                false,
            );
            *rank_draft = rank_f.round().clamp(0.0, 3.0);
            if resp.drag_stopped() {
                try_writeback(
                    place,
                    kind,
                    km_per_wu,
                    line_poi,
                    *rank_draft,
                    projection,
                    edit,
                    notice,
                    frame,
                );
            }
        }
    }

    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t(loc, I18nKey::LinePoiHint))
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
}

/// 把当前草稿与现值比对，真变化才提交一条 `UpdateLinePoiCommand`。
#[allow(clippy::too_many_arguments)]
fn try_writeback(
    place: &Place,
    kind: LineKind,
    km_per_wu: Option<f32>,
    line_poi: &LinePoiState,
    rank_draft: f32,
    projection: &mut ResMut<WorldProjection>,
    edit: &mut editing::EditHistory,
    notice: &mut ShellNotice,
    frame: u64,
) {
    let Some(cmd) = line_poi_writeback(
        place,
        kind,
        km_per_wu,
        &line_poi.name,
        line_poi.width_m,
        rank_draft.round().clamp(0.0, 3.0) as u8,
    ) else {
        return; // 无真变化：不发命令、不进撤销栈。
    };
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            // 名称 / 级别 / 宽度都属地点表——推导边随之重建（T9）。
            crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
        }
        Err(e) => notice.error(format!("属性写回失败：{e}"), frame),
    }
}

/// 宽度的显示单位标注：有比例尺 = 米；无 = 「世界单位」（`LinePoiWidthUnitWu`）。
fn width_unit_label(loc: Locale, km_per_wu: Option<f32>) -> &'static str {
    match km_per_wu {
        Some(_) => "m",
        None => t(loc, I18nKey::LinePoiWidthUnitWu),
    }
}

/// 新建态：当前工具设置。产出 [`PanelOutcome`]（提交在窗外做）。
fn render_new(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: Locale,
    line_poi: &mut LinePoiState,
    km_per_wu: Option<f32>,
    palette: &mut crate::ui_command_palette::CommandPalette,
) -> PanelOutcome {
    use crate::theme::ui_tokens::font_size;
    let mut outcome = PanelOutcome::None;

    // ── 种类高亮段（河 / 路）：点击发 SelectMapTool 命令（同一互斥链）──
    crate::ui::widgets::segmented_control::segmented(
        ui,
        shell,
        "line_poi_kind",
        64.0,
        &[
            t(loc, I18nKey::LinePoiKindRiver),
            t(loc, I18nKey::LinePoiKindRoad),
        ],
        usize::from(line_poi.kind == LineKind::Road),
        font_size::MICRO,
        |i| {
            let picked = if i == 1 {
                LineKind::Road
            } else {
                LineKind::River
            };
            if picked != line_poi.kind {
                palette.pending = Some(crate::ui_command_palette::CommandId::SelectMapTool(
                    crate::ui_command_palette::MapToolId::LinePoi(picked),
                ));
            }
        },
    );
    ui.add_space(4.0);

    // 名称（创建草稿）。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(t(loc, I18nKey::LinePoiName))
                .size(font_size::MICRO)
                .color(shell.t3()),
        );
        wv_ui::widgets::controls::FieldInput::new(&mut line_poi.name, "")
            .fill_width(true)
            .show(ui, shell);
    });

    // 河：宽度（路不显示——道路宽度不落盘，描画按级别折算，ADR-0070 D4）。
    if line_poi.kind == LineKind::River {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t(loc, I18nKey::LinePoiWidth))
                    .size(font_size::MICRO)
                    .color(shell.t3()),
            );
            let unit = width_unit_label(loc, km_per_wu);
            let mut w = line_poi.width_m;
            ui.allocate_ui(egui::vec2(120.0, 26.0), |ui| {
                wv_ui::widgets::controls::numeric_input_salted(
                    ui,
                    shell,
                    "line_poi_width_new",
                    &mut w,
                    unit,
                    WIDTH_STEP,
                    0.0,
                    WIDTH_MAX,
                );
            });
            line_poi.width_m = w.clamp(0.0, WIDTH_MAX);
        });
    }

    // ── 模式（绕行 / 下切）——路与河同款两模式（ADR-0070 D3「同」语义）──
    crate::ui::widgets::segmented_control::segmented(
        ui,
        shell,
        "line_poi_mode",
        64.0,
        &[
            t(loc, I18nKey::LinePoiModeRoute),
            t(loc, I18nKey::LinePoiModeCarve),
        ],
        match line_poi.mode {
            LinePathMode::Route => 0,
            LinePathMode::Carve => 1,
        },
        font_size::MICRO,
        |i| {
            let picked = if i == 1 {
                LinePathMode::Carve
            } else {
                LinePathMode::Route
            };
            if picked != line_poi.mode {
                line_poi.mode = picked;
                // 模式变了：作废已规划预览（缓存键 = end_cell + 模式），
                // 交互系统下一帧按新模式重算恰好一次。
                if let Some(p) = &mut line_poi.pending {
                    p.preview.clear();
                }
            }
        },
    );

    // ── 钉点进行中：确认 / 取消 ──
    if line_poi.pending.is_some() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let can_confirm = line_poi
                .pending
                .as_ref()
                .is_some_and(|p| p.preview.len() >= 2);
            ui.add_enabled_ui(can_confirm, |ui| {
                if ui.button(t(loc, I18nKey::CommonConfirm)).clicked() {
                    outcome = PanelOutcome::ConfirmPending;
                }
            });
            if ui.button(t(loc, I18nKey::CommonCancel)).clicked() {
                outcome = PanelOutcome::CancelPending;
            }
        });
    }
    outcome
}

// ============================================================================
// 测试（纯函数；egui 渲染不进断言）
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::id::Id;

    fn river_place(name: &str, width_m: Option<f32>) -> Place {
        Place {
            id: Id::new_unchecked("river_1"),
            name: name.to_owned(),
            feature: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank: None,
            line: Some(vec![0.0, 0.0, 100.0, 0.0]),
            kind: None,
            line_kind: None, // 河（缺省语义）
            line_width_m: width_m,
        }
    }

    fn road_place(name: &str, rank: Option<u8>) -> Place {
        Place {
            id: Id::new_unchecked("road_1"),
            name: name.to_owned(),
            feature: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            biome: None,
            envelope: None,
            rank,
            line: Some(vec![0.0, 0.0, 100.0, 0.0]),
            kind: None,
            line_kind: Some(LineKind::Road),
            line_width_m: None,
        }
    }

    // ── 计划 Task 11 Step 1 · 测试 3 ────────────────────────────────

    /// 点中既有线 → 面板字段被填充（名称 + 显示单位宽度）。
    #[test]
    fn selecting_existing_line_fills_panel_fields() {
        // 有比例尺：2000 m、0.5 km/wu → 显示 4.0（wu）。
        let p = river_place("青龙河", Some(2000.0));
        let (name, width) = fill_fields_from_place(&p, Some(0.5));
        assert_eq!(name, "青龙河");
        assert!(
            (width - 4.0).abs() < 1e-4,
            "2000 m ÷ 0.5 km/wu = 4 wu，实际 {width}"
        );
        // 无比例尺：数字按 wu 直读。
        let (name, width) = fill_fields_from_place(&p, None);
        assert_eq!(name, "青龙河");
        assert!((width - 2000.0).abs() < 1e-4, "无比例尺按 wu 原值");
        // 未声明宽度 → 0（= 未声明语义，不是错）。
        let (_, width) = fill_fields_from_place(&river_place("无名", None), None);
        assert!((width - 0.0).abs() < 1e-6);
    }

    // ── 计划 Task 11 Step 1 · 测试 4 ────────────────────────────────

    /// 写回裁决：全同 → None；改名 → 只带 new_name；改宽 → 只带 new_width_m；
    /// 改级别（路）→ 只带 new_rank。
    #[test]
    fn panel_writeback_uses_update_command() {
        let p = river_place("青龙河", Some(2000.0));
        // 全同（无比例尺口径：显示宽 2000 = 2000 m）→ 不发命令。
        assert!(line_poi_writeback(&p, LineKind::River, None, "青龙河", 2000.0, 1).is_none());
        // 只改名。
        let cmd = line_poi_writeback(&p, LineKind::River, None, "赤水河", 2000.0, 1)
            .expect("改名应产出命令");
        assert_eq!(cmd.new_name.as_deref(), Some("赤水河"));
        assert_eq!(cmd.new_width_m, None, "未变的字段 = 不改（None）");
        assert_eq!(cmd.new_rank, None);
        // 只改宽（显示单位 4 → 米 2000，经比例尺换算）。
        let cmd = line_poi_writeback(&p, LineKind::River, Some(0.5), "青龙河", 8.0, 1)
            .expect("改宽应产出命令");
        assert_eq!(cmd.new_name, None);
        let w = cmd.new_width_m.expect("宽变化应带新值");
        assert!(
            (w - 4000.0).abs() < 1e-2,
            "8 wu × 0.5 km/wu × 1000 = 4000 m，实际 {w}"
        );
        // 空白名称不是改名（误触防线）。
        assert!(line_poi_writeback(&p, LineKind::River, None, "   ", 2000.0, 1).is_none());

        // 路：级别写回。
        let r = road_place("官道", None);
        // rank None → effective 折算（无 w/h = 1）；草稿 1 = 全同。
        assert!(line_poi_writeback(&r, LineKind::Road, None, "官道", 0.0, 1).is_none());
        let cmd =
            line_poi_writeback(&r, LineKind::Road, None, "官道", 0.0, 3).expect("改级别应产出命令");
        assert_eq!(cmd.new_rank, Some(3));
        assert_eq!(cmd.new_name, None);
        assert_eq!(cmd.new_width_m, None, "道路不写宽度");
    }
}
