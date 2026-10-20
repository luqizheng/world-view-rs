//! 界面外壳装配 + 唯一命令分发器。
//!
//! ## 职责
//! - 注册 [`crate::ui_theme`] / [`crate::ui_command_palette`] / [`crate::ui_layout`] 等 `crate::ui_*` 资产。
//! - 装配快捷键（设计规格 §6.1，19 条）与命令面板（§6.3）。
//! - 布局持久化（§10 Q5）：面板被拖动 / 折叠 / 显隐时写回 `ui-layout.yaml`。
//! - **`apply_pending_commands` 是命令**唯一**分发器**（见 [§9.3 验证项 8]）：
//!   从 `CommandPalette::pending` 取 1 条、枚举派发、各类命中的具体动作都在这里。
//! - `StartupLoadRequest` 排队、文件 / 目录对话框任务、面板显隐 Toggle 注册。
//!
//! ## 边界（不做什么）
//! - **不持有权威世界数据**——经 `WorldProjection.world`；改动必须经 `wv-edit`。
//! - **不画地形**——`crate::world_projection` / `crate::terrain_view` 负责。
//! - **不画单个面板**——所有 `crate::ui::panel_*` 在 `ui.rs`。
//! - **未来可能拆出的 `project_io.rs` / `drill_state.rs`**：`save_world_to_dir`（§A-4 内部动作）与
//!   `drill.rs` 的调用已集成在分发里；下次需要物理拆时这两处是迁移锚点。
//!
//! ## 主要导出
//! - `register(app: &mut App)` — Bevy App 装配入口。
//! - `apply_pending_commands` / `dispatch_project_command` — **唯一**命令分发器与子派发器。
//! - `startup_load` / `load_error_card` — 启动期工程加载与失败卡片；
//!   `adopt_world_snapshot` — 运行期换世界（加载 / 新建 / 关闭）的视图接管。
//! - `save_world_to_dir` / `poll_open_project_dialog` — 工程加载与落盘收口。
//! - `persist_layout_on_change` — 布局写盘系统。
//!
//! ## 职责分段（grep `// ── 段名 ──` 可快速跳转）
//!
//! 1. `register` 装配；2. DrillState 同步；3. hotkeys 快捷键 + 日夜主题投影；
//! 4. 文件 / 目录对话框任务；5. 启动期工程接管；6. 保存落盘；
//! 7. **唯一**命令分发器；8. 布局写盘。
//!
//! 文件末尾有 `mod tests`。
#![cfg(feature = "app")]

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy_egui::{egui, EguiContexts};

use crate::state::{
    DrillState, Panel, PanelState, PlaybackState, RecentProjects, ViewState, WorldProjection,
};
use crate::theme::palette::MapPalette;
use crate::theme::ui_theme::UiShellConfig;
use crate::theme::ui_tokens::UiTheme;
use crate::ui_command_palette::{CommandId, CommandPalette};
use crate::ui_layout::LayoutFile;
use wv_render::state::SelectionShape;
use wv_render::terrain_view::{TerrainTexture, TileAtlas};

use crate::i18n::{parse as parse_locale, CurrentLocale};

/// 把界面外壳的资源与系统注册进 `App`。
///
/// 调用顺序：在 `EguiPlugin` 之后、UI 面板系统之前。
pub fn register(app: &mut App) {
    // 应用设置对话框（顶栏 settings-2）：只存开关，关闭即无开销。
    // `WV_SETTINGS_OPEN=1` 启动即打开——截图验收探针，与 `WV_SHOT` 同族
    //（GUI 点击注入在本机拿不到前台焦点，视觉验收靠它）。
    let mut settings_dialog = crate::ui::SettingsDialog::default();
    if std::env::var_os("WV_SETTINGS_OPEN").is_some() {
        settings_dialog.open = true;
    }
    app.insert_resource(settings_dialog)
        .init_resource::<UiShellConfig>()
        .init_resource::<CommandPalette>()
        .init_resource::<crate::ui::ProjectMenuState>()
        .init_resource::<ShellNotice>()
        .init_resource::<DrillState>()
        .init_resource::<RecentProjects>()
        .init_resource::<LayoutFile>()
        // 地图工具条朝向 + 吸附（2026-10-19）：由 `ui-layout.yaml` 的
        // `map_dock` / `map_snap` 读写；`map_controls` 与分发器共享同一份状态。
        .init_resource::<crate::ui::map_controls::MapDockState>()
        // ADR-0018：UI 文案语言；默认 `Locale::Zh`。`load_layout_on_project_open`
        // 在切工程时回灌；`persist_layout_on_change` 在用户切换时落盘。
        .init_resource::<CurrentLocale>()
        .init_resource::<OpenProjectTask>()
        // 「新建工程」对话框状态（欢迎页 / 分发器 / 目录轮询三方共用）。
        .init_resource::<crate::ui::NewProjectDialog>()
        // CLI `-m` 的启动加载请求（`gui::run` 已注入值，这里只兜底默认）。
        .init_resource::<StartupLoadRequest>()
        // 根图落位必须早于所有读 `DrillState` 的 UI 系统（同一 `Update` 里靠顺序保证）。
        .add_systems(
            PreUpdate,
            (
                seed_drill_state,
                hotkeys.after(bevy_egui::EguiPreUpdateSet::InitContexts),
                // 主题投影必须晚于 hotkeys（Ctrl+J 同帧生效）、早于所有 UI 绘制。
                // 只在 `UiShellConfig` 变化时写 egui，避免每帧重建样式。
                apply_ui_theme.after(hotkeys),
                // 窗口位置重置：必须早于本帧面板绘制，否则旧位置会先画一遍。
                apply_panel_reset.after(apply_ui_theme),
                poll_open_project_dialog,
            ),
        )
        .add_systems(
            Update,
            // 世界侧派生状态：每帧先于 pass 里的 UI 刷新。
            (
                apply_pending_commands,
                sync_drill_mirror,
                // **必须先读后写**：两者都拿 `LayoutFile`，无序时 persist 可能抢先
                // 用「默认 Dark」把磁盘上的 `theme: light` 覆盖掉，随后 load 读到的
                // 就是它自己刚写坏的默认值（打开工程永远回不到用户上次的日夜模式）。
                (load_layout_on_project_open, persist_layout_on_change).chain(),
                sync_window_clear_color,
            ),
        )
        // bevy_egui 0.42：UI 系统进 pass 调度（见 gui.rs 同款注释）；
        // `startup_load` 虽不碰 egui，但必须跟遮罩同调度并排在其后，
        // 否则 Update 先阻塞加载、遮罩永远赶不上渲染。
        //
        // `welcome_screen`（无工程锁）排在遮罩之后、失败卡片之前：
        // 启动加载期间（req 有值）它自己会让位；失败卡片画在其后压在其上。
        .add_systems(
            bevy_egui::EguiPrimaryContextPass,
            (
                crate::ui::command_palette,
                startup_load_overlay,
                crate::ui::welcome_screen,
                load_error_card,
                startup_load,
            )
                .chain(),
        );

    // 公共控件预览浮窗 + 截图探针（WV_WIDGET_PREVIEW / WV_SHOT 门控，缺省零开销）。
    crate::ui::widget_preview::register(app);
}

// ── 段 2 · DrillState 同步 ─────────────────────────────────────────────────────

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

// ── 段 3 · hotkeys 快捷键 ──────────────────────────────────────────────────────

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
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let wants_text = ctx.egui_wants_keyboard_input();
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
    // 数字 1–3：切换浏览视图模式（BROWSING 现行三项：矢量地形 / 色块地形 /
    // 势力图，见 `ViewMode::BROWSING`；其余模式不占数字键）。
    for (i, key) in [
        bevy_egui::egui::Key::Num1,
        bevy_egui::egui::Key::Num2,
        bevy_egui::egui::Key::Num3,
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
    // F5：强制重画地形（2026-10-14 用户需求）。和 FitMap 的 F 不冲突。
    if input.key_pressed(bevy_egui::egui::Key::F5) {
        palette.pending = Some(CommandId::ForceRebuildTexture);
    }

    // ── 地图工具条字母键（2026-10-19 Figma `3:21848`） ──────────────────
    // 与瓷砖上的字母一一对应；**只在无修饰键时生效**——Ctrl+E 是导出，
    // 不能被测量橡皮抢走。互斥逻辑统一在 `CommandId::SelectMapTool`。
    if !cmd && !shift {
        use crate::ui_command_palette::MapToolId;
        let tool = [
            (
                bevy_egui::egui::Key::V,
                MapToolId::Select(SelectionShape::Polygon),
            ),
            (bevy_egui::egui::Key::H, MapToolId::Pan),
            (bevy_egui::egui::Key::B, MapToolId::Brush),
            // 橡皮贴着涂抹：两把刷子一个键位族。E 原属「测量橡皮」——那枚
            // 瓷砖已随 ADR-0043 退出工具条，E 顺位给地形橡皮刷（测量橡皮
            // 仍可从命令面板 Ctrl+K 进入）。
            (bevy_egui::egui::Key::E, MapToolId::BrushErase),
            (bevy_egui::egui::Key::L, MapToolId::MeasureLine),
            (bevy_egui::egui::Key::M, MapToolId::MeasureRect),
            (bevy_egui::egui::Key::P, MapToolId::Continent),
            // 单格编辑（[ADR-0040]）：C = Cell。与 Ctrl+C 复制不冲突
            // （本表只在无修饰键时生效，见上方 `!cmd && !shift` gate）。
            (bevy_egui::egui::Key::C, MapToolId::EditCell),
        ]
        .into_iter()
        .find(|(key, _)| input.key_pressed(*key));
        if let Some((_, tool)) = tool {
            palette.pending = Some(CommandId::SelectMapTool(tool));
        }
        // G：参照网格开关（ADR-0043 把网格瓷砖留在工具条末位，给它配同款
        // 字母键；此前该命令无快捷键）。
        if input.key_pressed(bevy_egui::egui::Key::G) {
            palette.pending = Some(CommandId::ToggleGrid);
        }
    }
}

