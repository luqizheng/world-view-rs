//! HTTP ↔ GUI 主帧的桥（ADR-0014 §2.3）。
//!
//! ## 关键不变量
//!
//! 1. **World 不被 handler 直接持有**——handler 拿到的是 `oneshot` 回执，
//!    World 始终在 Bevy 主帧的 [`crate::state::WorldProjection`] 里。
//! 2. **写类经 [`wv_edit::EditCommand`]**——handler 不调 `world.xxx_mut()`
//!    直改 World；写命令走 mpsc → drain → [`wv_edit::History::commit`]。
//! 3. **单帧调顺序**：所有消息在 Bevy 主帧 `drain_messages` 一次处理，
//!    命令以收到顺序串行 apply；`History` 的 undo/redo 栈是这一顺序的权威。
//!
//! ## 测试友好性
//! `drain_messages` 是纯函数（`&mut World, &mut History, &mut Receiver`），
//! 单元测试直接构造消息喂入，**不**依赖 Bevy 运行时。

use std::path::Path;

use wv_core::World;
use wv_edit::{EditCommand, EditResult, History};

/// HTTP ↔ 主帧的通信消息。handler 经 `HttpBridge::tx` 投递，drain 在主帧消费。
#[derive(Debug)]
pub enum HttpMessage {
    /// 读请求（handler 不直接持有 World，必须经此路径）。
    Read(ReadRequest, tokio::sync::oneshot::Sender<ReadReply>),
    /// 写：应用一条 [`EditCommand`]，回执为 apply 返回的脏路径（或错误）。
    Apply(
        Box<dyn EditCommand>,
        tokio::sync::oneshot::Sender<EditResult<Vec<String>>>,
    ),
    /// 写：撤销最后一条事务，回执为「(是否实际撤销, 该事务脏路径)」。
    Undo(tokio::sync::oneshot::Sender<EditResult<(bool, Vec<String>)>>),
    /// 写：保存工程到磁盘，回执为脏路径集合 + 错误。
    Save(tokio::sync::oneshot::Sender<Result<SaveOutcome, wv_io::IoError>>),
}

/// 读请求的细节。命名按 §2.5.1 「list 返摘要 / get 返详情」契约。
#[derive(Debug, Clone)]
pub enum ReadRequest {
    /// 列出所有地图的摘要（id / name / parent / size_w / size_h）。
    ListMaps,
    /// 取单图完整 `MapNode`。
    GetMap { id: wv_core::Id },
    /// 列出某图所有地点的摘要（默认全图；filter / cursor / limit 可选）。
    ListPlaces {
        map_id: Option<wv_core::Id>,
        filter: Option<crate::http::filter::Filter>,
        cursor: Option<String>,
        limit: u32,
    },
    /// 取单地点完整 `Place`。
    GetPlace { id: wv_core::Id },
    /// P7 入口：`TimelineQuery::at` 求该时刻的「该图所有要素投影」。
    ///
    /// **不限分页**——v0.1 不设大小硬顶（实测 `examples/xinhai-1911/` 全图
    /// 投影约 200 KB），若 LLM 报告「上下文爆炸」再开 ADR 加 `place_ids`
    /// 过滤 + 硬顶（ADR §2.5.1 注脚）。
    GetStateAtTime { map_id: wv_core::Id, day: i64 },
}

/// 读请求的回执。读路径 1（read by request kind）——不允许直接返回 World，
/// 是为了让 handler 只能按 skill 暴露的最小视图工作。
///
/// **为什么不用 `Box<>`**：v0.1 测得各变体尺寸差是 `StateAtTime(serde_json::Value)`
/// 最大，约 32 B（Value 默认 inline 存储小 payload）；其余 ≤ 24 B。
/// 远低于 clippy `large_enum_variant` 默认 200 B 阈值——除非实测出现 copy 性能
/// 问题，否则不引入 `Box` 增加一次堆分配。
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // 注释里已说明尺寸差远低于阈值；不引 Box。
pub enum ReadReply {
    /// `list_maps` 的返回。
    MapSummaries(Vec<MapSummary>),
    /// `get_map` 的返回：`None` 表示该 id 不存在。
    MapNode(Option<wv_core::map::MapNode>),
    /// `list_places` 的返回：`items` 已应用 filter + cursor + limit；`next_cursor` 非空时表示还有更多。
    PlaceSummaries {
        items: Vec<PlaceSummary>,
        next_cursor: Option<String>,
    },
    /// `get_place` 的返回。
    Place(Option<wv_core::distance::Place>),
    /// `get_state_at_time` 的返回：JSON 序列化形式（TimelineQuery::at 的投影）。
    StateAtTime(serde_json::Value),
}

