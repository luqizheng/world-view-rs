//! 地标面板（ADR-0071）：选类别/级别 → 单击地图放置点状 `Place`（POI），
//! 并列出当前图全部点状地点供选中与删除。
//!
//! ## 职责
//! - `panel_landmark` —— 放置段（类别 / 级别 / 放置 toggle）+ 列表段。
//! - 放置 toggle 只**排队** `SelectMapTool(Landmark)`——工具互斥与开关
//!   语义全在 `dispatch` 分发器（架构边界：面板不直接改工具状态，与
//!   工具条瓷砖 `map_controls/widgets.rs` 同口径）。
//! - 删除直接组 `Transaction` 提交 `DeletePlaceCommand`（AGENTS §3.3：
//!   改世界必经 `wv-edit`；V1 不弹确认，撤销兜底）。
//!
//! ## 边界（不做什么）
//! - **不**重复属性表单——列表点击 = 选中该地点，编辑在统一 Inspector 的
//!   地点表单（ADR-0058 属性段唯一）。
//! - **不**做地图上的落点预览——即点即建的反馈走 `projection.dirty` →
//!   重投影 → POI 桶重画，放置后立即见点；wv-render 零改动。
//!
//! ## 主要导出
//! - `panel_landmark` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::edit::editing;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{DrillState, LandmarkToolState, Panel, PanelState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use crate::ui_command_palette::{CommandId, CommandPalette, MapToolId};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::float_window;
use super::widgets::{button, empty_state, icon_image, list_item, BadgeKind};
use super::widgets::{ButtonContent, ButtonStyle, ICON_TRASH};
use wv_core::distance::PoiKind;

