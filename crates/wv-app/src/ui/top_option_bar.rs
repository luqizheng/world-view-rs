//! topOptionBar —— 工具选项条（贴 topbar 正下方的水平参数条）。
//!
//! ## 裁定与来历（2026-10-27 用户裁定）
//! - **命名**：沟通用名 `topOptionBar` / 模块 `top_option_bar` / egui 面板 id
//!   `wv_top_option_bar`；中文名「工具选项条」。与 ADR-0043 的「地图工具条」
//!   （`map_controls`，**选工具**）分工——本条**调当前工具的参数**。
//! - **动机**：地形笔刷 8 把工具与各自参数散在命令面板 / 快捷键与（已裁撤的）
//!   笔刷浮窗段里，作者「工具太多看不全」。本条把工具切换与参数调节收进一条
//!   常驻水平条；落位 = root Ui 显式从顶栏下缘起（`root_ui_below_top_bar`）
//!   ——egui 0.36 的 top Panel **没有**跨系统注册序堆叠（2026-10-29 修正，
//!   见该函数文档），原「天然贴在其正下方」的假设不成立。
//! - **对 2026-10-24 裁定的修正**（AGENTS §6.3 追记）：「工具切换只走命令
//!   面板 / 快捷键」修正为「**亦**可走本条头部瓷砖行」；命令面板与快捷键入口
//!   不变。2026-10-25「笔刷浮窗只服务 CellMap 结构编辑」随之**更成立**——
//!   笔画调参的家是本条，浮窗不再需要回头。
//!
//! ## 内容矩阵（按 [`BrushTool`] 切换，判据纯函数见下方三个 `shows_*`）
//!
//! | 工具 | 材料格 | 半径 | 软度 | 强度 |
//! |---|---|---|---|---|
//! | PaintKind | ✓ | ✓ | ✓ | |
//! | 意图（山脉/高原/丘陵/火山/湖） | | ✓ | | ✓ |
//! | Eraser | | ✓ | ✓ |（条尾常显「擦回底图」提示） |
//!
//! ## 边界（不做什么）
//! - 会话态只读写 [`BrushState`]（与被裁撤的旧笔刷段同款数据流；**非世界
//!   数据**，不涉 §3.3）。选工具语义与分发器一致：`tool = t; enabled = true`。
//!   ADR-0086 起另写 `ViewState.hypsometric_tint`
//!   （海拔分层设色强度，自 map_controls 槽 10.75 迁入——视图读色参数，
//!   随 CellMap 常驻条常在）。
//! - **不是浮动窗**——不进 `PanelState` / `ui-layout.yaml`；显隐由
//!   [`bar_visible`] 决定（CellMap 工作区常驻；其他工作区笔刷开着才出现
//!   ，关掉整条隐没、地图不吃掉一行）。
//! - 不复活「涂 / 塑 / 放」三段分组控件（2026-10-24 裁定退役，本条平铺 8 块）。
//!
//! ## 主要导出
//! - [`top_option_bar`] —— Bevy 系统（注册在 `ui::top_bar` 之后、同链 `.chain()`）。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{t, CurrentLocale, I18nKey, Locale};
use crate::state::{BrushState, BrushTool, DrillState, GridConfig, WorldProjection};
use crate::theme::ui_theme::{color32, margin_xy, proportional, UiShellConfig};
use crate::theme::ui_tokens::{font_size, radius, space};
use wv_ui::widgets::rect_stroke;

use super::{
    slider, terrain_name_at, terrain_swatch, ToolButton, ToolIcon, UiShell, ICON_BRUSH,
    ICON_ERASER, ICON_HILLS, ICON_LAKE, ICON_MOUNTAIN_RANGE, ICON_PLATEAU, ICON_VOLCANO,
};

/// 条高：与顶栏 [`super::top_bar::TOP_BAR_H`] 同韵（38px，内容 shell.hit=28）。
pub(crate) const BAR_H: f32 = 38.0;

/// 左端「笔刷」药丸宽（旧笔刷浮窗 Brush 段同款语义：点亮时再点 = 关笔刷）。
const PILL_W: f32 = 60.0;

/// 单个紧凑滑条组（标签 + 轨 + 数值）的总宽。
const SLIDER_W: f32 = 168.0;

