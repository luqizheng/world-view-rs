//! CellMap 编辑 dialog（ADR-0097）：双击弹出、**打开时居中一次**的编辑框。
//!
//! 2026-11-01 根修：旧实现 `.anchor(CENTER_CENTER)` 每帧钉死屏幕正中——egui
//! 的 anchor 语义禁用拖拽，用户反馈「编辑选区无法移动」。改 `default_pos` +
//! 窗口 id 带 `open_seq`（每次打开重置窗口状态），打开即居中、之后可拖。
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

// 页签的**规范次序** = 群系 → POI → 单格 → 批量（dialog 装配处按此序过滤
// 出可见页签）。原先的 `index()/from_index()` 固定下标映射随动态装配退役——
// 可见集按前提过滤后不再有稳定的「下标 ↔ 页签」对应。

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

/// **可见页签**（按数据前提过滤）+ 回退后的当前页签（纯函数，可单测）。
///
/// 2026-11-02 用户裁定：「选中群系弹出属性框时，POI 与单格不需要」——用户
/// 实拍 712 格群系选区上，那两页要么是空态、要么语义不成立。故按**数据
/// 前提**决定页签在场与否，而不是四页永远并排：
///
/// - POI：仅当选中了 Place 实体（否则整页是「未选中地点」空态）；
/// - 单格：仅当选区恰为 1 格（多格选区没有「这一格」）；
/// - 群系 / 批量：常驻（命名物化与整选区编辑任何时候都可用）。
///
/// 当前页签若被隐藏（选区从 1 格变成 712 格），按选区规模回退：0 格 →
/// 群系 / 1 格 → 单格 / 多格 → 批量（与 [`decide_tab`] 同口径）。
pub fn visible_tabs(
    poi_ok: bool,
    cell_ok: bool,
    current: CellEditTab,
    sel_len: u32,
) -> (Vec<CellEditTab>, CellEditTab) {
    let visible: Vec<CellEditTab> = [
        CellEditTab::Biome,
        CellEditTab::Poi,
        CellEditTab::Cell,
        CellEditTab::Batch,
    ]
    .into_iter()
    .filter(|tab| match tab {
        CellEditTab::Poi => poi_ok,
        CellEditTab::Cell => cell_ok,
        _ => true,
    })
    .collect();
    let fallback = match sel_len {
        0 => CellEditTab::Biome,
        1 => CellEditTab::Cell,
        _ => CellEditTab::Batch,
    };
    let current = if visible.contains(&current) {
        current
    } else if visible.contains(&fallback) {
        fallback
    } else {
        // 前提与规模矛盾时的兜底：取可见集首项（群系恒在）。
        visible[0]
    };
    (visible, current)
}

/// dialog 跨帧状态（会话态，不落盘）。`Default` 直接 derive：
/// open=false、tab=Batch（`CellEditTab::default`）、seq=0、缓冲全空——与手写
/// impl 逐字段一致（clippy derivable_impls）。
#[derive(Resource, Default)]
pub struct CellEditDialogState {
    /// 弹窗开着。
    pub open: bool,
    /// 当前页签。
    pub tab: CellEditTab,
    /// 双击锚定格（栅格坐标）——单格 / 群系页签的编辑对象。
    pub anchor: (i32, i32),
    /// 打开序号：每次 [`Self::open_at`] 递增，进窗口 egui id——每次打开都
    /// 拿到全新窗口状态（无记忆位置），`default_pos` 的「居中一次」才生效。
    /// 若 id 恒定，egui 会记住上次拖走的位置，重开不再居中。
    pub open_seq: u64,
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
        self.open_seq = self.open_seq.wrapping_add(1);
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
    // 打开时居中一次、**之后可拖**（用户反馈「编辑选区无法移动」的根修）：
    // 旧写法 `.anchor(CENTER_CENTER, …)` 是每帧钉回屏幕正中——egui 的 anchor
    // 语义**禁用拖拽**，窗口永远搬不动。改 `default_pos`（只在窗口首次出现
    // 时生效）+ id 带 `open_seq`（每次打开换新窗口状态 = 无记忆位置，重开
    // 必然重新居中）；打开之后 egui 记住拖放位置，拖动照常工作。
    let screen_center = ctx.viewport_rect().center();
    egui::Window::new(t(loc, I18nKey::CellDlgTitle))
        .id(egui::Id::new("wv_cell_edit_dialog").with(state.open_seq))
        .order(bevy_egui::egui::Order::Foreground)
        .default_pos(screen_center)
        .resizable(false)
        .collapsible(false)
        .open(&mut visible)
        .show(ctx, |ui| {
            // ── 页签条：**按当前数据动态装配**（2026-11-02 用户裁定）──
            //
            // 「选中群系弹出属性框时，POI 与单格不需要」——判定规则与回退
            // 口径见 [`visible_tabs`]（纯函数，带单测）。
            let poi_ok = projection
                .selection
                .as_ref()
                .is_some_and(|s| s.kind == wv_core::EntityKind::Place);
            let cell_ok = sel.len() == 1;
            let (visible, current) = visible_tabs(poi_ok, cell_ok, state.tab, sel.len());
            state.tab = current;
            let selected = visible
                .iter()
                .position(|tab| *tab == state.tab)
                .unwrap_or(0);
            let labels: Vec<&str> = visible
                .iter()
                .map(|tab| match tab {
                    CellEditTab::Biome => t(loc, I18nKey::CellDlgTabBiome),
                    CellEditTab::Poi => t(loc, I18nKey::CellDlgTabPoi),
                    CellEditTab::Cell => t(loc, I18nKey::CellDlgTabCell),
                    CellEditTab::Batch => t(loc, I18nKey::CellDlgTabBatch),
                })
                .collect();
            if let Some(i) =
                crate::ui::widgets::tabs(ui, &shell, "cell_dlg_tabs", &labels, selected, &[])
            {
                state.tab = visible[i];
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
    // 已命名检测（2026-11-01 用户实测「物化后重开看不到名字」的根因之一）：
    // 旧实现只按格派生候选、对「这片已经命名过」毫无感知——重开是空白表单，
    // 再点一次物化就产出第二个重复区。offset 与物化同一条换算（两原点差）。
    let offset = (
        grid.map_origin.x - grid.raster_origin.x,
        grid.map_origin.y - grid.raster_origin.y,
    );
    let named = crate::edit::nature_region_actions::nature_region_containing(
        projection,
        &map,
        state.anchor,
        grid.cell_size,
        offset,
    );
    if let Some(id) = named {
        let region_name = projection
            .world
            .regions
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellDlgBiomeNamedHint))
                .small()
                .color(shell.t2()),
        );
        ui.label(egui::RichText::new(region_name).color(shell.t1()));
        ui.add_space(space::XS);
        ui.label(
            egui::RichText::new(t(loc, I18nKey::CellDlgBiomeManageHint))
                .small()
                .color(shell.t3()),
        );
        return;
    }
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
            // WORLDVIEW 两套原点陷阱的既有换算）——与上方已命名检测共用同一值。
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

