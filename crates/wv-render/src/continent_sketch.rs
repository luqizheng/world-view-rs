//! 大陆生成的「意图包络」绘制层（2026-10-17 大陆生成 UI plan Task 3）。
//!
//! ## 职责
//! - [`commit_continent_sketch_click`]：左键落点 / 双击或回车收笔 / Esc 丢弃。
//! - [`draw_continent_sketch`]：Gizmos 画已闭合包络 + 进行中折线 + 顶点。
//!
//! ## 与 `measurement::commit_measurement_click` 的对照
//!
//! 共用同一条早退链（UI 指针 / 播放 / 缩放下限 / 无工程锁 / `map_origin` 同源链）。
//! 差异：包络点来自**多次左键**并保持世界单位局部坐标，收笔后才成为一条
//! 可交给 `GenerateContinentCommand` 的环。
//!
//! ## 边界
//! - 不生成地形、不碰 `History`——那是 wv-app 的任务 4 接线。
//! - 包络不落盘（spec §8：会话态；撤销时由 stash 回驻留）。

#![cfg(feature = "app")]

use bevy::prelude::*;

use crate::state::{
    ContinentSketch, DrillState, GridConfig, PlaybackState, ViewState, WorldProjection,
};

/// 双击判定窗口（秒）与位置容差（世界单位，≈1.5 格）。
const DOUBLE_CLICK_SECS: f64 = 0.4;
const DOUBLE_CLICK_WU: f32 = 12.0;

/// 顶点半径（屏幕像素）——按 `view.scale` 反比换算成世界单位。
const VERTEX_PX: f32 = 3.0;

/// 左键 → 包络点；双击 / 回车收笔；Esc / 右键丢弃当前折线。
#[allow(clippy::too_many_arguments)]
pub fn commit_continent_sketch_click(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    drill: Res<DrillState>,
    projection: Res<WorldProjection>,
    time: Res<Time>,
    mut sketch: ResMut<ContinentSketch>,
) {
    if !sketch.enabled {
        return;
    }
    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    // 无工程锁（AGENTS 值班判据 `has_open_project`）：无工程一律早退。
    if projection.project_dir.is_none() {
        return;
    }

    // Esc：丢弃未闭合折线（已闭合包络保留）。
    if keys.just_pressed(KeyCode::Escape) {
        sketch.discard_active();
        sketch.last_click = None;
        return;
    }
    // 回车 / 右键：收笔（≥3 点才成环）。
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

/// 绘制包络（已闭合）+ 进行中折线 + 顶点。
///
/// Gizmos 即时线元没有每实体 Z；包络本就在地形之上（与测量同层口径），
/// 不做 z 排序。
pub fn draw_continent_sketch(
    mut gizmos: Gizmos,
    sketch: Res<ContinentSketch>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
) {
    if sketch.envelopes.is_empty() && sketch.active.is_none() {
        return;
    }
    let stroke = Color::WHITE.with_alpha(0.9);
    let closed = Color::srgb(1.0, 0.78, 0.18).with_alpha(0.95);
    let vertex_r = (VERTEX_PX * view.scale).max(0.5);

    for poly in &sketch.envelopes {
        let pts: Vec<Vec2> = poly
            .iter()
            .map(|&(x, y)| grid.map_to_view(Vec2::new(x, y)))
            .collect();
        for w in 0..pts.len() {
            gizmos.line_2d(pts[w], pts[(w + 1) % pts.len()], closed);
        }
        for p in &pts {
            gizmos.circle_2d(Isometry2d::from_translation(*p), vertex_r, closed);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sketch_clicks_close_and_escape_discard_active() {
        let mut s = ContinentSketch {
            enabled: true,
            ..Default::default()
        };
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 0.0));
        s.add_point((10.0, 10.0));
        assert_eq!(
            s.active.as_ref().map(Vec::len),
            Some(3),
            "三次落点应形成三点折线"
        );
        assert!(s.try_close(), "≥3 点应能闭合");
        assert_eq!(s.envelopes.len(), 1, "闭合后应得到一条包络");
        assert!(s.active.is_none(), "闭合后折线应清空");

        // Esc 丢弃进行中的折线，不动已闭合包络。
        s.add_point((50.0, 50.0));
        s.add_point((60.0, 50.0));
        s.add_point((60.0, 60.0));
        s.discard_active();
        assert!(s.active.is_none(), "Esc 后折线应丢弃");
        assert_eq!(s.envelopes.len(), 1, "已闭合包络不得被 Esc 清掉");

        // <3 点收笔：不成环。
        s.add_point((0.0, 0.0));
        s.add_point((10.0, 10.0));
        assert!(!s.try_close(), "两点不应成环");
        assert!(s.active.is_none(), "失败的收笔也清掉折线");
        assert_eq!(s.envelopes.len(), 1, "不得增加包络");
    }
}
