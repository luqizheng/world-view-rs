//! 界面外壳装配：把主题、快捷键、命令面板、布局持久化接入 Bevy App。
#![cfg(feature = "app")]

//! ## 职责（设计规格 §9.2「改 gui.rs」的落点）
//!
//! - 注册 [`crate::ui_theme`] / [`crate::ui_cmdk`] / [`crate::ui_layout`]。
//! - 装配快捷键（§6.1，19 条）与命令面板（§6.3）。
//! - 布局持久化（§10 Q5）：面板被拖动 / 折叠 / 显隐时写回 `ui-layout.yaml`。
//!
//! ## 为什么单独一个模块
//!
//! `gui.rs` 是 Bevy 装配层（相机、地形、投影）；界面外壳的装配与之正交，
//! 拆开后两侧各自可测、可替换（D2：换 Theme Pack 不该动装配层）。

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy_egui::EguiContexts;

use crate::state::{DrillState, Panel, PanelState, RecentProjects, ViewState, WorldProjection};
use crate::ui_cmdk::{CommandId, CommandPalette};
use crate::ui_layout::LayoutFile;
use crate::ui_theme::UiShellConfig;
use crate::ui_tokens::{UiDensity, UiTheme};

/// 把界面外壳的资源与系统注册进 `App`。
///
/// 调用顺序：在 `EguiPlugin` 之后、UI 面板系统之前。
pub fn register(app: &mut App) {
    app.init_resource::<UiShellConfig>()
        .init_resource::<CommandPalette>()
        .init_resource::<crate::ui::ProjectMenuState>()
        .init_resource::<ShellNotice>()
        .init_resource::<DrillState>()
        .init_resource::<RecentProjects>()
        .init_resource::<LayoutFile>()
        .init_resource::<OpenProjectTask>()
        // 根图落位必须早于所有读 `DrillState` 的 UI 系统（同一 `Update` 里靠顺序保证）。
        .add_systems(
            PreUpdate,
            (
                seed_drill_state,
                hotkeys.after(bevy_egui::EguiPreUpdateSet::InitContexts),
                poll_open_project_dialog,
            ),
        )
        .add_systems(
            Update,
            (
                apply_pending_commands,
                crate::ui::command_palette,
                sync_drill_mirror,
                persist_layout_on_change,
            )
                .chain(),
        );
}

/// 把钻取路径钉到根图（`world.meta.root_map`），并镜像到 [`ViewState::current_map`]。
///
/// 放在 `PreUpdate`：本帧的渲染 / 拾取随后都读得到一致的两份状态。
fn seed_drill_state(
    projection: Res<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut view: ResMut<ViewState>,
) {
    drill.ensure_root(&projection.world);
    drill.sync_view(&mut view);
}

/// 每帧把 `DrillState` 的当前图写回 `ViewState::current_map` 镜像。
///
/// **方向只有一个**：`DrillState` → `ViewState`。反向赋值会让面包屑与
/// 世界树各说各话（`current_map` 没有「从哪来」的信息，表达不了路径）。
fn sync_drill_mirror(drill: Res<DrillState>, mut view: ResMut<ViewState>) {
    if drill.is_changed() {
        drill.sync_view(&mut view);
    }
}

