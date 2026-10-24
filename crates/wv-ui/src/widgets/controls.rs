//! 共享表单 / 值控件 —— 对齐 Figma《公共组件目录》03/04 章。
//!
//! 覆盖：
//! - 输入：`FieldInput`（空/填充/聚焦/禁用/错误）、`search_input`、
//!   `numeric_input`、`text_area`、`FieldSelect`（ComboBox）、`option_menu`、
//!   `dropdown_box`（只读下拉外观）。
//! - 选择：`checkbox_state`（四态）、`check_row`（行式复选）、`radio`、
//!   `Switch`、`tabs`。
//! - 数值：`slider` / `slider_row`（蓝）/ `slider_row_brush`（金）/
//!   `slider_row_disabled`、`value_unit`、`progress`。
//! - 检视器：`PropertyRow` / `value_box`（42:2 组件族，随色板更新）。
//!
//! 几何口径（目录）：常规控件 28 高、紧凑 24；输入类内边距 0 8；圆角 4；
//! 描边 1px（聚焦 2px accent）。颜色全部走 `UiShell` 令牌，**不写裸 hex**。
//!
//! 边界：不承载浮窗外壳（`floater`）、不写主题令牌源头。

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::ui_theme::{margin_xy, mono, proportional};
use crate::ui_tokens::{font_size, radius, space};
use crate::widgets::primitives::{
    icon_image, paint_selected_row, row_rect, ICON_CHECK, ICON_CHEVRON_DOWN, ICON_SEARCH,
};

const CONTROL_H: f32 = 28.0;
const COMPACT_H: f32 = 24.0;
const INPUT_PAD_X: f32 = 8.0;

// ── FieldInput（单行文本输入 · 目录 UI/Input）──────────────────────────

/// 单行文本输入框，外观对齐目录 `UI/Input`：
/// `bg_input` 内嵌底 + `border_subtle` 1px + radius 4 + 左右 8 / 上下 4
/// 内边距（合计 28 高）；聚焦时描边换 `accent` 2px，错误态换 `danger`。
pub struct FieldInput<'a> {
    value: &'a mut String,
    hint: String,
    font_mono: bool,
    min_size: Option<egui::Vec2>,
    fill_width: bool,
    focus_on_add: bool,
    error: bool,
    enabled: bool,
}

impl<'a> FieldInput<'a> {
    /// 绑定 `&mut String` 与占位文本。
    pub fn new(value: &'a mut String, hint: &str) -> Self {
        Self {
            value,
            hint: hint.to_owned(),
            font_mono: false,
            min_size: None,
            fill_width: false,
            focus_on_add: false,
            error: false,
            enabled: true,
        }
    }

    /// 等宽字体（默认比例字体）。
    pub fn font_mono(mut self, on: bool) -> Self {
        self.font_mono = on;
        self
    }

    /// 显式最小外尺寸（含内边距与描边）。未设时高度 ≈ 28。
    pub fn min_size(mut self, s: egui::Vec2) -> Self {
        self.min_size = Some(s);
        self
    }

    /// 撑满父容器宽度。
    pub fn fill_width(mut self, on: bool) -> Self {
        self.fill_width = on;
        self
    }

    /// 添加帧自动抢焦点。
    pub fn focus_on_add(mut self, on: bool) -> Self {
        self.focus_on_add = on;
        self
    }

    /// 错误态：描边换 `danger`（目录 `Input/Error`）。
    pub fn error(mut self, on: bool) -> Self {
        self.error = on;
        self
    }

    /// 禁用态：文字换 `text_disabled`、不可编辑。
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// 装配并落地。返回文本编辑控件的 `Response`。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let stroke_color = if self.error {
            shell.danger()
        } else {
            shell.border_subtle()
        };
        let frame = egui::Frame::default()
            .fill(shell.input_bg())
            .stroke(egui::Stroke::new(1.0, stroke_color))
            .corner_radius(radius::FIELD)
            .inner_margin(margin_xy(INPUT_PAD_X, 4.0));
        let mut out = None;
        frame.show(ui, |ui| {
            let mut edit = egui::TextEdit::singleline(self.value)
                .hint_text(&self.hint)
                .desired_width(if self.fill_width { f32::INFINITY } else { 0.0 })
                .background_color(egui::Color32::TRANSPARENT)
                .margin(egui::Margin::ZERO)
                .frame(egui::Frame::NONE)
                .text_color(if self.enabled {
                    shell.t1()
                } else {
                    shell.text_disabled()
                });
            if self.font_mono {
                edit = edit.font(mono(font_size::MICRO));
            }
            let inner_min = self
                .min_size
                .map(|s| {
                    egui::vec2(
                        (s.x - 2.0 * (INPUT_PAD_X + 1.0)).max(0.0),
                        (s.y - 2.0 * (4.0 + 1.0)).max(0.0),
                    )
                })
                .unwrap_or(egui::vec2(0.0, 18.0));
            edit = edit.min_size(inner_min);
            let resp = ui.add_enabled(self.enabled, edit);
            // 聚焦环：目录 `Input/Focus` = accent 2px（画在框内侧）。
            if resp.has_focus() {
                let r = resp.rect.expand2(egui::vec2(INPUT_PAD_X, 4.0));
                ui.painter().rect_stroke(
                    r,
                    radius::FIELD,
                    egui::Stroke::new(2.0, shell.accent()),
                    egui::StrokeKind::Inside,
                );
            }
            if self.focus_on_add {
                resp.request_focus();
            }
            out = Some(resp);
        });
        out.expect("Frame::show 必然执行一次")
    }
}

// ── SearchInput（目录 UI/SearchInput）──────────────────────────────────

/// 搜索框：左侧 search 图标 + 文本 + 右侧快捷键提示（如 `Ctrl+K`）。
pub fn search_input(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut String,
    hint: &str,
    shortcut: &str,
) -> egui::Response {
    let frame = egui::Frame::default()
        .fill(shell.input_bg())
        .stroke(egui::Stroke::new(1.0, shell.border_subtle()))
        .corner_radius(radius::FIELD)
        .inner_margin(margin_xy(INPUT_PAD_X, 4.0));
    let mut out = None;
    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::SM;
            ui.add(icon_image(ICON_SEARCH, shell.t3(), 12.0));
            // 快捷键提示按内容宽度预留，输入框只吃剩余宽度——否则
            // `desired_width(INFINITY)` 会把提示挤到下一行、撑高整框。
            let shortcut_w = if shortcut.is_empty() {
                0.0
            } else {
                ui.painter()
                    .layout_no_wrap(
                        shortcut.to_owned(),
                        proportional(font_size::MICRO),
                        shell.t3(),
                    )
                    .size()
                    .x
                    + space::SM
            };
            let edit_w = (ui.available_width() - shortcut_w).max(40.0);
            let resp = ui.add(
                egui::TextEdit::singleline(value)
                    .hint_text(hint)
                    .desired_width(edit_w)
                    .background_color(egui::Color32::TRANSPARENT)
                    .margin(egui::Margin::ZERO)
                    .frame(egui::Frame::NONE)
                    .min_size(egui::vec2(0.0, 18.0)),
            );
            if !shortcut.is_empty() {
                ui.label(
                    egui::RichText::new(shortcut)
                        .size(font_size::MICRO)
                        .color(shell.t3()),
                );
            }
            out = Some(resp);
        });
    });
    out.expect("Frame::show 必然执行一次")
}

