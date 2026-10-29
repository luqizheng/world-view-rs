//! 地图树面板（2026-10-27 地图树增强 spec：真数据 / 右键复制粘贴删除 /
//! 垃圾桶 / 新增按钮）。
//!
//! ## 职责
//! - `panel_map_tree` —— 读 `World.maps` 的**存活森林**（多世界版本并列），
//!   当前图高亮；点击切换（`DrillState::drill_to` 既有链路）。
//! - 右键菜单：复制（进 `MapClipboard`）/ 粘贴（层级门控 + 仅当前打开的图）/
//!   删除（`TrashMapCommand` 软删整棵子树；最后一张根图守卫置灰）。
//! - 工具栏：`+` 新增（打开 `NewMapDialog`）；世界图源「粘贴为新根」。
//! - 垃圾桶分区（`trash_section`）：恢复 / 彻底删除（直应用，豁免撤销）/
//!   清空（两步行内确认）。
//!
//! ## 边界
//! - **不直接改世界**——一律 `EditCommand` → `History`（§3.3，`commit_map` 配方）；
//!   唯独「彻底删除/清空」走 `apply_detached`（spec §10 追记 2）。
//! - 剪贴板（`MapClipboard`）是**会话态** Resource，不落盘（P2/P6/P7 无涉）。
//! - 粘贴落点 = 目标图（= 当前图）视野中心换算的本图局部坐标；坐标链走
//!   `wv_core::resolve_frame_in` 单一实现点，不手搓。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashSet;

use wv_core::map::MapLevel;
use wv_core::{Id, World};

use crate::edit::editing::EditHistory;
use crate::i18n::{t_key, CurrentLocale, I18nKey};
use crate::state::{DrillState, Panel, PanelState, ViewState, WorldProjection};
use crate::theme::ui_theme::{proportional, UiShellConfig};
use crate::ui_layout::LayoutFile;

use super::shell::UiShell;
use super::widgets::float_window;
use wv_ui::widgets::{icon_button, ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_PASTE, ICON_PLUS};

/// 地图树会话内 UI 状态（不落盘、不属于世界数据，P7 不受影响）。
#[derive(Resource, Default)]
pub struct MapTreeUiState {
    /// 收起的节点（缺省全展开）。
    pub collapsed: HashSet<Id>,
    /// 选中行（树高亮之外的选择语义，垃圾桶恢复等场景用）。
    pub selected: Option<Id>,
    /// 垃圾桶分区展开态。
    pub trash_open: bool,
    /// 两步行内确认：武装中的「彻底删除」条目。
    pub purge_armed: Option<Id>,
    /// 两步行内确认：「清空垃圾桶」是否武装。
    pub empty_armed: bool,
}

/// 复制粘贴的会话剪贴板：源图 ID。跨图搬运 = 复制 → 切到目标图 → 粘贴。
#[derive(Resource, Default)]
pub struct MapClipboard {
    pub source: Option<Id>,
}

/// 垃圾桶条目快照。
pub(crate) struct TrashEntry {
    pub id: Id,
    pub name: String,
    pub trashed_at: String,
}

/// 行快照（进闭包前克隆，规避闭包内与世界借用打架——同图层面板 `LayerRow`）。
struct TreeRow {
    id: Id,
    name: String,
    level: MapLevel,
    depth: usize,
    has_children: bool,
    is_current: bool,
}

/// 面板内收集的动作，闭包外统一落地（提交命令 / 切图）。
enum TreeAction {
    Switch(Id),
    ToggleCollapse(Id),
    Copy(Id),
    PasteInto(Id),
    PasteAsRoot,
    Trash(Id),
    OpenNewMap,
    ToggleTrash,
    Restore(Id),
    Purge(Id),
    EmptyTrash,
}

