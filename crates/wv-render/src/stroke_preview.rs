//! 世界级笔刷**拖拽期**预览（UX-004 / round-2 审计 P1）。
//!
//! `ActiveStroke` 在拖拽中按 ≥半格采样世界坐标点列，但历史上无任何渲染
//! 消费者——作者松手才知道形状（CellMap 路径有红橙实时预览，世界级没有，
//! 两条笔刷路径心智分裂）。本模块把点列渲染成一串**半透明实心圆**
//! （半径 = 笔刷半径，重叠圆近似扫掠足印），配色语言与 [`crate::brush_cursor`]
//! 一致：涂抹金 / 橡皮危险色。
//!
//! ## 资产纪律
//!
//! 每帧 `materials.add` / `meshes.add` 是本项目已定性的资产泄漏反模式
//! （Topographic/EditCell 轮实测卡顿）——mesh 只在半径变化时重建、材质
//! 只在工具切换时重建，句柄缓存进 [`StrokePreviewState`]；实体池按点数
//! 增删，位置每帧 `insert Transform`（纯组件写，零资产）。
//!
//! ## Z
//!
//! 0.85：在地形/矢量层之上、笔刷光标（0.9）之下——预览是「将要发生」，
//! 光标是「正在指示」。

#![cfg(feature = "app")]

use bevy::math::primitives::Circle;
use bevy::prelude::*;

use crate::camera::ActiveStroke;
use crate::state::{BrushState, BrushTool};
use crate::theme::colors;

/// 预览点上限：超长笔触截断（512 圆 ≈ 256 wu 行程，足够视觉引导；
/// 采样密度半格，极端拖拽不会无限涨实体）。
const MAX_PREVIEW: usize = 512;
const PREVIEW_Z: f32 = 0.85;
const PREVIEW_ALPHA: f32 = 0.30;

/// 预览实体池 + 缓存的句柄。
#[derive(Resource, Default)]
pub struct StrokePreviewState {
    entities: Vec<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<ColorMaterial>>,
    last_radius: f32,
    last_tool: Option<BrushTool>,
}

impl StrokePreviewState {
    fn despawn_all(&mut self, commands: &mut Commands) {
        for e in self.entities.drain(..) {
            commands.entity(e).despawn();
        }
    }
}

/// 拖拽预览同步。在 [`crate::camera::collect_brush_stroke`] 之后跑
/// （同帧点列已就绪）；松手帧 `points` 被 drain，预览自动清空。
#[allow(clippy::too_many_arguments)]
pub fn sync_stroke_preview(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    stroke: Res<ActiveStroke>,
    brush: Res<BrushState>,
    mut state: ResMut<StrokePreviewState>,
) {
    let desired = stroke.points.len().min(MAX_PREVIEW);
    if desired == 0 {
        // 只清实体，句柄缓存保留（半径/工具不变时下次拖拽零重建）。
        state.despawn_all(&mut commands);
        return;
    }

    // 半径变了重建 mesh；工具变了重建材质（金↔危险色）。
    if state.last_radius != brush.radius {
        state.mesh = Some(meshes.add(Circle::new(brush.radius).mesh().resolution(48)));
        state.last_radius = brush.radius;
    }
    if state.last_tool != Some(brush.tool) {
        let base = if brush.tool == BrushTool::Eraser {
            colors::ACCENT_CURSOR
        } else {
            colors::ACCENT_SELECT
        };
        state.material = Some(materials.add(base.with_alpha(PREVIEW_ALPHA)));
        state.last_tool = Some(brush.tool);
    }
    let Some(mesh) = state.mesh.clone() else {
        return;
    };
    let Some(material) = state.material.clone() else {
        return;
    };

    // 实体池增删（只动差量），位置每帧 insert。
    while state.entities.len() > desired {
        if let Some(e) = state.entities.pop() {
            commands.entity(e).despawn();
        }
    }
    while state.entities.len() < desired {
        let e = commands
            .spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        state.entities.push(e);
    }
    for (entity, point) in state.entities.iter().zip(stroke.points.iter()) {
        commands
            .entity(*entity)
            .insert(Transform::from_translation(point.extend(PREVIEW_Z)));
    }
}

/// 注册（编辑器权威轨 + 游戏消费轨各一份，与 `brush_cursor` 同款双轨）。
pub fn register(app: &mut App) {
    app.init_resource::<StrokePreviewState>()
        .add_systems(Update, sync_stroke_preview);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 预览实体数封顶：超长笔触不得无限涨实体。
    #[test]
    fn preview_cap_is_bounded() {
        assert!(MAX_PREVIEW <= 1024, "上限语义：视觉引导足够且实体有界");
        let mut state = StrokePreviewState::default();
        assert_eq!(state.entities.len(), 0, "初始无实体");
    }
}