/// 材料色块的命中区宽（12px 色块 + 呼吸位）。
const SWATCH_HIT: f32 = 20.0;

/// 条显隐判据（纯函数供单测）。
///
/// **入口-参数分离**（ADR-0089 D4）：本条只画「当前激活工具有参数」的参数段
/// ——参数族目前唯一 = 笔刷（`brush_enabled`），LinePoi / 测量 / 点选等工具
/// 没有可调参数（模式选择在工具条瓷砖），故不出现、也不发明参数
/// （「线宽」等新参数 = 数据 schema 变更，另立待裁）。叠加 ADR-0086 的
/// CellMap 常驻：海拔分层设色滑条是**视图读色参数**（不是工具参数），
/// 随条常在，不该等作者先开笔刷才出现；其他工作区没有常驻参数，空条
/// 只会白吃一行。
pub(crate) fn bar_visible(cellmap: bool, brush_enabled: bool) -> bool {
    cellmap || brush_enabled
}

/// 材料格只在涂抹档出现——橡皮写的是本图底图态（`MapBase`），`kind` 不参与；
/// 照常画出来就是「一边显示当前材质一边忽略材质」的谎话（旧笔刷面板教训）。
pub(crate) fn shows_material(tool: BrushTool) -> bool {
    tool == BrushTool::PaintKind
}

/// 软度（衰减权重）只在涂 / 擦两档出现。
pub(crate) fn shows_softness(tool: BrushTool) -> bool {
    matches!(tool, BrushTool::PaintKind | BrushTool::Eraser)
}

/// 强度只在意图档出现（[`BrushTool::is_intent`]）。
pub(crate) fn shows_strength(tool: BrushTool) -> bool {
    tool.is_intent()
}

