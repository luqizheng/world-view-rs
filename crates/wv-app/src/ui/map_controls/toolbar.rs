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
    projection: Res<WorldProjection>,
    screen: Res<crate::app_screen::AppScreen>,
    mut params: DockParams,
    edit: Res<EditHistory>,
    panels: Res<PanelState>,
    // CellMap 工具活动态（ADR-0057 补记 6：EditCell 第 12 槽 / PointSelect
    // 槽 1 / FloodSelect 第 13 槽）——瓷砖按各自 Resource 的 `tool_active`
    // 画选中态。时间轴播放态（§9.4 Gate-1）：播放中 CellMap 编辑类瓷砖灰化。
    // 线段 POI 会话态（ADR-0070 D4，槽 14）同折进来。
    //
    // **折成一层元组**：Bevy 单系统参数上限 16，本函数原有 15 个，五个
    // `Res` 分开写会到 20（编译报「not a system set」）。元组算 1 个参数。
    (edit_cell, point_select, flood_select, playback, line_poi): ToolSessionStates,
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

    // 工具活动态（全部从资源读；工具切换本身走命令分发）。
    let select_active = selection.enabled;
    let pan_active = view.pan_tool;
    let eraser_active = brush.enabled && brush.tool == BrushTool::Eraser;
    // 涂抹笔刷活动态：橡皮以外都算「涂」（含意图工具——分派闸 `already_on`
    // 同口径，dispatch.rs `MapToolId::Brush`）。
    let brush_active = brush.enabled && brush.tool != BrushTool::Eraser;
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
    // **工作区白名单**（2026-10-28 裁定⑤）：编辑类工具只在所属工作区可用，
    // 声明表 = `MapToolId::available_in`（快捷键/命令面板/分发器同源）。
    // 灰化 + 悬停说明而非隐藏——作者看得到工具在哪，切档即亮。
    // （`view` 后文还有可变用法，这里按值快照模式，避免借用冲突。）
    let mode_now = view.mode;
    let ws = move |tool: MapToolId| tool.available_in(mode_now);
    let ws_hint = t(loc, I18nKey::MapControlsWorkspaceLocked);
    // UX-029①（round-2 审计）：地形笔刷只落**自有**地形栅格——当前图没有
    // `terrain_of` 时（子图继承父图底图）入口即灰。此前这个前置条件只在
    // 松手提交时报错（「笔刷未生效：地图无地形网格」），作者得先画一笔
    // 才知道画不了。
    let brush_writable = drill
        .current()
        .is_some_and(|id| projection.world.terrain_of(id).is_some());
    let no_terrain_hint = t(loc, I18nKey::MapControlsNoTerrain);

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
        // 让位判据与 `ViewMode::timeline_shows` 同源（2026-10-27 裁定：
        // 播放器只在 Browse）——Map/CellMap 里面板 visible 位即使为 true，
        // 浮窗也不在场，工具条不为不存在的窗抬升出一段空白。
        let bottom_lift = if panels.is_visible(Panel::Timeline) && view.mode.timeline_shows() {
            ctx.memory(|m| m.area_rect(egui::Id::new(("wv_panel", Panel::Timeline))))
                .filter(|r| r.top() > content.center().y)
                .map(|r| (content.bottom() - r.top()).max(0.0) + EDGE_OFFSET)
                .unwrap_or(EDGE_OFFSET)
        } else {
            EDGE_OFFSET
        };
        // 状态栏常驻（2026-10-25）：横向贴底的基础抬升让出底部 28px chrome，
        // 时间轴在场时其抬升已含状态栏高度，`max` 只兜「时间轴不在场」分支。
        let bottom_lift = bottom_lift.max(EDGE_OFFSET + crate::ui::top_bar::STATUS_BAR_H);
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

                        // ── 槽 3.5：地形笔刷（2026-10-27 补）─────────────
                        // 「涂」与「擦」同族贴放。历史上**涂**一直没有 dock 入口
                        // （原笔刷浮窗承载，浮窗裁撤后只剩 B / 命令面板）——多选
                        // 等互斥工具切走笔刷后，界面上没有任何鼠标入口能回去。
                        // 本瓷砖即回程入口，点它 = 按 B（同一 `MapToolId::Brush`）。
                        // Gate-1：播放中灰化（写入口）；CellMap 工作区外灰化（白名单）。
                        // 编号取 3.5 不重排后续槽位（ADR-0043 槽位号纪律）。
                        let brush_ok = cell_tools_enabled && ws(MapToolId::Brush) && brush_writable;
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_BRUSH,
                            if !cell_tools_enabled {
                                locked_hint
                            } else if !ws(MapToolId::Brush) {
                                ws_hint
                            } else if !brush_writable {
                                no_terrain_hint
                            } else {
                                t(loc, I18nKey::MapControlsBrushHint)
                            },
                            brush_active,
                            brush_ok,
                            &mut params.palette,
                            MapToolId::Brush,
                        );

                        // ── 槽 4：地形橡皮刷 ─────────────────────────────
                        // Gate-1：播放中灰化（写入口）；CellMap 工作区外灰化（白名单）。
                        let eraser_ok =
                            cell_tools_enabled && ws(MapToolId::BrushErase) && brush_writable;
                        tool_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_ERASER,
                            if !cell_tools_enabled {
                                locked_hint
                            } else if !ws(MapToolId::BrushErase) {
                                ws_hint
                            } else if !brush_writable {
                                no_terrain_hint
                            } else {
                                t(loc, I18nKey::MapControlsEraserHint)
                            },
                            eraser_active,
                            eraser_ok,
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
                            true,
                            "显示参照网格（G）",
                        )
                        .clicked()
                        {
                            view.grid_lines = !view.grid_lines;
                        }

                        // ── 槽 10.5：等值线（2026-10-23 验收裁定）────────
                        // 会话开关（`view.isolines`，默认关）：等值线默认是
                        // 「海面黑线修饰」，要时再开。仅在 pass 有产出的两
                        // 模式可用（VectorFlat 分析参照 / Topographic 数据视
                        // 图）——其他模式**恒渲染灰化**而非隐藏（UX-030 /
                        // round-2 审计：入口消失会让作者以为功能丢了）。
                        let isolines_ok =
                            matches!(view.mode, ViewMode::VectorFlat | ViewMode::Topographic);
                        let isolines_hint = if isolines_ok {
                            "显示等值线".to_owned()
                        } else {
                            format!(
                                "显示等值线（{}）",
                                t(loc, I18nKey::MapControlsViewModeLocked)
                            )
                        };
                        if icon_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_WAVES,
                            view.isolines,
                            isolines_ok,
                            &isolines_hint,
                        )
                        .clicked()
                        {
                            view.isolines = !view.isolines;
                        }

                        // ── 槽 10.75：海拔分层设色强度（ADR-0055）────────────
                        // 仅 CellMap 可用：数据视图用颜色读海拔。0 = 纯群系色块，
                        // 1 = 8 阶海拔色带完全替代群系色。会话态、不落盘。
                        // 槽位尺寸见 [`DockMetrics::tint_slider_slot`]：竖排就地
                        // 转竖（1 列 × 2 槽），否则横条的 3 槽宽会把纵向工具条
                        // 整条撑宽（2026-10-24 用户反馈）。
                        // UX-030：其他模式恒渲染、`set_enabled` 灰化而非消失。
                        let tint_ok = matches!(view.mode, ViewMode::Topographic);
                        let slot = metrics.tint_slider_slot(vertical);
                        // Slider 的长度取 `spacing().slider_width`（默认
                        // 100），与 `add_sized` 的 max_size 不同源；竖排不
                        // 同步改它，滑条会按 100px 长把框再撑高。
                        if vertical {
                            ui.spacing_mut().slider_width = slot.y;
                        }
                        let tint_hint = if tint_ok {
                            "海拔分层设色强度（0 = 只看群系色，1 = 只看海拔）".to_owned()
                        } else {
                            format!(
                                "海拔分层设色强度（{}）",
                                t(loc, I18nKey::MapControlsViewModeLocked)
                            )
                        };
                        // `add_enabled_ui`（egui 0.36 无 `set_enabled`）：禁用态
                        // 滑条灰化、不可拖。
                        ui.add_enabled_ui(tint_ok, |ui| {
                            let slider = egui::Slider::new(&mut view.hypsometric_tint, 0.0..=1.0)
                                .show_value(false);
                            let slider = if vertical { slider.vertical() } else { slider };
                            ui.add_sized(slot, slider).on_hover_text(tint_hint);
                        });

                        // ── 槽 11：自然区（2026-10-22 内容矩阵 §2.6）──────
                        // CellMap 里显示 Nature 类区域 + 未命名候选高亮。
                        // 仅 CellMap（Topographic 工作区）可用——其他模式区域
                        // 层不受开关控制（用户裁定：只在 cell map）；UX-030
                        // 后恒渲染灰化而非整枚消失。
                        let nature_ok = matches!(view.mode, ViewMode::Topographic);
                        let nature_hint = if nature_ok {
                            "显示自然区与候选（CellMap）".to_owned()
                        } else {
                            format!(
                                "显示自然区与候选（{}）",
                                t(loc, I18nKey::MapControlsViewModeLocked)
                            )
                        };
                        if icon_tile(
                            ui,
                            &shell,
                            metrics,
                            ICON_TREES,
                            view.nature_overlay,
                            nature_ok,
                            &nature_hint,
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
                            if !cell_tools_enabled {
                                locked_hint
                            } else if !ws(MapToolId::EditCell) {
                                ws_hint
                            } else {
                                t(loc, I18nKey::MapControlsEditCellHint)
                            },
                            edit_cell_active,
                            cell_tools_enabled && ws(MapToolId::EditCell),
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
                            if !cell_tools_enabled {
                                locked_hint
                            } else if !ws(MapToolId::FloodSelect) {
                                ws_hint
                            } else {
                                t(loc, I18nKey::MapControlsFloodSelectHint)
                            },
                            flood_select_active,
                            cell_tools_enabled && ws(MapToolId::FloodSelect),
                            &mut params.palette,
                            MapToolId::FloodSelect,
                        );

                        // ── 槽 14：线段 POI（河流 / 道路，ADR-0070 D4）──
                        // 作者工具只在放大到可编辑级别（Edit 档）才有意义——
                        // 旧实现整枚瓷砖不渲染（而非灰化），作者拉远半档工具
                        // 就「消失」，以为功能丢了（UX-030 / round-2 审计）。
                        // 现恒渲染：Edit 档亮，其余灰化 + 悬停说明。
                        // 分裂按钮：主键 = 河，弹层换类；命令排队后由分发器
                        // 统一互斥。播放锁对齐 CellMap 工具组（T10 评审
                        // minor ①）：播放中灰化不可点，与写入口工具同一
                        // Gate-1 口径。
                        let lp = MapToolId::LinePoi(line_poi.kind);
                        let lp_lod_ok =
                            line_poi_tile_visible(wv_render::state::lod_tier(view.scale));
                        let lp_hint = if !cell_tools_enabled {
                            locked_hint
                        } else if !ws(lp) {
                            ws_hint
                        } else if !lp_lod_ok {
                            t(loc, I18nKey::MapControlsLodLocked)
                        } else {
                            t(loc, I18nKey::LinePoiHint)
                        };
                        line_poi_split(
                            ui,
                            &shell,
                            metrics,
                            vertical,
                            loc,
                            line_poi.kind,
                            line_poi.tool_active,
                            lp_hint,
                            cell_tools_enabled && ws(lp) && lp_lod_ok,
                            &mut params.palette,
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
}
