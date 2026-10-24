//! 主窗口的启动细节：隐藏起步的揭示、标题跟随界面语言。

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// 隐藏起步的揭示：第 2 个 Update tick 把主窗口放出来（一次性）。
///
/// ## 时序（与 [`crate::ui_shell::drive_project_load`] 的后台加载咬合）
///
/// | 帧 | 发生什么 |
/// |---|---|
/// | 0 | 窗口隐藏创建（`Last` 里建 winit 窗口）；PreUpdate 装配 egui 字体、**派发后台加载**；pass 画欢迎页/遮罩；render 编译管线并 present（不可见） |
/// | 1 | Update 本系统翻转 `visible` → `Last` 应用 `set_visible(true)`，窗口亮起时屏上是帧 0 的完整画面 |
/// | 1 起 | 后台线程在读盘；主线程每帧轮询，结果到手即接管（遮罩全程在屏上转圈） |
///
/// ## 为什么是帧 1 而不是更早 / 更晚
/// - 帧 0 翻转会在窗口刚建好、尚无任何 present 时就揭白底——白屏回来了；
/// - 更晚（帧 2+）只会让用户多等，帧 0/1 的画面内容已无差别
///   （字体 atlas 与 egui 管线都在帧 0 建齐）。
///
/// ## 为什么不用 `PipelineCache` 就绪信号
/// 渲染管线的编译状态在渲染子世界，主世界拿不到（ADR-0020 后 render 已是
/// 独立 crate 边界）；「多跑一帧」以 8~16ms 的代价换同等保证，简单一个量级。
pub(super) fn reveal_window_once_first_frame_presented(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    (mut ticks, mut done): (Local<u32>, Local<bool>),
) {
    *ticks += 1;
    if *done || *ticks < 2 {
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        if !window.visible {
            window.visible = true;
        }
        *done = true;
    }
    // 窗口实体尚不存在（理论上不会：WindowPlugin 在插件构建期就生成主窗口）
    // 时不置 `done`，下一 tick 重试。
}

/// 窗口标题跟随界面语言切换（2026-10-22）。
///
/// `I18nKey::WelcomeHeading`（「World View · 地图编辑器」/「World View · Map editor」）
/// 此前登记在文案表里却**无任何调用点**——窗口标题一直是 `app_bootstrap` 里
/// 硬编码的中文串，英文界面下标题栏仍显示中文。本系统把
/// [`CurrentLocale`](crate::i18n::CurrentLocale) 的变化映射到主窗口标题，
/// 使标题与界面语言一致。
///
/// 幂等：只在语言**真的变了**时写标题（`Local` 缓存上次值），避免每帧
/// 触碰窗口导致无谓的重绘 / 事件。
pub(super) fn sync_window_title_with_locale(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    loc: Res<crate::i18n::CurrentLocale>,
    mut last: Local<Option<crate::i18n::Locale>>,
) {
    if last.as_ref() == Some(&loc.0) {
        return;
    }
    *last = Some(loc.0);
    let title = crate::i18n::t(loc.0, crate::i18n::I18nKey::WelcomeHeading);
    if let Ok(mut window) = windows.single_mut() {
        window.title = title.to_owned();
    }
}