/// 工具选项条（贴 topbar 正下方）。
///
/// 落位由 [`top_bar::root_ui_below_top_bar`](crate::ui::top_bar::root_ui_below_top_bar)
/// 显式保证（root Ui 从顶栏下缘起）——与 `ui::top_bar` 同链 `.chain()`
/// （`app_bootstrap/schedules.rs`）的注册顺序**不**决定落位（egui 0.36
/// Panel 无跨系统堆叠）。
#[allow(clippy::too_many_arguments)]
pub fn top_option_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut brush: ResMut<BrushState>,
    current_loc: Res<CurrentLocale>,
    // 折层元组（单系统 16 参上限同款手法）：世界侧只读三件 + 海拔分层
    // 设色强度（ADR-0055）落本条后要写的视图态（ADR-0086，`mode` 兼读
    // 常驻判据）。
    (grid, projection, drill, mut view): (
        Res<GridConfig>,
        Res<WorldProjection>,
        Res<DrillState>,
        ResMut<crate::state::ViewState>,
    ),
) {
    let cellmap = view.mode == crate::state::ViewMode::Topographic;
    if !bar_visible(cellmap, brush.enabled) {
        return;
    }
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let loc = current_loc.0;
    // 当前图是不是**场景图**（City / Interior 级）——决定画哪条材料条：
    // 场景图画分区条、世界图画地形材料条。两者共用 `brush.kind` 一个选择位，
    // 同时显示会互相抢（且在场景图上选「草原」会刷出群系色、破坏分区带）。
    let is_scene_map = drill
        .current_node(&projection.world)
        .is_some_and(|n| n.level != wv_core::map::MapLevel::World);
    // 不能用 `root_ui`：egui 0.36 的 Panel::top 只认父 Ui 剩余空间、无跨系统
    // 堆叠——fresh 全视口 Ui 会让本条叠回 y 0..38 盖住顶栏（2026-10-29 实机
    // 缺陷，见 `top_bar::root_ui_below_top_bar` 文档）。显式从顶栏下缘起。
    let mut root = super::top_bar::root_ui_below_top_bar(ctx);
    // 场景分区条：独立面板，落在**选项条下缘**（同一理由——它也是一条 top
    // Panel，若与选项条同起点会叠在一起）。
    if is_scene_map {
        district_strip_panel(ctx, &shell, &mut brush, loc, &projection, &drill);
    }
    egui::Panel::top("wv_top_option_bar")
        .exact_size(BAR_H)
        .frame(
            egui::Frame::NONE
                // 顶栏同款底色 / 描边，**无阴影**——层级低一档，别抢顶栏的视觉重量。
                .fill(color32(shell.palette.bg_app).gamma_multiply(0.96))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .inner_margin(margin_xy(space::SM, (BAR_H - shell.hit) * 0.5)),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                brush_pill(ui, &shell, &mut brush, loc);
                // ── 笔刷参数段：只在笔刷开着时画 ──
                // ADR-0086 后条在 CellMap 常驻，笔刷关着时工具瓷砖与各滑条
                // 若照常画出就是「一边显示参数一边不生效」的谎话（旧笔刷
                // 面板教训）——关 = 药丸熄灭 + 参数段整体让位。
                if brush.enabled {
                    ui.separator();
                    tool_tiles(ui, &shell, &mut brush, loc);
                    ui.separator();
                    match brush.tool {
                        // 涂抹：材料 + 半径 + 软度。
                        tool if shows_material(tool) && shows_softness(tool) => {
                            // 场景图上**不画地形材料条**——那里画的是分区条
                            // （`district_strip_panel`）。两条共用 `brush.kind`
                            // 一个选择位，同时显示会互相抢高亮；且在场景图上选
                            // 「草原」会刷出群系色（biome=3），既不是分区、又把
                            // 格子踢出分区带。
                            if !is_scene_map {
                                material_strip(ui, &shell, &mut brush.kind, loc);
                            }
                            radius_slider(ui, &shell, &mut brush.radius, &grid, loc);
                            compact_slider(
                                ui,
                                &shell,
                                t(loc, I18nKey::BrushSoftnessLabel),
                                &mut brush.softness,
                                0.0..=1.0,
                                false,
                                |n| format!("{}%", (n * 100.0).round() as i32),
                            );
                        }
                        // 橡皮：半径 + 软度 + 条尾「擦回底图」提示。
                        tool if shows_softness(tool) => {
                            radius_slider(ui, &shell, &mut brush.radius, &grid, loc);
                            compact_slider(
                                ui,
                                &shell,
                                t(loc, I18nKey::BrushSoftnessLabel),
                                &mut brush.softness,
                                0.0..=1.0,
                                false,
                                |n| format!("{}%", (n * 100.0).round() as i32),
                            );
                            // 「擦成什么」随图而变（切子图就可能变），作者下笔前
                            // 必须知道——贴条右缘常显（旧笔刷段文案沿用）。
                            let base_zh = drill
                                .current()
                                .and_then(|id| {
                                    projection
                                        .world
                                        .maps
                                        .iter()
                                        .find(|m| Some(&m.id) == Some(id))
                                })
                                .and_then(|m| m.base)
                                .unwrap_or(wv_core::map::MapBase::Ocean)
                                .zh_label();
                            let hint = format!(
                                "{} · {}",
                                t(loc, I18nKey::BrushHintErase),
                                t(loc, I18nKey::BrushBaseNow).replace("{base}", base_zh)
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(hint)
                                            .size(shell.scale.small)
                                            .color(shell.t3()),
                                    );
                                },
                            );
                        }
                        // 意图：强度 + 半径。
                        tool if shows_strength(tool) => {
                            compact_slider(
                                ui,
                                &shell,
                                t(loc, I18nKey::BrushStrengthLabel),
                                &mut brush.strength,
                                0.2..=2.0,
                                false,
                                |n| format!("×{:.2}", n),
                            );
                            radius_slider(ui, &shell, &mut brush.radius, &grid, loc);
                        }
                        _ => {}
                    }
                }
                // ── 海拔分层设色强度（ADR-0055；槽 10.75 → 本条，ADR-0086）──
                // 仅 CellMap（Topographic）：数据视图用颜色读海拔，0 = 纯群系
                // 色块，1 = 8 阶海拔色带完全替代群系色。**视图读色参数**，不是
                // 笔刷参数——随 CellMap 常驻条常在，笔刷关着也可调（2026-10-28
                // 用户裁定「放到 top option bar 上面，笔刷功能常开」）。会话态、
                // 不落盘。
                if cellmap {
                    ui.separator();
                    compact_slider(
                        ui,
                        &shell,
                        t(loc, I18nKey::HypsometricTintLabel),
                        &mut view.hypsometric_tint,
                        0.0..=1.0,
                        false,
                        |n| format!("{}%", (n * 100.0).round() as i32),
                    )
                    .on_hover_text(t(loc, I18nKey::HypsometricTintHint));
                }
            });
        });
}

