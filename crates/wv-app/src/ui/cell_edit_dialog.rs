//! CellMap 编辑 dialog（ADR-0097）：双击弹出的**居中**编辑框。
//!
//! ## 为什么是独立 dialog（用户裁定 2026-10-31）
//! 1. **可达性**：格子编辑框此前住在笔刷窗（CellMap 编辑段），作者「不知道
//!    在哪里」；双击 = 直达。
//! 2. **系统参数上限**：Bevy 单系统 16 参数上限（`drill_pick` 曾以 17 炸过
//!    合并，ADR-0089）。本 dialog 是**独立系统 + 独立资源**，双击侧只置一个
//!    `open` 位（参数折进既有元组，不涨签名）。
//! 3. **四页签**：群系（命名物化）/ POI 快改 / 单格（地表·群系·海拔）/
//!    批量（±海拔、整选区换地表/群系 + 派生运算）——单格与批量**复用**
//!    统一属性表单的现成子件（`inspector::cell` 的 `render_fields` /
//!    `render_batch_section`），不另造机器；旧「地形编辑器」段随本 dialog
//!    从笔刷窗退役。
//!
//! ## 打开规则（双击侧裁决，[`decide_tab`]）
//! - 双击**改写了选区**（泛洪出新群系）→ 群系页签（命名流）；
//! - 双击**已选中的格**（点选器不泛洪，保护选区）→ 单格（1 格）或沿用
//!   上次页签（N 格，缺省批量）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{PlaybackState, ViewState, WorldProjection};
use crate::theme::ui_tokens::space;
use crate::ui::shell::UiShell;

/// dialog 页签（ADR-0097 四面板）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellEditTab {
    /// 群系：命名并物化为自然区。
    Biome,
    /// POI 快改（名称 / 级别 / 类别）。
    Poi,
    /// 单格：锚定格的地表 / 群系 / 海拔。
    Cell,
    /// 批量：整选区 ±海拔 / 换地表 / 换群系（+派生运算）。
    #[default]
    Batch,
}

impl CellEditTab {
    fn index(self) -> usize {
        match self {
            CellEditTab::Biome => 0,
            CellEditTab::Poi => 1,
            CellEditTab::Cell => 2,
            CellEditTab::Batch => 3,
        }
    }

    fn from_index(i: usize) -> Self {
        match i {
            0 => CellEditTab::Biome,
            1 => CellEditTab::Poi,
            2 => CellEditTab::Cell,
            _ => CellEditTab::Batch,
        }
    }
}

/// 双击开 dialog 的页签裁决（纯函数，可单测）。
///
/// 泛洪改写了选区 = 作者刚圈出一片新群系 → 命名流优先；点在已有选区内
/// （点选器不泛洪）→ 1 格走单格、N 格沿用上次页签（缺省批量）。
pub fn decide_tab(sel_changed: bool, sel_len: usize, last: CellEditTab) -> CellEditTab {
    if sel_changed {
        CellEditTab::Biome
    } else if sel_len == 1 {
        CellEditTab::Cell
    } else {
        last
    }
}

/// dialog 跨帧状态（会话态，不落盘）。`Default` 直接 derive：
/// open=false、tab=Batch（`CellEditTab::default`）、缓冲全空——与手写
/// impl 逐字段一致（clippy derivable_impls）。
#[derive(Resource, Default)]
pub struct CellEditDialogState {
    /// 弹窗开着。
    pub open: bool,
    /// 当前页签。
    pub tab: CellEditTab,
    /// 双击锚定格（栅格坐标）——单格 / 群系页签的编辑对象。
    pub anchor: (i32, i32),
    /// 群系命名输入缓冲（跨帧持久；物化成功后清空）。
    pub name: String,
    /// POI 快改缓冲（目标变换时从实体重初始化）。
    pub poi_target: Option<wv_core::Id>,
    pub poi_name: String,
    pub poi_rank: String,
    pub poi_kind: Option<wv_core::distance::PoiKind>,
}

impl CellEditDialogState {
    /// 双击打开（双击侧只调这个，不碰输入缓冲）。
    pub fn open_at(&mut self, tab: CellEditTab, anchor: (i32, i32)) {
        self.open = true;
        self.tab = tab;
        self.anchor = anchor;
    }
}

