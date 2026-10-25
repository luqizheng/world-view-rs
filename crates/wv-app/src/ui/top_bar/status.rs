//! 状态栏与各读数段。

use super::prefs::count_entities;
use super::*;

/// 状态栏字号（Figma #3:21001 全部 10px/16px）。
///
/// 状态栏是「仪表读数」，比令牌阶梯 MICRO(11) 再小半档——顶栏主题分段
/// （`theme_segment`）已有 10px 直写先例。
pub(super) const STATUS_FONT: f32 = 10.0;
/// 中段（Pointer coordinates）组内间距（Figma gap 24px）。
pub(super) const STATUS_GAP_CENTER: f32 = 24.0;

/// 状态栏（2026-10-16 按 Figma `Untitled` #3:21001「Status bar」重做，28px）。
///
/// 三段式（设计稿 `space-between`）：
/// - 左「Project status」：`WORLD–VIEW` + 版本 + 分隔线 + 📁 工程目录名；
/// - 中「Pointer coordinates」：光标世界坐标 `X / Y`；当前图声明了
///   `geographic_source` 且光标落在图幅内时，追加经纬度 + `EPSG:4326`
///   （世界 → UV 走 [`wv_core::resolve_frame_in`] 单一实现点，
///   UV → 经纬度走 [`wv_core::LatLonBbox::uv_to_lat_lon`]，本文件零手搓换算）；
/// - 右「Scene statistics」：对象计数 + 金色 `N selected` + 「N warnings」
///   徽标（沿用旧版，>0 才出现）+ 分隔线 + 绿点 + FPS。
///
/// ## 常驻底部（2026-10-25 用户裁定，与顶栏同权）
///
/// 曾被时间轴等浮窗（拖到下方时）整条盖住——用户「要下拉才看得到」。
/// 收口手段是**几何钳制**而非抬层：所有浮窗统一
/// `constrain_to(central_area)`（视口挖去顶栏 38 / 状态栏 28），拖不进
/// 这 28px、也拖不出视口；按大窗口保存的 `ui-layout.yaml` 坐标在小窗口
/// 下被逐帧钳回。本栏仍画在 Background 层——曾试验把根 Ui 提到
/// Foreground，结果根 Ui 的全视口 widget 在 egui hit_test 层过滤里把
/// Background / Middle 层命中整体排除，全界面输入失灵（见
/// [`root_ui`] 文档），已回退。中段坐标为面板内绝对居中绘制
/// （`paint_pointer_coordinates`）。
///
/// ## 与设计稿的刻意偏差（令牌纪律，非漏做）
///
/// 色值一律走 [`crate::theme::ui_tokens::ShellPalette`]（AGENTS §8：UI 禁写裸
/// hex）：`#11171C@.96`→`bg_app@.96`、`#A3B1BA`→`text_2`、`#778994`→`text_3`、
/// `#D6B77B`→`gold`、`#8FB397`→`badge_ok`、`#303B43`→`border_subtle`——
/// 亮色主题因此自动获得同语义配色，而非设计稿的暗色单套。
///
/// 参数 9 个（含 [ADR-0052] D8-B 的笔触进度态）：Bevy 系统的宽签名是框架
/// 惯例，clippy 的 7 参数默认阈值不适用于 `Res`/`ResMut` 声明式依赖。
#[allow(clippy::too_many_arguments)]
pub fn status_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    validation_count: Res<ValidationCount>,
    drill: Res<DrillState>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    paint: Res<wv_world_cell_map::cell_paint::CellPaintSession>,
    current_loc: Res<CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // Figma #3:21001 designedHeight 28px（旧版 22px）。
    let h = STATUS_BAR_H;
    let mut root = root_ui(ctx);
    egui::Panel::bottom("wv_status_bar")
        .exact_size(h)
        .frame(
            egui::Frame::NONE
                // 设计稿 rgba(17,23,28,.96)：走 bg_app @0.96 的 CSS rgba 语义
                // （`to_color32_alpha` 非预乘），不学顶栏的 gamma_multiply 近似。
                .fill(color32_alpha(shell.palette.bg_app, 0.96))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .inner_margin(crate::theme::ui_theme::margin_xy(16.0, 0.0)),
        )
        .show(&mut root, |ui| {
            // Panel 默认纵向布局：三段式必须包在 horizontal 里（旧版同款）。
            ui.horizontal(|ui| {
                // ── 左：Project status（组内 gap 16）──
                project_status_segment(ui, &shell, &projection);
                // ── 中左：笔触进度瞬态槽（[ADR-0052] D8-B，仅 stroke 期间出现）──
                stroke_progress_segment(ui, &shell, &paint, current_loc.0);
                // ── 中：Pointer coordinates（面板内绝对居中，见 fn 文档）──
                paint_pointer_coordinates(ui, &shell, &view, &projection, &drill);
                // ── 右：Scene statistics（组内 gap 16，右对齐）──
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    scene_statistics_segment(
                        ui,
                        &shell,
                        &projection,
                        &validation_count,
                        &diagnostics,
                    );
                });
            });
        });
}

