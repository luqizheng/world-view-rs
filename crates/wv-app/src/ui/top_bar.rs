//! 顶栏（38px v0.3 · web `.topbar` 对齐）+ 状态栏（28px · Figma #3:21001）。
//!
//! ## 职责
//! - `top_bar` —— Figma `3:20966` 三区：左 = 项目按钮（蓝 logo +
//!   粗体名 + 下翻箭头，见 [`super::project_menu`]）+ 面包屑；**中 = Browse /
//!   Edit 分段（独立 Area 锚视口顶部居中）**；右 = 保存态 / 分隔线 /
//!   **日夜模式控件** / **语言控件** / **Rebuild** / **设置入口（`settings-2`）**。
//!
//! ## 右区对齐（2026-10-19 用户裁定）
//! - Figma 的 `Cozy`（密度）与 `Dusk`（地图主题）两个 26px 控件**不建**：
//!   密度与地图主题都走命令面板（`Ctrl+K`）。顶栏中区的地图主题分段也已于
//!   同日移除——`Faction` 同理（命令面板「视图」组 + 快捷键 `3` 仍可切）。
//! - `settings-2` 是应用设置入口：浮动窗口显隐 + 重置位置（见
//!   [`super::settings_dialog`]）；工程菜单因此只剩工程基本操作。
//! - zoom / entities / ⌘K 不再占顶栏：对象计数在状态栏、缩放控件在地图工具条、
//!   命令面板仍是 `Ctrl+K`。
//! - `status_bar`（2026-10-16 按 Figma `Untitled` #3:21001 重做）——
//!   左 = `WORLD–VIEW` + 版本 + 工程目录名；中 = 光标世界坐标 X/Y +
//!   经纬度（真实地理图才有）+ EPSG；右 = 对象计数 + 选中数 + 警告徽标 +
//!   FPS。
//!
//! ## web 对齐要点（`index.css` / `App.tsx`，v0.3 校准）
//! - 高度 38px（`.topbar`）；底色 `bg_app` 96% + 底边框 + `0 4px 20px` 投影。
//! - 分段控件（`.segmented`）：容器 `bg_app` 底 + border + 圆角 7、内边距 2px；
//!   选中段 `bg_active` 底 + `text_1`，**内嵌**于容器而非顶满。
//! - 中区段宽 64/段（Figma Mode selector）。`Rebuild` 2026-10-19 起移到右区
//!   语言控件之后，与日夜控件同款 26px 偏好控件（图标 + 文字 + `F5`）。
//! - ⌘K 按钮已退出顶栏（`Ctrl+K` 仍可开命令面板）。
//! - 面包屑末级 `text_2`（web `b` 标签色），非末级 `text_3`。
//!
//! ## 边界
//! - **不**画项目管理菜单的展开式内容 —— [`super::project_menu::project_menu`]。
//! - **不**装配 Bevy App / Startup —— [`crate::app_bootstrap`]。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::i18n::{CurrentLocale, I18nKey};
use crate::state::{
    DrillState, RecentProject, RecentProjects, ViewMode, ViewState, WorldProjection,
};
use crate::theme::ui_theme::{color32, color32_alpha, mono, proportional, UiShellConfig};

use super::project_menu::{project_menu_inner, ProjectMenuState};
use super::settings_dialog::SettingsDialog;
use super::shell::UiShell;
use super::widgets::{
    badge, icon_image, BadgeKind, ICON_CHEVRON_DOWN, ICON_CHEVRON_RIGHT, ICON_FOLDER_OPEN,
    ICON_LANGUAGES, ICON_MOON, ICON_REFRESH, ICON_SETTINGS, ICON_SUN,
};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui_command_palette::CommandId;

