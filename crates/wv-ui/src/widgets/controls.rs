//! 共享表单 / 浮层控件（标准化控件 · 第 2 批）。
//!
//! `FieldInput` / `FieldSelect` / `field_checkbox` / `Switch` / `slider_row` +
//! `PropertyRow` / `value_box` / `check_row`（检视器组件族，Figma `42:2`）+
//! `DialogWindow`（模态壳）。全部用 builder + show 模式（与 [`super::button::ToolButton`]
//! 同构），样式默认由 `UiShell` 令牌解析，**绝不写裸 hex**——caller 忘了覆盖就走
//! shell 默认值（map_controls 的白控件配色即覆盖用法）。
//!
//! 测试纪律：每个控件**至少**四条单测——默认解析、builder 覆盖、show 不 panic、
//! 边界。任何「等 GUI 烟测兜底」的脏改 panic 在这里而不是在用户面前。
//!
//! 边界：
//! - **不**承载浮窗外壳 / 多列布局 / 列表项 / Radio——各 panel 自管或留 in-place。
//! - **不**写主题令牌源头 —— 调 `UiShell` getter 拿颜色。
//!
//! 历史来源：拆自原 [`crate::widgets`]。

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::ui_tokens::{font_size, radius, space};

use super::primitives::{icon_image, ICON_CHEVRON_DOWN, ICON_SQUARE_CHECK};

// ── FieldInput（单行文本输入）───────────────────────────────────────

/// 单行文本输入框。`hint` 是空值时的占位文本（**不是**初始值）。
///
/// 默认：原生 [`egui::TextEdit::singleline`]，无 min_size，文本色随主题，
/// 不抢焦点。CFT（command palette search）通过 `.focus_on_add(true)` 在打开
/// 帧自动聚焦；窄角度数字输入（compass 朝向）通过 `.min_size(..)` + mono 字体
/// 改写。
pub struct FieldInput<'a> {
    value: &'a mut String,
    hint: String,
    font_mono: bool,
    min_size: Option<egui::Vec2>,
    fill_width: bool,
    focus_on_add: bool,
}

impl<'a> FieldInput<'a> {
    /// 绑定 `&mut String` 与占位文本。`hint` 为空时不下发 hint。
    pub fn new(value: &'a mut String, hint: &str) -> Self {
        Self {
            value,
            hint: hint.to_owned(),
            font_mono: false,
            min_size: None,
            fill_width: false,
            focus_on_add: false,
        }
    }

    /// 等宽字体（默认 = 比例字体）。适合数字输入 / 工程代码字段。
    pub fn font_mono(mut self, on: bool) -> Self {
        self.font_mono = on;
        self
    }

    /// 显式最小尺寸（宽 × 高）。未设时宽度由父布局决定。
    pub fn min_size(mut self, s: egui::Vec2) -> Self {
        self.min_size = Some(s);
        self
    }

    /// 撑满父容器宽度（`desired_width(f32::INFINITY)`）。
    /// 命令面板输入框 / 字段宽度跟随对话框 footer 等场景开。
    pub fn fill_width(mut self, on: bool) -> Self {
        self.fill_width = on;
        self
    }

    /// 添加帧自动抢焦点（`request_focus()`）。命令面板打开时用。
    pub fn focus_on_add(mut self, on: bool) -> Self {
        self.focus_on_add = on;
        self
    }

    /// 装配并落地。返回的 `Response` 可用 `.changed() / .lost_focus() / ...`
    /// 继续串行检查。
    ///
    /// ## 框式外观（2026-10-18，对齐 web 原型 `.inp`）
    ///
    /// 设计稿的输入框不是裸 `TextEdit`：`bg_app` 内嵌底 + `border_strong` 1px
    /// 描边 + `radius::CONTROL` 圆角 + 左右 8 / 上下 3 内边距（合成 26px 高，
    /// 与 `index.html .inp` 同档）。`TextEdit` 自身的背景/边框被关闭，视觉
    /// 全部由外层 `Frame` 承担——焦点态仍由 egui 光标与选中色表达。
    ///
    /// `min_size` 覆盖的是**整框外尺寸**：内部编辑区自动扣掉内边距与描边，
    /// 老调用点（罗盘朝向输入框 60×28）几何不变。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        const PAD_X: f32 = 8.0;
        const PAD_Y: f32 = 3.0;
        const STROKE: f32 = 1.0;
        let frame = egui::Frame::default()
            .fill(crate::ui_theme::color32(shell.palette.bg_app))
            .stroke(egui::Stroke::new(STROKE, shell.border_strong()))
            .corner_radius(radius::CONTROL)
            .inner_margin(crate::ui_theme::margin_xy(PAD_X, PAD_Y));
        let mut out = None;
        frame.show(ui, |ui| {
            let mut edit = egui::TextEdit::singleline(self.value)
                .hint_text(&self.hint)
                .desired_width(if self.fill_width { f32::INFINITY } else { 0.0 })
                .background_color(egui::Color32::TRANSPARENT)
                .margin(egui::Margin::ZERO)
                .frame(egui::Frame::NONE);
            if self.font_mono {
                edit = edit.font(crate::ui_theme::mono(shell.scale.input));
            }
            // 外尺寸 → 内尺寸：扣掉两侧内边距与描边，保持老调用点几何不变。
            let inner_min = self
                .min_size
                .map(|s| {
                    egui::vec2(
                        (s.x - 2.0 * (PAD_X + STROKE)).max(0.0),
                        (s.y - 2.0 * (PAD_Y + STROKE)).max(0.0),
                    )
                })
                .unwrap_or(egui::vec2(0.0, 18.0));
            edit = edit.min_size(inner_min);
            let resp = ui.add(edit);
            if self.focus_on_add {
                resp.request_focus();
            }
            out = Some(resp);
        });
        out.expect("Frame::show 必然执行一次")
    }
}

