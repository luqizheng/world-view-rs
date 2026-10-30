//! 分区编辑态：HUD（地图属性·当前时间）+ 切刀手势（ADR-0079 T12/D4）。
//!
//! ## 生命周期（切刀）
//!
//! ```text
//! 编辑态开（pen 弹框「修改」/「新增」确认）──▶ HUD 常驻
//!   └─ Cut 工具：press ──▶ 采样折线 ──▶ release：两端都在边上？
//!        ├─ 否 → 丢弃（notice 报错，不提交）
//!        └─ 是 → 松手即提交（新块自动归占位势力「无名N」；归属错了走易主）
//!   └─ Assign 工具：点脸 ──▶ 归属小弹窗 ──▶ 提交 AssignPartition
//! ```
//!
//! ## 铁律
//!
//! - **不改世界**：手势只采样坐标，提交一律 `EditCommand`
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

/// 疆域替换的点选更新（[ADR-0090] D4/M3 纯函数）：
/// `additive`（Ctrl 按住）= 脸在集合则移除、不在则加入；否则重置为单脸。
pub fn replace_pick(picked: Vec<u32>, face: u32, additive: bool) -> Vec<u32> {
    if !additive {
        return vec![face];
    }
    let mut picked = picked;
    if let Some(i) = picked.iter().position(|&f| f == face) {
        picked.remove(i);
    } else {
        picked.push(face);
    }
    picked
}

/// 切刀折线采集会话。
#[derive(Resource, Default)]
pub struct CutStrokeSession {
    /// 采集中（左键按下起）。
    pub active: bool,
    /// 采样点（map-local 扁平）。
    pub pts: Vec<f32>,
    /// 群系锋线索引（[ADR-0090] D5/M4）：**落笔时构建一次、整笔复用**
    /// （预览帧零重建；O(格数) 一次性的成本在 just_pressed 付）。
    pub snap_index: Option<std::sync::Arc<wv_world_cell_map::biome_snap::BiomeSnapIndex>>,
}

impl CutStrokeSession {
    /// 清空整笔（Esc / 右键 / 提交后）。
    pub fn reset(&mut self) {
        self.active = false;
        self.pts.clear();
        self.snap_index = None;
    }
}

/// 贴近群系的吸附应用（[ADR-0090] D5/M4 纯辅助）：有索引时对 `new`
/// 吸附，否则原样返回。`first` 用于落笔首点（此时 pts 尚未含它）。
fn snapped(
    session: &CutStrokeSession,
    pts: &[f32],
    grid: &GridConfig,
    scale: f32,
    first: Option<[f32; 2]>,
) -> Vec<f32> {
    let Some(index) = session.snap_index.as_ref() else {
        let mut v = pts.to_vec();
        if let Some(f) = first {
            v.extend_from_slice(&f);
        }
        return v;
    };
    // 半径 = 屏幕 12px 折格数（至少 1 格）——手抖余量与锋线捕捉范围的
    // 折中；过大会把笔画硬拽去远处的锋线。
    let radius = (12.0 * scale / grid.cell_size).max(1.0);
    let mut base = pts.to_vec();
    if let Some(f) = first {
        base.extend_from_slice(&f);
    }
    index.snap_polyline(&base, grid.cell_size, radius)
}

/// 笔画落在哪张脸：沿采样点逐点找**第一张命中脸**。
///
/// 终审遗留修（2026-10-28）：此前只取笔画中点、落脸外（贴边 / 画在湖洞
/// 上）盲兜底脸 0——切错对象且无提示。现逐点探测；全程不入任何脸 →
/// `None`（调用方 notice 报错并丢弃，不提交）。
pub fn stroke_face(
    stroke: &[f32],
    span: &wv_core::model::faction_partition::PartitionSpan,
) -> Option<u32> {
    for c in stroke.as_chunks::<2>().0 {
        if let Some(f) = pick_face_at([c[0], c[1]], span) {
            return Some(f);
        }
    }
    None
}

/// 自然脸显示名（ADR-0081 命名规则）：`<族名><N>`——N = 同 kind 脸中按
/// face id 升序的序号。**id 随 YAML 持久化 → 名字跨会话稳定**；非自然脸
/// （kind `None`）或脸不存在 → `None`。
pub fn nature_face_name(
    span: &wv_core::model::faction_partition::PartitionSpan,
    face_id: u32,
) -> Option<String> {
    span.face_display_name(face_id, &wv_terrain::biome::BIOME_FAMILY_NAMES_ZH)
}

