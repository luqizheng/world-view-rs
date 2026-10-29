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
use wv_render::state::ViewState;

/// `WV_MODE` → 启动即切模式（一次）。
///
/// 跑在 `Startup` 且排在 `setup_terrain_texture_after_world` 之前：首帧烘焙就
/// 用目标模式的着色器，避免「先画一帧水彩再跳专业制图」的闪动。
pub fn apply_env_view_mode(mut view: ResMut<ViewState>) {
    let Ok(raw) = std::env::var("WV_MODE") else {
        return;
    };
    // 词表单一真源在 `crate::state::parse_view_mode`（ADR-0078 D6）：HTTP
    // `set_view_mode` 工具与本探针共用同一映射，不会各说各话。
    let Some(mode) = crate::state::parse_view_mode(&raw) else {
        warn!(
            "[wv] WV_MODE={raw} 未知（browse/map/cellmap/faction/realterrain/event/coastline），忽略"
        );
        return;
    };
    view.mode = mode;
    info!("[wv] WV_MODE → {mode:?}");
    // `WV_FIELD=1`：启动即开 GPU 场渲染（`ViewState::field_render`）。默认
    // **true 已转正**（2026-10-26 用户实机验收后拍板，SPEC 追记十四）——本
    // 探针留作显式钉死路径用（老工程脚本 / A/B 对照时防止误关）。关闭走命令
    // 面板「显示地形场渲染（GPU）」；是否真的上场由 wv-render 的
    // `should_render_field` 按模式路由（ADR-0088），本开关只是意愿位。
    // 与 WV_MODE 同族的验收探针——两条底图管线 A/B 对照、场管线的截图验收
    // 都必须能从命令行钉死路径，缺省零开销。
    if std::env::var_os("WV_FIELD").is_some() {
        view.field_render = true;
        info!("[wv] WV_FIELD → field_render = true（GPU 场管线）");
    }
}

/// `WV_DRILL=<map_id>`：工程加载完成后直接钻到指定图（一次）。
///
/// 城市级街名 / 标签摆位的实机验收要求「开机即在城市图」——双击钻入图标
/// 的手势没法从命令行模拟，这是与 [`apply_env_view_mode`] 同族的图级探针。
/// 轮询到目标图进入 `WorldProjection` 才动手（异步加载竞态），成败都只试
/// 一次；`WV_DRILL` 未设置时首帧即永久退出，零开销。
pub fn apply_env_drill(
    mut state: Local<Option<String>>,
    projection: Res<wv_render::state::WorldProjection>,
    mut drill: ResMut<wv_render::state::DrillState>,
) {
    if state.is_none() {
        *state = Some(
            std::env::var("WV_DRILL")
                .map(|raw| raw.trim().to_string())
                .unwrap_or_default(),
        );
    }
    let Some(target) = state.as_deref() else {
        return;
    };
    if target.is_empty() {
        return; // 未设置：永久退出（state 保留空串哨兵）
    }
    let id = wv_core::id::Id::new_unchecked(target);
    if !projection.world.maps.iter().any(|m| m.id == id) {
        return; // 世界还没加载完（后台线程），下一帧再看
    }
    if drill.drill_to(&projection.world, &id) {
        info!("[wv] WV_DRILL → {target}");
    } else {
        warn!("[wv] WV_DRILL={target} drill_to 失败（悬空图？），忽略");
    }
    *state = Some(String::new());
}
