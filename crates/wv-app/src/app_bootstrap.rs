//! GUI 装配（S3–S9）：Bevy + bevy_egui，把各系统按顺序接入。
//!
//! ## 职责
//! - `run(config)`：构造 `App`、装 `DefaultPlugins` + `EguiPlugin`，
//!   按基线 §2.13 的顺序注册 `PreUpdate` / `Update` / `EguiPrimaryContextPass` 各系统。
//! - 注册所有 `crate::*` 模块的初始化与资源；具体面板逻辑留在 `ui.rs`，
//!   命令调度与启动接管留在 `ui_shell.rs`。
//!
//! ## 边界（不做什么）
//! - **feature 门控**——整个模块只在 `--features app` 下编译，让数据层（`wv-core` /
//!   `wv-io` / `wv-edit` / `wv-terrain`）开发不受 Bevy 全量编译影响（AGENTS §7.1）。
//! - **不画 UI**——只把面板注册进 schedule；面板绘制在 `ui.rs`，快捷键在 `ui_shell.rs`。
//! - **不加载工程**——`startup_project` 为 `Some(_)` 时，由 `ui_shell::startup_load`
//!   在窗口内做异步加载并显示进度 / 失败原因。
//!
//! ## 主要导出
//! - `run(config: AppConfig)` — 启动 GUI 的唯一入口。

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::EguiPlugin;

use crate::edit::editing;
use crate::render::brush_cursor;
use crate::render::camera;
use crate::render::faction_overlays;
use crate::render::map_frame;
use crate::render::streets;
use crate::render::terrain_chunks;
use crate::render::terrain_view;
use crate::render::vector_layers::{self, VectorLayerEntities};
use crate::render::world_projection;
use crate::state::{
    BrushState, DrillState, GridConfig, PanelState, PlaybackState, ViewState, WorldProjection,
};
use crate::ui;
use crate::AppConfig;

