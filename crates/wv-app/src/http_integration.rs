//! HTTP ↔ Bevy 主帧的集成层（ADR-0014 §2.3）。
//!
//! ## 装配模式
//! ```ignore
//! use crate::http_integration::HttpServerPlugin;
//! app.add_plugins(HttpServerPlugin::default());
//! ```
//!
//! 该 Plugin 在 `Startup` 时：
//! 1. 创建 `tokio::sync::mpsc` 通道对；
//! 2. 把 **sender** 端交 HTTP server（独立线程跑 `start_mcp_server`）；
//! 3. 把 **receiver** 端作为 `HttpReceiver` Resource 插入 Bevy 世界。
//!
//! 在 `Update` schedule 注册 [`drain_http_commands`] 系统：每帧从
//! `HttpReceiver` 排干消息，按顺序 apply 到 `WorldProjection.world`
//! + `EditHistory`。
//!
//! ## 与 §3.3 的关系
//! drain 系统只在 `Update` schedule 跑；HTTP handler 不持有任何 `World`
//! 句柄（全部经 `HttpBridge` → mpsc → drain）。这与 GUI 笔刷系统的纪律
//! 一致——`World` 是工作副本，**只**在主帧写。

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use std::sync::Arc;

use crate::edit::editing::EditHistory;
use crate::http::bridge::{
    drain_messages_with, HttpBridge, HttpMessage, ScreenshotPayload, UiReply, UiRequest,
};
use crate::http::server::start_mcp_server;
use crate::http::skills::ui;
use crate::state::{PanelState, WorldProjection};

/// MCP server 启动配置。
///
/// ## 默认值（ADR-0014 §2.2 + §6 Q1）
/// - bind: `127.0.0.1`
/// - port: `8732`
#[derive(Debug, Clone)]
pub struct HttpServerConfig {
    pub bind: String,
    pub port: u16,
}

impl Default for HttpServerConfig {
    fn default() -> Self {
        Self {
            bind: crate::http::DEFAULT_BIND.to_string(),
            port: crate::http::DEFAULT_PORT,
        }
    }
}

/// 持有 mpsc 的 receiver 端。Bevy 主帧每帧从这里 drain 消息。
#[derive(Resource)]
pub struct HttpReceiver {
    pub rx: tokio::sync::mpsc::UnboundedReceiver<HttpMessage>,
}

impl Default for HttpReceiver {
    fn default() -> Self {
        // 默认情况下创建一个空通道——仅在没接入 HTTP server 时占位。
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        Self { rx }
    }
}

/// HTTP 服务的运行期状态（设置对话框「HTTP 服务」分组展示用，2026-10-28）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpServiceStatus {
    /// 未启动 / 已停止。
    Stopped,
    /// 服务线程已拉起，等绑定结果（`poll_http_service` 每帧轮询）。
    Starting,
    /// 监听中。`addr` 为真实绑定地址（含 OS 分配端口），`port` 供
    /// 「输入框端口 ≠ 运行端口 → 换端口重启」的判定。
    Running { addr: String, port: u16 },
    /// 启动失败（端口占用 / 地址非法 / tokio 起不来）——`message` 为
    /// 原始错误串，设置对话框原样展示。
    Failed { message: String },
}

/// 服务线程 → 主帧的绑定结果槽位：`Ok((真实地址, 端口))` = 已监听；
/// `Err` = 绑定失败或 serve 异常退出。（别名只为压 type_complexity。）
type BindResultSlot = Arc<std::sync::Mutex<Option<Result<(String, u16), String>>>>;

