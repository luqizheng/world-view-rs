//! UI 操控 4 工具（ADR-0078）：`get_ui_state` / `set_view_mode` /
//! `set_panel_visible` / `screenshot`。
//!
//! ## 与世界数据工具的分界
//! 读/写类 skill（`read.rs` / `write.rs`）面向 `wv_core::World`；本模块面向
//! **UI 会话态**（`wv_render::state::ViewState` + `crate::state::PanelState`）。
//! 两者走同一条 mpsc 桥（`HttpMessage::Ui`），UI 状态同样只在主帧读写。
//!
//! ## 词表单一真源（ADR-0078 D6）
//! 模式词表与 `WV_MODE` 验收探针（`crate::debug_probe`）**同源**——实现收口
//! 在 `crate::state::parse_view_mode`（探针随 app feature 编译、不在 http 门内，
//! 故真源放 state），本模块只引用。panel 词表从 `Panel::ALL`（用户可见 12 项）
//! 派生：枚举里多出的 3 个变体（CellEditor / Faction / Situation）是 Inspector
//! 页签，不是独立浮窗，暴露出来只会造成「点了没反应」的死项。
//!
//! ## 测试友好性（承 ADR-0014）
//! 核心逻辑是纯函数 [`handle_ui_request`]（`&mut ViewState, &mut PanelState`
//! → 回执），不依赖 Bevy 运行时；`Screenshot` 是唯一例外——它要 spawn
//! Bevy 组件，由 `http_integration::begin_screenshot` 处理，本函数对其返回
//! `None`（调用方先截走该变体）。

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::http::bridge::{PanelVis, UiReply, UiRequest, UiStateSnapshot};
use crate::http::mcp::JsonRpcError;
use crate::http::tools::ToolOutput;
use crate::state::{parse_view_mode, view_mode_name, Panel};
use wv_render::state::ViewState;

/// screenshot 回执的兜底等待。渲染器正常 1–2 帧即落盘；窗口最小化 /
/// 后台挂起时可能长期无帧——超时明确报错，不让调用方吊死。
const SHOT_TIMEOUT_SECS: u64 = 10;

// ---------------------------------------------------------------------------
// 工具 schema（tools/list）
// ---------------------------------------------------------------------------

/// UI 操控 4 工具的 schema。`tools/list` 直接喂给 LLM（ADR-0078 §2）。
pub fn ui_tool_schemas() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "get_ui_state",
            "取当前 UI 快照：视图模式（browse/map/cellmap/faction/realterrain/event/coastline）、当前图 id、全部用户可见面板的显隐。操控前的定位调用。",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            }),
        ),
        (
            "set_view_mode",
            "切换视图模式（等价点顶栏）：browse=矢量地形、map=普通地图（专业制图）、cellmap=CellMap 工作区、faction=势力图、realterrain、event、coastline。返回切换后的 UI 快照。",
            json!({
                "type": "object",
                "required": ["mode"],
                "properties": {
                    "mode": {"type": "string", "description": "browse|map|cellmap|faction|realterrain|event|coastline"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "set_panel_visible",
            "开关一个用户可见面板（等价面板菜单勾选）。panel 词表：map_tree / layers / minimap / brush / timeline / event / character / region / landmark / inspector / validation / line_poi。返回切换后的 UI 快照。",
            json!({
                "type": "object",
                "required": ["panel", "visible"],
                "properties": {
                    "panel": {"type": "string", "description": "面板名（snake_case）"},
                    "visible": {"type": "boolean", "description": "true=开 false=关"}
                },
                "additionalProperties": false,
            }),
        ),
        (
            "screenshot",
            "截取当前主窗口画面（含全部面板叠加），以 MCP image content（base64 PNG）回传；同时落盘 PNG。save_to 可指定落盘路径（缺省落系统临时目录 wv-screenshots/）。GUI 最小化时无帧产出，10s 超时报错。",
            json!({
                "type": "object",
                "properties": {
                    "save_to": {"type": "string", "description": "可选：PNG 落盘路径（非 .png 结尾会自动补扩展名）"}
                },
                "additionalProperties": false,
            }),
        ),
    ]
}

// ---------------------------------------------------------------------------
// 词表（单一真源）
// ---------------------------------------------------------------------------
//
// 视图模式词表（`parse_view_mode` / `view_mode_name`）在 `crate::state`
// ——见模块注释「词表单一真源」。本节只有 panel 词表（仅 HTTP 消费）。

/// `Panel::ALL` 成员的 snake_case 词。枚举里多出的 3 个页签变体
/// （CellEditor / Faction / Situation）**不在** [`Panel::ALL`]，故永不经
/// 本表暴露——但 match 仍覆盖全枚举，编译器迫使新变体补词。
pub fn panel_word(p: Panel) -> &'static str {
    match p {
        Panel::MapTree => "map_tree",
        Panel::Layers => "layers",
        Panel::Brush => "brush",
        Panel::Inspector => "inspector",
        Panel::Timeline => "timeline",
        Panel::Faction => "faction",
        Panel::Region => "region",
        Panel::Situation => "situation",
        Panel::Validation => "validation",
        Panel::Minimap => "minimap",
        Panel::Character => "character",
        Panel::Event => "event",
        Panel::CellEditor => "cell_editor",
        Panel::LinePoi => "line_poi",
        Panel::Landmark => "landmark",
    }
}

/// 用户可见面板词表（= [`Panel::ALL`] 顺序，菜单同序）。
pub fn panel_vocab() -> Vec<&'static str> {
    Panel::ALL.iter().map(|p| panel_word(*p)).collect()
}

