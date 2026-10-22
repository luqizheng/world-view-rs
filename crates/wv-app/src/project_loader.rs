//! 后台工程加载任务（2026-10-22，C 档：加载不再阻塞 UI 帧）。
//!
//! ## 为什么要有这个模块
//!
//! 旧链路（`ui_shell::startup_load` → `load_project_into`）在 egui pass 的
//! **某一帧里同步**做完 `Project::open` + `load_world`（磁盘 IO + YAML 解析）。
//! 大工程（广东：7.8 万点 / 8.4 万边）加载期间那一帧直接冻住——Windows 会
//! 判「无响应」，遮罩的转圈动画也因为「遮罩画完了才开始加载」而看不到转。
//!
//! 新链路把**纯 IO + 解析**搬到后台线程：
//!
//! ```text
//! 后台线程：Project::open(dir) → load_world() → Result<World, String> ──(通道)──┐
//!                                                                            ▼
//! 主线程（每帧 poll）：取回结果 → 应用副作用（投影 / 层级 / 诊断 / 底图 / 相机）
//! ```
//!
//! ## 边界（不可越的两条线）
//!
//! 1. **后台线程只碰磁盘**。它不接触任何 Bevy 资源（`World` 是纯数据，`Send`）。
//! 2. **应用副作用只在主线程**。`adopt_world_snapshot` 要写 `Assets<Image>`、
//!    要改相机 transform / projection——这些必须在主线程做，且受 Bevy
//!    「单系统 16 参数」上限约束（见 `ui_shell::drive_project_load` 的折元组写法）。
//!
//! ## 与「应用屏」的关系
//!
//! 加载期间应用处于 [`AppScreen::loading`](crate::app_screen::AppScreen)——
//! 「正在加载」是**叠加态**，不夺走底下的表面（换工程时旧地图仍在屏上）。
//! 本模块只回答「有没有任务在跑 / 结果回来了没」，不回答「该显示什么」。

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use bevy::prelude::*;

use wv_core::World;

/// 加载结果：成功是整个世界，失败是**可直接展示给用户**的错误串。
///
/// 错误串在后台线程里就格式化好（带工程目录），主线程只管往 `load_error`
/// 与状态栏回执里放——主线程不再碰 `wv_io::Error`（它不是 `Send` 友好的契约面）。
pub type LoadOutcome = Result<World, String>;

/// 排队中的「要加载哪个工程」。
///
/// 三个发起点共用它（`ui_shell`）：
/// - CLI `-m <dir>`（经 `StartupLoadRequest` 转过来）；
/// - 「打开工程…」对话框选完目录；
/// - 命令面板 / 菜单的「打开最近工程」。
///
/// 排队而非直接派发，是为了**让加载只有一个发起点**（`drive_project_load`），
/// 避免三处各自 spawn 线程互相踩。
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct PendingProjectLoad(pub Option<String>);

/// 后台加载任务：一个在跑的线程 + 一条结果通道。
#[derive(Resource, Default)]
pub struct ProjectLoadTask {
    /// 正在加载的工程目录（`None` = 当前没有任务）。
    dir: Option<String>,
    /// 结果通道（后台线程 → 主线程）。
    rx: Option<Arc<Mutex<Receiver<LoadOutcome>>>>,
    /// 后台线程句柄。**不 join**：线程在送出结果后即退出，join 只会给主线程
    /// 加一次无谓的等待；丢句柄即 detach，进程退出时由 OS 回收。
    _handle: Option<JoinHandle<()>>,
}

impl ProjectLoadTask {
    /// 有任务在跑吗（已派发、结果还没回来）。
    pub fn is_active(&self) -> bool {
        self.dir.is_some()
    }

    /// 正在加载的工程目录（供遮罩显示）。
    pub fn dir(&self) -> Option<&str> {
        self.dir.as_deref()
    }

    /// 派发一次后台加载（幂等：已有任务在跑时不覆盖）。
    pub fn start(&mut self, dir: String) {
        if self.is_active() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let thread_dir = dir.clone();
        let handle = std::thread::spawn(move || {
            let outcome = wv_io::Project::open(&thread_dir)
                .and_then(|p| p.load_world())
                .map_err(|e| format!("打开工程 `{thread_dir}` 失败: {e}"));
            // 发送失败 = 主线程已丢弃任务（窗口关闭等），忽略即可。
            let _ = tx.send(outcome);
        });
        self.dir = Some(dir);
        self.rx = Some(Arc::new(Mutex::new(rx)));
        self._handle = Some(handle);
    }

    /// 取回后台结果（`None` = 还在跑）。
    ///
    /// 结果到手即清空任务状态（`is_active()` 变 false），让调用方在同一帧
    /// 完成应用副作用。
    pub fn poll(&mut self) -> Option<(String, LoadOutcome)> {
        let rx = self.rx.clone()?;
        let outcome = {
            let guard = rx.lock().ok()?;
            match guard.try_recv() {
                Ok(v) => Some(v),
                // 还在跑。
                Err(TryRecvError::Empty) => None,
                // 线程没了却没送结果：当作失败，避免永远卡在「加载中」。
                Err(TryRecvError::Disconnected) => Some(Err("加载线程意外结束".to_string())),
            }
        };
        let outcome = outcome?;
        let dir = self.dir.take().unwrap_or_default();
        self.rx = None;
        self._handle = None;
        Some((dir, outcome))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不存在的目录 → 后台线程回传失败，且**主线程不被阻塞**。
    ///
    /// 断言重点是「拿到的是 Err 而不是 Ok」，以及任务状态在取回后清空
    /// （`is_active()` 变 false，屏幕才能从「加载中」退出来）。
    #[test]
    fn loading_a_missing_directory_reports_failure_and_clears_the_task() {
        let mut task = ProjectLoadTask::default();
        task.start("definitely-not-a-project".to_string());
        assert!(task.is_active(), "派发后任务应处于进行中");

        // 轮询直到结果回来（后台线程可能还没跑完）。
        let mut result = None;
        for _ in 0..200 {
            if let Some((dir, outcome)) = task.poll() {
                result = Some((dir, outcome));
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (dir, outcome) = result.expect("后台加载应在有限时间内给出结果");
        assert_eq!(dir, "definitely-not-a-project");
        assert!(outcome.is_err(), "不存在的目录必须回传失败");
        assert!(
            !task.is_active(),
            "结果取回后任务应清空，否则界面会永远停在「加载中」"
        );
    }

    /// 没有任务在跑时 poll 返回 `None`（而不是 panic 或伪造结果）。
    #[test]
    fn polling_an_idle_task_yields_nothing() {
        let mut task = ProjectLoadTask::default();
        assert!(!task.is_active());
        assert!(task.poll().is_none());
    }

    /// 重复派发不覆盖在跑的任务（后一次 start 应被忽略）。
    #[test]
    fn starting_twice_keeps_the_first_task() {
        let mut task = ProjectLoadTask::default();
        task.start("first".to_string());
        task.start("second".to_string());
        assert_eq!(task.dir(), Some("first"), "在跑的任务不应被后来的派发覆盖");
    }
}
