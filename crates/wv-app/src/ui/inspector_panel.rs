//! 检视器面板（v0.3 Figma `5E8BCBCmtGAHqk3ORibBlU` 风格）。
//!
//! ## 职责
//! - `panel_inspector` —— 顶栏 `[Inspector · ⚙]` + 4 段折叠
//!   `Situations / Characters / Events / Regions`；未选中对象时显示空态卡。
//!
//! ## v0.3 变更
//! - 位置整体右移到右上角（`Panel::Inspector.default_pos = [992, 56]`，
//!   见 `state.rs`）；与 Character 错开避免矩形重叠。
//! - 选中态：4 段折叠（Figma 风格），每段列**该类型在当前 `WorldProjection`
//!   内的所有实例** + 高亮选中那条。
//! - 未选中态：v0.3 改用 `◎` 大图标 + `No object` + 提示语 + 命令面板入口
//!   提示（M2 仍然用 `empty_state`，文案按 Figma 调）。
//!
//! ## 边界
//! - **不**改世界数据 —— 只读 `WorldProjection.selection`。
//! - **不**重新画 `Selection` 系统 —— 现有 `SelectedEntity`（ADR-0017 §2.7）
//!   即真源，本面板只消费。
//!
//! ## 主要导出
//! - `panel_inspector` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::{empty_state, float_window};

/// 检视器面板 v0.3 —— 顶栏 + 4 段折叠 + 未选态。
#[allow(clippy::too_many_arguments)]
pub fn panel_inspector(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let selected = projection.selection.as_ref();
    let has_selection = selected.is_some();

    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Inspector,
        None,
        |ui| {
            if !has_selection {
                render_empty(ui, &shell);
                return;
            }
            render_selected(ui, &shell, &projection);
        },
    );
}

/// Figma v0.3 未选态：`◎ No object` + 提示 + 命令面板入口。
fn render_empty(ui: &mut egui::Ui, shell: &UiShell) {
    let _ = empty_state(
        ui,
        shell,
        "◎",
        "No object",
        "Click a map plot, event or character\nto inspect properties here",
        "Use ⌘K to find →",
    );
}

/// Figma v0.3 选中态：4 段折叠（Situations / Characters / Events / Regions）。
///
/// 每段列出**该类型**在 `WorldProjection.world` 内的全部实例；高亮当前
/// `selection` 那条。子项数据真实可达（M2 直接读真实 `World`），无需
/// 走 mock 通路——这是相对 M1 timeline 事件的进步。
fn render_selected(ui: &mut egui::Ui, shell: &UiShell, projection: &WorldProjection) {
    let Some(sel) = projection.selection.as_ref() else {
        return;
    };
    // 头部：`Inspector · ⚙` + 当前选中摘要
    header(ui, shell, &sel.kind, &sel.id);

    // 4 段折叠（与 Figma 一致）。
    section_situations(ui, shell, projection, sel);
    section_characters(ui, shell, projection, sel);
    section_events(ui, shell, projection, sel);
    section_regions(ui, shell, projection, sel);
}

fn header(ui: &mut egui::Ui, shell: &UiShell, kind: &wv_core::EntityKind, id: &wv_core::Id) {
    ui.horizontal(|ui| {
        // 头部装饰圆点（按 kind 着色）
        let dot_color = match kind {
            wv_core::EntityKind::Place => shell.accent(),
            wv_core::EntityKind::Region => shell.gold(),
            wv_core::EntityKind::Faction => shell.danger(),
            wv_core::EntityKind::Event => shell.success(),
            wv_core::EntityKind::StreetSegment => shell.t3(),
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 5.0, dot_color);

        ui.label(
            egui::RichText::new(format!("{} · {}", kind_label(*kind), id.as_str()))
                .size(shell.scale.small)
                .color(shell.t1())
                .strong(),
        );
    });
    ui.add_space(space::SM);
    ui.separator();
    ui.add_space(space::SM);
}

fn kind_label(k: wv_core::EntityKind) -> &'static str {
    match k {
        wv_core::EntityKind::Place => "Place",
        wv_core::EntityKind::Region => "Region",
        wv_core::EntityKind::Faction => "Faction",
        wv_core::EntityKind::StreetSegment => "Segment",
        wv_core::EntityKind::Event => "Event",
    }
}

fn section_situations(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    sel: &wv_core::SelectedEntity,
) {
    let header = format!("Situations · {}", projection.world.situations.len());
    egui::CollapsingHeader::new(header)
        .default_open(true)
        .show(ui, |ui| {
            if projection.world.situations.is_empty() {
                empty_section(ui, shell, "No situations yet");
                return;
            }
            for s in &projection.world.situations {
                let is_sel =
                    sel.kind == wv_core::EntityKind::Region && sel.id.as_str() == s.id.as_str();
                list_item_row(
                    ui,
                    shell,
                    shell.danger(),
                    &s.name,
                    &format!("Y{} D{} → …", span_year(s.span.from), span_day(s.span.from)),
                    is_sel,
                );
            }
        });
}

