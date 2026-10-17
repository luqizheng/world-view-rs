//! 投影与叠加层：事件点高亮（S8）、轨迹逐段生长（S8）、笔刷光标与视口网格（S4/S5）。
//!
//! ## 职责
//! - 求值「某时刻的世界状态」：从 `PlaybackState::current_day` 出发，经
//!   `wv_core::time::TimelineQuery::at` 把时间相关的世界片段投影成渲染就绪数据。
//! - 维护轨迹逐段生长（ANIMATION_SPEC §1.2）：起点全亮 + 当前段部分生长，
//!   末端是 `accent.cursor` 6px 圆点。
//! - 把相机网格、笔刷光标、POI 焦点圆等**瞬态高亮**画到 terrain_view 之上。
//!
//! ## 边界（不做什么）
//! - **不写时间比较**——所有时间求值只能走 `TimelineQuery::at` / `Track::sample_at`
//!   （AGENTS §3.2 / 基线 §2.14），System 不准自行 `if time > event.time`。
//! - **不修改世界**——只读 `WorldProjection.world`；改动仍走 `wv-edit` 的 `History`。
//! - **不画地形底色**——那是 [`crate::terrain_view`]；本模块只在上层叠加高亮。
//!
//! ## 主要导出
//! - `ProjectionFrame` — 帧快照（geo ↔ 屏幕相机矩阵）。
//! - `apply_projection_refresh` — 每帧把 `ProjectionFrame` 推给渲染系统。
//! - `PlaceDot` / `PlaceDotKind` — POI 红点组件（含可见性标记）。

use bevy::prelude::*;

use crate::layer_frame::LayerFrame;
use crate::state::{BrushState, GridConfig, PlaybackState, ViewState, WorldProjection};
use crate::theme::colors;
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
    /// 本帧道路层（矢量叠加）：每条地点边 `(PlaceEdge)` 的一段线（世界坐标端点对）。
    ///
    /// 只画**两端都锚定在当前地图**的边（`Place.map == 当前图 id` 且 `x/y` 齐备），
    /// 与 [`crate::drill::resolve_markers`] 的「只画当前层级」口径一致。
    pub road_segments: Vec<(Vec2, Vec2)>,
    /// 本帧地点（POI）点标（世界坐标），锚定在当前地图的地点。
    ///
    /// `is_city` = 该地点声明了矩形范围（`w/h` 齐备，如城市）；无范围的点标
    /// （如关隘、渡口）为 `false`。国家级视野（[`crate::state::LodTier::Nation`]）
    /// 只画 `is_city == true` 的点标——Google 地图式「低层级只画高级要素」。
    pub place_dots: Vec<PlaceDot>,
}

/// 一个 POI 点标的渲染数据。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaceDot {
    /// 视图空间坐标（本图局部经 [`GridConfig::map_to_view`] 换算而来）。
    pub pos: Vec2,
    /// 是否为「城市级」地点（`Place.w/h` 齐备）。
    pub is_city: bool,
}

