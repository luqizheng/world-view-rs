//! `App` 的插件装配：窗口 / 日志 / 图像采样 / 渲染材质 / egui / 诊断。

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::EguiPlugin;

/// 装上运行编辑器所需的全部插件。
///
/// 分出来只是为了让 [`run`](super::run) 读起来是「插件 → 资源 → 系统」三段，
/// 不是一张 800 行的单子；装配内容与顺序保持原样。
pub(super) fn configure(app: &mut App) {
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    // 窗口标题走 i18n（`WelcomeHeading`）：此前是硬编码中文串，
                    // 英文界面下标题栏仍显示中文。启动语言取 CLI `--language`
                    // （`cli_locale()`，未指定时为 zh）；运行期切换语言由
                    // [`sync_window_title_with_locale`] 跟随。
                    title: crate::i18n::t(
                        crate::i18n::cli_locale(),
                        crate::i18n::I18nKey::WelcomeHeading,
                    )
                    .to_owned(),
                    resolution: WindowResolution::new(1280, 800),
                    // 隐藏起步（2026-10-17，修「启动数秒白屏」）：原生窗口一旦
                    // 创建即以系统白底示人，而 wgpu 初始化、字体 atlas、管线
                    // 编译全都发生在窗口已显示之后——首帧 present 之前用户只能
                    // 干瞪白屏。改为隐藏创建，帧 1 末由
                    // [`reveal_window_once_first_frame_presented`] 揭示：窗口亮起
                    // 时首帧内容已在交换链上，第一眼就是欢迎页 / 加载遮罩。
                    // （bevy_winit 对全隐藏窗口会强制驱动更新帧，事件循环不卡。）
                    visible: false,
                    ..Default::default()
                }),
                ..Default::default()
            })
            // 全局保持最近邻采样；Google 模式的 chunk 纹理会单独切线性采样。
            .set(ImagePlugin::default_nearest())
            // icu_provider 对「中日文行断无字典模型」（parley 0.9 + icu_segmenter 2.3
            // 的上游缺口，LineSegmenter 的 compiled_data 构造器不含 CJ 字典）每次
            // 都 log::warn 一条并回退成整串不断行；编辑器中文标签极多，会每帧刷屏，
            // 压到 error 级。回退行为对单行标签无害；长段落折行缺失见 ADR-0011。
            .set(bevy::log::LogPlugin {
                filter: "naga=warn,wgpu=error,icu_provider=off".into(),
                level: bevy::log::Level::INFO,
                ..Default::default()
            })
            // 编辑器无任何音频功能：不装 AudioPlugin。理由（2026-10-20 实测）：
            // 音频设备中途切换 / 摘除时 rodio 抛 `audio stream error: The
            // requested device is no longer available`，进程随之退出且日志只剩
            // 启动行——用户在长时间看图时被无预警关窗。拔掉这个永不使用的
            // 插件即整类崩溃源消失（未来加音效时再按需装回来）。
            .build()
            .disable::<bevy::audio::AudioPlugin>(),
    )
    .add_plugins(EguiPlugin::default())
    // 逐格网格材质（2026-10-23）：单 quad + WGSL 双档网格，缩放零重建。
    // 材质属 `wv-world-cell-map`（bevy 非 optional），shader 由 `load_internal_asset!`
    // 编译期内联；装配点留在组合根（wv-app），桥接系统只建 mesh / 挂实体。
    .add_plugins(wv_render::terrain_view::grid_lines::GridLinesMaterialPlugin)
    // 地形场材质（ADR-0053 D1，Phase 1 接线 2026-10-24）：GPU 逐像素底图。
    // 与网格材质同款分工——材质 / WGSL 属 `wv-world-cell-map`，桥接系统在
    // `wv-render::cellmap_bridge`，**装配点留在组合根**（AGENTS §2.1）。
    // ⚠️ 注册不可省：`sync_terrain_field` 取 `ResMut<Assets<TerrainFieldMaterial>>`，
    // 没有 `Material2dPlugin` 时资源不存在，系统初始化即 panic。
    .add_plugins(wv_world_cell_map::terrain_view::field::TerrainFieldMaterialPlugin)
    // 状态栏 FPS 读数（Figma #3:21001 右段）：DefaultPlugins 只提供
    // DiagnosticsStore 容器，帧时间/帧率诊断要本插件显式注册。
    .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default());
}
