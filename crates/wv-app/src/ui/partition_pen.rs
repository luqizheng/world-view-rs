//! 势力层 pen 入口 + 新增/修改弹框（ADR-0079 D3/T11）。
//!
//! ## 流程（spec D4/D5）
//!
//! 1. 势力层列表行 **pen 按钮**（门控：当前图 World/Region ∧ 有自有地形栅格）；
//! 2. 弹框第一问：**新增**（填生效时间，预填时间轴当前日 + 起始归属可空）
//!    还是**修改**（列历史段，选段 → **移动全局时间轴**到该段）；
//! 3. 确认 → 提交 [`wv_edit::BeginPartitionSpanCommand`]（新增）或跳时间轴
//!    进入编辑态（修改，编辑态在 T12）。
//!
//! **单时间轴**（设计默认 3）：「修改」不引入第二编辑时钟——跳段就是写
//! `PlaybackState.current_day`；编辑态 HUD（T12）的「当前时间」选择器同源。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale};
use wv_render::state::{PlaybackState, ViewState, WorldProjection};

use crate::edit::editing::EditHistory;

/// pen 门控·栅格半（设计默认 2）：该图有自有地形栅格。
///
/// 图级（World/Region）半在调用方（Inspector 层行已知自身所在图级）；
/// 与意图菜单同款合取（「选区闭合」一条对笔不适用）；City / Interior 图
/// 不可分区。
pub fn pen_available_for(world: &wv_core::World, map: &wv_core::Id) -> bool {
    world.terrain_of(map).is_some()
}

/// 弹框动作（纯函数，可单测）。
#[derive(Debug, Clone, PartialEq)]
pub enum PenDecision {
    /// 新增：以给定生效日 + 起始归属开段。
    Create {
        /// 生效日（弹框预填当前日，可改）。
        from: i64,
        /// 起始归属；`None` = 无主。
        owner: Option<wv_core::Id>,
    },
    /// 修改：跳全局时间轴到该段起点。
    Modify {
        /// 选中段的 `from`。
        span_from: i64,
    },
    /// 取消 / 关闭。
    Cancel,
}

/// 「新增 / 修改」弹框决策（纯函数——UI 壳只搬运字段）。
///
/// - 新增：生效日任意（首段 / fork 的合法性由命令自检）；
/// - 修改：`span_from` 必须在该层既有段里（UI 列表只列存在段）。
pub fn decide(mode: PenMode, from: i64, owner: Option<wv_core::Id>) -> PenDecision {
    match mode {
        PenMode::Create => PenDecision::Create { from, owner },
        PenMode::Modify => PenDecision::Modify { span_from: from },
        PenMode::Off => PenDecision::Cancel,
    }
}

/// 弹框模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PenMode {
    /// 新增一段（填生效时间）。
    #[default]
    Create,
    /// 修改既有段（选段跳转）。
    Modify,
    /// 弹框未开。
    Off,
}

/// pen 弹框的跨帧状态。
#[derive(Resource, Default)]
pub struct PartitionPenDialog {
    /// 弹框开着（某层行 pen 点下）。
    pub open_for: Option<wv_core::Id>,
    /// 当前模式。
    pub mode: PenMode,
    /// 生效日输入（预填当前日）。
    pub from: i64,
    /// 起始归属（势力 id 字符串，空 = 无主）。
    pub owner: String,
}

impl PartitionPenDialog {
    /// 打开（预填当前日）。
    pub fn open(&mut self, layer: wv_core::Id, current_day: i64) {
        self.open_for = Some(layer);
        self.mode = PenMode::Create;
        self.from = current_day;
        self.owner.clear();
    }

    /// 关闭。
    pub fn close(&mut self) {
        self.open_for = None;
        self.mode = PenMode::Off;
    }
}

