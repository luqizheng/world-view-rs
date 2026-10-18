//! 控件层测试脚手架（仅 `cfg(test)`）。
//!
//! 收敛 `button` / `controls` / `primitives` / `floater` / `segmented_control`
//! 各处重复手写的 `UiShell::new(&UiShellConfig::default())` 与
//! `ctx.begin_pass` + `egui::Ui::new(UiBuilder…)` 样板。仅测试期编译，不污染
//! 生产 API。

use bevy_egui::egui;

use crate::shell::UiShell;
use crate::ui_theme::UiShellConfig;

/// 默认外壳样式快照（Dark + Cozy），供各控件单测断言几何 / 配色。
pub(crate) fn shell() -> UiShell {
    UiShell::new(&UiShellConfig::default())
}

/// 构造一个已 `begin_pass` 的 headless `egui::Context`。
///
/// egui 的控件构造需要处于一帧 pass 之中；headless 测试用此 helper 起帧。
pub(crate) fn ctx() -> egui::Context {
    let c = egui::Context::default();
    c.begin_pass(Default::default());
    c
}

/// 在 `ctx` 内、`w×h` 矩形范围内构造一个 headless `egui::Ui`。
///
/// 复用同一个 `ctx` 可在一帧内连续摆放多个控件（见 button 工厂多形态测试）。
pub(crate) fn ui_in(ctx: &egui::Context, name: &str, w: f32, h: f32) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        egui::Id::new(name),
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(w, h),
        )),
    )
}

/// `ctx()` + `ui_in()` 的便捷组合，返回首帧已就绪的 `(Context, Ui)`。
pub(crate) fn make_ui(name: &str, w: f32, h: f32) -> (egui::Context, egui::Ui) {
    let c = ctx();
    let ui = ui_in(&c, name, w, h);
    (c, ui)
}
