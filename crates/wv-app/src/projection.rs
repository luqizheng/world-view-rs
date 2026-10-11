//! 投影与叠加层：事件点高亮（S8）、轨迹逐段生长（S8）、笔刷光标与视口网格（S4/S5）。
//!
//! ## 铁律（`AGENTS.md` §3.2 / 基线 §2.14）
//!
//! **渲染层禁止做时间比较。** 本模块从 [`crate::state::PlaybackState::current_day`]
//! 出发，**只调用 `TimelineQuery::at` / `Track::sample_at`** 求出投影，
//! 再把结果写进渲染用的组件/资源。渲染 System 读到的已是「本帧该画什么」。
//!
//! ## 轨迹逐段生长（`ANIMATION_SPEC` §1.2）
//!
//! `t <= p0.day` → 只画起点；`pi.day <= t < p(i+1).day` → `p0..pi` 全亮 + 部分生长；
//! `t >= pn.day` → 全亮。生长末端的「生长笔尖」用 `accent.cursor` 6px 圆点。

use bevy::prelude::*;

use crate::palette;
use crate::state::{BrushState, GridConfig, PlaybackState, ViewState, WorldProjection};
use wv_core::time::TimeSpan;
use wv_core::time::TimelineQuery;

/// 一帧的投影结果（事件高亮 / 轨迹位置），供渲染系统只读。
#[derive(Resource, Default)]
pub struct ProjectionFrame {
    /// 本帧「正在发生」的事件 ID 与它们的图内位置（若可定位）。
    pub active_events: Vec<(String, Vec2)>,
    /// 本帧每条轨迹的「已生长折线」点列（世界坐标）。
    pub track_polylines: Vec<Vec<Vec2>>,
    /// 本帧每条轨迹的「生长笔尖」位置。
    pub track_heads: Vec<Vec2>,
    /// 本帧轨迹「空心起点」标记（推算 / 粗轨迹用空心，动画 §1.3）。
    pub track_hollow_starts: Vec<Vec2>,
}

/// 计算本帧投影。**所有时间判断集中在这里**，且走 `wv-core` 的纯函数。
pub fn compute_projection(
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    projection: Res<WorldProjection>,
    mut frame: ResMut<ProjectionFrame>,
) {
    frame.active_events.clear();
    frame.track_polylines.clear();
    frame.track_heads.clear();
    frame.track_hollow_starts.clear();

    if !view.events_visible && !view.tracks_visible {
        return;
    }

    let world = &projection.world;
    let t = playback.current_day;

    // ---- 事件高亮：用 TimelineQuery::at 求「t 时刻正在发生的事件」----
    if view.events_visible {
        let spans: Vec<(TimeSpan, usize)> = world
            .events
            .iter()
            .enumerate()
            .map(|(i, e)| (e.span, i))
            .collect();
        for hit in TimelineQuery::at(&spans, t) {
            let event = &world.events[hit.1];
            // 事件位置：原型阶段尚未建立「地点 → 坐标」的解析（属 P2），
            // 因此统一记在世界原点；列表/高亮仍可用。坐标解析就绪后替换此处。
            frame
                .active_events
                .push((event.id.as_str().to_string(), Vec2::ZERO));
        }
    }

    // ---- 轨迹逐段生长 ----
    if view.tracks_visible {
        for track in &world.tracks {
            let poly = grown_polyline(track, t, &grid);
            if poly.len() >= 2 {
                frame.track_polylines.push(poly);
            }
            if let Some(head) = track.sample_at(t) {
                frame.track_heads.push(world_to_view(head.x, head.y, &grid));
            }
            // 空心起点：轨迹第一个有坐标的 waypoint 且其 day 为推算（None）或为粗轨迹。
            let hollow = track
                .waypoints
                .iter()
                .find_map(|w| w.pos)
                .map(|p| world_to_view(p.x, p.y, &grid));
            if let Some(h) = hollow {
                frame.track_hollow_starts.push(h);
            }
        }
    }
}

/// 图内局部坐标 → 视图世界坐标（基线 §2.15 变换链）。
fn world_to_view(x: f32, y: f32, grid: &GridConfig) -> Vec2 {
    Vec2::new(grid.origin.x + x, grid.origin.y + y)
}

/// 生成「已生长」折线（动画 §1.2）：已知生长位置之前全亮，当前段线性插值到笔尖。
fn grown_polyline(track: &wv_core::track::Track, day: i64, grid: &GridConfig) -> Vec<Vec2> {
    // 带时间戳且有坐标的锚点，按 day 升序。
    let mut anchors: Vec<(i64, Vec2)> = track
        .waypoints
        .iter()
        .filter_map(|w| w.day.zip(w.pos))
        .map(|(d, p)| (d, world_to_view(p.x, p.y, grid)))
        .collect();
    anchors.sort_by_key(|(d, _)| *d);
    if anchors.is_empty() {
        return Vec::new();
    }
    if day <= anchors[0].0 {
        return vec![anchors[0].1];
    }
    let mut out = Vec::new();
    out.push(anchors[0].1);
    for i in 0..anchors.len() - 1 {
        let (d0, p0) = anchors[i];
        let (d1, p1) = anchors[i + 1];
        if day >= d1 {
            out.push(p1);
        } else {
            // 部分生长。
            let u = (day - d0) as f32 / (d1 - d0).max(1) as f32;
            out.push(p0.lerp(p1, u));
            return out;
        }
    }
    out
}

