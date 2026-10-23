//! 图层面板控件族（Figma `3:20892` Layers，2026-10-18 按设计稿逐项对位）。
//!
//! ## 职责
//! - [`layer_panel`] —— 完整面板体：动作行 + 图层列表 + 分隔线 + 不透明度行，
//!   全部尺寸/色值取自 Figma `3:20892` 的采样（见下方常量注释）。
//! - [`layer_list_row`] —— 单行：可见性眼睛 / 类型图标 / 名称 / 计数 / 锁定。
//! - [`opacity_row`] —— 4 px 轨道 + 10 px 圆钮的紧凑不透明度行。
//!
//! ## 边界
//! - **不**读世界数据、**不**做 i18n：标签文本、图标、计数值全由调用方给。
//! - **不**管浮窗外壳：由 [`crate::widgets::FloatWindow`] 装本模块的返回值。
//!
//! ## 尺寸对位（@1x）
//!
//! | 件 | Figma | 本模块 |
//! |---|---|---|
//! | 面板头高 | 30 | `UiShell::head_h`（密度） |
//! | 行高 / 圆角 / 左右内边距 | 28 / 4 / 8 | [`ROW_H`] / `radius::BADGE` / [`ROW_PAD_X`] |
//! | 行内 gap | 8 | [`ROW_GAP`] |
//! | 行图标 | 12×12 | [`ROW_ICON`] |
//! | 列表行距 | 2 | `space::XS / 2` |
//! | 内容块间距 | 12 | [`BLOCK_GAP`] |
//! | 不透明度轨道 / 圆钮 | 4 / 10（accent 2px 描边） | [`TRACK_H`] / [`KNOB`] |

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::ui_theme::{color32, proportional};
use crate::ui_tokens::{font_size, radius};

use super::primitives::{icon_image, row_rect, ICON_EYE, ICON_EYE_OFF, ICON_LOCK};

/// 行高（Figma `Map layer` 28）。
const ROW_H: f32 = 28.0;
/// 行左右内边距（Figma 8）。
const ROW_PAD_X: f32 = 8.0;
/// 行内子项间距（Figma itemSpacing 8）。
const ROW_GAP: f32 = 8.0;
/// 行内图标边长（Figma 12）。
const ROW_ICON: f32 = 12.0;
/// 内容块间距（Figma Panel content `itemSpacing: 12`）。
const BLOCK_GAP: f32 = 12.0;
/// 动作行高（Figma `Layer actions` 20）。
const ACTIONS_H: f32 = 20.0;
/// 不透明度轨道高（Figma 4）。
const TRACK_H: f32 = 4.0;
/// 不透明度圆钮直径（Figma 10）。
const KNOB: f32 = 10.0;
/// 不透明度行高（Figma 24）。
const OPACITY_H: f32 = 24.0;
/// 列表行距（Figma 2）。
const ROW_SPACING: f32 = 2.0;

/// 一条图层行（借用外部数据，本模块不改世界状态）。
#[derive(Clone)]
pub struct LayerEntry<'a> {
    /// 行名（调用方负责 i18n）。
    pub label: &'a str,
    /// 右端计数（如 `128`）；`None` 不占位。
    pub count: Option<u32>,
    /// 可见性。
    pub visible: bool,
    /// 锁定。
    pub locked: bool,
    /// 类型图标（12×12 SVG，Lucide）。
    pub icon: egui::ImageSource<'static>,
}

/// 面板交互结果（每帧至多一个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerPanelAction {
    /// 点中某行（选中）。
    Select(usize),
    /// 点中某行的眼睛。
    ToggleVisible(usize),
    /// 点中某行的锁。
    ToggleLocked(usize),
    /// 点中「新建图层组」。
    NewGroup,
    /// 点中「新建图层」。
    NewLayer,
}