/// 左端「笔刷」药丸：点亮 = 笔刷开着（金底），再点 = 关笔刷。
///
/// ADR-0086 后 CellMap 工作区里条**常驻**（海拔分层滑条不受笔刷开关影
/// 响），关笔刷只收起参数段；其他工作区关笔刷仍整条隐没。
fn brush_pill(ui: &mut egui::Ui, shell: &UiShell, brush: &mut BrushState, loc: Locale) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(PILL_W, shell.hit), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, radius::CONTROL, shell.raised_bg());
    if brush.enabled {
        p.rect_filled(rect, radius::CONTROL, shell.accent_soft());
    }
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        t(loc, I18nKey::BrushModeShort),
        proportional(font_size::SMALL),
        shell.t1(),
    );
    let resp = ui.interact(
        rect,
        egui::Id::new("wv_top_option_bar_pill"),
        egui::Sense::click(),
    );
    if resp.clicked() {
        brush.enabled = !brush.enabled;
    }
}

/// 工具瓷砖行：平铺 [`BrushTool::ALL`] 7 块（分组控件已退役，不复活）。
///
/// 点瓷砖 = 选工具 + 激活笔刷（与分发器 `CommandId::SelectBrushTool` 同语义）。
fn tool_tiles(ui: &mut egui::Ui, shell: &UiShell, brush: &mut BrushState, loc: Locale) {
    for tool in BrushTool::ALL {
        let tip = format!(
            "{}\n{}",
            t(loc, tool_name_key(tool)),
            t(loc, tool_hint_key(tool))
        );
        ToolButton::icon(ToolIcon::Svg(tool_icon(tool).clone()), &tip)
            .active(brush.tool == tool)
            .show(ui, shell, || {
                brush.tool = tool;
                brush.enabled = true;
            });
    }
}

/// 涂抹档材料条：单行（浮窗旧网格是 5×2，横条里放不下两行）。
/// 选中 = 金描边 + 段尾常显名称；悬停 tooltip = 地形名。
///
/// **遍历 [`MATERIALS`] 而非写死区间**：ADR-0070 D1 退役道路档时漏改
/// 循环上界，材质条多画了一块「?」僵尸砖（点了还涂出海）。
/// 单一权威表在 `shell::MATERIALS`，两处不再各写各的。
fn material_strip(ui: &mut egui::Ui, shell: &UiShell, kind: &mut u8, loc: Locale) {
    ui.label(
        egui::RichText::new(t(loc, I18nKey::BrushMaterialHeader))
            .size(shell.scale.micro)
            .color(shell.t2()),
    );
    for &k in crate::ui::shell::MATERIALS {
        let name = terrain_name_at(loc, k as usize);
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(SWATCH_HIT, shell.hit), egui::Sense::click());
        let p = ui.painter();
        if *kind == k {
            p.rect_filled(rect, radius::CONTROL, shell.active_bg());
            rect_stroke(
                p,
                rect,
                radius::CONTROL,
                egui::Stroke::new(1.5_f32, shell.focus()),
            );
        } else if resp.hovered() {
            p.rect_filled(rect, radius::CONTROL, shell.hover_bg());
        }
        let sw = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(12.0));
        p.rect_filled(sw, radius::BADGE, terrain_swatch(k));
        if resp.clicked() {
            *kind = k;
        }
        resp.on_hover_text(name);
    }
    ui.label(
        egui::RichText::new(format!("· {}", terrain_name_at(loc, *kind as usize)))
            .size(shell.scale.micro)
            .color(shell.t1()),
    );
}

/// 把材料选择位收进**分区保留带**。
///
/// `brush.kind` 是地形材料与分区**共用**的一个 u8：世界图上它是群系槽位
/// （0..=8），场景图上它是分区码（21..=32）。进场景图时它多半还停在世界图
/// 的取值上（缺省 3 = 草原）——作者第一笔就会刷出群系色，既不是分区，又把
/// 格子踢出分区带（分区判定看 biome 码）。
///
/// 故：**不在分区带内的选择位一律收编到「未分区」**（= 场景基底）。
/// 已在带内的原样保留——作者选了「商业区」再切图再回来，不该被重置。
pub(crate) fn district_selection(kind: u8) -> u8 {
    if wv_terrain::biome::is_district(kind) {
        kind
    } else {
        wv_terrain::biome::DISTRICT_FIRST
    }
}

