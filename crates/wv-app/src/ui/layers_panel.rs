//! 图层面板 + 世界树。
//!
//! ## 职责
//! - `panel_layers` —— **当前图声明图层列表**（Figma `3:20892` 逐项对位，
//!   2026-10-23 接真实 `MapNode.layers`）：动作行（DRAW ORDER）+ 图层行
//!   （眼睛 / 类型图标 / 名称 / 计数 / 锁）+ 分隔线 + 选中层不透明度滑条。
//!   眼睛 / 锁 / 不透明度经 [`wv_edit::UpdateLayerCommand`] 写回 YAML 声明
//!   （可撤销、驱动脏路径）——《MAP_LAYER_RENDERING_SPEC》§8 缺口 #4 收口。
//!   面板尾部保留 v0.3 的 8 个**会话开关**（`ViewState::*_visible`）：会话轴
//!   与声明轴在 `LayerFrame::resolve` 是**合取**关系，二者并存不冗余。
//! - 地图树已迁至 [`crate::ui::map_tree`]（2026-10-27 地图树增强 spec）。
//!
//! ## 边界
//! - **不**直接改世界 —— 眼睛 / 锁 / 滑条生成 `EditCommand` 交给
//!   [`crate::edit::editing::EditHistory`]（§3.3）。
//! - 图层行的眼睛写的是**声明层** `Layer.visible`（持久、可撤销），不是
//!   会话开关——`LayerFrame` 每 kind 只认首个声明层，逐层开关只能落在这里。
//!
//! ## 主要导出
//! - `panel_layers` / `panel_map_tree` —— Bevy 系统。
//! - `LayerPanelUiState` —— 面板会话内 UI 状态（选中层 + 滑条拖动中值）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use wv_core::map::{Layer, LayerKind};
use wv_core::{Id, World};

use crate::edit::editing::EditHistory;
use crate::i18n::{t_key, CurrentLocale, I18nKey, Locale};
use crate::state::{Panel, PanelState, ViewState, WorldProjection};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::theme::ui_tokens::{radius, space};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::float_window;
use wv_ui::widgets::{
    divider, empty_state, layer_actions, layer_list, opacity_row, LayerEntry, LayerPanelAction,
    LAYER_BLOCK_GAP,
};

/// 图层面板的会话内 UI 状态（不落盘、不属于世界数据，P7 不受影响）。
#[derive(Resource, Default)]
pub struct LayerPanelUiState {
    /// 选中图层 ID（跨帧保持；图层增删后按 ID 重新对位，对不上退回首行）。
    pub selected: Option<Id>,
    /// 不透明度滑条拖动中的即时值——拖完一次性提交命令，避免按帧碎提交
    /// 撑爆 Undo 栈。
    pub pending_opacity: Option<f32>,
}

/// 面板快照行（进闭包前从 `MapNode.layers` 克隆，规避闭包内与世界借用打架）。
struct LayerRow {
    id: Id,
    label: String,
    kind: LayerKind,
    visible: bool,
    locked: bool,
    opacity: f32,
    count: Option<u32>,
}

