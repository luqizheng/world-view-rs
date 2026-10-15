//! 相机、缩放、平移（S4）与笔刷拾取（S5）。
//!
//! ## 职责
//! - 处理 Bevy `Camera2d` 的缩放、平移；为 geo 坐标 ↔ 屏幕坐标提供 `ProjectionFrame`。
//! - 把当前地图的 `MapNode.orientation` 写进相机旋转（`apply_map_orientation`，
//!   拉动式每帧重读数据层；`wv-core` 字段文档：渲染层只读这个值，**不另设朝向字段**）。
//! - 收集 `ActiveStroke`：按住鼠标拖拽时把世界坐标点列进栈，
//!   左键释放后由 [`crate::editing`] 把这一段落笔画成 `EditCommand`。
//!
//! ## 边界（不做什么）
//! - **不直接改世界**——笔刷拖拽只收集笔触，松手后才落命令（AGENTS §3.3）。
//! - **不吃落在 egui 面板上的输入**——查 `EguiContexts::ctx_mut().egui_wants_pointer_input()`
//!   让面板优先（基线 §2.13）。
//! - **零惯性 / 无缓动**（ANIMATION_SPEC AD-3 / §4.3）——编辑要指哪打哪。
//!
//! ## 主要导出
//! - `apply_view_to_camera` — 把 `ViewState` 写进 `Transform` / `Projection`（平移 + 缩放）。
//! - `apply_map_orientation` — 把 `MapNode.orientation` 写进相机旋转。
//! - `screen_to_world` — 屏幕坐标 → 世界坐标（变换链唯一实现，含旋转）。
//! - `collect_stroke_points` — 笔刷拾取 / 笔触收集。
//! - `ActiveStroke` 组件、`CameraZoom` 资源。

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use crate::state::{BrushState, DrillState, GridConfig, MeasureState, ViewState, WorldProjection};

// 缩放区间不再用全局常量：它按**当前级**（`drill` 所在那张图的 `extent`）实时算，
// 见 `crate::zoom_level` 与 ADR-0009 §2 决策 ⑤。历史上的
// `SCALE_MIN = 0.1` / `SCALE_MAX = 8.0` 已删除——它们是单级地图时代的产物，
// 在分级缩放里会把深处挡在门外（世界级的外界 8.0 与历史值逐值相同，
// 由 `LEVEL_OVERVIEW` 标定保证，见 `zoom_level.rs`）。

/// 正在拖拽的笔触（世界坐标点列）。松手时交给 `editing` 生成命令。
#[derive(Resource, Default)]
pub struct ActiveStroke {
    pub points: Vec<Vec2>,
}

/// 写相机投影的缩放。非正交投影静默忽略（编辑器不会出现，且此处不该 panic）。
pub(crate) fn set_ortho_scale(projection: &mut Projection, scale: f32) {
    if let Projection::Orthographic(ortho) = projection {
        ortho.scale = scale;
    }
}

/// 把 `ViewState` 的外部写点落到相机（主真源）。
///
/// 小地图点击 / FitMap / Zoom 命令 / 校验锚点跳转改的都是 `ViewState`，
/// 但相机的 `Transform` / 投影 `scale` 才是渲染真源——
/// 只写 ViewState 相机纹丝不动，且 chunk 视口拣选按假视口抖动
/// （despawn/respawn 重建画面，表现为「render 很慢」）。凡是运行期
/// 想改取景的入口，写完 ViewState 后必须经本函数同步相机。
///
/// 只写平移与缩放；**旋转**归 [`apply_map_orientation`] 管（数据来自
/// `MapNode.orientation` 而非 `ViewState`——`ViewState` 禁设朝向字段）。
pub(crate) fn apply_view_to_camera(
    center: Vec2,
    scale: f32,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    for (mut tf, mut projection) in cameras.iter_mut() {
        tf.translation.x = center.x;
        tf.translation.y = center.y;
        set_ortho_scale(&mut projection, scale);
    }
}

