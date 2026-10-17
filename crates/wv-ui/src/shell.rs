//! UI 样式快照：调色板 / 字号 / 圆角 / 间距的 egui 投影（ADR-0026 自 wv-app 迁入）。
//!
//! ## 职责
//! - [`UiShell`] —— 每帧由 UI 系统装配的样式快照，避免到处传参。
//!
//! ## 边界（不做什么）
//! - **不**承载面板 / 菜单绘制 —— 那是消费方（wv-app 各 `*_panel.rs`）的事。
//! - **不**做令牌源头 —— 只读 [`crate::ui_tokens`] / [`crate::ui_theme`]。
//! - **不含**地形名 / 地形色块 helper —— 它们依赖 i18n 与世界色板，留 wv-app
//!   （ADR-0026 D3）；app 侧 shim 经 `pub use` 保持 `crate::ui::shell::UiShell` 路径。

use bevy_egui::egui;

use crate::ui_theme::{color32, color32_alpha, UiShellConfig};
use crate::ui_tokens::ShellPalette;

/// 每帧由 UI 系统装配的样式快照（避免到处传参）。
pub struct UiShell {
    /// 当前外壳令牌。
    pub palette: ShellPalette,
    /// 字号阶梯。
    pub scale: crate::ui_theme::FontScale,
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
            scale: crate::ui_theme::font_sizes(cfg.density),
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
    /// 输入框 / 值框内嵌底（目录 `ui.bg.input`）。
    pub fn input_bg(&self) -> egui::Color32 {
        color32(self.palette.bg_input)
    }
    /// 禁用态文字（目录 `ui.tx.disabled`）。
    pub fn text_disabled(&self) -> egui::Color32 {
        color32(self.palette.text_disabled)
    }
    /// 金色软底（当前画笔 / 选中工具）。
    pub fn gold_soft(&self) -> egui::Color32 {
        color32(self.palette.gold_soft)
    }
    /// 危险软底（Destructive 按钮）。
    pub fn danger_soft(&self) -> egui::Color32 {
        color32(self.palette.danger_soft)
    }
    /// 成功软底（内联成功提示）。
    pub fn success_soft(&self) -> egui::Color32 {
        color32(self.palette.success_soft)
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
    /// accent 实心底上的文字色（**不是**白色，见 [`crate::ui_tokens::ShellPalette::text_on_accent`]）。
    pub fn text_on_accent(&self) -> egui::Color32 {
        color32(self.palette.text_on_accent)
    }
    /// 面包屑「上级」文字色（v0.2 §B.3 新增令牌）。
    pub fn crumb_text_3(&self) -> egui::Color32 {
        color32(self.palette.crumb_text_3)
    }
    /// v0.3（Figma `5E8BCBCmtGAHqk3ORibBlU`）：accent 低饱和抬升底
    /// （分段控件选中态 / 主切换按钮底色）。
    pub fn accent_soft(&self) -> egui::Color32 {
        color32(self.palette.accent_soft)
    }
    /// v0.3：金色（"ACTIVE" 徽标 / 时间线播放 / 城市虚线圈）。
    pub fn gold(&self) -> egui::Color32 {
        color32(self.palette.gold)
    }
    /// v0.3：危险红（"Unsaved changes" / 错误态）。
    pub fn danger(&self) -> egui::Color32 {
        color32(self.palette.danger)
    }
    /// v0.3：成功绿（"Saved locally"）。
    pub fn success(&self) -> egui::Color32 {
        color32(self.palette.success)
    }
    /// v0.3：地图水体衬底。
    pub fn map_water(&self) -> egui::Color32 {
        color32(self.palette.map_water)
    }
    /// v0.3：地图陆地衬底。
    pub fn map_land(&self) -> egui::Color32 {
        color32(self.palette.map_land)
    }
    /// v0.3：时间线轨道底。
    pub fn timeline_track(&self) -> egui::Color32 {
        color32(self.palette.timeline_track)
    }
    /// v0.3：时间线季节段底。
    pub fn timeline_season(&self) -> egui::Color32 {
        color32(self.palette.timeline_season)
    }
}