/// 图层面板（Figma `3:20892`：动作行 + 图层列表 + 不透明度行 + 会话开关段）。
#[allow(clippy::too_many_arguments)]
pub fn panel_layers(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut layer_ui: ResMut<LayerPanelUiState>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let map_id = Id::new_unchecked(view.current_map.clone());
    let rows = layer_rows(&projection.world, &map_id, current_loc.0);
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Layers,
        None,
        current_loc.0,
        |ui| {
            // 块距显式给 0（同 wv-ui layer_panel 的口径），间距走 LAYER_BLOCK_GAP。
            ui.spacing_mut().item_spacing.y = 0.0;
            // 数据侧无图层组、Custom 层无渲染消费者——新建钮不画（spec §8）。
            let mut action = layer_actions(
                ui,
                &shell,
                t_key(&current_loc, I18nKey::PanelLayersDrawOrder),
                false,
            );
            ui.add_space(LAYER_BLOCK_GAP);

            if rows.is_empty() {
                empty_layers(ui, &shell, &current_loc);
            } else {
                let selected = rows
                    .iter()
                    .position(|r| layer_ui.selected.as_ref() == Some(&r.id))
                    .unwrap_or(0);
                let entries: Vec<LayerEntry<'_>> = rows
                    .iter()
                    .map(|r| LayerEntry {
                        label: r.label.as_str(),
                        count: r.count,
                        visible: r.visible,
                        locked: r.locked,
                        icon: kind_icon(&r.kind),
                    })
                    .collect();
                // 图层多于 6 行时列表内滚动（Figma 列表区 178 = 6×28 + 5×2），
                // 面板本体不无限长高。
                egui::ScrollArea::vertical()
                    .id_salt("wv_layer_list")
                    .auto_shrink([false, true])
                    .max_height(178.0)
                    .show(ui, |ui| {
                        if let Some(a) = layer_list(ui, &shell, &entries, selected) {
                            action = Some(a);
                        }
                    });
                ui.add_space(LAYER_BLOCK_GAP);
                divider(ui, &shell);
                ui.add_space(LAYER_BLOCK_GAP);

                // 不透明度行作用于**选中层**；拖动中只记 pending（面板本地），
                // 松手 / 单击跳值才提交命令——一次拖动 = 一条 Undo 记录。
                let sel = &rows[selected];
                let mut opacity = layer_ui.pending_opacity.unwrap_or(sel.opacity);
                let resp = opacity_row(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::PanelLayersOpacity),
                    &mut opacity,
                );
                let wants_commit = resp.drag_stopped() || (resp.changed() && !resp.dragged());
                if wants_commit && (opacity - sel.opacity).abs() > 1e-3 {
                    commit_layer(
                        &mut projection,
                        &mut edit,
                        wv_edit::UpdateLayerCommand::opacity(
                            map_id.clone(),
                            sel.id.clone(),
                            opacity,
                        ),
                    );
                }
                if resp.drag_stopped() {
                    layer_ui.pending_opacity = None;
                } else if resp.dragged() {
                    layer_ui.pending_opacity = Some(opacity);
                }
            }

            // 落地列表动作（眼睛 / 锁 / 选中行）。
            match action {
                Some(LayerPanelAction::ToggleVisible(i)) => {
                    let r = &rows[i];
                    commit_layer(
                        &mut projection,
                        &mut edit,
                        wv_edit::UpdateLayerCommand::visibility(
                            map_id.clone(),
                            r.id.clone(),
                            !r.visible,
                        ),
                    );
                }
                Some(LayerPanelAction::ToggleLocked(i)) => {
                    let r = &rows[i];
                    commit_layer(
                        &mut projection,
                        &mut edit,
                        wv_edit::UpdateLayerCommand::locked(
                            map_id.clone(),
                            r.id.clone(),
                            !r.locked,
                        ),
                    );
                }
                Some(LayerPanelAction::Select(i)) => {
                    layer_ui.selected = Some(rows[i].id.clone());
                }
                _ => {}
            }

            // 势力图层组（ADR-0079）：每层一行「名字 + 眼睛」，经既有
            // `UpdateFactionLayerCommand`（全字段快照命令，先读现值再改
            // `new_visible`）写回落盘。会话总开关 `factions_visible` 仍留在
            // 会话段——组内眼睛是**声明轴**（跨会话），总开关是**会话轴**。
            ui.add_space(LAYER_BLOCK_GAP);
            divider(ui, &shell);
            ui.add_space(LAYER_BLOCK_GAP);
            caption_row(
                ui,
                &shell,
                t_key(&current_loc, I18nKey::PanelLayersFactionGroup),
            );
            let faction_layers = wv_core::resolved_faction_layers(&projection.world);
            if faction_layers.is_empty() {
                caption_row(
                    ui,
                    &shell,
                    t_key(&current_loc, I18nKey::PanelLayersEmptyHint),
                );
            } else {
                for layer in &faction_layers {
                    let mut visible = layer.visible;
                    // 层行：icon 占位 + 名字 + 弹性 + 眼睛 toggle（28px 行高）。
                    ui.horizontal(|ui| {
                        let resp = ui.allocate_ui(egui::vec2(ui.available_width(), 22.0), |ui| {
                            ui.horizontal(|ui| {
                                ui.add_space(18.0);
                                ui.label(
                                    egui::RichText::new(layer.name.as_str())
                                        .font(proportional(shell.scale.base))
                                        .color(shell.t2()),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.add(egui::Checkbox::new(&mut visible, "")).changed()
                                            && visible != layer.visible
                                        {
                                            commit_faction_layer_visibility(
                                                &mut projection,
                                                &mut edit,
                                                layer,
                                                visible,
                                            );
                                        }
                                    },
                                );
                            });
                        });
                        resp.response.on_hover_text(layer.id.as_str());
                    });
                }
            }

            // 自然图行（ADR-0079 政治图集页开关）：声明级落盘——当前图无
            // terrain 声明层时 `EnsureTerrainLayerCommand` 插入一行，有则改
            // visible。取代旧会话轴 Terrain toggle（会话轴不落盘，重启即忘）。
            let terrain_declared = projection
                .world
                .maps
                .iter()
                .find(|m| m.id == map_id)
                .map(|m| {
                    m.layers
                        .iter()
                        .find(|l| l.kind == wv_core::map::LayerKind::Terrain)
                        .map(|l| l.visible)
                })
                .unwrap_or(None);
            let mut natural_visible = terrain_declared.unwrap_or(true);
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), 22.0), |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.label(
                            egui::RichText::new(t_key(
                                &current_loc,
                                I18nKey::PanelLayersNaturalMap,
                            ))
                            .font(proportional(shell.scale.base))
                            .color(shell.t2()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(egui::Checkbox::new(&mut natural_visible, ""))
                                .changed()
                                && natural_visible != terrain_declared.unwrap_or(true)
                            {
                                commit_layer2(
                                    &mut projection,
                                    &mut edit,
                                    wv_edit::EnsureTerrainLayerCommand::new(
                                        map_id.clone(),
                                        natural_visible,
                                    ),
                                );
                            }
                        });
                    });
                });
            });

            // 会话开关段（v0.3 的 toggle 保留；Terrain 行已升级为上面的声明级
            // 自然图行，`terrain_soften` 留会话轴）。
            ui.add_space(LAYER_BLOCK_GAP);
            divider(ui, &shell);
            caption_row(ui, &shell, t_key(&current_loc, I18nKey::PanelLayersSession));
            egui::Grid::new("wv_layers_grid")
                .num_columns(2)
                .spacing([space::MD, 2.0])
                .show(ui, |ui| {
                    // 一次性借走 7 个互不相同的字段（先降为普通 `&mut ViewState`，
                    // borrowck 才允许不相交字段的并列可变借用——ResMut 每次 DerefMut
                    // 都是整结构体借用），按 web 顺序排 2 列。
                    let view_state: &mut ViewState = &mut view;
                    let mut flags = [
                        ("Terrain Smooth", &mut view_state.terrain_soften),
                        ("Location", &mut view_state.places_visible),
                        ("Event", &mut view_state.events_visible),
                        ("Trail", &mut view_state.tracks_visible),
                        ("Road", &mut view_state.roads_visible),
                        ("Building", &mut view_state.buildings_visible),
                        ("Faction", &mut view_state.factions_visible),
                    ];
                    for pair in flags.chunks_mut(2) {
                        for (name, flag) in pair.iter_mut() {
                            layer_cell(ui, &shell, name, flag);
                        }
                        ui.end_row();
                    }
                });
        },
    );
}