/// HTTP 服务的运行时控制面：设置对话框 ↔ 服务线程。
///
/// ## 为什么存在
/// ADR-0014 的服务只在 `Startup` 拉起一次、无停机通道；设置里要「点按钮
/// 启动 / 停止 / 换端口重启」，就需要一个跨帧 Resource 持有停机信号与
/// 绑定结果槽位。自动启动与手动启停共用 [`start_http_service`] 一条路径。
///
/// ## 生命周期
/// - `Stopped/Failed` → [`start_http_service`]（拉线程，置 `Starting`）
///   → [`poll_http_service`] 轮询槽位 → `Running` / `Failed`。
/// - `Starting/Running` → [`stop_http_service`]（发优雅停机，置 `Stopped`）。
/// - 换端口 = stop + start（[`restart_http_service`]）。
///
/// `bridge` 跨重启复用：mpsc sender 克隆即得，`HttpReceiver` 的 receiver
/// 端全程只有一份，重启不换命令通道。
#[derive(Resource)]
pub struct HttpService {
    pub status: HttpServiceStatus,
    /// 绑定地址（CLI `--http-bind` 或默认 `127.0.0.1`）。启动后不可改
    /// （UI 只开端口；改绑定地址走 CLI flag 重启进程）。
    pub bind: String,
    /// 最近一次启动所用端口（`Starting/Running` 时即服务端口）。
    pub port: u16,
    /// 端口输入框的编辑缓冲（f32 适配 `numeric_input_salted`；初始 =
    /// 启动配置端口，跨对话框开关保留）。
    pub port_edit: f32,
    /// 跨重启复用的命令桥（sender 端克隆；receiver 端在 `HttpReceiver`）。
    bridge: HttpBridge,
    /// 优雅停机信号。`Some` = 服务线程在跑；`send(())` 即停机。
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    /// 服务线程 → 主帧的绑定结果槽位（`poll_http_service` 每帧取走）。
    /// `Ok((真实地址, 端口))` = 已监听；`Err` = 绑定失败或 serve 异常退出。
    bind_result: Option<BindResultSlot>,
}

impl HttpService {
    /// 从启动配置构造（`HttpServerPlugin` 用）。初始 `Stopped`，是否立即
    /// 拉起由 caller 调 [`start_http_service`] 决定。
    pub fn new(config: &HttpServerConfig, bridge: HttpBridge) -> Self {
        Self {
            status: HttpServiceStatus::Stopped,
            bind: config.bind.clone(),
            port: config.port,
            port_edit: f32::from(config.port),
            bridge,
            shutdown: None,
            bind_result: None,
        }
    }
}

/// 拉起 HTTP 服务线程（启动期自动拉起与设置对话框手动启动共用）。
///
/// 线程内起独立 tokio runtime（不动 GUI 主线程）跑 [`start_mcp_server`]；
/// 绑定成功经 `on_listen` 写槽位，失败 / serve 异常退出写 `Err`。状态置
/// `Starting`，由 [`poll_http_service`] 每帧轮询落定。
///
/// 已在 `Starting/Running` 时**不作为**（防双线程抢同端口）。
pub fn start_http_service(svc: &mut HttpService, port: u16) {
    if matches!(
        svc.status,
        HttpServiceStatus::Starting | HttpServiceStatus::Running { .. }
    ) {
        return;
    }
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let slot: BindResultSlot = Arc::new(std::sync::Mutex::new(None));
    let thread_slot = slot.clone();
    let bridge = svc.bridge.clone();
    let bind = svc.bind.clone();

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!(
                    "{}{e}",
                    crate::i18n::t(
                        crate::i18n::cli_locale(),
                        crate::i18n::I18nKey::HttpTokioStartFailed,
                    )
                );
                *thread_slot.lock().expect("槽位锁未中毒") = Some(Err(e.to_string()));
                return;
            }
        };
        rt.block_on(async move {
            // on_listen 回调与 Err 回写各持一份槽位句柄（回调 `move` 捕获）。
            let listen_slot = thread_slot.clone();
            let result = start_mcp_server(
                bridge,
                &bind,
                port,
                async move {
                    // sender drop（资源被清 / 进程退出）同样触发停机。
                    let _ = shutdown_rx.await;
                },
                move |addr| {
                    *listen_slot.lock().expect("槽位锁未中毒") =
                        Some(Ok((addr.to_string(), addr.port())));
                },
            )
            .await;
            // 绑定失败 / serve 中途异常：写 Err 供主帧展示（正常停机走
            // `Ok`，但那时主帧已置 `Stopped`、槽位已撤，此写入被丢弃）。
            // stderr 日志与 ADR-0014 同款：绑定失败已在 server 内报，
            // 这里只报 serve 阶段的异常退出。
            if let Err(e) = &result {
                if !matches!(e, crate::http::server::ServerError::Bind(_)) {
                    eprintln!(
                        "{}{e}",
                        crate::i18n::t(
                            crate::i18n::cli_locale(),
                            crate::i18n::I18nKey::HttpServerCrashed,
                        )
                    );
                }
                *thread_slot.lock().expect("槽位锁未中毒") = Some(Err(e.to_string()));
            }
        });
    });

    svc.shutdown = Some(shutdown_tx);
    svc.bind_result = Some(slot);
    svc.port = port;
    svc.status = HttpServiceStatus::Starting;
}