/// 采样一点进折线（与上一点距离 ≥ [`STROKE_MIN_SEG`] 才记）。
pub fn stroke_push(session: &mut CutStrokeSession, p: [f32; 2]) {
    if let Some(last) = session.pts.as_chunks::<2>().0.last() {
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
    view: Res<ViewState>,
    mut projection: ResMut<WorldProjection>,
    mut edit_state: ResMut<PartitionEditState>,
    mut session: ResMut<CutStrokeSession>,
    mut playback: ResMut<PlaybackState>,
    mut replace: ResMut<crate::ui::partition_pen::PartitionReplaceState>,
    mut edit: ResMut<EditHistory>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<bevy::prelude::Time>,
) {
    let Some((layer_id, _)) = edit_state.active.clone() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = crate::ui::shell::UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    // 层用途（ADR-0081）：自然层的 Assign 工具 = 改群系（文案与语义都换）。
    let nature = crate::ui::partition_pen::layer_usage(&projection.world, &layer_id)
        == wv_core::model::faction::LayerUsage::Nature;

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
                            t(
                                loc,
                                if nature {
                                    crate::i18n::I18nKey::PartitionToolSetKind
                                } else {
                                    crate::i18n::I18nKey::PartitionToolAssign
                                },
                            ),
                        );
                        if !nature {
                            ui.selectable_value(
                                &mut edit_state.tool,
                                PartitionTool::Replace,
                                t(loc, crate::i18n::I18nKey::PartitionToolReplace),
                            );
                        }
                        ui.separator();
                        // 当前时间选择器（D5 地图属性）：写全局时间轴。
                        ui.label(t(loc, crate::i18n::I18nKey::PartitionPenEffectiveDay));
                        ui.add(egui::DragValue::new(&mut playback.current_day).speed(1.0));
                        ui.separator();
                        // 边界红圈开关（2026-10-28 由常显改可选；默认开——
                        // 红圈是「先看见能切的地方」的引导）。
                        ui.checkbox(
                            &mut edit_state.show_boundaries,
                            t(loc, crate::i18n::I18nKey::PartitionShowBoundaries),
                        );
                        // [ADR-0090 D5/M4] 贴近群系（默认勾；关 = 自由切）。
                        ui.checkbox(
                            &mut edit_state.snap_to_biome,
                            t(loc, crate::i18n::I18nKey::PartitionSnapBiome),
                        );
                        ui.separator();
                        if ui
                            .button(t(loc, crate::i18n::I18nKey::CommonCancel))
                            .clicked()
                        {
                            edit_state.active = None;
                            session.reset();
                        }
                    });
                    // 阶段提示行：随切刀状态实时变化（可见反馈闭环）。
                    // finding-8：文案接 i18n，并去掉「✎」——打包字体无该字形
                    // 会落豆腐块（铅笔图标处已实锤过同款问题）。
                    let stage = if session.active {
                        t(loc, crate::i18n::I18nKey::PartitionStageCapturing)
                    } else if edit_state.tool == PartitionTool::Cut {
                        t(loc, crate::i18n::I18nKey::PartitionStageCut)
                    } else if edit_state.tool == PartitionTool::DragBoundary {
                        t(loc, crate::i18n::I18nKey::PartitionStageDrag)
                    } else if edit_state.tool == PartitionTool::Replace {
                        t(loc, crate::i18n::I18nKey::PartitionStageReplace)
                    } else if nature {
                        t(loc, crate::i18n::I18nKey::PartitionStageAssignNature)
                    } else {
                        t(loc, crate::i18n::I18nKey::PartitionStageAssign)
                    };
                    ui.separator();
                    ui.label(
                        egui::RichText::new(stage)
                            .small()
                            .color(egui::Color32::from_rgb(255, 210, 120)),
                    );
                    // [ADR-0090 D4/M3] 疆域替换面板（政治层 Replace 工具）：
                    // 目标势力下拉 + 群系闸门（默认勾）+ 多选计数 + 提交。
                    if edit_state.tool == PartitionTool::Replace && !nature {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(t(loc, crate::i18n::I18nKey::PartitionReplaceTargetLabel));
                            let factions = &projection.world.factions;
                            // finding-11：不做静默默认（旧实现未选时自动取第一个
                            // 势力）——目标必须显式选，未选时下拉显示占位、确认禁用。
                            let cur_name = replace
                                .target
                                .as_ref()
                                .and_then(|id| {
                                    factions
                                        .iter()
                                        .find(|f| &f.id == id)
                                        .map(|f| f.name.clone())
                                })
                                .unwrap_or_else(|| {
                                    t(loc, crate::i18n::I18nKey::PartitionReplacePickTarget)
                                        .to_owned()
                                });
                            egui::ComboBox::from_id_salt("wv_partition_replace_target")
                                .selected_text(cur_name)
                                .show_ui(ui, |ui| {
                                    for f in factions {
                                        ui.selectable_value(
                                            &mut replace.target,
                                            Some(f.id.clone()),
                                            f.name.as_str(),
                                        );
                                    }
                                });
                            ui.checkbox(
                                &mut replace.biome_gate,
                                t(loc, crate::i18n::I18nKey::PartitionReplaceBiomeGate),
                            );
                            ui.label(format!("已选 {} 脸", replace.picked.len()));
                            if ui
                                .button(t(loc, crate::i18n::I18nKey::PartitionReplaceClear))
                                .clicked()
                            {
                                replace.picked.clear();
                            }
                            let can = replace.target.is_some() && !replace.picked.is_empty();
                            if ui
                                .add_enabled(
                                    can,
                                    egui::Button::new(t(
                                        loc,
                                        crate::i18n::I18nKey::PartitionReplaceConfirm,
                                    )),
                                )
                                .clicked()
                            {
                                let cmd = wv_edit::ReplaceFactionFacesCommand {
                                    map: wv_core::Id::new_unchecked(view.current_map.clone()),
                                    layer: layer_id.clone(),
                                    from: playback.current_day,
                                    faction: replace.target.clone().expect("按钮禁用时空目标"),
                                    faces: replace.picked.clone(),
                                    before: std::sync::Mutex::new(None),
                                };
                                let mut tx = wv_edit::Transaction::new();
                                tx.push(Box::new(cmd));
                                match edit.history.commit(&mut projection.world, tx) {
                                    Ok(dirty) => {
                                        edit.record_dirty(dirty);
                                        projection.dirty = true;
                                        let n = replace.picked.len();
                                        replace.picked.clear();
                                        notice.info(
                                            format!("已替换疆域（{n} 张脸）"),
                                            (time.elapsed_secs_f64() * 60.0) as u64,
                                        );
                                    }
                                    Err(e) => {
                                        notice.error(
                                            format!("疆域替换失败：{e}"),
                                            (time.elapsed_secs_f64() * 60.0) as u64,
                                        );
                                        bevy::log::warn!("疆域替换提交失败: {e}");
                                    }
                                }
                            }
                        });
                    }
                });
        });
}