/// 快捷键（§6.1，19 条；**禁止占用裸 `Tab`**）。
///
/// **输入框聚焦时全部快捷键失效**（除 `Esc` / `Ctrl+K`）——
/// 在打字时按 `1` 不该切视图（§6.1）。
fn hotkeys(
    mut contexts: EguiContexts,
    mut palette: ResMut<CommandPalette>,
    mut cfg: ResMut<UiShellConfig>,
    mut panels: ResMut<PanelState>,
) {
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    let wants_text = ctx.wants_keyboard_input();
    let input = ctx.input(|i| i.clone());

    // Esc：关闭命令面板（即使聚焦输入框也生效）。
    if input.key_pressed(bevy_egui::egui::Key::Escape) && palette.open {
        palette.close();
        return;
    }

    // Ctrl+K：命令面板（任意时刻可用）。
    if input.modifiers.command && input.key_pressed(bevy_egui::egui::Key::K) {
        palette.open();
        return;
    }

    // 输入框聚焦时，除 Esc / Ctrl+K 外全部失效（§6.1）。
    if wants_text {
        return;
    }

    let cmd = input.modifiers.command;
    let shift = input.modifiers.shift;

    // Ctrl+J：切换明暗。
    if cmd && input.key_pressed(bevy_egui::egui::Key::J) {
        cfg.theme = cfg.theme.toggled();
    }
    // Ctrl+Shift+I：沉浸模式（裸 Tab 保留给焦点移动，WCAG 2.1.1）。
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::I) {
        panels.immersive = !panels.immersive;
    }
    // 数字 1–4：切换视图模式。
    for (i, key) in [
        bevy_egui::egui::Key::Num1,
        bevy_egui::egui::Key::Num2,
        bevy_egui::egui::Key::Num3,
        bevy_egui::egui::Key::Num4,
    ]
    .iter()
    .enumerate()
    {
        if input.key_pressed(*key) {
            palette.pending = Some(CommandId::SetViewMode(i as u8));
        }
    }
    // Ctrl+S / Ctrl+Shift+S：保存 / 另存为（后者先判，否则 Shift 版会被当成普通保存）。
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::S) {
        palette.pending = Some(CommandId::SaveProjectAs);
    } else if cmd && input.key_pressed(bevy_egui::egui::Key::S) {
        palette.pending = Some(CommandId::Save);
    }
    // Ctrl+O / Ctrl+E / Ctrl+W：工程命令（v0.2 §A）。
    if cmd && input.key_pressed(bevy_egui::egui::Key::O) {
        palette.pending = Some(CommandId::OpenProject);
    }
    if cmd && input.key_pressed(bevy_egui::egui::Key::E) {
        palette.pending = Some(CommandId::ExportProject);
    }
    if cmd && input.key_pressed(bevy_egui::egui::Key::W) {
        palette.pending = Some(CommandId::CloseProject);
    }
    // Ctrl+Shift+N：新建工程（同样先判 Shift 版）。
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::N) {
        palette.pending = Some(CommandId::NewProject);
    }
    if cmd && shift && input.key_pressed(bevy_egui::egui::Key::Z) {
        palette.pending = Some(CommandId::Redo);
    } else if cmd && input.key_pressed(bevy_egui::egui::Key::Z) {
        palette.pending = Some(CommandId::Undo);
    }
    // Backspace：钻回上一级地图（v0.2 §B.2）。
    // 输入框聚焦时不会走到这里（上面已 `return`），故与「删除文字」不冲突。
    if input.key_pressed(bevy_egui::egui::Key::Backspace) {
        palette.pending = Some(CommandId::DrillUp);
    }
    // Space：播放 / 暂停。
    if input.key_pressed(bevy_egui::egui::Key::Space) {
        palette.pending = Some(CommandId::TogglePlay);
    }
    // Home / End：时间轴首 / 尾。
    if input.key_pressed(bevy_egui::egui::Key::Home) {
        palette.pending = Some(CommandId::JumpToStart);
    }
    if input.key_pressed(bevy_egui::egui::Key::End) {
        palette.pending = Some(CommandId::JumpToEnd);
    }
    // [ ]：笔刷半径。
    if input.key_pressed(bevy_egui::egui::Key::OpenBracket) {
        palette.pending = Some(CommandId::BrushRadius(-1));
    }
    if input.key_pressed(bevy_egui::egui::Key::CloseBracket) {
        palette.pending = Some(CommandId::BrushRadius(1));
    }
    // F：全览地图。
    if input.key_pressed(bevy_egui::egui::Key::F) {
        palette.pending = Some(CommandId::FitMap);
    }
}

/// 一次用户可见的操作结果回执（成功 / 失败都显示）。
///
/// **为什么要有它**：v0.2 新增了六条「打开 / 另存 / 导出 / 关闭 / 设置」类命令，
/// 其中一部分在当前架构下**做不到**（见 [`dispatch_project_command`]）。让这些命令
/// 静默无反应会违反 §5.3「错误必须给出人类可读的原因与下一步」与 D5 零隐藏功能——
/// 用户会以为程序坏了。故一律回执。
#[derive(Resource, Debug, Default)]
pub struct ShellNotice {
    /// 最近一条回执文案。
    pub text: Option<String>,
    /// 是否为错误（决定状态栏里的徽标类型）。
    pub is_error: bool,
    /// 生成该回执的帧号：状态栏只显示最近若干秒，避免永久占位。
    pub frame: u64,
}

