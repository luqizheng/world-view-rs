//! 势力层 pen 入口 + 新增/修改弹框（ADR-0079 D3/T11）。
//!
//! ## 流程（spec D4/D5）
//!
//! 1. 势力层列表行 **pen 按钮**（门控：当前图 World/Region ∧ 有自有地形栅格）；
//! 2. 弹框第一问：**新增**（填生效时间，预填时间轴当前日 + 起始归属**显式
//!    选择**：无主 / 既有势力下拉 / 显式新建）还是**修改**（列历史段，选段
//!    → **移动全局时间轴**到该段）；
//! 3. 确认 → 提交 [`wv_edit::BeginPartitionSpanCommand`]（新增）或跳时间轴
//!    进入编辑态（修改，编辑态在 T12）。
//!
//! **单时间轴**（设计默认 3）：「修改」不引入第二编辑时钟——跳段就是写
//! `PlaybackState.current_day`；编辑态 HUD（T12）的「当前时间」选择器同源。
//!
//! **归属显式选择**（UX 评审 finding-1，2026-10-31）：旧实现是自由文本输入、
//! 提交时一律按「新势力名」建实体——想归属既有势力的作者必然踩出同名重复
//! 势力。现改为下拉三选（无主 / 既有 / 新建），新建才出名字框并明示「将
//! 新建实体」；无悬空引用的旧保证不变（新建 = 真建实体，同事务一次撤销）。

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

/// 起始归属的选择（纯数据，可单测）。
///
/// `Existing` 归属既有势力实体（开段不建新实体）；`New` 才新建势力实体
/// （同事务一次撤销，避免悬空引用——实机教训）。**默认 `Unclaimed`**：
/// 高语义写操作不做静默默认（同 finding-11 哲学）。
#[derive(Debug, Clone, PartialEq)]
pub enum OwnerChoice {
    /// 无主。
    Unclaimed,
    /// 归属既有势力（id 必须已在 world.factions——下拉来源即它）。
    Existing(wv_core::Id),
    /// 新建势力实体并归属之（载荷 = 新势力名）。
    New(String),
}

/// pen 弹框内的归属选择草稿（`OwnerChoice` 的 UI 态，New 带可编辑名）。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum OwnerPick {
    /// 无主（默认）。
    #[default]
    Unclaimed,
    /// 已点选的既有势力。
    Existing(wv_core::Id),
    /// 「新建势力」档（载荷 = 名字草稿；空名时确定钮禁用）。
    New(String),
}

