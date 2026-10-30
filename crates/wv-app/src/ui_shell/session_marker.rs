//! 崩溃恢复（UX-001 完整版的最后一块）：会话标记 + 异常退出告知。
//!
//! ## 为什么「崩溃恢复」落成标记而不是第二份数据副本
//!
//! 60 秒静默自动保存（[`super::dispatch::auto_save`]）是**原地增量落盘**——
//! 崩溃后重新打开工程，磁盘上就是最后一次自动保存的内容，不存在需要
//! 「合并 / 选择」的第二份副本。真正缺的是**告知**：作者不知道上次是不是
//! 正常退出，也不知道在崩掉前最后那段编辑还在不在。本模块补这一半。
//!
//! ## 机制
//!
//! 打开工程时在工程根写 `<工程>/.wv-session.yaml`（只含会话元数据，**不含任何
//! 世界数据**；P3：YAML 为主格式，人类可读可删），此后只在「脏 → 干净」
//! （落盘成功）、「干净 → 脏」（有新编辑）两种翻转时更新标记。正常关闭工程 /
//! 退出应用时删除。启动时若标记还在 = 上次没走正常退出路径（任务管理器杀进程 /
//! 断电 / panic），载入完成后弹一张恢复卡片，按标记里的 `dirty` 分两种话术：
//!
//! - `dirty: true` → 磁盘内容停在最后一次自动保存，此后最多
//!   [`AUTO_SAVE_INTERVAL_SECS`] 秒的编辑可能丢失；
//! - `dirty: false` → 没有未落盘的编辑，内容完整。
//!
//! ## 已知盲区（写下来而不是假装没有）
//!
//! 1. **同一工程被两个实例同时打开**：后开的实例会把前者的标记当成「上次异常
//!    退出」并覆盖。单机单实例是常态，且文案用「上次没有正常关闭」而非断言崩溃，
//!    误报只是一张可关的卡片。
//! 2. **进程被强杀在写标记那一瞬间**：标记可能半截（YAML 解析失败）。这种残片
//!    本身也算「没正常退出」的证据，按「可能丢 ≤60s」处理（宁可多说一句）。
//! 3. 标记写不进去（只读盘 / 权限）时**静默降级**：不打扰作者（V1——提示不该变成
//!    阻力），只是本次会话失去崩溃告知能力。

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::i18n::{t, CurrentLocale, I18nKey};
use crate::state::WorldProjection;
use crate::theme::ui_theme::UiShellConfig;
use crate::ui::shell::UiShell;
use crate::ui::widgets::DialogWindow;

use super::dispatch::AUTO_SAVE_INTERVAL_SECS;

/// 会话标记文件名（工程根下）。
const MARKER_FILE: &str = ".wv-session.yaml";
/// 标记归属标识：同名文件里没有它 = 不是我们写的，绝不删。
const MARKER_APP: &str = "world-view";
/// 标记格式版本（未来改结构时递增，旧版本标记按「无法确认」处理）。
const MARKER_VERSION: u32 = 1;

/// 会话标记文件的内容（P3：YAML，可读可删，无世界数据）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SessionFile {
    /// 归属标识，见 [`MARKER_APP`]。
    #[serde(default)]
    app: String,
    /// 格式版本，见 [`MARKER_VERSION`]。
    #[serde(default)]
    version: u32,
    /// 本会话唯一标识（`<pid>-<启动 unix 秒>`）：只有写下它的实例才删得掉。
    #[serde(default)]
    token: String,
    /// 会话开始时刻（unix 秒）。
    #[serde(default)]
    started_at: u64,
    /// 最后一次确认落盘的时刻（unix 秒）。
    #[serde(default)]
    last_saved_at: u64,
    /// 写标记时是否有未落盘的编辑。
    #[serde(default)]
    dirty: bool,
}

/// 读标记的结果：存在且可解析 / 存在但半截（写一半被打断）。
#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkerRead {
    Parsed(SessionFile),
    /// 文件在、但解析不了——同样是「没正常退出」的证据。
    Corrupt,
}

/// 会话标记状态（ui_shell 资源）。
#[derive(Resource, Debug, Default)]
pub struct SessionMarker {
    /// 当前标记落在哪个工程目录（`None` = 无工程 = 无标记）。
    dir: Option<String>,
    /// 本会话 token（见 [`SessionFile::token`]）。
    token: String,
    /// 会话开始时刻（unix 秒）。
    started_at: u64,
    /// 最后一次确认落盘的时刻（unix 秒）。
    last_saved_at: u64,
    /// 上一帧的 `dirty`：检测「脏 → 干净」= 一次落盘成功。
    dirty_prev: bool,
}