/// 存活森林 → 展示行（深度优先；collapsed 挡住的后代不出现）。
fn collect_rows(world: &World, current: &Id, collapsed: &HashSet<Id>) -> Vec<TreeRow> {
    fn walk(
        world: &World,
        id: &Id,
        depth: usize,
        current: &Id,
        collapsed: &HashSet<Id>,
        out: &mut Vec<TreeRow>,
    ) {
        let Some(node) = world.maps.iter().find(|m| &m.id == id) else {
            return;
        };
        let children: Vec<Id> = node
            .children
            .iter()
            .filter(|c| world.map_alive(c))
            .cloned()
            .collect();
        out.push(TreeRow {
            id: id.clone(),
            name: node.name.clone(),
            level: node.level,
            depth,
            has_children: !children.is_empty(),
            is_current: id == current,
        });
        if collapsed.contains(id) {
            return;
        }
        for child in children {
            walk(world, &child, depth + 1, current, collapsed, out);
        }
    }
    let mut out = Vec::new();
    for root in world.alive_roots() {
        walk(world, &root, 0, current, collapsed, &mut out);
    }
    out
}

/// 垃圾桶条目：被删根节点，按删除时间降序（新的在上）。
pub(crate) fn trash_entries(world: &World) -> Vec<TrashEntry> {
    let mut entries: Vec<TrashEntry> = world
        .maps
        .iter()
        .filter(|m| m.trashed)
        .map(|m| TrashEntry {
            id: m.id.clone(),
            name: m.name.clone(),
            trashed_at: m.trashed_at.clone().unwrap_or_default(),
        })
        .collect();
    entries.sort_by(|a, b| b.trashed_at.cmp(&a.trashed_at));
    entries
}

/// 粘贴菜单门控（spec §6）：层级匹配 + 只对**当前打开的图**生效
/// （视野中心才有定义）；世界图源不进右键，走工具栏「粘贴为新根」。
fn paste_menu_enabled(source: MapLevel, row: MapLevel, row_is_current: bool) -> bool {
    if !row_is_current {
        return false;
    }
    match source {
        MapLevel::World => false,
        MapLevel::Region | MapLevel::City => row == MapLevel::World,
        MapLevel::Interior => row == MapLevel::City,
    }
}

/// 删除菜单门控：最后一张存活根图不可删（工程至少留一张地图）。
fn delete_menu_enabled(world: &World, id: &Id) -> bool {
    match world.maps.iter().find(|m| &m.id == id) {
        Some(node) => !(node.parent.is_none() && world.alive_roots().len() <= 1),
        None => false,
    }
}

/// 删除后的自动切换兜底根（spec §6「删除当前图 → 自动切回第一张世界图」）。
///
/// **必须排除被删的图本身**（终审 I1）：`alive_roots().first()` 在提交删除
/// 前取，被删根图自己往往就是第一项——拿它兜底等于哪儿都不切。
fn fallback_root_after_trash(world: &World, trashing: &Id) -> Option<Id> {
    world.alive_roots().into_iter().find(|r| r != trashing)
}

/// 恢复菜单门控（spec §6 嵌套守卫）：原父图存活才放行。
fn restore_menu_enabled(world: &World, id: &Id) -> bool {
    match world.maps.iter().find(|m| &m.id == id) {
        Some(node) => match &node.parent {
            Some(p) => world.map_alive(p),
            None => true,
        },
        None => false,
    }
}

/// 现在时刻的 epoch 秒字符串——`trashed_at` 的落盘形态。
///
/// **裁定（2026-10-27）**：std 无时区/日历，写 UTC 时刻串会差 8 小时；
/// 存 epoch 秒、显示层换算相对时间（[`trash_time_label`]），零时区问题。
fn epoch_now_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// `trashed_at`（epoch 秒串）→ 相对时间标签（垃圾桶条目显示用）。
pub(crate) fn trash_time_label(trashed_at: &str) -> String {
    let Ok(secs) = trashed_at.parse::<u64>() else {
        return trashed_at.to_owned();
    };
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    match at.elapsed() {
        Ok(d) => {
            let s = d.as_secs();
            if s < 60 {
                "刚刚".into()
            } else if s < 3600 {
                format!("{} 分钟前", s / 60)
            } else if s < 86400 {
                format!("{} 小时前", s / 3600)
            } else {
                format!("{} 天前", s / 86400)
            }
        }
        Err(_) => "刚刚".into(),
    }
}

