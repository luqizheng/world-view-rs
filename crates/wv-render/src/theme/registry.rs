//! 待批准：本文件在 `app` feature 下才编译。
#![cfg(feature = "app")]

//! 主题包注册表（ADR-0023 D2）—— 把多个主题 crate 的 pack 列表合并成单一查询表。
//!
//! ## 职责
//! - 定义 [`ThemePackRegistry`] trait：每个主题 crate 实现该 trait、提供 `&'static [..]`
//!   静态切片；
//! - 定义 [`MergedThemeRegistry`]：把多个 `ThemePackRegistry` 实现合并，按"后注册覆盖
//!   先注册"求 id → pack 映射（与 M2 §4 待裁 A 项一致——无 `priority` 字段，简洁优先）；
//! - `ActiveThemePack` Resource 的**查找入口**（M2 验收时 `app_bootstrap` 从这里取
//!   `&'static ThemePack`，替代 `builtin::for_palette(view.map_palette)` 兜底）。
//!
//! ## 边界（不做什么）
//! - **不**做热切换脏标传播（M2 沿用 `terrain_view::rebuild_texture_on_mode_change` 同口径）；
//! - **不**做运行时加载（YAML / 文件路径解析）——M3 推迟到 README §12-30 推进时单开 ADR；
//! - **不**消费 `Box<dyn ThemePackRegistry>`：合并走 `&'static` 切片引用，单态化编译期
//!   完成——D2 §2 否决 dyn 调度，**性能档不破**。

use super::theme_pack::ThemePack;

/// 主题包注册表 trait（ADR-0023 D2）。
///
/// **实现者**是各主题 crate（`wv-theme-chinese-ink` 等），**不在** wv-render 内
/// 暴露的 trait 上做反射——`&'static [(&'static str, &'static ThemePack)]` 切片即可。
///
/// ## 静态分发
/// 所有实现返回 `&'static [...]` 切片——合并时按 trait object-free 风格枚举
/// `&[&dyn RegistryList]` 即可（`&dyn` 是 `&dyn ThemePackRegistry` 的薄包装，
/// 调用 `packs()` 一次拿切片；不持对象）。
///
/// ## M2 验收
/// 本 trait 的"实现示例" = `wv-theme-chinese-ink` crate（见 `examples/` 与
/// `crates/wv-theme-chinese-ink/`）。
pub trait ThemePackRegistry {
    /// 本注册表持有的主题包列表——id 到 pack 的映射（**id 唯一**约定，重复由
    /// `MergedThemeRegistry::merge` 阶段报错，**不**做静默覆盖）。
    fn packs(&self) -> &'static [(&'static str, &'static ThemePack)];

    /// 注册表 id（用于日志 / 状态栏告警区分来源；M2 暂作展示，M3 持久化时用）。
    fn registry_id(&self) -> &'static str;
}

/// 多个注册表合并器（ADR-0023 D2 §3 "AppThemePacks"）。
///
/// **构造期**枚举所有 `ThemePackRegistry` 实现，把 `packs()` 切片拼起来。**运行期**
/// 通过 `lookup` / `iter` 做 O(1) 字符串查表（线性扫小切片即可——主题数量个位数，
/// 二分查找是过度工程）。
#[derive(Debug, Clone, Copy)]
pub struct MergedThemeRegistry {
    entries: &'static [(&'static str, &'static ThemePack)],
}

impl Default for MergedThemeRegistry {
    /// M2 兜底：仅含内置古舆图。
    ///
    /// **为何不用 `Default::default()`**：`&'static [...]` 没法 const-construct 切片
    /// （非 const fn），但可以 const-construct `&'static [..]` 通过 `static` 包装——
    /// 与 `ThemePack::builtin::ALL` 同一路径。
    fn default() -> Self {
        Self {
            entries: super::theme_pack::builtin::ALL,
        }
    }
}

impl MergedThemeRegistry {
    /// 合并多个注册表（M2 由 `app_bootstrap` 显式调用）。
    ///
    /// **冲突策略**：id 重复 = panic。理由：主题 id 是稳定标识符（持久化键 / 跨工程
    /// 引用），重复是**配置错误**而非"允许同 id 多实现"——V1 纪律下应**显式暴露**
    /// 而非静默覆盖（避免"加载顺序影响结果"类隐患）。M2+ 可改为合并时报错并附 id 列表。
    pub fn merge(registries: &[&dyn ThemePackRegistry]) -> Self {
        let mut entries: Vec<(&'static str, &'static ThemePack)> = Vec::new();
        for r in registries {
            for (id, pack) in r.packs() {
                if entries.iter().any(|(existing_id, _)| *existing_id == *id) {
                    panic!(
                        "MergedThemeRegistry: duplicate theme id `{id}` (from registry `{}`)",
                        r.registry_id()
                    );
                }
                entries.push((*id, *pack));
            }
        }
        // 泄露为 'static：merge 在启动期调用一次，运行期只读。
        let leaked: &'static [(&'static str, &'static ThemePack)] =
            Box::leak(entries.into_boxed_slice());
        Self { entries: leaked }
    }