/// 地图朝向（度，`MapNode.orientation`，正值 = 顺时针）→ 相机旋转四元数。
///
/// 语义推导：Bevy 正角绕 Z 是**屏幕逆时针**；相机逆时针转 θ，画面内容即
/// **顺时针**转 θ——恰是字段文档「0.0 = 正北朝上，正值 = 顺时针偏转
/// （90.0 = 北方朝右）」的定义。钻入子图时朝向立即切换、不做缓动：
/// 朝向是作者对每张图的声明而非动画参数，过渡缓动留待有真实需求再加。
pub fn camera_rotation_for_orientation(orientation_deg: f32) -> Quat {
    Quat::from_rotation_z(orientation_deg.to_radians())
}

/// 当前地图的朝向（度）。drill 未就绪（无当前图）时按 0 = 正北朝上。
pub fn current_orientation(world: &wv_core::World, drill: &DrillState) -> f32 {
    drill
        .current_node(world)
        .map(|n| n.orientation)
        .unwrap_or(0.0)
}

/// 把当前地图的 `MapNode.orientation` 写进相机旋转（拉动式：每帧重读数据层）。
///
/// 只动 `Transform.rotation`；平移/缩放归 zoom / pan / `apply_view_to_camera`
/// 管，互不覆写。排在输入系统**之前**：同帧的拾取换算（`screen_to_world` 等）
/// 直接读 `Transform.rotation`，与渲染看到的是同一个值，帧内自洽。
pub fn apply_map_orientation(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
) {
    let rot = camera_rotation_for_orientation(current_orientation(&projection.world, &drill));
    for mut tf in camera.iter_mut() {
        tf.rotation = rot;
    }
}