/// 启动 GUI。`startup_project` 为 `None` 时进空编辑器（占位地形），
/// `Some(dir)` 时启动后由 `ui_shell::startup_load` 在窗口内加载并显示进度/失败原因。
pub fn run(config: AppConfig) {
    // 注意：这里**不做任何工程加载**（用户裁定：启动不主动加载地图）。
    // App 以空世界起步立刻开窗——首帧即是可交互的编辑器；
    // `-m` 的加载请求经 [`crate::ui_shell::StartupLoadRequest`] 在运行期执行，
    // 遮罩先画再阻塞加载，避免「白屏无反馈」。

    // 初始相机对准占位地形中心（根图 extent 缺省 4096²）。
    // 真正按工程内容 fit 的逻辑在 [`fit_view_for_world`]，
    // 运行期加载成功后会再次调用（相机 transform 才是主真源）。
    let (center, fit_scale) = fit_view_for_world(&wv_core::World::default());

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "World View · 地图编辑器".into(),
                    resolution: WindowResolution::new(1280, 800),
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
            }),
    )
    .add_plugins(EguiPlugin::default())
    // 播放范围：空编辑器起步先用默认值（1..180_001）；
    // 运行期加载工程成功后由 `startup_load` 按该工程 `timeline/axis.yaml` 重置。
    .insert_resource(PlaybackState {
        range_from: 1,
        range_to: 180_001,
        ..Default::default()
    })
    // 空编辑器起步：世界一律为空，`-m` 的加载请求交给 `StartupLoadRequest`
    // 在首帧遮罩之后执行（加载失败原因会以窗口内卡片展示）。
    .insert_resource(WorldProjection {
        world: wv_core::World::default(),
        project_dir: None,
        dirty: false,
        load_error: None,
    })
    // CLI `-m <目录>` 带来的启动加载请求；`None` = 空编辑器。
    .insert_resource(crate::ui_shell::StartupLoadRequest(
        config.startup_project.clone(),
    ))
    .insert_resource(ViewState {
        camera_center: center,
        scale: fit_scale,
        ..Default::default()
    })
    .init_resource::<BrushState>()
    .init_resource::<PanelState>()
    .init_resource::<GridConfig>()
    .init_resource::<world_projection::ProjectionFrame>()
    .init_resource::<camera::ActiveStroke>()
    .init_resource::<camera::PointerOverUi>()
    .init_resource::<editing::EditHistory>()
    // 校验面板 / 笔触提交 / 撤销重做 / 打开工程都读写它（`ResMut` 访问
    // 不存在的资源会在首帧 panic → 窗口无法显示）。`derive(Default)` 起步。
    .init_resource::<crate::state::ProjectDiagnostics>()
    .init_resource::<VectorLayerEntities>()
    .init_resource::<streets::StreetLayerCache>()
    .init_resource::<streets::StreetEditorState>()
    // 势力多边形层（S12）：同属「每帧 despawn+spawn 的实体集合」资源，
    // 漏 init 会在首帧 panic（`could not access system parameter`）。
    .init_resource::<faction_overlays::FactionOverlayEntities>()
    .init_resource::<brush_cursor::BrushCursorEntities>()
    .init_resource::<map_frame::MapFrameEntities>()
    .init_resource::<terrain_chunks::TerrainChunkEntities>()
    // 等值线矢量叠加层（ADR-0012 / plan v0.1 M2）：同属实体集合资源。
    .init_resource::<terrain_view::IsolineOverlay>()
    // 逐格编辑网格（plan v0.1 D4）：同上。
    .init_resource::<terrain_view::GridLinesOverlay>()
    .add_systems(
        Startup,
        // 字体装配不在这里：bevy_egui 0.42 的主上下文要等 PreUpdate 的
        // InitContexts 才创建，Startup 时 ctx_mut() 必然 Err（曾导致 egui
        // 全程豆腐块）。见 fonts.rs 文档与下方 PreUpdate 注册。
        (setup_camera, setup_terrain_texture_after_world).chain(),
    )
    .add_systems(
        PreUpdate,
        // 必须排在 bevy_egui 建上下文之后：否则首帧 ctx_mut() 对未初始化
        // 上下文调用会 panic（bevy_egui 0.32 lib.rs:437 的 expect）。
        // 字体重试式装配同口径：上下文就绪的首帧执行一次（fonts.rs 文档）。
        (
            crate::theme::fonts::setup_egui_fonts,
            camera::poll_egui_wants_input,
        )
            .after(bevy_egui::EguiPreUpdateSet::InitContexts),
    )
    .add_systems(
        Update,
        (
            // 主链已顶满 20 参上限，拆一层嵌套元组（内层先 chain 成一个整体）。
            (
                // 输入 → 视图状态
                // 朝向先行：把 MapNode.orientation 写进相机旋转，同帧的缩放锚点、
                // 平移与拾取换算读 Transform.rotation 时拿到的就是新值（帧内自洽）。
                camera::apply_map_orientation,
                camera::camera_zoom,
                camera::camera_pan,
                camera::camera_pan_keys,
                camera::brush_radius_scroll,
                camera::track_cursor_world,
                camera::collect_brush_stroke,
                // 编辑（笔触 → 命令 → History）
                editing::commit_stroke_on_release,
                streets::commit_street_click,
                // 撤销 / 重做不再在这里单独监听 Ctrl+Z：快捷键统一由
                // `ui_shell::hotkeys` 收（那里已经处理了「输入框聚焦时失效」），
                // 两处都听会导致一次按键应用两次。
                editing::flush_terrain_texture,
                // 播放推进（时间 → 当前时刻）
                world_projection::advance_playback,
                // 纹理先于网格几何：`grid.cell_size` 取自纹理的来源图与格数。
                terrain_view::rebuild_texture_on_mode_change,
                // 网格几何（`raster_origin` / `map_origin` 两套原点）必须先于所有
                // 消费它的系统：投影里的地点、街道、街道名、地图外框都按 `GridConfig`
                // 换算，顺序反了就会拿上一帧的原点画一帧——钻取那一帧最明显。
                terrain_view::sync_grid_config,
            )
                .chain(),
            // 换级淡化：旧底图淡出精灵逐帧降透明、归零回收（ADR-0009 §2⑦）。
            terrain_view::tick_basemap_fadeout,
            // 投影（时间 → 渲染资源）
            world_projection::compute_projection,
            // 等值线矢量叠加（ADR-0012 / plan v0.1 M2）：先于 chunk 层读
            // `tex.chunks_dirty`（chunk 系统会消费清零），触发重算。
            terrain_view::sync_isoline_overlay,
            // 编辑态逐格网格（plan v0.1 D4）：与等值线同管线族，先于 chunk 层。
            terrain_view::sync_grid_lines,
            // 地形分块 + mip + 视口拣选（优化项 3a/3b/3c）。
            terrain_chunks::sync_terrain_chunks,
            // 派生诊断刷新：在 flush 之后、UI 之前；面板本帧即可读新值。
            editing::refresh_project_diagnostics,
            // 街道图层缓存：当前图变化时重读（ADR-0010），先于 sync_vector_layers。
            streets::reload_street_cache,
            // 矢量叠加层（道路 + POI）：在 compute_projection 之后跑。
            vector_layers::sync_vector_layers,
            // 街道名标注：消费同一份 StreetLayerCache，在街道画完之后对齐标签。
            streets::update_street_labels,
            // 势力多边形叠加层（S12 v0.1）：在矢量层之后、地图外框之前。
            faction_overlays::sync_faction_overlays,
            // 地图外框 + 1/4 幅面参考网格（外框需地形尺寸）。
            map_frame::sync_map_frame,
        )
            .chain(),
    )
    // 主链已顶满 Bevy 系统元组的 20 参上限（all_tuples，bevy_ecs
    // function_system.rs），笔刷光标单独注册、显式排在光标取世界坐标之后。
    .add_systems(
        Update,
        brush_cursor::sync_brush_cursor.after(camera::track_cursor_world),
    )
    .add_systems(
        Update,
        (
            // 地图绘制必须在所有浮动面板**之前**执行，
            // 保证地图是最底层、面板浮在其上（基线 §1.11 R1）。
            world_projection::draw_map_gizmos,
            world_projection::draw_brush_cursor,
        ),
    )
    .add_systems(
        // bevy_egui 0.42 起 pass 是独立调度（在 PostUpdate 的 EndPass 里
        // `Context::run_ui` 内执行）：egui 首帧字体在该 pass 前就绪，
        // 放 `Update` 会在第一帧 panic「No fonts available until first call
        // to Context::run()」。世界系统留在 `Update`，先跑后画。
        bevy_egui::EguiPrimaryContextPass,
        (
            // 全部浮动面板（基线 §1.11 R1–R4：地图铺满，面板浮其上）。
            //
            // **必须 `.chain()`**：这些系统都要 `ResMut<PanelState>` 与
            // `ResMut<ViewState>`，Bevy 无法并行调度同时可变借用同一资源的系统
            // （否则运行期 panic：`could not access system parameter`）。
            ui::top_bar,
            // Google 式贴边导航控件先画，浮动面板仍可在需要时覆盖它。
            ui::map_controls,
            // 面包屑浮在地图上、压在面板之下（v0.2 §B.3）。
            ui::breadcrumb,
            ui::panel_map_tree,
            ui::panel_layers,
            ui::panel_brush,
            // ui::panel_minimap, // TODO: 恢复小地图后取消注释
            ui::panel_compass,
            ui::panel_timeline,
            ui::panel_event,
            ui::panel_situation,
            ui::panel_character,
            ui::panel_faction,
            ui::panel_inspector,
            ui::panel_validation,
            // 状态栏最后画：它读得到的 `ShellNotice` 已是本帧最终值。
            ui::status_bar,
        )
            .chain(),
    );

    // 界面外壳（主题 / 快捷键 / 命令面板 / 布局持久化）与地图钻取（v0.2 §B）。
    // 两者各自成模块并各自注册，`gui.rs` 只负责调用顺序（见各处模块文档）。
    crate::ui_shell::register(&mut app);
    crate::render::drill::register(&mut app);

    app.run();
}

