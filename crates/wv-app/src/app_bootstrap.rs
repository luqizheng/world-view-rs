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
//! - **不加载工程**——`startup_project` 为 `Some(_)` 时，由 `ui_shell::drive_project_load`
//!   在窗口内做异步加载并显示进度 / 失败原因。
//!
//! ## 主要导出
//! - `run(config: AppConfig)` — 启动 GUI 的唯一入口。

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowResolution};
use bevy_egui::EguiPlugin;

use crate::edit::editing;
use crate::edit::nature_region_actions;
use crate::state::{
    BrushState, DrillState, GridConfig, PanelState, PlaybackState, ViewState, WorldProjection,
};
use crate::theme::ui_tokens::ShellPalette;
use crate::ui;
use crate::AppConfig;
use wv_render::brush_cursor;
use wv_render::buildings;
use wv_render::camera;
use wv_render::cellmap_bridge;
use wv_render::faction_overlays;
use wv_render::feature_overlays;
use wv_render::layer_frame;
use wv_render::map_frame;
use wv_render::place_line;
use wv_render::region_outline;
use wv_render::region_overlay;
use wv_render::streets;
use wv_render::terrain_chunks;
use wv_render::terrain_decor;
use wv_render::terrain_view;
use wv_render::vector_layers::{self, VectorLayerEntities};
use wv_render::world_projection;

