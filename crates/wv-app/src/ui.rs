//! UI 面板：全部为**浮动窗体**（基线 §1.11 R1–R4）。
//!
//! ## 依据（用户本轮反馈 + 基线）
//!
//! - **地图铺满**（§1.11 R1）：**不再使用 `SidePanel` / `TopBottomPanel`**，
//!   地图区永远是最底层且占满窗口，所有面板用 `egui::Window` 浮在其上。
//!   （用户原话：「地图可视面积太少，整体设计要以地图为主」）
//! - **面板不推挤地图**（R2）：面板出现/消失**不改地图坐标系**。
//! - **顶栏五项**（§1.12）：项目 / 视图 / 日期 / 坐标 / 比例尺。
//! - **顶部菜单**（用户 1.3）：所有浮动窗的开关 + 重置位置。
//! - **地形设计器**（用户 1.1）：紧凑 Photoshop 式浮动窗，2 列 × 2 图标。
//! - **小地图**（用户 1.2）：浮动窗，内嵌图层开关。
//! - **事件/人物登记**（用户 1.4）：浮动窗；事件登记可从时间轴进入。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{
    BrushState, GridConfig, Panel, PanelState, PlaybackState, ViewMode, ViewState, WorldProjection,
};

/// 顶栏：菜单 + 项目 / 视图 / 日期 / 坐标 / 比例尺（基线 §1.12）。
///
/// 顶栏用 `TopBottomPanel` 是**唯一例外**——它是固定高 32px 的信息条，
/// 不属于「浮动面板」，不会显著侵占地图（§1.11 的布局图亦如此）。
pub fn top_bar(
    mut contexts: EguiContexts,
    projection: Res<WorldProjection>,
    mut view: ResMut<ViewState>,
    playback: Res<PlaybackState>,
    mut panels: ResMut<PanelState>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    egui::TopBottomPanel::top("wv_top_bar")
        .exact_height(32.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // ① 菜单：所有浮动窗的开关 + 重置位置（用户 1.3）
                ui.menu_button("窗口", |ui| {
                    for p in Panel::ALL {
                        let mut on = panels.is_visible(p);
                        if ui.checkbox(&mut on, p.title()).changed() {
                            panels.set_visible(p, on);
                        }
                    }
                    ui.separator();
                    if ui.button("重置所有面板位置").clicked() {
                        panels.reset_requested = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.checkbox(&mut panels.immersive, "沉浸模式（地图专属视图）");
                });
                ui.separator();

                // 加载失败时优先显示醒目警告，而不是假装一切正常。
                if let Some(err) = &projection.load_error {
                    ui.colored_label(egui::Color32::from_rgb(0xE0, 0x4F, 0x4F), "⚠ 工程未加载");
                    ui.separator();
                    ui.label(err);
                    return;
                }

                // ② 项目 + 脏标记
                let dirty = if projection.dirty { " ●" } else { "" };
                let name = projection
                    .world
                    .meta
                    .as_ref()
                    .map(|m| m.name.clone())
                    .unwrap_or_else(|| "（未命名工程）".into());
                ui.label(format!("📁 {name}{dirty}"));
                ui.separator();

                // ③ 视图：当前图 + 模式
                ui.menu_button(format!("视图[{}]", view.mode.label()), |ui| {
                    for mode in ViewMode::ALL {
                        if ui
                            .selectable_label(view.mode == mode, mode.label())
                            .clicked()
                        {
                            view.mode = mode;
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();

                // ④ 日期：当前时间轴位置，**经 Calendar 派生显示**
                let date = projection
                    .world
                    .timeline
                    .as_ref()
                    .map(|tl| tl.calendar.format(playback.current_day))
                    .unwrap_or_else(|| format!("@day{}", playback.current_day));
                ui.label(format!("· {date}"));
                ui.separator();

                // ⑤ 坐标：鼠标世界坐标
                let coord = match view.cursor_world {
                    Some(c) => format!("({:.0}, {:.0})", c.x, c.y),
                    None => "(—, —)".into(),
                };
                ui.label(coord);
                ui.separator();

                // ⑥ 比例尺：派生显示
                ui.label(format!("🌍 1:{:.0}", 1.0 / view.scale.max(1e-4) * 1000.0));
            });
        });
}

/// 统一样式的浮动窗外壳：无边框、半透明、可拖拽、可关闭（§1.11 R3/R4）。
///
/// 返回 `Some(ui)` 表示本帧应绘制内容；`None` 表示面板不可见。
fn float_window(
    ctx: &egui::Context,
    panels: &mut PanelState,
    panel: Panel,
    default_size: [f32; 2],
    resizable: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    if !panels.is_visible(panel) {
        return;
    }

    let mut open = true;
    let id = egui::Id::new(("wv_panel", panel));
    let reset = panels.reset_requested;

    // 沉浸模式：整体淡化但**仍可交互**（动画 §4：alpha 0.10）。
    let alpha = if panels.immersive { 0.10 } else { 0.92 };

    let mut win = egui::Window::new(panel.title())
        .id(id)
        .open(&mut open)
        .default_pos(panel.default_pos())
        .default_size(default_size)
        .resizable(resizable)
        .collapsible(true)
        .title_bar(true);

    if reset {
        win = win.default_pos(panel.default_pos());
    }

    let frame = egui::Frame::window(&ctx.style())
        .fill(ctx.style().visuals.window_fill.gamma_multiply(alpha))
        .inner_margin(egui::Margin::same(8.0));

    win.frame(frame).show(ctx, |ui| {
        body(ui);
    });

    if !open {
        panels.set_visible(panel, false);
    }
}

/// 地图树面板（P-1）。S3 骨架只有单张图，先给出结构占位。
pub fn panel_map_tree(mut contexts: EguiContexts, mut panels: ResMut<PanelState>, view: Res<ViewState>) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(ctx, &mut panels, Panel::MapTree, [200.0, 280.0], true, |ui| {
        ui.label("World → Region → City → Interior");
        ui.separator();
        let _ = ui.selectable_label(true, format!("🌍 {}", view.current_map));
        ui.weak("（子图层级在 S12 后展开）");
    });
}

/// 图层管理面板（P-2）。
pub fn panel_layers(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Layers, [200.0, 160.0], true, |ui| {
        ui.checkbox(&mut view.terrain_visible, "地形");
        ui.checkbox(&mut view.events_visible, "事件点");
        ui.checkbox(&mut view.tracks_visible, "轨迹");
        ui.checkbox(&mut view.roads_visible, "道路");
        ui.separator();
        ui.label(format!("缩放: {:.2}×", view.scale));
    });
}

