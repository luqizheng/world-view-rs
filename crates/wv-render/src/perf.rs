//! per-system 帧耗时探针（`WV_PERF=1` 开启，生产默认关闭）。
//!
//! ## 为什么要有这个模块
//!
//! 2026-10-23 用户实测：静止 120 FPS，**鼠标在地图上移动 → 40 FPS**。差 17ms
//! 必须落到**具体 system 名**上才许改代码（「不许猜」是本轮口径）。Bevy 自带的
//! `SystemInformationDiagnosticsPlugin` 只给进程级 CPU，没有 per-system 拆分，
//! 所以自己量。
//!
//! ## 用法（三行，可整段删）
//!
//! 1. 装配处 `init_resource::<PerfRecorder>()`；
//! 2. 链条尾部 `add_systems(Last, perf::report_frame)`；
//! 3. 每个想量的 system 用 [`perf_timed!`] 包一层：
//!    `perf_timed!("sync_region_outlines", region_outline::sync_region_outlines)`。
//!
//! **不往 system 体内塞计时代码**：包裹在装配点完成，删掉宏调用即彻底退场，
//! 被测系统签名与实现一行不动。
//!
//! ## 口径
//!
//! - 判据是**每帧每个 system 的墙钟毫秒**，只用于「谁吃掉了 17ms」的定位；
//!   验收仍以计数器为准（见 `docs/PERF_GRID_AND_PROFILE_v0.1.md` §6 的口径）。
//! - 单帧超过 [`SLOW_MS`]（4ms）**立刻**打一行；每 [`WINDOW`]（5s）打一次
//!   top5 与最差帧。
//! - `WV_PERF=1` 未设置时：`begin` / `end` 都是**两次布尔判断直接返回**，
//!   不取 `Instant`（`Instant::now()` 在 Windows 上约 20–30ns，但 100 个 system
//!   加起来仍不该白付）。
//!
//! ## 已知代价
//!
//! [`perf_timed!`] 把一个 system 变成 3 个（起点 / 本体 / 终点）——即使关闭，
//! 调度器仍要为两个空 system 付入场开销（实测 <1µs/帧/对，总 <0.1ms）。
//! 定标级改动（如渲染管线重构）完成后整段摘掉即可。

#![cfg(feature = "app")]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bevy::prelude::*;

/// 供 [`perf_timed!`] 展开使用（宏在别的 crate 里展开，路径必须能解析到）。
#[doc(hidden)]
pub use bevy::prelude::ResMut;

/// 单帧单 system 超过这个毫秒数就立刻打印一行。
///
/// 4ms：60FPS 预算 16.7ms 的四分之一。低于它的波动落在噪声里，逐条打印会
/// 把真正吃掉 17ms 的那条淹掉。
pub const SLOW_MS: f32 = 4.0;

/// 汇总窗口：每这么久打印一次 top5 + 最差帧。
///
/// 5s：够跨过一次缩放 / 一次框选，又短到作者还能把「我刚才做了什么」和
/// 数字对上。
pub const WINDOW: Duration = Duration::from_secs(5);

/// 窗口汇总打印前 N 名。
pub const TOP_N: usize = 5;

/// 单个 system 的累计样本（窗口内 / 全量各一份）。
#[derive(Debug, Clone, Copy, Default)]
struct Sample {
    n: u64,
    sum_ms: f64,
    max_ms: f32,
}

impl Sample {
    fn push(&mut self, ms: f32) {
        self.n += 1;
        self.sum_ms += f64::from(ms);
        if ms > self.max_ms {
            self.max_ms = ms;
        }
    }

    fn avg_ms(&self) -> f32 {
        if self.n == 0 {
            0.0
        } else {
            (self.sum_ms / self.n as f64) as f32
        }
    }
}

/// 最差帧（窗口内单帧「所有被测 system 之和」最大的一帧）。
#[derive(Debug, Clone, Copy)]
pub struct WorstFrame {
    pub frame: u64,
    pub work_ms: f32,
    pub top: &'static str,
    pub top_ms: f32,
}

