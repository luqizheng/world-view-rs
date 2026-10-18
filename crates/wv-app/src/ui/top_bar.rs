//! 顶栏（38px v0.3 · web `.topbar` 对齐）+ 状态栏（28px · Figma #3:21001）。
//!
//! ## 职责
//! - `top_bar` —— web `grid: 1fr auto 1fr` 三区：左 = 项目按钮（蓝 logo +
//!   粗体名 + 下翻箭头，见 [`super::project_menu`]）+ 面包屑；**中 = Browse /
//!   Edit 分段 + 地图主题分段 + Rebuild 按钮（独立 Area 锚视口顶部居中）**；
//!   右 = zoom / entities / 脏态徽标 / ⌘K 按钮 / 语言选择器 / **日夜模式按钮**。
//! - `status_bar`（2026-10-16 按 Figma `Untitled` #3:21001 重做）——
//!   左 = `WORLD–VIEW` + 版本 + 工程目录名；中 = 光标世界坐标 X/Y +
//!   经纬度（真实地理图才有）+ EPSG；右 = 对象计数 + 选中数 + 警告徽标 +
//!   FPS。
//!
//! ## web 对齐要点（`index.css` / `App.tsx`，v0.3 校准）
//! - 高度 38px（`.topbar`）；底色 `bg_app` 96% + 底边框 + `0 4px 20px` 投影。
//! - 分段控件（`.segmented`）：容器 `bg_app` 底 + border + 圆角 7、内边距 2px；
//!   选中段 `bg_active` 底 + `text_1`，**内嵌**于容器而非顶满。
//! - 主题分段按 web 顺序与文案：`Dusk Old Paper` / `Sunlit Ancient Atlas`
//!   （10px 字）；数据层仍映射 [`crate::theme::palette::MapPalette`]。
//! - `Rebuild` / `⌘K` 是**图标+文字**的 raised 按钮（web `.button`），非裸图标。
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
use crate::theme::palette::MapPalette;
use crate::theme::ui_theme::{color32, color32_alpha, mono, UiShellConfig};

use super::project_menu::{project_menu_inner, ProjectMenuState};
use super::shell::UiShell;
use super::widgets::{
    badge, icon_image, BadgeKind, ICON_CHEVRON_RIGHT, ICON_COMMAND, ICON_FOLDER_OPEN, ICON_MOON,
    ICON_REFRESH, ICON_SUN,
};
use crate::theme::ui_tokens::{font_size, radius, space};
use crate::ui::widgets::segmented_control::segmented;
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
    mut current_loc: ResMut<CurrentLocale>,
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
                    &mut panels,
                    &mut menu,
                    &current_loc,
                );
                // ── 右区：zoom / entities / 脏态徽标 / ⌘K（web .top-right，
                //    L→R 次序 zoom→entities→badge→⌘K，故 RTL 添加序相反）──
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    readout_segment(
                        ui,
                        &shell,
                        &view,
                        &projection,
                        &mut current_loc,
                        &mut shell_cfg,
                    );
                });
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
                browse_edit_segment(ui, &shell, &mut view);
                ui.add_space(6.0);
                theme_segment(ui, &shell, &mut view);
                ui.add_space(6.0);
                rebuild_button(ui, &shell, &mut palette);
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
    panels: &mut crate::state::PanelState,
    menu: &mut ProjectMenuState,
    current_loc: &CurrentLocale,
) {
    // 项目按钮（logo + 粗体名 + chevron，见 project_menu）+ 组间距。
    project_menu_inner(ui, shell, projection, recents, panels, menu, current_loc.0);
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

/// Browse / Edit / Faction 三段：`Browse` → `ViewMode::VectorFlat`（默认浏览）、
/// `Edit` → `ViewMode::Schematic`（色块格子）、`Faction` → `ViewMode::Faction`
/// （ADR-0027 D6 势力图）。
///
/// 数据层 `view.mode` 仍按原枚举流转；命令面板 label 来自 `ViewMode::label()`
/// （中文），不受影响（v0.3 用户拍板：仅顶栏用英文）。
fn browse_edit_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    let labels = ["Browse", "Edit", "Faction"];
    let modes = [ViewMode::VectorFlat, ViewMode::Schematic, ViewMode::Faction];
    let active = modes.iter().position(|m| *m == view.mode).unwrap_or(0);
    segmented(
        ui,
        shell,
        "wv_browse_edit",
        120.0,
        &labels,
        active,
        font_size::SMALL,
        |i| {
            view.mode = modes[i];
        },
    );
}

/// 地图主题分段（web `.theme-toggle`：`Dusk Old Paper / Sunlit Ancient Atlas`，
/// 10px 字，Dusk 在前）。数据层仍映射 [`MapPalette`]（Daylight/Dusk）。
fn theme_segment(ui: &mut egui::Ui, shell: &UiShell, view: &mut ViewState) {
    let order = [MapPalette::Dusk, MapPalette::Daylight];
    let labels = ["Dusk Old Paper", "Sunlit Ancient Atlas"];
    let active = order
        .iter()
        .position(|p| *p == view.map_palette)
        .unwrap_or(0);
    segmented(
        ui,
        shell,
        "wv_theme_seg",
        96.0,
        &labels,
        active,
        10.0,
        |i| {
            view.map_palette = order[i];
        },
    );
}

