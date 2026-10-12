//! 校验面板 → 跳转定位的动作层（阶段八 §A）。
//!
//! ## 职责
//!
//! 面板 UI（`ui.rs::panel_validation`）只负责**呈现**诊断条目与**收集**
//! 用户的点击；本模块负责把点击翻译成「视图跳转」动作。
//!
//! 与 `faction_panel_actions` 同源：UI 与提交**分文件**，便于复用与测试。
//!
//! ## 阶段八 §A 决策
//!
//! 校验面板的「点提示跳定位」落地为**单步动作**：
//! 1. `DrillState::drill_to(world, anchor.map)` 计算从根到 anchor.map 的路径，
//!    重写 trail（实现见 `state::DrillState`）。
//! 2. `ViewState::camera_center` 设为 anchor.at 的世界坐标。
//! 3. `PlaybackState::current_day` 设为 anchor.day（让时间轴拨到提示发生时刻）。
//! 4. `DrillState::sync_view(view)` 同步 `view.current_map` 镜像。
//!
//! **为何不**通过 `CommandId::DrillToSegment(_)`：`DrillToSegment` 只接受
//! 当前 trail 的索引，无法表达「跨层级跳到任意节点」。

#![cfg(feature = "app")]

use crate::state::{DrillState, PlaybackState, ViewState, WorldProjection};
use wv_io::diagnostics::DiagAnchor;

/// 跳到一条提示的锚点。返回是否成功（目标图不在树里就静默返回 false，
/// 让 UI 不刷回执——V1 守约镜像）。
///
/// 失败**不**刷 `notice`（这里不持有 `ShellNotice`，且校验面板的 row
/// 已被点击了，作者会从「视图没变」这一现象读懂原因）。
pub fn jump_to_anchor_now(
    projection: &WorldProjection,
    drill: &mut DrillState,
    view: &mut ViewState,
    playback: &mut PlaybackState,
    anchor: &DiagAnchor,
) -> bool {
    if !drill.drill_to(&projection.world, &anchor.map) {
        return false;
    }
    // 同步 `current_map` 镜像。
    drill.sync_view(view);
    // 把相机拨到 anchor.at——世界坐标。
    view.camera_center = bevy::prelude::Vec2::new(anchor.at.x, anchor.at.y);
    // 时间轴拨到 anchor.day。
    playback.current_day = anchor.day;
    true
}