/// 面板体状态。
pub struct LayerPanelState<'a> {
    /// 动作行左端小标题（设计稿 `DRAW ORDER · TOP TO BOTTOM`）。
    pub actions_label: &'a str,
    /// 是否画右端「新建组 / 新建图层」两枚图标钮。
    ///
    /// 数据侧暂无图层组、Custom 层也无渲染消费者（spec §8），接真实
    /// `MapNode.layers` 的调用方置 `false` 只留小标题；gallery 样张置
    /// `true` 展示完整设计稿形态。
    pub show_add: bool,
    /// 行数据。
    pub entries: Vec<LayerEntry<'a>>,
    /// 当前选中行索引（越界时不画选中态）。
    pub selected: usize,
    /// 不透明度 0..=1。
    pub opacity: f32,
    /// 不透明度行左端标签（调用方负责 i18n；设计稿 `Opacity` 41 px）。
    pub opacity_label: &'a str,
}

/// 图层面板体：动作行 + 列表 + 分隔线 + 不透明度行。
///
/// 返回值是**本帧的操作意图**，调用方决定怎么落地（本 crate 不碰世界数据）。
pub fn layer_panel(
    ui: &mut egui::Ui,
    shell: &UiShell,
    state: &mut LayerPanelState<'_>,
) -> Option<LayerPanelAction> {
    // 块距全部显式给（Figma `itemSpacing: 12`）：清掉 egui 的默认 6，
    // 否则设计稿逐项的高度会整体走样。
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut action = None;
    if let Some(a) = layer_actions(ui, shell, state.actions_label, state.show_add) {
        action = Some(a);
    }
    ui.add_space(BLOCK_GAP);

    if let Some(a) = layer_list(ui, shell, &state.entries, state.selected) {
        action = Some(a);
    }
    ui.add_space(BLOCK_GAP);

    super::primitives::divider(ui, shell);
    ui.add_space(BLOCK_GAP);

    opacity_row(ui, shell, state.opacity_label, &mut state.opacity);
    action
}

/// 动作行：左端 10 px 小标题 + 右端 `folder-plus` / `plus` 两枚 12 px 图标钮。
///
/// 设计稿 `Layer actions`（`3:20899`）：高 20、图标间距 10、右缘贴内容边。
/// `show_add = false` 时只画小标题（见 [`LayerPanelState::show_add`]）。
pub fn layer_actions(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    show_add: bool,
) -> Option<LayerPanelAction> {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ACTIONS_H),
        egui::Sense::hover(),
    );
    ui.painter().text(
        egui::pos2(rect.min.x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(font_size::MICRO - 1.0), // Figma 10 px
        shell.t3(),
    );
    if !show_add {
        return None;
    }

    // 从右往左：plus，再 folder-plus（Figma Actions gap 10）。
    let plus_rect = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - ROW_ICON, rect.center().y - ROW_ICON / 2.0),
        egui::Vec2::splat(ROW_ICON),
    );
    let folder_rect = plus_rect.translate(egui::vec2(-(ROW_ICON + 10.0), 0.0));

    let plus_resp = ui.interact(
        plus_rect,
        ui.id().with("layer_action_new"),
        egui::Sense::click(),
    );
    let folder_resp = ui.interact(
        folder_rect,
        ui.id().with("layer_action_group"),
        egui::Sense::click(),
    );
    let tint = |hovered: bool| if hovered { shell.t1() } else { shell.t3() };
    // 图标用 `Image::paint_at` 而不是 `ui.put`——`put` 会把父 ui 的光标推进到
    // 图标矩形（egui `scope` 语义），同一行之后的堆叠会被压扁。
    icon_image(
        super::primitives::ICON_FOLDER_PLUS,
        tint(folder_resp.hovered()),
        ROW_ICON,
    )
    .paint_at(ui, folder_rect);
    icon_image(
        super::primitives::ICON_PLUS,
        tint(plus_resp.hovered()),
        ROW_ICON,
    )
    .paint_at(ui, plus_rect);

    if plus_resp.clicked() {
        Some(LayerPanelAction::NewLayer)
    } else if folder_resp.clicked() {
        Some(LayerPanelAction::NewGroup)
    } else {
        None
    }
}

/// 图层列表（行距 2，每行 28）。
pub fn layer_list(
    ui: &mut egui::Ui,
    shell: &UiShell,
    entries: &[LayerEntry<'_>],
    selected: usize,
) -> Option<LayerPanelAction> {
    // 行距由本模块显式给 2（Figma `Layer list` itemSpacing 2）。
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut action = None;
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            ui.add_space(ROW_SPACING);
        }
        if let Some(a) = layer_list_row(ui, shell, entry, i == selected, i) {
            action = Some(a);
        }
    }
    action
}