/// 面板写回的唯一通道：命令 → 事务 → 脏路径（new_map_dialog 同款配方）。
///
/// 失败只 warn 不弹窗——帧数据与命令同源，失败即世界在帧间被换掉，
/// 下一帧列表自然重算（V1：校验只提示，不阻止）。
fn commit_layer(
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    cmd: wv_edit::UpdateLayerCommand,
) {
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
        }
        Err(e) => bevy::log::warn!("图层面板提交失败: {e}"),
    }
}

/// 同 [`commit_layer`] 但收任意 `EditCommand`（`EnsureTerrainLayerCommand`
/// 等非 `UpdateLayerCommand` 家族共用）。
fn commit_layer2(
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    cmd: impl wv_edit::EditCommand + 'static,
) {
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
        }
        Err(e) => bevy::log::warn!("图层面板提交失败: {e}"),
    }
}

/// 势力层眼睛写回：`UpdateFactionLayerCommand` 是全字段快照命令——先读
/// 该层现值再改 `new_visible`（ADR-0079 T4）。
fn commit_faction_layer_visibility(
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    layer: &wv_core::FactionLayer,
    visible: bool,
) {
    // 终审 I4：upsert 命令——合成默认层不在 world.faction_layers，旧
    // UpdateFactionLayerCommand 对它 TargetMissing（眼睛翻一帧即弹回）。
    commit_layer2(
        projection,
        edit,
        wv_edit::EnsureFactionLayerVisibleCommand::new(layer.id.clone(), visible),
    );
}