fn section_characters(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    sel: &wv_core::SelectedEntity,
) {
    let header = format!("Characters · {}", projection.world.characters.len());
    egui::CollapsingHeader::new(header)
        .default_open(false)
        .show(ui, |ui| {
            if projection.world.characters.is_empty() {
                empty_section(ui, shell, "No characters yet");
                return;
            }
            for c in &projection.world.characters {
                let is_sel =
                    sel.kind == wv_core::EntityKind::Place && sel.id.as_str() == c.id.as_str();
                list_item_row(
                    ui,
                    shell,
                    shell.accent(),
                    &c.name,
                    "alive · this map",
                    is_sel,
                );
            }
        });
}

fn section_events(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    sel: &wv_core::SelectedEntity,
) {
    let header = format!("Events · {}", projection.world.events.len());
    egui::CollapsingHeader::new(header)
        .default_open(false)
        .show(ui, |ui| {
            if projection.world.events.is_empty() {
                empty_section(ui, shell, "No events yet");
                return;
            }
            for e in &projection.world.events {
                let is_sel =
                    sel.kind == wv_core::EntityKind::Event && sel.id.as_str() == e.id.as_str();
                list_item_row(ui, shell, shell.gold(), &e.name, "click to focus", is_sel);
            }
        });
}

fn section_regions(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    sel: &wv_core::SelectedEntity,
) {
    // Regions 段用 `distances.places` 列表占位（v0.3 暂无独立 Region 集合；
    // M3 接 ADR-0017 §2.2 命名区域后改）。
    let places = &projection.world.distances.places;
    let header = format!("Regions · {}", places.len());
    egui::CollapsingHeader::new(header)
        .default_open(false)
        .show(ui, |ui| {
            if places.is_empty() {
                empty_section(ui, shell, "No regions yet");
                return;
            }
            for p in places {
                let is_sel =
                    sel.kind == wv_core::EntityKind::Region && sel.id.as_str() == p.id.as_str();
                list_item_row(ui, shell, shell.gold(), &p.name, "polygon", is_sel);
            }
        });
}

fn list_item_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    swatch: egui::Color32,
    title: &str,
    meta: &str,
    selected: bool,
) {
    let row_h = 26.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, radius::CONTROL, shell.hover_bg());
        p.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 2.0, rect.max.y)),
            0.0,
            shell.accent(),
        );
    }
    // swatch
    let sw = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + space::SM, rect.center().y - 4.0),
        egui::vec2(8.0, 8.0),
    );
    p.rect_filled(sw, radius::BADGE, swatch);
    p.text(
        egui::pos2(rect.min.x + space::SM + 8.0 + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        proportional(shell.scale.small),
        if selected { shell.t1() } else { shell.t2() },
    );
    p.text(
        egui::pos2(rect.max.x - space::SM, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        meta,
        proportional(shell.scale.micro),
        shell.t3(),
    );
}

fn empty_section(ui: &mut egui::Ui, shell: &UiShell, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(shell.scale.micro)
            .color(shell.t3()),
    );
}

fn span_year(day: i64) -> i32 {
    ((day - 1) / 360 + 1) as i32
}

fn span_day(day: i64) -> i32 {
    ((day - 1) % 360 + 1) as i32
}

// ── 测试 ────────────────────────────────────────────────────────────
//
// `panel_inspector` 是 Bevy 系统 + egui 渲染，集成测试要起 App 太重；
// 单元测试只覆盖**纯函数**（`kind_label` / `span_year` / `span_day`），
// 集成覆盖由 smoke-gui + 人工验收承担。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_label_maps_all_five_kinds() {
        assert_eq!(kind_label(wv_core::EntityKind::Place), "Place");
        assert_eq!(kind_label(wv_core::EntityKind::Region), "Region");
        assert_eq!(kind_label(wv_core::EntityKind::Faction), "Faction");
        assert_eq!(kind_label(wv_core::EntityKind::Event), "Event");
        assert_eq!(kind_label(wv_core::EntityKind::StreetSegment), "Segment");
    }

    #[test]
    fn span_year_and_day_match_timeline_season_of_day() {
        // 第 1 天 → Y1 D1
        assert_eq!(span_year(1), 1);
        assert_eq!(span_day(1), 1);
        // 第 360 天 → Y1 D360
        assert_eq!(span_year(360), 1);
        assert_eq!(span_day(360), 360);
        // 第 361 天 → Y2 D1
        assert_eq!(span_year(361), 2);
        assert_eq!(span_day(361), 1);
    }

    #[test]
    fn header_label_includes_kind_and_id() {
        let sel = wv_core::SelectedEntity::new(
            wv_core::EntityKind::Place,
            wv_core::id::Id::new_unchecked("city_zunyi"),
        );
        assert_eq!(sel.kind, wv_core::EntityKind::Place);
        assert_eq!(sel.id.as_str(), "city_zunyi");
        assert_eq!(
            format!("{} · {}", kind_label(sel.kind), sel.id.as_str()),
            "Place · city_zunyi"
        );
    }
}