/// 分区条专用 root Ui：max_rect 从「顶栏 + 选项条」下缘起。
///
/// **为什么不能复用 `top_bar::root_ui_below_top_bar`**：那个 root Ui 的
/// 剩余空间从**顶栏**下缘起，选项条就占着 y 38..76——分区条用同一个 root Ui
/// 会与选项条**完全重叠**。本条再往下让一个选项条高，落在选项条正下方。
fn root_ui_below_option_bar(ctx: &egui::Context) -> egui::Ui {
    let mut vp = ctx.viewport_rect();
    vp.min.y += super::top_bar::TOP_BAR_H + BAR_H;
    egui::Ui::new(
        ctx.clone(),
        "wv_root_district_strip".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(vp),
    )
}

/// 场景分区材料条：**独立顶栏面板**（不是主条的第二行）。
///
/// ## 为什么另起一个面板
///
/// 主条是 `exact_size(BAR_H)` 的**单行**固定高度横条（38px），已经装了
/// 笔刷胶囊 + 工具瓷砖 + 9 块地形材料 + 半径滑条 + 软度滑条。再塞 6 块
/// 分区（6×20=120px）会把 1280 宽的窗口挤爆，而 `exact_size` 意味着面板
/// **不会自动增高**——想换行就得改顶层布局，影响面远大于收益。
///
/// ## 落位：选项条下缘
///
/// egui 0.36 的 `Panel::top` 只从**父 Ui 的剩余空间**取位，**没有跨系统
/// 堆叠**（2026-10-29 实机缺陷，详见 `top_bar::root_ui_below_top_bar` 文档）。
/// 故本条不能复用选项条那个 root Ui——那会让两条**叠在同一处**
/// （都在 y 38..76）。本条用 [`root_ui_below_option_bar`]：从「顶栏 + 选项条」
/// 下缘起，即紧贴选项条正下方。
///
/// 选项条与本条的显示条件都含 `brush.enabled`（选项条：`bar_visible`；
/// 本条：门控），故两条总是同时出现或同时不出现——偏移量恒定，不需要
/// 按「选项条在不在」分支。
///
/// ## 显示门控
///
/// 「当前图是场景图」才出现——分区是场景图的概念，世界图上没有分区。
/// 门控天然落在「面板出不出来」上，不需要额外的开关状态。
///
/// ## 判定按「图级 ≠ World」
///
/// 不按语义（城市 / 地下城）——语义要落数据（`MapNode.scene_type`），而那
/// 是后续里程碑的事；本轮先按图级判，场景图都给出分区条。
fn district_strip_panel(
    ctx: &egui::Context,
    shell: &UiShell,
    brush: &mut BrushState,
    loc: Locale,
    projection: &WorldProjection,
    drill: &DrillState,
) {
    if !brush.enabled || !shows_material(brush.tool) {
        return;
    }
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    if node.level == wv_core::map::MapLevel::World {
        return;
    }
    // **接管选择位**：`brush.kind` 是地形材料与分区共用的一个 u8。进场景图
    // 时它多半还停在世界图的取值上（默认 3 = 草原）——作者第一笔就会刷出
    // 群系色（biome=3），既不是分区、又把格子踢出分区带。故场景图上一律
    // 把选择位收到分区带内（无选择时落在「未分区」= 基底）。
    brush.kind = district_selection(brush.kind);
    let mut root = root_ui_below_option_bar(ctx);
    egui::Panel::top("wv_top_option_bar_district")
        .exact_size(BAR_H)
        .frame(
            egui::Frame::NONE
                .fill(color32(shell.palette.bg_app).gamma_multiply(0.96))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .inner_margin(margin_xy(space::SM, (BAR_H - shell.hit) * 0.5)),
        )
        .show(&mut root, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("场景分区")
                        .size(shell.scale.micro)
                        .color(shell.t2()),
                );
                for &k in crate::ui::shell::DISTRICT_MATERIALS {
                    let name = terrain_name_at(loc, k as usize);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(SWATCH_HIT, shell.hit),
                        egui::Sense::click(),
                    );
                    let p = ui.painter();
                    if brush.kind == k {
                        p.rect_filled(rect, radius::CONTROL, shell.active_bg());
                        rect_stroke(
                            p,
                            rect,
                            radius::CONTROL,
                            egui::Stroke::new(1.5_f32, shell.focus()),
                        );
                    } else if resp.hovered() {
                        p.rect_filled(rect, radius::CONTROL, shell.hover_bg());
                    }
                    let sw = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(12.0));
                    p.rect_filled(sw, radius::BADGE, terrain_swatch(k));
                    if resp.clicked() {
                        brush.kind = k;
                    }
                    resp.on_hover_text(name);
                }
                ui.label(
                    egui::RichText::new(format!("· {}", terrain_name_at(loc, brush.kind as usize)))
                        .size(shell.scale.micro)
                        .color(shell.t1()),
                );
            });
        });
}

