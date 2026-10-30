//! `App` 的启动期资源装配：工作副本、视图 / 工具会话态、渲染实体池等。

use bevy::prelude::*;

use crate::edit::editing;
use crate::state::{BrushState, GridConfig, PanelState, PlaybackState, ViewState, WorldProjection};
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
use wv_render::place_line;
use wv_render::region_outline;
use wv_render::region_overlay;
use wv_render::streets;
use wv_render::stroke_preview;
use wv_render::terrain_chunks;
use wv_render::terrain_decor;
use wv_render::terrain_view;
use wv_render::vector_layers::VectorLayerEntities;
use wv_render::world_projection;

/// 插入启动期全部资源。
///
/// `center` / `fit_scale` 是空世界取景（见 [`super::view_fit::fit_view_for_world`]）；
/// `config.startup_project` 决定首帧是否已处于「加载中」。
pub(super) fn insert_all(app: &mut App, config: &AppConfig, center: Vec2, fit_scale: f32) {
    app.insert_resource({
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
    // 图层面板的会话态（选中层 + 不透明度拖动中值，Figma `3:20892` 接线）。
    .init_resource::<ui::layers_panel::LayerPanelUiState>()
    .init_resource::<ui::map_tree::MapClipboard>()
    .init_resource::<ui::map_tree::MapTreeUiState>()
    .init_resource::<GridConfig>()
    // 「新建地图」对话框状态（ui_shell 分发器与对话框系统共用）。
    .init_resource::<ui::NewMapDialog>()
    // 「地图属性」对话框状态（ui_shell 分发器与对话框系统共用）。
    // 2026-10-27 曾漏注册：全测试闸门绿（fixture 手动 init 掩盖）、首次
    // `wv gui` 即 panic——smoke 闸门存在的理由，见 §7.1。
    .init_resource::<ui::MapPropertiesDialog>()
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
    // 泛洪判据会话态（[ADR-0052] D6；ADR-0086 起仅为判据）：点选工具的
    // 默认点击（= 泛洪，ADR-0074 追记四）按此判据圈同值连通区。
    .init_resource::<crate::state::FloodSelectState>()
    // 地标放置会话态（ADR-0071）：placing + 默认类别/级别。
    .init_resource::<crate::state::LandmarkToolState>()
    // 派生运算请求（[ADR-0052] D6 · M3）：平滑 / 量化的参数与意图。
    // 面板写、`edit::derived_op::apply_derived_op` 消费。
    .init_resource::<crate::edit::derived_op::DerivedOpRequest>()
    // 单格连续绘制的丢弃确认弹窗态（[ADR-0052] D3-C）。
    // 笔触格集合 + 落笔值在 `wv_world_cell_map::cell_paint::CellPaintSession`。
    //
    // **为什么在此显式 init（2026-10-24 修复）**：`wv-render` 的
    // `EditorOverlayPlugin` 注册了这份资源与 cellmap 渲染系统，但**本 crate
    // 从未装配该插件**（渲染主链按 ADR-0020 直装，见本文件顶部注释）。
    // 后果：`paint_edit_cell_on_drag` 的 `ResMut<CellPaintSession>` 在
    // 首帧 panic；M1 的 hover 准星 / M2 的待落笔触色块也永远不画。
    // 与 `EditCellState` / `BiomeBoundaryState` 同款教训（见各自注释）。
    .init_resource::<wv_world_cell_map::cell_paint::CellPaintSession>()
    // 连续绘制的丢弃确认弹窗态（D3-C）：`paint_edit_cell_on_drag` 以
    // `ResMut<StrokeDiscardPrompt>` 读写。与上一行同一次装配遗漏（2026-10-24
    // GUI 烟测抓到：首帧 panic「Resource does not exist」）。
    .init_resource::<crate::edit::cell_stroke::StrokeDiscardPrompt>()
    // 撤销白闪 / 拒收红闪会话态（[ADR-0052] D8-A / D3-E）：应用层触发，
    // 渲染层 `sync_cell_flash` 读它画闪。
    .init_resource::<wv_world_cell_map::cell_flash::CellFlashState>()
    // 丢弃确认弹窗态（[ADR-0052] D3-C）：`paint_edit_cell_on_drag` 的
    // `ResMut<StrokeDiscardPrompt>`——漏 init 即启动期 panic（真机实测抓到，
    // 单测用 run_system_once 手动 insert 掩盖了装配缺失）。
    .init_resource::<crate::edit::cell_stroke::StrokeDiscardPrompt>()
    // cellmap 渲染实体池（区域填充 / 蚂蚁线 / hover 准星 / 待落笔触 / 闪视觉）：
    // 渲染系统 `ResMut` 它们——漏 init 即启动期 panic。
    .init_resource::<wv_render::cell_selection::CellRegionEntities>()
    .init_resource::<wv_render::cell_selection::CellAntsEntities>()
    .init_resource::<wv_render::cell_selection::CellHoverEntities>()
    .init_resource::<wv_render::cell_selection::CellStrokeEntities>()
    .init_resource::<wv_world_cell_map::cell_flash::CellFlashEntities>()
    // 编辑意图桥（ADR-0020 §2.5）：渲染层产意图、本 crate 翻译提交、回执回填。
    .init_resource::<wv_render::editing_bridge::EditIntents>()
    .init_resource::<wv_render::editing_bridge::EditReceipts>()
    .init_resource::<wv_render::streets::StreetCommitAcks>()
    .init_resource::<wv_render::measurement::MeasureCommitAcks>()
    // 校验面板 / 笔触提交 / 撤销重做 / 打开工程都读写它（`ResMut` 访问
    // 不存在的资源会在首帧 panic → 窗口无法显示）。`derive(Default)` 起步。
    .init_resource::<crate::state::ProjectDiagnostics>()
    // 尺度提示缓存（重构 P0-3）：面板常开时避免每帧全扫地形栅格求陆地包围盒。
    .init_resource::<crate::state::ScaleHintCache>()
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
    // CellMap 统一选区位图（ADR-0057）：`sync_cell_region` 每帧 `ResMut` 绑定
    // 当前图；wv-app 各落笔点 `ResMut` 改写。漏 init 即启动期 panic（同上）。
    .init_resource::<wv_render::state::CellSelection>()
    // 点选工具会话态（ADR-0057 补记 6）：与 EditCell 平级的独立 Resource。
    .init_resource::<wv_render::state::PointSelectState>()
    // 图钉拖动会话态（ADR-0074 追记三）：按住拖动改坐标的候选/预览。
    .init_resource::<wv_render::state::PinDragState>()
    // 线段 POI 工具会话态（ADR-0070 D4）：工具条分裂按钮、分发器互斥分支
    // 与（T11 的）地图交互系统都读写它——漏 init 即启动期 panic
    // （同 EditCellState 的教训：测试 fixture 手动 init 会掩盖装配遗漏）。
    .init_resource::<crate::state::LinePoiState>()
    // 套索活链接（ADR-0057 补记 5）：sync_region_cells ResMut 它。
    .init_resource::<crate::edit::editing::LassoLiveLink>()
    // 群系分界线工具会话态（2026-10-22 内容矩阵 / ADR-0051）：输入系统与
    // 预览系统都 ResMut 它——漏 init 即启动期 panic（同 EditCellState 教训）。
    .init_resource::<wv_render::state::BiomeBoundaryState>()
    // 区域边界拖拽调整会话态（2026-10-22 验收）：同款漏 init 即 panic。
    .init_resource::<wv_render::state::RegionAdjustState>()
    // 候选上下文镜像（渲染层用；漏 init 即 panic）。
    .init_resource::<wv_render::state::RegionCandidateContext>()
    // Inspector 的当前页签（2026-10-22 统一 Inspector）：面板系统 ResMut 它,
    // 漏 init 即启动期 panic(同 EditCellState 的教训)。
    .init_resource::<crate::ui::inspector::InspectorState>()
    .init_resource::<crate::ui::partition_pen::PartitionPenDialog>()
    .init_resource::<crate::ui::partition_pen::PartitionEditState>()
    .init_resource::<crate::ui::partition_edit::CutStrokeSession>()
    .init_resource::<crate::ui::partition_edit::PartitionAssignDialog>()
    .init_resource::<crate::ui::partition_edit::DragSession>()
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
    .init_resource::<ui::FeaturePlaceDialog>()
    .init_resource::<wv_render::measurement::MeasurementCache>()
    // 整图后台合成任务槽（CONTINUOUS_LOD M3）：落地校验在主线程读取。
    .init_resource::<wv_render::terrain_view::rebuild::PendingTerrainSynth>()
    // 风格包（ADR-0023 M1 / ADR-0066）：单风格时代恒为唯一默认包。
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
    // UX-004：世界级笔刷拖拽预览实体池（点列 → 半透明足印）。
    .init_resource::<stroke_preview::StrokePreviewState>()
    .init_resource::<map_frame::MapFrameEntities>()
    .init_resource::<terrain_chunks::TerrainChunkEntities>()
    // 地形场渲染（ADR-0053 D1，Phase 1 接线 2026-10-24）：quad 资产缓存 + 计数器。
    .init_resource::<wv_render::cellmap_bridge::TerrainFieldRender>()
    .init_resource::<wv_render::cellmap_bridge::FieldRenderStats>()
    // 等值线矢量叠加层（ADR-0012 / plan v0.1 M2）：同属实体集合资源。
    .init_resource::<terrain_view::IsolineOverlay>()
    // 逐格编辑网格（plan v0.1 D4）：同上。
    .init_resource::<terrain_view::GridLinesOverlay>();
}
