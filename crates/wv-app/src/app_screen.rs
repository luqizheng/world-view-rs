//! 应用屏状态（B 档：把「现在该显示什么」收敛到**一处**求值）。
//!
//! ## 为什么要有这个模块
//!
//! 旧写法是每个 UI 系统自己拼布尔合取：
//!
//! ```text
//! welcome_screen:  has_open_project() || startup_req.is_some() || load_error.is_some()
//! map_controls:    !has_open_project() || startup_req.is_some()
//! intent_menu:     !has_open_project() || startup_req.is_some()
//! city_intent:     !has_open_project()
//! ……
//! ```
//!
//! 同一个问题被写了七遍，每遍取的项还不完全一样——于是「加载失败时欢迎页
//! 盖住错误卡」「暗层盖住卡片」这类回归反复出现（两次已发生，根因都是
//! 「谁该显示」没有单一裁决点）。
//!
//! 新写法：事实只在一处求值（[`current_screen`]），每个 UI 问**自己关心的
//! 那一个问题**（`shows_welcome()` / `editor_visible()` / `editor_ready()` …）。
//!
//! ## 为什么不是一个枚举而是三个事实
//!
//! 「正在加载」是**叠加态**：换工程时旧地图还在屏上，工具条不该消失。
//! 若把状态压成单枚举（`Loading` 盖过 `Editor`），加载期间编辑器表面会被
//! 整体卸掉——那是新引入的 UX 回退。故本模块用三个正交事实 + 若干判定，
//! 让每个调用点拿到与旧行为**逐条等价**的答案。
//!
//! ## 与「场景」无关
//!
//! 这里的「屏」是**应用外壳层**的概念（见 GLOSSARY「启动屏 / 欢迎页 /
//! 主界面」）。它与场景编辑器里的「场景」（= 一张地点图）毫无关系。

use bevy::prelude::*;

use crate::project_loader::{PendingProjectLoad, ProjectLoadTask};
use crate::state::WorldProjection;
use crate::ui_shell::StartupLoadRequest;

/// 应用当前状态——**每帧**由 [`sync_app_screen`] 求值一次。
///
/// 三条事实正交，各自独立求值：
/// - `has_project`：手握工程（= `WorldProjection::has_open_project()`）；
/// - `loading`：有工程正在后台加载（含排队中）；
/// - `load_error`：上一次加载失败（错误卡片要显示原因——V1：不静默退化）。
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppScreen {
    /// 手握工程：编辑器表面可用。
    pub has_project: bool,
    /// 正在加载（遮罩显示中）。**叠加态**——不夺走底下的表面。
    pub loading: bool,
    /// 加载失败（错误卡片显示中）。
    pub load_error: bool,
}

impl AppScreen {
    /// 欢迎页接管全屏吗？
    ///
    /// 旧口径：`!has_open_project() && !startup_req.is_some() && load_error.is_none()`。
    pub fn shows_welcome(self) -> bool {
        !self.has_project && !self.loading && !self.load_error
    }

    /// 加载遮罩显示中吗？
    pub fn shows_loading_overlay(self) -> bool {
        self.loading
    }

    /// 加载失败卡片显示中吗？
    pub fn shows_error_card(self) -> bool {
        self.load_error
    }

    /// 编辑器表面（工具条 / 意图菜单 / 面板）显示中吗？
    ///
    /// 旧口径：`has_open_project()`。**换工程加载期间仍显示**——旧地图还在
    /// 屏上，把工具条一起卸掉是 UX 回退。
    pub fn editor_visible(self) -> bool {
        self.has_project
    }

    /// 编辑器表面**可操作**吗（可改数据 / 可保存）？
    ///
    /// 旧口径：`has_open_project()`。这里多挡一层「加载中」：换工程期间
    /// 世界即将被替换，此时落盘会写错工程（改的是旧世界、存的是新目录）。
    pub fn editor_ready(self) -> bool {
        self.has_project && !self.loading
    }
}

/// 求当前屏状态（**纯函数**——无 Bevy 依赖，可直接单测）。
///
/// 事实来源三处：
/// - [`WorldProjection`]：有没有工程、上次有没有加载失败；
/// - [`StartupLoadRequest`]：CLI `-m` 的加载请求（还没派发也算「在加载」）；
/// - [`PendingProjectLoad`] / [`ProjectLoadTask`]：排队中 / 后台在跑。
pub fn current_screen(
    projection: &WorldProjection,
    startup_req: &StartupLoadRequest,
    pending: &PendingProjectLoad,
    task: &ProjectLoadTask,
) -> AppScreen {
    AppScreen {
        has_project: projection.has_open_project(),
        loading: startup_req.0.is_some() || pending.0.is_some() || task.is_active(),
        load_error: projection.load_error.is_some(),
    }
}

/// 每帧刷新 [`AppScreen`] 资源（PreUpdate，排在工程加载驱动之后）。
pub fn sync_app_screen(
    mut screen: ResMut<AppScreen>,
    projection: Res<WorldProjection>,
    startup_req: Res<StartupLoadRequest>,
    pending: Res<PendingProjectLoad>,
    task: Res<ProjectLoadTask>,
) {
    *screen = current_screen(&projection, &startup_req, &pending, &task);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_of(has_project: bool, loading: bool, load_error: bool) -> AppScreen {
        AppScreen {
            has_project,
            loading,
            load_error,
        }
    }

    /// 无工程、无加载、无错误 → 欢迎页接管。
    #[test]
    fn idle_app_shows_the_welcome_screen() {
        let s = screen_of(false, false, false);
        assert!(s.shows_welcome());
        assert!(!s.editor_visible());
        assert!(!s.editor_ready());
        assert!(!s.shows_loading_overlay());
        assert!(!s.shows_error_card());
    }

    /// 加载中 → 遮罩显示，欢迎页让位（但**不**夺走已有工程的编辑器表面）。
    #[test]
    fn loading_hides_the_welcome_screen_but_keeps_the_editor_surface() {
        let s = screen_of(false, true, false);
        assert!(!s.shows_welcome(), "加载期间欢迎页必须让位给遮罩");
        assert!(s.shows_loading_overlay());

        // 换工程场景：手上有旧工程 + 正在加载新工程。
        let s = screen_of(true, true, false);
        assert!(
            s.editor_visible(),
            "加载期间编辑器表面仍应显示（旧地图还在屏上）"
        );
        assert!(
            !s.editor_ready(),
            "加载期间编辑器表面不可操作（会写错工程）"
        );
    }

    /// 加载失败 → 错误卡片显示，欢迎页让位（**不**静默退化成空白）。
    #[test]
    fn load_error_shows_the_error_card_and_hides_the_welcome_screen() {
        let s = screen_of(false, false, true);
        assert!(s.shows_error_card());
        assert!(
            !s.shows_welcome(),
            "报错时欢迎页必须让位，否则用户看不到失败原因"
        );
    }

    /// 有工程 → 编辑器表面可操作。
    #[test]
    fn project_in_hand_makes_the_editor_ready() {
        let s = screen_of(true, false, false);
        assert!(s.editor_visible());
        assert!(s.editor_ready());
        assert!(!s.shows_welcome());
    }
}