/// pen 弹框系统：画窗 + 处理决策（提交经 `EditCommand` 链，AGENTS §3.3）。
#[allow(clippy::too_many_arguments)]
pub fn partition_pen_dialog(
    mut contexts: EguiContexts,
    current_loc: Res<CurrentLocale>,
    mut dialog: ResMut<PartitionPenDialog>,
    mut projection: ResMut<WorldProjection>,
    view: Res<ViewState>,
    mut playback: ResMut<PlaybackState>,
    mut edit: ResMut<EditHistory>,
    mut edit_state: ResMut<PartitionEditState>,
) {
    let Some(layer_id) = dialog.open_for.clone() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = current_loc.0;
    let mut commit: Option<PenDecision> = None;
    egui::Window::new(t(loc, crate::i18n::I18nKey::PartitionPenTitle))
        .id(egui::Id::new("wv_partition_pen"))
        .order(bevy_egui::egui::Order::Foreground)
        .resizable(false)
        .collapsible(false)
        .default_pos(egui::pos2(400.0, 200.0))
        .default_size(egui::vec2(300.0, 160.0))
        .show(ctx, |ui| {
            ui.label(format!(
                "{}: {}",
                t(loc, crate::i18n::I18nKey::PartitionPenLayer),
                layer_id.as_str()
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut dialog.mode,
                    PenMode::Create,
                    t(loc, crate::i18n::I18nKey::PartitionPenCreate),
                );
                ui.selectable_value(
                    &mut dialog.mode,
                    PenMode::Modify,
                    t(loc, crate::i18n::I18nKey::PartitionPenModify),
                );
            });
            ui.add_space(8.0);
            match dialog.mode {
                PenMode::Create => {
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenEffectiveDay));
                        ui.add(egui::DragValue::new(&mut dialog.from));
                    });
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenOwner));
                        egui::TextEdit::singleline(&mut dialog.owner).show(ui);
                    });
                    ui.small(t(loc, crate::i18n::I18nKey::PartitionPenOwnerHint));
                }
                PenMode::Modify => {
                    // 历史段列表（跳段 = 移动全局时间轴）。
                    if let Some(p) = world_partition(&projection, &layer_id, &view) {
                        for span in &p.spans {
                            let label = match span.to {
                                Some(to) => format!("{} .. {}", span.from, to),
                                None => format!("{} .. ∞", span.from),
                            };
                            if ui.button(&label).clicked() {
                                dialog.from = span.from;
                            }
                        }
                    }
                    ui.small(t(loc, crate::i18n::I18nKey::PartitionPenModifyHint));
                }
                PenMode::Off => {}
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t(loc, crate::i18n::I18nKey::CommonOk)).clicked() {
                    let owner = if dialog.owner.trim().is_empty() {
                        None
                    } else {
                        Some(wv_core::Id::new_unchecked(dialog.owner.trim()))
                    };
                    commit = Some(decide(dialog.mode, dialog.from, owner));
                }
                if ui
                    .button(t(loc, crate::i18n::I18nKey::CommonCancel))
                    .clicked()
                {
                    commit = Some(PenDecision::Cancel);
                }
            });
        });

    if let Some(decision) = commit {
        match decision {
            PenDecision::Create { from, owner } => {
                let mut tx = wv_edit::Transaction::new();
                // 归属名输入 = **新建势力实体**（同 Begin 一个事务）——
                // 只挂 Id 不建实体是悬空引用，首脸渲染灰上灰（实机教训）。
                // id = `faction_<Unix 毫秒>`（faction_panel_actions 同款）。
                let owner = if let Some(name) = owner.as_ref() {
                    let ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let id = wv_core::Id::new_unchecked(format!("faction_{ms}"));
                    let palette = [
                        "#C05A4E", "#4E79C0", "#5A9E6F", "#B07FD8", "#D8A94E", "#4EB8C0",
                        "#C04E9E", "#7F8F4E",
                    ];
                    let color = wv_core::Color(
                        palette[projection.world.factions.len() % palette.len()].into(),
                    );
                    tx.push(Box::new(wv_edit::AddFactionCommand::new(
                        wv_core::Faction::new(id.clone(), name.as_str(), color),
                    )));
                    Some(id)
                } else {
                    None
                };
                let cmd = wv_edit::BeginPartitionSpanCommand::new(
                    wv_core::Id::new_unchecked(view.current_map.clone()),
                    layer_id.clone(),
                    from,
                    owner,
                );
                tx.push(Box::new(cmd));
                match edit.history.commit(&mut projection.world, tx) {
                    Ok(dirty) => {
                        edit.record_dirty(dirty);
                        projection.dirty = true;
                        // 开段成功 → 进入该段编辑态（切刀默认工具）。
                        edit_state.active = Some((layer_id.clone(), from));
                        edit_state.tool = PartitionTool::Cut;
                    }
                    Err(e) => bevy::log::warn!("分区开段失败: {e}"),
                }
            }
            PenDecision::Modify { span_from } => {
                // 修改 = 跳全局时间轴（单时钟，设计默认 3）+ 进入编辑态。
                playback.current_day = span_from;
                edit_state.active = Some((layer_id.clone(), span_from));
                edit_state.tool = PartitionTool::Cut;
            }
            PenDecision::Cancel => {}
        }
        dialog.close();
    }
}