// ── NumericInput（目录 UI/NumericInput：值 + 单位 + 步进器）────────────

/// 数值输入：等宽值 + 单位（t3）+ 右侧 24×28 步进器（上下三角）。
/// 步进器点击即 ±`step`；返回值区域的 `Response`。
pub fn numeric_input(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut f32,
    unit: &str,
    step: f32,
) -> egui::Response {
    numeric_input_bounded(ui, shell, value, unit, step, f32::MIN, f32::MAX)
}

/// [`numeric_input`] 的**带边界变体**（目录 UI/NumericInput/Dark/Default）：
/// stepper 与任何写入都会被钳到 `[min, max]`——Cell 编辑器海拔（i16 米）
/// 等有真实值域的字段用这个，调用方不再需要事后 clamp。
pub fn numeric_input_bounded(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut f32,
    unit: &str,
    step: f32,
    min: f32,
    max: f32,
) -> egui::Response {
    // **id 唯一性（根治）**：本控件内部用 `ui.id()` 派生的 id 登记 stepper
    // 三个交互区（up / down / body）。同一 Ui scope 里放**两个**数值输入时，
    // 两者的 `ui.id()` 相同 → egui 报「First use of widget ID …」并让控件
    // 状态互串（2026-10-20 实机：批量区「绝对值 + 增减」两个输入告警）。
    //
    // 修法：按「被编辑值的地址」派生一个盐，把整个控件放进 push_id 作用域
    // ——同一 scope 的不同输入框地址不同 → id 天然不同；同一输入框跨帧
    // 地址稳定 → id 稳定（步进器状态不丢）。
    let salt = value as *mut f32 as usize;
    let mut resp: Option<egui::Response> = None;
    ui.push_id(salt, |ui| {
        resp = Some(numeric_input_bounded_inner(
            ui, shell, value, unit, step, min, max,
        ));
    });
    resp.expect("push_id 闭包必然执行一次")
}

// ── 数值输入手势状态机（D1 / D1-A / D1-B · ADR-0052 §9.2）──────────────
//
// D1 的核心是**判定顺序**：按下时**不**立即决定意图——位移越过阈值才进
// scrub，未越过则 release 时进文本编辑态。这条不钉死，实现者会把
// 「点按」与「拖拽」各解释一遍（每次点击闪一下编辑态 / 永远进不了编辑态）。
//
// D1-A 在 D1 的 4px 阈值之上加一层 **8px 起手宽限期**：位移落在
// (4, 8] 且松开 = 静默回滚原值，既不进编辑态也不改值——把「快而短的
// 手抖」从误判里摘出去。手抖的判据是位移而非时间，故不引入 time-based
// 延迟（那会让 scrub 本身感觉迟钝）。
//
// 状态跨帧存活在 egui memory（`data.get_temp_mut_or_default`）——纯
// immediate-mode 控件没有别的地方放手势态。

/// 点按进文本编辑的位移上限（D1）。按下后位移 ≤ 此值并松开 = 点按。
const NUMERIC_TAP_PX: f32 = 4.0;
/// SCRUB 起手宽限区上界（D1-A）。位移落在 (4, 8] 且松开 = 静默回滚。
const NUMERIC_SCRUB_ARM_PX: f32 = 8.0;
/// 每个 step 的像素（D1）。scrub 中每 4 px 走一个 step。
const NUMERIC_PX_PER_STEP: f32 = 4.0;

/// 数值输入的会话手势态（D1 + D1-A）。
#[derive(Clone, Debug, Default, PartialEq)]
enum NumericGesture {
    /// 空闲。
    #[default]
    Idle,
    /// 按下中，位移尚未越过阈值。
    Pressed {
        press_pos: egui::Pos2,
        press_value: f32,
    },
    /// 文本编辑态（D1）：进入时全选既有值，键入即覆盖。
    TextEditing {
        /// 进入编辑态时的原值——`Esc` 回滚到它。
        orig_value: f32,
        /// 文本缓冲（egui `TextEdit` 由调用方持文本，每帧传同一个 `String`）。
        text: String,
        /// 进入编辑态后的**第一帧**需强制全选既有值（D1）。
        select_pending: bool,
    },
    /// SCRUB 起手宽限期（D1-A）：气泡已提示，值尚未改动。
    ScrubArmed {
        press_pos: egui::Pos2,
        press_value: f32,
    },
    /// SCRUB 写值中（D1）：跨过 8px 宽限，每 4px 一个 step。
    ScrubStepping {
        press_pos: egui::Pos2,
        press_value: f32,
    },
}

impl NumericGesture {
    /// 按下时的指针位置——scrub 各态据此自算累计位移。
    ///
    /// **为什么不用 egui 的 `drag_started()`**：egui 自带拖拽起动阈值，
    /// 会架空 D1 的 4px 判定（D1 明写「判定顺序是控件规格的一部分，
    /// 不是实现细节」）。本控件一律用「按下位置 vs 当前指针位置」自算。
    fn press_pos(&self) -> Option<egui::Pos2> {
        match self {
            NumericGesture::Pressed { press_pos, .. }
            | NumericGesture::ScrubArmed { press_pos, .. }
            | NumericGesture::ScrubStepping { press_pos, .. } => Some(*press_pos),
            _ => None,
        }
    }

    /// 按下时的值——scrub 的基准值（`Esc` 回滚到它）。
    fn press_value(&self) -> Option<f32> {
        match self {
            NumericGesture::Pressed { press_value, .. }
            | NumericGesture::ScrubArmed { press_value, .. }
            | NumericGesture::ScrubStepping { press_value, .. } => Some(*press_value),
            _ => None,
        }
    }
}

/// 修饰键档位（D1）：`Alt` ×100 / `Ctrl` ×0.1 / `Shift` ×10，其余 ×1。
///
/// 取最极端档：`Ctrl`（精修）与 `Alt`（粗调）同按时以 `Alt` 为准——
/// 二者语义相反，混档会让用户拿不到预期的粒度。
fn numeric_scrub_multiplier(m: egui::Modifiers) -> f32 {
    if m.alt {
        100.0
    } else if m.ctrl {
        0.1
    } else if m.shift {
        10.0
    } else {
        1.0
    }
}