/// 停止 HTTP 服务：发优雅停机信号（axum 排空在途请求后线程自然退出），
/// 状态立置 `Stopped`。线程退出是异步的——紧接着在同端口重启可能撞
/// `EADDRINUSE`（Windows 上表现为绑定失败 → UI 显示 Failed），属已知
/// 取舍：不阻塞主帧等线程 join。
pub fn stop_http_service(svc: &mut HttpService) {
    if let Some(tx) = svc.shutdown.take() {
        let _ = tx.send(());
    }
    // 撤槽位：线程退出时的迟到 `Ok` 写入随 Arc 一起被丢弃。
    if let Some(slot) = svc.bind_result.take() {
        if let Ok(mut guard) = slot.lock() {
            *guard = None;
        }
    }
    svc.status = HttpServiceStatus::Stopped;
}

/// 换端口重启 = 停止 + 以新端口启动（设置对话框「换端口重启」按钮）。
pub fn restart_http_service(svc: &mut HttpService, port: u16) {
    stop_http_service(svc);
    start_http_service(svc, port);
}

/// 每帧轮询绑定结果槽位，把 `Starting` 落定为 `Running` / `Failed`。
///
/// `Running` 中收到 `Err`（serve 中途崩溃，极罕见）也转 `Failed`——
/// 否则 UI 会永远显示运行中。`Stopped`/`Failed` 态的迟到写入直接丢弃。
pub fn poll_http_service(mut svc: ResMut<HttpService>) {
    poll_http_service_inner(&mut svc);
}

/// [`poll_http_service`] 的纯函数形态（测试友好）：不依赖 Bevy 资源。
fn poll_http_service_inner(svc: &mut HttpService) {
    let Some(slot) = svc.bind_result.as_ref() else {
        return;
    };
    let taken = slot.lock().expect("槽位锁未中毒").take();
    match (&svc.status, taken) {
        (HttpServiceStatus::Starting, Some(Ok((addr, port)))) => {
            svc.status = HttpServiceStatus::Running { addr, port };
        }
        (HttpServiceStatus::Starting, Some(Err(message))) => {
            svc.status = HttpServiceStatus::Failed { message };
            // 线程已退出，停机句柄作废，回收以免残留「可停」假象。
            let _ = svc.shutdown.take();
        }
        (HttpServiceStatus::Running { .. }, Some(Err(message))) => {
            svc.status = HttpServiceStatus::Failed { message };
            let _ = svc.shutdown.take();
        }
        _ => {}
    }
}

/// 把 MCP server 装到 Bevy App 的 Plugin。
#[derive(Default)]
pub struct HttpServerPlugin {
    pub config: HttpServerConfig,
}

impl Plugin for HttpServerPlugin {
    fn build(&self, app: &mut App) {
        // 1) 建通道对：sender 给 server（经 HttpService 持有，跨重启复用），
        //    receiver 给主帧。
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);

        // 2) 控制面资源 + 启动期自动拉起（ADR-0014 行为不变：CLI
        //    `--http-port` 依旧生效，smoke-http.sh 依赖此默认）。
        //    运行中手动启停见设置对话框「HTTP 服务」分组。
        let mut svc = HttpService::new(&self.config, bridge);
        start_http_service(&mut svc, self.config.port);
        app.insert_resource(svc);

        // 3) receiver 端入 Bevy。
        app.insert_resource(HttpReceiver { rx });