impl ShellNotice {
    fn info(&mut self, text: impl Into<String>, frame: u64) {
        self.text = Some(text.into());
        self.is_error = false;
        self.frame = frame;
    }

    fn error(&mut self, text: impl Into<String>, frame: u64) {
        self.text = Some(text.into());
        self.is_error = true;
        self.frame = frame;
    }

    /// 回执是否还在展示窗口内（约 6 秒 @60fps）。
    pub fn is_fresh(&self, now: u64) -> bool {
        self.text.is_some() && now.saturating_sub(self.frame) < 360
    }
}

/// 「打开工程…」对话框的进行中任务（v0.2 §A）。
///
/// 入口（菜单 / `Ctrl+O` / 命令面板）把用户想打开工程的意图交给 [`Self::spawn`]
/// ——`rfd::AsyncFileDialog` 的 future 在 `AsyncComputeTaskPool` 上跑，不阻塞渲染。
/// 每帧的 [`poll_open_project_dialog`] 拿这个资源判 `is_finished()`；用户关掉
/// 对话框那一刻 `Task` 完成，`block_on` 拿到 `Option<PathBuf>`，再走
/// [`load_project_into`]。
///
/// ## 为什么单独一个资源（不塞进 `CommandPalette`）
///
/// `pending` 是「下一个要执行的命令」，生命周期一帧；这里是要「等用户回话」，
/// 跨帧存活，且同一时间只能有一个对话框在跑——独立的资源类型能让这两件事互不
/// 污染，也方便测试时直接 `let mut t = OpenProjectTask::default();`。
#[derive(Resource, Default)]
pub struct OpenProjectTask {
    task: Option<Task<Option<PathBuf>>>,
}

impl OpenProjectTask {
    /// 起一个原生目录选择对话框。重复点击会**取消**上一次未完成的对话框
    /// ——只保留最新的意图（与 macOS Finder、Windows 资源管理器的行为一致）。
    pub fn spawn(&mut self) {
        let pool = AsyncComputeTaskPool::get();
        let task = pool.spawn(async move {
            // `set_title` 让 Windows / macOS / Linux 三大平台都显示一致的话术；
            // rfd 在 WASM 上 `pick_folder` 不支持（见 crate 文档），但本工程
            // 不打 WASM，无需在该资源管理形式分支里设防。
            rfd::AsyncFileDialog::new()
                .set_title("选择工程目录（含 world.yaml 的目录）")
                .pick_folder()
                .await
                .map(|h| h.path().to_path_buf())
        });
        // 旧任务直接 `drop`：它会自动取消（`Task<T>::Drop` 文档），
        // 不会再回写到一个已被替换的 `task`，不会与新对话框争抢。
        self.task = Some(task);
    }

    /// 每帧轮询一次：完成就 `block_on` 取结果，否则返回 `None`。
    ///
    /// `block_on` 只在用户关掉对话框的**那一帧**执行，瞬时阻塞可接受
    /// （用户此刻正等着反馈，渲染线程卡几毫秒不显违和）。
    fn take_if_finished(&mut self) -> Option<Option<PathBuf>> {
        let task = self.task.as_ref()?;
        if !task.is_finished() {
            return None;
        }
        // `Task<T>` 实现了 `Future`（Unpin），可以直接 `block_on`。
        let task = self.task.take()?;
        Some(bevy::tasks::block_on(task))
    }
}

