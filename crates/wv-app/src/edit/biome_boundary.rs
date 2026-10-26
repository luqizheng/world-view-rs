//! 群系分界线工具（2026-10-22 内容矩阵 §2.7 / ADR-0051）。
//!
//! ## 职责
//! - [`biome_boundary_input`] —— 创建 / 拖动 / 提交：工具激活时在地图上拖出
//!   线段（创建不做任何改写）；线存在后拖线身 = 平移、拖端点 = 旋转伸缩；
//!   松手 = `swept_changes` 算改写 → **1 条 `BatchEditCellsCommand`** 直提
//!   （+ `mark_dirty`，照 cell_form 直提路径）。
//! - 坐标：线端点 = **栅格局部 wu**（`cursor_world − raster_origin`，与
//!   `swept_changes` / `rasterize_polygon_cells` 同空间——两套原点陷阱见
//!   cell_selection 的坐标链注释）。

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::edit::editing::EditHistory;
use crate::state::{GridConfig, PlaybackState, ViewState, WorldProjection};

use wv_render::biome_boundary::{swept_changes, BoundaryLine};

/// 点到线段的最短距离（wu）。
fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (ax, ay) = a;
    let (bx, by) = b;
    let (px, py) = p;
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    if len2 <= f32::EPSILON {
        return ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
    }
    let t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// 拖动模式。
#[derive(Debug, Clone, Copy, PartialEq)]
enum DragMode {
    /// 拖出一条新线（创建**不做改写**——落线只是放工具，不动格子）。
    Creating,
    /// 拖线身 = 平移。
    Translate,
    /// 拖 a 端点。
    EndpointA,
    /// 拖 b 端点。
    EndpointB,
}

/// 分界线输入系统。
#[allow(clippy::too_many_arguments)]
pub fn biome_boundary_input(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    grid: Res<GridConfig>,
    playback: Res<PlaybackState>,
    ui: Res<wv_render::camera::PointerOverUi>,
    drill: Res<wv_render::state::DrillState>,
    mut projection: ResMut<WorldProjection>,
    mut edit: ResMut<EditHistory>,
    mut tex: ResMut<wv_render::terrain_view::TerrainTexture>,
    mut boundary: ResMut<wv_render::state::BiomeBoundaryState>,
) {
    use crate::state::ViewMode;
    if !boundary.tool_active
        || !matches!(view.mode, ViewMode::Topographic)
        || playback.playing
        || ui.wants_pointer
    {
        return;
    }
    let Some(map) = drill.current().cloned() else {
        return;
    };
    // 切图检测（final review I4）：线坐标是旧图的栅格局部 wu，带到新图会
    // 按新图地形提交出错误改写——图一变即清线（工具保持激活）。
    if boundary.map_id.as_ref() != Some(&map) {
        boundary.map_id = Some(map.clone());
        boundary.line = None;
        boundary.drag_from = None;
        boundary.drag_line = None;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };
    // 世界 → 栅格局部 wu（减栅格原点；见模块文档坐标链）。
    let local = (cursor - grid.raster_origin).into();
    let cs = grid.cell_size;

    // ── 按下：判定拖动模式 ──
    if buttons.just_pressed(MouseButton::Left) {
        let near = 0.6 * cs;
        if let Some(line) = boundary.line {
            let near_end = 0.6 * cs;
            let on_body = dist_to_segment(local, line.a, line.b) <= near * 1.5;
            // 三种拖动（端点 a / 端点 b / 线身）在按下时的动作一致：记起点 +
            // 存拖动基线；模式区分在松手时按按下点就近归属。
            if dist_to_segment(local, line.a, line.a) <= near_end
                || dist_to_segment(local, line.b, line.b) <= near_end
                || on_body
            {
                boundary.drag_from = Some(cursor);
                boundary.drag_line = Some(line);
            } else {
                // 点在空白：拖出新线（创建不改写格子）。
                boundary.drag_from = Some(cursor);
                boundary.drag_line = None;
            }
        } else {
            // 无线：拖出新线。
            boundary.drag_from = Some(cursor);
            boundary.drag_line = None;
        }
        return;
    }

    // ── 拖动中：更新预览线 ──
    if buttons.pressed(MouseButton::Left) {
        let Some(from) = boundary.drag_from else {
            return;
        };
        let from_local: (f32, f32) = (from - grid.raster_origin).into();
        let existing = boundary.line;
        // 模式判定：drag_line 为 None ⇒ Creating；否则按按下时离哪个特征近。
        let Some(base) = existing.or(boundary.drag_line) else {
            // Creating：预览线 = 起点 → 光标（两侧群系探测放松手时做）。
            boundary.drag_line = Some(BoundaryLine {
                a: from_local,
                b: local,
                side_a: default_side_a(),
                side_b: default_side_b(),
            });
            return;
        };
        // 平移 / 端点：按下点离哪个特征近。
        let da = dist_to_segment(from_local, base.a, base.a);
        let db = dist_to_segment(from_local, base.b, base.b);
        let d_body = dist_to_segment(from_local, base.a, base.b);
        let dx = local.0 - from_local.0;
        let dy = local.1 - from_local.1;
        let mut line = base;
        if da <= 0.6 * cs && da <= db {
            line.a = (base.a.0 + dx, base.a.1 + dy);
        } else if db <= 0.6 * cs {
            line.b = (base.b.0 + dx, base.b.1 + dy);
        } else if d_body <= 0.9 * cs {
            line.a = (base.a.0 + dx, base.a.1 + dy);
            line.b = (base.b.0 + dx, base.b.1 + dy);
        } else {
            return;
        }
        boundary.drag_line = Some(line);
        return;
    }

    // ── 松手：提交 ──
    if buttons.just_released(MouseButton::Left) {
        let Some(from) = boundary.drag_from.take() else {
            return;
        };
        let Some(new_line) = boundary.drag_line.take() else {
            return; // Creating 已在按下时记录为 None——松手 = 放线，不改格子
        };
        let from_local: (f32, f32) = (from - grid.raster_origin).into();
        // 模式判定必须对**按下时的基线**（boundary.line），不是拖动后的线——
        // 平移距离一超过 ~1 格，对拖后线量「按下点」会误判成 Creating，
        // 扫过改写被整体跳过（final review C2）。
        let base = boundary.line;
        let (da, db, d_body) = base
            .map(|b| {
                (
                    dist_to_segment(from_local, b.a, b.a),
                    dist_to_segment(from_local, b.b, b.b),
                    dist_to_segment(from_local, b.a, b.b),
                )
            })
            .unwrap_or((f32::MAX, f32::MAX, f32::MAX));
        let mode = if da <= 0.6 * cs && da <= db {
            DragMode::EndpointA
        } else if db <= 0.6 * cs {
            DragMode::EndpointB
        } else if d_body <= 0.9 * cs {
            DragMode::Translate
        } else {
            DragMode::Creating
        };

        match mode {
            DragMode::Creating => {
                // 创建：探测两侧群系（中点法线两侧各半格采样），失败给默认。
                let (sa, sb) = detect_sides(&new_line, &projection, &map, cs);
                let mut line = new_line;
                line.side_a = sa;
                line.side_b = sb;
                boundary.line = Some(line);
            }
            DragMode::Translate | DragMode::EndpointA | DragMode::EndpointB => {
                let Some(old) = boundary.line else {
                    boundary.line = Some(new_line);
                    return;
                };
                let Some(terrain) = projection.world.terrain_of(&map) else {
                    return;
                };
                let changes = swept_changes(&old, &new_line, terrain, grid.cell_size);
                if changes.is_empty() {
                    boundary.line = Some(new_line);
                    return;
                }
                use wv_edit::command::{BatchEditCellsCommand, CellChange};
                let cell_changes: Vec<CellChange> = changes
                    .into_iter()
                    .map(|(gx, gy, before, after)| CellChange {
                        gx,
                        gy,
                        before,
                        after,
                    })
                    .collect();
                let cmd = BatchEditCellsCommand::new(map.clone(), cell_changes.clone());
                let mut tx = wv_edit::history::Transaction::new();
                tx.push(Box::new(cmd));
                match edit.history.commit(&mut projection.world, tx) {
                    Ok(dirty) => {
                        edit.record_dirty(dirty);
                        projection.dirty = true;
                        // ADR-0070 D5（T9）：提交后重建推导边（不进脏路径）。
                        crate::edit::road_sync::sync_derived_road_edges(&mut projection.world);
                        for c in &cell_changes {
                            tex.mark_dirty([
                                c.gx.max(0) as u32,
                                c.gy.max(0) as u32,
                                c.gx as u32 + 1,
                                c.gy as u32 + 1,
                            ]);
                        }
                        boundary.line = Some(new_line);
                    }
                    Err(e) => bevy::log::warn!("分界线提交失败: {e}"),
                }
            }
        }
    }
}