/// per-system 计时器（Resource）。
#[derive(Resource)]
pub struct PerfRecorder {
    enabled: bool,
    /// 已 `begin` 未 `end` 的 system（按名字分槽——不同链条可能交错执行，
    /// 单一「当前槽」会被互相覆盖）。
    open: HashMap<&'static str, Instant>,
    /// 本帧已完成的样本。
    frame: Vec<(&'static str, f32)>,
    frame_no: u64,
    window_start: Instant,
    window: HashMap<&'static str, Sample>,
    worst: Option<WorstFrame>,
    /// 全量累计（测试 / QA 读 [`PerfRecorder::snapshot`]）。
    totals: HashMap<&'static str, Sample>,
}

impl Default for PerfRecorder {
    /// 缺省读环境变量 `WV_PERF`：设置即开启。
    fn default() -> Self {
        Self::new(std::env::var_os("WV_PERF").is_some())
    }
}

impl PerfRecorder {
    /// 显式指定开关（测试用——不依赖进程环境）。
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            open: HashMap::new(),
            frame: Vec::new(),
            frame_no: 0,
            window_start: Instant::now(),
            window: HashMap::new(),
            worst: None,
            totals: HashMap::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 本帧已结束的 system 条数。
    pub fn frame_samples(&self) -> usize {
        self.frame.len()
    }

    /// `begin` 与 `end` 配对成功（= 真正量到）的次数，全量累计。
    pub fn sample_count(&self) -> u64 {
        self.totals.values().map(|s| s.n).sum()
    }