/// **地形设计器**（P-3，用户 1.1）：紧凑 Photoshop 式浮动窗。
///
/// 用户要求：**2 列 × 2 图标**、可移动、可关闭。
/// 因此地形类型用**图标网格**（每行 2 个），而非竖向长列表。
pub fn panel_brush(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    mut brush: ResMut<BrushState>,
    grid: Res<GridConfig>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Brush, [176.0, 300.0], true, |ui| {
        ui.checkbox(&mut brush.enabled, "启用笔刷");
        ui.separator();

        // 地形类型：2 列图标网格（10 项 → 5 行）
        ui.label("地形");
        egui::Grid::new("terrain_icons")
            .num_columns(2)
            .spacing([4.0, 4.0])
            .show(ui, |ui| {
                for k in 0u8..10 {
                    let icon = TERRAIN_ICONS[k as usize];
                    let name = TERRAIN_NAMES[k as usize];
                    let selected = brush.kind == k;
                    // 图标按钮：固定 52×36，紧凑
                    let btn = egui::Button::new(
                        egui::RichText::new(format!("{icon}\n{name}"))
                            .size(11.0)
                            .line_height(Some(12.0)),
                    )
                    .min_size(egui::vec2(66.0, 36.0))
                    .selected(selected);
                    if ui.add(btn).clicked() {
                        brush.kind = k;
                    }
                    if (k + 1) % 2 == 0 {
                        ui.end_row();
                    }
                }
            });
        ui.separator();

        ui.label("半径");
        ui.add(
            egui::Slider::new(&mut brush.radius, grid.cell_size * 2.0..=600.0)
                .suffix(" wu")
                .logarithmic(true),
        );
        ui.label("衰减软度");
        ui.add(egui::Slider::new(&mut brush.softness, 0.0..=1.0));
        ui.separator();
        ui.weak("左键涂抹 · Ctrl+滚轮改半径");
    });
}

