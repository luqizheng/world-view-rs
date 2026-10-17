//! 圈地界（事实地界）绘制层（theme overlay Task 7）。
//!
//! ## 职责
//! - [`commit_territory_sketch_click`]：左键落点 / 双击或回车收笔 / Esc 丢弃进行中折线。
//! - [`draw_territory_sketch`]：Gizmos 画进行中折线 + 顶点 + 已闭合待命名的环。
//!
//! ## 与 `measurement::commit_measurement_click` 的对照
//!
//! 共用同一条早退链（enabled 闸 → UI 指针 / 播放 / 缩放下限 → 无工程锁 →
//! `drill::map_origin` 同源链），差异只在收笔去向：测量直接提交命令，
//! 本工具把闭合环写入 [`TerritorySketch::pending`]，由应用层命名对话框
//! （`wv-app::ui::feature_place_dialog`）确认后才落成 `AddFeaturePlaceCommand`。
//!
//! ## 边界（不做什么）
//! - 不生成世界数据、不碰 `History`——那是 `wv-app::edit::feature_place_actions` 的事；
//! - 不持有对话框状态——`pending` 只表达「有一个环等着命名」；
//! - 不做时间比较——地界 span 的 `from` 由应用层从 `PlaybackState.current_day`
//!   取值并写入命令，本层只画会话态草稿。
//!
//! ## 坐标口径
//! 全部顶点是**当前图局部 wu**（与 `MeasureState::draft` 同口径）；落盘前由
//! `wv-edit` 原样写入 `GeometrySpan`，渲染时经 [`GridConfig::map_to_view`] 回到
//! 视图空间。子图继承父图底图时 `map_origin ≠ raster_origin`，这里必须用
//! `map_origin`（与测量 / 街道同源）。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{
    DrillState, GridConfig, PlaybackState, TerritorySketch, ViewState, WorldProjection,
};

/// 双击判定窗口（秒）与位置容差（世界单位，≈1.5 格）。
const DOUBLE_CLICK_SECS: f64 = 0.4;
const DOUBLE_CLICK_WU: f32 = 12.0;

/// 顶点半径（屏幕像素）——按 `view.scale` 反比换算成世界单位。
const VERTEX_PX: f32 = 3.0;

/// 进行中折线的颜色（白，与测量「未提交」同族）。
const ACTIVE_STROKE: Color = Color::WHITE;
/// 已闭合待命名环的颜色（森林绿——提示「这条要成为事实地界」）。
const PENDING_STROKE_RGB: (f32, f32, f32) = (0.42, 0.72, 0.42);

/// 左键 → 环顶点；双击 / 回车 / 右键收笔；Esc 丢弃当前折线。
///
/// 早退链与 `measurement::commit_measurement_click` 逐条同源：
/// 工具未开 → UI 指针 / 播放 / 过低缩放 → 无工程锁 → 光标与当前图缺一不可。
///
/// **`pending` 存在时不再接收输入**：命名对话框正在消费这个环，
/// 此时再落点 / 收笔只会制造第二个待命名环，先来后到被静默覆盖。
/// 对话框的取消路径负责把 `pending` 清掉。
#[allow(clippy::too_many_arguments)]
pub fn commit_territory_sketch_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    time: Res<Time>,
    mut sketch: ResMut<TerritorySketch>,
) {
    if !sketch.enabled {
        return;
    }
    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    // 无工程锁（`WorldProjection::has_open_project` 的同一判据）：无工程一律早退。
    if projection.project_dir.is_none() {
        return;
    }
    // 命名中：环已被对话框接管，本系统不接任何输入。
    if sketch.pending.is_some() {
        return;
    }

    // Esc：丢弃未闭合折线（已闭合待命名的环保留——那是对话框的东西）。
    if keys.just_pressed(KeyCode::Escape) {
        sketch.discard_active();
        sketch.last_click = None;
        return;
    }
    // 回车 / 右键：收笔（≥3 点才成环，失败清空折线）。
    if keys.just_pressed(KeyCode::Enter) || buttons.just_pressed(MouseButton::Right) {
        sketch.try_close();
        sketch.last_click = None;
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor_world) = view.cursor_world else {
        return;
    };
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let origin = crate::drill::map_origin(&projection.world, node, &grid);
    let local = cursor_world - origin;
    let p = (local.x, local.y);

    // 双击收笔：与上一落点近且间隔短——不追加点，直接成环。
    if let Some(((lx, ly), t)) = sketch.last_click {
        let dx = p.0 - lx;
        let dy = p.1 - ly;
        if (dx * dx + dy * dy).sqrt() <= DOUBLE_CLICK_WU
            && time.elapsed_secs_f64() - t <= DOUBLE_CLICK_SECS
        {
            sketch.try_close();
            sketch.last_click = None;
            return;
        }
    }
    sketch.add_point(p);
    sketch.last_click = Some((p, time.elapsed_secs_f64()));
}

/// 绘制进行中折线 + 顶点 + 已闭合待命名环。
///
/// Gizmos 即时线元没有每实体 Z；草稿本就在地形之上（与测量同层口径），
/// 不做 z 排序。`pending` 环画成闭合多边形（首尾相连），让作者在对话框
/// 出现时仍能看清自己圈的范围。
pub fn draw_territory_sketch(
    mut gizmos: Gizmos,
    sketch: Res<TerritorySketch>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
) {
    if sketch.pending.is_none() && sketch.active.is_none() {
        return;
    }
    let stroke = ACTIVE_STROKE.with_alpha(0.9);
    let closed = Color::srgb(
        PENDING_STROKE_RGB.0,
        PENDING_STROKE_RGB.1,
        PENDING_STROKE_RGB.2,
    )
    .with_alpha(0.95);
    let vertex_r = (VERTEX_PX * view.scale).max(0.5);

    if let Some(ring) = &sketch.pending {
        let pts: Vec<Vec2> = ring
            .iter()
            .map(|&(x, y)| grid.map_to_view(Vec2::new(x, y)))
            .collect();
        if pts.len() >= 2 {
            for w in 0..pts.len() {
                gizmos.line_2d(pts[w], pts[(w + 1) % pts.len()], closed);
            }
            for p in &pts {
                gizmos.circle_2d(Isometry2d::from_translation(*p), vertex_r, closed);
            }
        }
    }

    if let Some(active) = &sketch.active {
        let pts: Vec<Vec2> = active
            .iter()
            .map(|&(x, y)| grid.map_to_view(Vec2::new(x, y)))
            .collect();
        for w in pts.windows(2) {
            gizmos.line_2d(w[0], w[1], stroke);
        }
        for p in &pts {
            gizmos.circle_2d(Isometry2d::from_translation(*p), vertex_r, stroke);
        }
        // ≥3 点时提示「再点一下或回车即可闭合」：把首尾虚线补上。
        if pts.len() >= 3 {
            gizmos.line_2d(*pts.last().expect("非空"), pts[0], stroke.with_alpha(0.35));
        }
    }
}