/// 数值输入的错误态（D1-B）：红闪 200ms + tooltip 持续到下次成功提交。
#[derive(Clone, Debug, Default)]
struct NumericError {
    /// 提示文案（tooltip 持续显示，直到下一次成功提交）。
    message: String,
    /// 红闪截止时刻（egui 时钟秒）。
    flash_until: f64,
}

/// [`numeric_input_bounded`] 的实际实现（调用方已保证 id 作用域隔离）。
fn numeric_input_bounded_inner(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut f32,
    unit: &str,
    step: f32,
    min: f32,
    max: f32,
) -> egui::Response {
    // ── 手势态 / 错误态跨帧存活（D1 / D1-B）──
    let gesture_id = ui.id().with("numeric_gesture");
    let error_id = ui.id().with("numeric_error");
    let mut gesture = ui
        .memory(|m| m.data.get_temp::<NumericGesture>(gesture_id))
        .unwrap_or_default();
    let mut error = ui
        .memory(|m| m.data.get_temp::<NumericError>(error_id))
        .unwrap_or_default();
    let now = ui.ctx().time();

    // ── 编辑态（D1）：egui 原生 TextEdit 覆盖在自绘框上 ──
    if let NumericGesture::TextEditing {
        orig_value,
        text,
        select_pending,
    } = &mut gesture
    {
        let text_id = ui.make_persistent_id("numeric_text");
        let just_entered = *select_pending;
        let orig = *orig_value;

        let frame = egui::Frame::default()
            .fill(shell.input_bg())
            .stroke(egui::Stroke::new(2.0, shell.focus()))
            .corner_radius(radius::FIELD)
            .inner_margin(margin_xy(INPUT_PAD_X, 0.0));
        let mut out = None;
        frame.show(ui, |ui| {
            out = Some(ui.add(
                egui::TextEdit::singleline(text)
                    .id_source("numeric_text")
                    .desired_width(f32::INFINITY)
                    .background_color(egui::Color32::TRANSPARENT)
                    .margin(egui::Margin::ZERO)
                    .frame(egui::Frame::NONE)
                    .text_color(shell.t1()),
            ));
        });
        let resp = out.expect("Frame::show 必然执行一次");

        // 进入编辑态的第一帧：请求焦点 + 全选既有值（D1「键入即覆盖」）
        if just_entered {
            ui.memory_mut(|m| m.request_focus(text_id));
            let mut st = egui::widgets::text_edit::TextEditState::load(ui.ctx(), text_id)
                .unwrap_or_default();
            let len = text.chars().count();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(len),
            )));
            st.store(ui.ctx(), text_id);
            *select_pending = false;
        }

        // 键盘（D1：Enter 提交 / Esc 取消 / Tab 提交并跳下一字段）
        let mut key_enter = false;
        let mut key_esc = false;
        let mut key_tab = false;
        ui.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } = ev
                {
                    match key {
                        egui::Key::Enter => key_enter = true,
                        egui::Key::Escape => key_esc = true,
                        egui::Key::Tab => key_tab = true,
                        _ => {}
                    }
                }
            }
        });

        if key_enter || key_tab {
            numeric_commit_text(text, orig, value, min, max, now, &mut error);
            gesture = NumericGesture::Idle;
            ui.memory_mut(|m| m.surrender_focus(text_id));
        } else if key_esc {
            // Esc 回滚到进入编辑态前的值（不提交）
            *value = orig;
            gesture = NumericGesture::Idle;
            ui.memory_mut(|m| m.surrender_focus(text_id));
        } else if resp.lost_focus() && !just_entered {
            // 失焦：合法则提交，非法则回滚（D1）
            numeric_commit_text(text, orig, value, min, max, now, &mut error);
            gesture = NumericGesture::Idle;
        }

        ui.memory_mut(|m| m.data.insert_temp(gesture_id, gesture));
        ui.memory_mut(|m| m.data.insert_temp(error_id, error));
        return resp;
    }

    // ── 常态：自绘框 + stepper + body 手势（D1）──
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CONTROL_H),
        egui::Sense::hover(),
    );
    // D1-B：非法输入后红闪 200ms（边框换 danger 色 + 加粗）
    let flashing = now < error.flash_until;
    let border_color = if flashing {
        shell.danger()
    } else {
        shell.border_subtle()
    };
    let p = ui.painter();
    p.rect_filled(rect, radius::FIELD, shell.input_bg());
    p.rect_stroke(
        rect,
        radius::FIELD,
        egui::Stroke::new(if flashing { 2.0 } else { 1.0 }, border_color),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(rect.min.x + INPUT_PAD_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{value:.0}"),
        mono(font_size::MICRO),
        shell.t1(),
    );
    p.text(
        egui::pos2(rect.min.x + INPUT_PAD_X + 34.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        unit,
        proportional(font_size::MICRO),
        shell.t3(),
    );
    let stepper = egui::Rect::from_min_max(egui::pos2(rect.max.x - 24.0, rect.min.y), rect.max);
    p.line_segment(
        [
            egui::pos2(stepper.min.x, stepper.min.y),
            egui::pos2(stepper.min.x, stepper.max.y),
        ],
        egui::Stroke::new(1.0, shell.border_subtle()),
    );
    let cx = stepper.center().x;
    let up_cy = stepper.center().y - 5.0;
    let dn_cy = stepper.center().y + 5.0;
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(cx - 3.0, up_cy + 2.0),
            egui::pos2(cx + 3.0, up_cy + 2.0),
            egui::pos2(cx, up_cy - 2.0),
        ],
        shell.t3(),
        egui::Stroke::NONE,
    ));
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(cx - 3.0, dn_cy - 2.0),
            egui::pos2(cx + 3.0, dn_cy - 2.0),
            egui::pos2(cx, dn_cy + 2.0),
        ],
        shell.t3(),
        egui::Stroke::NONE,
    ));
    let _ = p;
    let up = ui.interact(
        egui::Rect::from_min_max(stepper.min, egui::pos2(stepper.max.x, stepper.center().y)),
        ui.id().with("numeric_up"),
        egui::Sense::click(),
    );
    let down = ui.interact(
        egui::Rect::from_min_max(egui::pos2(stepper.min.x, stepper.center().y), stepper.max),
        ui.id().with("numeric_down"),
        egui::Sense::click(),
    );
    if up.clicked() {
        *value = (*value + step).clamp(min, max);
        error.message.clear();
        error.flash_until = 0.0;
    }
    if down.clicked() {
        *value = (*value - step).clamp(min, max);
        error.message.clear();
        error.flash_until = 0.0;
    }

    // ── body 手势（D1）：点按 → 文本编辑；拖动 → scrub ──
    let body = ui.interact(
        rect,
        ui.id().with("numeric_body"),
        egui::Sense::click_and_drag(),
    );
    let pressed_on = body.is_pointer_button_down_on();
    let pointer_pos = body.interact_pointer_pos();

    if pressed_on {
        // 刚按下（状态机自判，不依赖 egui 的拖拽起动阈值）
        if gesture == NumericGesture::Idle {
            if let Some(pos) = pointer_pos {
                gesture = NumericGesture::Pressed {
                    press_pos: pos,
                    press_value: *value,
                };
            }
        }
        // 位移判定 → 状态迁移
        if let (Some(press_pos), Some(pos)) = (gesture.press_pos(), pointer_pos) {
            let total = (press_pos - pos).length();
            let press_value = gesture.press_value().unwrap_or(*value);
            if total > NUMERIC_SCRUB_ARM_PX {
                gesture = NumericGesture::ScrubStepping {
                    press_pos,
                    press_value,
                };
            } else if total > NUMERIC_TAP_PX {
                gesture = NumericGesture::ScrubArmed {
                    press_pos,
                    press_value,
                };
            }
        }
        // SCRUB 写值（D1：每 4px 一个 step；修饰键实时换档）
        if let NumericGesture::ScrubStepping {
            press_pos,
            press_value,
        } = &gesture
        {
            if let Some(pos) = pointer_pos {
                let total = (*press_pos - pos).length();
                let mult = ui.input(|i| numeric_scrub_multiplier(i.modifiers));
                let steps = (total / NUMERIC_PX_PER_STEP).floor();
                *value = (press_value + steps * step * mult).clamp(min, max);
            }
        }
    } else if gesture != NumericGesture::Idle {
        // ── 松手 ──
        match &gesture {
            NumericGesture::Pressed {
                press_pos,
                press_value,
            } => {
                let total = pointer_pos
                    .map(|pos| (*press_pos - pos).length())
                    .unwrap_or(0.0);
                if total <= NUMERIC_TAP_PX {
                    // 点按 → 进文本编辑态（D1：进入时全选既有值，键入即覆盖）
                    gesture = NumericGesture::TextEditing {
                        orig_value: *press_value,
                        text: format!("{press_value:.0}"),
                        select_pending: true,
                    };
                    error.message.clear();
                    error.flash_until = 0.0;
                }
            }
            NumericGesture::ScrubArmed { press_value, .. } => {
                // 宽限期（4–8px）→ 静默回滚（D1-A）：不改值、不进编辑态
                *value = *press_value;
                gesture = NumericGesture::Idle;
            }
            NumericGesture::ScrubStepping { .. } => {
                // 已写值 → 提交（拖动结束即生效）
                gesture = NumericGesture::Idle;
            }
            _ => {}
        }
    }

    // scrub 气泡（D1：拖动中在光标上方浮动显示当前值 + 单位）
    if matches!(
        gesture,
        NumericGesture::ScrubArmed { .. } | NumericGesture::ScrubStepping { .. }
    ) {
        if let Some(pos) = pointer_pos {
            let label = format!("{value:.0} {unit}");
            let p = ui.painter();
            let galley = p.layout_no_wrap(label, mono(font_size::MICRO), shell.t1());
            let bubble = egui::Rect::from_min_size(
                pos + egui::vec2(0.0, -galley.size().y - 10.0),
                galley.size() + egui::vec2(10.0, 6.0),
            );
            p.rect_filled(bubble, 3.0, shell.float_fill());
            p.rect_stroke(
                bubble,
                3.0,
                egui::Stroke::new(1.0, shell.border_strong()),
                egui::StrokeKind::Inside,
            );
            p.galley(bubble.min + egui::vec2(5.0, 3.0), galley, shell.t1());
        }
    }

    ui.memory_mut(|m| m.data.insert_temp(gesture_id, gesture));
    ui.memory_mut(|m| m.data.insert_temp(error_id, error));
    ui.interact(rect, ui.id().with("numeric_body"), egui::Sense::hover())
}