/// 顶栏：身份 / 主控（居中独立 Area）/ 读数。**唯一例外**的 `TopBottomPanel`。
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
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    // web `.topbar` 精确高度 38px。
    let h = 38.0;
    let recents = recents_res.visible(&projection);
    let mut root = root_ui(ctx);
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
                // 几何按右缘绝对反推（见 `readout_segment` 文档），
                // 不再依赖 `right_to_left` 的游标推进。
                readout_segment(
                    ui,
                    &shell,
                    &projection,
                    &mut current_loc,
                    &mut shell_cfg,
                    &mut settings,
                    &mut palette,
                );
            });
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
/// **几何全部按视口右缘反推、绝对定宽**，不依赖 `ui` 当前方向：
/// 该段原先挂在 `right_to_left` 布局里，而 `egui::Ui::put`（放图标）会按子矩形
/// **推进游标**——每个控件的尾随 icon 都把下一件的起点顶偏，结果两个 26px
/// 控件互相叠了 27px（2026-10-19 实机量 rect 确认）。改成「先算宽度、从右往左
/// 摆」，`put` 再推游标也不影响任何其它控件。
fn readout_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    current_loc: &mut CurrentLocale,
    shell_cfg: &mut UiShellConfig,
    settings: &mut SettingsDialog,
    palette: &mut crate::ui_command_palette::CommandPalette,
) {
    /// 右区相邻控件间距（Figma Workspace preferences `gap: 8px`）。
    const GAP: f32 = space::SM;

    let bar = ui.max_rect();
    let cy = bar.center().y;
    let mut right = bar.right();

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

/// 偏好控件高度（Figma `EL-bf36dfe9` 26px）。
const PREF_HEIGHT: f32 = 26.0;
/// 偏好控件左右内边距（Figma `padding: 0px 8px`）。
const PREF_PAD: f32 = 8.0;
/// 偏好控件子项间距（Figma `gap: 6px`）。
const PREF_GAP: f32 = 6.0;
/// 偏好控件图标尺寸（Figma 12×12）。
const PREF_ICON: f32 = 12.0;

/// 语言短标签（Zh=「简」/ En=「EN」）。
fn language_short(loc: crate::i18n::Locale) -> &'static str {
    match loc {
        crate::i18n::Locale::Zh => "简",
        crate::i18n::Locale::En => "EN",
    }
}

/// 日夜控件的（图标, 当前模式标签, 切换提示）——图标表示**当前**模式。
fn theme_parts(cfg: &UiShellConfig) -> (egui::ImageSource<'static>, I18nKey, I18nKey) {
    if cfg.theme.is_dark() {
        (
            ICON_MOON,
            I18nKey::TopBarThemeDark,
            I18nKey::TopBarSwitchToDay,
        )
    } else {
        (
            ICON_SUN,
            I18nKey::TopBarThemeLight,
            I18nKey::TopBarSwitchToNight,
        )
    }
}

/// 保存态（Figma `3:20981/3:20982`：5px 圆点 + 11px 弱文本）。
///
/// 设计稿示例带时刻（`Saved 14:32`），但工程目前**不持久化保存时间戳**；
/// 不拿「当前时钟」冒充保存时刻（会骗人），只报已保存 / 未保存。
fn save_indicator(
    ui: &mut egui::Ui,
    shell: &UiShell,
    dirty: bool,
    loc: crate::i18n::Locale,
    right: f32,
    cy: f32,
) {
    let (dot, key) = if dirty {
        (color32(shell.palette.badge_warn), I18nKey::TopBarUnsaved)
    } else {
        (shell.success(), I18nKey::TopBarSaved)
    };
    let galley = ui.painter().layout_no_wrap(
        crate::i18n::t(loc, key).to_owned(),
        proportional(font_size::MICRO),
        shell.t3(),
    );
    let text_x = right - galley.size().x;
    ui.painter().galley(
        egui::pos2(text_x, cy - galley.size().y * 0.5),
        galley,
        shell.t3(),
    );
    let dot_center = egui::pos2(text_x - space::XS - 2.5, cy);
    ui.painter().circle_filled(dot_center, 2.5, dot);
}

/// 偏好控件尾随内容（Figma：快捷键文本 / 下翻 chevron）。
#[derive(Clone, Copy)]
enum PrefTrailing<'a> {
    Shortcut(&'a str),
    Chevron,
}

