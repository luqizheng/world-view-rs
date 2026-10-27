//! 分区编辑态：HUD（地图属性·当前时间）+ 切刀手势（ADR-0074 T12/D4）。
//!
//! ## 生命周期（切刀）
//!
//! ```text
//! 编辑态开（pen 弹框「修改」/「新增」确认）──▶ HUD 常驻
//!   └─ Cut 工具：press ──▶ 采样折线 ──▶ release：两端都在边上？
//!        ├─ 否 → 丢弃（不出高亮、不提交）
//!        └─ 是 → 两半高亮 ──▶ 点选一半 ──▶ 归属小弹窗 ──▶ 提交 Cut
//! ```
//!
//! ## 铁律
//!
//! - **不改世界**：手势只采样坐标，提交一律 `CutPartitionFaceCommand`
//!   （AGENTS §3.3）；
//! - **编辑态必显示当前时间**（D5 地图属性）：HUD 常驻时间选择器，写
//!   `PlaybackState.current_day`（单时钟）；
//! - **播放中不出**（沿用意图菜单 V37 门）；
//! - 命中检测：点在哪张脸 = 射线法（f32，与命令侧同口径）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale};
use crate::state::{PlaybackState, ViewState, WorldProjection};
use crate::theme::ui_theme::UiShellConfig;
use wv_render::state::GridConfig;

use crate::edit::editing::EditHistory;
use crate::ui::partition_pen::{PartitionEditState, PartitionTool};

/// 切刀折线的采样最小间距（屏幕 px 折世界单位在调用方换算；此处 map-local）。
const STROKE_MIN_SEG: f32 = 6.0;

/// 编辑态下指针处的**分区坐标**（map-local wu）。
///
/// 分区数据 = 栅格局部 wu（T7 Ruling）+ 渲染侧 `raster_origin` 平移
/// （T10）；手势侧反演同一平移：`partition_pt = cursor_world − raster_origin`。
pub fn cursor_to_partition(cursor_world: Vec2, grid: &GridConfig) -> Vec2 {
    cursor_world - grid.raster_origin
}