impl SessionMarker {
    /// 当前状态对应的标记文件内容。
    fn file(&self, dirty: bool) -> SessionFile {
        SessionFile {
            app: MARKER_APP.to_owned(),
            version: MARKER_VERSION,
            token: self.token.clone(),
            started_at: self.started_at,
            last_saved_at: self.last_saved_at,
            dirty,
        }
    }
}

/// 一次「上次没正常退出」的报告（恢复卡片的数据源）。
#[derive(Resource, Debug, Default)]
pub struct CrashRecovery {
    /// 有值 = 恢复卡片在屏上等待「知道了」。
    pub report: Option<CrashReport>,
}

/// 恢复卡片的三段信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    /// 工程显示名（`WorldMeta.name`，缺省退回目录名）。
    pub project_name: String,
    /// 崩溃时是否有未落盘的编辑：`None` = 标记半截、无法确认（按有风险处理）。
    pub had_unsaved_edits: Option<bool>,
}

fn marker_path(dir: &str) -> PathBuf {
    PathBuf::from(dir).join(MARKER_FILE)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_token(now: u64) -> String {
    format!("{}-{now}", std::process::id())
}

/// 读工程根的会话标记：`None` = 没有（或同名文件不是我们写的）。
fn read_marker(dir: &str) -> Option<MarkerRead> {
    let text = std::fs::read_to_string(marker_path(dir)).ok()?;
    Some(match serde_yaml::from_str::<SessionFile>(&text) {
        // 同名但不是我们的文件：当作不存在，也绝不删它。
        Ok(file) if file.app != MARKER_APP => return None,
        Ok(file) => MarkerRead::Parsed(file),
        Err(_) => MarkerRead::Corrupt,
    })
}

/// 写标记（失败静默：标记是附赠的告知，不是数据本身）。
fn write_marker(dir: &str, file: &SessionFile) {
    if let Ok(text) = serde_yaml::to_string(file) {
        let _ = std::fs::write(marker_path(dir), text);
    }
}

/// 只有写下标记的实例才删得掉（多实例时的最低限度互不误伤）。
fn remove_marker_if_owner(dir: &str, token: &str) {
    let path = marker_path(dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let ours = serde_yaml::from_str::<SessionFile>(&text)
        .map(|f| f.app == MARKER_APP && f.token == token)
        .unwrap_or(false);
    if ours {
        let _ = std::fs::remove_file(&path);
    }
}

/// 工程显示名：`WorldMeta.name` 优先，缺省用目录名（已是路径就取最后一段）。
fn project_name(projection: &WorldProjection, dir: &str) -> String {
    if let Some(name) = projection.world.meta.as_ref().map(|m| m.name.clone()) {
        if !name.trim().is_empty() {
            return name;
        }
    }
    std::path::Path::new(dir)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(dir)
        .to_owned()
}

/// 把一次「标记还在」翻译成报告。
fn report_for(projection: &WorldProjection, dir: &str, read: MarkerRead) -> CrashReport {
    let project_name = project_name(projection, dir);
    match read {
        MarkerRead::Parsed(file) => CrashReport {
            project_name,
            had_unsaved_edits: Some(file.dirty),
        },
        // 半截标记：不知道当时脏不脏，按「有风险」说（宁可多说一句）。
        MarkerRead::Corrupt => CrashReport {
            project_name,
            had_unsaved_edits: None,
        },
    }
}

/// 会话标记的生命周期（Update）：换工程 / 关工程时收旧写新，落盘翻转时心跳。
///
/// 顺序要求：必须在 `apply_pending_commands` 之后（关闭工程会把
/// `project_dir` 清空、另存为会换目录，同帧就要把标记搬过去）。
pub fn maintain_session_marker(
    mut marker: ResMut<SessionMarker>,
    mut recovery: ResMut<CrashRecovery>,
    projection: Res<WorldProjection>,
) {
    let now = unix_now();
    let current = projection.project_dir.as_deref();
    if current != marker.dir.as_deref() {
        // ── 换工程（含关闭工程 → None）──
        // 旧工程的恢复卡片不该挂到新工程 / 欢迎页上。
        recovery.report = None;
        if let Some(old) = marker.dir.take() {
            // 正常换工程 = 本会话与旧工程和平分手，标记该收。
            remove_marker_if_owner(&old, &marker.token);
        }
        let Some(dir) = current else {
            marker.token.clear();
            marker.dirty_prev = projection.dirty;
            return;
        };
        // 写自己的标记之前先看旧标记：还在 = 上次没正常退出。
        if let Some(read) = read_marker(dir) {
            recovery.report = Some(report_for(&projection, dir, read));
        }
        marker.token = new_token(now);
        marker.started_at = now;
        marker.last_saved_at = now;
        marker.dir = Some(dir.to_owned());
        marker.dirty_prev = projection.dirty;
        write_marker(dir, &marker.file(projection.dirty));
        return;
    }
    // ── 同一工程：只在脏状态翻转时写标记 ──
    let Some(dir) = marker.dir.clone() else {
        return;
    };
    if marker.dirty_prev == projection.dirty {
        return;
    }
    if !projection.dirty {
        // 脏 → 干净：一次落盘成功（手动 / 自动 / 另存为都走这里）。
        marker.last_saved_at = now;
    }
    marker.dirty_prev = projection.dirty;
    write_marker(&dir, &marker.file(projection.dirty));
}

/// 退出应用时收标记（Last）：正常退出 = 下次开工程不该看到恢复卡片。
///
/// 排在 `ExitSystems` 之后：既能看到 UI 在 `Update` / egui pass 写的
/// `AppExit`，也能看到 `exit_on_all_closed` 这类系统在同帧写的那条。
pub fn end_session_on_exit(mut marker: ResMut<SessionMarker>, exits: MessageReader<AppExit>) {
    if exits.is_empty() {
        return;
    }
    if let Some(dir) = marker.dir.take() {
        remove_marker_if_owner(&dir, &marker.token);
    }
}

/// 恢复卡片（egui pass）：载入完成后告知「上次没正常退出」。
pub fn crash_recovery_card(
    mut ctxs: bevy_egui::EguiContexts,
    shell_cfg: Res<UiShellConfig>,
    mut recovery: ResMut<CrashRecovery>,
    current_loc: Res<CurrentLocale>,
) {
    let Some(report) = recovery.report.clone() else {
        return;
    };
    let Ok(ctx) = ctxs.ctx_mut() else {
        return;
    };
    let shell = UiShell::new(&shell_cfg);
    let loc = current_loc.0;
    let mut open = true;
    let mut dismissed = false;
    DialogWindow::new(t(loc, I18nKey::CrashRecoveryTitle), &mut open)
        .width(420.0)
        .show(ctx, &shell, |ui| {
            ui.label(body_text(loc, &report));
            ui.add_space(8.0);
            if ui.button(t(loc, I18nKey::LoadErrorDismiss)).clicked() {
                dismissed = true;
            }
        });
    if !open {
        // × / Esc 关框 = 知道了。
        dismissed = true;
    }
    if dismissed {
        recovery.report = None;
    }
}

/// 卡片正文：按「有没有未落盘编辑」分两种话术。
fn body_text(loc: crate::i18n::Locale, report: &CrashReport) -> String {
    let head = format!(
        "{}{}",
        t(loc, I18nKey::CrashRecoveryNamePrefix),
        report.project_name
    );
    match report.had_unsaved_edits {
        Some(false) => format!("{head}{}", t(loc, I18nKey::CrashRecoveryCleanBody)),
        Some(true) | None => format!(
            "{head}{}{}{}",
            t(loc, I18nKey::CrashRecoveryDirtyPrefix),
            AUTO_SAVE_INTERVAL_SECS as u64,
            t(loc, I18nKey::CrashRecoveryDirtySuffix),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    /// 每个测试一个独享目录（`process::id` + 名字，跑完删）。
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wv_session_marker_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn sample(dirty: bool, token: &str) -> SessionFile {
        SessionFile {
            app: MARKER_APP.to_owned(),
            version: MARKER_VERSION,
            token: token.to_owned(),
            started_at: 100,
            last_saved_at: 200,
            dirty,
        }
    }

    #[test]
    fn marker_yaml_round_trips_dirty_flag() {
        let dir = scratch_dir("roundtrip");
        let dir_s = dir.to_string_lossy().into_owned();
        write_marker(&dir_s, &sample(true, "t1"));
        assert_eq!(
            read_marker(&dir_s),
            Some(MarkerRead::Parsed(sample(true, "t1"))),
            "写进去的 dirty/时间戳要原样读回来"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_marker_still_counts_as_unclean_exit_but_reports_intact() {
        // 标记还在 = 上次没走正常退出；但标记说「没有未落盘编辑」→ 内容完整。
        let dir = scratch_dir("clean");
        let dir_s = dir.to_string_lossy().into_owned();
        write_marker(&dir_s, &sample(false, "t2"));
        let projection = WorldProjection::default();
        let report = report_for(&projection, &dir_s, read_marker(&dir_s).expect("应有标记"));
        assert_eq!(report.had_unsaved_edits, Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_marker_is_treated_as_crash_with_unknown_details() {
        // 写标记时被打断 = 半截 YAML：本身也是「没正常退出」的证据。
        let dir = scratch_dir("corrupt");
        let dir_s = dir.to_string_lossy().into_owned();
        std::fs::write(marker_path(&dir_s), "token: [unclosed\n").expect("写半截标记");
        assert_eq!(read_marker(&dir_s), Some(MarkerRead::Corrupt));
        let projection = WorldProjection::default();
        let report = report_for(&projection, &dir_s, MarkerRead::Corrupt);
        assert_eq!(report.had_unsaved_edits, None, "无法确认 → 按有风险说");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn foreign_file_with_same_name_is_ignored_and_never_deleted() {
        // 作者手放的同名文件（app 字段不对）不是我们的：不报告、不删除。
        let dir = scratch_dir("foreign");
        let dir_s = dir.to_string_lossy().into_owned();
        let path = marker_path(&dir_s);
        std::fs::write(&path, "app: other-tool\ntoken: x\n").expect("写他人文件");
        assert_eq!(read_marker(&dir_s), None, "不认领 → 当不存在");
        remove_marker_if_owner(&dir_s, "x");
        assert!(path.is_file(), "别人写的文件不该被我们删掉");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_requires_owning_token() {
        let dir = scratch_dir("owner");
        let dir_s = dir.to_string_lossy().into_owned();
        let path = marker_path(&dir_s);
        write_marker(&dir_s, &sample(false, "mine"));
        remove_marker_if_owner(&dir_s, "someone-else");
        assert!(
            path.is_file(),
            "token 不符时不能删（可能是另一个活着的实例）"
        );
        remove_marker_if_owner(&dir_s, "mine");
        assert!(!path.is_file(), "自己的标记要删掉");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 主流程：带着上次的脏标记打开工程 → 报异常退出 + 换新标记；
    /// 落盘（脏→干净）→ 标记心跳成 dirty:false。
    #[test]
    fn opening_a_crashed_project_reports_and_then_heartbeats_on_save() {
        let dir = scratch_dir("flow");
        let dir_s = dir.to_string_lossy().into_owned();
        // 上一次会话留下的标记：崩溃时是脏的。
        write_marker(&dir_s, &sample(true, "dead-session"));

        let mut world = World::new();
        world.init_resource::<SessionMarker>();
        world.init_resource::<CrashRecovery>();
        world.insert_resource(WorldProjection {
            project_dir: Some(dir_s.clone()),
            dirty: false,
            ..Default::default()
        });
        world
            .run_system_once(maintain_session_marker)
            .expect("系统运行失败");

        let report = world
            .resource::<CrashRecovery>()
            .report
            .clone()
            .expect("旧标记还在 → 必须报异常退出");
        assert_eq!(report.had_unsaved_edits, Some(true), "旧标记是脏的");

        // 新标记已就位：token 换新、dirty=false。
        match read_marker(&dir_s) {
            Some(MarkerRead::Parsed(file)) => {
                assert_ne!(file.token, "dead-session", "新会话要有新 token");
                assert!(!file.dirty, "刚载入 = 与磁盘一致");
            }
            other => panic!("新标记应可解析，实际 {other:?}"),
        }

        // 编辑 → 脏；落盘 → 干净：标记心跳到 dirty:false 且 last_saved_at 前进。
        world.resource_mut::<WorldProjection>().dirty = true;
        world
            .run_system_once(maintain_session_marker)
            .expect("系统运行失败");
        match read_marker(&dir_s) {
            Some(MarkerRead::Parsed(file)) => assert!(file.dirty, "有未落盘编辑时要记下来"),
            other => panic!("标记应可解析，实际 {other:?}"),
        }
        let before = world.resource::<SessionMarker>().last_saved_at;
        world.resource_mut::<WorldProjection>().dirty = false;
        world
            .run_system_once(maintain_session_marker)
            .expect("系统运行失败");
        match read_marker(&dir_s) {
            Some(MarkerRead::Parsed(file)) => {
                assert!(!file.dirty, "落盘后心跳成干净");
                assert!(file.last_saved_at >= before, "落盘时刻要前移（或同秒）");
            }
            other => panic!("标记应可解析，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 关工程（project_dir → None）时标记必须收掉：正常关闭不该留下恢复卡片。
    #[test]
    fn closing_project_removes_the_marker() {
        let dir = scratch_dir("close");
        let dir_s = dir.to_string_lossy().into_owned();
        let mut world = World::new();
        world.init_resource::<SessionMarker>();
        world.init_resource::<CrashRecovery>();
        world.insert_resource(WorldProjection {
            project_dir: Some(dir_s.clone()),
            ..Default::default()
        });
        world
            .run_system_once(maintain_session_marker)
            .expect("系统运行失败");
        assert!(marker_path(&dir_s).is_file(), "开工程应写标记");

        world.resource_mut::<WorldProjection>().project_dir = None;
        world
            .run_system_once(maintain_session_marker)
            .expect("系统运行失败");
        assert!(
            !marker_path(&dir_s).is_file(),
            "正常关闭工程后标记必须删除，否则下次打开误报崩溃"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