/// 每帧看一眼「打开工程」对话框是否回来了（v0.2 §A）。
///
/// 顺序：在 `seed_drill_state` 之后、`hotkeys` 之后——`hotkeys` 可能
/// 会在这一帧把 `OpenProjectTask::spawn` 叫起来，本系统再读同一个资源
/// 至少要等到写入完成之后；同一 `PreUpdate` 内系统顺序由 `.after(...)` 声明，
/// 这里靠「放在 `hotkeys` 之后」+「下一帧才 `block_on`」共同保证不读到半截状态。
fn poll_open_project_dialog(
    mut task: ResMut<OpenProjectTask>,
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut recents: ResMut<RecentProjects>,
    mut notice: ResMut<ShellNotice>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
    mut edit: ResMut<crate::editing::EditHistory>,
    time: Res<Time>,
) {
    let Some(outcome) = task.take_if_finished() else {
        return;
    };
    let frame = (time.elapsed_secs() * 60.0) as u64;
    // 用户选了目录并确认。目录用 `display()` 直接给人类读——
    // 这里不进 YAML，路径含空格 / 非 ASCII 时不要做 `as_ref` 路径切片，
    // 否则 Windows 的 `\\?\` 前缀会被吞。
    // 用户点了「取消」（`None`）：静默。不刷回执，免得状态栏常驻一条「已取消」。
    if let Some(path) = outcome {
        let dir = path.to_string_lossy().into_owned();
        load_project_into(
            &dir,
            &mut projection,
            &mut drill,
            &mut recents,
            &mut notice,
            &mut diagnostics,
            &mut edit,
            frame,
        );
    }
}

/// 从磁盘加载一个工程并替换当前工作副本（v0.2 §A「切换最近工程」）。
///
/// 成功后必须同时做三件事，缺一不可：
/// 1. 替换 [`WorldProjection::world`] 与 `project_dir`、清 `dirty`；
/// 2. [`DrillState::reset_to_world`]——**重置地图层级回世界**，否则新工程的
///    路径里还留着旧工程的 id（§A 明确要求）；
/// 3. 推进 [`RecentProjects`] 的顺序。
fn load_project_into(
    dir: &str,
    projection: &mut WorldProjection,
    drill: &mut DrillState,
    recents: &mut RecentProjects,
    notice: &mut ShellNotice,
    diagnostics: &mut crate::state::ProjectDiagnostics,
    edit: &mut crate::editing::EditHistory,
    frame: u64,
) {
    match wv_io::Project::open(dir).and_then(|p| p.load_world()) {
        Ok(world) => {
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
            // §A：切工程要重置地图层级并清访问痕迹（`reset_to_world` 两者都做）。
            drill.reset_to_world();
            drill.ensure_root(&projection.world);
            // 加载成功：立刻灌一次诊断，避免面板首帧空白。
            // 系统链上 `refresh_project_diagnostics` 也会重算（并清 `needs_diagnose`），
            // 但本帧若不立刻灌，UI 会先看到旧工程的诊断，视觉闪烁。
            let d = wv_io::diagnostics::derive_hints(&projection.world);
            diagnostics.last_count = d.items.len();
            diagnostics.diagnostics = d;
            diagnostics.needs_diagnose = false;
            notice.info(format!("已加载工程「{name}」"), frame);
        }
        Err(e) => {
            projection.load_error = Some(format!("打开工程 `{dir}` 失败: {e}"));
            notice.error(format!("打开工程失败：{e}"), frame);
        }
    }
}