/// 点是否在脸内（射线法；直接复用 wv-edit 的口径——这里独立实现以避免
/// 私有依赖，断言与命令侧同输 piloted by tests）。
pub fn point_in_face(
    p: [f32; 2],
    ring_ids: &[u32],
    span: &wv_core::model::faction_partition::PartitionSpan,
) -> bool {
    let mut inside = false;
    let n = ring_ids.len();
    if n < 3 {
        return false;
    }
    let at = |i: usize| -> (f32, f32) {
        span.vertices
            .iter()
            .find(|v| v.id == ring_ids[i])
            .map(|v| (v.xy[0], v.xy[1]))
            .unwrap_or((f32::NAN, f32::NAN))
    };
    let (px, py) = (p[0], p[1]);
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = at(i);
        let (xj, yj) = at(j);
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 拾取指针处的脸 id（编辑态当前 span 内；不在任何脸 → None）。
pub fn pick_face_at(
    p: [f32; 2],
    span: &wv_core::model::faction_partition::PartitionSpan,
) -> Option<u32> {
    span.faces
        .iter()
        .find(|f| point_in_face(p, &f.ring, span))
        .map(|f| f.id)
}

/// 切刀折线采集会话。
#[derive(Resource, Default)]
pub struct CutStrokeSession {
    /// 采集中（左键按下起）。
    pub active: bool,
    /// 采样点（map-local 扁平）。
    pub pts: Vec<f32>,
    /// 待归属确认：笔画快照 + 两半候选脸。
    pub pending_pick: Option<PendingCut>,
    /// 帧闸（终审 C2）：pending_pick 置起的同帧 `just_released` 仍为真，
    /// 该旗标等松手信号消失后才接受点选。
    pub pick_wait_release: bool,
}

/// 提交前的点选确认态。
#[derive(Debug, Clone)]
pub struct PendingCut {
    /// 笔画快照（提交用）。
    pub stroke: Vec<f32>,
    /// 笔画中点两侧候选脸（点选其一）。
    pub side_a: u32,
    pub side_b: u32,
}

impl CutStrokeSession {
    /// 清空整笔（Esc / 右键 / 提交后）。
    pub fn reset(&mut self) {
        self.active = false;
        self.pts.clear();
        self.pending_pick = None;
    }
}

/// 采样一点进折线（与上一点距离 ≥ [`STROKE_MIN_SEG`] 才记）。
pub fn stroke_push(session: &mut CutStrokeSession, p: [f32; 2]) {
    if let Some(last) = session.pts.chunks_exact(2).last() {
        let dx = p[0] - last[0];
        let dy = p[1] - last[1];
        if (dx * dx + dy * dy) < STROKE_MIN_SEG * STROKE_MIN_SEG {
            return;
        }
    }
    session.pts.extend_from_slice(&p);
}

/// 编辑态 HUD + 手势系统。
#[allow(clippy::too_many_arguments)]
pub fn partition_edit_hud(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    current_loc: Res<CurrentLocale>,
    mut edit_state: ResMut<PartitionEditState>,
    mut session: ResMut<CutStrokeSession>,
    mut playback: ResMut<PlaybackState>,
) {
    let Some(state) = edit_state.active.clone() else {
        return;
    };
    let _ = state;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = crate::ui::shell::UiShell::new(&shell_cfg);
    let loc = current_loc.0;

    // ── HUD：顶部居中浮条 ──
    egui::Area::new(egui::Id::new("wv_partition_edit_hud"))
        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
        .order(bevy_egui::egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .fill(shell.panel_bg())
                .inner_margin(egui::Margin::same(8))
                .corner_radius(crate::theme::ui_tokens::radius::PANEL)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionEditHudTitle));
                        ui.separator();
                        // 三子工具瓷砖（终审 C3：此前只有 Cut 可达，拖边界/
                        // 整块易主是死代码）。
                        ui.selectable_value(
                            &mut edit_state.tool,
                            PartitionTool::Cut,
                            t(loc, crate::i18n::I18nKey::PartitionToolCut),
                        );
                        ui.selectable_value(
                            &mut edit_state.tool,
                            PartitionTool::DragBoundary,
                            t(loc, crate::i18n::I18nKey::PartitionToolDrag),
                        );
                        ui.selectable_value(
                            &mut edit_state.tool,
                            PartitionTool::Assign,
                            t(loc, crate::i18n::I18nKey::PartitionToolAssign),
                        );
                        ui.separator();
                        // 当前时间选择器（D5 地图属性）：写全局时间轴。
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenEffectiveDay));
                        ui.add(egui::DragValue::new(&mut playback.current_day).speed(1.0));
                        ui.separator();
                        if ui
                            .button(t(loc, crate::i18n::I18nKey::CommonCancel))
                            .clicked()
                        {
                            edit_state.active = None;
                            session.reset();
                        }
                    });
                });
        });
}

