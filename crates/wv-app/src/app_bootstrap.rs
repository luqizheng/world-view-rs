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
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy_egui::EguiPlugin;

use crate::edit::editing;
use crate::state::{
    BrushState, DrillState, GridConfig, PanelState, PlaybackState, ViewState, WorldProjection,
};
use crate::theme::ui_tokens::ShellPalette;
use crate::ui;
use crate::AppConfig;
use wv_render::brush_cursor;
use wv_render::buildings;
use wv_render::camera;
use wv_render::faction_overlays;
use wv_render::feature_overlays;
use wv_render::layer_frame;
use wv_render::map_frame;
use wv_render::streets;
use wv_render::terrain_chunks;
use wv_render::terrain_decor;
use wv_render::terrain_view;
use wv_render::vector_layers::{self, VectorLayerEntities};
use wv_render::world_projection;

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
            }),
    )
    .add_plugins(EguiPlugin::default())
    // 状态栏 FPS 读数（Figma #3:21001 右段）：DefaultPlugins 只提供
    // DiagnosticsStore 容器，帧时间/帧率诊断要本插件显式注册。
    .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
    // 清屏色对齐外壳暗色 bg_app：隐藏揭示若与 present 有毫秒级竞争，
    // 用户瞥见的也是深底而非默认灰/白（防御性，正常时序下被欢迎页盖住）。
    .insert_resource({
        let bg = ShellPalette::DARK.bg_app;
        ClearColor(Color::srgb(bg.r, bg.g, bg.b))
    })
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
        selection: None,
    })
    // v0.3 状态栏徽标用：默认 0 警告，由 validation_actions 在 Update
    // 链每帧 insert_resource 覆盖。资源必须存在以让 status_bar 系统能取到。
    .insert_resource(crate::ui::ValidationCount::default())
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
    // 「新建地图」对话框状态（ui_shell 分发器与对话框系统共用）。
    .init_resource::<ui::NewMapDialog>()
    // 「新建工程」对话框状态（欢迎页 / 分发器 / 目录轮询三方共用；
    // 资源本体在 ui_shell::register 里也有 init，幂等，这里注册是就近可读）。
    .init_resource::<ui::NewProjectDialog>()
    .init_resource::<world_projection::ProjectionFrame>()
    // 地点缓存索引（A1）：compute_projection 的端点解析 / POI 收集查表用。
    // 渲染主链在本 crate 直装（不走 MapRenderPlugin），这里必须同注册。
    .init_resource::<wv_render::world_projection::PlaceIndex>()
    // 图层帧（ADR-0013）：`MapNode.layers[].z/visible/opacity` 的每帧投影。
    .init_resource::<layer_frame::LayerFrame>()
    .init_resource::<camera::ActiveStroke>()
    .init_resource::<camera::PointerOverUi>()
    .init_resource::<editing::EditHistory>()
    // 编辑意图桥（ADR-0020 §2.5）：渲染层产意图、本 crate 翻译提交、回执回填。
    .init_resource::<wv_render::editing_bridge::EditIntents>()
    .init_resource::<wv_render::editing_bridge::EditReceipts>()
    .init_resource::<wv_render::streets::StreetCommitAcks>()
    .init_resource::<wv_render::measurement::MeasureCommitAcks>()
    // 校验面板 / 笔触提交 / 撤销重做 / 打开工程都读写它（`ResMut` 访问
    // 不存在的资源会在首帧 panic → 窗口无法显示）。`derive(Default)` 起步。
    .init_resource::<crate::state::ProjectDiagnostics>()
    .init_resource::<VectorLayerEntities>()
    .init_resource::<buildings::BuildingOverlays>()
    .init_resource::<terrain_view::atmosphere::AtmosphereLayer>()
    .init_resource::<streets::StreetLayerCache>()
    .init_resource::<streets::StreetEditorState>()
    .init_resource::<crate::state::MeasureState>()
    // 选择工具会话态（2026-10-17 选择工具 spec；drill::register 也会 init——
    // 这里显式补一份，资源幂等，双保险防装配顺序遗漏）
    .init_resource::<wv_render::state::SelectionState>()
    // 单格编辑工具会话态（[ADR-0040]）：commit_edit_cell_on_click 与
    // apply_pending_commands 都 ResMut 它——漏 init 即启动期 panic
    // （2026-10-20 实机验收抓到：测试 fixture 手动 init 掩盖了装配遗漏）。
    .init_resource::<wv_render::state::EditCellState>()
    .init_resource::<ui::ContinentDialog>()
    .init_resource::<ui::city_intent::CityIntentDialog>()
    // 森林意图的命名对话框（选择工具 M4）：环 + 钉住图 id 自持在对话框里。
    .init_resource::<ui::FactionDivisionDialog>()
    .init_resource::<ui::FeaturePlaceDialog>()
    .init_resource::<wv_render::measurement::MeasurementCache>()
    // 整图后台合成任务槽（CONTINUOUS_LOD M3）：落地校验在主线程读取。
    .init_resource::<wv_render::terrain_view::rebuild::PendingTerrainSynth>()
    // 主题包（ADR-0023 M1）：当前 `map_palette` 仍为权威源，本 Resource 仅作
    // 入口就位、不被任何渲染系统消费；M2 切消费点时再写回 `view.theme_pack_id`。
    .init_resource::<wv_render::theme::theme_pack::ActiveThemePack>()
    // 渲染配方（ADR-0021 D1）：每帧 PreUpdate 末尾单点求值，渲染 gate 改读
    // `recipe.enabled(PassId::X)` 替代散落的 `matches!(view.mode, ...)`。
    .init_resource::<wv_render::recipe::RenderRecipe>()
    // 势力多边形层（S12）：同属「每帧 despawn+spawn 的实体集合」资源，
    // 漏 init 会在首帧 panic（`could not access system parameter`）。
    .init_resource::<faction_overlays::FactionOverlayEntities>()
    // 势力图呈现（ADR-0027 D6）：当前层 / 其他层边界 / 声称 / focus 的会话状态。
    .init_resource::<faction_overlays::FactionDisplay>()
    // 事实地界覆盖层（theme overlay）：与势力层同族的「每帧 despawn+spawn」
    // 实体集合资源，漏 init 会在首帧 panic。
    .init_resource::<feature_overlays::FeatureOverlayEntities>()
    // 地形装饰（ADR-0031）：六桶合批池 + 每桶材质，惰性创建。
    .init_resource::<terrain_decor::TerrainDecorPools>()
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
    );
    // LLM 可控编辑（ADR-0014 / 0016）：仅 `--features http` 时启用。单独
    // `add_plugins` 调用（不进表达式链）——`#[cfg(feature = "http")]` 属性
    // 不能裸放在 `.method()` 调用位置（`#` 会让解析器把后续当作属性参数）。
    // 上面的 `;` 把 Startup 链断在这里：链断是必须的——若在 `.add_systems(Startup, ...)`
    // 末尾不加分号，下面的 `#[cfg]` 块会被解析器视为链上的下一项而失败。
    // 配置（`bind` / `port`）由 CLI `--http-bind` / `--http-port` 注入
    // （v0.2 增量）；默认 `127.0.0.1:8732` 走 [`HttpServerConfig::default()`]。
    #[cfg(feature = "http")]
    app.add_plugins(crate::http_integration::HttpServerPlugin {
        config: config.http_server_config.clone(),
    });
    app.add_systems(
        PreUpdate,
        // 必须排在 bevy_egui 建上下文之后：否则首帧 ctx_mut() 对未初始化
        // 上下文调用会 panic（bevy_egui 0.32 lib.rs:437 的 expect）。
        // 字体重试式装配同口径：上下文就绪的首帧执行一次（fonts.rs 文档）。
        // 图标 loader 同口径：bytes:// URI 没这两个 loader 全部变红三角。
        (
            crate::theme::fonts::setup_egui_fonts,
            crate::ui::setup_egui_image_loaders,
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
                // 单格编辑（[ADR-0040]）：点击 → cell_pick → EditCellState.selected
                editing::commit_edit_cell_on_click,
                streets::commit_street_click,
                // 撤销 / 重做不再在这里单独监听 Ctrl+Z：快捷键统一由
                // `ui_shell::hotkeys` 收（那里已经处理了「输入框聚焦时失效」），
                // 两处都听会导致一次按键应用两次。
                editing::flush_terrain_texture,
                // 播放推进（时间 → 当前时刻）
                world_projection::advance_playback,
                // 纹理先于网格几何：`grid.cell_size` 取自纹理的来源图与格数。
                terrain_view::rebuild_texture_on_mode_change,
                // 后台合成落地（M3）：必须在重建触发之后、网格几何之前——
                // 它写 tex.size，`sync_grid_config` 同帧要读到。
                terrain_view::land_pending_terrain_synth,
                // 网格几何（`raster_origin` / `map_origin` 两套原点）必须先于所有
                // 消费它的系统：投影里的地点、街道、街道名、地图外框都按 `GridConfig`
                // 换算，顺序反了就会拿上一帧的原点画一帧——钻取那一帧最明显。
                terrain_view::sync_grid_config,
            )
                .chain(),
            // 换级淡化：旧底图淡出精灵逐帧降透明、归零回收（ADR-0009 §2⑦）。
            terrain_view::tick_basemap_fadeout,
            // 图层帧（ADR-0013）：先于一切消费它的渲染系统求值。
            layer_frame::resolve_layer_frame,
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
            // 地物符号 PNG 发起加载（一次）。
            wv_render::vector_layers::ensure_landmark_png_loaded,
            // 矢量叠加层（道路 + POI）：在 compute_projection 之后跑。
            vector_layers::sync_vector_layers,
            // 街道名标注：消费同一份 StreetLayerCache，在街道画完之后对齐标签。
            streets::update_street_labels,
            // 势力多边形叠加层（S12 v0.1）：在矢量层之后、地图外框之前。
            faction_overlays::sync_faction_overlays,
            // 事实地界覆盖层（theme overlay）：地盘随 (图, 日, 缩放) 求值，
            // 排在势力层之后、地图外框之前（z 由各层常量决定，与顺序无关）。
            feature_overlays::sync_feature_overlays,
            terrain_decor::sync_terrain_decor,
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
    // 编辑意图翻译（ADR-0020 §2.5）：渲染 commit_* 产意图 → 翻译器提交 wv-edit
    // → 渲染 apply_*_receipts 回填本地。必须晚于两个 commit 系统（同帧闭环）。
    .add_systems(
        Update,
        (
            crate::edit::intent_translator::translate_edit_intents,
            wv_render::streets::apply_street_receipts,
            wv_render::measurement::apply_measurement_receipts,
        )
            .chain()
            .after(wv_render::streets::commit_street_click)
            .after(wv_render::measurement::commit_measurement_click),
    )
    // 纸面氛围层（ADR-0016 M2：雾霭 + 纸张噪点）：独立注册，排在主链
    // `sync_map_frame` 之后（同帧即可，只要不先于地形 chunk 出现）。
    .add_systems(
        Update,
        terrain_view::atmosphere::sync_atmosphere.after(map_frame::sync_map_frame),
    )
    // 2.5D 建筑挤出（ADR-0016 §6）：独立注册，消费主链产出的 LayerFrame。
    .add_systems(
        Update,
        buildings::sync_building_overlays.after(vector_layers::sync_vector_layers),
    )
    .add_systems(
        Update,
        (
            // 测量：先 reload 缓存，再 commit 点击，最后 render。
            // reload 在 `world_projection::is_changed` 后即可（缓存早退）。
            wv_render::measurement::reload_measurement_cache.after(camera::track_cursor_world),
            wv_render::measurement::commit_measurement_click
                .after(wv_render::measurement::reload_measurement_cache),
        ),
    )
    // 选择工具（pen 方法）：变图清空 → 点击采集 → 拖动移锚，同一条光标坐标链。
    .add_systems(
        Update,
        (
            wv_render::selection::sync_selection_map_change,
            wv_render::selection::commit_selection_click
                .after(wv_render::selection::sync_selection_map_change),
            wv_render::selection::drag_selected_anchor
                .after(wv_render::selection::commit_selection_click),
        )
            .after(camera::track_cursor_world),
    )
    .add_systems(
        Update,
        (
            // 地图绘制必须在所有浮动面板**之前**执行，
            // 保证地图是最底层、面板浮在其上（基线 §1.11 R1）。
            world_projection::draw_map_gizmos,
            world_projection::draw_brush_cursor,
            // 测量顶置渲染——与 gizmos 一起画。
            wv_render::measurement::render_measurements,
            // 选择工具（草稿折线 + 橡皮筋 / 闭合环 + 选中锚点）。
            wv_render::selection::draw_selection,
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
            ui::panel_timeline,
            ui::panel_event,
            ui::panel_situation,
            ui::panel_character,
            ui::panel_faction,
            ui::panel_region,
            // v0.3：Inspector 整体右移后绘在最后（右上角，离顶栏最近），
            // 避免被左列 / 中列面板遮挡。
            ui::panel_inspector,
            ui::panel_cell_editor,
            ui::panel_validation,
            // 「新建地图」对话框（CommandId::NewMap 打开；居中模态窗）。
            ui::new_map_dialog,
            // 「生成大陆」属性面板（工具条 / 命令面板打开；居中模态窗）。
            ui::continent_dialog_system,
            // 「新建工程」对话框（CommandId::NewProject / 欢迎页打开；
            // 与 new_map_dialog 同族的居中模态窗，排在状态栏之前——
            // 确认创建写的回执本帧就要被状态栏读到）。
            ui::new_project_dialog,
            // 「新建工程」apply 系统：消费 UI 系统上一帧塞进
            // `NewProjectDialog::pending_apply` 的意图，做实际落盘 + 打开。
            // 拆出来的目的是让 UI 系统跟 apply 系统都在 Bevy 0.19 的 16 系统
            // 参数上限内（详见 `new_project_dialog` 文档）。
            ui::new_project_apply_create,
            // 状态栏最后画：它读得到的 `ShellNotice` 已是本帧最终值。
            ui::status_bar,
        )
            .chain(),
    );
    // 「新建地理事实」命名对话框（选择工具 M4「森林」意图打开；居中模态窗）。
    // 主链已顶满 20 元组上限，单独注册并排在状态栏之后。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        (
            ui::feature_place_dialog.after(ui::status_bar),
            ui::faction_divide_dialog.after(ui::feature_place_dialog),
            // 应用设置（顶栏 settings-2）：与状态栏同帧、压在所有面板之上
            // （`DialogWindow` 走 `Order::Foreground`）。
            ui::settings_dialog.after(ui::status_bar),
        ),
    );
    // 选区意图菜单（底部居中浮动条；spec 2026-10-17 §4）。ego pass 主链已到
    // Bevy 元组上限（20），单独注册、排在 map_controls 之后即可（只读选区）。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        ui::intent_menu.after(ui::map_controls),
    );
    // 「生成城市」对话框（意图菜单打开；居中模态窗）。与菜单同链尾段，
    // 排在其后（先画菜单、后画它弹出的对话框，同帧即可）。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        ui::city_intent::city_intent_dialog_system.after(ui::intent_menu),
    );

    // 界面外壳（主题 / 快捷键 / 命令面板 / 布局持久化）与地图钻取（v0.2 §B）。
    // 两者各自成模块并各自注册，`gui.rs` 只负责调用顺序（见各处模块文档）。
    crate::ui_shell::register(&mut app);
    wv_render::drill::register(&mut app);
    // 隐藏起步的揭示（见 Window 配置注释）：帧 1 的 Update 翻转 `visible`，
    // 本帧 `Last` 里 bevy_winit 的 `changed_windows` 调 `set_visible(true)`——
    // 此时帧 0 已完整 present，窗口亮起即是有内容的画面。
    app.add_systems(Update, reveal_window_once_first_frame_presented);
    // 渲染配方求值（ADR-0021 D1）：PreUpdate 单点求值 RenderRecipe——已迁移的
    // gate（buildings / faction_overlays / atmosphere）读 `recipe.enabled(...)`，
    // **必须**挂本系统，否则 recipe 停留在 Default 全关、三个 pass 永不渲染。
    wv_render::recipe::register_recipe_system(&mut app);
    // ADR-0020 §2.6：钻入标记界面色由应用层灌（渲染只读 DrillMarkerPalette）；
    // 悬停气泡（egui 浮窗）从 wv-render 链尾移回应用层，仍排海名标签之后。
    app.add_systems(
        Update,
        crate::render_bridge::sync_drill_marker_palette
            .before(wv_render::drill::draw_drill_markers),
    );
    app.add_systems(
        Update,
        ui::hover_tooltip::hover_tooltip_system.after(wv_render::drill::update_sea_labels),
    );

    app.run();
}

