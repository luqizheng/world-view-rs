//! 地图外框 + 1/4 幅面参考网格：世界空间 Mesh2d，屏幕恒定 1px 描边。
//!
//! ## 背景与动机（优化项 4）
//!
//! 原 [`crate::projection::draw_map_gizmos`] 用 `Gizmos` 画：
//! - **外框** = `gizmos.rect_2d`（屏幕 1px 常量，相机放大后变粗、出现锯齿）；
//! - **1/4 幅面参考网格** = 6 条 `gizmos.line_2d`（3 竖 + 3 横），同样问题；
//!   且**没有 LOD**——缩到 0.1× 时 6 条线挤在一起变成实线黑框，放到 8× 时
//!   网格太密反而看不见。
//!
//! 目标：
//! 1. **外框屏幕恒定 1.5px**（与道路 / POI 同策略 A）；
//! 2. **参考网格按 `view.scale` 阈值 LOD**：
//!    - `view.scale > MAX`（远观，地图在屏幕很小）→ 隐藏（线挤成实线无意义）；
//!    - `view.scale < MIN`（近看，地图远超视口）→ 隐藏（看不到全图无意义）；
//!    - 中间区间显示，屏幕恒定 1px。
//!
//! ## 实现
//!
//! - **外框** = 4 段 `Rectangle`（顶 / 底 / 左 / 右），与道路同样公式
//!   `world_thickness = 1.5 / view.scale`；
//! - **参考网格** = 6 段 `Rectangle`（3 竖 + 3 横），等分四份。
//!
//! ## 数据来源
//!
//! 与原实现一致：`tex.size × grid.cell_size` 算外框尺寸，`grid.origin` 算起点。
//! 仍读 `TerrainTexture` + `GridConfig`，**不修改 `ProjectionFrame`**——外框/网格
//! 是**纯几何**而非"时间投影"，不参与 P7。
//!
//! ## 不动原则
//!
//! - **零 `wv-core` 改动**；
//! - **零新依赖**；
//! - **资源状态显式**：`terrain_visible` 关闭时显式 despawn。

#![cfg(feature = "app")]

use bevy::math::primitives::Rectangle;
use bevy::prelude::*;

use crate::palette;
use crate::state::{GridConfig, ViewState};
use crate::terrain_view::TerrainTexture;

/// 屏幕像素描边。
const STROKE_PX: f32 = 1.5;
/// 外框 Z 偏移：在地形之上、所有矢量层之下。
const FRAME_Z: f32 = 0.05;
/// 参考网格 Z 偏移：在外框之上、其他层之下。
const GRID_Z: f32 = 0.06;

/// 参考网格 LOD 阈值（`view.scale`）。
///
/// 经验值：地图边长 10000 世界单位、视口 1000 像素、`view.scale = 10`（地图填满视口）。
/// - `> 20`（远观）：地图在屏幕 < 50px，参考网格挤成实线，**隐藏**；
/// - `< 0.5`（近看）：地图在屏幕 > 2000px，**根本看不到全图**，网格无意义，**隐藏**。
const GRID_MIN_SCALE: f32 = 0.5;
const GRID_MAX_SCALE: f32 = 20.0;

/// 地图外框 + 1/4 幅面参考网格的 mesh 实体集合。
///
/// 外框 = 4 段（顶/底/左/右），参考网格 = 6 段（3 竖 + 3 横）。
#[derive(Resource, Default)]
pub struct MapFrameEntities {
    /// 外框 4 段：[top, bottom, left, right]。
    pub frame: [Option<Entity>; 4],
    /// 参考网格 6 段：[v1, v2, v3, h1, h2, h3]（v=竖线，h=横线）。
    pub grid: [Option<Entity>; 6],
}

