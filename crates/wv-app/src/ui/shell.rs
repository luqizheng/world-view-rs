//! UI 样式快照：调色板 / 字号 / 圆角 / 间距的 egui 投影。
//!
//! ## 职责
//! - `TERRAIN_NAMES` —— 地形类型名常量（10 种），供笔刷面板与图例。
//! - `terrain_swatch` —— 地形色块（依 `crate::theme::colors::core_terrain`）。
//! - `UiShell` —— 每帧由 UI 系统装配的样式快照，避免到处传参。
//!
//! ## 边界（不做什么）
//! - **不**承载面板 / 菜单 / 命令面板的绘制 —— 那是同目录各 `*_panel.rs` / `top_bar.rs` 等。
//! - **不**改写 `crate::ui_tokens` / `crate::ui_theme` 的源头 —— 只读它们。
//!
//! ## 主要导出
//! - `TERRAIN_NAMES` —— 地形名常量。
//! - `terrain_swatch` —— `u8` 地形索引 → `egui::Color32`。
//! - `UiShell` —— 样式快照（含 `t1` / `t2` / `t3` / `accent` / `panel_bg` 等 getter）。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::theme::ui_theme::{color32, color32_alpha, UiShellConfig};
use crate::theme::ui_tokens::ShellPalette;

/// 地形类型名（供笔刷面板与图例）。
pub const TERRAIN_NAMES: [&str; 10] = [
    "深水", "浅水", "沙滩", "草原", "林地", "丘陵", "山地", "雪原", "沼泽", "道路",
];

/// 地形色块索引 → `palette::CORE_TERRAIN` 同序（§4.5：色块就是地形本色）。
pub fn terrain_swatch(kind: u8) -> egui::Color32 {
    let c = crate::theme::colors::core_terrain(kind);
    let bevy::prelude::Color::Srgba(s) = c else {
        return egui::Color32::GRAY;
    };
    egui::Color32::from_rgb(
        (s.red * 255.0) as u8,
        (s.green * 255.0) as u8,
        (s.blue * 255.0) as u8,
    )
}

/// 每帧由 UI 系统装配的样式快照（避免到处传参）。
pub struct UiShell {
    /// 当前外壳令牌。
    pub palette: ShellPalette,
    /// 字号阶梯。
    pub scale: crate::theme::ui_theme::FontScale,
    /// 面板头高度（密度）。
    pub head_h: f32,
    /// 命中区（密度）。
    pub hit: f32,
    /// 行高（密度）。
    pub row: f32,
}

impl UiShell {
    /// 由主题配置构建。
    pub fn new(cfg: &UiShellConfig) -> Self {
        Self {
            palette: cfg.palette(),
            scale: crate::theme::ui_theme::font_sizes(cfg.density),
            head_h: cfg.density.panel_head(),
            hit: cfg.density.hit(),
            row: cfg.density.row(),
        }
    }

    /// 主文本色。
    pub fn t1(&self) -> egui::Color32 {
        color32(self.palette.text_1)
    }
    /// 次文本色。
    pub fn t2(&self) -> egui::Color32 {
        color32(self.palette.text_2)
    }
    /// 弱文本色。
    pub fn t3(&self) -> egui::Color32 {
        color32(self.palette.text_3)
    }
    /// 交互主色。
    pub fn accent(&self) -> egui::Color32 {
        color32(self.palette.accent)
    }
    /// 焦点色。
    pub fn focus(&self) -> egui::Color32 {
        color32(self.palette.focus)
    }
    /// 面板底。
    pub fn panel_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_panel)
    }
    /// 抬升块底。
    pub fn raised_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_raised)
    }
    /// hover 底。
    pub fn hover_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_hover)
    }
    /// 选中 / pressed 底。
    pub fn active_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_active)
    }
    /// 分隔线。
    pub fn border_subtle(&self) -> egui::Color32 {
        color32(self.palette.border_subtle)
    }
    /// 外描边。
    pub fn border_strong(&self) -> egui::Color32 {
        color32(self.palette.border_strong)
    }
    /// 浮层底（含标定不透明度）。
    pub fn float_fill(&self) -> egui::Color32 {
        let (c, a) = self.palette.float_fill();
        color32_alpha(c, a)
    }
    /// accent 实心底上的文字色（**不是**白色，见 [`crate::theme::ui_tokens::ShellPalette::text_on_accent`]）。
    pub fn text_on_accent(&self) -> egui::Color32 {
        color32(self.palette.text_on_accent)
    }
    /// 面包屑「上级」文字色（v0.2 §B.3 新增令牌）。
    pub fn crumb_text_3(&self) -> egui::Color32 {
        color32(self.palette.crumb_text_3)
    }
}
