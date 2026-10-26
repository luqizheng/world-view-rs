//! 势力面板 → 提交 `wv_edit::command` 的动作层。
//!
//! ## 职责
//!
//! 面板 UI（`ui.rs::panel_faction`）只负责**收集用户输入**与**呈现**；
//! 所有"提交 `EditCommand` 到 `EditHistory`"的动作都在这里集中。
//!
//! ## 为什么需要这一层
//!
//! AGENTS §3.3 铁律："System 只能生成 `EditCommand`，不能自己改世界"。
//! 把面板 UI 与编辑提交**分文件**，便于：
//! 1. 测试时只调用 action 函数，无需构造 egui Context；
//! 2. 复用：撤销/重做、未来自动化脚本、热键都能用同一套 action；
//! 3. 隔离 UI 库：`ui.rs` 知道 egui，本文件不依赖。
//!
//! ## V1 守约（不阻止）
//!
//! `EditError` 一律走 `Bevy::log::warn` 报告，**不**抛错阻断 UI。
//! 面板上点击 "删除" 不会因为几何仍存在而被拒（`RemoveFactionCommand` 会
//! 自动清空旗下几何）。
//!
//! ## v0.2 测试覆盖状态
//!
//! 单元测试留给 wv-edit 那一层（`Add/Remove/Update/Upsert/RemoveGeometry` 命令
//! 11 个 round-trip 测试），不在这里复测——本文件只是命令的"面板入口"，
//! 没有自己的领域逻辑。面板 UI 端到端测试要靠 `cargo run -p wv-app --features app`
//! 启动 GUI 走交互验证。

#![cfg(feature = "app")]

use crate::edit::editing::EditHistory;
use crate::state::{ViewState, WorldProjection};
use wv_core::map::{GeometryData, GeometrySpan};
use wv_core::time::TimeSpan;
use wv_core::{Color, Faction, FactionColorMode, FactionLayer, Id, Membership};
use wv_edit::command::{
    AddFactionCommand, AddFactionLayerCommand, AddMembershipCommand, DivideFactionCommand,
    DivisionMode, EditCommand, RemoveFactionCommand, RemoveFactionGeometryCommand,
    RemoveFactionLayerCommand, RemoveMembershipCommand, SetFactionLayerCommand,
    SetFactionRankCommand, SetFactionSeatCommand, UpdateFactionCommand, UpdateFactionLayerCommand,
    UpsertFactionGeometryCommand,
};
use wv_edit::history::Transaction;

/// 新建一个势力。
///
/// `id` 用 `faction_<Unix 毫秒>`——保证唯一但可读性差；这是 v0.2 妥协，
/// 真实工程里作者会在面板内改 ID（v0.3）。
pub fn add_faction_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    name: String,
    color: Color,
) {
    let id_str = format!("faction_{}", current_unix_ms());
    let id = Id::new_unchecked(&id_str);
    let f = Faction::new(id, name, color);
    push_cmd(world, edit, view, Box::new(AddFactionCommand::new(f)));
}

/// 删除一个势力（连带名下所有几何）。
pub fn remove_faction_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
) {
    push_cmd(world, edit, view, Box::new(RemoveFactionCommand::new(id)));
}

/// 改名 / 改色。
pub fn update_faction_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    new_name: String,
    new_color: Color,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(UpdateFactionCommand::new(id, new_name, new_color)),
    );
}

/// 为当前地图 + 势力 + from 新建/替换一段几何。
///
/// 多边形先放一个最小可画三角形占位（v0.2 暂不提供点位拖拽编辑，
/// 那是 v0.3 势力多边形编辑器的工作）。
pub fn upsert_geometry_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    owner: Id,
    from: i64,
    to: Option<i64>,
) {
    let map = Id::new_unchecked(&view.current_map);
    let span = GeometrySpan {
        map,
        owner,
        span: TimeSpan { from, to },
        // 最小占位三角形：1×1 世界单位。本位置覆盖地图角落；
        // v0.2 暂不接点位编辑器，作者后续可手改 YAML。
        geometry: GeometryData::Polygon(vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0]),
        relation: None,
    };
    push_cmd(
        world,
        edit,
        view,
        Box::new(UpsertFactionGeometryCommand::new(span)),
    );
}

/// 删除当前地图下，指定势力 + from 的一段几何。
pub fn remove_geometry_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    owner: Id,
    from: i64,
) {
    let map = Id::new_unchecked(&view.current_map);
    push_cmd(
        world,
        edit,
        view,
        Box::new(RemoveFactionGeometryCommand::new(map, owner, from)),
    );
}

