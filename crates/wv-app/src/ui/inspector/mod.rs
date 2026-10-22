//! 统一 Inspector 面板（2026-10-22 设计，单面板 · 按选中对象分派属性）。
//!
//! ## 职责
//! - [`panel_inspector`] —— 右栏**唯一**的属性面：窗口 + 页签条 + 分派。
//! - 三个页签：**属性**（当前选中对象的属性表单）/ **势力层**（层的增删改）/
//!   **事态**（事态列表）。
//!
//! ## 为什么要「一个面板」
//!
//! 改动前，属性编辑按**类型**分裂在四个浮窗里（区域卡 / 势力卡 / cell 编辑器 /
//! 计数式 Inspector）。作者要改一个对象的属性，得先知道「哪种对象在哪个面板里」
//! ——这是按实现分类而非按任务组织的界面。
//!
//! 基线 §1.11 的 P-4 原本写的就是「显示/编辑**当前选中对象的全部属性**」
//! （右侧停靠 · 320px），Figma `42:2` 与 web 原型也都是单一 Inspector。
//! 本模块把那个设计兑现回来：点地图上不同对象，同一窗口里换出该类型的表单。
//!
//! Bevy 生态的同款惯例见 `bevy_inspector_egui`（社区主流）：左树 + 右属性区，
//! 属性区按选中实体类型切换。`bevy_egui` 本身不管布局，惯例全在应用层——
//! 本项目世界数据在 YAML（Bevy 只是渲染投影），所以「换内容」纯在 egui 层。
//!
//! ## 分派规则
//!
//! 1. [`wv_render::state::EditCellState::tool_active`] → Cell 表单（工具态优先）。
//! 2. 否则 `projection.selection` 按 [`EntityKind`] 分派到四类表单。
//! 3. 否则 → 空态（空态卡 + 四条计数行 + 事态卡，现状不变）。
//!
//! ## 分层纪律
//!
//! 本模块**只读世界**并渲染；一切写入经 `wv-edit` 的 `EditCommand`
//! （AGENTS §3.3）。动作层在 [`crate::edit::inspector_actions`]。
//!
//! 关联：[2026-10-22 统一 Inspector 设计](../../../../docs/superpowers/specs/2026-10-22-unified-inspector-panel-design.md)、
//! [ADR-0040](../../../../docs/adr/ADR-0040-cell-inspector-tool.md)（决策 6 被本设计修正）、
//! [MAP_EDITOR_BASELINE_v1 §1.11](../../../../docs/MAP_EDITOR_BASELINE_v1.md)（P-4）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::CurrentLocale;
use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::{color32, proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window, BadgeKind, ICON_CHEVRON_RIGHT};

use wv_core::{EntityKind, SelectedEntity};

pub mod event;
pub mod faction;
pub mod place;
pub mod region;

/// Inspector 的页签。**会话态**——不落 `ui-layout.yaml`
/// （那是布局记忆，不是 UI 偏好）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    /// 「属性」：当前选中对象的属性表单。默认页。
    #[default]
    Properties,
    /// 「势力层」：势力层的增删改 + 势力列表。
    FactionLayers,
    /// 「事态」：事态列表。
    Situations,
}

impl InspectorTab {
    /// 页签下标（供 [`wv_ui::widgets::controls::tabs`] 用）。
    pub fn index(self) -> usize {
        match self {
            InspectorTab::Properties => 0,
            InspectorTab::FactionLayers => 1,
            InspectorTab::Situations => 2,
        }
    }

    /// 由下标反查页签（越界时回落到默认页）。
    pub fn from_index(i: usize) -> Self {
        match i {
            1 => InspectorTab::FactionLayers,
            2 => InspectorTab::Situations,
            _ => InspectorTab::Properties,
        }
    }
}

/// Inspector 的 UI 会话状态（当前页签）。
///
/// 与 [`PanelState`]（可见性 / 位置）同层——都是**会话态**，不落盘。
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct InspectorState {
    /// 当前页签。
    pub tab: InspectorTab,
}