        // 4) drain 系统每帧排空。
        //
        // **复用 GUI 的 [`EditHistory`]**（`edit::editing::EditHistory`，已由
        // `app_bootstrap` init_resource）：HTTP 与 Ctrl+Z 共用**同一**撤销栈——
        // ADR-0014 §2.3「单线程命令队列」的本意就是只有一个权威 History。
        // （v0.2 修：此前本模块自定义了一个带 `Arc<Mutex>` 的同名 Resource 且
        // 从未插入，真机 smoke 抓到 `Res<EditHistory> does not exist` 每帧 panic
        // ——drain 只在主帧跑，`ResMut` 本身就是正确且足够的访问形态，锁是多余的。）
        app.insert_resource(PendingShots::default());
        // `.chain()`：两系统都持 `ResMut<PendingShots>`，显式钉序（入队 → 轮询），
        // 不靠调度器对歧义系统的隐式顺序。
        app.add_systems(Update, (drain_http_commands, poll_pending_shots).chain());
        // 启停状态轮询：与 drain 无共享资源，独立注册即可。
        app.add_systems(Update, poll_http_service);
    }
}

/// 等待落盘的截图请求（ADR-0078 D3）。主帧 spawn `Screenshot` 后入此表，
/// [`poll_pending_shots`] 每帧轮询文件出现 → base64 → 回执。
#[derive(Resource, Default)]
pub struct PendingShots(Vec<PendingShot>);

struct PendingShot {
    path: std::path::PathBuf,
    /// `Option` 是为了能在 `&mut PendingShot` 上 `take()` 出来发回执
    /// （oneshot sender 不可 Copy，直接 move 字段过不了借用检查）。
    /// 发送前必然 Some——发送即取走，取走即移除条目。
    reply: Option<tokio::sync::oneshot::Sender<UiReply>>,
    /// 轮询过的帧数——超过 [`SHOT_MAX_WAIT_FRAMES`] 仍未落盘即报错回收，
    /// 不让 handler 吊死到 handler 侧 10s 超时（双保险，主帧侧先撤）。
    frames_waited: u32,
}

/// 落盘等待的帧数上限。渲染器正常 1–2 帧产出；60 帧（60 FPS 下 1 秒）
/// 还没出 = 窗口最小化 / 后台挂起，明确报错比静默等待好。
const SHOT_MAX_WAIT_FRAMES: u32 = 60;