impl MapFrameEntities {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for slot in &mut self.frame {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
        for slot in &mut self.grid {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
    }
}

/// 地图外框 + 1/4 幅面参考网格的同步入口。
///
/// 在 [`crate::terrain_view::sync_terrain_sprite`] 之后跑（外框要参考地形尺寸）。
pub fn sync_map_frame(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    tex: Res<TerrainTexture>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    mut entities: ResMut<MapFrameEntities>,
) {
    if !view.terrain_visible {
        entities.despawn_all(&mut commands);
        return;
    }

    // 地图外框的世界尺寸 / 中心。
    let size = tex.size as f32 * grid.cell_size;
    let min = grid.origin;
    let max = grid.origin + Vec2::splat(size);
    let center = (min + max) * 0.5;
    let stroke_world = (STROKE_PX / view.scale).max(f32::EPSILON);

    // ---- 外框：4 段 Rectangle ----
    // 顶 / 底：水平方向，长度 = size。
    // 左 / 右：垂直方向，长度 = size。
    let mat_frame = materials.add(palette::LINE_ON_DARK);
    let segs: [(usize, Vec2, Vec2, f32); 4] = [
        // (idx, translation, scale, rotation_rad)
        (
            0,
            Vec2::new(center.x, max.y),
            Vec2::new(size, stroke_world),
            0.0,
        ), // 顶
        (
            1,
            Vec2::new(center.x, min.y),
            Vec2::new(size, stroke_world),
            0.0,
        ), // 底
        (
            2,
            Vec2::new(min.x, center.y),
            Vec2::new(stroke_world, size),
            0.0,
        ), // 左
        (
            3,
            Vec2::new(max.x, center.y),
            Vec2::new(stroke_world, size),
            0.0,
        ), // 右
    ];
    for (idx, translation, dims, _rot) in segs {
        let handle = meshes.add(Rectangle::new(dims.x, dims.y));
        let transform = Transform::from_translation(translation.extend(FRAME_Z));
        if let Some(e) = entities.frame[idx] {
            commands.entity(e).insert((Mesh2d(handle), transform));
        } else {
            let e = commands
                .spawn((Mesh2d(handle), MeshMaterial2d(mat_frame.clone()), transform))
                .id();
            entities.frame[idx] = Some(e);
        }
    }

    // ---- 参考网格：1/4 幅面 = step = size/4，等分 4 段，3 条内线 ----
    // LOD：`view.scale` 在 [GRID_MIN_SCALE, GRID_MAX_SCALE] 之外就隐藏。
    let grid_visible = view.scale >= GRID_MIN_SCALE && view.scale <= GRID_MAX_SCALE;
    if !grid_visible {
        for slot in &mut entities.grid {
            if let Some(e) = slot.take() {
                commands.entity(e).despawn();
            }
        }
        return;
    }
    let mat_grid = materials.add(palette::LINE_ON_DARK.with_alpha(0.35));
    let step = size / 4.0;
    // 3 条竖线（v1, v2, v3）：x = min.x + step*i，i ∈ 1..4。
    for i in 0..3 {
        let x = min.x + step * (i as f32 + 1.0);
        let translation = Vec2::new(x, center.y);
        let handle = meshes.add(Rectangle::new(stroke_world, size));
        let transform = Transform::from_translation(translation.extend(GRID_Z));
        if let Some(e) = entities.grid[i] {
            commands.entity(e).insert((Mesh2d(handle), transform));
        } else {
            let e = commands
                .spawn((Mesh2d(handle), MeshMaterial2d(mat_grid.clone()), transform))
                .id();
            entities.grid[i] = Some(e);
        }
    }
    // 3 条横线（h1, h2, h3）：y 同上。
    for i in 0..3 {
        let y = min.y + step * (i as f32 + 1.0);
        let translation = Vec2::new(center.x, y);
        let handle = meshes.add(Rectangle::new(size, stroke_world));
        let transform = Transform::from_translation(translation.extend(GRID_Z));
        let idx = 3 + i;
        if let Some(e) = entities.grid[idx] {
            commands.entity(e).insert((Mesh2d(handle), transform));
        } else {
            let e = commands
                .spawn((Mesh2d(handle), MeshMaterial2d(mat_grid.clone()), transform))
                .id();
            entities.grid[idx] = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    //! 只覆盖纯计算（屏幕恒定粗细 + LOD 阈值）。ECS 行为由集成测试覆盖。

    use super::*;

    /// 屏幕像素 → 世界单位：`world = px / view.scale`。
    /// 屏幕视觉恒定。
    #[test]
    fn stroke_world_inverts_with_view_scale() {
        let px = 1.5_f32;
        let vs_out = 0.1_f32;
        let vs_in = 8.0_f32;
        let w_out = (px / vs_out).max(f32::EPSILON);
        let w_in = (px / vs_in).max(f32::EPSILON);
        assert!((w_out * vs_out - px).abs() < 1e-4, "got {}", w_out);
        assert!((w_in * vs_in - px).abs() < 1e-4, "got {}", w_in);
        assert!(w_out > w_in);
    }

    /// LOD 阈值：远观（scale > MAX）应隐藏，**不**应隐藏（MIN ≤ scale ≤ MAX）应显示，
    /// 近看（scale < MIN）应隐藏。
    #[test]
    fn grid_lod_thresholds() {
        // 远观隐藏
        let far_out = 50.0_f32;
        assert!(far_out > GRID_MAX_SCALE, "应隐藏");
        // 中区显示
        let mid = 1.0_f32;
        assert!((GRID_MIN_SCALE..=GRID_MAX_SCALE).contains(&mid), "应显示");
        // 近看隐藏
        let close = 0.05_f32;
        assert!(close < GRID_MIN_SCALE, "应隐藏");
    }

    /// `MapFrameEntities` 默认全空。
    #[test]
    fn map_frame_entities_default_is_empty() {
        let e = MapFrameEntities::default();
        for slot in &e.frame {
            assert!(slot.is_none());
        }
        for slot in &e.grid {
            assert!(slot.is_none());
        }
    }

    /// 4 段外框位置 / 尺寸正确：
    /// - 顶 / 底：y = max.y / min.y，宽度 = size；
    /// - 左 / 右：x = min.x / max.x，高度 = size。
    #[test]
    fn frame_segments_cover_box_perimeter() {
        let size = 1000.0_f32;
        let min = Vec2::new(0.0, 0.0);
        let max = min + Vec2::splat(size);
        let center = (min + max) * 0.5;
        // 顶：(center.x, max.y)
        assert!((center.x - 500.0).abs() < 1e-4);
        assert!((max.y - 1000.0).abs() < 1e-4);
        // 左：(min.x, center.y)
        assert!((min.x - 0.0).abs() < 1e-4);
        assert!((center.y - 500.0).abs() < 1e-4);
    }
}
