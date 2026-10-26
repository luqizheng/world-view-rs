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
//! - **不出现在 CellMap 工作区**（2026-10-24 用户裁定）——cell 级的应用条件
//!   （海拔增减 / 设水陆 / 设群系）在 Inspector 的 Cell 批量区，见
//!   [`cell_workspace_active`]。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::DrillState;
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::city_intent::{prefill, CityIntentDialog};

use super::shell::UiShell;
use super::widgets::icon_image;
use super::{
    ICON_GLOBE, ICON_HILLS, ICON_LAKE, ICON_LAYERS, ICON_MAP_PIN, ICON_MOUNTAIN_RANGE,
    ICON_PLATEAU, ICON_TREES,
};
use crate::theme::ui_tokens::radius;

/// 单瓷砖边长与图标内嵌（视觉对齐 [`super::map_controls`] 的瓷砖规格）。
const TILE_SIZE: f32 = 34.0;
const ICON_INSET: f32 = 7.0;

/// 地形意图可用性：当前图有**自有地形栅格**（spec §4 谓词后半句；
/// 终审 Important 1——无栅格图的预检/提交口径不一致，按钮直接不给）。
fn terrain_intents_available(world: &wv_core::World, map: &wv_core::Id) -> bool {
    world.terrain_of(map).is_some()
}

/// 意图目录大小：按图级过滤（spec §4 裁定——有地形栅格 ∧ 图级 ∈
/// {World, Region}）。M4 起目录含「森林」：大陆 + 城市 + 森林 + 地形四意图。
fn intent_count_for(level: wv_core::map::MapLevel) -> usize {
    match level {
        wv_core::map::MapLevel::World | wv_core::map::MapLevel::Region => 8,
        wv_core::map::MapLevel::City | wv_core::map::MapLevel::Interior => 0,
    }
}

/// 意图菜单要打开的四个对话框（打包成一个 SystemParam）。
///
/// **为什么打包**：`intent_menu` 的参数已顶到 Bevy 的 16 元组上限
/// （2026-10-17 对话框拆分时的同一条经验）；每加一个对话框就超限。
/// 打包后资源只占 1 个参数槽。2026-10-19：地图工具条偏好（吸附开关）
/// 也搭这一层——划分势力的默认边界模式由它决定。
#[derive(bevy::ecs::system::SystemParam)]
pub struct IntentMenuDialogs<'w> {
    pub continent: ResMut<'w, crate::ui::ContinentDialog>,
    pub city: ResMut<'w, CityIntentDialog>,
    pub feature: ResMut<'w, crate::ui::FeaturePlaceDialog>,
    pub divide: ResMut<'w, crate::ui::FactionDivisionDialog>,
    pub dock: Res<'w, crate::ui::map_controls::MapDockState>,
}

/// 划分势力按钮的可用性：当前图有自有地形栅格（与地形意图同门）。
fn terrain_ok_for_divide(world: &wv_core::World, drill: &DrillState) -> bool {
    drill
        .current()
        .map(|m| terrain_intents_available(world, m))
        .unwrap_or(false)
}

/// 菜单可见性：工具**启用**且选区已闭合。`enabled` 门必不可少——
/// 「生成 → 关掉瓷砖 → Ctrl+Z 回驻留」会留下 enabled=false 的环，
/// 环不可见也不可交互，菜单若浮现就是「从空气里生成」（终审 Important 2）。
fn menu_visible(selection: &wv_render::state::SelectionState) -> bool {
    selection.enabled && selection.is_closed()
}

