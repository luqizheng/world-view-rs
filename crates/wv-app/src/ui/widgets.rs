//! 共享 widget shim（ADR-0026 D3）。
//!
//! 控件本体全部在 [`wv_ui::widgets`]（button / controls / floater / primitives /
//! segmented_control / svg_loader）；此处 glob re-export 保持
//! `crate::ui::widgets::X` 既有路径零改动。本文件只承载两个**域件**——它们
//! 依赖 app 状态（`Panel` / `PanelState` / `ui-layout.yaml`）或 i18n + 世界色板，
//! 不进 `wv-ui`：
//!
//! - [`float_window`] —— `Panel` 特化浮窗包装（委托 `wv_ui::FloatWindow`）；
//! - [`terrain_grid`] —— 笔刷面板地形色块网格（被 [`crate::ui::brush_panel`] 调用）。

#![cfg(feature = "app")]

pub use wv_ui::widgets::*;

mod local;
pub use local::{float_window, terrain_grid};

use crate::i18n::{t, I18nKey, Locale};

/// 数值输入错误提示文案（D1-B 红闪 tooltip）——从 i18n 表取词注入 wv-ui
/// 控件。wv-ui 按 ADR-0026 无 i18n，用户可见文字一律由调用方给
/// （2026-10-28 评审 P2：crate 不再持有用户可见文案）。
pub fn numeric_texts(loc: Locale) -> NumericTexts {
    NumericTexts {
        invalid: t(loc, I18nKey::NumericInvalidInput),
        clamped_to: t(loc, I18nKey::NumericClampedTo),
    }
}