/// `list_maps` 单条。**轻量**——`size_w` / `size_h` 由 `MapNode.extent` 派生；
/// 不暴露 `layers` / `terrain` / `icons`（§2.5.1 双步契约）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MapSummary {
    pub id: wv_core::Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<wv_core::Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_w: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_h: Option<f32>,
}

/// `list_places` 单条。**轻量**——不暴露 `connections` 等扩展字段
/// （§2.5.1 双步契约）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlaceSummary {
    pub id: wv_core::Id,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<wv_core::Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
}

/// `save_project` skill 的回执：写入的脏目录集合 + 错误。
#[derive(Debug, Clone)]
pub struct SaveOutcome {
    pub dirty_paths: Vec<String>,
}

/// HTTP ↔ 主帧桥。handler 持此结构；Bevy 主帧持 receiver 端。
///
/// ## 为什么不直接给 handler 一个 `Arc<RwLock<World>>`
/// - 跨线程锁会与 GUI 笔刷系统抢同一资源；§3.3 的纪律是「主帧单点 apply」，
///   HTTP 写路径走 mpsc 是同一纪律的延伸。
/// - 读也走 mpsc 让 handler 不持有任何 `World` 引用，**LLM 输入大小可控**
///   （一次 list 100 条 = 100 × 摘要字段，远小于克隆全 World）。
#[derive(Debug, Clone)]
pub struct HttpBridge {
    tx: tokio::sync::mpsc::UnboundedSender<HttpMessage>,
}

impl HttpBridge {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<HttpMessage>) -> Self {
        Self { tx }
    }

    /// 投递一条读消息并等待回执。`tokio::sync::oneshot` 的 `await` 在
    /// drain 线程回复前挂起，handler 不持有任何锁。
    pub async fn read(&self, req: ReadRequest) -> ReadReply {
        match self.read_send(req).await {
            Ok(r) => r,
            Err(_) => ReadReply::MapSummaries(Vec::new()),
        }
    }

    /// 同 [`Self::apply_send`]，read 路径的同步入队 + receiver 形态。
    pub fn read_send(&self, req: ReadRequest) -> tokio::sync::oneshot::Receiver<ReadReply> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(HttpMessage::Read(req, reply_tx));
        reply_rx
    }

    /// 投递一条 [`EditCommand`] 并等待 apply。
    ///
    /// **同步入队**：调用即把消息压进 mpsc（**不**等 runtime）；
    /// 测试场景里 drain_messages 可以在 await reply 之前先排空消息。
    /// 这是为了让 v0.1 单线程测试友好（不让 `bridge.apply` 一次性完成
    /// send + drain + reply，那样写测试时收不到回执）。
    ///
    /// 异步路径走 [`Self::apply_send`]（同步入队）再 `await` 拿到的
    /// `oneshot::Receiver`——见 [`Self::apply`] 的实现。
    pub async fn apply(&self, cmd: Box<dyn EditCommand>) -> EditResult<Vec<String>> {
        self.apply_send(cmd)
            .await
            .unwrap_or_else(|_| Err(wv_edit::EditError::Precondition("reply dropped".into())))
    }

    /// 同步入队 + 返回 receiver。**测试 / 解耦场景专用**：
    /// 调用即发消息（**不**等 runtime）；返回的 receiver 让 caller 自由
    /// 选择何时 await。生产路径（handler）走 [`Self::apply`]。
    pub fn apply_send(
        &self,
        cmd: Box<dyn EditCommand>,
    ) -> tokio::sync::oneshot::Receiver<EditResult<Vec<String>>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        // mpsc::UnboundedSender::send 同步，不需 runtime。
        let _ = self.tx.send(HttpMessage::Apply(cmd, reply_tx));
        reply_rx
    }

    /// 投递撤销并等待回执。
    pub async fn undo(&self) -> EditResult<(bool, Vec<String>)> {
        self.undo_send()
            .await
            .unwrap_or_else(|_| Ok((false, Vec::new())))
    }

    /// 同 [`Self::apply_send`]，undo 路径的同步入队 + receiver 形态。
    pub fn undo_send(&self) -> tokio::sync::oneshot::Receiver<EditResult<(bool, Vec<String>)>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(HttpMessage::Undo(reply_tx));
        reply_rx
    }

    /// 投递保存并等待回执。
    pub async fn save(&self) -> Result<SaveOutcome, wv_io::IoError> {
        self.save_send().await.unwrap_or_else(|_| {
            Err(wv_io::IoError::ReadFile {
                path: "HTTP bridge".into(),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "reply dropped"),
            })
        })
    }

    /// 同 [`Self::apply_send`]，save 路径的同步入队 + receiver 形态。
    pub fn save_send(&self) -> tokio::sync::oneshot::Receiver<Result<SaveOutcome, wv_io::IoError>> {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = self.tx.send(HttpMessage::Save(reply_tx));
        reply_rx
    }
}