/// 「属性」页签当前该渲染哪一型表单。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    /// 无选中 → 空态。
    None,
    /// 地点。
    Place,
    /// 命名区域。
    Region,
    /// 势力。
    Faction,
    /// 事件。
    Event,
    /// 地形格（单格或批量）。
    Cell,
    /// 街道段——**无稳定 id，点不到**（`wv-render/src/label_pick.rs:17`），
    /// 故没有表单；真到了这一支须显式提示而非静默空白。
    UnsupportedSegment,
}

/// 分派：当前该渲染哪一型表单。
///
/// **Cell 优先于实体选中态**——`EditCell` 工具激活时，作者的心智是
/// 「我在改格子」，不是「我在看一个地点」（设计稿 §11 Q4）。
pub fn resolve_form(selection: Option<&SelectedEntity>, cell_tool_active: bool) -> FormKind {
    if cell_tool_active {
        return FormKind::Cell;
    }
    let Some(sel) = selection else {
        return FormKind::None;
    };
    match sel.kind {
        EntityKind::Place => FormKind::Place,
        EntityKind::Region => FormKind::Region,
        EntityKind::Faction => FormKind::Faction,
        EntityKind::Event => FormKind::Event,
        EntityKind::StreetSegment => FormKind::UnsupportedSegment,
    }
}

/// 选中指向的实体是否**还在**世界里。
///
/// 作者删掉一个地点后，`selection` 仍可能指着它（选谁是会话态，不随撤销走）。
/// 表单渲染前必须过这道守卫——否则会拿着一个已不存在的实体去查表。
pub fn selection_alive(world: &wv_core::World, sel: &SelectedEntity) -> bool {
    match sel.kind {
        EntityKind::Place => world.distances.places.iter().any(|p| p.id == sel.id),
        EntityKind::Region => world.regions.iter().any(|r| r.id == sel.id),
        EntityKind::Faction => world.factions.iter().any(|f| f.id == sel.id),
        EntityKind::Event => world.events.iter().any(|e| e.id == sel.id),
        // 街道段无稳定 id，无法判定存活——恒 false。
        EntityKind::StreetSegment => false,
    }
}

/// Inspector 面板。
///
/// **参数按元组折叠**（Bevy 0.19 单系统上限 16 个参数）：本系统实际需要
/// 20+ 个资源，按职责折成嵌套元组——`SystemParam` 对元组递归实现，每层
/// 元组算 1 个参数，解构保留原名，函数体不受影响。
///
/// 仓内先例：`crate::ui_shell::apply_pending_commands`（19 参折成嵌套元组）。
#[allow(clippy::too_many_arguments)]
pub fn panel_inspector(
    mut contexts: EguiContexts,
    // `projection` 需可变：属性表单要提交 EditCommand（经 `EditHistory`），
    // 而命令的 `apply` 会写回 `projection.world`。
    (mut projection, mut edit): (
        ResMut<WorldProjection>,
        ResMut<crate::edit::editing::EditHistory>,
    ),
    (mut panels, mut inspector): (ResMut<PanelState>, ResMut<InspectorState>),
    (view, playback, grid): (
        Res<crate::state::ViewState>,
        Res<crate::state::PlaybackState>,
        Res<wv_render::state::GridConfig>,
    ),
    (mut edit_cell, mut display): (
        ResMut<wv_render::state::EditCellState>,
        ResMut<wv_render::faction_overlays::FactionDisplay>,
    ),
    (shell_cfg, layouts, current_loc): (Res<UiShellConfig>, Res<LayoutFile>, Res<CurrentLocale>),
    (mut tex, mut notice, time): (
        ResMut<wv_render::terrain_view::TerrainTexture>,
        ResMut<crate::ui_shell::ShellNotice>,
        Res<Time>,
    ),
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let selection_clone = projection.selection.clone();
    let selected = selection_clone.as_ref();

    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Inspector,
        None,
        current_loc.0,
        |ui| {
            // ── 页签条 ──────────────────────────────────────────────
            // 三页签共用**一个** egui 窗口（`float_window` 的 id 由
            // `Panel::Inspector` 决定），故切页签 = 换 body，不是开新窗口。
            let labels = ["属性", "势力层", "事态"];
            if let Some(idx) = crate::ui::widgets::tabs(
                ui,
                &shell,
                "inspector_tabs",
                &labels,
                inspector.tab.index(),
                &[],
            ) {
                inspector.tab = InspectorTab::from_index(idx);
            }
            ui.add_space(space::SM);

            // ── 页签内容 ────────────────────────────────────────────
            match inspector.tab {
                InspectorTab::Properties => {
                    render_properties(
                        ui,
                        &shell,
                        selected,
                        &mut projection,
                        &mut edit,
                        &view,
                        &playback,
                        &grid,
                        &mut edit_cell,
                        &mut display,
                        &mut tex,
                        &mut notice,
                        &time,
                        current_loc.0,
                    );
                }
                InspectorTab::FactionLayers => {
                    ui.label("势力层（M4 接入）");
                }
                InspectorTab::Situations => {
                    ui.label("事态（M4 接入）");
                }
            }
        },
    );
}