/// CellMap 编辑 dialog 系统（ADR-0097）。
///
/// 参数 15 个（上限 16）——(view, grid, playback) 折元组；这是**独立系统**，
/// 不给双击侧或任何既有系统涨签名（见模块文档第 2 条）。
#[allow(clippy::too_many_arguments)]
pub fn cell_edit_dialog(
    mut contexts: EguiContexts,
    shell_cfg: Res<crate::theme::ui_theme::UiShellConfig>,
    current_loc: Res<CurrentLocale>,
    mut state: ResMut<CellEditDialogState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut sel: ResMut<wv_render::state::CellSelection>,
    (view, grid, playback): (
        Res<ViewState>,
        Res<wv_render::state::GridConfig>,
        Res<PlaybackState>,
    ),
    mut edit_cell: ResMut<wv_render::state::EditCellState>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    mut nature: ResMut<crate::state::NatureRegionUiState>,
    mut derived: ResMut<crate::edit::derived_op::DerivedOpRequest>,
    mut cont: ResMut<crate::ui::ContinentDialog>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
) {
    if !state.open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let map = wv_core::Id::new_unchecked(view.current_map.clone());

    // 读格辅助（与 inspector::cell 同口径）。地形快照先借出（(w,h,cells
    // 引用) 不与 show 闭包内的 &mut projection 打架）。
    let mut close = false;
    // X 按钮：本地镜像（state 借位给 show 闭包，不能再借 &mut state.open）。
    let mut visible = state.open;
    egui::Window::new(t(loc, I18nKey::CellDlgTitle))
        .id(egui::Id::new("wv_cell_edit_dialog"))
        .order(bevy_egui::egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .resizable(false)
        .collapsible(false)
        .open(&mut visible)
        .show(ctx, |ui| {
            // ── 页签条（四面板）──
            let labels = [
                t(loc, I18nKey::CellDlgTabBiome),
                t(loc, I18nKey::CellDlgTabPoi),
                t(loc, I18nKey::CellDlgTabCell),
                t(loc, I18nKey::CellDlgTabBatch),
            ];
            if let Some(i) = crate::ui::widgets::tabs(
                ui,
                &shell,
                "cell_dlg_tabs",
                &labels,
                state.tab.index(),
                &[],
            ) {
                state.tab = CellEditTab::from_index(i);
            }
            ui.add_space(space::SM);

            match state.tab {
                CellEditTab::Biome => render_biome_tab(
                    ui,
                    &shell,
                    loc,
                    &mut state,
                    &mut projection,
                    &mut edit,
                    &view,
                    &grid,
                    &mut nature,
                    &mut notice,
                    &time,
                    &mut close,
                ),
                CellEditTab::Poi => render_poi_tab(
                    ui,
                    &shell,
                    loc,
                    &mut state,
                    &mut projection,
                    &mut edit,
                    &mut notice,
                    &time,
                    &mut close,
                ),
                CellEditTab::Cell => {
                    // 单格：锚定格绝对设值（复用统一属性表单的单格子件，
                    // 内部自带「应用」按钮与手势帧提交）。
                    match read_cell_of(&projection.world, &map, state.anchor.0, state.anchor.1) {
                        Some(before) => crate::ui::inspector::cell::render_fields(
                            ui,
                            &shell,
                            &mut projection,
                            Some(&map),
                            &mut edit,
                            &mut edit_cell,
                            state.anchor,
                            before,
                            &mut tex,
                            loc,
                        ),
                        None => {
                            ui.label(t(loc, I18nKey::CellDlgBatchEmpty));
                        }
                    }
                }
                CellEditTab::Batch => {
                    if sel.len() >= 2 {
                        // 读数条（与属性表单同款双段：hover / 选区）。
                        // read 只不可变借 world，用后即弃，随后的 &mut 借不冲突。
                        let read = |gx: i32, gy: i32| -> Option<(i16, u8, u8)> {
                            read_cell_of(&projection.world, &map, gx, gy)
                                .map(|c| (c.height, c.surface, c.biome))
                        };
                        crate::ui::inspector::cell_paint_panel::render_readout_bar(
                            ui, &shell, loc, &edit_cell, &sel, read,
                        );
                        ui.add_space(space::XS);
                        crate::ui::inspector::cell::render_batch_section(
                            ui,
                            &shell,
                            &mut projection,
                            &mut edit,
                            &mut tex,
                            loc,
                            &mut cont,
                            &mut sel,
                            Some(&map),
                            playback.playing,
                        );
                        ui.add_space(space::XS);
                        crate::ui::inspector::cell_paint_panel::render_derived_section(
                            ui,
                            &shell,
                            loc,
                            &mut derived,
                            sel.len() as usize,
                            playback.playing,
                        );
                    } else {
                        ui.label(t(loc, I18nKey::CellDlgBatchEmpty));
                    }
                }
            }

            ui.add_space(space::SM);
            ui.separator();
            // 单格 / 批量页签的子件自提交（应用按钮 / 手势帧）——底部只给关闭。
            if ui.button(t(loc, I18nKey::CellDlgClose)).clicked() {
                close = true;
            }
        });
    if close || !visible {
        state.open = false;
    }
}

/// 群系页签：命名 + 物化（ADR-0045 D4 命名即物化；候选为**纯派生**，
/// 与 Nature overlay 无关——「哪怕它没有创建」的命名半场）。
#[allow(clippy::too_many_arguments)]
fn render_biome_tab(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: crate::i18n::Locale,
    state: &mut CellEditDialogState,
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    view: &ViewState,
    grid: &wv_render::state::GridConfig,
    nature: &mut crate::state::NatureRegionUiState,
    notice: &mut crate::ui_shell::ShellNotice,
    time: &Time,
    close: &mut bool,
) {
    let map = wv_core::Id::new_unchecked(&view.current_map);
    let candidate =
        crate::edit::nature_region_actions::candidate_at(projection, &map, state.anchor);
    if let Some((biome, cells)) = candidate {
        let names = &wv_render::terrain_view::biome_raw::BIOME_NAMES_ZH;
        ui.label(
            egui::RichText::new(format!(
                "{} · {} {}",
                names.get(biome as usize).copied().unwrap_or("?"),
                cells,
                t(loc, I18nKey::PartitionPenSpanFaces)
            ))
            .small()
            .color(shell.t2()),
        );
    }
    ui.label(
        egui::RichText::new(t(loc, I18nKey::CellDlgBiomeHint))
            .small()
            .color(shell.t3()),
    );
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.label(t(loc, I18nKey::CellDlgBiomeName));
        ui.text_edit_singleline(&mut state.name);
        let can = !state.name.trim().is_empty() && candidate.is_some();
        if ui
            .add_enabled(
                can,
                egui::Button::new(t(loc, I18nKey::CellDlgMaterializeClose)),
            )
            .clicked()
        {
            // offset 口径 = 候选命名表单同款（map − raster 两原点差，
            // WORLDVIEW 两套原点陷阱的既有换算）。
            let offset = (
                grid.map_origin.x - grid.raster_origin.x,
                grid.map_origin.y - grid.raster_origin.y,
            );
            match crate::edit::nature_region_actions::materialize_nature_region_now(
                projection,
                edit,
                view,
                state.name.trim().to_owned(),
                state.anchor,
                grid.cell_size,
                offset,
            ) {
                Ok(id) => {
                    notice.info(
                        format!("已物化自然区「{}」({id})", state.name.trim()),
                        (time.elapsed_secs() * 60.0) as u64,
                    );
                    state.name.clear();
                    nature.pending_candidate = None;
                    *close = true;
                }
                Err(msg) => notice.error(msg, (time.elapsed_secs() * 60.0) as u64),
            }
        }
    });
}