/// 弹框动作（纯函数，可单测）。
#[derive(Debug, Clone, PartialEq)]
pub enum PenDecision {
    /// 新增：以给定生效日 + 起始归属开段。
    Create {
        /// 生效日（弹框预填当前日，可改）。
        from: i64,
        /// 起始归属（显式三选，默认无主）。
        owner: OwnerChoice,
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
pub fn decide(mode: PenMode, from: i64, owner: OwnerChoice) -> PenDecision {
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

/// 层用途解析（ADR-0081）：编辑态各手势 / 弹框按它分派语义——自然蛋糕
/// 不加势力（owner 恒空）、归属输入换群系下拉。层查不到（悬空）按政治层
/// 兜底（既有行为最小扰动）。
pub fn layer_usage(
    world: &wv_core::World,
    layer: &wv_core::Id,
) -> wv_core::model::faction::LayerUsage {
    world
        .faction_layers
        .iter()
        .find(|l| &l.id == layer)
        .map(|l| l.usage)
        .unwrap_or_default()
}

/// 参与自然蛋糕的族下标（序 = `wv_terrain::BIOME_FAMILY_ORDER` 前 6 个
/// 陆面族；水域 / 道路族不是「地块的群系」）。
pub const NATURE_LAND_FAMILIES: [u8; 6] = [0, 1, 2, 3, 4, 5];

/// pen 弹框的跨帧状态。
#[derive(Resource)]
pub struct PartitionPenDialog {
    /// 弹框开着（某层行 pen 点下）。
    pub open_for: Option<wv_core::Id>,
    /// 当前模式。
    pub mode: PenMode,
    /// 生效日输入（预填当前日）。
    pub from: i64,
    /// 起始归属选择草稿（政治层；默认无主——finding-1 显式选择）。
    pub owner_pick: OwnerPick,
    /// 初始群系族下标（自然层 Create 用；默认 1 = 草原族）。
    pub kind: u8,
}

impl Default for PartitionPenDialog {
    fn default() -> Self {
        Self {
            open_for: None,
            mode: PenMode::default(),
            from: 0,
            owner_pick: OwnerPick::default(),
            kind: 1,
        }
    }
}

impl PartitionPenDialog {
    /// 打开（预填当前日）。
    pub fn open(&mut self, layer: wv_core::Id, current_day: i64) {
        self.open_for = Some(layer);
        self.mode = PenMode::Create;
        self.from = current_day;
        self.owner_pick = OwnerPick::default();
        self.kind = 1;
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
    // 层用途（ADR-0081）：窗体与提交段共用。
    let nature =
        layer_usage(&projection.world, &layer_id) == wv_core::model::faction::LayerUsage::Nature;
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
            // 层用途分派（ADR-0081）：自然层的「起始归属」= 初始群系族下拉
            // （`nature` 在窗体闭包前算好，提交段共用）。
            match dialog.mode {
                PenMode::Create if nature => {
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenEffectiveDay));
                        ui.add(egui::DragValue::new(&mut dialog.from));
                    });
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionKindTitle));
                        let names = wv_terrain::biome::BIOME_FAMILY_NAMES_ZH;
                        egui::ComboBox::from_id_salt("wv_partition_initial_kind")
                            .selected_text(names.get(dialog.kind as usize).copied().unwrap_or("?"))
                            .show_ui(ui, |ui| {
                                for k in NATURE_LAND_FAMILIES {
                                    let label = names.get(k as usize).copied().unwrap_or("?");
                                    ui.selectable_value(&mut dialog.kind, k, label);
                                }
                            });
                    });
                    ui.small(t(loc, crate::i18n::I18nKey::PartitionKindHint));
                }
                PenMode::Create => {
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenEffectiveDay));
                        ui.add(egui::DragValue::new(&mut dialog.from));
                    });
                    // 归属 = 显式三选（finding-1）：无主 / 既有势力 / 新建。
                    // 不做静默默认——默认档是无主，作者必须点一下才归属。
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenOwner));
                        let factions = &projection.world.factions;
                        let cur_text: String = match &dialog.owner_pick {
                            OwnerPick::Unclaimed => {
                                t(loc, crate::i18n::I18nKey::PartitionPenOwnerNone).into()
                            }
                            OwnerPick::Existing(id) => factions
                                .iter()
                                .find(|f| &f.id == id)
                                .map(|f| f.name.clone())
                                .unwrap_or_else(|| id.to_string()),
                            OwnerPick::New(_) => {
                                t(loc, crate::i18n::I18nKey::PartitionPenOwnerNew).into()
                            }
                        };
                        egui::ComboBox::from_id_salt("wv_partition_pen_owner")
                            .selected_text(cur_text)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        matches!(dialog.owner_pick, OwnerPick::Unclaimed),
                                        t(loc, crate::i18n::I18nKey::PartitionPenOwnerNone),
                                    )
                                    .clicked()
                                {
                                    dialog.owner_pick = OwnerPick::Unclaimed;
                                }
                                for f in factions {
                                    let sel = matches!(&dialog.owner_pick, OwnerPick::Existing(e) if e == &f.id);
                                    if ui.selectable_label(sel, f.name.as_str()).clicked() {
                                        dialog.owner_pick = OwnerPick::Existing(f.id.clone());
                                    }
                                }
                                if ui
                                    .selectable_label(
                                        matches!(dialog.owner_pick, OwnerPick::New(_)),
                                        t(loc, crate::i18n::I18nKey::PartitionPenOwnerNew),
                                    )
                                    .clicked()
                                {
                                    dialog.owner_pick = OwnerPick::New(String::new());
                                }
                            });
                    });
                    // 新建档：名字框 + 语义明示（将真建实体，不静默）。
                    if let OwnerPick::New(name) = &mut dialog.owner_pick {
                        ui.horizontal(|ui| {
                            ui.label(t(loc, crate::i18n::I18nKey::PartitionPenOwnerNewName));
                            ui.text_edit_singleline(name);
                        });
                        ui.small(t(loc, crate::i18n::I18nKey::PartitionPenOwnerNewHint));
                    }
                }
                PenMode::Modify => {
                    // 历史段列表（finding-5）：行 = day 范围 · 脸数 · 归属（自然层
                    // = 群系族），点选高亮；确定后才跳时间轴（hint 同步措辞）。
                    if let Some(p) = world_partition(&projection, &layer_id, &view) {
                        let joiner = if loc == crate::i18n::Locale::Zh { "、" } else { ", " };
                        let faction_name = |id: &wv_core::Id| -> String {
                            projection
                                .world
                                .factions
                                .iter()
                                .find(|f| &f.id == id)
                                .map(|f| f.name.clone())
                                .unwrap_or_else(|| id.to_string())
                        };
                        for span in &p.spans {
                            let to = span
                                .to
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "∞".into());
                            // 归属摘要：去重势力名（政治层）/ 群系族名（自然层）。
                            let mut parts: Vec<String> = Vec::new();
                            if nature {
                                let mut kinds: Vec<u8> = span
                                    .faces
                                    .iter()
                                    .filter_map(|f| f.kind)
                                    .collect();
                                kinds.sort_unstable();
                                kinds.dedup();
                                let names = wv_terrain::biome::BIOME_FAMILY_NAMES_ZH;
                                for k in kinds {
                                    parts.push(
                                        names.get(k as usize).copied().unwrap_or("?").to_owned(),
                                    );
                                }
                            } else {
                                let mut owners: Vec<String> = Vec::new();
                                for f in &span.faces {
                                    if let Some(o) = &f.owner {
                                        let name = faction_name(o);
                                        if !owners.contains(&name) {
                                            owners.push(name);
                                        }
                                    }
                                }
                                if owners.is_empty() {
                                    owners
                                        .push(t(loc, crate::i18n::I18nKey::PartitionPenOwnerNone).to_owned());
                                }
                                parts = owners;
                            }
                            let mut owner_label = parts.join(joiner);
                            if parts.len() > 2 {
                                owner_label = format!("{}…", parts[..2].join(joiner));
                            }
                            let label = format!(
                                "{} .. {} · {} {} · {}",
                                span.from,
                                to,
                                span.faces.len(),
                                t(loc, crate::i18n::I18nKey::PartitionPenSpanFaces),
                                owner_label
                            );
                            let selected = dialog.from == span.from;
                            if ui.selectable_label(selected, label).clicked() {
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
                // 新建档空名 = 未完成输入：确定禁用（防误建无名势力）。
                let ok_enabled = !matches!(&dialog.owner_pick, OwnerPick::New(n) if n.trim().is_empty());
                if ui
                    .add_enabled(
                        ok_enabled,
                        egui::Button::new(t(loc, crate::i18n::I18nKey::CommonOk)),
                    )
                    .clicked()
                {
                    let owner = match &dialog.owner_pick {
                        OwnerPick::Unclaimed => OwnerChoice::Unclaimed,
                        OwnerPick::Existing(id) => OwnerChoice::Existing(id.clone()),
                        OwnerPick::New(name) => OwnerChoice::New(name.trim().to_string()),
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
            PenDecision::Create { from, owner } if nature => {
                // 自然层（ADR-0081）：开段带初始群系族，不建势力实体。
                let cmd = wv_edit::BeginPartitionSpanCommand::new_nature(
                    wv_core::Id::new_unchecked(view.current_map.clone()),
                    layer_id.clone(),
                    from,
                    dialog.kind,
                );
                let mut tx = wv_edit::Transaction::new();
                tx.push(Box::new(cmd));
                match edit.history.commit(&mut projection.world, tx) {
                    Ok(dirty) => {
                        edit.record_dirty(dirty);
                        projection.dirty = true;
                        edit_state.active = Some((layer_id.clone(), from));
                        edit_state.tool = PartitionTool::Cut;
                    }
                    Err(e) => bevy::log::warn!("自然层开段失败: {e}"),
                }
                let _ = owner; // 自然层无归属语义（decide 恒传 None）。
            }
            PenDecision::Create { from, owner } => {
                let mut tx = wv_edit::Transaction::new();
                // 归属三选（finding-1）：无主 / 既有（直接挂 Id，不建实体）/
                // 新建 = **真建势力实体**（同 Begin 一个事务）——只挂 Id 不建
                // 实体是悬空引用，首脸渲染灰上灰（实机教训）。
                // id = `faction_<Unix 毫秒>`（faction_panel_actions 同款）。
                let owner = match owner {
                    OwnerChoice::Unclaimed => None,
                    OwnerChoice::Existing(id) => Some(id),
                    OwnerChoice::New(name) => {
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
                    }
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
///
/// `Default` 手写而非 derive：`show_boundaries` 缺省 **true**（红圈是作者
/// 要先看见「能切的地方」的引导，2026-10-28 改可选时保默认开）。
#[derive(Resource)]
pub struct PartitionEditState {
    /// 编辑中：`(层 id, 段 from)`；`None` = 非编辑态。
    pub active: Option<(wv_core::Id, i64)>,
    /// 当前子工具。
    pub tool: PartitionTool,
    /// 边界红圈（编辑态常显的当前层脸环闪烁红线）是否显示——2026-10-28
    /// 由常显改可选（HUD 勾选框，会话级不落盘）。
    pub show_boundaries: bool,
    /// 贴近群系（[ADR-0090] D5/M4）：切刀采样折线逐点吸附群系锋线
    /// （`biome_snap::BiomeSnapIndex`），切出的脸边界贴自然地貌。
    /// **默认 true**（D5「默认勾」；关 = 自由切，现状行为）。
    pub snap_to_biome: bool,
}

impl Default for PartitionEditState {
    fn default() -> Self {
        Self {
            active: None,
            tool: PartitionTool::default(),
            show_boundaries: true,
            snap_to_biome: true,
        }
    }
}

/// 疆域替换会话态（[ADR-0090] D4/M3）。
///
/// `Default` 手写而非 derive：`biome_gate` 缺省 **true**（D4「群系 checkbox
/// 默认勾」；`bool::default()` 是 false——同 `LayerFrame` 教训）。
#[derive(Resource)]
pub struct PartitionReplaceState {
    /// 目标势力（HUD 下拉选；`None` = 未选，替换按钮禁用）。
    pub target: Option<wv_core::Id>,
    /// 群系闸门（D4）：勾选 = 点脸可选；不勾 = 点脸不选（自由态）。
    pub biome_gate: bool,
    /// 多选脸集（单击 = 重置为单脸；Ctrl 单击 = 切换累加）。
    pub picked: Vec<u32>,
}

impl Default for PartitionReplaceState {
    fn default() -> Self {
        Self {
            target: None,
            biome_gate: true,
            picked: Vec::new(),
        }
    }
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
    /// 疆域替换（[ADR-0090] D4/M3，政治层专用）：Ctrl 单击多选脸 → 一次
    /// 替换目标势力疆域（多脸并集写入，原持有不在名单的脸转无主）。
    Replace,
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
        let d = decide(PenMode::Modify, 10, OwnerChoice::Unclaimed);
        assert_eq!(d, PenDecision::Modify { span_from: 10 });
    }

    #[test]
    fn create_decision_carries_owner() {
        let d = decide(
            PenMode::Create,
            5,
            OwnerChoice::Existing(wv_core::Id::new_unchecked("faction_a")),
        );
        assert_eq!(
            d,
            PenDecision::Create {
                from: 5,
                owner: OwnerChoice::Existing(wv_core::Id::new_unchecked("faction_a"))
            }
        );
        // 无主不造实体；新建携带名字（提交侧才建实体）。
        let d2 = decide(PenMode::Create, 5, OwnerChoice::Unclaimed);
        assert_eq!(
            d2,
            PenDecision::Create {
                from: 5,
                owner: OwnerChoice::Unclaimed
            }
        );
        let d3 = decide(PenMode::Create, 5, OwnerChoice::New("王国".into()));
        assert_eq!(
            d3,
            PenDecision::Create {
                from: 5,
                owner: OwnerChoice::New("王国".into())
            }
        );
    }

    #[test]
    fn owner_pick_defaults_to_unclaimed_on_open() {
        // finding-1：打开弹框不预选任何归属（无静默默认）。
        let mut d = PartitionPenDialog::default();
        d.open(wv_core::Id::new_unchecked("l1"), 7);
        assert_eq!(d.owner_pick, OwnerPick::Unclaimed, "默认无主");
        assert_eq!(d.from, 7, "预填当前日");
    }
}
