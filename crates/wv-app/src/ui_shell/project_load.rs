//! 工程加载：启动请求、后台线程派发、加载遮罩 / 失败卡片、换世界后的视图接管。
//!
//! IO 与 YAML 解析在后台线程（[`crate::project_loader`]），本模块在主线程只做
//! **应用副作用**（写 `Assets<Image>`、改相机、重置播放 / 钻取状态）。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::app_screen::AppScreen;
use crate::project_loader::{PendingProjectLoad, ProjectLoadTask};
use crate::state::{DrillState, PlaybackState, RecentProjects, ViewState, WorldProjection};
use wv_render::terrain_view::{TerrainTexture, TileAtlas};

use super::notice::ShellNotice;
use super::OpenProjectTask;

/// CLI `-m <目录>` 带来的启动加载请求。
///
/// 需求（用户裁定）：启动**不主动加载**任何地图——`None` = 空编辑器起步；
/// `Some(dir)` = 启动后加载。由 [`drive_project_load`] 在遮罩画出并 present 之后
/// 消费：直接在 `Startup` 里做阻塞加载，遮罩来不及画，「白屏无反馈」
/// 只会从启动前挪到首帧前。
#[derive(Resource, Debug, Default)]
pub struct StartupLoadRequest(pub Option<String>);