    /// 全量累计快照：`(name, 次数, 最大 ms, 平均 ms)`，按最大 ms 降序。
    pub fn snapshot(&self) -> Vec<(&'static str, u64, f32, f32)> {
        let mut out: Vec<(&'static str, u64, f32, f32)> = self
            .totals
            .iter()
            .map(|(k, s)| (*k, s.n, s.max_ms, s.avg_ms()))
            .collect();
        out.sort_by(|a, b| b.2.total_cmp(&a.2));
        out
    }

    /// 记录一个 system 的起点。
    pub fn begin(&mut self, name: &'static str) {
        if !self.enabled {
            return;
        }
        self.open.insert(name, Instant::now());
    }

    /// 记录一个 system 的终点（没配到起点则忽略——例如系统被 run condition 跳过）。
    pub fn end(&mut self, name: &'static str) {
        if !self.enabled {
            return;
        }
        let Some(t0) = self.open.remove(name) else {
            return;
        };
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.frame.push((name, ms));
        self.totals.entry(name).or_default().push(ms);
    }

    /// 帧末：单帧超阈值立刻打印；满窗口打 top5 + 最差帧。
    pub fn flush_frame(&mut self) {
        if !self.enabled {
            return;
        }
        self.frame_no += 1;
        if self.frame.is_empty() {
            return;
        }
        let mut work_ms = 0.0f32;
        let mut top = ("", 0.0f32);
        for (name, ms) in &self.frame {
            work_ms += *ms;
            if *ms > top.1 {
                top = (name, *ms);
            }
            if *ms >= SLOW_MS {
                bevy::log::info!(target: "wv_perf", "frame {}: {name} {ms:.2}ms", self.frame_no);
            }
        }
        if self.worst.is_none_or(|w| work_ms > w.work_ms) {
            self.worst = Some(WorstFrame {
                frame: self.frame_no,
                work_ms,
                top: top.0,
                top_ms: top.1,
            });
        }
        for (name, ms) in self.frame.drain(..) {
            self.window.entry(name).or_default().push(ms);
        }
        if self.window_start.elapsed() >= WINDOW {
            self.print_window();
            self.window.clear();
            self.worst = None;
            self.window_start = Instant::now();
        }
    }

    fn print_window(&self) {
        let frames = self.window.values().map(|s| s.n).max().unwrap_or(0);
        let mut ranked: Vec<(&'static str, Sample)> =
            self.window.iter().map(|(k, s)| (*k, *s)).collect();
        ranked.sort_by(|a, b| b.1.max_ms.total_cmp(&a.1.max_ms));
        let top: Vec<String> = ranked
            .iter()
            .take(TOP_N)
            .map(|(name, s)| format!("{name} max={:.2} avg={:.2} n={}", s.max_ms, s.avg_ms(), s.n))
            .collect();
        bevy::log::info!(
            target: "wv_perf",
            "window: frames={frames} top{TOP_N}: {}",
            top.join(" | ")
        );
        if let Some(w) = self.worst {
            bevy::log::info!(
                target: "wv_perf",
                "worst frame #{}: work={:.2}ms top={} {:.2}ms",
                w.frame, w.work_ms, w.top, w.top_ms
            );
        }
    }
}

/// 帧末汇总系统（注册在 `Last`）。
pub fn report_frame(mut perf: ResMut<PerfRecorder>) {
    perf.flush_frame();
}

/// 把一个 system 包成「起点 + 本体 + 终点」的链。
///
/// ```ignore
/// app.add_systems(Update, perf_timed!("sync_region_outlines", sync_region_outlines));
/// ```
///
/// 需要在作用域内有 `bevy::prelude::*`（`.chain()` 来自
/// `IntoScheduleConfigs`）。整段摘除 = 把 `perf_timed!(..)` 换回裸 system。
#[macro_export]
macro_rules! perf_timed {
    ($name:literal, $system:expr $(,)?) => {
        ((
            (move |mut __perf: $crate::perf::ResMut<$crate::perf::PerfRecorder>| {
                __perf.begin($name);
            }),
            $system,
            (move |mut __perf: $crate::perf::ResMut<$crate::perf::PerfRecorder>| {
                __perf.end($name);
            }),
        )
            .chain())
    };
}

/// 测试用最小 App（`App::new()` 即够：`MainSchedulePlugin` 已内置 `Update`/`Last`）。
///
/// 放在测试模块**之前**：clippy 的 `items_after_test_module` 会拦。
#[doc(hidden)]
pub fn __test_app() -> App {
    let mut app = App::new();
    app.insert_resource(PerfRecorder::new(true));
    app
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_recorder_records_nothing() {
        let mut r = PerfRecorder::new(false);
        r.begin("a");
        r.end("a");
        assert_eq!(r.sample_count(), 0, "关闭时不该记样本");
        assert!(r.snapshot().is_empty());
    }

    #[test]
    fn begin_end_pair_records_one_sample_with_that_name() {
        let mut r = PerfRecorder::new(true);
        r.begin("alpha");
        r.end("alpha");
        assert_eq!(r.sample_count(), 1);
        let snap = r.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, "alpha");
        assert_eq!(snap[0].1, 1);
        assert!(snap[0].2 >= 0.0, "最大耗时应是有限非负数");
    }

    #[test]
    fn end_without_begin_is_ignored() {
        let mut r = PerfRecorder::new(true);
        r.end("ghost");
        assert_eq!(r.sample_count(), 0, "没有起点就不该产出样本");
    }

    #[test]
    fn keyed_slots_do_not_clobber_each_other() {
        // 两条链交错：A.start → B.start → A.end → B.end，两个样本都要在。
        let mut r = PerfRecorder::new(true);
        r.begin("a");
        r.begin("b");
        r.end("a");
        r.end("b");
        assert_eq!(r.sample_count(), 2);
    }

    #[test]
    fn flush_frame_clears_the_frame_buffer_but_keeps_totals() {
        let mut r = PerfRecorder::new(true);
        r.begin("a");
        r.end("a");
        assert_eq!(r.frame_samples(), 1);
        r.flush_frame();
        assert_eq!(r.frame_samples(), 0, "帧末应清空本帧缓冲");
        assert_eq!(r.sample_count(), 1, "全量累计不该被帧末清掉");
    }

    #[test]
    fn snapshot_is_sorted_by_worst_first() {
        let mut r = PerfRecorder::new(true);
        // 直接灌样本（不经过 Instant）——排序口径只看 max_ms。
        r.totals.entry("slow").or_default().push(9.0);
        r.totals.entry("fast").or_default().push(0.5);
        let snap = r.snapshot();
        assert_eq!(snap[0].0, "slow", "最大耗时应排第一");
        assert_eq!(snap[1].0, "fast");
    }

    /// 端到端：`perf_timed!` 包进真实 App 后
    /// ① 执行顺序不变（嵌套 `.chain()` 真的把顺序传下去了）；
    /// ② 标记系统**确实在跑**（`snapshot` 有 3 条样本）——「连接成功 ≠ 会执行」。
    #[test]
    fn timed_wrapper_keeps_order_and_actually_records() {
        #[derive(Resource, Default)]
        struct Log(Vec<&'static str>);

        fn a(mut log: ResMut<Log>) {
            log.0.push("a");
        }
        fn b(mut log: ResMut<Log>) {
            log.0.push("b");
        }
        fn c(mut log: ResMut<Log>) {
            log.0.push("c");
        }

        let mut app = crate::perf::__test_app();
        app.init_resource::<Log>()
            .init_resource::<PerfRecorder>()
            .add_systems(
                Update,
                (
                    crate::perf_timed!("a", a),
                    crate::perf_timed!("b", b),
                    crate::perf_timed!("c", c),
                )
                    .chain(),
            );
        app.update();
        assert_eq!(
            app.world().resource::<Log>().0,
            vec!["a", "b", "c"],
            "嵌套 chain 必须保持声明顺序"
        );
        let rec = app.world().resource::<PerfRecorder>();
        let names: Vec<&'static str> = rec.snapshot().into_iter().map(|s| s.0).collect();
        for want in ["a", "b", "c"] {
            assert!(
                names.contains(&want),
                "{want} 没有样本——标记系统没被执行（snapshot={names:?}）"
            );
        }
    }
}