/// 面板写回的唯一通道（new_map_dialog / 图层面板同款配方）。
fn commit_map(
    projection: &mut WorldProjection,
    edit: &mut EditHistory,
    cmd: impl wv_edit::EditCommand + 'static,
) -> bool {
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            true
        }
        Err(e) => {
            bevy::log::warn!("地图树提交失败: {e}");
            false
        }
    }
}

/// 工具栏行：`+` 新增；世界图源的「粘贴为新根」。
fn toolbar(
    ui: &mut egui::Ui,
    shell: &UiShell,
    clipboard_level: Option<MapLevel>,
    actions: &mut Vec<TreeAction>,
) {
    ui.horizontal(|ui| {
        // 走 wv-ui 目录件 `icon_button`：图标 tint 随日/夜令牌（t1/禁用灰），
        // 手写 `egui::Image` 漏 tint 会落回默认 WHITE——日间浅底上图标隐形
        // （2026-10-29 用户报告的日间模式 bug）。
        let plus = icon_button(ui, shell, ICON_PLUS, true);
        if plus.clicked() {
            actions.push(TreeAction::OpenNewMap);
        }
        // 世界图作源：粘贴 = 新独立根（多版本世界的生成路径，spec §6）。
        let world_src = clipboard_level == Some(MapLevel::World);
        let paste = icon_button(ui, shell, ICON_PASTE, world_src);
        if world_src && paste.clicked() {
            actions.push(TreeAction::PasteAsRoot);
        }
    });
}

/// 单行树节点（27px 高；当前图 accent 18% 高亮；chevron 可折叠）。
/// 返回整行 `Response`——右键菜单挂在它上面（调用方 [`row_context_menu`]）。
fn tree_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    row: &TreeRow,
    row_collapsed: bool,
    actions: &mut Vec<TreeAction>,
) -> egui::Response {
    let row_h = 27.0;
    let indent = 18.0 * row.depth as f32;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::click(),
    );
    {
        let painter = ui.painter();
        if row.is_current {
            painter.rect_filled(rect, 5.0, shell.accent().linear_multiply(0.18));
        } else if resp.hovered() {
            painter.rect_filled(rect, 5.0, shell.hover_bg());
        }
    }
    let icon_x = rect.min.x + 6.0 + indent;
    // 行首：有子图 = 折叠 chevron（可点）；无子图 = 留白。
    if row.has_children {
        let chevron_rect = egui::Rect::from_min_size(
            egui::pos2(icon_x, rect.center().y - 6.0),
            egui::vec2(12.0, 12.0),
        );
        let chev = ui.put(
            chevron_rect,
            egui::Image::new(if row_collapsed {
                ICON_CHEVRON_RIGHT
            } else {
                ICON_CHEVRON_DOWN
            })
            .tint(shell.t3())
            .fit_to_exact_size(egui::vec2(12.0, 12.0)),
        );
        if chev.clicked() {
            actions.push(TreeAction::ToggleCollapse(row.id.clone()));
        }
    }
    let text_x = icon_x + 18.0;
    ui.painter().text(
        egui::pos2(text_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &row.name,
        proportional(shell.scale.base),
        if row.is_current {
            shell.t1()
        } else {
            shell.t2()
        },
    );
    if resp.clicked() {
        actions.push(TreeAction::Switch(row.id.clone()));
    }
    resp
}

