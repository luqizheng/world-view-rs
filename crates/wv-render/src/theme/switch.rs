//! 主题包切换（ADR-0023 M2 落地 · ADR-0049 提议态）——把 `ViewState.theme_pack_id`
//! 解析成运行期权威的 [`ActiveThemePack`]。
//!
//! ## 为什么需要这一层
//!
//! ADR-0023 D1 预留了 `ViewState.theme_pack_id`，但 **M1 阶段它零消费点**：
//! 渲染层一律走 `builtin::for_palette(view.map_palette)`（`MapPalette` 只有
//! Daylight / Dusk 两值）。后果是外部主题 crate（`wv-theme-chinese-ink`）
//! 写好了配方却**永远用不到**——ADR-0043「加新风格 = 加新 crate、零改渲染层
//! 源码」在端到端意义上并未成立。
//!
//! 本模块接通最后一环：**权威从 `map_palette` 迁到 `theme_pack_id`**，
//! `map_palette` 降级为「内置包的快捷选择器」（顶栏日夜按钮仍走它）。
//!
//! ## 边界
//!
//! - **不做热切换脏标传播**：主题变了由消费方各自比对（地形纹理比 `last_theme_id`、
//!   覆盖物比缓存键）——与既有 `map_palette` 切换的处理同口径。
//! - **不做运行时加载**（YAML / 路径解析）：M3 挂账（README §12-30）。
//! - **不消费 `Box<dyn>`**：注册表合并走 `&'static` 切片（ADR-0023 D2 §2）。

use bevy::prelude::*;

use crate::state::ViewState;
use wv_theme::palette::MapPalette;
use wv_theme::registry::MergedThemeRegistry;
use wv_theme::theme_pack::{builtin, ThemePack};

/// 运行期可用的主题包表。
///
/// 启动期由 `app_bootstrap` 用 [`MergedThemeRegistry::merge`] 装配
/// （内置包 + 各外部主题 crate 的注册表），之后只读。
#[derive(Resource, Debug, Clone, Copy)]
pub struct ThemeRegistry(pub MergedThemeRegistry);

impl Default for ThemeRegistry {
    /// 兜底 = 仅内置包（与 `MergedThemeRegistry::default()` 同源）。
    fn default() -> Self {
        Self(MergedThemeRegistry::default())
    }
}

/// 解析当前应生效的主题包。
///
/// 优先级：
/// 1. `view.theme_pack_id` 在注册表里命中 → 用它；
/// 2. `view.theme_pack_id` 是存量 YAML 的 `map_theme` 值（`MapPalette::key()`，
///    即 `daylight` / `dusk`）——与 pack id 不同名，**兼容回退**到内置包
///    （存量工程零迁移）；
/// 3. 都落空 → 按 `view.map_palette` 取内置包（`map_palette` 降级为快捷选择器）。
pub fn resolve_pack(view: &ViewState, registry: &MergedThemeRegistry) -> &'static ThemePack {
    if let Some(id) = view.theme_pack_id.as_deref() {
        if let Some(pack) = registry.lookup(id) {
            return pack;
        }
        if let Some(pal) = MapPalette::from_key(id) {
            return builtin::for_palette(pal);
        }
    }
    builtin::for_palette(view.map_palette)
}