/// 单条图层行（Figma `Map layer` `3:20905`）。
///
/// 布局：`眼睛 12 | 类型 12 | 名称（弹性） | 计数 | 锁定 12`，左右内边距 8、
/// 子项间距 8；选中 = `accent_soft` 底 + accent 字（`3:20911`）。
pub fn layer_list_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    entry: &LayerEntry<'_>,
    selected: bool,
    index: usize,
) -> Option<LayerPanelAction> {
    let (rect, resp) = row_rect(ui, ROW_H, egui::Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, radius::BADGE, shell.accent_soft());
    } else if resp.hovered() {
        p.rect_filled(rect, radius::BADGE, shell.hover_bg());
    }

    let cy = rect.center().y;
    let eye_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + ROW_PAD_X, cy - ROW_ICON / 2.0),
        egui::Vec2::splat(ROW_ICON),
    );
    let type_rect = eye_rect.translate(egui::vec2(ROW_ICON + ROW_GAP, 0.0));
    let label_x = type_rect.max.x + ROW_GAP;

    // 右端：锁（12）贴右内边距，计数在其左 8 px。
    let lock_rect = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - ROW_PAD_X - ROW_ICON, cy - ROW_ICON / 2.0),
        egui::Vec2::splat(ROW_ICON),
    );
    let count_right = lock_rect.min.x - ROW_GAP;
    let label_right = match entry.count {
        Some(_) => count_right - 24.0,
        None => count_right,
    };

    // 名称：可见 t1 / 隐藏 t3；选中 accent（`3:20914`）。
    let label_color = if selected {
        shell.accent()
    } else if entry.visible {
        shell.t1()
    } else {
        shell.t3()
    };
    let galley = p.layout_no_wrap(
        entry.label.to_owned(),
        proportional(font_size::MICRO), // Figma 11 px
        label_color,
    );
    let max_w = (label_right - label_x).max(0.0);
    let clipped = galley.size().x > max_w;
    let clip = if clipped {
        Some(egui::Rect::from_min_max(
            egui::pos2(label_x, rect.min.y),
            egui::pos2(label_x + max_w, rect.max.y),
        ))
    } else {
        None
    };
    {
        let painter = ui.painter().with_clip_rect(clip.unwrap_or(rect));
        painter.galley(
            egui::pos2(label_x, cy - galley.size().y / 2.0),
            galley,
            label_color,
        );
    }

    if let Some(count) = entry.count {
        p.text(
            egui::pos2(count_right, cy),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            proportional(font_size::MICRO - 1.0), // Figma 10 px
            shell.t3(),
        );
    }

    // 眼睛：可见 t2 / 隐藏 t3（设计稿眼睛描边同类型图标）。
    let eye_resp = ui.interact(
        eye_rect,
        ui.id().with(("layer_eye", index)),
        egui::Sense::click(),
    );
    let type_resp = ui.interact(
        type_rect,
        ui.id().with(("layer_type", index)),
        egui::Sense::hover(),
    );
    let _ = type_resp;
    let lock_resp = ui.interact(
        lock_rect,
        ui.id().with(("layer_lock", index)),
        egui::Sense::click(),
    );
    // 图标走 `Image::paint_at`（同动作行）：`ui.put` 会推进父 ui 光标压扁行距。
    icon_image(
        if entry.visible {
            ICON_EYE
        } else {
            ICON_EYE_OFF
        },
        if entry.visible {
            shell.t2()
        } else {
            shell.t3()
        },
        ROW_ICON,
    )
    .paint_at(ui, eye_rect);
    icon_image(
        entry.icon.clone(),
        if entry.visible {
            shell.t2()
        } else {
            shell.t3()
        },
        ROW_ICON,
    )
    .paint_at(ui, type_rect);
    // 锁：锁定 = 金色常显；未锁定只在行 hover 时提示（设计稿透明占位）。
    if entry.locked || resp.hovered() {
        icon_image(
            ICON_LOCK,
            if entry.locked {
                shell.gold()
            } else {
                shell.t3()
            },
            ROW_ICON,
        )
        .paint_at(ui, lock_rect);
    }

    if eye_resp.clicked() {
        Some(LayerPanelAction::ToggleVisible(index))
    } else if lock_resp.clicked() {
        Some(LayerPanelAction::ToggleLocked(index))
    } else if resp.clicked() {
        Some(LayerPanelAction::Select(index))
    } else {
        None
    }
}