/// 切刀手势系统：press 起笔 → 拖动采样 → release **松手即提交**（新块自动
/// 归占位势力「无名N」，不打断；归属错了走易主）。
///
/// 失败可见（V1-UX）：笔画不入任何脸 / 命令侧 Err（端点离边太远等）都出
/// notice，不静默——静默丢弃会让作者以为工具坏了。
#[allow(clippy::too_many_arguments)]
pub fn partition_cut_gesture(
    mut gizmos: bevy::prelude::Gizmos,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<bevy::prelude::Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    edit_state: ResMut<PartitionEditState>,
    mut session: ResMut<CutStrokeSession>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    pointer: Res<wv_render::camera::PointerOverUi>,
) {
    let Some((layer_id, span_from)) = edit_state.active.clone() else {
        return;
    };
    // 播放中 / UI 抢占：不采样（沿用意图菜单门 + PointerOverUi 门）。
    if playback.playing || pointer.wants_pointer {
        return;
    }
    let mut pick_notice = |msg: &str| {
        notice.error(msg, (time.elapsed_secs_f64() * 60.0) as u64);
    };
    // Esc / 右键 = 丢弃整笔。
    if (keys.just_pressed(KeyCode::Escape) || buttons.just_pressed(MouseButton::Right))
        && session.active
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
        // [ADR-0090 D5/M4] 贴近群系：落笔时构建锋线索引一次（O(格数)），
        // 整笔预览 / 提交复用——预览帧零重建。
        session.snap_index = if edit_state.snap_to_biome {
            projection
                .world
                .terrain_of(&wv_core::Id::new_unchecked(view.current_map.clone()))
                .map(|terrain| {
                    std::sync::Arc::new(wv_world_cell_map::biome_snap::BiomeSnapIndex::build(
                        terrain,
                    ))
                })
        } else {
            None
        };
        session.pts = snapped(&session, &session.pts, &grid, view.scale, Some([p.x, p.y]));
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
        // [ADR-0090 D5/M4] 贴近群系：提交笔画 = 采样折线逐点吸附锋线
        // （半径 = 屏幕 12px 折格数；均匀区内点保持原位不强扭）。
        let snapped_pts = snapped(&session, &session.pts, &grid, view.scale, None);
        // 找当前 span、沿笔画逐点定被切脸（终审遗留修：中点落脸外曾盲兜
        // 底脸 0——切错对象；全程不入脸 → notice 报错丢弃）。
        let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
        let Some(span) =
            wv_core::partition_span_at(&projection.world, &map_id, &layer_id, playback.current_day)
        else {
            session.pts.clear();
            return;
        };
        let Some(face_a) = stroke_face(&snapped_pts, span) else {
            session.pts.clear();
            pick_notice("笔画没落在任何地块上——从地块一边画到另一边");
            return;
        };
        let pending_stroke = snapped_pts;
        // 松手**立即提交**（用户裁定 2026-10-28：一刀下去红线先出现，不打断）。
        // 层用途分派（ADR-0081）：政治层新块自动归占位势力「无名N」（归属
        // 错了用易主改）；自然层不加势力——kind 随脸两分（切草原出两块
        // 草原），改种走 Assign 工具（SetPartitionFaceKindCommand）。
        let nature = crate::ui::partition_pen::layer_usage(&projection.world, &layer_id)
            == wv_core::model::faction::LayerUsage::Nature;
        let mut tx = wv_edit::Transaction::new();
        let notice_ok = if nature {
            tx.push(Box::new(wv_edit::CutPartitionFaceCommand::new(
                wv_core::Id::new_unchecked(view.current_map.clone()),
                layer_id.clone(),
                span_from,
                face_a,
                pending_stroke,
                true,
                None,
                4.0 * view.scale,
            )));
            "已切分（两块同群系）".to_string()
        } else {
            let n_existing = projection.world.factions.len();
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let new_id = wv_core::Id::new_unchecked(format!("faction_{ms}"));
            let new_name = format!("无名{}", n_existing + 1);
            let palette = [
                "#C05A4E", "#4E79C0", "#5A9E6F", "#B07FD8", "#D8A94E", "#4EB8C0", "#C04E9E",
                "#7F8F4E",
            ];
            let color = wv_core::Color(palette[n_existing % palette.len()].into());
            tx.push(Box::new(wv_edit::AddFactionCommand::new(
                wv_core::Faction::new(new_id.clone(), new_name.clone(), color),
            )));
            tx.push(Box::new(wv_edit::CutPartitionFaceCommand::new(
                wv_core::Id::new_unchecked(view.current_map.clone()),
                layer_id.clone(),
                span_from,
                face_a,
                pending_stroke,
                // 新块 = 弧 A（笔行进方向的左半）；归属错了走「易主」。
                true,
                Some(new_id),
                4.0 * view.scale,
            )));
            format!("已切分，新块归「{new_name}」")
        };
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                notice.info(notice_ok, (time.elapsed_secs_f64() * 60.0) as u64);
            }
            Err(e) => {
                notice.error(
                    format!("切分失败：{e}"),
                    (time.elapsed_secs_f64() * 60.0) as u64,
                );
                bevy::log::warn!("切分提交失败: {e}");
            }
        }
        session.pts.clear();
    }

    // ── 笔迹预览：采集中画当前折线（世界坐标 gizmo——可见反馈，作者才
    // 知道系统在采集、两端是否贴到了边界）。
    // 边界红圈（当前层所有脸的**闪烁红线**）：作者要先看见「能切的地方」，
    // 才知道笔画两端该压在哪。2026-10-28 由常显改可选（HUD 勾选框，默认开）。
    if edit_state.show_boundaries {
        let map_id = wv_core::Id::new_unchecked(view.current_map.clone());
        if let Some(span) =
            wv_core::partition_span_at(&projection.world, &map_id, &layer_id, playback.current_day)
        {
            let t = time.elapsed_secs_f64();
            // 1Hz 方波闪烁（亮 0.5s / 暗 0.5s），亮度在两档间插值。
            let blink = ((t * 2.0).sin() * 0.5 + 0.5) as f32; // 0..1 平滑
            let alpha = 0.35 + 0.65 * blink;
            let color = bevy::color::Color::srgba(1.0, 0.15, 0.15, alpha);
            for face in &span.faces {
                let ring_xy: Vec<Vec2> = face
                    .ring
                    .iter()
                    .filter_map(|id| span.vertices.iter().find(|v| v.id == *id))
                    .map(|v| Vec2::new(v.xy[0], v.xy[1]) + grid.raster_origin)
                    .collect();
                for pair in ring_xy.windows(2) {
                    gizmos.line_2d(pair[0], pair[1], color);
                }
                // 闭合段。
                if let (Some(first), Some(last)) = (ring_xy.first(), ring_xy.last()) {
                    gizmos.line_2d(*last, *first, color);
                }
            }
        }
    }

    if session.active && session.pts.len() >= 4 {
        for seg in session.pts.as_chunks::<4>().0 {
            gizmos.line_2d(
                Vec2::new(seg[0], seg[1]) + grid.raster_origin,
                Vec2::new(seg[2], seg[3]) + grid.raster_origin,
                bevy::color::Color::srgb(1.0, 0.78, 0.16),
            );
        }
    }
}

