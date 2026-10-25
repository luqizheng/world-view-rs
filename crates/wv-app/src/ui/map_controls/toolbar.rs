//! 工具条系统本体：槽位布局、工具互斥排队与视图开关。

use super::widgets::*;
use super::*;

/// 地图工具条（横向 / 纵向两形态）。
///
/// 顶部早返守卫与 `welcome_screen` 同源 —— 无工程时整条不渲染。
#[allow(clippy::too_many_arguments)]
pub fn map_controls(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut view: ResMut<ViewState>,
    measure: Res<MeasureState>,
    selection: Res<SelectionState>,
    brush: Res<BrushState>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    tex: Res<wv_render::terrain_view::TerrainTexture>,
    projection: Res<WorldProjection>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    screen: Res<crate::app_screen::AppScreen>,
    mut params: DockParams,
    edit: Res<EditHistory>,
    panels: Res<PanelState>,
    // CellMap 工具活动态（ADR-0057 补记 6：EditCell 第 12 槽 / PointSelect
    // 槽 1 / FloodSelect 第 13 槽）——瓷砖按各自 Resource 的 `tool_active`
    // 画选中态。时间轴播放态（§9.4 Gate-1）：播放中 CellMap 编辑类瓷砖灰化。
    //
    // **折成一层元组**：Bevy 单系统参数上限 16，本函数原有 15 个，四个
    // `Res` 分开写会到 19（编译报「not a system set」）。元组算 1 个参数。
    (edit_cell, point_select, flood_select, playback): (
        Res<wv_render::state::EditCellState>,
        Res<crate::state::PointSelectState>,
        Res<crate::state::FloodSelectState>,
        Res<crate::state::PlaybackState>,
    ),
) {
    // 无工程闸门（图片规格 §首页不可见）：与 `welcome_screen` 共用同一信号。
    // 2026-10-22（B 档）：改读 `AppScreen`——「首页不可见」的口径由屏状态
    // 一处裁决，不再在本系统重拼 `has_open_project || 加载中`。
    if !screen.editor_visible() {
        return;
    }

    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = params.locale.0;

    // 当前级的缩放区间与「全图」尺寸。
    let screen = ctx.content_rect().size();
    let window_size = Vec2::new(screen.x, screen.y);
    let raster_size = Vec2::new(tex.size as f32, tex.height as f32) * grid.cell_size;
    let level_map_size =
        wv_render::zoom_level::map_size_of(&projection.world, drill.current(), raster_size);
    let zoom = wv_render::zoom_level::LevelZoom::for_map(level_map_size, window_size);

    // 工具活动态（全部从资源读；工具切换本身走命令分发）。
    let select_active = selection.enabled;
    let pan_active = view.pan_tool;
    let eraser_active = brush.enabled && brush.tool == BrushTool::Eraser;
    let measure_active = measure.mode.is_active();
    // 清除按钮的可用态：当前图真有测量才点得动（空图按下去只会弹「没有测量」）。
    let has_measurements = drill
        .current()
        .and_then(|id| projection.world.measurement_of(id))
        .is_some_and(|o| !o.measurements.is_empty());
    let grid_active = view.grid_lines;
    let can_undo = edit.history.can_undo();
    let can_redo = edit.history.can_redo();
    // EditCell / PointSelect 活动态（ADR-0057 补记 6）：两件工具两个独立
    // Resource，判据各读各的 `tool_active`——不再共用 `allow_drag` 形态位。
    let edit_cell_active = edit_cell.tool_active;
    let point_select_active = point_select.tool_active;
    // 泛洪活动态（[ADR-0052] D6）：判据 = `tool_active`。
    let flood_select_active = flood_select.tool_active;
    // **Gate-1**（[ADR-0052] §9.4）：时间轴播放中，CellMap 写入口工具灰化。
    // 禁工具而非禁面板——播放中看读数条是合法只读操作。
    let cell_tools_enabled = !playback.playing;
    let locked_hint = t(loc, I18nKey::MapControlsPlaybackLocked);

    let vertical = params.dock.orientation == MapDockOrientation::Vertical;
    let metrics = DockMetrics::for_orientation(vertical);
    // 默认锚点（无拖动历史时）：纵向贴顶、横向贴底让时间轴。
    let (anchor, default_offset) = if vertical {
        (
            egui::Align2::RIGHT_TOP,
            egui::vec2(-EDGE_OFFSET, DOCK_TOP_CLEARANCE),
        )
    } else {
        let content = ctx.content_rect();
        let bottom_lift = if panels.is_visible(Panel::Timeline) {
            ctx.memory(|m| m.area_rect(egui::Id::new(("wv_panel", Panel::Timeline))))
                .filter(|r| r.top() > content.center().y)
                .map(|r| (content.bottom() - r.top()).max(0.0) + EDGE_OFFSET)
                .unwrap_or(EDGE_OFFSET)
        } else {
            EDGE_OFFSET
        };
        (egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -bottom_lift))
    };
    // 用户拖动 grip 后的位置偏移：仍是「相对锚点的屏幕偏移」，但不再
    // 是「贴边 -18」，而是任意 (x, y)。
    let offset = params
        .dock
        .pos
        .map(|p| egui::vec2(p.x, p.y))
        .unwrap_or(default_offset);
    let margin = margin_xy(metrics.pad_x, metrics.pad_y);

    egui::Area::new(egui::Id::new("wv_map_controls_v07"))
        .anchor(anchor, offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // 圆角大框：float_fill 底 + border_strong 描边 + 投影。
            egui::Frame::NONE
                .fill(shell.float_fill())
                .stroke(egui::Stroke::new(1.0, shell.border_strong()))
                .corner_radius(egui::CornerRadius::from(DOCK_RADIUS))
                .inner_margin(margin)
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(51),
                })
                .show(ui, |ui| {
                    let mut slots = |ui: &mut egui::Ui| {
                        // ── 槽 0：朝向切换 + 拖动手柄（grip-vertical） ─────
                        let (swap_icon, swap_hint) = if vertical {
                            (ICON_MOVE_HORIZONTAL, "拖动移动 / 单击切换为横向工具条")
                        } else {
                            (ICON_MOVE_VERTICAL, "拖动移动 / 单击切换为纵向工具条")
                        };
                        let grip_resp = grip_handle(ui, &shell, metrics, swap_icon, swap_hint);
                        if grip_resp.dragged() {
                            let pos = params.dock.pos.get_or_insert(MapDockPos {
                                x: default_offset.x,
                                y: default_offset.y,
                            });
                            pos.x += grip_resp.drag_delta().x;
                            pos.y += grip_resp.drag_delta().y;
                        }
                        if grip_resp.clicked() {
                            params.dock.orientation = params.dock.orientation.toggled();
                            // 朝向切换后旧 pos 不再合身：清掉，回到默认锚点。
                            params.dock.pos = None;
                        }

                        // ── 槽 1：点选工具（ADR-0057 补记 6，排在抓手之前）──
                        // 「点一格进选区看属性」，不落笔——独立 `PointSelectState`。
                        // 放在抓手前面：它是**导航级**的轻量动作（和抓手一样
                        // 不改数据），比后面的编辑工具更常用。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_MOUSE_POINTER,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsPointSelectHint)
                            } else {
                                locked_hint
                            },
                            point_select_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::PointSelect,
                        );

                        // ── 槽 2：抓手 ────────────────────────────────────
                        // 抓手是**导航**工具，不是写入口——Gate-1 不灰化它
                        // （播放中仍可拖动看图，属只读操作）。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_HAND,
                            t(loc, I18nKey::MapControlsPanHint),
                            pan_active,
                            true,
                            &mut params.palette,
                            MapToolId::Pan,
                        );

                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 3：选择组（分裂按钮） ──────────────────────
                        selection_split(
                            ui,
                            &shell,
                            metrics,
                            vertical,
                            loc,
                            selection.shape,
                            select_active,
                            &mut params.palette,
                        );

                        // ── 槽 4：地形橡皮刷 ─────────────────────────────
                        // Gate-1：播放中灰化（写入口）。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_ERASER,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsEraserHint)
                            } else {
                                locked_hint
                            },
                            eraser_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::BrushErase,
                        );
                        // EditCell 工具的 dock 瓷砖见下方「槽 12」
                        // （[ADR-0052] D7-1 已补齐入口）。

                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 6：测量组（分裂按钮） ──────────────────────
                        measure_split(
                            ui,
                            &shell,
                            metrics,
                            vertical,
                            loc,
                            measure.mode,
                            measure_active,
                            &mut params.palette,
                        );

                        // ── 槽 6.5：清除全部测量（2026-10-23 用户需求）────
                        // 与测量组相邻：紧贴「测量」右侧，一眼归到测量这一族。
                        // 语义 = 一键清空当前图的全部测量（可一次撤销）；删**一条**
                        // 仍走测量橡皮 / `Del`（渲染层输入，不经本工具条）。
                        // 编号取 6.5（不重排后续槽位）：沿用 ADR-0043 的 0–10 号
                        // 与 isolines「10.5」同款做法，历史注释不必整段改号。
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_TRASH,
                            false,
                            has_measurements,
                            t(loc, I18nKey::MapControlsClearMeasurements),
                        )
                        .clicked()
                        {
                            params.palette.pending = Some(CommandId::ClearMeasurements);
                        }

                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 8/9：重做 / 撤销 ───────────────────────────
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_REDO,
                            false,
                            can_redo,
                            "重做 (Ctrl+Shift+Z)",
                        )
                        .clicked()
                        {
                            params.palette.pending = Some(CommandId::Redo);
                        }
                        if icon_button(
                            ui,
                            &shell,
                            metrics,
                            ICON_UNDO,
                            false,
                            can_undo,
                            "撤销 (Ctrl+Z)",
                        )
                        .clicked()
                        {
                            params.palette.pending = Some(CommandId::Undo);
                        }

                        // ── 槽 10：网格 ──────────────────────────────────
                        if icon_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_GRID,
                            grid_active,
                            "显示参照网格（G）",
                        )
                        .clicked()
                        {
                            view.grid_lines = !view.grid_lines;
                        }

                        // ── 槽 10.5：等值线（2026-10-23 验收裁定）────────
                        // 会话开关（`view.isolines`，默认关）：等值线默认是
                        // 「海面黑线修饰」，要时再开。仅在 pass 有产出的两
                        // 模式显示（VectorFlat 分析参照 / Topographic 数据视图）。
                        if matches!(view.mode, ViewMode::VectorFlat | ViewMode::Topographic)
                            && icon_tile(
                                ui,
                                &shell,
                                metrics,
                                ICON_WAVES,
                                view.isolines,
                                "显示等值线",
                            )
                            .clicked()
                        {
                            view.isolines = !view.isolines;
                        }

                        // ── 槽 10.75：海拔分层设色强度（ADR-0055）────────────
                        // 仅 CellMap：数据视图用颜色读海拔。0 = 纯群系色块，
                        // 1 = 8 阶海拔色带完全替代群系色。会话态、不落盘。
                        // 槽位尺寸见 [`DockMetrics::tint_slider_slot`]：竖排就地
                        // 转竖（1 列 × 2 槽），否则横条的 3 槽宽会把纵向工具条
                        // 整条撑宽（2026-10-24 用户反馈）。
                        if matches!(view.mode, ViewMode::Topographic) {
                            let slot = metrics.tint_slider_slot(vertical);
                            // Slider 的长度取 `spacing().slider_width`（默认
                            // 100），与 `add_sized` 的 max_size 不同源；竖排不
                            // 同步改它，滑条会按 100px 长把框再撑高。
                            if vertical {
                                ui.spacing_mut().slider_width = slot.y;
                            }
                            let slider = egui::Slider::new(&mut view.hypsometric_tint, 0.0..=1.0)
                                .show_value(false);
                            let slider = if vertical { slider.vertical() } else { slider };
                            ui.add_sized(slot, slider)
                                .on_hover_text("海拔分层设色强度（0 = 只看群系色，1 = 只看海拔）");
                        }

                        // ── 槽 11：自然区（2026-10-22 内容矩阵 §2.6）──────
                        // CellMap 里显示 Nature 类区域 + 未命名候选高亮；
                        // 仅 CellMap（Topographic 工作区）显示本按钮——
                        // 其他模式区域层不受开关控制（用户裁定：只在 cell map）。
                        if matches!(view.mode, ViewMode::Topographic)
                            && icon_tile(
                                ui,
                                &shell,
                                metrics,
                                ICON_TREES,
                                view.nature_overlay,
                                "显示自然区与候选（CellMap）",
                            )
                            .clicked()
                        {
                            view.nature_overlay = !view.nature_overlay;
                        }

                        // ── 分隔条：通用地图工具 ↔ CellMap 编辑工具 ──
                        // [ADR-0052] §4.3（UX 审视）：槽 0–11 是「通用地图工具」
                        // （导航 / 选择 / 测量 / 视图 / 历史），槽 12+ 是
                        // 「CellMap 专属编辑工具」——语义层级不同，必须视觉隔断。
                        separator(ui, &shell, metrics, vertical);

                        // ── 槽 12：EditCell（单格编辑）────────────────
                        // [ADR-0052] D7-1。此前 EditCell 只有后端 plumbing
                        //（命令 + 状态 + 点选），**工具条没有可见入口**——
                        // 用户无法从 UI 打开单格编辑工作台。
                        // 二次点击 = 关闭（互斥由 `apply_pending_commands` 统一做）。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_SQUARE_MOUSE_POINTER,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsEditCellHint)
                            } else {
                                locked_hint
                            },
                            edit_cell_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::EditCell,
                        );

                        // ── 槽 13：FloodSelect（泛洪选择）────────────
                        // [ADR-0052] D6：点一下 = 按判据自动圈出同值连通区。
                        // 与槽 12 同属「CellMap 编辑」组，不另加分隔条。
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_PAINT_BUCKET,
                            if cell_tools_enabled {
                                t(loc, I18nKey::MapControlsFloodSelectHint)
                            } else {
                                locked_hint
                            },
                            flood_select_active,
                            cell_tools_enabled,
                            &mut params.palette,
                            MapToolId::FloodSelect,
                        );
                    };

                    // **不用**交叉轴 `Align::Center`（2026-10-20 教训）：大框的
                    // 内容 Ui 铺满整屏，Center 会把小钮摆到屏幕中央，而
                    // `Frame` 按 `min_rect` 收缩——min_rect 从锚点一路扩张到
                    // 屏幕中央，整条工具条鼓成一块盖住半个屏幕的面板。
                    // 对齐改由**槽位统一**解决：小钮 / 分隔条与瓷砖占同样的
                    // `metrics.tile` 槽位，视觉内容在槽内居中（见 `icon_button`
                    // / `grip_handle` / `separator`），帧自然贴合。
                    ui.spacing_mut().item_spacing = egui::vec2(metrics.gap, metrics.gap);
                    if vertical {
                        ui.vertical(|ui| slots(ui));
                    } else {
                        ui.horizontal(|ui| slots(ui));
                    }
                });
        });

    // ── 缩放控件条（独立 Area，grip 可拖） ──────────────────────────
    //
    // 与 dock 是兄弟关系：dock 是「工具集合」，zoom_strip 是「相机操作」。
    // 二者共享 [`MapDockState`]（同一份持久化），但位置偏移独立记
    // `zoom_strip_pos` —— dock 横向 / 纵向切换时，strip 默认贴到对侧。
    //
    // Figma `3:21882` 的 strip 只能横排（设计上 32 高一行），dock 任何朝向
    // 下 strip 都是横排。默认位置：纵向 dock → 贴左缘**底**（让开顶栏
    // 顶区域，避开左下方的笔刷/小地图面板）；横向 dock → 贴顶栏下沿
    // **右**（让开顶栏的"主世界/日夜"标题，让开 dock 占的底部位置）。
    let (zoom_anchor, zoom_default) = if vertical {
        (
            egui::Align2::LEFT_BOTTOM,
            egui::vec2(EDGE_OFFSET, -EDGE_OFFSET),
        )
    } else {
        // 贴顶栏**下沿**（DOCK_TOP_CLEARANCE = 顶栏 38 + 8）；只偏移 18 会
        // 落在 38px 顶栏内部，压住 Rebuild / settings 按钮（2026-10-19 实测）。
        (
            egui::Align2::RIGHT_TOP,
            egui::vec2(-EDGE_OFFSET, DOCK_TOP_CLEARANCE),
        )
    };
    let zoom_offset = params
        .dock
        .zoom_strip_pos
        .map(|p| egui::vec2(p.x, p.y))
        .unwrap_or(zoom_default);
    let mut zoom_state = crate::ui::widgets::ZoomControlsState {
        pos: params.dock.zoom_strip_pos.map(|p| [p.x, p.y]),
        show_scale_text: true,
    };
    let can_zoom_in = !zoom.at_inner_limit(view.scale);
    let can_zoom_out = !zoom.at_outer_limit(view.scale);
    // 三个回调都需要 `&mut view` + `&mut cameras`；闭包同时持三份会撞借用。
    // 折中：闭包内只产生**意图**（写三个互斥 bool），闭包外统一执行。
    // 同时存目标的 `target_scale`，让 `set_map_scale` 在外边单写。
    let mut zoom_in_clicked = false;
    let mut zoom_out_clicked = false;
    let mut zoom_fit_clicked = false;
    egui::Area::new(egui::Id::new("wv_zoom_controls_v01"))
        .anchor(zoom_anchor, zoom_offset)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let current_scale = view.scale;
            crate::ui::widgets::zoom_controls(
                ui,
                &shell,
                &mut zoom_state,
                current_scale,
                can_zoom_in,
                can_zoom_out,
                || zoom_in_clicked = true,
                || zoom_out_clicked = true,
                || zoom_fit_clicked = true,
            );
        });
    if zoom_in_clicked {
        let target = view.scale / 1.6;
        set_map_scale(&mut view, &mut cameras, target, zoom);
    } else if zoom_out_clicked {
        let target = view.scale * 1.6;
        set_map_scale(&mut view, &mut cameras, target, zoom);
    } else if zoom_fit_clicked {
        let frame_origin = drill
            .current()
            .and_then(|id| wv_core::resolve_frame_in(&projection.world.maps, id))
            .map(|f| Vec2::new(f.world_origin.x, f.world_origin.y))
            .unwrap_or(grid.map_origin);
        view.camera_center = frame_origin + level_map_size * 0.5;
        set_map_scale(&mut view, &mut cameras, zoom.fit, zoom);
    }
    // strip 的拖动位置回灌到 dock（落盘由 caller 负责）。
    params.dock.zoom_strip_pos = zoom_state.pos.map(|[x, y]| MapDockPos { x, y });
}