/// 词 → [`Panel`]。宽容匹配：规范词（`map_tree`）、枚举名小写（`maptree`）
/// 都收——LLM 偶尔会直接抄枚举名，拒掉徒增一轮对话。
pub fn parse_panel(raw: &str) -> Option<Panel> {
    let norm = raw.trim().to_ascii_lowercase();
    Panel::ALL
        .iter()
        .copied()
        .find(|p| panel_word(*p) == norm || format!("{p:?}").to_ascii_lowercase() == norm)
}

// ---------------------------------------------------------------------------
// 快照与纯 handler
// ---------------------------------------------------------------------------

/// 从视图态 + 面板态取快照（ADR-0078 §2.1）。
pub fn ui_snapshot(view: &ViewState, panels: &crate::state::PanelState) -> UiStateSnapshot {
    UiStateSnapshot {
        view_mode: view_mode_name(view.mode).to_string(),
        current_map: view.current_map.clone(),
        panels: Panel::ALL
            .iter()
            .map(|p| PanelVis {
                panel: panel_word(*p).to_string(),
                visible: panels.is_visible(*p),
            })
            .collect(),
    }
}

/// UI 请求的纯函数 handler（`Screenshot` 除外——见模块注释）。
///
/// 返回 `None` 当且仅当 `req` 是 `Screenshot`（调用方必须先截走它交给
/// `http_integration::begin_screenshot`；误入本函数的 Screenshot 拿不到
/// 回执，由调用方的 `unwrap_or_else` 兜底成 Error）。
pub fn handle_ui_request(
    req: UiRequest,
    view: &mut ViewState,
    panels: &mut crate::state::PanelState,
) -> Option<UiReply> {
    match req {
        UiRequest::GetState => Some(UiReply::State(ui_snapshot(view, panels))),
        UiRequest::SetViewMode { mode } => match parse_view_mode(&mode) {
            Some(m) => {
                // 从 CellMap 工作区切走：还原面板快照（2026-10-28 旁路泄漏
                // 根修——与顶栏/快捷键/命令面板同一退出入口；palette 传
                // None，EditCell 工具关闭由用户下一次切工具时的互斥全关兜底）。
                if m != wv_render::state::ViewMode::Topographic {
                    crate::ui_command_palette::exit_cellmap_workspace(panels, None);
                }
                view.mode = m;
                Some(UiReply::State(ui_snapshot(view, panels)))
            }
            None => Some(UiReply::Error(format!(
                "未知视图模式 `{mode}`；合法词表：browse / map / cellmap / faction / realterrain / event / coastline"
            ))),
        },
        UiRequest::SetPanelVisible { panel, visible } => match parse_panel(&panel) {
            Some(p) => {
                panels.set_visible(p, visible);
                Some(UiReply::State(ui_snapshot(view, panels)))
            }
            None => Some(UiReply::Error(format!(
                "未知面板 `{panel}`；合法词表：{}",
                panel_vocab().join(" / ")
            ))),
        },
        UiRequest::Screenshot { .. } => None,
    }
}