/// 提交文本值（D1 + D1-B）。
///
/// - 合法：写值并钳制到 `[min, max]`；发生钳制时提示「已钳制到 X」。
/// - 非法（空 / 非数字 / NaN）：**回滚原值，绝不写 0**（D1 明写），并红闪 200ms。
fn numeric_commit_text(
    text: &str,
    orig_value: f32,
    value: &mut f32,
    min: f32,
    max: f32,
    now: f64,
    error: &mut NumericError,
) {
    let parsed = text.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    match parsed {
        Some(v) => {
            let clamped = v.clamp(min, max);
            *value = clamped;
            if (clamped - v).abs() > f32::EPSILON {
                error.message = format!("已钳制到 {clamped:.0}");
            } else {
                error.message.clear();
            }
            error.flash_until = 0.0;
        }
        None => {
            *value = orig_value;
            error.message = "非法输入，已恢复原值".to_string();
            error.flash_until = now + 0.2;
        }
    }
}

// ── TextArea（目录 UI/TextArea：3 行）─────────────────────────────────

/// 多行文本区：`bg_input` 底 + border_subtle + radius 4 + 8px 内边距。
pub fn text_area(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut String,
    rows: usize,
) -> egui::Response {
    let h = rows as f32 * 18.0 + 16.0;
    let frame = egui::Frame::default()
        .fill(shell.input_bg())
        .stroke(egui::Stroke::new(1.0, shell.border_subtle()))
        .corner_radius(radius::FIELD)
        .inner_margin(margin_xy(8.0, 8.0));
    let mut out = None;
    frame.show(ui, |ui| {
        ui.set_min_height(h);
        let resp = ui.add(
            egui::TextEdit::multiline(value)
                .desired_width(f32::INFINITY)
                .desired_rows(rows)
                .background_color(egui::Color32::TRANSPARENT)
                .margin(egui::Margin::ZERO)
                .frame(egui::Frame::NONE)
                .text_color(shell.t1()),
        );
        out = Some(resp);
    });
    out.expect("Frame::show 必然执行一次")
}

// ── Dropdown（只读外观） / OptionMenu ─────────────────────────────────

