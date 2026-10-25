//! 风格包解析（ADR-0066）——单风格时代恒定指向唯一默认包。
//!
//! ## 之前是什么
//!
//! ADR-0023 M2 落地时，这里是一套「多主题注册表」机制：
//! `ViewState.theme_pack_id` → `MergedThemeRegistry::lookup` → 命中的
//! `ThemePack`，并由 `sync_active_theme_pack` 在 `PreUpdate` 最前写进
//! `ViewState.active_theme`。外部主题 crate（`wv-theme-chinese-ink`）经
//! `ThemePackRegistry` 注入这张表。
//!
//! ## 现在是什么
//!
//! 主理人 2026-10-25 裁定「软件不需要做 theme 的贴图功能」后，**多主题机制
//! 整体退役**（ADR-0066）：注册表、外部风格 crate、暮色档一并删除，只留
//! 一个默认风格。
//!
//! 但**运行期权威仍是一个 `&'static ThemePack`**——渲染层有 40+ 处读
//! `view.active_theme.palette` / `.treatment` / `.sea`，把这个字段拆掉会
//! 波及整个渲染管线。保留它的代价只是「单值恒等」，收益是**风格数据仍只有
//! 一个家**（`wv_default_theme`），渲染层不持有任何风格副本。
//!
//! ## 边界
//!
//! - **不做热切换脏标传播**：风格恒定，无从切换；
//! - **不做运行时加载**：风格是编译期常量；
//! - **不消费 `Box<dyn>`**：仍是 `&'static` 引用（ADR-0023 D2 §2 的性能前提）。

use bevy::prelude::*;

use crate::state::ViewState;
use wv_default_theme::theme_pack::{builtin, ThemePack};

/// 解析当前应生效的风格包。
///
/// **恒返回唯一默认包**（ADR-0066）：多主题机制退役后没有别的可解析。
///
/// 参数保留是为了让调用点（`sync_active_theme_pack` / 未来的多风格扩展）
/// 不必改签名——单值时代它被忽略。
pub fn resolve_pack(_view: &ViewState) -> &'static ThemePack {
    &builtin::DEFAULT
}

/// 风格包同步系统：把 `ViewState.active_theme` 钉在唯一默认包上。
///
/// **只在值不同才写**：`ResMut` 写入会把 Resource 标记为 changed，每帧写
/// 会让下游「缓存键比对」类逻辑误判为「风格变了」而白白重算。
///
/// 调度位置：必须**先于**所有消费点。本 system 挂在 `PreUpdate` 最前，
/// 而渲染消费点在 `Update`——PreUpdate 一定先于 Update；唯一同在 PreUpdate
/// 的消费者（`poll_open_project_dialog`）由 app 侧显式 `.before()` 保证。
pub fn sync_active_theme_pack(mut view: ResMut<ViewState>) {
    let pack = resolve_pack(&view);
    if view.active_theme.id != pack.id {
        view.active_theme = pack;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无条件解析到唯一默认包。
    #[test]
    fn resolve_pack_always_returns_the_single_default() {
        let v = ViewState::default();
        let p = resolve_pack(&v);
        assert_eq!(p.id, builtin::DEFAULT.id, "单风格：恒为默认包");
        assert_eq!(p.id, "default", "默认包 id 稳定");
    }

    /// 同步系统把 `active_theme` 钉在默认包上——即使有人手改过。
    #[test]
    fn sync_pins_active_theme_to_the_default_pack() {
        let mut app = App::new();
        app.init_resource::<ViewState>();
        app.add_systems(Update, sync_active_theme_pack);
        app.update();
        let view = app.world().resource::<ViewState>();
        assert_eq!(view.active_theme.id, builtin::DEFAULT.id);
    }
}