/// 把**已加载到内存**的工程接管为当前工作副本（v0.2 §A「切换最近工程」）。
///
/// **运行期换世界唯一完整入口**：对话框打开（`poll_open_project_dialog`）、
/// 最近工程（`OpenRecentProject`）、启动加载（`drive_project_load`）最终都落到
/// 这里——但**只在主线程**落。
///
/// 成功后必须同时做四件事，缺一不可：
/// 1. 替换 [`WorldProjection::world`] 与 `project_dir`、清 `dirty`；
/// 2. [`DrillState::reset_to_world`]——**重置地图层级回世界**，否则新工程的
///    路径里还留着旧工程的 id（§A 明确要求）；
/// 3. 推进 [`RecentProjects`] 的顺序；
/// 4. [`adopt_world_snapshot`] 接管纹理 / 相机 / 播放——漏了它就是「加载完成
///    像没加载」：底图停在旧工程、相机纹丝不动（见该函数文档的回归记录）。
///
/// ## 与旧 `load_project_into` 的差别（2026-10-22，C 档）
///
/// 旧函数自己同步做 `Project::open` + `load_world`（磁盘 IO + YAML 解析），
/// 在 egui pass 的某一帧里**阻塞整个 UI**。现在 IO 与解析在后台线程完成
/// （见 [`crate::project_loader`]），本函数只负责**应用副作用**——它要写
/// `Assets<Image>`、改相机 transform / projection，**必须留在主线程**。
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_loaded_world(
    world: wv_core::World,
    dir: &str,
    projection: &mut WorldProjection,
    drill: &mut DrillState,
    recents: &mut RecentProjects,
    notice: &mut ShellNotice,
    diagnostics: &mut crate::state::ProjectDiagnostics,
    edit: &mut crate::edit::editing::EditHistory,
    playback: &mut PlaybackState,
    view: &mut ViewState,
    tex: &mut TerrainTexture,
    atlas: Option<&TileAtlas>,
    images: &mut Assets<Image>,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    open_task: &mut OpenProjectTask,
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
) {
    let name = world
        .meta
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| dir.to_owned());
    recents.remember(dir, &name);
    projection.world = world;
    projection.project_dir = Some(dir.to_owned());
    projection.dirty = false;
    projection.load_error = None;
    // 新工程：清空历史与 dirty 集合，避免旧工程的脏目录被误重写。
    // 历史也清空——切工程后旧撤销栈没有意义（V2 守约：作者是新工程的主人）。
    edit.history = Default::default();
    edit.clear_dirty();
    // ADR-0070 §4.6（T9）：加载期自动迁移——**必须紧跟 History 清空之后**：
    // 迁移事务要落进新工程的干净撤销栈，且「全零报告 → 不入栈」的等价性
    // 依赖此刻栈里只有本事务这一事实（见 migrate_legacy_lines_on_load 文档）。
    // IO/解析虽在后台线程，迁移改的是世界工作副本 + EditHistory（二者都是
    // 主线程所有权），在 WorldProjection 装配后的首帧做是安全的。
    let migration =
        crate::edit::road_sync::migrate_legacy_lines_on_load(&mut projection.world, edit);
    if migration.is_some() {
        // 迁移改了涂格 / 删了手写边 / 落了链实体——工作副本与磁盘分叉了。
        projection.dirty = true;
        // UX-012（2026-10-29 根因钉死）：迁移是**机械格式转换**而非创作，
        // 结果必须立即增量落盘——否则模板世界（`meta.template` + 栅格河/路
        // biome 码恒真触发迁移）每次打开都重迁移一次，顶栏永远「未保存」，
        // 警戒信号被训练成背景噪音。保存失败（V1）不阻塞：dirty 保持，
        // 作者 Ctrl+S 可补。复用保存执行体保证与 Ctrl+S 完全同路径。
        crate::ui_shell::dispatch::dispatch_save_command(
            projection,
            edit,
            open_task,
            notice,
            current_loc,
            frame,
        );
    }
    // ADR-0070 D5（T9）：加载完成后重建推导边（无论是否迁移）——新工程
    // 首帧即有正确路网投影；重建不进脏路径（derived 永不落盘）。
    crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
    // §A：切工程要重置地图层级并清访问痕迹（`reset_to_world` 两者都做）。
    drill.reset_to_world();
    drill.ensure_root(&projection.world);
    // 加载成功：立刻灌一次诊断，避免面板首帧空白。
    // 系统链上 `refresh_project_diagnostics` 也会重算（并清 `needs_diagnose`），
    // 但本帧若不立刻灌，UI 会先看到旧工程的诊断，视觉闪烁。
    // 走 `derive_diagnostics`（错误+提示），不只 `derive_hints`。
    let theme = wv_render::theme::theme_pack::builtin::for_palette(view.map_palette);
    let d = crate::edit::diagnostics_bridge::derive_diagnostics(&projection.world, theme);
    diagnostics.last_count = d.items.len();
    diagnostics.diagnostics = d;
    diagnostics.needs_diagnose = false;
    notice.info(
        format!(
            "{}{}{}",
            crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeProjectLoadedPrefix),
            name,
            crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeProjectLoadedSuffix)
        ),
        frame,
    );
    // 迁移回执后置（V1：迁移可见）：ShellNotice 是单槽，后写的覆盖先写的
    // ——链计数 / chains_skipped 比通用「已加载」更值得留在状态栏。
    if let Some(report) = migration.as_ref() {
        notice.info(crate::edit::road_sync::migration_notice_text(report), frame);
    }
    adopt_world_snapshot(
        projection, drill, playback, view, tex, atlas, images, cameras,
    );
}

/// 加载失败：把原因显示出来（V1——**必须**在窗口内可见，不能静默退化成空白）。
///
/// `message` 是后台线程已经格式化好的完整原因（`打开工程 \`dir\` 失败: …`），
/// 主线程不再拼——它只负责把原因放进 `load_error`（错误卡片读它）与状态栏回执。
fn apply_load_failure(
    message: &str,
    projection: &mut WorldProjection,
    notice: &mut ShellNotice,
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
) {
    projection.load_error = Some(message.to_owned());
    notice.error(
        format!(
            "{}{}",
            crate::i18n::t_key(
                current_loc,
                crate::i18n::I18nKey::NoticeOpenProjectFailedPrefix
            ),
            message
        ),
        frame,
    );
}

