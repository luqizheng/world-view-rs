//! 相机、缩放、平移（S4）与笔刷拾取（S5）。
//!
//! ## 硬约束
//!
//! - **缩放/平移零惯性、刚性跟随**（`ANIMATION_SPEC` AD-3 / §4.3）：
//!   地图编辑要「指哪打哪」，任何缓动都会让作者对不齐要素。
//! - **缩放锚点 = 鼠标指针位置**（基线 §1.16 / §2.12）。
//! - **输入冲突**：指针落在 egui 面板上时，地图不吃输入（基线 §2.13）。
//!   查 `EguiContexts::ctx_mut().wants_pointer_input()`。
//! - **不直接改世界**：笔刷拖拽只**收集笔触**，松手后由 [`crate::editing`] 生成命令。

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::state::{BrushState, GridConfig, ViewState};

/// 相机缩放范围（基线 §2.12：渲染层 0.1×–8.0×）。
pub const SCALE_MIN: f32 = 0.1;
pub const SCALE_MAX: f32 = 8.0;

/// 正在拖拽的笔触（世界坐标点列）。松手时交给 `editing` 生成命令。
#[derive(Resource, Default)]
pub struct ActiveStroke {
    pub points: Vec<Vec2>,
}

/// egui 是否想要指针输入（读上一帧结果，基线 §2.13）。
#[derive(Resource, Default)]
pub struct PointerOverUi {
    pub wants_pointer: bool,
    pub wants_keyboard: bool,
}

/// 每帧开头更新「指针是否在 UI 上」。放在 `PreUpdate`。
///
/// **必须排在 `EguiPreUpdateSet::InitContexts` 之后**（见 gui.rs 的 `.after()`）：
/// 否则首帧 egui 上下文尚未建立，`ctx_mut()` 会 panic 导致启动即崩溃。
pub fn poll_egui_wants_input(mut contexts: EguiContexts, mut flag: ResMut<PointerOverUi>) {
    let ctx = contexts.ctx_mut();
    flag.wants_pointer = ctx.wants_pointer_input();
    flag.wants_keyboard = ctx.wants_keyboard_input();
}

/// 滚轮缩放：锚点 = 鼠标指针（零惯性）。
pub fn camera_zoom(
    mut scroll: EventReader<MouseWheel>,
    windows: Query<&Window>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    mut view: ResMut<ViewState>,
    ui: Res<PointerOverUi>,
) {
    if ui.wants_pointer {
        // 指针在面板上时不吃滚轮（面板自己可能滚动）。
        scroll.clear();
        return;
    }
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let mut scroll_y = 0.0;
    for ev in scroll.read() {
        scroll_y += ev.y;
    }
    if scroll_y == 0.0 {
        return;
    }

    let Ok((mut transform, mut projection)) = camera.get_single_mut() else {
        return;
    };

    let old_scale = view.scale;
    let factor = (1.0 + scroll_y * 0.12).clamp(0.5, 2.0);
    let new_scale = (old_scale * factor).clamp(SCALE_MIN, SCALE_MAX);
    if (new_scale - old_scale).abs() < f32::EPSILON {
        return;
    }

    // 指针处的世界坐标在缩放前后保持不变（锚点 = 指针）。
    let size = window.resolution.size();
    let cursor_ndc = Vec2::new(
        (cursor.x / size.x) * 2.0 - 1.0,
        -((cursor.y / size.y) * 2.0 - 1.0),
    );
    let cursor_world_before =
        transform.translation.truncate() + cursor_ndc * (size * 0.5) * old_scale;

    if let Projection::Orthographic(ortho) = &mut *projection {
        ortho.scale = new_scale;
    }
    view.scale = new_scale;

    let cursor_world_after =
        transform.translation.truncate() + cursor_ndc * (size * 0.5) * new_scale;
    let delta = cursor_world_before - cursor_world_after;
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
    view.camera_center = transform.translation.truncate();
}