/// 播放推进：每帧 `current_day += speed * dt`（视图状态推进，基线 §2.14 / 动画 §1.1）。
pub fn advance_playback(time: Res<Time>, mut playback: ResMut<PlaybackState>) {
    if !playback.playing {
        return;
    }
    let dt = time.delta_secs();
    playback.current_day += (playback.speed * dt) as i64;
    // 在范围内循环（播到头回绕，动画 §1.1）。
    if playback.current_day >= playback.range_to {
        playback.current_day = playback.range_from;
    }
}

/// 绘制视口网格与唯一的地形四边形（S3）。用 `Gizmos` 画（原型轻量方案）。
pub fn draw_map_gizmos(
    mut gizmos: Gizmos,
    tex: Res<crate::terrain_view::TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    frame: Res<ProjectionFrame>,
) {
    if !view.terrain_visible {
        return;
    }
    // 地形四边形边框（描线色，线宽 = TILE_BASE/16 的屏幕近似）。
    let size = tex.size as f32 * grid.cell_size;
    let min = grid.origin;
    let max = grid.origin + Vec2::splat(size);
    gizmos.rect_2d(
        Isometry2d::from_translation((min + max) * 0.5),
        Vec2::splat(size),
        palette::LINE_ON_MID,
    );

    // 事件高点亮：常驻底环 + 内芯（特效 §4.2：状态永不抑制）。
    if view.events_visible {
        for (_id, pos) in &frame.active_events {
            gizmos.circle_2d(
                Isometry2d::from_translation(*pos),
                grid.cell_size * 1.6,
                palette::ACCENT_EVENT.with_alpha(0.25),
            );
            gizmos.circle_2d(
                Isometry2d::from_translation(*pos),
                grid.cell_size * 0.7,
                palette::ACCENT_EVENT,
            );
        }
    }

    // 轨迹：已生长折线（`accent.event`）+ 生长笔尖（`accent.cursor`）。
    if view.tracks_visible {
        for poly in &frame.track_polylines {
            for w in poly.windows(2) {
                gizmos.line_2d(w[0], w[1], palette::ACCENT_EVENT);
            }
        }
        for head in &frame.track_heads {
            gizmos.circle_2d(
                Isometry2d::from_translation(*head),
                grid.cell_size * 0.75,
                palette::ACCENT_CURSOR,
            );
        }
        for s in &frame.track_hollow_starts {
            gizmos.circle_2d(
                Isometry2d::from_translation(*s),
                grid.cell_size * 0.6,
                palette::LINE_ON_LIGHT,
            );
        }
    }
}

/// 笔刷光标 falloff 双环（VFX §3.1：外环实线=影响半径，内环虚线=50% 强度半径）。
///
/// **属「状态」，永不抑制**（VFX §1.3 / 基线 §1.20）。光标用屏幕叠加，不写数据。
pub fn draw_brush_cursor(
    mut gizmos: Gizmos,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    grid: Res<GridConfig>,
) {
    if !brush.enabled {
        return;
    }
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(transform) = camera.get_single() else {
        return;
    };
    let world = crate::camera::screen_to_world(
        cursor,
        window.resolution.size(),
        transform.translation,
        view.scale,
    );

    // 外环：实线，半径 = 真实笔刷半径（误差 0）。
    gizmos.circle_2d(
        Isometry2d::from_translation(world),
        brush.radius,
        palette::ACCENT_SELECT,
    );
    // 内环：50% 强度半径 R * (1 - softness)，虚线（用短弧近似）。
    let inner = brush.radius * (1.0 - brush.softness).max(0.15);
    draw_dashed_circle(
        &mut gizmos,
        world,
        inner,
        palette::ACCENT_SELECT.with_alpha(0.6),
    );
    // 中心十字。
    let c = grid.cell_size * 0.5;
    gizmos.line_2d(
        world - Vec2::X * c,
        world + Vec2::X * c,
        palette::ACCENT_SELECT,
    );
    gizmos.line_2d(
        world - Vec2::Y * c,
        world + Vec2::Y * c,
        palette::ACCENT_SELECT,
    );
}

/// 用短弧拼出一圈虚线（原型阶段的虚线近似）。
fn draw_dashed_circle(gizmos: &mut Gizmos, center: Vec2, radius: f32, color: Color) {
    const SEGMENTS: usize = 48;
    for i in 0..SEGMENTS {
        if i % 2 == 1 {
            continue;
        }
        let a0 = (i as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        let a1 = ((i + 1) as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        let p0 = center + Vec2::new(a0.cos(), a0.sin()) * radius;
        let p1 = center + Vec2::new(a1.cos(), a1.sin()) * radius;
        gizmos.line_2d(p0, p1, color);
    }
}