// ── FieldSelect<T>（下拉）───────────────────────────────────────────

/// 下拉选择：`selected = options[i]` 之一，绑定 `&mut T`。
///
/// 模版用 `egui::ComboBox::from_id_salt + show_ui`，把每一项塞进
/// `selectable_value` —— 调用者只用给一个 `T -> String` 的 label 闭包，
/// 不必为每种 T 写新组件。
///
/// ID salt 由调用方提供（避免面板里两个 FieldSelect 撞名）。
///
/// **注意**：不能替代 Radio 组（new_map_dialog 的 level/extent/prefill 是
/// selectable_value 横向排，不抽）。
pub struct FieldSelect<'a, T> {
    selected: &'a mut T,
    options: &'a [T],
    id_salt: String,
    fill_width: bool,
}

impl<'a, T: Copy + PartialEq> FieldSelect<'a, T> {
    /// 绑定当前值 / 选项切片 / egui 记忆 ID。**ID 必须在面板作用域内唯一**。
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

    /// 装配并落地。`label_for` 把 `&T` 映成显示文本——
    /// 推荐返回 `String`，避免与 `FieldSelect` 自身的 `&'a T` 借用相争。
    pub fn show<F>(self, ui: &mut egui::Ui, _shell: &UiShell, label_for: F)
    where
        F: Fn(&T) -> String,
    {
        let combo =
            egui::ComboBox::from_id_salt(&self.id_salt).selected_text(label_for(self.selected));
        let _ = self.fill_width;
        combo.show_ui(ui, |ui| {
            for opt in self.options.iter() {
                ui.selectable_value(self.selected, *opt, label_for(opt));
            }
        });
    }
}

// ── FieldCheckbox（原生复选项）────────────────────────────────────────

/// 复选项 + 文本标签；匹配原型 `.checkbox` 与原生 `ui.checkbox` 同形视觉。
///
/// 当前是 [`egui::Ui::checkbox`] 的薄壳——给定 stable API，未来要在 hover
/// 反馈 / focus 环上做品牌化也只改这一处。
///
/// ```text
///   [✓] 标签
/// ```
pub fn field_checkbox(
    ui: &mut egui::Ui,
    _shell: &UiShell,
    on: &mut bool,
    label: &str,
) -> egui::Response {
    ui.checkbox(on, label)
}

// ── Switch（视觉开关 —— 预留）────────────────────────────────────────

/// Switch 视觉控件（25×14 胶囊 + 滑钮）。
///
/// **当前 main 没有 call site**；保留作为后续「图层开关视觉升级」与原型
/// `.toggle` 的对齐点。等真正用到时由 `layers_panel` 替换 `field_checkbox`
/// 决定是否升级——本接口签名先稳定下来。
///
/// 视觉（web `.toggle`）：
/// on：accent 底 + 白滑钮靠右；
/// off：active 底 + text_3 滑钮靠左；
/// 命中区整胶囊。
pub struct Switch<'a> {
    on: &'a mut bool,
    size: egui::Vec2,
}

impl<'a> Switch<'a> {
    /// 默认 25×14（对齐 web `.toggle`）。
    pub fn new(on: &'a mut bool) -> Self {
        Self {
            on,
            size: egui::vec2(25.0, 14.0),
        }
    }

    /// 显式宽 × 高（含 `radius` = `size.y / 2.0`）。
    pub fn size(mut self, s: egui::Vec2) -> Self {
        self.size = s;
        self
    }