/// 只读下拉外观（目录 `UI/Dropdown/Closed`）：输入底 + 文本 + `⌄`。
pub fn dropdown_box(ui: &mut egui::Ui, shell: &UiShell, text: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CONTROL_H),
        egui::Sense::click(),
    );
    let p = ui.painter();
    p.rect_filled(rect, radius::FIELD, shell.input_bg());
    p.rect_stroke(
        rect,
        radius::FIELD,
        egui::Stroke::new(1.0, shell.border_subtle()),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(rect.min.x + INPUT_PAD_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        proportional(font_size::SMALL),
        shell.t1(),
    );
    p.text(
        egui::pos2(rect.max.x - INPUT_PAD_X, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "⌄",
        proportional(font_size::SMALL),
        shell.t3(),
    );
    resp
}

/// 展开的选项菜单（目录 `UI/OptionMenu/Expanded`）：内嵌底 + border_strong
/// 描边 + radius 6 + 4px 内边距；选项 28 高，选中 = accent_soft 底 + accent
/// 字 + check 图标；禁用 = text_disabled。返回被点击的选项下标。
///
/// `id` 作为各行交互 id 的命名空间（同 [`crate::widgets::segmented_control::segmented`]）：
/// 同一面板内并存多个 OptionMenu 时，靠它避免 egui 自动 id 相互覆盖。
pub fn option_menu(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: &str,
    labels: &[&str],
    selected: usize,
    disabled: &[usize],
) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::default()
        .fill(shell.input_bg())
        .stroke(egui::Stroke::new(1.0, shell.border_strong()))
        .corner_radius(radius::CONTROL)
        .inner_margin(margin_xy(4.0, 4.0))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, label) in labels.iter().enumerate() {
                let is_disabled = disabled.contains(&i);
                let sense = if is_disabled {
                    egui::Sense::hover()
                } else {
                    egui::Sense::click()
                };
                // 行 id 由 `id` 命名空间派生（同 `segmented`），不依赖 egui 自动 id。
                let (rect, _) = row_rect(ui, CONTROL_H, egui::Sense::hover());
                let resp = ui.interact(rect, egui::Id::new((id, i)), sense);
                let is_sel = i == selected;
                let p = ui.painter();
                if is_sel {
                    paint_selected_row(p, rect, shell, radius::FIELD, false);
                } else if resp.hovered() && !is_disabled {
                    p.rect_filled(rect, radius::FIELD, shell.hover_bg());
                }
                let fg = if is_disabled {
                    shell.text_disabled()
                } else if is_sel {
                    shell.accent()
                } else {
                    shell.t1()
                };
                p.text(
                    egui::pos2(rect.min.x + INPUT_PAD_X, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    *label,
                    proportional(font_size::SMALL),
                    fg,
                );
                if is_sel {
                    let _ = ui.put(
                        egui::Rect::from_center_size(
                            egui::pos2(rect.max.x - 12.0, rect.center().y),
                            egui::vec2(12.0, 12.0),
                        ),
                        icon_image(ICON_CHECK, shell.accent(), 12.0),
                    );
                }
                if resp.clicked() && !is_disabled {
                    clicked = Some(i);
                }
            }
        });
    clicked
}

// ── Checkbox（四态） / Radio ───────────────────────────────────────────

/// 复选框状态（目录 `UI/Checkbox` 四变体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// 未选：`bg_input` 底 + `border_strong` 描边。
    Unchecked,
    /// 已选：`accent` 底 + 深色勾。
    Checked,
    /// 半选：`accent` 底 + 6×1 横条。
    Indeterminate,
    /// 禁用：文字 `text_disabled`。
    Disabled,
}

/// 复选框（目录几何）：12×12 方块 radius 2 + 8px 间距 + 11px 标签，行高 24。
pub fn checkbox_state(
    ui: &mut egui::Ui,
    shell: &UiShell,
    state: CheckState,
    label: &str,
) -> egui::Response {
    const BOX: f32 = 12.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), COMPACT_H),
        egui::Sense::click(),
    );
    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + BOX / 2.0, rect.center().y),
        egui::vec2(BOX, BOX),
    );
    let disabled = state == CheckState::Disabled;
    let checked = matches!(state, CheckState::Checked | CheckState::Indeterminate);
    let p = ui.painter();
    p.rect_filled(
        box_rect,
        2.0,
        if checked {
            shell.accent()
        } else {
            shell.input_bg()
        },
    );
    p.rect_stroke(
        box_rect,
        2.0,
        egui::Stroke::new(1.0, shell.border_strong()),
        egui::StrokeKind::Inside,
    );
    if state == CheckState::Indeterminate {
        p.rect_filled(
            egui::Rect::from_center_size(box_rect.center(), egui::vec2(6.0, 1.0)),
            0.0,
            shell.text_on_accent(),
        );
    }
    let fg = if disabled {
        shell.text_disabled()
    } else {
        shell.t1()
    };
    p.text(
        egui::pos2(box_rect.max.x + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(font_size::MICRO),
        fg,
    );
    let _ = p;
    if state == CheckState::Checked {
        let _ = ui.put(
            egui::Rect::from_center_size(box_rect.center(), egui::vec2(10.0, 10.0)),
            icon_image(ICON_CHECK, shell.text_on_accent(), 10.0),
        );
    }
    resp
}

/// 行式复选（整行可点 + 右端挂件槽）。`on` 在点击时翻转。
pub fn check_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    on: &mut bool,
    label: &str,
    trailing: Option<egui::ImageSource<'static>>,
) -> egui::Response {
    let state = if *on {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    };
    let resp = checkbox_state(ui, shell, state, label);
    if resp.clicked() {
        *on = !*on;
    }
    if let Some(src) = trailing {
        let r = egui::Rect::from_center_size(
            egui::pos2(resp.rect.max.x - 8.0, resp.rect.center().y),
            egui::vec2(12.0, 12.0),
        );
        let _ = ui.put(r, icon_image(src, shell.t3(), 12.0));
    }
    resp
}

/// 单选（目录 `UI/Radio`）：12×12 圆环 + 选中 6×6 圆点，行高 24。
pub fn radio(ui: &mut egui::Ui, shell: &UiShell, on: bool, label: &str) -> egui::Response {
    const RING: f32 = 12.0;
    // 单选是**内容宽度**控件：横向排多个时不能吃满整行（否则第二个会被
    // 推到列宽之外）。宽度 = 圆环 + 间距 + 文本。
    let text_w = ui
        .painter()
        .layout_no_wrap(
            label.to_string(),
            proportional(font_size::MICRO),
            shell.t1(),
        )
        .size()
        .x;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(RING + space::SM + text_w, COMPACT_H),
        egui::Sense::click(),
    );
    let ring = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + RING / 2.0, rect.center().y),
        egui::vec2(RING, RING),
    );
    let p = ui.painter();
    p.circle_stroke(
        ring.center(),
        RING / 2.0,
        egui::Stroke::new(
            1.0,
            if on {
                shell.accent()
            } else {
                shell.border_strong()
            },
        ),
    );
    if on {
        p.circle_filled(ring.center(), 3.0, shell.accent());
    }
    p.text(
        egui::pos2(ring.max.x + space::SM, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        proportional(font_size::MICRO),
        shell.t1(),
    );
    resp
}

// ── Tabs（目录 UI/Tabs）───────────────────────────────────────────────