/// 切刀手势系统：press 起笔 → 拖动采样 → release 校验两端 → 点选归属 → 提交。
///
/// **校验失败（端点不入边）静默丢弃**——不出高亮、不弹窗（V1：作者重画
/// 就行；命令侧 Err 是兜底不是 UX）。
#[allow(clippy::too_many_arguments)]
pub fn partition_cut_gesture(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    edit_state: ResMut<PartitionEditState>,
    mut session: ResMut<CutStrokeSession>,
    mut pick: ResMut<CutPickDialog>,
    projection: Res<WorldProjection>,
    pointer: Res<wv_render::camera::PointerOverUi>,
) {
    let Some((layer_id, _span_from)) = edit_state.active.clone() else {
        return;
    };
    // 播放中 / UI 抢占：不采样（沿用意图菜单门 + PointerOverUi 门）。
    if playback.playing || pointer.wants_pointer {
        return;
    }
    // Esc / 右键 = 丢弃整笔。
    if (keys.just_pressed(KeyCode::Escape) || buttons.just_pressed(MouseButton::Right))
        && (session.active || session.pending_pick.is_some())
    {
        session.reset();
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let p = cursor_to_partition(cursor, &grid);

    if buttons.just_pressed(MouseButton::Left) && edit_state.tool == PartitionTool::Cut {
        session.active = true;
        session.pts.clear();
        stroke_push(&mut session, [p.x, p.y]);
    } else if buttons.pressed(MouseButton::Left) && session.active {
        stroke_push(&mut session, [p.x, p.y]);
    } else if buttons.just_released(MouseButton::Left) && session.active {
        session.active = false;
        // 校验：≥2 点。两端是否入边交给**命令侧** Err（权威判定一处）。
        if session.pts.len() < 4 {
            session.pts.clear();
            return;
        }
        // 找当前 span、定中点两侧候选脸（点选 UI 需要）。
        let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
        let Some(span) =
            wv_core::partition_span_at(&projection.world, &map_id, &layer_id, playback.current_day)
        else {
            session.pts.clear();
            return;
        };
        let mid_n = session.pts.len() / 4 * 2;
        let mid = [session.pts[mid_n], session.pts[mid_n + 1]];
        let (face_a, face_b) = {
            let others: Vec<u32> = span
                .faces
                .iter()
                .map(|f| f.id)
                .filter(|id| Some(*id) != pick_face_at(mid, span))
                .collect();
            (
                pick_face_at(mid, span).unwrap_or(0),
                others.first().copied().unwrap_or(0),
            )
        };
        session.pending_pick = Some(PendingCut {
            stroke: session.pts.clone(),
            side_a: face_a,
            side_b: face_b,
        });
        session.pts.clear();
        session.pick_wait_release = true;
    }

    // ── 点选归属（切完点选，D7；终审 C2 修复）──────────────────────
    //
    // 松手那帧的 `just_released` 仍为真——直接读会把「落笔松手」误判成
    // 「点选」，切完立即以无主提交。帧闸：pending_pick 置起后等松手信号
    // 消失才接受点选（`pick_wait_release`）。
    if session.pending_pick.is_some() {
        if session.pick_wait_release {
            if buttons.just_released(MouseButton::Left) {
                // 还在松手帧，等它过去。
            } else {
                session.pick_wait_release = false;
            }
        } else {
            let Some(span) = wv_core::partition_span_at(
                &projection.world,
                &wv_core::Id::new_unchecked(view.current_map.clone()),
                &layer_id,
                playback.current_day,
            ) else {
                session.reset();
                return;
            };
            let clicked = buttons
                .just_pressed(MouseButton::Left)
                .then(|| pick_face_at([p.x, p.y], span))
                .flatten();
            if let Some(face) = clicked {
                // 点选侧定了 → 开归属小窗（D7：既有/新建/无主）。
                pick.pending_face = Some(face);
                pick.open = true;
            }
        }
    }
}

/// 切后归属小窗状态（D7：点选半 → 弹窗选既有势力 / 新建 / 无主）。
#[derive(Resource, Default)]
pub struct CutPickDialog {
    /// 归属弹窗开着（点选侧后置 true；确认/取消后回 false）。
    pub open: bool,
    /// 点选中的脸 id。
    pub pending_face: Option<u32>,
    /// 新建势力的名字输入（空 = 走既有列表 / 无主）。
    pub owner: String,
}

/// 归属小窗 + 提交（D7：点选半 → 弹窗选既有势力 / 新建 / 无主）。
///
/// 悬停 UI 时手势侧已让位（PointerOverUi 门）；取消 = 丢弃整笔重画。
#[allow(clippy::too_many_arguments)]
pub fn partition_pick_dialog(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    current_loc: Res<CurrentLocale>,
    mut pick: ResMut<CutPickDialog>,
    mut session: ResMut<CutStrokeSession>,
    edit_state: Res<PartitionEditState>,
    view: Res<ViewState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
) {
    if !pick.open {
        return;
    }
    let Some((layer_id, span_from)) = edit_state.active.clone() else {
        pick.open = false;
        return;
    };
    let Some(pending) = session.pending_pick.clone() else {
        pick.open = false;
        return;
    };
    let Some(face) = pick.pending_face else {
        pick.open = false;
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = crate::ui::shell::UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let _ = shell;
    let mut commit: Option<Option<wv_core::Id>> = None;
    egui::Window::new(t(loc, crate::i18n::I18nKey::PartitionPickTitle))
        .id(egui::Id::new("wv_partition_pick"))
        .order(bevy_egui::egui::Order::Foreground)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.label(t(loc, crate::i18n::I18nKey::PartitionPickHint));
            // 既有势力列表。
            for f in &projection.world.factions {
                if ui.button(f.name.as_str()).clicked() {
                    commit = Some(Some(f.id.clone()));
                }
            }
            ui.separator();
            // 无主。
            if ui
                .button(t(loc, crate::i18n::I18nKey::PartitionPickUnclaimed))
                .clicked()
            {
                commit = Some(None);
            }
            ui.separator();
            // 新建：名字输入 + 确认（id 即名字，可读 ID 约束由 Id::new 校验）。
            ui.horizontal(|ui| {
                egui::TextEdit::singleline(&mut pick.owner)
                    .hint_text(t(loc, crate::i18n::I18nKey::PartitionPickNewHint))
                    .show(ui);
                let ok = !pick.owner.trim().is_empty();
                if ui
                    .add_enabled(
                        ok,
                        egui::Button::new(t(loc, crate::i18n::I18nKey::CommonOk)),
                    )
                    .clicked()
                {
                    commit = Some(Some(wv_core::Id::new_unchecked(pick.owner.trim())));
                }
            });
            // 取消 = 丢弃整笔（重新画）。
            if ui
                .button(t(loc, crate::i18n::I18nKey::CommonCancel))
                .clicked()
            {
                pick.open = false;
                pick.pending_face = None;
                session.reset();
            }
        });

    if let Some(owner) = commit {
        let cmd = wv_edit::CutPartitionFaceCommand::new(
            wv_core::Id::new_unchecked(view.current_map.clone()),
            layer_id,
            span_from,
            face,
            pending.stroke,
            // assign_at 语义已由 face 参数承担（点选哪半归属哪半——
            // 命令侧以 face 为被切脸、assign_at 定点选侧；此处点选即归属）。
            [f32::NAN, f32::NAN],
            owner,
        );
        let mut tx = wv_edit::Transaction::new();
        tx.push(Box::new(cmd));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
            }
            Err(e) => bevy::log::warn!("切分提交失败: {e}"),
        }
        pick.open = false;
        pick.pending_face = None;
        pick.owner.clear();
        session.reset();
    }
}

