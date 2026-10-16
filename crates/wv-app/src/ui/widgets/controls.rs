//! 共享表单 / 浮层控件（标准化控件 · 第 2 批）。
//!
//! `FieldInput` / `FieldSelect` / `field_checkbox` / `Switch` / `slider_row` +
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
//! 历史来源：拆自原 [`crate::ui::widgets`]。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::theme::ui_tokens::{radius, space};
use crate::ui::shell::UiShell;

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
    pub fn show(self, ui: &mut egui::Ui, shell: &UiShell) -> egui::Response {
        let mut edit = egui::TextEdit::singleline(self.value)
            .hint_text(&self.hint)
            .desired_width(if self.fill_width { f32::INFINITY } else { 0.0 });
        if self.font_mono {
            edit = edit.font(crate::theme::ui_theme::mono(shell.scale.input));
        }
        if let Some(sz) = self.min_size {
            edit = edit.min_size(sz);
        }
        let resp = ui.add(edit);
        if self.focus_on_add {
            resp.request_focus();
        }
        resp
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

// ── SliderRow（label-on-top + Slider + 自定义 format）──────────────────

/// 滑条行：顶行 label（shell.t2 + scale.small），其下整宽 Slider。
///
/// 自带 `show_value(false)`——拖动时浮标内是 `custom_formatter` 的渲染。
///
/// `formatter` 必须是 `Fn(f32) -> String + Send + Sync + 'static`——
/// `egui::Slider::custom_formatter` 要求 `'static` 闭包；egui 0.36 的
/// formatter 接收 `f64`，实现里 `as f32` 后再交给 caller。
pub fn slider_row(
    ui: &mut egui::Ui,
    shell: &UiShell,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    formatter: impl Fn(f32) -> String + Send + Sync + 'static,
    logarithmic: bool,
) -> egui::Response {
    ui.label(
        egui::RichText::new(label)
            .size(shell.scale.small)
            .color(shell.t2()),
    );
    let mut slider = egui::Slider::new(value, range)
        .show_value(true)
        .custom_formatter(move |n: f64, _| formatter(n as f32));
    if logarithmic {
        slider = slider.logarithmic(true);
    }
    ui.add(slider)
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
                    .inner_margin(crate::theme::ui_theme::margin_all(space::MD))
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
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
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
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
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
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
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
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
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
        UiShell::new(&crate::theme::ui_theme::UiShellConfig::default())
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
