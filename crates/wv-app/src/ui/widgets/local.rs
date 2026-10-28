//! wv-app 侧的 widget 域件（ADR-0026 D3）：`float_window`（Panel 包装）+
//! `terrain_grid`（笔刷面板地形网格）。
//!
//! 历史来源：二者原住 `ui/widgets/{floater,primitives}.rs`；2026-10-17 控件层
//! 迁 `wv-ui` 时随域留下——前者耦合 `Panel` / `PanelState` / `ui-layout.yaml`，
//! 后者耦合 i18n 与世界色板（`terrain_swatch` 走 `crate::theme::colors`）。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::i18n::Locale;
use crate::state::{Panel, PanelState};
use crate::ui::shell::{terrain_name_at, terrain_swatch};
use crate::ui_layout::LayoutFile;
use wv_ui::shell::UiShell;
use wv_ui::ui_tokens::{radius, space};
use wv_ui::widgets::{rect_stroke, BadgeKind, FloatWindow};

/// 浮动面板外壳（`Panel` 特化薄包装，ADR-0026 D3 留守 wv-app）。
///
/// 可见性 / 跨会话位置尺寸（`ui-layout.yaml`）/ 沉浸模式淡化在此解析后
/// 全部委托 [`FloatWindow`]；本帧的拖动结果由 egui 按窗口 `Id` 自己记住，
/// 并由 [`crate::ui_shell::persist_layout_on_change`] 回写磁盘
/// （D1：面板是客人，地图是主体）。
///
/// ## 层序约定（2026-10-14 用户要求「弹窗/浮动都在最顶层」后固化）
///
/// 本外壳**不设** `order`——即 egui 缺省 `Middle`：面板互斥点击由 egui 的
/// 「点谁谁置顶」管理，且永远压在 egui Background 面板（顶栏/状态栏）与
/// Bevy 地图之上。模态弹窗（新建地图 / 新建工程 / 命令面板 / 欢迎页 /
/// 启动加载）在 `Order::Foreground`—— egui 绘制序
/// `Background < Middle < Foreground < Tooltip`，故弹窗压面板、悬浮提示仍在最上。
/// 新增浮层时按此表对号入座，不要把弹窗放回 Middle。
///
/// ## 活动范围（2026-10-25 无边框窗配套）
///
/// 全部浮窗统一 `constrain_to(central_area)`：视口挖去顶栏 38 与状态栏
/// 28——拖不进两端 chrome（状态栏因此永不被盖，也不必抬层：曾试验把
/// 状态栏根 Ui 提 Foreground，全视口 widget 会闷死 hit_test 的低层命中，
/// 见 `top_bar::root_ui` 文档）、也拖不出视口；按大窗口保存的
/// `ui-layout.yaml` 坐标在小窗口下被逐帧钳回可见区。
#[allow(clippy::too_many_arguments)]
pub fn float_window(
    ctx: &egui::Context,
    shell: &UiShell,
    layouts: &LayoutFile,
    panels: &mut PanelState,
    panel: Panel,
    tag: Option<(BadgeKind, String)>,
    loc: Locale,
    body: impl FnOnce(&mut egui::Ui),
) {
    if !panels.is_visible(panel) {
        return;
    }

    let layout = crate::ui_layout::current_layout(layouts, panels, panel);
    let id = egui::Id::new(("wv_panel", panel));

    // 沉浸模式：整体淡化但**仍可交互**（D3：状态永不抑制）。
    let alpha = if panels.immersive { 0.10 } else { 0.96 };

    // 越界回拉（2026-10-25）：egui 的 `constrain_to` 不钳位置（只钳尺寸 /
    // 锚点 / move 命中区），上一会话拖进 chrome 的 `ui-layout.yaml` 坐标
    // 会原样复现。见 [`crate::ui::top_bar::snap_into_central`] 文档。
    //
    // 宽度界（2026-10-28 A 案）：所有面板统一吃 `width_bounds` 策略——
    // 自适应测量 / 用户拖拽 / yaml 记忆尺寸三路都被钳，窗不会再被
    // 整宽内容撑爆（机制见 wv-ui `FloatWindow::max_width` 文档）。
    let (min_w, max_w) = panel.width_bounds();
    let mut fw = FloatWindow::new(id, panel.title(loc))
        .icon(panel.icon())
        .tag(tag)
        .default_pos(egui::pos2(layout.x, layout.y))
        .default_size(egui::vec2(layout.width, layout.height))
        .default_open(!layout.collapsed)
        .alpha(alpha)
        .constrain_to(crate::ui::top_bar::central_area(ctx))
        .min_width(min_w);
    if max_w.is_finite() {
        fw = fw.max_width(max_w);
    }
    if let Some(p) = crate::ui::top_bar::snap_into_central(ctx, id) {
        fw = fw.current_pos(p);
    }
    fw.show(ctx, shell, body);
}

/// 地形色块瓷砖网格（2026-10-16 改版：5 列 × 2 行纯色块，26 px 方块；
/// ADR-0070 D1 退役后 10 → 9 块，末行 4 块）。
///
/// 旧版 2 列 × 5 行「色块 + 名称」行高 22 px——名字占掉一半面板宽，
/// 十种地形竖排近一掌高。改版后名称两处补偿：悬停 `on_hover_text`（逐块），
/// 选中名由面板段标题常显（`TERRAIN MATERIAL · 草原`）。选中 = 1.5 px
/// `focus` 金描边（截图验收过的既视感），hover = `hover_bg` 底。
///
/// 「道路」（旧第 10 块）已退役（ADR-0070 D1）：路改由线段 POI 承载，
/// 不再入笔刷色板——保留控件与本表同收口，避免回归时色板与
/// `brush_target`（9 档权威表）漂移出「点了道路涂出水」的错位。
pub fn terrain_grid(ui: &mut egui::Ui, shell: &UiShell, selected: &mut u8, loc: Locale) {
    const SWATCH: f32 = 26.0;
    egui::Grid::new("terrain_swatches")
        .num_columns(5)
        .spacing([space::XS, space::XS])
        .show(ui, |ui| {
            for k in 0u8..9 {
                let name = terrain_name_at(loc, k as usize);
                let is_sel = *selected == k;
                let (rect, resp) =
                    ui.allocate_exact_size(egui::Vec2::splat(SWATCH), egui::Sense::click());
                let painter = ui.painter();
                if is_sel {
                    painter.rect_filled(rect, radius::CONTROL, shell.active_bg());
                    rect_stroke(
                        painter,
                        rect,
                        radius::CONTROL,
                        egui::Stroke::new(1.5_f32, shell.focus()),
                    );
                } else if resp.hovered() {
                    painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
                }
                // 16px 色块居中（瓷砖是 26px 命中区，色块小一点留呼吸）。
                let sw_size = 16.0;
                let sw = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(sw_size));
                painter.rect_filled(sw, radius::BADGE, terrain_swatch(k));
                if resp.clicked() {
                    *selected = k;
                }
                resp.on_hover_text(name);
                // 5 列换行：9 块 = 5 + 4（末行 4 块）。
                if (k + 1) % 5 == 0 {
                    ui.end_row();
                }
            }
        });
}