/// 小地图（用户 1.2）：浮动窗 + **图层开关**。
///
/// 当前用「示意缩略」近似——把整幅地形的**缩略色块**画出来，
/// 并以矩形框标出当前视口范围。
pub fn panel_minimap(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    mut view: ResMut<ViewState>,
    grid: Res<GridConfig>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Minimap, [200.0, 260.0], true, |ui| {
        // ---- 缩略图 ----
        let side = 168.0;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
        let painter = ui.painter();
        // 底（地表色，用美术 §1.4.2 的 core 令牌近似）
        painter.rect_filled(rect, 2.0_f32, egui::Color32::from_rgb(0x2A, 0x2D, 0x33));
        painter.rect_stroke(
            rect,
            2.0_f32,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(0x4A, 0x4E, 0x57)),
        );
        // 视口矩形（由相机中心 + 缩放派生）
        let half = 1.0 / view.scale.max(1e-4);
        let cx = 0.5 + (view.camera_center.x / (grid.cell_size * 512.0)).clamp(-0.5, 0.5);
        let cy = 0.5 + (view.camera_center.y / (grid.cell_size * 512.0)).clamp(-0.5, 0.5);
        let vw = (half * 0.5).clamp(0.05, 1.0);
        let vp = egui::Rect::from_center_size(
            egui::pos2(rect.left() + rect.width() * cx, rect.top() + rect.height() * cy),
            egui::vec2(rect.width() * vw, rect.height() * vw),
        );
        painter.rect_stroke(
            vp,
            1.0_f32,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(0xE8, 0xB3, 0x4D)),
        );
        // 点击小地图 → 把相机中心移过去
        if response.clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let nx = (p.x - rect.left()) / rect.width() - 0.5;
                let ny = (p.y - rect.top()) / rect.height() - 0.5;
                view.camera_center = Vec2::new(
                    nx * grid.cell_size * 512.0,
                    ny * grid.cell_size * 512.0,
                );
            }
        }
        ui.add_space(4.0);

        // ---- 图层开关（用户 1.2 明确要求）----
        ui.separator();
        ui.strong("图层");
        ui.checkbox(&mut view.terrain_visible, "地形");
        ui.checkbox(&mut view.events_visible, "事件");
        ui.checkbox(&mut view.tracks_visible, "轨迹");
        ui.checkbox(&mut view.roads_visible, "道路");
    });
}

/// 属性检查器（P-4）。
pub fn panel_inspector(mut contexts: EguiContexts, mut panels: ResMut<PanelState>) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Inspector, [260.0, 220.0], true, |ui| {
        ui.weak("未选中对象");
        ui.separator();
        ui.label("在图上点选地块/事件/人物后，此处显示其全部属性。");
        ui.add_space(4.0);
        ui.weak("（选择系统属 S12 后）");
    });
}

/// 底部时间轴 + 播放控件（P-5，基线 §1.8）。
///
/// 用户 1.4 要求：**事件登记要在时间轴这里加入口**。
pub fn panel_timeline(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    mut playback: ResMut<PlaybackState>,
    projection: Res<WorldProjection>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    let mut open_event = false;
    float_window(&ctx, &mut panels, Panel::Timeline, [560.0, 150.0], true, |ui| {
        ui.horizontal(|ui| {
            let play_label = if playback.playing { "⏸ 暂停" } else { "▶ 播放" };
            if ui.button(play_label).clicked() {
                playback.playing = !playback.playing;
            }
            for s in [1.0f32, 5.0, 30.0] {
                if ui
                    .selectable_label(
                        (playback.speed - s).abs() < f32::EPSILON,
                        format!("{s:.0}天/秒"),
                    )
                    .clicked()
                {
                    playback.speed = s;
                }
            }
            ui.separator();
            // **事件登记入口**（用户 1.4）
            if ui.button("＋ 登记事件").clicked() {
                open_event = true;
            }
            ui.separator();
            let date = projection
                .world
                .timeline
                .as_ref()
                .map(|tl| tl.calendar.format(playback.current_day))
                .unwrap_or_else(|| format!("@day{}", playback.current_day));
            ui.label(date);
        });

        ui.add_space(4.0);

        let mut day = playback.current_day as f64;
        ui.horizontal(|ui| {
            if ui.small_button("|◀").clicked() {
                playback.current_day = playback.range_from;
            }
            if ui.small_button("◀").clicked() {
                playback.current_day = (playback.current_day - 30).max(playback.range_from);
            }
            if ui.small_button("▶|").clicked() {
                playback.current_day = playback.range_to;
            }
            ui.add(
                egui::Slider::new(
                    &mut day,
                    (playback.range_from as f64)..=(playback.range_to as f64),
                )
                .show_value(false),
            );
        });
        playback.current_day = day as i64;

        let situations = &projection.world.situations;
        if !situations.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label("事态:");
                for sit in situations {
                    if ui.selectable_label(false, &sit.name).clicked() {
                        playback.range_from = sit.span.from;
                        playback.range_to = sit.span.to.unwrap_or(sit.span.from + 365);
                        playback.current_day = playback.range_from;
                    }
                }
            });
        }
    });

    if open_event {
        panels.set_visible(Panel::Event, true);
    }
}

