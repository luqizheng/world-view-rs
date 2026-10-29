//! 工具条自绘控件：分裂按钮、工具瓷砖、图标钮、分隔条、抓手。

use super::*;

/// 线段 POI 瓷砖的档位门控（ADR-0070 D4）：**仅 Edit 档渲染**。
///
/// 纯函数——「该档显不显」的口径收在这里，工具条渲染与单测共用一条，
/// 避免门控逻辑散落在 UI 闭包里没法断言（ADR-0043 弹层测试的教训）。
pub(super) fn line_poi_tile_visible(tier: wv_render::state::LodTier) -> bool {
    matches!(tier, wv_render::state::LodTier::Edit)
}

/// 槽 14 —— 线段 POI 分裂按钮（ADR-0070 D4）。
///
/// 主键 = 河（Lucide `river`），菜单两项 = 河 / 路（Lucide `route`）。
/// 主区图标**跟着当前类走**：河显 `river`、路显 `route`。点主区 = 用当前
/// 类开关线段 POI 工具；点小三角 = 弹层里换类（换类 = 换参数不关工具——
/// 分发器口径：同类开着 = 关，异类开着 = 换，与 `Select(shape)` 同款）。
///
/// 调用方负责档位门控（[`line_poi_tile_visible`]，仅 Edit 档调用本函数）
/// 与播放锁（`enabled`，Gate-1 同款：灰化 + 不成命中区，见 `tool_tile`）。
#[allow(clippy::too_many_arguments)]
pub(super) fn line_poi_split(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    vertical: bool,
    loc: Locale,
    kind: wv_core::distance::LineKind,
    active: bool,
    hint: &str,
    enabled: bool,
    palette: &mut CommandPalette,
) {
    use wv_core::distance::LineKind;

    // 播放锁（T10 评审 minor ①）：与 `tool_tile` 同口径——灰化占位、
    // `Sense::hover` 不成命中区（不是「点了不响应」，是连误触的机会都不给）。
    // 底色透明：大框（`float_fill`）已提供底，瓷砖只在 hover / 活动时上色。
    if !enabled {
        let (rect, _resp) = ui.allocate_exact_size(metrics.tile, egui::Sense::hover());
        let icon = match kind {
            LineKind::River => ICON_RIVER,
            LineKind::Road => ICON_ROUTE,
        };
        icon_image(icon, shell.text_disabled(), metrics.tile_icon).paint_at(
            ui,
            egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.tile_icon)),
        );
        return;
    }

    let items = [
        SplitMenuItem::new(ICON_RIVER, t(loc, I18nKey::LinePoiKindRiver)),
        SplitMenuItem::new(ICON_ROUTE, t(loc, I18nKey::LinePoiKindRoad)),
    ];
    let icon = match kind {
        LineKind::River => ICON_RIVER,
        LineKind::Road => ICON_ROUTE,
    };
    let mut main_cmd: Option<CommandId> = None;
    let mut pick_cmd: Option<CommandId> = None;
    split_button(
        ui,
        shell,
        "wv_split_line_poi",
        metrics.split_metrics(vertical),
        icon,
        active,
        hint,
        &items,
        usize::from(kind == LineKind::Road),
        || main_cmd = Some(CommandId::SelectMapTool(MapToolId::LinePoi(kind))),
        |i| {
            let picked = if i == 1 {
                LineKind::Road
            } else {
                LineKind::River
            };
            pick_cmd = Some(CommandId::SelectMapTool(MapToolId::LinePoi(picked)));
        },
    );
    if let Some(c) = main_cmd.or(pick_cmd) {
        palette.pending = Some(c);
    }
}