/// 分发工程 / 导航类命令（v0.2 §A §B），返回是否已消费该命令。
///
/// ## 能力边界（如实回执，不假装成功）
///
/// | 命令 | 现状 |
/// |---|---|
/// | 打开工程… | ✅ 原生目录选择对话框（`rfd`），加载走 [`wv_io::Project::open`] |
/// | 打开最近工程 | ✅ 真加载（`wv_io::Project::open`） |
/// | 新建工程 | ✅ 内存中新建空白工程（尚未落盘） |
/// | 关闭工程 | ✅ 清空工作副本 |
/// | 另存为… | ⛔ 需要文件对话框 → 提示走 `Ctrl+S`（v0.3 一起补） |
/// | 导出… | ⛔ 导出格式未定（见 v0.2 §G） |
/// | 项目设置 | ⛔ 面板未实现（v0.3） |
/// | 保存 | ✅ 阶段七 v0.1：`wv_io::Project::save_world` 全量写，dirty 路由 v0.2 |
///
/// 保存那条不是偷懒：数据落盘要处理「每实体一文件 + 引用完整性 + 二进制分块」
/// 三件事（AGENTS §5/§4.5），不是一次 `serde_yaml::to_string` 能收尾的。
fn dispatch_project_command(
    id: &CommandId,
    projection: &mut WorldProjection,
    drill: &mut DrillState,
    recents: &mut RecentProjects,
    notice: &mut ShellNotice,
    open_task: &mut OpenProjectTask,
    diagnostics: &mut crate::state::ProjectDiagnostics,
    edit: &mut crate::editing::EditHistory,
    frame: u64,
) -> bool {
    match id {
        CommandId::OpenRecentProject(i) => {
            let list = recents.visible(projection);
            match list.get(*i as usize) {
                Some(entry) => {
                    let dir = entry.dir.clone();
                    load_project_into(
                        &dir,
                        projection,
                        drill,
                        recents,
                        notice,
                        diagnostics,
                        edit,
                        frame,
                    );
                }
                None => notice.error("最近工程列表已变化，请重开菜单", frame),
            }
            true
        }
        CommandId::NewProject => {
            projection.world = wv_core::World::default();
            projection.project_dir = None;
            projection.dirty = true;
            projection.load_error = None;
            drill.reset_to_world();
            // 新建空白工程：清空诊断并打上重算标（避免显示旧工程的提示）。
            diagnostics.diagnostics = wv_io::diagnostics::Diagnostics::default();
            diagnostics.last_count = 0;
            diagnostics.needs_diagnose = true;
            // 新工程：清空编辑历史与 dirty 集合（与 `load_project_into` 同源）。
            edit.history = Default::default();
            edit.clear_dirty();
            notice.info("已新建空白工程（尚未落盘）", frame);
            true
        }
        CommandId::CloseProject => {
            projection.world = wv_core::World::default();
            projection.project_dir = None;
            projection.dirty = false;
            projection.load_error = None;
            drill.reset_to_world();
            // 关闭工程：与新建同样清诊断。
            diagnostics.diagnostics = wv_io::diagnostics::Diagnostics::default();
            diagnostics.last_count = 0;
            diagnostics.needs_diagnose = true;
            edit.history = Default::default();
            edit.clear_dirty();
            notice.info("已关闭工程", frame);
            true
        }
        CommandId::OpenProject => {
            // 发起系统目录选择对话框（不阻塞）；选完路径由
            // `poll_open_project_dialog` 收口、复用 `OpenRecentProject` 的同一条
            // `load_project_into` 加载路径——结果就只是「多了一条最近工程」。
            open_task.spawn();
            true
        }
        CommandId::SaveProjectAs => {
            notice.info("另存为需要文件对话框（本版本未接）；保存请用 Ctrl+S", frame);
            true
        }
        CommandId::ExportProject => {
            notice.info(
                "导出尚未实现：需要先定导出格式（PNG / SVG），见 v0.2 §G",
                frame,
            );
            true
        }
        CommandId::ProjectSettings => {
            notice.info("项目设置面板待 v0.3", frame);
            true
        }
        CommandId::DrillUp => {
            if !drill.up() {
                notice.info("已在世界地图，无法再钻出", frame);
            }
            true
        }
        CommandId::DrillToRoot => {
            drill.jump_to(0);
            true
        }
        CommandId::DrillToSegment(i) => {
            // 面包屑点第 i 段（0 = 根）。
            if !drill.jump_to(*i as usize) {
                notice.info("该层级就是当前层级", frame);
            }
            true
        }
        _ => false,
    }
}