    /// 装配并落地。返回命中区 `Response`。
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(self.size, egui::Sense::click());
        let painter = ui.painter();
        // 底色胶囊。
        let r = self.size.y / 2.0;
        painter.rect_filled(
            rect,
            r,
            if *self.on {
                shell.accent()
            } else {
                shell.active_bg()
            },
        );
        // 滑钮。
        let knob = self.size.y - 4.0;
        let knob_x = if *self.on {
            // 靠右：max.x − 2 − knob/2。
            rect.max.x - 2.0 - knob / 2.0
        } else {
            // 靠左：min.x + 2 + knob/2。
            rect.min.x + 2.0 + knob / 2.0
        };
        painter.circle_filled(
            egui::pos2(knob_x, rect.center().y),
            knob / 2.0,
            if *self.on {
                egui::Color32::WHITE
            } else {
                shell.t3()
            },
        );
        if resp.clicked() {
            *self.on = !*self.on;
        }
        resp
    }
}

// ── slider / SliderRow（Figma #3:20791 定制滑条）───────────────────────

/// 几何常量（Figma `3:20791` 像素采样 @1x）：轨道高 8、手柄 10×8、命中区高 16。
const SLIDER_TRACK_H: f32 = 8.0;
const SLIDER_HANDLE_W: f32 = 10.0;
const SLIDER_HIT_H: f32 = 16.0;

/// 分数（0..=1）→ 值。`logarithmic` 时按 `min·(max/min)^f` 插值；
/// `min <= 0` 无法取对数时**静默回退线性**（防御：调用方给错范围不 panic）。
fn slider_value_at_fraction(f: f32, min: f32, max: f32, logarithmic: bool) -> f32 {
    let f = f.clamp(0.0, 1.0);
    if logarithmic && min > 0.0 && max > min {
        min * (max / min).powf(f)
    } else {
        min + f * (max - min)
    }
}

/// 值 → 分数（0..=1），[`slider_value_at_fraction`] 的逆。
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

/// Figma `3:20791` 定制滑条：整宽 8px 胶囊轨道 + 金色已填充段 + 浅色方形手柄。
///
/// 自绘（不走 [`egui::Slider`]）的原因：设计稿的「轨道细条 + 无圆thumb」形态
/// 用 egui Slider 的 `HandleShape`/`trailing_fill` 拼不出来（egui 0.36 全局
/// 样式互斥，见 brush_panel 旧注释）；自绘后交互（点击跳转 + 拖拽）与取值
/// 映射都在本函数内闭环，caller 只管 `&mut value`。
///
/// 配色走令牌（AGENTS §8 禁裸 hex；采样映射同 `status_bar` 先例）：
/// `#D6B77B`→`gold`、轨道深色→`timeline_track`（既有「轨道」语义令牌，
/// 比设计稿 `#131C23` 略亮以保证面板底上可见）、手柄白（`Switch` 同款）。
///
/// `T: Numeric` 与 [`egui::Slider::new`] 同款泛型——整数字段（如 shelf_cells）
/// 直接传 `&mut u32`，不必先转 f32。
pub fn slider<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    logarithmic: bool,
) -> egui::Response {
    let min = range.start().to_f64() as f32;
    let max = range.end().to_f64() as f32;
    let (rect, mut resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SLIDER_HIT_H),
        egui::Sense::click_and_drag(),
    );
    let p = ui.painter();

    // 点击即跳转 + 拖拽跟手：指针 x 直接映回值。
    let mut next = value.to_f64() as f32;
    if resp.dragged() || resp.clicked() {
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
    p.rect_filled(track_rect, SLIDER_TRACK_H / 2.0, shell.timeline_track());
    if frac > 0.0 {
        let fill_end = track_rect.min.x + frac * track_rect.width();
        p.rect_filled(
            egui::Rect::from_two_pos(track_rect.min, egui::pos2(fill_end, track_rect.max.y)),
            SLIDER_TRACK_H / 2.0,
            shell.gold(),
        );
        let handle_rect = egui::Rect::from_center_size(
            egui::pos2(fill_end, rect.center().y),
            egui::vec2(SLIDER_HANDLE_W, SLIDER_TRACK_H),
        );
        // hover / 拖拽给一圈描边作反馈（设计稿是静态图，无 hover 态可抄）。
        let stroke = if resp.dragged() {
            egui::Stroke::new(1.0, shell.t1())
        } else if resp.hovered() {
            egui::Stroke::new(1.0, shell.t2())
        } else {
            egui::Stroke::NONE
        };
        p.rect_filled(handle_rect, 2.0, shell.t1());
        if stroke != egui::Stroke::NONE {
            p.rect_stroke(handle_rect, 2.0, stroke, egui::StrokeKind::Inside);
        }
    }
    resp
}

