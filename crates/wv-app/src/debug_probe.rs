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
//! 家族现状（ADR-0088 Task 4 增列）：`WV_STRIP`（Themed 加工链剥离位域，
//! 启动读一次）与 `WV_SCALE`（钉死相机缩放——对比帧采集的同机位锚，等
//! `WV_DRILL` 钻入过渡结束后每帧幂等写回）。
//!
//! ## 边界
//!
//! 只在环境变量存在时动作：正常启动多一次 `var_os` 查找，零渲染开销、
//! 不写盘、不改任何默认行为。

use bevy::prelude::*;
use wv_render::state::{DrillState, ViewState};

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

/// `WV_STRIP=<0-31>`：Themed 加工链剥离探针（ADR-0088 Task 4），启动读一次。
///
/// 位语义真源 = `wv_world_cell_map::terrain_view::field::material` 的
/// `STRIP_BIT_*` 常量（Rust 镜像 fn + 真值表测试在那边；WGSL 同步解码）：
/// **位置 1 = 剥掉该层**——bit0 色板 / bit1 soften（保留位）/ bit2 wash /
/// bit3 veil / bit4 浮雕；**0 = 全开 = 恒等**。掩码经场桥接写
/// `FieldParams.pad1.x` 未用槽，只改 uniform、永不触发重打。
///
/// 非法输入（非数字 / > 31）告警并忽略——探针不得把异常输入变成静默的
/// 「全剥」画面，那会让对比帧结论整个作废。
pub fn apply_env_strip(mut view: ResMut<ViewState>) {
    let Ok(raw) = std::env::var("WV_STRIP") else {
        return;
    };
    match raw.trim().parse::<u32>() {
        Ok(mask) if mask <= 31 => {
            view.strip_mask = mask;
            info!("[wv] WV_STRIP → strip_mask = {mask}（0 = 全开）");
        }
        _ => warn!("[wv] WV_STRIP={raw} 非法（需 0-31），忽略"),
    }
}

/// `WV_SCALE=<f32>`：钉死相机缩放（世界单位/屏幕像素）——对比帧采集的
/// **同机位锚**（ADR-0088 Task 4）。与 `WV_DRILL` 配套：钻入过渡
/// （`drill_transition`）每帧写 `view.scale`，本探针必须等过渡结束
/// （`transition_from` 归 `None`）才接管，否则两个系统同帧写同一投影、
/// 谁后写谁赢。接管后**每帧幂等写回**（只动缩放，中心保持钻入落点）：
/// 过渡结束的下一帧即落位，不与 `WV_SHOT` 的第 120 帧赛跑。
///
/// 未设置即永久退出（`Some(0.0)` 哨兵，与 [`apply_env_drill`] 同款），缺省零开销。
pub fn apply_env_scale(
    mut wanted: Local<Option<f32>>,
    drill: Res<DrillState>,
    mut view: ResMut<ViewState>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    if wanted.is_none() {
        let parsed = std::env::var("WV_SCALE")
            .ok()
            .and_then(|raw| raw.trim().parse::<f32>().ok())
            .filter(|s| s.is_finite() && *s > 0.0);
        if std::env::var_os("WV_SCALE").is_some() && parsed.is_none() {
            warn!("[wv] WV_SCALE 非法（需正数，世界单位/屏幕像素），忽略");
        }
        // 哨兵：Some(0.0) = 已读、未设置/非法 → 永久退出。
        *wanted = Some(parsed.unwrap_or(0.0));
    }
    let scale = *wanted.as_ref().expect("上面已填哨兵");
    if scale <= 0.0 || drill.transition_from.is_some() {
        return; // 未设置永久退出 / 钻入过渡进行中让位（见函数文档）
    }
    view.scale = scale;
    wv_render::camera::apply_view_to_camera(view.camera_center, scale, &mut camera);
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
