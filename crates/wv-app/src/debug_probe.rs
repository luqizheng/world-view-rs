//! 实机验收探针（环境变量门控，缺省零开销）。
//!
//! ## 为什么存在
//!
//! 专业制图（ADR-0061）的验收必须**看画面**：色板、海岸线、群系面、国境线、
//! POI 分类色都是视觉契约，单测只能钉住「接线对不对」。但 GUI 默认落
//! `Browse`（`VectorFlat`），自动化截图点不了顶栏按钮——所以需要
//! `WV_MODE` 代替那一下点击。
//!
//! 截图本身已有全局探针：`ui::widget_preview::screenshot_probe`（`WV_SHOT`
//! 环境变量，第 120 帧落盘）。本模块**不重复注册**截图系统，只补模式切换。
//!
//! ## 用法
//!
//! ```text
//! WV_MODE=map WV_SHOT=D:\shot.png wv gui -m examples/jrpg-world
//! ```
//!
//! ## 边界
//!
//! 只在环境变量存在时动作：正常启动多一次 `var_os` 查找，零渲染开销、
//! 不写盘、不改任何默认行为。

use bevy::prelude::*;
use wv_render::state::{ViewMode, ViewState};

/// `WV_MODE` → 启动即切模式（一次）。
///
/// 跑在 `Startup` 且排在 `setup_terrain_texture_after_world` 之前：首帧烘焙就
/// 用目标模式的着色器，避免「先画一帧水彩再跳专业制图」的闪动。
pub fn apply_env_view_mode(mut view: ResMut<ViewState>) {
    let Ok(raw) = std::env::var("WV_MODE") else {
        return;
    };
    let mode = match raw.trim().to_ascii_lowercase().as_str() {
        "map" | "google" | "cartographic" => ViewMode::GoogleMap,
        "browse" | "vector" | "vector_flat" => ViewMode::VectorFlat,
        "cellmap" | "topographic" => ViewMode::Topographic,
        "faction" => ViewMode::Faction,
        other => {
            warn!("[wv] WV_MODE={other} 未知（map/browse/cellmap/faction），忽略");
            return;
        }
    };
    view.mode = mode;
    info!("[wv] WV_MODE → {mode:?}");
    // `WV_FIELD=1`：启动即开 GPU 场渲染（`ViewState::field_render`，默认 false 走
    // chunk 烘焙路径）。与 WV_MODE 同族的验收探针——两条底图管线 A/B 对照、
    // 场管线的截图验收都必须能从命令行钉死路径，缺省零开销。
    if std::env::var_os("WV_FIELD").is_some() {
        view.field_render = true;
        info!("[wv] WV_FIELD → field_render = true（GPU 场管线）");
    }
}