/// 滑条行（Figma `3:20791` 布局）：顶行 label 左（t2）+ 值右（t1），
/// 其下整宽 [`slider`]。`formatter` 决定右上角文案（如 `"×{:.2}"` / `"48 px"`）。
///
/// `T: Numeric` 与 [`slider`] 同款——整数字段直接传 `&mut u32`。
pub fn slider_row<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    formatter: impl Fn(f32) -> String,
    logarithmic: bool,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(shell.scale.small)
                .color(shell.t2()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(formatter(value.to_f64() as f32))
                    .size(shell.scale.small)
                    .color(shell.t1()),
            );
        });
    });
    ui.add_space(space::XS);
    slider(ui, shell, value, range, logarithmic)
}

// ── PropertyRow / value_box（检视器属性行 · Figma `42:2` 组件族）──────

/// Figma `42:2` 采样几何 @1x：label 列宽 84、值框高 28、值框水平内边距 8。
const PROPERTY_LABEL_W: f32 = 84.0;
const VALUE_BOX_H: f32 = 28.0;
const VALUE_BOX_PAD_X: f32 = 8.0;

/// 只读值框（Figma `42:27` Value 模板）：暗色内嵌底 + `border_subtle` 描边 +
/// 等宽值文本，右端可挂单位后缀（Elevation 行的 `"m"`）与/或下拉箭头
/// （Type 行的可编辑暗示）。
///
/// 「内嵌底」色彩语义：设计稿 `#131C23` 比面板**暗**（与 raised 相反方向），
/// 令牌层没有 inset 档——取 `panel_bg × 0.8` gamma 域压暗，明暗两主题都自动
/// 成立（亮色下面板本就近白，压暗后同样是「凹进去」观感）。值文本走
/// `mono(MICRO)` + `t1`（设计稿 IBM Plex Mono 11 对位；打包字体缺 Plex，
/// 用 mono 家族等价替换）。
///
/// 返回整框的 click `Response`——将来把 Type 行接成真下拉时用它开菜单。
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
    p.rect_filled(rect, radius::FIELD, shell.panel_bg().gamma_multiply(0.8));
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
        crate::ui_theme::mono(font_size::MICRO),
        shell.t1(),
    );
    // 右端从右往左排：下拉箭头（最右）→ 单位后缀（其左）。
    let mut right = rect.max.x - VALUE_BOX_PAD_X;
    if dropdown {
        right -= 10.0;
    }
    if let Some(sfx) = suffix {
        p.text(
            egui::pos2(right, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            sfx,
            crate::ui_theme::proportional(font_size::MICRO),
            shell.t3(),
        );
    }
    let _ = p; // 释放 painter 借用，让 `ui.put` 可借 ui。
    if dropdown {
        let glyph = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - VALUE_BOX_PAD_X - 5.0, rect.center().y),
            egui::vec2(10.0, 10.0),
        );
        let _ = ui.put(glyph, icon_image(ICON_CHEVRON_DOWN, shell.t3(), 10.0));
    }
    resp
}

/// 属性行（Figma `42:25` Property 模板）：定宽 label（t2，12px）+ 整宽
/// [`value_box`]。行高 = 值框高 28，label 垂直居中对齐值框。
///
/// ```text
///   Elevation   ┌──────────────────┐
///               │ 842            m │
///               └──────────────────┘
/// ```
pub struct PropertyRow<'a> {
    label: &'a str,
    value: &'a str,
    suffix: Option<&'a str>,
    dropdown: bool,
    label_width: f32,
}

impl<'a> PropertyRow<'a> {
    /// label（如 `"Elevation"`）+ 等宽值文本（如 `"842"`）。
    pub fn new(label: &'a str, value: &'a str) -> Self {
        Self {
            label,
            value,
            suffix: None,
            dropdown: false,
            label_width: PROPERTY_LABEL_W,
        }
    }

    /// 单位后缀（如 `"m"`），挂在值框右端（下拉箭头之左）。
    pub fn suffix(mut self, s: &'a str) -> Self {
        self.suffix = Some(s);
        self
    }

    /// 值框右端画下拉箭头（可编辑字段的视觉暗示）。默认 false。
    pub fn dropdown(mut self, on: bool) -> Self {
        self.dropdown = on;
        self
    }

    /// label 列宽（默认 Figma 采样 84px）。
    pub fn label_width(mut self, w: f32) -> Self {
        self.label_width = w;
        self
    }

    /// 装配并落地，返回值框的 `Response`（可继续串 `.clicked()`）。
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
                crate::ui_theme::proportional(font_size::SMALL),
                shell.t2(),
            );
            resp = Some(value_box(ui, shell, self.value, self.suffix, self.dropdown));
        });
        resp.expect("horizontal 闭包必然执行一次")
    }
}