/// 槽 3 —— 选择组的分裂按钮（ADR-0043）。
///
/// 主区图标**跟着当前形状走**：多边形显示 `vector-polygon`、矩形显示
/// `vector-square`、椭圆显示 `ellipse`。点主区 = 用当前形状开关选择工具；
/// 点小三角 = 弹层里换形状（换形状即重开选择工具——分发器语义：
/// 不同形状开着 = 换，不关）。
///
/// 两个待发命令用两个局部变量接（主区回调是 `FnOnce`、选中回调是 `FnMut`，
/// 两者不能同时可变借用同一份 `palette`），闭包外统一入队——与缩放条
/// 三 bool 模式同款。
#[allow(clippy::too_many_arguments)]
pub(super) fn selection_split(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    vertical: bool,
    loc: Locale,
    shape: SelectionShape,
    active: bool,
    palette: &mut CommandPalette,
) {
    let items = [
        SplitMenuItem::new(
            ICON_VECTOR_POLYGON,
            t(loc, I18nKey::MapControlsSelectPolygon),
        )
        .with_hint("V"),
        SplitMenuItem::new(ICON_VECTOR_SQUARE, t(loc, I18nKey::MapControlsSelectRect)),
        SplitMenuItem::new(ICON_ELLIPSE, t(loc, I18nKey::MapControlsSelectEllipse)),
    ];
    let selected = match shape {
        SelectionShape::Polygon => 0,
        SelectionShape::Rect => 1,
        SelectionShape::Ellipse => 2,
    };
    let icon = match shape {
        SelectionShape::Polygon => ICON_VECTOR_POLYGON,
        SelectionShape::Rect => ICON_VECTOR_SQUARE,
        SelectionShape::Ellipse => ICON_ELLIPSE,
    };
    let hint = match shape {
        SelectionShape::Polygon => t(loc, I18nKey::MapControlsSelectGroupHint),
        SelectionShape::Rect => t(loc, I18nKey::MapControlsSelectRectHint),
        SelectionShape::Ellipse => t(loc, I18nKey::MapControlsSelectEllipseHint),
    };
    let mut main_cmd: Option<CommandId> = None;
    let mut pick_cmd: Option<CommandId> = None;
    split_button(
        ui,
        shell,
        "wv_split_select",
        metrics.split_metrics(vertical),
        icon,
        active,
        hint,
        &items,
        selected,
        || main_cmd = Some(CommandId::SelectMapTool(MapToolId::Select(shape))),
        |i| {
            let picked = [
                SelectionShape::Polygon,
                SelectionShape::Rect,
                SelectionShape::Ellipse,
            ][i];
            pick_cmd = Some(CommandId::SelectMapTool(MapToolId::Select(picked)));
        },
    );
    if let Some(c) = main_cmd.or(pick_cmd) {
        palette.pending = Some(c);
    }
}

/// 槽 6 —— 测量组的分裂按钮（ADR-0043）。
///
/// 两项语义（用户裁定：功能不变，只换图标）：
/// - **线段距离**（`ruler-dimension-line`，默认）= 现有折线测量
///   （多点累积，每段读数 + Σ 总长）；
/// - **面积**（`squares-unite`）= 现有矩形测量（两点定对角，长 × 宽）。
#[allow(clippy::too_many_arguments)]
pub(super) fn measure_split(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    vertical: bool,
    loc: Locale,
    mode: MeasureMode,
    active: bool,
    palette: &mut CommandPalette,
) {
    let items = [
        SplitMenuItem::new(
            ICON_RULER_DIMENSION_LINE,
            t(loc, I18nKey::MapControlsMeasureDistance),
        )
        .with_hint("L"),
        SplitMenuItem::new(ICON_SQUARES_UNITE, t(loc, I18nKey::MapControlsMeasureArea))
            .with_hint("M"),
    ];
    // Off 时主区显示默认项（线段距离）——「默认工具」的含义就是没开过也长这样。
    let rect_mode = mode == MeasureMode::Rect;
    let icon = if rect_mode {
        ICON_SQUARES_UNITE
    } else {
        ICON_RULER_DIMENSION_LINE
    };
    let hint = if rect_mode {
        t(loc, I18nKey::MapControlsMeasureRectHint)
    } else {
        t(loc, I18nKey::MapControlsMeasurePolylineHint)
    };
    let mut main_cmd: Option<CommandId> = None;
    let mut pick_cmd: Option<CommandId> = None;
    split_button(
        ui,
        shell,
        "wv_split_measure",
        metrics.split_metrics(vertical),
        icon,
        active,
        hint,
        &items,
        usize::from(rect_mode),
        || {
            main_cmd = Some(CommandId::SelectMapTool(if rect_mode {
                MapToolId::MeasureRect
            } else {
                MapToolId::MeasureLine
            }))
        },
        |i| {
            pick_cmd = Some(CommandId::SelectMapTool(if i == 1 {
                MapToolId::MeasureRect
            } else {
                MapToolId::MeasureLine
            }));
        },
    );
    if let Some(c) = main_cmd.or(pick_cmd) {
        palette.pending = Some(c);
    }
}