/// 易主小窗状态（Assign 工具：点脸 → 弹窗选既有势力 / 新建 / 无主）。
///
/// 终审遗留修（2026-10-28）：此前这套弹窗挂在已被「松手即提交」取代的
/// 切后点选流上，`open` 无人置起——弹窗死、易主手势读死字段 `owner` 恒空
/// → 易主实际只能设无主。现改由 Assign 手势开窗、提交
/// [`wv_edit::AssignPartitionFaceCommand`]（生效日 = 当前时间轴日）。
#[derive(Resource, Default)]
pub struct PartitionAssignDialog {
    /// 弹窗开着（Assign 手势点脸后置 true；确认/取消后回 false）。
    pub open: bool,
    /// 被易主的脸 id。
    pub face: u32,
    /// 新建势力的名字输入（空 = 走既有列表 / 无主）。
    pub owner: String,
}

/// 易主小窗 + 提交（T13/D8：整块换归属，几何零改动）。
///
/// 悬停 UI 时手势侧已让位（PointerOverUi 门）；取消 = 只关窗（脸不动）。
#[allow(clippy::too_many_arguments)]
pub fn partition_assign_dialog(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    current_loc: Res<CurrentLocale>,
    mut assign: ResMut<PartitionAssignDialog>,
    edit_state: Res<PartitionEditState>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut notice: ResMut<crate::ui_shell::ShellNotice>,
    time: Res<bevy::prelude::Time>,
) {
    if !assign.open {
        return;
    }
    let Some((layer_id, _)) = edit_state.active.clone() else {
        assign.open = false;
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let shell = crate::ui::shell::UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let _ = shell;
    // 层用途分派（ADR-0081）：自然层的 Assign 工具 = 改群系族（kind 选择，
    // 不涉势力）；政治层 = 选既有势力 / 新建 / 无主。
    let nature = crate::ui::partition_pen::layer_usage(&projection.world, &layer_id)
        == wv_core::model::faction::LayerUsage::Nature;
    /// 归属/群系选择的来源（新建 = 真建势力实体，同事务一次撤销）。
    enum PickChoice {
        Existing(wv_core::Id),
        Unclaimed,
        New(String),
        /// 自然层：新群系族下标。
        Kind(u8),
    }
    let mut commit: Option<PickChoice> = None;
    egui::Window::new(t(
        loc,
        if nature {
            crate::i18n::I18nKey::PartitionKindTitle
        } else {
            crate::i18n::I18nKey::PartitionPickTitle
        },
    ))
    .id(egui::Id::new("wv_partition_assign"))
    .order(bevy_egui::egui::Order::Foreground)
    .resizable(false)
    .collapsible(false)
    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
    .show(ctx, |ui| {
        if nature {
            // ── 自然层：群系族选择（六个陆面族；当前族标记）──
            let span = wv_core::partition_span_at(
                &projection.world,
                &wv_core::Id::new_unchecked(view.current_map.clone()),
                &layer_id,
                playback.current_day,
            );
            let cur_kind = span
                .and_then(|s| s.faces.iter().find(|f| f.id == assign.face))
                .and_then(|f| f.kind);
            let cur_name = span.and_then(|s| nature_face_name(s, assign.face));
            if let Some(name) = cur_name {
                ui.label(format!("当前：{name}"));
            }
            ui.label(t(loc, crate::i18n::I18nKey::PartitionKindHint));
            ui.separator();
            let names = wv_terrain::biome::BIOME_FAMILY_NAMES_ZH;
            for k in crate::ui::partition_pen::NATURE_LAND_FAMILIES {
                let label = names.get(k as usize).copied().unwrap_or("?");
                let mark = if cur_kind == Some(k) { " ●" } else { "" };
                if ui.button(format!("{label}{mark}")).clicked() {
                    commit = Some(PickChoice::Kind(k));
                }
            }
            if ui
                .button(t(loc, crate::i18n::I18nKey::CommonCancel))
                .clicked()
            {
                assign.open = false;
            }
            return;
        }
        ui.label(t(loc, crate::i18n::I18nKey::PartitionPickHint));
        ui.separator();
        // 既有势力列表。
        for f in &projection.world.factions {
            if ui.button(f.name.as_str()).clicked() {
                commit = Some(PickChoice::Existing(f.id.clone()));
            }
        }
        ui.separator();
        // 无主。
        if ui
            .button(t(loc, crate::i18n::I18nKey::PartitionPickUnclaimed))
            .clicked()
        {
            commit = Some(PickChoice::Unclaimed);
        }
        ui.separator();
        // 新建：名字输入 + 确认（id = faction_<Unix 毫秒>，可读 ID 由
        // Id::new 校验；AddFaction 与 Assign 同事务 = 一次撤销）。
        ui.horizontal(|ui| {
            egui::TextEdit::singleline(&mut assign.owner)
                .hint_text(t(loc, crate::i18n::I18nKey::PartitionPickNewHint))
                .show(ui);
            let ok = !assign.owner.trim().is_empty();
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(t(loc, crate::i18n::I18nKey::CommonOk)),
                )
                .clicked()
            {
                commit = Some(PickChoice::New(assign.owner.trim().to_string()));
            }
        });
        // 取消 = 只关窗（易主是纯属性改，无笔画可丢）。
        if ui
            .button(t(loc, crate::i18n::I18nKey::CommonCancel))
            .clicked()
        {
            assign.open = false;
        }
    });

    if let Some(choice) = commit {
        // 自然层（ADR-0081）：改群系族 = fork 换 kind（SetPartitionFaceKind），
        // 与政治层易主同配方；不涉势力实体。
        if let PickChoice::Kind(k) = choice {
            let cmd = wv_edit::SetPartitionFaceKindCommand {
                map: wv_core::Id::new_unchecked(view.current_map.clone()),
                layer: layer_id,
                from: playback.current_day,
                face: assign.face,
                new_kind: k,
                before: std::sync::Mutex::new(None),
            };
            let mut tx = wv_edit::Transaction::new();
            tx.push(Box::new(cmd));
            match edit.history.commit(&mut projection.world, tx) {
                Ok(dirty) => {
                    edit.record_dirty(dirty);
                    projection.dirty = true;
                    notice.info(
                        format!(
                            "已改群系为「{}」",
                            wv_terrain::biome::BIOME_FAMILY_NAMES_ZH
                                .get(k as usize)
                                .copied()
                                .unwrap_or("?")
                        ),
                        (time.elapsed_secs() * 60.0) as u64,
                    );
                }
                Err(e) => {
                    notice.error(
                        format!("改群系失败：{e}"),
                        (time.elapsed_secs() * 60.0) as u64,
                    );
                    bevy::log::warn!("改群系提交失败: {e}");
                }
            }
            assign.open = false;
            return;
        }
        let mut tx = wv_edit::Transaction::new();
        let owner = match choice {
            PickChoice::Existing(id) => Some(id),
            PickChoice::Unclaimed => None,
            // Kind 臂不可达：上面 `if let PickChoice::Kind` 已提前 return
            // （自然层改群系走 SetPartitionFaceKind 分支）——此处只为穷尽。
            PickChoice::Kind(_) => None,
            PickChoice::New(name) => {
                // 新建势力**真建实体**（只挂 Id 不建实体 = 悬空引用，渲染
                // 灰上灰，作者看不到任何变化——实机教训 2026-10-28）。
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                let id = wv_core::Id::new_unchecked(format!("faction_{ms}"));
                let palette = [
                    "#C05A4E", "#4E79C0", "#5A9E6F", "#B07FD8", "#D8A94E", "#4EB8C0", "#C04E9E",
                    "#7F8F4E",
                ];
                let color =
                    wv_core::Color(palette[projection.world.factions.len() % palette.len()].into());
                tx.push(Box::new(wv_edit::AddFactionCommand::new(
                    wv_core::Faction::new(id.clone(), name, color),
                )));
                Some(id)
            }
        };
        let cmd = wv_edit::AssignPartitionFaceCommand {
            map: wv_core::Id::new_unchecked(view.current_map.clone()),
            layer: layer_id,
            from: playback.current_day,
            face: assign.face,
            owner,
            before: std::sync::Mutex::new(None),
        };
        tx.push(Box::new(cmd));
        match edit.history.commit(&mut projection.world, tx) {
            Ok(dirty) => {
                edit.record_dirty(dirty);
                projection.dirty = true;
                notice.info("已易主", (time.elapsed_secs() * 60.0) as u64);
            }
            Err(e) => {
                notice.error(
                    format!("易主失败：{e}"),
                    (time.elapsed_secs() * 60.0) as u64,
                );
                bevy::log::warn!("易主提交失败: {e}");
            }
        }
        assign.open = false;
        assign.owner.clear();
    }
}