/// 不透明度行（Figma `Layer opacity` `3:20942`）：`Opacity 41px | 轨道 | 100%`。
///
/// 轨道 4 px（radius 2），已完成段 accent，圆钮 10 px（`t1` 填充 + 2 px accent
/// 描边，`3:20945`）。命中区是整行高，便于拖动。标签由调用方给（i18n 归
/// 调用方），标签宽按实测文本取（设计稿 41 px 是 `Opacity` 的实测宽，中文
/// 「不透明度」更宽，按 Figma 定宽会截字）。
pub fn opacity_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut f32,
) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), OPACITY_H),
        egui::Sense::click_and_drag(),
    );

    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), proportional(font_size::MICRO), shell.t2());
    let value_text = format!("{:.0}%", (*value * 100.0).round());
    let value_galley =
        ui.painter()
            .layout_no_wrap(value_text, proportional(font_size::MICRO - 1.0), shell.t2());

    let label_w = label_galley.size().x; // Figma Label 41 px（EN 实测）
    let value_w = value_galley.size().x.max(24.0);
    let track_x0 = rect.min.x + label_w + ROW_GAP;
    let track_x1 = rect.max.x - value_w - ROW_GAP;
    let track = egui::Rect::from_min_max(
        egui::pos2(track_x0, rect.center().y - TRACK_H / 2.0),
        egui::pos2(track_x1.max(track_x0), rect.center().y + TRACK_H / 2.0),
    );

    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if track.width() > 0.0 {
                let frac = ((pos.x - track.min.x) / track.width()).clamp(0.0, 1.0);
                if (frac - *value).abs() > f32::EPSILON {
                    *value = frac;
                    resp.mark_changed();
                }
            }
        }
    }

    let p = ui.painter();
    p.galley(
        egui::pos2(rect.min.x, rect.center().y - label_galley.size().y / 2.0),
        label_galley,
        shell.t2(),
    );
    p.galley(
        egui::pos2(
            rect.max.x - value_galley.size().x,
            rect.center().y - value_galley.size().y / 2.0,
        ),
        value_galley,
        shell.t2(),
    );
    p.rect_filled(track, 2.0, shell.input_bg());
    let frac = value.clamp(0.0, 1.0);
    if frac > 0.0 {
        let fill_end = track.min.x + frac * track.width();
        p.rect_filled(
            egui::Rect::from_min_max(track.min, egui::pos2(fill_end, track.max.y)),
            2.0,
            shell.accent(),
        );
        let knob = egui::pos2(fill_end, rect.center().y);
        p.circle_filled(knob, KNOB / 2.0, color32(shell.palette.text_1));
        p.circle_stroke(knob, KNOB / 2.0, egui::Stroke::new(2.0, shell.accent()));
    }
    resp
}

/// 供测试/调用方复用的行距常量。
pub const LAYER_ROW_SPACING: f32 = ROW_SPACING;
/// 供测试/调用方复用的行高常量。
pub const LAYER_ROW_HEIGHT: f32 = ROW_H;
/// 供测试/调用方复用的内容块间距。
pub const LAYER_BLOCK_GAP: f32 = BLOCK_GAP;