/// 偏好控件总宽（含左右内边距）。
fn preference_width(ui: &egui::Ui, label: &str, trailing: PrefTrailing<'_>) -> f32 {
    let label_w = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            proportional(font_size::MICRO),
            ui.visuals().text_color(),
        )
        .size()
        .x;
    let trail_w = match trailing {
        PrefTrailing::Shortcut(keys) => {
            ui.painter()
                .layout_no_wrap(keys.to_owned(), mono(10.0), ui.visuals().weak_text_color())
                .size()
                .x
        }
        PrefTrailing::Chevron => PREF_ICON,
    };
    PREF_PAD + PREF_ICON + PREF_GAP + label_w + PREF_GAP + trail_w + PREF_PAD
}

/// 26px 偏好控件（Figma `EL-bf36dfe9`）：raised 底 + 1px 描边 + 圆角 4，
/// 内边距左右 8、子项间距 6。整块可点（含 padding 区）。
///
/// 内容全部在**给定矩形内**左起绝对定位——不参与父布局方向。
fn preference_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    icon: egui::ImageSource<'static>,
    label: &str,
    trailing: PrefTrailing<'_>,
) -> egui::Response {
    ui.painter().rect(
        rect,
        radius::BADGE,
        shell.raised_bg(),
        egui::Stroke::new(1.0, shell.border_subtle()),
        egui::StrokeKind::Inside,
    );

    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), proportional(font_size::MICRO), shell.t1());
    let trail_galley = match trailing {
        PrefTrailing::Shortcut(keys) => Some(ui.painter().layout_no_wrap(
            keys.to_owned(),
            mono(10.0),
            shell.t3(),
        )),
        PrefTrailing::Chevron => None,
    };

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + PREF_PAD + PREF_ICON * 0.5, rect.center().y),
        egui::vec2(PREF_ICON, PREF_ICON),
    );
    let _ = ui.put(icon_rect, icon_image(icon, shell.t2(), PREF_ICON));

    let label_x = icon_rect.max.x + PREF_GAP;
    let trail_x = label_x + label_galley.size().x + PREF_GAP;
    ui.painter().galley(
        egui::pos2(label_x, rect.center().y - label_galley.size().y * 0.5),
        label_galley,
        shell.t1(),
    );
    match trail_galley {
        Some(galley) => {
            ui.painter().galley(
                egui::pos2(trail_x, rect.center().y - galley.size().y * 0.5),
                galley,
                shell.t3(),
            );
        }
        None => {
            let chevron_rect = egui::Rect::from_center_size(
                egui::pos2(trail_x + PREF_ICON * 0.5, rect.center().y),
                egui::vec2(PREF_ICON, PREF_ICON),
            );
            let _ = ui.put(
                chevron_rect,
                egui::Image::new(ICON_CHEVRON_DOWN)
                    .tint(shell.t3())
                    .fit_to_exact_size(egui::vec2(PREF_ICON, PREF_ICON)),
            );
        }
    }

    // 交互矩形**最后注册**：图标 `put` 进来的 hover 区不会挡住整块点击。
    let resp = ui.allocate_rect(rect, egui::Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// 日夜模式控件：点击只写 [`UiShellConfig::theme`]；真正投影到 egui
/// 由 [`crate::ui_shell`] 的 `apply_ui_theme` 在 `PreUpdate` 统一完成。
#[allow(clippy::too_many_arguments)]
fn theme_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    icon: egui::ImageSource<'static>,
    label: &str,
    hint: I18nKey,
    cfg: &mut UiShellConfig,
    current_loc: &CurrentLocale,
) {
    let resp = preference_control(
        ui,
        shell,
        rect,
        icon,
        label,
        PrefTrailing::Shortcut("Ctrl+J"),
    )
    .on_hover_text(crate::i18n::t(current_loc.0, hint));
    if resp.clicked() {
        cfg.theme = cfg.theme.toggled();
    }
}