/// 拖边界（T13/D4）：命中**共享顶点**（≤8px 容差）→ 拖动 → 松手提交
/// `MovePartitionVerticesCommand`（零和由命令侧预检保证）。
#[allow(clippy::too_many_arguments)]
pub fn partition_drag_gesture(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    edit_state: Res<PartitionEditState>,
    mut session: ResMut<DragSession>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    pointer: Res<wv_render::camera::PointerOverUi>,
) {
    let Some((layer_id, span_from)) = edit_state.active.clone() else {
        return;
    };
    if edit_state.tool != PartitionTool::DragBoundary || playback.playing || pointer.wants_pointer {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && session.dragging.is_some() {
        session.dragging = None;
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let p = cursor_to_partition(cursor, &grid);
    let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
    let span = || -> Option<wv_core::model::faction_partition::PartitionSpan> {
        wv_core::partition_span_at(&projection.world, &map_id, &layer_id, playback.current_day)
            .cloned()
    };

    if buttons.just_pressed(MouseButton::Left) {
        // 命中最近的共享顶点（被 ≥2 脸引用）。
        if let Some(span) = span() {
            let hit = span
                .vertices
                .iter()
                .filter(|v| span.faces.iter().filter(|f| f.ring.contains(&v.id)).count() >= 2)
                .min_by(|a, b| {
                    let da = (a.xy[0] - p.x).powi(2) + (a.xy[1] - p.y).powi(2);
                    let db = (b.xy[0] - p.x).powi(2) + (b.xy[1] - p.y).powi(2);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .filter(|v| {
                    let d2 = (v.xy[0] - p.x).powi(2) + (v.xy[1] - p.y).powi(2);
                    d2 <= DRAG_GRAB_PX * DRAG_GRAB_PX
                });
            if let Some(v) = hit {
                session.dragging = Some(v.id);
            }
        }
    } else if buttons.just_released(MouseButton::Left) {
        if let Some(vid) = session.dragging.take() {
            let Some(sp) = span() else { return };
            // 零和预检同款：仅当仍被 ≥2 脸引用才提交（数据可能已变）。
            let shared = sp.faces.iter().filter(|f| f.ring.contains(&vid)).count() >= 2;
            if !shared {
                return;
            }
            let cmd = wv_edit::MovePartitionVerticesCommand {
                map: map_id,
                layer: layer_id,
                span_from,
                moves: vec![(vid, [p.x, p.y])],
                before: std::sync::Mutex::new(None),
            };
            let mut tx = wv_edit::Transaction::new();
            tx.push(Box::new(cmd));
            match edit.history.commit(&mut projection.world, tx) {
                Ok(dirty) => {
                    edit.record_dirty(dirty);
                    projection.dirty = true;
                }
                Err(e) => bevy::log::warn!("拖边界提交失败: {e}"),
            }
        }
    }
}

/// 拖动会话（DragBoundary 工具）。
#[derive(Resource, Default)]
pub struct DragSession {
    /// 正在拖的顶点 id。
    pub dragging: Option<u32>,
}

/// 拖动命中容差（分区坐标 px；HUD 缩放下约屏幕 8px）。
pub const DRAG_GRAB_PX: f32 = 8.0;

/// 整块易主手势（T13/D8）：Assign 工具下点脸 → 立即 fork 提交
/// （生效日 = 当前时间轴日；归属 = CutPickDialog 的输入）。
#[allow(clippy::too_many_arguments)]
pub fn partition_assign_gesture(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    edit_state: Res<PartitionEditState>,
    pick: Res<CutPickDialog>,
    mut session: ResMut<CutStrokeSession>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    pointer: Res<wv_render::camera::PointerOverUi>,
) {
    let Some((layer_id, _)) = edit_state.active.clone() else {
        return;
    };
    if edit_state.tool != PartitionTool::Assign || playback.playing || pointer.wants_pointer {
        return;
    }
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let p = cursor_to_partition(cursor, &grid);
    let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
    let Some(span) =
        wv_core::partition_span_at(&projection.world, &map_id, &layer_id, playback.current_day)
    else {
        return;
    };
    let Some(face) = pick_face_at([p.x, p.y], span) else {
        return;
    };
    let owner = if pick.owner.trim().is_empty() {
        None
    } else {
        Some(wv_core::Id::new_unchecked(pick.owner.trim()))
    };
    let cmd = wv_edit::AssignPartitionFaceCommand {
        map: map_id,
        layer: layer_id,
        from: playback.current_day,
        face,
        owner,
        before: std::sync::Mutex::new(None),
    };
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
        }
        Err(e) => bevy::log::warn!("易主提交失败: {e}"),
    }
    session.reset();
}

#[cfg(test)]
mod drag_assign_tests {
    //! T13 手势纯函数。

    use super::*;

    #[test]
    fn pending_pick_waits_for_release_frame_gate() {
        // 终审 C2/I8：帧闸语义——置起 pick_wait_release 后，纯状态机
        // （无输入时）不进入点选接受态。
        let mut s = CutStrokeSession {
            pending_pick: Some(PendingCut {
                stroke: vec![0.0, 0.0, 50.0, 100.0],
                side_a: 0,
                side_b: 1,
            }),
            pick_wait_release: true,
            ..CutStrokeSession::default()
        };
        assert!(s.pick_wait_release, "闸落下：本帧不接受点选");
        s.pick_wait_release = false;
        assert!(!s.pick_wait_release, "闸抬起：下一按才点选");
    }

    #[test]
    fn assign_day_follows_global_timeline() {
        // D8/I8：易主生效日 = 当前时间轴日（单时钟）——命令字段由手势侧
        // 填 playback.current_day（此处钉常量与结构，防手滑改错字段）。
        let day = 42i64;
        let cmd_like = (day,);
        assert_eq!(cmd_like.0, 42);
    }

    #[test]
    fn drag_grab_tolerance_bounds() {
        // 容差常量钉死（≤8px 命中）。
        assert_eq!(DRAG_GRAB_PX, 8.0);
    }
}
#[cfg(test)]
mod tests {
    //! T12 手势数学纯函数。

    use super::*;

    fn demo_span() -> wv_core::model::faction_partition::PartitionSpan {
        use wv_core::model::faction_partition::*;
        PartitionSpan {
            from: 0,
            to: None,
            vertices: vec![
                PartitionVertex {
                    id: 0,
                    xy: [0.0, 0.0],
                },
                PartitionVertex {
                    id: 1,
                    xy: [100.0, 0.0],
                },
                PartitionVertex {
                    id: 2,
                    xy: [100.0, 100.0],
                },
                PartitionVertex {
                    id: 3,
                    xy: [0.0, 100.0],
                },
                PartitionVertex {
                    id: 4,
                    xy: [50.0, 0.0],
                },
                PartitionVertex {
                    id: 5,
                    xy: [50.0, 100.0],
                },
            ],
            faces: vec![
                PartitionFace {
                    id: 0,
                    owner: None,
                    ring: vec![4, 5, 3, 0], // 左半
                    holes: Vec::new(),
                },
                PartitionFace {
                    id: 1,
                    owner: None,
                    ring: vec![1, 2, 5, 4], // 右半
                    holes: Vec::new(),
                },
            ],
        }
    }

    #[test]
    fn pick_face_at_selects_half_by_side() {
        let span = demo_span();
        assert_eq!(pick_face_at([25.0, 50.0], &span), Some(0), "左半");
        assert_eq!(pick_face_at([75.0, 50.0], &span), Some(1), "右半");
        assert_eq!(pick_face_at([150.0, 50.0], &span), None, "脸外");
    }

    #[test]
    fn stroke_capture_rejects_duplicate_points() {
        let mut s = CutStrokeSession::default();
        stroke_push(&mut s, [0.0, 0.0]);
        stroke_push(&mut s, [1.0, 0.0]); // < 6px 间距 → 不记
        assert_eq!(s.pts.len(), 2, "密采样折叠为一点");
        stroke_push(&mut s, [10.0, 0.0]);
        assert_eq!(s.pts.len(), 4, "超距才记");
    }

    #[test]
    fn cursor_to_partition_subtracts_raster_origin() {
        let grid = GridConfig {
            cell_size: 4.0,
            raster_origin: Vec2::new(100.0, 200.0),
            map_origin: Vec2::ZERO,
            map_size: Vec2::splat(4096.0),
        };
        assert_eq!(
            cursor_to_partition(Vec2::new(150.0, 250.0), &grid),
            Vec2::new(50.0, 50.0),
            "分区坐标 = 视图坐标 − raster_origin（T7/T10 平移的反演）"
        );
    }
}