/// 新建一个势力层（ADR-0027）。
pub fn add_layer_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    name: String,
    z: i32,
) {
    let id_str = format!("faction_layer_{}", current_unix_ms());
    let layer = FactionLayer {
        id: Id::new_unchecked(&id_str),
        name,
        z,
        color_mode: FactionColorMode::Own,
        visible: true,
    };
    push_cmd(
        world,
        edit,
        view,
        Box::new(AddFactionLayerCommand::new(layer)),
    );
}

/// 更新势力层的名称 / z / 配色策略 / 可见性。
#[allow(clippy::too_many_arguments)]
pub fn update_layer_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    name: String,
    z: i32,
    color_mode: FactionColorMode,
    visible: bool,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(UpdateFactionLayerCommand::new(
            id, name, z, color_mode, visible,
        )),
    );
}

/// 删除一个势力层（其下势力回落默认层，不删势力本体）。
pub fn remove_layer_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(RemoveFactionLayerCommand::new(id)),
    );
}

/// 把势力挂到某个层（`None` = 默认层）。
pub fn set_faction_layer_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    layer: Option<Id>,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(SetFactionLayerCommand::new(id, layer)),
    );
}

/// 改势力的等级提示（自由字符串）。
pub fn set_faction_rank_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    rank: Option<String>,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(SetFactionRankCommand::new(id, rank)),
    );
}

/// 改势力的驻地引用。
pub fn set_faction_seat_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    id: Id,
    seat: Option<Id>,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(SetFactionSeatCommand::new(id, seat)),
    );
}

/// 追加一条归属边。
pub fn add_membership_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    faction: Id,
    membership: Membership,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(AddMembershipCommand::new(faction, membership)),
    );
}

/// 删除一条归属边（按 target + from 定位）。
pub fn remove_membership_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    view: &ViewState,
    faction: Id,
    target: Id,
    from: i64,
) {
    push_cmd(
        world,
        edit,
        view,
        Box::new(RemoveMembershipCommand::new(faction, target, from)),
    );
}

/// 从宿主（或空势力）划分出一个新势力（ADR-0028 / M1 Task 6）。
///
/// 与其它 `*_now` 不同，本动作**返回结果**：对话框要区分成功（清空选区 +
/// 成功回执）与失败（留在框内 + 人话回执），不能像面板那样一 warn 了事。
///
/// - `host = None`：空势力宿主（全图陆地，水域扣除）；
/// - `intent`：捕获图局部 wu 的意图环；
/// - `cell_size`：图内一格的世界单位（wu → 格坐标）；
/// - `day`：钉住的生效日（与打开对话框那一刻一致，M6）。
///
/// 划分结果：新势力 id + 贴地是否真的吸附到地物（Q2 退化回执）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivisionOutcome {
    pub id: Id,
    pub snapped: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn divide_faction_now(
    world: &mut WorldProjection,
    edit: &mut EditHistory,
    map: Id,
    host: Option<Id>,
    intent: Vec<(f32, f32)>,
    mode: DivisionMode,
    name: String,
    cell_size: f32,
    day: i64,
) -> Result<DivisionOutcome, String> {
    let id = Id::new_unchecked(format!("faction_{}", current_unix_ms()));
    let faction = Faction::new(id.clone(), name, Color("#6f7fa8".into()));
    let cmd = DivideFactionCommand::new(map, host, intent, mode, faction, day, cell_size);
    let snapped_flag = cmd.snapped_flag();
    let mut tx = Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut world.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            world.dirty = true;
            // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut world.world);
            let snapped = snapped_flag
                .lock()
                .map(|f| *f)
                .unwrap_or_else(|poisoned| *poisoned.into_inner());
            Ok(DivisionOutcome { id, snapped })
        }
        Err(e) => {
            bevy::log::warn!("势力划分失败: {e}");
            Err(format!("{e}"))
        }
    }
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
            // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
            crate::edit::road_sync::sync_derived_road_edges(&mut world.world);
            // 提示：v0.2 暂不写盘（路线图阶段七），但 `dirty` 标记保证下
            // 次手动保存时一切就绪。
            let _ = view;
        }
        Err(e) => {
            bevy::log::warn!("势力编辑失败: {e}");
        }
    }
}

/// `std::time::SystemTime::now()` 转 Unix 毫秒（仅用于势力 ID 缺省值）。
fn current_unix_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
