//! 顶栏三区：身份 / 主控（Browse–Edit）/ 读数。

use super::prefs::*;
use super::status::vsep_h_at;
use super::window_controls::{window_controls_segment, WindowChromeState, CONTROLS_W};
use super::*;

use bevy::window::PrimaryWindow;

/// 顶栏：窗控（最左）/ 身份 / 主控（居中独立 Area）/ 读数。
#[allow(clippy::too_many_arguments)]
pub fn top_bar(
    mut contexts: EguiContexts,
    mut shell_cfg: ResMut<UiShellConfig>,
    projection: Res<WorldProjection>,
    recents_res: Res<RecentProjects>,
    mut menu: ResMut<ProjectMenuState>,
    mut palette: ResMut<crate::ui_command_palette::CommandPalette>,
    mut view: ResMut<ViewState>,
    mut panels: ResMut<crate::state::PanelState>,
    mut settings: ResMut<SettingsDialog>,
    mut current_loc: ResMut<CurrentLocale>,
    // Gate-2（[ADR-0052] §9.4）：无当前地图时顶栏 CellMap 段灰化。
    drill: Res<DrillState>,
    // 无边框窗（2026-10-25）：窗控三键 / 拖拽 / 双击最大化要写主窗口，
    // 关闭键写 `AppExit` 消息（bevy 0.19 事件体系改名 Message；单窗口
    // 应用，关主窗 = 退出）。
    mut primary: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
    mut chrome: ResMut<WindowChromeState>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let Ok(mut window) = primary.single_mut() else {
        return;
    };
    // web `.topbar` 精确高度 38px。
    let h = TOP_BAR_H;
    let recents = recents_res.visible(&projection);
    let mut root = root_ui(ctx);
    let mut drag_resp: Option<egui::Response> = None;
    egui::Panel::top("wv_top_bar")
        .exact_size(h)
        .frame(
            egui::Frame::NONE
                // web：color-mix(app 96%) + border-bottom + 0 4px 20px 阴影
                .fill(color32(shell.palette.bg_app).gamma_multiply(0.96))
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 20,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(56),
                })
                .inner_margin(crate::theme::ui_theme::margin_xy(space::SM, 0.0)),
        )
        .show(&mut root, |ui| {
            // 拖拽区**最先注册**（垫底）：后画的窗控/菜单/标签压在其上，
            // 点控件不会误触拖窗；空白条背吃到拖拽与双击（无边框窗 2026-10-25）。
            let drag = ui.interact(
                ui.max_rect(),
                egui::Id::new("wv_top_bar_drag"),
                egui::Sense::click_and_drag(),
            );
            ui.horizontal(|ui| {
                // ── 左区：项目按钮 + 面包屑（web .top-left）──
                identity_segment(
                    ui,
                    &shell,
                    &projection,
                    &view,
                    &recents,
                    &mut menu,
                    &current_loc,
                );
                // ── 右区：保存态 / 分隔线 / 日夜 / 语言 / Rebuild / 设置 ──
                // 几何按右缘绝对反推（见 `readout_segment` 文档）；右缘让出
                // 窗控三键宽，两段同用绝对反推、永不相叠。
                readout_segment(
                    ui,
                    &shell,
                    &projection,
                    &mut current_loc,
                    &mut shell_cfg,
                    &mut settings,
                    &mut palette,
                    ui.max_rect().right() - CONTROLS_W,
                );
                // ── 最右上角：窗控三键（最小化/最大化/关闭，关闭键贴右缘）──
                // 首轮裁定放最左上，2026-10-25 二轮改裁右上（Windows 惯例位）。
                window_controls_segment(ui, &shell, &mut window, &mut chrome, &mut exit);
            });
            drag_resp = Some(drag);
        });

    // ── 中区：web grid 的 `auto` 列——独立 Area 锚视口顶部居中 ──
    //（TopBottomPanel 里做不了「剩余宽度居中」，锚定 Area 是 egui 等价物；
    //  高 38 内垂直居中：内容 shell.hit=28 → 偏移 (38-28)/2 = 5px。）
    egui::Area::new(egui::Id::new("wv_topbar_middle"))
        .anchor(
            egui::Align2::CENTER_TOP,
            egui::vec2(0.0, (h - shell.hit) * 0.5),
        )
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                browse_edit_segment(
                    ui,
                    &shell,
                    &mut view,
                    &mut panels,
                    &mut palette,
                    current_loc.0,
                    drill.current().is_some(),
                );
            });
        });

    // ── 无边框窗（2026-10-25）：顶栏空白背按住拖动移窗、双击切最大化 ──
    //（Windows 惯例；中区 Browse/Edit 分段与各控件压在拖拽区之上，不受影响。）
    if let Some(drag) = drag_resp {
        if drag.drag_started() {
            window.start_drag_move();
        }
        if drag.double_clicked() {
            chrome.maximized = !chrome.maximized;
            window.set_maximized(chrome.maximized);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn identity_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    view: &ViewState,
    recents: &[RecentProject],
    menu: &mut ProjectMenuState,
    current_loc: &CurrentLocale,
) {
    // 项目按钮（logo + 粗体名 + chevron，见 project_menu）+ 组间距。
    project_menu_inner(ui, shell, projection, recents, menu, current_loc.0);
    ui.add_space(space::MD);

    // 面包屑（web .breadcrumbs：gap 4、上级 text_3、**末级 text_2**）。
    let crumb = crate::ui_time::format_breadcrumb(projection, &view.current_map);
    for (i, seg) in crumb.iter().enumerate() {
        if i > 0 {
            ui.add(
                egui::Image::new(ICON_CHEVRON_RIGHT)
                    .tint(shell.t3())
                    .fit_to_exact_size(egui::vec2(12.0, 12.0)),
            );
            ui.add_space(space::XS);
        }
        let color = if i + 1 == crumb.len() {
            shell.t2()
        } else {
            shell.t3()
        };
        ui.label(
            egui::RichText::new(seg.as_str())
                .size(shell.scale.base)
                .color(color),
        );
        ui.add_space(space::XS);
    }

    // 工程未加载时的醒目错误（§5.1）。脏态徽标在右侧读数段。
    if projection.load_error.is_some() {
        ui.add_space(space::SM);
        badge(ui, shell, BadgeKind::Err, "工程未加载");
    }
}

/// Browse / Edit / CellMap 三段：`Browse` → `ViewMode::VectorFlat`（默认浏览）、
/// `Edit` → `ViewMode::Schematic`（色块格子）、`CellMap` → 单格编辑工作区
/// （2026-10-20 用户提案：等高分析底图 + EditCell 工具 + 只留相关面板）。
///
/// `Faction` 已从顶栏移除（2026-10-19 用户裁定），但 `ViewMode::Faction`
/// 本身仍在——命令面板「视图」组 + 快捷键 `3` 可切（见
/// [`crate::ui_command_palette`]）。段宽 64 对齐 Figma Mode selector。
///
/// **CellMap 是 UI 工作区编排**，不是新 ViewMode——渲染技法复用
/// `ViewMode::Topographic`（ADR-0039），叠加三件事：
/// ① `view.mode = Topographic`；② 经 `CommandPalette` 排
/// `SelectMapTool(EditCell)`（互斥/开关语义全在分发器，不重复实现）；
/// ③ 面板编排——快照全部面板可见性到
/// `PanelState::cell_workspace_backup`，只开 Inspector；退出时还原。
/// 再点 CellMap = 退出（回快照 + 关工具）；点 Browse/Edit 也退出。
///
/// 数据层 `view.mode` 仍按原枚举流转；命令面板 label 来自 `ViewMode::label()`
/// （中文），不受影响（v0.3 用户拍板：仅顶栏用英文，段名硬编码英文字符串）。
#[allow(clippy::too_many_arguments)]
fn browse_edit_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &mut ViewState,
    panels: &mut crate::state::PanelState,
    palette: &mut crate::ui_command_palette::CommandPalette,
    loc: crate::i18n::Locale,
    can_enter_cellmap: bool,
) {
    use crate::state::Panel;
    use crate::ui_command_palette::{CommandId, MapToolId};

    let labels = ["Browse", "Edit", "CellMap"];
    let modes = [
        ViewMode::VectorFlat,
        ViewMode::Schematic,
        ViewMode::Topographic,
    ];
    // 当前若是 Faction 等段外模式，`usize::MAX` 让各段都不高亮，
    // 不误报 Browse；不改 `view.mode`（V1：不阻止）。
    let active = modes
        .iter()
        .position(|m| *m == view.mode)
        .unwrap_or(usize::MAX);
    // **Gate-2**（[ADR-0052] §9.4）：没有当前地图时 `CellMap` 段灰化不可点——
    // 进去了也没有格可编辑。`Browse` / `Edit` 不受影响（它们是看图档，
    // 且 MapTree 仍要能选图）。
    let seg_disabled = [false, false, !can_enter_cellmap];

    let seg_resp = crate::ui::widgets::segmented_control::segmented_with_disabled(
        ui,
        shell,
        "wv_browse_edit",
        64.0,
        &labels,
        active,
        font_size::SMALL,
        &seg_disabled,
        |i| {
            match i {
                // ── CellMap：进/出工作区 ──
                2 => {
                    if view.mode == ViewMode::Topographic {
                        // 已在工作区 → 退出：还原面板快照 + 关 EditCell 工具
                        // （排 toggle 语义的 SelectMapTool，分发器统一关）。
                        if let Some(backup) = panels.cell_workspace_backup.take() {
                            for (p, v) in backup {
                                panels.set_visible(p, v);
                            }
                        }
                        palette.pending = Some(CommandId::SelectMapTool(MapToolId::EditCell));
                        view.mode = ViewMode::Schematic;
                    } else {
                        // 进入：快照面板 → 只开笔刷窗 → 排 EditCell 工具。
                        let backup = Panel::ALL
                            .iter()
                            .map(|p| (*p, panels.is_visible(*p)))
                            .collect();
                        panels.cell_workspace_backup = Some(backup);
                        for p in Panel::ALL {
                            panels.set_visible(p, false);
                        }
                        // 2026-10-24 用户裁定：CellMap 的编辑控件（读数条 /
                        // 落笔值 / 泛洪判据 / 派生运算 / 单格与批量表单）
                        // 迁入**笔刷浮动窗**——地形编辑的「笔刷语义」聚在
                        // 一起，Inspector 回归通用属性检查。故工作区开的
                        // 是 Brush 而不是 Inspector。
                        panels.set_visible(Panel::Brush, true);
                        palette.pending = Some(CommandId::SelectMapTool(MapToolId::EditCell));
                        view.mode = ViewMode::Topographic;
                    }
                }
                // ── Browse / Edit：先退工作区（若在），再切底图模式 ──
                plain => {
                    if let Some(backup) = panels.cell_workspace_backup.take() {
                        for (p, v) in backup {
                            panels.set_visible(p, v);
                        }
                        // 从 CellMap 切走：关 EditCell 工具（toggle）。
                        palette.pending = Some(CommandId::SelectMapTool(MapToolId::EditCell));
                    }
                    view.mode = modes[plain];
                }
            }
        },
    );
    // Gate-2 的 tooltip：禁用段只画不注册命中，故提示挂在整段的分配矩形上
    // （只要光标落在段控件范围内就提示「请先打开一张地图」）。
    if !can_enter_cellmap {
        seg_resp.on_hover_text(crate::i18n::t(loc, I18nKey::MapControlsNeedMap));
    }
}