/// 平移：右键 / 中键拖拽（零惯性，当帧跟随）。
pub fn camera_pan(
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: EventReader<bevy::input::mouse::MouseMotion>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
    mut view: ResMut<ViewState>,
    ui: Res<PointerOverUi>,
    windows: Query<&Window>,
) {
    if ui.wants_pointer {
        return;
    }
    if !(buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle)) {
        return;
    }
    let Ok(window) = windows.get_single() else {
        return;
    };
    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }
    let Ok(mut transform) = camera.get_single_mut() else {
        return;
    };
    // 屏幕像素 → 世界单位：乘以 scale，且 y 轴反向。
    let world_delta = Vec2::new(-delta.x, delta.y) * view.scale;
    transform.translation.x += world_delta.x;
    transform.translation.y += world_delta.y;
    view.camera_center = transform.translation.truncate();
    let _ = window;
}

/// 屏幕坐标 → 世界坐标（基线 §2.15 变换链，唯一实现）。
pub fn screen_to_world(
    cursor: Vec2,
    window_size: Vec2,
    camera_translation: Vec3,
    scale: f32,
) -> Vec2 {
    let ndc = Vec2::new(
        (cursor.x / window_size.x) * 2.0 - 1.0,
        -((cursor.y / window_size.y) * 2.0 - 1.0),
    );
    camera_translation.truncate() + ndc * (window_size * 0.5) * scale
}

/// 更新光标世界坐标（顶栏坐标显示）。
pub fn track_cursor_world(
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    mut state: ResMut<ViewState>,
) {
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Ok(transform) = camera.get_single() else {
        return;
    };
    let scale = state.scale;
    state.cursor_world = window
        .cursor_position()
        .map(|c| screen_to_world(c, window.resolution.size(), transform.translation, scale));
}

/// 笔刷拾取：左键按下/拖拽时收集笔触点列（**不改世界**）。
pub fn collect_brush_stroke(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    ui: Res<PointerOverUi>,
    mut stroke: ResMut<ActiveStroke>,
) {
    if ui.wants_pointer || !brush.enabled {
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
    let world = screen_to_world(
        cursor,
        window.resolution.size(),
        transform.translation,
        view.scale,
    );

    if buttons.just_pressed(MouseButton::Left) {
        stroke.points.clear();
        stroke.points.push(world);
    } else if buttons.pressed(MouseButton::Left) {
        // 采样去抖：与上一点距离小于半格则不重复记录。
        let cell = crate::state::GridConfig::default().cell_size;
        if stroke
            .points
            .last()
            .map_or(true, |p| p.distance(world) > cell * 0.5)
        {
            stroke.points.push(world);
        }
    }
}

/// 笔刷光标世界坐标（供光标绘制与顶栏半径显示）。
pub fn brush_cursor_world(
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
) -> Option<Vec2> {
    let window = windows.get_single().ok()?;
    let cursor = window.cursor_position()?;
    let transform = camera.get_single().ok()?;
    Some(screen_to_world(
        cursor,
        window.resolution.size(),
        transform.translation,
        view.scale,
    ))
}

/// 键盘平移：WASD / 方向键（零惯性）。
pub fn camera_pan_keys(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
    mut view: ResMut<ViewState>,
    ui: Res<PointerOverUi>,
) {
    if ui.wants_keyboard {
        return;
    }
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir.y -= 1.0;
    }
    if dir == Vec2::ZERO {
        return;
    }
    let Ok(mut transform) = camera.get_single_mut() else {
        return;
    };
    let speed = 900.0 * view.scale; // 世界单位/秒
    let delta = dir.normalize() * speed * time.delta_secs();
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
    view.camera_center = transform.translation.truncate();
    let _ = &mut transform;
}

/// Ctrl+滚轮 = 只缩放笔刷大小（基线 §1.16）。
pub fn brush_radius_scroll(
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: EventReader<MouseWheel>,
    mut brush: ResMut<BrushState>,
    mut grid: ResMut<GridConfig>,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }
    let mut sy = 0.0;
    for ev in scroll.read() {
        sy += ev.y;
    }
    if sy == 0.0 {
        return;
    }
    let factor = (1.0 + sy * 0.15).clamp(0.5, 2.0);
    brush.radius = (brush.radius * factor).clamp(grid.cell_size * 2.0, 600.0);
    // grid 保持可变以允许后续同步其它参数。
    let _ = &mut grid;
}