    use super::{decide_tab, visible_tabs, CellEditTab};

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

    /// 用户裁定（2026-11-02）：选中群系（多格选区）时，**POI 与单格页签
    /// 不在场**——它们要么是空态、要么语义不成立（多格选区没有「这一格」）。
    #[test]
    fn biome_multi_selection_hides_poi_and_cell_tabs() {
        let (visible, current) = visible_tabs(false, false, CellEditTab::Biome, 712);
        assert_eq!(
            visible,
            vec![CellEditTab::Biome, CellEditTab::Batch],
            "多格群系选区只留「群系 / 批量」两页"
        );
        assert!(
            !visible.contains(&CellEditTab::Poi),
            "无 Place 实体选中时 POI 页应在场之外"
        );
        assert!(
            !visible.contains(&CellEditTab::Cell),
            "多格选区时单格页应在场之外"
        );
        assert_eq!(current, CellEditTab::Biome, "当前页签可见则保持");
    }

    /// 单格选区：单格页在场；POI 仍需 Place 实体选中才在场。
    #[test]
    fn single_cell_selection_shows_cell_tab() {
        let (visible, current) = visible_tabs(false, true, CellEditTab::Cell, 1);
        assert_eq!(
            visible,
            vec![CellEditTab::Biome, CellEditTab::Cell, CellEditTab::Batch],
            "单格选区留「群系 / 单格 / 批量」"
        );
        assert_eq!(current, CellEditTab::Cell, "当前页签可见则保持");
        // 选了 Place 实体 → POI 页回到场内。
        let (visible, _) = visible_tabs(true, true, CellEditTab::Poi, 1);
        assert!(
            visible.contains(&CellEditTab::Poi),
            "选中 Place 实体时 POI 页应在场"
        );
    }

    /// 当前页签被隐藏时的回退：多格 → 批量；单格 → 单格；无选区 → 群系。
    ///
    /// 注意前提：**当前页签必须真的被隐藏**才回退——批量页恒在场，故
    /// 「0 格 + 当前批量」保持批量不动（回退只针对消失的页签）。
    #[test]
    fn hidden_current_tab_falls_back_by_selection_size() {
        let (_, current) = visible_tabs(false, false, CellEditTab::Cell, 712);
        assert_eq!(current, CellEditTab::Batch, "多格 → 批量");
        let (_, current) = visible_tabs(false, true, CellEditTab::Poi, 1);
        assert_eq!(current, CellEditTab::Cell, "单格 → 单格");
        let (_, current) = visible_tabs(false, false, CellEditTab::Cell, 0);
        assert_eq!(current, CellEditTab::Biome, "无选区 → 群系");
        let (_, current) = visible_tabs(false, false, CellEditTab::Batch, 0);
        assert_eq!(current, CellEditTab::Batch, "批量页恒在场，不触发回退");
    }
}