/// 加载遮罩：正在加载工程时全屏压暗 + 居中「正在加载」（2026-10-22 起按
/// [`AppScreen`] 求值，见 `app_screen` 模块）。
///
/// 旧口径是「[`StartupLoadRequest`] 有值」——那只覆盖 CLI `-m`。现在加载有
/// 三个发起点（`-m` / 打开对话框 / 最近工程），遮罩必须跟着**屏状态**走，
/// 否则运行期换工程时没有「正在加载」的反馈。
///
/// 用 [`EguiContexts::try_ctx_mut`] 而非 `ctx_mut`：主窗口销毁后同一帧
/// 仍可能轮到本系统（bevy_egui 0.32 关窗路径的已知 panic），此时直接跳过，
/// 不给关窗级联添新的触发点。
pub(crate) fn startup_load_overlay(
    mut ctxs: EguiContexts,
    screen: Res<AppScreen>,
    startup_req: Res<StartupLoadRequest>,
    pending: Res<PendingProjectLoad>,
    task: Res<ProjectLoadTask>,
    current_loc: Res<crate::i18n::CurrentLocale>,
) {
    if !screen.shows_loading_overlay() {
        return;
    }
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    // 正在加载的目录：在跑的任务 > 排队中的 > 还没派发的启动请求。
    let dir = task
        .dir()
        .or(pending.0.as_deref())
        .or(startup_req.0.as_deref())
        .unwrap_or_default();
    let screen_rect = ctx.viewport_rect();
    egui::Area::new(egui::Id::new("wv_startup_load_dim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen_rect.min)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(150));
        });
    egui::Window::new(crate::i18n::t_key(
        &current_loc,
        crate::i18n::I18nKey::LoadOverlayTitle,
    ))
    .order(egui::Order::Foreground)
    .collapsible(false)
    .resizable(false)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ctx, |ui| {
        ui.with_layout(
            egui::Layout::top_down_justified(egui::Align::Center),
            |ui| {
                ui.add(egui::Spinner::new().size(32.0));
                ui.add_space(6.0);
                if !dir.is_empty() {
                    ui.label(dir);
                }
            },
        );
    });
}

/// 加载失败卡片：居中显示失败原因与下一步指引，点「知道了」关闭。
///
/// 需求（用户裁定）：加载失败**必须在窗口内显示原因**——控制台日志一闪
/// 而过，用户只看到「没画出来」，无从知道是路径错了还是 YAML 坏了。
/// 对话框打开工程失败（[`apply_load_failure`]）同样落到这里。
pub(crate) fn load_error_card(
    mut ctxs: EguiContexts,
    mut projection: ResMut<WorldProjection>,
    screen: Res<AppScreen>,
    current_loc: Res<crate::i18n::CurrentLocale>,
) {
    if !screen.shows_error_card() {
        return;
    }
    let Some(err) = projection.load_error.clone() else {
        return;
    };
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    // UX-018：失败卡片三段文案走 i18n（此前硬编码中文，英文界面下同屏混排）。
    egui::Window::new(crate::i18n::t_key(
        &current_loc,
        crate::i18n::I18nKey::LoadErrorTitle,
    ))
    .order(egui::Order::Foreground)
    .collapsible(false)
    .resizable(false)
    .default_width(560.0)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ctx, |ui| {
        ui.label(egui::RichText::new(&err).color(egui::Color32::LIGHT_RED));
        ui.add_space(8.0);
        ui.label(crate::i18n::t_key(
            &current_loc,
            crate::i18n::I18nKey::LoadErrorGuidance,
        ));
        ui.add_space(8.0);
        if ui
            .button(crate::i18n::t_key(
                &current_loc,
                crate::i18n::I18nKey::LoadErrorDismiss,
            ))
            .clicked()
        {
            projection.load_error = None;
        }
    });
}