/// 从当前图节点克隆图层行（名称空白 → 按 kind 取 i18n 默认名）。
fn layer_rows(world: &World, map_id: &Id, loc: Locale) -> Vec<LayerRow> {
    let Some(node) = world.maps.iter().find(|m| &m.id == map_id) else {
        return Vec::new();
    };
    node.layers
        .iter()
        .map(|l| LayerRow {
            label: if l.name.trim().is_empty() {
                kind_name(loc, &l.kind).to_owned()
            } else {
                l.name.clone()
            },
            id: l.id.clone(),
            kind: l.kind.clone(),
            visible: l.visible,
            locked: l.locked,
            opacity: l.opacity,
            count: layer_count(world, map_id, l),
        })
        .collect()
}

/// 图层行的右端计数：只统计**能从当前数据直接数出**的 kind；数不出的
/// （event_point 无地图归属、border/custom 无渲染消费者）留 `None` 不占位。
fn layer_count(world: &World, map_id: &Id, layer: &Layer) -> Option<u32> {
    match layer.kind {
        LayerKind::Terrain => world.terrain_of(map_id).map(|_| 1),
        LayerKind::Place => Some(
            world
                .distances
                .places
                .iter()
                .filter(|p| p.map.as_ref() == Some(map_id))
                .count() as u32,
        ),
        LayerKind::Road => {
            // 边不直接挂图，按「端点落在本图的 Place 集合」折算。
            let on_map: std::collections::HashSet<&Id> = world
                .distances
                .places
                .iter()
                .filter(|p| p.map.as_ref() == Some(map_id))
                .map(|p| &p.id)
                .collect();
            Some(
                world
                    .distances
                    .edges
                    .iter()
                    .filter(|e| on_map.contains(&e.a) || on_map.contains(&e.b))
                    .count() as u32,
            )
        }
        LayerKind::Label => Some(
            (world.regions.iter().filter(|r| &r.map == map_id).count()
                + world.seas.iter().filter(|s| &s.map == map_id).count()) as u32,
        ),
        LayerKind::Building => world
            .buildings
            .get(&layer.id)
            .map(|b| b.buildings.len() as u32),
        LayerKind::Influence => {
            // 本图上有几何的**去重势力数**（不是 span 数——同一势力多段算 1）。
            let mut owners: std::collections::HashSet<&Id> = std::collections::HashSet::new();
            for g in &world.faction_geometry {
                if &g.map == map_id {
                    owners.insert(&g.owner);
                }
            }
            Some(owners.len() as u32)
        }
        LayerKind::EventPoint | LayerKind::Border | LayerKind::Custom => None,
    }
}