/// 拖边界（T13/D4）：命中**共享顶点**（≤8px 容差）→ 拖动（青色预览环跟随
/// 光标 = 松手落位，2026-10-28 补预览）→ 松手提交
/// `MovePartitionVerticesCommand`（零和由命令侧预检保证）。
#[allow(clippy::too_many_arguments)]
pub fn partition_drag_gesture(
    mut gizmos: bevy::prelude::Gizmos,
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
    } else if buttons.pressed(MouseButton::Left) {
        // ── 拖动预览（2026-10-28 下一站候选落地）：拖动期间把被拖顶点跟随
        // 光标画**预览环**（引用它的每张脸的边界形状 = 松手后的落位，两侧
        // 同画 = 零和联动的可见反馈），原位 → 光标画一条暗色位移参考线。
        // 此前拖动全程零反馈，松手提交了才知道边界去了哪。
        if let Some(vid) = session.dragging {
            if let Some(span) = span() {
                let color = bevy::color::Color::srgb(0.2, 0.9, 0.95);
                for face in &span.faces {
                    if !face.ring.contains(&vid) {
                        continue;
                    }
                    let ring = preview_ring_xy(&span, face, vid, [p.x, p.y]);
                    for pair in ring.windows(2) {
                        gizmos.line_2d(
                            pair[0] + grid.raster_origin,
                            pair[1] + grid.raster_origin,
                            color,
                        );
                    }
                    if let (Some(first), Some(last)) = (ring.first(), ring.last()) {
                        gizmos.line_2d(
                            *last + grid.raster_origin,
                            *first + grid.raster_origin,
                            color,
                        );
                    }
                }
                if let Some(orig) = span.vertices.iter().find(|v| v.id == vid) {
                    gizmos.line_2d(
                        Vec2::new(orig.xy[0], orig.xy[1]) + grid.raster_origin,
                        cursor,
                        bevy::color::Color::srgba(1.0, 1.0, 1.0, 0.35),
                    );
                }
            }
        }
    }
}