/// 取当前图 × 层的分区（只读引用）。
fn world_partition<'w>(
    projection: &'w WorldProjection,
    layer: &wv_core::Id,
    view: &ViewState,
) -> Option<&'w wv_core::model::faction_partition::FactionPartition> {
    let map = wv_core::Id::new_unchecked(view.current_map.clone());
    projection
        .world
        .partitions
        .iter()
        .find(|p| p.map == map && &p.layer == layer)
}

/// 分区编辑态（T12 消费）：「正在编辑哪层的哪段 + 用哪把子工具」。
#[derive(Resource, Default)]
pub struct PartitionEditState {
    /// 编辑中：`(层 id, 段 from)`；`None` = 非编辑态。
    pub active: Option<(wv_core::Id, i64)>,
    /// 当前子工具。
    pub tool: PartitionTool,
}

/// 编辑态三子工具（ADR-0079 D4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PartitionTool {
    /// 切刀（边到边一笔，切后点选归属）。
    #[default]
    Cut,
    /// 拖边界（零和，T13）。
    DragBoundary,
    /// 整块易主（点脸改归属，T13）。
    Assign,
}

#[cfg(test)]
mod tests {
    //! pen 门控 + 决策纯函数（T11）。

    use super::*;

    #[test]
    fn pen_gated_off_without_own_raster() {
        let world = wv_core::World::default();
        assert!(
            !pen_available_for(&world, &wv_core::Id::new_unchecked("map_a")),
            "无自有栅格的图不可 pen（设计默认 2）"
        );
    }

    #[test]
    fn pen_gated_on_with_raster() {
        let mut world = wv_core::World::default();
        world.set_terrain(
            wv_core::Id::new_unchecked("map_a"),
            wv_core::terrain::TerrainData {
                width: 4,
                height: 4,
                cells: vec![Default::default(); 16],
            },
        );
        assert!(
            pen_available_for(&world, &wv_core::Id::new_unchecked("map_a")),
            "有自有栅格即可 pen（图级门槛在调用方）"
        );
    }

    #[test]
    fn create_prefills_current_day() {
        // 打开即预填当前日（spec D4：生效时间预填可改）。
        let mut d = PartitionPenDialog::default();
        d.open(wv_core::Id::new_unchecked("l1"), 42);
        assert_eq!(d.from, 42, "预填当前日");
        assert_eq!(d.mode, PenMode::Create, "默认新增模式");
    }

    #[test]
    fn modify_jumps_global_timeline_to_span_from() {
        // 决策 → Modify{span_from}：UI 侧写 playback.current_day（单时钟）。
        let d = decide(PenMode::Modify, 10, None);
        assert_eq!(d, PenDecision::Modify { span_from: 10 });
    }

    #[test]
    fn create_decision_carries_owner() {
        let d = decide(
            PenMode::Create,
            5,
            Some(wv_core::Id::new_unchecked("faction_a")),
        );
        assert_eq!(
            d,
            PenDecision::Create {
                from: 5,
                owner: Some(wv_core::Id::new_unchecked("faction_a"))
            }
        );
        // 空 owner 字符串 → None（无主，不造实体）。
        let d2 = decide(PenMode::Create, 5, None);
        assert_eq!(
            d2,
            PenDecision::Create {
                from: 5,
                owner: None
            }
        );
    }
}
