//! 应用层 → 渲染层的桥（ADR-0020）：把 UI 外壳令牌喂给渲染的窄通道。
//!
//! 渲染层（`wv-render`）不持有 UI 令牌源头（`UiShellConfig` / `ShellPalette`
//! 是应用层关注点），但渲染**读**令牌投影——ADR-0020 §2.6 的分界是：
//! 应用层每帧把令牌灌进渲染侧的 Resource（如 [`wv_render::drill::DrillMarkerPalette`]），
//! 渲染只读自己的资源。本模块就是那些「灌」系统与色值换算的家。
//!
//! ## 边界（不做什么）
//! - **不**构造渲染实体 / 不排 Z 序——那是 wv-render 的事；
//! - **不**持有编辑链——翻译器在 [`crate::edit::intent_translator`]（ADR-0020 §2.5）。

use bevy::prelude::*;

use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::{ShellPalette, UiTheme};
use crate::ui::shell::UiShell;
use wv_render::drill::DrillMarkerPalette;

/// 每帧把外壳令牌灌进钻入标记调色（ADR-0020 §2.6）。
///
/// 注册时用 `.before(wv_render::drill::draw_drill_markers)`——绘制只读本资源，
/// 首帧即取到真值（`DrillMarkerPalette::default()` 的白色只是占位，永不显示）。
pub fn sync_drill_marker_palette(
    shell_cfg: Res<UiShellConfig>,
    mut palette: ResMut<DrillMarkerPalette>,
) {
    let shell = UiShell::new(&shell_cfg);
    palette.accent = color32_to_bevy(shell.accent());
    palette.visited = color32_to_bevy(shell.t3());
}

/// egui `Color32` → Bevy `Color`（gizmo 用的世界色）。
///
/// 原 `wv_render::drill::to_bevy_color`，ADR-0020 随令牌源头一起迁回应用层。
pub fn color32_to_bevy(c: bevy_egui::egui::Color32) -> Color {
    Color::srgb_u8(c.r(), c.g(), c.b())
}

/// 当前主题下的界面强调色（供钻入标记之外的调用者复用）。
///
/// 原 `wv_render::drill::accent_for`，ADR-0020 迁入——它读 `UiTheme`
/// （`ui_tokens`，应用层关注点）。
pub fn accent_for(theme: UiTheme) -> Color {
    let c = ShellPalette::for_theme(theme).accent.to_color32();
    color32_to_bevy(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原 wv-render::drill 的同名测试（ADR-0020 随实现迁入）：
    /// 两套主题的界面蓝应不同；金色（选中地块语义）绝不能出现在钻入标记上
    /// （v0.2 §B.2 明令）。
    #[test]
    fn accent_differs_between_themes_but_is_never_gold() {
        let dark = accent_for(UiTheme::Dark);
        let light = accent_for(UiTheme::Light);
        assert_ne!(dark, light, "两套主题的界面蓝应不同");
        let gold = Color::srgb_u8(0xF2, 0xC1, 0x4E);
        assert_ne!(dark, gold);
        assert_ne!(light, gold);
    }
}