/// 主题包同步系统：把 `ViewState.theme_pack_id` 解析成 `ViewState.active_theme`。
///
/// **只在 id 变化时写**：`ResMut` 写入会把 Resource 标记为 changed，每帧写
/// 会让下游「缓存键比对」类逻辑误判为「主题变了」而白白重算。
///
/// **调度位置**：必须**先于**所有消费点。本 system 挂在 `PreUpdate` 最前，
/// 而渲染消费点在 `Update` —— PreUpdate 一定先于 Update；唯一同在 PreUpdate
/// 的消费者（`poll_open_project_dialog`）由 app 侧显式 `.before()` 保证。
pub fn sync_active_theme_pack(registry: Res<ThemeRegistry>, mut view: ResMut<ViewState>) {
    let pack = resolve_pack(&view, &registry.0);
    if view.active_theme.id != pack.id {
        view.active_theme = pack;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_theme::theme_pack::builtin;

    fn view_with(id: Option<&str>) -> ViewState {
        ViewState {
            theme_pack_id: id.map(str::to_string),
            ..Default::default()
        }
    }

    /// 无 id → 落回 `map_palette` 的内置包。
    #[test]
    fn no_theme_id_falls_back_to_map_palette() {
        let v = view_with(None);
        let p = resolve_pack(&v, &MergedThemeRegistry::default());
        assert_eq!(p.id, builtin::DEFAULT.id, "应回落到内置日间包");
    }

    /// 存量 YAML 的 `map_theme: daylight` 要能解析（零迁移）。
    #[test]
    fn legacy_map_palette_key_still_resolves() {
        let v = view_with(Some("daylight"));
        let p = resolve_pack(&v, &MergedThemeRegistry::default());
        assert_eq!(p.id, builtin::DEFAULT.id, "daylight 应解析到日间内置包");
        let v = view_with(Some("dusk"));
        let p = resolve_pack(&v, &MergedThemeRegistry::default());
        assert_eq!(p.id, builtin::DUSK.id, "dusk 应解析到暮色内置包");
    }

    /// 未知 id → 回落到内置包（V1 纪律：不阻止、不报错）。
    #[test]
    fn unknown_theme_id_falls_back_without_panicking() {
        let v = view_with(Some("no_such_theme"));
        let p = resolve_pack(&v, &MergedThemeRegistry::default());
        assert_eq!(p.id, builtin::DEFAULT.id);
    }

    /// 注册表里存在的 id 优先于兼容回退。
    #[test]
    fn registry_hit_wins_over_legacy_fallback() {
        // 构造一个只含 DEFAULT（id = "antique_default"）的注册表：
        // `daylight` 不在里面 → 走兼容回退；`antique_default` 在里面 → 直接命中。
        let reg = MergedThemeRegistry::default();
        let p = resolve_pack(&view_with(Some("antique_default")), &reg);
        assert_eq!(p.id, builtin::DEFAULT.id);
    }

    /// **注册表命中优先于内置回退**（ADR-0043 注入契约的锚点）：
    /// 注册表登记的 id 必须解析出**它自己登记的那个包**，而不是内置表里的同名项。
    ///
    /// 外部包（`wv-theme-chinese-ink`）的真实接线由 app 侧的 registry 装配
    /// 验证；本测试锁住「解析按注册表走」这条语义。
    #[test]
    fn registry_lookup_takes_priority_over_builtin_fallback() {
        let reg = MergedThemeRegistry::merge(&[&wv_theme::registry::BuiltinRegistry]);
        // 内置包 id（"antique_default"）在注册表里 → 直接命中。
        let p = resolve_pack(&view_with(Some("antique_default")), &reg);
        assert_eq!(p.id, builtin::DEFAULT.id);
        // 注册表里没有的 id → 走兼容回退（存量 YAML 的 "daylight"）。
        let p = resolve_pack(&view_with(Some("daylight")), &reg);
        assert_eq!(p.id, builtin::DEFAULT.id);
    }

    /// 合并器拒绝重复 id（V1 纪律：配置错误显式暴露，不静默覆盖）。
    #[test]
    fn merge_rejects_duplicate_theme_ids() {
        // 同一个注册表 merge 两次 → id 重复（"antique_default" 出现两次）。
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let res = std::panic::catch_unwind(|| {
            MergedThemeRegistry::merge(&[
                &wv_theme::registry::BuiltinRegistry,
                &wv_theme::registry::BuiltinRegistry,
            ])
        });
        std::panic::set_hook(prev);
        assert!(res.is_err(), "重复 id 应 panic（配置错误显式暴露）");
    }
}