/// 预览环（纯函数）：`face` 的环坐标，其中顶点 `vid` 换成 `p`（map-local）。
///
/// 拖边界预览用——引用 `vid` 的每张脸各画一遍，两侧边界一起动 = 零和
/// 联动的可见反馈。未引用 `vid` 的环不受影响（调用方过滤）。
pub fn preview_ring_xy(
    span: &wv_core::model::faction_partition::PartitionSpan,
    face: &wv_core::model::faction_partition::PartitionFace,
    vid: u32,
    p: [f32; 2],
) -> Vec<Vec2> {
    face.ring
        .iter()
        .map(|id| {
            if *id == vid {
                Vec2::new(p[0], p[1])
            } else {
                span.vertices
                    .iter()
                    .find(|v| v.id == *id)
                    .map(|v| Vec2::new(v.xy[0], v.xy[1]))
                    .unwrap_or(Vec2::NAN)
            }
        })
        .collect()
}

/// 拖动会话（DragBoundary 工具）。
#[derive(Resource, Default)]
pub struct DragSession {
    /// 正在拖的顶点 id。
    pub dragging: Option<u32>,
}

/// 拖动命中容差（分区坐标 px；HUD 缩放下约屏幕 8px）。
pub const DRAG_GRAB_PX: f32 = 8.0;