/// 执行命令面板 / 快捷键 / 菜单排队的命令。
///
/// **唯一分发器**：所有入口只写 `palette.pending = Some(id)`，由这里统一执行。
/// 拆成多个系统各自 `take()` 会在「谁先拿到」上产生时序耦合。
#[allow(clippy::too_many_arguments)]
fn apply_pending_commands(
    mut palette: ResMut<CommandPalette>,
    mut panels: ResMut<PanelState>,
    mut cfg: ResMut<UiShellConfig>,
    mut view: ResMut<ViewState>,
    mut playback: ResMut<crate::state::PlaybackState>,
    mut brush: ResMut<crate::state::BrushState>,
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut recents: ResMut<RecentProjects>,
    mut notice: ResMut<ShellNotice>,
    mut edit: ResMut<crate::editing::EditHistory>,
    mut tex: ResMut<crate::terrain_view::TerrainTexture>,
    mut open_task: ResMut<OpenProjectTask>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
    time: Res<Time>,
) {
    let Some(id) = palette.pending.take() else {
        return;
    };
    // 帧号：回执过期判断只依赖它，不引入计时器资源。
    let frame = (time.elapsed_secs() * 60.0) as u64;

    // 工程 / 导航类先试（它们改的是世界与路径，与下面的视图状态互不重叠）。
    if dispatch_project_command(
        &id,
        &mut projection,
        &mut drill,
        &mut recents,
        &mut notice,
        &mut open_task,
        &mut diagnostics,
        &mut edit,
        frame,
    ) {
        return;
    }

    match id {
        CommandId::SetViewMode(i) => {
            if let Some(m) = crate::state::ViewMode::ALL.get(i as usize) {
                view.mode = *m;
            }
        }
        CommandId::OpenCommandPalette => palette.open(),
        CommandId::ToggleTheme => cfg.theme = cfg.theme.toggled(),
        CommandId::ToggleDensity => cfg.density = cfg.density.toggled(),
        CommandId::ToggleImmersive => panels.immersive = !panels.immersive,
        CommandId::TogglePanel(p) => panels.toggle(p),
        CommandId::ResetPanelLayout => panels.reset_requested = true,
        CommandId::FitMap => {
            view.camera_center = Vec2::ZERO;
            view.scale = 1.0;
        }
        CommandId::Zoom(d) => {
            view.scale = (view.scale * if d > 0 { 1.25 } else { 0.8 }).clamp(0.1, 8.0);
        }
        CommandId::TogglePlay => playback.playing = !playback.playing,
        CommandId::JumpToStart => playback.current_day = playback.range_from,
        CommandId::JumpToEnd => playback.current_day = playback.range_to,
        CommandId::BrushRadius(d) => {
            brush.radius = (brush.radius + if d > 0 { 8.0 } else { -8.0 }).clamp(8.0, 600.0);
        }
        // 撤销 / 重做走 `editing` 的公开入口（同一份逻辑，不复制）。
        CommandId::Undo | CommandId::Redo => {
            let redo = matches!(id, CommandId::Redo);
            match crate::editing::apply_history_command(
                redo,
                &mut edit,
                &mut projection,
                &mut tex,
                &playback,
            ) {
                Ok(msg) => {
                    // 撤销/重做后世界可能改——诊断需要重算。
                    // `Ok(false)`（无可撤销/重做）时不动，避免无谓重算。
                    if msg != "没有可撤销的操作" && msg != "没有可重做的操作" {
                        diagnostics.needs_diagnose = true;
                    }
                    notice.info(msg, frame);
                }
                Err(msg) => notice.error(msg, frame),
            }
        }
        // 保存（阶段七 v0.2）：走 `wv_io::Project::save_world`，只重写 dirty
        // 目录。`take_dirty` 原子取空集合；空 Vec 走全量路径（首次保存、
        // 刚加载、刚撤销等场景）。失败不排空——脏标记仍在，下次保存能补上。
        CommandId::Save => {
            let Some(dir) = projection.project_dir.clone() else {
                notice.error("当前没有工程目录，无法保存", frame);
                return;
            };
            let dirty = edit.take_dirty();
            let n = dirty.len();
            match wv_io::Project::open(&dir) {
                Ok(project) => match project.save_world(&projection.world, &dirty) {
                    Ok(()) => {
                        projection.dirty = false;
                        let scope = if n == 0 {
                            "全量".to_owned()
                        } else {
                            format!("{n} 个目录")
                        };
                        notice.info(format!("已保存到 {dir}（{scope}）"), frame);
                    }
                    Err(e) => {
                        // 失败时把 dirty 放回，让下次保存能再试。
                        edit.record_dirty(dirty);
                        notice.error(format!("保存失败：{e}"), frame);
                    }
                },
                Err(e) => notice.error(format!("打开工程目录失败：{e}"), frame),
            }
        }
        // 其余工程 / 导航命令已在上面消费。
        CommandId::OpenRecentProject(_)
        | CommandId::NewProject
        | CommandId::OpenProject
        | CommandId::SaveProjectAs
        | CommandId::CloseProject
        | CommandId::ExportProject
        | CommandId::ProjectSettings
        | CommandId::DrillUp
        | CommandId::DrillToRoot
        | CommandId::DrillToSegment(_) => {}
    }
}