/// 计算本帧投影。**所有时间判断集中在这里**，且走 `wv-core` 的纯函数。
///
/// 图层可见性读 [`LayerFrame`]（ADR-0013：YAML 声明 ∧ 会话开关的合取投影），
/// 不直接读 `ViewState` 的布尔——轨迹无对应 `LayerKind`，仍走会话开关。
pub fn compute_projection(
    playback: Res<PlaybackState>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    drill: Res<crate::state::DrillState>,
    projection: Res<WorldProjection>,
    layers: Res<LayerFrame>,
    mut frame: ResMut<ProjectionFrame>,
) {
    frame.active_events.clear();
    frame.track_polylines.clear();
    frame.track_heads.clear();
    frame.track_hollow_starts.clear();
    frame.road_segments.clear();
    frame.place_dots.clear();

    if !layers.event_point.visible
        && !view.tracks_visible
        && !layers.road.visible
        && !layers.place.visible
    {
        return;
    }

    let world = &projection.world;
    let t = playback.current_day;

    // ---- 事件高亮：用 TimelineQuery::at 求「t 时刻正在发生的事件」----
    if layers.event_point.visible {
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
            let poly = grown_polyline(track, t);
            if poly.len() >= 2 {
                frame.track_polylines.push(poly);
            }
            if let Some(head) = track.sample_at(t) {
                frame.track_heads.push(root_to_view(head.x, head.y));
            }
            // 空心起点：轨迹第一个有坐标的 waypoint 且其 day 为推算（None）或为粗轨迹。
            let hollow = track
                .waypoints
                .iter()
                .find_map(|w| w.pos)
                .map(|p| root_to_view(p.x, p.y));
            if let Some(h) = hollow {
                frame.track_hollow_starts.push(h);
            }
        }
    }

    // ---- 道路层：地点边（纯几何叠加，无时间语义，不涉 P7）----
    if layers.road.visible || layers.place.visible {
        if let Some(current) = drill.current_node(world) {
            // 只取锚定在当前地图、且坐标齐备的地点（与标记层「只画当前层级」同口径）。
            // 地点坐标是**本图局部**坐标 → `grid.map_to_view`（不是栅格原点）。
            let find_place = |id: &wv_core::id::Id| -> Option<Vec2> {
                let p = world
                    .distances
                    .places
                    .iter()
                    .find(|p| &p.id == id && p.map.as_ref() == Some(&current.id))?;
                Some(grid.map_to_view(Vec2::new(p.x?, p.y?)))
            };
            if layers.road.visible {
                for edge in &world.distances.edges {
                    if let (Some(a), Some(b)) = (find_place(&edge.a), find_place(&edge.b)) {
                        frame.road_segments.push((a, b));
                    }
                }
            }
            // POI 点标：当前地图上的全部锚定地点；带「是否城市级」标记供 LOD 筛选。
            if layers.place.visible {
                for p in &world.distances.places {
                    if p.map.as_ref() == Some(&current.id) {
                        if let (Some(x), Some(y)) = (p.x, p.y) {
                            frame.place_dots.push(PlaceDot {
                                pos: grid.map_to_view(Vec2::new(x, y)),
                                is_city: p.w.is_some() && p.h.is_some(),
                            });
                        }
                    }
                }
            }
        }
    }
}

