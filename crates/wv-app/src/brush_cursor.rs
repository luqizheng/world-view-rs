//! 笔刷光标：屏幕恒定粗细的 Mesh2d 实体。
//!
//! ## 背景与动机（优化项 2）
//!
//! 原实现 [`crate::projection::draw_brush_cursor`] 用 `Gizmos` 画：
//! - 外环：`gizmos.circle_2d`（48 段）——`Gizmos` 屏幕 1px 描线，
//!   **世界空间半径 = `brush.radius`**，相机放大后屏幕尺寸放大、外环变粗；
//! - 内环虚线：48 段短弧拼成——**段长 = 2π·inner/48 世界单位固定**，
//!   相机缩放 ±10× 时，**段长屏幕尺寸跟着 ±10×**，远观变实线、近看段消失；
//! - 十字：1 像素 Gizmos，相机放大变粗。
//!
//! 目标：**光标的"线"在屏幕恒定 1–3 像素**（与 `vector_layers` 策略 A 一致），
//! 半径、十字长度等"世界指示"按 `brush.radius` 真实世界尺寸保留——
//! 它们是**地图上画多大一块**的指示，跟着地图一起缩放是对的。
//!
//! ## 实现
//!
//! - **外环** = `Annulus::new(outer - thickness, outer)`，`outer = brush.radius`，
//!   `thickness = 1.5 * view.scale`（屏幕 1.5px 描边；`view.scale` 的单位是
//!   世界单位/像素，恒定屏幕宽度必须**乘** scale）；
//! - **内环虚线** = 24 个 `Rectangle` mesh 拼接在半径 `inner = brush.radius * (1 - softness)`
//!   的圆周上；**每段世界长度 = 3 * view.scale**（屏幕 3px），段间隙 = 段长——
//!   与原 48 段（24 段亮 + 24 段暗）等价，但**段长随 view.scale 缩放**保持屏幕恒定；
//! - **十字** = 2 个 `Rectangle` mesh，长度 12 屏幕 px 视觉，
//!   粗细 1.5 屏幕 px。
//!
//! ## 数据流
//!
//! 每帧 `sync_brush_cursor` 读光标的屏幕坐标（来自 `Window`）→ 算世界坐标（`screen_to_world`）
//! → 复用 / spawn 三个 mesh 实体（外环 / 虚线 / 十字）。不需要 `ProjectionFrame` 中转
//! ——光标是**纯 UI 状态**，无时间语义、不参与 P7。
//!
//! ## 不动原则
//!
//! - **零 `wv-core` 改动**；
//! - **零新依赖**；
//! - **状态即数据**：禁用 / 切到子级时显式 despawn，避免光标"残留"在场景里。

#![cfg(feature = "app")]

use bevy::math::primitives::{Annulus, Rectangle};
use bevy::prelude::*;

use crate::camera::{screen_to_world, PointerOverUi};
use crate::palette;
use crate::state::{BrushState, DrillState, ViewState};

/// 屏幕像素描边（外环 / 十字线宽）。
///
/// 2.0px（2026-10-13 用户反馈「光标发糊」）：1.5px 在 MSAA4 下被采样稀释，
/// 视觉上偏灰偏虚；2px 配合高细分圆环即可得到清晰的实线。
const STROKE_PX: f32 = 2.0;
/// 屏幕像素虚线段长 + 间隙（= 段长 = 间隙，循环）。
const DASH_PX: f32 = 3.0;
/// 虚线段数（24 = 24 段亮，24 段暗，合计 48 段周期——与原 48 段 Gizmos 一致）。
const DASH_COUNT: usize = 24;
/// 屏幕像素十字臂长。
const CROSS_PX: f32 = 12.0;

/// Z 偏移：光标永远在所有矢量层之上、egui 面板之下。
const CURSOR_Z: f32 = 0.9;

/// 笔刷光标的 mesh 实体集合。
///
/// 复用机制：每帧按需 `insert` 新 mesh + Transform。Cursor 实体数恒为
/// 1（外环）+ 24（虚线段）+ 2（十字）= 27。
#[derive(Resource, Default)]
pub struct BrushCursorEntities {
    /// 外环（`Annulus` 实线）。
    pub outer: Option<Entity>,
    /// `DASH_COUNT` 个虚线段。
    pub dashes: Vec<Entity>,
    /// 横臂。
    pub cross_h: Option<Entity>,
    /// 纵臂。
    pub cross_v: Option<Entity>,
}

