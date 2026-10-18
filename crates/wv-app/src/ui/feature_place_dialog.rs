//! 「新建地理事实」命名对话框（theme overlay v1 / 选择工具 M4）。
//!
//! ## 职责
//! - [`FeaturePlaceDialog`] —— 对话框的跨帧 UI 状态：打开标志、名称、以及
//!   **待落盘环 + 闭合那一刻钉住的地图 id**。
//! - [`feature_place_dialog`] —— 绘制 + 确认提交：把对话框自持的环经
//!   [`crate::edit::feature_place_actions::add_feature_place_now`] 落成
//!   `AddFeaturePlaceCommand`；成功后清空选区。
//!
//! ## 打开 / 关闭的单一来源
//!
//! 本对话框**没有**独立工具：它只由选区意图菜单的「森林」按钮打开
//! （[`super::intent_menu`] 调用 [`FeaturePlaceDialog::open_with`]）。
//! 点击意图按钮时把**当前闭合选区环**与**当前图 id**一并搬进对话框——
//! 打开后作者仍可编辑选区或下钻换图，都不影响这一圈地界：环坐标只在捕获图
//! 的局部帧里合法，钉住的 id 永远正确（final review 数据完整性口径）。
//!
//! ## 边界（不做什么）
//! - **不**直接改世界——提交走动作层的 `EditCommand`（AGENTS §3.3）；
//! - **不**画选区几何——那是 `wv_render::selection::draw_selection`；
//! - **不**硬编码文案——标题 / 标签 / 按钮全走 `I18nKey`（AGENTS §8）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing::EditHistory;
use crate::edit::feature_place_actions::place_biome_marker_now;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{PlaybackState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::widgets::{button as widget_button, ButtonContent, ButtonStyle};

use super::shell::UiShell;

/// 标记可选的生物群系目录（ADR-0029；海洋/河流不做——后者是线状，M2）。
///
/// 顺序即下拉框顺序：常见面状群系在前。
pub const MARKER_BIOMES: [(u8, I18nKey); 17] = [
    (wv_terrain::biome::BIOME_FOREST, I18nKey::MarkerBiomeForest),
    (wv_terrain::biome::BIOME_PLAINS, I18nKey::MarkerBiomePlains),
    (
        wv_terrain::biome::BIOME_GRASSLAND,
        I18nKey::MarkerBiomeGrassland,
    ),
    (wv_terrain::biome::BIOME_GOBI, I18nKey::MarkerBiomeGobi),
    (wv_terrain::biome::BIOME_DESERT, I18nKey::MarkerBiomeDesert),
    (wv_terrain::biome::BIOME_SWAMP, I18nKey::MarkerBiomeSwamp),
    (
        wv_terrain::biome::BIOME_WETLAND,
        I18nKey::MarkerBiomeWetland,
    ),
    (wv_terrain::biome::BIOME_TUNDRA, I18nKey::MarkerBiomeTundra),
    (
        wv_terrain::biome::BIOME_SNOWFIELD,
        I18nKey::MarkerBiomeSnowfield,
    ),
    (wv_terrain::biome::BIOME_JUNGLE, I18nKey::MarkerBiomeJungle),
    (
        wv_terrain::biome::BIOME_TROPICAL_RAINFOREST,
        I18nKey::MarkerBiomeTropicalRainforest,
    ),
    (wv_terrain::biome::BIOME_HILLS, I18nKey::MarkerBiomeHills),
    (
        wv_terrain::biome::BIOME_MOUNTAINS,
        I18nKey::MarkerBiomeMountains,
    ),
    (wv_terrain::biome::BIOME_OASIS, I18nKey::MarkerBiomeOasis),
    (
        wv_terrain::biome::BIOME_SAVANNA,
        I18nKey::MarkerBiomeSavanna,
    ),
    (
        wv_terrain::biome::BIOME_SALT_FLAT,
        I18nKey::MarkerBiomeSaltFlat,
    ),
    (wv_terrain::biome::BIOME_LAKE, I18nKey::MarkerBiomeLake),
];

/// 群系的本地化名（目录外返回 `"?"`，防御性兜底不 panic）。
pub fn marker_biome_name(loc: crate::i18n::Locale, biome: u8) -> &'static str {
    MARKER_BIOMES
        .iter()
        .find(|(b, _)| *b == biome)
        .map(|(_, key)| crate::i18n::t(loc, *key))
        .unwrap_or("?")
}

/// 「新建地理事实」对话框的跨帧状态。
///
/// **为什么是资源而不是 egui memory**：打开它的是选区意图菜单（egui 系统），
/// 但环与钉图需要在多个帧之间存活；创建失败时还要把输入留在框内跨帧展示。
///
/// 环与地图 id **自持**（不读渲染层会话态）：意图按钮点击那一刻把
/// `SelectionState.closed` 与 `drill.current()` 拷进来，之后选区怎么变、
/// 地图怎么下钻都与这次提交无关。
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct FeaturePlaceDialog {
    /// 是否打开。
    pub open: bool,
    /// 作者输入的名称（提交前 trim；空名禁用「创建」）。
    pub name: String,
    /// 闭合那一刻钉住的图 id（`drill.current()`）；`None` = 无待提交环。
    pub map: Option<wv_core::Id>,
    /// 待落盘的闭合环（**捕获图局部 wu**，首尾不重复，≥3 点）。
    pub ring: Vec<(f32, f32)>,
    /// 生物群系类型（ADR-0029；缺省森林）。
    pub biome: u8,
    /// 捕获那一刻钉住的格尺寸（wu/格）——打开后作者可下钻换图，
    /// 提交必须用捕获图的口径（终审 C1）。
    pub cell_size: f32,
    /// 捕获那一刻钉住的时间轴日。
    pub from: i64,
}