/// 中左段「笔触进度」瞬态槽（[ADR-0052] D8-B）。
///
/// **只在 stroke 期间出现**，松手 / Esc 后自动消失。内容：
/// `STROKE_PENDING` 色点 + 「笔触进行中 · 已落 N 格」。
///
/// ## 与 D8 读数条为何分开
///
/// D8 读数条（右侧检查器）是**持久信息**——hover / selected 格的读数，
/// 不随笔触起落而变。笔触格数是**瞬态操作反馈**——手指按住才有的计数，
/// 松手即无。两者混在一处会让读数条在拖绘时闪烁，故 D8-B 独立占底栏槽位
/// （ADR-0052 §9.3 D8-B 原文：「底栏是瞬态操作反馈，不与 D8 固定双段冲突」）。
///
/// ## 为何用金色点而非 `STROKE_PENDING` 红橙
///
/// 底栏是 chrome（跟随主题明暗），`STROKE_PENDING` 是**世界层**语义色
/// （画布上的待落色块），固定不随主题变。把世界色搬到底栏会在亮色主题下
/// 失去对比度校准。故此处用 shell 金色（`ui.gold` = 「活动中的工具」语义，
/// 与顶栏 ACTIVE 徽标同源）表示「进行中」，红橙留在画布上表示「这些格待落」。
pub(super) fn stroke_progress_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    paint: &wv_world_cell_map::cell_paint::CellPaintSession,
    locale: crate::i18n::Locale,
) {
    if !paint.is_stroking() {
        return;
    }
    let n = paint.stroke_len();
    ui.add_space(space::LG);
    vsep(ui, color32(shell.palette.border_subtle));
    ui.add_space(space::LG);
    // 金点 = 「进行中」（同 `ui.gold` 的 ACTIVE 语义）。
    status_dot(ui, shell.gold());
    ui.add_space(space::SM);
    ui.label(
        egui::RichText::new(format!(
            "{} {}{}",
            crate::i18n::t(locale, I18nKey::StatusBarStrokeProgress),
            n,
            crate::i18n::t(locale, I18nKey::StatusBarStrokeCells)
        ))
        .size(STATUS_FONT)
        .color(shell.gold()),
    );
}

/// 左段「Project status」：产品名 / 版本 / 分隔线 / 工程目录名（gap 16 均匀）。
pub(super) fn project_status_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
) {
    // WORLD–VIEW（设计稿 Inter SemiBold——打包仅 Regular、egui 无合成粗体，
    // `.strong()` 退化为强调色，字体加粗待 ADR-0019 后续字重接线）。
    ui.label(
        egui::RichText::new("WORLD–VIEW")
            .size(STATUS_FONT)
            .strong()
            .color(shell.t2()),
    );
    ui.add_space(space::LG);
    // 版本号（真实 Cargo 版本，非设计稿示例 0.8.4）。
    ui.label(
        egui::RichText::new(env!("CARGO_PKG_VERSION"))
            .size(STATUS_FONT)
            .color(shell.t3())
            .font(mono(STATUS_FONT)),
    );
    ui.add_space(space::LG);
    vsep(ui, color32(shell.palette.border_subtle));
    ui.add_space(space::LG);
    // 工程目录名（未加载工程时不画——空编辑器没有「当前文件」可报）。
    if let Some(dir) = projection.project_dir.as_deref() {
        ui.add(icon_image(ICON_FOLDER_OPEN, shell.t2(), 12.0));
        ui.add_space(space::LG);
        ui.label(
            egui::RichText::new(project_display_name(dir))
                .size(STATUS_FONT)
                .color(shell.t2())
                .font(mono(STATUS_FONT)),
        );
    }
}