/// 顶栏右区（Figma `3:20980`「Workspace preferences」）。
///
/// 从左到右：保存态 → 分隔线 → 日夜 → 语言 → Rebuild → 设置。
///
/// **几何全部按 `right_edge` 反推、绝对定宽**，不依赖 `ui` 当前方向：
/// 该段原先挂在 `right_to_left` 布局里，而 `egui::Ui::put`（放图标）会按子矩形
/// **推进游标**——每个控件的尾随 icon 都把下一件的起点顶偏，结果两个 26px
/// 控件互相叠了 27px（2026-10-19 实机量 rect 确认）。改成「先算宽度、从右往左
/// 摆」，`put` 再推游标也不影响任何其它控件。`right_edge` 由调用方给
/// （视口右缘减窗控三键宽 [`super::window_controls::CONTROLS_W`]，2026-10-25
/// 窗控入顶栏右上后让位）。
#[allow(clippy::too_many_arguments)]
fn readout_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    current_loc: &mut CurrentLocale,
    shell_cfg: &mut UiShellConfig,
    settings: &mut SettingsDialog,
    palette: &mut crate::ui_command_palette::CommandPalette,
    right_edge: f32,
) {
    /// 右区相邻控件间距（Figma Workspace preferences `gap: 8px`）。
    const GAP: f32 = space::SM;

    let bar = ui.max_rect();
    let cy = bar.center().y;
    let mut right = right_edge;

    // ── 设置入口（最右）：28×28 命中区、14px Lucide `settings-2` ──
    right -= shell.hit;
    let hit_rect = egui::Rect::from_min_max(
        egui::pos2(right, cy - shell.hit * 0.5),
        egui::pos2(right + shell.hit, cy + shell.hit * 0.5),
    );
    settings_button(ui, shell, hit_rect, settings, current_loc);
    right -= GAP;

    // ── Rebuild（2026-10-19 用户裁定：移到语言控件之后，与日夜同款 26px 控件）──
    let rebuild_w = preference_width(ui, "Rebuild", PrefTrailing::Shortcut("F5"));
    right -= rebuild_w;
    let rebuild_rect = egui::Rect::from_min_max(
        egui::pos2(right, cy - PREF_HEIGHT * 0.5),
        egui::pos2(right + rebuild_w, cy + PREF_HEIGHT * 0.5),
    );
    rebuild_control(ui, shell, rebuild_rect, palette, current_loc);
    right -= GAP;

    // ── 语言控件（ADR-0018）：图标 + 短标签 + chevron，点击弹菜单 ──
    let short = language_short(current_loc.0);
    let lang_w = preference_width(ui, short, PrefTrailing::Chevron);
    right -= lang_w;
    let lang_rect = egui::Rect::from_min_max(
        egui::pos2(right, cy - PREF_HEIGHT * 0.5),
        egui::pos2(right + lang_w, cy + PREF_HEIGHT * 0.5),
    );
    language_control(ui, shell, lang_rect, short, current_loc);
    right -= GAP;

    // ── 日夜控件（D2：只切界面外壳，不碰 `view.map_palette` 的地图配色）──
    let (icon, label_key, hint_key) = theme_parts(shell_cfg);
    let label = crate::i18n::t(current_loc.0, label_key);
    let theme_w = preference_width(ui, label, PrefTrailing::Shortcut("Ctrl+J"));
    right -= theme_w;
    let theme_rect = egui::Rect::from_min_max(
        egui::pos2(right, cy - PREF_HEIGHT * 0.5),
        egui::pos2(right + theme_w, cy + PREF_HEIGHT * 0.5),
    );
    theme_control(
        ui,
        shell,
        theme_rect,
        icon,
        label,
        hint_key,
        shell_cfg,
        current_loc,
    );
    right -= GAP;

    // ── 分隔线（1×16，Figma Separator）──
    right -= 1.0;
    vsep_h_at(ui, shell.border_subtle(), 16.0, egui::pos2(right, cy));
    right -= GAP;

    // ── 保存态：绿点 + Saved / 橙点 + Unsaved。未加载工程时整段不画 ──
    if projection.project_dir.is_some() {
        save_indicator(ui, shell, projection.dirty, current_loc.0, right, cy);
    }
}