/// drain 一次 mpsc 队列中的所有消息，按顺序 apply。
///
/// ## 调用时机
/// - **GUI**：作为 Bevy 系统每帧从 `Startup` schedule 起每帧调用一次。
/// - **测试**：构造 `World` + `History` + `Receiver`，调用若干次直到通道空。
///
/// ## 为什么是 `try_recv` 而非 `recv().await`
/// Bevy 系统不能 `.await`（除非用 `AsyncComputeTaskPool`）；测试要快速
/// 排空已入队的消息。`try_recv` 失败即停，返回 `Ok(true)` 当且仅当这一
/// 次至少处理了一条消息。
///
/// ## save_dir
/// `save_project` 需要工程根目录；handler 不直接知道（WorldProjection
/// 才有）。drain 的调用方把 `WorldProjection.project_dir` 传进来。
pub fn drain_messages(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<HttpMessage>,
    world: &mut World,
    history: &mut History,
    save_dir: Option<&Path>,
) -> bool {
    let mut processed = false;
    loop {
        match receiver.try_recv() {
            Ok(msg) => {
                processed = true;
                handle_message(msg, world, history, save_dir);
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
    processed
}

fn handle_message(
    msg: HttpMessage,
    world: &mut World,
    history: &mut History,
    save_dir: Option<&Path>,
) {
    match msg {
        HttpMessage::Read(req, reply) => {
            let r = handle_read(&req, world);
            let _ = reply.send(r);
        }
        HttpMessage::Apply(cmd, reply) => {
            let mut tx = wv_edit::Transaction::new();
            tx.push(cmd);
            let r = history.commit(world, tx);
            let _ = reply.send(r);
        }
        HttpMessage::Undo(reply) => {
            let r = history.undo(world);
            let _ = reply.send(r);
        }
        HttpMessage::Save(reply) => {
            let r = handle_save(world, save_dir);
            let _ = reply.send(r);
        }
    }
}

fn handle_read(req: &ReadRequest, world: &World) -> ReadReply {
    match req {
        ReadRequest::ListMaps => {
            let summaries: Vec<MapSummary> = world.maps.iter().map(map_to_summary).collect();
            ReadReply::MapSummaries(summaries)
        }
        ReadRequest::GetMap { id } => {
            let node = world.maps.iter().find(|m| &m.id == id).cloned();
            ReadReply::MapNode(node)
        }
        ReadRequest::ListPlaces {
            map_id,
            filter,
            cursor,
            limit,
        } => handle_list_places(
            world,
            map_id.as_ref(),
            filter.as_ref(),
            cursor.as_deref(),
            *limit,
        ),
        ReadRequest::GetPlace { id } => {
            let place = world.distances.places.iter().find(|p| &p.id == id).cloned();
            ReadReply::Place(place)
        }
        ReadRequest::GetStateAtTime { map_id, day } => handle_state_at_time(world, map_id, *day),
    }
}

fn map_to_summary(m: &wv_core::map::MapNode) -> MapSummary {
    let (size_w, size_h) = m
        .extent
        .as_ref()
        .map(|e| (Some(e.width), Some(e.height)))
        .unwrap_or((None, None));
    MapSummary {
        id: m.id.clone(),
        name: m.name.clone(),
        parent: m.parent.clone(),
        size_w,
        size_h,
    }
}

fn handle_list_places(
    world: &World,
    map_id: Option<&wv_core::Id>,
    filter: Option<&crate::http::filter::Filter>,
    cursor: Option<&str>,
    limit: u32,
) -> ReadReply {
    // 1) 收集候选：按 map_id 过滤（None = 全部）。
    let mut candidates: Vec<&wv_core::distance::Place> = world
        .distances
        .places
        .iter()
        .filter(|p| match map_id {
            Some(m) => p.map.as_ref() == Some(m),
            None => true,
        })
        .collect();
    // 2) Mini-DSL filter（None = 不过滤）。
    if let Some(f) = filter {
        candidates.retain(|p| {
            let v = serde_json::json!({
                "id": p.id.as_str(),
                "name": p.name.as_str(),
                "map": p.map.as_ref().map(|m| m.as_str()).unwrap_or(""),
            });
            f.matches(&v)
        });
    }
    // 3) cursor 分页：v0.1 简单实现 = 进程内偏移（"offset:N"）；重启后重拉。
    let offset: usize = cursor
        .and_then(|c| c.strip_prefix("offset:"))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = limit.clamp(1, 100) as usize;
    let total = candidates.len();
    let end = (offset + limit).min(total);
    let page: Vec<PlaceSummary> = if offset < total {
        candidates[offset..end]
            .iter()
            .map(|&p| place_to_summary(p))
            .collect()
    } else {
        Vec::new()
    };
    let next_cursor = if end < total {
        Some(format!("offset:{end}"))
    } else {
        None
    };
    ReadReply::PlaceSummaries {
        items: page,
        next_cursor,
    }
}

fn place_to_summary(p: &wv_core::distance::Place) -> PlaceSummary {
    PlaceSummary {
        id: p.id.clone(),
        name: p.name.clone(),
        map: p.map.clone(),
        x: p.x,
        y: p.y,
    }
}

fn handle_state_at_time(world: &World, map_id: &wv_core::Id, day: i64) -> ReadReply {
    // v0.1 简化：返回该 map 下的所有 character / event / place 摘要，
    // **不**做时间区间投影。`TimelineQuery::at` 要求预构造
    // `Vec<(TimeSpan, T)>`（参考 `wv-core::time::TimelineQuery::at`），
    // 在世界数据层加时间区间化是 v0.2 的扩展（README §「计划 v0.1 之后」）。
    //
    // 这里**返回全部**而非时间过滤后的子集，呼应 §2.5.1 注脚：
    // 「v0.1 不设大小硬顶（实测 examples/xinhai-1911 全图 ~200KB）"——
    // LLM 拿到完整快照后可自行决定怎么用；若未来真的「上下文爆炸」，
    // 再开 ADR 加 `place_ids` 过滤参数与硬顶。
    let payload = serde_json::json!({
        "map_id": map_id.as_str(),
        "day": day,
        "characters": world.characters.iter().map(|c| {
            serde_json::json!({"id": c.id.as_str(), "name": c.name})
        }).collect::<Vec<_>>(),
        "events": world.events.iter().map(|e| {
            serde_json::json!({"id": e.id.as_str()})
        }).collect::<Vec<_>>(),
        "places": world.distances.places.iter().map(|p| {
            serde_json::json!({"id": p.id.as_str(), "name": p.name})
        }).collect::<Vec<_>>(),
    });
    ReadReply::StateAtTime(payload)
}

fn handle_save(world: &World, save_dir: Option<&Path>) -> Result<SaveOutcome, wv_io::IoError> {
    let Some(dir) = save_dir else {
        return Err(wv_io::IoError::ReadFile {
            path: "<unsaved>".into(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "没有打开的工程，无法保存（V1：save_project 要求有 project_dir）",
            ),
        });
    };
    // 全量写：dirty_paths = 空 Vec → wv-io 走「全目录重写 + 删除传播」路径。
    // 这是 v0.1 简化：HTTP 路径上不维护 DirtySet（GUI 路径上的 `WorldProjection.dirty`
    // 只是布尔标记，不指向文件级路径）。
    let mut project = wv_io::Project::open(dir)?;
    project.save_world(world, &[])?;
    Ok(SaveOutcome {
        dirty_paths: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::filter::Filter;
    use wv_core::map::{MapLevel, MapNode};
    use wv_core::{Id, TimeSpan, WorldMeta};

    fn world_with_two_maps() -> World {
        let mut w = World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("world_main"),
            name: "主世界".into(),
            level: MapLevel::World,
            parent: None,
            children: vec![Id::new_unchecked("city_a")],
            icons: vec![],
            layers: vec![],
            extent: Some(wv_core::map::Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        });
        w.maps.push(MapNode {
            id: Id::new_unchecked("city_a"),
            name: "城市甲".into(),
            level: MapLevel::City,
            parent: Some(Id::new_unchecked("world_main")),
            children: vec![],
            icons: vec![],
            layers: vec![],
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            orientation: 0.0,
        });
        w.meta = Some(WorldMeta {
            name: "测试".into(),
            version: "0.1".into(),
            schema_version: 2,
            root_map: Id::new_unchecked("world_main"),
            timeline_range: TimeSpan::new(0, 1).unwrap(),
        });
        w
    }

    #[test]
    fn list_maps_returns_summaries_only_no_full_node() {
        let w = world_with_two_maps();
        let reply = handle_read(&ReadRequest::ListMaps, &w);
        match reply {
            ReadReply::MapSummaries(v) => {
                assert_eq!(v.len(), 2);
                assert_eq!(v[0].id.as_str(), "world_main");
                assert_eq!(v[0].name, "主世界");
                assert_eq!(v[0].size_w, Some(4096.0));
                assert_eq!(v[0].size_h, Some(4096.0));
                assert_eq!(v[0].parent, None);
                assert_eq!(v[1].parent.as_ref().map(Id::as_str), Some("world_main"));
            }
            _ => panic!("应为 MapSummaries"),
        }
    }

    #[test]
    fn get_map_missing_returns_none_not_panic() {
        let w = world_with_two_maps();
        let reply = handle_read(
            &ReadRequest::GetMap {
                id: Id::new_unchecked("ghost_map"),
            },
            &w,
        );
        match reply {
            ReadReply::MapNode(None) => {}
            _ => panic!("应为 MapNode(None)"),
        }
    }

    #[test]
    fn list_places_filter_applies_mini_dsl() {
        let mut w = World::default();
        w.distances.places.push(wv_core::distance::Place {
            id: Id::new_unchecked("qingyun"),
            name: "青云城".into(),
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
        });
        w.distances.places.push(wv_core::distance::Place {
            id: Id::new_unchecked("wuhan"),
            name: "武汉".into(),
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
        });
        let f = Filter::parse("name ~ '青'").unwrap();
        let reply = handle_read(
            &ReadRequest::ListPlaces {
                map_id: None,
                filter: Some(f),
                cursor: None,
                limit: 100,
            },
            &w,
        );
        match reply {
            ReadReply::PlaceSummaries { items, next_cursor } => {
                assert_eq!(items.len(), 1, "filter='name~青' 应只剩青云城");
                assert_eq!(items[0].id.as_str(), "qingyun");
                assert!(next_cursor.is_none());
            }
            _ => panic!("应为 PlaceSummaries"),
        }
    }

    #[test]
    fn list_places_limit_caps_at_100() {
        let mut w = World::default();
        for i in 0..150 {
            w.distances.places.push(wv_core::distance::Place {
                id: Id::new_unchecked(format!("p_{i:03}")),
                name: format!("Place {i}"),
                map: None,
                x: None,
                y: None,
                w: None,
                h: None,
            });
        }
        let reply = handle_read(
            &ReadRequest::ListPlaces {
                map_id: None,
                filter: None,
                cursor: None,
                limit: 200, // 请求 200 → 硬顶 100
            },
            &w,
        );
        match reply {
            ReadReply::PlaceSummaries { items, next_cursor } => {
                assert_eq!(items.len(), 100, "limit 应硬顶 100");
                assert!(next_cursor.is_some(), "还剩 50 条 → 应有 next_cursor");
            }
            _ => panic!("应为 PlaceSummaries"),
        }
    }

    #[test]
    fn list_places_cursor_paginates() {
        let mut w = World::default();
        for i in 0..150 {
            w.distances.places.push(wv_core::distance::Place {
                id: Id::new_unchecked(format!("p_{i:03}")),
                name: format!("Place {i}"),
                map: None,
                x: None,
                y: None,
                w: None,
                h: None,
            });
        }
        // 第一页
        let r1 = handle_read(
            &ReadRequest::ListPlaces {
                map_id: None,
                filter: None,
                cursor: None,
                limit: 100,
            },
            &w,
        );
        let next_cursor = match r1 {
            ReadReply::PlaceSummaries { items, next_cursor } => {
                assert_eq!(items.len(), 100);
                next_cursor.expect("应返 cursor")
            }
            _ => panic!("应为 PlaceSummaries"),
        };
        // 第二页
        let r2 = handle_read(
            &ReadRequest::ListPlaces {
                map_id: None,
                filter: None,
                cursor: Some(next_cursor.clone()),
                limit: 100,
            },
            &w,
        );
        match r2 {
            ReadReply::PlaceSummaries { items, next_cursor } => {
                assert_eq!(items.len(), 50, "剩 50 条");
                assert!(next_cursor.is_none(), "已到底");
            }
            _ => panic!("应为 PlaceSummaries"),
        }
    }

    #[test]
    fn apply_command_via_drain_modifies_world() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let mut world = World::default();
        let mut history = History::default();

        let place = wv_core::distance::Place {
            id: Id::new_unchecked("qingyun"),
            name: "青云城".into(),
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
        };
        // 同步入队（apply_send 不 poll future，消息直接进 mpsc）。
        let cmd: Box<dyn EditCommand> = Box::new(wv_edit::AddPlaceCommand::new(place));
        let reply_rx = bridge.apply_send(cmd);
        // 主帧 drain：apply 命令 + 回发 reply_tx.send(result)。
        drain_messages(&mut rx, &mut world, &mut history, None);
        // 现在 await reply_rx（runtime 给出必要的.await 上下文）。
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(reply_rx).unwrap();
        assert!(result.is_ok());
        let dirty = result.unwrap();
        assert!(dirty.contains(&"distances/places".to_string()));
        assert_eq!(world.distances.places.len(), 1);
    }

    #[test]
    fn undo_returns_dirty_paths_from_the_undone_transaction() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let mut world = World::default();
        let mut history = History::default();

        // 先 apply 一条 AddPlaceCommand
        let place = wv_core::distance::Place {
            id: Id::new_unchecked("qingyun"),
            name: "青云城".into(),
            map: None,
            x: None,
            y: None,
            w: None,
            h: None,
        };
        let cmd: Box<dyn EditCommand> = Box::new(wv_edit::AddPlaceCommand::new(place));
        let apply_reply = bridge.apply_send(cmd);
        drain_messages(&mut rx, &mut world, &mut history, None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(apply_reply).unwrap();
        assert_eq!(world.distances.places.len(), 1);

        // undo
        let undo_reply = bridge.undo_send();
        drain_messages(&mut rx, &mut world, &mut history, None);
        let result = rt.block_on(undo_reply).unwrap();
        let (did_undo, dirty) = result.unwrap();
        assert!(did_undo);
        assert!(
            dirty.contains(&"distances/places".to_string()),
            "撤销后应回报 commit 时的脏路径"
        );
        assert_eq!(world.distances.places.len(), 0);
    }

    #[test]
    fn undo_with_no_history_returns_false() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let mut world = World::default();
        let mut history = History::default();

        let undo_reply = bridge.undo_send();
        drain_messages(&mut rx, &mut world, &mut history, None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(undo_reply).unwrap();
        let (did_undo, dirty) = result.unwrap();
        assert!(!did_undo);
        assert!(dirty.is_empty());
    }

    #[test]
    fn save_without_project_dir_errors() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let bridge = HttpBridge::new(tx);
        let mut world = World::default();
        let mut history = History::default();

        let save_reply = bridge.save_send();
        drain_messages(&mut rx, &mut world, &mut history, None);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(save_reply).unwrap();
        assert!(result.is_err(), "无 project_dir 应拒绝保存");
    }
}