/// 把 [`UiShellConfig`] 投影进 egui 上下文（日夜模式的**唯一**全局写入口）。
///
/// 只在配置变化时执行：`Res::is_changed()` 覆盖「资源首次加入」与「切换主题」
/// 两种情形。写入内容见 [`wv_ui::ui_theme::apply_ui_theme`]——`set_theme` 决定
/// egui 自带控件取哪套风格，`set_visuals_of` 把令牌投影上去。
///
/// 这里只改**界面外壳**；地图配色走 [`MapPalette`]（`ViewState::map_palette`），
/// 两条线互不影响（D2：世界色归世界、界面色归界面）。
fn apply_ui_theme(mut contexts: EguiContexts, cfg: Res<UiShellConfig>) {
    if !cfg.is_changed() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    wv_ui::ui_theme::apply_ui_theme(ctx, &cfg);
}

/// 消费「重置浮动窗口位置」信号（设置对话框 / 命令面板 `ResetPanelLayout`）。
///
/// 位置的真相有两份：egui 自己的窗口记忆（`Memory::area_rect`，拖动后再也不看
/// `default_pos`）和 `ui-layout.yaml` 的内存副本。只清一份不能复位——清磁盘
/// 不清 memory，窗口仍停在旧处；清 memory 不清磁盘，下一帧 `persist_layout_on_change`
/// 会把旧矩形写回来。因此这里两处同时抹掉：
/// - `reset_areas()`：所有 egui Area（含 12 个浮动面板）回到首次布局；
/// - `LayoutFile.panels.clear()`：`get` 回退 [`crate::ui_layout::default_layout`]。
///
/// 只清 `panels`，**不动** `theme` / `language` / `map_theme`——重置的是位置，
/// 不是用户偏好（用户裁定 2026-10-19：设置里的「重置浮动窗口位置」只对窗口几何）。
///
/// 放在 `PreUpdate`（`apply_ui_theme` 之后）：本帧面板绘制前生效，避免旧位置
/// 先闪一帧。
fn apply_panel_reset(
    mut contexts: EguiContexts,
    mut panels: ResMut<PanelState>,
    mut layouts: ResMut<LayoutFile>,
    projection: Res<WorldProjection>,
) {
    if !panels.reset_requested {
        return;
    }
    panels.reset_requested = false;
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    ctx.memory_mut(|m| m.reset_areas());
    layouts.panels.clear();
    // 磁盘也要复位：否则下次启动 `load_layout_on_project_open` 又把旧几何读回来。
    // 这里保留 `theme` / `density` / `map_theme` / `language`——重置的是窗口位置，
    // 不是用户偏好。
    if let Some(path) = crate::ui_layout::layout_path(projection.project_dir.as_deref()) {
        layouts.save(&path);
    }
}

/// 窗口清屏色跟随界面外壳底色（日夜模式切换时）。
///
/// 清屏色只在**地图外**可见（地图铺满时被完全盖住），属于应用外壳而非地图配色；
/// 亮色模式下若仍留着暗色底，窗口边缘会有一圈割裂的黑边。
fn sync_window_clear_color(cfg: Res<UiShellConfig>, mut clear: ResMut<ClearColor>) {
    if !cfg.is_changed() {
        return;
    }
    let bg = cfg.palette().bg_app;
    *clear = ClearColor(Color::srgb(bg.r, bg.g, bg.b));
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
    /// 记一条成功回执（`ui_shell` 之外的编辑链 / 对话框也用，故 pub）。
    pub fn info(&mut self, text: impl Into<String>, frame: u64) {
        self.text = Some(text.into());
        self.is_error = false;
        self.frame = frame;
    }

    /// 记一条失败回执（同上 pub：笔刷提交失败等路径要给人话说明）。
    pub fn error(&mut self, text: impl Into<String>, frame: u64) {
        self.text = Some(text.into());
        self.is_error = true;
        self.frame = frame;
    }

    /// 回执是否还在展示窗口内（约 6 秒 @60fps）。
    pub fn is_fresh(&self, now: u64) -> bool {
        self.text.is_some() && now.saturating_sub(self.frame) < 360
    }
}

// ── 段 4 · 文件 / 目录对话框任务 ──────────────────────────────────────────────

/// 「打开工程…」对话框的进行中任务（v0.2 §A）。
///
/// 入口（菜单 / `Ctrl+O` / 命令面板 / 无目录保存）只把「用户想弹目录对话框」的
/// 意图交给 [`Self::request`] 排队；真正的 `rfd::AsyncFileDialog` 由
/// [`poll_open_project_dialog`] 在下一帧 `PreUpdate` 发起——future 在
/// `AsyncComputeTaskPool` 上跑，不阻塞渲染。每帧的 [`poll_open_project_dialog`]
/// 拿这个资源判 `is_finished()`；用户关掉对话框那一刻 `Task` 完成，
/// `block_on` 拿到 `Option<PathBuf>`，再按 [`Self::purpose`] 分流处理。
///
/// ## 为什么单独一个资源（不塞进 `CommandPalette`）
///
/// `pending` 是「下一个要执行的命令」，生命周期一帧；这里是要「等用户回话」，
/// 跨帧存活，且同一时间只能有一个对话框在跑——独立的资源类型能让这两件事互不
/// 污染，也方便测试时直接 `let mut t = OpenProjectTask::default();`。
///
/// ## 为什么发起与排队分开
///
/// `spawn_for` 起的 future 一被 poll 就弹**原生**窗口——若由命令分发器直接发起，
/// 「无目录保存走对话框」这类路径在单测里一跑就弹真窗口。收口到轮询系统一处后，
/// 分发器保持纯状态，测试只需断言 [`Self::wanted`] 的意图（见 `mod tests`）。
#[derive(Resource, Default)]
pub struct OpenProjectTask {
    task: Option<Task<Option<PathBuf>>>,
    /// 这次对话框的**意图**：打开已有工程，还是给新工程挑保存位置。
    /// 轮询收口时按它分流——两个意图共用同一个「等用户回话」的载体，
    /// 但回话后的处理完全不同（加载 vs 创建+保存），不能混。
    purpose: DialogPurpose,
    /// 已排队、尚未发起的对话框意图。由 [`Self::request`] 写入，
    /// [`poll_open_project_dialog`] 取走并 [`Self::spawn_for`]。
    wanted: Option<DialogPurpose>,
}

/// 目录选择对话框的意图（v0.3 §B；2026-10-14 追加第三个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogPurpose {
    /// 打开已有工程：选完走 [`load_project_into`]。
    #[default]
    Open,
    /// 另存为 / 新工程首次保存：选完 `Project::create`（或 `open`）+ 全量保存。
    SaveAs,
    /// 「新建工程」对话框点「浏览…」：选完把路径写回
    /// [`crate::ui::NewProjectDialog::location`]（不加载、不保存，
    /// 只回填输入框——真正的创建在对话框「创建」确认时发生）。
    PickNewProjectLocation,
}

impl OpenProjectTask {
    /// 排队一个目录对话框意图（打开工程 / 选保存位置共用）。
    ///
    /// 重复排队会**覆盖**上一次未消费的意图——只保留最新的（与对话框自身的
    /// 「重开一次取消上一个」行为一致）。真正的原生窗口由
    /// [`poll_open_project_dialog`] 发起，本方法**不弹窗**。
    pub fn request(&mut self, purpose: DialogPurpose) {
        self.wanted = Some(purpose);
    }

    /// 取走排队的意图（仅 [`poll_open_project_dialog`] 使用）。
    fn take_request(&mut self) -> Option<DialogPurpose> {
        self.wanted.take()
    }

    /// 带意图起原生目录选择对话框（**唯一发起点**在 [`poll_open_project_dialog`]；
    /// `SaveAs` 的标题话术与打开不同，见 [`DialogPurpose`]）。
    ///
    /// 重复发起会**取消**上一次未完成的对话框
    /// ——只保留最新的意图（与 macOS Finder、Windows 资源管理器的行为一致）。
    fn spawn_for(&mut self, purpose: DialogPurpose) {
        let pool = AsyncComputeTaskPool::get();
        let title = match purpose {
            DialogPurpose::Open => "选择工程目录（含 world.yaml 的目录）",
            DialogPurpose::SaveAs => "选择保存位置（将在此目录创建工程）",
            DialogPurpose::PickNewProjectLocation => "选择项目位置（将在所选目录下创建项目子目录）",
        };
        let task = pool.spawn(async move {
            // `set_title` 让 Windows / macOS / Linux 三大平台都显示一致的话术；
            // rfd 在 WASM 上 `pick_folder` 不支持（见 crate 文档），但本工程
            // 不打 WASM，无需在该资源管理形式分支里设防。
            rfd::AsyncFileDialog::new()
                .set_title(title)
                .pick_folder()
                .await
                .map(|h| h.path().to_path_buf())
        });
        // 旧任务直接 `drop`：它会自动取消（`Task<T>::Drop` 文档），
        // 不会再回写到一个已被替换的 `task`，不会与新对话框争抢。
        self.task = Some(task);
        self.purpose = purpose;
    }