/// 每帧从 `HttpReceiver` 排空消息，apply 到 WorldProjection.world + GUI 的 History。
///
/// ## 不破 §3.3
/// 这是**唯一**改 WorldProjection.world 的 HTTP 路径；handler 不直接持有
/// `World` 句柄（见 [`crate::http::bridge::HttpBridge`]）。
///
/// ## 与 GUI 撤销栈的关系
/// `edit.history` 就是 Ctrl+Z 用的那一个栈：LLM 的 `create_place` 之后
/// 用户按 Ctrl+Z 撤掉的是它，`undo` skill 撤掉的是 GUI 笔刷——同一时间线。
///
/// ## UI 操控消息（ADR-0078 D2）
/// UI 状态（`ViewState` / `PanelState`）与 World 一样**只在主帧读写**：
/// 世界数据类消息走 [`drain_messages`] 的既有路径，UI 类消息经
/// [`drain_messages_with`] 的钩子在本帧同步处理；截图特殊——本帧只
/// spawn `Screenshot` 组件并入 [`PendingShots`]，回执由 [`poll_pending_shots`]
/// 在落盘后发（渲染器要跑完这一帧才有图）。
pub fn drain_http_commands(
    mut receiver: ResMut<HttpReceiver>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut view: ResMut<wv_render::state::ViewState>,
    mut panels: ResMut<PanelState>,
    mut commands: Commands,
    mut pending: ResMut<PendingShots>,
    // 合成输入（ADR-0096）：队列入队 + 命令面板借道 SelectMapTool。
    mut synthetic: ResMut<crate::ui_shell::synthetic_input::SyntheticInputQueue>,
    mut palette: ResMut<crate::ui_command_palette::CommandPalette>,
    // 地图导航 / 相机（ADR-0096 追记）：AI 验收需要程序化「切图 + 定缩放」，
    // 替代早先只能靠 WV_DRILL / WV_SCALE 启动探针的绕行。
    (mut drill, mut camera_q): (
        ResMut<wv_render::state::DrillState>,
        Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    ),
) {
    // 把 save_dir 拷成 owned，先结束 `projection.project_dir` 的不可变借用，
    // 然后才能用 `&mut projection.world`（§3.3）。
    let save_dir: Option<std::path::PathBuf> = projection
        .project_dir
        .as_deref()
        .map(std::path::Path::new)
        .map(|p| p.to_path_buf());
    // 地图导航 / 缩放延迟到 drain 之后：`drill_to` 要 `&projection.world`，
    // 而该借用必须等 `drain_messages_with(&mut projection.world, …)` 结束。
    let mut pending_nav: Vec<(UiRequest, tokio::sync::oneshot::Sender<UiReply>)> = Vec::new();
    let mut ui_hook = |req: UiRequest, reply_tx: tokio::sync::oneshot::Sender<UiReply>| match req {
        UiRequest::Screenshot { save_to } => {
            begin_screenshot(save_to.as_deref(), &mut commands, &mut pending, reply_tx);
        }
        // 合成输入（ADR-0096）：入队即回执（消费在 apply_synthetic_input，
        // 每帧一个——调用方要发「两击成双击」就分两次 HTTP 调用）。
        UiRequest::EnqueueInput { events } => {
            for ev in events {
                match crate::ui_shell::synthetic_input::SyntheticEvent::try_from(ev) {
                    Ok(ev) => synthetic.push(ev),
                    Err(e) => {
                        let _ = reply_tx.send(UiReply::Error(e));
                        return;
                    }
                }
            }
            let _ = reply_tx.send(UiReply::State(ui::ui_snapshot(&view, &panels)));
        }
        // 切当前图 / 设置缩放（ADR-0096 追记）：delayed 到 drain 之后执行。
        r @ (UiRequest::SetCurrentMap { .. } | UiRequest::SetZoom { .. }) => {
            pending_nav.push((r, reply_tx));
        }
        // 切地图工具（ADR-0096）：借用命令面板既有 SelectMapTool 通路
        // （apply_pending_commands 消费，互斥全关 / 面板联动零重复实现）。
        UiRequest::SetMapTool { tool } => match ui::parse_map_tool(&tool) {
            Some(id) => {
                palette.pending = Some(crate::ui_command_palette::CommandId::SelectMapTool(id));
                let _ = reply_tx.send(UiReply::State(ui::ui_snapshot(&view, &panels)));
            }
            None => {
                let _ = reply_tx.send(UiReply::Error(format!(
                    "未知地图工具 `{tool}`；合法词表：{}",
                    ui::map_tool_vocab().join(" / ")
                )));
            }
        },
        other => {
            let reply = ui::handle_ui_request(other, &mut view, &mut panels)
                .unwrap_or_else(|| UiReply::Error("内部错误：非截图请求无回执".into()));
            let _ = reply_tx.send(reply);
        }
    };
    let processed = drain_messages_with(
        &mut receiver.rx,
        &mut projection.world,
        &mut edit.history,
        save_dir.as_deref(),
        &mut ui_hook,
    );
    // 本帧处理过消息 = 有未落盘的编辑，点亮顶栏 `●`（与 GUI 编辑同语义）。
    if processed {
        projection.dirty = true;
    }

    // 地图导航 / 缩放（ADR-0096 追记）——在 drain 之后，此时 `&projection.world`
    // 借用可用（drain 的可变借用已结束）。
    for (req, reply_tx) in pending_nav {
        match req {
            UiRequest::SetCurrentMap { map } => match wv_core::Id::new(&map) {
                Ok(id) => {
                    if drill.drill_to(&projection.world, &id) {
                        // 立即同步 `ViewState.current_map`：常态由每帧同步系统
                        // 从 drill 写回，但回执若等下一帧就是旧值（验收脚本
                        // 惯于「切完立刻断言」）。同步写 = 回执即真。
                        view.current_map = id.as_str().to_owned();
                        let _ = reply_tx.send(UiReply::State(ui::ui_snapshot(&view, &panels)));
                    } else {
                        let _ = reply_tx.send(UiReply::Error(format!(
                            "地图 `{map}` 不存在或不在 parent/children 链路（drill_to 返回 false）"
                        )));
                    }
                }
                Err(e) => {
                    let _ = reply_tx.send(UiReply::Error(format!("map id 非法: {e}")));
                }
            },
            UiRequest::SetZoom { scale } => {
                if scale.is_finite() && scale > 0.0 {
                    view.scale = scale;
                    wv_render::camera::apply_view_to_camera(
                        view.camera_center,
                        scale,
                        &mut camera_q,
                    );
                    let _ = reply_tx.send(UiReply::State(ui::ui_snapshot(&view, &panels)));
                } else {
                    let _ = reply_tx.send(UiReply::Error(format!(
                        "scale 必须是正有限数，收到 {scale}"
                    )));
                }
            }
            // 闭包只把这两类放进延迟队列。
            _ => {}
        }
    }
}