// ---------------------------------------------------------------------------
// 截图落盘辅助（纯函数）
// ---------------------------------------------------------------------------

/// 解析 `screenshot` 的落盘路径。`save_to` 给了就用它（父目录必须已存在，
/// 非 .png 结尾自动补）；缺省 = OS 临时目录 `wv-screenshots/wv-shot-<毫秒>.png`
/// （目录不存在则建）。纯路径决策 + 一次建目录，可单测。
pub fn shot_path(save_to: Option<&str>) -> Result<PathBuf, String> {
    match save_to {
        Some(raw) => {
            let mut p = PathBuf::from(raw);
            if p.extension().is_none() {
                p.set_extension("png");
            }
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    return Err(format!("save_to 的父目录不存在：{}", parent.display()));
                }
            }
            Ok(p)
        }
        None => {
            let dir = std::env::temp_dir().join("wv-screenshots");
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("创建截图目录 {} 失败：{e}", dir.display()))?;
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            Ok(dir.join(format!("wv-shot-{ms}.png")))
        }
    }
}

/// PNG 宽高：从 IHDR（固定偏移 16..24，大端）解析。不引图片解码依赖——
/// 回执只给调用方一个尺寸参照，缩放裁剪是待裁项（spec §8）。
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIG {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((w, h))
}

