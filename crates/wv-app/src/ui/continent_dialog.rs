//! 「生成大陆」属性面板（大陆生成 UI plan v0.1 Task 2；spec §8 2026-10-17）。
//!
//! ## 职责
//! - [`ContinentDialog`] —— 八参数 + 种子 + 文本框状态的跨帧 UI 状态。
//! - [`continent_dialog`] —— 绘制面板，确认时经回调把 `(ContinentParams, seed)`
//!   交给调用方（ui / apply 拆分，`new_map_dialog` 同款模式）。
//!
//! ## 边界（不做什么）
//! - **不**直接生成地形 / 不碰 `History`——回调方在 wv-app 装配层派
//!   `GenerateContinentCommand`（§3.3）。
//! - **不**管入口——工具条 / 命令面板只置 `open = true`。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::i18n::Locale;
use wv_terrain::continent::{ContinentParams, IslandDensity, ReliefLevel};

use crate::i18n::{t, I18nKey};
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;

/// 对话框跨帧状态。
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq)]
pub struct ContinentDialog {
    /// 是否打开。
    pub open: bool,
    /// 八参数（全局单一面板，跨圈共享）。
    pub params: ContinentParams,
    /// 当前种子（骰子按钮直接改写）。
    pub seed: u64,
    /// 文本框中显示的种子串（解析失败保持旧值，不打断输入）。
    seed_text: String,
}

impl Default for ContinentDialog {
    fn default() -> Self {
        let params = ContinentParams::default();
        Self {
            open: false,
            params,
            seed: 0,
            seed_text: "0".into(),
        }
    }
}

/// 解析种子文本：非数字回退旧值（不 panic、不弹错——V1 只提示不阻止）。
fn parse_seed_text(text: &str, fallback: u64) -> u64 {
    text.trim().parse::<u64>().unwrap_or(fallback)
}

/// 骰子按钮的「下一颗」：线性同余，对作者是伪随机、对测试是确定性。
fn next_seed(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

/// 绘制面板；「生成」确认时调用 `on_generate`。
pub fn continent_dialog(
    ctx: &egui::Context,
    shell: &UiShell,
    dialog: &mut ContinentDialog,
    loc: Locale,
    on_generate: impl FnOnce(ContinentParams, u64),
) {
    if !dialog.open {
        return;
    }
    let mut generate = false;
    let mut cancel = false;

    let title = t(loc, I18nKey::ContinentDialogTitle);
    DialogWindow::new(title, &mut dialog.open)
        .width(420.0)
        .show(ctx, shell, |ui| {
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamSeed));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut dialog.seed_text)
                        .desired_width(160.0)
                        .hint_text("0"),
                );
                if resp.changed() {
                    dialog.seed = parse_seed_text(&dialog.seed_text, dialog.seed);
                }
                if ui.button("🎲").clicked() {
                    dialog.seed = next_seed(dialog.seed);
                    dialog.seed_text = dialog.seed.to_string();
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamCoastWiggle));
                ui.add(
                    egui::Slider::new(&mut dialog.params.coast_wiggle, 0.0..=1.0).show_value(false),
                );
                ui.label(format!("{:.0}%", dialog.params.coast_wiggle * 100.0));
            });

            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamCoastDetail));
                for (oct, label) in [(2u32, "2"), (4, "4"), (6, "6")] {
                    ui.selectable_value(&mut dialog.params.coast_octaves, oct, label);
                }
            });

            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamRelief));
                for (level, label) in [
                    (ReliefLevel::Hills, "600m"),
                    (ReliefLevel::Mountains, "2000m"),
                    (ReliefLevel::HighMountains, "4000m"),
                ] {
                    ui.selectable_value(&mut dialog.params.relief, level, label);
                }
            });

            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamForest));
                ui.add(
                    egui::Slider::new(&mut dialog.params.forest_cover, 0.0..=1.0).show_value(false),
                );
                ui.label(format!("{:.0}%", dialog.params.forest_cover * 100.0));
            });

            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamDesert));
                ui.add(
                    egui::Slider::new(&mut dialog.params.desert_cover, 0.0..=1.0).show_value(false),
                );
                ui.label(format!("{:.0}%", dialog.params.desert_cover * 100.0));
            });

            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamShelf));
                ui.add(egui::Slider::new(&mut dialog.params.shelf_cells, 0..=32));
                ui.label("cells");
            });

            ui.horizontal(|ui| {
                ui.label(t(loc, I18nKey::ContinentParamIslands));
                for (d, label) in [
                    (IslandDensity::Off, "Off"),
                    (IslandDensity::Low, "Low"),
                    (IslandDensity::Medium, "Mid"),
                    (IslandDensity::High, "High"),
                ] {
                    ui.selectable_value(&mut dialog.params.islands, d, label);
                }
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button(t(loc, I18nKey::ContinentCancel)).clicked() {
                    cancel = true;
                }
                if ui
                    .button(egui::RichText::new(t(loc, I18nKey::ContinentGenerate)).strong())
                    .clicked()
                {
                    generate = true;
                }
            });
        });

    if cancel {
        dialog.open = false;
    }
    if generate {
        dialog.open = false;
        on_generate(dialog.params, dialog.seed);
    }
}