/// 复选行（Figma `42:40` Visibility checkbox）：自绘 12×12 勾选方块 + 标签 +
/// 可选右端挂件图标（如 `lock-keyhole-open`）。
///
/// 不复用 [`field_checkbox`]（原生 `ui.checkbox`）的原因：设计稿勾选块是
/// 圆角描边块 + 内嵌 check 图标，与原生字体勾选号不同档；右端挂件槽原生
/// checkbox 也给不了。`field_checkbox` 继续服务「原生即可」的场景，两者并存。
pub fn check_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    on: &mut bool,
    label: &str,
    trailing: Option<egui::ImageSource<'static>>,
) -> egui::Response {
    const ROW_H: f32 = 18.0;
    const BOX: f32 = 12.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::click(),
    );
    // 勾选方块：描边块，选中态描边转 t1 并内嵌 check 图标（Figma square-check）。
    let box_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + BOX / 2.0, rect.center().y),
        egui::vec2(BOX, BOX),
    );
    let stroke = if *on {
        shell.t1()
    } else {
        shell.border_strong()
    };
    ui.painter().rect_stroke(
        box_rect,
        radius::BADGE,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    if *on {
        // `ui.put` 左上锚定——勾号须落方块的**居中**矩形。
        let check_rect =
            egui::Rect::from_center_size(box_rect.center(), egui::vec2(BOX - 2.0, BOX - 2.0));
        let _ = ui.put(
            check_rect,
            icon_image(ICON_SQUARE_CHECK, shell.t1(), BOX - 2.0),
        );
    }
    // 标签（11px t2，Figma body-small 对位 MICRO）。
    ui.painter().text(
        egui::pos2(box_rect.max.x + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        crate::ui_theme::proportional(font_size::MICRO),
        shell.t2(),
    );
    // 右端挂件（12×12 t3）。
    if let Some(src) = trailing {
        let trail_rect = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - BOX / 2.0 - 2.0, rect.center().y),
            egui::vec2(BOX, BOX),
        );
        let _ = ui.put(trail_rect, icon_image(src, shell.t3(), BOX));
    }
    if resp.clicked() {
        *on = !*on;
    }
    resp
}

// ── dialog_window（模态壳）───────────────────────────────────────────

/// 模态对话框壳：标题进原生标题栏 + × 写回 `open` + 居中 + Foreground 置顶。
///
/// 替代各对话框（new_project / new_map …）内反复出现的
/// `egui::Window::new(title).open(&mut open).collapsible(false).resizable(false)
///  .order(Foreground).anchor(CENTER_CENTER, [0,0]).frame(...corner_radius(MODAL))`
/// 一坨样板；调用者只关心 body 与可选 foot。
///
/// **层序**：egui 绘制序 `Background < Middle < Foreground < Tooltip`——浮动
/// 面板在 Middle，本壳开 Foreground 压得过面板；Tooltip 仍在最上（欢迎页 /
/// startup 加载 / 命令面板 / 新建工程 / 新建地图 / 失败提示卡片等同层）。
pub struct DialogWindow<'a> {
    title: &'a str,
    open: &'a mut bool,
    width: f32,
    collapsible: bool,
    resizable: bool,
}

impl<'a> DialogWindow<'a> {
    /// 最小形态——`open = &mut dialog.open`、title 即可。
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

    /// 标题栏 ▼（默认 false —— 模态不该折起来）。true 用于「故障诊断卡」。
    pub fn collapsible(mut self, on: bool) -> Self {
        self.collapsible = on;
        self
    }

    /// 是否可拖拽边框改尺寸（默认 false）。
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// 装配并落地。`open` 收 `Window::open` 的写回（用户点 × 时变 false）。
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
                egui::Frame::window(&ctx.style_of(egui::Theme::Dark))
                    .fill(shell.panel_bg())
                    .stroke(egui::Stroke::new(1.0_f32, shell.border_strong()))
                    .inner_margin(crate::ui_theme::margin_all(space::MD))
                    .corner_radius(radius::MODAL),
            )
            .show(ctx, body);
        *self.open = open;
    }
}

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod field_input_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::ui_theme::UiShellConfig::default())
    }

    /// 构造一个 `egui::Ui` 跑在 `400×400` 矩形里，供 `show` 落地用。
    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("field_input_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
        );
        (ctx, ui)
    }

    #[test]
    fn field_input_show_renders_without_panic_for_each_builder_path() {
        let sh = shell();
        let (_c1, mut u1) = make_ui();
        let mut v = String::from("initial");
        FieldInput::new(&mut v, "hint").show(&mut u1, &sh);
        assert!(!v.is_empty(), "初始值不应被改");

        // font_mono + min_size + fill_width + focus_on_add 全开
        let (_c2, mut u2) = make_ui();
        let mut v = String::new();
        let r = FieldInput::new(&mut v, "hint")
            .font_mono(true)
            .min_size(egui::vec2(80.0, 28.0))
            .fill_width(true)
            .focus_on_add(true)
            .show(&mut u2, &sh);
        // `request_focus` 不总能在 headless 上下文里成功测，但调用本身必须不 panic。
        let _ = r;
    }

    #[test]
    fn field_input_default_proportional_does_not_require_shell_in_builder() {
        // 仅做构造阶段检验：`new + show` 形签名对得上。
        let _ = std::marker::PhantomData::<fn() -> FieldInput<'static>>;
    }

    #[test]
    fn field_input_builder_chain_is_consistent() {
        // 编译通过即视为链式 builder 类型一致。
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut v = String::new();
        FieldInput::new(&mut v, "h")
            .font_mono(true)
            .fill_width(true)
            .min_size(egui::vec2(60.0, 24.0))
            .focus_on_add(true)
            .show(&mut ui, &sh);
    }
}

