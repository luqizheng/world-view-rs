//! GUI 装配（S3–S9）：Bevar + bevy_egui，把各系统按顺序接入。
//!
//! ## 系统顺序（基线 §2.13）
//!
//! - `poll_egui_wants_input` 在 `PreUpdate`：**先**知道 egui 要不要输入，
//!   地图输入系统读的就是这个（避免指针落在面板上还去拾取地图）。
//! - 地图输入/投影在 `Update`。
//! - egui 面板在 `Update` 的 `EguiPrimaryContextPass` 阶段（bevy_egui 0.32 约定）。
//!
//! ## feature 门控
//!
//! 整个模块只在 `--features app` 下编译。这是为了让数据层（`wv-core` /
//! `wv-io` / `wv-edit` / `wv-terrain`）的开发不受 Bevy 全量编译影响。

use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_egui::EguiPlugin;

use crate::camera;
use crate::editing;
use crate::projection;
use crate::state::{BrushState, GridConfig, PanelState, PlaybackState, ViewState, WorldProjection};
use crate::terrain_view::{self, TerrainTexture};
use crate::ui;
use crate::AppConfig;

/// 启动 GUI。`project_dir` 为 `None` 时用内置占位地形。
pub fn run(config: AppConfig) {
    // 加载工程（如果给了目录）。加载失败时**明确报告**并退化为占位地形。
    //
    // 注意：不要只写 `bevy::log::error!`——窗口刚创建时日志一闪而过，
    // 用户会以为程序崩溃。这里同时（a）打印到 stderr，（b）存入资源供 UI 显示。
    let mut load_error: Option<String> = None;
    let world = match &config.project_dir {
        Some(dir) => match wv_io::Project::open(dir) {
            Ok(project) => match project.load_world() {
                Ok(w) => w,
                Err(e) => {
                    let msg = format!("加载工程 `{dir}` 失败: {e}（已退化为空白占位地形）");
                    eprintln!("[wv] 错误: {msg}");
                    load_error = Some(msg);
                    Default::default()
                }
            },
            Err(e) => {
                let msg = format!(
                    "打开工程 `{dir}` 失败: {e}\n\
                     提示: 若在 `target/debug` 下启动，请改用 `wv gui <工程目录>`，\
                     或设置环境变量 `WV_PROJECT`，或在工程根目录下运行。"
                );
                eprintln!("[wv] 错误: {msg}");
                load_error = Some(msg);
                Default::default()
            }
        },
        None => Default::default(),
    };

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "World View · 地图编辑器".into(),
                    resolution: WindowResolution::new(1280.0, 800.0),
                    ..Default::default()
                }),
                ..Default::default()
            })
            // 编辑器是工具，不是游戏：图像用最近邻采样，避免色块被平滑成糊。
            .set(ImagePlugin::default_nearest()),
    )
    .add_plugins(EguiPlugin)
    .insert_resource(WorldProjection {
        world,
        project_dir: config.project_dir.clone(),
        dirty: false,
        load_error,
    })
    .init_resource::<PlaybackState>()
    .init_resource::<ViewState>()
    .init_resource::<BrushState>()
    .init_resource::<PanelState>()
    .init_resource::<GridConfig>()
    .init_resource::<projection::ProjectionFrame>()
    .init_resource::<camera::ActiveStroke>()
    .init_resource::<camera::PointerOverUi>()
    .init_resource::<editing::EditHistory>()
    .add_systems(
        Startup,
        (
            // 字体必须最先装配：egui 面板首次渲染前需就位，否则中文是方块。
            crate::fonts::setup_egui_fonts,
            setup_camera,
            setup_terrain_texture_after_world,
        )
            .chain(),
    )
    .add_systems(
        PreUpdate,
        // 必须排在 bevy_egui 建上下文之后：否则首帧 ctx_mut() 对未初始化
        // 上下文调用会 panic（bevy_egui 0.32 lib.rs:437 的 expect）。
        camera::poll_egui_wants_input.after(bevy_egui::EguiPreUpdateSet::InitContexts),
    )
    .add_systems(
        Update,
        (
            // 输入 → 视图状态
            camera::camera_zoom,
            camera::camera_pan,
            camera::camera_pan_keys,
            camera::brush_radius_scroll,
            camera::track_cursor_world,
            camera::collect_brush_stroke,
            // 编辑（笔触 → 命令 → History）
            editing::commit_stroke_on_release,
            // 撤销 / 重做不再在这里单独监听 Ctrl+Z：快捷键统一由
            // `ui_shell::hotkeys` 收（那里已经处理了「输入框聚焦时失效」），
            // 两处都听会导致一次按键应用两次。
            editing::flush_terrain_texture,
            // 投影（时间 → 渲染资源）
            projection::advance_playback,
            projection::compute_projection,
            terrain_view::rebuild_texture_on_mode_change,
            terrain_view::sync_grid_config,
        )
            .chain(),
    )
    .add_systems(
        Update,
        (
            // 地图绘制必须在所有浮动面板**之前**执行，
            // 保证地图是最底层、面板浮在其上（基线 §1.11 R1）。
            projection::draw_map_gizmos,
            projection::draw_brush_cursor,
        ),
    )
    .add_systems(
        Update,
        (
            // 全部浮动面板（基线 §1.11 R1–R4：地图铺满，面板浮其上）。
            //
            // **必须 `.chain()`**：这些系统都要 `ResMut<PanelState>` 与
            // `ResMut<ViewState>`，Bevy 无法并行调度同时可变借用同一资源的系统
            // （否则运行期 panic：`could not access system parameter`）。
            ui::top_bar,
            // 面包屑浮在地图上、压在面板之下（v0.2 §B.3）。
            ui::breadcrumb,
            ui::panel_map_tree,
            ui::panel_layers,
            ui::panel_brush,
            ui::panel_minimap,
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
    crate::drill::register(&mut app);

    app.run();
}

/// 2D 正交相机。初始缩放 1×（整图入窗，基线 §1.16）。
fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// 地形纹理的初始化必须在 `WorldProjection` 就位之后。
///
/// 保证地形数据存在：若工程没带 `terrain`，用占位地形填进世界，
/// 这样笔刷编辑与纹理重建都有统一的数据源。
fn setup_terrain_texture_after_world(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut projection: ResMut<WorldProjection>,
    view: Res<ViewState>,
) {
    if projection.world.terrain.is_none() {
        // 原型默认 512×512 占位（基线 §2.19：1024² 亦轻松 60 FPS）。
        projection.world.terrain = Some(terrain_view::placeholder_terrain(512, 512));
    }
    let data = projection.world.terrain.clone().unwrap();
    let image = terrain_view::image_from_terrain(&data, view.mode);
    let handle = images.add(image);
    commands.insert_resource(TerrainTexture {
        handle,
        size: data.width.max(1),
        dirty: None,
        last_mode: Some(view.mode),
    });
}