/// Bevy 系统：绘制面板 + 把「生成」接到 `GenerateContinentCommand`。
///
/// 拆成系统而非塞进 `ui_shell` 的大分发器：面板要读写 10+ 个资源，
/// 单系统参数上限（16）比分发器已经顶满的元组链安全得多。
#[allow(clippy::too_many_arguments)]
pub fn continent_dialog_system(
    mut ctxs: bevy_egui::EguiContexts,
    shell_cfg: bevy::prelude::Res<crate::theme::ui_theme::UiShellConfig>,
    mut dialog: bevy::prelude::ResMut<ContinentDialog>,
    current_loc: bevy::prelude::Res<crate::i18n::CurrentLocale>,
    drill: bevy::prelude::Res<crate::state::DrillState>,
    grid: bevy::prelude::Res<crate::state::GridConfig>,
    mut projection: bevy::prelude::ResMut<crate::state::WorldProjection>,
    mut edit: bevy::prelude::ResMut<crate::edit::editing::EditHistory>,
    mut tex: bevy::prelude::ResMut<wv_render::terrain_view::TerrainTexture>,
    mut sketch: bevy::prelude::ResMut<crate::state::ContinentSketch>,
    mut gen: bevy::prelude::ResMut<crate::state::ContinentGenerationState>,
    mut notice: bevy::prelude::ResMut<crate::ui_shell::ShellNotice>,
    time: bevy::prelude::Res<bevy::prelude::Time>,
) {
    if !dialog.open {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let map = drill.current().cloned();
    let cell_size = grid.cell_size;
    let frame = (time.elapsed_secs() * 60.0) as u64;

    continent_dialog(ctx, &shell, &mut dialog, loc, |params, seed| {
        let Some(map) = map else {
            notice.error(t(loc, I18nKey::ContinentNoticeNoMap), frame);
            return;
        };
        if sketch.envelopes.is_empty() {
            notice.error(t(loc, I18nKey::ContinentNoticeNoEnvelopes), frame);
            return;
        }
        let envelopes = std::mem::take(&mut sketch.envelopes);
        match crate::edit::editing::commit_continent_generation(
            map,
            envelopes,
            params,
            seed,
            cell_size,
            &mut edit,
            &mut projection,
            &mut tex,
            &mut sketch,
            &mut gen,
        ) {
            Ok(()) => notice.info(t(loc, I18nKey::ContinentNoticeGenerated), frame),
            Err(e) => notice.error(
                format!("{}: {e}", t(loc, I18nKey::ContinentNoticeGenerateFailed)),
                frame,
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dialog_params_match_ruling_and_seed_parse_falls_back() {
        let d = ContinentDialog::default();
        assert_eq!(d.params, ContinentParams::default(), "缺省应等于裁决八参数");
        assert!(!d.open, "缺省不打开");
        assert_eq!(
            parse_seed_text("not-a-number", 42),
            42,
            "非数字种子输入应保持旧值"
        );
        assert_eq!(parse_seed_text("12345", 42), 12345, "合法数字应采纳");
        assert_eq!(parse_seed_text("  7 ", 42), 7, "两侧空白应容忍");
    }
}