/// 屏幕空间的二维偏移经相机旋转映到世界空间。
///
/// 相机带旋转后，「屏幕上的右/上」对应的世界方向随之转动；所有
/// 「屏幕像素偏移 → 世界偏移」的换算（平移、标注偏移）都必须过这里，
/// 与 [`screen_to_world`] 同一条变换链。
pub fn screen_vec_to_world(v: Vec2, camera_rotation: Quat) -> Vec2 {
    camera_rotation.mul_vec3(v.extend(0.0)).truncate()
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
/// 否则首帧 egui 上下文尚未建立。bevy_egui 0.42 起 `ctx_mut()` 返回 `Result`
/// （旧版是直接 panic），故这里按 `Err` 直接返回、保留上一帧的判定——
/// 启动首帧的「指针不在 UI 上」是安全默认。
pub fn poll_egui_wants_input(mut contexts: EguiContexts, mut flag: ResMut<PointerOverUi>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    flag.wants_pointer = ctx.egui_wants_pointer_input();
    flag.wants_keyboard = ctx.egui_wants_keyboard_input();
}

/// 滚轮缩放：锚点 = 鼠标指针（零惯性）。
///
/// 缩放区间按当前级实时算（`crate::zoom_level`），故需要 `projection`（读 `extent`）
/// 与 `drill`（读「现在在哪一级」）。撞到级的边界后**继续同向滚动**表示「换级」，
/// 本函数只把意图写进 [`crate::render::zoom_level::ZoomCrossRequest`]，真正的换级由
/// [`crate::render::drill::apply_zoom_cross`] 执行（地图路径的写入口只有 `DrillState`）。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `drill.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn camera_zoom(
    mut scroll: MessageReader<MouseWheel>,
    windows: Query<&Window>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    mut view: ResMut<ViewState>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    mut cross: ResMut<crate::render::zoom_level::ZoomCrossRequest>,
    ui: Res<PointerOverUi>,
) {
    if ui.wants_pointer {
        // 指针在面板上时不吃滚轮（面板自己可能滚动）。
        scroll.clear();
        return;
    }
    let Ok(window) = windows.single() else {
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

    let Ok((mut transform, mut ortho)) = camera.single_mut() else {
        return;
    };
    // 旋转（地图朝向）参与锚点换算：屏幕像素偏移要先转过相机旋转才是世界偏移。
    let cam_rot = transform.rotation;

    let old_scale = view.scale;
    let factor = (1.0 + scroll_y * 0.12).clamp(0.5, 2.0);
    let size = window.resolution.size();
    let zoom = crate::render::zoom_level::level_zoom(
        &projection.world,
        drill.current(),
        size,
        Vec2::splat(crate::render::zoom_level::FALLBACK_MAP_SIZE),
    );
    let new_scale = zoom.clamp(old_scale * factor);

    // 指针处的世界坐标在缩放前后保持不变（锚点 = 指针）。
    let cursor_ndc = Vec2::new(
        (cursor.x / size.x) * 2.0 - 1.0,
        -((cursor.y / size.y) * 2.0 - 1.0),
    );
    let cursor_world_before = transform.translation.truncate()
        + screen_vec_to_world(cursor_ndc * (size * 0.5) * old_scale, cam_rot);

    // 撞界后继续同向滚动 = 请求换级。**必须在「值没变就返回」之前判**：
    // 撞界时 `new_scale == old_scale`，恰恰是下面那个提前返回会吞掉的情形。
    if factor > 1.0 && zoom.at_outer_limit(old_scale) {
        // 继续往外：回到父级（世界级再往外没有父级，消费方会自行忽略）。
        cross.pending = Some(crate::render::zoom_level::ZoomCross::Out);
    } else if factor < 1.0 && zoom.at_inner_limit(old_scale) {
        // 继续往里：光标所在的那个子图。用与双击同一套命中测试——不然
        // 「双击能进的图，滚轮进不去」会让人以为坏了。
        if let Some(child) = crate::render::drill::marker_hit(
            &projection.world,
            &drill,
            &grid,
            cursor_world_before,
            old_scale,
        ) {
            cross.pending = Some(crate::render::zoom_level::ZoomCross::Into(child));
        }
    }

    if (new_scale - old_scale).abs() < f32::EPSILON {
        return;
    }

    // `Camera2d` 的投影是 `Projection::Orthographic(..)`（0.19 起统一为枚举）。
    set_ortho_scale(&mut ortho, new_scale);
    view.scale = new_scale;

    let cursor_world_after = transform.translation.truncate()
        + screen_vec_to_world(cursor_ndc * (size * 0.5) * new_scale, cam_rot);
    let delta = cursor_world_before - cursor_world_after;
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
    view.camera_center = transform.translation.truncate();
}

/// 平移：右键 / 中键拖拽（零惯性，当帧跟随）。
/// 左键拖拽平移地图（右键/中键始终可平移；画笔或街道模式开启时左键让渡）。
#[allow(clippy::too_many_arguments)]
pub fn camera_pan(
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<bevy::input::mouse::MouseMotion>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
    mut view: ResMut<ViewState>,
    brush: Res<BrushState>,
    streets: Res<crate::render::streets::StreetEditorState>,
    measure: Res<MeasureState>,
    ui: Res<PointerOverUi>,
    windows: Query<&Window>,
) {
    if ui.wants_pointer {
        return;
    }
    // 启用笔刷 / 街道 / 测量时，左键让渡给对应工具（与笔刷同一条手势让渡规则）。
    let any_tool_active = brush.enabled || streets.enabled || measure.mode.is_active();
    if !pan_button_pressed(&buttons, any_tool_active) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    // 屏幕像素 → 世界单位：乘以 scale，y 轴反向，再转过相机旋转（地图朝向）。
    let world_delta = screen_vec_to_world(
        Vec2::new(-delta.x, delta.y) * view.scale,
        transform.rotation,
    );
    transform.translation.x += world_delta.x;
    transform.translation.y += world_delta.y;
    view.camera_center = transform.translation.truncate();
    let _ = window;
}

/// 当前按下的按钮是否应该平移地图。
///
/// 平移按键口径（2026-10-14 用户裁定：**右键 / 中键**承担平移，左键让给
/// 测量工具——保证「左键点落点 = 起笔/收笔/橡皮」不与地图拖动冲突）。
///
/// - **右键 / 中键**：始终保留给平移，与工具状态无关。
/// - **左键**：**任何情况下都不平移**——空操作，让工具（笔刷 / 街道 / 测量）自取。
fn pan_button_pressed(buttons: &ButtonInput<MouseButton>, _any_tool_active: bool) -> bool {
    buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle)
}