/// 布局持久化：把面板的**真实几何**回写 `ui-layout.yaml`（Q5）。
///
/// 位置的真相在 egui 的窗口记忆里（`Memory::area_rect`），不在 [`crate::ui_layout::default_layout`]。
/// 拖动过程中不写盘（`is_using_pointer`），松手后的那一帧才落盘——避免 60 次/秒的文件写。
fn persist_layout_on_change(
    mut contexts: EguiContexts,
    panels: Res<PanelState>,
    cfg: Res<UiShellConfig>,
    mut layout: ResMut<LayoutFile>,
    project: Res<WorldProjection>,
) {
    let Some(path) = crate::ui_layout::layout_path(project.project_dir.as_deref()) else {
        return;
    };
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };

    let mut file = (*layout).clone();
    file.theme = Some(match cfg.theme {
        UiTheme::Dark => "dark".into(),
        UiTheme::Light => "light".into(),
    });
    file.density = Some(match cfg.density {
        UiDensity::Cozy => "cozy".into(),
        UiDensity::Compact => "compact".into(),
    });

    let mut changed = false;
    for p in Panel::ALL {
        let id = bevy_egui::egui::Id::new(("wv_panel", p));
        let rect = ctx
            .memory(|m| m.area_rect(id))
            .map(|r| [r.min.x, r.min.y, r.width(), r.height()]);
        // egui 的窗口折叠态与 `Window` 同 Id，另挂一个 "collapsing" 子 Id。
        let collapsed =
            bevy_egui::egui::collapsing_header::CollapsingState::load(ctx, id.with("collapsing"))
                .map(|s| !s.is_open())
                .unwrap_or(false);
        let before = file.get(p);
        crate::ui_layout::record_layout(&mut file, p, rect, collapsed, panels.is_visible(p));
        if file.get(p) != before {
            changed = true;
        }
    }
    if !changed {
        return;
    }
    // 拖动中不落盘：松手后的帧自然还是 changed，届时再写（只写一次）。
    if ctx.is_using_pointer() {
        return;
    }
    file.save(&path);
    *layout = file;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immersive_toggle_flips_state() {
        // §6.4：沉浸模式只隐藏面板，不隐藏时间轴与日期。
        let mut p = PanelState::default();
        assert!(!p.immersive);
        p.immersive = !p.immersive;
        assert!(p.immersive);
    }

    #[test]
    fn theme_toggle_round_trips() {
        let mut c = UiShellConfig::default();
        assert_eq!(c.theme, UiTheme::Dark);
        c.theme = c.theme.toggled();
        assert_eq!(c.theme, UiTheme::Light);
        c.theme = c.theme.toggled();
        assert_eq!(c.theme, UiTheme::Dark);
    }

    #[test]
    fn open_project_task_is_idle_by_default() {
        // 没有 `spawn()` 过的任务，轮询必须给 `None`，否则「打开工程」会被
        // 默认状态误触发。
        let mut t = OpenProjectTask::default();
        assert!(t.task.is_none());
        assert!(t.take_if_finished().is_none());
    }

    #[test]
    fn open_project_task_does_not_block_when_dialog_is_still_open() {
        // 模拟「对话框还开着」——用一个 pending 的 future 装进 task 槽位。
        // `take_if_finished` 必须立刻返回 `None`，**不能**阻塞等它完成。
        // `get_or_init`：裸单测没有 TaskPoolPlugin，`get()` 会 panic；
        // 应用运行时 Bevy 已初始化，这里 init 是幂等的。
        let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let pending = pool.spawn(async move { std::future::pending::<Option<PathBuf>>().await });
        let mut t = OpenProjectTask {
            task: Some(pending),
        };
        // 此刻 future 永远 pending，is_finished 一定为 false。
        assert!(
            t.take_if_finished().is_none(),
            "未关闭的对话框不应让 UI 线程阻塞"
        );
    }
}