/// 工具瓷砖：纯图标居中（2026-10-20 用户裁定：不画快捷键字母，字母退到
/// tooltip 与 i18n 提示里）。
///
/// 点击只排队 [`CommandId::SelectMapTool`]——互斥与状态写入统一在分发器，
/// 本函数不碰任何工具资源。
#[allow(clippy::too_many_arguments)]
pub(super) fn tool_tile(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    hint: &str,
    active: bool,
    enabled: bool,
    palette: &mut CommandPalette,
    tool: MapToolId,
) {
    // `enabled = false`（[ADR-0052] §9.4 Gate-1/Gate-2）：灰化不可点。
    // `Sense::hover` 而非 `click`——不是「点了不响应」，而是**根本不成命中区**，
    // 连误触的机会都不给。
    let (rect, resp) = ui.allocate_exact_size(
        metrics.tile,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hovered = enabled && resp.hovered();
    let fill = if active {
        shell.gold_soft()
    } else if hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if active {
        egui::Stroke::new(1.0, shell.gold())
    } else {
        egui::Stroke::NONE
    };
    ui.painter()
        .rect(rect, TILE_RADIUS, fill, stroke, egui::StrokeKind::Inside);

    let tint = if !enabled {
        shell.text_disabled()
    } else if active {
        shell.gold()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.tile_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.tile_icon)),
    );

    if enabled && resp.clicked() {
        palette.pending = Some(CommandId::SelectMapTool(tool));
    }
    if !enabled {
        // 禁用态不给手型光标——避免「看起来能点」的误判（与 `icon_button` 同口径）。
        resp.on_hover_text(hint);
        return;
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint);
}

/// 图标瓷砖：仅图标、无字母（视图 / 导航组）。
///
/// `enabled=false` 时图标压暗、不可点（UX-030 / round-2 审计：工具入口
/// **恒渲染、灰化而非隐藏**——作者随时知道工具在哪，条件满足即亮）。
pub(super) fn icon_tile(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    active: bool,
    enabled: bool,
    hint: &str,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(metrics.tile, sense);
    let hovered = enabled && resp.hovered();
    let fill = if active {
        shell.gold_soft()
    } else if hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if active {
        egui::Stroke::new(1.0, shell.gold())
    } else {
        egui::Stroke::NONE
    };
    ui.painter()
        .rect(rect, TILE_RADIUS, fill, stroke, egui::StrokeKind::Inside);
    let tint = if !enabled {
        // 禁用态：压暗到与背景可分但不成主导（与 icon_button 禁用色同思路）。
        shell.t3().gamma_multiply(0.45)
    } else if active {
        shell.gold()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.tile_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.tile_icon)),
    );

    if !enabled {
        // 禁用态不给手型光标——避免「看起来能点」的误判（与 `icon_button` 同口径）。
        return resp.on_hover_text(hint);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint)
}