/// Rebuild 控件（2026-10-19 用户裁定：位置移到语言之后、样式与日夜控件同款）。
///
/// 只排队命令，不直接碰渲染资源——`ForceRebuildTexture` 由
/// [`crate::ui_shell::apply_pending_commands`] 统一执行。
fn rebuild_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    palette: &mut crate::ui_command_palette::CommandPalette,
    current_loc: &CurrentLocale,
) {
    let resp = preference_control(
        ui,
        shell,
        rect,
        ICON_REFRESH,
        "Rebuild",
        PrefTrailing::Shortcut("F5"),
    )
    .on_hover_text(crate::i18n::t(
        current_loc.0,
        I18nKey::TopBarForceRedrawTooltip,
    ));
    if resp.clicked() {
        palette.pending = Some(CommandId::ForceRebuildTexture);
    }
}

/// 语言控件（ADR-0018）：图标 + 短标签 + chevron，点击弹出全名菜单。
///
/// 用 [`egui::Popup::from_toggle_button_response`]（memory 驱动）而不是
/// `from_response`：后者默认常开（egui 0.36 迁移坑，详见
/// [`super::project_menu`] 同款注释）。
fn language_control(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    short: &str,
    current_loc: &mut CurrentLocale,
) {
    let resp = preference_control(
        ui,
        shell,
        rect,
        ICON_LANGUAGES,
        short,
        PrefTrailing::Chevron,
    );

    egui::Popup::from_toggle_button_response(&resp)
        .id(egui::Id::new("wv_language_menu"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(140.0);
            for (label, loc) in [
                ("简体中文", crate::i18n::Locale::Zh),
                ("English", crate::i18n::Locale::En),
            ] {
                let selected = current_loc.0 == loc;
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new(label)
                            .size(shell.scale.small)
                            .color(if selected { shell.t1() } else { shell.t2() }),
                    ))
                    .clicked()
                {
                    current_loc.0 = loc;
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });
}