/// 「属性」页签：按选中对象分派。
///
/// 各类型的真表单在 M2 逐个落地（`place_form` / `region_form` / …）。
/// 本函数只负责**分派**与**存活守卫**——守卫不可省：选中的实体可能已被
/// 作者删掉（选谁是会话态，不随撤销走），拿着不存在的实体查表会拿到空值，
/// 渲染出空白表单而不报错——那是「点了没反应」的静默失败。
#[allow(clippy::too_many_arguments)]
fn render_properties(
    ui: &mut egui::Ui,
    shell: &UiShell,
    selected: Option<&SelectedEntity>,
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    view: &crate::state::ViewState,
    playback: &crate::state::PlaybackState,
    grid: &wv_render::state::GridConfig,
    _edit_cell: &mut wv_render::state::EditCellState,
    display: &mut wv_render::faction_overlays::FactionDisplay,
    tex: &mut wv_render::terrain_view::TerrainTexture,
    notice: &mut crate::ui_shell::ShellNotice,
    time: &Time,
    current_loc: crate::i18n::Locale,
) {
    let form = resolve_form(selected, false);
    match form {
        FormKind::None => {
            // 空态：空态卡 + 四条计数行 + 事态卡（现状不变）。
            render_empty(ui, shell);
            render_count_rows(ui, shell, projection);
            situation_cards(ui, shell, projection);
        }
        FormKind::UnsupportedSegment => {
            ui.label("街道段暂无可编辑属性");
        }
        FormKind::Cell => {
            ui.label("Cell 表单（M2 接入）");
        }
        FormKind::Place => {
            // 存活守卫。
            let Some(sel) = selected else {
                render_empty(ui, shell);
                return;
            };
            if !selection_alive(&projection.world, sel) {
                render_empty(ui, shell);
                return;
            }
            render_selection_header(ui, shell, sel);
            // 取出该地点的**克隆**再渲染——表单要可变引用（写命令），
            // 而 `projection.world` 的借用不能与写冲突。克隆是必要的代价：
            // Place 是小结构体（几个 Option），克隆成本可忽略。
            let Some(place) = projection
                .world
                .distances
                .places
                .iter()
                .find(|p| p.id == sel.id)
                .cloned()
            else {
                render_empty(ui, shell);
                return;
            };
            let has_territory = place::place_has_territory(&projection.world, &place.id);
            place::place_form(
                ui,
                shell,
                &place,
                has_territory,
                projection,
                edit,
                view,
                tex,
                notice,
                time,
                playback,
            );
        }
        FormKind::Region => {
            // 存活守卫。
            let Some(sel) = selected else {
                render_empty(ui, shell);
                return;
            };
            if !selection_alive(&projection.world, sel) {
                render_empty(ui, shell);
                return;
            }
            render_selection_header(ui, shell, sel);
            let Some(region) = projection
                .world
                .regions
                .iter()
                .find(|r| r.id == sel.id)
                .cloned()
            else {
                render_empty(ui, shell);
                return;
            };
            // 主题支持的覆盖物键（词表由主题拥有，ADR-0045 D10）。
            let overlay_keys: Vec<&'static str> =
                view.active_theme.region_overlays.keys().collect();
            // 两套原点的差（区域多边形 = 图内局部 wu；格子属于栅格所在的图）。
            let raster_to_map = (
                grid.map_origin.x - grid.raster_origin.x,
                grid.map_origin.y - grid.raster_origin.y,
            );
            region::region_form(
                ui,
                shell,
                &region,
                view,
                projection,
                edit,
                &CurrentLocale(current_loc),
                &overlay_keys,
                grid.cell_size,
                raster_to_map,
                notice,
                time.elapsed_secs() as u64,
            );
        }
        FormKind::Faction => {
            // 存活守卫。
            let Some(sel) = selected else {
                render_empty(ui, shell);
                return;
            };
            if !selection_alive(&projection.world, sel) {
                render_empty(ui, shell);
                return;
            }
            render_selection_header(ui, shell, sel);
            let Some(faction) = projection
                .world
                .factions
                .iter()
                .find(|f| f.id == sel.id)
                .cloned()
            else {
                render_empty(ui, shell);
                return;
            };
            // 势力层词表（供「所属层」下拉用）。
            let layers = wv_core::resolved_faction_layers(&projection.world);
            let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
            faction::faction_form(
                ui, shell, &faction, &layers, view, playback, &map_id, projection, edit, display,
            );
        }
        FormKind::Event => {
            // 存活守卫。
            let Some(sel) = selected else {
                render_empty(ui, shell);
                return;
            };
            if !selection_alive(&projection.world, sel) {
                render_empty(ui, shell);
                return;
            }
            render_selection_header(ui, shell, sel);
            let Some(event) = projection
                .world
                .events
                .iter()
                .find(|e| e.id == sel.id)
                .cloned()
            else {
                render_empty(ui, shell);
                return;
            };
            event::event_form(ui, shell, &event, projection, edit, view);
        }
    }
}