/// POI 页签：选中地点的名称 / 级别 / 类别快改（Update + SetKind 同事务 =
/// 一次撤销）。
#[allow(clippy::too_many_arguments)]
fn render_poi_tab(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: crate::i18n::Locale,
    state: &mut CellEditDialogState,
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    notice: &mut crate::ui_shell::ShellNotice,
    time: &Time,
    close: &mut bool,
) {
    let target = projection
        .selection
        .as_ref()
        .and_then(|s| (s.kind == wv_core::EntityKind::Place).then(|| s.id.clone()));
    let Some(id) = target else {
        ui.label(t(loc, I18nKey::CellDlgPoiEmpty));
        return;
    };
    // 目标变换 → 从实体重初始化缓冲。
    if state.poi_target.as_ref() != Some(&id) {
        state.poi_target = Some(id.clone());
        if let Some(p) = projection
            .world
            .distances
            .places
            .iter()
            .find(|p| p.id == id)
        {
            state.poi_name = p.name.clone();
            state.poi_rank = p.rank.map(|r| r.to_string()).unwrap_or_default();
            state.poi_kind = p.kind;
        }
    }
    ui.label(
        egui::RichText::new(format!(
            "{}: {}",
            t(loc, I18nKey::CellDlgPoiTarget),
            id.as_str()
        ))
        .small()
        .color(shell.t3()),
    );
    ui.add_space(space::XS);
    ui.horizontal(|ui| {
        ui.label(t(loc, I18nKey::CellDlgBiomeName));
        ui.text_edit_singleline(&mut state.poi_name);
    });
    ui.horizontal(|ui| {
        // 级别自由文本（0–3；与地点属性表单同口径，空 = 不改）。
        ui.label("级别：");
        ui.add(egui::TextEdit::singleline(&mut state.poi_rank).desired_width(40.0));
    });
    ui.horizontal(|ui| {
        ui.label("类别：");
        let cur = state.poi_kind;
        egui::ComboBox::from_id_salt("wv_cell_dlg_poi_kind")
            .selected_text(match cur {
                Some(k) => t(loc, crate::ui::inspector::place::poi_kind_label_key(k)).to_owned(),
                None => t(loc, I18nKey::PoiKindOther).to_owned(),
            })
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(cur.is_none(), t(loc, I18nKey::PoiKindOther))
                    .clicked()
                {
                    state.poi_kind = None;
                }
                for k in wv_core::distance::PoiKind::ALL {
                    let label = t(loc, crate::ui::inspector::place::poi_kind_label_key(k));
                    if ui.selectable_label(cur == Some(k), label).clicked() {
                        state.poi_kind = Some(k);
                    }
                }
            });
    });
    ui.add_space(space::XS);
    if ui.button(t(loc, I18nKey::CellDlgApplyClose)).clicked() {
        let rank: Option<u8> = state.poi_rank.trim().parse().ok();
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(wv_edit::UpdatePlaceCommand::new(
            id.clone(),
            state.poi_name.clone(),
            rank,
            None,
            None,
        )));
        tx.push(Box::new(wv_edit::SetPlaceKindCommand::new(
            id.clone(),
            state.poi_kind,
        )));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                notice.info(
                    "已更新 POI".to_string(),
                    (time.elapsed_secs() * 60.0) as u64,
                );
                *close = true;
            }
            Err(e) => notice.error(
                format!("更新失败：{e}"),
                (time.elapsed_secs() * 60.0) as u64,
            ),
        }
    }
}

