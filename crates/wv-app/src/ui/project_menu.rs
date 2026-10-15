//! 项目管理菜单（v0.2 §A）+ 面包屑（v0.2 §B.3）。
//!
//! ## 职责
//! - `ProjectMenuState` —— 跨帧高亮项（egui `Popup::Memory` 只存开关，不存选中行）。
//! - `project_menu_inner` —— 工程名下拉菜单；开合走 egui，命令通过 `pending` 排队。
//! - `menu_section` / `menu_row` —— 分组小标题与菜单行的渲染 helper。
//! - `breadcrumb` —— `世界 › 遵义城 › 府衙`，浮在地图区左上角。
//!
//! ## 边界（不做什么）
//! - **不**分发命令 —— `state.pending` 交给 [`crate::ui_shell::apply_pending_commands`]。
//! - **不**重置布局 —— 见 [`crate::ui_layout`]。
//!
//! ## 主要导出
//! - `ProjectMenuState` —— Bevy 资源。
//! - `project_menu_inner` —— 由 [`super::top_bar::top_bar`] 调用（`pub(crate)` 内部桥）。
//! - `breadcrumb` —— Bevy 系统。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::state::{DrillState, Panel, PanelState, RecentProject, WorldProjection};
use crate::theme::ui_theme::{mono, proportional, UiShellConfig};
use crate::theme::ui_tokens::{font_size, radius, space};

use super::shell::UiShell;

/// 项目管理菜单的交互状态。
///
/// **为什么单独一个资源**：菜单要支持 `↑ ↓ ↵`（v0.2 §A「焦点落当前项、↑↓ 选择、
/// ↵ 执行」），需要跨帧记住高亮项；`egui::memory` 只存 popup 的开关，不存选中行。
#[derive(Resource, Debug, Default)]
pub struct ProjectMenuState {
    /// 高亮行索引（对**命令行**编号，跳过分隔线与复选框）。
    pub selected: usize,
    /// 待执行的命令。菜单绘制函数拿不到 [`CommandPalette`]（不穿进本函数），
    /// 所以先落在这里，由 [`crate::ui_shell::apply_pending_commands`] 每帧与
    /// `CommandPalette::pending` 汇合消费——**唯一分发器**。
    pub pending: Option<crate::ui_command_palette::CommandId>,
}

/// 菜单里的一行。
///
/// `section` 让「分组标题 + 分隔线」的绘制**跟着行数据走**，而不是靠绘制方数
/// 「第几行该换组」——后者在增删一条命令时会悄悄把分隔线留在错误的位置，
/// 而且空的分组（比如没有最近工程时）会留下一个孤零零的标题。
struct MenuRow {
    section: &'static str,
    label: String,
    keys: Option<&'static str>,
    id: crate::ui_command_palette::CommandId,
}