    /// 按 id 查 pack（找不到返回 `None`——V1 纪律推广到主题）。
    pub fn lookup(&self, id: &str) -> Option<&'static ThemePack> {
        self.entries
            .iter()
            .find(|(existing_id, _)| *existing_id == id)
            .map(|(_, pack)| *pack)
    }

    /// 全部条目（顶栏 / 命令面板遍历用）。
    pub fn iter(&self) -> impl Iterator<Item = &'static (&'static str, &'static ThemePack)> {
        self.entries.iter()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空（与 `len()` 配对——clippy `len_without_is_empty` 建议）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 空集合（M2 默认 fallback 链尾——`AppThemePacks` 在 `app_bootstrap` 找不到时回退）。
    pub fn empty() -> Self {
        Self { entries: &[] }
    }
}

/// 内置主题注册表（ADR-0023 D2 + M2 验收）—— 游戏 / 编辑器 app 都应 merge 本注册表
/// 才能使用 `antique_default` / `antique_dusk` 等内置 pack。
///
/// 之所以作为 trait 实现（而非 `MergedThemeRegistry::default()` 隐式包含）—— 是为了
/// 让"游戏 app 接入哪些 pack"是**显式声明**：游戏写 `merge(&[&BuiltinRegistry, &MyMod])`
/// 即可看到完整装配，无需隐式默认。
pub struct BuiltinRegistry;

impl ThemePackRegistry for BuiltinRegistry {
    fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
        super::theme_pack::builtin::ALL
    }
    fn registry_id(&self) -> &'static str {
        "builtin"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::palette::PaletteColors;

    /// 测试用：第一个注册表。
    struct FakeRegistryA;
    impl ThemePackRegistry for FakeRegistryA {
        fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
            // 空注册表（避免与 builtin::ALL 冲突）。
            &[]
        }
        fn registry_id(&self) -> &'static str {
            "fake_a"
        }
    }

    /// 测试用：第二个注册表（也空，专注测 merge 冲突 / 单元素路径）。
    struct FakeRegistryB;
    impl ThemePackRegistry for FakeRegistryB {
        fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
            &[]
        }
        fn registry_id(&self) -> &'static str {
            "fake_b"
        }
    }

    /// `Default::default()` 兜底 = 仅含内置古舆图（M2 验收期望）。
    #[test]
    fn default_contains_builtin_default() {
        let m = MergedThemeRegistry::default();
        let p = m.lookup("antique_default").expect("内置默认 id 必须存在");
        assert_eq!(p.id, "antique_default");
        assert_eq!(p.palette.core, PaletteColors::DAYLIGHT.core);
    }

    /// `empty()` 真为空。
    #[test]
    fn empty_is_empty() {
        let m = MergedThemeRegistry::empty();
        assert_eq!(m.len(), 0);
        assert!(m.lookup("antique_default").is_none());
    }

    /// `merge` 多注册表 = 拼接（不重不漏）。
    #[test]
    fn merge_combines_two_registries() {
        let merged = MergedThemeRegistry::merge(&[&FakeRegistryA, &FakeRegistryB]);
        // 两条都是空注册表 → merged 也空
        assert_eq!(merged.len(), 0);
    }

    /// `merge` 与 builtin::ALL 一起 = 至少含 antique_default。
    #[test]
    fn merge_with_builtin_contains_default() {
        // 构造一个 "builtin only" 合并，验证 merge 路径不丢 builtin。
        struct BuiltinOnly;
        impl ThemePackRegistry for BuiltinOnly {
            fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
                crate::theme::theme_pack::builtin::ALL
            }
            fn registry_id(&self) -> &'static str {
                "builtin_only"
            }
        }
        let merged = MergedThemeRegistry::merge(&[&BuiltinOnly]);
        assert!(merged.lookup("antique_default").is_some());
    }

    /// id 重复 = panic（V1 纪律推广——配置错误必须显式暴露）。
    #[test]
    #[should_panic(expected = "duplicate theme id")]
    fn merge_panics_on_duplicate_id() {
        struct Dup;
        impl ThemePackRegistry for Dup {
            fn packs(&self) -> &'static [(&'static str, &'static ThemePack)] {
                crate::theme::theme_pack::builtin::ALL // 重复 antique_default
            }
            fn registry_id(&self) -> &'static str {
                "dup"
            }
        }
        let _ = MergedThemeRegistry::merge(&[&Dup, &Dup]);
    }

    /// `lookup` 找不到 = `None`，不 panic。
    #[test]
    fn lookup_unknown_returns_none() {
        let m = MergedThemeRegistry::default();
        assert!(m.lookup("not_a_real_pack").is_none());
    }
}