/// 页签条：容器 `bg_input` 底 + radius 4；选中页签 `bg_raised` 底 +
/// `t1` 字；其余 `t2`；`disabled` 下标用 `text_disabled`。
pub fn tabs(
    ui: &mut egui::Ui,
    shell: &UiShell,
    id: &str,
    labels: &[&str],
    selected: usize,
    disabled: &[usize],
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CONTROL_H),
        egui::Sense::hover(),
    );
    let p = ui.painter();
    p.rect_filled(rect, radius::FIELD, shell.input_bg());
    let each = rect.width() / labels.len().max(1) as f32;
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            egui::pos2(rect.min.x + each * i as f32, rect.min.y),
            egui::vec2(each, rect.height()),
        );
        let is_sel = i == selected;
        let is_dis = disabled.contains(&i);
        let resp = ui.interact(r, egui::Id::new((id, i)), egui::Sense::click());
        let p = ui.painter();
        if is_sel {
            p.rect_filled(r, radius::FIELD, shell.raised_bg());
        } else if resp.hovered() && !is_dis {
            p.rect_filled(r, radius::FIELD, shell.hover_bg());
        }
        let fg = if is_dis {
            shell.text_disabled()
        } else if is_sel {
            shell.t1()
        } else {
            shell.t2()
        };
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            proportional(font_size::SMALL),
            fg,
        );
        if resp.clicked() && !is_dis {
            clicked = Some(i);
        }
    }
    clicked
}

// ── Progress / ValueUnit ──────────────────────────────────────────────

/// 进度条（目录 `UI/Progress`）：标签行 + 4px 轨道。
pub fn progress(ui: &mut egui::Ui, shell: &UiShell, label: &str, fraction: f32) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(font_size::MICRO)
                .color(shell.t2()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{:.0}%", (fraction * 100.0).clamp(0.0, 100.0)))
                    .font(mono(font_size::MICRO))
                    .color(shell.t1()),
            );
        });
    });
    ui.add_space(space::XS);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, shell.input_bg());
    let w = rect.width() * fraction.clamp(0.0, 1.0);
    if w > 0.0 {
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height())),
            2.0,
            shell.accent(),
        );
    }
}

/// 只读数值 + 单位（目录 `UI/ValueUnit`）：等宽 20px 主值 + 11px 说明。
pub fn value_unit(ui: &mut egui::Ui, shell: &UiShell, value: &str, caption: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(value)
                .font(mono(20.0))
                .color(shell.t1()),
        );
        ui.label(
            egui::RichText::new(caption)
                .size(font_size::SMALL)
                .color(shell.t2()),
        );
    });
}

// ── FieldSelect（下拉 · ComboBox 封装）────────────────────────────────

/// 下拉选择：`selected = options[i]` 之一，绑定 `&mut T`。
pub struct FieldSelect<'a, T> {
    selected: &'a mut T,
    options: &'a [T],
    id_salt: String,
    fill_width: bool,
}

impl<'a, T: Copy + PartialEq> FieldSelect<'a, T> {
    /// 绑定当前值 / 选项切片 / egui 记忆 ID（作用域内唯一）。
    pub fn new(selected: &'a mut T, options: &'a [T], id_salt: &str) -> Self {
        Self {
            selected,
            options,
            id_salt: id_salt.to_owned(),
            fill_width: false,
        }
    }

    /// 撑满父容器宽度。
    pub fn fill_width(mut self, on: bool) -> Self {
        self.fill_width = on;
        self
    }

    /// 装配并落地。
    pub fn show<F>(self, ui: &mut egui::Ui, _shell: &UiShell, label_for: F)
    where
        F: Fn(&T) -> String,
    {
        let mut combo =
            egui::ComboBox::from_id_salt(&self.id_salt).selected_text(label_for(self.selected));
        if self.fill_width {
            combo = combo.width(ui.available_width());
        }
        combo.show_ui(ui, |ui| {
            for opt in self.options.iter() {
                ui.selectable_value(self.selected, *opt, label_for(opt));
            }
        });
    }
}

// ── Switch（目录 UI/Switch）───────────────────────────────────────────

/// 开关（目录 28×16）：on = accent_soft 底 + accent 描边 + accent 滑钮；
/// off = bg_input 底 + border_subtle 描边 + t2 滑钮；disabled 滑钮 t3。
pub struct Switch<'a> {
    on: &'a mut bool,
    size: egui::Vec2,
    enabled: bool,
}

impl<'a> Switch<'a> {
    /// 默认 28×16（目录）。
    pub fn new(on: &'a mut bool) -> Self {
        Self {
            on,
            size: egui::vec2(28.0, 16.0),
            enabled: true,
        }
    }

    /// 显式宽 × 高。
    pub fn size(mut self, s: egui::Vec2) -> Self {
        self.size = s;
        self
    }

    /// 禁用态（目录 `Switch/Disabled`）。
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// 装配并落地。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(self.size, egui::Sense::click());
        let p = ui.painter();
        let r = self.size.y / 2.0;
        let (fill, stroke, thumb) = if !self.enabled {
            (shell.input_bg(), shell.border_subtle(), shell.t3())
        } else if *self.on {
            (shell.accent_soft(), shell.accent(), shell.accent())
        } else {
            (shell.input_bg(), shell.border_subtle(), shell.t2())
        };
        p.rect_filled(rect, r, fill);
        p.rect_stroke(
            rect,
            r,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
        let knob = self.size.y - 4.0;
        let knob_x = if *self.on {
            rect.max.x - 2.0 - knob / 2.0
        } else {
            rect.min.x + 2.0 + knob / 2.0
        };
        p.circle_filled(egui::pos2(knob_x, rect.center().y), knob / 2.0, thumb);
        if resp.clicked() && self.enabled {
            *self.on = !*self.on;
        }
        resp
    }
}

// ── Slider（目录 UI/Slider：4px 轨道 + 6px 方钮）───────────────────────

const SLIDER_TRACK_H: f32 = 4.0;
const SLIDER_HANDLE: f32 = 6.0;
const SLIDER_HIT_H: f32 = 16.0;

/// 滑条色调（目录三变体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderTone {
    /// 默认：accent 蓝（选择语义）。
    Accent,
    /// 画笔：gold 金（仅画笔 / 播放语义）。
    Gold,
    /// 禁用：text_3 灰。
    Disabled,
}

fn slider_colors(shell: &UiShell, tone: SliderTone) -> (egui::Color32, egui::Color32) {
    match tone {
        SliderTone::Accent => (shell.accent(), shell.t1()),
        SliderTone::Gold => (shell.gold(), shell.t1()),
        SliderTone::Disabled => (shell.t3(), shell.t3()),
    }
}

fn slider_value_at_fraction(f: f32, min: f32, max: f32, logarithmic: bool) -> f32 {
    let f = f.clamp(0.0, 1.0);
    if logarithmic && min > 0.0 && max > min {
        min * (max / min).powf(f)
    } else {
        min + f * (max - min)
    }
}

fn slider_fraction_of_value(v: f32, min: f32, max: f32, logarithmic: bool) -> f32 {
    if max <= min {
        return 0.0;
    }
    let v = v.clamp(min, max);
    if logarithmic && min > 0.0 {
        ((v / min).ln() / (max / min).ln()).clamp(0.0, 1.0)
    } else {
        ((v - min) / (max - min)).clamp(0.0, 1.0)
    }
}