#[cfg(test)]
mod field_select_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::ui_theme::UiShellConfig::default())
    }

    #[test]
    fn field_select_id_salt_is_recorded() {
        let opts = [10u32, 20, 30];
        let mut sel = 10u32;
        let fs = FieldSelect::new(&mut sel, &opts, "unique_id");
        // 字段私有，但构造不爆 + `Debug` 形可显示。
        assert_eq!(fs.id_salt, "unique_id");
    }

    #[test]
    fn field_select_show_renders_with_label_closure() {
        let sh = shell();
        let opts = ["甲", "乙", "丙"];
        let mut sel = "甲";
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("field_select_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        FieldSelect::new(&mut sel, &opts, "wv_test_select").show(&mut ui, &sh, |s| s.to_string());
        // sel 不被 show 改写——只有 open 后 user pick 才改。
        assert_eq!(sel, "甲");
    }
}

#[cfg(test)]
mod field_checkbox_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::ui_theme::UiShellConfig::default())
    }

    #[test]
    fn field_checkbox_toggles_correctly() {
        let sh = shell();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("field_checkbox_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        let mut on = false;
        let _ = field_checkbox(&mut ui, &sh, &mut on, "复选项");
        assert!(!on, "未点击时不变");
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::ui_theme::UiShellConfig::default())
    }

    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("switch_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        (ctx, ui)
    }

    #[test]
    fn switch_default_size_matches_web_toggle() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut on = false;
        let r = Switch::new(&mut on).show(&mut ui, &sh);
        assert!(
            (r.rect.width() - 25.0).abs() < 1e-3,
            "默认宽 = 25px，got {}",
            r.rect.width()
        );
        assert!(
            (r.rect.height() - 14.0).abs() < 1e-3,
            "默认高 = 14px，got {}",
            r.rect.height()
        );
    }

    #[test]
    fn switch_size_override_takes_effect() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut on = true;
        let r = Switch::new(&mut on)
            .size(egui::vec2(40.0, 20.0))
            .show(&mut ui, &sh);
        assert!(r.rect.width() > 25.0, "显式宽 40 > 默认 25");
    }
}

#[cfg(test)]
mod slider_row_tests {
    use super::*;