/// 半径滑条（世界单位 → 米显示；量程与换算沿用旧笔刷段口径）。
fn radius_slider(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut f32,
    grid: &GridConfig,
    loc: Locale,
) {
    compact_slider(
        ui,
        shell,
        t(loc, I18nKey::BrushRadiusLabel),
        value,
        grid.cell_size * 2.0..=600.0,
        true,
        |n| format!("{} m", (n / grid.cell_size * 10.0).round() as i32),
    );
}

/// 单行紧凑滑条：标签 t2 左、轨占中、mono 数值 t1 右（高 = shell.hit）。
///
/// wv-ui 的 `slider_row_brush` 是「两行」结构（标签行 + 整宽轨行），塞不进
/// 38px 横条；这里借 wv-ui 裸 [`slider`] 自拼单行版，视觉令牌同源。
/// 返回整组的 hover Response（海拔分层滑条靠它挂 tooltip）。
fn compact_slider<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    logarithmic: bool,
    formatter: impl Fn(f32) -> String,
) -> egui::Response {
    ui.allocate_ui(egui::vec2(SLIDER_W, shell.hit), |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(shell.scale.micro)
                    .color(shell.t2()),
            );
            slider(ui, shell, value, range, logarithmic);
            ui.label(
                egui::RichText::new(formatter(value.to_f64() as f32))
                    .size(shell.scale.micro)
                    .color(shell.t1())
                    .monospace(),
            )
        })
        .inner
    })
    .inner
}

/// 工具 → 图标（`ICON_*` 与旧笔刷浮窗瓷砖同源，wv-ui 资产）。
fn tool_icon(tool: BrushTool) -> &'static egui::ImageSource<'static> {
    match tool {
        BrushTool::PaintKind => &ICON_BRUSH,
        BrushTool::MountainRange => &ICON_MOUNTAIN_RANGE,
        BrushTool::Plateau => &ICON_PLATEAU,
        BrushTool::Hills => &ICON_HILLS,
        BrushTool::Volcano => &ICON_VOLCANO,
        BrushTool::Lake => &ICON_LAKE,
        BrushTool::Eraser => &ICON_ERASER,
    }
}

/// 工具 → 名称 Key（i18n 表是唯一文案源）。
fn tool_name_key(tool: BrushTool) -> I18nKey {
    match tool {
        BrushTool::PaintKind => I18nKey::BrushToolNamePaint,
        BrushTool::MountainRange => I18nKey::BrushToolNameRange,
        BrushTool::Plateau => I18nKey::BrushToolNamePlateau,
        BrushTool::Hills => I18nKey::BrushToolNameHills,
        BrushTool::Volcano => I18nKey::BrushToolNameVolcano,
        BrushTool::Lake => I18nKey::BrushToolNameLake,
        BrushTool::Eraser => I18nKey::BrushToolNameErase,
    }
}