/// 启动 GUI。`startup_project` 为 `None` 时进空编辑器（占位地形），
/// `Some(dir)` 时启动后由 `ui_shell::drive_project_load` 在后台线程加载，
/// 窗口内显示进度 / 失败原因。
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
    .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
    // 清屏色对齐外壳暗色 bg_app：隐藏揭示若与 present 有毫秒级竞争，
    // 用户瞥见的也是深底而非默认灰/白（防御性，正常时序下被欢迎页盖住）。
    .insert_resource({
        let bg = ShellPalette::DARK.bg_app;
        ClearColor(Color::srgb(bg.r, bg.g, bg.b))
    })
    // 播放范围：空编辑器起步先用默认值（1..180_001）；
    // 运行期加载工程成功后由 `apply_loaded_world` 按该工程 `timeline/axis.yaml` 重置。
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
    // 应用屏状态（2026-10-22 B 档）：初值按「有没有 CLI 加载请求」给——
    // 有 `-m` 时第一帧就处于「加载中」，欢迎页不会抢在加载遮罩前面闪一下。
    .insert_resource(crate::app_screen::AppScreen {
        loading: config.startup_project.is_some(),
        ..Default::default()
    })
    .insert_resource(ViewState {
        camera_center: center,
        scale: fit_scale,
        ..Default::default()
    })
    .init_resource::<BrushState>()
    .init_resource::<PanelState>()
    // 自然区域面板的会话态（物化时的名字输入，ADR-0045 D4）。
    .init_resource::<crate::state::NatureRegionUiState>()
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
    // 群系分界线工具会话态（2026-10-22 内容矩阵 / ADR-0051）：输入系统与
    // 预览系统都 ResMut 它——漏 init 即启动期 panic（同 EditCellState 教训）。
    .init_resource::<wv_render::state::BiomeBoundaryState>()
    // 区域边界拖拽调整会话态（2026-10-22 验收）：同款漏 init 即 panic。
    .init_resource::<wv_render::state::RegionAdjustState>()
    // 候选上下文镜像（渲染层用；漏 init 即 panic）。
    .init_resource::<wv_render::state::RegionCandidateContext>()
    // Cell 编辑器的「单格 / 批量」工作模式（[ADR-0040] 补 B）——面板系统
    // ResMut 它,漏 init 即启动期 panic(同 EditCellState 的教训)。
    .init_resource::<crate::ui::inspector::cell::CellPanelMode>()
    // 「圈完自动落批量」的边沿检测（2026-10-24）——同属面板系统 ResMut
    // 的会话态,漏 init 即启动期 panic(同上)。
    .init_resource::<crate::ui::inspector::cell::BatchModeAutoLand>()
    // Inspector 的当前页签（2026-10-22 统一 Inspector）：面板系统 ResMut 它,
    // 漏 init 即启动期 panic(同 EditCellState 的教训)。
    .init_resource::<crate::ui::inspector::InspectorState>()
    // CellMap 未命名候选缓存（2026-10-22 内容矩阵 T11）：渲染主链在本 crate
    // 直装（不走 MapRenderPlugin），draw_map_gizmos ResMut 它——漏 init 即
    // 启动期 panic（实机已踩：plugins.rs 里的 init 在 GUI 路径不运行）。
    .init_resource::<wv_render::region_overlay::CandidateOverlayCache>()
    // Inspector 表单的暂存输入缓冲（egui TextEdit 调用方持有文本，
    // 漏 init 即启动期 panic——同 EditCellState 的教训）。
    .init_resource::<crate::ui::inspector::event::InspectorDrafts>()
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
    // 主题包注册表（ADR-0023 M2）：内置包 + 外部主题 crate（`wv-theme-chinese-ink`）。
    // **显式声明接入哪些包**——游戏 app 写自己的 merge 列表即可看到完整装配
    // （ADR-0023 D2 §3：不用隐式默认）。
    .insert_resource(wv_render::theme::switch::ThemeRegistry(
        wv_theme::registry::MergedThemeRegistry::merge(&[
            &wv_theme::registry::BuiltinRegistry,
            &wv_theme_chinese_ink::ChineseInkRegistry,
        ]),
    ))
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
    // 离岸距离场缓存（ADR-0048 追记四）：OceanWaves 的离岸带判定用，
    // 按 terrain hash 键控（地形没变就复用）。
    .init_resource::<terrain_decor::ShoreFieldCache>()
    // 森林密度场缓存（方案 1）：森林符号的疏密由地形事实（群系 + 林线 +
    // 聚集噪声）给出，主题再乘 `forest_density_gain` 调制；按 terrain hash
    // 键控（地形没变就复用）。
    .init_resource::<terrain_decor::ForestDensityCache>()
    // 区域覆盖物解析索引 + 缺失标记运行时资源（ADR-0045）。
    .init_resource::<region_overlay::RegionOverlayIndex>()
    .init_resource::<region_overlay::MissingMarkerAssets>()
    // 区域轮廓 / 底色覆盖的运行时资源（ADR-0045 追记四）。
    .init_resource::<region_outline::RegionOutlinePool>()
    .init_resource::<region_outline::CandidateRegionInput>()
    // 选中区域装饰（阶梯描边 + 顶点手柄）的派生缓存（2026-10-23）：
    // `region_decor::sync_region_decor` ResMut 它——漏 init 即启动期 panic。
    .init_resource::<wv_render::world_projection::RegionDecorCache>()
    // 装饰的合批 mesh 池（2026-10-24）：阶梯描边与实心白方块锚点各一块 mesh。
    // 选中大区域时旧实现逐条 `gizmos.line_2d`（上万条/帧）⇒ FPS 掉到 20 多。
    .init_resource::<wv_render::region_decor::RegionDecorPool>()
    // per-system 帧耗时探针（`WV_PERF=1` 开启；默认只做两次布尔判断）。
    .init_resource::<wv_render::perf::PerfRecorder>()
    // 作者小河的会话态与预览资源（ADR-0046）。
    .init_resource::<place_line::PlaceLineEditorState>()
    .init_resource::<place_line::PlaceLinePreviewPool>()
    .init_resource::<brush_cursor::BrushCursorEntities>()
    .init_resource::<map_frame::MapFrameEntities>()
    .init_resource::<terrain_chunks::TerrainChunkEntities>()
    // 地形场渲染（ADR-0053 D1，Phase 1 接线 2026-10-24）：quad 资产缓存 + 计数器。
    .init_resource::<wv_render::cellmap_bridge::TerrainFieldRender>()
    .init_resource::<wv_render::cellmap_bridge::FieldRenderStats>()
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
    // 主题包同步（ADR-0023 M2）：`ViewState.theme_pack_id` → `ViewState.active_theme`。
    // **必须先于所有消费点**——挂在 PreUpdate 最前；渲染消费点在 Update（PreUpdate
    // 一定先于 Update），唯一同在 PreUpdate 的消费者（`poll_open_project_dialog`）
    // 由 ui_shell 侧显式 `.before()` 保证。
    app.add_systems(PreUpdate, wv_render::theme::switch::sync_active_theme_pack);
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
                //
                // 每个系统都套 `perf_timed!`（`WV_PERF=1` 才计时，默认两次布尔
                // 判断即返回）：2026-10-23「鼠标移动 120→40 FPS」要把 17ms 落到
                // 具体 system 名上。整段摘除 = 把宏换回裸 system。
                wv_render::perf_timed!("apply_map_orientation", camera::apply_map_orientation),
                wv_render::perf_timed!("camera_zoom", camera::camera_zoom),
                wv_render::perf_timed!("camera_pan", camera::camera_pan),
                wv_render::perf_timed!("camera_pan_keys", camera::camera_pan_keys),
                wv_render::perf_timed!("brush_radius_scroll", camera::brush_radius_scroll),
                wv_render::perf_timed!("track_cursor_world", camera::track_cursor_world),
                wv_render::perf_timed!("collect_brush_stroke", camera::collect_brush_stroke),
                // 编辑（笔触 → 命令 → History）
                wv_render::perf_timed!(
                    "commit_stroke_on_release",
                    editing::commit_stroke_on_release
                ),
                // 单格编辑（[ADR-0040]）：点击 → cell_pick → EditCellState.selected
                wv_render::perf_timed!(
                    "commit_edit_cell_on_click",
                    editing::commit_edit_cell_on_click
                ),
                // 候选点选（2026-10-22 内容矩阵）：在 cell 点选之后跑——
                // 读 selected 判定是否落进未命名候选。
                wv_render::perf_timed!(
                    "pick_candidate_on_click",
                    crate::edit::nature_region_actions::pick_candidate_on_click
                ),
                // 群系分界线：创建/拖动/提交（在 cell 点选之后，互斥工具态）。
                wv_render::perf_timed!(
                    "biome_boundary_input",
                    crate::edit::biome_boundary::biome_boundary_input
                ),
                // 候选过期的编辑信号（在提交路径之后跑，读 History 形状）。
                wv_render::perf_timed!(
                    "sync_candidate_invalidation",
                    crate::edit::nature_region_actions::sync_candidate_invalidation
                ),
                // 区域边界拖拽（在 cell 点选之后；同为输入系统）。
                wv_render::perf_timed!(
                    "region_adjust_input",
                    crate::edit::region_adjust::region_adjust_input
                ),
                // 自然区选择手势（2026-10-24 用户裁定）拆到主链**之后**单独注册：
                // 左键点在自然区内 ⇒ 选中；右键**单击**（位移 <4px）⇒ 取消选中。
                // 这两条必须排在 ①`drill_pick`（空白单击会清选中）与
                // ②`region_adjust_input`（拽锚点优先）之后，否则会被覆盖或抢走。
                // 见下方「自然区选择」。
                // 批量选区（2026-10-20）：多边形闭合 → 栅格化贴合方格
                wv_render::perf_timed!("sync_region_cells", editing::sync_region_cells)
                    .after(camera::track_cursor_world),
                wv_render::perf_timed!("commit_street_click", streets::commit_street_click),
                // 撤销 / 重做不再在这里单独监听 Ctrl+Z：快捷键统一由
                // `ui_shell::hotkeys` 收（那里已经处理了「输入框聚焦时失效」），
                // 两处都听会导致一次按键应用两次。
                wv_render::perf_timed!("flush_terrain_texture", editing::flush_terrain_texture),
                // 播放推进（时间 → 当前时刻）
                wv_render::perf_timed!("advance_playback", world_projection::advance_playback),
                // 纹理先于网格几何：`grid.cell_size` 取自纹理的来源图与格数。
                wv_render::perf_timed!(
                    "rebuild_texture_on_mode_change",
                    terrain_view::rebuild_texture_on_mode_change
                ),
                // 后台合成落地（M3）：必须在重建触发之后、网格几何之前——
                // 它写 tex.size，`sync_grid_config` 同帧要读到。
                wv_render::perf_timed!(
                    "land_pending_terrain_synth",
                    terrain_view::land_pending_terrain_synth
                ),
                // 网格几何（`raster_origin` / `map_origin` 两套原点）必须先于所有
                // 消费它的系统：投影里的地点、街道、街道名、地图外框都按 `GridConfig`
                // 换算，顺序反了就会拿上一帧的原点画一帧——钻取那一帧最明显。
                wv_render::perf_timed!("sync_grid_config", terrain_view::sync_grid_config),
            )
                .chain(),
            // 换级淡化：旧底图淡出精灵逐帧降透明、归零回收（ADR-0009 §2⑦）。
            wv_render::perf_timed!("tick_basemap_fadeout", terrain_view::tick_basemap_fadeout),
            // 图层帧（ADR-0013）：先于一切消费它的渲染系统求值。
            wv_render::perf_timed!("resolve_layer_frame", layer_frame::resolve_layer_frame),
            // 投影（时间 → 渲染资源）
            wv_render::perf_timed!("compute_projection", world_projection::compute_projection),
            // 等值线矢量叠加（ADR-0012 / plan v0.1 M2）：先于 chunk 层读
            // `tex.chunks_dirty`（chunk 系统会消费清零），触发重算。
            wv_render::perf_timed!("sync_isoline_overlay", terrain_view::sync_isoline_overlay),
            // 编辑态逐格网格（plan v0.1 D4）：与等值线同管线族，先于 chunk 层。
            wv_render::perf_timed!("sync_grid_lines", terrain_view::sync_grid_lines),
            // 地形分块 + mip + 视口拣选（优化项 3a/3b/3c）。
            // `view.field_render` 开启时整层早退——见函数开头的回退开关。
            wv_render::perf_timed!("sync_terrain_chunks", cellmap_bridge::sync_terrain_chunks),
            // 地形场渲染（ADR-0053 D1，Phase 1 接线）：**必须排在 chunk 层之后**。
            // 两个理由：① `field_render_on_chunk_entities` 要报**帧末**的块数，
            // 而块的 spawn / despawn 由 chunk 系统在本帧完成（排在前面只能读到
            // 上一帧的残留，与组件口径对不上）；② 编辑信号取自变更戳
            // （`WorldProjection` / `TerrainTexture` 的 `is_changed`），不抢
            // `tex.chunks_dirty`——那个旗标照旧归 chunk 层消费。
            // 独占性不依赖顺序：场开着时 chunk 层自己早退。
            wv_render::perf_timed!("sync_terrain_field", cellmap_bridge::sync_terrain_field),
            // 派生诊断刷新：在 flush 之后、UI 之前；面板本帧即可读新值。
            wv_render::perf_timed!(
                "refresh_project_diagnostics",
                editing::refresh_project_diagnostics
            ),
            // 候选区预览输入（ADR-0045 追记四）：光标格 → biome 连通区环。
            wv_render::perf_timed!(
                "sync_candidate_input",
                nature_region_actions::sync_candidate_input
            ),
            // 街道图层缓存：当前图变化时重读（ADR-0010），先于 sync_vector_layers。
            wv_render::perf_timed!("reload_street_cache", streets::reload_street_cache),
            // 矢量叠加层（道路 + POI）：在 compute_projection 之后跑。
            wv_render::perf_timed!("sync_vector_layers", vector_layers::sync_vector_layers),
            // 街道名标注：消费同一份 StreetLayerCache，在街道画完之后对齐标签。
            wv_render::perf_timed!("update_street_labels", streets::update_street_labels),
            // 势力多边形叠加层（S12 v0.1）：在矢量层之后、地图外框之前。
            wv_render::perf_timed!(
                "sync_faction_overlays",
                faction_overlays::sync_faction_overlays
            ),
            // 事实地界覆盖层（theme overlay）：地盘随 (图, 日, 缩放) 求值，
            // 排在势力层之后、地图外框之前（z 由各层常量决定，与顺序无关）。
            wv_render::perf_timed!(
                "sync_feature_overlays",
                feature_overlays::sync_feature_overlays
            ),
            wv_render::perf_timed!("sync_terrain_decor", terrain_decor::sync_terrain_decor),
            // 地图外框 + 1/4 幅面参考网格（外框需地形尺寸）。
            wv_render::perf_timed!("sync_map_frame", map_frame::sync_map_frame),
        )
            .chain(),
    )
    // per-system 计时汇总（`WV_PERF=1` 才有输出）：`Last` 里跑，本帧所有
    // Update 系统都已收工——单帧超 4ms 立刻打印，每 5s 打 top5 + 最差帧。
    .add_systems(Last, wv_render::perf::report_frame)
    // 主链已顶满 Bevy 系统元组上限：新三件单独成链，显式排序——
    // 候选输入在主链（diagnostics 之后天然有序），轮廓/预览消费它。
    .add_systems(
        Update,
        (
            // 区域轮廓 + 底色覆盖 + 候选区预览（ADR-0045 追记四）：区域的
            // 地理边界此前只有标签没有轮廓——作者看不出圈了哪块地。
            wv_render::perf_timed!("sync_region_outlines", region_outline::sync_region_outlines)
                .after(map_frame::sync_map_frame),
            // 作者小河的落笔 + 预览（ADR-0046）：左键加点 / 右键收线。
            wv_render::perf_timed!(
                "commit_place_line_click",
                place_line::commit_place_line_click
            ),
            wv_render::perf_timed!(
                "sync_place_line_preview",
                place_line::sync_place_line_preview
            )
            .after(place_line::commit_place_line_click),
        )
            .chain(),
    )
    // 区域覆盖物解析（ADR-0045 D8/D9/D10）：**必须早于装饰采样**（装饰要按
    // region 的声明换覆盖物变体 / 颜色），缺失标记紧随其后。
    .add_systems(
        Update,
        cellmap_bridge::sync_region_overlay_index
            .after(terrain_view::sync_grid_config)
            .before(terrain_decor::sync_terrain_decor),
    )
    .add_systems(
        Update,
        cellmap_bridge::sync_missing_overlay_markers
            .after(cellmap_bridge::sync_region_overlay_index),
    )
    // 主链已顶满 Bevy 系统元组的 20 参上限（all_tuples，bevy_ecs
    // function_system.rs），笔刷光标单独注册、显式排在光标取世界坐标之后。
    .add_systems(
        Update,
        wv_render::perf_timed!("sync_brush_cursor", brush_cursor::sync_brush_cursor)
            .after(camera::track_cursor_world),
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
        cellmap_bridge::sync_atmosphere.after(map_frame::sync_map_frame),
    )
    // 自然区选择手势（2026-10-24 用户裁定）：左键点在自然区内 ⇒ 选中该区域；
    // 右键**单击**（松手位移 <4px）⇒ 取消选中。
    //
    // 单独注册而不是塞进主链：这两条要挂在 `drill_pick`（空白单击清选中）与
    // `region_adjust_input`（拽锚点优先）**之后**——主链是一整条 `.chain()`，
    // 混进 `.after()` 会让整条链的调度约束求不出来（实测编译失败）。
    .add_systems(
        Update,
        (
            wv_render::perf_timed!(
                "select_nature_region_on_click",
                crate::edit::region_select::select_nature_region_on_click
            )
            .after(wv_render::drill::drill_pick)
            .after(crate::edit::region_adjust::region_adjust_input),
            // 右键取消选中：只读按键 + 光标，不依赖任何输入系统的先后。
            wv_render::perf_timed!(
                "cancel_selection_on_right_click",
                crate::edit::region_select::cancel_selection_on_right_click
            ),
        ),
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
            // 框选形状（ADR-0043）：按住拖拽的那一套手势。与上面三个系统读同
            // 一批资源、各自在函数开头按形状早退（互斥，不会同时改状态）。
            wv_render::selection::commit_selection_shape_drag
                .after(wv_render::selection::sync_selection_map_change),
        )
            .after(camera::track_cursor_world),
    )
    .add_systems(
        Update,
        (
            // 选中区域装饰（阶梯描边 + 实心白方块锚点）：**合批 mesh**，
            // 2026-10-24 从 `draw_map_gizmos` 迁出——逐条 gizmo 在大区域上是
            // 上万条/帧（用户症状：点选自然区后 FPS 掉到 20 多）。
            // 排在地图绘制之前：它只写 mesh 资产与实体，不依赖本帧 gizmos。
            wv_render::perf_timed!(
                "sync_region_decor",
                wv_render::region_decor::sync_region_decor
            ),
            // 地图绘制必须在所有浮动面板**之前**执行，
            // 保证地图是最底层、面板浮在其上（基线 §1.11 R1）。
            wv_render::perf_timed!("draw_map_gizmos", world_projection::draw_map_gizmos),
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
            ui::panel_character,
            // 2026-10-22 统一 Inspector：panel_situation / panel_faction /
            // panel_cell_editor 均已退役——内容分别成为 Inspector 的
            // 「事态」「势力层」「属性（Cell）」页签。
            ui::panel_region,
            // v0.3：Inspector 整体右移后绘在最后（右上角，离顶栏最近），
            // 避免被左列 / 中列面板遮挡。
            ui::panel_inspector,
            // 2026-10-22 统一 Inspector：Cell 编辑器的**内容**已迁入 Inspector
            // 的「属性」页签，独立浮窗不再注册（枚举项暂留，退役见 M4）。
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
    // 候选上下文镜像（2026-10-22 验收）：主链已顶满 20 元组，单独注册。
    app.add_systems(
        Update,
        (
            crate::edit::nature_region_actions::sync_candidate_context,
            // 群系图例（CellMap 左下角）：读 biome_raw 色板与名，自动跟随。
            ui::biome_legend,
        ),
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
    // 窗口标题跟随界面语言（见 [`sync_window_title_with_locale`]）。
    app.add_systems(Update, sync_window_title_with_locale);
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
fn sync_window_title_with_locale(
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
        cellmap_bridge::shade_mode_of(view.mode),
        atlas.as_ref(),
        view.terrain_soften,
        &cellmap_bridge::conv_pack_with(view.active_theme, view.hypsometric_tint),
        0.0,
    );
    commands.insert_resource(terrain_view::new_terrain_texture(
        &mut images,
        image,
        &data,
        map,
        cellmap_bridge::shade_mode_of(view.mode),
        view.terrain_soften,
        view.active_theme.id,
        view.hypsometric_tint,
    ));
}

/// 按世界内容计算初始取景（中心 + 缩放）。
///
/// - 优先**框住根图内容**（icons 包围盒 + 25% 边距）：大图幅上内容只占
///   一小块时，按整图 fit 会把内容挤成屏幕中心的一撮点、四周全是空黑；
/// - 无内容/空世界时退回根图 extent（缺省 4096²）的中心。
///
/// Startup（`run` 装配）与运行期加载（`ui_shell::apply_loaded_world`）共用，
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