/// 工程加载驱动：**派发后台线程 + 主线程轮询结果并应用**（2026-10-22，C 档）。
///
/// ## 三步
///
/// 1. **收口排队意图**：CLI `-m`（[`StartupLoadRequest`]）与运行期排队
///    （对话框 / 最近工程 → [`PendingProjectLoad`]）汇到一处；
/// 2. **派发后台线程**：没有任务在跑且有排队 → [`ProjectLoadTask::start`]。
///    IO 与 YAML 解析在后台跑，**主线程不阻塞**（旧实现在 egui pass 里同步
///    加载，大工程加载期间窗口直接冻住）；
/// 3. **主线程应用**：结果回来 → [`apply_loaded_world`] / [`apply_load_failure`]
///    （这一步要写 `Assets<Image>`、改相机，**只能**在主线程做）。
///
/// ## 为什么不再有「帧 3 才开始加载」的门槛
///
/// 旧门槛是为**阻塞式**加载设计的：帧 0/1 画遮罩 + 揭示窗口，帧 2 才加载，
/// 好让用户先看到遮罩在转。现在加载在后台线程，**派发本身不耗时**，
/// 早派发 = 早开始 = 早出图——这正是「加快系统第一版面」的收益所在。
/// 窗口揭示时机（`reveal_window_once_first_frame_presented`）不受影响。
///
/// ## 参数
///
/// Bevy 单系统参数上限 16（本系统 15）。可成对的 Resources 已折成元组：
/// `time` + `current_loc` 一组。
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_project_load(
    mut startup_req: ResMut<StartupLoadRequest>,
    mut pending: ResMut<PendingProjectLoad>,
    mut task: ResMut<ProjectLoadTask>,
    (mut frames, mut hinted): (Local<u32>, Local<bool>),
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut recents: ResMut<RecentProjects>,
    mut notice: ResMut<ShellNotice>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    (mut playback, mut view): (ResMut<PlaybackState>, ResMut<ViewState>),
    // UX-012：迁移自动保存走 `dispatch_save_command`，需要它的 SaveAs 兜底口
    // （open_task 折进同层元组——单系统 16 参数上限）。
    (mut tex, mut open_task): (ResMut<TerrainTexture>, ResMut<OpenProjectTask>),
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    (time, current_loc): (Res<Time>, Res<crate::i18n::CurrentLocale>),
) {
    *frames += 1;
    // 1) CLI `-m` 的请求**只消费一次**（转成排队意图）。
    //    若此刻已有别的排队（用户刚点了「打开工程…」），CLI 请求作废——
    //    用户的显式操作优先；且绝不能让它「等排队空了再补跑」，
    //    否则刚换好的工程会被 CLI 指定的旧工程再覆盖一次。
    if let Some(dir) = startup_req.0.take() {
        if pending.0.is_none() {
            pending.0 = Some(dir);
        }
    }
    // 2) 派发后台线程（已有任务在跑则跳过——同一时刻只加载一个工程）。
    if !task.is_active() {
        if let Some(dir) = pending.0.take() {
            *hinted = true;
            task.start(dir);
        }
    }
    let frame = (time.elapsed_secs() * 60.0) as u64;
    // 3) 主线程应用结果。
    if let Some((dir, outcome)) = task.poll() {
        match outcome {
            Ok(world) => apply_loaded_world(
                world,
                &dir,
                &mut projection,
                &mut drill,
                &mut recents,
                &mut notice,
                &mut diagnostics,
                &mut edit,
                &mut playback,
                &mut view,
                &mut tex,
                atlas.as_deref(),
                &mut images,
                &mut cameras,
                &mut open_task,
                frame,
                &current_loc,
            ),
            Err(message) => {
                apply_load_failure(&message, &mut projection, &mut notice, frame, &current_loc)
            }
        }
    }
    // 4) 空编辑器起步（无 `-m`、也没有别的加载）时给一条一次性状态栏指引。
    //    帧 3 之后才说：让窗口先把欢迎页亮出来（隐藏起步的揭示在帧 1 末）。
    if *frames >= 3 && !*hinted && !task.is_active() && pending.0.is_none() {
        *hinted = true;
        // 2026-10-14 起无工程时欢迎页全屏接管，指引跟着改口。
        notice.info(
            crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeNoProjectNeed),
            frame,
        );
    }
}