/// base64 编码（无 `data:` 前缀，MCP image content 直接可用）。
pub fn encode_base64(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

// ---------------------------------------------------------------------------
// dispatch（handler 侧）
// ---------------------------------------------------------------------------

/// UI 操控 skill 的入口（`tools::dispatch` 路由到此）。
///
/// 与 read/write handler 的差别：回执是 [`ToolOutput`]（screenshot 产
/// `Image`），且 screenshot 的 `await` 带 [`SHOT_TIMEOUT_SECS`] 超时——
/// oneshot 的回执端在 Bevy 主帧的轮询系统手里。
pub async fn dispatch_ui(
    bridge: &crate::http::bridge::HttpBridge,
    name: &str,
    args: Value,
) -> Result<ToolOutput, JsonRpcError> {
    let req = match name {
        "get_ui_state" => UiRequest::GetState,
        "set_view_mode" => {
            #[derive(serde::Deserialize)]
            struct In {
                mode: String,
            }
            let inp: In = serde_json::from_value(args).map_err(|e| {
                JsonRpcError::invalid_params(format!("set_view_mode 参数非法: {e}"))
            })?;
            UiRequest::SetViewMode { mode: inp.mode }
        }
        "set_panel_visible" => {
            #[derive(serde::Deserialize)]
            struct In {
                panel: String,
                visible: bool,
            }
            let inp: In = serde_json::from_value(args).map_err(|e| {
                JsonRpcError::invalid_params(format!("set_panel_visible 参数非法: {e}"))
            })?;
            UiRequest::SetPanelVisible {
                panel: inp.panel,
                visible: inp.visible,
            }
        }
        "screenshot" => {
            #[derive(serde::Deserialize)]
            struct In {
                save_to: Option<String>,
            }
            let inp: In = serde_json::from_value(args)
                .map_err(|e| JsonRpcError::invalid_params(format!("screenshot 参数非法: {e}")))?;
            UiRequest::Screenshot {
                save_to: inp.save_to,
            }
        }
        other => return Err(JsonRpcError::method_not_found(other)),
    };

    let rx = bridge.ui_send(req);
    let reply = if name == "screenshot" {
        match tokio::time::timeout(std::time::Duration::from_secs(SHOT_TIMEOUT_SECS), rx).await {
            Ok(r) => r.unwrap_or_else(|_| UiReply::Error("截图回执通道关闭（GUI 退出？）".into())),
            Err(_) => UiReply::Error(format!(
                "截图超时（{SHOT_TIMEOUT_SECS}s）——窗口是否最小化或后台挂起？"
            )),
        }
    } else {
        rx.await
            .unwrap_or_else(|_| UiReply::Error("UI 回执通道关闭（GUI 退出？）".into()))
    };

    match reply {
        UiReply::State(s) => Ok(ToolOutput::Json(
            serde_json::to_value(&s).unwrap_or_else(|e| json!({"serialize_error": e.to_string()})),
        )),
        UiReply::Shot(p) => Ok(ToolOutput::Image {
            data_base64: p.base64_png,
            mime: "image/png",
            meta: json!({
                "saved_path": p.saved_path,
                "width": p.width,
                "height": p.height,
            }),
        }),
        UiReply::Error(msg) => Err(JsonRpcError::internal_error(msg)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PanelState;
    use wv_render::state::ViewMode;

    fn default_view() -> ViewState {
        ViewState::default()
    }

    // -- 词表：模式 --------------------------------------------------------

    #[test]
    fn parse_view_mode_covers_wv_mode_vocabulary_and_beyond() {
        // WV_MODE 探针的四组同源词
        assert_eq!(parse_view_mode("browse"), Some(ViewMode::VectorFlat));
        assert_eq!(parse_view_mode("vector_flat"), Some(ViewMode::VectorFlat));
        assert_eq!(parse_view_mode("map"), Some(ViewMode::GoogleMap));
        assert_eq!(parse_view_mode("cartographic"), Some(ViewMode::GoogleMap));
        assert_eq!(parse_view_mode("cellmap"), Some(ViewMode::Topographic));
        assert_eq!(parse_view_mode("topographic"), Some(ViewMode::Topographic));
        assert_eq!(parse_view_mode("faction"), Some(ViewMode::Faction));
        // 本轮新增的三模式
        assert_eq!(parse_view_mode("realterrain"), Some(ViewMode::RealTerrain));
        assert_eq!(parse_view_mode("real_terrain"), Some(ViewMode::RealTerrain));
        assert_eq!(parse_view_mode("event"), Some(ViewMode::Event));
        assert_eq!(parse_view_mode("coastline"), Some(ViewMode::Coastline));
        // 宽容：大小写与首尾空白
        assert_eq!(parse_view_mode("  CellMap "), Some(ViewMode::Topographic));
        assert_eq!(parse_view_mode("放肆"), None);
    }

    #[test]
    fn view_mode_name_round_trips_through_parse() {
        for m in ViewMode::ALL {
            let name = view_mode_name(m);
            assert_eq!(
                parse_view_mode(name),
                Some(m),
                "规范词 {name} 必须能解析回 {m:?}"
            );
        }
    }

    // -- 词表：面板 --------------------------------------------------------

    #[test]
    fn panel_vocab_matches_panel_all_exactly() {
        assert_eq!(panel_vocab().len(), Panel::ALL.len());
        assert!(panel_vocab().contains(&"brush"));
        assert!(panel_vocab().contains(&"map_tree"));
        assert!(panel_vocab().contains(&"line_poi"));
        // 页签变体不进词表（点了没反应的死项，ADR-0078 D6）
        assert!(!panel_vocab().contains(&"cell_editor"));
        assert!(!panel_vocab().contains(&"faction"));
        assert!(!panel_vocab().contains(&"situation"));
    }

    #[test]
    fn parse_panel_accepts_word_and_enum_name_and_rejects_unknown() {
        assert_eq!(parse_panel("map_tree"), Some(Panel::MapTree));
        assert_eq!(parse_panel("MapTree"), Some(Panel::MapTree));
        assert_eq!(parse_panel(" brush "), Some(Panel::Brush));
        assert_eq!(parse_panel("line_poi"), Some(Panel::LinePoi));
        // 页签变体即使词对也不收——它们不是可开关浮窗
        assert_eq!(parse_panel("cell_editor"), None);
        assert_eq!(parse_panel("不存在的面板"), None);
    }

    // -- 快照与 handler ----------------------------------------------------

    #[test]
    fn handle_ui_request_get_state_reports_all_visible_panels() {
        let mut view = default_view();
        let mut panels = PanelState::default();
        panels.set_visible(Panel::Brush, true);
        let reply = handle_ui_request(UiRequest::GetState, &mut view, &mut panels)
            .expect("GetState 必有回执");
        match reply {
            UiReply::State(s) => {
                assert_eq!(s.view_mode, "browse", "默认模式 = browse");
                assert_eq!(s.panels.len(), Panel::ALL.len(), "快照应列全量面板");
                let brush = s.panels.iter().find(|p| p.panel == "brush").unwrap();
                assert!(brush.visible);
                let layers = s.panels.iter().find(|p| p.panel == "layers").unwrap();
                assert!(!layers.visible);
            }
            other => panic!("应为 State，got {other:?}"),
        }
    }

    #[test]
    fn handle_ui_request_set_view_mode_mutates_and_echoes() {
        let mut view = default_view();
        let mut panels = PanelState::default();
        let reply = handle_ui_request(
            UiRequest::SetViewMode {
                mode: "cellmap".into(),
            },
            &mut view,
            &mut panels,
        )
        .expect("合法词必有回执");
        match reply {
            UiReply::State(s) => assert_eq!(s.view_mode, "cellmap", "echo 新快照"),
            other => panic!("应为 State，got {other:?}"),
        }
        assert_eq!(view.mode, ViewMode::Topographic, "视图态应被改写");
    }

    #[test]
    fn handle_ui_request_set_view_mode_unknown_word_errors_without_mutation() {
        let mut view = default_view();
        let before = view.mode;
        let mut panels = PanelState::default();
        let reply = handle_ui_request(
            UiRequest::SetViewMode {
                mode: "hologram".into(),
            },
            &mut view,
            &mut panels,
        )
        .expect("未知词也应有回执");
        match reply {
            UiReply::Error(msg) => {
                assert!(msg.contains("合法词表"), "错误要带词表：{msg}");
            }
            other => panic!("应为 Error，got {other:?}"),
        }
        assert_eq!(view.mode, before, "失败不得改写视图态");
    }

    #[test]
    fn handle_ui_request_set_panel_visible_round_trip() {
        let mut view = default_view();
        let mut panels = PanelState::default();
        for (word, want) in [("brush", false), ("map_tree", true), ("brush", true)] {
            let reply = handle_ui_request(
                UiRequest::SetPanelVisible {
                    panel: word.into(),
                    visible: want,
                },
                &mut view,
                &mut panels,
            )
            .expect("合法词必有回执");
            let UiReply::State(s) = reply else {
                panic!("应为 State");
            };
            let got = s.panels.iter().find(|p| p.panel == word).unwrap();
            assert_eq!(got.visible, want, "{word} 应为 {want}");
            assert_eq!(
                panels.is_visible(parse_panel(word).unwrap()),
                want,
                "面板态应被改写"
            );
        }
    }

    #[test]
    fn handle_ui_request_screenshot_returns_none_for_bevy_side() {
        let mut view = default_view();
        let mut panels = PanelState::default();
        assert!(handle_ui_request(
            UiRequest::Screenshot { save_to: None },
            &mut view,
            &mut panels,
        )
        .is_none());
    }

    // -- 截图辅助 ----------------------------------------------------------

    #[test]
    fn shot_path_rejects_missing_parent_and_appends_png_extension() {
        let err = shot_path(Some("Z:/不存在的目录奶/shot.png")).unwrap_err();
        assert!(err.contains("父目录不存在"), "raw = {err}");
        let p = shot_path(Some("./shot_target")).unwrap();
        assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
    }

    #[test]
    fn shot_path_default_lands_in_temp_wv_screenshots() {
        let p = shot_path(None).unwrap();
        assert!(
            p.to_string_lossy().contains("wv-screenshots"),
            "缺省应落 wv-screenshots 目录：{}",
            p.display()
        );
        assert!(p.to_string_lossy().contains("wv-shot-"));
    }

    /// 手工拼最小 PNG 头（签名 + IHDR 长度/类型 + 宽高），驱动 [`png_dimensions`]
    /// 的字节偏移契约——不靠真截图（那要 GPU）。
    #[test]
    fn png_dimensions_parses_ihdr_width_height() {
        let mut b = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        b.extend_from_slice(&[0, 0, 0, 13]); // IHDR 长度
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&1920u32.to_be_bytes());
        b.extend_from_slice(&1080u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]); // 位深/色型/压缩/滤波/隔行
        assert_eq!(png_dimensions(&b), Some((1920, 1080)));
        // 签名错 / 太短 → None
        assert_eq!(png_dimensions(&b[1..]), None);
        assert_eq!(png_dimensions(&b[..20]), None);
    }

    #[test]
    fn encode_base64_produces_decodable_text() {
        let s = encode_base64(b"hello");
        assert_eq!(s, "aGVsbG8=");
    }
}