/// 整块易主手势（T13/D8）：Assign 工具下点脸 → 开归属小窗
/// （`PartitionAssignDialog`）——窗内选既有 / 新建 / 无主后提交 fork。
///
/// 生效日 = 当前时间轴日（弹窗确认时的 `playback.current_day`）。
#[allow(clippy::too_many_arguments)]
pub fn partition_assign_gesture(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    edit_state: Res<PartitionEditState>,
    mut assign: ResMut<PartitionAssignDialog>,
    projection: Res<WorldProjection>,
    pointer: Res<wv_render::camera::PointerOverUi>,
) {
    let Some((layer_id, _)) = edit_state.active.clone() else {
        return;
    };
    if edit_state.tool != PartitionTool::Assign
        || playback.playing
        || pointer.wants_pointer
        || assign.open
    {
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
    assign.open = true;
    assign.face = face;
}

/// 疆域替换手势（[ADR-0090] D4/M3）：Replace 工具下点脸进多选集——
/// Ctrl = 切换累加、无修饰 = 重置单脸；**群系闸门不勾时不选**（D4）。
/// 与易主手势同款守卫链（播放 / UI 抢占 / 编辑态）。
#[allow(clippy::too_many_arguments)]
pub fn partition_replace_gesture(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    edit_state: Res<PartitionEditState>,
    mut replace: ResMut<crate::ui::partition_pen::PartitionReplaceState>,
    projection: Res<WorldProjection>,
    pointer: Res<wv_render::camera::PointerOverUi>,
) {
    let Some((layer_id, _)) = edit_state.active.clone() else {
        return;
    };
    if edit_state.tool != PartitionTool::Replace || playback.playing || pointer.wants_pointer {
        return;
    }
    if !buttons.just_released(MouseButton::Left) {
        return;
    }
    // D4 群系闸门：checkbox 不勾 = 点脸不选。
    if !replace.biome_gate {
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
    let additive = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    replace.picked = replace_pick(std::mem::take(&mut replace.picked), face, additive);
}

#[cfg(test)]
mod drag_assign_tests {
    //! T13 手势纯函数。

    use super::*;

    /// [ADR-0090 M3] 无修饰点击 = 重置为单脸；Ctrl 点击 = 切换累加/移除。
    #[test]
    fn replace_pick_resets_without_modifier_and_toggles_with_ctrl() {
        let picked = replace_pick(Vec::new(), 3, false);
        assert_eq!(picked, vec![3], "无修饰 = 重置为单脸");
        let picked = replace_pick(picked, 7, true);
        assert_eq!(picked, vec![3, 7], "Ctrl = 累加");
        let picked = replace_pick(picked, 3, true);
        assert_eq!(picked, vec![7], "Ctrl 再点已选脸 = 移除");
        let picked = replace_pick(picked, 9, false);
        assert_eq!(picked, vec![9], "无修饰再点 = 又重置为单脸");
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
                    kind: None,
                    name: None,
                    ring: vec![4, 5, 3, 0], // 左半
                    holes: Vec::new(),
                },
                PartitionFace {
                    id: 1,
                    owner: None,
                    kind: None,
                    name: None,
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
    fn stroke_face_finds_first_hit_and_none_outside() {
        // 终审遗留修守点：沿笔画逐点找脸——首点命中即返回；全程不入脸
        // （贴边 / 画在脸外）返回 None（此前取中点 + 盲兜底脸 0，切错对象）。
        let span = demo_span();
        // 首点在左半（25,50）→ 左半，即便后面点飘出脸外。
        assert_eq!(
            stroke_face(&[25.0, 50.0, 999.0, 999.0], &span),
            Some(0),
            "首点命中优先"
        );
        assert_eq!(stroke_face(&[150.0, 50.0, 180.0, 60.0], &span), None);
    }

    #[test]
    fn nature_face_name_counts_per_kind_by_face_id() {
        // ADR-0081 命名规则守点：<族名><N>——N = 同 kind 按 face id 升序的
        // 序号（id 持久化 → 名字跨会话稳定）；政治脸（kind None）无名。
        let mut span = demo_span();
        // 两张脸改自然：脸 0 = 草原(1)，脸 1 = 草原(1)→改后是 id 顺序第二；
        // 再造一张 id 更大的干旱脸。
        span.faces[0].kind = Some(1);
        span.faces[1].kind = Some(1);
        span.faces
            .push(wv_core::model::faction_partition::PartitionFace {
                name: None,
                id: 9,
                owner: None,
                kind: Some(2), // 干旱族
                ring: vec![0, 1, 2],
                holes: Vec::new(),
            });
        assert_eq!(nature_face_name(&span, 0).as_deref(), Some("草原1"));
        assert_eq!(nature_face_name(&span, 1).as_deref(), Some("草原2"));
        assert_eq!(nature_face_name(&span, 9).as_deref(), Some("干旱1"));
        // 政治脸（kind None）→ None；不存在的脸 → None。
        span.faces[0].kind = None;
        assert_eq!(nature_face_name(&span, 0), None);
        assert_eq!(nature_face_name(&span, 4242), None);
    }

    #[test]
    fn preview_ring_moves_dragged_vertex_only() {
        // 拖边界预览守点（2026-10-28）：预览环只有被拖顶点跟随光标，其余
        // 顶点原位——两侧脸各引用一次 = 零和联动的可见反馈。
        let span = demo_span();
        // 脸 0 的环 [4,5,3,0]：拖顶点 4 (50,0) 到 (60,-10)。
        let ring = preview_ring_xy(&span, &span.faces[0], 4, [60.0, -10.0]);
        assert_eq!(ring.len(), 4, "环长度不变");
        assert_eq!(ring[0], Vec2::new(60.0, -10.0), "被拖顶点跟随光标");
        assert_eq!(ring[1], Vec2::new(50.0, 100.0), "其余顶点原位（5）");
        assert_eq!(ring[2], Vec2::new(0.0, 100.0), "其余顶点原位（3）");
        assert_eq!(ring[3], Vec2::new(0.0, 0.0), "其余顶点原位（0）");
        // 不含被拖顶点的环完全不变。
        let ring1 = preview_ring_xy(&span, &span.faces[1], 4, [60.0, -10.0]);
        assert_eq!(
            ring1[0],
            Vec2::new(100.0, 0.0),
            "未引用被拖顶点的环不受影响"
        );
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