/// 读格（`inspector::cell::read_cell` 的自由函数版——egui show 闭包内无法
/// 持外层不可变闭包再传 `&mut projection`，借位在此收口）。
pub(crate) fn read_cell_of(
    world: &wv_core::World,
    map: &wv_core::Id,
    gx: i32,
    gy: i32,
) -> Option<wv_core::terrain::TerrainCellData> {
    let terrain = world.terrain_of(map)?;
    let w = terrain.width as i32;
    let h = terrain.height as i32;
    if gx < 0 || gy < 0 || gx >= w || gy >= h {
        return None;
    }
    terrain.cells.get((gy * w + gx) as usize).copied()
}

#[cfg(test)]
mod tests {
    //! 打开规则的页签裁决（ADR-0097）。

    use super::{decide_tab, CellEditTab};

    #[test]
    fn changed_selection_opens_biome_tab_for_naming() {
        assert_eq!(
            decide_tab(true, 64, CellEditTab::Batch),
            CellEditTab::Biome,
            "泛洪出新群系 = 命名流优先"
        );
    }

    #[test]
    fn single_cell_click_opens_cell_tab() {
        assert_eq!(
            decide_tab(false, 1, CellEditTab::Batch),
            CellEditTab::Cell,
            "1 格选区 = 单格编辑"
        );
    }

    #[test]
    fn multi_selection_keeps_last_tab() {
        assert_eq!(
            decide_tab(false, 12, CellEditTab::Batch),
            CellEditTab::Batch,
            "N 格沿用上次页签（缺省批量）"
        );
        assert_eq!(
            decide_tab(false, 12, CellEditTab::Poi),
            CellEditTab::Poi,
            "上次页签被保留"
        );
    }
}
