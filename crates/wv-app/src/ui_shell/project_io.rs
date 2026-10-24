//! 工程落盘与目录对话框：排队意图、发起原生目录选择、保存 / 另存为，
//! 以及重生成地形前对磁盘 `terrain.bin` 的备份。
//!
//! **不在这里加载工程**：加载派发与结果应用在 [`super::project_load`]。

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task};

use crate::project_loader::PendingProjectLoad;
use crate::state::WorldProjection;

use super::notice::ShellNotice;

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
    pub(crate) task: Option<Task<Option<PathBuf>>>,
    /// 这次对话框的**意图**：打开已有工程，还是给新工程挑保存位置。
    /// 轮询收口时按它分流——两个意图共用同一个「等用户回话」的载体，
    /// 但回话后的处理完全不同（加载 vs 创建+保存），不能混。
    pub(crate) purpose: DialogPurpose,
    /// 已排队、尚未发起的对话框意图。由 [`Self::request`] 写入，
    /// [`poll_open_project_dialog`] 取走并 [`Self::spawn_for`]。
    pub(crate) wanted: Option<DialogPurpose>,
}

/// 目录选择对话框的意图（v0.3 §B；2026-10-14 追加第三个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogPurpose {
    /// 打开已有工程：选完**排队**（[`PendingProjectLoad`]），加载由
    /// `drive_project_load` 在后台线程派发。
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
    pub(crate) fn take_if_finished(&mut self) -> Option<Option<PathBuf>> {
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
/// 也是目录对话框的**唯一发起点**：命令分发器（`Update`）只把意图排进
/// [`OpenProjectTask::wanted`]，本系统（`PreUpdate`）先发起、再轮询——写入
/// 发生在上一帧的 `Update`，天然读不到半截状态；`block_on` 只在用户关掉
/// 对话框的那一帧执行（见 [`OpenProjectTask::take_if_finished`]）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn poll_open_project_dialog(
    mut task: ResMut<OpenProjectTask>,
    mut new_project_dialog: ResMut<crate::ui::NewProjectDialog>,
    mut pending: ResMut<PendingProjectLoad>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut notice: ResMut<ShellNotice>,
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
            // 只**排队**，不直接加载：加载由 `drive_project_load` 在后台线程
            // 派发（2026-10-22 C 档）——这里同步加载会冻住整个窗口。
            pending.0 = Some(path.to_string_lossy().into_owned());
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

/// 重生成前备份磁盘上的 `terrain.bin`：`<file>.bak.<unix_ts>`，最多保留 5 份。
///
/// 备份失败不阻止重生成（V1：诊断可提示，但不把作者挡在门外）；文件不存在
/// （新建工程尚未保存）时静默跳过。
pub(crate) fn backup_terrain_before_regen(
    project_dir: &str,
    map_id: &wv_core::Id,
    world: &wv_core::World,
) {
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