/// 设置入口（Figma `3:22599` `settings-2`，裸图标 14×14，悬停才有底色）。
fn settings_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    rect: egui::Rect,
    settings: &mut SettingsDialog,
    current_loc: &CurrentLocale,
) {
    if ui.rect_contains_pointer(rect) {
        ui.painter()
            .rect_filled(rect, radius::CONTROL, shell.hover_bg());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
    let _ = ui.put(
        icon_rect,
        egui::Image::new(ICON_SETTINGS)
            .tint(shell.t2())
            .fit_to_exact_size(egui::vec2(14.0, 14.0)),
    );
    let resp = ui
        .allocate_rect(rect, egui::Sense::click())
        .on_hover_text(crate::i18n::t(
            current_loc.0,
            I18nKey::TopBarSettingsTooltip,
        ));
    if resp.clicked() {
        settings.open = true;
    }
}

fn count_entities(projection: &WorldProjection) -> usize {
    let w = &projection.world;
    w.units.len() + w.characters.len() + w.situations.len() + w.events.len()
}

/// 状态栏字号（Figma #3:21001 全部 10px/16px）。
///
/// 状态栏是「仪表读数」，比令牌阶梯 MICRO(11) 再小半档——顶栏主题分段
/// （`theme_segment`）已有 10px 直写先例。
const STATUS_FONT: f32 = 10.0;
/// 状态栏行高（Figma lineHeight 16px）：中区锚定 Area 的垂直居中偏移用。
const STATUS_ROW_H: f32 = 16.0;
/// 中段（Pointer coordinates）组内间距（Figma gap 24px）。
const STATUS_GAP_CENTER: f32 = 24.0;

/// 状态栏（2026-10-16 按 Figma `Untitled` #3:21001「Status bar」重做，28px）。
///
/// 三段式（设计稿 `space-between`；中区真居中用锚定 Area，顶栏中区同款先例）：
/// - 左「Project status」：`WORLD–VIEW` + 版本 + 分隔线 + 📁 工程目录名；
/// - 中「Pointer coordinates」：光标世界坐标 `X / Y`；当前图声明了
///   `geographic_source` 且光标落在图幅内时，追加经纬度 + `EPSG:4326`
///   （世界 → UV 走 [`wv_core::resolve_frame_in`] 单一实现点，
///   UV → 经纬度走 [`wv_core::LatLonBbox::uv_to_lat_lon`]，本文件零手搓换算）；
/// - 右「Scene statistics」：对象计数 + 金色 `N selected` + 「N warnings」
///   徽标（沿用旧版，>0 才出现）+ 分隔线 + 绿点 + FPS。
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
    let h = 28.0;
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

    // ── 中区：Pointer coordinates——锚定 Area 视口底部居中 ──
    //（TopBottomPanel 做不了「剩余宽度居中」；内容行高 16，(28-16)/2 = 6px 偏移。）
    egui::Area::new(egui::Id::new("wv_status_bar_center"))
        .anchor(
            egui::Align2::CENTER_BOTTOM,
            egui::vec2(0.0, (h - STATUS_ROW_H) * 0.5),
        )
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            // Area 默认纵向布局，横排同样要包 horizontal（顶栏中区同款）。
            ui.horizontal(|ui| {
                pointer_coordinates_segment(ui, &shell, &view, &projection, &drill);
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
fn stroke_progress_segment(
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
fn project_status_segment(ui: &mut egui::Ui, shell: &UiShell, projection: &WorldProjection) {
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
/// 光标不在窗口内（`cursor_world = None`）整段不画。
fn pointer_coordinates_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &WorldProjection,
    drill: &DrillState,
) {
    let Some(cursor) = view.cursor_world else {
        return;
    };
    ui.label(
        egui::RichText::new(format!("X  {}", format_fixed2(cursor.x as f64)))
            .size(STATUS_FONT)
            .color(shell.t2())
            .font(mono(STATUS_FONT)),
    );
    ui.add_space(STATUS_GAP_CENTER);
    ui.label(
        egui::RichText::new(format!("Y  {}", format_fixed2(cursor.y as f64)))
            .size(STATUS_FONT)
            .color(shell.t2())
            .font(mono(STATUS_FONT)),
    );

    // 经纬度：当前图声明了 geographic_source 且光标在图幅内才显示——
    // 虚构地图（绝大多数）没有地理参照，硬造经纬度是骗人。
    if let Some((lat, lon)) = cursor_lat_lon(cursor, projection, drill) {
        ui.add_space(STATUS_GAP_CENTER);
        ui.label(
            egui::RichText::new(format!("{}  {}", format_lat(lat), format_lon(lon)))
                .size(STATUS_FONT)
                .color(shell.t3())
                .font(mono(STATUS_FONT)),
        );
        ui.add_space(STATUS_GAP_CENTER);
        ui.label(
            egui::RichText::new("EPSG:4326")
                .size(STATUS_FONT)
                .color(shell.t3())
                .font(mono(STATUS_FONT)),
        );
    }
}

/// 光标世界坐标 → (纬度, 经度)。
///
/// 世界 → UV 是坐标链（基线 §2.15 禁止手搓）：[`wv_core::resolve_frame_in`]
/// 还原本图世界帧 → [`wv_core::coord::MapFrame::world_to_uv`]；UV → 经纬度是
/// ADR-0007 的地理派生（[`wv_core::LatLonBbox::uv_to_lat_lon`]）。
/// 光标出图幅返回 `None`（可见性判断在这里，外推不夹取在 wv-core）。
fn cursor_lat_lon(
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
fn scene_statistics_segment(
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
fn vsep(ui: &mut egui::Ui, color: egui::Color32) {
    vsep_h(ui, color, 12.0);
}

/// 任意高度的 1px 竖分隔线（顶栏 `3:20973` = 16px，状态栏 = 12px）。
fn vsep_h(ui: &mut egui::Ui, color: egui::Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, height), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// 不占布局游标的竖分隔线：以 `center` 为中心画 1×`height`。
/// 顶栏右区按绝对矩形排版，用这个而不是 [`vsep_h`]。
fn vsep_h_at(ui: &egui::Ui, color: egui::Color32, height: f32, center: egui::Pos2) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(1.0, height));
    ui.painter().rect_filled(rect, 0.0, color);
}

/// 4×4px 圆点（Figma「Render status」#3:21017）。
fn status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(4.0, 4.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 2.0, color);
}

/// 工程目录的展示名：取路径末段（`…/guizhou-atlas.world` → `guizhou-atlas.world`）。
///
/// `file_name` 失败（路径本身就是根/空）时退回原串——显示个完整路径
/// 也好过什么都不显示。
fn project_display_name(dir: &str) -> &str {
    std::path::Path::new(dir)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(dir)
}

/// 整数千分位（en-US 逗号）：`12480` → `"12,480"`。
fn format_thousands(value: u64) -> String {
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
fn format_fixed2(value: f64) -> String {
    let scaled = (value.abs() * 100.0).round() as u64;
    format!(
        "{}{}.{:02}",
        if value.is_sign_negative() { "-" } else { "" },
        format_thousands(scaled / 100),
        scaled % 100
    )
}

/// 纬度读数：`28.034` → `"28.03° N"`，南半球出 `S`。
fn format_lat(lat: f64) -> String {
    format!("{:.2}° {}", lat.abs(), if lat >= 0.0 { "N" } else { "S" })
}

/// 经度读数：`116.4` → `"116.40° E"`，西半球出 `W`。
fn format_lon(lon: f64) -> String {
    format!("{:.2}° {}", lon.abs(), if lon >= 0.0 { "E" } else { "W" })
}

#[cfg(test)]
mod status_bar_format_tests {
    use super::{format_fixed2, format_lat, format_lon, format_thousands, project_display_name};

    #[test]
    fn thousands_groups_every_three_digits() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(480), "480");
        assert_eq!(format_thousands(12_480), "12,480");
        assert_eq!(format_thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn fixed2_pads_two_decimals_and_groups() {
        // 设计稿示例：X 1,284.50。
        assert_eq!(format_fixed2(1284.5), "1,284.50");
        assert_eq!(format_fixed2(0.0), "0.00");
        assert_eq!(format_fixed2(60.0), "60.00");
        // 0.125 在 f64 里精确，×100 = 12.5，round 半值远离零 → 13。
        assert_eq!(format_fixed2(0.125), "0.13");
    }

    #[test]
    fn fixed2_keeps_sign_for_negative_coordinates() {
        // 真实地理图（西雅图）x 常为负。
        assert_eq!(format_fixed2(-122.5), "-122.50");
        assert_eq!(format_fixed2(-1_284.25), "-1,284.25");
    }

    #[test]
    fn lat_lon_hemisphere_suffixes_follow_sign() {
        assert_eq!(format_lat(28.034), "28.03° N");
        assert_eq!(format_lat(-33.9), "33.90° S");
        assert_eq!(format_lon(116.4), "116.40° E");
        assert_eq!(format_lon(-122.5), "122.50° W");
    }

    #[test]
    fn project_display_name_takes_the_last_segment() {
        assert_eq!(
            project_display_name(r"D:\projects\guizhou-atlas.world"),
            "guizhou-atlas.world"
        );
        assert_eq!(project_display_name("foo/atlas"), "atlas");
        // 无可拆分段时退回原串（显示完整路径好过空白）。
        assert_eq!(project_display_name(""), "");
    }
}

/// 验证警告数（v0.3：状态条右侧徽标需要）。
///
/// wv-app 当前没有现成的 Resource 暴露「总验证警告数」——它由
/// `validation_actions` 模块现算。为不引入新依赖，本 Resource 由
/// `app_bootstrap` 在 Update 链上每帧 `insert_resource(ValidationCount{warnings:...})`。
/// 未来 M3 真接 `validation` Resource 时可直接替换。
#[derive(Default, Resource, Debug, Clone, Copy)]
pub struct ValidationCount {
    pub warnings: usize,
}

/// 构造一个「视口根 `Ui`」。
pub(crate) fn root_ui(ctx: &egui::Context) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        "wv_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    )
}
