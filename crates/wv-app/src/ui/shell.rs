//! UI 样式快照：调色板 / 字号 / 圆角 / 间距的 egui 投影。
//!
//! ## 职责
//! - `terrain_name_at` —— 10 种地形的本地化显示名（ADR-0018）。
//! - `terrain_swatch` —— 地形色块（依 `crate::theme::colors::core_terrain`）。
//! - `UiShell` —— 每帧由 UI 系统装配的样式快照，避免到处传参。
//!
//! ## 边界（不做什么）
//! - **不**承载面板 / 菜单 / 命令面板的绘制 —— 那是同目录各 `*_panel.rs` / `top_bar.rs` 等。
//! - **不**改写 `crate::ui_tokens` / `crate::ui_theme` 的源头 —— 只读它们。
//!
//! ## 主要导出
//! - `terrain_name_at` —— `(Locale, idx) -> &'static str` 地形名查表（取代常量 TERRAIN_NAMES）。
//! - `terrain_swatch` —— `u8` 地形索引 → `egui::Color32`。
//! - `UiShell` —— 样式快照（含 `t1` / `t2` / `t3` / `accent` / `panel_bg` 等 getter）。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::i18n::{t, I18nKey, Locale};
use crate::theme::ui_theme::{color32, color32_alpha, UiShellConfig};
use crate::theme::ui_tokens::ShellPalette;

/// 10 种地形的 i18n Key 表（与 `crate::i18n::table.rs` 的 zh/en 镜像同序）。
const TERRAIN_KEYS: [I18nKey; 10] = [
    I18nKey::TerrainKindDeepWater,
    I18nKey::TerrainKindShallowWater,
    I18nKey::TerrainKindBeach,
    I18nKey::TerrainKindGrassland,
    I18nKey::TerrainKindForest,
    I18nKey::TerrainKindHill,
    I18nKey::TerrainKindMountain,
    I18nKey::TerrainKindSnow,
    I18nKey::TerrainKindSwamp,
    I18nKey::TerrainKindRoad,
];

/// 按索引返回本地化后的地形名。索引越界时返回 `"?"`（防御性兜底，绝不在 UI 字符串里 panic）。
pub fn terrain_name_at(loc: Locale, idx: usize) -> &'static str {
    match TERRAIN_KEYS.get(idx) {
        Some(k) => t(loc, *k),
        None => "?",
    }
}

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
