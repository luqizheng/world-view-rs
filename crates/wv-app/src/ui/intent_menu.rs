//! 选区意图菜单（2026-10-17 选择工具 spec §4；M1 Task 6）。
//!
//! ## 职责
//! - [`intent_menu`] —— 选区闭合时在画布**底部居中**浮现的浮动条：
//!   列出当前图级可用的生成意图（M1 仅「生成大陆」入口，八参面板照旧）。
//! - [`intent_count_for`] —— 意图目录按图级过滤的谓词（spec §4 裁定：
//!   有地形栅格 ∧ 图级 ∈ {World, Region}；City / Interior 不出意图）。
//!
//! ## 边界（不做什么）
//! - **不碰 `History` / 不构造 `EditCommand`**——只置 `ContinentDialog::open`
//!   与取消选区；生成经既有 `continent_dialog_system` 提交（AGENTS §3.3）。
//! - **不画选区几何**——那是 `wv_render::selection::draw_selection`。
//! - **不隐藏工具**——无意图的图级显示占位文案而非隐藏（spec §12：选择优先
//!   模型要全图级一致）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::{DrillState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::city_intent::{prefill, CityIntentDialog};
use crate::ui_shell::StartupLoadRequest;

use super::shell::UiShell;
use super::widgets::icon_image;
use super::ICON_MOUNTAIN_RANGE;
use crate::theme::ui_tokens::radius;

/// 单瓷砖边长与图标内嵌（视觉对齐 [`super::map_controls`] 的瓷砖规格）。
const TILE_SIZE: f32 = 34.0;
const ICON_INSET: f32 = 7.0;

/// 意图目录大小：按图级过滤（spec §4 裁定——有地形栅格 ∧ 图级 ∈
/// {World, Region}；M1 目录只有「生成大陆」一项，森林随 M4 加入）。
fn intent_count_for(level: wv_core::map::MapLevel) -> usize {
    match level {
        wv_core::map::MapLevel::World | wv_core::map::MapLevel::Region => 2,
        wv_core::map::MapLevel::City | wv_core::map::MapLevel::Interior => 0,
    }
}

/// 菜单可见性：工具**启用**且选区已闭合。`enabled` 门必不可少——
/// 「生成 → 关掉瓷砖 → Ctrl+Z 回驻留」会留下 enabled=false 的环，
/// 环不可见也不可交互，菜单若浮现就是「从空气里生成」（终审 Important 2）。
fn menu_visible(selection: &wv_render::state::SelectionState) -> bool {
    selection.enabled && selection.is_closed()
}