/// 隐藏起步的揭示：第 2 个 Update tick 把主窗口放出来（一次性）。
///
/// ## 时序（与 [`crate::ui_shell::startup_load`] 的「帧 0/1 只画遮罩」咬合）
///
/// | 帧 | 发生什么 |
/// |---|---|
/// | 0 | 窗口隐藏创建（`Last` 里建 winit 窗口）；PreUpdate 装配 egui 字体；pass 画欢迎页/遮罩；render 编译管线并 present（不可见） |
/// | 1 | Update 本系统翻转 `visible` → `Last` 应用 `set_visible(true)`，窗口亮起时屏上是帧 0 的完整画面 |
/// | 2 起 | `startup_load` 才开始阻塞加载（阈值 `< 3`），`-m` 的加载遮罩已在屏上 |
///
/// ## 为什么是帧 1 而不是更早 / 更晚
/// - 帧 0 翻转会在窗口刚建好、尚无任何 present 时就揭白底——白屏回来了；
/// - 更晚（帧 2+）只会让用户多等，帧 0/1 的画面内容已无差别
///   （字体 atlas 与 egui 管线都在帧 0 建齐）。
///
/// ## 为什么不用 `PipelineCache` 就绪信号
/// 渲染管线的编译状态在渲染子世界，主世界拿不到（ADR-0020 后 render 已是
/// 独立 crate 边界）；「多跑一帧」以 8~16ms 的代价换同等保证，简单一个量级。
fn reveal_window_once_first_frame_presented(
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
    let image = terrain_view::image_from_terrain(
        &data,
        view.mode,
        atlas.as_ref(),
        view.terrain_soften,
        crate::theme::palette::colors(view.map_palette),
        wv_render::theme::theme_pack::builtin::for_palette(view.map_palette).treatment,
        0.0,
    );
    commands.insert_resource(terrain_view::new_terrain_texture(
        &mut images,
        image,
        &data,
        map,
        view.mode,
        view.terrain_soften,
        view.map_palette,
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
            Vec2::new(root_extent.0 * 0.5, root_extent.1 * 0.5),
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