/// web `.empty-inspector`：虚线圆圈 + `No object selected` + 提示。
fn render_empty(ui: &mut egui::Ui, shell: &UiShell) {
    let _ = empty_state(
        ui,
        shell,
        "◎",
        "No object selected",
        "Click a map plot, event, or character\nto inspect properties.",
        "",
    );
}

/// 选中摘要头：类型色点 + `kind · id`。
fn render_selection_header(ui: &mut egui::Ui, shell: &UiShell, sel: &SelectedEntity) {
    ui.horizontal(|ui| {
        let dot_color = match sel.kind {
            EntityKind::Place => shell.accent(),
            EntityKind::Region => shell.gold(),
            EntityKind::Faction => shell.danger(),
            EntityKind::Event => shell.success(),
            EntityKind::StreetSegment => shell.t3(),
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, dot_color);
        ui.label(
            egui::RichText::new(format!("{} · {}", kind_label(sel.kind), sel.id.as_str()))
                .size(shell.scale.small)
                .color(shell.t1())
                .strong(),
        );
    });
    ui.add_space(space::SM);
}

/// 四条计数行（Situations / Characters / Events / Regions）——空态下的概览。
fn render_count_rows(ui: &mut egui::Ui, shell: &UiShell, projection: &WorldProjection) {
    compact_row(
        ui,
        shell,
        "Situations",
        (
            BadgeKind::Warn,
            format!("{} active", projection.world.situations.len()),
        ),
    );
    compact_row(
        ui,
        shell,
        "Characters",
        (
            BadgeKind::Neutral,
            format!("{}", projection.world.characters.len()),
        ),
    );
    compact_row(
        ui,
        shell,
        "Events",
        (
            BadgeKind::Event,
            format!("{}", projection.world.events.len()),
        ),
    );
    compact_row(
        ui,
        shell,
        "Regions",
        (
            BadgeKind::Neutral,
            format!("{}", projection.world.distances.places.len()),
        ),
    );
}