/// 世界树面板。
#[allow(clippy::too_many_arguments)]
pub fn panel_map_tree(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    layouts: Res<LayoutFile>,
    mut panels: ResMut<PanelState>,
    view: Res<ViewState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut ui_state: ResMut<MapTreeUiState>,
    mut clipboard: ResMut<MapClipboard>,
    mut drill: ResMut<DrillState>,
    mut dialog: ResMut<crate::ui::NewMapDialog>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let current = Id::new_unchecked(view.current_map.clone());
    let rows = collect_rows(&projection.world, &current, &ui_state.collapsed);
    let clipboard_level = clipboard
        .source
        .as_ref()
        .and_then(|s| projection.world.maps.iter().find(|m| &m.id == s))
        .map(|m| m.level);
    let mut actions: Vec<TreeAction> = Vec::new();
    float_window(
        ctx,
        &shell,
        &layouts,
        &mut panels,
        Panel::MapTree,
        None,
        current_loc.0,
        |ui| {
            toolbar(ui, &shell, clipboard_level, &mut actions);
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("wv_map_tree_rows")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for row in &rows {
                        let resp = tree_row(
                            ui,
                            &shell,
                            row,
                            ui_state.collapsed.contains(&row.id),
                            &mut actions,
                        );
                        row_context_menu(
                            &resp,
                            &current_loc,
                            row,
                            clipboard_level,
                            &projection.world,
                            &mut actions,
                        );
                    }
                });
            ui.add_space(6.0);
            wv_ui::widgets::divider(ui, &shell);
            ui.add_space(6.0);
            trash_section(
                ui,
                &shell,
                &current_loc,
                &projection.world,
                &mut ui_state,
                &mut actions,
            );
        },
    );

    // —— 闭包外统一落地：提交命令 / 切图（借用干净，同帧生效下一帧可见）——
    for action in actions {
        match action {
            TreeAction::Switch(id) => {
                drill.drill_to(&projection.world, &id);
            }
            TreeAction::ToggleCollapse(id) => {
                if !ui_state.collapsed.remove(&id) {
                    ui_state.collapsed.insert(id);
                }
            }
            TreeAction::Copy(id) => {
                ui_state.selected = Some(id.clone());
                clipboard.source = Some(id);
            }
            TreeAction::PasteInto(target) => {
                let Some(source) = clipboard.source.clone() else {
                    continue;
                };
                // 视野中心 → 目标图局部坐标（resolve_frame_in 单一实现点）。
                let anchor = wv_core::resolve_frame_in(&projection.world.maps, &target)
                    .map(|frame| {
                        let local = view.camera_center
                            - bevy::math::Vec2::new(frame.world_origin.x, frame.world_origin.y);
                        wv_core::Point::new(local.x, local.y)
                    })
                    .unwrap_or(wv_core::Point::new(0.0, 0.0));
                let cmd = wv_edit::PasteMapCommand::new(source, Some(target.clone()), anchor);
                if commit_map(&mut projection, &mut edit, cmd.clone()) {
                    if let Some(root) = cmd.created_root() {
                        drill.drill_to(&projection.world, &root);
                    }
                }
            }
            TreeAction::PasteAsRoot => {
                let Some(source) = clipboard.source.clone() else {
                    continue;
                };
                let cmd =
                    wv_edit::PasteMapCommand::new(source, None, wv_core::Point::new(0.0, 0.0));
                if commit_map(&mut projection, &mut edit, cmd.clone()) {
                    if let Some(root) = cmd.created_root() {
                        // 独立新根：settle_on（trail 只含新图），不能 enter。
                        drill.settle_on(root);
                    }
                }
            }
            TreeAction::Trash(id) => {
                // 删除的子树若含当前图 → 先切到幸存根（排除被删者），再提交删除。
                let subtree = projection.world.subtree_ids(&id);
                if subtree.contains(&current) {
                    if let Some(first) = fallback_root_after_trash(&projection.world, &id) {
                        // 幸存根可能是兄弟根（钻取轨迹到不了）→ drill_to 失败
                        // 就 settle_on 直接落地。
                        if !drill.drill_to(&projection.world, &first.clone()) {
                            drill.settle_on(first);
                        }
                    }
                }
                commit_map(
                    &mut projection,
                    &mut edit,
                    wv_edit::TrashMapCommand::new(id, epoch_now_string()),
                );
            }
            TreeAction::OpenNewMap => {
                dialog.open_for(&projection.world);
            }
            TreeAction::ToggleTrash => {
                ui_state.trash_open = !ui_state.trash_open;
                ui_state.purge_armed = None;
                ui_state.empty_armed = false;
            }
            TreeAction::Restore(id) => {
                // 恢复走正常 Undo 栈（可撤销的恢复，spec §6）。
                commit_map(
                    &mut projection,
                    &mut edit,
                    wv_edit::RestoreMapCommand::new(id),
                );
            }
            TreeAction::Purge(id) => {
                // 彻底删除：直应用通道，不进 Undo 栈（spec §10 追记 2）。
                let cmd = wv_edit::PurgeMapCommand::new(id);
                if edit
                    .apply_detached(&mut projection.world, Box::new(cmd))
                    .is_err()
                {
                    bevy::log::warn!("彻底删除失败");
                } else {
                    projection.dirty = true;
                }
            }
            TreeAction::EmptyTrash => {
                // 逐条直应用（快照当下垃圾桶清单，边删边查会漂移）。
                let ids: Vec<Id> = projection
                    .world
                    .maps
                    .iter()
                    .filter(|m| m.trashed)
                    .map(|m| m.id.clone())
                    .collect();
                for id in ids {
                    let cmd = wv_edit::PurgeMapCommand::new(id.clone());
                    if edit
                        .apply_detached(&mut projection.world, Box::new(cmd))
                        .is_err()
                    {
                        bevy::log::warn!("清空垃圾桶失败: {}", id.as_str());
                    }
                }
                projection.dirty = true;
            }
        }
    }
}

