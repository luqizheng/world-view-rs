//! UI 多语言支持（ADR-0018）。
//!
//! ## 职责
//! - 提供 `Locale { Zh, En }`（`Default = Zh`）与 `I18nKey` 枚举；
//! - 提供 `t(locale, key) -> &'static str` 查表函数（无插值 — 调用点负责 format!）；
//! - 提供 `CurrentLocale(pub Locale)` 作为 Bevy `Resource`，
//!   在 `app_bootstrap::run` 与其他 Resource 一并 `.init_resource::<CurrentLocale>()`；
//! - 持久化复用 `crate::ui_layout::LayoutFile.language: Option<String>`，
//!   见 `ui_shell::load_layout_on_project_open` / `persist_layout_on_change`（ADR-0018 §2.2）。
//!
//! ## 边界（不做什么）
//! - **不引入插值**：v1 文案都短，且无复数 / 性别 / ICU 选择变量
//!   （AGENTS §9 默认拒绝 Fluent）。
//! - **不改字体**：`theme/fonts.rs` CJK 注册已经天然兼容 EN
//!   （CJK fallback 只在含 CJK 字符时触发，EN 用户无副作用）。
//! - **不持 OS 探测**：`sys-locale` 不引入（v1 仅依赖 `ui-layout.yaml#language` 与默认 zh）。
//!
//! ## 主要导出
//! - [`Locale`] 枚举 / [`parse`] 自由函数
//! - [`I18nKey`] 枚举
//! - [`t`] / [`t_key`] 查表入口
//! - [`CurrentLocale`] Bevy Resource
//!
//! ## 用法
//! ```rust,ignore
//! use crate::i18n::{I18nKey, CurrentLocale, t_key};
//!
//! fn draw_topbar(ui: &mut egui::Ui, loc: &CurrentLocale) {
//!     ui.label(t_key(loc, I18nKey::TopBarSaved));
//! }
//! ```

#![cfg(feature = "app")]

use bevy::prelude::Resource;

mod keys;
mod locale;
mod table;

pub use keys::I18nKey;
pub use locale::{parse, Locale};

/// 当前 UI 文案语言（与工程绑定的会话内存副本）。
///
/// 默认 `Locale::Zh`（与现行工程唯一工作语言一致）。打开工程时由
/// `crate::ui_shell::load_layout_on_project_open` 覆盖之；
/// 切换语言时由 `crate::ui_shell::persist_layout_on_change` 落盘回 `ui-layout.yaml`。
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CurrentLocale(pub Locale);

/// 查表：给定 Locale + Key 返回翻译好的字面量。
///
/// 本函数**绝不**返回错误或空字符串；缺翻译时编译器不让构建（`table.rs` 的
/// `non-exhaustive patterns` 兜底）。
#[inline]
pub fn t(locale: Locale, key: I18nKey) -> &'static str {
    match locale {
        Locale::Zh => table::lookup_zh(key),
        Locale::En => table::lookup_en(key),
    }
}

/// `t()` 的 Resource 版便捷封装，UI 面板绝大多数调用点都用这个。
#[inline]
pub fn t_key(loc: &CurrentLocale, key: I18nKey) -> &'static str {
    t(loc.0, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全 I18nKey 都能在指定 locale 下查表（非空字符串）。
    fn every_key_resolves(locale: Locale) {
        for k in I18nKey::ALL {
            let s = t(locale, k);
            assert!(!s.is_empty(), "{locale:?} {k:?} resolves to empty string");
        }
    }

    #[test]
    fn every_key_resolves_in_chinese() {
        every_key_resolves(Locale::Zh);
    }

    #[test]
    fn every_key_resolves_in_english() {
        every_key_resolves(Locale::En);
    }

    #[test]
    fn chinese_and_english_differ_on_distinctive_key() {
        // 取一对**有把握确实不同的** key 验证真在切换：
        // 中文 `未保存`，英文 `Unsaved`。
        assert_ne!(
            t(Locale::Zh, I18nKey::TopBarUnsaved),
            t(Locale::En, I18nKey::TopBarUnsaved)
        );
        assert_ne!(
            t(Locale::Zh, I18nKey::TopBarNoProjectLoaded),
            t(Locale::En, I18nKey::TopBarNoProjectLoaded)
        );
    }

    #[test]
    fn ascii_glyph_keys_match_across_locales() {
        // 「●」等 ASCII 字符在两个 locale 下应一致；保证 zh 表不会意外改写这些共享 glyph
        assert_eq!(
            t(Locale::Zh, I18nKey::TopBarUnsavedDot),
            t(Locale::En, I18nKey::TopBarUnsavedDot)
        );
    }

    #[test]
    fn current_locale_default_is_zh() {
        let cl = CurrentLocale::default();
        assert_eq!(cl.0, Locale::Zh);
        assert_eq!(t_key(&cl, I18nKey::TopBarSaved), "已保存");
    }

    #[test]
    fn switch_locale_to_en_changes_text() {
        let cl = CurrentLocale(Locale::En);
        assert_eq!(t_key(&cl, I18nKey::TopBarSaved), "Saved");
    }
}