/// 侧群系默认（探测失败时的兜底）：森林 | 草原。
fn default_side_a() -> u8 {
    wv_terrain::BIOME_FOREST
}
fn default_side_b() -> u8 {
    wv_terrain::BIOME_GRASSLAND
}

/// 创建落线时自动探测两侧群系：中点法线两侧各 0.6 格采样。
fn detect_sides(
    line: &BoundaryLine,
    projection: &WorldProjection,
    map: &wv_core::Id,
    cs: f32,
) -> (u8, u8) {
    use wv_terrain::{BIOME_FOREST, BIOME_GRASSLAND};
    let Some(terrain) = projection.world.terrain_of(map) else {
        return (BIOME_FOREST, BIOME_GRASSLAND);
    };
    let mid = ((line.a.0 + line.b.0) * 0.5, (line.a.1 + line.b.1) * 0.5);
    let (dx, dy) = (line.b.0 - line.a.0, line.b.1 - line.a.1);
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    // 法线（左）= (dy, -dx) / len。
    let (nx, ny) = (dy / len, -dx / len);
    let sample = |sx: f32, sy: f32| -> Option<u8> {
        let gx = (sx / cs).floor() as i32;
        let gy = (sy / cs).floor() as i32;
        if gx < 0 || gy < 0 || gx >= terrain.width as i32 || gy >= terrain.height as i32 {
            return None;
        }
        Some(terrain.cells[(gy as usize) * terrain.width as usize + gx as usize].biome)
    };
    let a = sample(mid.0 + nx * 0.6 * cs, mid.1 + ny * 0.6 * cs);
    let b = sample(mid.0 - nx * 0.6 * cs, mid.1 + -ny * 0.6 * cs);
    match (a, b) {
        (Some(x), Some(y)) if x != y => (x, y),
        _ => (BIOME_FOREST, BIOME_GRASSLAND),
    }
}