/// 工程名下拉菜单（v0.2 §A）。
///
/// 规格明确要求「**不用固定菜单栏**」——那会破坏顶栏的三段式密度（§3.2）。
/// 于是入口就是身份段的工程名本身：点它展开，焦点落在当前项上。
///
/// 菜单内容顺序（与原型一致）：最近工程 → 工程操作 → 面板显隐。
/// **保存 / 撤销不进这个菜单**：它们已有独立位置（§A）。
///
/// 开合状态存放在 egui memory（`Popup` 的 `OpenKind::Memory`）里，**只在点击工程名时打开**；
/// 它不是一个「常驻浮层」——常驻会盖住左上角的面包屑（当前图名）。
pub(crate) fn project_menu_inner(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    recents: &[RecentProject],
    panels: &mut PanelState,
    state: &mut ProjectMenuState,
    loc: crate::i18n::Locale,
) {
    let name = projection
        .world
        .meta
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| {
            crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuUnnamedProject).to_owned()
        });
    let dirty = projection.dirty;
    let popup_id = egui::Id::new("wv_project_menu");
    let opened = egui::Popup::is_id_open(ui.ctx(), popup_id);

    // ── 触发器 ──
    let text = format!("◈ {name}{}", if dirty { " ●" } else { "" });
    let galley = ui
        .painter()
        .layout_no_wrap(text, proportional(shell.scale.base), shell.t1());
    let size = egui::vec2(galley.size().x + space::SM * 2.0, shell.hit);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    // 自绘而不是用 `Button`：菜单在顶栏里的 hover 底必须是 `bg.hover` 令牌（§4.2）。
    if resp.hovered() || opened {
        ui.painter()
            .rect_filled(rect, radius::CONTROL, shell.hover_bg());
    }
    ui.painter().galley(
        egui::pos2(
            rect.min.x + space::SM,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        shell.t1(),
    );
    if resp.clicked() {
        state.selected = 0;
        // **不要**在这里再 toggle 一次：开合状态由下方 `from_toggle_button_response`
        // 负责（它在点击帧写入 `SetOpenCommand::Toggle`）。两处都 toggle 会互相抵消，
        // 菜单永远打不开——见 `Popup::show` 的 `OpenKind::Memory` 分支。
    }

    // ── 菜单内容 ──
    // 命令行清单先建好：键盘导航要知道一共几行，绘制时才知道哪行高亮。
    use crate::ui_command_palette::CommandId;
    let mut rows: Vec<MenuRow> = Vec::new();
    for (i, p) in recents.iter().enumerate() {
        rows.push(MenuRow {
            section: crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuRecentSection),
            label: format!(
                "{}{}",
                crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuRecentOpenPrefix),
                p.name
            ),
            keys: None,
            id: CommandId::OpenRecentProject(i as u8),
        });
    }
    let lbls: [&str; 7] = [
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuNewProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuNewMap),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuOpenProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuSaveProjectAs),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuExportProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuCloseProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuProjectSettings),
    ];
    let ids: [CommandId; 7] = [
        CommandId::NewProject,
        CommandId::NewMap,
        CommandId::OpenProject,
        CommandId::SaveProjectAs,
        CommandId::ExportProject,
        CommandId::CloseProject,
        CommandId::ProjectSettings,
    ];
    let sect: &'static str = crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuProjectSection);
    let keys_arr: [Option<&str>; 7] = [
        Some("Ctrl+Shift+N"),
        None,
        Some("Ctrl+O"),
        Some("Ctrl+Shift+S"),
        Some("Ctrl+E"),
        Some("Ctrl+W"),
        None,
    ];
    for (idx, label) in lbls.into_iter().enumerate() {
        rows.push(MenuRow {
            section: sect,
            label: label.to_owned(),
            keys: keys_arr[idx],
            id: ids[idx],
        });
    }

    // 键盘先于绘制处理：绘制时才知道 hover，键盘不该依赖它。
    let (down, up, enter, esc) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if opened {
        let last = rows.len().saturating_sub(1);
        if down {
            state.selected = (state.selected + 1).min(last);
        }
        if up {
            state.selected = state.selected.saturating_sub(1);
        }
        if enter {
            if let Some(r) = rows.get(state.selected) {
                state.pending = Some(r.id);
            }
            egui::Popup::close_all(ui.ctx());
        }
        if esc {
            egui::Popup::close_all(ui.ctx());
        }
    }
    state.selected = state.selected.min(rows.len().saturating_sub(1));

    let mut clicked_id: Option<CommandId> = None;
    // **必须**用 `from_toggle_button_response`（= `open_memory(Toggle)`）而不是 `from_response`：
    // egui 0.36 的 `Popup::from_response` 默认 `OpenKind::Open`——文档原话「The popup will be
    // always open」，于是菜单会在启动第一帧就弹出来、再也关不掉（Esc 只影响 `Memory` 分支）。
    // 旧 API `popup_below_widget(popup_id, …)` 是 memory 驱动的，0.36 换 API 时丢了这层语义，
    // 属 ADR-0011 §5 记录的那类「迁移静默回归」。
    egui::Popup::from_toggle_button_response(&resp)
        .id(popup_id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_min_width(268.0);
            // 分组标题与分隔线跟着行数据走：空分组不会留下孤立标题，
            // 增删命令也不需要同步改这里的计数（见 [`MenuRow::section`]）。
            let mut section = "";
            for (i, r) in rows.iter().enumerate() {
                if r.section != section {
                    if !section.is_empty() {
                        ui.separator();
                    }
                    menu_section(ui, shell, r.section);
                    section = r.section;
                }
                let highlighted = state.selected == i;
                if menu_row(ui, shell, &r.label, r.keys, highlighted, true).clicked() {
                    clicked_id = Some(r.id);
                }
            }
            // 面板显隐 + 重置位置：v0.1 的「窗口菜单」内容保留在这里，
            // 它是三级可发现性的第 3 级，不能删（§6.2）。
            ui.separator();
            menu_section(
                ui,
                shell,
                crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuPanelsSection),
            );
            for p in Panel::ALL {
                let mut on = panels.is_visible(p);
                if ui.checkbox(&mut on, p.title(loc)).changed() {
                    panels.set_visible(p, on);
                }
            }
            ui.separator();
            if ui
                .button(crate::i18n::t(
                    loc,
                    crate::i18n::I18nKey::ProjectMenuResetPanels,
                ))
                .clicked()
            {
                panels.reset_requested = true;
                egui::Popup::close_all(ui.ctx());
            }
        });
    if let Some(id) = clicked_id {
        state.pending = Some(id);
        egui::Popup::close_all(ui.ctx());
    }
}

/// 菜单里的分组小标题（11px、`text.3`、字距放宽）。
fn menu_section(ui: &mut egui::Ui, shell: &UiShell, title: &str) {
    ui.add_space(space::XS);
    ui.label(
        egui::RichText::new(title)
            .size(font_size::MICRO)
            .color(shell.t3()),
    );
    ui.add_space(space::XS * 0.5);
}