impl Default for FeaturePlaceDialog {
    fn default() -> Self {
        Self {
            open: false,
            name: String::new(),
            map: None,
            ring: Vec::new(),
            biome: wv_terrain::biome::BIOME_FOREST,
            cell_size: 0.0,
            from: 0,
        }
    }
}

impl FeaturePlaceDialog {
    /// 用选区环 + 当前图 id 打开（意图菜单点击「标记…」时调用）。
    ///
    /// 不触碰选区——选区保持到作者确认或取消（intent-menu 契约）。
    pub fn open_with(
        &mut self,
        map: wv_core::Id,
        ring: Vec<(f32, f32)>,
        cell_size: f32,
        from: i64,
    ) {
        self.open = true;
        self.name.clear();
        self.map = Some(map);
        self.ring = ring;
        self.biome = wv_terrain::biome::BIOME_FOREST;
        self.cell_size = cell_size;
        self.from = from;
    }

    /// 关闭并清空一切（取消 / X / 无工程 / 提交成功）。
    pub fn close(&mut self) {
        self.open = false;
        self.name.clear();
        self.map = None;
        self.ring.clear();
        self.biome = wv_terrain::biome::BIOME_FOREST;
        self.cell_size = 0.0;
        self.from = 0;
    }

    /// 提交参数：钉住的图 id + 环（调用方已确认 `open` 且环非空）。
    pub fn take_pending(&self) -> Option<(wv_core::Id, Vec<(f32, f32)>)> {
        self.map.clone().map(|m| (m, self.ring.clone()))
    }
}

/// 命名对话框：绘制 + 确认提交。
///
/// 提交成功后**不清空**选择工具选区（ADR-0033：意图生效不再消费选区，
/// 作者可继续调轮廓再生成；撤销只回滚命令本身）。
#[allow(clippy::too_many_arguments)]
pub fn feature_place_dialog(
    mut ctxs: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut dialog: ResMut<FeaturePlaceDialog>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    playback: Res<PlaybackState>,
    current_loc: Res<CurrentLocale>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<Time>,
) {
    // 无工程（关工程 / 加载失败）：不弹新建框，顺手回归无待提交状态。
    if !projection.has_open_project() {
        if dialog.open {
            dialog.close();
        }
        return;
    }

    if !dialog.open {
        return;
    }

    // 打开态但待提交环已失效（环 <3 点 / 无钉图）：跟随关闭，不留孤儿状态。
    if dialog.ring.len() < 3 || dialog.map.is_none() {
        dialog.close();
        return;
    }

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
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(t_key(&current_loc, I18nKey::MarkerBiomeLabel));
                let loc = current_loc.0;
                egui::ComboBox::from_id_salt("wv_marker_biome")
                    .selected_text(marker_biome_name(loc, dialog.biome))
                    .show_ui(ui, |ui| {
                        for (biome, _) in MARKER_BIOMES {
                            ui.selectable_value(
                                &mut dialog.biome,
                                biome,
                                marker_biome_name(loc, biome),
                            );
                        }
                    });
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

    // X（`open(&mut open)` 写回）与「取消」都关闭；「创建」失败时留在框内。
    if cancelled || !open {
        dialog.close();
        return;
    }

    if confirmed {
        let name = dialog.name.trim().to_owned();
        if name.is_empty() {
            return; // 按钮本应禁用；双保险（回车等未来入口）。
        }
        let Some((map, ring)) = dialog.take_pending() else {
            dialog.close();
            return;
        };
        let frame = (time.elapsed_secs() * 60.0) as u64;
        match place_biome_marker_now(
            &mut projection,
            &mut edit,
            &playback,
            &mut tex,
            map,
            name,
            dialog.biome,
            &ring,
            dialog.cell_size,
            dialog.from,
        ) {
            Ok(_) => {
                // 选区保持不动（ADR-0033）。
                dialog.close();
            }
            Err(msg) => {
                // 失败留在框内（作者可改群系/重圈），回执给人话（终审 I4）。
                notice.error(msg, frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_defaults_to_forest_biome() {
        let mut d = FeaturePlaceDialog::default();
        d.open_with(
            wv_core::Id::new_unchecked("map_world"),
            vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)],
            8.0,
            7,
        );
        assert_eq!(d.biome, wv_terrain::biome::BIOME_FOREST, "缺省群系应为森林");
        assert_eq!(d.cell_size, 8.0, "打开时应钉住格尺寸（终审 C1）");
        assert_eq!(d.from, 7, "打开时应钉住时间轴日");
        d.close();
        assert_eq!(d.biome, wv_terrain::biome::BIOME_FOREST);
    }

    #[test]
    fn biome_catalog_has_no_ocean_or_river() {
        assert!(
            !MARKER_BIOMES
                .iter()
                .any(|(b, _)| *b == wv_terrain::biome::BIOME_OCEAN),
            "海洋不做标记（空势力/水域）"
        );
        assert!(
            !MARKER_BIOMES
                .iter()
                .any(|(b, _)| *b == wv_terrain::biome::BIOME_RIVER),
            "河流是线状标记，M2 再做"
        );
        assert_eq!(MARKER_BIOMES.len(), 17, "目录应有 17 个面状群系");
        // 每个群系都能取到本地化名（不存在 "?" 兜底）。
        for (b, _) in MARKER_BIOMES {
            assert_ne!(
                marker_biome_name(crate::i18n::Locale::Zh, b),
                "?",
                "群系 {b} 缺中文名"
            );
        }
    }
}