/// 滑条（目录几何）：整宽 4px 轨道 + 6×6 方钮（1px 同色描边）。
pub fn slider<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    logarithmic: bool,
) -> egui::Response {
    slider_toned(ui, shell, value, range, logarithmic, SliderTone::Accent)
}

fn slider_toned<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    logarithmic: bool,
    tone: SliderTone,
) -> egui::Response {
    let (fill_color, thumb_fill) = slider_colors(shell, tone);
    let enabled = tone != SliderTone::Disabled;
    let min = range.start().to_f64() as f32;
    let max = range.end().to_f64() as f32;
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SLIDER_HIT_H),
        if enabled {
            egui::Sense::click_and_drag()
        } else {
            egui::Sense::hover()
        },
    );
    let p = ui.painter();
    let mut next = value.to_f64() as f32;
    if enabled && (resp.dragged() || resp.clicked()) {
        if let Some(pos) = resp.interact_pointer_pos() {
            let frac = if rect.width() > 0.0 {
                (pos.x - rect.min.x) / rect.width()
            } else {
                0.0
            };
            next = slider_value_at_fraction(frac, min, max, logarithmic);
        }
    }
    if (next - value.to_f64() as f32).abs() > f32::EPSILON {
        *value = T::from_f64(next as f64);
        resp.mark_changed();
    }
    let frac = slider_fraction_of_value(next, min, max, logarithmic);
    let track_rect =
        egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), SLIDER_TRACK_H));
    p.rect_filled(track_rect, 2.0, shell.input_bg());
    if frac > 0.0 {
        let fill_end = track_rect.min.x + frac * track_rect.width();
        p.rect_filled(
            egui::Rect::from_two_pos(track_rect.min, egui::pos2(fill_end, track_rect.max.y)),
            2.0,
            fill_color,
        );
        let handle = egui::Rect::from_center_size(
            egui::pos2(fill_end, rect.center().y),
            egui::vec2(SLIDER_HANDLE, SLIDER_HANDLE),
        );
        p.rect_filled(handle, 1.0, thumb_fill);
        p.rect_stroke(
            handle,
            1.0,
            egui::Stroke::new(1.0, fill_color),
            egui::StrokeKind::Inside,
        );
    }
    resp
}

/// 滑条行（目录）：标签 t2 + 数值 mono t1 上行，下行整宽滑条（默认蓝）。
pub fn slider_row<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    formatter: impl Fn(f32) -> String,
    logarithmic: bool,
) -> egui::Response {
    slider_row_toned(
        ui,
        shell,
        label,
        value,
        range,
        formatter,
        logarithmic,
        SliderTone::Accent,
    )
}

/// 画笔滑条（目录 `Slider/Brush`：金色）。
pub fn slider_row_brush<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    formatter: impl Fn(f32) -> String,
    logarithmic: bool,
) -> egui::Response {
    slider_row_toned(
        ui,
        shell,
        label,
        value,
        range,
        formatter,
        logarithmic,
        SliderTone::Gold,
    )
}

/// 禁用滑条（目录 `Slider/Disabled`）。
pub fn slider_row_disabled<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    formatter: impl Fn(f32) -> String,
    logarithmic: bool,
) -> egui::Response {
    slider_row_toned(
        ui,
        shell,
        label,
        value,
        range,
        formatter,
        logarithmic,
        SliderTone::Disabled,
    )
}

#[allow(clippy::too_many_arguments)]
fn slider_row_toned<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    formatter: impl Fn(f32) -> String,
    logarithmic: bool,
    tone: SliderTone,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(font_size::MICRO)
                .color(shell.t2()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(formatter(value.to_f64() as f32))
                    .font(mono(font_size::MICRO))
                    .color(if tone == SliderTone::Disabled {
                        shell.text_disabled()
                    } else {
                        shell.t1()
                    }),
            );
        });
    });
    ui.add_space(space::XS);
    slider_toned(ui, shell, value, range, logarithmic, tone)
}

// ── PropertyRow / value_box（42:2 组件族 · 随目录色板更新）─────────────

const PROPERTY_LABEL_W: f32 = 84.0;
const VALUE_BOX_H: f32 = 28.0;
const VALUE_BOX_PAD_X: f32 = 8.0;

/// 只读值框（目录 `Input` 同壳）：`bg_input` 底 + `border_subtle` 描边 +
/// radius 4 + 等宽值文本；右端可挂单位后缀与下拉箭头。
pub fn value_box(
    ui: &mut egui::Ui,
    shell: &UiShell,
    text: &str,
    suffix: Option<&str>,
    dropdown: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), VALUE_BOX_H),
        egui::Sense::click(),
    );
    let p = ui.painter();
    p.rect_filled(rect, radius::FIELD, shell.input_bg());
    p.rect_stroke(
        rect,
        radius::FIELD,
        egui::Stroke::new(1.0, shell.border_subtle()),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(rect.min.x + VALUE_BOX_PAD_X, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        mono(font_size::MICRO),
        shell.t1(),
    );
    let mut right = rect.max.x - VALUE_BOX_PAD_X;
    if dropdown {
        right -= 10.0;
    }
    if let Some(sfx) = suffix {
        p.text(
            egui::pos2(right, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            sfx,
            proportional(font_size::MICRO),
            shell.t3(),
        );
    }
    let _ = p;
    if dropdown {
        let glyph = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - VALUE_BOX_PAD_X - 5.0, rect.center().y),
            egui::vec2(10.0, 10.0),
        );
        let _ = ui.put(glyph, icon_image(ICON_CHEVRON_DOWN, shell.t3(), 10.0));
    }
    resp
}

/// 属性行：定宽 label（t2，12px）+ 整宽 [`value_box`]。
pub struct PropertyRow<'a> {
    label: &'a str,
    value: &'a str,
    suffix: Option<&'a str>,
    dropdown: bool,
    label_width: f32,
}

impl<'a> PropertyRow<'a> {
    /// label + 等宽值文本。
    pub fn new(label: &'a str, value: &'a str) -> Self {
        Self {
            label,
            value,
            suffix: None,
            dropdown: false,
            label_width: PROPERTY_LABEL_W,
        }
    }

    /// 单位后缀。
    pub fn suffix(mut self, s: &'a str) -> Self {
        self.suffix = Some(s);
        self
    }

    /// 值框右端画下拉箭头。
    pub fn dropdown(mut self, on: bool) -> Self {
        self.dropdown = on;
        self
    }

    /// label 列宽（默认 84）。
    pub fn label_width(mut self, w: f32) -> Self {
        self.label_width = w;
        self
    }

    /// 装配并落地。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let mut resp: Option<egui::Response> = None;
        ui.horizontal(|ui| {
            let (lab_rect, _) = ui.allocate_exact_size(
                egui::vec2(self.label_width, VALUE_BOX_H),
                egui::Sense::hover(),
            );
            ui.painter().text(
                egui::pos2(lab_rect.min.x, lab_rect.center().y),
                egui::Align2::LEFT_CENTER,
                self.label,
                proportional(font_size::SMALL),
                shell.t2(),
            );
            resp = Some(value_box(ui, shell, self.value, self.suffix, self.dropdown));
        });
        resp.expect("horizontal 闭包必然执行一次")
    }
}

