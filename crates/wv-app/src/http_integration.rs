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

/// 把 MCP server 装到 Bevy App 的 Plugin。
#[derive(Default)]
pub struct HttpServerPlugin {
    pub config: HttpServerConfig,
}

impl Plugin for HttpServerPlugin {
    fn build(&self, app: &mut App) {
        // 1) 建通道对：sender 给 server，receiver 给主帧。
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);

        // 2) 启独立线程跑 tokio runtime（不动 GUI 主线程——Bevy 自己跑）。
        let cfg = self.config.clone();
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
                    return;
                }
            };
            rt.block_on(async move {
                if let Err(e) = start_mcp_server(bridge, &cfg.bind, cfg.port).await {
                    eprintln!(
                        "{}{e}",
                        crate::i18n::t(
                            crate::i18n::cli_locale(),
                            crate::i18n::I18nKey::HttpServerCrashed,
                        )
                    );
                }
            });
        });

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
) {
    // 把 save_dir 拷成 owned，先结束 `projection.project_dir` 的不可变借用，
    // 然后才能用 `&mut projection.world`（§3.3）。
    let save_dir: Option<std::path::PathBuf> = projection
        .project_dir
        .as_deref()
        .map(std::path::Path::new)
        .map(|p| p.to_path_buf());
    let mut ui_hook = |req: UiRequest, reply_tx: tokio::sync::oneshot::Sender<UiReply>| match req {
        UiRequest::Screenshot { save_to } => {
            begin_screenshot(save_to.as_deref(), &mut commands, &mut pending, reply_tx);
        }
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