    /// 完成后的意图（仅当有任务时才有意义）。
    fn purpose(&self) -> DialogPurpose {
        self.purpose
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

// ── 段 5 · 启动期工程接管 ──────────────────────────────────────────────────────

/// CLI `-m <目录>` 带来的启动加载请求。
///
/// 需求（用户裁定）：启动**不主动加载**任何地图——`None` = 空编辑器起步；
/// `Some(dir)` = 启动后加载。由 [`startup_load`] 在遮罩画出并 present 之后
/// 消费：直接在 `Startup` 里做阻塞加载，遮罩来不及画，「白屏无反馈」
/// 只会从启动前挪到首帧前。
#[derive(Resource, Debug, Default)]
pub struct StartupLoadRequest(pub Option<String>);

/// 每帧看一眼「打开工程」对话框是否回来了（v0.2 §A）。
///
/// 也是目录对话框的**唯一发起点**：命令分发器（`Update`）只把意图排进
/// [`OpenProjectTask::wanted`]，本系统（`PreUpdate`）先发起、再轮询——写入
/// 发生在上一帧的 `Update`，天然读不到半截状态；`block_on` 只在用户关掉
/// 对话框的那一帧执行（见 [`OpenProjectTask::take_if_finished`]）。
#[allow(clippy::too_many_arguments)]
fn poll_open_project_dialog(
    mut task: ResMut<OpenProjectTask>,
    mut new_project_dialog: ResMut<crate::ui::NewProjectDialog>,
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut recents: ResMut<RecentProjects>,
    mut notice: ResMut<ShellNotice>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut playback: ResMut<PlaybackState>,
    mut view: ResMut<ViewState>,
    mut tex: ResMut<TerrainTexture>,
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    time: Res<Time>,
    current_loc: Res<crate::i18n::CurrentLocale>,
) {
    // 先发起上一帧排队的对话框意图（唯一发起点，见 `OpenProjectTask::request`）。
    if let Some(purpose) = task.take_request() {
        task.spawn_for(purpose);
    }
    let Some(outcome) = task.take_if_finished() else {
        return;
    };
    let frame = (time.elapsed_secs() * 60.0) as u64;
    // 用户选了目录并确认。目录用 `display()` 直接给人类读——
    // 这里不进 YAML，路径含空格 / 非 ASCII 时不要做 `as_ref` 路径切片，
    // 否则 Windows 的 `\\?\` 前缀会被吞。
    // 用户点了「取消」（`None`）：静默。不刷回执，免得状态栏常驻一条「已取消」。
    let Some(path) = outcome else {
        return;
    };
    match task.purpose() {
        DialogPurpose::Open => {
            let dir = path.to_string_lossy().into_owned();
            load_project_into(
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
                frame,
                &current_loc,
            );
        }
        DialogPurpose::SaveAs => {
            let dir = path.to_string_lossy().into_owned();
            save_world_to_dir(
                &dir,
                &mut projection,
                &mut edit,
                &mut notice,
                frame,
                &current_loc,
            );
        }
        DialogPurpose::PickNewProjectLocation => {
            // 只回填输入框；位置变了，框内残留的旧错误不再成立，一并清掉。
            new_project_dialog.location = path.to_string_lossy().into_owned();
            new_project_dialog.error = None;
        }
    }
}

/// 把当前工作副本落盘到指定目录（另存为 / 首次保存的共用收口）。
///
/// 目录里已有工程 → `Project::open`（往里写，不覆盖元数据）；
/// 目录是空的（或不存在）→ `Project::create` 先立 `world.yaml` 再全量保存。
/// 保存统一走空 dirty 表 = 全量路径——「另存为」必须把整个世界写过去，
/// 增量路由只对「已在跟踪 dirty 的同一目录」有意义。
fn save_world_to_dir(
    dir: &str,
    projection: &mut WorldProjection,
    edit: &mut crate::edit::editing::EditHistory,
    notice: &mut ShellNotice,
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
) {
    // 新工程的默认名：元数据里有就用，没有就「未命名工程」。
    let default_name = projection
        .world
        .meta
        .as_ref()
        .map(|m| m.name.clone())
        .unwrap_or_else(|| "未命名工程".to_owned());
    let open_or_create = || -> Result<wv_io::Project, String> {
        match wv_io::Project::open(dir) {
            Ok(p) => Ok(p),
            // NotAProject = 目录还没有 world.yaml → 视为「新位置」去创建；
            // 其他错误（权限等）如实上报。
            Err(e @ wv_io::IoError::NotAProject(_)) => {
                wv_io::Project::create(dir, &default_name).map_err(|e2| e2.to_string())?;
                let _ = e;
                wv_io::Project::open(dir).map_err(|e2| e2.to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    };
    match open_or_create() {
        Ok(mut project) => match project.save_world(&projection.world, &[]) {
            Ok(()) => {
                projection.project_dir = Some(dir.to_owned());
                projection.dirty = false;
                edit.clear_dirty();
                notice.info(
                    format!(
                        "{}{}",
                        crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeSavedToPrefix),
                        dir
                    ),
                    frame,
                );
            }
            Err(e) => notice.error(
                format!(
                    "{}{}",
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeSaveFailedPrefix),
                    e
                ),
                frame,
            ),
        },
        Err(e) => notice.error(
            format!(
                "{}{}",
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeSaveFailedPrefix),
                e
            ),
            frame,
        ),
    }
}

/// 从磁盘加载一个工程并替换当前工作副本（v0.2 §A「切换最近工程」）。
///
/// **运行期换世界唯一完整入口**：对话框打开（`poll_open_project_dialog`）、
/// 最近工程（`OpenRecentProject`）、启动加载（`startup_load`）都从这里过。
/// 成功后必须同时做四件事，缺一不可：
/// 1. 替换 [`WorldProjection::world`] 与 `project_dir`、清 `dirty`；
/// 2. [`DrillState::reset_to_world`]——**重置地图层级回世界**，否则新工程的
///    路径里还留着旧工程的 id（§A 明确要求）；
/// 3. 推进 [`RecentProjects`] 的顺序；
/// 4. [`adopt_world_snapshot`] 接管纹理 / 相机 / 播放——漏了它就是「加载完成
///    像没加载」：底图停在旧工程、相机纹丝不动（见该函数文档的回归记录）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_project_into(
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
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
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
            // 走 `derive_diagnostics`（错误+提示），不只 `derive_hints`。
            let theme = wv_render::theme::theme_pack::builtin::for_palette(view.map_palette);
            let d = crate::edit::diagnostics_bridge::derive_diagnostics(&projection.world, theme);
            diagnostics.last_count = d.items.len();
            diagnostics.diagnostics = d;
            diagnostics.needs_diagnose = false;
            notice.info(
                format!(
                    "{}{}{}",
                    crate::i18n::t_key(
                        current_loc,
                        crate::i18n::I18nKey::NoticeProjectLoadedPrefix
                    ),
                    name,
                    crate::i18n::t_key(
                        current_loc,
                        crate::i18n::I18nKey::NoticeProjectLoadedSuffix
                    )
                ),
                frame,
            );
            adopt_world_snapshot(
                projection, drill, playback, view, tex, atlas, images, cameras,
            );
        }
        Err(e) => {
            projection.load_error = Some(format!("打开工程 `{dir}` 失败: {e}"));
            notice.error(
                format!(
                    "{}{}",
                    crate::i18n::t_key(
                        current_loc,
                        crate::i18n::I18nKey::NoticeOpenProjectFailedPrefix
                    ),
                    e
                ),
                frame,
            );
        }
    }
}

/// 启动加载遮罩：[`StartupLoadRequest`] 未消费期间全屏压暗 + 居中「正在加载」。
///
/// 用 [`EguiContexts::try_ctx_mut`] 而非 `ctx_mut`：主窗口销毁后同一帧
/// 仍可能轮到本系统（bevy_egui 0.32 关窗路径的已知 panic），此时直接跳过，
/// 不给关窗级联添新的触发点。
fn startup_load_overlay(mut ctxs: EguiContexts, req: Res<StartupLoadRequest>) {
    let Some(dir) = req.0.as_deref() else {
        return;
    };
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let screen = ctx.viewport_rect();
    egui::Area::new(egui::Id::new("wv_startup_load_dim"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(150));
        });
    egui::Window::new("正在加载工程…")
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
                    ui.label(dir);
                },
            );
        });
}

/// 加载失败卡片：居中显示失败原因与下一步指引，点「知道了」关闭。
///
/// 需求（用户裁定）：加载失败**必须在窗口内显示原因**——控制台日志一闪
/// 而过，用户只看到「没画出来」，无从知道是路径错了还是 YAML 坏了。
/// 对话框打开工程失败（[`load_project_into`] 的失败分支）同样落到这里。
fn load_error_card(mut ctxs: EguiContexts, mut projection: ResMut<WorldProjection>) {
    let Some(err) = projection.load_error.clone() else {
        return;
    };
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    egui::Window::new("工程加载失败")
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&err).color(egui::Color32::LIGHT_RED));
            ui.add_space(8.0);
            ui.label(
                "换一个工程：顶栏工程菜单 → 打开工程…（Ctrl+O），或命令行 `wv gui -m <工程目录>`。",
            );
            ui.add_space(8.0);
            if ui.button("知道了").clicked() {
                projection.load_error = None;
            }
        });
}