/// Rebuild 按钮（web：`<Button><Icon redo/> Rebuild</Button>`——raised 底 +
/// border 的标准按钮，非裸图标）。
///
/// **走 [`icon_image`] helper 而不是 [`crate::ui::widgets::button`] 工厂**：因为
/// 本按钮 stroke 是 `border_subtle`（与工厂默认 `border_strong` 不一致——顶栏设计稿
/// 用浅边，与对话/CTA 的强边区分）。工厂升级时若统一改 stroke，可以并入。
fn rebuild_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    palette: &mut crate::ui_command_palette::CommandPalette,
) {
    let resp = ui.add(
        egui::Button::image_and_text(
            icon_image(ICON_REFRESH, shell.t2(), shell.hit - 8.0),
            egui::RichText::new("Rebuild").size(shell.scale.small),
        )
        .fill(shell.raised_bg())
        .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
        .corner_radius(radius::CONTROL)
        .min_size(egui::vec2(0.0, shell.hit)),
    );
    if resp.clicked() {
        palette.pending = Some(CommandId::ForceRebuildTexture);
    }
}

fn readout_segment(
    ui: &mut egui::Ui,
    shell: &UiShell,
    view: &ViewState,
    projection: &WorldProjection,
    current_loc: &mut CurrentLocale,
    shell_cfg: &mut UiShellConfig,
) {
    // 右至左排布；web `.top-right` 从左到右是 zoom / entities / 脏态徽标 / ⌘K，
    // 故本段添加顺序：日夜 → 语言 → ⌘K → 脏态 → entities → zoom，间距 12px（web gap）。

    // 日夜模式按钮（2026-10-18）：图标表示**点击后要去**的模式——暗色时显示太阳，
    // 亮色时显示月亮。只切界面外壳（`UiShellConfig`），不碰 `view.map_palette`
    // 的地图配色（D2：世界色归世界、界面色归界面）。
    theme_toggle_button(ui, shell, shell_cfg, current_loc);
    ui.add_space(space::LG);

    // 语言选择器（ADR-0018）：最右端。按钮显示单字（Zh=「简」/ En=「EN」），
    // 点击弹出全名菜单（简体中文 / English）。`menu_button` 即
    // 「Button 点击开菜单」——不是 select 框，符合 2026-10-15 用户裁定。
    {
        let short = match current_loc.0 {
            crate::i18n::Locale::Zh => "简",
            crate::i18n::Locale::En => "EN",
        };
        ui.menu_button(
            egui::RichText::new(short)
                .size(shell.scale.small)
                .color(shell.t2()),
            |ui| {
                let mut pick = |ui: &mut egui::Ui, label: &str, loc: crate::i18n::Locale| {
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
                        ui.close();
                    }
                };
                pick(ui, "简体中文", crate::i18n::Locale::Zh);
                pick(ui, "English", crate::i18n::Locale::En);
            },
        );
        ui.add_space(space::LG);
    }

    // ⌘K 按钮（web：command 图标 + K 的 raised 按钮；tooltip 提示快捷键）
    let cmdk = ui.add(
        egui::Button::image_and_text(
            icon_image(ICON_COMMAND, shell.t2(), shell.hit - 8.0),
            egui::RichText::new("K").size(shell.scale.small),
        )
        .fill(shell.raised_bg())
        .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
        .corner_radius(radius::CONTROL)
        .min_size(egui::vec2(0.0, shell.hit)),
    );
    let _ = cmdk.on_hover_text("命令面板（Ctrl+K / ⌘K）");
    ui.add_space(space::LG);

    // 脏态徽标（web `.badge.warn`：橙底 + 圆点；已保存时不画，状态栏负责）。
    if projection.dirty {
        badge(ui, shell, BadgeKind::Warn, "● Unsaved changes");
        ui.add_space(space::LG);
    }

    // 实体计数（"128 entities"）
    let entity_count = count_entities(projection);
    ui.label(
        egui::RichText::new(format!("{entity_count} entities"))
            .size(shell.scale.small)
            .color(shell.t2())
            .font(mono(shell.scale.small)),
    );
    ui.add_space(space::LG);

    // 缩放百分比（"86%"）
    let pct = (view.scale * 100.0).round() as i32;
    ui.label(
        egui::RichText::new(format!("{pct}%"))
            .size(shell.scale.small)
            .color(shell.t2())
            .font(mono(shell.scale.small)),
    );
}

/// 日夜模式切换按钮（图标表示**点击后**的模式）。
///
/// 放在顶栏右侧读数段（与语言选择器相邻）：这是应用级外观开关，不是地图主题——
/// 地图主题仍是中区的 `Dusk Old Paper / Sunlit Ancient Atlas` 分段，两者互不影响。
/// 点击只写 [`UiShellConfig::theme`]；真正投影到 egui 由
/// [`crate::ui_shell`] 的 `apply_ui_theme` 在 `PreUpdate` 统一完成。
fn theme_toggle_button(
    ui: &mut egui::Ui,
    shell: &UiShell,
    cfg: &mut UiShellConfig,
    current_loc: &CurrentLocale,
) {
    let (icon, hint) = if cfg.theme.is_dark() {
        (ICON_SUN, I18nKey::TopBarSwitchToDay)
    } else {
        (ICON_MOON, I18nKey::TopBarSwitchToNight)
    };
    let resp = ui
        .add(
            egui::Button::image(icon_image(icon, shell.t2(), shell.hit - 8.0))
                .min_size(egui::vec2(shell.hit, shell.hit))
                .fill(shell.raised_bg())
                .stroke(egui::Stroke::new(1.0, color32(shell.palette.border_subtle)))
                .corner_radius(radius::CONTROL),
        )
        .on_hover_text(crate::i18n::t_key(current_loc, hint));
    if resp.clicked() {
        cfg.theme = cfg.theme.toggled();
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
pub fn status_bar(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    view: Res<ViewState>,
    projection: Res<WorldProjection>,
    validation_count: Res<ValidationCount>,
    drill: Res<DrillState>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
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

/// 1×12px 竖分隔线（Figma「Separator」#3:21005）。
fn vsep(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 12.0), egui::Sense::hover());
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