// ── 测试 ────────────────────────────────────────────────────────────────
//
// 只做 headless 几何 / 不 panic 断言：点击分发靠 egui 的交互层，单测里没有
// 指针事件，真实交互由 `wv gallery` 人工验收（与其它控件模块同口径）。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::primitives::{
        ICON_IMAGE, ICON_LANDMARK, ICON_MOUNTAIN, ICON_ROUTE, ICON_TYPE, ICON_WAVES,
    };
    use crate::widgets::test_support::{make_ui, shell};

    fn entries() -> Vec<LayerEntry<'static>> {
        vec![
            LayerEntry {
                label: "Labels",
                count: Some(128),
                visible: true,
                locked: false,
                icon: ICON_TYPE,
            },
            LayerEntry {
                label: "Settlements",
                count: Some(42),
                visible: true,
                locked: false,
                icon: ICON_LANDMARK,
            },
            LayerEntry {
                label: "Roads & paths",
                count: Some(86),
                visible: true,
                locked: false,
                icon: ICON_ROUTE,
            },
            LayerEntry {
                label: "Rivers & water",
                count: Some(24),
                visible: true,
                locked: false,
                icon: ICON_WAVES,
            },
            LayerEntry {
                label: "Terrain",
                count: Some(1),
                visible: true,
                locked: true,
                icon: ICON_MOUNTAIN,
            },
            LayerEntry {
                label: "Reference sketch",
                count: Some(1),
                visible: false,
                locked: false,
                icon: ICON_IMAGE,
            },
        ]
    }

    #[test]
    fn layer_row_height_matches_figma_28() {
        let sh = shell();
        let (_ctx, mut ui) = make_ui("layer_row_h", 244.0, 200.0);
        let entries = entries();
        let _ = layer_list_row(&mut ui, &sh, &entries[0], false, 0);
        assert!(
            (ui.min_rect().height() - ROW_H).abs() < 1e-3,
            "单行高应 = {ROW_H}（Figma Map layer 28），got {}",
            ui.min_rect().height()
        );
    }

    #[test]
    fn layer_list_six_rows_take_figma_178() {
        let sh = shell();
        let (_ctx, mut ui) = make_ui("layer_list_h", 244.0, 400.0);
        let entries = entries();
        let _ = layer_list(&mut ui, &sh, &entries, 1);
        // 6×28 + 5×2 = 178（Figma `Layer list` 高 178）。
        assert!(
            (ui.min_rect().height() - 178.0).abs() < 1e-3,
            "6 行列表应 = 178，got {}",
            ui.min_rect().height()
        );
    }

    #[test]
    fn layer_panel_body_height_matches_figma_259() {
        let sh = shell();
        let (_ctx, mut ui) = make_ui("layer_panel_h", 244.0, 600.0);
        let entries = entries();
        let mut state = LayerPanelState {
            actions_label: "DRAW ORDER · TOP TO BOTTOM",
            show_add: true,
            entries,
            selected: 1,
            opacity: 1.0,
            opacity_label: "Opacity",
        };
        let _ = layer_panel(&mut ui, &sh, &mut state);
        // Figma Panel content 内高 259 = 20 + 12 + 178 + 12 + 1 + 12 + 24。
        assert!(
            (ui.min_rect().height() - 259.0).abs() < 1e-3,
            "面板体应 = 259，got {}",
            ui.min_rect().height()
        );
    }

    #[test]
    fn layer_panel_without_add_buttons_keeps_body_height() {
        let sh = shell();
        let (_ctx, mut ui) = make_ui("layer_panel_no_add", 244.0, 600.0);
        let entries = entries();
        let mut state = LayerPanelState {
            actions_label: "DRAW ORDER · TOP TO BOTTOM",
            show_add: false,
            entries,
            selected: 0,
            opacity: 1.0,
            opacity_label: "Opacity",
        };
        let action = layer_panel(&mut ui, &sh, &mut state);
        assert!(action.is_none(), "无指针事件不应有动作");
        assert!(
            (ui.min_rect().height() - 259.0).abs() < 1e-3,
            "隐藏新建钮不改变面板体高（图标是 paint_at 叠加），got {}",
            ui.min_rect().height()
        );
    }

    #[test]
    fn opacity_row_height_and_range_are_figma_spec() {
        let sh = shell();
        let (_ctx, mut ui) = make_ui("opacity_row_h", 244.0, 100.0);
        let mut v = 1.0_f32;
        let _ = opacity_row(&mut ui, &sh, "Opacity", &mut v);
        assert!(
            (ui.min_rect().height() - OPACITY_H).abs() < 1e-3,
            "不透明度行应 = {OPACITY_H}，got {}",
            ui.min_rect().height()
        );
        assert!((v - 1.0).abs() < 1e-6, "无指针事件时值不变，got {v}");
    }
}
