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
use std::sync::OnceLock;

mod keys;
mod locale;
mod table;

pub use keys::I18nKey;
pub use locale::{parse, Locale};

/// CLI 进程级单例 Locale（Bevy 资源走不到，CLI 子命令 / 选项解析 / help 都用这个）。
///
/// **设置时机**：进程启动早期（main() 第一步）。CLI 不支持热切换；GUI 启动后
/// 用 Bevy 的 `CurrentLocale` 资源（两者互不相通，CLI 设的值仅影响 stderr / stdout
/// 文案）。
static CLI_LOCALE: OnceLock<Locale> = OnceLock::new();

/// 进程启动时一次性设 CLI Locale。多次调用取第一次。
///
/// 由 [`crate::main`] 在解析 `--language` 标记后立即调用；其它路径不调用时
/// 退回 [`Locale::Zh`] 默认。
pub fn set_cli_locale(loc: Locale) {
    let _ = CLI_LOCALE.set(loc);
}

/// 读 CLI Locale；未设置时返回 [`Locale::default`]（= [`Locale::Zh`]）。
pub fn cli_locale() -> Locale {
    CLI_LOCALE.get().copied().unwrap_or_default()
}

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
    /// **空字符串是允许的**：作为拼接片段（如「项目将在 」的前缀、
    /// 「 创建目录」的后缀），某条语言下可能为空（例如英文后缀无对应词）——
    /// 这种情况不算缺翻译，**assert_eq! 全词匹配交给另一个测试**。
    fn every_key_has_a_resolved_value_in(locale: Locale) {
        for k in I18nKey::ALL {
            let s = t(locale, k);
            // 必须能查表（漏译时编译失败）—— 不强制非空。
            let _ = s.len();
        }
    }

    #[test]
    fn every_key_resolves_in_chinese() {
        every_key_has_a_resolved_value_in(Locale::Zh);
    }

    #[test]
    fn every_key_resolves_in_english() {
        every_key_has_a_resolved_value_in(Locale::En);
    }

    /// 必填 Key（非拼接片段）在双 locale 下都非空。
    #[test]
    fn substantive_keys_are_non_empty_in_both_locales() {
        let substantive: &[I18nKey] = &[
            I18nKey::CommonDelete,
            I18nKey::CommonCancel,
            I18nKey::CommonOk,
            I18nKey::CommonConfirm,
            I18nKey::TopBarUnsaved,
            I18nKey::TopBarSaved,
            I18nKey::TopBarForceRedrawTooltip,
            I18nKey::TopBarNoProjectLoaded,
            I18nKey::WelcomeHeading,
            I18nKey::WelcomeTagline,
            I18nKey::WelcomeHelpText,
            I18nKey::WelcomeOpenProjectBtn,
            I18nKey::WelcomeNewProjectBtn,
            I18nKey::NewProjectDialogTitle,
            I18nKey::NewProjectDialogNameLabel,
            I18nKey::NewProjectDialogCreateBtn,
            I18nKey::NewMapDialogTitle,
            I18nKey::NewMapDialogCreateBtn,
        ];
        for k in substantive {
            assert!(!t(Locale::Zh, *k).is_empty(), "{k:?} zh 空");
            assert!(!t(Locale::En, *k).is_empty(), "{k:?} en 空");
        }
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

    /// `ALL` 数组必须与变体集合严格一致（长度 = 去重后条数 = 变体总数）。
    ///
    /// 真正的硬闸是编译器：`ALL: [I18nKey; N]` 的 N 与初始化元素数不符、或
    /// 漏写变体都直接编译失败。本测试把「预期条数」钉成显式断言并防**重复项**
    /// （元素数凑对但某变体被写了两次、另一个被挤掉——这种编译器不报）。
    /// 计数沿革（以 ALL 数组实际长度为准，漏写/重复都会在此暴露）：合并并集
    /// 569，减 FactionPanel 死键 16，加 pen 悬停 1（终审四修轮），加工作区锁
    /// 提示 1（ADR-0080），加边界红圈开关 1（常显改可选），加改群系三键（ADR-0081）
    /// 与笔刷窗自然区段三键（ADR-0082），加设置 HTTP 服务分组 10 键（2026-10-28），加复制 agent skill 两键（ADR-0085）= 574，
    /// 加 UX round-2 quick-wins 五键（2026-10-29：菜单面板分组 / 缩放档锁定 / 视图模式锁定 / 无地形网格 / 撤销栈逐出）= 579。
    /// 加 UX batch2 十一键（2026-10-29：关闭确认四键 + 登记事件七键）= 591。
    /// 加 batch3 净增两键（2026-10-30：删 NoticeExportPending、增导出三键）= 593。
    /// 加人物等级列表编辑器十键（2026-10-30：对话框六键 + 列表段数单位 + 提示 + 回执两键）= 603。
    /// 另有 13 个「划分势力」Division 族键只进过枚举与译文表、从未入 ALL
    /// （ADR-0028 退役漏清，2026-10-28 一并删除）。
    #[test]
    fn i18n_all_length_matches_key_count() {
        let mut seen = std::collections::HashSet::new();
        for k in I18nKey::ALL {
            assert!(seen.insert(k), "{k:?} 在 ALL 中重复（某个变体被挤掉了）");
        }
        assert_eq!(
            I18nKey::ALL.len(),
            612,
            "ALL 应为 612 项（… + 人物等级列表编辑器十键 + UX batch5 六键 + ADR-0090 三键：MapControlsSelectCell/SelectCellHint/CellFloodOff）；加新键时同步更新数组长度与元素（got {})",
            I18nKey::ALL.len()
        );
        assert_eq!(seen.len(), 612, "去重后应仍为 612 项"); (wv-app,wv-render,wv-ui,wv-world-cell-map: ADR-0090 M1b——圈选增「单格」第四形状（SelectionShape::Cell，用户裁定改道：显式形状替代手势推断）+ commit_cell_shape_click 单格/泛洪判据点击 + 点选减法（图钉>群系区，Alt/泛洪/浏览族格子档退役）+ FloodCriterion::Off 新默认 + 泛洪判据段迁圈选单格 + lucide mouse-pointer-click 图标 + i18n 596)
    }
}