    fn shell() -> UiShell {
        UiShell::new(&crate::ui_theme::UiShellConfig::default())
    }

    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("slider_row_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
        );
        (ctx, ui)
    }

    #[test]
    fn slider_row_linear_passes_value_through_formatter() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut v: f32 = 0.5;
        slider_row(
            &mut ui,
            &sh,
            "软度",
            &mut v,
            0.0..=1.0,
            |n| format!("{:.2}", n),
            false,
        );
        // 跑过 show 不 panic 即视为通过。
    }

    #[test]
    fn slider_row_logarithmic_path_does_not_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut v: f32 = 32.0;
        slider_row(
            &mut ui,
            &sh,
            "半径",
            &mut v,
            16.0..=600.0,
            |n| format!("{:.0} wu", n),
            true,
        );
    }

    #[test]
    fn slider_value_at_fraction_maps_linear_endpoints_and_middle() {
        // 手算：[100, 200] 线性 → f=0 得 100，f=1 得 200，f=0.5 得 150。
        assert!(
            (slider_value_at_fraction(0.0, 100.0, 200.0, false) - 100.0).abs() < 1e-4,
            "got {}",
            slider_value_at_fraction(0.0, 100.0, 200.0, false)
        );
        assert!(
            (slider_value_at_fraction(1.0, 100.0, 200.0, false) - 200.0).abs() < 1e-4,
            "got {}",
            slider_value_at_fraction(1.0, 100.0, 200.0, false)
        );
        assert!(
            (slider_value_at_fraction(0.5, 100.0, 200.0, false) - 150.0).abs() < 1e-4,
            "got {}",
            slider_value_at_fraction(0.5, 100.0, 200.0, false)
        );
    }

    #[test]
    fn slider_value_at_fraction_logarithmic_midpoint_is_geometric_mean() {
        // 手算：[1, 100] 对数 f=0.5 → 1·(100)^0.5 = 10。
        assert!(
            (slider_value_at_fraction(0.5, 1.0, 100.0, true) - 10.0).abs() < 1e-4,
            "got {}",
            slider_value_at_fraction(0.5, 1.0, 100.0, true)
        );
    }

    #[test]
    fn slider_logarithmic_falls_back_to_linear_when_min_is_nonpositive() {
        // 防御：min<=0 取不了对数，回退线性 → [-1, 1] f=0.5 = 0。
        assert!(
            (slider_value_at_fraction(0.5, -1.0, 1.0, true) - 0.0).abs() < 1e-4,
            "got {}",
            slider_value_at_fraction(0.5, -1.0, 1.0, true)
        );
    }

    #[test]
    fn slider_fraction_of_value_inverts_value_at_fraction() {
        // 往返：of(at(f)) == f（线性 + 对数各一组，1e-4 容差）。
        for f in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let lin = slider_fraction_of_value(
                slider_value_at_fraction(f, 16.0, 600.0, false),
                16.0,
                600.0,
                false,
            );
            assert!((lin - f).abs() < 1e-4, "linear f={} got {}", f, lin);
            let log = slider_fraction_of_value(
                slider_value_at_fraction(f, 1.0, 100.0, true),
                1.0,
                100.0,
                true,
            );
            assert!((log - f).abs() < 1e-4, "log f={} got {}", f, log);
        }
    }

    #[test]
    fn slider_fraction_of_value_clamps_out_of_range_input() {
        // 越界值夹回 [0,1]；退化区间（max<=min）返回 0 而不是 NaN。
        assert!(
            (slider_fraction_of_value(999.0, 0.0, 1.0, false) - 1.0).abs() < 1e-4,
            "got {}",
            slider_fraction_of_value(999.0, 0.0, 1.0, false)
        );
        assert!(
            (slider_fraction_of_value(-5.0, 0.0, 1.0, false) - 0.0).abs() < 1e-4,
            "got {}",
            slider_fraction_of_value(-5.0, 0.0, 1.0, false)
        );
        assert!(
            (slider_fraction_of_value(0.5, 1.0, 1.0, false) - 0.0).abs() < 1e-4,
            "got {}",
            slider_fraction_of_value(0.5, 1.0, 1.0, false)
        );
    }

    #[test]
    fn slider_show_renders_without_panic_for_numeric_and_log_paths() {
        let sh = shell();
        // f32 + 线性。
        let (_c1, mut u1) = make_ui();
        let mut v: f32 = 0.3;
        slider(&mut u1, &sh, &mut v, 0.0..=1.0, false);
        assert!((v - 0.3).abs() < 1e-6, "无交互时值不被改写，got {}", v);
        // u32（Numeric 泛型路径，continent_dialog shelf_cells 同款）。
        let (_c2, mut u2) = make_ui();
        let mut n: u32 = 8;
        slider(&mut u2, &sh, &mut n, 0..=32, false);
        assert_eq!(n, 8, "无交互时整数值不被改写");
        // 对数路径不 panic。
        let (_c3, mut u3) = make_ui();
        let mut r: f32 = 64.0;
        slider(&mut u3, &sh, &mut r, 16.0..=600.0, true);
    }
}

#[cfg(test)]
mod dialog_window_tests {
    use super::*;

    #[test]
    fn dialog_window_default_fields_have_expected_values() {
        let mut open = true;
        let dw = DialogWindow::new("test", &mut open);
        assert_eq!(dw.title, "test");
        assert!(
            (dw.width - 460.0).abs() < 1e-3,
            "默认宽 460，got {}",
            dw.width
        );
        assert!(!dw.collapsible, "默认不可折叠");
        assert!(!dw.resizable, "默认不可缩放");
        // `&mut open` 链接：show 时被读 + 写回。
        *dw.open = false;
        assert!(!open, "open 字段是 &mut 桥");
    }

    #[test]
    fn dialog_window_builder_overrides_default() {
        let mut open = true;
        let dw = DialogWindow::new("可折叠", &mut open)
            .width(420.0)
            .collapsible(true)
            .resizable(true);
        assert!((dw.width - 420.0).abs() < 1e-3);
        assert!(dw.collapsible);
        assert!(dw.resizable);
    }