/// 垃圾桶分区（spec §5 D6）：头行（标题 + 计数 + 清空钮）+ 条目列表
/// （名字 + 相对时间；条目右键 恢复 / 彻底删除；均两步行内确认）。
fn trash_section(
    ui: &mut egui::Ui,
    shell: &UiShell,
    loc: &CurrentLocale,
    world: &World,
    ui_state: &mut MapTreeUiState,
    actions: &mut Vec<TreeAction>,
) {
    let entries = trash_entries(world);
    // 头行（点击展开/收起）。
    let (header_rect, header) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
    let painter = ui.painter();
    painter.text(
        egui::pos2(header_rect.min.x, header_rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!(
            "{} ({})",
            t_key(loc, I18nKey::MapTreeTrashSection),
            entries.len()
        ),
        proportional(shell.scale.micro),
        shell.t3(),
    );
    // 头行右端：清空按钮（两步确认）。
    let empty_label = if ui_state.empty_armed {
        t_key(loc, I18nKey::MapTreeEmptyTrashConfirmText).to_owned()
    } else {
        t_key(loc, I18nKey::MapTreeActionEmptyTrash).to_owned()
    };
    let btn_rect = egui::Rect::from_min_max(
        egui::pos2(header_rect.max.x - 104.0, header_rect.min.y + 2.0),
        egui::pos2(header_rect.max.x, header_rect.max.y - 2.0),
    );
    let empty_resp = ui.put(btn_rect, egui::Button::new(empty_label).small());
    let empty_resp = if entries.is_empty() {
        empty_resp.on_disabled_hover_text(t_key(loc, I18nKey::MapTreeTrashEmpty))
    } else {
        empty_resp
    };
    if empty_resp.clicked() {
        if ui_state.empty_armed {
            actions.push(TreeAction::EmptyTrash);
            ui_state.empty_armed = false;
        } else {
            ui_state.empty_armed = true;
        }
    }
    if header.clicked() {
        actions.push(TreeAction::ToggleTrash);
    }
    if !ui_state.trash_open {
        return;
    }
    if entries.is_empty() {
        let (next_rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(next_rect.min.x + 12.0, next_rect.center().y),
            egui::Align2::LEFT_CENTER,
            t_key(loc, I18nKey::MapTreeTrashEmpty),
            proportional(shell.scale.micro),
            shell.t3(),
        );
        return;
    }
    for entry in &entries {
        let (row_rect, row) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
        let painter = ui.painter();
        painter.text(
            egui::pos2(row_rect.min.x + 12.0, row_rect.center().y),
            egui::Align2::LEFT_CENTER,
            &entry.name,
            proportional(shell.scale.micro),
            shell.t2(),
        );
        painter.text(
            egui::pos2(row_rect.max.x - 6.0, row_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            trash_time_label(&entry.trashed_at),
            proportional(shell.scale.micro),
            shell.t3(),
        );
        let _ = painter;
        row.context_menu(|ui| {
            let restore_ok = restore_menu_enabled(world, &entry.id);
            let restore = ui.add_enabled(
                restore_ok,
                egui::Button::new(t_key(loc, I18nKey::MapTreeActionRestore).to_owned()),
            );
            if !restore_ok {
                restore.on_disabled_hover_text(t_key(loc, I18nKey::MapTreeRestoreBlockedHint));
            } else if restore.clicked() {
                actions.push(TreeAction::Restore(entry.id.clone()));
            }
            let armed = ui_state.purge_armed.as_ref() == Some(&entry.id);
            let purge_label = if armed {
                t_key(loc, I18nKey::MapTreePurgeConfirmText).to_owned()
            } else {
                t_key(loc, I18nKey::MapTreeActionPurge).to_owned()
            };
            let purge = ui.add(egui::Button::new(purge_label));
            if purge.clicked() {
                if armed {
                    actions.push(TreeAction::Purge(entry.id.clone()));
                    ui_state.purge_armed = None;
                } else {
                    ui_state.purge_armed = Some(entry.id.clone());
                }
            }
        });
    }
}

/// 行右键菜单（挂在整行 `Response` 上：复制 / 粘贴 / 删除）。
///
/// 门控：粘贴按层级 + 仅当前图（置灰不隐藏，作者看得见能力）；删除在
/// 最后一张根图上置灰并给 `MapTreeLastWorldHint` 悬浮说明。
#[allow(clippy::too_many_arguments)]
fn row_context_menu(
    resp: &egui::Response,
    loc: &CurrentLocale,
    row: &TreeRow,
    clipboard_level: Option<MapLevel>,
    world: &World,
    actions: &mut Vec<TreeAction>,
) {
    resp.context_menu(|ui| {
        if ui
            .button(t_key(loc, I18nKey::MapTreeActionCopy).to_owned())
            .clicked()
        {
            actions.push(TreeAction::Copy(row.id.clone()));
        }
        let paste_ok =
            clipboard_level.is_some_and(|src| paste_menu_enabled(src, row.level, row.is_current));
        if ui
            .add_enabled(
                paste_ok,
                egui::Button::new(t_key(loc, I18nKey::MapTreeActionPaste).to_owned()),
            )
            .clicked()
        {
            actions.push(TreeAction::PasteInto(row.id.clone()));
        }
        let delete_ok = delete_menu_enabled(world, &row.id);
        let delete_btn = ui.add_enabled(
            delete_ok,
            egui::Button::new(t_key(loc, I18nKey::MapTreeActionDelete).to_owned()),
        );
        if !delete_ok {
            delete_btn.on_disabled_hover_text(t_key(loc, I18nKey::MapTreeLastWorldHint));
        } else if delete_btn.clicked() {
            actions.push(TreeAction::Trash(row.id.clone()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: Option<&str>, level: MapLevel) -> wv_core::map::MapNode {
        wv_core::map::MapNode {
            id: Id::new_unchecked(id),
            name: id.into(),
            level,
            parent: parent.map(Id::new_unchecked),
            ..Default::default()
        }
    }

    #[test]
    fn paste_menu_enabled_matches_level_and_current_map() {
        // 城市源 + 世界行 + 当前 → 通。
        assert!(paste_menu_enabled(MapLevel::City, MapLevel::World, true));
        // 城市源 + 世界行 + 非当前 → 置灰（视野中心无定义）。
        assert!(!paste_menu_enabled(MapLevel::City, MapLevel::World, false));
        // 内部源 + 世界行 → 层级不匹配。
        assert!(!paste_menu_enabled(
            MapLevel::Interior,
            MapLevel::World,
            true
        ));
        // 内部源 + 城市行 + 当前 → 通。
        assert!(paste_menu_enabled(MapLevel::Interior, MapLevel::City, true));
        // 世界源不进右键（走工具栏新根粘贴）。
        assert!(!paste_menu_enabled(MapLevel::World, MapLevel::World, true));
    }

    #[test]
    fn delete_menu_enabled_blocks_last_alive_root() {
        let mut w = World {
            maps: vec![node("world_solo", None, MapLevel::World)],
            ..World::default()
        };
        assert!(
            !delete_menu_enabled(&w, &Id::new_unchecked("world_solo")),
            "最后一张根图不可删"
        );
        w.maps.push(node("world_b", None, MapLevel::World));
        assert!(delete_menu_enabled(&w, &Id::new_unchecked("world_solo")));
        // 子图不受「最后一张」守卫限制。
        w.maps
            .push(node("city_a", Some("world_solo"), MapLevel::City));
        assert!(delete_menu_enabled(&w, &Id::new_unchecked("city_a")));
    }

    #[test]
    fn trash_entries_list_trashed_roots_newest_first() {
        let mut a = node("city_a", Some("world"), MapLevel::City);
        a.trashed = true;
        a.trashed_at = Some("100".into());
        let mut b = node("city_b", Some("world"), MapLevel::City);
        b.trashed = true;
        b.trashed_at = Some("200".into());
        let w = World {
            maps: vec![node("world", None, MapLevel::World), a, b],
            ..World::default()
        };
        let entries = trash_entries(&w);
        assert_eq!(entries.len(), 2, "got {}", entries.len());
        assert_eq!(entries[0].id.as_str(), "city_b", "新的在上");
        assert_eq!(entries[0].trashed_at, "200");
    }

    /// 两步确认状态转移：`(是否触发, 新武装态)`。
    /// 第一击只武装；武装后的第二击触发并复位。
    fn two_step_click(armed: bool) -> (bool, bool) {
        if armed {
            (true, false)
        } else {
            (false, true)
        }
    }

    #[test]
    fn fallback_root_excludes_the_map_being_trashed() {
        let mut w = World {
            maps: vec![
                node("world_a", None, MapLevel::World),
                node("world_b", None, MapLevel::World),
            ],
            ..World::default()
        };
        // 删 A → 兜底 B；删 B → 兜底 A（I1：first 永远不该是被删图自己）。
        assert_eq!(
            fallback_root_after_trash(&w, &Id::new_unchecked("world_a"))
                .expect("应有兜底")
                .as_str(),
            "world_b"
        );
        assert_eq!(
            fallback_root_after_trash(&w, &Id::new_unchecked("world_b"))
                .expect("应有兜底")
                .as_str(),
            "world_a"
        );
        // 删子图：任何存活根都行（首根即可），不受排除影响。
        w.maps.push(node("city_x", Some("world_a"), MapLevel::City));
        assert_eq!(
            fallback_root_after_trash(&w, &Id::new_unchecked("city_x"))
                .expect("应有兜底")
                .as_str(),
            "world_a"
        );
        // 单根删自己 → None（命令层本来就拒绝，这里是纯函数契约）。
        let solo = World {
            maps: vec![node("world_solo", None, MapLevel::World)],
            ..World::default()
        };
        assert!(fallback_root_after_trash(&solo, &Id::new_unchecked("world_solo")).is_none());
    }

    #[test]
    fn purge_uses_two_step_confirmation_state_machine() {
        // 第一击：只武装，不触发。
        let (fired, armed) = two_step_click(false);
        assert!(!fired, "第一次点击不得直接触发");
        assert!(armed, "第一次点击应武装");
        // 第二击：触发并复位。
        let (fired, armed) = two_step_click(armed);
        assert!(fired, "武装后的第二次点击应触发");
        assert!(!armed, "触发后复位");
        // 再来一轮（第三击 = 新的第一击）。
        let (fired, armed) = two_step_click(armed);
        assert!(!fired && armed, "复位后重新开始两步");
    }

    #[test]
    fn restore_menu_disabled_while_parent_trashed() {
        let mut world = node("world", None, MapLevel::World);
        world.trashed = true;
        let mut city = node("city_a", Some("world"), MapLevel::City);
        city.trashed = true;
        let w = World {
            maps: vec![node("world_b", None, MapLevel::World), world, city],
            ..World::default()
        };
        assert!(
            !restore_menu_enabled(&w, &Id::new_unchecked("city_a")),
            "父图在垃圾桶 → 恢复置灰"
        );
        assert!(restore_menu_enabled(&w, &Id::new_unchecked("world")));
    }
}