// ── dialog_window（模态壳）───────────────────────────────────────────

/// 模态对话框壳（居中 + Foreground + 原生标题栏 ×）。
pub struct DialogWindow<'a> {
    title: &'a str,
    open: &'a mut bool,
    width: f32,
    collapsible: bool,
    resizable: bool,
}

impl<'a> DialogWindow<'a> {
    /// 最小形态。
    pub fn new(title: &'a str, open: &'a mut bool) -> Self {
        Self {
            title,
            open,
            width: 460.0,
            collapsible: false,
            resizable: false,
        }
    }

    /// 显式宽度（默认 460）。
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// 标题栏折叠（默认 false）。
    pub fn collapsible(mut self, on: bool) -> Self {
        self.collapsible = on;
        self
    }

    /// 可缩放（默认 false）。
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// 装配并落地。
    pub fn show<F: FnOnce(&mut egui::Ui)>(self, ctx: &egui::Context, shell: &UiShell, body: F) {
        let mut open = *self.open;
        egui::Window::new(self.title)
            .open(&mut open)
            .collapsible(self.collapsible)
            .resizable(self.resizable)
            .default_width(self.width)
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                // 跟随应用当前日夜模式，而不是钉死暗色样式。
                egui::Frame::window(&ctx.style_of(ctx.theme()))
                    .fill(shell.panel_bg())
                    .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                    .inner_margin(crate::ui_theme::margin_all(space::MD))
                    .corner_radius(radius::PANEL),
            )
            .show(ctx, body);
        *self.open = open;
    }
}

// ── 测试 ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::test_support::shell;

    /// 本模块控件默认落在 320×400 的 headless 画布内。
    fn make_ui(name: &str) -> (egui::Context, egui::Ui) {
        crate::widgets::test_support::make_ui(name, 320.0, 400.0)
    }

    #[test]
    fn field_input_does_not_mutate_value_without_input() {
        let sh = shell();
        let (_c, mut ui) = make_ui("input_geom");
        let mut v = String::from("Zunyi");
        let r = FieldInput::new(&mut v, "hint")
            .fill_width(true)
            .show(&mut ui, &sh);
        assert!(r.rect.height() > 0.0, "编辑区应有高度");
        assert_eq!(v, "Zunyi", "未输入时值不变");
    }

    #[test]
    fn input_states_render_without_panic() {
        let sh = shell();
        for (name, error, enabled) in [("a", false, true), ("b", true, true), ("c", false, false)] {
            let (_c, mut ui) = make_ui(name);
            let mut v = String::from("x");
            let _ = FieldInput::new(&mut v, "hint")
                .error(error)
                .enabled(enabled)
                .show(&mut ui, &sh);
        }
    }

    #[test]
    fn slider_value_mapping_is_linear_and_logarithmic() {
        assert!((slider_value_at_fraction(0.5, 0.0, 1.0, false) - 0.5).abs() < 1e-6);
        assert!((slider_value_at_fraction(0.5, 1.0, 100.0, true) - 10.0).abs() < 1e-4);
        assert!((slider_fraction_of_value(10.0, 1.0, 100.0, true) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn slider_tones_select_catalogue_colors() {
        let sh = shell();
        assert_eq!(slider_colors(&sh, SliderTone::Accent).0, sh.accent());
        assert_eq!(slider_colors(&sh, SliderTone::Gold).0, sh.gold());
        assert_eq!(slider_colors(&sh, SliderTone::Disabled).0, sh.t3());
    }

    #[test]
    fn checkbox_states_and_radio_render_without_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui("check_states");
        for state in [
            CheckState::Unchecked,
            CheckState::Checked,
            CheckState::Indeterminate,
            CheckState::Disabled,
        ] {
            let r = checkbox_state(&mut ui, &sh, state, "Visible");
            assert!(r.rect.height() > 0.0);
        }
        let _ = radio(&mut ui, &sh, false, "World");
        let _ = radio(&mut ui, &sh, true, "Local");
    }

    #[test]
    fn check_row_toggles_only_on_click() {
        let sh = shell();
        let (_c, mut ui) = make_ui("check_row");
        let mut on = false;
        let _ = check_row(&mut ui, &sh, &mut on, "Visible on map", None);
        assert!(!on, "headless 无点击，状态不变");
    }

    #[test]
    fn tabs_and_option_menu_return_none_without_clicks() {
        let sh = shell();
        let (_c, mut ui) = make_ui("tabs_geom");
        let r = tabs(&mut ui, &sh, "t", &["Terrain", "Objects", "Roads"], 0, &[2]);
        assert!(r.is_none());
        let (_c2, mut ui2) = make_ui("menu");
        let r2 = option_menu(&mut ui2, &sh, "m", &["Government", "Settlement"], 0, &[1]);
        assert!(r2.is_none());
    }

    #[test]
    fn switch_defaults_match_catalogue_28x16() {
        let sh = shell();
        let (_c, mut ui) = make_ui("switch");
        let mut on = false;
        let r = Switch::new(&mut on).show(&mut ui, &sh);
        assert!(
            (r.rect.width() - 28.0).abs() < 1e-3,
            "got {}",
            r.rect.width()
        );
        assert!(
            (r.rect.height() - 16.0).abs() < 1e-3,
            "got {}",
            r.rect.height()
        );
        assert!(!on);
    }

    #[test]
    fn value_box_is_28_high_and_full_width() {
        let sh = shell();
        let (_c, mut ui) = make_ui("value_box");
        let r = value_box(&mut ui, &sh, "842", Some("m"), false);
        assert!(
            (r.rect.height() - 28.0).abs() < 1e-3,
            "got {}",
            r.rect.height()
        );
        assert!(
            (r.rect.width() - 320.0).abs() < 1e-3,
            "got {}",
            r.rect.width()
        );
    }

    #[test]
    fn property_row_renders_without_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui("property_row");
        let _ = PropertyRow::new("Elevation", "842")
            .suffix("m")
            .show(&mut ui, &sh);
    }

    #[test]
    fn search_numeric_text_area_and_progress_render_without_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui("composites");
        let mut q = String::new();
        let _ = search_input(&mut ui, &sh, &mut q, "Find a place…", "Ctrl+K");
        let mut n: f32 = 842.0;
        let _ = numeric_input(&mut ui, &sh, &mut n, "m", 1.0);
        let mut t = String::from("Regional government office.");
        let _ = text_area(&mut ui, &sh, &mut t, 3);
        progress(&mut ui, &sh, "Exporting terrain", 0.64);
        value_unit(&mut ui, &sh, "842", "m · Elevation");
        let _ = dropdown_box(&mut ui, &sh, "Government");
    }
}