/// 执行启动加载（`-m`）；帧 2 后才开始，给遮罩留出「画 + present + 窗口揭示」。
///
/// 为什么不在 `Startup` 里做：Bevy 要等首帧渲染完成才 present——在 Startup
/// 阻塞加载，用户看到的仍是白屏。帧 0 画遮罩 → present（窗口隐藏，见
/// `app_bootstrap` 的隐藏起步）；帧 1 末窗口揭示、遮罩已在屏上；帧 2 这时
/// 才做真正（阻塞）的磁盘加载；帧 3 起显示加载结果 / 失败卡片。
/// 空编辑器起步（无 `-m`）时顺带给一条一次性状态栏指引。
///
/// 参数已 16+1（基本类同 `apply_pending_commands`），按 Bevy 单系统 16 上限
/// 把可成对的 Resources 折成元组：`time` + `current_loc` 一组，省一个位置。
#[allow(clippy::too_many_arguments)]
fn startup_load(
    mut req: ResMut<StartupLoadRequest>,
    (mut frames, mut hinted): (Local<u32>, Local<bool>),
    mut projection: ResMut<WorldProjection>,
    mut drill: ResMut<DrillState>,
    mut recents: ResMut<RecentProjects>,
    mut notice: ResMut<ShellNotice>,
    mut diagnostics: ResMut<crate::state::ProjectDiagnostics>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    (mut playback, mut view): (ResMut<PlaybackState>, ResMut<ViewState>),
    mut tex: ResMut<TerrainTexture>,
    atlas: Option<Res<TileAtlas>>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    (time, current_loc): (Res<Time>, Res<crate::i18n::CurrentLocale>),
) {
    *frames += 1;
    if *frames < 3 {
        return; // 帧 0/1：只让遮罩画；帧 1 末窗口才揭示（隐藏起步），加载必须排在其后。
    }
    let frame = (time.elapsed_secs() * 60.0) as u64;
    let Some(dir) = req.0.take() else {
        if !*hinted {
            *hinted = true;
            // 2026-10-14 起无工程时欢迎页全屏接管，指引跟着改口。
            notice.info(
                crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeNoProjectNeed),
                frame,
            );
        }
        return;
    };
    *hinted = true;
    load_project_into(
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
        frame,
        &current_loc,
    );
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
fn adopt_world_snapshot(
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
        view.mode,
        atlas,
        view.terrain_soften,
        crate::theme::palette::colors(view.map_palette),
        wv_render::theme::theme_pack::builtin::for_palette(view.map_palette).treatment,
        0.0,
    );
    wv_render::terrain_view::replace_terrain_texture(
        tex,
        images,
        image,
        &data,
        map,
        view.mode,
        view.terrain_soften,
        view.map_palette,
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

/// 分发工程 / 导航类命令（v0.2 §A §B），返回是否已消费该命令。
///
/// 段：唯一命令分发器 · project 子派发。
///
/// 保存那条不是偷懒：数据落盘要处理「每实体一文件 + 引用完整性 + 二进制分块」
/// 三件事（AGENTS §5/§4.5），不是一次 `serde_yaml::to_string` 能收尾的。
///
/// 能力边界（如实回执，不假装成功）：
///
/// - 打开工程… → ✅ 原生目录选择对话框（`rfd`），加载走 `wv_io::Project::open`
/// - 打开最近工程 → ✅ 真加载 + 视图接管（`load_project_into` 内含 adopt）
/// - 新建工程 → ✅ 弹「新建工程」对话框（名称 + 位置 + 路径预览）；确认后在
///   `位置\名称` 落盘（含首张世界地图）并打开（2026-10-14 改版，取代旧的
///   「内存空白工程 + 首次保存再选目录」链路）
/// - 关闭工程 → ✅ 清空工作副本并归位视图（底图 / 相机 / 播放）
/// - 另存为… → ✅ 目录对话框 → `Project::create`/`open` + 全量保存
/// - 导出… → ⛔ 导出格式未定（见 v0.2 §G）
/// - 项目设置 → ⛔ 面板未实现（v0.3）
/// - 保存 → ✅ 无目录时排队「选保存位置」对话框；有目录走
///   `wv_io::Project::save_world` 全量写，dirty 路由 v0.2
#[allow(clippy::too_many_arguments)]
fn dispatch_project_command(
    id: &CommandId,
    projection: &mut WorldProjection,
    new_project_dialog: &mut crate::ui::NewProjectDialog,
    drill: &mut DrillState,
    recents: &mut RecentProjects,
    notice: &mut ShellNotice,
    open_task: &mut OpenProjectTask,
    diagnostics: &mut crate::state::ProjectDiagnostics,
    edit: &mut crate::edit::editing::EditHistory,
    playback: &mut PlaybackState,
    view: &mut ViewState,
    tex: &mut TerrainTexture,
    atlas: Option<&TileAtlas>,
    images: &mut Assets<Image>,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    frame: u64,
    current_loc: &crate::i18n::CurrentLocale,
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
                        playback,
                        view,
                        tex,
                        atlas,
                        images,
                        cameras,
                        frame,
                        current_loc,
                    );
                }
                None => notice.error(
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeRecentsChanged),
                    frame,
                ),
            }
            true
        }
        CommandId::NewProject => {
            // 2026-10-14 改版（用户需求：参考 VS 的新建项目对话框）：不再默默
            // 造一个内存空白世界——弹对话框收「名称 + 位置」，确认后在
            // `位置\名称` 落盘（含首张世界地图）并打开。创建逻辑与落盘细节
            // 全在 `ui::new_project_dialog`；这里只负责把框打开。
            new_project_dialog.open();
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
            // 关闭 = 换成空世界：底图 / 相机 / 播放必须跟着归位，否则旧工程的
            // 地图永远挂在屏上（回归 2026-10-14「关了像没关」，见 adopt 文档）。
            adopt_world_snapshot(
                projection, drill, playback, view, tex, atlas, images, cameras,
            );
            notice.info(
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeProjectClosed),
                frame,
            );
            true
        }
        CommandId::OpenProject => {
            // 排队「打开工程」对话框意图（不阻塞）；下一帧由
            // `poll_open_project_dialog` 发起、收口，复用 `OpenRecentProject`
            // 的同一条 `load_project_into` 加载路径——结果就只是「多了一条最近工程」。
            open_task.request(DialogPurpose::Open);
            true
        }
        CommandId::SaveProjectAs => {
            // v0.3 §B：排队「选保存位置」意图——下一帧发起，选完由
            // `poll_open_project_dialog` 的 SaveAs 分支收口（create-or-open + 全量保存）。
            open_task.request(DialogPurpose::SaveAs);
            true
        }
        CommandId::ExportProject => {
            notice.info(
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeExportPending),
                frame,
            );
            true
        }
        CommandId::ProjectSettings => {
            notice.info(
                crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeSettingsPendingV03),
                frame,
            );
            true
        }
        CommandId::DrillUp => {
            if !drill.up() {
                notice.info(
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeAlreadyWorldMap),
                    frame,
                );
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
                notice.info(
                    crate::i18n::t_key(current_loc, crate::i18n::I18nKey::NoticeAlreadyAtLevel),
                    frame,
                );
            }
            true
        }
        _ => false,
    }
}

// ── 段 7 · 唯一命令分发器 · 主循环 ──────────────────────────────────────────