impl BrushCursorEntities {
    /// 全部 despawn（光标被禁用 / 切到子级 / 指针出窗时调用）。
    fn despawn_all(&mut self, commands: &mut Commands) {
        if let Some(e) = self.outer.take() {
            commands.entity(e).despawn();
        }
        for e in self.dashes.drain(..) {
            commands.entity(e).despawn();
        }
        if let Some(e) = self.cross_h.take() {
            commands.entity(e).despawn();
        }
        if let Some(e) = self.cross_v.take() {
            commands.entity(e).despawn();
        }
    }
}

/// 笔刷光标的同步入口。在 [`crate::camera::track_cursor_world`] 之后跑。
///
/// **只在世界级显示**（v0.2 §B：城市级/建筑级不显示笔刷光标）。
/// **落在 egui 面板上时隐藏**（读 `PointerOverUi`，指针在 UI 上时光标该消失）。
#[allow(clippy::too_many_arguments)]
pub fn sync_brush_cursor(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    drill: Res<DrillState>,
    pointer_over_ui: Res<PointerOverUi>,
    mut entities: ResMut<BrushCursorEntities>,
) {
    // 关闭 / 切到子级 / 指针在 UI 上 → 清空所有实体。
    let visible = brush.enabled && drill.depth() == 0 && !pointer_over_ui.wants_pointer;
    if !visible {
        entities.despawn_all(&mut commands);
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_screen) = window.cursor_position() else {
        entities.despawn_all(&mut commands);
        return;
    };
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    let cursor_world = screen_to_world(
        cursor_screen,
        window.resolution.size(),
        camera_transform.translation,
        view.scale,
    );

    // ---- 屏幕恒定粗细：`world = 像素 × view.scale`（`view.scale` 的单位是
    // 世界单位/像素）。历史上写成 `像素 / view.scale`，量纲反了——近看时光标
    // 缩成看不见，深缩时胀成巨环。----
    let stroke_world = (STROKE_PX * view.scale).max(f32::EPSILON);
    let dash_world = (DASH_PX * view.scale).max(f32::EPSILON);
    let cross_world = (CROSS_PX * view.scale).max(f32::EPSILON);

    let mat_select = materials.add(palette::ACCENT_SELECT);
    let mat_select_dim = materials.add(palette::ACCENT_SELECT.with_alpha(0.6));

    // ---- 外环（Annulus，屏幕 2px 描边）----
    let outer_radius = brush.radius;
    let annulus_inner = (outer_radius - stroke_world).max(f32::EPSILON);
    // 细分 128 段（默认 32 段在大半径下圆环呈多边形 + 采样发糊，用户反馈「不清晰」）。
    let outer_mesh = Annulus::new(annulus_inner, outer_radius)
        .mesh()
        .resolution(128);
    let outer_handle = meshes.add(outer_mesh);
    let outer_transform = Transform::from_translation(cursor_world.extend(CURSOR_Z));
    if let Some(e) = entities.outer {
        commands
            .entity(e)
            .insert((Mesh2d(outer_handle), outer_transform));
    } else {
        let e = commands
            .spawn((
                Mesh2d(outer_handle),
                MeshMaterial2d(mat_select.clone()),
                outer_transform,
            ))
            .id();
        entities.outer = Some(e);
    }

    // ---- 内环虚线（24 段 Rectangle）----
    let inner_radius = (brush.radius * (1.0 - brush.softness).max(0.15)).max(f32::EPSILON);
    // 段数 = 实体数：缺则补，多则删。
    while entities.dashes.len() > DASH_COUNT {
        if let Some(e) = entities.dashes.pop() {
            commands.entity(e).despawn();
        }
    }
    while entities.dashes.len() < DASH_COUNT {
        let handle = meshes.add(Rectangle::new(dash_world, stroke_world));
        let e = commands
            .spawn((
                Mesh2d(handle),
                MeshMaterial2d(mat_select_dim.clone()),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        entities.dashes.push(e);
    }
    // 按角度 0..TAU 均匀分布。
    for (i, &entity) in entities.dashes.iter().enumerate() {
        let theta = (i as f32 / DASH_COUNT as f32) * std::f32::consts::TAU;
        // 段中心在圆周上：方向 = (cos θ, sin θ)。
        let dir = Vec2::new(theta.cos(), theta.sin());
        let center = cursor_world + dir * inner_radius;
        // Rectangle 默认沿 X 轴长；旋转 = θ 让 X 沿 tangent 方向。
        // 切线方向 = (-sin θ, cos θ)，角度 = atan2(-sin θ, cos θ) = θ - π/2。
        // 但 Quat::from_rotation_z(θ) 会让 X 旋到 (cos θ, sin θ) 方向——不是切线。
        // 正确：旋转 = θ + π/2，让 X 指向 (-sin θ, cos θ)。
        let rotation = Quat::from_rotation_z(theta + std::f32::consts::FRAC_PI_2);
        let translation = center.extend(CURSOR_Z + 0.01);
        commands
            .entity(entity)
            .insert(Transform::from_translation(translation).with_rotation(rotation));
    }

    // ---- 十字（2 个 Rectangle）----
    // 横臂：水平方向。
    let h_handle = meshes.add(Rectangle::new(cross_world, stroke_world));
    let h_transform = Transform::from_translation(cursor_world.extend(CURSOR_Z + 0.02));
    if let Some(e) = entities.cross_h {
        commands.entity(e).insert((Mesh2d(h_handle), h_transform));
    } else {
        let e = commands
            .spawn((
                Mesh2d(h_handle),
                MeshMaterial2d(mat_select.clone()),
                h_transform,
            ))
            .id();
        entities.cross_h = Some(e);
    }
    // 纵臂：垂直方向。
    let v_handle = meshes.add(Rectangle::new(stroke_world, cross_world));
    let v_rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let v_transform =
        Transform::from_translation(cursor_world.extend(CURSOR_Z + 0.02)).with_rotation(v_rotation);
    if let Some(e) = entities.cross_v {
        commands.entity(e).insert((Mesh2d(v_handle), v_transform));
    } else {
        let e = commands
            .spawn((Mesh2d(v_handle), MeshMaterial2d(mat_select), v_transform))
            .id();
        entities.cross_v = Some(e);
    }
}

#[cfg(test)]
mod tests {
    //! 只覆盖纯计算（屏幕恒定粗细换算）。光标的 ECS 行为由集成测试覆盖。

    use super::*;

    /// 屏幕像素 → 世界单位：`world = px * view_scale`（`view_scale` 的单位是
    /// 世界单位/像素）。屏幕视觉始终 = `world / view_scale == px`。
    #[test]
    fn screen_px_to_world_keeps_screen_px_constant() {
        let vs_out = 0.1_f32;
        let vs_in = 8.0_f32;
        let px = 3.0_f32;
        let w_out = (px * vs_out).max(f32::EPSILON);
        let w_in = (px * vs_in).max(f32::EPSILON);
        // 屏幕等价：world / view_scale == px。
        assert!((w_out / vs_out - px).abs() < 1e-4, "got {}", w_out);
        assert!((w_in / vs_in - px).abs() < 1e-4, "got {}", w_in);
        // 近看（scale 大）的世界长度 ≫ 远观——屏幕宽度恒定为 px。
        assert!(w_in > w_out, "近看世界长度应更大");
    }

    /// `BrushCursorEntities` 默认所有字段空。
    #[test]
    fn brush_cursor_entities_default_is_empty() {
        let e = BrushCursorEntities::default();
        assert!(e.outer.is_none());
        assert!(e.dashes.is_empty());
        assert!(e.cross_h.is_none());
        assert!(e.cross_v.is_none());
    }

    /// 角度 → 切线方向旋转：`θ + π/2` 让 Rectangle 默认 X 轴指向切线 (-sin θ, cos θ)。
    #[test]
    fn dash_rotation_is_tangent_direction() {
        use std::f32::consts::FRAC_PI_2;
        // 角度 0：dir = (1, 0)，切线 = (0, 1)。旋转 π/2 后 X → (0, 1) ✓。
        let theta = 0.0_f32;
        let rot = Quat::from_rotation_z(theta + FRAC_PI_2);
        let x_after = rot * Vec3::X;
        assert!((x_after.x - 0.0).abs() < 1e-4, "x={}", x_after.x);
        assert!((x_after.y - 1.0).abs() < 1e-4, "y={}", x_after.y);
        // 角度 π/2：dir = (0, 1)，切线 = (-1, 0)。旋转 π 后 X → (-1, 0) ✓。
        let theta = std::f32::consts::FRAC_PI_2;
        let rot = Quat::from_rotation_z(theta + FRAC_PI_2);
        let x_after = rot * Vec3::X;
        assert!((x_after.x - (-1.0)).abs() < 1e-4, "x={}", x_after.x);
        assert!((x_after.y - 0.0).abs() < 1e-4, "y={}", x_after.y);
    }
}