/// 事态登记（P-7）。
pub fn panel_situation(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Situation, [300.0, 260.0], true, |ui| {
        let sits = &projection.world.situations;
        if sits.is_empty() {
            ui.weak("暂无已登记事态");
        } else {
            for s in sits {
                ui.group(|ui| {
                    ui.strong(&s.name);
                    let span = match s.span.to {
                        Some(to) => format!("@day{} – @day{}", s.span.from, to),
                        None => format!("@day{} – 至今", s.span.from),
                    };
                    ui.label(span);
                });
            }
        }
    });
}

/// 势力编辑（P-6）。
pub fn panel_faction(mut contexts: EguiContexts, mut panels: ResMut<PanelState>) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Faction, [280.0, 240.0], true, |ui| {
        ui.weak("势力层未加载");
        ui.separator();
        ui.label("势力层 / 势力 / 归属关系");
        ui.weak("（D5：几何绘制延后，模型在 S12 落地）");
    });
}

/// 校验面板（P-8）。
pub fn panel_validation(mut contexts: EguiContexts, mut panels: ResMut<PanelState>) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Validation, [300.0, 200.0], true, |ui| {
        ui.weak("暂无校验问题");
        ui.separator();
        ui.label("四维校验：时间 / 引用 / 几何 / 一致性");
        ui.weak("（引用完整性校验已落地，见 wv-io）");
    });
}

/// 人物登记（用户 1.4）。
pub fn panel_character(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Character, [280.0, 240.0], true, |ui| {
        let chars = &projection.world.characters;
        if chars.is_empty() {
            ui.weak("暂无已登记人物");
        } else {
            for c in chars {
                ui.label(&c.name);
            }
        }
    });
}

/// 事件登记（用户 1.4，可从时间轴进入）。
pub fn panel_event(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    projection: Res<WorldProjection>,
) {
    let ctx: &mut egui::Context = contexts.ctx_mut();
    float_window(&ctx, &mut panels, Panel::Event, [300.0, 260.0], true, |ui| {
        let events = &projection.world.events;
        if events.is_empty() {
            ui.weak("暂无已登记事件");
        } else {
            for e in events {
                ui.group(|ui| {
                    ui.strong(&e.id.as_str().to_string());
                    ui.label(format!("@day{} – @day{}", e.span.from, e.span.to.unwrap_or(e.span.from)));
                });
            }
        }
        ui.separator();
        ui.weak("（新增/编辑表单属 S12 后）");
    });
}

/// 地形类型名（供笔刷面板与图例）。
pub const TERRAIN_NAMES: [&str; 10] = [
    "深水", "浅水", "沙滩", "草原", "林地", "丘陵", "山地", "雪原", "沼泽", "道路",
];

/// 地形图标（紧凑设计器用；emoji 保证跨系统可读，中文名作副标题）。
pub const TERRAIN_ICONS: [&str; 10] = [
    "🌊", "💧", "🏖", "🌾", "🌲", "⛰", "🏔", "❄", "🪵", "🛤",
];