/// 2D 正交相机。若启动前已装配好 `ViewState`（fit 缩放 + 根图中心），
/// 直接把它落到 Camera 的 transform / projection——**Camera 的 Transform 才是
/// 相机主真源**，`ViewState.camera_center` 只是 pan/zoom 回写的镜像，
/// 光写 ViewState 不动 transform 相机就还停在原点。
fn setup_camera(mut commands: Commands, view: Option<Res<ViewState>>) {
    let (center, scale) = view
        .as_deref()
        .map(|v| (v.camera_center, v.scale))
        .unwrap_or((Vec2::ZERO, 1.0));
    commands.spawn((
        Camera2d,
        Transform::from_xyz(center.x, center.y, 0.0),
        // 方案 C（矢量覆盖层抗锯齿）：4x MSAA。POI 圆环 / 道路条 / 地名底框
        // 都是 Mesh2d，多边形边缘靠 MSAA 平滑——纹理内部的地形边界不受益
        // （那由 terrain_view 的边界扰动负责），两者互补。
        Msaa::Sample4,
        // Bevy 0.16 起 `Camera2d` require 的是 `Projection` **枚举**组件
        // （`OrthographicProjection` 自己不再是 Component）：要覆盖默认 scale=1.0
        // 必须包一层 `Projection::Orthographic(..)`。改动出处见 ADR-0011。
        Projection::Orthographic(OrthographicProjection {
            scale,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// 地形纹理的初始化必须在 `WorldProjection` 就位之后。
///
/// 保证地形数据存在：若**当前图及其祖先**都没带栅格，用占位地形填进根图，
/// 这样笔刷编辑与纹理重建都有统一的数据源（Phase 2：地形按图存，见
/// [`terrain_view::active_terrain`]）。
fn setup_terrain_texture_after_world(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    view: Res<ViewState>,
) {
    // 瓦片图集（README §12-14）：加载失败静默回退纯色平涂。
    let atlas = terrain_view::ensure_atlas(&mut commands);
    // 原型默认 512×512 占位（基线 §2.19：1024² 亦轻松 60 FPS）。
    let (map, data) =
        terrain_view::active_terrain_or_placeholder(&mut projection.world, drill.current(), 512);
    let image =
        terrain_view::image_from_terrain(&data, view.mode, atlas.as_ref(), view.terrain_soften);
    commands.insert_resource(terrain_view::new_terrain_texture(
        &mut images,
        image,
        &data,
        map,
        view.mode,
        view.terrain_soften,
    ));
}

/// 按世界内容计算初始取景（中心 + 缩放）。
///
/// - 优先**框住根图内容**（icons 包围盒 + 25% 边距）：大图幅上内容只占
///   一小块时，按整图 fit 会把内容挤成屏幕中心的一撮点、四周全是空黑；
/// - 无内容/空世界时退回根图 extent（缺省 4096²）的中心。
///
/// Startup（`run` 装配）与运行期加载（`ui_shell::startup_load`）共用，
/// 保证「加载工程后看到的取景」与「启动即加载」完全一致。
pub(crate) fn fit_view_for_world(world: &wv_core::World) -> (Vec2, f32) {
    let root_id = world
        .meta
        .as_ref()
        .map(|m| m.root_map.clone())
        .unwrap_or_else(|| wv_core::Id::new_unchecked("world_main"));
    let root_extent = world
        .maps
        .iter()
        .find(|m| m.id == root_id)
        .and_then(|m| m.extent.as_ref())
        .map(|e| (e.width, e.height))
        .unwrap_or((4096.0f32, 4096.0));
    // 口径：marker/Place 的世界坐标 = anchor 直接乘 extent（不做 y 翻转，
    // 与 drill 标记一致）；这里只做取景，别自作聪明翻转。
    let content_box: Option<(Vec2, Vec2)> =
        world.maps.iter().find(|m| m.id == root_id).and_then(|m| {
            let (w, h) = m
                .extent
                .as_ref()
                .map(|e| (e.width, e.height))
                .unwrap_or((4096.0, 4096.0));
            let pts: Vec<Vec2> = m
                .icons
                .iter()
                .filter_map(|ic| ic.anchor.map(|a| Vec2::new(a[0] * w, a[1] * h)))
                .collect();
            (pts.len() >= 2).then(|| {
                let lo = pts.iter().copied().fold(Vec2::MAX, Vec2::min);
                let hi = pts.iter().copied().fold(Vec2::MIN, Vec2::max);
                (lo, hi)
            })
        });
    fit_for_content(root_extent, content_box)
}

/// 纯几何部分：给定根图 extent 与内容包围盒，算 (center, scale)。
///
/// `ViewState.scale` 的口径是「世界单位 / 像素」（screen_to_world 乘它），
/// 所以 fit = 图宽 / 视口宽，取两轴较紧的一个；缩放不小于 1（避免比像素还细）。
fn fit_for_content(root_extent: (f32, f32), content_box: Option<(Vec2, Vec2)>) -> (Vec2, f32) {
    match content_box {
        Some((lo, hi)) => {
            let size = hi - lo;
            let margin = size.max_element() * 0.25 + 64.0;
            let span = size + Vec2::splat(margin * 2.0);
            (
                (lo + hi) * 0.5,
                (span.x / 1280.0).max(span.y / 800.0).max(1.0),
            )
        }
        None => (
            Vec2::splat(root_extent.0 * 0.5),
            (root_extent.0 / 1280.0).max(root_extent.1 / 800.0).max(1.0),
        ),
    }
}

#[cfg(test)]
mod fit_view_tests {
    use super::fit_for_content;
    use bevy::math::Vec2;

    #[test]
    fn empty_world_falls_back_to_centered_4096_view() {
        let (center, scale) = fit_for_content((4096.0, 4096.0), None);
        assert_eq!(center, Vec2::splat(2048.0), "center {center:?}");
        // 4096 / 800 = 5.12（取两轴较紧的一轴）。
        assert!((scale - 5.12).abs() < 1e-4, "got {scale}");
    }

    #[test]
    fn content_box_centers_the_view_with_25_percent_margin() {
        // 内容 640×400 位于 (0,0)-(640,400)：
        // margin = 640*0.25 + 64 = 224；span = (1088, 848)；
        // scale = max(1088/1280, 848/800) = 1.06；center = (320, 200)。
        let (center, scale) = fit_for_content(
            (4096.0, 4096.0),
            Some((Vec2::ZERO, Vec2::new(640.0, 400.0))),
        );
        assert_eq!(center, Vec2::new(320.0, 200.0), "center {center:?}");
        assert!((scale - 1.06).abs() < 1e-4, "got {scale}");
    }

    #[test]
    fn tiny_content_is_not_zoomed_past_one_to_one() {
        // 内容只有 10×10：span ≈ 10 + 2*(2.5+64) = 143；两轴 fit 都 < 1，
        // 缩放下限 1.0 兜底（世界单位 ≈ 像素时不再放大）。
        let (_, scale) = fit_for_content((4096.0, 4096.0), Some((Vec2::ZERO, Vec2::splat(10.0))));
        assert!((scale - 1.0).abs() < 1e-6, "got {scale}");
    }
}