/// 工作副本整体替换后的视图 / 纹理 / 播放接管——**加载成功 / 关闭工程 / 新建工程**
/// 三个入口共用（原 `adopt_loaded_world`，2026-10-14 收编，见下方回归说明）。
///
/// 三件事缺一不可：
/// 1. `TerrainTexture` 整体重建：Startup 只按空世界建过一次纹理，
///    `rebuild_texture_on_mode_change` 只认「模式 / 来源图变化」——换世界后若不换
///    纹理，画面会永远停留在旧底图上；
/// 2. `ViewState` + 相机 transform 重新 fit：相机 transform 才是主真源
///    （`setup_camera` 只在 Startup 跑一次），只写 ViewState 相机纹丝不动；
/// 3. `PlaybackState` 按新工程 `timeline/axis.yaml` 重置，当前日回到起点
///    （旧工程的播放日可能落在新范围之外）。
///
/// ## 为什么关闭 / 新建也必须走这里（回归 2026-10-14：关闭工程「关了像没关」）
///
/// `rebuild_texture_on_mode_change` 在 `active_terrain` 为 `None`（空世界没有任何
/// 图）时**故意早退不重建**（等地形就绪再建，见该函数文档）。而「关闭工程」恰好
/// 把世界换成了空世界——于是旧工程的底图永远挂在屏上，用户看到的是「已关闭工程」
/// 的回执 + 原封不动的旧地图。加载成功走 `last_map` 不一致触发重建，掩盖了这条
/// 路径；关闭 / 新建没有任何触发器，只能在这里显式接管。
// 参数偏多：本函数是「工作副本整体替换后接管运行期资源」的清单，逐项对应一个
// 资源，拆成结构体只是把同一份清单挪个地方，反而看不清谁被接管。
#[allow(clippy::too_many_arguments)]
pub(crate) fn adopt_world_snapshot(
    projection: &mut WorldProjection,
    drill: &DrillState,
    playback: &mut PlaybackState,
    view: &mut ViewState,
    tex: &mut TerrainTexture,
    atlas: Option<&TileAtlas>,
    images: &mut Assets<Image>,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    // 地形保底（与 `setup_terrain_texture_after_world` 同一不变量：总有底图可用）。
    // Phase 2：底图按图存，取「当前图向上的第一张自带栅格」，都没有才塞占位。
    let (map, data) = wv_render::terrain_view::active_terrain_or_placeholder(
        &mut projection.world,
        drill.current(),
        512,
    );
    let image = wv_render::terrain_view::image_from_terrain(
        &data,
        wv_render::cellmap_bridge::shade_mode_of(view.mode),
        atlas,
        view.terrain_soften,
        &wv_render::cellmap_bridge::conv_pack_with(
            view.active_theme,
            view.hypsometric_tint,
            view.mode,
        ),
        0.0,
    );
    wv_render::terrain_view::replace_terrain_texture(
        tex,
        images,
        image,
        &data,
        map,
        wv_render::cellmap_bridge::shade_mode_of(view.mode),
        view.terrain_soften,
        view.active_theme.id,
        view.hypsometric_tint,
    );
    // 整图内容已整体替换：chunk 层全量重贴。
    tex.mark_all_dirty();
    let (center, scale) = crate::app_bootstrap::fit_view_for_world(&projection.world);
    view.camera_center = center;
    view.scale = scale;
    // 同步当前图 id 镜像。`drill` 是 `Res<DrillState>`（借用）所以这里手动调——
    // 不调的话状态栏的「已选」与新工程的真实层级会有一帧失同步。
    drill.sync_view(view);
    playback.range_from = projection
        .world
        .timeline
        .as_ref()
        .map(|t| t.timeline_range.from)
        .unwrap_or(1);
    playback.range_to = projection
        .world
        .timeline
        .as_ref()
        .and_then(|t| t.timeline_range.to)
        .unwrap_or(180_001);
    playback.current_day = playback.range_from;
    for (mut transform, mut proj) in cameras.iter_mut() {
        transform.translation.x = center.x;
        transform.translation.y = center.y;
        wv_render::camera::set_ortho_scale(&mut proj, scale);
    }
}
