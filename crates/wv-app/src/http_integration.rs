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

use crate::edit::editing::EditHistory;
use crate::http::bridge::{drain_messages, HttpBridge, HttpMessage};
use crate::http::server::start_mcp_server;
use crate::state::WorldProjection;

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
        app.add_systems(Update, drain_http_commands);
    }
}

/// 每帧从 `HttpReceiver` 排空消息，apply 到 WorldProjection.world + GUI 的 History。
///
/// ## 不破 §3.3
/// 这是**唯一**改 WorldProjection.world 的 HTTP 路径；handler 不直接持有
/// `World` 句柄（见 [`crate::http::bridge::HttpBridge`]）。
///
/// ## 与 GUI 撤销栈的关系
/// `edit.history` 就是 Ctrl+Z 用的那一个栈：LLM 的 `create_place` 之后
/// 用户按 Ctrl+Z 撤掉的是它，`undo` skill 撤掉的是 GUI 笔刷——同一时间线。
pub fn drain_http_commands(
    mut receiver: ResMut<HttpReceiver>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
) {
    // 把 save_dir 拷成 owned，先结束 `projection.project_dir` 的不可变借用，
    // 然后才能用 `&mut projection.world`（§3.3）。
    let save_dir: Option<std::path::PathBuf> = projection
        .project_dir
        .as_deref()
        .map(std::path::Path::new)
        .map(|p| p.to_path_buf());
    let processed = drain_messages(
        &mut receiver.rx,
        &mut projection.world,
        &mut edit.history,
        save_dir.as_deref(),
    );
    // 本帧处理过消息 = 有未落盘的编辑，点亮顶栏 `●`（与 GUI 编辑同语义）。
    if processed {
        projection.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::bridge::{HttpBridge, ReadReply, ReadRequest};
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::Id;

    /// 直接构造 drain 路径，避免起 Plugin 整盘——单元测试用更稳。
    #[test]
    fn drain_applies_apply_message_through_resource_path() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);

        let mut world = wv_core::World::default();
        world.maps.push(MapNode {
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
        });

        let place = wv_core::distance::Place {
            id: Id::new_unchecked("qingyun"),
            name: "青云城".into(),
            map: Some(Id::new_unchecked("world_main")),
            x: Some(100.0),
            y: Some(200.0),
            w: None,
            h: None,
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