/// 中段「Pointer coordinates」：X/Y 世界坐标 +（真实地理图）经纬度 + EPSG。
///
/// **面板内绝对居中**（2026-10-25）：按各读数 galley 总宽反推起点、
/// painter 直接绘制——顶栏读数段「绝对反推」的同款先例。原先用锚定
/// `CENTER_BOTTOM` 的独立 Area，但状态栏迁 Foreground 层后同层多 Area
/// 叠序不定，读数可能被本栏底板盖住。光标不在窗口内（`cursor_world =
/// None`）整段不画。
fn paint_pointer_coordinates(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &WorldProjection,
    drill: &DrillState,
) {
    let Some(cursor) = view.cursor_world else {
        return;
    };
    let t2 = color32(shell.palette.text_2);
    let t3 = color32(shell.palette.text_3);
    let items = vec![
        (format!("X  {}", format_fixed2(cursor.x as f64)), t2),
        (format!("Y  {}", format_fixed2(cursor.y as f64)), t2),
    ];
    // 经纬度两枚与 X/Y 同款组距 24 追加在尾部（原 Area 版布局不变）。
    let items = match cursor_lat_lon(cursor, projection, drill) {
        Some((lat, lon)) => {
            let mut v = items;
            v.push((format!("{}  {}", format_lat(lat), format_lon(lon)), t3));
            v.push(("EPSG:4326".to_owned(), t3));
            v
        }
        None => items,
    };
    let font = mono(STATUS_FONT);
    let galleys: Vec<_> = items
        .into_iter()
        .map(|(text, color)| {
            ui.painter()
                .layout(text, font.clone(), color, f32::INFINITY)
        })
        .collect();
    let total = galleys.iter().map(|g| g.size().x).sum::<f32>()
        + STATUS_GAP_CENTER * galleys.len().saturating_sub(1) as f32;
    let bounds = ui.max_rect();
    let mut x = bounds.center().x - total * 0.5;
    for galley in &galleys {
        let size = galley.size();
        // galley 自带 layout 时的颜色；fallback 色给中性值（不会用到）。
        ui.painter().galley(
            egui::pos2(x, bounds.center().y - size.y * 0.5),
            galley.clone(),
            t2,
        );
        x += size.x + STATUS_GAP_CENTER;
    }
}

/// 光标世界坐标 → (纬度, 经度)。
///
/// 世界 → UV 是坐标链（基线 §2.15 禁止手搓）：[`wv_core::resolve_frame_in`]
/// 还原本图世界帧 → [`wv_core::coord::MapFrame::world_to_uv`]；UV → 经纬度是
/// ADR-0007 的地理派生（[`wv_core::LatLonBbox::uv_to_lat_lon`]）。
/// 光标出图幅返回 `None`（可见性判断在这里，外推不夹取在 wv-core）。
pub(super) fn cursor_lat_lon(
    cursor: Vec2,
    projection: &WorldProjection,
    drill: &DrillState,
) -> Option<(f64, f64)> {
    let node = drill.current_node(&projection.world)?;
    let geo = node.geographic_source.as_ref()?;
    let frame = wv_core::resolve_frame_in(&projection.world.maps, &node.id)?;
    let uv = frame.world_to_uv(wv_core::Point::new(cursor.x, cursor.y))?;
    if !(0.0..=1.0).contains(&uv[0]) || !(0.0..=1.0).contains(&uv[1]) {
        return None;
    }
    Some(geo.bbox.uv_to_lat_lon([uv[0] as f64, uv[1] as f64]))
}