/// CellMap 工作区判定：底座档为 `ViewMode::Topographic`（顶栏第三段的
/// 进入 / 退出都落在这个模式，见 `top_bar::browse_edit_segment`）。
///
/// 工作区内**不出意图菜单**（2026-10-24 用户裁定）：作者圈多边形是在选
/// 「一片格子」，应用条件——海拔增减 / 设水陆 / 设群系——全在 Inspector
/// 的 Cell 批量区；再弹八意图条等于给同一份选区两套「接下来做什么」，
/// 每圈一次都要手动关一次。图级意图（大陆 / 城市 / 标记 / 划分势力 /
/// 地形四意图）在 Browse / Edit 档照常可得。经命令面板切到 Topographic
/// 的边角情形同样适用：等高底图档就是 cell 级编辑语境。
fn cell_workspace_active(mode: wv_render::state::ViewMode) -> bool {
    mode == wv_render::state::ViewMode::Topographic
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
    selection: Res<wv_render::state::SelectionState>,
    mut dialogs: IntentMenuDialogs,
    mut projection: ResMut<wv_render::state::WorldProjection>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    grid: Res<wv_render::state::GridConfig>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    playback: Res<crate::state::PlaybackState>,
    time: Res<bevy::prelude::Time>,
    screen: Res<crate::app_screen::AppScreen>,
    view: Res<wv_render::state::ViewState>,
) {
    // 无工程闸门 / 启动加载中 / 播放中（V37）/ 工具未启用或未闭合：同源早返。
    if playback.playing {
        return;
    }
    if !screen.editor_visible() || !menu_visible(&selection) {
        return;
    }
    // CellMap 工作区：应用条件在 Inspector 批量区（2026-10-24 裁定，
    // 见 `cell_workspace_active`）。
    if cell_workspace_active(view.mode) {
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
                                ICON_GLOBE,
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
                                dialogs.continent.open = true;
                            }
                            let icon = icon_image(
                                ICON_MAP_PIN,
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
                                            &mut dialogs.city,
                                            &selection,
                                            map.clone(),
                                            node.extent,
                                            projection.world.maps.len(),
                                        );
                                    }
                                }
                            }
                            // 森林（M4）：只把环与**当前图 id** 搬进命名对话框，
                            // 不消费选区——确认 / 取消都发生在对话框里。
                            let icon = icon_image(
                                ICON_TREES,
                                egui::Color32::WHITE,
                                TILE_SIZE - ICON_INSET,
                            );
                            if ui
                                .add(
                                    egui::Button::image_and_text(
                                        icon,
                                        t(loc, I18nKey::IntentMenuMarker),
                                    )
                                    .fill(shell.accent())
                                    .corner_radius(egui::CornerRadius::from(radius::PANEL)),
                                )
                                .clicked()
                            {
                                if let (Some(map), Some(ring)) =
                                    (drill.current().cloned(), selection.closed.clone())
                                {
                                    dialogs.feature.open_with(
                                        map,
                                        ring,
                                        grid.cell_size,
                                        playback.current_day,
                                    );
                                }
                            }
                            // 划分势力（ADR-0028）：把闭合环 + 当前图 + 解析出的宿主
                            // 搬进对话框；宿主解析只认实控、取面积最小的含入势力。
                            let icon = icon_image(
                                ICON_LAYERS,
                                egui::Color32::WHITE,
                                TILE_SIZE - ICON_INSET,
                            );
                            if ui
                                .add_enabled(
                                    terrain_ok_for_divide(&projection.world, &drill),
                                    egui::Button::image_and_text(
                                        icon,
                                        t(loc, I18nKey::IntentMenuDivideFaction),
                                    )
                                    .fill(shell.accent())
                                    .corner_radius(egui::CornerRadius::from(radius::PANEL)),
                                )
                                .clicked()
                            {
                                if let (Some(map), Some(ring)) =
                                    (drill.current().cloned(), selection.closed.clone())
                                {
                                    let host = crate::ui::faction_divide_dialog::resolve_host(
                                        &projection.world,
                                        &map,
                                        &ring,
                                        playback.current_day,
                                    );
                                    dialogs.divide.open_with(
                                        map,
                                        ring,
                                        host,
                                        grid.cell_size,
                                        playback.current_day,
                                        dialogs.dock.snapping,
                                    );
                                }
                            }
                            // 地形按钮栅格门（spec §4：有**自有**地形栅格才可用，
                            // 终审 Important 1——无栅格图的预检/提交口径不一致）。
                            let terrain_ok = drill
                                .current()
                                .map(|m| terrain_intents_available(&projection.world, m))
                                .unwrap_or(false);
                            // 地形四意图（spec §4 D10：一键应用，无弹窗）。
                            // ADR-0033：生效后**不清空选区**——作者可继续加锚点 /
                            // 调轮廓再点其它意图；撤销只回滚地形。
                            let ring = selection.closed.clone();
                            let apply_map = drill.current().cloned();
                            let mut apply = |kind: wv_terrain::intent::Intent| {
                                if let Some(map) = apply_map.clone() {
                                    let frame = (time.elapsed_secs() * 60.0) as u64;
                                    let _ = crate::edit::editing::commit_region_intent(
                                        map,
                                        kind,
                                        grid.cell_size,
                                        &playback,
                                        loc,
                                        &mut edit,
                                        &mut projection,
                                        &mut tex,
                                        &mut notice,
                                        frame,
                                    );
                                }
                            };
                            let mut terrain_button =
                                |icon: egui::ImageSource<'static>, label: &'static str| -> bool {
                                    ui.add_enabled(
                                        terrain_ok,
                                        egui::Button::image_and_text(
                                            icon_image(
                                                icon,
                                                egui::Color32::WHITE,
                                                TILE_SIZE - ICON_INSET,
                                            ),
                                            label,
                                        )
                                        .fill(shell.accent())
                                        .corner_radius(egui::CornerRadius::from(radius::PANEL)),
                                    )
                                    .clicked()
                                };
                            if terrain_button(
                                ICON_MOUNTAIN_RANGE,
                                t(loc, I18nKey::IntentMenuMountains),
                            ) {
                                apply(wv_terrain::intent::Intent::MountainMassif {
                                    polygon: ring.clone().unwrap_or_default(),
                                });
                            }
                            if terrain_button(ICON_HILLS, t(loc, I18nKey::IntentMenuHills)) {
                                apply(wv_terrain::intent::Intent::HillsRegion {
                                    polygon: ring.clone().unwrap_or_default(),
                                });
                            }
                            if terrain_button(ICON_PLATEAU, t(loc, I18nKey::IntentMenuPlains)) {
                                apply(wv_terrain::intent::Intent::PlainsRegion {
                                    polygon: ring.clone().unwrap_or_default(),
                                });
                            }
                            if terrain_button(ICON_LAKE, t(loc, I18nKey::IntentMenuLake)) {
                                apply(wv_terrain::intent::Intent::LakeRegion {
                                    polygon: ring.clone().unwrap_or_default(),
                                });
                            }
                        }
                        ui.weak(t(loc, I18nKey::IntentMenuCancelHint));
                    });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::{cell_workspace_active, intent_count_for, menu_visible, terrain_intents_available};
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
            8,
            "世界级：大陆 + 城市 + 森林（M4）+ 划分势力（ADR-0028）+ 地形四意图（M3）"
        );
        assert_eq!(
            intent_count_for(MapLevel::Region),
            8,
            "区域级同世界级（spec §4 裁定）"
        );
    }

    /// 终审 Important 2：生成 → 关掉选择瓷砖 → Ctrl+Z 回驻留选区——
    /// 此时 `enabled = false`（环不可见、不可交互），菜单不得浮现，
    /// 否则作者会「从空气里」生成一片大陆。
    /// 地形按钮栅格门（终审 Important 1）：无自有栅格的图不出地形按钮。
    #[test]
    fn terrain_intents_require_own_raster() {
        use wv_core::map::{MapLevel, MapNode};
        let mut world = wv_core::World::default();
        world.maps.push(MapNode {
            id: wv_core::Id::new_unchecked("bare_map"),
            level: MapLevel::Region,
            ..Default::default()
        });
        let bare = wv_core::Id::new_unchecked("bare_map");
        assert!(
            !terrain_intents_available(&world, &bare),
            "无自有栅格 → 地形意图不可用"
        );
        world.set_terrain(
            bare.clone(),
            wv_core::terrain::TerrainData {
                width: 8,
                height: 8,
                cells: vec![
                    wv_core::terrain::TerrainCellData {
                        height: 10,
                        surface: 1,
                        biome: 3,
                    };
                    64
                ],
            },
        );
        assert!(
            terrain_intents_available(&world, &bare),
            "有自有栅格 → 可用"
        );
    }

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

    /// CellMap 工作区（Topographic 档）不出意图菜单：应用条件在 Inspector
    /// 的 Cell 批量区（2026-10-24 用户裁定）；Browse / Edit 档照常出。
    #[test]
    fn intent_menu_suppressed_in_cell_workspace() {
        use wv_render::state::{SelectionState, ViewMode};
        let closed = SelectionState {
            enabled: true,
            closed: Some(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]),
            ..Default::default()
        };
        assert!(menu_visible(&closed), "前置：选区闭合本应可见");
        assert!(
            cell_workspace_active(ViewMode::Topographic),
            "等高底图档 = CellMap 工作区 → 菜单不出"
        );
        assert!(
            !cell_workspace_active(ViewMode::VectorFlat),
            "Browse 档照常出菜单（大陆 / 城市等图级意图的入口）"
        );
    }
}
