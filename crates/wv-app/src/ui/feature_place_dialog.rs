//! 「新建地理事实」命名对话框（theme overlay Task 7）。
//!
//! ## 职责
//! - [`FeaturePlaceDialog`] —— 对话框的跨帧 UI 状态（打开标志 + 名称）。
//! - [`feature_place_dialog`] —— 绘制 + 确认提交：消费
//!   [`TerritorySketch::pending`] 里已闭合的环，经
//!   [`crate::edit::feature_place_actions::add_feature_place_now`] 落成
//!   `AddFeaturePlaceCommand`。
//!
//! ## 打开 / 关闭的单一来源
//!
//! 本对话框**没有**命令面板入口：它只由圈地界工具的闭合动作触发。
//! 判据是 `TerritorySketch.pending.is_some()`——`try_close` 写环、对话框
//! 系统读环并置 `open = true`；创建 / 取消 / 环被外部清掉都关闭它。
//! 这样「有一个待命名的环」与「对话框可见」永不脱节。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界——提交走动作层的 `EditCommand`（AGENTS §3.3）；
//! - **不**持有环——环属于 `TerritorySketch`，本资源只有 open/name；
//! - **不**硬编码文案——标题 / 标签 / 按钮全走 `I18nKey`（AGENTS §8）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing::EditHistory;
use crate::edit::feature_place_actions::add_feature_place_now;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{PlaybackState, TerritorySketch, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};
use wv_render::theme::overlays::FEATURE_FOREST;

use super::shell::UiShell;

/// 「新建地理事实」对话框的跨帧状态。
///
/// **为什么是资源而不是 egui memory**：打开它的是圈地界系统（渲染层，
/// 不持 UI 记忆），且创建失败时要把输入留在框内跨帧展示。
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct FeaturePlaceDialog {
    /// 是否打开（`pending` 出现即开；创建 / 取消即关）。
    pub open: bool,
    /// 作者输入的名称（提交前 trim；空名禁用「创建」）。
    pub name: String,
}

/// 命名对话框：绘制 + 确认提交。
///
/// `pending` → 对话框是单行道：没有 `pending` 就没有对话框；取消 / 创建
/// 都把它清掉。这样工具与对话框不会互相留下孤儿状态。
#[allow(clippy::too_many_arguments)]
pub fn feature_place_dialog(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<FeaturePlaceDialog>,
    mut sketch: ResMut<TerritorySketch>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    playback: Res<PlaybackState>,
    current_loc: Res<CurrentLocale>,
) {
    // 无工程（关工程 / 加载失败）：不弹新建框，顺手回归没有待命名环的状态。
    if !projection.has_open_project() {
        if dialog.open || sketch.pending.is_some() {
            dialog.open = false;
            dialog.name.clear();
            sketch.pending = None;
        }
        return;
    }

    // 关闭态：环一闭合就打开命名框（唯一的打开路径）。
    if !dialog.open {
        if sketch.pending.is_some() {
            dialog.open = true;
            dialog.name.clear();
        }
        return;
    }

    // 打开态但环没了（被外部清掉 / 撤销 / 切工具）：跟随关闭。
    let Some(pending) = sketch.pending.clone() else {
        dialog.open = false;
        dialog.name.clear();
        return;
    };

    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let mut open = dialog.open;
    let mut confirmed = false;
    let mut cancelled = false;

    egui::Window::new(t_key(&current_loc, I18nKey::FeaturePlaceDialogTitle))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(360.0)
        // 模态置顶：浮动面板是 Middle，弹窗必须在 Foreground 才压得住
        // （与新建地图 / 新建工程对话框同层）。
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(t_key(&current_loc, I18nKey::FeaturePlaceNameLabel));
                let mut name = dialog.name.clone();
                if ui
                    .add(egui::TextEdit::singleline(&mut name).desired_width(220.0))
                    .changed()
                {
                    dialog.name = name;
                }
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui
                    .button(t_key(&current_loc, I18nKey::CommonCancel))
                    .clicked()
                {
                    cancelled = true;
                }
                let can_create = !dialog.name.trim().is_empty();
                if widget_button(
                    ui,
                    &shell,
                    ButtonContent::Text(t_key(&current_loc, I18nKey::FeaturePlaceCreate)),
                    ButtonStyle::Accent,
                    None,
                    can_create,
                )
                .clicked()
                {
                    confirmed = true;
                }
            });
        });

    // X（`open(&mut open)` 写回）与「取消」都清环；「创建」失败时留在框内。
    dialog.open = open && !cancelled;
    if !dialog.open {
        sketch.pending = None;
        dialog.name.clear();
        return;
    }

    if confirmed {
        let name = dialog.name.trim().to_owned();
        if name.is_empty() {
            return; // 按钮本应禁用；双保险（回车等未来入口）。
        }
        // 地图用闭合时钉住的捕获图 id（`pending.map`）——对话框开着时
        // 地图背后仍可下钻，现取 `view.current_map` 会把环落进错误的图。
        if add_feature_place_now(
            &mut projection,
            &mut edit,
            &playback,
            name,
            FEATURE_FOREST,
            &pending.map,
            &pending.ring,
        )
        .is_some()
        {
            // 环已消费：关框并清名，下一圈从空名开始。
            sketch.pending = None;
            dialog.name.clear();
            dialog.open = false;
        }
    }
}