/// 右段「Scene statistics」：对象数 / 选中数 / 警告徽标 / 分隔线 / 绿点 / FPS。
///
/// **右至左添加**（父布局 `right_to_left`）：设计稿从左到右是
/// objects → selected → 分隔线 → 绿点 → FPS，故添加序相反。
pub(super) fn scene_statistics_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    validation_count: &ValidationCount,
    diagnostics: &bevy::diagnostic::DiagnosticsStore,
) {
    // FPS（bevy Diagnostics 平滑值；插件未跑出数据的首帧不画）。
    if let Some(fps) = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
    {
        ui.label(
            egui::RichText::new(format!("{fps:.0} FPS"))
                .size(STATUS_FONT)
                .color(shell.t2())
                .font(mono(STATUS_FONT)),
        );
        ui.add_space(space::LG);
    }
    status_dot(ui, color32(shell.palette.badge_ok));
    ui.add_space(space::LG);
    vsep(ui, color32(shell.palette.border_subtle));
    ui.add_space(space::LG);
    // 警告徽标（旧版功能保留：>0 才出现，默认视图与设计稿一致）。
    if validation_count.warnings > 0 {
        badge(
            ui,
            shell,
            BadgeKind::Warn,
            &format!(
                "{} warning{}",
                validation_count.warnings,
                if validation_count.warnings == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        );
        ui.add_space(space::LG);
    }
    // 选中数（金 = 世界「选中」语义，同 `ui.focus` 暗色复用的理由）。
    if projection.selection.is_some() {
        ui.label(
            egui::RichText::new("1 selected")
                .size(STATUS_FONT)
                .color(shell.gold()),
        );
        ui.add_space(space::LG);
    }
    // 对象计数（旧顶栏 count_entities 同口径：四类实体合计）。
    let objects = count_entities(projection);
    ui.label(
        egui::RichText::new(format!("{} objects", format_thousands(objects as u64)))
            .size(STATUS_FONT)
            .color(shell.t3()),
    );
}

/// 1×12px 竖分隔线（Figma 状态栏「Separator」#3:21005）。
pub(super) fn vsep(ui: &mut egui::Ui, color: egui::Color32) {
    vsep_h(ui, color, 12.0);
}

/// 任意高度的 1px 竖分隔线（顶栏 `3:20973` = 16px，状态栏 = 12px）。
pub(super) fn vsep_h(ui: &mut egui::Ui, color: egui::Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// 不占布局游标的竖分隔线：以 `center` 为中心画 1×`height`。
/// 顶栏右区按绝对矩形排版，用这个而不是 [`vsep_h`]。
pub(super) fn vsep_h_at(ui: &egui::Ui, color: egui::Color32, height: f32, center: egui::Pos2) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(1.0, height));
    ui.painter().rect_filled(rect, 0.0, color);
}

/// 4×4px 圆点（Figma「Render status」#3:21017）。
pub(super) fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(4.0, 4.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 2.0, color);
}

/// 工程目录的展示名：取路径末段（`…/guizhou-atlas.world` → `guizhou-atlas.world`）。
///
/// `file_name` 失败（路径本身就是根/空）时退回原串——显示个完整路径
/// 也好过什么都不显示。
pub(super) fn project_display_name(dir: &str) -> &str {
    std::path::Path::new(dir)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(dir)
}

/// 整数千分位（en-US 逗号）：`12480` → `"12,480"`。
pub(super) fn format_thousands(value: u64) -> String {
    let digits = value.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, b) in digits.bytes().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(b as char);
    }
    out
}

/// 定宽读数：两位小数 + 千分位、保留负号（`1284.5` → `"1,284.50"`）。
///
/// 经 `u64` 整数化（×100 取整）避免格式化器逐平台分叉；负零归 `-0.00`
/// 可接受（读数不是账本）。
pub(super) fn format_fixed2(value: f64) -> String {
    let scaled = (value.abs() * 100.0).round() as u64;
    format!(
        "{}{}.{:02}",
        if value.is_sign_negative() { "-" } else { "" },
        format_thousands(scaled / 100),
        scaled % 100
    )
}

/// 纬度读数：`28.034` → `"28.03° N"`，南半球出 `S`。
pub(super) fn format_lat(lat: f64) -> String {
    format!("{:.2}° {}", lat.abs(), if lat >= 0.0 { "N" } else { "S" })
}

/// 经度读数：`116.4` → `"116.40° E"`，西半球出 `W`。
pub(super) fn format_lon(lon: f64) -> String {
    format!("{:.2}° {}", lon.abs(), if lon >= 0.0 { "E" } else { "W" })
}