/// LayerKind → 面板默认名（i18n；作者起了名则以作者为准）。
fn kind_name(loc: Locale, kind: &LayerKind) -> &'static str {
    let key = match kind {
        LayerKind::Terrain => I18nKey::PanelLayerTerrain,
        LayerKind::Border => I18nKey::PanelLayerBorder,
        LayerKind::Influence => I18nKey::PanelLayerInfluence,
        LayerKind::Road => I18nKey::PanelLayerRoad,
        LayerKind::Building => I18nKey::PanelLayerBuilding,
        LayerKind::EventPoint => I18nKey::PanelLayerEventPoint,
        LayerKind::Place => I18nKey::PanelLayerPlace,
        LayerKind::Label => I18nKey::PanelLayerLabel,
        LayerKind::Custom => I18nKey::PanelLayerCustom,
    };
    crate::i18n::t(loc, key)
}

/// LayerKind → 12×12 Lucide 类型图标（Figma `3:20892` 行内类型图标位）。
///
/// Place 用 map-pin（与 CellMap 地物符号同词汇）；Building 用 landmark
/// （Figma `Settlements` 位）；Custom 用 image（Figma `Reference sketch` 位）。
fn kind_icon(kind: &LayerKind) -> egui::ImageSource<'static> {
    use wv_ui::widgets as icons;
    match kind {
        LayerKind::Terrain => icons::ICON_MOUNTAIN,
        LayerKind::Border => icons::ICON_VECTOR_SQUARE,
        LayerKind::Influence => icons::ICON_FLAG,
        LayerKind::Road => icons::ICON_ROUTE,
        LayerKind::Building => icons::ICON_LANDMARK,
        LayerKind::EventPoint => icons::ICON_CALENDAR,
        LayerKind::Place => icons::ICON_MAP_PIN,
        LayerKind::Label => icons::ICON_TYPE,
        LayerKind::Custom => icons::ICON_IMAGE,
    }
}

/// 本图未声明图层时的占位卡（老工程 `layers:` 缺省是空 Vec）。
fn empty_layers(ui: &mut egui::Ui, shell: &UiShell, loc: &CurrentLocale) {
    empty_state(
        ui,
        shell,
        "≡",
        t_key(loc, I18nKey::PanelLayersEmptyTitle),
        t_key(loc, I18nKey::PanelLayersEmptyHint),
        "",
    );
}

/// 会话开关段的小节标题行（16 px，micro t3——面板小标题同款式）。
fn caption_row(ui: &mut egui::Ui, shell: &UiShell, text: &str) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 16.0), egui::Sense::hover());
    ui.painter().text(
        egui::pos2(rect.min.x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        proportional(shell.scale.micro),
        shell.t3(),
    );
}

/// 单个图层格：名称（micro text_2）+ 右侧 toggle（web `.layer-row` 高 25）。
///
/// 整格作为命中区（点名称也能切，比 web 只点 toggle 更顺手）。
fn layer_cell(ui: &mut egui::Ui, shell: &UiShell, name: &str, flag: &mut bool) {
    let cell_w = 92.0_f32.min(ui.available_width());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell_w, 25.0), egui::Sense::click());
    ui.painter().text(
        egui::pos2(rect.min.x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        proportional(shell.scale.micro),
        shell.t2(),
    );
    // toggle（web `.toggle`：25×14、圆角 10；knob 10px，on 时右移 11px 变白）
    let toggle_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 14.0, rect.center().y),
        egui::vec2(25.0, 14.0),
    );
    let painter = ui.painter();
    painter.rect_filled(
        toggle_rect,
        radius::PILL,
        if *flag {
            shell.accent()
        } else {
            shell.active_bg()
        },
    );
    let knob_x = if *flag {
        toggle_rect.min.x + 2.0 + 11.0
    } else {
        toggle_rect.min.x + 2.0
    };
    painter.circle_filled(
        egui::pos2(knob_x + 5.0, toggle_rect.center().y),
        5.0,
        if *flag {
            egui::Color32::WHITE
        } else {
            shell.t3()
        },
    );
    if resp.clicked() {
        *flag = !*flag;
    }
}