/// 菜单里的一行（§4.2）。
///
/// `highlighted` 是键盘高亮（↑↓），与 hover 分开：键盘用户没有指针，
/// 只靠 hover 的话完全看不出焦点在哪（§7 非颜色依赖的同类问题）。
fn menu_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    keys: Option<&str>,
    highlighted: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), shell.row),
        egui::Sense::click(),
    );
    let fg = if !enabled {
        shell.t3()
    } else if highlighted || resp.hovered() {
        shell.t1()
    } else {
        shell.t2()
    };
    if highlighted || resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius::BADGE, shell.hover_bg());
    }
    ui.painter().text(
        egui::pos2(rect.min.x + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(shell.scale.small),
        fg,
    );
    if let Some(k) = keys {
        ui.painter().text(
            egui::pos2(rect.max.x - space::SM, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            k,
            mono(shell.scale.micro),
            shell.t3(),
        );
    }
    resp
}

// ---------------------------------------------------------------------------
// 面包屑（v0.2 §B.3）
// ---------------------------------------------------------------------------

/// 面包屑：`世界 › 遵义城 › 府衙`。
///
/// 浮在地图区左上角（避顶栏与时间轴），**不推挤地图坐标系**（D1）。层级状态是
/// [`DrillState`] 的 id 链，名字现查 `world.maps`——那才是唯一真源。
///
/// 当前级 700 字重 / 14px / `text.1` 且**不可点**；上级 400 字重 / 13px /
/// 新增令牌 `crumb.text_3`；分隔符 `›`(U+203A)。沉浸模式降 60% 透明度但**保留可见**
/// （§B.3：沉浸只隐面板与演示控制，导航痕迹必须留着，否则用户不知道自己身在何处）。
///
/// **偏离记录**：v0.2 把 LOD 标签（`四渡赤水 · 正在发生`）画在地图上，但那需要
/// Bevy `Text2d` 的字体资产，而本工程不打包字体（§2.3 用系统字体栈，且只有 egui 侧
/// 接了 `fonts.rs`）。故 LOD 文案并入面包屑行尾——信息不丢，代价是位置不同。
pub fn breadcrumb(
    mut contexts: EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    projection: Res<WorldProjection>,
    mut drill: ResMut<DrillState>,
    panels: Res<PanelState>,
    current_loc: Res<crate::i18n::CurrentLocale>,
) {
    let shell = UiShell::new(&shell_cfg);
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if drill.current().is_none() {
        return;
    }
    let alpha = if panels.immersive { 0.4 } else { 1.0 };
    let top = shell_cfg.density.top_bar_height() + space::MD;

    // 先把要画的内容收集出来（借 `world` 的不可变借用不能跨进闭包）。
    let mut segments: Vec<(String, bool)> = Vec::new();
    let trail = drill.trail().to_vec();
    for (i, id) in trail.iter().enumerate() {
        let name = drill
            .node(&projection.world, id)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| id.as_str().to_owned());
        segments.push((name, i + 1 == trail.len()));
    }
    let lod = drill
        .current_node(&projection.world)
        .map(|n| {
            format!(
                "{} · {}",
                n.name,
                crate::state::map_level_label(n.level, current_loc.0)
            )
        })
        .unwrap_or_default();

    let mut jump: Option<usize> = None;
    egui::Area::new(egui::Id::new("wv_breadcrumb"))
        .fixed_pos(egui::pos2(space::MD, top))
        .order(egui::Order::Middle)
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(shell.float_fill().gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0_f32, shell.border_subtle()))
                .inner_margin(crate::theme::ui_theme::margin_xy(space::SM, space::XS))
                .corner_radius(radius::CONTROL)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, (name, is_current)) in segments.iter().enumerate() {
                            if i > 0 {
                                ui.label(
                                    egui::RichText::new("›")
                                        .size(shell.scale.small)
                                        .color(shell.t3().gamma_multiply(alpha)),
                                );
                            }
                            if *is_current {
                                // 当前级：不可点（§B.3）。
                                ui.label(
                                    egui::RichText::new(name)
                                        .size(font_size::INPUT)
                                        .strong()
                                        .color(shell.t1().gamma_multiply(alpha)),
                                );
                            } else {
                                let resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(name)
                                            .size(shell.scale.base)
                                            .color(shell.crumb_text_3().gamma_multiply(alpha)),
                                    )
                                    .sense(egui::Sense::click()),
                                );
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if resp.clicked() {
                                    jump = Some(i);
                                }
                            }
                        }
                        ui.add_space(space::SM);
                        ui.label(
                            egui::RichText::new(&lod)
                                .size(shell.scale.micro)
                                .color(shell.t3().gamma_multiply(alpha)),
                        );
                    });
                });
        });

    if let Some(i) = jump {
        drill.jump_to(i);
    }
}