/// 图标钮（Figma `Undo` / `Redo` / `Snapping` 32×32）。
///
/// `active` 走 `accent_soft` 底 + accent 图标；
/// `enabled=false` 用禁用色且不可点（撤销栈空 / 重做栈空）。
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    active: bool,
    enabled: bool,
    hint: &str,
) -> egui::Response {
    // 槽位对齐（2026-10-20）：小钮与瓷砖占**同样大小**的槽（`metrics.tile`），
    // 视觉钮（28/32px）画在槽正中。这样纵向排里小钮不会贴左、横向排里不会
    // 贴顶，整条天然成列；命中区也比视觉钮大一圈，更好点。
    let (slot, resp) = ui.allocate_exact_size(
        metrics.tile,
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let rect = egui::Rect::from_center_size(slot.center(), egui::Vec2::splat(metrics.icon_btn));
    let hovered = enabled && resp.hovered();
    let fill = if active {
        shell.accent_soft()
    } else if hovered {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, TILE_RADIUS, fill);

    let tint = if !enabled {
        shell.text_disabled()
    } else if active {
        shell.accent()
    } else if hovered {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.icon_btn_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.icon_btn_icon)),
    );

    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(hint)
    } else {
        resp
    }
}

/// 分隔条：横向布局画 1×24 竖线，纵向布局画 24×1 横线（Figma `Tool separator`）。
///
/// 占一个完整瓷砖槽（竖排 = tile 宽 ×1，横排 = 1×tile 高），线段在槽内居中
/// ——与 [`icon_button`] 的槽位对齐同一口径。
pub(super) fn separator(ui: &mut egui::Ui, shell: &UiShell, metrics: DockMetrics, vertical: bool) {
    let size = if vertical {
        egui::vec2(metrics.tile.x, 1.0)
    } else {
        egui::vec2(1.0, metrics.tile.y)
    };
    let (slot, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let stroke = egui::Stroke::new(1.0, shell.border_subtle());
    let [a, b] = if vertical {
        let y = slot.center().y;
        let half = metrics.sep_len * 0.5;
        [
            egui::pos2(slot.center().x - half, y),
            egui::pos2(slot.center().x + half, y),
        ]
    } else {
        let x = slot.center().x;
        let half = metrics.sep_len * 0.5;
        [
            egui::pos2(x, slot.center().y - half),
            egui::pos2(x, slot.center().y + half),
        ]
    };
    ui.painter().line_segment([a, b], stroke);
}

/// 拖动手柄 + 朝向切换按钮（grip-vertical）。
///
/// **双重语义**：
/// - 拖动（`Sense::drag`）→ 整条工具条跟随，调用方累计 `drag_delta` 到
///   `MapDockState::pos`。
/// - 单击（`Sense::click`）→ 切换朝向（与拖动不冲突：拖动距离足够大时
///   `clicked()` 不会触发——egui 0.36 内部阈值）。
///
/// 视觉上与 `icon_button` 风格一致：t1 文字色，hover 走 `hover_bg`，
/// 鼠标在把柄上时**显 `Grab` 光标**提示可拖。
pub(super) fn grip_handle(
    ui: &mut egui::Ui,
    shell: &UiShell,
    metrics: DockMetrics,
    icon: egui::ImageSource<'static>,
    hint: &str,
) -> egui::Response {
    // 槽位对齐同 `icon_button`：占满一个瓷砖槽，视觉钮画在槽正中。
    let (slot, resp) = ui.allocate_exact_size(metrics.tile, egui::Sense::click_and_drag());
    let rect = egui::Rect::from_center_size(slot.center(), egui::Vec2::splat(metrics.icon_btn));
    let hovered = resp.hovered();
    let fill = if hovered || resp.dragged() {
        shell.hover_bg()
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, TILE_RADIUS, fill);
    let tint = if hovered || resp.dragged() {
        shell.t1()
    } else {
        shell.t3()
    };
    icon_image(icon, tint, metrics.icon_btn_icon).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(metrics.icon_btn_icon)),
    );
    if resp.dragged() {
        resp.on_hover_cursor(egui::CursorIcon::Grabbing)
    } else {
        resp.on_hover_cursor(egui::CursorIcon::Grab)
    }
    .on_hover_text(hint)
}