/// 执行命令面板 / 快捷键 / 菜单排队的命令。
///
/// **唯一分发器**：快捷键与命令面板写 [`CommandPalette::pending`]；顶栏工程菜单
/// 的绘制函数只拿得到菜单自身状态，命令先落 [`crate::ui::ProjectMenuState::pending`]。
/// 两条队列在这里汇合（菜单优先，每帧最多消费一条）统一执行——拆成多个系统各自
/// `take()` 会在「谁先拿到」上产生时序耦合。菜单的写入发生在
/// `EguiPrimaryContextPass`（晚于本 `Update`），所以菜单命令在下一帧这里被执行，
/// 一帧延迟与快捷键「PreUpdate 写 / 本帧 Update 收」同量级，用户不可感。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply_pending_commands(
    // Bevy 单个函数系统的参数上限是 16 个；汇入菜单队列、换世界接管资源后
    // 到这里共 19 个，超限报「not a valid system」。按职责折成嵌套元组——
    // `SystemParam` 对元组递归实现，每层元组算 1 个参数；解构保留原名，函数体不受影响。
    (mut palette, mut menu, mut panels): (
        ResMut<CommandPalette>,
        ResMut<crate::ui::ProjectMenuState>,
        ResMut<PanelState>,
    ),
    (mut cfg, mut view, mut playback): (
        ResMut<UiShellConfig>,
        ResMut<ViewState>,
        ResMut<crate::state::PlaybackState>,
    ),
    (mut brush, mut measure, mut streets, mut projection, mut drill): (
        ResMut<crate::state::BrushState>,
        ResMut<crate::state::MeasureState>,
        ResMut<wv_render::streets::StreetEditorState>,
        ResMut<WorldProjection>,
        ResMut<DrillState>,
    ),
    (mut recents, mut notice, mut edit): (
        ResMut<RecentProjects>,
        ResMut<ShellNotice>,
        ResMut<crate::edit::editing::EditHistory>,
    ),
    (mut tex, mut open_task, mut diagnostics): (
        ResMut<wv_render::terrain_view::TerrainTexture>,
        ResMut<OpenProjectTask>,
        ResMut<crate::state::ProjectDiagnostics>,
    ),
    // 「新建地图」对话框的开关（CommandId::NewMap → open_for；真正的创建
    // 在对话框自身的系统里提交，见 `ui::new_map_dialog`）。
    // 「新建工程」对话框的开关同源（CommandId::NewProject / 欢迎页 → open）。
    // 两个 ResMut 折成一层元组：单系统参数上限 16，拆元组后每层元组算 1 个。
    (mut new_map_dialog, mut new_project_dialog, mut continent_dialog): (
        ResMut<crate::ui::NewMapDialog>,
        ResMut<crate::ui::NewProjectDialog>,
        ResMut<crate::ui::ContinentDialog>,
    ),
    // 换世界接管（adopt_world_snapshot）所需：瓦片图集可缺席（纯色平涂回退）。
    (atlas, mut images): (
        Option<Res<wv_render::terrain_view::TileAtlas>>,
        ResMut<Assets<Image>>,
    ),
    // 选择工具会话态：命令面板「生成大陆…」顺手启用选择工具。
    mut selection: ResMut<crate::state::SelectionState>,
    // 单格编辑工具会话态（[ADR-0040]）：跟 selection 同款粒度。
    mut edit_cell: ResMut<crate::state::EditCellState>,
    current_loc: Res<CurrentLocale>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    time: Res<Time>,
) {
    // 菜单队列优先（egui 一帧最多产生一条），命令面板其次。
    let id = menu.pending.take().or_else(|| palette.pending.take());
    let Some(id) = id else {
        return;
    };
    // 帧号：回执过期判断只依赖它，不引入计时器资源。
    let frame = (time.elapsed_secs() * 60.0) as u64;

    // 无工程锁（2026-10-14 用户需求）：没有打开任何工程时，一切「改数据 /
    // 需要工程上下文」的命令一律挡下并指出下一步——欢迎页是唯一入口。
    // NewProject / OpenProject / CloseProject（本身就是入口）与视图 / 面板
    // 命令不在其列；笔刷与街道的输入侧另有同口径门禁（见 collect_brush_stroke）。
    if !projection.has_open_project()
        && matches!(
            id,
            CommandId::NewMap
                | CommandId::Save
                | CommandId::SaveProjectAs
                | CommandId::ExportProject
                | CommandId::GenerateContinent
        )
    {
        notice.error(
            crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeNeedProjectFirst),
            frame,
        );
        return;
    }

    // 工程 / 导航类先试（它们改的是世界与路径，与下面的视图状态互不重叠）。
    if dispatch_project_command(
        &id,
        &mut projection,
        &mut new_project_dialog,
        &mut drill,
        &mut recents,
        &mut notice,
        &mut open_task,
        &mut diagnostics,
        &mut edit,
        &mut playback,
        &mut view,
        &mut tex,
        atlas.as_deref(),
        &mut images,
        &mut cameras,
        frame,
        &current_loc,
    ) {
        return;
    }

    match id {
        CommandId::SetViewMode(i) => {
            // 索引对 `BROWSING` 而不是 `ALL`：命令面板 / 快捷键只开放两种浏览模式
            // （2026-10-14 用户裁定），索引语义必须与列表来源一致。
            if let Some(m) = crate::state::ViewMode::BROWSING.get(i as usize) {
                view.mode = *m;
            }
        }
        CommandId::OpenCommandPalette => palette.open(),
        // 新建地图：只负责把对话框打开（默认值按世界现状预填）；创建动作
        // 在对话框自身的系统里经 `AddMapCommand` 提交（见 `ui::new_map_dialog`）。
        CommandId::NewMap => new_map_dialog.open_for(&projection.world),
        CommandId::ToggleTheme => cfg.theme = cfg.theme.toggled(),
        CommandId::ToggleDensity => cfg.density = cfg.density.toggled(),
        CommandId::ToggleMapPalette => view.map_palette = view.map_palette.toggled(),
        CommandId::ToggleImmersive => panels.immersive = !panels.immersive,
        CommandId::TogglePanel(p) => panels.toggle(p),
        CommandId::ResetPanelLayout => panels.reset_requested = true,
        CommandId::FitMap => {
            // FitMap 的语义 = 与启动/加载后相同的取景（`adopt_loaded_world`
            // 同款 fit），而不是字面 ZERO/1.0——旧实现只写 ViewState，
            // 相机从不跟随，语义从未真正生效过。
            let (center, scale) = crate::app_bootstrap::fit_view_for_world(&projection.world);
            view.camera_center = center;
            view.scale = scale;
            wv_render::camera::apply_view_to_camera(center, scale, &mut cameras);
        }
        CommandId::Zoom(d) => {
            // 命令没有指针上下文（锚点固定为视口中心），但**缩放上下限必须按当前级别取**：
            // 固定 `0.1..8.0` 是单级时代的产物，分级后城市/街道级的区间完全不同
            // （ADR-0009 §8 决策①）。窗口尺寸只影响区间宽度，取不到时按默认窗口。
            let window_size = view.window_size;
            let band = wv_render::zoom_level::level_zoom(
                &projection.world,
                drill.current(),
                window_size,
                Vec2::splat(wv_render::zoom_level::FALLBACK_MAP_SIZE),
            );
            view.scale = band.clamp(view.scale * if d > 0 { 1.25 } else { 0.8 });
            // 缩放锚点 = 视口中心（命令无指针上下文，保持中心不动）。
            wv_render::camera::apply_view_to_camera(view.camera_center, view.scale, &mut cameras);
        }
        CommandId::TogglePlay => playback.playing = !playback.playing,
        CommandId::JumpToStart => playback.current_day = playback.range_from,
        CommandId::JumpToEnd => playback.current_day = playback.range_to,
        CommandId::BrushRadius(d) => {
            brush.radius = (brush.radius + if d > 0 { 8.0 } else { -8.0 }).clamp(8.0, 600.0);
        }
        CommandId::SelectBrushTool(i) => {
            // 越界索引静默忽略（与 SetViewMode 同口径）；选中即激活笔刷——
            // 「选工具 = 准备画」，与面板段控件 / 瓷砖同一语义。同步
            // `tool_group` 让段控件高亮跟随。
            if let Some(t) = crate::state::BrushTool::ALL.get(i as usize) {
                brush.tool = *t;
                brush.on_tool_changed();
                brush.enabled = true;
            }
        }
        CommandId::ToggleGrid => view.grid_lines = !view.grid_lines,
        // 地图工具条单选工具（2026-10-19）：**互斥的唯一实现**。
        // 工具条瓷砖与 V/H/B/E/L/M/P 快捷键都排到这里，先关掉所有工具再开目标，
        // 保证「左键」永远只有一个消费者（相机平移 / 选区 / 笔刷 / 测量 / 街道）。
        CommandId::SelectMapTool(tool) => {
            use crate::state::MeasureMode;
            use crate::ui_command_palette::MapToolId;

            // 目标工具当前是否已启用（再次点击 = 关闭）。
            //
            // `Select(shape)` 的口径是「同形状开着 = 关；不同形状开着 = 换形状」
            // ——三个形状是同一把选择工具的三种笔尖，V 快捷键在多边形 / 矩形 /
            // 椭圆之间走一圈时不应出现「先关再开把选区弄丢」的顿挫。
            let already_on = match tool {
                MapToolId::Select(shape) => selection.enabled && selection.shape == shape,
                MapToolId::Pan => view.pan_tool,
                MapToolId::Brush => brush.enabled && brush.tool != crate::state::BrushTool::Eraser,
                MapToolId::BrushErase => {
                    brush.enabled && brush.tool == crate::state::BrushTool::Eraser
                }
                MapToolId::MeasureLine => measure.mode == MeasureMode::Line,
                MapToolId::MeasureRect => measure.mode == MeasureMode::Rect,
                MapToolId::MeasureEraser => measure.mode == MeasureMode::Eraser,
                MapToolId::Continent => false,
                MapToolId::EditCell => edit_cell.selected.is_some(),
            };

            if matches!(tool, MapToolId::Continent) {
                // 「生成大陆」是开门动作，不是互斥工具：沿用旧语义——只开面板 +
                // 顺手启用选择工具（圈出意图包络）。
                continent_dialog.open = true;
                selection.enabled = true;
            } else {
                // ① 先全关：互斥。切走时清掉草稿/半边选区，避免「看不见的手」。
                let had_selection = selection.enabled;
                let had_measure = measure.mode != MeasureMode::Off;
                selection.enabled = false;
                view.pan_tool = false;
                brush.enabled = false;
                streets.enabled = false;
                measure.mode = MeasureMode::Off;
                if had_selection {
                    selection.cancel();
                }
                if had_measure {
                    measure.clear_draft();
                }
                // 关掉 EditCell 时清空状态(避免「切走工具后旧选择留着」)。
                edit_cell.clear();

                // ② 再按需打开（已启用 → 保持全关，即切换关闭）。
                if !already_on {
                    match tool {
                        MapToolId::Select(shape) => {
                            selection.shape = shape;
                            selection.enabled = true;
                        }
                        MapToolId::Pan => view.pan_tool = true,
                        MapToolId::Brush => brush.enabled = true,
                        // 橡皮与涂抹共用 `BrushState`，只差 `tool` 指到哪把；
                        // `on_tool_changed` 同步 tool_group（面板段控件双真相防漂）。
                        MapToolId::BrushErase => {
                            brush.tool = crate::state::BrushTool::Eraser;
                            brush.on_tool_changed();
                            brush.enabled = true;
                        }
                        MapToolId::MeasureLine => {
                            measure.clear_draft();
                            measure.mode = MeasureMode::Line;
                        }
                        MapToolId::MeasureRect => {
                            measure.clear_draft();
                            measure.mode = MeasureMode::Rect;
                        }
                        MapToolId::MeasureEraser => {
                            measure.clear_draft();
                            measure.mode = MeasureMode::Eraser;
                        }
                        MapToolId::Continent => unreachable!("Continent 已在上方分支处理"),
                        // EditCell 工具激活:`selected` 仍为 None,要等用户点击一格
                        // 才出 Inspector 字段 + PassId::CellSelection 高亮。
                        // (这里**不**自动开 selection.enabled——那是 Circle Select 工具)
                        MapToolId::EditCell => {
                            // 标记 EditCell 工具激活;点击事件自己写到 `edit_cell.selected`
                            // (见 editing::commit_edit_cell_on_click)。
                            edit_cell.tool_active = true;
                        }
                    }
                }
            }
        }
        // 大陆生成属性面板（面板本身只收参数；生成在 continent_dialog_system）。
        CommandId::GenerateContinent => {
            continent_dialog.open = true;
            // 顺手启用选择工具，作者圈完选区回面板确认（spec D1/D5；
            // 与测量的互斥由 map_controls 的瓷砖在启用路径上处理）。
            selection.enabled = true;
        }
        // 单格编辑：消费 Inspector 写下的 `EditCellState.pending`，
        // 通过 wv-edit 的 History 落盘（snapshot-diff-revert 四步模式，
        // 每格 1 command 直接进撤销栈，[ADR-0040] §决策 8）。
        //
        // [wv_edit::EditCellCommand::new] 之前要先把 `pending.before` 取
        // 出后**重新读** world 中当前 cell 值 — 避免 Inspector 读到的
        // before 与 `EditCellCommand.apply` 校验时的 before 不一致
        // （中途可能笔刷重绘了一把 cell）。
        CommandId::CommitEditCell => {
            let Some(pending) = edit_cell.pending.take() else {
                // 没 pending 直接忽略：UI race / 重复点提交按钮。
                return;
            };
            // 校验：当前 world 的 cell 真值必须 = pending.before，否则作废。
            // 与 EditCellCommand.apply 的前置校验同口径。
            let current_matches = projection
                .world
                .terrain_of(&pending.map)
                .and_then(|t| {
                    let w = t.width as i32;
                    let h = t.height as i32;
                    if pending.gx < 0 || pending.gy < 0 || pending.gx >= w || pending.gy >= h {
                        return None;
                    }
                    t.cells
                        .get((pending.gy as usize) * (w as usize) + (pending.gx as usize))
                })
                .map(|c| *c == pending.before)
                .unwrap_or(false);
            if !current_matches {
                notice.error(
                    format!(
                        "提交被拒：cell ({}, {}) 当前值已不是你 Inspector 上看到的值;\
                         取消改动以避免叠加冲突",
                        pending.gx, pending.gy
                    ),
                    (time.elapsed_secs() * 60.0) as u64,
                );
                return;
            }
            let cmd = wv_edit::command::EditCellCommand::new(
                pending.map.clone(),
                pending.gx,
                pending.gy,
                pending.before,
                pending.after,
            );
            let mut tx = wv_edit::history::Transaction::new();
            tx.push(Box::new(cmd));
            match edit.history.commit(&mut projection.world, tx) {
                Ok(dirty) => {
                    edit.record_dirty(dirty);
                    projection.dirty = true;
                    // 脏矩形 = 单格（增量 patch）
                    let gx = pending.gx as u32;
                    let gy = pending.gy as u32;
                    tex.mark_dirty([gx, gy, gx + 1, gy + 1]);
                    diagnostics.needs_diagnose = true;
                    notice.info(
                        format!("已改 cell ({}, {})", pending.gx, pending.gy),
                        (time.elapsed_secs() * 60.0) as u64,
                    );
                }
                Err(e) => {
                    notice.error(
                        format!("改 cell 失败：{e}"),
                        (time.elapsed_secs() * 60.0) as u64,
                    );
                }
            }
        }
        // 撤销 / 重做走 `editing` 的公开入口（同一份逻辑，不复制）。
        CommandId::Undo | CommandId::Redo => {
            let redo = matches!(id, CommandId::Redo);
            match crate::edit::editing::apply_history_command(
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
        // 重置地形到底图（2026-10-16 用户需求）。
        CommandId::ResetTerrainToBase => {
            let Some(map_id) = drill.current().cloned() else {
                notice.error(
                    crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeResetTerrainNoMap),
                    frame,
                );
                return;
            };
            match crate::edit::editing::apply_reset_terrain_to_base(
                &map_id,
                &mut edit,
                &mut projection,
                &mut tex,
                &playback,
            ) {
                Ok(msg) => {
                    // 重置后地形 cells 改了 → 派生诊断（事件位移 / 相遇）需要重算。
                    diagnostics.needs_diagnose = true;
                    notice.info(msg, frame);
                }
                Err(msg) => notice.error(msg, frame),
            }
        }
        // 重新生成世界地形（ADR-0037 D4）：新种子重跑冷北模板，单事务可撤销。
        // 备份文件在提交前落盘——重生成是破坏性的，必须给作者退路。
        CommandId::RegenWorld => {
            let Some(map_id) = drill.current().cloned() else {
                notice.error(
                    crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeRegenNoMap),
                    frame,
                );
                return;
            };
            let (w, h) = projection
                .world
                .terrain_of(&map_id)
                .map(|t| (t.width.max(2), t.height.max(2)))
                .unwrap_or((512, 288));
            let seed = crate::ui::new_project_dialog::seed_from_clock();
            let generated = wv_terrain::climate::generate_cold_north(w, h, seed);
            let terrain = wv_core::terrain::TerrainData {
                width: generated.width,
                height: generated.height,
                cells: generated
                    .cells
                    .into_iter()
                    .map(|c| wv_core::terrain::TerrainCellData {
                        height: c.height,
                        surface: c.surface,
                        biome: c.biome,
                    })
                    .collect(),
            };
            let template = wv_core::WorldTemplate {
                kind: wv_core::WorldTemplateKind::ColdNorth,
                seed,
                rivers_seed: seed ^ 0x9E37_79B9_7F4A_7C15,
            };
            if let Some(dir) = projection.project_dir.clone() {
                backup_terrain_before_regen(&dir, &map_id, &projection.world);
            }
            match crate::edit::editing::apply_regen_world(
                &map_id,
                terrain,
                template,
                &mut edit,
                &mut projection,
                &mut tex,
                &playback,
            ) {
                Ok(msg) => {
                    diagnostics.needs_diagnose = true;
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
                // 新建工程还没目录：不再报死错，而是排队「选保存位置」对话框
                // 意图（v0.3 §B，下一帧由 `poll_open_project_dialog` 发起）。
                // 这补上了「新建 → 首次保存」的断链。
                open_task.request(DialogPurpose::SaveAs);
                return;
            };
            let dirty = edit.take_dirty();
            let n = dirty.len();
            match wv_io::Project::open(&dir) {
                Ok(mut project) => match project.save_world(&projection.world, &dirty) {
                    Ok(()) => {
                        projection.dirty = false;
                        let scope = if n == 0 {
                            crate::i18n::t_key(
                                &current_loc,
                                crate::i18n::I18nKey::NoticeSavedScopeFull,
                            )
                            .to_owned()
                        } else {
                            format!(
                                "{}{}{}",
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopePartialCountPrefix
                                ),
                                n,
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopePartialCountSuffix
                                )
                            )
                        };
                        notice.info(
                            format!(
                                "{}{}{}{}{}",
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedToPrefix
                                ),
                                dir,
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopePrefix
                                ),
                                scope,
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSavedScopeSuffix
                                )
                            ),
                            frame,
                        );
                    }
                    Err(e) => {
                        // 失败时把 dirty 放回，让下次保存能再试。
                        edit.record_dirty(dirty);
                        notice.error(
                            format!(
                                "{}{}",
                                crate::i18n::t_key(
                                    &current_loc,
                                    crate::i18n::I18nKey::NoticeSaveFailedPrefix
                                ),
                                e
                            ),
                            frame,
                        );
                    }
                },
                Err(e) => notice.error(
                    format!(
                        "{}{}",
                        crate::i18n::t_key(
                            &current_loc,
                            crate::i18n::I18nKey::NoticeOpenProjectDirFailedPrefix
                        ),
                        e
                    ),
                    frame,
                ),
            }
        }
        // 强制重画地形纹理（2026-10-14 用户需求：怀疑数据已改、画面没跟上时点 ↻）。
        // 不动世界数据；只清 `last_mode/last_map` 让下一帧必然全图重建。
        CommandId::ForceRebuildTexture => {
            wv_render::terrain_view::force_rebuild(&mut tex);
            notice.info(
                crate::i18n::t_key(&current_loc, crate::i18n::I18nKey::NoticeForceRedrawn),
                frame,
            );
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

// ── 段 8 · 布局写盘 ────────────────────────────────────────────────────────────

/// 布局持久化：把面板的**真实几何**回写 `ui-layout.yaml`（Q5）。
///
/// 位置的真相在 egui 的窗口记忆里（`Memory::area_rect`），不在 [`crate::ui_layout::default_layout`]。
/// 拖动过程中不写盘（`is_using_pointer`），松手后的那一帧才落盘——避免 60 次/秒的文件写。
/// 打开工程时读回 `ui-layout.yaml` 里**当前已落盘的 layout 偏好**。
///
/// 当前覆盖：
/// - `theme`（2026-10-18：界面日夜模式，`dark` / `light`）
/// - `map_theme`（ADR-0016：日间 / 暮色）
/// - `language`（ADR-0018：zh / en，注入 `CurrentLocale`）
/// - `map_dock` / `map_snap`（2026-10-19：地图工具条朝向与吸附开关）
///
/// 界面 theme 与地图 map_theme 是两条独立的线（D2）：前者只影响外壳与 egui
/// 系统控件，后者只影响地图配色。不可识别值静默保留当前值（V1，不阻塞加载）。
///
/// **追加说明（2026-10-18）**：此前本系统刻意不读回软件壳 `theme` / `density`，
/// 理由是「避免修一个字段扩成激活整套壳配置」。日夜模式落地后，`theme` 必须随
/// 工程恢复（否则按钮切完下次打开又回到默认）；`density` 仍不读回，保持原状，
/// 待单独裁定。
///
/// 面板几何由 egui 自带的窗口记忆按 panel id 接管，不属于本函数职责。
fn load_layout_on_project_open(
    project: Res<WorldProjection>,
    mut layout: ResMut<LayoutFile>,
    mut view: ResMut<ViewState>,
    mut shell_cfg: ResMut<UiShellConfig>,
    mut current_loc: ResMut<CurrentLocale>,
    mut dock: ResMut<crate::ui::map_controls::MapDockState>,
    mut loaded_dir: Local<Option<String>>,
) {
    let Some(dir) = project.project_dir.clone() else {
        return;
    };
    if loaded_dir.as_ref() == Some(&dir) {
        return;
    }
    let Some(path) = crate::ui_layout::layout_path(Some(&dir)) else {
        return;
    };
    let file = LayoutFile::load(&path);
    if let Some(theme) = file.theme.as_deref().and_then(UiTheme::from_key) {
        shell_cfg.theme = theme;
    }
    if let Some(t) = file.map_theme.as_deref().and_then(MapPalette::from_key) {
        view.map_palette = t;
    }
    if let Some(lang) = file.language.as_deref() {
        if let Some(loc) = parse_locale(lang) {
            // V1：language 缺字段 / 不可解析 → 保留 CurrentLocale 当前值
            // （project 没换就不该重置；project 第一次打开保持默认 Zh）。
            current_loc.0 = loc;
        }
    }
    // 地图工具条偏好（2026-10-19）：缺字段 / 不可识别 → 保留当前值（V1）。
    if let Some(orient) = file
        .map_dock
        .as_deref()
        .and_then(crate::ui::map_controls::MapDockOrientation::from_key)
    {
        dock.orientation = orient;
    }
    if let Some(snap) = file.map_snap {
        dock.snapping = snap;
    }
    if let Some([x, y]) = file.map_dock_pos {
        dock.pos = Some(crate::ui::map_controls::MapDockPos { x, y });
    }
    if let Some([x, y]) = file.map_zoom_strip_pos {
        dock.zoom_strip_pos = Some(crate::ui::map_controls::MapDockPos { x, y });
    }
    *layout = file;
    *loaded_dir = Some(dir);
}

#[allow(clippy::too_many_arguments)]
fn persist_layout_on_change(
    mut contexts: EguiContexts,
    panels: Res<PanelState>,
    cfg: Res<UiShellConfig>,
    mut layout: ResMut<LayoutFile>,
    project: Res<WorldProjection>,
    view: Res<ViewState>,
    current_loc: Res<CurrentLocale>,
    dock: Res<crate::ui::map_controls::MapDockState>,
) {
    let Some(path) = crate::ui_layout::layout_path(project.project_dir.as_deref()) else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut file = (*layout).clone();
    file.theme = Some(cfg.theme.key().into());
    file.density = Some(cfg.density.key().into());
    file.map_theme = Some(view.map_palette.key().to_string());
    file.language = Some(current_loc.0.as_str().to_string());
    file.map_dock = Some(dock.orientation.key().to_string());
    file.map_snap = Some(dock.snapping);
    file.map_dock_pos = dock.pos.map(|p| [p.x, p.y]);
    file.map_zoom_strip_pos = dock.zoom_strip_pos.map(|p| [p.x, p.y]);

    let mut changed = false;
    // 语言切换本身就让 layout 跟着变；与面板几何检测并入同一个 `changed` 累加器。
    if file.language != layout.language
        || file.theme != layout.theme
        || file.density != layout.density
        || file.map_theme != layout.map_theme
        || file.map_dock != layout.map_dock
        || file.map_snap != layout.map_snap
        || file.map_dock_pos != layout.map_dock_pos
        || file.map_zoom_strip_pos != layout.map_zoom_strip_pos
    {
        changed = true;
    }
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
    if ctx.egui_is_using_pointer() {
        return;
    }
    file.save(&path);
    *layout = file;
}

/// 重生成前备份磁盘上的 `terrain.bin`：`<file>.bak.<unix_ts>`，最多保留 5 份。
///
/// 备份失败不阻止重生成（V1：诊断可提示，但不把作者挡在门外）；文件不存在
/// （新建工程尚未保存）时静默跳过。
fn backup_terrain_before_regen(project_dir: &str, map_id: &wv_core::Id, world: &wv_core::World) {
    let rel = world
        .maps
        .iter()
        .find(|m| &m.id == map_id)
        .and_then(|m| m.raster.as_ref())
        .map(|r| r.path.clone())
        .unwrap_or_else(|| format!("maps/{map_id}/terrain.bin"));
    let path = std::path::Path::new(project_dir).join(rel);
    if !path.is_file() {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bak = path.with_extension(format!("bin.bak.{ts}"));
    if std::fs::copy(&path, &bak).is_ok() {
        prune_terrain_backups(&path, 5);
    }
}

/// 只保留最近 `keep` 份 `terrain.bin.bak.<ts>`（按文件名时间戳排序）。
fn prune_terrain_backups(path: &std::path::Path, keep: usize) {
    let Some(dir) = path.parent() else {
        return;
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let prefix = format!("{name}.bak.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut backups: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with(&prefix))
        })
        .collect();
    backups.sort();
    if backups.len() > keep {
        for old in &backups[..backups.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
    }
}

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
            purpose: DialogPurpose::default(),
            wanted: None,
        };
        // 此刻 future 永远 pending，is_finished 一定为 false。
        assert!(
            t.take_if_finished().is_none(),
            "未关闭的对话框不应让 UI 线程阻塞"
        );
    }

    /// 给分发器测试搭一个最小 `World`：与真实装配同名的资源 + 一条待执行命令
    /// （落在 `CommandPalette::pending`，即快捷键 / 命令面板的入队口）。
    ///
    /// `AsyncComputeTaskPool::get_or_init`：分发器走对话框路径时会 `get()`，
    /// 裸单测没有 TaskPoolPlugin 未初始化会 panic；应用运行时 Bevy 已初始化，
    /// 这里 init 是幂等的。
    #[allow(dead_code)] // helper used by tests; warnings emitted in non-test lib build
    fn dispatcher_world(project_dir: Option<&str>, dirty: bool, pending: CommandId) -> World {
        AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let mut world = World::new();
        world.init_resource::<CommandPalette>();
        world.init_resource::<crate::ui::ProjectMenuState>();
        world.init_resource::<PanelState>();
        world.init_resource::<UiShellConfig>();
        world.init_resource::<ViewState>();
        world.init_resource::<crate::state::PlaybackState>();
        world.init_resource::<crate::state::BrushState>();
        world.init_resource::<DrillState>();
        world.init_resource::<RecentProjects>();
        world.init_resource::<ShellNotice>();
        world.init_resource::<crate::edit::editing::EditHistory>();
        world.init_resource::<OpenProjectTask>();
        world.init_resource::<crate::state::ProjectDiagnostics>();
        world.init_resource::<crate::ui::NewMapDialog>();
        world.init_resource::<crate::ui::NewProjectDialog>();
        world.init_resource::<crate::state::SelectionState>();
        // 2026-10-19 地图工具条：分发器互斥工具需要测量 / 街道会话态。
        world.init_resource::<crate::state::MeasureState>();
        world.init_resource::<wv_render::streets::StreetEditorState>();
        // 2026-10-19 单格编辑工具（[ADR-0040]）:分发器需要 EditCellState 才能
        // 运行 `apply_pending_commands` 的 SelectMapTool(EditCell) 分支。
        world.init_resource::<crate::state::EditCellState>();
        world.init_resource::<crate::ui::ContinentDialog>();
        // ADR-0018：UI 文案语言；默认 `Locale::Zh`（与 GUI 启动默认值一致）。
        world.init_resource::<crate::i18n::CurrentLocale>();
        world.init_resource::<LayoutFile>();
        use bevy::asset::Assets;
        use bevy::image::Image;
        world.init_resource::<Assets<Image>>();
        world.insert_resource(WorldProjection {
            world: wv_core::World::default(),
            project_dir: project_dir.map(str::to_owned),
            dirty,
            load_error: None,
            selection: None,
        });
        world.insert_resource(TerrainTexture {
            handle: Default::default(),
            size: 1,
            height: 1,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
            last_map: None,
        });
        world.insert_resource(Time::<()>::default());
        world.resource_mut::<CommandPalette>().pending = Some(pending);
        world
    }

    /// 无工程锁（2026-10-14）：没有打开任何工程时，「保存」不再排队「选保存
    /// 位置」对话框（那是旧的「内存空白工程」链路），而是挡下并给出下一步
    /// 指引——欢迎页是唯一入口。有工程在手时才走原有的 dirty 保存路径。
    #[test]
    fn save_without_project_shows_guidance_instead_of_a_dialog() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(None, true, CommandId::Save);
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        let task = world.resource::<OpenProjectTask>();
        assert!(
            task.wanted.is_none() && task.task.is_none(),
            "无工程时不得发起 / 排队任何目录对话框"
        );
        let notice = world.resource::<ShellNotice>();
        assert!(
            notice.is_error
                && notice
                    .text
                    .as_deref()
                    .unwrap_or("")
                    .contains("新建或打开工程"),
            "应给出「先新建或打开工程」的指引，实得 {:?}",
            notice.text
        );
    }

    #[test]
    fn open_project_command_queues_open_dialog() {
        // 「打开工程…」同样只排队意图：发起收口在轮询系统（见
        // `OpenProjectTask::request` 的文档）。
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(None, false, CommandId::OpenProject);
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        let task = world.resource::<OpenProjectTask>();
        assert_eq!(
            task.wanted,
            Some(DialogPurpose::Open),
            "打开工程应排队「打开」对话框意图"
        );
        assert!(task.task.is_none(), "分发器不得直接发起 rfd 对话框");
    }

    #[test]
    fn select_brush_tool_command_picks_tool_and_activates_brush() {
        // 命令面板 / 未来快捷键选工具的唯一路径：索引对 `BrushTool::ALL`，
        // 选中即激活笔刷（与面板瓷砖点击同语义）。同时 sync `tool_group`
        // 让段控件高亮跟随。越界索引不 panic。
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(1));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        let brush = world.resource::<crate::state::BrushState>();
        assert_eq!(
            brush.tool,
            crate::state::BrushTool::MountainRange,
            "索引 1 应选中山脉"
        );
        assert_eq!(
            brush.tool_group,
            crate::state::BrushToolGroup::Shape,
            "选 MountainRange 后段应切到塑"
        );
        assert!(brush.enabled, "选工具 = 激活笔刷");

        // 选 Place 组工具（Volcano=索引 4）→ 段切到放。
        let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(4));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        let brush = world.resource::<crate::state::BrushState>();
        assert_eq!(brush.tool_group, crate::state::BrushToolGroup::Place);

        // 越界索引：忽略而不改状态（不 panic 也不误切）。
        let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(99));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        let brush = world.resource::<crate::state::BrushState>();
        assert_eq!(
            brush.tool,
            crate::state::BrushTool::PaintKind,
            "越界不改工具"
        );
        assert_eq!(
            brush.tool_group,
            crate::state::BrushToolGroup::Paint,
            "越界不改组"
        );
        assert!(!brush.enabled, "越界不激活笔刷");
    }

    /// 地图工具条互斥（2026-10-19）：开抓手 = 关笔刷 / 选区 / 测量 / 街道；
    /// 再点一次同一工具 = 关闭。左键永远只有一个消费者。
    #[test]
    fn map_tool_selection_is_mutually_exclusive_and_toggles_off() {
        use crate::ui_command_palette::MapToolId;
        use bevy::ecs::system::RunSystemOnce;

        // 先激活笔刷（与工具条 B 同语义的既有命令）。
        let mut world = dispatcher_world(None, false, CommandId::SelectBrushTool(0));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert!(world.resource::<crate::state::BrushState>().enabled);

        // 点 H：开抓手，笔刷让位。
        world.resource_mut::<CommandPalette>().pending =
            Some(CommandId::SelectMapTool(MapToolId::Pan));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert!(world.resource::<ViewState>().pan_tool, "H 应开启抓手工具");
        assert!(
            !world.resource::<crate::state::BrushState>().enabled,
            "开抓手应关掉笔刷（互斥）"
        );
        assert!(
            !world.resource::<crate::state::SelectionState>().enabled,
            "开抓手应关掉选区"
        );

        // 再点 H：关闭抓手。
        world.resource_mut::<CommandPalette>().pending =
            Some(CommandId::SelectMapTool(MapToolId::Pan));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert!(
            !world.resource::<ViewState>().pan_tool,
            "再次点击同一工具应关闭"
        );

        // 点 L：折线测量，且选区仍关闭。
        world.resource_mut::<CommandPalette>().pending =
            Some(CommandId::SelectMapTool(MapToolId::MeasureLine));
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert_eq!(
            world.resource::<crate::state::MeasureState>().mode,
            crate::state::MeasureMode::Line,
            "L 应进入折线测量"
        );
        assert!(!world.resource::<crate::state::SelectionState>().enabled);
    }

    #[test]
    fn toggle_grid_command_flips_the_view_flag() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(None, false, CommandId::ToggleGrid);
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert!(
            world.resource::<ViewState>().grid_lines,
            "默认关 → 执行一次应为开"
        );
    }

    #[test]
    fn project_menu_pending_command_reaches_the_dispatcher() {
        // 回归 1：工程菜单的点击 / 回车写的是 `ProjectMenuState::pending`，而唯一
        // 分发器曾只消费 `CommandPalette::pending`——菜单里的「打开工程 / 新建 /
        // 关闭」全部悬空（点击无反应）。
        // 回归 2：换世界后必须走 `adopt_world_snapshot` 接管纹理 / 相机 / 播放，
        // 否则「关闭像没点」——旧底图原封不动挂在屏上。
        // 本测试用最小 World 跑真实分发器：菜单队列里的 CloseProject 必须被执行
        //（世界被清空 + 状态栏回执 + 纹理整体重建标记）。
        //（2026-10-14 起用 CloseProject 验证 adopt：NewProject 改为只弹对话框、
        // 不再直接换世界，adopt 的菜单级回归覆盖转移到 CloseProject 上。）
        use crate::ui::ProjectMenuState;
        use bevy::asset::Assets;
        use bevy::ecs::system::RunSystemOnce;
        use bevy::image::Image;
        use wv_render::terrain_view::TerrainTexture;

        let mut world = World::new();
        world.init_resource::<CommandPalette>();
        world.init_resource::<ProjectMenuState>();
        world.init_resource::<PanelState>();
        world.init_resource::<UiShellConfig>();
        world.init_resource::<ViewState>();
        world.init_resource::<crate::state::PlaybackState>();
        world.init_resource::<crate::state::BrushState>();
        world.init_resource::<DrillState>();
        world.init_resource::<RecentProjects>();
        world.init_resource::<ShellNotice>();
        world.init_resource::<crate::edit::editing::EditHistory>();
        world.init_resource::<OpenProjectTask>();
        world.init_resource::<crate::state::ProjectDiagnostics>();
        world.init_resource::<crate::ui::NewMapDialog>();
        world.init_resource::<crate::ui::NewProjectDialog>();
        world.init_resource::<crate::state::SelectionState>();
        // 2026-10-19 地图工具条：分发器互斥工具需要测量 / 街道会话态。
        world.init_resource::<crate::state::MeasureState>();
        world.init_resource::<wv_render::streets::StreetEditorState>();
        // 2026-10-19 单格编辑工具（[ADR-0040]）:分发器需要 EditCellState 才能
        // 运行 `apply_pending_commands` 的 SelectMapTool(EditCell) 分支。
        world.init_resource::<crate::state::EditCellState>();
        world.init_resource::<crate::ui::ContinentDialog>();
        // ADR-0018：UI 文案语言；默认 `Locale::Zh`（与 GUI 启动默认值一致）。
        world.init_resource::<crate::i18n::CurrentLocale>();
        world.init_resource::<LayoutFile>();
        world.init_resource::<Assets<Image>>();
        world.insert_resource(WorldProjection {
            world: wv_core::World::default(),
            project_dir: Some("D:/some/project".into()),
            dirty: true,
            load_error: None,
            selection: None,
        });
        world.insert_resource(TerrainTexture {
            handle: Default::default(),
            size: 1,
            height: 1,
            res: 1,
            dirty: None,
            chunks_dirty: None,
            last_mode: None,
            last_soften: false,
            last_palette: Default::default(),
            last_map: None,
        });
        world.insert_resource(Time::<()>::default());
        world.resource_mut::<ProjectMenuState>().pending = Some(CommandId::CloseProject);

        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");

        let projection = world.resource::<WorldProjection>();
        assert!(
            projection.project_dir.is_none() && !projection.dirty,
            "菜单 pending 的 CloseProject 未被执行（菜单命令悬空回归）"
        );
        assert!(
            world.resource::<ShellNotice>().text.is_some(),
            "命令执行后状态栏应有回执"
        );
        let tex = world.resource::<TerrainTexture>();
        assert!(
            tex.dirty.is_some(),
            "CloseProject 后纹理未整体重建（adopt_world_snapshot 未被触发：地图停在旧世界）"
        );
        // 初始手工构造的纹理 `last_mode` 为 None；adopt 内的
        // `replace_terrain_texture` 必然写入 `Some(mode)`。
        assert!(
            tex.last_mode.is_some(),
            "adopt 未按新世界重建纹理（last_mode 仍为 None）"
        );
    }

    /// 2026-10-14 改版：`CommandId::NewProject` 只负责弹「新建工程」对话框，
    /// 不再默默清世界 / 标脏（创建与落盘在对话框确认后发生）。无工程锁下的
    /// 门禁不拦它——它就是入口之一。
    #[test]
    fn new_project_command_opens_the_dialog_not_a_blank_world() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(None, false, CommandId::NewProject);
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert!(
            world.resource::<crate::ui::NewProjectDialog>().open,
            "NewProject 应打开「新建工程」对话框"
        );
        let projection = world.resource::<WorldProjection>();
        assert!(
            !projection.dirty && projection.project_dir.is_none(),
            "对话框未确认前不得改工作副本（世界替换 / 落盘都在确认后）"
        );
    }

    /// 无工程锁：`CommandId::NewMap`（同样改数据）在无工程时必须被挡下，
    /// 对话框不打开、世界不动、回执给指引。
    #[test]
    fn new_map_is_blocked_until_a_project_is_open() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(None, false, CommandId::NewMap);
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");
        assert!(
            !world.resource::<crate::ui::NewMapDialog>().open,
            "无工程时不得打开「新建地图」对话框"
        );
        let notice = world.resource::<ShellNotice>();
        assert!(
            notice.is_error
                && notice
                    .text
                    .as_deref()
                    .unwrap_or("")
                    .contains("新建或打开工程"),
            "应给出指引，实得 {:?}",
            notice.text
        );
    }

    /// 2026-10-14 用户反馈「新建工程后笔刷涂大地图没变化」的根本原因（旧版
    /// NewProject 直接换世界）：换世界后 `view.current_map` 没同步到 drill 镜像
    /// ——笔触按陈旧 id 提交命令，落到不存在的图上。
    ///
    /// NewProject 改版后 adopt 的换世界路径由 CloseProject 代表验证：预置一个
    /// 陈旧的 `view.current_map`，断言 adopt 路径把它清回 drill 的镜像；且若
    /// drill.current() 指向某张图，那张图必须真实存在。
    #[test]
    fn adopt_world_syncs_view_current_map_to_drill_current() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = dispatcher_world(Some("D:/old/project"), true, CommandId::CloseProject);
        // 预置陈旧镜像：关闭工程后世界为空，正确的同步结果是把它清空。
        world.resource_mut::<ViewState>().current_map = "stale_map".into();
        world
            .run_system_once(apply_pending_commands)
            .expect("系统运行失败");

        let drill = world.resource::<DrillState>();
        let view = world.resource::<ViewState>();
        // adopt 路径必须把 view.current_map 与 drill.current 对齐。
        assert_eq!(
            view.current_map,
            drill
                .current()
                .map(|i| i.as_str().to_owned())
                .unwrap_or_default(),
            "adopt 路径必须把 view.current_map 同步到 drill.current()，\
             否则笔触会按陈旧 id 提交"
        );
        // 若 drill.current 指向某 id，那张图必须存在。
        if let Some(id) = drill.current() {
            let exists = world
                .resource::<WorldProjection>()
                .world
                .maps
                .iter()
                .any(|m| &m.id == id);
            assert!(exists, "drill.current() 指向的图应真实存在：{id:?}");
        }
    }
}