/// **根图帧**（世界）坐标 → 视图空间。
///
/// 视图空间**就是**根图坐标系（基线 §2.15：相机、`ViewState::cursor_world`、
/// `drill_target` 全在根帧），所以这里是恒等映射；保留函数是为了把「这份数据
/// 属于根帧」写在调用处，别让读者把它和随手一个 `Vec2::new` 混起来。
/// 本图局部坐标（地点 / 街道）走 [`GridConfig::map_to_view`]。
///
/// 待决 §12-26：轨迹 waypoint 与事项事件的坐标**今天一律按根帧（世界）解释**，
/// 数据里没有「这份坐标属于哪张图」的声明。世界级事件/轨迹因此天然正确；若将来
/// 允许事件或轨迹按子图局部坐标书写，必须先加所属帧字段，再改这里——收敛点是
/// 本函数与 `grown_polyline`，改动面可控。
fn root_to_view(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// 生成「已生长」折线（动画 §1.2）：已知生长位置之前全亮，当前段线性插值到笔尖。
fn grown_polyline(track: &wv_core::track::Track, day: i64) -> Vec<Vec2> {
    // 带时间戳且有坐标的锚点，按 day 升序。
    let mut anchors: Vec<(i64, Vec2)> = track
        .waypoints
        .iter()
        .filter_map(|w| w.day.zip(w.pos))
        .map(|(d, p)| (d, root_to_view(p.x, p.y)))
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
            // 部分生长：进度归一化走 wv-core 纯函数（day 空间插值，动画 §5.1），
            // 渲染层不自行比较时间点（AGENTS §3.2）。`d1 == d0`（同天锚点）→ 0.0。
            let u = TimelineQuery::day_progress(day, d0, (d1 - d0) as f32);
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
    // 先加小数进位再取整：60fps 下每帧增量不足 1 天，直接截断会永远推不动。
    let add = playback.speed * dt + playback.carry;
    let whole = add as i64;
    playback.carry = add - whole as f32;
    playback.current_day += whole;
    // 在范围内循环（播到头回绕，动画 §1.1）。
    if playback.current_day >= playback.range_to {
        playback.current_day = playback.range_from;
    }
}

/// 绘制视口网格与唯一的地形四边形（S3）。用 `Gizmos` 画（原型轻量方案）。
///
/// 事件高亮读 [`LayerFrame`]（ADR-0013）：visible 门 + opacity 乘进 alpha。
/// 事件属 `Gizmos` 即时线元，**没有每实体 Z**——`EventPoint` 的 `z` 偏移
/// 在 Gizmos 路径不生效（ADR-0013 待裁项）。
#[allow(clippy::too_many_arguments)]
pub fn draw_map_gizmos(
    mut gizmos: Gizmos,
    tex: Res<crate::terrain_view::TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    frame: Res<ProjectionFrame>,
    layers: Res<LayerFrame>,
) {
    let _ = &*tex; // suppress unused warning
    if false {
        if !view.terrain_visible {
            return;
        }
        // 地图轮廓：边框用反白描线（深底上可见——LINE_ON_MID 是给浅底地形用的，
        // 在深色空白工程上等于隐形），并补 1/4 幅面参考网格，避免「一片黑看不到图在哪」。
        // 画的是**当前图**的幅面（与 `map_frame::sync_map_frame` 同口径，现由它接替）。
        let size = grid.map_size.x.max(grid.map_size.y);
        let min = grid.map_origin;
        let max = grid.map_origin + Vec2::splat(size);
        gizmos.rect_2d(
            Isometry2d::from_translation((min + max) * 0.5),
            Vec2::splat(size),
            crate::theme::palette::colors(view.map_palette).line_on_dark,
        );
        let step = size / 4.0;
        for i in 1..4 {
            let x = min.x + step * i as f32;
            gizmos.line_2d(
                Vec2::new(x, min.y),
                Vec2::new(x, max.y),
                crate::theme::palette::colors(view.map_palette)
                    .line_on_dark
                    .with_alpha(0.35),
            );
            let y = min.y + step * i as f32;
            gizmos.line_2d(
                Vec2::new(min.x, y),
                Vec2::new(max.x, y),
                crate::theme::palette::colors(view.map_palette)
                    .line_on_dark
                    .with_alpha(0.35),
            );
        }
    }

    // 道路：地点边直线（在事件/轨迹之下，视觉上先铺路网再叠动态要素）。
    //
    // **已迁出 Gizmos**：`Gizmos` 线宽是屏幕 1px 常量，放大后变粗、出现锯齿。
    // 现由 [`crate::vector_layers::sync_vector_layers`] 写到世界空间 Mesh2d，
    // 线宽随 `view.scale` 反比缩放（屏幕恒定 1px）。`if false` 保留原实现
    // 作为回退开关——若新实现出问题，把 `false` 改 `true` 即可恢复。
    if false && view.roads_visible {
        for (a, b) in &frame.road_segments {
            gizmos.line_2d(*a, *b, colors::ACCENT_NOTE.with_alpha(0.9));
        }
    }

    // 地点（POI）点标：白圈 + 深芯（导航地图惯例；不占用任何强调色的语义）。
    //
    // **已迁出 Gizmos**：同上路层理由（屏幕 1px 描线 + 抗锯齿圆）。
    // 现由 [`crate::vector_layers::sync_vector_layers`] 写 Circle / Ring mesh。
    if false && view.places_visible {
        for dot in &frame.place_dots {
            let r = grid.cell_size * 0.9;
            gizmos.circle_2d(
                Isometry2d::from_translation(dot.pos),
                r,
                crate::theme::palette::colors(view.map_palette).line_on_dark,
            );
            gizmos.circle_2d(
                Isometry2d::from_translation(dot.pos),
                r * 0.5,
                crate::theme::palette::colors(view.map_palette).line_on_mid,
            );
        }
    }

    // 事件高点亮：常驻底环 + 内芯（特效 §4.2：状态永不抑制）。
    // 图层门 + 透明度走 LayerFrame（ADR-0013）；Gizmos 无逐实体 Z，z 偏移不适用。
    if layers.event_point.visible {
        let alpha = layers.event_point.opacity;
        for (_id, pos) in &frame.active_events {
            gizmos.circle_2d(
                Isometry2d::from_translation(*pos),
                grid.cell_size * 1.6,
                colors::ACCENT_EVENT.with_alpha(0.25 * alpha),
            );
            gizmos.circle_2d(
                Isometry2d::from_translation(*pos),
                grid.cell_size * 0.7,
                colors::ACCENT_EVENT.with_alpha(alpha),
            );
        }
    }

    // 轨迹：已生长折线（`accent.event`）+ 生长笔尖（`accent.cursor`）。
    if view.tracks_visible {
        for poly in &frame.track_polylines {
            for w in poly.windows(2) {
                gizmos.line_2d(w[0], w[1], colors::ACCENT_EVENT);
            }
        }
        for head in &frame.track_heads {
            gizmos.circle_2d(
                Isometry2d::from_translation(*head),
                grid.cell_size * 0.75,
                colors::ACCENT_CURSOR,
            );
        }
        for s in &frame.track_hollow_starts {
            gizmos.circle_2d(
                Isometry2d::from_translation(*s),
                grid.cell_size * 0.6,
                crate::theme::palette::colors(view.map_palette).line_on_light,
            );
        }
    }
}

/// 笔刷光标 falloff 双环（VFX §3.1：外环实线=影响半径，内环虚线=50% 强度半径）。
///
/// **属「状态」，永不抑制**（VFX §1.3 / 基线 §1.20）。光标用屏幕叠加，不写数据。
///
/// **只在世界级显示**（v0.2 §B：城市级/建筑级不显示笔刷光标）——下面那级没有地形
/// 网格，光标画在那儿只会让人以为可以涂抹。
///
/// **已迁出 Gizmos**：原实现用 `Gizmos` + 48 段短弧拼虚线，**段长世界空间
/// 固定、相机放大后虚线变长、变实线**。现由 [`crate::brush_cursor::sync_brush_cursor`]
/// 写 Mesh2d 实体（外环 `Annulus` + 虚线 24 段 `Rectangle` + 十字），屏幕 1px/3px
/// 粗细由 `view.scale` 反比缩放保持恒定。`if false` 保留原实现作回退。
pub fn draw_brush_cursor(
    mut gizmos: Gizmos,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    grid: Res<GridConfig>,
    drill: Res<crate::state::DrillState>,
) {
    if false {
        if !brush.enabled || drill.depth() > 0 {
            return;
        }
        let Ok(window) = windows.single() else {
            return;
        };
        let Some(cursor) = window.cursor_position() else {
            return;
        };
        let Ok(transform) = camera.single() else {
            return;
        };
        let world =
            crate::camera::screen_to_world(cursor, window.resolution.size(), transform, view.scale);

        // 外环：实线，半径 = 真实笔刷半径（误差 0）。
        gizmos.circle_2d(
            Isometry2d::from_translation(world),
            brush.radius,
            colors::ACCENT_SELECT,
        );
        // 内环：50% 强度半径 R * (1 - softness)，虚线（用短弧近似）。
        let inner = brush.radius * (1.0 - brush.softness).max(0.15);
        draw_dashed_circle(
            &mut gizmos,
            world,
            inner,
            colors::ACCENT_SELECT.with_alpha(0.6),
        );
        // 中心十字。
        let c = grid.cell_size * 0.5;
        gizmos.line_2d(
            world - Vec2::X * c,
            world + Vec2::X * c,
            colors::ACCENT_SELECT,
        );
        gizmos.line_2d(
            world - Vec2::Y * c,
            world + Vec2::Y * c,
            colors::ACCENT_SELECT,
        );
    } // 关闭 `if false` 回退开关。
}

/// 用短弧拼出一圈虚线（原型阶段的虚线近似）。
///
/// **仅供 `draw_brush_cursor` 的 `if false` 回退分支调用**（[Gizmos] 旧实现）。
/// `#[allow(dead_code)]` 防止被 clippy 误报——Gizmos 路径平时不走，但保留作为
/// 一键回退。
#[allow(dead_code)]
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
