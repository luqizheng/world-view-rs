//! 区域面板 → 提交 `wv_edit::command` 的动作层。
//!
//! ## 职责
//!
//! 面板 UI（`region_panel.rs`）只负责**收集用户输入**与**呈现**；
//! 所有"提交 `EditCommand` 到 `EditHistory`"的动作都在这里集中。
//!
//! ## 设计依据
//!
//! 与 `faction_panel_actions.rs` 同构——参见其模块文档 §V1 守约。

#![cfg(feature = "app")]

use crate::edit::editing::EditHistory;
use crate::state::{ViewState, WorldProjection};
use wv_core::Region;
use wv_edit::command::{AddRegionCommand, EditCommand, RemoveRegionCommand, UpdateRegionCommand};
use wv_edit::history::Transaction;

/// 新建一个命名区域（空轮廓，作者后续绘制）。
pub fn add_region_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    name: String,
) {
    let id_str = format!("region_{}", current_unix_ms());
    let id = wv_core::Id::new_unchecked(&id_str);
    let map = wv_core::Id::new_unchecked(&view.current_map);
    let r = Region {
        id,
        name,
        map,
        polygon: Vec::new(),
        // ADR-0045 D3：新建的普通区域不带洞 / 不嵌套 / 无覆盖物。
        holes: Vec::new(),
        kind: wv_core::model::region::RegionKind::Area,
        parent: None,
        overlay: None,
    };
    push_cmd(world, edit, view, Box::new(AddRegionCommand::new(r)));
}

/// 删除一个命名区域。
pub fn remove_region_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: wv_core::Id,
) {
    push_cmd(world, edit, view, Box::new(RemoveRegionCommand::new(id)));
}

/// 更新区域名称 / 轮廓。
pub fn update_region_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: wv_core::Id,
    new_name: String,
    new_polygon: Vec<f32>,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(UpdateRegionCommand::new(id, new_name, new_polygon)),
    );
}

/// 内部：单条命令包成事务提交，失败时 warn 不抛。
fn push_cmd(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    cmd: Box<dyn EditCommand>,
) {
    let mut tx = Transaction::new();
    tx.push(cmd);
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            let _ = view;
        }
        Err(e) => {
            bevy::log::warn!("区域编辑失败: {e}");
        }
    }
}

fn current_unix_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