/// `screenshot` 请求的主帧入口（ADR-0078 D3）：解出落盘路径 → spawn
/// `Screenshot::primary_window()` + `save_to_disk` observer（与 `WV_SHOT`
/// 探针同款）→ 入 [`PendingShots`] 等轮询。路径解不出（父目录不存在等）
/// 当帧直接回 Error——连组件都不必 spawn。
fn begin_screenshot(
    save_to: Option<&str>,
    commands: &mut Commands,
    pending: &mut PendingShots,
    reply_tx: tokio::sync::oneshot::Sender<UiReply>,
) {
    let path = match ui::shot_path(save_to) {
        Ok(p) => p,
        Err(msg) => {
            let _ = reply_tx.send(UiReply::Error(msg));
            return;
        }
    };
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
    pending.0.push(PendingShot {
        path,
        reply: Some(reply_tx),
        frames_waited: 0,
    });
}

/// 每帧轮询等落盘的截图：文件出现（且 PNG 头可解析）→ 读字节 → base64 →
/// 回执并移除；超过 [`SHOT_MAX_WAIT_FRAMES`] → 回 Error 并移除。
/// 文件半写（读到了但头不完整）按未出现处理，下一帧再看。
pub fn poll_pending_shots(mut pending: ResMut<PendingShots>) {
    poll_pending_vec(&mut pending.0);
}

