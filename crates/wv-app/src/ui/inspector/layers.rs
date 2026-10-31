//! 「势力层」页签（统一 Inspector 的第二个页签）。
//!
//! 从旧 `faction_panel` 整体迁入（2026-10-22 统一 Inspector 设计）：
//! 层头管理（名称 / z / 配色策略 / 可见 / 删除）+「当前填色层」选择器 +
//! 势力列表（点行 = 选中 →「属性」页签）。旧面板随之退役。
//!
//! ## 与「属性」页签的分工
//!
//! - 本页签 = **集合层**：有哪几个层、每层有哪几个势力、当前填色层。
//! - 「属性」页签 = **单体层**：选中势力的名称 / 颜色 / 归属边 / 几何。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::edit::editing;
use crate::edit::faction_panel_actions;
use crate::state::{ViewState, WorldProjection};
use crate::theme::ui_tokens::space;
use crate::ui::shell::UiShell;
use crate::ui::widgets::icon_image;
use crate::ui::ICON_PENCIL;

use crate::ui::faction_panel::{faction_is_selected, faction_selection};
use crate::ui::widgets::{empty_state, list_item};
use wv_render::faction_overlays::FactionDisplay;

/// 「势力层」页签内容。
#[allow(clippy::too_many_arguments)]
pub fn layers_tab(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    display: &mut FactionDisplay,
    pen_dialog: &mut crate::ui::partition_pen::PartitionPenDialog,
    playback_day: i64,
    loc: crate::i18n::Locale,
    faction_create: &mut crate::ui::entity_create_dialog::FactionCreateDialog,
) {
    let layers = wv_core::resolved_faction_layers(&projection.world);
    let pen_gated = crate::ui::partition_pen::pen_available_for(
        &projection.world,
        &wv_core::Id::new_unchecked(view.current_map.clone()),
    );
    let mut factions: Vec<wv_core::Faction> = projection.world.factions.clone();
    factions.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    // finding-7：顶行「⋯」按钮（TODO M2 未实现）撤下——死控件不再渲染。
    // 保留图级标签（作者需要知道本页签看的是哪张图）。
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{}: {}",
                crate::i18n::t(loc, crate::i18n::I18nKey::FactionTabMap),
                view.current_map
            ))
            .size(shell.scale.small)
            .color(shell.t3()),
        );
    });
    ui.add_space(space::MD);

    // 新建层：政治层（切蛋糕=势力归属）+ 自然层（ADR-0081，切蛋糕=群系族）。
    let add_w = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(egui::vec2(add_w, shell.hit), egui::Sense::click());
    ui.painter().rect_stroke(
        rect,
        crate::theme::ui_tokens::radius::CONTROL,
        egui::Stroke::new(1.0, shell.border_strong()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        crate::i18n::t(loc, crate::i18n::I18nKey::FactionLayerAddPolitical),
        crate::theme::ui_theme::proportional(shell.scale.base),
        shell.t2(),
    );
    if r.clicked() {
        faction_panel_actions::add_layer_now(
            projection,
            edit,
            view,
            "新势力层".into(),
            layers.len() as i32,
            wv_core::model::faction::LayerUsage::Political,
        );
    }
    // 自然层（ADR-0081/0082）：**全工程唯一**——建过即灰（用户裁定
    // 2026-10-28）。编辑入口在 CellMap（笔刷窗自然区段）；本列表的自然层
    // 行只留眼睛（Map 只看不管，见下方层循环）。
    let nature_exists = layers
        .iter()
        .any(|l| l.usage == wv_core::model::faction::LayerUsage::Nature);
    let (rect_n, r_n) = ui.allocate_exact_size(egui::vec2(add_w, shell.hit), egui::Sense::click());
    ui.painter().rect_stroke(
        rect_n,
        crate::theme::ui_tokens::radius::CONTROL,
        egui::Stroke::new(
            1.0,
            if nature_exists {
                shell.border_subtle()
            } else {
                shell.border_strong()
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect_n.center(),
        egui::Align2::CENTER_CENTER,
        if nature_exists {
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionLayerNatureExists)
        } else {
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionLayerAddNature)
        },
        crate::theme::ui_theme::proportional(shell.scale.base),
        shell.t3(),
    );
    if r_n.clicked() && !nature_exists {
        faction_panel_actions::add_layer_now(
            projection,
            edit,
            view,
            "自然层".into(),
            layers.len() as i32,
            wv_core::model::faction::LayerUsage::Nature,
        );
    }
    if nature_exists {
        r_n.on_hover_text(crate::i18n::t(
            loc,
            crate::i18n::I18nKey::FactionLayerNatureHover,
        ));
    }
    ui.add_space(space::MD);

    // 新建势力本体（UX-002 收尾）：空态文案承诺的「+ 新建势力」自此存在。
    // 恒渲染——势力与层是两个动作：层是画布的分组，本体是政治实体，
    // 有层没层都该能先登记本体（无层时落默认层，登记后可改归属）。
    let (rect_f, r_f) = ui.allocate_exact_size(egui::vec2(add_w, shell.hit), egui::Sense::click());
    ui.painter().rect_stroke(
        rect_f,
        crate::theme::ui_tokens::radius::CONTROL,
        egui::Stroke::new(1.0, shell.border_strong()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect_f.center(),
        egui::Align2::CENTER_CENTER,
        crate::i18n::t(loc, crate::i18n::I18nKey::FactionCreateNew),
        crate::theme::ui_theme::proportional(shell.scale.base),
        shell.t2(),
    );
    if r_f.clicked() {
        faction_create.open = true;
    }
    ui.add_space(space::MD);

    // 当前填色层选择器（势力图一次只填一个层）。
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::t(
                loc,
                crate::i18n::I18nKey::FactionTabActiveLayer,
            ))
            .small()
            .color(shell.t2()),
        );
        if ui
            .selectable_label(
                display.active_layer.is_none(),
                crate::i18n::t(loc, crate::i18n::I18nKey::FactionTabAuto),
            )
            .clicked()
        {
            display.active_layer = None;
        }
        let selected = display.active_layer.clone();
        for l in &layers {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(selected.as_ref() == Some(&l.id), l.name.as_str())
                    .clicked()
                {
                    display.active_layer = Some(l.id.clone());
                }
                // pen 按钮（ADR-0079 D3/T11）：动刀入口。门控 = 该图有自有
                // 栅格（图级 World/Region 由 Inspector 语境保证）。
                // ⚠️ 不用 with_layout(right_to_left) 右对齐——收缩 Ui 里它把
                // 行 min_rect 撑到整屏宽，Inspector 窗被顶成全屏宽（实测）。
                // ⚠️ 图标用 Lucide pencil.svg——"✎" 字符在打包字体无字形，
                // 落豆腐块（AGENTS §8 图标纪律）。
                // ADR-0082：**自然层不出 pen**——Map 只看不管，编辑在
                // CellMap 笔刷窗自然区段（用户裁定 2026-10-28）。
                // UX-006：Influence 声明层锁定 → 动刀入口灰化 + 悬停说明
                //（锁=防误写；前置披露挂入口端）。
                // finding-6：无自有栅格的图（City/Interior）铅笔**照渲染但
                // 禁用** + 悬停解释——入口整颗消失是无反馈的死端。
                if l.usage != wv_core::model::faction::LayerUsage::Nature {
                    let influence_locked = crate::edit::editing::layer_locked(
                        &projection.world,
                        &wv_core::Id::new_unchecked(view.current_map.clone()),
                        wv_core::map::LayerKind::Influence,
                    );
                    let pen_enabled = pen_gated && !influence_locked;
                    let hover = if !pen_gated {
                        crate::i18n::t(loc, crate::i18n::I18nKey::PartitionPenNoRaster).to_owned()
                    } else if influence_locked {
                        crate::i18n::t(loc, crate::i18n::I18nKey::LockedLayerWriteBlocked)
                            .to_owned()
                    } else {
                        crate::i18n::t(loc, crate::i18n::I18nKey::PartitionPenButtonHover)
                            .to_owned()
                    };
                    let pen = ui.add_enabled(
                        pen_enabled,
                        egui::Button::image(icon_image(ICON_PENCIL, egui::Color32::WHITE, 12.0))
                            .fill(egui::Color32::TRANSPARENT),
                    );
                    if pen.on_hover_text(hover).clicked() {
                        pen_dialog.open(l.id.clone(), playback_day);
                    }
                }
            });
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut display.show_other_borders,
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionTabOtherBorders),
        );
        ui.checkbox(
            &mut display.show_claims,
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionTabClaims),
        );
    });
    ui.add_space(space::SM);

    if factions.is_empty() && layers.is_empty() {
        // finding-7/9：空态 CTA 接线（旧文案「查看势力层说明 →」点击无动作、
        // hint 指路 YAML）——现在按钮直接开「新建势力」对话框。
        let cta = empty_state(
            ui,
            shell,
            "◆",
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionEmptyTitle),
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionEmptyHint),
            crate::i18n::t(loc, crate::i18n::I18nKey::FactionCreateNew),
        );
        if cta {
            faction_create.open = true;
        }
        return;
    }

    for layer in &layers {
        let layer_id = layer.id.clone();
        let in_layer: Vec<wv_core::Faction> = factions
            .iter()
            .filter(|f| wv_core::resolve_faction_layer_id(&projection.world, f) == layer_id)
            .cloned()
            .collect();

        let mut deleted = false;
        egui::CollapsingHeader::new(format!(
            "▤ {} ({}) · {}",
            layer.name,
            layer.id,
            in_layer.len()
        ))
        .id_salt(format!("faction_layer_{}", layer.id))
        .default_open(true)
        .show(ui, |ui| {
            deleted = crate::ui::faction_panel::layer_controls(
                ui, shell, loc, layer, projection, edit, view,
            );
            if in_layer.is_empty() {
                ui.label(
                    egui::RichText::new(crate::i18n::t(
                        loc,
                        crate::i18n::I18nKey::FactionLayerNoFactions,
                    ))
                    .italics()
                    .small()
                    .color(shell.t2()),
                );
            }
            // 列表项：点击 = 选中（驱动 Inspector 的「属性」页签）。
            // 2026-10-22 统一 Inspector：势力的属性编辑已迁入
            // Inspector，本面板只当选择器 + 层管理。
            for f in &in_layer {
                let is_selected = faction_is_selected(projection.selection.as_ref(), &f.id);
                let resp = list_item(
                    ui,
                    shell,
                    shell.danger(),
                    &f.name,
                    f.id.as_str(),
                    is_selected,
                );
                if resp.clicked() {
                    projection.selection = Some(faction_selection(&f.id));
                    // 切回「属性」页签（同 region/faction 列表）。
                }
                ui.add_space(space::XS);
            }
        });
        if deleted && display.active_layer.as_ref() == Some(&layer.id) {
            display.active_layer = None;
        }
        ui.add_space(space::XS);
    }
}