/// 选区意图菜单：底部居中浮动条。`selection.is_closed()` 才画；
/// 无意图图级显示占位文案（spec §12：不隐藏工具，要全图级一致）。
///
/// 「生成大陆」按钮只置 `ContinentDialog::open = true`——面板确认时经
/// `continent_dialog_system` 从 `SelectionState` 取环提交，本系统不碰 History。
#[allow(clippy::too_many_arguments)]
pub fn intent_menu(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    current_loc: Res<CurrentLocale>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    selection: Res<wv_render::state::SelectionState>,
    mut continent_dialog: ResMut<crate::ui::ContinentDialog>,
    mut city_dialog: ResMut<CityIntentDialog>,
    startup_req: Res<StartupLoadRequest>,
) {
    // 无工程锁 / 启动加载中 / 工具未启用或未闭合：与 map_controls 同源早返。
    if !projection.has_open_project() || startup_req.0.is_some() || !menu_visible(&selection) {
        return;
    }
    let loc = current_loc.0;
    let level = drill
        .current_node(&projection.world)
        .map(|n| n.level)
        .unwrap_or(wv_core::map::MapLevel::World);
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Area::new(egui::Id::new("wv_intent_menu_v1"))
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -18.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // 视觉对齐 map_controls 的圆角大框：panel_bg + 描边 + 投影。
            egui::Frame::NONE
                .fill(shell.panel_bg())
                .stroke(egui::Stroke::new(1.0, shell.border_subtle()))
                .corner_radius(egui::CornerRadius::from(radius::PANEL))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(71),
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if intent_count_for(level) == 0 {
                            // 无意图图级：占位文案，不误导作者去别的工具。
                            ui.weak(t(loc, I18nKey::IntentMenuNoIntents));
                        } else {
                            let icon = icon_image(
                                ICON_MOUNTAIN_RANGE,
                                egui::Color32::WHITE,
                                TILE_SIZE - ICON_INSET,
                            );
                            if ui
                                .add(
                                    egui::Button::image_and_text(
                                        icon.clone(),
                                        t(loc, I18nKey::IntentMenuGenerateContinent),
                                    )
                                    .fill(shell.accent())
                                    .corner_radius(egui::CornerRadius::from(radius::PANEL)),
                                )
                                .clicked()
                            {
                                // 只开面板；选区保持到面板确认时被消费（spec D5）。
                                continent_dialog.open = true;
                            }
                            let icon = icon_image(
                                ICON_MOUNTAIN_RANGE,
                                egui::Color32::WHITE,
                                TILE_SIZE - ICON_INSET,
                            );
                            if ui
                                .add(
                                    egui::Button::image_and_text(
                                        icon,
                                        t(loc, I18nKey::IntentMenuGenerateCity),
                                    )
                                    .fill(shell.accent())
                                    .corner_radius(egui::CornerRadius::from(radius::PANEL)),
                                )
                                .clicked()
                            {
                                // 预填进对话框自持（spec Review Focus 1：确认前选区
                                // 可被 Esc 清掉，不依赖存活）；确认后自动钻入。
                                if let Some(node) = drill.current_node(&projection.world) {
                                    if let Some(map) = drill.current() {
                                        prefill(
                                            &mut city_dialog,
                                            &selection,
                                            map.clone(),
                                            node.extent,
                                            projection.world.maps.len(),
                                        );
                                    }
                                }
                            }
                        }
                        ui.weak(t(loc, I18nKey::IntentMenuCancelHint));
                    });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::{intent_count_for, menu_visible};
    use wv_core::map::MapLevel;

    #[test]
    fn intent_catalog_empty_for_city_level() {
        assert_eq!(intent_count_for(MapLevel::City), 0, "城市级 v1 无意图");
        assert_eq!(intent_count_for(MapLevel::Interior), 0, "内景级 v1 无意图");
    }

    #[test]
    fn intent_catalog_nonempty_for_world_and_region() {
        assert_eq!(
            intent_count_for(MapLevel::World),
            2,
            "世界级：生成大陆 + 生成城市（M2）"
        );
        assert_eq!(
            intent_count_for(MapLevel::Region),
            2,
            "区域级同世界级（spec §4 裁定）"
        );
    }

    /// 终审 Important 2：生成 → 关掉选择瓷砖 → Ctrl+Z 回驻留选区——
    /// 此时 `enabled = false`（环不可见、不可交互），菜单不得浮现，
    /// 否则作者会「从空气里」生成一片大陆。
    #[test]
    fn intent_menu_requires_enabled_tool() {
        use wv_render::state::SelectionState;
        let ring = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let disabled = SelectionState {
            enabled: false,
            closed: Some(ring.clone()),
            ..Default::default()
        };
        assert!(!menu_visible(&disabled), "工具关着不得弹菜单");
        let enabled = SelectionState {
            enabled: true,
            closed: Some(ring),
            ..Default::default()
        };
        assert!(menu_visible(&enabled), "工具开着 + 选区闭合 → 弹菜单");
        let drawing = SelectionState {
            enabled: true,
            draft: vec![(0.0, 0.0)],
            ..Default::default()
        };
        assert!(!menu_visible(&drawing), "画中未闭合不弹菜单");
    }
}