/// 屏幕坐标 → 世界坐标（基线 §2.15 变换链，唯一实现）。
///
/// 含相机旋转（地图朝向）：世界点 = 相机中心 + 相机旋转 × (NDC × 半窗 × scale)。
/// 旋转来自相机 `Transform.rotation` 本身（渲染真源），调用方传整个 `Transform`
/// 而不是只传平移——否则拾取与画面会各转各的。
pub fn screen_to_world(cursor: Vec2, window_size: Vec2, camera: &Transform, scale: f32) -> Vec2 {
    let ndc = Vec2::new(
        (cursor.x / window_size.x) * 2.0 - 1.0,
        -((cursor.y / window_size.y) * 2.0 - 1.0),
    );
    camera.translation.truncate()
        + screen_vec_to_world(ndc * (window_size * 0.5) * scale, camera.rotation)
}

/// 更新光标世界坐标（顶栏坐标显示）。
pub fn track_cursor_world(
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    mut state: ResMut<ViewState>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(transform) = camera.single() else {
        return;
    };
    let scale = state.scale;
    state.window_size = window.resolution.size();
    state.cursor_world = window
        .cursor_position()
        .map(|c| screen_to_world(c, window.resolution.size(), transform, scale));
}

/// 笔刷拾取：左键按下/拖拽时收集笔触点列（**不改世界**）。
///
/// 两条与地图钻取（v0.2 §B）的接口约定：
///
/// 1. **只在世界级落笔**。原型 `index.html:1837` 的判定是
///    `brushOn && !immersive && mapState.level===0`——城市级/建筑级没有地形网格，
///    在那里涂抹既无对象也易误解。
/// 2. **点在钻入标记上不落笔**。原型里标记是独立的 DOM 元素、`dblclick` 自带
///    `stopPropagation`，画布收不到那两下点击；Bevy 没有 DOM 挡刀，只能自己判，
///    否则「双击钻入」会先在地上刷一笔。
///
/// Bevy 系统的参数即「依赖声明」，宽签名是框架惯例而非坏味道（同 `editing.rs`）。
#[allow(clippy::too_many_arguments)]
pub fn collect_brush_stroke(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera: Query<&Transform, With<Camera2d>>,
    view: Res<ViewState>,
    brush: Res<BrushState>,
    ui: Res<PointerOverUi>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    mut stroke: ResMut<ActiveStroke>,
) {
    if ui.wants_pointer || !brush.enabled || drill.depth() > 0 {
        stroke.points.clear();
        return;
    }
    // 无工程锁（2026-10-14 用户需求）：没有打开任何工程时不允许落笔。欢迎页
    // 会盖住画布（UI 侧第一道），这里是数据侧的第二道——防指针穿透边角、
    // 也防测试直跑本系统时绕过 UI。
    if projection.project_dir.is_none() {
        stroke.points.clear();
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
    let world = screen_to_world(cursor, window.resolution.size(), transform, view.scale);

    if buttons.just_pressed(MouseButton::Left) {
        if crate::render::drill::marker_hit(&projection.world, &drill, &grid, world, view.scale)
            .is_some()
        {
            // 这一下是冲着标记去的：不落笔（见上文第 2 条）。
            stroke.points.clear();
            return;
        }
        stroke.points.clear();
        stroke.points.push(world);
    } else if buttons.pressed(MouseButton::Left) {
        // 采样去抖：与上一点距离小于半格则不重复记录。
        let cell = grid.cell_size;
        if stroke
            .points
            .last()
            .is_none_or(|p| p.distance(world) > cell * 0.5)
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
    let window = windows.single().ok()?;
    let cursor = window.cursor_position()?;
    let transform = camera.single().ok()?;
    Some(screen_to_world(
        cursor,
        window.resolution.size(),
        transform,
        view.scale,
    ))
}

/// 键盘平移：WASD / 方向键（零惯性）。
///
/// 方向按**屏幕**语义（W = 画面上移）：相机带旋转时，「画面上方」对应的世界
/// 方向随朝向转，与鼠标拖拽平移同一口径。
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
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let speed = 900.0 * view.scale; // 世界单位/秒
    let delta = screen_vec_to_world(dir * speed * time.delta_secs(), transform.rotation);
    transform.translation.x += delta.x;
    transform.translation.y += delta.y;
    view.camera_center = transform.translation.truncate();
    let _ = &mut transform;
}

/// Ctrl+滚轮 = 只缩放笔刷大小（基线 §1.16）。
pub fn brush_radius_scroll(
    keys: Res<ButtonInput<KeyCode>>,
    mut scroll: MessageReader<MouseWheel>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::mouse::MouseScrollUnit;
    use wv_core::id::Id;
    use wv_core::map::{IconShape, IconSpec, MapLevel, MapNode};

    /// 2026-10-14 用户裁定：右键 / 中键承担平移，左键让给工具。
    /// - 工具全部关闭时：左键**不**再平移（空操作，留给未来测量 / 笔刷外的工具）。
    /// - 工具关闭时：右键 / 中键始终平移。
    /// - 工具开启时：左键让给工具，平移继续走右键 / 中键。
    #[test]
    fn pan_keys_are_right_and_middle_by_default() {
        let mut buttons = ButtonInput::<MouseButton>::default();
        // 无工具：左键 → 不平移（释放给测量等工具）。
        buttons.press(MouseButton::Left);
        assert!(
            !pan_button_pressed(&buttons, false),
            "无工具时左键不应平移（2026-10-14 用户裁定）"
        );
        // 右键 / 中键始终平移。
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Right);
        assert!(pan_button_pressed(&buttons, false));
        buttons.release(MouseButton::Right);
        buttons.press(MouseButton::Middle);
        assert!(pan_button_pressed(&buttons, false));
        // 工具开启时左键让给工具（不平移）；右键仍平移。
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Left);
        assert!(
            !pan_button_pressed(&buttons, true),
            "工具开启时左键让给工具"
        );
        buttons.release(MouseButton::Left);
        buttons.press(MouseButton::Right);
        assert!(pan_button_pressed(&buttons, true));
    }

    /// 世界 4096²（`fit` 5.12 → 区间 `[0.064, 8.0]`）+ 一座 1024² 的城市，
    /// 城市入口图标摆在世界图正中 (2048,2048)。
    fn two_level_world() -> wv_core::World {
        let icon = IconSpec {
            id: Id::new_unchecked("icon_city_a"),
            anchor: Some([0.5, 0.5]),
            shape: IconShape::Diamond,
            size: None,
            color: None,
            drill_into: Some(Id::new_unchecked("city_a")),
        };
        let node = |id: &str,
                    level: MapLevel,
                    parent: Option<&str>,
                    children: &[&str],
                    extent: f32,
                    icons: Vec<IconSpec>| MapNode {
            id: Id::new_unchecked(id),
            name: id.to_owned(),
            level,
            parent: parent.map(Id::new_unchecked),
            children: children.iter().map(|c| Id::new_unchecked(*c)).collect(),
            layers: Vec::new(),
            icons,
            extent: Some(wv_core::map::Extent::sized(extent, extent)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
        };
        wv_core::World {
            maps: vec![
                node(
                    "map_world",
                    MapLevel::World,
                    None,
                    &["city_a"],
                    4096.0,
                    vec![icon],
                ),
                node(
                    "city_a",
                    MapLevel::City,
                    Some("map_world"),
                    &[],
                    1024.0,
                    Vec::new(),
                ),
            ],
            ..Default::default()
        }
    }

    /// 一个只装了 `camera_zoom` 的 App：相机在世界图正中，指针停在窗口正中，
    /// 于是「指针处的世界坐标」恰好是城市入口图标 (2048,2048)。
    fn zoom_app(scale: f32) -> App {
        let world = two_level_world();
        let mut drill = DrillState::default();
        drill.ensure_root(&world);

        let mut app = App::new();
        let view = ViewState {
            scale,
            ..Default::default()
        };
        app.init_resource::<PointerOverUi>()
            .init_resource::<GridConfig>()
            .init_resource::<crate::render::zoom_level::ZoomCrossRequest>()
            .add_message::<MouseWheel>()
            .insert_resource(view)
            .insert_resource(drill)
            .insert_resource(WorldProjection {
                world,
                ..Default::default()
            })
            .add_systems(Update, camera_zoom);

        app.world_mut().spawn((
            Camera2d,
            Transform::from_xyz(2048.0, 2048.0, 0.0),
            Projection::Orthographic(OrthographicProjection::default_2d()),
        ));
        let mut window = Window::default();
        // `Window::default()` = 1280×720，正中即缩放锚点。
        window.set_cursor_position(Some(Vec2::new(640.0, 360.0)));
        let window_id = app.world_mut().spawn(window).id();
        app.insert_resource(TestWindow(window_id));
        app
    }

    #[derive(Resource)]
    struct TestWindow(Entity);

    /// 世界级区间。**必须**从 `zoom_level` 现算而不是写死常量：窗口尺寸决定了 `fit`
    /// （`Window::default()` 是 1280×720，`fit = 4096/720 ≈ 5.688`，不是 800 高时的 5.12）。
    fn world_band() -> crate::render::zoom_level::LevelZoom {
        crate::render::zoom_level::level_zoom(
            &two_level_world(),
            Some(&Id::new_unchecked("map_world")),
            Vec2::new(1280.0, 720.0),
            Vec2::splat(crate::render::zoom_level::FALLBACK_MAP_SIZE),
        )
    }

    fn scroll(app: &mut App, y: f32) {
        let window = app.world().resource::<TestWindow>().0;
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y,
            window,
            // 0.19 起 `MouseWheel` 带触控板阶段字段；鼠标滚轮恒为 `Moved`。
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }

    /// 已在**外界**还继续往外滚 = 请求上溯一级。
    #[test]
    fn camera_zoom_requests_a_step_out_at_the_outer_limit() {
        let mut app = zoom_app(world_band().max);
        scroll(&mut app, 1.0); // y > 0 → factor > 1 → 缩小（往外）
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            Some(crate::render::zoom_level::ZoomCross::Out),
            "踩在外界上继续往外滚，必须发出「上溯一级」的请求"
        );
    }

    /// 已在**内界**且指针压在城市入口上时继续往里滚 = 请求进入该城。
    #[test]
    fn camera_zoom_requests_a_step_into_the_child_under_the_cursor() {
        let mut app = zoom_app(world_band().min);
        scroll(&mut app, -1.0); // y < 0 → factor < 1 → 放大（往里）
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            Some(crate::render::zoom_level::ZoomCross::Into(
                Id::new_unchecked("city_a")
            )),
            "指针在城市图标上继续往里滚，必须请求进入该城"
        );
    }

    /// 区间**中间**的滚动只是缩放，不得换级——否则正常缩放会莫名换图。
    #[test]
    fn camera_zoom_does_not_cross_in_the_middle_of_the_band() {
        let fit = world_band().fit;
        let mut app = zoom_app(fit);
        scroll(&mut app, -1.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            None
        );
        let view = app.world().resource::<ViewState>();
        assert!(view.scale < fit, "中间地带只缩放，got {}", view.scale);
    }

    /// 指针在面板上时不吃滚轮（面板自己要滚），也不得因此换级。
    #[test]
    fn camera_zoom_ignores_scroll_over_ui() {
        let max = world_band().max;
        let mut app = zoom_app(max);
        app.world_mut()
            .resource_mut::<PointerOverUi>()
            .wants_pointer = true;
        scroll(&mut app, 1.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<crate::render::zoom_level::ZoomCrossRequest>()
                .pending,
            None
        );
        assert_eq!(app.world().resource::<ViewState>().scale, max);
    }

    // ---------------------------------------------------------------------------
    // 地图朝向 → 相机旋转（手算断言）
    // ---------------------------------------------------------------------------

    /// 朝向 90°（「北方朝右」）时，屏幕「右」对应世界「上」：
    /// 相机中心 (100, 200)、scale 2、指针在窗口中心右 100px，
    /// 手算 = 中心 + 旋转90°·(200, 0) = (100, 400)。
    #[test]
    fn screen_to_world_rotates_with_camera_orientation() {
        let cam = Transform {
            translation: Vec3::new(100.0, 200.0, 0.0),
            rotation: camera_rotation_for_orientation(90.0),
            ..Default::default()
        };
        let w = screen_to_world(Vec2::new(740.0, 360.0), Vec2::new(1280.0, 720.0), &cam, 2.0);
        assert!((w.x - 100.0).abs() < 1e-4, "got {:?}", w);
        assert!((w.y - 400.0).abs() < 1e-4, "got {:?}", w);
    }

    /// 朝向语义必须是「正值 = 顺时针」：相机取 +90°（屏幕逆时针）后，
    /// 世界 +Y（默认画面的北/上）在屏幕上指向右。
    #[test]
    fn orientation_positive_is_clockwise_on_screen() {
        let rot = camera_rotation_for_orientation(90.0);
        // 世界 → 相机空间 = 旋转的逆：北出现在相机 +X（屏幕右）。
        let north = rot.inverse().mul_vec3(Vec3::Y).truncate();
        assert!((north.x - 1.0).abs() < 1e-5, "got {:?}", north);
        assert!(north.y.abs() < 1e-5, "got {:?}", north);
    }

    /// 「屏幕上方」的标注偏移在朝向 90° 时应映为世界的「左」——
    /// 这是文字标注不随地图转的两条支柱之一（另一条是实体反向旋转）。
    #[test]
    fn label_offset_stays_screen_up_under_rotation() {
        let rot = camera_rotation_for_orientation(90.0);
        let off = screen_vec_to_world(Vec2::Y * 10.0, rot);
        assert!((off.x + 10.0).abs() < 1e-5, "got {:?}", off);
        assert!(off.y.abs() < 1e-5, "got {:?}", off);
    }

    /// 标注反向旋转 = 相机旋转的逆，叠加后屏幕角为零（文字保持水平）。
    #[test]
    fn label_counter_rotation_cancels_camera_rotation() {
        for deg in [0.0_f32, 30.0, 90.0, 217.5] {
            let cam = camera_rotation_for_orientation(deg);
            let label = cam.inverse();
            let total = (cam * label).mul_vec3(Vec3::X).truncate();
            assert!(
                (total - Vec2::X).length() < 1e-5,
                "deg {}, got {:?}",
                deg,
                total
            );
        }
    }

    /// 当前图的朝向从数据层现读：无图按 0，有图按 `MapNode.orientation`。
    #[test]
    fn current_orientation_reads_the_active_map() {
        let mut world = two_level_world();
        world.maps[0].orientation = 90.0;
        let mut drill = DrillState::default();
        assert_eq!(current_orientation(&world, &drill), 0.0, "无当前图按 0");
        drill.ensure_root(&world);
        assert_eq!(current_orientation(&world, &drill), 90.0);
    }
}