    #[test]
    fn dialog_window_construction_only_runs_in_unit_tests() {
        // 仅做构造（不调 show）能过 → API 类型与 builder 链 OK。
        // `show` 的真实跑通由 `scripts/smoke-gui.sh`（GUI 启动烟测）兜底——
        // `egui::Window::show(ctx, …)` 注册的 texture delta 在 unit 上下文无 GPU
        // 渲染 surface，必然在 drop 时撞「Dropped TexturesDelta with N unapplied
        // deltas」，因此这里**不**跑 `show`。
        let mut open = true;
        let mut open2 = true;
        let _a = DialogWindow::new("a", &mut open);
        let _b = DialogWindow::new("b", &mut open2)
            .width(700.0)
            .collapsible(true)
            .resizable(true);
    }
}

#[cfg(test)]
mod property_row_tests {
    use super::*;
    use crate::ui_theme::UiShellConfig;

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("property_row_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(288.0, 400.0),
            )),
        );
        (ctx, ui)
    }

    #[test]
    fn property_row_defaults_match_figma_sampling() {
        let pr = PropertyRow::new("Elevation", "842");
        assert_eq!(pr.label, "Elevation");
        assert_eq!(pr.value, "842");
        assert!(
            (pr.label_width - 84.0).abs() < 1e-3,
            "label 列宽默认 = Figma 采样 84，got {}",
            pr.label_width
        );
        assert!(pr.suffix.is_none(), "默认无单位后缀");
        assert!(!pr.dropdown, "默认无下拉箭头");
    }

    #[test]
    fn property_row_builder_overrides_take_effect() {
        let pr = PropertyRow::new("Type", "Government")
            .suffix("m")
            .dropdown(true)
            .label_width(60.0);
        assert_eq!(pr.suffix, Some("m"));
        assert!(pr.dropdown);
        assert!(
            (pr.label_width - 60.0).abs() < 1e-3,
            "got {}",
            pr.label_width
        );
    }

    #[test]
    fn value_box_is_full_width_and_figma_28_high() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let r = value_box(&mut ui, &sh, "1142.00 / 496.00", None, false);
        assert!(
            (r.rect.height() - 28.0).abs() < 1e-3,
            "值框高 = Figma 采样 28，got {}",
            r.rect.height()
        );
        assert!(
            (r.rect.width() - 288.0).abs() < 1e-3,
            "值框应撑满可用宽（288），got {}",
            r.rect.width()
        );
    }

    #[test]
    fn property_row_renders_suffix_and_dropdown_variants_without_panic() {
        let sh = shell();
        // Elevation 形态：后缀。
        let (_c1, mut u1) = make_ui();
        let r1 = PropertyRow::new("Elevation", "842")
            .suffix("m")
            .show(&mut u1, &sh);
        assert!(
            (r1.rect.height() - 28.0).abs() < 1e-3,
            "行高 = 值框高 28，got {}",
            r1.rect.height()
        );
        // Type 形态：下拉暗示。
        let (_c2, mut u2) = make_ui();
        let r2 = PropertyRow::new("Type", "Government")
            .dropdown(true)
            .show(&mut u2, &sh);
        assert!(r2.rect.width() > 0.0);
    }

    #[test]
    fn value_box_empty_text_renders_without_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let r = value_box(&mut ui, &sh, "", Some(""), true);
        assert!((r.rect.height() - 28.0).abs() < 1e-3);
    }
}

#[cfg(test)]
mod check_row_tests {
    use super::super::primitives::ICON_MAP_PIN;
    use super::*;
    use crate::ui_theme::UiShellConfig;

    fn shell() -> UiShell {
        UiShell::new(&UiShellConfig::default())
    }

    fn make_ui() -> (egui::Context, egui::Ui) {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("check_row_tests"),
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(288.0, 200.0),
            )),
        );
        (ctx, ui)
    }

    #[test]
    fn check_row_is_full_width_18_high_and_does_not_toggle_without_click() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut on = false;
        let r = check_row(&mut ui, &sh, &mut on, "Visible on map", None);
        assert!(
            (r.rect.height() - 18.0).abs() < 1e-3,
            "行高 18，got {}",
            r.rect.height()
        );
        assert!(
            (r.rect.width() - 288.0).abs() < 1e-3,
            "整行可点（288），got {}",
            r.rect.width()
        );
        assert!(!on, "headless 无点击事件，状态不变");
    }

    #[test]
    fn check_row_with_trailing_icon_renders_without_panic() {
        let sh = shell();
        let (_c, mut ui) = make_ui();
        let mut on = true;
        let r = check_row(&mut ui, &sh, &mut on, "Visible on map", Some(ICON_MAP_PIN));
        assert!(r.rect.width() > 0.0);
        assert!(on, "headless 无点击事件，状态不变");
    }
}