/// [`poll_pending_shots`] 的纯函数形态（测试友好）：对等待表做一轮轮询，
/// 不依赖 Bevy 运行时。
fn poll_pending_vec(pending: &mut Vec<PendingShot>) {
    pending.retain_mut(|shot| {
        match std::fs::read(&shot.path)
            .ok()
            .and_then(|b| ui::png_dimensions(&b).map(|(w, h)| (b, w, h)))
        {
            Some((bytes, width, height)) => {
                if let Some(reply) = shot.reply.take() {
                    let _ = reply.send(UiReply::Shot(ScreenshotPayload {
                        saved_path: shot.path.display().to_string(),
                        width,
                        height,
                        base64_png: ui::encode_base64(&bytes),
                    }));
                }
                false
            }
            None => {
                shot.frames_waited += 1;
                if shot.frames_waited > SHOT_MAX_WAIT_FRAMES {
                    if let Some(reply) = shot.reply.take() {
                        let _ = reply.send(UiReply::Error(format!(
                            "截图落盘超时（{SHOT_MAX_WAIT_FRAMES} 帧无产出）：{}",
                            shot.path.display()
                        )));
                    }
                    false
                } else {
                    true
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::{drain_messages, HttpBridge, ReadReply, ReadRequest};
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::Id;

    /// 轮询落定状态机（2026-10-28 设置对话框启停）：Starting+Ok → Running；
    /// Starting/Running+Err → Failed 且回收停机句柄；Stopped 态迟到写入丢弃。
    #[test]
    fn poll_http_service_lands_starting_into_running_and_failed() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut svc = HttpService::new(&HttpServerConfig::default(), HttpBridge::new(tx));
        assert_eq!(svc.status, HttpServiceStatus::Stopped);

        // ① Starting + 槽位 Ok → Running（地址/端口来自 on_listen 回报）
        svc.status = HttpServiceStatus::Starting;
        let slot = Arc::new(std::sync::Mutex::new(None));
        *slot.lock().unwrap() = Some(Ok(("127.0.0.1:12345".into(), 12345)));
        svc.bind_result = Some(slot);
        poll_http_service_inner(&mut svc);
        assert_eq!(
            svc.status,
            HttpServiceStatus::Running {
                addr: "127.0.0.1:12345".into(),
                port: 12345
            }
        );

        // ② Starting + 槽位 Err → Failed，且停机句柄被回收（线程已死）
        let (shutdown_tx, _shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        svc.shutdown = Some(shutdown_tx);
        svc.status = HttpServiceStatus::Starting;
        svc.bind_result = Some(Arc::new(std::sync::Mutex::new(Some(
            Err("绑定失败".into()),
        ))));
        poll_http_service_inner(&mut svc);
        assert_eq!(
            svc.status,
            HttpServiceStatus::Failed {
                message: "绑定失败".into()
            }
        );
        assert!(svc.shutdown.is_none(), "Failed 萆定时应回收停机句柄");

        // ③ Running 中 serve 崩溃（迟到 Err）→ Failed，不留「假运行中」
        svc.status = HttpServiceStatus::Running {
            addr: "127.0.0.1:1".into(),
            port: 1,
        };
        svc.bind_result = Some(Arc::new(std::sync::Mutex::new(Some(Err(
            "serve 崩溃".into()
        )))));
        poll_http_service_inner(&mut svc);
        assert_eq!(
            svc.status,
            HttpServiceStatus::Failed {
                message: "serve 崩溃".into()
            }
        );

        // ④ Stopped 态的迟到 Ok 写入（停机后线程正常退出）→ 保持 Stopped
        svc.status = HttpServiceStatus::Stopped;
        svc.bind_result = Some(Arc::new(std::sync::Mutex::new(Some(Ok((
            "127.0.0.1:1".into(),
            1,
        ))))));
        poll_http_service_inner(&mut svc);
        assert_eq!(svc.status, HttpServiceStatus::Stopped, "迟到写入应被丢弃");
    }

    /// 真线程生命周期（2026-10-28 设置对话框启停）：port 0 启动 → OS 分配
    /// 端口 → Running 且可 TCP 连接 → 停止 → 状态立落 Stopped、端口释放；
    /// Running 中重复 start 不作为；重启后再次 Running（bridge 复用路径）。
    #[test]
    fn http_service_start_stop_lifecycle_binds_then_frees_port() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let cfg = HttpServerConfig {
            bind: "127.0.0.1".into(),
            port: 0,
        };
        let mut svc = HttpService::new(&cfg, HttpBridge::new(tx));

        // ① 启动 → Starting → 轮询至 Running（绑定毫秒级，余量 5s）
        start_http_service(&mut svc, 0);
        assert_eq!(svc.status, HttpServiceStatus::Starting);
        let bound = poll_until_running(&mut svc, 500);
        let port = bound.expect("5s 内应进入 Running");
        assert_ne!(port, 0, "端口 0 应由 OS 分配真实端口");

        // ② Running 态该端口可建立 TCP 连接（真实监听存在）
        assert!(
            std::net::TcpStream::connect(("127.0.0.1", port)).is_ok(),
            "Running 态应可 TCP 连接 port={port}"
        );

        // ③ Running 中重复 start：不作为（状态与端口不变，防双线程抢端口）
        start_http_service(&mut svc, 9999);
        assert_eq!(
            svc.status,
            HttpServiceStatus::Running {
                addr: format!("127.0.0.1:{port}"),
                port
            }
        );

        // ④ 停止 → 状态立落 Stopped；线程退出后端口释放（连接被拒）
        stop_http_service(&mut svc);
        assert_eq!(svc.status, HttpServiceStatus::Stopped);
        let mut freed = false;
        for _ in 0..200 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
                freed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(freed, "停机后 2s 内端口应释放 port={port}");

        // ⑤ 重启（port 0 再来一次）：bridge 复用路径走通，再次 Running
        restart_http_service(&mut svc, 0);
        assert_eq!(svc.status, HttpServiceStatus::Starting);
        let rebound = poll_until_running(&mut svc, 500);
        assert!(rebound.is_some(), "重启后 5s 内应再次 Running");
        stop_http_service(&mut svc);
    }

    /// 轮询至 Running 的测试辅助：最多 `max_polls` 次、每次 10ms。
    fn poll_until_running(svc: &mut HttpService, max_polls: u32) -> Option<u16> {
        for _ in 0..max_polls {
            poll_http_service_inner(svc);
            if let HttpServiceStatus::Running { port, .. } = &svc.status {
                return Some(*port);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    /// 造一个最小合法 PNG 头（签名 + IHDR + 1920×1080），给 poll 测试当
    /// 「已落盘」的假截图。
    fn fake_png(width: u32, height: u32) -> Vec<u8> {
        let mut b = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        b.extend_from_slice(&[0, 0, 0, 13]);
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&width.to_be_bytes());
        b.extend_from_slice(&height.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        b
    }

    /// poll 纯函数三态：文件未出现 → 保留且不回执；出现 → Shot 回执并清表；
    /// 超帧数 → Error 回执并清表。落盘目录走系统临时目录（受限环境跑测试
    /// 须按 AGENTS §2.2 追记改 TEMP/TMP）。
    #[test]
    fn poll_pending_vec_replies_shot_when_file_appears_and_errors_on_timeout() {
        let dir = std::env::temp_dir().join("wv-http-ui-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("poll-test.png");
        let _ = std::fs::remove_file(&path);

        let (tx, mut rx) = tokio::sync::oneshot::channel::<UiReply>();
        let mut pending = vec![PendingShot {
            path: path.clone(),
            reply: Some(tx),
            frames_waited: 0,
        }];

        // ① 文件未出现：保留待轮询，不回执
        poll_pending_vec(&mut pending);
        assert_eq!(pending.len(), 1, "文件未出现应保留");
        assert!(rx.try_recv().is_err(), "不应提前回执");

        // ② 落盘后：Shot 回执 + 表清空
        std::fs::write(&path, fake_png(800, 600)).unwrap();
        poll_pending_vec(&mut pending);
        assert!(pending.is_empty(), "回执后应移除");
        match rx.try_recv().unwrap() {
            UiReply::Shot(p) => {
                assert_eq!(p.width, 800, "IHDR 宽");
                assert_eq!(p.height, 600, "IHDR 高");
                assert!(!p.base64_png.is_empty());
                assert_eq!(p.saved_path, path.display().to_string());
            }
            other => panic!("应为 Shot，got {other:?}"),
        }

        // ③ 超帧数：Error 回执 + 表清空（新请求，文件永不出现）
        let (tx2, mut rx2) = tokio::sync::oneshot::channel::<UiReply>();
        let mut stale = vec![PendingShot {
            path: dir.join("never-appears.png"),
            reply: Some(tx2),
            frames_waited: SHOT_MAX_WAIT_FRAMES,
        }];
        poll_pending_vec(&mut stale);
        assert!(stale.is_empty());
        match rx2.try_recv().unwrap() {
            UiReply::Error(msg) => assert!(msg.contains("超时"), "raw = {msg}"),
            other => panic!("应为 Error，got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// 直接构造 drain 路径，避免起 Plugin 整盘——单元测试用更稳。
    #[test]
    fn drain_applies_apply_message_through_resource_path() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);

        let mut world = wv_core::World::default();
        world.maps.push(MapNode {
            comment: None,
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: Some(Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
        });

        let place = wv_core::distance::Place {
            kind: None,
            id: Id::new_unchecked("qingyun"),
            name: "青云城".into(),
            map: Some(Id::new_unchecked("world_main")),
            x: Some(100.0),
            y: Some(200.0),
            w: None,
            h: None,
            feature: None,
            biome: None,
            envelope: None,
            rank: None,
            line_kind: None,
            line_width_m: None,
            line: None,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let cmd: Box<dyn wv_edit::EditCommand> = Box::new(wv_edit::AddPlaceCommand::new(place));
        let apply_reply = bridge.apply_send(cmd);
        let mut history = wv_edit::History::default();
        drain_messages(&mut { rx }, &mut world, &mut history, None);
        let _ = rt.block_on(apply_reply).unwrap();
        assert_eq!(world.distances.places.len(), 1);
    }

    /// 读消息经过 bridge.read 的路径：从 rx 取出 read，handler 调用 read，回执
    /// 应是 MapSummaries。这条路径在 `skills/read.rs` 的 list_maps_round_trip
    /// 已覆盖；这里只测 drain 不破坏 World 的不变量。
    #[test]
    fn drain_read_message_does_not_mutate_world() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);

        let mut world = wv_core::World::default();
        let initial_maps = world.maps.len();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let reply_rx = bridge.read_send(ReadRequest::ListMaps);
        drain_messages(&mut rx, &mut world, &mut wv_edit::History::default(), None);
        let reply = rt.block_on(reply_rx).unwrap();
        match reply {
            ReadReply::MapSummaries(v) => assert!(v.is_empty()),
            _ => panic!("应为 MapSummaries"),
        }
        assert_eq!(world.maps.len(), initial_maps, "读消息不应动 World");
    }
}