/// 地标面板（ADR-0071）。
#[allow(clippy::too_many_arguments)]
pub fn panel_landmark(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    mut palette: ResMut<CommandPalette>,
    mut landmark: ResMut<LandmarkToolState>,
    (mut projection, drill, mut edit): (
        ResMut<WorldProjection>,
        Res<DrillState>,
        ResMut<editing::EditHistory>,
    ),
    mut inspector: ResMut<crate::ui::inspector::InspectorState>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = current_loc.0;
    let badge = if landmark.placing {
        Some((BadgeKind::Warn, "ACTIVE".into()))
    } else {
        None
    };
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::Landmark,
        badge,
        loc,
        |ui| {
            // ══ 放置段：先名词后动词（类别/级别是默认值，放置是动作）══
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{}：",
                    t_key(&current_loc, I18nKey::InspectorPlaceKind)
                ));
                // 与 Inspector 类别下拉同款语义：选「未分类」= None（Place
                // 不落 kind 行，渲染折算 Other）。id_salt 不能与 Inspector 的
                // `place_kind` 撞（同屏同 scope 同 id 会互相干扰）。
                let kind_buf = landmark.default_kind.unwrap_or(PoiKind::Other);
                egui::ComboBox::from_id_salt("landmark_kind")
                    .selected_text(t_key(
                        &current_loc,
                        super::inspector::place::poi_kind_label_key(kind_buf),
                    ))
                    .show_ui(ui, |ui| {
                        for k in PoiKind::ALL {
                            ui.selectable_value(
                                &mut landmark.default_kind,
                                if k == PoiKind::Other { None } else { Some(k) },
                                t_key(&current_loc, super::inspector::place::poi_kind_label_key(k)),
                            );
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{}：",
                    t_key(&current_loc, I18nKey::InspectorPlaceRank)
                ));
                egui::ComboBox::from_id_salt("landmark_rank")
                    .selected_text(format!("{}", landmark.default_rank))
                    .show_ui(ui, |ui| {
                        for r in 0u8..=3 {
                            ui.selectable_value(&mut landmark.default_rank, r, r.to_string());
                        }
                    });
            });
            // 放置 toggle：开关语义在 dispatch（已开 = 再点关），按钮样式
            // 随激活态换——Accent 高亮 = 正在放置。
            //
            // 宽度必须 `.min()` 钳死：egui Window 的**无界测量 pass** 里
            // `available_width()` 是屏幕级宽，直接当按钮 min_size 会把浮窗
            // 撑到全屏且缩不回来（Resize 只扩不缩，place.rs:186 同款实机
            // bug——2026-10-26「地标面板启动后无限拉大」的根因）。正常
            // pass 里 available ≤ 上限，按钮仍跟随面板宽。
            let style = if landmark.placing {
                ButtonStyle::Accent
            } else {
                ButtonStyle::Raised
            };
            let btn_w = ui.available_width().min(272.0);
            if button(
                ui,
                &shell,
                ButtonContent::Text(t_key(&current_loc, I18nKey::LandmarkPlaceToggle)),
                style,
                Some(egui::vec2(btn_w, 30.0)),
                true,
            )
            .clicked()
            {
                palette.pending = Some(CommandId::SelectMapTool(MapToolId::Landmark));
            }
            if landmark.placing {
                ui.label(
                    egui::RichText::new(t_key(&current_loc, I18nKey::LandmarkPlaceHint))
                        .small()
                        .color(shell.t3()),
                );
            }

            // ══ 列表段：当前图的点状地点 ══
            ui.add_space(crate::theme::ui_tokens::space::SM);
            ui.separator();
            ui.add_space(crate::theme::ui_tokens::space::XS);

            let Some(map_id) = drill.current().cloned() else {
                empty_state(
                    ui,
                    &shell,
                    "⬡",
                    t_key(&current_loc, I18nKey::LandmarkEmpty),
                    t_key(&current_loc, I18nKey::LandmarkEmptyHint),
                    "",
                );
                return;
            };
            let rows: Vec<(wv_core::Id, String, Option<PoiKind>, Option<u8>)> =
                editing::landmark_candidates(&projection.world, &map_id)
                    .into_iter()
                    .map(|p| (p.id.clone(), p.name.clone(), p.kind, p.rank))
                    .collect();
            if rows.is_empty() {
                // 空态动作 = 直接激活放置（省一次找按钮）。
                if empty_state(
                    ui,
                    &shell,
                    "⬡",
                    t_key(&current_loc, I18nKey::LandmarkEmpty),
                    t_key(&current_loc, I18nKey::LandmarkEmptyHint),
                    t_key(&current_loc, I18nKey::LandmarkPlaceToggle),
                ) {
                    palette.pending = Some(CommandId::SelectMapTool(MapToolId::Landmark));
                }
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                // 行数据先收集成 owned（上面借 &projection.world 的引用不能
                // 活进闭包——删除按钮要对 projection 拿 &mut）。
                for (place_id, name, kind, rank) in rows {
                    let selected = projection
                        .selection
                        .as_ref()
                        .is_some_and(|s| s.kind == wv_core::EntityKind::Place && s.id == place_id);
                    let meta = format!(
                        "{} · R{}",
                        t_key(
                            &current_loc,
                            super::inspector::place::poi_kind_label_key(
                                kind.unwrap_or(PoiKind::Other)
                            )
                        ),
                        rank.unwrap_or(1),
                    );
                    ui.horizontal(|ui| {
                        // 行宽 = 可用宽 − 删除钮；list_item 吃 available_width，
                        // 必须 allocate_ui 圈定（否则按钮被挤出界）。
                        // 同上：`.min()` 钳死无界测量 pass 的屏幕级宽（不然
                        // allocate_ui 的请求尺寸本身就撑爆浮窗）。
                        let trash_w = 26.0;
                        let list_w = (ui.available_width() - trash_w - 4.0).clamp(120.0, 242.0);
                        let resp = ui
                            .allocate_ui(egui::vec2(list_w, 28.0), |ui| {
                                list_item(ui, &shell, shell.accent(), &name, &meta, selected)
                            })
                            .inner;
                        if resp.clicked() {
                            projection.selection = Some(wv_core::SelectedEntity::new(
                                wv_core::EntityKind::Place,
                                place_id.clone(),
                            ));
                            // 点行即看到属性（与区域面板同款承诺）。
                            inspector.tab = crate::ui::inspector::InspectorTab::Properties;
                        }
                        // 删除：V1 不追问，撤销兜底（Ctrl+Z 一条命令整体回退，
                        // 含级联边）。
                        if ui
                            .add_sized(
                                egui::vec2(trash_w, 24.0),
                                egui::Button::new(icon_image(ICON_TRASH, shell.t3(), 14.0)),
                            )
                            .on_hover_text(t_key(&current_loc, I18nKey::CommonDelete))
                            .clicked()
                        {
                            delete_landmark_now(&mut projection, &mut edit, &place_id);
                        }
                    });
                }
            });
        },
    );
}

/// 删除一个地标（ADR-0071）：直提 `DeletePlaceCommand`，一次撤销整体回退。
///
/// 若删的正是当前选中实体，顺带清选——Inspector 别显示幽灵条目。
fn delete_landmark_now(
    projection: &mut WorldProjection,
    edit: &mut editing::EditHistory,
    id: &wv_core::Id,
) {
    let mut tx = wv_edit::history::Transaction::new();
    tx.push(Box::new(wv_edit::command::DeletePlaceCommand::new(
        id.clone(),
    )));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            if projection.selection.as_ref().is_some_and(|s| &s.id == id) {
                projection.selection = None;
            }
        }
        Err(e) => {
            bevy::log::warn!("地标删除失败: {e}");
        }
    }
}