/// web `.compact-section`：高 29、raised 底、圆角 5：名称 + 计数徽标 + chevron。
fn compact_row(ui: &mut egui::Ui, shell: &UiShell, label: &str, count: (BadgeKind, String)) {
    let row_h = 29.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, 5.0, shell.raised_bg());
    // 名称（web `.compact-section > span:first-child`：600 weight）
    painter.text(
        egui::pos2(rect.min.x + 7.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.base),
        shell.t1(),
    );
    // 徽标与 chevron 的几何先算好（painter 借用随作用域结束，再 ui.put 图标）
    let (kind, text) = count;
    let bc = shell.palette.badge(match kind {
        BadgeKind::Neutral => crate::theme::ui_tokens::BadgeTone::Neutral,
        BadgeKind::Warn => crate::theme::ui_tokens::BadgeTone::Warn,
        BadgeKind::Event => crate::theme::ui_tokens::BadgeTone::Event,
        BadgeKind::Ok => crate::theme::ui_tokens::BadgeTone::Ok,
        BadgeKind::Err => crate::theme::ui_tokens::BadgeTone::Err,
    });
    let pill_w = 30.0 + text.chars().count() as f32 * 6.0;
    let pill = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 25.0 - pill_w * 0.5, rect.center().y),
        egui::vec2(pill_w, 16.0),
    );
    painter.rect_filled(
        pill,
        radius::BADGE,
        color32(bc.background_on(shell.palette.bg_panel)),
    );
    painter.text(
        pill.center(),
        egui::Align2::CENTER_CENTER,
        text,
        proportional(shell.scale.micro),
        color32(bc.fg),
    );
    // chevron（最右，t3）—— painter 借用已随上文最后使用结束，可安全 ui.put
    let chevron_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 13.0, rect.center().y),
        egui::vec2(12.0, 12.0),
    );
    let _ = ui.put(
        chevron_rect,
        egui::Image::new(ICON_CHEVRON_RIGHT)
            .tint(shell.t3())
            .fit_to_exact_size(egui::vec2(12.0, 12.0)),
    );
}

/// Situations 列表卡（web `.list-card`：3px 色条 + 名称 + 跨度 meta）。
/// 最多 3 张（计数行已给总数，卡是"最近关注"预览）。
fn situation_cards(ui: &mut egui::Ui, shell: &UiShell, projection: &WorldProjection) {
    for s in projection.world.situations.iter().take(3) {
        let card_h = 42.0;
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), card_h),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        // 卡底：raised 60% + border 描边（web `color-mix(raised 60%)`）
        painter.rect_filled(rect, 6.0, shell.raised_bg().linear_multiply(0.6));
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, shell.border_subtle()),
            egui::StrokeKind::Inside,
        );
        // 3px 事件色条（web `.color-bar`：stretch、圆角 4）
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 6.0, rect.min.y + 6.0),
            egui::pos2(rect.min.x + 9.0, rect.max.y - 6.0),
        );
        painter.rect_filled(bar, 4.0, shell.danger());
        // 名称 + meta
        painter.text(
            egui::pos2(rect.min.x + 17.0, rect.center().y - 7.0),
            egui::Align2::LEFT_CENTER,
            &s.name,
            proportional(shell.scale.micro),
            shell.t1(),
        );
        painter.text(
            egui::pos2(rect.min.x + 17.0, rect.center().y + 7.0),
            egui::Align2::LEFT_CENTER,
            format_span(s.span.from, s.span.to),
            proportional(shell.scale.micro),
            shell.t3(),
        );
    }
}

/// `Y214 D40 — Y215 D12` / `Y214 D82 — …`（360 天/年简化口径，M3 接历法）。
fn format_span(from: i64, to: Option<i64>) -> String {
    let (fy, fd) = year_day(from);
    match to {
        Some(t) => {
            let (ty, td) = year_day(t);
            format!("Y{fy} D{fd} — Y{ty} D{td}")
        }
        None => format!("Y{fy} D{fd} — …"),
    }
}

fn year_day(day: i64) -> (i32, i32) {
    (((day - 1) / 360 + 1) as i32, ((day - 1) % 360 + 1) as i32)
}

fn kind_label(k: EntityKind) -> &'static str {
    match k {
        EntityKind::Place => "Place",
        EntityKind::Region => "Region",
        EntityKind::Faction => "Faction",
        EntityKind::StreetSegment => "Segment",
        EntityKind::Event => "Event",
    }
}

