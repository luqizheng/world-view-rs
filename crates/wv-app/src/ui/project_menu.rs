//! 项目管理菜单（v0.2 §A）。
//!
//! ## 职责
//! - `ProjectMenuState` —— 跨帧高亮项（egui `Popup::Memory` 只存开关，不存选中行）。
//! - `project_menu_inner` —— 工程名下拉菜单；开合走 egui，命令通过 `pending` 排队。
//! - `menu_section` / `menu_row` —— 分组小标题与菜单行的渲染 helper。
//!
//! ## 边界（不做什么）
//! - **不**分发命令 —— `state.pending` 交给 [`crate::ui_shell::apply_pending_commands`]。
//! - **不**重置布局 —— 见 [`crate::ui_layout`]。
//!
//! ## 主要导出
//! - `ProjectMenuState` —— Bevy 资源。
//! - `project_menu_inner` —— 由 [`super::top_bar::top_bar`] 调用（`pub(crate)` 内部桥）。
//!
//! > **追记（2026-10-25 用户裁定）**：原挂在本模块的 `breadcrumb` 浮层
//! > （v0.2 §B.3，地图区左上角「世界 › …」）已删除——顶栏身份区已有同内容
//! > 面包屑，浮层重复且挡地图。点击跳级能力仍由地图树面板承担。

#![cfg(feature = "app")]

use bevy::prelude::*;
use bevy_egui::egui;

use crate::state::{RecentProject, WorldProjection};
use crate::theme::ui_theme::{mono, proportional};
use crate::theme::ui_tokens::{font_size, radius, space};

use super::shell::UiShell;
use super::widgets::{ICON_CHEVRON_DOWN, ICON_MAP};

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
/// 菜单内容顺序（与原型一致）：最近工程 → 工程操作 → 面板。
/// **保存 / 撤销不进这个菜单**：它们已有独立位置（§A）。
/// ~~**面板显隐 / 重置位置也不在这里**（2026-10-19 用户裁定）~~——
/// **已被 ADR-0087 推翻**（2026-10-29，UX-009 / round-2 审计修正版）：
/// 设置对话框三步入口 + 命令面板对新用户不可见，构成「图层面板（P5）入口
/// 最难找」的信息架构缺陷；面板显隐以 ✓ 前缀行回填本菜单（重置位置仍在
/// 设置与命令面板）。见 `docs/adr/ADR-0087-project-menu-panels-section.md`。
///
/// 开合状态存放在 egui memory（`Popup` 的 `OpenKind::Memory`）里，**只在点击工程名时打开**；
/// 它不是一个「常驻浮层」——常驻会盖住左上角的面包屑（当前图名）。
pub(crate) fn project_menu_inner(
    ui: &mut egui::Ui,
    shell: &UiShell,
    projection: &WorldProjection,
    recents: &[RecentProject],
    state: &mut ProjectMenuState,
    panels: &crate::state::PanelState,
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

    // ── 触发器（web `.project-button`：24×24 accent 蓝 logo 方块 + 粗体工程名
    //    + 下翻 chevron；v0.3 前 的 `◈ 名字 ●` 文本形态已退役——脏态徽标在
    //    顶栏右侧，不在按钮里）──
    let _ = dirty; // 脏态展示已上移到顶栏右侧徽标（top_bar::readout_segment）
    let logo_size = 24.0;
    let chevron_w = 12.0_f32;
    let galley =
        ui.painter()
            .layout_no_wrap(name.clone(), proportional(shell.scale.base), shell.t1());
    // 左内边距 3 + logo 24 + gap 6 + 名字 + gap 4 + chevron 12 + 右内边距 8
    let total_w = 3.0 + logo_size + 6.0 + galley.size().x + 4.0 + chevron_w + space::SM;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(total_w, 30.0), egui::Sense::click());
    // 自绘而不是用 `Button`：菜单在顶栏里的 hover 底必须是 `bg.hover` 令牌（§4.2）。
    {
        let painter = ui.painter();
        if resp.hovered() || opened {
            painter.rect_filled(rect, radius::CONTROL, shell.hover_bg());
        }
        // logo：accent 蓝底圆角方块（web `.logo`），白色 map 图标由下方 ui.put 画
        let logo_rect = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + 3.0, rect.center().y - logo_size * 0.5),
            egui::vec2(logo_size, logo_size),
        );
        painter.rect_filled(logo_rect, 6.0, shell.accent());
        // 工程名（web 加粗）
        painter.galley(
            egui::pos2(
                logo_rect.max.x + 6.0,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            shell.t1(),
        );
    }
    // logo 内的白色 map 图标（mut 借用须在 painter 作用域之后）
    let logo_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 3.0, rect.center().y - logo_size * 0.5),
        egui::vec2(logo_size, logo_size),
    );
    let _ = ui.put(
        logo_rect.shrink(4.0),
        egui::Image::new(ICON_MAP)
            .tint(egui::Color32::WHITE)
            .fit_to_exact_size(egui::vec2(16.0, 16.0)),
    );
    // 下翻 chevron（web：chevron 旋转 90° → 直接用 chevron-down 资源）
    let chevron_rect = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - space::SM - chevron_w * 0.5, rect.center().y),
        egui::vec2(chevron_w, chevron_w),
    );
    let _ = ui.put(
        chevron_rect,
        egui::Image::new(ICON_CHEVRON_DOWN)
            .tint(shell.t3())
            .fit_to_exact_size(egui::vec2(chevron_w, chevron_w)),
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
    let lbls: [&str; 8] = [
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuNewProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuNewMap),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuMapProperties),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuOpenProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuSaveProjectAs),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuExportProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuCloseProject),
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuProjectSettings),
    ];
    let ids: [CommandId; 8] = [
        CommandId::NewProject,
        CommandId::NewMap,
        CommandId::MapProperties,
        CommandId::OpenProject,
        CommandId::SaveProjectAs,
        CommandId::ExportProject,
        CommandId::CloseProject,
        CommandId::ProjectSettings,
    ];
    let sect: &'static str = crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuProjectSection);
    let keys_arr: [Option<&str>; 8] = [
        Some("Ctrl+Shift+N"),
        None,
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

    // ── 面板分组（ADR-0087 / UX-009 round-2 修正版）──
    // 与命令面板「面板」组同一真源（`Panel::ALL` + `panel_toggle_label_key`），
    // 点击排 `TogglePanel` 走同一分发；可见性以 ✓ 前缀行内标注。重置位置
    // 不回填（仍走设置对话框 / 命令面板 `ResetPanelLayout`）。
    let panel_sect: &'static str =
        crate::i18n::t(loc, crate::i18n::I18nKey::ProjectMenuPanelsSection);
    for p in crate::state::Panel::ALL {
        let visible = panels.is_visible(p);
        let label_key = crate::ui_command_palette::panel_toggle_label_key(p);
        let label = if visible {
            format!("✓ {}", crate::i18n::t(loc, label_key))
        } else {
            crate::i18n::t(loc, label_key).to_string()
        };
        rows.push(MenuRow {
            section: panel_sect,
            label,
            keys: None,
            id: CommandId::TogglePanel(p),
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