/// 工具 → 交互提示 Key（瓷砖 tooltip 第二行）。
fn tool_hint_key(tool: BrushTool) -> I18nKey {
    match tool {
        BrushTool::PaintKind => I18nKey::BrushHintPaint,
        BrushTool::MountainRange => I18nKey::BrushHintRange,
        BrushTool::Plateau => I18nKey::BrushHintPlateau,
        BrushTool::Hills => I18nKey::BrushHintHills,
        BrushTool::Volcano => I18nKey::BrushHintVolcano,
        BrushTool::Lake => I18nKey::BrushHintLake,
        BrushTool::Eraser => I18nKey::BrushHintErase,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bar_visible, district_selection, shows_material, shows_softness, shows_strength,
        tool_hint_key, tool_name_key,
    };
    use crate::i18n::{t, Locale};
    use crate::state::BrushTool;

    /// 涂抹档：材料 / 半径 / 软度全给，强度不给（强度是意图参数）。
    #[test]
    fn paint_kind_shows_material_and_softness_but_not_strength() {
        let tool = BrushTool::PaintKind;
        assert!(shows_material(tool));
        assert!(shows_softness(tool));
        assert!(!shows_strength(tool));
    }

    /// 橡皮：软度有，无材料（写底图态，kind 不参与）、无强度。
    #[test]
    fn eraser_shows_softness_neither_material_nor_strength() {
        let tool = BrushTool::Eraser;
        assert!(shows_softness(tool));
        assert!(!shows_material(tool));
        assert!(!shows_strength(tool));
    }

    /// 全体意图工具：强度必给；材料 / 软度都不给。
    #[test]
    fn every_intent_tool_shows_strength_only() {
        for &tool in BrushTool::ALL.iter().filter(|t| t.is_intent()) {
            assert!(shows_strength(tool), "{tool:?} 应显示强度");
            assert!(!shows_material(tool), "{tool:?} 不应显示材料");
            assert!(!shows_softness(tool), "{tool:?} 不应显示软度");
        }
    }

    /// CellMap 工作区条常驻（ADR-0086「笔刷功能常开」：海拔分层滑条是视图
    /// 参数，不随笔刷开关隐没）；其他工作区维持「笔刷开着才出现」。
    #[test]
    fn bar_visible_in_cellmap_even_with_brush_off() {
        assert!(
            !bar_visible(false, false),
            "浏览族 + 笔刷关 = 无参数可调，不显条"
        );
        assert!(bar_visible(false, true), "浏览族 + 笔刷开 = 显条");
        assert!(bar_visible(true, false), "CellMap 常驻：笔刷关也显条");
        assert!(bar_visible(true, true));
    }

    /// 场景图上的材料选择位必须落在**分区带**内。
    ///
    /// 回归护栏：`brush.kind` 是地形材料与分区共用的一个 u8，缺省是 3
    /// （草原）。若场景图上不把它收进分区带，作者第一笔就会刷出群系色
    /// （biome=3）——既不是分区，又把格子踢出分区带（分区判定看 biome 码）。
    #[test]
    fn scene_map_selection_stays_inside_the_district_band() {
        // 缺省选择位（3 = 草原）**不在**分区带内 → 被收编到「未分区」。
        assert_eq!(
            district_selection(3),
            wv_terrain::biome::DISTRICT_FIRST,
            "世界图的选择位进场景图要被收编"
        );
        // 已在带内的原样保留——作者选了「商业区」再切图回来不该被重置。
        let commercial = wv_terrain::biome::DISTRICT_FIRST + 4;
        assert_eq!(district_selection(commercial), commercial);
        // 带外的其他取值（深水 0 / 越界 200）同样收编。
        assert_eq!(district_selection(0), wv_terrain::biome::DISTRICT_FIRST);
        assert_eq!(district_selection(200), wv_terrain::biome::DISTRICT_FIRST);
        // 分区材料表本身必须都在带内。
        for &k in crate::ui::shell::DISTRICT_MATERIALS {
            assert!(
                wv_terrain::biome::is_district(k),
                "分区材料 {k} 必须落在分区保留带内"
            );
        }
    }

    /// 7 个工具的名称 / 提示 Key 全覆盖：中英双语都非空——瓷砖纯图标，
    /// tooltip 是这些名字的唯一入口，漏译 = 悬停出一个空泡。
    #[test]
    fn every_tool_resolves_name_and_hint_in_both_locales() {
        for tool in BrushTool::ALL {
            let name_key = tool_name_key(tool);
            let hint_key = tool_hint_key(tool);
            for loc in [Locale::Zh, Locale::En] {
                assert!(!t(loc, name_key).is_empty(), "{tool:?} {loc:?} 名称缺失");
                assert!(!t(loc, hint_key).is_empty(), "{tool:?} {loc:?} 提示缺失");
            }
        }
    }
}