// ── 测试 ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::Id;

    fn sel(kind: EntityKind, id: &str) -> SelectedEntity {
        SelectedEntity::new(kind, Id::new_unchecked(id))
    }

    #[test]
    fn resolve_form_prefers_cell_tool_over_entity_selection() {
        // 工具激活时，即使选中了 Place，也应出 Cell 表单——工具即模式。
        let s = sel(EntityKind::Place, "p1");
        assert_eq!(resolve_form(Some(&s), true), FormKind::Cell);
    }

    #[test]
    fn resolve_form_returns_cell_when_tool_active_without_selection() {
        assert_eq!(resolve_form(None, true), FormKind::Cell);
    }

    #[test]
    fn resolve_form_dispatches_by_kind() {
        assert_eq!(resolve_form(None, false), FormKind::None);
        assert_eq!(
            resolve_form(Some(&sel(EntityKind::Place, "p1")), false),
            FormKind::Place
        );
        assert_eq!(
            resolve_form(Some(&sel(EntityKind::Region, "r1")), false),
            FormKind::Region
        );
        assert_eq!(
            resolve_form(Some(&sel(EntityKind::Faction, "f1")), false),
            FormKind::Faction
        );
        assert_eq!(
            resolve_form(Some(&sel(EntityKind::Event, "e1")), false),
            FormKind::Event
        );
    }

    #[test]
    fn resolve_form_marks_street_segment_unsupported() {
        // 街道段无稳定 id，点不到——但若真到了这一支，须显式提示而非静默空白。
        assert_eq!(
            resolve_form(Some(&sel(EntityKind::StreetSegment, "s1")), false),
            FormKind::UnsupportedSegment
        );
    }

    #[test]
    fn selection_alive_true_for_existing_entities() {
        let mut w = wv_core::World::default();
        w.distances.places.push(wv_core::distance::Place {
            line: None,
            id: Id::new_unchecked("p1"),
            name: "P".into(),
            feature: None,
            biome: None,
            envelope: None,
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
            rank: None,
        });
        w.regions.push(wv_core::Region {
            id: Id::new_unchecked("r1"),
            name: "R".into(),
            map: Id::new_unchecked("m1"),
            polygon: vec![],
            holes: vec![],
            kind: wv_core::model::region::RegionKind::Area,
            parent: None,
            overlay: None,
        });
        assert!(selection_alive(&w, &sel(EntityKind::Place, "p1")));
        assert!(selection_alive(&w, &sel(EntityKind::Region, "r1")));
    }

    #[test]
    fn selection_alive_false_for_missing_entity() {
        let w = wv_core::World::default();
        assert!(!selection_alive(&w, &sel(EntityKind::Place, "ghost")));
        assert!(!selection_alive(&w, &sel(EntityKind::Region, "ghost")));
        assert!(!selection_alive(&w, &sel(EntityKind::Faction, "ghost")));
        assert!(!selection_alive(&w, &sel(EntityKind::Event, "ghost")));
    }

    #[test]
    fn selection_alive_false_for_street_segment() {
        // 街道段无稳定 id，无法判定存活——恒 false。
        let w = wv_core::World::default();
        assert!(!selection_alive(&w, &sel(EntityKind::StreetSegment, "s1")));
    }

    #[test]
    fn inspector_tab_index_roundtrips() {
        for tab in [
            InspectorTab::Properties,
            InspectorTab::FactionLayers,
            InspectorTab::Situations,
        ] {
            assert_eq!(InspectorTab::from_index(tab.index()), tab);
        }
    }

    #[test]
    fn inspector_tab_from_out_of_range_index_falls_back_to_properties() {
        assert_eq!(InspectorTab::from_index(9), InspectorTab::Properties);
    }

    #[test]
    fn kind_label_maps_all_five_kinds() {
        assert_eq!(kind_label(EntityKind::Place), "Place");
        assert_eq!(kind_label(EntityKind::Region), "Region");
        assert_eq!(kind_label(EntityKind::Faction), "Faction");
        assert_eq!(kind_label(EntityKind::Event), "Event");
        assert_eq!(kind_label(EntityKind::StreetSegment), "Segment");
    }

    #[test]
    fn year_day_matches_timeline_360_day_year() {
        // 第 1 天 → Y1 D1
        assert_eq!(year_day(1), (1, 1));
        // 第 360 天 → Y1 D360
        assert_eq!(year_day(360), (1, 360));
        // 第 361 天 → Y2 D1
        assert_eq!(year_day(361), (2, 1));
    }

    #[test]
    fn format_span_renders_both_bounds_or_open_end() {
        assert_eq!(format_span(14441, Some(14445)), "Y41 D41 — Y41 D45");
        assert_eq!(format_span(14441, None), "Y41 D41 — …");
    }
}
